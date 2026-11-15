# NuGallery's commands: rename, new folder, search, copy / paste, delete, compress and the four batch dialogs (convert,
# rename, time stamp, remove metadata), plus 'NuViewer started with no picture opens NuGallery'.
# Windowed, with its own scratch settings / state / thumbnail cache; messages in, window titles and files out (never the
# real mouse or keyboard). Needs nuviewer.exe and nugallery.exe in one folder:
#   tools\test\e2e-gallery.ps1  [-Dir <that folder>]
param([string]$Dir = $(if ($env:CARGO_TARGET_DIR) { Join-Path $env:CARGO_TARGET_DIR 'release' } else { Join-Path $PSScriptRoot '..\..\target\release' }))
. (Join-Path $PSScriptRoot 'lib.ps1')
Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class Top {
    public delegate bool EP(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EP cb, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    public static IntPtr Form(uint pid) { IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var c = new StringBuilder(64); GetClassName(h, c, 64); if (c.ToString() == "NuViewerForm") { found = h; return false; } } return true; }, IntPtr.Zero); return found; }
}
'@
$g = Join-Path $Dir 'nugallery.exe'
$root = Join-Path $env:TEMP 'nug_cmds'
if (Test-Path $root) { Remove-Item $root -Recurse -Force }
New-Item -ItemType Directory $root | Out-Null
$cols = @([System.Drawing.Color]::FromArgb(200,30,30),[System.Drawing.Color]::FromArgb(30,160,60),[System.Drawing.Color]::FromArgb(40,70,200))
for ($i = 1; $i -le 8; $i++) { Make-Picture (Join-Path $root ("pic{0:00}.png" -f $i)) (300 + $i * 20) 200 60 $cols[$i % 3] }
$st = Join-Path $env:TEMP 'nug_cmds_state'; New-Item -ItemType Directory $st -Force | Out-Null
$env:NUGALLERY_STATE = Join-Path $st 'state.ini'; Remove-Item $env:NUGALLERY_STATE -ErrorAction SilentlyContinue
$env:NUVIEWER_SETTINGS = Join-Path $st 'settings.ini'
Set-Content $env:NUVIEWER_SETTINGS "start_fullscreen=0`r`n" -Encoding ASCII
$env:LOCALAPPDATA = Join-Path $st 'local'; New-Item -ItemType Directory $env:LOCALAPPDATA -Force | Out-Null
$gp = Start-Process $g -ArgumentList @("`"$root`"", '--windowed', '--new-window') -PassThru
$script:win = Wait-Until { $gp.Refresh(); if ($gp.MainWindowHandle -ne 0 -and $gp.MainWindowTitle -like '*NuGallery*') { $gp.MainWindowHandle } } 15000
Start-Sleep -Milliseconds 1200
function GT { $gp.Refresh(); $gp.MainWindowTitle }
function Inject([int]$vk, [int]$mods = 0, [int]$wait = 400) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
function Typed([string]$s) { foreach ($c in $s.ToCharArray()) { [void][In]::PostMessage($script:win, 0x0102, [IntPtr][int]$c, [IntPtr]::Zero); Start-Sleep -Milliseconds 40 }; Start-Sleep -Milliseconds 200 }
function Items { if ((GT) -match '. (\d+) items') { [int]$Matches[1] } else { -1 } }
function Form-Wait { Wait-Until { $f = [Top]::Form($gp.Id); if ($f -ne [IntPtr]::Zero) { $f } } 6000 }
function Set-Edit([IntPtr]$form, [int]$id, [string]$text) { $c = [In]::Children($form) | Where-Object { $_.Id -eq $id } | Select-Object -First 1; [void][In]::SendMessage($c.H, 0x0C, [IntPtr]::Zero, $text) }
function Press([IntPtr]$form, [int]$id) { $c = [In]::Children($form) | Where-Object { $_.Id -eq $id } | Select-Object -First 1; [void][In]::PostMessage($c.H, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero); Start-Sleep -Milliseconds 600 }

Check 'folder opened with 8 items' ((Items) -eq 8) (GT)

# --- rename (F2, type, Enter)
Click 100 100      # first tile
Key 0x71 300
Typed 'holiday'
Key 0x0D 800
Check 'F2 renames' (Test-Path (Join-Path $root 'holiday.png')) ((Get-ChildItem $root).Name -join ',')
Check 'renamed file is selected' ((GT) -like '*selected: holiday.png*') (GT)

# --- new folder (Ctrl+N)
Inject 0x4E 1
Typed 'Album X'
Key 0x0D 900
Check 'Ctrl+N creates a folder' (Test-Path (Join-Path $root 'Album X')) ((Get-ChildItem $root).Name -join ',')
Check 'folder shows up (8 pictures + folder = 9)' ((Items) -eq 9) (GT)

# --- search
Inject 0x46 1
Typed 'pic0'
Check 'search filters (7 pictures match pic0)' ((Items) -eq 7) (GT)
Key 0x1B 400
Check 'Esc clears the search' ((Items) -eq 9) (GT)

# --- copy + paste into the same folder (keeps both)
Click 400 100
Inject 0x43 1
Inject 0x56 1 1200
$n = (Get-ChildItem $root -File).Count
Check 'Ctrl+C then Ctrl+V adds a copy' ($n -eq 9) ("files=$n " + ((Get-ChildItem $root -File).Name -join ','))

# --- delete (to Recycle Bin)
Key 0x2E 1500
$n2 = (Get-ChildItem $root -File).Count
Check 'Del removes the selected file' ($n2 -eq ($n - 1)) "files=$n2"

# --- select all + compress (job)
Inject 0x41 1
Inject 0xFFF5 0 2500
$zip = Get-ChildItem $root -Filter *.zip | Select-Object -First 1
Check 'Compress makes a .zip' ($null -ne $zip) ((Get-ChildItem $root).Name -join ',')
if ($zip) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $z = [IO.Compression.ZipFile]::OpenRead($zip.FullName)
    Check 'zip holds the pictures' (($z.Entries | Where-Object { $_.Name -like '*.png' }).Count -ge 6) (($z.Entries | ForEach-Object { $_.FullName }) -join ',')
    $z.Dispose()
}

# --- quick convert to PNG of a copy ... use JPEG batch convert dialog on 2 files
Key 0x1B 200
Inject 0x41 1
Check 'several selected' ((GT) -match '(\d+) selected' -and [int]$Matches[1] -ge 2) (GT)
Inject 0xFFF2 0 600
$f = Form-Wait
Check 'convert dialog opens' ($f -ne $null)
if ($f) {
    Set-Edit $f 12 '100'
    Set-Edit $f 14 '_web'
    Press $f 1
    Start-Sleep -Milliseconds 1800
    $j = Get-ChildItem $root -Filter '*_web.jpg'
    Check 'batch convert wrote the JPEGs' ($j.Count -ge 2) ((Get-ChildItem $root).Name -join ',')
    if ($j.Count -gt 0) { $img = [System.Drawing.Image]::FromFile($j[0].FullName); Check 'resized to 100 px longest side' (($img.Width -eq 100) -or ($img.Height -eq 100)) "$($img.Width)x$($img.Height)"; $img.Dispose() }
}

# --- batch rename dialog
Key 0x1B 200
Inject 0x41 1
Inject 0xFFF1 0 600
$f = Form-Wait
Check 'rename dialog opens' ($f -ne $null)
if ($f) {
    Set-Edit $f 10 'shot_{n:2}'
    Press $f 18
    Press $f 1
    Start-Sleep -Milliseconds 900
    $names = (Get-ChildItem $root -File).Name
    Check 'batch rename numbered the files' (($names | Where-Object { $_ -like 'shot_*' }).Count -ge 8) ($names -join ',')
}

# --- batch time stamp dialog
Key 0x1B 200
Inject 0x41 1
Inject 0xFFF3 0 600
$f = Form-Wait
Check 'time stamp dialog opens' ($f -ne $null)
if ($f) {
    Set-Edit $f 11 '2020-02-03 04:05:06'
    Press $f 1
    Start-Sleep -Milliseconds 1500
    $m = (Get-ChildItem $root -File | Where-Object { $_.Extension -eq '.png' } | Select-Object -First 1)
    Check 'files got the date' ($m.LastWriteTime.Year -eq 2020 -and $m.LastWriteTime.Month -eq 2 -and $m.LastWriteTime.Day -eq 3) "$($m.LastWriteTime)"
}

# --- remove metadata dialog on a PNG with a text chunk is covered by unit tests; just check the dialog runs
Key 0x1B 200
Inject 0x41 1
Inject 0xFFF4 0 600
$f = Form-Wait
Check 'metadata dialog opens' ($f -ne $null)
if ($f) { Press $f 2; Start-Sleep -Milliseconds 400 }
Check 'gallery still running' (-not $gp.HasExited)

# --- type-ahead: a letter jumps to the first name that starts with it
Key 0x1B 200
Typed 's'
Check 'typing s jumps to shot_* (the renamed files)' ((GT) -like '*selected: shot_*') (GT)
Start-Sleep -Milliseconds 1200
Typed 'a'
Check 'typing a jumps to the folder Album X' ((GT) -like '*selected: Album X*') (GT)

# --- a big folder (address bar with Ctrl+L), then the scroll bar
$big = Join-Path $root 'big'
New-Item -ItemType Directory $big | Out-Null
1..80 | ForEach-Object { Make-Picture (Join-Path $big ("b{0:000}.png" -f $_)) 120 90 20 }
Inject 0x4C 1
Typed $big
Key 0x0D 1200
Check 'address bar opens the folder (80 pictures)' ((Items) -eq 80) (GT)
function Bar-Rows { $b = Grab; $x = $b.Width - 18; $rows = @(); for ($y = 40; $y -lt $b.Height - 10; $y++) { $c = $b.GetPixel($x, $y); if ($c.R -gt 70 -and $c.G -gt 70 -and $c.B -gt 70) { $rows += $y } }; $b.Dispose(); $rows }
$before = Bar-Rows
Check 'a scroll bar thumb is drawn' ($before.Count -gt 20) "rows=$($before.Count)"
$r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
$h = $r.B - $r.T
Click (($r.R - $r.L) - 26) ($h - 99)      # click the track near the bottom: the thumb jumps there
Start-Sleep -Milliseconds 500
$after = Bar-Rows
Check 'clicking the track scrolls' (($after | Measure-Object -Minimum).Minimum -gt (($before | Measure-Object -Minimum).Minimum + 100)) "before=$(($before|Measure-Object -Minimum).Minimum) after=$(($after|Measure-Object -Minimum).Minimum)"

# --- Alt+Enter, as Windows sends it for a real key press (WM_SYSKEYDOWN): fullscreen = exactly the monitor, no caption;
# a window = captioned and in the middle of the screen. Three trips, because the second one used to come back wrong.
$wa = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
function Alt-Enter { [void][In]::PostMessage($script:win, 0x0104, [IntPtr]0x0D, [IntPtr]0x201C0001); Start-Sleep -Milliseconds 1000 }
function Is-Full { [void][In]::GetWindowRect($script:win, [ref]$r); (($r.R - $r.L) -eq $bounds.Width) -and (($r.B - $r.T) -eq $bounds.Height) -and (([In]::GetWindowLong($script:win, -16) -band 0x00C00000) -eq 0) }
function Is-Window { [void][In]::GetWindowRect($script:win, [ref]$r); $cx = ($r.L + $r.R) / 2; $cy = ($r.T + $r.B) / 2
    (($r.R - $r.L) -lt $bounds.Width) -and (([In]::GetWindowLong($script:win, -16) -band 0x00C00000) -ne 0) -and ([math]::Abs($cx - ($wa.Left + $wa.Width / 2)) -lt 6) -and ([math]::Abs($cy - ($wa.Top + $wa.Height / 2)) -lt 6) }
function Rect-Text { [void][In]::GetWindowRect($script:win, [ref]$r); "rect $($r.L),$($r.T) $($r.R-$r.L)x$($r.B-$r.T) style 0x{0:X}" -f [In]::GetWindowLong($script:win, -16) }
Alt-Enter; Check 'Alt+Enter 1: fullscreen' (Is-Full) (Rect-Text)
Alt-Enter; Check 'Alt+Enter 2: a captioned window in the middle' (Is-Window) (Rect-Text)
Alt-Enter; Check 'Alt+Enter 3: fullscreen again' (Is-Full) (Rect-Text)
Alt-Enter; Check 'Alt+Enter 4: the window again, still in the middle' (Is-Window) (Rect-Text)
$gp.Kill(); Start-Sleep -Milliseconds 400
# NuViewer started on its own opens the gallery in the folder of last time; without one (first run / folder gone) in Pictures.
$leaf = Split-Path $root -Leaf
Set-Content $env:NUGALLERY_STATE "dir = $root`r`nview = thumbs`r`n" -Encoding ASCII
$vp0 = Start-Process (Join-Path $Dir 'nuviewer.exe') -ArgumentList @('--windowed') -PassThru
$g0 = Wait-Until { Get-Process nugallery -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge (Get-Date).AddSeconds(-8) -and $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -like '*NuGallery*' } | Select-Object -First 1 } 10000
Check 'nuviewer.exe alone opens the last folder' ($g0 -and $g0.MainWindowTitle -like "*$leaf*") ($g0.MainWindowTitle)
if ($g0) { $g0.Kill(); Start-Sleep -Milliseconds 400 }
$pics = Split-Path ([Environment]::GetFolderPath('MyPictures')) -Leaf
Set-Content $env:NUGALLERY_STATE "dir = Z:\no\such\folder`r`n" -Encoding ASCII
$vp0 = Start-Process (Join-Path $Dir 'nuviewer.exe') -ArgumentList @('--windowed') -PassThru
$g0 = Wait-Until { Get-Process nugallery -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge (Get-Date).AddSeconds(-8) -and $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -like '*NuGallery*' } | Select-Object -First 1 } 10000
Check 'a missing last folder falls back to Pictures' ($g0 -and $g0.MainWindowTitle -like "*$pics*") ($g0.MainWindowTitle)
if ($g0) { $g0.Kill(); Start-Sleep -Milliseconds 400 }
Remove-Item $env:NUGALLERY_STATE -ErrorAction SilentlyContinue
# NuViewer started on its own (no picture) opens NuGallery instead and exits.
$v = Join-Path $Dir 'nuviewer.exe'
$t1 = Get-Date
$vp = Start-Process $v -ArgumentList @('--windowed') -PassThru
$g2 = Wait-Until { Get-Process nugallery -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge $t1 -and $_.MainWindowHandle -ne 0 } | Select-Object -First 1 } 10000
Check 'nuviewer.exe with no picture opens NuGallery' ($null -ne $g2)
Start-Sleep -Milliseconds 500
$vp.Refresh(); Check 'and the viewer process exits' $vp.HasExited
if ($g2) { $g2.Kill() }
Write-Host "pass=$script:pass fail=$script:fail"
