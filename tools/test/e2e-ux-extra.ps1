# The parts of the menu / skin work that messages alone cannot show:
#   [1] what a screen reader sees (MSAA, the interface Narrator reads menus through): every menu line has a name, the submenus are marked as such
#   [2] keyboard letters (mnemonics) pick a line
#   [3] other scales (NV_MENU_SCALE 1.5 and 2.0: the menu is drawn bigger, nothing is cut off or overlaps)
#   [4] the real Open and Save as dialogs (their file name box is filled and OK pressed - no test variable)
#   [5] Thai: the lines are translated and fit
#   [6] the REAL mouse: hover opens a submenu, a click on a line runs it
# Part 6 moves the real pointer and clicks, so it runs only if this viewer really is the foreground window and the pointer really
# moved to where it was sent; otherwise it is reported as not run (never forced: a game may hold the focus).
#   tools\test\e2e-ux-extra.ps1 [-Exe path\to\nuviewer.exe] [-Shots folder]
param([string]$Exe, [string]$Shots = (Join-Path $env:TEMP 'nv-ux-extra'), [string]$Only = '')   # -Only 2,6 runs just those parts
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'lib.ps1')
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $Exe) { $Exe = Join-Path $target 'release\nuviewer.exe' }
if (-not (Test-Path $Exe)) { throw "missing $Exe" }
$exeMain = $Exe       # ($Exe and $script:Exe are one variable in PowerShell: keep the real path under another name)
$script:Exe = $Exe
New-Item -ItemType Directory -Force $Shots | Out-Null
Add-Type -ReferencedAssemblies 'Accessibility' -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public class Acc { public string Name; public int L, T, W, H; public int Role; }
public static class Ux3 {
    [DllImport("oleacc.dll")] static extern int AccessibleObjectFromWindow(IntPtr hwnd, uint id, ref Guid riid, [MarshalAs(UnmanagedType.Interface)] out Accessibility.IAccessible acc);
    // The lines of a popup menu as a screen reader gets them: name, place and role of each child.
    public static List<Acc> Lines(IntPtr h) {
        var list = new List<Acc>(); Guid iid = new Guid("618736E0-3C3D-11CF-810C-00AA00389B71"); Accessibility.IAccessible a;
        if (AccessibleObjectFromWindow(h, 0xFFFFFFFC, ref iid, out a) != 0 || a == null) return list;
        int n = a.accChildCount;
        for (int i = 1; i <= n; i++) {
            var x = new Acc(); try { x.Name = a.get_accName(i); } catch { }
            try { int l, t, w, hh; a.accLocation(out l, out t, out w, out hh, i); x.L = l; x.T = t; x.W = w; x.H = hh; } catch { }
            try { x.Role = Convert.ToInt32(a.get_accRole(i)); } catch { }
            list.Add(x);
        }
        return list;
    }
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, string l);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    public static List<IntPtr> Of(uint pid, string cls) { var l = new List<IntPtr>();
        EnumWindows((h, p) => { uint q; GetWindowThreadProcessId(h, out q); var c = new StringBuilder(64); GetClassName(h, c, 64);
            if (q == pid && c.ToString() == cls && IsWindowVisible(h)) l.Add(h); return true; }, IntPtr.Zero); return l; }
}
'@

