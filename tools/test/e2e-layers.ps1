param([string]$Exe = 'R:\cargo-target\NuViewer\debug\nuviewer.exe', [string]$Shots = '')
# Layer panel (Ctrl+L) on an OpenRaster file made here: eye, slider, filter, hover highlight, solo, reset, Save with transparency.
# Window messages only (no real mouse / keyboard). Needs no other NuViewer window.
. (Join-Path $PSScriptRoot 'lib.ps1')
$script:Exe = $Exe
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$work = Join-Path $env:TEMP ("nv_layers_" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $work | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $work 'settings.ini'
Set-Content -Path $env:NUVIEWER_SETTINGS -Value "fit_enlarge = 0`nsidebar = 0`nconfirm_delete = 0" -Encoding ASCII
$env:NUVIEWER_TEST = '1'

function Png-Bytes([int]$w, [int]$h, $c) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear($c); $g.Dispose()
    $ms = New-Object System.IO.MemoryStream; $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    return $ms.ToArray()
}
function Add-Entry($zip, [string]$name, [byte[]]$bytes) {
    $e = $zip.CreateEntry($name, [System.IO.Compression.CompressionLevel]::NoCompression)
    $s = $e.Open(); $s.Write($bytes, 0, $bytes.Length); $s.Close()
}

# 400 x 300 canvas. Top first: Blue (100x100 at 250,100), Green (100x100 at 50,100), Background (300x300 at 0,0, red). The right 100 px stay transparent.
$ora = Join-Path $work 'art.ora'
$zip = [System.IO.Compression.ZipFile]::Open($ora, 'Create')
Add-Entry $zip 'mimetype' ([Text.Encoding]::ASCII.GetBytes('image/openraster'))
$xml = '<?xml version="1.0"?><image w="400" h="300"><stack><layer name="Blue square" src="data/blue.png" x="250" y="100"/><layer name="Green square" src="data/green.png" x="50" y="100"/><layer name="Background" src="data/back.png" x="0" y="0"/></stack></image>'
Add-Entry $zip 'stack.xml' ([Text.Encoding]::UTF8.GetBytes($xml))
Add-Entry $zip 'data/blue.png' (Png-Bytes 100 100 ([System.Drawing.Color]::FromArgb(255, 0, 0, 255)))
Add-Entry $zip 'data/green.png' (Png-Bytes 100 100 ([System.Drawing.Color]::FromArgb(255, 0, 255, 0)))
Add-Entry $zip 'data/back.png' (Png-Bytes 300 300 ([System.Drawing.Color]::FromArgb(255, 200, 30, 30)))
# the merged picture (what a plain viewer shows)
Add-Entry $zip 'mergedimage.png' (Png-Bytes 400 300 ([System.Drawing.Color]::FromArgb(255, 200, 30, 30)))
$zip.Dispose()

