# The 17-point fix list of 2026-11-17 (gallery: selection after the viewer, Back / Forward, archives, folder tree, drag and drop,
# address bar, hints, menus, video preview). Windowed, scratch settings / state / caches, window messages in, window titles and
# files out; nothing uses the real mouse or keyboard. The gallery exe is started with NUGALLERY_TEST=1, which adds what is only on
# the screen (tree rows and where they are, the hint, the video player's state) to the window title.
#   tools\test\e2e-fixes.ps1 [-Dir <folder with nugallery.exe and nuviewer.exe>] [-BrokenZip <a zip without a table of contents>]
param([string]$Dir = $(if ($env:CARGO_TARGET_DIR) { Join-Path $env:CARGO_TARGET_DIR 'debug' } else { 'R:\cargo-target\NuViewer\debug' }),
      [string]$BrokenZip = '')
. (Join-Path $PSScriptRoot 'lib.ps1')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class Dlg2 {
    public delegate bool EP(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EP cb, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    public static IntPtr Of(uint pid, string cls) { IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var c = new StringBuilder(64); GetClassName(h, c, 64); if (c.ToString() == cls) { found = h; return false; } } return true; }, IntPtr.Zero); return found; }
}
'@
$g = Join-Path $Dir 'nugallery.exe'
if (-not (Test-Path $g)) { Write-Host "no nugallery.exe in $Dir"; exit 2 }
if (Get-Process nugallery -ErrorAction SilentlyContinue) { Write-Host 'NuGallery is already running (maybe yours): close it first'; exit 2 }
$base = Join-Path $env:TEMP 'nug_fix'
if (Test-Path $base) { Remove-Item $base -Recurse -Force }
New-Item -ItemType Directory $base | Out-Null
$st = Join-Path $base 'state'; New-Item -ItemType Directory $st | Out-Null
$env:NUGALLERY_STATE = Join-Path $st 'state.ini'
$env:NUVIEWER_SETTINGS = Join-Path $st 'settings.ini'
Set-Content $env:NUVIEWER_SETTINGS "start_fullscreen=0`r`nconfirm_delete=0`r`n" -Encoding ASCII
$env:LOCALAPPDATA = Join-Path $st 'local'; New-Item -ItemType Directory $env:LOCALAPPDATA -Force | Out-Null
$env:NUGALLERY_TEST = '1'
$cols = @([System.Drawing.Color]::FromArgb(200,30,30), [System.Drawing.Color]::FromArgb(30,160,60), [System.Drawing.Color]::FromArgb(40,70,200))

