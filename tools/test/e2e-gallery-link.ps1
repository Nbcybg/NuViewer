# The hand-over between NuGallery and NuViewer: Space opens the selected picture in the viewer, Esc there returns to the
# gallery with the last picture seen selected, G in a standalone viewer shows that picture in the gallery.
# Windowed, with its own scratch settings / state / thumbnail cache; messages in, window titles and files out (never the
# real mouse or keyboard). Needs nuviewer.exe and nugallery.exe in one folder:
#   tools\test\e2e-gallery-link.ps1  [-Dir <that folder>]
param([string]$Dir = $(if ($env:CARGO_TARGET_DIR) { Join-Path $env:CARGO_TARGET_DIR 'release' } else { Join-Path $PSScriptRoot '..\..\target\release' }))
. (Join-Path $PSScriptRoot 'lib.ps1')
$g = Join-Path $Dir 'nugallery.exe'; $v = Join-Path $Dir 'nuviewer.exe'
$root = Join-Path $env:TEMP 'nug_link'
if (Test-Path $root) { Remove-Item $root -Recurse -Force }
New-Item -ItemType Directory $root | Out-Null
$cols = @([System.Drawing.Color]::FromArgb(200,30,30),[System.Drawing.Color]::FromArgb(30,160,60),[System.Drawing.Color]::FromArgb(40,70,200))
for ($i = 1; $i -le 12; $i++) { Make-Picture (Join-Path $root ("pic{0:00}.png" -f $i)) 500 400 80 $cols[$i % 3] }
$st = Join-Path $env:TEMP 'nug_link_state'; New-Item -ItemType Directory $st -Force | Out-Null
$env:NUGALLERY_STATE = Join-Path $st 'state.ini'; Remove-Item $env:NUGALLERY_STATE -ErrorAction SilentlyContinue
$env:NUVIEWER_SETTINGS = Join-Path $st 'settings.ini'
Set-Content $env:NUVIEWER_SETTINGS "start_fullscreen=0`r`n" -Encoding ASCII
$env:LOCALAPPDATA = Join-Path $st 'local'; New-Item -ItemType Directory $env:LOCALAPPDATA -Force | Out-Null
$t0 = Get-Date
$gp = Start-Process $g -ArgumentList @("`"$root`"", '--windowed') -PassThru
$gw = Wait-Until { $gp.Refresh(); if ($gp.MainWindowHandle -ne 0 -and $gp.MainWindowTitle -like '*NuGallery*') { $gp.MainWindowHandle } } 15000
Start-Sleep -Milliseconds 1200
function GT { $gp.Refresh(); $gp.MainWindowTitle }
$script:win = $gw
Click 640 330     # a tile in the 2nd row
Write-Host (GT)
Check 'one picture selected' ((GT) -like '*1 selected: pic*') (GT)
$sel = [regex]::Match((GT), 'selected: (pic\d+\.png)').Groups[1].Value
Key 0x20 100      # Space = view
$vp = Wait-Until { Get-Process nuviewer -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge $t0 -and $_.MainWindowHandle -ne 0 } | Select-Object -First 1 } 12000
Check 'viewer started' ($null -ne $vp)
if ($vp) {
    $vp.Refresh(); Start-Sleep -Milliseconds 800; $vp.Refresh()
    Write-Host ("viewer: " + $vp.MainWindowTitle)
    Check 'viewer shows the selected picture' ($vp.MainWindowTitle -like "*$sel*") $vp.MainWindowTitle
    $script:win = $vp.MainWindowHandle
    Key 0x27 600; Key 0x27 600
    $vp.Refresh(); Write-Host ("viewer after 2x right: " + $vp.MainWindowTitle)
    $seen = [regex]::Match($vp.MainWindowTitle, '(pic\d+\.png)').Groups[1].Value
    Check 'viewer moved on' ($seen -ne $sel -and $seen -ne '') $vp.MainWindowTitle
    Key 0x1B 100
    $gone = Wait-Until { $vp.Refresh(); $vp.HasExited } 5000
    Check 'Esc closes the viewer' ($gone -eq $true)
    Start-Sleep -Milliseconds 900
    Write-Host ("gallery: " + (GT))
    Check 'gallery selected the last picture seen' ((GT) -like "*selected: $seen*") ("want $seen; " + (GT))
}
# G in a standalone viewer starts the gallery on that folder (the gallery is already running: it is told)
$vp2 = Start-Process $v -ArgumentList @("`"$root\pic03.png`"", '--new-window') -PassThru
$w2 = Wait-Until { $vp2.Refresh(); if ($vp2.MainWindowHandle -ne 0 -and $vp2.MainWindowTitle -like '*pic03*') { $vp2.MainWindowHandle } } 10000
Start-Sleep -Milliseconds 800
$script:win = $w2
Key 0x47 100   # G
$gone2 = Wait-Until { $vp2.Refresh(); $vp2.HasExited } 5000
Check 'G leaves the viewer' ($gone2 -eq $true)
Start-Sleep -Milliseconds 900
Write-Host ("gallery: " + (GT))
Check 'G shows that picture in the gallery' ((GT) -like '*selected: pic03.png*') (GT)
Get-Process nugallery, nuviewer -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge $t0 } | Stop-Process -Force
Write-Host "pass=$script:pass fail=$script:fail"
