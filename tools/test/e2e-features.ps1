# End-to-end test of the settings, key map, image sequences, playback bar, PDF pages, SVG, 3D models and
# the Settings window. Messages in, window pixels out (see lib.ps1): it never touches your real mouse or keyboard,
# and every scenario reads its own settings file, so your real settings are never read or changed.
#   tools\test\e2e-features.ps1 [-Exe path\to\nuviewer.exe]
param([string]$Exe)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $Exe) { $Exe = Join-Path $target 'release\nuviewer.exe' }
if (-not (Test-Path $Exe)) { throw "missing $Exe" }
$script:Exe = $Exe
. (Join-Path $PSScriptRoot 'lib.ps1')

$sandbox = Join-Path $env:TEMP "nv_feat_$([Diagnostics.Process]::GetCurrentProcess().Id)"
New-Item -ItemType Directory -Force $sandbox | Out-Null
$realSettings = $env:NUVIEWER_SETTINGS
$scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$cx = [int]($scr.Width / 2); $cy = [int]($scr.Height / 2)
function Sub-Dir([string]$name) { $d = Join-Path $sandbox $name; New-Item -ItemType Directory -Force $d | Out-Null; $d }
function Key-Mods([int]$vk, [int]$mods) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 350 }   # WM_NV_INJECT_KEY: 1 ctrl, 2 shift, 4 alt
# A real pointer is over the button before it is clicked; posted clicks don't move the pointer (Windows even reports the
# REAL pointer position after SetCapture), so say where it is first.
function Bar-Click([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0200, [IntPtr]0, (Pack $x $y)); Start-Sleep -Milliseconds 200; Click $x $y }
function Signature { $b = Grab; $s = New-Object System.Text.StringBuilder; for ($y = 20; $y -lt $b.Height; $y += [int]($b.Height / 24)) { for ($x = 20; $x -lt $b.Width; $x += [int]($b.Width / 40)) { $c = $b.GetPixel($x, $y); [void]$s.Append("$($c.R / 8)$($c.G / 8)$($c.B / 8),") } }; $b.Dispose(); $s.ToString() }

