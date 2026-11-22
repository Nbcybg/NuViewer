param([string]$Exe = 'R:\cargo-target\NuViewer\debug\nuviewer.exe', [string]$Core = 'R:\cargo-target\NuViewer\debug\Live2DCubismCore.dll', [string]$Shots = '')
# Live2D player against the MOCK Cubism Core (tools/mock-cubism: `cargo build -p mock-cubism`). Proves the whole pipeline (model files, Core binding,
# motion, parts as layers, snapshot) with the documented API; it says nothing about Live2D's real Core. Messages only; needs no other NuViewer window.
. (Join-Path $PSScriptRoot 'lib.ps1')
$script:Exe = $Exe
if (-not (Test-Path $Core)) { Write-Host "mock Core not built: $Core"; exit 2 }
$work = Join-Path $env:TEMP ("nv_l2d_" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $work | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $work 'settings.ini'
Set-Content -Path $env:NUVIEWER_SETTINGS -Value "fit_enlarge = 0`nsidebar = 0`nconfirm_delete = 0`nplay_loop = 1" -Encoding ASCII
$env:NUVIEWER_TEST = '1'
$env:NUVIEWER_CUBISM_CORE = $Core

$bmp = New-Object System.Drawing.Bitmap 4, 4, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear([System.Drawing.Color]::FromArgb(255, 255, 160, 0)); $g.Dispose()
$bmp.Save((Join-Path $work 'tex.png'), [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
$moc = New-Object byte[] 128; [Text.Encoding]::ASCII.GetBytes('MOC3').CopyTo($moc, 0)
[IO.File]::WriteAllBytes((Join-Path $work 'm.moc3'), $moc)
Set-Content (Join-Path $work 'm.model3.json') '{"Version":3,"FileReferences":{"Moc":"m.moc3","Textures":["tex.png"],"DisplayInfo":"m.cdi3.json","Motions":{"Idle":[{"File":"sway.motion3.json"}]}}}' -Encoding ASCII
Set-Content (Join-Path $work 'm.cdi3.json') '{"Version":3,"Parameters":[],"Parts":[{"Id":"PartBody","Name":"Body part"},{"Id":"PartHead","Name":"Head part"}]}' -Encoding ASCII
Set-Content (Join-Path $work 'sway.motion3.json') '{"Version":3,"Meta":{"Duration":2.0,"Fps":30,"Loop":true},"Curves":[{"Target":"Parameter","Id":"ParamAngleX","Segments":[0,-30,0,1,30,0,2,-30]}]}' -Encoding ASCII
$file = Join-Path $work 'm.model3.json'

function Panel-Title { $t = Title; if ($t -match 'LAYERS') { $t } else { $null } }
function Differs($a, $b) { $n = 0; for ($y = 40; $y -lt $a.Height - 40; $y += 6) { for ($x = 40; $x -lt $a.Width - 40; $x += 6) { if ($a.GetPixel($x, $y).ToArgb() -ne $b.GetPixel($x, $y).ToArgb()) { $n++ } } }; $n }
function Orange-Count($b) { $n = 0; for ($y = 40; $y -lt $b.Height - 40; $y += 4) { for ($x = 40; $x -lt $b.Width - 340; $x += 4) { $c = $b.GetPixel($x, $y); if ($c.R -gt 200 -and $c.G -gt 120 -and $c.G -lt 200 -and $c.B -lt 60) { $n++ } } }; $n }

Write-Host "== Live2D model (mock Core) =="
if (-not (Open-Viewer $file @('--windowed'))) { Check 'the model opens' $false (Title); Close-Viewer; exit 1 }
Start-Sleep -Milliseconds 1200
$b1 = Grab
Check 'the model is drawn' ((Orange-Count $b1) -gt 30) "count $(Orange-Count $b1)"
Start-Sleep -Milliseconds 500
$b2 = Grab
Check 'it plays: the motion moves the head' ((Differs $b1 $b2) -gt 3) "differing samples $(Differs $b1 $b2)"
$b1.Dispose(); $b2.Dispose()
Key 0x50; Start-Sleep -Milliseconds 400
$p1 = Grab; Start-Sleep -Milliseconds 600; $p2 = Grab
Check 'P pauses it' ((Differs $p1 $p2) -eq 0)
$p1.Dispose(); $p2.Dispose()
[void][In]::PostMessage($script:win, 0x8001, [IntPtr](0x4C -bor (1 -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 800
$t = Wait-Until { Panel-Title } 5000
Check 'Ctrl+L opens the panel with the two parts' ($t -and $t -match 'state=Ready' -and $t -match 'rows=2') "$t"
if ($t -match 'list=([\d,]+) row_h=([\d.]+)') {
    $l = $Matches[1].Split(',') | ForEach-Object { [double]$_ }; $rh = [double]$Matches[2]; $d = $rh / 26
    Click ([int]($l[0] + 28 * $d)) ([int]($l[1] + $rh / 2))   # the top row = the last part ("Head part"): off
    Start-Sleep -Milliseconds 700
    Check 'a part switched off is not drawn (its opacity goes to 0)' ((Panel-Title) -match 'shown=1') (Panel-Title)
    if ($Shots) { $b = Grab; $b.Save((Join-Path $Shots 'l2d-panel.png')); $b.Dispose() }
}
[void][In]::PostMessage($script:win, 0x8001, [IntPtr](0x53 -bor (3 -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 1500
$saved = Get-ChildItem $work -Filter '*.png' | Where-Object { $_.Name -ne 'tex.png' } | Select-Object -First 1
Check 'the snapshot is saved as a PNG next to the model' ($null -ne $saved)
if ($saved) { $bm = [System.Drawing.Bitmap]::FromFile($saved.FullName); Check 'and its corner is transparent' ($bm.GetPixel(1, 1).A -eq 0); $bm.Dispose() }
Close-Viewer
Write-Host "== without the Core =="
$env:NUVIEWER_CUBISM_CORE = (Join-Path $work 'nowhere.dll')
$env:APPDATA = $work   # nothing in %APPDATA%\NuViewer\live2d either
New-Item -ItemType Directory -Path (Join-Path $work 'bin') | Out-Null   # a copy of the program in a folder with no DLL beside it
Copy-Item $Exe (Join-Path $work 'bin\nuviewer.exe')
$script:Exe = Join-Path $work 'bin\nuviewer.exe'
if (Open-Viewer $file @('--windowed')) { Start-Sleep -Milliseconds 800 }
Check 'with no Core the viewer says how to get it' ((Title) -match 'Live2DCubismCore|cannot open') (Title)
Close-Viewer
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
Write-Host ("`n{0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip)
if ($script:fail -gt 0) { exit 1 }
