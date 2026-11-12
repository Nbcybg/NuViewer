# Builds a release and packages it:
#   dist\NuViewer-<version>-setup.exe      installer + uninstaller (per user, no admin rights, file-type chooser)
#   dist\NuViewer-<version>-win64.zip      portable copy
#   tools\package.ps1                      (output in .\dist)
#   tools\package.ps1 -OutDir D:\out       (somewhere else)
# Honours CARGO_TARGET_DIR if you set it (or pass -TargetDir).
param(
    [string]$OutDir = (Join-Path $PSScriptRoot '..\dist'),
    [string]$TargetDir = $env:CARGO_TARGET_DIR
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $root
try {
    if ($TargetDir) { $env:CARGO_TARGET_DIR = $TargetDir }
    # cargo prints progress on stderr, which Windows PowerShell 5.1 would turn into a terminating error under 'Stop'.
    $ErrorActionPreference = 'Continue'
    cargo build --release -p nuviewer 2>&1 | ForEach-Object { "$_" }
    $buildExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($buildExit -ne 0) { throw "cargo build failed (exit code $buildExit)" }

    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    $exe = Join-Path $target 'release\nuviewer.exe'
    if (-not (Test-Path $exe)) { throw "missing $exe" }

    # The installer carries nuviewer.exe inside itself (setup\build.rs reads $env:NUVIEWER_EXE), so it is built second.
    $env:NUVIEWER_EXE = $exe
    $ErrorActionPreference = 'Continue'
    cargo build --release -p nuviewer-setup 2>&1 | ForEach-Object { "$_" }
    $setupExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($setupExit -ne 0) { throw "cargo build (setup) failed (exit code $setupExit)" }
    $setupExe = Join-Path $target 'release\nuviewer-setup.exe'
    if (-not (Test-Path $setupExe)) { throw "missing $setupExe" }

    $version = [regex]::Match((Get-Content (Join-Path $root 'Cargo.toml') -Raw), '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    if (-not $version) { throw "couldn't read the version from Cargo.toml" }

    New-Item -ItemType Directory -Force $OutDir | Out-Null
    $name = "NuViewer-$version-win64"
    $stage = Join-Path $OutDir $name
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
    New-Item -ItemType Directory $stage | Out-Null

    Copy-Item $exe (Join-Path $stage 'nuviewer.exe')
    Copy-Item (Join-Path $root 'README.md') $stage
    # Helpers so people don't need a terminal to register / unregister the file types.
    Set-Content (Join-Path $stage 'Choose-FileTypes.cmd')     "@echo off`r`n`"%~dp0nuviewer.exe`" --associate`r`n"  -Encoding ASCII
    Set-Content (Join-Path $stage 'Register-FileTypes.cmd')   "@echo off`r`n`"%~dp0nuviewer.exe`" --register`r`n"   -Encoding ASCII
    Set-Content (Join-Path $stage 'Unregister-FileTypes.cmd') "@echo off`r`n`"%~dp0nuviewer.exe`" --unregister`r`n" -Encoding ASCII

    $setupOut = Join-Path $OutDir "NuViewer-$version-setup.exe"
    Copy-Item $setupExe $setupOut -Force
    "{0}  ({1:N0} KB)  sha256 {2}" -f $setupOut, ((Get-Item $setupOut).Length / 1KB), (Get-FileHash $setupOut -Algorithm SHA256).Hash

    $zip = Join-Path $OutDir "$name.zip"
    if (Test-Path $zip) { Remove-Item $zip -Force }
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash
    "{0}  ({1:N0} KB)  sha256 {2}" -f $zip, ((Get-Item $zip).Length / 1KB), $hash
}
finally {
    Pop-Location
}
