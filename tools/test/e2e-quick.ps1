# NuQuick end-to-end test. Window messages only (no real keys or mouse), so it is safe while a game has the focus.
# Starts its own nuquick.exe with NUQUICK_TEST=1 (a log file instead of silence), opens one Explorer window on a temp
# folder, and drives the agent with its test messages. The real Space key hook is NOT exercised here (needs real input).
param([string]$Exe = 'T:\cargo-target\NuViewer\release\nuquick.exe', [string]$Src = 'P:\Project_Code\NuViewer\tools\bench\img12')
. (Join-Path $PSScriptRoot 'lib.ps1')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class Qk {
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
}
'@
function Vis($h) { ($h -ne $null) -and ($h -ne [IntPtr]::Zero) -and [Qk]::IsWindowVisible([IntPtr]$h) }
$WM_SPACE = 0x8000 + 10; $WM_CLOSE_REQ = 0x8000 + 2; $WM_QUIT = 0x8000 + 11
$log = Join-Path $env:TEMP 'nuquick-test.log'
Remove-Item $log -ErrorAction SilentlyContinue
Get-Process nuquick -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Exe } | Stop-Process -Force

# a temp folder with pictures and one non-picture
$dir = Join-Path $env:TEMP 'nuquick-e2e'
Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $dir | Out-Null
$imgs = @(Get-ChildItem $Src -Filter *.jpg | Select-Object -First 3)
$i = 0; foreach ($f in $imgs) { Copy-Item $f.FullName (Join-Path $dir ("pic{0}.jpg" -f $i)); $i++ }
Set-Content (Join-Path $dir 'note.txt') 'hello'
New-Item -ItemType Directory (Join-Path $dir 'sub') | Out-Null
Set-Content (Join-Path $dir 'sub\inner.txt') 'x'
$pic0 = Join-Path $dir 'pic0.jpg'; $pic1 = Join-Path $dir 'pic1.jpg'

# warm the exe once (first start of a new exe is slow: Defender)
$w = Start-Process $Exe -ArgumentList '--quit' -PassThru; $w.WaitForExit()

$env:NUQUICK_TEST = '1'
$posfile = Join-Path $env:TEMP 'nuquick-e2e-pos.txt'; Remove-Item $posfile -ErrorAction SilentlyContinue
$env:NUQUICK_POS_FILE = $posfile   # never touch the user's remembered position
$env:NUQUICK_TEST_DIR = $dir   # the agent looks only in this folder, never at what the user has selected
$agent = Start-Process $Exe -PassThru
$h = Wait-Until { $x = [Qk]::FindWindow('NuQuickAgent', [NullString]::Value); if ($x -ne [IntPtr]::Zero) { $x } } 5000
Check 'agent window appears' ($null -ne $h)
Start-Sleep -Milliseconds 800
$agent.Refresh()
Write-Host ("  agent idle: WS {0:N1} MB  private {1:N1} MB  threads {2}" -f ($agent.WorkingSet64/1MB), ($agent.PrivateMemorySize64/1MB), $agent.Threads.Count)
Check 'agent private memory < 8 MB' ($agent.PrivateMemorySize64 -lt 8MB) ("{0:N1} MB" -f ($agent.PrivateMemorySize64/1MB))
$c0 = $agent.TotalProcessorTime.TotalMilliseconds; Start-Sleep -Seconds 5; $agent.Refresh()
Check 'agent idle CPU < 50 ms per 5 s' (($agent.TotalProcessorTime.TotalMilliseconds - $c0) -lt 50) ("{0:N1} ms" -f ($agent.TotalProcessorTime.TotalMilliseconds - $c0))

