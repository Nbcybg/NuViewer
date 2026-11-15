# NuEditor's mouse tools, end to end: crop frame (apply, undo, cancel), straighten line, white balance picker, curve editor and
# hue wheel. Messages only (no real mouse / keyboard), scratch files and settings.
#
#   powershell -File tools\test\e2e-editor-tools.ps1 [-Exe path\to\nueditor.exe]
param([string]$Exe = 'T:\cargo-target\NuViewer\release\nueditor.exe')
. (Join-Path $PSScriptRoot 'lib.ps1')

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class Et {
    [StructLayout(LayoutKind.Sequential)] public struct RC { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RC r);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EP cb, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    delegate bool EP(IntPtr h, IntPtr l);
    public static List<IntPtr> Windows(uint pid, string cls) {
        var r = new List<IntPtr>();
        EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var s = new StringBuilder(64); GetClassName(h, s, 64); if (s.ToString() == cls) r.Add(h); } return true; }, IntPtr.Zero);
        return r;
    }
}
'@

$scratch = Join-Path ([IO.Path]::GetTempPath()) ("nueditor-tools-" + [Diagnostics.Process]::GetCurrentProcess().Id)
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $scratch 'settings.ini'
Set-Content -Path $env:NUVIEWER_SETTINGS -Value "" -Encoding ASCII
$env:NUEDITOR_BACKUP_DIR = Join-Path $scratch 'backups'
$env:NUEDITOR_TEST = '1'   # the older panel layout (no graph dock) these scripts' coordinates assume
if (-not (Test-Path $Exe)) { Write-Host "no $Exe"; exit 2 }

