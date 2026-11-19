$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootDir = (Resolve-Path (Join-Path $ScriptDir "..")).Path
Set-Location $RootDir

$MetadataJson = cargo metadata --locked --no-deps --format-version 1
if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed" }
$Version = (($MetadataJson | ConvertFrom-Json).packages | Where-Object name -eq "earth" | Select-Object -First 1).version
if (-not $Version) { throw "Cargo metadata does not contain Earth version" }
$Channel = if ($env:EARTH_RELEASE_CHANNEL) { $env:EARTH_RELEASE_CHANNEL } else { "development" }
if ($Channel -notin @("official", "development")) { throw "Invalid EARTH_RELEASE_CHANNEL" }
if ($Channel -eq "official") {
    if ($env:EARTH_UPDATE_WINDOWS_CERT_SHA256 -notmatch '^[0-9A-Fa-f]{64}$' -or -not $env:EARTH_WINDOWS_SIGN_THUMBPRINT) {
        throw "Official releases require a certificate and pinned publisher SHA-256"
    }
}
function Sign-AndVerify([string]$Path) {
    if ($Channel -ne "official") { return }
    $SignTool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $SignTool) { throw "Windows SDK signtool.exe is required" }
    & $SignTool.FullName sign /sha1 $env:EARTH_WINDOWS_SIGN_THUMBPRINT /s My /fd SHA256 /tr https://timestamp.digicert.com /td SHA256 $Path
    if ($LASTEXITCODE -ne 0) { throw "Code signing failed: $Path" }
    & $SignTool.FullName verify /pa /all $Path
    if ($LASTEXITCODE -ne 0) { throw "Authenticode verification failed: $Path" }
    $Signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($Signature.Status -ne 'Valid' -or -not $Signature.SignerCertificate) { throw "Untrusted signature: $Path" }
    $Hash = [Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($Signature.SignerCertificate.RawData))
    if ($Hash -ne $env:EARTH_UPDATE_WINDOWS_CERT_SHA256) { throw "Publisher certificate does not match pinned identity" }
}

# Normalize to 4 numeric segments for Windows VIProductVersion (e.g. 0.4.0 -> 0.4.0.0)
$CoreVersion = ($Version -split '[-+]')[0]
$Parts = @($CoreVersion -split '\.')
while ($Parts.Count -lt 4) { $Parts += "0" }
$ViVersion = ($Parts[0..3] -join '.')

$Target = "x86_64-pc-windows-msvc"
Write-Host "==> Earth Windows packaging start (version=$Version, target=$Target)"

cargo build --release --target $Target
if ($LASTEXITCODE -ne 0) {
    throw "cargo build failed with exit code $LASTEXITCODE"
}

$BuiltExe = Join-Path $RootDir "target\$Target\release\earth.exe"
if (-not (Test-Path $BuiltExe)) {
    throw "Built binary not found: $BuiltExe"
}

$DistDir = Join-Path $RootDir "dist"
if (Test-Path $DistDir) {
    Remove-Item $DistDir -Recurse -Force
}
New-Item -ItemType Directory -Path $DistDir | Out-Null

$StagedExe = Join-Path $DistDir "Earth.exe"
Copy-Item $BuiltExe $StagedExe -Force
$ResourceVersion = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($StagedExe)
if ($ResourceVersion.ProductName -ne "Earth" -or $ResourceVersion.ProductVersion -ne $Version -or $ResourceVersion.FileVersion -ne $Version) {
    throw "Earth.exe is missing the expected target Windows version resources"
}
Sign-AndVerify $StagedExe
$ArtifactSuffix = if ($Channel -eq "official") { "" } else { "-UNSIGNED" }
if ($Channel -ne "official") { Write-Warning "Development artifacts are unsigned and are not eligible for automatic updates or official releases." }