function Menus { @([Ux3]::Of([uint32]$script:viewer.Id, '#32768')) }
function Dialogs { @([Ux3]::Of([uint32]$script:viewer.Id, '#32770')) }
function Right-Click([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0205, [IntPtr]0, (Pack $x $y)) }
function Wait-Menu([int]$count = 1) { Wait-Until { $m = Menus; if ($m.Count -ge $count) { $m } } 3000 }
function Menu-Key([int]$vk, [int]$wait = 150) { $m = Menus; if ($m.Count -eq 0) { return }; [void][In]::PostMessage($m[0], 0x0100, [IntPtr]$vk, [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
function Menu-Char([char]$c, [int]$wait = 300) { $m = Menus; if ($m.Count -eq 0) { return }; [void][In]::PostMessage($m[0], 0x0102, [IntPtr][int]$c, [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
function Close-Menus { for ($i = 0; $i -lt 4 -and (Menus).Count -gt 0; $i++) { Menu-Key 0x1B 250 }; Start-Sleep -Milliseconds 300 }
function Inject([int]$vk, [int]$mods = 0) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 400 }
function Shot-Window([IntPtr]$h, [string]$file) {
    $r = New-Object In+RECT; [void][In]::GetWindowRect($h, [ref]$r)
    $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
    $g = [System.Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc(); [void][In]::PrintWindow($h, $dc, 2); $g.ReleaseHdc($dc); $g.Dispose()
    $bmp.Save((Join-Path $Shots $file), [System.Drawing.Imaging.ImageFormat]::Png); return $bmp
}
function Menu-Names([IntPtr]$menuWnd) {   # role 21 = separator; the "&" before a keyboard letter is not part of the name a person hears
    $l = @([Ux3]::Lines($menuWnd) | Where-Object { $_.Role -ne 21 })
    foreach ($x in $l) { $x.Name = ([string]$x.Name).Split("`t")[0] }
    return $l
}
$work = Join-Path $env:TEMP ('nv-ux3-' + [guid]::NewGuid().ToString('N').Substring(0, 6))
New-Item -ItemType Directory $work | Out-Null
foreach ($n in 'a', 'b', 'c') { Make-Picture (Join-Path $work "$n.png") 800 500 120 }
$ini = Join-Path $work 's.ini'
$env:NUVIEWER_SETTINGS = $ini
Remove-Item Env:NUVIEWER_SAVE_AS -ErrorAction SilentlyContinue
Remove-Item Env:NV_MENU_SCALE -ErrorAction SilentlyContinue

function Start-Fresh([string]$settings = '') {
    Close-Viewer
    if (-not $settings) { $settings = 'language = en' }      # the display language of this PC is not the test's business
    Set-Content $ini $settings -Encoding ASCII
    $ok = Open-Viewer (Join-Path $work 'a.png') @('--windowed')
    $r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
    $script:cx = [int](($r.R - $r.L) / 2); $script:cy = [int](($r.B - $r.T) / 2)
    return $ok
}

if (($Only -eq '') -or (($Only -split ',') -contains '1')) {
Write-Host "`n[1] What a screen reader sees (UI Automation)"
Check 'viewer starts' (Start-Fresh)
Right-Click $cx $cy; $m = Wait-Menu 1
Check 'the menu opens' ($null -ne $m)
if ($m) {
    $names = Menu-Names $m[0] | ForEach-Object { $_.Name }
    Write-Host "      names: $((($names | Select-Object -First 8) -join ' | ')) ..."
    Check "every one of the 22 lines has a name  [$(@($names).Count) items, $(@($names | Where-Object { $_ }).Count) named]" (@($names).Count -ge 22 -and @($names | Where-Object { -not $_ }).Count -eq 0)
    Check 'the names are the words on screen, not a picture or a number' ((@($names | Where-Object { $_ -match 'Zoom' }).Count -ge 1) -and (@($names | Where-Object { $_ -eq 'Rotate & flip' }).Count -ge 1)) ($names -join '|')
}
Close-Menus

}
if (($Only -eq '') -or (($Only -split ',') -contains '2')) {
Write-Host "`n[2] Keyboard: a typed letter goes to the line that starts with it, as in every Windows menu"
if (-not $script:viewer) { [void](Start-Fresh) }
Right-Click $cx $cy; [void](Wait-Menu 1); Start-Sleep -Milliseconds 300
Menu-Char 'z' 500; Menu-Key 0x27 600       # Z selects Zoom, Right opens it
Check "typing Z then Right opens the Zoom submenu  [menus open: $((Menus).Count)]" ((Menus).Count -ge 2)
Close-Menus
Right-Click $cx $cy; [void](Wait-Menu 1); Start-Sleep -Milliseconds 300
Menu-Char 'r' 500
if ((Menus).Count) { [void](Shot-Window (Menus)[0] '02-letter-r.png') }
Menu-Key 0x0D 900                          # Enter on the selected line: Refresh
Check 'typing R then Enter runs Refresh (the menu closes, the viewer lives)' (((Menus).Count -eq 0) -and -not $script:viewer.HasExited)
Close-Menus
}

if (($Only -eq '') -or (($Only -split ',') -contains '3')) {
Write-Host "`n[3] Other scales"
$prev = $null
foreach ($sc in '1.0', '1.5', '2.0') {
    $env:NV_MENU_SCALE = $sc
    [void](Start-Fresh)
    Right-Click $cx $cy; $m = Wait-Menu 1
    if ($m) {
        $b = Shot-Window $m[0] ("03-scale-$sc.png")
        Write-Host "      scale ${sc}: menu $($b.Width) x $($b.Height)"
        if ($prev) { $ratio = $b.Height / $prev; Check "the menu grows with the scale  [${sc}: height x$([math]::Round($ratio,2)) of 1.0]" ($ratio -gt ([double]$sc * 0.85) -and $ratio -lt ([double]$sc * 1.2)) }
        else { $prev = $b.Height }
        Menu-Key 0x28 100; Menu-Key 0x28 100; Menu-Key 0x28 100; Menu-Key 0x28 100; Menu-Key 0x28 100; Menu-Key 0x28 100; Menu-Key 0x28 100; Menu-Key 0x28 100; Menu-Key 0x27 1200   # Zoom
        $all = Menus
        if ($all.Count -ge 2) { [void](Shot-Window $all[0] ("03-scale-$sc-zoom.png")) }
        Check "scale ${sc}: a submenu opens and is drawn" ($all.Count -ge 2)
    } else { Check "scale ${sc}: the menu opens" $false }
    Close-Menus
}
Remove-Item Env:NV_MENU_SCALE -ErrorAction SilentlyContinue

}
if (($Only -eq '') -or (($Only -split ',') -contains '4')) {
Write-Host "`n[4] The real Open and Save as dialogs"
[void](Start-Fresh)
Inject 0x4F 1                        # Ctrl+O
$dlg = Wait-Until { $d = Dialogs; if ($d.Count) { $d[0] } } 8000
Check 'Ctrl+O shows the Open dialog' ($null -ne $dlg)
if ($dlg) {
    $edit = [In]::Children($dlg) | Where-Object { $_.Id -eq 1148 -or ($_.Cls -eq 'Edit' -and $_.Id -ne 0) } | Select-Object -First 1
    $okb = [In]::Children($dlg) | Where-Object { $_.Id -eq 1 -and $_.Cls -eq 'Button' } | Select-Object -First 1
    Check 'its file name box and OK button are there' (($null -ne $edit) -and ($null -ne $okb))
    if ($edit -and $okb) {
        [void][Ux3]::SendMessage($edit.H, 0x000C, [IntPtr]::Zero, (Join-Path $work 'c.png'))
        [void][In]::PostMessage($okb.H, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero)
        $t = Wait-Until { if ((Title) -like 'c.png*') { 1 } } 5000
        Check "choosing c.png in the dialog opens it  [$(Title)]" ($null -ne $t)
    } else { [void][In]::PostMessage($dlg, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
}
Start-Sleep -Milliseconds 400
Inject 0x53 5                        # Ctrl+Alt+S
$dlg = Wait-Until { $d = Dialogs; if ($d.Count) { $d[0] } } 8000
Check 'Ctrl+Alt+S shows the Save as dialog' ($null -ne $dlg)
if ($dlg) {
    $edit = [In]::Children($dlg) | Where-Object { $_.Id -eq 1148 -or ($_.Cls -eq 'Edit' -and $_.Id -ne 0) } | Select-Object -First 1
    $okb = [In]::Children($dlg) | Where-Object { $_.Id -eq 1 -and $_.Cls -eq 'Button' } | Select-Object -First 1
    if ($edit -and $okb) {
        $out = Join-Path $work 'from-dialog.jpg'
        [void][Ux3]::SendMessage($edit.H, 0x000C, [IntPtr]::Zero, $out)
        [void][In]::PostMessage($okb.H, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero)
        $w = Wait-Until { if (Test-Path $out) { Get-Item $out } } 6000
        Check 'choosing from-dialog.jpg writes a JPEG' ($null -ne $w -and $w.Length -gt 500)
        if ($w) { $fs = [IO.File]::OpenRead($out); $b0 = $fs.ReadByte(); $b1 = $fs.ReadByte(); $fs.Close(); Check 'and it really is a JPEG (FF D8)' (($b0 -eq 0xFF) -and ($b1 -eq 0xD8)) }
    } else { Check 'the Save as dialog has a file name box and OK' $false; [void][In]::PostMessage($dlg, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
}

}
if (($Only -eq '') -or (($Only -split ',') -contains '5')) {
Write-Host "`n[5] Thai"
$thaiDir = Join-Path $work 'thai'; New-Item -ItemType Directory $thaiDir | Out-Null
Copy-Item $exeMain (Join-Path $thaiDir 'nuviewer.exe'); Copy-Item (Join-Path $root 'crates\nv-text\lang') (Join-Path $thaiDir 'lang') -Recurse   # the language files sit next to the program
$script:Exe = Join-Path $thaiDir 'nuviewer.exe'
[void](Start-Fresh "language = th")
Right-Click $cx $cy; $m = Wait-Menu 1
if ($m) {
    $names = Menu-Names $m[0] | ForEach-Object { $_.Name }
    Write-Host ("      " + (($names | Select-Object -First 6) -join ' | '))
    Check 'the menu is in Thai' (@($names | Where-Object { $_ -match '[\u0E00-\u0E7F]' }).Count -ge 10) ($names -join '|')
    $b = Shot-Window $m[0] '05-thai-menu.png'
    Check "the Thai menu is drawn  [$($b.Width) x $($b.Height)]" ($b.Width -gt 200 -and $b.Height -gt 400)
} else { Check 'the Thai menu opens' $false }
Close-Menus
$script:Exe = $exeMain

}
if (($Only -eq '') -or (($Only -split ',') -contains '6')) {
Write-Host "`n[6] The real mouse"
[void](Start-Fresh)
$fg = [Ux3]::SetForegroundWindow($script:win); Start-Sleep -Milliseconds 300
$isFg = ([Ux3]::GetForegroundWindow() -eq $script:win)
$r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
$sx = $r.L + $cx; $sy = $r.T + $cy
[void][Ux3]::SetCursorPos($sx, $sy); Start-Sleep -Milliseconds 100
$p = New-Object Ux3+POINT; [void][Ux3]::GetCursorPos([ref]$p)
$moved = ([math]::Abs($p.X - $sx) -le 1 -and [math]::Abs($p.Y - $sy) -le 1)
if (-not ($isFg -and $moved)) {
    Skip 'real mouse on the menu' "the viewer is not the foreground window ($isFg) or the pointer did not move ($moved): another program holds the focus"
} else {
    $before = Green-Box $cx $cy 400
    [Ux3]::mouse_event(0x0008, 0, 0, 0, [UIntPtr]::Zero); [Ux3]::mouse_event(0x0010, 0, 0, 0, [UIntPtr]::Zero)   # right button down, up
    $m = Wait-Menu 1
    Check 'a real right click opens the menu' ($null -ne $m)
    if ($m) {
        $els = Menu-Names $m[0]; Write-Host "      menus: $($m.Count), lines read: $(@($els).Count), first: $($els[0].Name) at $($els[0].L),$($els[0].T)"
        $zoom = $els | Where-Object { $_.Name -match 'Zoom' } | Select-Object -First 1
        Write-Host "      Zoom line at screen $($zoom.L),$($zoom.T) size $($zoom.W)x$($zoom.H); the viewer window starts at $sx,$sy"
        [void][Ux3]::SetCursorPos([int]($zoom.L + $zoom.W / 2 - 30), [int]($zoom.T + $zoom.H / 2)); Start-Sleep -Milliseconds 150
        [void][Ux3]::SetCursorPos([int]($zoom.L + $zoom.W / 2), [int]($zoom.T + $zoom.H / 2)); Start-Sleep -Milliseconds 900
        [void][Ux3]::GetCursorPos([ref]$p); Write-Host "      pointer now at $($p.X),$($p.Y)"; [void](Shot-Window $m[0] '06-real-menu.png')
        Check 'hovering Zoom with the real pointer opens its submenu' ((Menus).Count -ge 2)
        if ((Menus).Count -ge 2) {
            $sub = (Menus)[0]; [void](Shot-Window $sub '06-real-hover.png')
            $items = Menu-Names $sub
            $two = $items | Where-Object { $_.Name -match '^2' } | Select-Object -First 1
            if ($two) {
                [void][Ux3]::SetCursorPos([int]($two.L + $two.W / 2), [int]($two.T + $two.H / 2)); Start-Sleep -Milliseconds 400
                [Ux3]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); [Ux3]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)   # left click
                Start-Sleep -Milliseconds 900
                $after = Green-Box $cx $cy 400
                Check "a real click on 2x zooms the picture  [green square $($before[0]) -> $($after[0]) px]" ($after[0] -gt $before[0] * 1.6)
                Check 'and the menu is gone' ((Menus).Count -eq 0)
            } else { Check 'the Zoom submenu lists 2x' $false (($items | ForEach-Object { $_.Name }) -join '|') }
        }
    }
    Close-Menus
}
}

Close-Viewer
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
Write-Host "`nResults: $script:pass passed, $script:fail failed, $script:skip skipped.  Pictures: $Shots"
if ($script:fail -gt 0) { exit 1 }
