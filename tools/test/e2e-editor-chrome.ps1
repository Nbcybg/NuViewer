# NuEditor you can find your way around: the graph dock, the right-click menu, the menu bar (Alt+letter), F11 window mode,
# Ctrl + / Ctrl - zoom and the F1 help page. Messages only (no real mouse / keyboard), scratch settings.
# The editor runs with NUEDITOR_TEST=dock: the real panel layout, plus the state line in the title.
#
#   powershell -File tools\test\e2e-editor-chrome.ps1 [-Exe path\to\nueditor.exe]
param([string]$Exe = 'T:\cargo-target\NuViewer\release\nueditor.exe')
. (Join-Path $PSScriptRoot 'lib.ps1')

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class Ec {
    [StructLayout(LayoutKind.Sequential)] public struct RC { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RC r);
    public delegate bool EP(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EP cb, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    public static IntPtr Of(uint pid, string cls) { IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var c = new StringBuilder(64); GetClassName(h, c, 64); if (c.ToString() == cls) { found = h; return false; } } return true; }, IntPtr.Zero); return found; }
}
'@

$scratch = Join-Path ([IO.Path]::GetTempPath()) ("nueditor-chrome-" + [Diagnostics.Process]::GetCurrentProcess().Id)
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $scratch 'settings.ini'
Set-Content -Path $env:NUVIEWER_SETTINGS -Value "start_fullscreen = 0" -Encoding ASCII
$env:NUEDITOR_BACKUP_DIR = Join-Path $scratch 'backups'
$env:NUEDITOR_TEST = 'dock'
if (-not (Test-Path $Exe)) { Write-Host "no $Exe"; exit 2 }

$pic = Join-Path $scratch 'chrome.png'
Make-Picture $pic 1200 800 300

