# The NuEditor tab of NuViewer's Settings window: opens, shows the save choice, and writes `editor_save` / `editor_ask`.
# Messages only; scratch settings file (NUVIEWER_SETTINGS). Needs no other NuViewer running.
#
#   powershell -File tools\test\e2e-editor-settings.ps1 [-Exe path\to\nuviewer.exe]
param([string]$Exe = 'T:\cargo-target\NuViewer\release\nuviewer.exe')
. (Join-Path $PSScriptRoot 'lib.ps1')

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class Es {
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, IntPtr l);
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

if (Get-Process nuviewer -ErrorAction SilentlyContinue) { Write-Host 'a NuViewer is running - close it first'; exit 2 }
$scratch = Join-Path ([IO.Path]::GetTempPath()) ("nueditor-set-" + [Diagnostics.Process]::GetCurrentProcess().Id)
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
$ini = Join-Path $scratch 'settings.ini'
Set-Content -Path $ini -Value "" -Encoding ASCII
$env:NUVIEWER_SETTINGS = $ini
$p = $null
try {
    $p = Start-Process $Exe -ArgumentList '--settings' -PassThru
    $form = Wait-Until { $l = [Es]::Windows([uint32]$p.Id, 'NuViewerForm'); if ($l.Count -gt 0) { $l[0] } } 10000
    Check 'the Settings window opens' ($null -ne $form)
    # The tab buttons are child windows: find the one called NuEditor and click it with WM_COMMAND (its id is 17).
    $kids = [In]::Children($form)
    $tab = $kids | Where-Object { $_.Text -eq 'NuEditor' } | Select-Object -First 1
    Check 'there is a NuEditor tab' ($null -ne $tab)
    [void][Es]::PostMessage($form, 0x0111, [IntPtr]$tab.Id, $tab.H); Start-Sleep -Milliseconds 500
    $kids = [In]::Children($form)
    $label = $kids | Where-Object { $_.Text -like 'Keep my edits as*' } | Select-Object -First 1
    Check 'the page shows "Keep my edits as"' ($label -and ($label.Style -band 0x10000000)) ''
    $ask = $kids | Where-Object { $_.Id -eq 902 } | Select-Object -First 1
    $save = $kids | Where-Object { $_.Id -eq 901 } | Select-Object -First 1
    Check 'the save choice and the ask box exist' ($ask -and $save)
    # Factory: the first entry (pair file) is selected, and "ask" is ticked.
    $sel = [Es]::SendMessage($save.H, 0x147, [IntPtr]::Zero, [IntPtr]::Zero)   # CB_GETCURSEL
    Check 'factory choice is the pair file (first entry)' ($sel.ToInt32() -eq 0) "$sel"
    $checked = [Es]::SendMessage($ask.H, 0x00F0, [IntPtr]::Zero, [IntPtr]::Zero) # BM_GETCHECK
    Check 'factory: ask when leaving is on' ($checked.ToInt32() -eq 1) "$checked"
    # Pick "New file" and untick ask, then OK.
    [void][Es]::SendMessage($save.H, 0x14E, [IntPtr]1, [IntPtr]::Zero)          # CB_SETCURSEL
    [void][Es]::SendMessage($ask.H, 0x00F1, [IntPtr]0, [IntPtr]::Zero)          # BM_SETCHECK off
    [void][Es]::PostMessage($form, 0x0111, [IntPtr]1, [IntPtr]::Zero)
    Check 'OK closes the window' ([bool](Wait-Until { $p.Refresh(); $p.HasExited } 8000))
    $text = Get-Content $ini -Raw
    Check 'settings file has editor_save = new' ($text -match 'editor_save\s*=\s*new') $text
    Check 'settings file has editor_ask = false' ($text -match 'editor_ask\s*=\s*(false|0|no|off)') $text
}
catch { Write-Host "  ERROR  $_" -ForegroundColor Red; $script:fail++ }
finally {
    if ($p -and -not $p.HasExited) { $p.Kill() }
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
Write-Host ""
Write-Host "passed $script:pass, failed $script:fail"
if ($script:fail -gt 0) { exit 1 }
