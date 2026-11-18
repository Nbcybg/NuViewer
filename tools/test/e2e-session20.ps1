# Session 20: the gallery preview of a 4K60 MP4 plays at its own speed, a clip with an alpha channel plays with the
# transparency cut out (PNG-in-MOV, ProRes 4444), and a heavy 3D model turns without stalling the window.
# Windowed, scratch settings, window messages in, window title and pixels out; no real mouse or keyboard.
#   tools\test\e2e-session20.ps1 [-Dir <folder with nugallery.exe>] [-Bench <folder with the clips>] [-Shots <folder for pictures>]
param([string]$Dir = 'R:\cargo-target\NuViewer\release',
      [string]$Bench = 'T:\Benchmark-folder',
      [string]$Shots = $env:TEMP)
. (Join-Path $PSScriptRoot 'lib.ps1')
$g = Join-Path $Dir 'nugallery.exe'
if (-not (Test-Path $g)) { Write-Host "no nugallery.exe in $Dir"; exit 2 }
if (Get-Process nugallery -ErrorAction SilentlyContinue) { Write-Host 'NuGallery is already running (maybe yours): close it first'; exit 2 }
$base = Join-Path $Bench '_s20_work'
if (Test-Path $base) { Remove-Item $base -Recurse -Force }
New-Item -ItemType Directory $base | Out-Null
$st = Join-Path $env:TEMP 'nug_s20'; if (Test-Path $st) { Remove-Item $st -Recurse -Force }
New-Item -ItemType Directory $st | Out-Null
$env:NUGALLERY_STATE = Join-Path $st 'state.ini'
$env:NUVIEWER_SETTINGS = Join-Path $st 'settings.ini'
Set-Content $env:NUVIEWER_SETTINGS "start_fullscreen=0`r`nconfirm_delete=0`r`n" -Encoding ASCII
$env:LOCALAPPDATA = Join-Path $st 'local'; New-Item -ItemType Directory $env:LOCALAPPDATA -Force | Out-Null
$env:NUGALLERY_TEST = '1'
$ffd = Split-Path (Get-Command ffmpeg -ErrorAction SilentlyContinue).Source
if ($ffd) { $env:PATH = "$ffd;$env:PATH" }

