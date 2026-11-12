# End-to-end usage test of the installer, the file-type chooser and the middle-click close.
#   tools\test\e2e.ps1                          (uses $env:CARGO_TARGET_DIR\release, else .\target\release)
#   tools\test\e2e.ps1 -Exe X:\nuviewer.exe -Setup X:\nuviewer-setup.exe
#
# Safe to run on your own PC: the installer is pointed at a scratch registry key (HKCU\Software\NuViewerE2E_*),
# scratch Start menu / Desktop folders and a temp install folder, so your real file associations are never touched.
# It drives the real windows (finds controls, clicks them, sends a real middle mouse click) and checks the results.
param(
    [string]$Exe,
    [string]$Setup
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $Exe)   { $Exe   = Join-Path $target 'release\nuviewer.exe' }
if (-not $Setup) { $Setup = Join-Path $target 'release\nuviewer-setup.exe' }
foreach ($f in $Exe, $Setup) { if (-not (Test-Path $f)) { throw "missing $f (build first: tools\package.ps1)" } }

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class Win {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(int x, int y);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public class Ctl { public IntPtr H; public string Cls; public string Text; public int Style; }
    public static List<Ctl> Children(IntPtr parent) {
        var list = new List<Ctl>();
        EnumChildWindows(parent, (h, l) => {
            var c = new StringBuilder(64); GetClassName(h, c, 64);
            var t = new StringBuilder(256); GetWindowText(h, t, 256);
            list.Add(new Ctl { H = h, Cls = c.ToString(), Text = t.ToString(), Style = GetWindowLong(h, -16) });
            return true;
        }, IntPtr.Zero);
        return list;
    }
}
'@

$script:pass = 0; $script:fail = 0
function Check([string]$name, [bool]$ok, [string]$detail = '') {
    if ($ok) { $script:pass++; Write-Host "  PASS  $name" -ForegroundColor Green }
    else { $script:fail++; Write-Host "  FAIL  $name  $detail" -ForegroundColor Red }
}
function Wait-Until([scriptblock]$cond, [int]$ms = 8000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $ms) { $r = & $cond; if ($r) { return $r }; Start-Sleep -Milliseconds 50 }
    return $null
}
function Find-Form([string]$cls, [string]$titlePart) {
    Wait-Until { Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -like "*$titlePart*" } | Select-Object -First 1 } 10000
}
function Click([IntPtr]$h) { [void][Win]::SendMessage($h, 0xF5, [IntPtr]::Zero, [IntPtr]::Zero) }  # BM_CLICK
function Checkboxes([IntPtr]$form) { [Win]::Children($form) | Where-Object { $_.Cls -eq 'Button' -and (($_.Style -band 0xF) -eq 3) } }  # BS_AUTOCHECKBOX
function Is-Checked([IntPtr]$h) { [Win]::SendMessage($h, 0xF0, [IntPtr]::Zero, [IntPtr]::Zero) -eq [IntPtr]1 }  # BM_GETCHECK
function Dismiss-MessageBox([int]$ms = 8000) {
    $mb = Wait-Until { $h = [Win]::FindWindow('#32770', 'NuViewer'); if ($h -ne [IntPtr]::Zero) { $h } } $ms
    if ($mb) { [void][Win]::PostMessage($mb, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero); return $true }  # WM_CLOSE
    return $false
}