# one Explorer window with pic0 selected
$shell = New-Object -ComObject Shell.Application
$before = @($shell.Windows()).Count
Start-Process explorer.exe -ArgumentList "/select,`"$pic0`""
$ex = Wait-Until { @($shell.Windows()).Count -gt $before } 8000
Check 'explorer window with a selection opens' ($null -ne $ex)
Start-Sleep -Milliseconds 1200

# Space on a picture: shown, no focus taken
[In]::PostMessage($h, $WM_SPACE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
$view = Wait-Until { $v = [Qk]::FindWindow('NuQuickView', [NullString]::Value); if ($v -ne [IntPtr]::Zero -and [Qk]::IsWindowVisible($v)) { $v } } 4000
Check 'Space shows the preview window' ($null -ne $view)
if ($view) {
    $ex = [Qk]::GetWindowLong($view, -20)
    Check 'preview window is NOACTIVATE + TOPMOST' ((($ex -band 0x08000000) -ne 0) -and (($ex -band 0x8) -ne 0)) ("0x{0:X}" -f $ex)
    $r = New-Object Qk+R; [void][Qk]::GetWindowRect($view, [ref]$r)
    Check 'preview window has a sane size' ((InRange ($r.Rt - $r.L) 300 4000) -and (InRange ($r.B - $r.T) 200 4000)) ("{0}x{1}" -f ($r.Rt - $r.L), ($r.B - $r.T))
}
Start-Sleep -Milliseconds 300
$line = (Get-Content $log -ErrorAction SilentlyContinue | Where-Object { $_ -match 'space_to_shown_ms' } | Select-Object -Last 1)
Write-Host "  first Space (cold, no prefetch): $line"
Check 'log has a space_to_shown line' ($null -ne $line)

# Drag the window (press, move, release as posted messages): it follows, and remembers where it was left.
function Rect($v) { $r = New-Object Qk+R; [void][Qk]::GetWindowRect($v, [ref]$r); $r }
function MkLp([int]$x, [int]$y) { [IntPtr](($y -shl 16) -bor ($x -band 0xFFFF)) }
$r0 = Rect $view
[In]::PostMessage($view, 0x201, [IntPtr]1, (MkLp 60 60)) | Out-Null; Start-Sleep -Milliseconds 80
[In]::PostMessage($view, 0x200, [IntPtr]1, (MkLp 260 60)) | Out-Null; Start-Sleep -Milliseconds 80
[In]::PostMessage($view, 0x202, [IntPtr]0, (MkLp 260 60)) | Out-Null; Start-Sleep -Milliseconds 200
$r1 = Rect $view
Check 'dragging moves the window with the pointer' (([Math]::Abs(($r1.L - $r0.L) - 200) -le 2) -and ([Math]::Abs($r1.T - $r0.T) -le 2)) ("moved {0},{1}" -f ($r1.L - $r0.L), ($r1.T - $r0.T))
Check 'the drag did not close it' (Vis $view)
$cx = [int](($r1.L + $r1.Rt) / 2); $cy = [int](($r1.T + $r1.B) / 2)
$saved = (Get-Content $posfile -ErrorAction SilentlyContinue)
Check 'the place is remembered in the position file' ($saved -eq "$cx $cy") "file: $saved, centre: $cx $cy"

# close, then Space again: now it is in the cache
[In]::PostMessage($h, $WM_CLOSE_REQ, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
$gone = Wait-Until { -not (Vis $view) } 2000
Check "close request hides the preview" ($null -ne $gone)
$null = $null
Remove-Item $log -ErrorAction SilentlyContinue
[In]::PostMessage($h, $WM_SPACE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
$null = Wait-Until { (Vis $view) } 3000
Start-Sleep -Milliseconds 200
$line2 = (Get-Content $log -ErrorAction SilentlyContinue | Where-Object { $_ -match 'space_to_shown_ms' } | Select-Object -Last 1)
Write-Host "  second Space (cached): $line2"
Check 'second Space is faster than 40 ms' ($line2 -match 'space_to_shown_ms=([\d.]+)' -and [double]$Matches[1] -lt 40) $line2
$r2 = Rect $view
Check 'closed and opened again, it is where it was left' (([Math]::Abs($r2.L - $r1.L) -le 2) -and ([Math]::Abs($r2.T - $r1.T) -le 2)) ("{0},{1} vs {2},{3}" -f $r2.L, $r2.T, $r1.L, $r1.T)
[In]::PostMessage($h, $WM_CLOSE_REQ, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null

# Mac behaviour: with the preview up, selecting something else shows that, until Space / Esc closes it.
# The selection is changed through Explorer's own COM object (not the keyboard), so it is safe while a game has the focus.
function Select-InExplorer([string]$name) {
    foreach ($wnd in @($shell.Windows())) {
        try {
            if ($wnd.LocationURL -like '*nuquick-e2e*') {
                $item = $wnd.Document.Folder.ParseName($name)
                $wnd.Document.SelectItem($item, 29)   # select + deselect others + ensure visible + focus
                return $true
            }
        } catch {}
    }
    return $false
}
function Shown([string]$name, [int]$ms = 3000) {
    $null -ne (Wait-Until { (Get-Content $log -ErrorAction SilentlyContinue) -contains "shown $name" } $ms)
}
Remove-Item $log -ErrorAction SilentlyContinue
[In]::PostMessage($h, $WM_SPACE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Check 'Space shows pic0 again' (Shown 'pic0.jpg')
$null = Select-InExplorer 'pic1.jpg'
Check 'selecting pic1 with the preview up shows pic1' (Shown 'pic1.jpg')
Check 'the preview is still up' (Vis $view)
$null = Select-InExplorer 'pic2.jpg'
Check 'selecting pic2 shows pic2' (Shown 'pic2.jpg')
$null = Select-InExplorer 'note.txt'
Check 'selecting a .txt shows its card' (Shown 'note.txt')
Check 'the preview is still up after the card' (Vis $view)
$null = Select-InExplorer 'sub'
Check 'selecting a folder shows its info card' (Shown 'sub')
$null = Select-InExplorer 'pic0.jpg'
Check 'selecting a picture again shows it' (Shown 'pic0.jpg')
Start-Sleep -Milliseconds 600
$shownCount = @(Get-Content $log -ErrorAction SilentlyContinue | Where-Object { $_ -like 'shown *' }).Count
Check 'no flicker: a resting selection is not re-shown' ($shownCount -le 6) "shown $shownCount times"
[In]::PostMessage($h, $WM_CLOSE_REQ, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
$null = Wait-Until { -not (Vis $view) } 2000
Check 'Esc / Space (close) ends it' (-not (Vis $view))
Start-Sleep -Milliseconds 600
$stay = Select-InExplorer 'pic1.jpg'
Start-Sleep -Milliseconds 800
Check 'after closing, a new selection does not reopen it' (-not (Vis $view))

# Space on something that is not a picture: its info card (like Quick Look), not nothing
$null = Select-InExplorer 'note.txt'
Start-Sleep -Milliseconds 800
Remove-Item $log -ErrorAction SilentlyContinue
[In]::PostMessage($h, $WM_SPACE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Check 'Space on a .txt shows its info card' (Shown 'note.txt')
Check 'the card window is up' (Vis $view)
# a click without moving closes it
[In]::PostMessage($view, 0x201, [IntPtr]1, (MkLp 60 60)) | Out-Null; Start-Sleep -Milliseconds 60
[In]::PostMessage($view, 0x202, [IntPtr]0, (MkLp 60 60)) | Out-Null
$null = Wait-Until { -not (Vis $view) } 2000
Check 'a click on the preview closes it' (-not (Vis $view))

# memory after use, then trim
$agent.Refresh()
Write-Host ("  agent after use: WS {0:N1} MB  private {1:N1} MB" -f ($agent.WorkingSet64/1MB), ($agent.PrivateMemorySize64/1MB))

[In]::PostMessage($h, $WM_QUIT, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
$sw = [Diagnostics.Stopwatch]::StartNew(); while (-not $agent.HasExited -and $sw.ElapsedMilliseconds -lt 4000) { Start-Sleep -Milliseconds 20; $agent.Refresh() }
$exited = $agent.HasExited
Check 'quit message ends the agent' $exited
if (-not $exited) { $agent | Stop-Process -Force }
# close the Explorer windows we opened (only those showing our temp folder)
foreach ($wnd in @($shell.Windows())) { try { if ($wnd.LocationURL -like '*nuquick-e2e*') { $wnd.Quit() } } catch {} }
Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
Write-Host ("`n{0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip)
if ($script:fail -gt 0) { exit 1 }
