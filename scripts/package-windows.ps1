$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootDir = (Resolve-Path (Join-Path $ScriptDir "..")).Path
Set-Location $RootDir

$CargoToml = Get-Content (Join-Path $RootDir "Cargo.toml") -Raw
$VersionMatch = [regex]::Match($CargoToml, '(?m)^version\s*=\s*"([^"]+)"')
$Version = if ($VersionMatch.Success) { $VersionMatch.Groups[1].Value } else { "0.3.0" }

# Normalize to 4 numeric segments for Windows VIProductVersion (e.g. 0.3.0 -> 0.3.0.0)
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