$script:gp = $null
function Start-Gallery([string]$arg, [bool]$newWindow = $true) {
    Stop-Gallery
    $a = @("`"$arg`"", '--windowed'); if ($newWindow) { $a += '--new-window' }
    $script:gp = Start-Process $g -ArgumentList $a -PassThru
    $script:win = Wait-Until { $script:gp.Refresh(); if ($script:gp.MainWindowHandle -ne 0 -and $script:gp.MainWindowTitle -like '*NuGallery*') { $script:gp.MainWindowHandle } } 15000
    Start-Sleep -Milliseconds 1200
}
function Stop-Gallery { if ($script:gp -and -not $script:gp.HasExited) { $script:gp.Kill(); [void]$script:gp.WaitForExit(3000) }; $script:gp = $null }
function GT { $script:gp.Refresh(); $script:gp.MainWindowTitle }
function Inject([int]$vk, [int]$mods = 0, [int]$wait = 400) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
function Typed([string]$s) { foreach ($c in $s.ToCharArray()) { [void][In]::PostMessage($script:win, 0x0102, [IntPtr][int]$c, [IntPtr]::Zero); Start-Sleep -Milliseconds 30 }; Start-Sleep -Milliseconds 200 }
function Items { if ((GT) -match '[^\d](\d+) items') { [int]$Matches[1] } else { -1 } }
function Dpi { [In]::GetDpiForWindow($script:win) / 96.0 }
function Client { $r = New-Object Dlg2+RECT; [void][Dlg2]::GetClientRect($script:win, [ref]$r); @($r.R, $r.B) }
function RClick([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0205, [IntPtr]0, (Pack $x $y)); Start-Sleep -Milliseconds 500 }
function Hover([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0200, [IntPtr]0, (Pack $x $y)); Start-Sleep -Milliseconds 120 }
function Menu-Texts([IntPtr]$menuWnd) {
    $hm = [In]::SendMessage($menuWnd, 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero)
    $n = [In]::GetMenuItemCount($hm); $out = @()
    for ($i = 0; $i -lt $n; $i++) { $out += [In]::MenuText($hm, $i) }
    return ,@($hm, $out)
}
function Popup-Wait { Wait-Until { $m = [Dlg2]::Of($script:gp.Id, '#32768'); if ($m -ne [IntPtr]::Zero) { $m } } 6000 }
function Popup-Close { [void][In]::PostMessage($script:win, 0x001F, [IntPtr]0, [IntPtr]0); Start-Sleep -Milliseconds 400 }
function Dbg([string]$l) { if ($env:E2E_DEBUG) { Write-Host ("   [" + $l + "] " + ((GT) -replace 'T\[[^\]]*\]', '')) } }
function Tree-Row([string]$name) { if ((GT) -match ('T\[' + [regex]::Escape($name) + '\|(-?\d+)\|(-?\d+)\]')) { @([int]$Matches[1], [int]$Matches[2]) } else { $null } }

# ================================================================== 1. Back / Forward return to where the view was
$big = Join-Path $base 'big'; $sub = Join-Path $base 'sub'
New-Item -ItemType Directory $big, $sub | Out-Null
1..80 | ForEach-Object { Make-Picture (Join-Path $big ("b{0:000}.png" -f $_)) 120 90 20 $cols[$_ % 3] }
1..3 | ForEach-Object { Make-Picture (Join-Path $sub ("s{0}.png" -f $_)) 120 90 20 $cols[$_ % 3] }
Start-Gallery $big
Check 'big folder opens (80 items)' ((Items) -eq 80) (GT)
Inject 0x23 0 700     # End: the last picture
Check 'End selects the last picture' ((GT) -like '*selected: b080.png*') (GT)
Inject 0x4C 1; Typed $sub; Inject 0x0D 0 1000
Check 'the address bar goes to the other folder (3 items)' ((Items) -eq 3) (GT)
Inject 0x25 4 1200    # Alt+Left
Check 'Back returns to the big folder' ((Items) -eq 80) (GT)
Check 'Back keeps the picture that was selected (not the first one)' ((GT) -like '*selected: b080.png*') (GT)
Inject 0x27 4 1000    # Alt+Right
Check 'Forward goes to the small folder again' ((Items) -eq 3) (GT)
Stop-Gallery

# ================================================================== 2. coming back from the viewer: one picture selected, not two
Start-Gallery $big $false
Click 100 100
Check 'one picture selected by a click' ((GT) -match '1 selected:') (GT)
$second = Start-Process $g -ArgumentList @("`"$(Join-Path $big 'b040.png')`"") -PassThru   # hands over to the running one (what the viewer does when it is closed)
Start-Sleep -Milliseconds 1500
Check 'the running gallery shows the picture the viewer ended on: exactly 1 selected' ((GT) -like '*1 selected: b040.png*') (GT)
Stop-Gallery
Get-Process nugallery -ErrorAction SilentlyContinue | Stop-Process -Force

# ================================================================== 3. archives
Add-Type -AssemblyName System.IO.Compression.FileSystem
$ar = Join-Path $base 'arc'; New-Item -ItemType Directory $ar | Out-Null
$pre = Join-Path $base 'zsrc'; New-Item -ItemType Directory "$pre\pics\deep" | Out-Null
Make-Picture "$pre\pics\a_red.png" 300 200 80 $cols[0]
Make-Picture "$pre\pics\b_green.png" 300 200 80 $cols[1]
Make-Picture "$pre\pics\deep\c_blue.png" 300 200 80 $cols[2]
Set-Content "$pre\readme.txt" 'hello'
[IO.Compression.ZipFile]::CreateFromDirectory($pre, (Join-Path $ar 'pictures.zip'))
$pre2 = Join-Path $base 'zsrc2'; New-Item -ItemType Directory $pre2 | Out-Null
Set-Content "$pre2\notes.txt" 'one'; Set-Content "$pre2\data.csv" 'a,b'
[IO.Compression.ZipFile]::CreateFromDirectory($pre2, (Join-Path $ar 'nopics.zip'))
if ($BrokenZip -and (Test-Path $BrokenZip)) { Copy-Item $BrokenZip (Join-Path $ar 'broken.zip') }
Start-Gallery $ar
Check 'folder with the archives opens' ((Items) -ge 2) (GT)
Inject 0x41 1 200; Inject 0x1B 0 200    # clear
# A tag on the tile says it is an archive: look for the badge colour (D9A400) in the window.
$b = Grab; $tag = 0; for ($y = 20; $y -lt $b.Height - 20; $y += 2) { for ($x = 0; $x -lt $b.Width; $x += 2) { $c = $b.GetPixel($x, $y); if ([math]::Abs($c.R - 217) -lt 8 -and [math]::Abs($c.G - 164) -lt 8 -and $c.B -lt 20) { $tag++ } } }; $b.Dispose()
Check 'archives carry a coloured ZIP tag' ($tag -gt 20) "tag pixels=$tag"
function Open-Archive([string]$name) {
    $t = GT
    Inject 0x24 0 300     # Home: first item
    for ($i = 0; $i -lt 6; $i++) { if ((GT) -like "*selected: $name*") { break }; Inject 0x27 0 250 }
    Inject 0x20 0 2500    # Space: open
}
Open-Archive 'pictures.zip'
Check 'pictures.zip opens as a folder' ((GT) -match 'ARC\[.*pictures\.zip\]') (GT)
Check 'it lists what is inside: 2 pictures, the folder pics' (((Items) -ge 2)) (GT)
Start-Sleep -Milliseconds 1500
# Three coloured pictures must show their own colours (red, green, blue), nothing else.
Inject 0x08 0 1200   # Backspace: out of the archive
Check 'Backspace leaves the archive for its folder' ((GT) -notmatch 'ARC\[') (GT)
Open-Archive 'nopics.zip'
Check 'a zip with no pictures opens too' ((GT) -match 'ARC\[.*nopics\.zip\]') (GT)
Check 'and shows its 2 files' ((Items) -eq 2) (GT)
Inject 0x08 0 1000
if (Test-Path (Join-Path $ar 'broken.zip')) {
    Open-Archive 'broken.zip'
    Check 'a zip without a table of contents opens' ((GT) -match 'ARC\[.*broken\.zip\]') (GT)
    Check 'and lists the 21 files 7-Zip finds' ((Items) -eq 21) (GT)
    Inject 0x08 0 1000
}
Stop-Gallery

# ================================================================== 4. address bar: select, copy, paste with the mouse and the keys
Start-Gallery $big
Inject 0x4C 1 600
Inject 0x43 1 300     # Ctrl+C: the whole path (everything is selected on entering the bar)
$clip = [System.Windows.Forms.Clipboard]::GetText()
Check 'Ctrl+L then Ctrl+C copies the folder path' ($clip -eq $big) "clip=$clip"
$d = Dpi
# drag over the middle of the text with the mouse
Btn-Down ([int](270 * $d)) ([int](22 * $d)); Btn-Move ([int](330 * $d)) ([int](22 * $d)) $true; Btn-Up ([int](330 * $d)) ([int](22 * $d))
Inject 0x43 1 300
$clip2 = [System.Windows.Forms.Clipboard]::GetText()
Check 'dragging with the mouse selects part of the path' ($clip2.Length -gt 0 -and $clip2.Length -lt $big.Length -and $big.Contains($clip2)) "clip=$clip2"
RClick ([int](300 * $d)) ([int](22 * $d))
$pm = Popup-Wait
Check 'right click in the bar shows a menu' ($null -ne $pm)
if ($pm) { $t = (Menu-Texts $pm)[1]; Check 'it has Cut / Copy / Paste / Select all' (($t -join '|') -match 'Cut' -and ($t -join '|') -match 'Paste' -and ($t -join '|') -match 'Select all') ($t -join '|'); Popup-Close }
Inject 0x1B 0 300
Stop-Gallery

# ================================================================== 5. right click menus: Open with, Properties, Settings; hints
Start-Gallery $big
$d = Dpi
RClick ([int](100 * $d)) ([int](100 * $d))
$pm = Popup-Wait
Check 'right click on a picture shows the menu' ($null -ne $pm)
if ($pm) {
    $r = Menu-Texts $pm; $t = $r[1]; $joined = $t -join '|'
    Check 'menu has Open with' ($joined -match 'Open with') $joined
    Check 'menu has Properties' ($joined -match 'Properties') $joined
    Check 'menu has Settings' ($joined -match 'Settings') $joined
    Check 'menu has View and Sort by' ($joined -match 'View' -and $joined -match 'Sort by') $joined
    $ow = [Array]::IndexOf($t, ($t | Where-Object { $_ -match 'Open with' } | Select-Object -First 1))
    if ($ow -ge 0) { $sm = [In]::GetSubMenu($r[0], $ow); $cnt = [In]::GetMenuItemCount($sm); $items = @(); for ($i = 0; $i -lt $cnt; $i++) { $items += [In]::MenuText($sm, $i) }
        Check 'Open with lists programs and "Choose another app"' ($cnt -ge 1 -and ($items -join '|') -match 'Choose another app') ($items -join '|') }
    Popup-Close
}
Hover ([int](100 * $d)) ([int](100 * $d)); Hover ([int](102 * $d)) ([int](101 * $d))
Start-Sleep -Milliseconds 900
Check 'resting on a picture shows a hint with its name' ((GT) -match 'TIP\[b00[0-9]\.png') (GT)
Hover ([int](600 * $d)) ([int](600 * $d))
Stop-Gallery

# ================================================================== 6. the folder tree: right click, drag and drop, sideways scroll
$p = Join-Path $base 'dnd_parent'; $dnd = Join-Path $p 'dnd'
New-Item -ItemType Directory $dnd | Out-Null
1..4 | ForEach-Object { Make-Picture (Join-Path $dnd ("m{0}.png" -f $_)) 120 90 20 $cols[$_ % 3] }
Start-Gallery $dnd
Inject 0x42 1 900     # Ctrl+B: keep the folders panel open
$row = Tree-Row 'dnd_parent'
Check 'the tree shows the folder above the one on screen' ($null -ne $row) (GT)
if ($row) {
    Dbg 'before right click'; RClick $row[0] $row[1]
    $pm = Popup-Wait
    Check 'right click on a tree folder shows a menu' ($null -ne $pm)
    if ($pm) { $t = (Menu-Texts $pm)[1]; $j = $t -join '|'
        Check 'tree menu: Open, Cut, Copy, Paste, New folder, Rename, Delete, Properties' (($j -match 'Open') -and ($j -match 'Cut') -and ($j -match 'Paste') -and ($j -match 'New folder') -and ($j -match 'Rename') -and ($j -match 'Delete') -and ($j -match 'Properties')) $j
        Popup-Close }
    # drag the first picture onto that row
    $d = Dpi; $tx = [int]((300 + 70) * $d); $ty = [int](70 * $d)
    Dbg 'after menu closed'; Click $tx $ty; Dbg 'after click on tile'
    Btn-Down $tx $ty; Btn-Move ($tx + 25) ($ty + 25) $true; Btn-Move $row[0] $row[1] $true
    Check 'while dragging, the folder under the pointer is the target' ((GT).Contains('DRAG[' + $p + ']')) (GT)
    Btn-Up $row[0] $row[1]
    Start-Sleep -Milliseconds 1500
    Check 'dropping on the tree row moved the picture there' ((Get-ChildItem $p -File).Count -eq 1 -and (Get-ChildItem $dnd -File).Count -eq 3) ("parent=" + ((Get-ChildItem $p -File).Name -join ',') + " dnd=" + ((Get-ChildItem $dnd -File).Name -join ','))
    Check 'and it is not the old "already in that folder" message' ((GT) -notlike '*already*') (GT)
}
Stop-Gallery
# deep folders: the tree scrolls sideways
$deep = $base; 1..9 | ForEach-Object { $deep = Join-Path $deep ('a_rather_long_folder_name_number_{0}' -f $_) }
New-Item -ItemType Directory $deep | Out-Null
Make-Picture (Join-Path $deep 'x.png') 100 80 20
Start-Gallery $deep
Inject 0x42 1 1200
Check 'a deep tree is wider than its panel (sideways scroll bar)' ((GT) -match 'TH\[0/(\d+)\]' -and [int]$Matches[1] -gt (320 * (Dpi))) (GT)
Stop-Gallery

# ================================================================== 7. video preview with a controller
$ff = (Get-Command ffmpeg -ErrorAction SilentlyContinue).Source
if (-not $ff) { $ff = (Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter ffmpeg.exe -ErrorAction SilentlyContinue | Select-Object -First 1).FullName }
$vd = Join-Path $base 'vid'; New-Item -ItemType Directory $vd | Out-Null
if ($ff) {
    & $ff -v error -f lavfi -i 'testsrc=duration=8:size=640x360:rate=30' -f lavfi -i 'sine=frequency=440:duration=8' -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest (Join-Path $vd 'clip.mp4') 2>$null
}
if ($ff -and (Test-Path (Join-Path $vd 'clip.mp4'))) {
    Start-Gallery $vd
    Inject 0x49 1 1500     # Ctrl+I: keep the details panel open
    $ok = Wait-Until { (GT) -match 'MEDIA\[open=True' } 8000
    Check 'the video preview opens a player' ($null -ne $ok) (GT)
    $d = Dpi; $cl = Client
    $rw = [math]::Min(360 * $d, $cl[0] * 0.45); $pad = 14 * $d
    $pl = $cl[0] - $rw + $pad; $pr = $cl[0] - $pad
    $ph = [math]::Min(($pr - $pl) * 0.72, 250 * $d); $pt = 36 * $d
    $playX = [int]($pl + 18 * $d); $playY = [int]($pt + $ph - 15 * $d)
    Click $playX $playY
    Start-Sleep -Milliseconds 1800
    $s1 = GT
    Check 'the play button starts it' ($s1 -match 'playing=True') $s1
    Check 'a video frame reached the preview' ($s1 -match 'frame=True') $s1
    $pos1 = if ($s1 -match 'pos=(\d+)') { [int]$Matches[1] } else { -1 }
    Start-Sleep -Milliseconds 1500
    $s2 = GT; $pos2 = if ($s2 -match 'pos=(\d+)') { [int]$Matches[1] } else { -1 }
    Check 'the time moves on while it plays' ($pos2 -gt $pos1 + 800) "pos $pos1 -> $pos2"
    # pixels of the preview change between two looks (the picture is moving, not frozen)
    $b1 = Grab; Start-Sleep -Milliseconds 700; $b2 = Grab
    $diff = 0; for ($y = [int]($pt + 4); $y -lt [int]($pt + $ph - 40 * $d); $y += 3) { for ($x = [int]$pl + 4; $x -lt [int]$pr - 4; $x += 3) { if ($b1.GetPixel($x, $y).ToArgb() -ne $b2.GetPixel($x, $y).ToArgb()) { $diff++ } } }
    $b1.Dispose(); $b2.Dispose()
    Check 'the preview picture is not frozen' ($diff -gt 40) "changed samples=$diff"
    Click $playX $playY
    Start-Sleep -Milliseconds 600
    Check 'pressing it again pauses' ((GT) -match 'playing=False') (GT)
    Stop-Gallery
} else { Skip 'video preview' 'no ffmpeg to make a test clip' }

Write-Host ""
Write-Host "passed $($script:pass), failed $($script:fail), skipped $($script:skip)"
Get-Process nugallery -ErrorAction SilentlyContinue | Stop-Process -Force
exit $(if ($script:fail -gt 0) { 1 } else { 0 })


