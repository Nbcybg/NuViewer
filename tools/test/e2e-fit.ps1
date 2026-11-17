# Fit to screen really fits (a small picture is enlarged unless Settings says not), and F / A are one toggle between fit and 100%.
# A 200 x 100 picture with a 60 px green square in the middle, windowed; the square's size on screen says what the scale is.
#   tools\test\e2e-fit.ps1 [-Dir <folder with nuviewer.exe>]
param([string]$Dir = $(if ($env:CARGO_TARGET_DIR) { Join-Path $env:CARGO_TARGET_DIR 'debug' } else { 'R:\cargo-target\NuViewer\debug' }))
$script:Exe = Join-Path $Dir 'nuviewer.exe'
. (Join-Path $PSScriptRoot 'lib.ps1')
if (-not (Test-Path $script:Exe)) { Write-Host "no nuviewer.exe in $Dir"; exit 2 }
$base = Join-Path $env:TEMP 'nuv_fit'
if (Test-Path $base) { Remove-Item $base -Recurse -Force }
New-Item -ItemType Directory $base | Out-Null
$env:LOCALAPPDATA = Join-Path $base 'local'; New-Item -ItemType Directory $env:LOCALAPPDATA | Out-Null
$pic = Join-Path $base 'small.png'
Make-Picture $pic 200 100 60 ([System.Drawing.Color]::FromArgb(200, 30, 30))

function Square-Width {
    $b = Grab; $w = $b.Width; $h = $b.Height; $b.Dispose()
    $r = Green-Box ([int]($w / 2)) ([int]($h / 2)) ([int]([math]::Min($w, $h) / 2 - 20))
    return $r[0]
}

# --- the default: fit enlarges
Use-Settings (Join-Path $base 'on.ini') "start_fullscreen=0`r`nfit_enlarge=1`r`n"
$ok = Open-Viewer $pic @('--windowed')
Check 'opens' $ok (Title)
Start-Sleep -Milliseconds 600
$w = Square-Width
Check 'fit enlarges the small picture (the 60 px square is now more than 250 px)' ($w -gt 250) "square=$w"
Key 0x46 600   # F: fit is on, so it switches to 100%
$w100 = Square-Width
Check 'F switches to 100% (the square is 60 px)' (InRange $w100 55 66) "square=$w100"
Key 0x46 600
$wf = Square-Width
Check 'F again is back to fit: one toggle' ($wf -gt 250) "square=$wf"
Key 0x41 600   # A: fit is on, so it goes to 100%
Check 'A toggles too (100%)' (InRange (Square-Width) 55 66) ''
Key 0x41 600
Check 'A again: fit' ((Square-Width) -gt 250) ''
Close-Viewer

# --- switched off: fit never enlarges
Use-Settings (Join-Path $base 'off.ini') "start_fullscreen=0`r`nfit_enlarge=0`r`n"
$ok = Open-Viewer $pic @('--windowed')
Start-Sleep -Milliseconds 600
$w = Square-Width
Check 'with the setting off, fit leaves a small picture at 100%' (InRange $w 55 66) "square=$w"
Close-Viewer

Write-Host ""
Write-Host "passed $($script:pass), failed $($script:fail), skipped $($script:skip)"
exit $(if ($script:fail -gt 0) { 1 } else { 0 })
