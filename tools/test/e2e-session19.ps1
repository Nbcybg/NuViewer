# Session 19 features, end to end, with window messages only (no real mouse / keyboard): borderless window (viewer, editor), the colour
# picker and its clipboard value, the rename box's text editing (select all, copy, paste), hue curves in the editor (a pair file with a
# huecurve line changes the picture) and the editor's colour picker.
#
#   powershell -File tools\test\e2e-session19.ps1 [-Exe nuviewer.exe] [-Editor nueditor.exe]
param(
    [string]$Exe = 'R:\cargo-target\NuViewer\release\nuviewer.exe',
    [string]$Editor = 'R:\cargo-target\NuViewer\release\nueditor.exe'
)
. (Join-Path $PSScriptRoot 'lib.ps1')
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class S19 {
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct RC { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RC r);
}
'@
$scratch = Join-Path ([IO.Path]::GetTempPath()) ("nv-s19-" + [Diagnostics.Process]::GetCurrentProcess().Id)
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $scratch 'settings.ini'
Set-Content -Path $env:NUVIEWER_SETTINGS -Value "" -Encoding ASCII
$env:NUVIEWER_NO_PREWARM = '1'
$env:NUEDITOR_BACKUP_DIR = Join-Path $scratch 'backups'
$env:NUEDITOR_TEST = '1'
function Clip { try { [System.Windows.Forms.Clipboard]::GetText() } catch { '' } }
function Set-Clip([string]$t) { [System.Windows.Forms.Clipboard]::SetText($t) }
function Is-Borderless([IntPtr]$h) { ([S19]::GetWindowLong($h, -16) -band 0xC00000) -eq 0 }
function Inject([IntPtr]$h, [int]$vk, [int]$mods = 0, [int]$wait = 400) { [void][In]::PostMessage($h, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
function Client-Of([IntPtr]$h) { $r = New-Object S19+RC; [void][S19]::GetClientRect($h, [ref]$r); $r }

try {
    Write-Host "== session 19 e2e =="
    if (-not (Test-Path $Exe)) { Write-Host "no $Exe"; exit 2 }
    # A picture of one colour, so the picked value is known: (200, 50, 100) = #C83264.
    $pic = Join-Path $scratch 'solid.png'
    Make-Picture $pic 400 300 0 ([System.Drawing.Color]::FromArgb(200, 50, 100))

    # ---- viewer ----
    Check 'viewer opens' (Open-Viewer $pic @('--windowed', '--new-window'))
    Check 'starts with a frame' (-not (Is-Borderless $script:win))
    Inject $script:win 0x7A 2          # Shift+F11
    Check 'Shift+F11: no title bar and frame' (Is-Borderless $script:win)
    Inject $script:win 0x7A 2
    Check 'Shift+F11 again: the frame is back' (-not (Is-Borderless $script:win))

    # The colour picker: K, then a click on the picture copies its value.
    Set-Clip 'untouched'
    Inject $script:win 0x4B 0 1200     # K: the picture is read on a worker thread
    $c = Client-Of $script:win
    Btn-Down ([int]($c.R / 2)) ([int]($c.B / 2)); Btn-Up ([int]($c.R / 2)) ([int]($c.B / 2))
    Check 'K + click copies #C83264' ((Clip) -eq '#C83264') (Clip)
    Inject $script:win 0x4B 0
    Set-Clip 'untouched'
    Btn-Down ([int]($c.R / 2)) ([int]($c.B / 2)); Btn-Up ([int]($c.R / 2)) ([int]($c.B / 2))
    Check 'after K again a click copies nothing' ((Clip) -eq 'untouched') (Clip)

    # The palette: Ctrl+K shows a bar; clicking its first swatch copies the dominant colour.
    Inject $script:win 0x4B 1 1500
    $win = $script:win
    $hit = $false
    for ($y = $c.B - 40; $y -gt $c.B - 160 -and -not $hit; $y -= 10) {
        Set-Clip 'untouched'
        Btn-Down ([int]($c.R / 2)) $y; Btn-Up ([int]($c.R / 2)) $y
        if ((Clip) -eq '#C83264') { $hit = $true }
    }
    Check 'Ctrl+K palette: a swatch click copies the dominant colour' $hit (Clip)
    Inject $script:win 0x1B 0           # Esc stops it (the viewer must still be there)
    Start-Sleep -Milliseconds 300
    $script:viewer.Refresh()
    Check 'Esc stops the palette without closing the viewer' (-not $script:viewer.HasExited)

    # The rename box is a text box now: Ctrl+A, Ctrl+C, typing replaces, Ctrl+V pastes.
    Inject $script:win 0x71 0          # F2
    Inject $script:win 0x41 1          # Ctrl+A
    Set-Clip 'untouched'
    Inject $script:win 0x43 1          # Ctrl+C
    Check 'rename: Ctrl+A then Ctrl+C copies the whole name' ((Clip) -eq 'solid.png') (Clip)
    Set-Clip 'abc'
    Inject $script:win 0x56 1          # Ctrl+V replaces the selection
    Inject $script:win 0x41 1
    Inject $script:win 0x43 1
    Check 'rename: Ctrl+V pasted over the selection' ((Clip) -eq 'abc') (Clip)
    Inject $script:win 0x1B 0
    Close-Viewer

    # ---- editor ----
    if (-not (Test-Path $Editor)) { Skip 'editor tests' "no $Editor" }
    else {
        $a = Join-Path $scratch 'a.png'
        Make-Picture $a 1200 800 300
        # A pair file that turns every hue by +45 degrees: the red background must come out different (more yellow).
        Set-Content -Path ($a + '.nuedit') -Value "nuedit version=`"1`"`nhuecurve kind=`"hue`" points=`"0,0.5`"`n" -Encoding ASCII
        $p = Start-Process $Editor -ArgumentList @("`"$a`"", '--new-window', '--windowed') -PassThru
        $w = Wait-Until { $p.Refresh(); if ($p.MainWindowHandle -ne 0 -and $p.MainWindowTitle -like '*first image*') { $p.MainWindowHandle } } 15000
        Check 'editor opens with the pair file' ($null -ne $w)
        Start-Sleep -Milliseconds 800
        $script:win = $w; $script:viewer = $p
        $cl = Client-Of $w
        $d = [double][In]::GetDpiForWindow($w) / 96.0
        $cx = [int](($cl.R - 300.0 * $d) / 2) - 450; $cy = [int]($cl.B / 2) - 300
        $b = Grab; $r = New-Object In+RECT; [void][In]::GetWindowRect($w, [ref]$r)
        $off = [Math]::Max(0, [int](($r.R - $r.L) - $cl.R) / 2); $top = ($r.B - $r.T) - $cl.B - $off
        $bg = $b.GetPixel($cx + $off, $cy + $top); $b.Dispose()
        # Red (200,30,30) turned by +45 degrees of OKLCH hue is orange / yellow-ish: green is no longer ~30.
        Check 'hue curve: the red background was turned (green channel rose)' ($bg.G -gt 70 -and $bg.R -gt 120) "$bg"
        # Editor colour picker: K, click the middle (the green square of the original, turned too), the clipboard holds a hex value.
        Set-Clip 'untouched'
        Inject $w 0x4B 0 1000
        $mx = [int](($cl.R - 300.0 * $d) / 2); $my = [int]($cl.B / 2)
        Btn-Down $mx $my; Btn-Up $mx $my
        Check 'editor: K + click copies a #RRGGBB value' ((Clip) -match '^#[0-9A-F]{6}$') (Clip)
        Inject $w 0x7A 2
        Check 'editor: Shift+F11 borderless' (Is-Borderless $w)
        Inject $w 0x7A 2
        if (-not $p.HasExited) { $p.Kill() }
    }
} finally {
    Close-Viewer
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
Write-Host ""
Write-Host "passed $($script:pass), failed $($script:fail), skipped $($script:skip)"
exit $(if ($script:fail -gt 0) { 1 } else { 0 })
