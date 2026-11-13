# End-to-end test of session 8: dialogs owned by the viewer window (Alt+Tab), error codes, sort / shuffle, the film strip and
# read mode. Messages in, window pixels out (see lib.ps1): it never touches your real mouse or keyboard, and every scenario
# reads its own settings file.
#   tools\test\e2e-session8.ps1 [-Exe path\to\nuviewer.exe]
param([string]$Exe)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $Exe) { $Exe = Join-Path $target 'release\nuviewer.exe' }
if (-not (Test-Path $Exe)) { throw "missing $Exe" }
$script:Exe = $Exe
. (Join-Path $PSScriptRoot 'lib.ps1')
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class In8 {
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    // FindWindow(class, $null) from PowerShell passes an empty title, which only matches untitled windows: look by process and class instead.
    public static IntPtr TopLevelOf(uint pid, string cls) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid) { var c = new StringBuilder(64); GetClassName(h, c, 64); if (c.ToString() == cls) { found = h; return false; } } return true; }, IntPtr.Zero);
        return found;
    }
}
'@

$sandbox = Join-Path $env:TEMP "nv_s8_$([Diagnostics.Process]::GetCurrentProcess().Id)"
New-Item -ItemType Directory -Force $sandbox | Out-Null
$realSettings = $env:NUVIEWER_SETTINGS
$realLocal = $env:LOCALAPPDATA
$env:LOCALAPPDATA = Join-Path $sandbox 'local'
New-Item -ItemType Directory -Force $env:LOCALAPPDATA | Out-Null
$scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$W = $scr.Width; $H = $scr.Height
$cx = [int]($W / 2); $cy = [int]($H / 2)
function Sub-Dir([string]$name) { $d = Join-Path $sandbox $name; New-Item -ItemType Directory -Force $d | Out-Null; $d }
function Key-Mods([int]$vk, [int]$mods, [int]$wait = 450) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }   # 1 ctrl, 2 shift, 4 alt
function Hover([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0200, [IntPtr]0, (Pack $x $y)); Start-Sleep -Milliseconds 250 }
function Dpi { [In]::GetDpiForWindow($script:win) / 96.0 }
function Near([System.Drawing.Color]$c, [int]$r, [int]$g, [int]$b, [int]$tol = 40) { ([math]::Abs($c.R - $r) -le $tol) -and ([math]::Abs($c.G - $g) -le $tol) -and ([math]::Abs($c.B - $b) -le $tol) }
function Pix-At([int]$x, [int]$y) { Pixel $x $y }
function Touch-Date([string]$path, [int]$year) { (Get-Item $path).LastWriteTime = [datetime]::new($year, 6, 1) }

