param([string]$Exe = 'R:\cargo-target\NuViewer\debug\nuviewer.exe', [string]$Samples = 'T:\Benchmark-folder\spine-samples', [string]$Shots = '')
# Spine 4.3 files made by Esoteric (the example assets of playcanvas-spine, kept outside the repository): the binary .skel of four skeletons open, draw and
# play - path constraints (tank treads), physics (snowglobe), sliders and image sequences (diamond). Skips itself when the folder is absent.
# Window messages only (no real mouse / keyboard). Needs no other NuViewer window.
. (Join-Path $PSScriptRoot 'lib.ps1')
$script:Exe = $Exe
if (-not (Test-Path $Samples)) { Write-Host "skipped: no sample folder $Samples"; exit 0 }
$work = Join-Path $env:TEMP ("nv_spine43_" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $work | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $work 'settings.ini'
Set-Content -Path $env:NUVIEWER_SETTINGS -Value "fit_enlarge = 0`nsidebar = 0`nconfirm_delete = 0`nplay_loop = 1" -Encoding ASCII
$env:NUVIEWER_TEST = '1'

function Differs($a, $b) {
    $n = 0
    for ($y = 40; $y -lt $a.Height - 40; $y += 5) { for ($x = 40; $x -lt $a.Width - 40; $x += 5) { if ($a.GetPixel($x, $y).ToArgb() -ne $b.GetPixel($x, $y).ToArgb()) { $n++ } } }
    $n
}
# How many sampled pixels are not the colour of the window corner (the canvas).
function Painted($b) {
    $bg = $b.GetPixel(60, 60).ToArgb(); $n = 0
    for ($y = 40; $y -lt $b.Height - 40; $y += 5) { for ($x = 40; $x -lt $b.Width - 40; $x += 5) { if ($b.GetPixel($x, $y).ToArgb() -ne $bg) { $n++ } } }
    $n
}

foreach ($s in @(
    @{ name = 'tank (path + transform constraints)'; rel = 'spine-4.3\tank\tank-pro.skel'; moves = $true },
    @{ name = 'snowglobe (physics)'; rel = 'spine-4.3\snowglobe\snowglobe-pro.skel'; moves = $true },
    @{ name = 'celestial circus (physics, transform)'; rel = 'spine-4.3\celestial-circus\celestial-circus-pro.skel'; moves = $true },
    @{ name = 'diamond (slider, image sequences)'; rel = 'spine-4.3\diamond\diamond-pro.skel'; moves = $true }
)) {
    $file = Join-Path $Samples $s.rel
    if (-not (Test-Path $file)) { Write-Host "  skip  $($s.name): no $file"; continue }
    # The skeleton alone: the JSON beside it must not be what is read, so it is copied away without it.
    $dir = Join-Path $work ([IO.Path]::GetFileNameWithoutExtension($file))
    New-Item -ItemType Directory -Path $dir | Out-Null
    Get-ChildItem (Split-Path $file) -File | Where-Object { $_.Extension -ne '.json' } | ForEach-Object { Copy-Item $_.FullName $dir }
    $copy = Join-Path $dir (Split-Path $file -Leaf)
    Write-Host "== $($s.name) =="
    if (-not (Open-Viewer $copy @('--windowed'))) { Check 'opens' $false (Title); Close-Viewer; continue }
    Start-Sleep -Milliseconds 1800
    $b1 = Grab
    Check 'is drawn' ((Painted $b1) -gt 200) "painted samples $(Painted $b1)"
    Start-Sleep -Milliseconds 700
    $b2 = Grab
    if ($s.moves) { Check 'plays: the picture changes by itself' ((Differs $b1 $b2) -gt 5) "differing samples $(Differs $b1 $b2)" }
    if ($Shots) { $b2.Save((Join-Path $Shots ("spine43-" + [IO.Path]::GetFileNameWithoutExtension($file) + '.png'))) }
    $b1.Dispose(); $b2.Dispose()
    Close-Viewer
}
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
Write-Host ("`n{0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip)
if ($script:fail -gt 0) { exit 1 }
