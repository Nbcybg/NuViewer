# Does the new right-click menu / skin system cost time? Alternates an old and a new nuviewer.exe over the same folder and measures
#   menu     right click (posted) -> the popup menu window is visible                        (ms, median and min of N)
#   sub      Down x8 + Right -> the Zoom submenu is visible                                  (new menu only: the old one is a different list)
#   skin     Ctrl+Alt+K (next skin) -> the window's pixels have changed                      (new only; ~15 ms resolution)
#   ws       working set of the viewer after the menus were used                             (MB)
#   tools\bench\menu-speed.ps1 -Old dist\NuViewer-0.7.2-portable-win64\nuviewer.exe -New T:\cargo-target\NuViewer\release\nuviewer.exe
# Messages only, no real input. Each exe is started once and thrown away first (Windows scans a program it has not seen).
param(
    [Parameter(Mandatory)][string]$Old,
    [Parameter(Mandatory)][string]$New,
    [string]$Folder = '',
    [int]$Runs = 15
)
$ErrorActionPreference = 'Stop'
if (-not $Folder) { $Folder = Join-Path $PSScriptRoot 'img12' }
$env:NUVIEWER_NO_PREWARM = '1'
$env:NUVIEWER_SETTINGS = Join-Path $env:TEMP ('nv-bench-' + [guid]::NewGuid().ToString('N').Substring(0, 6) + '.ini')
Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class Mb {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    public static int PopupCount(uint pid) {
        int n = 0;
        EnumWindows((h, p) => { uint q; GetWindowThreadProcessId(h, out q); var c = new StringBuilder(32); GetClassName(h, c, 32);
            if (q == pid && c.ToString() == "#32768" && IsWindowVisible(h)) n++; return true; }, IntPtr.Zero);
        return n;
    }
}
'@
[void][Mb]::SetProcessDPIAware()
Add-Type -AssemblyName System.Drawing
$first = (Get-ChildItem $Folder -Filter *.jpg | Sort-Object Name | Select-Object -First 1).FullName
function Pack([int]$x, [int]$y) { [IntPtr]([int](($x -band 0xFFFF) -bor (($y -band 0xFFFF) -shl 16))) }
function Median($a) { $s = @($a | Sort-Object); if ($s.Count -eq 0) { return -1 }; $s[[int][math]::Floor($s.Count / 2)] }
function Min($a) { @($a | Sort-Object)[0] }
function Wait-Popups([uint32]$pid2, [int]$n, [int]$ms = 3000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $ms) { if ([Mb]::PopupCount($pid2) -ge $n) { return $sw.Elapsed.TotalMilliseconds }; Start-Sleep -Milliseconds 1 }
    return -1
}
function Esc-All([IntPtr]$h, [uint32]$pid2) { for ($i = 0; $i -lt 4 -and [Mb]::PopupCount($pid2) -gt 0; $i++) { [void][Mb]::PostMessage($h, 0x0100, [IntPtr]0x1B, [IntPtr]0); Start-Sleep -Milliseconds 120 } }
function Sample([IntPtr]$h) {
    # PrintWindow paints the whole window (frame and title bar too): sum a few pixels of the controls bar's panel (left edge).
    $r = New-Object Mb+RECT; [void][Mb]::GetWindowRect($h, [ref]$r)
    $bmp = New-Object System.Drawing.Bitmap ([math]::Max(1, $r.R - $r.L)), ([math]::Max(1, $r.B - $r.T))
    $g = [System.Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc(); [void][Mb]::PrintWindow($h, $dc, 2); $g.ReleaseHdc($dc); $g.Dispose()
    $mid = [int]($bmp.Height / 2); $sum = 0
    foreach ($dy in -120, -60, 0, 60, 120) { $c = $bmp.GetPixel(18, $mid + $dy); $sum += $c.R * 65536 + $c.G * 256 + $c.B }
    $bmp.Dispose(); return $sum
}

function Measure-One([string]$exe, [bool]$isNew) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe; $psi.Arguments = "`"$first`" --windowed --new-window"; $psi.UseShellExecute = $false
    $p = [System.Diagnostics.Process]::Start($psi)
    $res = [ordered]@{ menu = @(); sub = @(); skin = @(); ws = 0 }
    try {
        $sw = [Diagnostics.Stopwatch]::StartNew(); $h = [IntPtr]::Zero
        while ($sw.ElapsedMilliseconds -lt 15000) { $p.Refresh(); if ($p.MainWindowTitle -match 'first image') { $h = $p.MainWindowHandle; break }; Start-Sleep -Milliseconds 20 }
        if ($h -eq [IntPtr]::Zero) { return $res }
        Start-Sleep -Milliseconds 800
        $r = New-Object Mb+RECT; [void][Mb]::GetClientRect($h, [ref]$r)
        $pt = Pack ([int]($r.R / 2)) ([int]($r.B / 2)); $id = [uint32]$p.Id
        for ($i = 0; $i -lt ($Runs + 2); $i++) {
            $t0 = [Diagnostics.Stopwatch]::StartNew()
            [void][Mb]::PostMessage($h, 0x0205, [IntPtr]0, $pt)
            $ms = Wait-Popups $id 1
            if ($i -ge 2 -and $ms -ge 0) { $res.menu += $ms }
            if ($isNew -and $ms -ge 0) {
                for ($k = 0; $k -lt 8; $k++) { [void][Mb]::PostMessage($h, 0x0100, [IntPtr]0x28, [IntPtr]0); Start-Sleep -Milliseconds 25 }
                $t1 = [Diagnostics.Stopwatch]::StartNew()
                [void][Mb]::PostMessage($h, 0x0100, [IntPtr]0x27, [IntPtr]0)
                $ms2 = Wait-Popups $id 2
                if ($i -ge 2 -and $ms2 -ge 0) { $res.sub += $ms2 }
            }
            Esc-All $h $id
            Start-Sleep -Milliseconds 150
        }
        if ($isNew) {
            [void][Mb]::PostMessage($h, 0x8001, [IntPtr](0x56), [IntPtr]0); Start-Sleep -Milliseconds 500   # V: the controls bar, the pixel sampled is its panel
            for ($i = 0; $i -lt 8; $i++) {
                $before = Sample $h
                $t2 = [Diagnostics.Stopwatch]::StartNew()
                [void][Mb]::PostMessage($h, 0x8001, [IntPtr](0x4B -bor (5 -shl 16)), [IntPtr]0)
                $changed = $false
                while ($t2.ElapsedMilliseconds -lt 1500) { if ((Sample $h) -ne $before) { $changed = $true; break } }
                if ($changed -and $i -ge 1) { $res.skin += $t2.Elapsed.TotalMilliseconds }
                Start-Sleep -Milliseconds 250
            }
        }
        $p.Refresh(); $res.ws = [math]::Round($p.WorkingSet64 / 1MB, 1)
    } finally { if (-not $p.HasExited) { $p.Kill() }; $p.WaitForExit(3000) | Out-Null }
    return $res
}

$null = Measure-One $Old $false; $null = Measure-One $New $true   # warm-up: first start of an unseen exe is slow
$o = Measure-One $Old $false; $n = Measure-One $New $true; $o2 = Measure-One $Old $false; $n2 = Measure-One $New $true
$omenu = $o.menu + $o2.menu; $nmenu = $n.menu + $n2.menu
'{0,-4} menu {1,6:N1} ms (min {2:N1}, n={3})   working set {4} MB' -f 'old', (Median $omenu), (Min $omenu), $omenu.Count, $o.ws
'{0,-4} menu {1,6:N1} ms (min {2:N1}, n={3})   working set {4} MB' -f 'new', (Median $nmenu), (Min $nmenu), $nmenu.Count, $n.ws
$ns = $n.sub + $n2.sub; $nk = $n.skin + $n2.skin
'new  submenu (Zoom) {0:N1} ms (min {1:N1}, n={2})' -f (Median $ns), (Min $ns), $ns.Count
'new  skin switch -> repaint {0:N1} ms (min {1:N1}, n={2}; polling = ~15 ms resolution)' -f (Median $nk), (Min $nk), $nk.Count
