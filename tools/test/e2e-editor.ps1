# NuEditor end to end: opens a picture, moves sliders with posted mouse messages, checks the pixels on screen, and
# walks through the three ways of leaving (pair file, burn in, new file). Messages only - no real mouse or keyboard,
# so it is safe while a game has the focus. Scratch files, scratch settings, scratch backups.
#
#   powershell -File tools\test\e2e-editor.ps1 [-Exe path\to\nueditor.exe]
param([string]$Exe = 'T:\cargo-target\NuViewer\release\nueditor.exe')
. (Join-Path $PSScriptRoot 'lib.ps1')

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class Ed {
    [StructLayout(LayoutKind.Sequential)] public struct RC { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RC r);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EP cb, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    delegate bool EP(IntPtr h, IntPtr l);
    // Visible top-level windows of a process with the given class name (the "keep your edits?" form is class NuViewerForm).
    public static List<IntPtr> Windows(uint pid, string cls) {
        var r = new List<IntPtr>();
        EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var s = new StringBuilder(64); GetClassName(h, s, 64); if (s.ToString() == cls) r.Add(h); } return true; }, IntPtr.Zero);
        return r;
    }
}
'@

$scratch = Join-Path ([IO.Path]::GetTempPath()) ("nueditor-e2e-" + [Diagnostics.Process]::GetCurrentProcess().Id)
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $scratch 'settings.ini'
Set-Content -Path $env:NUVIEWER_SETTINGS -Value "" -Encoding ASCII
$env:NUEDITOR_BACKUP_DIR = Join-Path $scratch 'backups'
$env:NUEDITOR_TEST = '1'   # the older panel layout (no graph dock) these scripts' coordinates assume

if (-not (Test-Path $Exe)) { Write-Host "no $Exe - build it first (cargo build --release -p nueditor)"; exit 2 }
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
function Client { $r = New-Object Ed+RC; [void][Ed]::GetClientRect($script:win, [ref]$r); $r }
function Scale { [double][In]::GetDpiForWindow($script:win) / 96.0 }
# Client coordinates of the track of row `n` (0-based) of the open group (Light is open at start; it follows two headers).
function Track-At([int]$n, [double]$t) {
    $d = Scale; $c = Client
    $y = (116.0 + 44.0 * $n + 30.0) * $d
    $x0 = $c.R - 300.0 * $d + 14.0 * $d; $x1 = $c.R - 14.0 * $d
    return @([int]($x0 + ($x1 - $x0) * $t), [int]$y)
}
function Image-Centre { $d = Scale; $c = Client; @([int](($c.R - 300.0 * $d) / 2), [int]($c.B / 2)) }
function Win-Pixel([int]$cx, [int]$cy) {
    # PrintWindow gives the whole window; client (0,0) sits at the frame offset.
    $b = Grab; $r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
    $pt = New-Object System.Drawing.Point 0, 0
    $off = [Math]::Max(0, [int](($r.R - $r.L) - (Client).R) / 2); $top = ($r.B - $r.T) - (Client).B - $off
    $c = $b.GetPixel($cx + $off, $cy + $top); $b.Dispose(); $c
}
function Drag-Slider([int]$n, [double]$t) {
    $p = Track-At $n $t
    Btn-Down $p[0] $p[1]
    Btn-Up $p[0] $p[1]
}
function Press-Key([int]$vk, [int]$mods = 0) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 350 }
function Leave-With([int]$buttonId) {
    # Esc, then answer the question form by clicking one of its buttons (WM_COMMAND), like the real click would.
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero)
    $f = Wait-Until { $l = [Ed]::Windows([uint32]$script:ed.Id, 'NuViewerForm'); if ($l.Count -gt 0) { $l[0] } } 5000
    if (-not $f) { return $false }
    [void][Ed]::PostMessage($f, 0x0111, [IntPtr]$buttonId, [IntPtr]::Zero)
    return [bool](Wait-Until { $script:ed.Refresh(); $script:ed.HasExited } 8000)
}
function Stop-Editor { if ($script:ed -and -not $script:ed.HasExited) { $script:ed.Kill(); [void]$script:ed.WaitForExit(3000) }; $script:ed = $null }
function Hash([string]$p) { (Get-FileHash $p -Algorithm MD5).Hash }

