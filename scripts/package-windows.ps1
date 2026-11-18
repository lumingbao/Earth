$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootDir = (Resolve-Path (Join-Path $ScriptDir "..")).Path
Set-Location $RootDir

$CargoToml = Get-Content (Join-Path $RootDir "Cargo.toml") -Raw
$VersionMatch = [regex]::Match($CargoToml, '(?m)^version\s*=\s*"([^"]+)"')
$Version = if ($VersionMatch.Success) { $VersionMatch.Groups[1].Value } else { "0.4.0" }

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
$ZipPath = Join-Path $DistDir "Earth-$Version-Windows-x64-Portable.zip"
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

$SetupPath = Join-Path $DistDir "Earth-$Version-Windows-x64-Setup.exe"
$NsiScript = Join-Path $RootDir "packaging\windows\installer.nsi"
$IconPath = Join-Path $RootDir "packaging\windows\earth.ico"

Write-Host "==> Building NSIS installer: $SetupPath"
& $Makensis `
    "/DVERSION=$Version" `
    "/DVI_VERSION=$ViVersion" `
    "/DEXE_PATH=$StagedExe" `
    "/DICON_PATH=$IconPath" `
    "/DOUT_FILE=$SetupPath" `
    $NsiScript

if ($LASTEXITCODE -ne 0) {
    throw "makensis failed with exit code $LASTEXITCODE"
}

Write-Host "==> Windows packaging complete! Artifacts:"
Get-ChildItem $DistDir | Format-Table Name, Length, LastWriteTime -AutoSize