$ed = Start-Process $Exe -ArgumentList @("`"$pic`"", '--new-window', '--windowed') -PassThru
function Ed-Title { $ed.Refresh(); $ed.MainWindowTitle }
function Client { $r = New-Object Ec+RC; [void][Ec]::GetClientRect($script:win, [ref]$r); $r }
function Scale { [double][In]::GetDpiForWindow($script:win) / 96.0 }
function Press-Key([int]$vk, [int]$mods = 0) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 400 }
function Menu-Open { $m = [Ec]::Of($ed.Id, '#32768'); $m -ne [IntPtr]::Zero }
function Cancel-Menu { [void][In]::PostMessage($script:win, 0x001F, [IntPtr]0, [IntPtr]0); Start-Sleep -Milliseconds 400 }

try {
    $script:win = Wait-Until { $ed.Refresh(); if ($ed.MainWindowHandle -ne 0 -and $ed.MainWindowTitle -like '*chrome.png*' -and $ed.MainWindowTitle -like '*first image*') { $ed.MainWindowHandle } } 15000
    Check 'the editor opens the picture' ($null -ne $script:win) (Ed-Title)
    Start-Sleep -Milliseconds 700
    $d = Scale; $c = Client

    # ---- the graph dock: on from the start, at the top of the panel, under the file name
    $b = Grab
    $panelBg = $b.GetPixel($c.R - 4, 60)
    $differs = 0
    for ($y = [int](90 * $d); $y -lt [int](200 * $d); $y += 3) { for ($x = $c.R - [int](280 * $d); $x -lt $c.R - [int](24 * $d); $x += 3) { $q = $b.GetPixel($x, $y); if ([Math]::Abs($q.R - $panelBg.R) + [Math]::Abs($q.G - $panelBg.G) + [Math]::Abs($q.B - $panelBg.B) -gt 40) { $differs++ } } }
    $b.Dispose()
    Check 'the histogram is drawn in the panel without pressing anything' ($differs -gt 40) "$differs points differ from the panel colour"
    # Y goes on to the parade (a different picture in the same place); three more presses come back to the histogram.
    $b1 = Grab; Press-Key 0x59; $b2 = Grab
    $changed = 0
    for ($y = [int](90 * $d); $y -lt [int](200 * $d); $y += 3) { for ($x = $c.R - [int](280 * $d); $x -lt $c.R - [int](24 * $d); $x += 3) { $q = $b1.GetPixel($x, $y); $w = $b2.GetPixel($x, $y); if ([Math]::Abs($q.R - $w.R) + [Math]::Abs($q.G - $w.G) + [Math]::Abs($q.B - $w.B) -gt 30) { $changed++ } } }
    $b1.Dispose(); $b2.Dispose()
    Check 'Y changes the graph in the dock' ($changed -gt 100) "$changed points changed"
    Press-Key 0x59; Press-Key 0x59; Press-Key 0x59

    # ---- right click on the picture: a popup menu
    $mid = @([int](($c.R - 300.0 * $d) / 2), [int]($c.B / 2))
    [void][In]::PostMessage($script:win, 0x0205, [IntPtr]0, (Pack $mid[0] $mid[1]))
    $opened = Wait-Until { Menu-Open } 3000
    Check 'right-clicking the picture opens a menu' ([bool]$opened)
    Cancel-Menu
    Check 'and it goes away again' (-not (Menu-Open))

    # ---- the menu bar: Alt+V opens the View menu
    [void][In]::PostMessage($script:win, 0x0104, [IntPtr]0x56, [IntPtr]0)    # WM_SYSKEYDOWN 'V'
    $opened = Wait-Until { Menu-Open } 3000
    Check 'Alt+V opens the View menu' ([bool]$opened)
    Cancel-Menu

    # ---- Ctrl + zooms (the title says "fit nn%" until the picture is zoomed), Ctrl+0 goes back to fit
    Check 'it starts fitted' ((Ed-Title) -match 'fit') (Ed-Title)
    Press-Key 0xBB 1
    Check 'Ctrl + zooms in' (-not ((Ed-Title) -match 'fit \d+%')) (Ed-Title)
    Press-Key 0x30 1
    Check 'Ctrl+0 is back to fit' ((Ed-Title) -match 'fit') (Ed-Title)

    # ---- F11: window <-> fullscreen. A fullscreen window is as big as its monitor.
    $r0 = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r0)
    Press-Key 0x7A
    Start-Sleep -Milliseconds 500
    $r1 = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r1)
    $screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    Check 'F11 makes the window fullscreen' (($r1.R - $r1.L) -ge $screen.Width -and ($r1.B - $r1.T) -ge $screen.Height) "$($r1.R - $r1.L) x $($r1.B - $r1.T) vs $($screen.Width) x $($screen.Height)"
    Press-Key 0x7A
    Start-Sleep -Milliseconds 500
    $r2 = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r2)
    Check 'F11 again is a window again' (($r2.R - $r2.L) -lt $screen.Width -or ($r2.B - $r2.T) -lt $screen.Height) "$($r2.R - $r2.L) x $($r2.B - $r2.T)"

    # ---- F1: the help page, with the credits (a big dark box over the middle of the picture)
    $before = Grab; $mx = [int](($c.R - 300.0 * $d) / 2); $my = [int]($c.B / 2)
    Press-Key 0x70
    $after = Grab
    $dark = 0
    for ($i = 0; $i -lt 30; $i++) { $q = $after.GetPixel($mx - 200 + $i * 13, $my + 20); if ($q.R -lt 90 -and $q.G -lt 60 -and $q.B -lt 60) { $dark++ } }
    $before.Dispose(); $after.Dispose()
    Check 'F1 puts the help page over the picture' ($dark -gt 5) "$dark dark samples"
    Press-Key 0x70
    Check 'the editor is still alive after all that' (-not $ed.HasExited)
}
catch { Write-Host "  ERROR  $_" -ForegroundColor Red; $script:fail++ }
finally {
    if ($ed -and -not $ed.HasExited) { $ed.Kill(); [void]$ed.WaitForExit(3000) }
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
Write-Host ""
Write-Host "passed $script:pass, failed $script:fail"
if ($script:fail -gt 0) { exit 1 }
