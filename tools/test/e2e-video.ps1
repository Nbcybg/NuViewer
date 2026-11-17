# Does a video really play in the viewer? For each container / codec ffmpeg can make: open it windowed, wait for it to start
# (an unsupported one gets a playable copy first), then look at the window twice, a second apart: the picture must change.
# Window pixels only (PrintWindow); no real mouse or keyboard.
#   tools\test\e2e-video.ps1 [-Dir <folder with nuviewer.exe>] [-Seconds 25]
param([string]$Dir = $(if ($env:CARGO_TARGET_DIR) { Join-Path $env:CARGO_TARGET_DIR 'debug' } else { 'R:\cargo-target\NuViewer\debug' }),
      [int]$Seconds = 25)
$script:Exe = Join-Path $Dir 'nuviewer.exe'
. (Join-Path $PSScriptRoot 'lib.ps1')
if (-not (Test-Path $script:Exe)) { Write-Host "no nuviewer.exe in $Dir"; exit 2 }
$ff = (Get-Command ffmpeg -ErrorAction SilentlyContinue).Source
if (-not $ff) { $ff = (Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter ffmpeg.exe -ErrorAction SilentlyContinue | Select-Object -First 1).FullName }
if (-not $ff) { Write-Host 'no ffmpeg to make the test clips'; exit 2 }
$base = Join-Path $env:TEMP 'nuv_video'
if (Test-Path $base) { Remove-Item $base -Recurse -Force }
New-Item -ItemType Directory $base | Out-Null
$st = Join-Path $base 'state'; New-Item -ItemType Directory $st | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $st 'settings.ini'
Set-Content $env:NUVIEWER_SETTINGS "start_fullscreen=0`r`n" -Encoding ASCII
$env:LOCALAPPDATA = Join-Path $st 'local'; New-Item -ItemType Directory $env:LOCALAPPDATA -Force | Out-Null
$src = 'testsrc2=duration=12:size=1280x720:rate=30'
$clips = @(
    @{ n = 'h264.mp4';  a = @('-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac') },
    @{ n = 'h264.mov';  a = @('-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac') },
    @{ n = 'h264.mkv';  a = @('-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac') },
    @{ n = 'h264b.mp4'; a = @('-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-bf', '3', '-g', '250', '-c:a', 'aac') },
    @{ n = 'hevc.mp4';  a = @('-c:v', 'libx265', '-pix_fmt', 'yuv420p', '-c:a', 'aac') },
    @{ n = 'vp9.webm';  a = @('-c:v', 'libvpx-vp9', '-b:v', '1M', '-c:a', 'libopus') },
    @{ n = 'mpeg4.avi'; a = @('-c:v', 'mpeg4', '-c:a', 'mp3') },
    @{ n = 'wmv.wmv';   a = @('-c:v', 'wmv2', '-c:a', 'wmav2') },
    @{ n = 'flv.flv';   a = @('-c:v', 'flv', '-c:a', 'mp3') },
    @{ n = 'silent.mp4'; a = @('-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-an') }
)
foreach ($c in $clips) {
    $out = Join-Path $base $c.n
    $args2 = @('-v', 'error', '-y', '-f', 'lavfi', '-i', $src)
    if ($c.a -notcontains '-an') { $args2 += @('-f', 'lavfi', '-i', 'sine=frequency=440:duration=12') }
    $args2 += $c.a + @('-shortest', $out)
    & $ff @args2 2>$null
}
function Frame-Diff([int]$samples = 3) {
    # how many sampled pixels of the middle of the window differ between two looks 700 ms apart
    $b1 = Grab; Start-Sleep -Milliseconds 700; $b2 = Grab
    $diff = 0; $total = 0
    for ($y = [int]($b1.Height * 0.25); $y -lt [int]($b1.Height * 0.75); $y += 6) { for ($x = [int]($b1.Width * 0.25); $x -lt [int]($b1.Width * 0.75); $x += 6) { $total++; if ($b1.GetPixel($x, $y).ToArgb() -ne $b2.GetPixel($x, $y).ToArgb()) { $diff++ } } }
    $b1.Dispose(); $b2.Dispose()
    return @($diff, $total)
}
foreach ($c in $clips) {
    $f = Join-Path $base $c.n
    if (-not (Test-Path $f)) { Skip $c.n 'ffmpeg could not make it'; continue }
    $ok = Open-Viewer $f @('--windowed')
    if (-not $ok) { Check "$($c.n): opens" $false (Title); Close-Viewer; continue }
    # wait until the picture moves (the proxy for unsupported video can take a while)
    $sw = [Diagnostics.Stopwatch]::StartNew(); $moving = $false; $last = @(0, 1)
    while ($sw.Elapsed.TotalSeconds -lt $Seconds) {
        Start-Sleep -Milliseconds 800
        $last = Frame-Diff
        if ($last[0] -gt 20) { $moving = $true; break }
    }
    $t = [math]::Round($sw.Elapsed.TotalSeconds, 1)
    Check "$($c.n): the picture plays (moving after $t s)" $moving ("changed $($last[0]) of $($last[1]) samples; title: " + (Title))
    Close-Viewer
}
Write-Host ""
Write-Host "passed $($script:pass), failed $($script:fail), skipped $($script:skip)"
exit $(if ($script:fail -gt 0) { 1 } else { 0 })
