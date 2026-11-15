# How fast does NuGallery put the thumbnails of a folder on the screen?
#
#   tools\bench\gallery-thumbs.ps1                       (release build from CARGO_TARGET_DIR or .\target)
#   tools\bench\gallery-thumbs.ps1 -Folder D:\Photos     (a folder of your own: read only, nothing is written there)
#   tools\bench\gallery-thumbs.ps1 -Runs 7 -Exe C:\x\nugallery.exe
#
# For every folder it starts the gallery windowed (1280x800) with --bench-out/--bench-exit and reads the timings the
# program writes itself (milliseconds since the process started):
#   window  the window exists          listing  the folder is read          gpu  Direct3D is up
#   first   the first thumbnail is ready (decoded)       cpu  the first picture is on screen (painted on the CPU
#   while Direct3D starts)       frame  the first Direct3D frame is drawn
#   ready   every thumbnail that fits on the screen is on the GPU (this is "thumbnails shown")
# and, from outside, `wall`: process start until the window title says the thumbnails are ready.
#
# COLD = no thumbnail cache (a scratch LOCALAPPDATA, so your real cache is never touched or emptied), so every
# thumbnail is decoded from the picture. WARM = the cache made by the cold run (a 256 px JPEG per picture).
# The numbers are the median of -Runs runs; min in brackets.
param(
    [string]$Exe,
    [string[]]$Folder = @(),
    [int]$Runs = 5
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $Exe) {
    $t = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    $Exe = Join-Path $t 'release\nugallery.exe'
}
if (-not (Test-Path $Exe)) { throw "missing $Exe (cargo build --release -p nugallery)" }
Add-Type -AssemblyName System.Drawing

$scratch = Join-Path $env:TEMP "nug_bench_$PID"
New-Item -ItemType Directory $scratch -Force | Out-Null

# Folders to measure: made here unless given.
$sets = @()
foreach ($f in $Folder) { $sets += [pscustomobject]@{ Name = (Split-Path $f -Leaf); Path = $f } }
if ($sets.Count -eq 0) {
    $png = Join-Path $scratch 'png200'
    New-Item -ItemType Directory $png | Out-Null
    $rnd = New-Object System.Random 7
    for ($i = 0; $i -lt 200; $i++) {
        $bmp = New-Object System.Drawing.Bitmap 1000, 700
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.Clear([System.Drawing.Color]::FromArgb($rnd.Next(256), $rnd.Next(256), $rnd.Next(256)))
        for ($k = 0; $k -lt 6; $k++) {
            $br = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb($rnd.Next(256), $rnd.Next(256), $rnd.Next(256)))
            $g.FillEllipse($br, $rnd.Next(800), $rnd.Next(500), 50 + $rnd.Next(300), 50 + $rnd.Next(300)); $br.Dispose()
        }
        $g.Dispose(); $bmp.Save((Join-Path $png ("p{0:000}.png" -f $i))); $bmp.Dispose()
    }
    $sets += [pscustomobject]@{ Name = '200 PNG, 1000x700'; Path = $png }
    # 12 MP photos: the generated set of tools\bench\gen.ps1 if it exists (copied under new names so each is its own file).
    $src = Join-Path $PSScriptRoot 'img12'
    if (Test-Path $src) {
        $jpg = Join-Path $scratch 'jpeg12mp'
        New-Item -ItemType Directory $jpg | Out-Null
        $n = 0
        foreach ($rep in 1..5) { foreach ($f in Get-ChildItem $src -Filter *.jpg) { Copy-Item $f.FullName (Join-Path $jpg ("j{0:000}.jpg" -f $n)); $n++ } }
        $sets += [pscustomobject]@{ Name = "$n JPEG, 12 MP"; Path = $jpg }
    } else { Write-Host "(no tools\bench\img12: run tools\bench\gen.ps1 to add the 12 MP set)" }
}

function Median([double[]]$v) { $s = $v | Sort-Object; if ($s.Count % 2) { $s[[int](($s.Count - 1) / 2)] } else { ($s[$s.Count / 2 - 1] + $s[$s.Count / 2]) / 2 } }

function Run-One([string]$dir, [string]$local) {
    $out = Join-Path $scratch 'bench.txt'
    Remove-Item $out -ErrorAction SilentlyContinue
    $env:LOCALAPPDATA = $local
    $env:NUGALLERY_STATE = Join-Path $scratch 'state.ini'
    $env:NUVIEWER_SETTINGS = Join-Path $scratch 'settings.ini'
    Set-Content $env:NUVIEWER_SETTINGS "start_fullscreen=0`r`n" -Encoding ASCII
    Remove-Item $env:NUGALLERY_STATE -ErrorAction SilentlyContinue
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process $Exe -ArgumentList @("`"$dir`"", '--windowed', '--new-window', '--bench-out', "`"$out`"", '--bench-exit') -PassThru
    $wall = $null
    while ($sw.ElapsedMilliseconds -lt 30000 -and -not $p.HasExited) {
        $p.Refresh()
        if (-not $wall -and $p.MainWindowTitle -like '*thumbnails ready*') { $wall = $sw.Elapsed.TotalMilliseconds }
        if ($wall -and (Test-Path $out)) { break }
        Start-Sleep -Milliseconds 3
    }
    [void]$p.WaitForExit(3000)
    if (-not $p.HasExited) { $p.Kill() }
    if (-not (Test-Path $out)) { return $null }
    $r = @{ wall = $wall }
    foreach ($l in Get-Content $out) { $k, $v = $l -split '=', 2; $r[$k] = $v }
    return $r
}

foreach ($s in $sets) {
    Write-Host ""
    Write-Host ("=== {0}  ({1})" -f $s.Name, $s.Path)
    foreach ($mode in 'cold', 'warm') {
        $rows = @()
        for ($i = 0; $i -lt $Runs; $i++) {
            $local = Join-Path $scratch "local_$($s.Name -replace '[^A-Za-z0-9]', '_')"
            if ($mode -eq 'cold' -or -not (Test-Path $local)) { if (Test-Path $local) { Remove-Item $local -Recurse -Force }; New-Item -ItemType Directory $local | Out-Null }
            $r = Run-One $s.Path $local
            if ($r) { $rows += $r }
            if ($mode -eq 'cold' -and $i -eq 0) { }
        }
        if ($rows.Count -eq 0) { Write-Host "  $mode : no result"; continue }
        $fmt = {
            param($key)
            $vals = $rows | ForEach-Object { if ($_[$key] -and $_[$key] -ne 'n/a') { [double]$_[$key] } } | Where-Object { $_ -ne $null }
            if (-not $vals) { 'n/a' } else { '{0,6:N0} ({1:N0})' -f (Median $vals), ($vals | Measure-Object -Minimum).Minimum }
        }
        $items = $rows[0]['items']; $vis = $rows[0]['visible_count']
        Write-Host ("  {0}: window {1}  listing {2}  gpu {3}  first {4}  cpu {10}  frame {5}  READY {6} ms   wall {7}   [{8} items, {9} thumbnails on screen]" -f $mode.ToUpper(), (& $fmt 'window_ms'), (& $fmt 'listing_ms'), (& $fmt 'gpu_ms'), (& $fmt 'first_thumb_ms'), (& $fmt 'first_frame_ms'), (& $fmt 'visible_ready_ms'), (& $fmt 'wall'), $items, $vis, (& $fmt 'cpu_frame_ms'))
        if ($mode -eq 'cold') { }
    }
}
Remove-Item $scratch -Recurse -Force -ErrorAction SilentlyContinue
