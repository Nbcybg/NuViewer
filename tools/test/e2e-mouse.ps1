# End-to-end test of the viewer's mouse / fullscreen behaviour.
#   tools\test\e2e-mouse.ps1 [-Exe path\to\nuviewer.exe] [-RealInput]
#
# Everything is sent to the viewer's own window as Windows messages (wheel, button down/move/up with the Ctrl flag,
# right button up), and the result is read back from the window's own pixels (PrintWindow), so it never touches your
# real mouse or keyboard and keeps working while another program (a game, say) has the focus.
# Pictures are synthetic: a green square in the middle of a red field; its size on screen tells the zoom.
#
# -RealInput adds the one thing messages can't show, the taskbar reveal at the bottom screen edge. It moves the
# real pointer, so it only runs if the viewer really is the foreground window and the pointer really moves; if
# another program holds the focus or clips the cursor (many games do) it is skipped with a note, never forced.
param([string]$Exe, [switch]$RealInput)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $Exe) { $Exe = Join-Path $target 'release\nuviewer.exe' }
if (-not (Test-Path $Exe)) { throw "missing $Exe" }

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class In {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
    [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetMenuString(IntPtr m, uint pos, StringBuilder s, int n, uint flags);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    public static string MenuText(IntPtr m, int i) { var s = new StringBuilder(128); GetMenuString(m, (uint)i, s, 128, 0x400); return s.ToString(); }
}
'@
[void][In]::SetProcessDPIAware()
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

$script:pass = 0; $script:fail = 0; $script:skip = 0
function Check([string]$name, [bool]$ok, [string]$detail = '') {
    if ($ok) { $script:pass++; Write-Host "  PASS  $name" -ForegroundColor Green }
    else { $script:fail++; Write-Host "  FAIL  $name  $detail" -ForegroundColor Red }
}
function Skip([string]$name, [string]$why) { $script:skip++; Write-Host "  SKIP  $name  ($why)" -ForegroundColor Yellow }
function Wait-Until([scriptblock]$cond, [int]$ms = 6000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $ms) { $r = & $cond; if ($r) { return $r }; Start-Sleep -Milliseconds 40 }
    return $null
}
function Pack([int]$x, [int]$y) { [IntPtr]([int](($x -band 0xFFFF) -bor (($y -band 0xFFFF) -shl 16))) }
function Wheel([int]$delta, [int]$x, [int]$y) {
    [void][In]::PostMessage($script:win, 0x020A, [IntPtr](($delta -band 0xFFFF) -shl 16), (Pack $x $y)); Start-Sleep -Milliseconds 350
}
function Btn-Down([int]$x, [int]$y, [bool]$ctrl = $false) { [void][In]::PostMessage($script:win, 0x0201, [IntPtr]$(if ($ctrl) { 9 } else { 1 }), (Pack $x $y)); Start-Sleep -Milliseconds 250 }
function Btn-Move([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0200, [IntPtr]1, (Pack $x $y)); Start-Sleep -Milliseconds 250 }
function Btn-Up([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0202, [IntPtr]0, (Pack $x $y)); Start-Sleep -Milliseconds 250 }
function Title { $script:viewer.Refresh(); $script:viewer.MainWindowTitle }

# Pixels of the viewer's own window (works even when something else is on top of it).
function Grab {
    $r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
    $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
    $g = [System.Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc()
    [void][In]::PrintWindow($script:win, $dc, 2); $g.ReleaseHdc($dc); $g.Dispose()
    return $bmp
}
# Bounding box of the green marker near (cx, cy): width, height, centre x, centre y (zeros if none).
function Green-Box([int]$cx, [int]$cy, [int]$half) {
    $bmp = Grab
    $minx = 99999; $miny = 99999; $maxx = -1; $maxy = -1
    for ($y = [math]::Max(0, $cy - $half); $y -lt [math]::Min($bmp.Height, $cy + $half); $y++) {
        for ($x = [math]::Max(0, $cx - $half); $x -lt [math]::Min($bmp.Width, $cx + $half); $x++) {
            $c = $bmp.GetPixel($x, $y)
            if ($c.G -gt 180 -and $c.R -lt 90 -and $c.B -lt 90) { if ($x -lt $minx) { $minx = $x }; if ($x -gt $maxx) { $maxx = $x }; if ($y -lt $miny) { $miny = $y }; if ($y -gt $maxy) { $maxy = $y } }
        } }
    $bmp.Dispose()
    if ($maxx -lt 0) { return @(0, 0, 0, 0) }
    return @(($maxx - $minx + 1), ($maxy - $miny + 1), (($minx + $maxx) / 2), (($miny + $maxy) / 2))
}
function Make-Picture([string]$path, [int]$w, [int]$h, [int]$sq, $bgc = [System.Drawing.Color]::FromArgb(200, 30, 30)) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear($bgc)
    $g.FillRectangle((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(0, 255, 0))), [int](($w - $sq) / 2), [int](($h - $sq) / 2), $sq, $sq)
    $g.Dispose(); $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
