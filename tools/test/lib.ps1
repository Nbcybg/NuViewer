# Shared helpers for the e2e scripts: window messages in, window pixels out. Nothing here uses the real mouse or
# keyboard, so it keeps working while another program (a game, say) has the focus.
# Dot-source it:  . (Join-Path $PSScriptRoot 'lib.ps1')

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class In {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, string l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, StringBuilder l);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
    [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
    [DllImport("user32.dll")] public static extern IntPtr GetSubMenu(IntPtr m, int pos);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetMenuString(IntPtr m, uint pos, StringBuilder s, int n, uint flags);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern int GetDlgCtrlID(IntPtr h);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    public class Ctl { public IntPtr H; public string Cls; public string Text; public int Style; public int Id; }
    public static string MenuText(IntPtr m, int i) { var s = new StringBuilder(160); GetMenuString(m, (uint)i, s, 160, 0x400); return s.ToString(); }
    public static List<Ctl> Children(IntPtr parent) {
        var list = new List<Ctl>();
        EnumChildWindows(parent, (h, l) => {
            var c = new StringBuilder(64); GetClassName(h, c, 64);
            var t = new StringBuilder(512); GetWindowText(h, t, 512);
            list.Add(new Ctl { H = h, Cls = c.ToString(), Text = t.ToString(), Style = GetWindowLong(h, -16), Id = GetDlgCtrlID(h) });
            return true;
        }, IntPtr.Zero);
        return list;
    }
    public static string ListText(IntPtr list, int i) { var s = new StringBuilder(512); SendMessage(list, 0x0189, (IntPtr)i, s); return s.ToString(); }
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
function InRange([double]$v, [double]$lo, [double]$hi) { ($v -ge $lo) -and ($v -le $hi) }
function Pack([int]$x, [int]$y) { [IntPtr]([int](($x -band 0xFFFF) -bor (($y -band 0xFFFF) -shl 16))) }

# --- one viewer at a time ---
$script:viewer = $null; $script:win = [IntPtr]::Zero
function Title { $script:viewer.Refresh(); $script:viewer.MainWindowTitle }
function Open-Viewer([string]$file, [string[]]$extra = @()) {
    Close-Viewer
    $args2 = @("`"$file`"", '--new-window') + $extra
    $p = Start-Process $script:Exe -ArgumentList $args2 -PassThru
    $name = [IO.Path]::GetFileName($file)
    $w = Wait-Until { $p.Refresh(); if ($p.MainWindowHandle -ne 0 -and $p.MainWindowTitle -like "*$name*" -and $p.MainWindowTitle -notlike '*loading*') { $p.MainWindowHandle } } 12000
    Start-Sleep -Milliseconds 700
    $script:viewer = $p; $script:win = $w
    return ($null -ne $w)
}
function Close-Viewer {
    if ($script:viewer -and -not $script:viewer.HasExited) { $script:viewer.Kill(); [void]$script:viewer.WaitForExit(3000) }
    $script:viewer = $null; $script:win = [IntPtr]::Zero
}
function Key([int]$vk, [int]$wait = 350) { [void][In]::PostMessage($script:win, 0x0100, [IntPtr]$vk, [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
function Wheel([int]$delta, [int]$x, [int]$y, [bool]$ctrl = $false) {
    $lo = $(if ($ctrl) { 0x0008 } else { 0 })
    [void][In]::PostMessage($script:win, 0x020A, [IntPtr]((($delta -band 0xFFFF) -shl 16) -bor $lo), (Pack $x $y)); Start-Sleep -Milliseconds 350
}
function Btn-Down([int]$x, [int]$y, [bool]$ctrl = $false) { [void][In]::PostMessage($script:win, 0x0201, [IntPtr]$(if ($ctrl) { 9 } else { 1 }), (Pack $x $y)); Start-Sleep -Milliseconds 250 }
function Btn-Move([int]$x, [int]$y, [bool]$down = $true) { [void][In]::PostMessage($script:win, 0x0200, [IntPtr]$(if ($down) { 1 } else { 0 }), (Pack $x $y)); Start-Sleep -Milliseconds 250 }
function Btn-Up([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0202, [IntPtr]0, (Pack $x $y)); Start-Sleep -Milliseconds 300 }
function Click([int]$x, [int]$y) { Btn-Down $x $y; Btn-Up $x $y }

# --- window pixels ---
function Grab {
    $r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
    $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
    $g = [System.Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc()
    [void][In]::PrintWindow($script:win, $dc, 2); $g.ReleaseHdc($dc); $g.Dispose()
    return $bmp
}
function Pixel([int]$x, [int]$y) { $b = Grab; $c = $b.GetPixel($x, $y); $b.Dispose(); $c }
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
    if ($sq -gt 0) { $g.FillRectangle((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(0, 255, 0))), [int](($w - $sq) / 2), [int](($h - $sq) / 2), $sq, $sq) }
    $g.Dispose(); $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
# A settings file for one scenario; the viewer reads it through NUVIEWER_SETTINGS (never touches your real settings).
function Use-Settings([string]$path, [string]$text) {
    Set-Content -Path $path -Value $text -Encoding ASCII
    $env:NUVIEWER_SETTINGS = $path
}