$id = [Diagnostics.Process]::GetCurrentProcess().Id
$sandbox = Join-Path $env:TEMP "nv_e2e_$id"
$regPrefix = "Software\NuViewerE2E_$id"
$hk = "HKCU:\$regPrefix"
$installDir = Join-Path $sandbox 'app'
$startDir = Join-Path $sandbox 'start'
$deskDir = Join-Path $sandbox 'desk'
$log = Join-Path $sandbox 'setup.log'
New-Item -ItemType Directory -Force $sandbox | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $sandbox "settings.ini"   # tests never read or write your real settings
$sbArgs = @("/regprefix=$regPrefix", "/startmenudir=$startDir", "/desktopdir=$deskDir", "/log=$log")
# Only ever stop processes this script started (the exe under test, the setup, or the installed scratch copy):
# a NuViewer you have open yourself must survive a test run.
function Stop-Mine {
    Get-Process nuviewer, nuviewer-setup -ErrorAction SilentlyContinue | Where-Object {
        $_.Path -and (($_.Path -ieq $Exe) -or ($_.Path -ieq $Setup) -or $_.Path.StartsWith($sandbox, [StringComparison]::OrdinalIgnoreCase))
    } | Stop-Process -Force
}
# Your real NuViewer registration (if you installed it) must come out of the test exactly as it went in.
function Real-State {
    $k = 'HKCU:\Software\NuViewer\Capabilities\FileAssociations'
    '{0}|{1}|{2}|{3}' -f (Test-Path 'HKCU:\Software\Classes\NuViewer.Image'),
        (Get-Item 'HKCU:\Software\Classes\NuViewer.Image\shell\open\command' -ErrorAction SilentlyContinue).GetValue(''),
        $(if (Test-Path $k) { (Get-Item $k).ValueCount } else { -1 }),
        (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\NuViewer' -ErrorAction SilentlyContinue).DisplayVersion
}
$realBefore = Real-State

try {
    Write-Host "`n[1] Silent install (formats=jpg,png,raf) into a scratch profile"
    $p = Start-Process $Setup -ArgumentList (@('/S', "/D=$installDir", '/formats=jpg,png,raf', '/desktop') + $sbArgs) -Wait -PassThru
    Check 'setup exit code 0' ($p.ExitCode -eq 0) "exit=$($p.ExitCode) log=$(Get-Content $log -ErrorAction SilentlyContinue)"
    Check 'nuviewer.exe installed and identical to the build' ((Test-Path "$installDir\nuviewer.exe") -and ((Get-FileHash "$installDir\nuviewer.exe").Hash -eq (Get-FileHash $Exe).Hash))
    Check 'uninstall.exe installed' (Test-Path "$installDir\uninstall.exe")
    Check 'Start menu + file-types + desktop shortcuts' ((Test-Path "$startDir\NuViewer.lnk") -and (Test-Path "$startDir\NuViewer file types.lnk") -and (Test-Path "$deskDir\NuViewer.lnk"))
    $assoc = (Get-Item "$hk\NuViewer\Capabilities\FileAssociations" -ErrorAction SilentlyContinue).Property | Sort-Object
    Check 'only the ticked formats are registered' (($assoc -join ',') -eq '.jpg,.png,.raf') ($assoc -join ',')
    Check 'Add/Remove Programs entry' ((Get-ItemProperty "$hk\Uninstall\NuViewer" -ErrorAction SilentlyContinue).DisplayName -eq 'NuViewer')
    Check 'your real registration is untouched' ((Real-State) -eq $realBefore) "before: $realBefore  after: $(Real-State)"

    Write-Host "`n[2] Middle click closes the viewer"
    $png = Join-Path $sandbox 'test.png'
    Add-Type -AssemblyName System.Drawing
    $bmp = New-Object System.Drawing.Bitmap 640, 360
    $g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear([System.Drawing.Color]::SteelBlue); $g.Dispose()
    $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    $viewer = Start-Process "$installDir\nuviewer.exe" -ArgumentList @("`"$png`"", '--new-window') -PassThru
    $win = Wait-Until { $viewer.Refresh(); if ($viewer.MainWindowHandle -ne 0 -and $viewer.MainWindowTitle -like '*test.png*') { $viewer.MainWindowHandle } } 10000
    Check 'viewer opened the image' ($null -ne $win) $viewer.MainWindowTitle
    if ($win) {
        Start-Sleep -Milliseconds 500
        # Gallery: a middle click must NOT close it (G enters the gallery).
        [void][Win]::PostMessage($win, 0x0100, [IntPtr]0x47, [IntPtr]::Zero); Start-Sleep -Milliseconds 400
        [void][Win]::PostMessage($win, 0x0208, [IntPtr]::Zero, [IntPtr](100 + (100 -shl 16))); Start-Sleep -Milliseconds 600
        Check 'middle click in the gallery does not close' (-not $viewer.HasExited)
        [void][Win]::PostMessage($win, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero); Start-Sleep -Milliseconds 400   # Esc: back to the viewer
        Check 'Esc went back to the viewer (still running)' (-not $viewer.HasExited)
        # Viewer: a real middle mouse click on the picture area closes it.
        $r = New-Object Win+RECT; [void][Win]::GetWindowRect($win, [ref]$r)
        [void][Win]::SetWindowPos($win, [IntPtr]-1, 0, 0, 0, 0, 3)   # HWND_TOPMOST, so nothing else covers the click point
        [void][Win]::SetForegroundWindow($win)
        $cx = [int](($r.Left + $r.Right) / 2); $cy = [int](($r.Top + $r.Bottom) / 2)
        [void][Win]::SetCursorPos($cx, $cy); Start-Sleep -Milliseconds 300
        $under = [Win]::GetAncestor([Win]::WindowFromPoint($cx, $cy), 2)
        if ($under -eq $win) {
            [Win]::mouse_event(0x0020, 0, 0, 0, [UIntPtr]::Zero); [Win]::mouse_event(0x0040, 0, 0, 0, [UIntPtr]::Zero)   # MIDDLEDOWN, MIDDLEUP
            $closed = Wait-Until { $viewer.HasExited } 3000
            Check 'real middle click in the viewer closes it' ($null -ne $closed -and $viewer.HasExited)
        } else {
            # Another always-on-top window covers the spot (e.g. a screen recorder): fall back to the window message the click produces.
            [void][Win]::PostMessage($win, 0x0208, [IntPtr]::Zero, [IntPtr](100 + (100 -shl 16)))
            $closed = Wait-Until { $viewer.HasExited } 3000
            Check 'middle click (WM_MBUTTONUP, real click was blocked by another window) in the viewer closes it' ($null -ne $closed -and $viewer.HasExited)
        }
    }
    if ($viewer -and -not $viewer.HasExited) { $viewer.Kill() }

    Write-Host "`n[3] File-type chooser (nuviewer --associate): the check boxes"
    $assocProc = Start-Process $Exe -ArgumentList '--associate' -PassThru
    $w = Find-Form 'NuViewerForm' 'file types'
    Check 'chooser window appears' ($null -ne $w)
    if ($w) {
        $h = $w.MainWindowHandle
        $boxes = @(Checkboxes $h)
        Check 'one check box per format (34)' ($boxes.Count -eq 34) "found $($boxes.Count)"
        Check 'RAW and picture formats are listed' ((($boxes | Where-Object { $_.Text -like 'Fujifilm RAW*' }).Count -eq 1) -and (($boxes | Where-Object { $_.Text -like 'JPEG*' }).Count -ge 1))
        $none = [Win]::Children($h) | Where-Object { $_.Text -eq 'Select none' }
        $all = [Win]::Children($h) | Where-Object { $_.Text -eq 'Select all' }
        Click $none.H; Start-Sleep -Milliseconds 100
        Check 'Select none clears every box' (@($boxes | Where-Object { Is-Checked $_.H }).Count -eq 0)
        Click $all.H; Start-Sleep -Milliseconds 100
        Check 'Select all ticks every box' (@($boxes | Where-Object { -not (Is-Checked $_.H) }).Count -eq 0)
        $cancel = [Win]::Children($h) | Where-Object { $_.Text -eq 'Cancel' }
        Click $cancel.H
        $done = Wait-Until { $assocProc.HasExited } 5000
        Check 'Cancel closes the chooser without touching the registry' ($null -ne $done -and ((Real-State) -eq $realBefore))
    }
    if (-not $assocProc.HasExited) { $assocProc.Kill() }

    Write-Host "`n[4] Setup wizard: pick formats with the check boxes, then Install"
    $wiz = Start-Process $Setup -ArgumentList (@("/D=$installDir") + $sbArgs) -PassThru
    $w = Find-Form 'NuViewerForm' 'Setup'
    Check 'wizard appears' ($null -ne $w)
    if ($w) {
        $h = $w.MainWindowHandle
        $boxes = @(Checkboxes $h)
        Check 'wizard has 34 format boxes + 3 option boxes' ($boxes.Count -eq 37) "found $($boxes.Count)"
        $pre = @($boxes | Where-Object { $_.Text -like 'JPEG*' }) | Select-Object -First 1
        Check 'previous selection is pre-ticked (JPEG)' (Is-Checked $pre.H)
        $none = [Win]::Children($h) | Where-Object { $_.Text -eq 'Select none' }
        Click $none.H; Start-Sleep -Milliseconds 100
        foreach ($label in 'WebP*', 'Sony RAW*') { $b = $boxes | Where-Object { $_.Text -like $label }; Click $b.H }
        Click (($boxes | Where-Object { $_.Text -like 'Create a Start menu*' }).H)   # untick the Start menu shortcut
        $install = [Win]::Children($h) | Where-Object { $_.Text -in 'Install', 'Update' }
        Click $install.H
        $shown = Dismiss-MessageBox 10000
        Check 'completion message shown' $shown
        $done = Wait-Until { $wiz.HasExited } 8000
        Check 'wizard finished with exit code 0' ($null -ne $done -and $wiz.ExitCode -eq 0) "exit=$($wiz.ExitCode)"
        $assoc = (Get-Item "$hk\NuViewer\Capabilities\FileAssociations" -ErrorAction SilentlyContinue).Property | Sort-Object
        Check 'registered exactly WebP + Sony RAW (webp, arw, sr2, srf)' (($assoc -join ',') -eq '.arw,.sr2,.srf,.webp') ($assoc -join ',')
        Check 'earlier jpg/png/raf registration was replaced, not accumulated' (-not ($assoc -contains '.jpg'))
        Check 'an update never deletes shortcuts from the earlier install' (Test-Path "$startDir\NuViewer.lnk")
    }
    if ($wiz -and -not $wiz.HasExited) { $wiz.Kill() }

    Write-Host "`n[5] Uninstall"
    $p = Start-Process "$installDir\uninstall.exe" -ArgumentList (@('/uninstall', '/S', "/D=$installDir") + $sbArgs) -Wait -PassThru
    Check 'uninstall exit code 0' ($p.ExitCode -eq 0)
    Check 'program file removed' (-not (Test-Path "$installDir\nuviewer.exe"))
    Check 'shortcuts removed' (-not (Test-Path "$startDir\NuViewer.lnk") -and -not (Test-Path "$startDir\NuViewer file types.lnk") -and -not (Test-Path "$deskDir\NuViewer.lnk"))
    Check 'registry entries removed' (-not (Test-Path "$hk\NuViewer") -and -not (Test-Path "$hk\Uninstall\NuViewer") -and -not (Test-Path "$hk\Classes\NuViewer.Image"))
    $gone = Wait-Until { -not (Test-Path $installDir) } 6000
    Check 'uninstall.exe and the install folder removed after exit' ($null -ne $gone)
}
finally {
    Stop-Mine
    Remove-Item $hk -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host ("`n{0} passed, {1} failed" -f $script:pass, $script:fail) -ForegroundColor $(if ($script:fail) { 'Red' } else { 'Green' })
exit $(if ($script:fail) { 1 } else { 0 })