# Verify CRT is statically linked so Earth.exe runs on clean Windows without VCRUNTIME140.dll
function Get-PeImportedDlls([string]$ExePath) {
    $Bytes = [System.IO.File]::ReadAllBytes($ExePath)
    $PeOff = [BitConverter]::ToInt32($Bytes, 0x3C)
    if ($Bytes[$PeOff] -ne 0x50 -or $Bytes[$PeOff + 1] -ne 0x45 -or $Bytes[$PeOff + 2] -ne 0 -or $Bytes[$PeOff + 3] -ne 0) {
        throw "Invalid PE header in $ExePath"
    }
    $OptOff = $PeOff + 24
    $Magic = [BitConverter]::ToUInt16($Bytes, $OptOff)
    $ImpDirOff = if ($Magic -eq 0x20B) { $OptOff + 120 } else { $OptOff + 104 }
    $ImpRva = [BitConverter]::ToUInt32($Bytes, $ImpDirOff)
    $NumSec = [BitConverter]::ToUInt16($Bytes, $PeOff + 6)
    $OptSize = [BitConverter]::ToUInt16($Bytes, $PeOff + 20)
    $SecBase = $OptOff + $OptSize
    $Sections = @()
    for ($i = 0; $i -lt $NumSec; $i++) {
        $s = $SecBase + ($i * 40)
        $VSize = [BitConverter]::ToUInt32($Bytes, $s + 8)
        $VA = [BitConverter]::ToUInt32($Bytes, $s + 12)
        $RawSize = [BitConverter]::ToUInt32($Bytes, $s + 16)
        $RawPtr = [BitConverter]::ToUInt32($Bytes, $s + 20)
        $Span = [Math]::Max($VSize, $RawSize)
        $Sections += [PSCustomObject]@{ VA = $VA; EndVA = ($VA + $Span); RawPtr = $RawPtr }
    }
    $ResolveRva = {
        param([uint32]$Rva, $SecList)
        foreach ($sec in $SecList) {
            if ($Rva -ge $sec.VA -and $Rva -lt $sec.EndVA) {
                return [int]($sec.RawPtr + ($Rva - $sec.VA))
            }
        }
        throw ("Failed to map PE RVA 0x{0:X}" -f $Rva)
    }
    $Dlls = @()
    if ($ImpRva -ne 0) {
        $Pos = & $ResolveRva $ImpRva $Sections
        while ($true) {
            $NameRva = [BitConverter]::ToUInt32($Bytes, $Pos + 12)
            $FirstThunk = [BitConverter]::ToUInt32($Bytes, $Pos + 16)
            if ($NameRva -eq 0 -and $FirstThunk -eq 0) { break }
            if ($NameRva -ne 0) {
                $NameOff = & $ResolveRva $NameRva $Sections
                $End = $NameOff
                while ($End -lt $Bytes.Length -and $Bytes[$End] -ne 0) { $End++ }
                $Dlls += [System.Text.Encoding]::ASCII.GetString($Bytes, $NameOff, $End - $NameOff)
            }
            $Pos += 20
        }
    }
    return $Dlls
}

$ImportedDlls = Get-PeImportedDlls $StagedExe
Write-Host "==> PE imported DLLs: $($ImportedDlls -join ', ')"
$BadRuntime = $ImportedDlls | Where-Object { $_ -match '^(?i)(vcruntime|msvcp|concrt)' }
if ($BadRuntime) {
    throw "Dynamic VC++ runtime DLL detected ($($BadRuntime -join ', ')); +crt-static failed!"
}

# 1. Portable ZIP package
$ZipPath = Join-Path $DistDir "Earth-$Version-Windows-x64-Portable$ArtifactSuffix.zip"
Write-Host "==> Creating portable archive: $ZipPath"
Compress-Archive -Path $StagedExe -DestinationPath $ZipPath -CompressionLevel Optimal -Force

# 2. NSIS Setup Installer (.exe)
$Makensis = (Get-Command makensis -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1)
if (-not $Makensis) {
    $Candidates = @(
        "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
        "${env:ProgramFiles}\NSIS\makensis.exe"
    )
    foreach ($c in $Candidates) {
        if (Test-Path $c) {
            $Makensis = $c
            break
        }
    }
}

if (-not $Makensis) {
    throw "makensis.exe not found. Please install NSIS (e.g. choco install nsis -y)."
}

$SetupPath = Join-Path $DistDir "Earth-$Version-Windows-x64-Setup$ArtifactSuffix.exe"
$NsiScript = Join-Path $RootDir "packaging\windows\installer.nsi"
$IconPath = Join-Path $RootDir "packaging\windows\earth.ico"
$VersionCheckScript = Join-Path $RootDir "packaging\semver.js"

Write-Host "==> Building NSIS installer: $SetupPath"
& $Makensis `
    "/DVERSION=$Version" `
    "/DVI_VERSION=$ViVersion" `
    "/DEXE_PATH=$StagedExe" `
    "/DICON_PATH=$IconPath" `
    "/DVERSION_CHECK_SCRIPT=$VersionCheckScript" `
    "/DOUT_FILE=$SetupPath" `
    $NsiScript

if ($LASTEXITCODE -ne 0) {
    throw "makensis failed with exit code $LASTEXITCODE"
}

Sign-AndVerify $SetupPath

Write-Host "==> Windows packaging complete! Artifacts:"
Get-ChildItem $DistDir | Format-Table Name, Length, LastWriteTime -AutoSize
