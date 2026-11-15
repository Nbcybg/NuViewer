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
# Window or fullscreen follows from one program to the other (and the window stays in the same place).
$mon = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
function Rect-Of([IntPtr]$h) { $rr = New-Object In+RECT; [void][In]::GetWindowRect($h, [ref]$rr); $rr }
function Is-Full-Rect($rr) { (($rr.R - $rr.L) -eq $mon.Width) -and (($rr.B - $rr.T) -eq $mon.Height) }
function Alt-Enter-On([IntPtr]$h) { [void][In]::PostMessage($h, 0x0104, [IntPtr]0x0D, [IntPtr]0x201C0001); Start-Sleep -Milliseconds 1000 }
$gr = Rect-Of $gw
Check 'the gallery is a window to begin with' (-not (Is-Full-Rect $gr)) "$($gr.R-$gr.L)x$($gr.B-$gr.T)"
$script:win = $gw; Click 640 330; Key 0x20 100
$t3 = Get-Date
$vp4 = Wait-Until { Get-Process nuviewer -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge $t3.AddSeconds(-3) -and $_.MainWindowHandle -ne 0 } | Select-Object -First 1 } 12000
Start-Sleep -Milliseconds 800
$vr = Rect-Of $vp4.MainWindowHandle
Check 'the viewer opens as a window too, where the gallery is' ((-not (Is-Full-Rect $vr)) -and ([math]::Abs($vr.L - $gr.L) -le 2) -and ([math]::Abs($vr.T - $gr.T) -le 2) -and ([math]::Abs(($vr.R - $vr.L) - ($gr.R - $gr.L)) -le 2)) "viewer $($vr.L),$($vr.T) $($vr.R-$vr.L)x$($vr.B-$vr.T); gallery $($gr.L),$($gr.T) $($gr.R-$gr.L)x$($gr.B-$gr.T)"
Alt-Enter-On $vp4.MainWindowHandle
Check 'Alt+Enter makes the viewer fullscreen' (Is-Full-Rect (Rect-Of $vp4.MainWindowHandle))
$script:win = $vp4.MainWindowHandle; Key 0x1B 100
Wait-Until { $vp4.Refresh(); $vp4.HasExited } 5000 | Out-Null
Start-Sleep -Milliseconds 1200
Check 'back in the gallery it is fullscreen as well' (Is-Full-Rect (Rect-Of $gw)) ("gallery " + (Rect-Of $gw).R)
Alt-Enter-On $gw
Check 'Alt+Enter in the gallery: window again' (-not (Is-Full-Rect (Rect-Of $gw)))
$script:win = $gw; Click 640 330; Key 0x20 100
$t4 = Get-Date
$vp5 = Wait-Until { Get-Process nuviewer -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge $t4.AddSeconds(-3) -and $_.MainWindowHandle -ne 0 } | Select-Object -First 1 } 12000
Start-Sleep -Milliseconds 800
Check 'and the next viewer is a window' (-not (Is-Full-Rect (Rect-Of $vp5.MainWindowHandle)))
$script:win = $vp5.MainWindowHandle; Key 0x1B 100
Wait-Until { $vp5.Refresh(); $vp5.HasExited } 5000 | Out-Null
Start-Sleep -Milliseconds 800

# Closing the gallery closes the viewer it opened, and nothing comes back afterwards.
$t2 = Get-Date
$script:win = $gw
Click 640 330; Key 0x20 100
$vp3 = Wait-Until { Get-Process nuviewer -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge $t2 -and $_.MainWindowHandle -ne 0 } | Select-Object -First 1 } 12000
Check 'a viewer is open again' ($null -ne $vp3)
[void][In]::PostMessage($gw, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)   # WM_CLOSE on the gallery window
if ($vp3) { $vgone = Wait-Until { $vp3.Refresh(); $vp3.HasExited } 6000; Check 'closing the gallery closes the viewer' ($vgone -eq $true) }
$ggone = Wait-Until { $gp.Refresh(); $gp.HasExited } 4000
Check 'the gallery is gone' ($ggone -eq $true)
Start-Sleep -Milliseconds 1800
Check 'nothing is started again' (-not (Get-Process nugallery, nuviewer -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge $t2 -and -not $_.HasExited -and $_.Id -ne $vp3.Id }))
# And the other way round: a viewer closed with the gallery already gone ends the program.
Get-Process nugallery, nuviewer -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge $t0 } | Stop-Process -Force
Write-Host "pass=$script:pass fail=$script:fail"