# one clip per folder (hard links: nothing is copied), so each is the only item and gets the focus on its own
function Link([string]$name, [string]$sub) {
    $d = Join-Path $base $sub; New-Item -ItemType Directory $d -Force | Out-Null
    $src = Join-Path $Bench $name
    if (-not (Test-Path $src)) { return $null }
    $dst = Join-Path $d $name
    New-Item -ItemType HardLink -Path $dst -Target $src | Out-Null
    return $d
}
$script:gp = $null
function Start-Gallery([string]$arg) {
    Stop-Gallery
    $script:gp = Start-Process $g -ArgumentList @("`"$arg`"", '--windowed', '--new-window') -PassThru
    $script:win = Wait-Until { $script:gp.Refresh(); if ($script:gp.MainWindowHandle -ne 0 -and $script:gp.MainWindowTitle -like '*NuGallery*') { $script:gp.MainWindowHandle } } 15000
    Start-Sleep -Milliseconds 1200
}
function Stop-Gallery { if ($script:gp -and -not $script:gp.HasExited) { $script:gp.Kill(); [void]$script:gp.WaitForExit(3000) }; $script:gp = $null }
function GT { $script:gp.Refresh(); $script:gp.MainWindowTitle }
function Inject([int]$vk, [int]$mods = 0, [int]$wait = 400) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
function Grab {
    $r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
    $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
    $gg = [System.Drawing.Graphics]::FromImage($bmp); $dc = $gg.GetHdc()
    [void][In]::PrintWindow($script:win, $dc, 2); $gg.ReleaseHdc($dc); $gg.Dispose()
    return $bmp
}
function Dpi { [In]::GetDpiForWindow($script:win) / 96.0 }
function Btn-Down([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0201, [IntPtr]1, (Pack $x $y)); Start-Sleep -Milliseconds 60 }
function Btn-Move([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0200, [IntPtr]1, (Pack $x $y)); Start-Sleep -Milliseconds 12 }
function Btn-Up([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0202, [IntPtr]0, (Pack $x $y)); Start-Sleep -Milliseconds 100 }
function Pos { if ((GT) -match 'pos=(\d+)') { [int]$Matches[1] } else { -1 } }

# where the preview box and its play button are (same geometry as e2e-fixes.ps1)
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class Cr { [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out R r); [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; } }'
function Box {
    $r = New-Object Cr+R; [void][Cr]::GetClientRect($script:win, [ref]$r); $d = Dpi
    $rw = [math]::Min(360 * $d, $r.Rt * 0.45); $pad = 14 * $d
    $pl = $r.Rt - $rw + $pad; $pr = $r.Rt - $pad
    $ph = [math]::Min(($pr - $pl) * 0.72, 250 * $d); $pt = 36 * $d
    @($pl, $pt, $pr, ($pt + $ph))
}

function Play-Test([string]$label, [string]$file, [string]$shot) {
    $d = Link $file $label
    if (-not $d) { Skip $label "$file not in $Bench"; return $null }
    Start-Gallery $d
    Inject 0x49 1 1500     # Ctrl+I: keep the details panel open
    $ok = Wait-Until { (GT) -match 'MEDIA\[open=True' } 15000
    Check "$label : the preview opens a player" ($null -ne $ok) (GT)
    if (-not $ok) { return $null }
    $b = Box
    $px = [int]($b[0] + 18 * (Dpi)); $py = [int]($b[3] - 15 * (Dpi))
    Btn-Down $px $py; Btn-Up $px $py
    Start-Sleep -Milliseconds 2500
    $p1 = Pos; $t0 = [Diagnostics.Stopwatch]::StartNew()
    # sample the picture for 3 s: how many different pictures does the preview show?
    $seen = 0; $last = 0; $sig = @{}
    while ($t0.ElapsedMilliseconds -lt 3000) {
        $bm = Grab
        $h = 17
        for ($y = [int]($b[1] + 10); $y -lt [int]($b[3] - 40); $y += 23) { for ($x = [int]$b[0] + 10; $x -lt [int]$b[2] - 10; $x += 23) { $h = ($h * 31 + $bm.GetPixel($x, $y).ToArgb()) -band 0x7FFFFFFF } }
        $bm.Dispose()
        $sig[$h] = 1
    }
    $p2 = Pos
    Check "$label : plays at its own speed (clock moved $($p2 - $p1) ms in $($t0.ElapsedMilliseconds) ms)" (($p2 - $p1) -gt 2400) "pos $p1 -> $p2"
    Check "$label : the picture keeps changing ($($sig.Count) different looks)" ($sig.Count -gt 8) "looks=$($sig.Count)"
    $bm = Grab; $bm.Save((Join-Path $Shots "s20-$label.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    # the preview box: a corner of the picture must show the panel colour (alpha cut out), not black
    $bm.Dispose()
    Stop-Gallery
    return $b
}

[void](Play-Test 'mp4-4k60' '20231230_132319.mp4')
[void](Play-Test 'alpha-png-mov' 'bokeh_loop_alpha1.mov')
[void](Play-Test 'alpha-prores' 'asianface.mov')

# ---- the 3D model turns without stalling the window ----
foreach ($m in @('1cf82e4d8f1a3790a1fd1639200ed16c.glb', 'e4dcdfb2b5bddb3a456f83fe6ba6b753.stl')) {
    $d = Link $m ('model-' + [IO.Path]::GetExtension($m).TrimStart('.'))
    if (-not $d) { Skip "3D $m" 'not in the folder'; continue }
    Start-Gallery $d
    Inject 0x49 1 1500
    $b = Box
    Start-Sleep -Milliseconds 4000    # the model loads (1-2 s) and its first picture is drawn
    $cx = [int](($b[0] + $b[2]) / 2); $cy = [int](($b[1] + $b[3]) / 2)
    # The window must keep answering while it is dragged: time a round trip of messages (SendMessage returns only when the
    # window thread has handled it) around the drag.
    Btn-Down $cx $cy
    $worst = 0; $sw = [Diagnostics.Stopwatch]::StartNew(); $n = 0
    for ($i = 0; $i -lt 120; $i++) {
        Btn-Move ($cx + ($i % 40) * 3) ($cy + [int]([math]::Sin($i / 9) * 30))
        $t = [Diagnostics.Stopwatch]::StartNew(); [void][In]::SendMessage($script:win, 0x0000, [IntPtr]::Zero, [IntPtr]::Zero); $t.Stop()
        if ($t.ElapsedMilliseconds -gt $worst) { $worst = $t.ElapsedMilliseconds }; $n++
    }
    Btn-Up $cx $cy
    Check "$m : the window answers while the model turns (slowest answer $worst ms, $($sw.ElapsedMilliseconds) ms for $n moves)" ($worst -lt 60) "worst=$worst"
    Start-Sleep -Milliseconds 900
    $bm = Grab; $bm.Save((Join-Path $Shots ("s20-" + [IO.Path]::GetFileName($d) + ".png")), [System.Drawing.Imaging.ImageFormat]::Png); $bm.Dispose()
    Stop-Gallery
}

Remove-Item $base -Recurse -Force -ErrorAction SilentlyContinue
Write-Host ""
Write-Host "passed $($script:pass), failed $($script:fail), skipped $($script:skip)"
Get-Process nugallery -ErrorAction SilentlyContinue | Stop-Process -Force
exit $(if ($script:fail -gt 0) { 1 } else { 0 })