try {
    Write-Host "== NuEditor e2e =="
    $a = Join-Path $scratch 'a.png'; Make-Picture $a 1200 800 300
    Check 'opens a picture and reports the first image' (Open-Editor $a) (Ed-Title)
    if (-not $script:win) { throw 'no window' }
    $t = Ed-Title
    Check 'title shows the size and the edit time' ($t -match '1200x800' -and $t -match 'edit [0-9.]+ ms') $t

    $c = Image-Centre; $green = Win-Pixel $c[0] $c[1]
    Check 'the picture is on screen (green square at the centre)' ($green.G -gt 200 -and $green.R -lt 60) "$green"
    $dx = [int](0.25 * (Client).R * 0.0) ; $bg = Win-Pixel ($c[0] - 450) ($c[1] - 300)
    Check 'the red background is there too' ($bg.R -gt 150 -and $bg.G -lt 70) "$bg"
    $hashBefore = Hash $a

    # Exposure is the first row of Light. Right end of the track = +5 EV.
    Drag-Slider 0 1.0
    Start-Sleep -Milliseconds 300
    $bright = Win-Pixel ($c[0] - 450) ($c[1] - 300)
    Check 'exposure to the top end brightens the picture' ($bright.G -gt $bg.G + 60) "$bg -> $bright"
    $t = Ed-Title
    Check 'edited marker in the title' ($t.StartsWith([string][char]0x25CF)) $t
    if ($t -match 'edit ([0-9.]+) ms') { Check 'one slider move costs well under a frame (< 30 ms)' ([double]$Matches[1] -lt 30) "$($Matches[1]) ms" }

    Press-Key 0x5A 1   # Ctrl+Z
    $back = Win-Pixel ($c[0] - 450) ($c[1] - 300)
    Check 'Ctrl+Z puts the picture back' ([Math]::Abs($back.G - $bg.G) -lt 6) "$bg vs $back"
    Press-Key 0x59 1   # Ctrl+Y
    $again = Win-Pixel ($c[0] - 450) ($c[1] - 300)
    Check 'Ctrl+Y does the edit again' ($again.G -gt $bg.G + 60) "$again"

    # Right-click on the row resets that slider.
    $p = Track-At 0 0.5
    [void][In]::PostMessage($script:win, 0x0205, [IntPtr]0, (Pack $p[0] $p[1])); Start-Sleep -Milliseconds 350
    $reset = Win-Pixel ($c[0] - 450) ($c[1] - 300)
    Check 'right-click resets the slider' ([Math]::Abs($reset.G - $bg.G) -lt 6) "$reset"

    # ---- looking: zoom, pan, split, scopes, false colour ----
    $cl = Client
    Key 0x31 600
    $t = Ed-Title
    Check 'key 1 zooms to 100%' ($t -match '100%') $t
    $edge = Win-Pixel ($c[0] + 140) $c[1]
    Check '100%: the green square reaches further than it did fitted' ($edge.G -gt 200 -and $edge.R -lt 80) "$edge"
    Key 0x32 600
    Check 'key 2 zooms to 200%' ((Ed-Title) -match '200%') (Ed-Title)
    $edge2 = Win-Pixel ($c[0] + 250) $c[1]
    Check '200%: bigger still' ($edge2.G -gt 200 -and $edge2.R -lt 80) "$edge2"
    Key 0x31 600
    Btn-Down $c[0] $c[1]; Btn-Move ($c[0] - 200) $c[1] $true; Btn-Up ($c[0] - 200) $c[1]
    Start-Sleep -Milliseconds 400
    $moved = Win-Pixel ($c[0] - 200) $c[1]; $gone = Win-Pixel ($c[0] + 140) $c[1]
    Check 'dragging pans the zoomed picture' ($moved.G -gt 200 -and $gone.R -gt 150) "$moved / $gone"
    Key 0x46 600
    Check 'F goes back to fit' ((Ed-Title) -match 'fit') (Ed-Title)

    Drag-Slider 0 1.0
    Key 0x53 600   # S = split
    $lp = Win-Pixel ([int]($c[0] * 0.5)) ($c[1] - 250)
    $rp = Win-Pixel ([int]($c[0] * 1.5)) ($c[1] - 250)
    Check 'split: the original on the left of the divider, the edit on the right' ($rp.G -gt $lp.G + 60) "$lp | $rp"
    Key 0x53 400
    $b1 = Grab
    Key 0x59 600   # Y = scope
    $b2 = Grab
    $changed = 0
    for ($yy = $b1.Height - 130; $yy -lt $b1.Height - 30; $yy += 3) { for ($xx = 20; $xx -lt 280; $xx += 3) { $q = $b1.GetPixel($xx, $yy); $w = $b2.GetPixel($xx, $yy); if ([Math]::Abs($q.R - $w.R) + [Math]::Abs($q.G - $w.G) + [Math]::Abs($q.B - $w.B) -gt 30) { $changed++ } } }
    $b1.Dispose(); $b2.Dispose()
    Check 'Y puts a scope over the picture (the corner changed)' ($changed -gt 100) "$changed sample points differ"
    Key 0x59 300; Key 0x59 300; Key 0x59 300
    Key 0x58 600   # X = false colour
    $fc = Win-Pixel $c[0] $c[1]
    Check 'X: false colour paints the green square pink (skin-light band)' ($fc.R -gt 200 -and $fc.G -gt 100 -and $fc.G -lt 190) "$fc"
    Key 0x58 400
    $p = Track-At 0 0.5
    [void][In]::PostMessage($script:win, 0x0205, [IntPtr]0, (Pack $p[0] $p[1])); Start-Sleep -Milliseconds 300

    # Leave with a pair file: picture untouched, pair file written.
    Drag-Slider 0 0.75
    Check 'leaving with edits asks, pair file keeps the picture untouched' (Leave-With 12) 'form or exit failed'
    Check 'the picture file is byte for byte the same' ((Hash $a) -eq $hashBefore)
    $xml = "$a.nuedit"
    Check 'the pair file exists and names exposure' ((Test-Path $xml) -and ((Get-Content $xml -Raw) -match 'exposure=')) ''

    # Open again: the XML edits are applied without touching the file; leaving with no further change asks nothing.
    Check 'reopens with the pair file' (Open-Editor $a)
    $applied = Win-Pixel ($c[0] - 450) ($c[1] - 300)
    Check 'the pair-file edit is applied on opening' ($applied.G -gt $bg.G + 40) "$bg -> $applied"
    Check 'no edited marker: nothing is unsaved' (-not (Ed-Title).StartsWith([string][char]0x25CF)) (Ed-Title)
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero)
    Check 'Esc leaves at once when nothing changed' ([bool](Wait-Until { $script:ed.Refresh(); $script:ed.HasExited } 5000))

    # Burn in: the picture changes, the pair file goes, the original is kept in the backup folder.
    Check 'reopens for burning in' (Open-Editor $a)
    Drag-Slider 0 1.0
    Check 'burn in: leaves' (Leave-With 10)
    Check 'burn in: the picture changed' ((Hash $a) -ne $hashBefore)
    Check 'burn in: the pair file is gone' (-not (Test-Path $xml))
    Check 'burn in: the original is in the backup folder' ((Test-Path $env:NUEDITOR_BACKUP_DIR) -and (@(Get-ChildItem $env:NUEDITOR_BACKUP_DIR -ErrorAction SilentlyContinue).Count -ge 1))
    Add-Type -AssemblyName System.Drawing
    $img = [System.Drawing.Bitmap]::FromFile($a); $px = $img.GetPixel(10, 10); $img.Dispose()
    Check 'burn in: the written pixels are brighter than the original red background' ($px.G -gt 90) "$px"

    # New file: next to the original, which is left alone.
    $b = Join-Path $scratch 'b.png'; Make-Picture $b 600 400 100
    $bHash = Hash $b
    Check 'reopens for a new file' (Open-Editor $b)
    Press-Key 0x30   # 0 = reset all (nothing yet), then an edit
    Drag-Slider 2 1.0   # contrast
    Check 'new file: leaves' (Leave-With 11)
    Check 'new file: b_edit.png exists and b.png is untouched' ((Test-Path (Join-Path $scratch 'b_edit.png')) -and ((Hash $b) -eq $bHash))

    # The default way of saving is a setting: "new file, don't ask" leaves without any question.
    Set-Content -Path $env:NUVIEWER_SETTINGS -Value "editor_save = new`r`neditor_ask = false" -Encoding ASCII
    $d = Join-Path $scratch 'd.png'; Make-Picture $d 600 400 100
    Check 'reopens with the setting "new file, do not ask"' (Open-Editor $d)
    Drag-Slider 2 1.0
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero)
    Check 'Esc saves a new file without asking' ([bool](Wait-Until { $script:ed.Refresh(); $script:ed.HasExited } 8000))
    Check 'd_edit.png was written' (Test-Path (Join-Path $scratch 'd_edit.png'))
    Set-Content -Path $env:NUVIEWER_SETTINGS -Value "editor_save = burn`r`neditor_ask = true" -Encoding ASCII
    Check 'with the setting "burn in" the question opens with that button as the default' (Open-Editor $d)
    Drag-Slider 2 1.0
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero)
    $f = Wait-Until { $l = [Ed]::Windows([uint32]$script:ed.Id, 'NuViewerForm'); if ($l.Count -gt 0) { $l[0] } } 5000
    Check 'the question appears' ($null -ne $f)
    if ($f) {
        # The default push button has the style BS_DEFPUSHBUTTON (1); button ids: 10 burn in, 11 new file, 12 pair file.
        $btn = [In]::Children($f) | Where-Object { $_.Cls -eq 'Button' -and ($_.Style -band 0xF) -eq 1 }
        Check 'the default button is Burn in (id 10)' (@($btn).Count -eq 1 -and $btn.Id -eq 10) "$(@($btn | ForEach-Object { $_.Id }))"
        [void][Ed]::PostMessage($f, 0x0111, [IntPtr]2, [IntPtr]::Zero)   # Keep editing (cancel)
        Start-Sleep -Milliseconds 400
        Check 'Keep editing leaves the editor open' (-not $script:ed.HasExited)
    }
}
catch { Write-Host "  ERROR  $_" -ForegroundColor Red; $script:fail++ }
finally {
    Stop-Editor
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
Write-Host ""
Write-Host "passed $script:pass, failed $script:fail"
if ($script:fail -gt 0) { exit 1 }
