# NuViewer <-> NuEditor hand-over, end to end (messages only; scratch settings so the window stays windowed):
#   viewer --E--> editor (viewer closes, a pending turn goes along) --Esc--> viewer again on the same picture.
# Needs no other NuViewer / NuEditor running (single instance): the test refuses to run otherwise, so it can never
# hand a scratch picture to a window you are using.
#
#   powershell -File tools\test\e2e-editor-link.ps1 [-Dir T:\cargo-target\NuViewer\release]
param([string]$Dir = 'T:\cargo-target\NuViewer\release')
. (Join-Path $PSScriptRoot 'lib.ps1')

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class Ed2 {
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

$script:Exe = Join-Path $Dir 'nuviewer.exe'
$editorExe = Join-Path $Dir 'nueditor.exe'
foreach ($p in @($script:Exe, $editorExe)) { if (-not (Test-Path $p)) { Write-Host "missing $p"; exit 2 } }
if (Get-Process nuviewer, nueditor -ErrorAction SilentlyContinue) { Write-Host 'a NuViewer or NuEditor is running - close it first (single instance)'; exit 2 }

$scratch = Join-Path ([IO.Path]::GetTempPath()) ("nueditor-link-" + [Diagnostics.Process]::GetCurrentProcess().Id)
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $scratch 'settings.ini'
Set-Content -Path $env:NUVIEWER_SETTINGS -Value "start_fullscreen=false" -Encoding ASCII
$a = Join-Path $scratch 'a.png'; Make-Picture $a 800 500 200

function Kill-Ours { foreach ($n in 'nuviewer', 'nueditor') { Get-Process $n -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$Dir*" } | ForEach-Object { try { $_.Kill() } catch {} } } }
try {
    Write-Host "== viewer -> editor -> viewer =="
    Check 'viewer opens the picture' (Open-Viewer $a @('--windowed'))
    $viewerPid = $script:viewer.Id
    # Turn it first: the pending turn must arrive in the editor (key ] = turn).
    Key 0xDD 500
    $t0 = Get-Date
    Key 0x45 100   # E
    $ed = Wait-Until { Get-Process nueditor -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -like '*a.png*' } | Select-Object -First 1 } 15000
    Check 'E starts NuEditor on the same picture' ($null -ne $ed)
    if ($ed) { Write-Host ("    editor window after {0:N0} ms; title: {1}" -f ((Get-Date) - $t0).TotalMilliseconds, $ed.MainWindowTitle) }
    Check 'the viewer closed itself' ([bool](Wait-Until { -not (Get-Process -Id $viewerPid -ErrorAction SilentlyContinue) } 5000))
    if ($ed) {
        $ed.Refresh()
        Check 'a pending turn arrives as an edit (marker in the title)' ($ed.MainWindowTitle.StartsWith([string][char]0x25CF)) $ed.MainWindowTitle
        # Esc asks what to do with the turn; Discard (button 13) leaves it, and the viewer must come back on the same picture.
        [void][Ed2]::PostMessage($ed.MainWindowHandle, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero)
        $f = Wait-Until { $l = [Ed2]::Windows([uint32]$ed.Id, 'NuViewerForm'); if ($l.Count -gt 0) { $l[0] } } 6000
        Check 'Esc asks what to keep' ($null -ne $f)
        if ($f) { [void][Ed2]::PostMessage($f, 0x0111, [IntPtr]13, [IntPtr]::Zero) }
        $t1 = Get-Date
        $back = Wait-Until { Get-Process nuviewer -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$Dir*" -and $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -like '*a.png*' } | Select-Object -First 1 } 15000
        Check 'the viewer is back on the same picture' ($null -ne $back)
        if ($back) { Write-Host ("    viewer back after {0:N0} ms; title: {1}" -f ((Get-Date) - $t1).TotalMilliseconds, $back.MainWindowTitle) }
        Check 'the editor has closed' ([bool](Wait-Until { -not (Get-Process nueditor -ErrorAction SilentlyContinue) } 5000))
        Check 'the discarded turn left the file alone and no pair file behind' ((Test-Path $a) -and -not (Test-Path "$a.nuedit"))
    }
}
catch { Write-Host "  ERROR  $_" -ForegroundColor Red; $script:fail++ }
finally {
    Kill-Ours
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
Write-Host ""
Write-Host "passed $script:pass, failed $script:fail"
if ($script:fail -gt 0) { exit 1 }
