# UX walk-through of the right-click menu and the skins, from "step 0" (a first start with no settings at all), the way a
# new user meets it. Messages only (no real mouse / keyboard), so it is safe while a game has the focus.
#   tools\test\e2e-ux-menu.ps1 [-Exe path\to\nuviewer.exe] [-Shots C:\some\folder]
# Pictures of every step are written to -Shots (default: %TEMP%\nv-ux) so a person can look at them.
param([string]$Exe, [string]$Shots = (Join-Path $env:TEMP 'nv-ux'))
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'lib.ps1')
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $Exe) { $Exe = Join-Path $target 'release\nuviewer.exe' }
if (-not (Test-Path $Exe)) { throw "missing $Exe" }
$script:Exe = $Exe
New-Item -ItemType Directory -Force $Shots | Out-Null

Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class Ux {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [StructLayout(LayoutKind.Sequential)] public struct MII { public uint cbSize, fMask, fType, fState, wID; public IntPtr hSubMenu, hbmpChecked, hbmpUnchecked, dwItemData, dwTypeData; public uint cch; public IntPtr hbmpItem; }
    [DllImport("user32.dll")] static extern bool GetMenuItemInfo(IntPtr m, uint pos, bool byPos, ref MII mii);
    [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
    // (lines, separators, lines that open a submenu) of a popup menu handle.
    public static int[] Stats(IntPtr hm) {
        int lines = 0, seps = 0, subs = 0; int n = GetMenuItemCount(hm);
        for (uint i = 0; i < n; i++) { var mi = new MII(); mi.cbSize = (uint)Marshal.SizeOf(typeof(MII)); mi.fMask = 0x104; // MIIM_FTYPE | MIIM_SUBMENU
            if (!GetMenuItemInfo(hm, i, true, ref mi)) continue;
            if ((mi.fType & 0x800) != 0) seps++; else { lines++; if (mi.hSubMenu != IntPtr.Zero) subs++; } }
        return new int[] { lines, seps, subs };
    }
    // Visible top-level windows of class `cls` that belong to process `pid`, topmost first.
    public static List<IntPtr> Of(uint pid, string cls) {
        var l = new List<IntPtr>();
        EnumWindows((h, p) => { uint q; GetWindowThreadProcessId(h, out q); var c = new StringBuilder(64); GetClassName(h, c, 64);
            if (q == pid && c.ToString() == cls && IsWindowVisible(h)) l.Add(h); return true; }, IntPtr.Zero);
        return l;
    }
}
'@

function Shot-Window([IntPtr]$h, [string]$file) {
    $r = New-Object In+RECT; [void][In]::GetWindowRect($h, [ref]$r)
    $w = $r.R - $r.L; $hh = $r.B - $r.T
    if ($w -le 0 -or $hh -le 0) { return $null }
    $bmp = New-Object System.Drawing.Bitmap $w, $hh
    $g = [System.Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc()
    [void][In]::PrintWindow($h, $dc, 2); $g.ReleaseHdc($dc); $g.Dispose()
    $bmp.Save((Join-Path $Shots $file), [System.Drawing.Imaging.ImageFormat]::Png)
    return $bmp
}
function Mode-Colour($bmp) { $h = @{}; for ($y = 8; $y -lt $bmp.Height - 8; $y += 5) { for ($x = 6; $x -lt $bmp.Width - 6; $x += 5) { $c = $bmp.GetPixel($x, $y); $k = '{0:X2}{1:X2}{2:X2}' -f $c.R, $c.G, $c.B; $h[$k]++ } }; ($h.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First 1).Key }
function Menus { @([Ux]::Of([uint32]$script:viewer.Id, '#32768')) }
function Right-Click([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0205, [IntPtr]0, (Pack $x $y)) }
function Wait-Menu([int]$count = 1) { Wait-Until { $m = Menus; if ($m.Count -ge $count) { $m } } 3000 }
function Menu-Key([int]$vk, [int]$wait = 150) {
    $m = Menus; if ($m.Count -eq 0) { return }
    [void][In]::PostMessage($m[0], 0x0100, [IntPtr]$vk, [IntPtr]::Zero); Start-Sleep -Milliseconds $wait
}
function Inject([int]$vk, [int]$mods = 0) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 400 }
function Close-Menus { for ($i = 0; $i -lt 4 -and (Menus).Count -gt 0; $i++) { Menu-Key 0x1B 200 } }

# ---- the folder a new user would have: a few pictures, no settings file, no history ------------------------------------
$work = Join-Path $env:TEMP ('nv-ux-' + [guid]::NewGuid().ToString('N').Substring(0, 6))
New-Item -ItemType Directory $work | Out-Null
$names = 'holiday-001', 'holiday-002', 'holiday-003', 'portrait', 'street', 'sunset'
$cols = @('#C8381E', '#2E8B57', '#2F6DB5', '#B8860B', '#6A4C93', '#E07B39')
for ($i = 0; $i -lt $names.Count; $i++) {
    $bmp = New-Object System.Drawing.Bitmap 1600, 1000
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $c1 = [System.Drawing.ColorTranslator]::FromHtml($cols[$i])
    $br = New-Object System.Drawing.Drawing2D.LinearGradientBrush ([System.Drawing.Point]::new(0, 0)), ([System.Drawing.Point]::new(1600, 1000)), $c1, ([System.Drawing.Color]::FromArgb(20, 20, 30))
    $g.FillRectangle($br, 0, 0, 1600, 1000)
    $f = New-Object System.Drawing.Font 'Segoe UI', 90
    $g.DrawString($names[$i], $f, [System.Drawing.Brushes]::White, 80, 380)
    $g.Dispose(); $bmp.Save((Join-Path $work ($names[$i] + '.jpg')), [System.Drawing.Imaging.ImageFormat]::Jpeg); $bmp.Dispose()
}
$ini = Join-Path $work 'fresh-settings.ini'          # does not exist: the defaults, exactly what a first start sees
$env:NUVIEWER_SETTINGS = $ini
$env:NUVIEWER_SAVE_AS = Join-Path $work 'saved-copy.png'      # Save as skips its dialog and writes here
$first = Join-Path $work 'holiday-001.jpg'

Write-Host "`n[0] First start: no settings file, a picture opened from Explorer"
$sw = [Diagnostics.Stopwatch]::StartNew()
$ok = Open-Viewer $first @('--windowed')
Check 'the viewer starts and shows the first picture' $ok (Title)
Write-Host ("      first picture after {0} ms (incl. harness waits)   title: {1}" -f $sw.ElapsedMilliseconds, (Title))
[void](Shot-Window $script:win '00-first-start.png')
Check 'defaults: nu-dark is the skin of a first start' (-not (Test-Path $ini)) 'no settings file is created by merely looking'

$r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
$cx = [int](($r.R - $r.L) / 2); $cy = [int](($r.B - $r.T) / 2)

Write-Host "`n[1] A new user right-clicks the picture"
Right-Click $cx $cy
$m = Wait-Menu 1
Check 'a popup menu appears' ($null -ne $m)
if ($m) {
    $hm = [In]::SendMessage($m[0], 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero)   # MN_GETHMENU
    $st = [Ux]::Stats($hm); $items = $st[0]; $sep = $st[1]; $subs = $st[2]
    Write-Host "      first level: $items lines + $sep separators, $subs of them open a submenu"
    Check "the first level is short (at most 24 lines)  [$items]" ($items -le 24)
    Check "it is grouped: at least 5 separators  [$sep]" ($sep -ge 5)
    Check "the long lists are folded into submenus  [$subs]" ($subs -ge 8)
    Check "each group of the first level is separated: 5 separators  [$sep]" ($sep -eq 5)
    $bm = Shot-Window $m[0] '01-menu.png'
    Check 'the menu is painted (not blank)' ($null -ne $bm -and $bm.Width -gt 200 -and $bm.Height -gt 300)
    if ($bm) {
        # Skin check: the panel colour of nu-dark (0x202225) must dominate the menu body.
        $px = $bm.GetPixel(40, [int]($bm.Height / 2)); $dark = ($px.R -lt 80 -and $px.G -lt 80 -and $px.B -lt 90)
        Check "the menu is drawn in the skin colours, not the system's  [$($px.R),$($px.G),$($px.B)]" $dark
    }
}

Write-Host "`n[2] Opening the groups with the keyboard (Down, Right)"
$groups = [ordered]@{ 'go-to' = 7; 'zoom' = 8; 'rotate' = 9; 'view' = 10; 'sort' = 11; 'rating' = 12; 'file' = 15; 'tools' = 19 }
foreach ($g in $groups.Keys) {
    Close-Menus; Start-Sleep -Milliseconds 400
    Right-Click $cx $cy; [void](Wait-Menu 1); Start-Sleep -Milliseconds 300
    for ($i = 0; $i -lt $groups[$g]; $i++) { Menu-Key 0x28 120 }
    Menu-Key 0x27 500    # Right = open the submenu
    $all = Menus
    if ($all.Count -ge 2) {
        $hs = [In]::SendMessage($all[0], 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero)
        $cnt = [In]::GetMenuItemCount($hs)
        [void](Shot-Window $all[0] ("02-sub-$g.png"))
        Check "submenu '$g' opens  [$cnt entries]" ($cnt -ge 3)
    } else { Check "submenu '$g' opens" $false }
}
Close-Menus

Write-Host "`n[3] Every skin: the menu and the viewer's own panels"
Inject 0x49          # I: info panel
Inject 0x56          # V: viewer controls
$skinCount = 11
$ids = 'nu-dark', 'nu-light', 'marigold', 'rouge', 'verdant', 'azure', 'bumblebee', 'candy', 'aubergine', 'greyscale', 'jet-black'
for ($k = 0; $k -lt $skinCount; $k++) {
    if ($k -gt 0) { Inject 0x4B 5 }       # Ctrl+Alt+K: next skin
    Start-Sleep -Milliseconds 200
    [void](Shot-Window $script:win ("03-skin-$k-$($ids[$k])-window.png"))
    Right-Click $cx $cy; [void](Wait-Menu 1)
    $mm = Menus
    if ($mm.Count -gt 0) {
        $b = Shot-Window $mm[0] ("03-skin-$k-$($ids[$k])-menu.png")
        $c = $b.GetPixel(30, 100)
        Write-Host ("      {0,-10} menu body #{1:X2}{2:X2}{3:X2}" -f $ids[$k], $c.R, $c.G, $c.B)
    }
    Close-Menus
}
$env:NUVIEWER_SETTINGS = $ini
$saved = if (Test-Path $ini) { Get-Content $ini -Raw } else { '' }
Check 'the chosen skin is kept in the settings file' ($saved -match 'skin = jet-black') $saved.Substring(0, [math]::Min(80, $saved.Length))

Write-Host "`n[4] A second start, and the new actions"
Close-Viewer
$ok = Open-Viewer $first @('--windowed')
Check 'restart works' $ok
$r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
Right-Click $cx $cy; $mm = Wait-Menu 1
if ($mm) { $b = Shot-Window $mm[0] '04-after-restart-menu.png'; $c = $b.GetPixel(30, 100)
    Check ("the skin chosen last time is still the one in use (Jet Black)  [#{0:X2}{1:X2}{2:X2}]" -f $c.R, $c.G, $c.B) ($c.R -lt 20 -and $c.G -lt 20 -and $c.B -lt 20) }
Close-Menus
New-Item -ItemType File (Join-Path $work 'extra-zz.jpg') -Force | Out-Null
Copy-Item (Join-Path $work 'sunset.jpg') (Join-Path $work 'extra-zz.jpg') -Force
Inject 0x52 1     # Ctrl+R = Refresh
Start-Sleep -Milliseconds 700
$tm = [regex]::Match((Title), '(\d+)/(\d+)')
Check "Refresh finds the file added behind its back and stays on the picture  [$(Title)]" ($tm.Success -and [int]$tm.Groups[2].Value -ge 7 -and (Title) -like 'holiday-001*') (Title)

$dst = Join-Path $work 'saved-copy.png'
Inject 0x53 5     # Ctrl+Alt+S = Save as (the dialog is skipped by the test variable)
$w = Wait-Until { if (Test-Path $dst) { Get-Item $dst } } 6000
Check 'Save as writes a PNG copy in another format' ($null -ne $w -and $w.Length -gt 1000)
if ($w) {
    $b = [System.Drawing.Image]::FromFile($dst); Check "the copy is a real PNG of the same size  [$($b.Width)x$($b.Height)]" ($b.Width -eq 1600 -and $b.Height -eq 1000); $b.Dispose()
}


Write-Host "`n[5] NuGallery: the same menu painter and skin (Azure, a light one)"
Close-Viewer
Set-Content $ini 'skin = azure' -Encoding ASCII
$gexe = Join-Path (Split-Path $Exe) 'nugallery.exe'
if (Test-Path $gexe) {
    $env:NUGALLERY_STATE = Join-Path $work 'gallery-state.ini'
    $gp = Start-Process $gexe -ArgumentList @("`"$work`"", '--new-window', '--windowed') -PassThru
    $gw = Wait-Until { $gp.Refresh(); if ($gp.MainWindowHandle -ne 0) { $gp.MainWindowHandle } } 12000
    Check 'NuGallery starts' ($null -ne $gw)
    if ($gw) {
        Start-Sleep -Milliseconds 1800
        [void](Shot-Window $gw '05-gallery-azure.png')
        $script:viewer = $gp; $script:win = $gw
        $gr = New-Object In+RECT; [void][In]::GetWindowRect($gw, [ref]$gr)
        Right-Click ([int](($gr.R - $gr.L) * 0.55)) ([int](($gr.B - $gr.T) * 0.35)); $gm = Wait-Menu 1
        Check 'the gallery menu opens' ($null -ne $gm)
        if ($gm) { $hb = [Ux]::Stats([In]::SendMessage($gm[0], 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero)); Write-Host "      gallery menu: $($hb[0]) lines, $($hb[1]) separators, $($hb[2]) submenus"
            $b = Shot-Window $gm[0] '05-gallery-menu.png'; $mc = Mode-Colour $b
            Check "the gallery menu is drawn in Azure (panel F3F3F3)  [#$mc]" ($mc -eq 'F3F3F3')
            Check "the gallery's first level is short too (at most 28 lines)  [$($hb[0])]" ($hb[0] -le 28) }
        Close-Menus
        $script:viewer = $null
    }
    if (-not $gp.HasExited) { $gp.Kill() }
} else { Skip 'NuGallery' "no nugallery.exe next to $Exe" }

Close-Viewer
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
Write-Host "`nResults: $script:pass passed, $script:fail failed, $script:skip skipped.  Pictures: $Shots"
if ($script:fail -gt 0) { exit 1 }
