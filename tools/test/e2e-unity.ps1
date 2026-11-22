# Unity asset bundles (.unity3d) in the viewer and the gallery: the texture inside is the picture, and the thumbnails come out.
# Uses real bundles from a game's cache (Watcher of Realms) copied into a scratch folder; skipped when they are not there.
# Window messages in, window title and pixels out; never the real mouse or keyboard.
#   tools\test\e2e-unity.ps1 [-Dir <folder with nuviewer.exe and nugallery.exe>]
param([string]$Dir = 'R:\cargo-target\NuViewer\debug')
. (Join-Path $PSScriptRoot 'lib.ps1')
$script:Exe = Join-Path $Dir 'nuviewer.exe'
$gal = Join-Path $Dir 'nugallery.exe'
if (-not (Test-Path $script:Exe) -or -not (Test-Path $gal)) { Write-Host "need nuviewer.exe and nugallery.exe in $Dir"; exit 2 }
if ((Get-Process nuviewer -ErrorAction SilentlyContinue) -or (Get-Process nugallery -ErrorAction SilentlyContinue)) { Write-Host 'NuViewer / NuGallery is already running (maybe yours): close it first'; exit 2 }
$cache = Join-Path $env:USERPROFILE 'AppData\LocalLow\Watcher of Realms\Watcher of Realms\cacheimages'
if (-not (Test-Path $cache)) { Skip 'Unity bundles' 'the game cache folder is not on this PC'; exit 0 }

$work = Join-Path $env:TEMP 'nuv_unity'
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory $work | Out-Null
$pics = Join-Path $work 'pics'; New-Item -ItemType Directory $pics | Out-Null
Get-ChildItem $cache -Filter *.unity3d | Select-Object -First 12 | Copy-Item -Destination $pics
$bg = Get-ChildItem $cache -Filter 'activity_bg_demonsoldier*.unity3d' | Select-Object -First 1
Copy-Item $bg.FullName $pics -Force
$env:NUVIEWER_SETTINGS = Join-Path $work 'settings.ini'
Set-Content $env:NUVIEWER_SETTINGS "start_fullscreen=0`r`nfit_enlarge=0`r`n" -Encoding ASCII
$env:LOCALAPPDATA = Join-Path $work 'local'; New-Item -ItemType Directory $env:LOCALAPPDATA -Force | Out-Null
$env:NUGALLERY_STATE = Join-Path $work 'state.ini'

# Number of distinct-looking pixels (a picture, not a flat fill or a card) in the client area, sampled.
function Variety {
    $b = Grab; $seen = New-Object 'System.Collections.Generic.HashSet[int]'
    for ($y = 60; $y -lt $b.Height - 10; $y += 9) { for ($x = 10; $x -lt $b.Width - 10; $x += 9) { $c = $b.GetPixel($x, $y); [void]$seen.Add((($c.R -shr 4) -shl 8) -bor (($c.G -shr 4) -shl 4) -bor ($c.B -shr 4)) } }
    $b.Dispose(); $seen.Count
}

Write-Host '--- the viewer ---'
$file = Join-Path $pics $bg.Name
if (-not (Open-Viewer $file)) { Write-Host 'viewer did not open'; exit 1 }
$t = Title
Check 'the title has the picture size (1024x436)' ($t -match '1024x436') $t
Check 'the title has no error' ($t -notmatch '\[NV-' -and $t -notmatch 'unsupported') $t
Check 'the screen shows a picture with many colours' ((Variety) -gt 60) "$(Variety)"
Close-Viewer

Write-Host '--- the gallery ---'
$gp = Start-Process $gal -ArgumentList @("`"$pics`"", '--windowed', '--new-window') -PassThru
$script:win = Wait-Until { $gp.Refresh(); if ($gp.MainWindowHandle -ne 0 -and $gp.MainWindowTitle -like '*NuGallery*') { $gp.MainWindowHandle } } 15000
Start-Sleep -Milliseconds 2500
$gp.Refresh()
Check 'the gallery lists the bundles' ($gp.MainWindowTitle -match '1[23] items') $gp.MainWindowTitle
Check 'the thumbnails are pictures' ((Variety) -gt 80) "$(Variety)"
if (-not $gp.HasExited) { $gp.Kill() }

Write-Host ("`n{0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip)
exit $(if ($script:fail -eq 0) { 0 } else { 1 })