try {
    # ============================================================================================
    Write-Host "`n[1] Dialogs belong to the viewer window (one Alt+Tab entry, switching back lands on the dialog)"
    $d = Sub-Dir 'own'
    Make-Picture "$d\a.png" 200 100 0; Make-Picture "$d\b.png" 200 100 0
    Use-Settings "$sandbox\own.ini" "canvas = 204060"
    Check 'viewer opens' (Open-Viewer "$d\a.png")
    Key-Mods 0xBC 1 900          # Ctrl+,  opens Settings
    $form = Wait-Until { $f = [In8]::TopLevelOf([uint32]$script:viewer.Id, 'NuViewerForm'); if ($f -ne [IntPtr]::Zero) { $f } } 6000
    Check 'Settings opens' ($null -ne $form)
    if ($form) {
        Check 'the Settings window is owned by the viewer window' ([In8]::GetWindow($form, 4) -eq $script:win)
        Check 'the viewer window is disabled while Settings is open (it takes no input)' (-not [In8]::IsWindowEnabled($script:win))
        [void][In]::PostMessage($form, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)   # WM_CLOSE
        $gone = Wait-Until { -not [In8]::IsWindow($form) } 4000
        Check 'closing Settings destroys the form' ($null -ne $gone)
        Check 'and the viewer window works again' ([In8]::IsWindowEnabled($script:win))
    }
    Key-Mods 0x2E 2 800          # Shift+Delete: "delete permanently?" question
    $mb = Wait-Until { $m = [In]::FindWindow('#32770', 'NuViewer'); if ($m -ne [IntPtr]::Zero) { $m } } 4000
    Check 'a question box appears' ($null -ne $mb)
    if ($mb) {
        Check 'the question box is owned by the viewer window' ([In8]::GetWindow($mb, 4) -eq $script:win)
        [void][In]::PostMessage($mb, 0x0111, [IntPtr]7, [IntPtr]::Zero); Start-Sleep -Milliseconds 600   # IDNO
        Check 'No keeps the file' (Test-Path "$d\a.png")
    }
    Close-Viewer

    # ============================================================================================
    Write-Host "`n[2] Error codes"
    $d = Sub-Dir 'err'
    Make-Picture "$d\a_ok.png" 200 100 0
    [IO.File]::WriteAllBytes("$d\bad.png", [byte[]](1..300 | ForEach-Object { ($_ * 37) % 256 }))
    [IO.File]::WriteAllBytes("$d\empty.jpg", [byte[]]@())
    Use-Settings "$sandbox\err.ini" "canvas = 204060"
    Check 'viewer opens a damaged PNG' (Open-Viewer "$d\bad.png")
    $t = Title
    Check "the title carries an error code  [$t]" ($t -match '\[NV-\d{4}\]')
    $b = Grab; $red = $false
    for ($y = [int]($H * 0.3); $y -lt [int]($H * 0.7) -and -not $red; $y += 2) { for ($x = [int]($W * 0.3); $x -lt [int]($W * 0.7); $x += 2) { $c = $b.GetPixel($x, $y); if ($c.R -gt 230 -and $c.G -gt 90 -and $c.G -lt 125 -and $c.B -gt 90 -and $c.B -lt 125) { $red = $true; break } } }
    $b.Dispose()
    Check 'the error panel (with its red edge) is drawn over the picture area' $red
    Key 0x49 500     # I: the info panel lists the error
    $b = Grab; $b.Dispose()
    Key 0x27 600     # Right: next file (empty.jpg)
    $t = Title
    Check "an empty file says NV-1004  [$t]" ($t -match 'NV-1004')
    Key 0x24 600     # Home: a_ok.png
    Check 'and Home goes on to a good picture' ((Title) -like 'a_ok.png*')
    Close-Viewer

    # ============================================================================================
    Write-Host "`n[3] Sort and shuffle"
    $d = Sub-Dir 'ord'
    Make-Picture "$d\a_big.png" 1200 900 0; Make-Picture "$d\b_small.png" 100 80 0; Make-Picture "$d\c_mid.png" 400 300 0
    Touch-Date "$d\c_mid.png" 2020; Touch-Date "$d\a_big.png" 2021; Touch-Date "$d\b_small.png" 2022
    $ini = "$sandbox\ord.ini"
    Use-Settings $ini "sort = name"
    Check 'viewer opens a_big.png' (Open-Viewer "$d\a_big.png")
    Key 0x23 400; Check "End goes to the last file in name order  [$(Title)]" ((Title) -like 'c_mid*')
    Key 0x4A 400; Key 0x24 400      # J: date, then Home
    Check "J sorts by date: the oldest file is first  [$(Title)]" ((Title) -like 'c_mid*')
    Key 0x23 400
    Check "and the newest is last  [$(Title)]" ((Title) -like 'b_small*')
    Key 0x4A 400; Key 0x24 400      # J: size
    Check "J again sorts by size: the smallest first  [$(Title)]" ((Title) -like 'b_small*')
    Key-Mods 0x4A 2 400; Key 0x24 400   # Shift+J: descending
    Check "Shift+J reverses it: the largest first  [$(Title)]" ((Title) -like 'a_big*')
    $text = Get-Content $ini -Raw
    Check 'the sort was saved' (($text -match 'sort = size') -and ($text -match 'sort_desc = true')) $text
    Key 0x52 500                    # R: shuffle
    Check "R shuffles: the title says so  [$(Title)]" ((Title) -like '*shuffled*')
    $now = ((Title) -split ' ')[0]
    Key 0x52 500
    Check "R again restores the sort  [$(Title)]" ((-not ((Title) -like '*shuffled*')) -and (((Title) -split ' ')[0]) -eq $now)
    Close-Viewer
    # A restart reads the saved sort.
    Check 'viewer reopens' (Open-Viewer "$d\a_big.png")
    Key 0x24 400
    Check "the saved sort applies at start-up: largest first  [$(Title)]" ((Title) -like 'a_big*')
    Close-Viewer

    # ============================================================================================
    Write-Host "`n[4] Film strip"
    $d = Sub-Dir 'strip'
    1..14 | ForEach-Object { Make-Picture ("$d\p{0:00}.png" -f $_) 300 200 0 ([System.Drawing.Color]::FromArgb(40 + $_ * 12, 60, 220 - $_ * 12)) }
    Use-Settings "$sandbox\strip.ini" "canvas = 204060`nsidebar = false"
    Check 'viewer opens' (Open-Viewer "$d\p01.png")
    $dpi = Dpi
    $before = Pix-At ($W - 40) $cy
    Check "before: the canvas colour is at the right edge  [$($before.R),$($before.G),$($before.B)]" (Near $before 32 64 96 20)
    Hover ($W - 2) $cy
    $after = Pix-At ($W - 8) ([int](34 * $dpi))
    Check "the film strip opens when the pointer touches the right edge  [$($after.R),$($after.G),$($after.B)]" ($after.B -lt 70 -and $after.R -lt 70)
    # A thumbnail: item 2 (third) is at list_top + 2 * pitch.
    $barX = $W - 176 * $dpi
    $itemX = [int]($barX + (12 + 76) * $dpi); $itemY = [int]((72.4 + 2 * 158 + 76) * $dpi)
    $thumb = Pix-At $itemX $itemY
    Check "a thumbnail is drawn in the strip, not the canvas  [$($thumb.R),$($thumb.G),$($thumb.B)]" (-not (Near $thumb 32 64 96 12))
    Hover $itemX $itemY; Click $itemX $itemY
    Start-Sleep -Milliseconds 500
    Check "clicking the third thumbnail jumps to it  [$(Title)]" ((Title) -like 'p03.png*')
    # The wheel over the strip scrolls the strip; over the picture it flips pages. (Pinned with L: Windows sends a mouse move at the
    # real pointer position after a posted click, which would close an unpinned strip under the test.)
    Key 0x4C 400
    Wheel -120 $itemX $itemY
    Check "the wheel over the strip does not change the picture  [$(Title)]" ((Title) -like 'p03.png*')
    Wheel -120 $cx $cy
    Check "the wheel over the picture goes to the next one  [$(Title)]" ((Title) -like 'p04.png*')
    # The header: sort by size, then shuffle.
    $chipSizeX = [int]($barX + (12 + 2 * 39 + 17.5) * $dpi); $chipY = [int](19.2 * $dpi)
    Hover ($W - 2) $cy; Hover $chipSizeX $chipY; Click $chipSizeX $chipY
    Check 'clicking "Size" in the strip header saves the sort' ((Get-Content "$sandbox\strip.ini" -Raw) -match 'sort = size')
    $chipShufX = [int]($barX + 122 * $dpi); $chipShufY = [int](47.2 * $dpi)
    Hover ($W - 2) $cy; Hover $chipShufX $chipShufY; Click $chipShufX $chipShufY
    Check "clicking Shuffle shuffles  [$(Title)]" ((Title) -like '*shuffled*')
    Key 0x4C 400     # unpin
    Hover $cx $cy; Start-Sleep -Milliseconds 300
    $closed = Pix-At ($W - 8) ([int](34 * $dpi))
    Check 'moving the pointer away closes the strip' (Near $closed 32 64 96 25)
    Key 0x4C 500     # L pins it
    $pinned = Pix-At ($W - 8) ([int](34 * $dpi))
    Check 'L keeps the strip open' ($pinned.B -lt 70 -and $pinned.R -lt 70)
    Close-Viewer

    # ============================================================================================
    Write-Host "`n[5] Read mode: two pages, no gap"
    $d = Sub-Dir 'book'
    $cols = @(@(220, 30, 30), @(30, 200, 30), @(30, 30, 220), @(220, 220, 30), @(240, 240, 240))
    1..5 | ForEach-Object { Make-Picture ("$d\page{0}.png" -f $_) 400 600 0 ([System.Drawing.Color]::FromArgb($cols[$_ - 1][0], $cols[$_ - 1][1], $cols[$_ - 1][2])) }
    Use-Settings "$sandbox\book.ini" "canvas = 204060`nsidebar = false`nread_start = 2`nread_rtl = false"
    Check 'viewer opens page 1' (Open-Viewer "$d\page1.png")
    Key 0x42 600     # B
    Check "B turns read mode on  [$(Title)]" ((Title) -like '*read mode*')
    Check 'page 1 (the cover) stands alone' ((Near (Pix-At ($cx - 100) $cy) 220 30 30) -and (Near (Pix-At ($cx + 300) $cy) 32 64 96 25) -and (Near (Pix-At ($cx - 300) $cy) 32 64 96 25))
    Key 0x27 700
    Check "Right shows pages 2 and 3 together  [$(Title)]" ((Title) -like '*2-3/5*')
    Check 'page 2 (green) on the left, page 3 (blue) on the right' ((Near (Pix-At ($cx - 200) $cy) 30 200 30) -and (Near (Pix-At ($cx + 200) $cy) 30 30 220))
    Check 'no gap at the seam: green right up to the middle, blue from the middle' ((Near (Pix-At ($cx - 2) $cy) 30 200 30 25) -and (Near (Pix-At ($cx + 1) $cy) 30 30 220 25))
    Check 'the spread is 800 px wide: canvas just outside it' ((Near (Pix-At ($cx - 410) $cy) 32 64 96 25) -and (Near (Pix-At ($cx + 410) $cy) 32 64 96 25))
    Key 0x27 700
    Check "Right again: pages 4 and 5  [$(Title)]" (((Title) -like '*4-5/5*') -and (Near (Pix-At ($cx - 200) $cy) 220 220 30) -and (Near (Pix-At ($cx + 200) $cy) 240 240 240 20))
    Key 0x27 500
    Check 'at the end Right does nothing' ((Title) -like '*4-5/5*')
    Key 0x25 600; Key 0x25 600
    Check "Left twice is back at the cover  [$(Title)]" ((Title) -like 'page1.png*')
    Key-Mods 0x42 1 500     # Ctrl+B: right to left
    Key 0x27 700
    Check 'right-to-left puts page 2 on the right and page 3 on the left' ((Near (Pix-At ($cx + 200) $cy) 30 200 30) -and (Near (Pix-At ($cx - 200) $cy) 30 30 220))
    Check 'the direction was saved' ((Get-Content "$sandbox\book.ini" -Raw) -match 'read_rtl = true')
    Key-Mods 0x42 1 500     # back to left to right
    Key 0x24 600            # Home
    Key-Mods 0x42 2 700     # Shift+B: pairs start at this page
    Check "Shift+B starts the pairs at page 1: pages 1-2  [$(Title)]" ((Title) -like '*1-2/5*')
    Check 'red on the left, green on the right' ((Near (Pix-At ($cx - 200) $cy) 220 30 30) -and (Near (Pix-At ($cx + 200) $cy) 30 200 30))
    Key 0x42 600
    Check "B again turns it off  [$(Title)]" (-not ((Title) -like '*read mode*'))
    Close-Viewer

    Write-Host "`n[6] Read mode: a wide double-page scan stands alone"
    $d = Sub-Dir 'wide'
    Make-Picture "$d\w1.png" 400 600 0 ([System.Drawing.Color]::FromArgb(220, 30, 30))
    Make-Picture "$d\w2.png" 800 600 0 ([System.Drawing.Color]::FromArgb(30, 200, 30))
    Make-Picture "$d\w3.png" 400 600 0 ([System.Drawing.Color]::FromArgb(30, 30, 220))
    Make-Picture "$d\w4.png" 400 600 0 ([System.Drawing.Color]::FromArgb(220, 220, 30))
    Use-Settings "$sandbox\wide.ini" "canvas = 204060`nsidebar = false`nread_start = 1"
    Check 'viewer opens w1' (Open-Viewer "$d\w1.png")
    Key 0x42 600
    Check "a page next to a wide scan stays alone  [$(Title)]" (((Title) -like '*1/4*') -and (Near (Pix-At ($cx + 300) $cy) 32 64 96 25))
    Key 0x27 700
    Check "the wide scan fills the middle on its own  [$(Title)]" (((Title) -like 'w2.png*') -and (Near (Pix-At ($cx - 350) $cy) 30 200 30) -and (Near (Pix-At ($cx + 350) $cy) 30 200 30))
    Key 0x27 700
    Check "pairing carries on after it: pages 3 and 4  [$(Title)]" (((Title) -like '*3-4/4*') -and (Near (Pix-At ($cx - 200) $cy) 30 30 220) -and (Near (Pix-At ($cx + 200) $cy) 220 220 30))
    Close-Viewer
}
finally {
    Close-Viewer
    $env:NUVIEWER_SETTINGS = $realSettings
    $env:LOCALAPPDATA = $realLocal
    Remove-Item $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host ("`n{0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip) -ForegroundColor $(if ($script:fail) { 'Red' } else { 'Green' })
exit $(if ($script:fail) { 1 } else { 0 })