function Inject([int]$vk, [int]$bits = 0) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($bits -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 400 }
function Panel-Info {
    $t = Title
    if ($t -notmatch 'LAYERS panel=([\d,]+) list=([\d,]+) row_h=([\d.]+) rows=(\d+) shown=(\d+) state=(\w+(?:\(.*?\))?) hover=(None|Some\(\d+\)) applied=(\w+)') { return $null }
    $l = $Matches[2].Split(',') | ForEach-Object { [double]$_ }
    [pscustomobject]@{ Panel = $Matches[1].Split(',') | ForEach-Object { [double]$_ }; List = $l; RowH = [double]$Matches[3]; Rows = [int]$Matches[4]; Shown = [int]$Matches[5]; State = $Matches[6]; Hover = $Matches[7]; Applied = $Matches[8]; D = [double]$Matches[3] / 26.0 }
}
function Row-Y($pi, [int]$slot) { [int]($pi.List[1] + $slot * $pi.RowH + $pi.RowH / 2) }
function Eye-X($pi) { [int]($pi.List[0] + 28 * $pi.D) }
function Name-X($pi) { [int]($pi.List[0] + 80 * $pi.D) }
function Slider-X($pi, [double]$frac) { $r = $pi.Panel[2] - 10 * $pi.D - 4 * $pi.D; $l = $r - 88 * $pi.D; [int]($l + ($r - $l) * $frac) }
function Shot([string]$name) { if ($Shots) { $b = Grab; $b.Save((Join-Path $Shots "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png); $b.Dispose() } }
# Colour at a picture point: the picture is fitted into the client area; read it back from the window.
function Client-Size { $r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r); @(($r.R - $r.L), ($r.B - $r.T)) }
function Colour-At([int]$px, [int]$py) {
    # find the picture rectangle by scanning for the red background along the middle row
    $b = Grab; $w = $b.Width; $h = $b.Height
    $s = [math]::Min($w / 400.0, $h / 300.0); if ($s -gt 1.0) { $s = 1.0 }
    $ox = ($w - 400 * $s) / 2; $oy = ($h - 300 * $s) / 2
    $c = $b.GetPixel([int]($ox + $px * $s), [int]($oy + $py * $s)); $b.Dispose(); $c
}

Write-Host "== layer panel on an OpenRaster file =="
if (-not (Open-Viewer $ora @('--windowed'))) { Check 'viewer starts on the .ora' $false; Close-Viewer; exit 1 }
Check 'the .ora shows its merged picture' ((Colour-At 20 20).R -gt 150)

Inject 0x4C 1   # Ctrl+L
$pi = Wait-Until { $p = Panel-Info; if ($p -and $p.State -eq 'Ready') { $p } } 8000
Check 'Ctrl+L opens the panel and the layers are read' ($null -ne $pi)
if (-not $pi) { Close-Viewer; exit 1 }
Check 'three layers, all on' (($pi.Rows -eq 3) -and ($pi.Shown -eq 3)) "rows=$($pi.Rows) shown=$($pi.Shown)"
Shot 'layers-open'

# Eye of the green layer (row 1): off. The picture is put together again with the green square gone.
Click (Eye-X $pi) (Row-Y $pi 1)
$ok = Wait-Until { $p = Panel-Info; if ($p -and $p.Shown -eq 2 -and $p.Applied -eq 'true') { $p } } 5000
Check 'eye: the layer goes off and a recomposed picture is shown' ($null -ne $ok)
Start-Sleep -Milliseconds 500
$c = Colour-At 100 150
Check 'the green square is gone (background shows through)' (($c.R -gt 150) -and ($c.G -lt 90)) "$c"
$c2 = Colour-At 300 150
Check 'the blue square is still there' (($c2.B -gt 200) -and ($c2.R -lt 60)) "$c2"
Shot 'layers-green-off'
Click (Eye-X $pi) (Row-Y $pi 1)
$null = Wait-Until { $p = Panel-Info; if ($p -and $p.Shown -eq 3) { $p } } 5000
Start-Sleep -Milliseconds 400
Check 'eye again: green is back' (((Colour-At 100 150).G -gt 200))

# Opacity slider of the blue layer (row 0) to half: the square mixes with the red below it.
Btn-Down (Slider-X $pi 0.5) (Row-Y $pi 0)
Btn-Up (Slider-X $pi 0.5) (Row-Y $pi 0)
Start-Sleep -Milliseconds 700
$m = Colour-At 280 150
Check 'slider at half: blue mixed with the red behind it' (($m.R -gt 70) -and ($m.R -lt 130) -and ($m.B -gt 100) -and ($m.B -lt 180)) "$m"
Shot 'layers-half'
# Double click on the slider: back to what the file says.
[void][In]::PostMessage($script:win, 0x0203, [IntPtr]1, (Pack (Slider-X $pi 0.5) (Row-Y $pi 0))); Start-Sleep -Milliseconds 700
$m = Colour-At 300 150
Check 'double click on the slider: opacity as the file has it' (($m.B -gt 200) -and ($m.R -lt 60)) "$m"

# Hover: the row under the pointer is highlighted (the title reports it).
Btn-Move (Name-X $pi) (Row-Y $pi 1) $false
$h = Wait-Until { $p = Panel-Info; if ($p -and $p.Hover -eq 'Some(1)') { $p } } 4000
Check 'hover: row 1 is the highlighted layer' ($null -ne $h)
Start-Sleep -Milliseconds 600
$dim = Colour-At 20 20
Check 'hover: the other layers are dimmed' ($dim.R -lt 120) "$dim"
Shot 'layers-hover'
Btn-Move 5 5 $false
$null = Wait-Until { $p = Panel-Info; if ($p -and $p.Hover -eq 'None') { $p } } 3000
Start-Sleep -Milliseconds 500
Check 'pointer away: back to full strength' ((Colour-At 20 20).R -gt 150)

# Filter: type "blue"; only that row stays.
$f = [int]($pi.Panel[0] + 40 * $pi.D); $fy = [int]($pi.Panel[1] + 54 * $pi.D)
Click $f $fy
foreach ($ch in 'b', 'l', 'u') { [void][In]::PostMessage($script:win, 0x0102, [IntPtr][int][char]$ch, [IntPtr]::Zero); Start-Sleep -Milliseconds 120 }
Start-Sleep -Milliseconds 300
$pf = Panel-Info
Check 'filter "blu": one row' ($pf.Rows -eq 1) "rows=$($pf.Rows)"
Shot 'layers-filter'
Key 0x1B   # Esc clears the filter, the box keeps the keyboard
Start-Sleep -Milliseconds 300
Check 'Esc empties the filter' ((Panel-Info).Rows -eq 3)
Key 0x1B   # Esc leaves the box

# Right click on a row = only that layer.
[void][In]::PostMessage($script:win, 0x0205, [IntPtr]0, (Pack (Name-X $pi) (Row-Y $pi 2))); [void][In]::PostMessage($script:win, 0x0204, [IntPtr]0, (Pack (Name-X $pi) (Row-Y $pi 2)))
Start-Sleep -Milliseconds 800
$only = Panel-Info
Check 'right click: only the background is drawn' ($only.Shown -eq 1) "shown=$($only.Shown)"

# Reset (4th button) and Save (3rd? order: All on, All off, Reset, Save).
$bw = ($pi.Panel[2] - $pi.Panel[0] - 20 * $pi.D - 18 * $pi.D) / 4
$by = [int]($pi.Panel[1] + 68 * $pi.D + 8 * $pi.D + 13 * $pi.D)
function Button-X([int]$i) { [int]($pi.Panel[0] + 10 * $pi.D + $i * ($bw + 6 * $pi.D) + $bw / 2) }
Click (Button-X 2) $by
$null = Wait-Until { $p = Panel-Info; if ($p -and $p.Shown -eq 3) { $p } } 4000
Check 'Reset: all three layers are back' ((Panel-Info).Shown -eq 3)

Click (Button-X 3) $by
Start-Sleep -Milliseconds 1500
$saved = Get-ChildItem $work -Filter '*.png' | Where-Object { $_.Name -notlike 'data*' } | Select-Object -First 1
Check 'Save writes a PNG next to the file' ($null -ne $saved)
if ($saved) {
    $bmp = [System.Drawing.Bitmap]::FromFile($saved.FullName)
    Check 'the saved picture is the canvas size' (($bmp.Width -eq 400) -and ($bmp.Height -eq 300)) "$($bmp.Width)x$($bmp.Height)"
    Check 'the transparent part stays transparent in the PNG' ($bmp.GetPixel(380, 20).A -eq 0) "alpha $($bmp.GetPixel(380, 20).A)"
    Check 'and the layers are in it' (($bmp.GetPixel(100, 150).G -gt 200) -and ($bmp.GetPixel(300, 150).B -gt 200))
    $bmp.Dispose()
}

# Esc closes the panel, and the file's own picture is read again.
Key 0x1B
Start-Sleep -Milliseconds 800
$t = Title
Check 'Esc closes the panel' ($t -notmatch 'LAYERS')
Close-Viewer
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
Write-Host ("`n{0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip)
if ($script:fail -gt 0) { exit 1 }