try {
    # ============================================================================================
    Write-Host "`n[1] Settings file: canvas colour, wheel action, hold zoom, key map"
    $d = Sub-Dir 's1'
    Make-Picture "$d\a.png" 400 300 20
    Make-Picture "$d\b.png" 4000 2000 40
    Make-Picture "$d\c.png" 1200 800 30
    Use-Settings "$sandbox\s1.ini" "canvas = 204060`nwheel = zoom`nhold_zoom = 400`nedge_click = true`nkey.next = N"
    Check 'viewer opens b.png with the settings file' (Open-Viewer "$d\b.png")
    $bar = Pixel $cx 20
    Check "canvas colour 204060 shows in the letterbox bars  [pixel $($bar.R),$($bar.G),$($bar.B)]" ((InRange $bar.R 29 35) -and (InRange $bar.G 61 67) -and (InRange $bar.B 93 99))
    Wheel 120 $cx $cy
    Check 'wheel = zoom: the wheel zooms and the picture stays b.png' (((Green-Box $cx $cy 150)[0] -gt 28) -and ((Title) -like 'b.png*'))
    Click $cx $cy
    Check 'a click on the zoomed picture goes back to fit' (InRange (Green-Box $cx $cy 150)[0] 22 30)
    Wheel -120 $cx $cy $true
    Check 'with wheel = zoom, Ctrl+wheel turns pages (next: c.png)' ((Title) -like 'c.png*') (Title)
    Wheel 120 $cx $cy $true
    Check 'Ctrl+wheel the other way: back to b.png' ((Title) -like 'b.png*') (Title)
    Btn-Down $cx $cy
    $z = Green-Box $cx $cy 220
    Check "hold zoom set to 400%: the 40 px marker is 160 px  [width $($z[0])]" (InRange $z[0] 150 170)
    Btn-Up $cx $cy
    Click 100 $cy
    Check 'a click near the left edge no longer turns the page (edge click was removed; an old edge_click line is ignored)' ((Title) -like 'b.png*') (Title)
    Click ($scr.Width - 100) $cy
    Check 'nor on the right edge' ((Title) -like 'b.png*') (Title)
    Key 0x4E
    Check 'key.next = N: N goes to the next picture (c.png)' ((Title) -like 'c.png*') (Title)
    Key 0x27
    Check 'the old Right key no longer does anything' ((Title) -like 'c.png*') (Title)
    Key 0x25
    Check 'other keys keep their meaning: Left goes back (b.png)' ((Title) -like 'b.png*') (Title)

    Write-Host "`n[2] Wheel and hold zoom switched off"
    Use-Settings "$sandbox\s2.ini" "wheel = off`nhold_zoom = off"
    Check 'viewer opens' (Open-Viewer "$d\b.png")
    Wheel -120 $cx $cy
    Check 'wheel = off: nothing happens' ((Title) -like 'b.png*' -and (InRange (Green-Box $cx $cy 150)[0] 22 30)) (Title)
    Btn-Down $cx $cy
    Check 'hold zoom = off: pressing does not zoom' (InRange (Green-Box $cx $cy 150)[0] 22 30)
    Btn-Up $cx $cy

    # ============================================================================================
    Write-Host "`n[3] PDF pages, SVG"
    $d = Sub-Dir 's3'
    # A 3-page PDF by hand: page 1 red, 2 green, 3 blue.
    $fills = '1 0 0 rg', '0 1 0 rg', '0 0 1 rg'
    $objs = @('<</Type/Catalog/Pages 2 0 R>>', '<</Type/Pages/Kids[3 0 R 4 0 R 5 0 R]/Count 3>>')
    1..3 | ForEach-Object { $objs += "<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]/Contents $(5 + $_) 0 R>>" }
    0..2 | ForEach-Object { $body = "$($fills[$_]) 0 0 1000 1000 re f"; $objs += "<</Length $($body.Length)>>`nstream`n$body`nendstream" }
    $sb = New-Object System.Text.StringBuilder; [void]$sb.Append("%PDF-1.4`n"); $offs = @()
    for ($i = 0; $i -lt $objs.Count; $i++) { $offs += $sb.Length; [void]$sb.Append("$($i + 1) 0 obj`n$($objs[$i])`nendobj`n") }
    $xref = $sb.Length; [void]$sb.Append("xref`n0 $($objs.Count + 1)`n0000000000 65535 f `n")
    foreach ($o in $offs) { [void]$sb.Append(("{0:D10} 00000 n `n" -f $o)) }
    [void]$sb.Append("trailer`n<</Size $($objs.Count + 1)/Root 1 0 R>>`nstartxref`n$xref`n%%EOF`n")
    [IO.File]::WriteAllBytes("$d\doc.pdf", [Text.Encoding]::ASCII.GetBytes($sb.ToString()))
    Set-Content "$d\logo.svg" '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" viewBox="0 0 40 20"><rect width="40" height="20" fill="#0000ff"/><rect x="10" y="5" width="20" height="10" fill="#ff0000"/></svg>' -Encoding ASCII
    Make-Picture "$d\z.png" 200 100 0
    Use-Settings "$sandbox\s3.ini" ""
    Check 'PDF opens' (Open-Viewer "$d\doc.pdf")
    $c = Pixel $cx $cy
    Check "title says page 1/3 and the page is red  [$(Title)]" (((Title) -like '*page 1/3*') -and ($c.R -gt 200) -and ($c.G -lt 60))
    Wheel -120 $cx $cy
    [void](Wait-Until { (Title) -like '*page 2/3*' } 4000); Start-Sleep -Milliseconds 300
    $c = Pixel $cx $cy
    Check 'wheel down turns to page 2 (green)' (((Title) -like '*page 2/3*') -and ($c.G -gt 200) -and ($c.R -lt 60)) (Title)
    Key 0x23
    [void](Wait-Until { (Title) -like '*page 3/3*' } 4000); Start-Sleep -Milliseconds 300
    $c = Pixel $cx $cy
    Check 'End goes to the last page (blue)' (((Title) -like '*page 3/3*') -and ($c.B -gt 200) -and ($c.R -lt 60)) (Title)
    Wheel -120 $cx $cy
    [void](Wait-Until { (Title) -like 'logo.svg*' } 4000); Start-Sleep -Milliseconds 300
    Check 'wheel down past the last page continues to the next file (logo.svg)' ((Title) -like 'logo.svg*') (Title)
    $c = Pixel $cx $cy
    Check "SVG is drawn (red rectangle on blue)  [centre $($c.R),$($c.G),$($c.B)]" (($c.R -gt 200) -and ($c.B -lt 60))
    Wheel 120 $cx $cy
    [void](Wait-Until { (Title) -like 'doc.pdf*' } 4000)
    Check 'wheel up from the next file returns to the document' ((Title) -like 'doc.pdf*') (Title)
    Key 0x24
    Key-Mods 0x27 1
    [void](Wait-Until { (Title) -like 'logo.svg*' } 4000)
    Check 'Ctrl+Right skips the pages: straight to the next file' ((Title) -like 'logo.svg*') (Title)
    Key-Mods 0x25 1
    [void](Wait-Until { (Title) -like 'doc.pdf*' } 4000)
    Check 'Ctrl+Left goes back to the document' ((Title) -like 'doc.pdf*') (Title)

    # ============================================================================================
    Write-Host "`n[4] 3D model"
    $d = Sub-Dir 's4'
    $ms = New-Object IO.MemoryStream; $bw = New-Object IO.BinaryWriter $ms
    $bw.Write([byte[]]::new(80)); $bw.Write([uint32]12)
    function W3([single[]]$f) { foreach ($x in $f) { $bw.Write([single]$x) } }
    $v = @(@(0,0,0), @(1,0,0), @(1,1,0), @(0,1,0), @(0,0,1), @(1,0,1), @(1,1,1), @(0,1,1))
    $quads = @(@(0,3,2,1), @(4,5,6,7), @(0,1,5,4), @(2,3,7,6), @(1,2,6,5), @(0,4,7,3))
    foreach ($q in $quads) { foreach ($t in @(@($q[0],$q[1],$q[2]), @($q[0],$q[2],$q[3]))) { W3 @(0,0,0); foreach ($i in $t) { W3 $v[$i] }; $bw.Write([uint16]0) } }
    [IO.File]::WriteAllBytes("$d\cube.stl", $ms.ToArray())
    Use-Settings "$sandbox\s4.ini" "canvas = 000000"
    Check 'STL opens' (Open-Viewer "$d\cube.stl")
    Check "title says 3D with 12 triangles  [$(Title)]" ((Title) -like '*3D, 12 triangles*')
    $c = Pixel $cx $cy
    Check "the model is drawn (centre isn't the canvas)  [$($c.R),$($c.G),$($c.B)]" (($c.R + $c.G + $c.B) -gt 150)
    $sig0 = Signature
    Btn-Down $cx $cy; Btn-Move ($cx + 120) ($cy + 40); Btn-Move ($cx + 260) ($cy + 80); Btn-Up ($cx + 260) ($cy + 80)
    $sig1 = Signature
    Check 'dragging turns the model' ($sig0 -ne $sig1)
    $count = { $b = Grab; $n = 0; for ($y = 0; $y -lt $b.Height; $y += 4) { for ($x = 0; $x -lt $b.Width; $x += 4) { if ($b.GetPixel($x, $y).R -gt 40) { $n++ } } }; $b.Dispose(); $n }
    $n0 = & $count
    Wheel 120 $cx $cy; Wheel 120 $cx $cy; Wheel 120 $cx $cy
    $n1 = & $count
    Check "the wheel zooms the model ($n0 -> $n1 lit samples)" ($n1 -gt $n0 * 1.2)
    Key 0x46
    Check 'F puts the camera back' ((Signature) -eq $sig0)

    # ============================================================================================
    Write-Host "`n[5] Image sequences in the gallery and the playback bar"
    $d = Sub-Dir 's5'
    Make-Picture "$d\other.png" 64 64 0 ([System.Drawing.Color]::FromArgb(60, 60, 60))
    1..12 | ForEach-Object { Make-Picture ("$d\seq_{0:D4}.png" -f $_) 64 64 0 ([System.Drawing.Color]::FromArgb(20 * $_, 30, 200 - 10 * $_)) }
    Use-Settings "$sandbox\s5.ini" "sequences = true`nsequence_min = 5`nfps = 30`ncanvas = 808080"
    Check 'folder opens' (Open-Viewer "$d\other.png")
    Key 0x09
    Key 0x23
    Check "gallery folds the run into one tile  [$(Title)]" (((Title) -like '*sequence, 12 frames*') -and ((Title) -like '*2/2*'))
    Key 0x0D 700
    Check 'Enter opens the first frame' ((Title) -like 'seq_0001.png*') (Title)
    $dpi = [In]::GetDpiForWindow($script:win) / 96.0
    $w = $scr.Width; $h = $scr.Height
    $barW = [math]::Min([math]::Max($w * 0.7, 460 * $dpi), 920 * $dpi); $barW = [math]::Min($barW, $w - 24 * $dpi)
    $x0 = ($w - $barW) / 2; $y0 = $h - 48 * $dpi - 30 * $dpi; $btn = 34 * $dpi; $by = $y0 + (48 * $dpi - $btn) / 2
    $playX = [int]($x0 + 8 * $dpi + 38 * $dpi + $btn / 2); $nextX = [int]($x0 + 8 * $dpi + 76 * $dpi + $btn / 2); $btnY = [int]($by + $btn / 2)
    $right = $x0 + $barW - 8 * $dpi; $fasterL = $right - 30 * $dpi; $slowerL = $fasterL - 30 * $dpi - 64 * $dpi
    $seekL = $x0 + 8 * $dpi + 3 * 38 * $dpi + 6 * $dpi; $seekR = $slowerL - 96 * $dpi - 8 * $dpi
    $before = Pixel $cx ([int]($y0 + 24 * $dpi))
    [void][In]::PostMessage($script:win, 0x0200, [IntPtr]0, (Pack $cx ([int]($h - 60 * $dpi)))); Start-Sleep -Milliseconds 400
    $after = Pixel $cx ([int]($y0 + 8 * $dpi))
    Check "the playback bar appears when the pointer is near the bottom (grey $($before.R) -> $($after.R))" (($before.R -gt 100) -and ($after.R -lt 60))
    Bar-Click $nextX $btnY
    Check "the bar's next-frame button steps one frame" ((Title) -like 'seq_0002.png*') (Title)
    Bar-Click $playX $btnY; Start-Sleep -Milliseconds 700
    $t1 = Title; Start-Sleep -Milliseconds 300; $t2 = Title
    Check "play button: the sequence plays ($t1 / $t2)" (($t1 -notlike 'seq_0002.png*') -or ($t2 -ne $t1))
    Bar-Click $playX $btnY; Start-Sleep -Milliseconds 200; $t3 = Title; Start-Sleep -Milliseconds 500; $t4 = Title
    Check 'play button again: it stops' ($t3 -eq $t4) "$t3 / $t4"
    Bar-Click ([int](($seekL + $seekR) / 2)) ([int]($y0 + 24 * $dpi))
    Check "seek bar: the middle is frame 6-8  [$(Title)]" ((Title) -match 'seq_000[678]\.png|seq_0006')
    Key 0x50
    Start-Sleep -Milliseconds 500; $p1 = Title; Start-Sleep -Milliseconds 300; $p2 = Title
    Check 'P key plays too' ($p1 -ne $p2) "$p1 / $p2"
    Key 0x50; Start-Sleep -Milliseconds 200
    Key 0x51
    Check 'Q unfolds the sequence setting (written to the settings file)' ((Get-Content "$sandbox\s5.ini" -Raw) -match 'sequences = false')

    # ============================================================================================
    Write-Host "`n[6] Animated GIF: play / pause"
    $d = Sub-Dir 's6'
    [IO.File]::WriteAllBytes("$d\anim.gif", [Convert]::FromBase64String('R0lGODlhAgACAIAAAP8AAAD/ACH/C05FVFNDQVBFMi4wAwEAAAAh+QQACgAAACwAAAAAAgACAAACBARBEAUAIfkEAAoAAAAsAAAAAAIAAgAAAgQMwzAFADs='))
    Use-Settings "$sandbox\s6.ini" ""
    $opened = Open-Viewer "$d\anim.gif"
    Check "GIF opens  [$(Title)]" ($opened -and ((Title) -notlike '*cannot open*'))
    $seen = { param($ms) $r = $false; $g = $false; $sw = [Diagnostics.Stopwatch]::StartNew(); while ($sw.ElapsedMilliseconds -lt $ms) { $c = Pixel $cx $cy; if ($c.R -gt 180 -and $c.G -lt 80) { $r = $true }; if ($c.G -gt 180 -and $c.R -lt 80) { $g = $true } }; @($r, $g) }
    $s = & $seen 1500
    Check "the animation plays (red $($s[0]), green $($s[1]))" ($s[0] -and $s[1])
    Key 0x50
    $s = & $seen 1200
    Check "P pauses it (only one frame seen: red $($s[0]), green $($s[1]))" ($s[0] -xor $s[1])
    Key 0x50
    $s = & $seen 1500
    Check 'P resumes it' ($s[0] -and $s[1])

    # ============================================================================================
    Write-Host "`n[7] Context menu: Open with, Settings"
    $d = Sub-Dir 's7'
    Make-Picture "$d\a.png" 100 100 0
    Use-Settings "$sandbox\s7.ini" "external.0.name = Mark it`nexternal.0.command = cmd.exe /c echo {file}"
    Check 'viewer opens' (Open-Viewer "$d\a.png")
    [void][In]::PostMessage($script:win, 0x0205, [IntPtr]0, (Pack $cx $cy))
    $menu = Wait-Until { $m = [In]::FindWindow('#32768', $null); if ($m -ne [IntPtr]::Zero) { $m } } 3000
    Check 'context menu opens' ($null -ne $menu)
    if ($menu) {
        $hm = [In]::SendMessage($menu, 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero)
        $n = [In]::GetMenuItemCount($hm)
        $texts = @(0..($n - 1) | ForEach-Object { [In]::MenuText($hm, $_) })
        Check "menu has 31 entries  [$n]" ($n -eq 31)
        Check 'Settings… and Fold image sequences are in the menu' ((@($texts | Where-Object { $_ -like 'Settings*' }).Count -eq 1) -and (@($texts | Where-Object { $_ -like 'Fold image sequences*' }).Count -eq 1))
        $ow = [array]::IndexOf(@($texts | ForEach-Object { $_ -like 'Open with*' }), $true)
        $sub = [In]::GetSubMenu($hm, $ow)
        Check "the Open with submenu lists the external program 'Mark it'" (($sub -ne [IntPtr]::Zero) -and ([In]::MenuText($sub, 0) -eq 'Mark it'))
        [void][In]::PostMessage($menu, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero); Start-Sleep -Milliseconds 300
    }
    Close-Viewer

    # ============================================================================================
    Write-Host "`n[8] Settings window"
    $ini = "$sandbox\s8.ini"
    Use-Settings $ini "fps = 24"
    $sp = Start-Process $Exe -ArgumentList '--settings' -PassThru
    $sw = Wait-Until { $sp.Refresh(); if ($sp.MainWindowHandle -ne 0 -and $sp.MainWindowTitle -like '*Settings*') { $sp.MainWindowHandle } } 10000
    Check 'Settings window opens' ($null -ne $sw)
    if ($sw) {
        $kids = [In]::Children($sw)
        Check 'four pages: Viewer, File types, External programs, Mouse & keyboard' ((@($kids | Where-Object { $_.Text -in 'Viewer', 'File types', 'External programs', 'Mouse && keyboard' }).Count) -eq 4)
        $tab4 = $kids | Where-Object { $_.Text -eq 'Mouse && keyboard' }
        [void][In]::SendMessage($tab4.H, 0xF5, [IntPtr]::Zero, [IntPtr]::Zero)   # BM_CLICK
        Start-Sleep -Milliseconds 300
        $list = ([In]::Children($sw) | Where-Object { $_.Id -eq 410 }).H
        $cnt = [In]::SendMessage($list, 0x018B, [IntPtr]::Zero, [IntPtr]::Zero)
        Check "keyboard list holds all 65 actions  [$cnt]" ($cnt -eq 65)
        [void][In]::SendMessage($list, 0x0186, [IntPtr]0, [IntPtr]::Zero)   # LB_SETCURSEL 0
        [void][In]::SendMessage($sw, 0x0111, [IntPtr](410 -bor (1 -shl 16)), $list)   # WM_COMMAND, LBN_SELCHANGE
        $edit = ([In]::Children($sw) | Where-Object { $_.Id -eq 411 }).H
        $sbuf = New-Object System.Text.StringBuilder 256; [void][In]::SendMessage($edit, 0x000D, [IntPtr]256, $sbuf)
        Check "selecting 'Next picture' shows its keys  [$sbuf]" ($sbuf.ToString() -eq 'Right, Space')
        [void][In]::SendMessage($edit, 0x000C, [IntPtr]::Zero, 'K, Space')   # WM_SETTEXT
        $setBtn = ([In]::Children($sw) | Where-Object { $_.Id -eq 412 }).H
        [void][In]::SendMessage($setBtn, 0xF5, [IntPtr]::Zero, [IntPtr]::Zero)
        Check "Set updates the list row  [$([In]::ListText($list, 0))]" ([In]::ListText($list, 0) -like '*K, Space*')
        [void][In]::SendMessage($edit, 0x000C, [IntPtr]::Zero, 'Left')       # Left already belongs to "Previous"
        [void][In]::PostMessage($setBtn, 0xF5, [IntPtr]::Zero, [IntPtr]::Zero)
        $mb = Wait-Until { $m = [In]::FindWindow('#32770', 'NuViewer'); if ($m -ne [IntPtr]::Zero) { $m } } 5000
        Check 'a key that another action owns is refused with a message' ($null -ne $mb)
        if ($mb) { [void][In]::PostMessage($mb, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero); Start-Sleep -Milliseconds 300 }
        Check 'and the row is unchanged' ([In]::ListText($list, 0) -like '*K, Space*')
        $edge = ([In]::Children($sw) | Where-Object { $_.Id -eq 113 }).H   # "viewer controls on the left edge" (on by default)
        [void][In]::PostMessage($edge, 0xF5, [IntPtr]::Zero, [IntPtr]::Zero); Start-Sleep -Milliseconds 300
        $ok = ([In]::Children($sw) | Where-Object { $_.Id -eq 1 -and $_.Cls -eq 'Button' }).H
        [void][In]::PostMessage($ok, 0xF5, [IntPtr]::Zero, [IntPtr]::Zero)
        $done = Wait-Until { $sp.HasExited } 5000
        Check 'OK closes the window' ($null -ne $done)
        $text = Get-Content $ini -Raw
        Check 'the keys were saved to the settings file' ($text -match 'key\.next = K, Space') $text
        Check 'the left-edge controls setting was saved' ($text -match 'sidebar = false')
        Check 'untouched settings stay as they were' ($text -match 'fps = 24')
    }
    if ($sp -and -not $sp.HasExited) { $sp.Kill() }
}
finally {
    Close-Viewer
    $env:NUVIEWER_SETTINGS = $realSettings
    Remove-Item $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host ("`n{0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip) -ForegroundColor $(if ($script:fail) { 'Red' } else { 'Green' })
exit $(if ($script:fail) { 1 } else { 0 })
