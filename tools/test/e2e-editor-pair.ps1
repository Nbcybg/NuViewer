# A picture with a NuEditor pair file is shown with its edits applied in NuViewer (and not when Settings says no), a pair file for
# another size is ignored, and Ctrl+Z in the viewer takes back a burn-in made in the editor (the shared journal).
# Messages only; scratch files and settings.
#
#   powershell -File tools\test\e2e-editor-pair.ps1 [-Viewer path\to\nuviewer.exe]
param([string]$ViewerExe = 'T:\cargo-target\NuViewer\release\nuviewer.exe')
. (Join-Path $PSScriptRoot 'lib.ps1')
$script:Exe = $ViewerExe
if (-not (Test-Path $ViewerExe)) { Write-Host "no $ViewerExe"; exit 2 }

$scratch = Join-Path ([IO.Path]::GetTempPath()) ("nueditor-pair-" + [Diagnostics.Process]::GetCurrentProcess().Id)
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
$on = Join-Path $scratch 'on.ini'; $off = Join-Path $scratch 'off.ini'
Set-Content -Path $on -Value "" -Encoding ASCII
Set-Content -Path $off -Value "editor_show_pair = off" -Encoding ASCII

function Centre-Pixel { $b = Grab; $c = $b.GetPixel([int]($b.Width / 2), [int]($b.Height / 2)); $b.Dispose(); $c }

try {
    Write-Host "== pair file in the viewer e2e =="
    $a = Join-Path $scratch 'a.png'; Make-Picture $a 600 400 0
    $pair = @'
nuedit version="1" source="a.png" width="600" height="400" size="0" mtime="0" auto="off"
flags invert="1"
'@
    Set-Content -Path "$a.nuedit" -Value $pair -Encoding ASCII

    $env:NUVIEWER_SETTINGS = $on
    Check 'opens with the pair file' (Open-Viewer $a)
    $p = Centre-Pixel
    Check 'the pair file is applied: the red picture is inverted (cyan)' ($p.R -lt 90 -and $p.G -gt 170 -and $p.B -gt 170) "$p"
    Close-Viewer

    $env:NUVIEWER_SETTINGS = $off
    Check 'opens again with the setting off' (Open-Viewer $a)
    $p = Centre-Pixel
    Check 'edits are not shown when Settings says no' ($p.R -gt 150 -and $p.G -lt 90) "$p"
    Close-Viewer

    # A pair file made for a picture of another size is left alone.
    $env:NUVIEWER_SETTINGS = $on
    Set-Content -Path "$a.nuedit" -Value $pair.Replace('width="600"', 'width="999"') -Encoding ASCII
    Check 'opens with a pair file for another size' (Open-Viewer $a)
    $p = Centre-Pixel
    Check 'a pair file for another size is ignored' ($p.R -gt 150 -and $p.G -lt 90) "$p"
    Close-Viewer
    Remove-Item "$a.nuedit"

    # ---- the burn-in journal: the viewer's Ctrl+Z restores what the editor burned in ----
    $env:NUEDITOR_BACKUP_DIR = Join-Path $scratch 'backups'
    $b = Join-Path $scratch 'b.png'; Make-Picture $b 600 400 0
    $before = [IO.File]::ReadAllBytes($b)
    New-Item -ItemType Directory -Force -Path $env:NUEDITOR_BACKUP_DIR | Out-Null
    $backup = Join-Path $env:NUEDITOR_BACKUP_DIR '1_b.png'
    [IO.File]::WriteAllBytes($backup, $before)
    # The picture is "burned in": different pixels on disk now.
    Make-Picture $b 600 400 0 ([System.Drawing.Color]::FromArgb(30, 30, 200))
    Set-Content -Path (Join-Path $env:NUEDITOR_BACKUP_DIR 'burn-in-journal.tsv') -Value ("1`t$b`t$backup") -Encoding ASCII
    Check 'opens the burned-in picture' (Open-Viewer $b)
    $p = Centre-Pixel
    Check 'it is blue now' ($p.B -gt 150 -and $p.R -lt 90) "$p"
    [void][In]::PostMessage($script:win, 0x8001, [IntPtr](0x5A -bor (1 -shl 16)), [IntPtr]::Zero)   # Ctrl+Z
    Start-Sleep -Milliseconds 1200
    $p = Centre-Pixel
    Check 'Ctrl+Z in the viewer brings the original back (the journal)' ($p.R -gt 150 -and $p.B -lt 90) "$p"
    Check 'and the file on disk is the original again' ((Get-FileHash $b).Hash -eq (Get-FileHash -InputStream ([IO.MemoryStream]::new($before))).Hash)
    Close-Viewer
}
catch { Write-Host "  ERROR  $_" -ForegroundColor Red; $script:fail++ }
finally {
    Close-Viewer
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
Write-Host ""
Write-Host "passed $script:pass, failed $script:fail"
if ($script:fail -gt 0) { exit 1 }
