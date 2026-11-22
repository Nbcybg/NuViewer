param([string]$Exe = 'R:\cargo-target\NuViewer\debug\nuviewer.exe', [string]$Shots = '')
# Spine player: a skeleton made here (JSON 4.1 + atlas + page) opens, plays, pauses, shows its slots in the layer panel, saves a snapshot with transparency.
# Window messages only (no real mouse / keyboard). Needs no other NuViewer window.
. (Join-Path $PSScriptRoot 'lib.ps1')
$script:Exe = $Exe
$work = Join-Path $env:TEMP ("nv_spine_" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $work | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $work 'settings.ini'
Set-Content -Path $env:NUVIEWER_SETTINGS -Value "fit_enlarge = 0`nsidebar = 0`nconfirm_delete = 0`nplay_loop = 1" -Encoding ASCII
$env:NUVIEWER_TEST = '1'

$bmp = New-Object System.Drawing.Bitmap 8, 8, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear([System.Drawing.Color]::FromArgb(255, 255, 160, 0)); $g.Dispose()
$bmp.Save((Join-Path $work 'rig.png'), [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
Set-Content -Path (Join-Path $work 'rig.atlas') -Value "rig.png`nsize:8,8`nfilter:Linear,Linear`npma:false`nbox`nbounds:0,0,8,8`n" -Encoding ASCII
$json = '{"skeleton":{"hash":"x","spine":"4.1.24","x":-150,"y":-150,"width":300,"height":300,"fps":30},"bones":[{"name":"root"},{"name":"arm","parent":"root","length":100,"x":80}],"slots":[{"name":"back","bone":"root","attachment":"box"},{"name":"front","bone":"arm","attachment":"box"}],"skins":[{"name":"default","attachments":{"back":{"box":{"width":60,"height":60}},"front":{"box":{"width":50,"height":50,"x":50}}}}],"animations":{"idle":{"bones":{"arm":{"rotate":[{"time":0,"value":0},{"time":1,"value":180},{"time":2,"value":360}]}}},"other":{"bones":{"arm":{"rotate":[{"time":0,"value":0},{"time":1,"value":10}]}}}}}'
Set-Content -Path (Join-Path $work 'rig.json') -Value $json -Encoding ASCII
$file = Join-Path $work 'rig.json'

function Panel-Title { $t = Title; if ($t -match 'LAYERS') { $t } else { $null } }
function Differs($a, $b) {
    $n = 0
    for ($y = 40; $y -lt $a.Height - 40; $y += 6) { for ($x = 40; $x -lt $a.Width - 40; $x += 6) { if ($a.GetPixel($x, $y).ToArgb() -ne $b.GetPixel($x, $y).ToArgb()) { $n++ } } }
    $n
}
function Orange-Count($b) {
    $n = 0
    for ($y = 40; $y -lt $b.Height - 40; $y += 4) { for ($x = 40; $x -lt $b.Width - 340; $x += 4) { $c = $b.GetPixel($x, $y); if ($c.R -gt 200 -and $c.G -gt 120 -and $c.G -lt 200 -and $c.B -lt 60) { $n++ } } }
    $n
}

Write-Host "== Spine skeleton =="
if (-not (Open-Viewer $file @('--windowed'))) { Check 'the skeleton opens' $false (Title); Close-Viewer; exit 1 }
Start-Sleep -Milliseconds 1200
$b1 = Grab
Check 'the skeleton is drawn (orange pixels on screen)' ((Orange-Count $b1) -gt 30) "count $(Orange-Count $b1)"
Start-Sleep -Milliseconds 500
$b2 = Grab
Check 'it plays: the picture changes by itself' ((Differs $b1 $b2) -gt 5) "differing samples $(Differs $b1 $b2)"
$b1.Dispose(); $b2.Dispose()

Key 0x50   # P pauses
Start-Sleep -Milliseconds 400
$p1 = Grab; Start-Sleep -Milliseconds 600; $p2 = Grab
Check 'P pauses it' ((Differs $p1 $p2) -eq 0) "differing samples $(Differs $p1 $p2)"
$p1.Dispose(); $p2.Dispose()
Key 0x50   # and plays again
Start-Sleep -Milliseconds 500

# Ctrl+L: the slots are the rows
[void][In]::PostMessage($script:win, 0x8001, [IntPtr](0x4C -bor (1 -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 800
$t = Wait-Until { Panel-Title } 5000
Check 'Ctrl+L opens the panel in Spine mode' ($t -and $t -match 'state=Ready' -and $t -match 'rows=2') "$t"
if ($t -match 'list=([\d,]+) row_h=([\d.]+)') {
    $l = $Matches[1].Split(',') | ForEach-Object { [double]$_ }; $rh = [double]$Matches[2]; $d = $rh / 26
    # row 0 = the top slot ("front"): eye off
    Click ([int]($l[0] + 28 * $d)) ([int]($l[1] + $rh / 2))
    Start-Sleep -Milliseconds 700
    Check 'switching a slot off keeps the skeleton drawing' ((Panel-Title) -match 'shown=1') (Panel-Title)
    if ($Shots) { $b = Grab; $b.Save((Join-Path $Shots 'spine-panel.png')); $b.Dispose() }
    Click ([int]($l[0] + 28 * $d)) ([int]($l[1] + $rh / 2))
}
# Ctrl+Shift+S: a picture with transparency
Key 0x53 0 | Out-Null
[void][In]::PostMessage($script:win, 0x8001, [IntPtr](0x53 -bor (3 -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 1500
$saved = Get-ChildItem $work -Filter '*.png' | Where-Object { $_.Name -ne 'rig.png' } | Select-Object -First 1
Check 'the snapshot is saved as a PNG next to the file' ($null -ne $saved)
if ($saved) {
    $bm = [System.Drawing.Bitmap]::FromFile($saved.FullName)
    Check 'and its corner is transparent' ($bm.GetPixel(2, 2).A -eq 0) "alpha $($bm.GetPixel(2, 2).A)"
    $bm.Dispose()
}
Close-Viewer
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
Write-Host ("`n{0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip)
if ($script:fail -gt 0) { exit 1 }