$script:ed = $null
function Open-Editor([string]$file) {
    $name = [IO.Path]::GetFileName($file)
    $p = Start-Process $Exe -ArgumentList @("`"$file`"", '--new-window', '--windowed') -PassThru
    $w = Wait-Until { $p.Refresh(); if ($p.MainWindowHandle -ne 0 -and $p.MainWindowTitle -like "*$name*" -and $p.MainWindowTitle -like '*first image*') { $p.MainWindowHandle } } 15000
    Start-Sleep -Milliseconds 600
    $script:ed = $p; $script:win = $w
    return ($null -ne $w)
}
function Ed-Title { $script:ed.Refresh(); $script:ed.MainWindowTitle }
function Client { $r = New-Object Et+RC; [void][Et]::GetClientRect($script:win, [ref]$r); $r }
function Scale { [double][In]::GetDpiForWindow($script:win) / 96.0 }
function Centre { $d = Scale; $c = Client; @([int](($c.R - 300.0 * $d) / 2), [int]($c.B / 2)) }
function Win-Pixel([int]$cx, [int]$cy) {
    $b = Grab; $r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
    $off = [Math]::Max(0, [int](($r.R - $r.L) - (Client).R) / 2); $top = ($r.B - $r.T) - (Client).B - $off
    $c = $b.GetPixel($cx + $off, $cy + $top); $b.Dispose(); $c
}
function Press-Key([int]$vk, [int]$mods = 0) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 350 }
function Stop-Editor { if ($script:ed -and -not $script:ed.HasExited) { $script:ed.Kill(); [void]$script:ed.WaitForExit(3000) }; $script:ed = $null }
function Panel-Left { (Client).R - 300.0 * (Scale) }
function Leave-Pair {
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero)
    $f = Wait-Until { $l = [Et]::Windows([uint32]$script:ed.Id, 'NuViewerForm'); if ($l.Count -gt 0) { $l[0] } } 5000
    if (-not $f) { return $false }
    [void][Et]::PostMessage($f, 0x0111, [IntPtr]12, [IntPtr]::Zero)
    return [bool](Wait-Until { $script:ed.Refresh(); $script:ed.HasExited } 8000)
}
# The fitted picture's top-left corner and size in client pixels (the 1200 x 800 test picture).
function Pic-Rect { $c = Centre; $cl = Client; $d = Scale; $aw = $cl.R - 300.0 * $d
    $s = [Math]::Min([Math]::Min($aw / 1200.0, $cl.B / 800.0), 1.0); $w = 1200 * $s; $h = 800 * $s
    @(($c[0] - $w / 2), ($c[1] - $h / 2), $w, $h) }

try {
    Write-Host "== NuEditor tools e2e =="
    $a = Join-Path $scratch 'a.png'; Make-Picture $a 1200 800 300
    Check 'opens' (Open-Editor $a)
    $c = Centre; $d = Scale
    $bgPoint = @(($c[0] - 450), ($c[1] - 300))
    $bg0 = Win-Pixel $bgPoint[0] $bgPoint[1]
    $mid0 = Win-Pixel $c[0] $c[1]
    Check 'start: green at the centre, red background' ($mid0.G -gt 200 -and $bg0.R -gt 150 -and $bg0.G -lt 70) "$mid0 $bg0"

    # ---- crop: apply and undo ----
    $pr = Pic-Rect
    Press-Key 0x52   # R
    Btn-Down ([int]($pr[0] + 3)) ([int]($pr[1] + 3))
    Btn-Move ($c[0] - 100) ($c[1] - 50) $true
    Btn-Up ($c[0] - 100) ($c[1] - 50)
    Press-Key 0x0D   # Enter
    $m = Win-Pixel $c[0] $c[1]
    Check 'crop applied: the picture centre moved, so the centre is no longer the green square' ($m.R -gt 150 -and $m.G -lt 80) "$m"
    Press-Key 0x5A 1   # Ctrl+Z
    $m = Win-Pixel $c[0] $c[1]
    Check 'Ctrl+Z takes the crop back' ($m.G -gt 200 -and $m.R -lt 80) "$m"

    # ---- crop: Esc cancels ----
    Press-Key 0x52
    Btn-Down ([int]($pr[0] + 3)) ([int]($pr[1] + 3))
    Btn-Move ($c[0] - 100) ($c[1] - 50) $true
    Btn-Up ($c[0] - 100) ($c[1] - 50)
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero); Start-Sleep -Milliseconds 400
    $m = Win-Pixel $c[0] $c[1]
    Check 'Esc cancels the crop and the editor stays open' ((-not $script:ed.HasExited) -and $m.G -gt 200) "$m"
    Check 'nothing was changed (no edited marker)' (-not (Ed-Title).StartsWith([string][char]0x25CF)) (Ed-Title)

    # ---- straighten line ----
    Press-Key 0x52
    Btn-Down ($c[0] - 200) ($c[1] + 250) $true
    Btn-Move ($c[0] + 200) ($c[1] + 286) $true
    Btn-Up ($c[0] + 200) ($c[1] + 286)
    Press-Key 0x0D
    Check 'the level line marks the picture as edited' ((Ed-Title).StartsWith([string][char]0x25CF)) (Ed-Title)
    Check 'leaving keeps it as a pair file' (Leave-Pair)
    $text = Get-Content "$a.nuedit" -Raw
    Check 'the pair file holds a turn of about -5 degrees' ($text -match 'angle="-5\.\d') $text

    # ---- white balance picker ----
    $b = Join-Path $scratch 'b.png'; Make-Picture $b 1200 800 300
    Check 'opens b' (Open-Editor $b)
    $c = Centre; $bgPoint = @(($c[0] - 450), ($c[1] - 300))
    Press-Key 0x57   # W
    Click $bgPoint[0] $bgPoint[1]
    Start-Sleep -Milliseconds 400
    $n = Win-Pixel $bgPoint[0] $bgPoint[1]
    $spread = ([Math]::Max([Math]::Max($n.R, $n.G), $n.B)) - ([Math]::Min([Math]::Min($n.R, $n.G), $n.B))
    Check 'the picked red background becomes neutral' ($spread -lt 30) "$n spread $spread"

    # ---- curve editor ----
    $pl = Panel-Left
    Click ([int]($pl + 60 * $d)) ([int](100 * $d))       # collapse Light (it starts open)
    Click ([int]($pl + 60 * $d)) ([int](281 * $d))       # the Curves header
    $w = 300 * $d - 28 * $d; $areaL = $pl + 14 * $d; $areaT = (296 + 36) * $d
    $before = Win-Pixel $bgPoint[0] $bgPoint[1]
    Click ([int]($areaL + 0.5 * $w)) ([int]($areaT + 0.25 * 0.8 * $w))
    Start-Sleep -Milliseconds 400
    $after = Win-Pixel $bgPoint[0] $bgPoint[1]
    $moved = [Math]::Abs($after.R - $before.R) + [Math]::Abs($after.G - $before.G) + [Math]::Abs($after.B - $before.B)
    Check 'a point pulled up on the curve brightens the picture' ($moved -gt 12) "$before -> $after"
    # Right-click on that point removes it.
    [void][In]::PostMessage($script:win, 0x0205, [IntPtr]0, (Pack ([int]($areaL + 0.5 * $w)) ([int]($areaT + 0.25 * 0.8 * $w)))); Start-Sleep -Milliseconds 400
    $gone = Win-Pixel $bgPoint[0] $bgPoint[1]
    Check 'right-click removes the point' (([Math]::Abs($gone.G - $before.G) + [Math]::Abs($gone.R - $before.R)) -lt 6) "$before -> $gone"

    # ---- hue wheel ----
    Press-Key 0x30   # 0 = reset all: the picker made the background neutral, so start again from red
    Click ([int]($pl + 60 * $d)) ([int](281 * $d))       # collapse Curves
    Click ([int]($pl + 60 * $d)) ([int](311 * $d))       # the Hue wheel header
    $top = (326 + 26) * $d; $side = 170 * $d
    $cx = $pl + 150 * $d
    Click ([int]$cx) ([int]($top + 0.1 * $side))         # the ring at the top: the red band
    $rowTop = $top + $side + 8 * $d
    Click ([int]($pl + 14 * $d)) ([int]($rowTop + 38 * $d + 26 * $d))   # saturation slider, far left = -100
    Start-Sleep -Milliseconds 400
    $h = Win-Pixel $bgPoint[0] $bgPoint[1]
    Check 'red band at -100 saturation drains the red background' (([Math]::Abs($h.R - $h.G) -lt 40) -and $h.G -gt 100) "$h"
}
catch { Write-Host "  ERROR  $_" -ForegroundColor Red; $script:fail++ }
finally {
    Stop-Editor
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
Write-Host ""
Write-Host "passed $script:pass, failed $script:fail"
if ($script:fail -gt 0) { exit 1 }
