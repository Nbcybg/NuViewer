# Builds a release and packages it:
#   dist\NuViewer-<version>-setup.exe           installer + uninstaller (per user, no admin rights, file-type chooser; the
#                                              same exe can also unpack a portable copy: `/portable /D=<folder>`)
#   dist\NuViewer-<version>-portable-win64.zip  portable copy: unzip anywhere (portable.txt inside keeps settings in .\Data)
#   dist\SHA256SUMS.txt                    SHA-256 of both, `sha256sum -c` / Get-FileHash compatible
#   tools\package.ps1                      (output in .\dist)
#   tools\package.ps1 -OutDir D:\out       (somewhere else)
# Honours CARGO_TARGET_DIR if you set it (or pass -TargetDir).
# Code signing: if NUVIEWER_SIGN_PFX names a .pfx certificate (password in NUVIEWER_SIGN_PASSWORD, optional timestamp server in
# NUVIEWER_SIGN_TIMESTAMP, default http://timestamp.digicert.com) every program and the installer are signed with signtool.exe.
# Without it nothing is signed and the script says so: the SHA-256 sums are then the proof of what was built.
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
    cargo build --release -p nuviewer -p nugallery -p nueditor -p nuimporter -p nuquick -p nunote 2>&1 | ForEach-Object { "$_" }
    $buildExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($buildExit -ne 0) { throw "cargo build failed (exit code $buildExit)" }

    # Authenticode signing (optional, see the top of this file). Signed before the installer is built, so the copies it carries are signed too.
    function Sign-Files([string[]]$Files) {
        if (-not $env:NUVIEWER_SIGN_PFX) { return }
        $signtool = (Get-Command signtool.exe -ErrorAction SilentlyContinue).Source
        if (-not $signtool) {
            $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match '\\x64\\' } | Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
        }
        if (-not $signtool) { throw "NUVIEWER_SIGN_PFX is set but signtool.exe was not found (install the Windows SDK)" }
        $ts = if ($env:NUVIEWER_SIGN_TIMESTAMP) { $env:NUVIEWER_SIGN_TIMESTAMP } else { 'http://timestamp.digicert.com' }
        foreach ($f in $Files) {
            & $signtool sign /f $env:NUVIEWER_SIGN_PFX /p $env:NUVIEWER_SIGN_PASSWORD /fd SHA256 /tr $ts /td SHA256 $f
            if ($LASTEXITCODE -ne 0) { throw "signing $f failed (exit code $LASTEXITCODE)" }
        }
    }

    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    $exe = Join-Path $target 'release\nuviewer.exe'
    if (-not (Test-Path $exe)) { throw "missing $exe" }
    $galleryExe = Join-Path $target 'release\nugallery.exe'
    if (-not (Test-Path $galleryExe)) { throw "missing $galleryExe" }
    $editorExe = Join-Path $target 'release\nueditor.exe'
    if (-not (Test-Path $editorExe)) { throw "missing $editorExe" }
    $importerExe = Join-Path $target 'release\nuimporter.exe'
    if (-not (Test-Path $importerExe)) { throw "missing $importerExe" }
    $quickExe = Join-Path $target 'release\nuquick.exe'
    if (-not (Test-Path $quickExe)) { throw "missing $quickExe" }
    $noteExe = Join-Path $target 'release\nunote.exe'
    if (-not (Test-Path $noteExe)) { throw "missing $noteExe" }

    Sign-Files @($exe, $galleryExe, $editorExe, $importerExe, $quickExe, $noteExe)
    # The installer carries nuviewer.exe inside itself (setup\build.rs reads $env:NUVIEWER_EXE), so it is built second.
    $env:NUVIEWER_EXE = $exe
    $env:NUGALLERY_EXE = $galleryExe
    $env:NUEDITOR_EXE = $editorExe
    $env:NUIMPORTER_EXE = $importerExe
    $env:NUQUICK_EXE = $quickExe
    $env:NUNOTE_EXE = $noteExe
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
    $name = "NuViewer-$version-portable-win64"
    $stage = Join-Path $OutDir $name
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
    New-Item -ItemType Directory $stage | Out-Null

    Copy-Item $exe (Join-Path $stage 'nuviewer.exe')
    Copy-Item $galleryExe (Join-Path $stage 'nugallery.exe')
    Copy-Item $editorExe (Join-Path $stage 'nueditor.exe')
    # Languages: one text file each (en.txt is the template, th.txt a Thai starter); see docs\localization.md
    New-Item -ItemType Directory (Join-Path $stage 'lang') | Out-Null
    Copy-Item (Join-Path $root 'crates\nv-text\lang\*.txt') (Join-Path $stage 'lang')
    Copy-Item $importerExe (Join-Path $stage 'nuimporter.exe')
    Copy-Item $quickExe (Join-Path $stage 'nuquick.exe')
    Copy-Item $noteExe (Join-Path $stage 'nunote.exe')
    Copy-Item (Join-Path $root 'README.md') $stage
    Copy-Item (Join-Path $root 'LICENSE') $stage
    Copy-Item (Join-Path $root 'CHANGELOG.md') $stage
    Copy-Item (Join-Path $root 'docs\INSTALL.md') $stage
    # The marker that makes this folder portable: settings, thumbnails and presets go to .\Data, nothing else on the PC is touched.
    $marker = "This folder is a portable copy of NuViewer.`r`n`r`nWhile this file is here, the programs keep their settings, thumbnails, presets and the backups of edited pictures in the `"Data`" folder`r`nnext to them and change nothing else on the PC (no registry entries, nothing in %APPDATA%). Copy or move the whole folder anywhere;`r`ndelete it to remove everything. Delete this file to make the programs use the ordinary per-user folders again.`r`n"
    [System.IO.File]::WriteAllText((Join-Path $stage 'portable.txt'), $marker, (New-Object System.Text.UTF8Encoding($false)))
    New-Item -ItemType Directory (Join-Path $stage 'docs') | Out-Null
    Copy-Item (Join-Path $root 'docs\TRUST.md') (Join-Path $stage 'docs')
    Copy-Item (Join-Path $root 'docs\plugins.md') (Join-Path $stage 'docs')
    Copy-Item (Join-Path $root 'docs\plugin-examples') (Join-Path $stage 'docs\plugin-examples') -Recurse
    Copy-Item (Join-Path $root 'docs\third-party') (Join-Path $stage 'docs\third-party') -Recurse
    # Helpers so people don't need a terminal to register / unregister the file types.
    Set-Content (Join-Path $stage 'Choose-FileTypes.cmd')     "@echo off`r`n`"%~dp0nuviewer.exe`" --associate`r`n"  -Encoding ASCII
    Set-Content (Join-Path $stage 'Register-FileTypes.cmd')   "@echo off`r`n`"%~dp0nuviewer.exe`" --register`r`n"   -Encoding ASCII
    Set-Content (Join-Path $stage 'Unregister-FileTypes.cmd') "@echo off`r`n`"%~dp0nuviewer.exe`" --unregister`r`n" -Encoding ASCII

    $setupOut = Join-Path $OutDir "NuViewer-$version-setup.exe"
    Copy-Item $setupExe $setupOut -Force
    Sign-Files @($setupOut)
    if (-not $env:NUVIEWER_SIGN_PFX) { "NOT code-signed (no NUVIEWER_SIGN_PFX): Windows SmartScreen may say 'unknown publisher'. See docs\TRUST.md." }
    "{0}  ({1:N0} KB)  sha256 {2}" -f $setupOut, ((Get-Item $setupOut).Length / 1KB), (Get-FileHash $setupOut -Algorithm SHA256).Hash

    $zip = Join-Path $OutDir "$name.zip"
    if (Test-Path $zip) { Remove-Item $zip -Force }
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
    # What a portable download must hold: the programs, the marker, the language files, the licence.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zf = [System.IO.Compression.ZipFile]::OpenRead($zip)
    try {
        $inside = $zf.Entries | ForEach-Object { $_.FullName -replace '/', '\' }
        foreach ($need in @('nuviewer.exe', 'nugallery.exe', 'nueditor.exe', 'nuimporter.exe', 'nuquick.exe', 'nunote.exe', 'portable.txt', 'LICENSE', 'lang\th.txt', 'lang\ja.txt')) {
            if ($inside -notcontains $need) { throw "the portable zip is missing $need" }
        }
    } finally { $zf.Dispose() }
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash
    "{0}  ({1:N0} KB)  sha256 {2}" -f $zip, ((Get-Item $zip).Length / 1KB), $hash

    # SHA256SUMS.txt in the format `sha256sum -c` reads ("<hash> *<file>"), so anyone can check a download.
    $sums = foreach ($f in @($setupOut, $zip)) { "{0} *{1}" -f (Get-FileHash $f -Algorithm SHA256).Hash.ToLower(), (Split-Path $f -Leaf) }
    [System.IO.File]::WriteAllText((Join-Path $OutDir 'SHA256SUMS.txt'), (($sums -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))
    "SHA256SUMS.txt written to $OutDir"
}
finally {
    Pop-Location
}