function Open-Viewer([string]$file) {
    $p = Start-Process $Exe -ArgumentList @("`"$file`"", '--new-window') -PassThru
    $name = [IO.Path]::GetFileName($file)
    $w = Wait-Until { $p.Refresh(); if ($p.MainWindowHandle -ne 0 -and $p.MainWindowTitle -like "*$name*" -and $p.MainWindowTitle -notlike '*loading*') { $p.MainWindowHandle } } 10000
    Start-Sleep -Milliseconds 700
    return @($p, $w)
}
function InRange([double]$v, [double]$lo, [double]$hi) { ($v -ge $lo) -and ($v -le $hi) }

$sandbox = Join-Path $env:TEMP "nv_mouse_$([Diagnostics.Process]::GetCurrentProcess().Id)"
New-Item -ItemType Directory -Force $sandbox | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $sandbox "settings.ini"   # tests never read or write your real settings
$scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$cx = [int]($scr.Width / 2); $cy = [int]($scr.Height / 2)
$script:viewer = $null; $script:win = [IntPtr]::Zero
try {
    # a.png b.png c.png: different sizes so a wrong picture is obvious; b.png is the big one (4000x2000).
    Make-Picture (Join-Path $sandbox 'a.png') 400 300 20
    Make-Picture (Join-Path $sandbox 'b.png') 4000 2000 40
    Make-Picture (Join-Path $sandbox 'c.png') 1200 800 30
    Make-Picture (Join-Path $sandbox 'd.png') 8000 6000 0 ([System.Drawing.Color]::FromArgb(0, 0, 200))   # big and blue: slow enough to catch a stale frame

    Write-Host "`n[1] Opens fullscreen by default"
    $script:viewer, $script:win = Open-Viewer (Join-Path $sandbox 'b.png')
    Check 'viewer opened b.png' ($script:win -ne $null -and $script:win -ne [IntPtr]::Zero) (Title)
    $r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
    Check "window covers the whole screen ($($scr.Width)x$($scr.Height))" (($r.L -eq $scr.X) -and ($r.T -eq $scr.Y) -and (($r.R - $r.L) -eq $scr.Width) -and (($r.B - $r.T) -eq $scr.Height)) "$($r.L),$($r.T),$($r.R),$($r.B)"
    Check 'no title bar / border (WS_CAPTION cleared)' (([In]::GetWindowLong($script:win, -16) -band 0x00C00000) -eq 0)
    $probe = Grab; $bg = $probe.GetPixel(40, $cy); $probe.Dispose()   # (a 4000x2000 picture is letterboxed top and bottom, so sample mid-height)
    Check 'window pixels can be read back (red field)' (($bg.R -gt 150) -and ($bg.G -lt 80)) "pixel $($bg.R),$($bg.G),$($bg.B)"

    Write-Host "`n[2] Wheel = previous / next picture"
    Wheel -120 $cx $cy
    Check 'wheel down goes to the next picture (c.png)' ((Title) -like 'c.png*') (Title)
    Wheel 120 $cx $cy
    Check 'wheel up goes back (b.png)' ((Title) -like 'b.png*') (Title)
    Wheel 120 $cx $cy
    Check 'wheel up again goes to the previous picture (a.png)' ((Title) -like 'a.png*') (Title)
    Wheel 120 $cx $cy
    Check 'wheel up at the first picture stays put' ((Title) -like 'a.png*') (Title)
    Wheel -60 $cx $cy
    Check 'half a notch does not change the picture' ((Title) -like 'a.png*') (Title)
    Wheel -60 $cx $cy
    Check 'two half notches add up to one step (b.png)' ((Title) -like 'b.png*') (Title)

    Write-Host "`n[3] Hold the left button: zoom to 100% (large picture b.png, 4000x2000)"
    $fit = Green-Box $cx $cy 150
    Check "fitted: the 40 px marker is shown at ~64% (about 26 px)  [width $($fit[0])]" (InRange $fit[0] 22 30)
    Btn-Down $cx $cy
    $held = Green-Box $cx $cy 150
    Check "held: marker is 100% (40 px) and stays under the pointer  [box $($held -join ',')]" ((InRange $held[0] 38 42) -and (InRange $held[2] ($cx - 3) ($cx + 3)) -and (InRange $held[3] ($cy - 3) ($cy + 3)))
    Btn-Move ($cx + 60) ($cy + 40)
    $panned = Green-Box ($cx + 60) ($cy + 40) 150
    Check "dragging while held pans the picture with the pointer  [box $($panned -join ',')]" ((InRange $panned[0] 38 42) -and (InRange $panned[2] ($cx + 57) ($cx + 63)) -and (InRange $panned[3] ($cy + 37) ($cy + 43)))
    Btn-Up ($cx + 60) ($cy + 40)
    $back = Green-Box $cx $cy 150
    Check "released: back to the fitted view (about 26 px, centred)  [box $($back -join ',')]" ((InRange $back[0] 22 30) -and (InRange $back[2] ($cx - 3) ($cx + 3)))
    Btn-Down ($cx + 300) ($cy + 100)
    Btn-Down $cx $cy
    Btn-Up $cx $cy
    Check 'a second press while held is ignored and release still restores the view' ((InRange (Green-Box $cx $cy 150)[0] 22 30))

    Write-Host "`n[4] Hold on a picture smaller than half the screen: 200% (a.png, 400x300)"
    Wheel 120 $cx $cy
    $small = Green-Box $cx $cy 100
    Check "a.png fitted shows the 20 px marker at 100% (not enlarged)  [width $($small[0])]" (InRange $small[0] 19 21)
    Btn-Down $cx $cy
    $two = Green-Box $cx $cy 100
    Check "held: marker is 200% (40 px)  [width $($two[0])]" (InRange $two[0] 38 42)
    Btn-Up $cx $cy
    $small2 = Green-Box $cx $cy 100
    Check "released: back to 100% (20 px)  [width $($small2[0])]" (InRange $small2[0] 19 21)
    Wheel -120 $cx $cy

    Write-Host "`n[5] Ctrl + drag: zoom into an area (b.png)"
    Btn-Down ($cx - 200) ($cy - 112) $true
    Btn-Move ($cx + 200) ($cy + 113)
    $rubber = Grab; $edge = $rubber.GetPixel($cx - 200, $cy); $rubber.Dispose()
    Check "a rubber band is drawn while dragging (outline pixel $($edge.R),$($edge.G),$($edge.B))" (($edge.B -gt $edge.R + 20))
    Btn-Up ($cx + 200) ($cy + 113)
    $area = Green-Box $cx $cy 220
    $expected = 25.6 * 6.4
    Check "area 400x225 around the marker fills the screen (marker about $([int]$expected) px, centred)  [box $($area -join ',')]" ((InRange $area[0] ($expected - 12) ($expected + 12)) -and (InRange $area[2] ($cx - 6) ($cx + 6)) -and (InRange $area[3] ($cy - 6) ($cy + 6)))
    Wheel -120 $cx $cy; Wheel 120 $cx $cy
    Check 'changing picture and back resets to the fitted view' (InRange (Green-Box $cx $cy 150)[0] 22 30)
    Btn-Down 100 100 $true; Btn-Up 103 102
    Check 'a tiny Ctrl-click (under 8 px) does not zoom' (InRange (Green-Box $cx $cy 150)[0] 22 30)

    Write-Host "`n[5b] Zoom aims at the picture, panning never shows canvas"
    Wheel 120 $cx $cy; Wheel -120 $cx $cy      # back to a fresh b.png (fitted)
    $sc = Grab; $bar = $sc.GetPixel($cx, 20); $sc.Dispose()
    Check "fitted b.png has dark canvas bars above and below  [pixel $($bar.R),$($bar.G),$($bar.B)]" ($bar.R -lt 60)
    Btn-Down $cx 20                              # pointer on the canvas above the picture
    $sc = Grab; $top = $sc.GetPixel($cx, 3); $bot = $sc.GetPixel($cx, $scr.Height - 4); $sc.Dispose()
    Check 'hold with the pointer on the canvas: zoom goes to the picture edge, no canvas shows' (($top.R -gt 150) -and ($bot.R -gt 150)) "top $($top.R) bottom $($bot.R)"
    foreach ($d in @(@(3000, 3000), @(-6000, -6000), @(6000, 0), @(0, 6000))) {
        Btn-Move ($cx + $d[0]) ($cy + $d[1])
        $sc = Grab; $p = @($sc.GetPixel(3, 3), $sc.GetPixel($scr.Width - 4, 3), $sc.GetPixel(3, $scr.Height - 4), $sc.GetPixel($scr.Width - 4, $scr.Height - 4)); $sc.Dispose()
        Check "dragging far ($($d -join ',')) keeps the picture edges at the window edges" (@($p | Where-Object { $_.R -lt 150 }).Count -eq 0) (($p | ForEach-Object { $_.R }) -join ',')
    }
    Btn-Up $cx $cy
    Check 'released: fitted again' (InRange (Green-Box $cx $cy 150)[0] 22 30)
    function Wheel-Ctrl([int]$delta, [int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x020A, [IntPtr]((($delta -band 0xFFFF) -shl 16) -bor 0x0008), (Pack $x $y)); Start-Sleep -Milliseconds 300 }
    1..3 | ForEach-Object { Wheel-Ctrl 120 ($cx + 100) $cy }
    $zm = Green-Box $cx $cy 150
    $k = [math]::Pow(1.15, 3)
    Check "Ctrl+wheel zooms around the pointer (marker to x=$([int]($cx + 100 - 100 * $k)), width $([int](25.6 * $k)))  [box $($zm -join ',')]" ((InRange $zm[0] (25.6 * $k - 4) (25.6 * $k + 4)) -and (InRange $zm[2] ($cx + 100 - 100 * $k - 4) ($cx + 100 - 100 * $k + 4)))
    1..12 | ForEach-Object { Wheel-Ctrl -120 $cx $cy }
    Check 'Ctrl+wheel out returns to the fitted view and stops there' (InRange (Green-Box $cx $cy 150)[0] 22 30)
    1..3 | ForEach-Object { Wheel-Ctrl 120 $cx 20 }
    $sc = Grab; $top = $sc.GetPixel($cx, 3); $sc.Dispose()
    Check 'Ctrl+wheel with the pointer on the canvas zooms to the picture edge (no canvas)' ($top.R -gt 150) "pixel $($top.R)"
    Btn-Down 700 500; Btn-Up 700 500
    Check 'a click on a zoomed picture cancels the zoom' (InRange (Green-Box $cx $cy 150)[0] 22 30)

    Write-Host "`n[5c] Zoomed picture: drag pans, plain click cancels"
    Btn-Down ($cx - 200) ($cy - 112) $true; Btn-Move ($cx + 200) ($cy + 113); Btn-Up ($cx + 200) ($cy + 113)
    Check 'area zoom is active' (InRange (Green-Box $cx $cy 220)[0] 150 180)
    Btn-Down $cx $cy; Btn-Move ($cx + 50) $cy; Btn-Up ($cx + 50) $cy
    $moved = Green-Box ($cx + 50) $cy 220
    Check "dragging a zoomed picture pans it and keeps the zoom  [box $($moved -join ',')]" ((InRange $moved[0] 150 180) -and (InRange $moved[2] ($cx + 44) ($cx + 56)))
    Btn-Down 700 500; Btn-Up 700 500
    Check 'a plain click cancels the area zoom (back to fit)' (InRange (Green-Box $cx $cy 150)[0] 22 30)
    Btn-Down ($cx - 200) ($cy - 112) $true; Btn-Move ($cx + 100) ($cy + 50)
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero); Start-Sleep -Milliseconds 300   # Esc while dragging
    Btn-Up ($cx + 100) ($cy + 50)
    Check 'Esc cancels a Ctrl+drag selection (nothing zoomed)' (InRange (Green-Box $cx $cy 150)[0] 22 30)

    Write-Host "`n[5d] Info bar on the top edge"
    function Bright-Top { $sc = Grab; $n = 0; for ($y = 0; $y -lt 34; $y += 1) { for ($x = 0; $x -lt $sc.Width; $x += 2) { $c = $sc.GetPixel($x, $y); if ($c.R -gt 200 -and $c.G -gt 200 -and $c.B -gt 200) { $n++ } } }; $sc.Dispose(); $n }
    $before = Bright-Top
    [void][In]::PostMessage($script:win, 0x0200, [IntPtr]0, (Pack $cx 1)); Start-Sleep -Milliseconds 400
    $shown = Bright-Top
    Check "pointer on the top edge shows the info bar (text pixels $before -> $shown)" (($before -lt 5) -and ($shown -gt 80))
    [void][In]::PostMessage($script:win, 0x0200, [IntPtr]0, (Pack $cx 20)); Start-Sleep -Milliseconds 300
    Check 'bar stays while the pointer is over it' ((Bright-Top) -gt 80)
    [void][In]::PostMessage($script:win, 0x0200, [IntPtr]0, (Pack $cx 600)); Start-Sleep -Milliseconds 400
    Check 'moving away hides the bar' ((Bright-Top) -lt 5)

    Write-Host "`n[5e] Alt+Enter toggles fullscreen"
    $altEnter = [IntPtr]0x201C0001
    [void][In]::PostMessage($script:win, 0x0104, [IntPtr]0x0D, $altEnter); Start-Sleep -Milliseconds 700
    $r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
    Check "Alt+Enter leaves fullscreen: a normal window with a title bar  [$($r.R - $r.L)x$($r.B - $r.T)]" ((($r.R - $r.L) -lt $scr.Width) -and (([In]::GetWindowLong($script:win, -16) -band 0x00C00000) -ne 0))
    Check 'the picture is still shown (title unchanged)' ((Title) -like 'b.png*') (Title)
    [void][In]::PostMessage($script:win, 0x0104, [IntPtr]0x0D, $altEnter); Start-Sleep -Milliseconds 700
    [void][In]::GetWindowRect($script:win, [ref]$r)
    Check 'Alt+Enter again returns to fullscreen' ((($r.R - $r.L) -eq $scr.Width) -and (($r.B - $r.T) -eq $scr.Height))

    Write-Host "`n[6] Right click opens the context menu"
    [void][In]::PostMessage($script:win, 0x0205, [IntPtr]0, (Pack $cx $cy))
    $menu = Wait-Until { $m = [In]::FindWindow('#32768', $null); if ($m -ne [IntPtr]::Zero) { $m } } 3000
    Check 'a popup menu appeared' ($null -ne $menu)
    if ($menu) {
        $hm = [In]::SendMessage($menu, 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero)   # MN_GETHMENU
        $n = [In]::GetMenuItemCount($hm)
        $texts = @(0..($n - 1) | ForEach-Object { [In]::MenuText($hm, $_) })
        Check "menu has the expected 31 entries (4 separators)  [count $n]" ($n -eq 31)
        $want = 'Previous picture*', 'Next picture*', 'Fit to screen*', 'Actual size*', 'Gallery*', 'Fullscreen*', 'Slideshow*', 'Info*', 'Rename*', 'Copy to*', 'Move to*', 'Copy picture*', 'Copy file path*', 'Show in Explorer*', 'Delete*', 'Undo*', 'File types*', 'Help*', 'Close*'
        $missing = @($want | Where-Object { $w = $_; -not ($texts | Where-Object { $_ -like $w }) })
        Check "menu lists every action  [missing: $($missing -join ', ')]" ($missing.Count -eq 0)
        # Keyboard through the menu: Down, Down (first enabled = Previous, second = Next), Enter -> next picture.
        foreach ($vk in 0x28, 0x28, 0x0D) { [void][In]::PostMessage($menu, 0x0100, [IntPtr]$vk, [IntPtr]::Zero); Start-Sleep -Milliseconds 150 }
        Start-Sleep -Milliseconds 500
        Check 'choosing "Next picture" in the menu goes to c.png' ((Title) -like 'c.png*') (Title)
        Check 'the menu closed' (([In]::FindWindow('#32768', $null)) -eq [IntPtr]::Zero)
    }
    [void][In]::PostMessage($script:win, 0x0205, [IntPtr]0, (Pack $cx $cy))
    $menu = Wait-Until { $m = [In]::FindWindow('#32768', $null); if ($m -ne [IntPtr]::Zero) { $m } } 3000
    if ($menu) {
        [void][In]::PostMessage($menu, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero); Start-Sleep -Milliseconds 400   # Esc
        $script:viewer.Refresh()
        Check 'Esc dismisses the menu without closing the viewer' ((-not $script:viewer.HasExited) -and (([In]::FindWindow('#32768', $null)) -eq [IntPtr]::Zero))
    } else { Check 'second right click opened the menu again' $false }

    Write-Host "`n[7] Bottom edge shows the taskbar"
    if (-not $RealInput) {
        Skip 'taskbar reveal at the bottom edge' 'needs the real pointer: run with -RealInput while no game/other program holds the focus'
    } else {
        $fg = ([In]::GetForegroundWindow() -eq $script:win)
        $pt = New-Object In+POINT; [void][In]::SetCursorPos($cx, $cy); Start-Sleep -Milliseconds 200; [void][In]::GetCursorPos([ref]$pt)
        if (-not $fg -or [math]::Abs($pt.X - $cx) -gt 2 -or [math]::Abs($pt.Y - $cy) -gt 2) {
            Skip 'taskbar reveal at the bottom edge' "viewer is not in the foreground ($fg) or the pointer can't be moved (clipped by another program): not sending any input"
        } else {
            function Strip-Light { $b = New-Object System.Drawing.Bitmap 6, 6; $g = [System.Drawing.Graphics]::FromImage($b)
                $g.CopyFromScreen(300, $scr.Height - 14, 0, 0, $b.Size); $g.Dispose(); $c = $b.GetPixel(2, 2); $b.Dispose(); [int](($c.R + $c.G + $c.B) / 3) }
            Start-Sleep -Milliseconds 400; $hidden = Strip-Light
            Check "taskbar hidden while the pointer is in the picture (brightness $hidden)" ($hidden -lt 80)
            [void][In]::SetCursorPos(300, $scr.Height - 1); Start-Sleep -Milliseconds 700; $shown = Strip-Light
            Check "pointer on the bottom row shows the taskbar (brightness $shown)" ($shown -gt $hidden + 40)
            [void][In]::SetCursorPos(300, $scr.Height - 300); Start-Sleep -Milliseconds 700; $again = Strip-Light
            Check "moving up hides it again (brightness $again)" ($again -lt 80)
        }
    }

    Write-Host "`n[5f] Gallery: the previous picture must not stay on screen"
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x24, [IntPtr]::Zero); Start-Sleep -Milliseconds 500        # Home: a.png (red)
    $sc = Grab; $old = $sc.GetPixel($cx - 120, $cy - 80); $sc.Dispose()
    Check "a.png shows its red field before opening the gallery  [pixel $($old.R),$($old.G),$($old.B)]" (($old.R -gt 150) -and ($old.G -lt 80))
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x09, [IntPtr]::Zero); Start-Sleep -Milliseconds 700        # Tab: gallery
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x23, [IntPtr]::Zero); Start-Sleep -Milliseconds 300        # End: select d.png (the big blue one)
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x0D, [IntPtr]::Zero)                                      # Enter: open it
    $sw = [Diagnostics.Stopwatch]::StartNew(); $sawOld = $false; $sawNew = $false; $samples = 0
    while ($sw.ElapsedMilliseconds -lt 6000 -and -not $sawNew) {
        $sc = Grab; $p = $sc.GetPixel($cx - 120, $cy - 80); $sc.Dispose(); $samples++
        if (($p.R -gt 150) -and ($p.G -lt 80) -and ($p.B -lt 80)) { $sawOld = $true }
        if (($p.B -gt 150) -and ($p.R -lt 80)) { $sawNew = $true }
    }
    Check "opening d.png from the gallery never shows the previous picture  [$samples samples]" (-not $sawOld)
    Check 'and d.png does appear (blue)' $sawNew
    Check 'title is d.png' ((Title) -like 'd.png*') (Title)

    Write-Host "`n[8] Esc closes the fullscreen viewer straight away (no windowed step)"
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero)
    $gone = Wait-Until { $script:viewer.HasExited } 3000
    Check 'Esc closes the viewer' ($null -ne $gone)
}
finally {
    if ($script:viewer -and -not $script:viewer.HasExited) { $script:viewer.Kill() }   # only the one this script started
    Remove-Item $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host ("`n{0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip) -ForegroundColor $(if ($script:fail) { 'Red' } else { 'Green' })
exit $(if ($script:fail) { 1 } else { 0 })
