# End-to-end test of: zoom keys 1-5 / ` / F / A, the left-edge viewer controls, rotation (+45 / -45, save, ask, undo), the
# 3D keys and the NuEditor mock-up. Messages in, window pixels out (see lib.ps1): it never touches your real mouse or
# keyboard, and every scenario reads its own settings file.
#   tools\test\e2e-session7.ps1 [-Exe path\to\nuviewer.exe]
param([string]$Exe)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $Exe) { $Exe = Join-Path $target 'release\nuviewer.exe' }
if (-not (Test-Path $Exe)) { throw "missing $Exe" }
$script:Exe = $Exe
. (Join-Path $PSScriptRoot 'lib.ps1')

$sandbox = Join-Path $env:TEMP "nv_s7_$([Diagnostics.Process]::GetCurrentProcess().Id)"
New-Item -ItemType Directory -Force $sandbox | Out-Null
$realSettings = $env:NUVIEWER_SETTINGS
$realLocal = $env:LOCALAPPDATA
$env:LOCALAPPDATA = Join-Path $sandbox 'local'          # backups of rewritten files go here, not into your profile
New-Item -ItemType Directory -Force $env:LOCALAPPDATA | Out-Null
$scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$cx = [int]($scr.Width / 2); $cy = [int]($scr.Height / 2)
function Sub-Dir([string]$name) { $d = Join-Path $sandbox $name; New-Item -ItemType Directory -Force $d | Out-Null; $d }
function Key-Mods([int]$vk, [int]$mods) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 400 }   # 1 ctrl, 2 shift, 4 alt
function Hover([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0200, [IntPtr]0, (Pack $x $y)); Start-Sleep -Milliseconds 250 }
function Signature { $b = Grab; $s = New-Object System.Text.StringBuilder; for ($y = 20; $y -lt $b.Height; $y += [int]($b.Height / 24)) { for ($x = 20; $x -lt $b.Width; $x += [int]($b.Width / 40)) { $c = $b.GetPixel($x, $y); [void]$s.Append("$($c.R / 8)$($c.G / 8)$($c.B / 8),") } }; $b.Dispose(); $s.ToString() }
function Img-Size([string]$path) { $i = [System.Drawing.Image]::FromFile($path); $r = @($i.Width, $i.Height); $i.Dispose(); $r }
# Centre of button $i of the left-edge bar (16 buttons), mirroring sidebar.rs.
function Bar-Button([int]$i) {
    $d = [In]::GetDpiForWindow($script:win) / 96.0
    $h = $scr.Height
    $btn = [math]::Min(44 * $d, ($h - 16 * $d - 12 * $d - 15 * 3 * $d) / 16)
    $total = 16 * $btn + 15 * 3 * $d + 12 * $d
    $y0 = ($h - $total) / 2
    @([int](6 * $d + 6 * $d + $btn / 2), [int]($y0 + 6 * $d + $i * ($btn + 3 * $d) + $btn / 2))
}
# Posted clicks don't move the real pointer, and Windows sends a mouse move at the REAL pointer position after a click, which
# closes the bar again; so open it at the edge and hover the button before every click.
function Bar-Click([int]$i) { $b = Bar-Button $i; Hover 2 $cy; Hover $b[0] $b[1]; Click $b[0] $b[1] }
function Dialog-Answer([int]$id, [string]$what) {
    $mb = Wait-Until { $m = [In]::FindWindow('#32770', 'NuViewer'); if ($m -ne [IntPtr]::Zero) { $m } } 5000
    Check "${what}: the question appears" ($null -ne $mb)
    if ($mb) { [void][In]::PostMessage($mb, 0x0111, [IntPtr]$id, [IntPtr]::Zero); Start-Sleep -Milliseconds 700 }   # WM_COMMAND IDYES 6 / IDNO 7 / IDCANCEL 2
}

try {
    # ============================================================================================
    Write-Host "`n[1] Zoom keys: 1-5 multiply the base view, ` resets, F / A pick the base"
    $d = Sub-Dir 'z'
    Make-Picture "$d\big.png" 4000 2000 40
    Use-Settings "$sandbox\z.ini" "canvas = 204060"
    Check 'viewer opens a 4000x2000 picture' (Open-Viewer "$d\big.png")
    $fit = (Green-Box $cx $cy 150)[0]
    Check "fitted: the 40 px marker is shown smaller  [$fit px]" (InRange $fit 10 38)
    Key 0x32
    $w2 = (Green-Box $cx $cy 250)[0]
    Check "2 = twice the fit  [$w2 vs $fit]" (InRange $w2 ($fit * 2 - 3) ($fit * 2 + 3))
    Key 0x35
    $w5 = (Green-Box $cx $cy 400)[0]
    Check "5 = five times the fit  [$w5 vs $fit]" (InRange $w5 ($fit * 5 - 5) ($fit * 5 + 5))
    Key 0xC0
    $wr = (Green-Box $cx $cy 150)[0]
    Check "` resets to the base view  [$wr]" (InRange $wr ($fit - 2) ($fit + 2))
    Key 0x41
    $w100 = (Green-Box $cx $cy 150)[0]
    Check "A = 100%: the marker is 40 px  [$w100]" (InRange $w100 38 42)
    Key 0x33
    $w3 = (Green-Box $cx $cy 250)[0]
    Check "with 100% selected, 3 = 300%: 120 px  [$w3]" (InRange $w3 117 123)
    Key 0x31
    Check '1 = back to 1x of 100%' (InRange (Green-Box $cx $cy 150)[0] 38 42)
    Key 0x46
    Check 'F = fit again' (InRange (Green-Box $cx $cy 150)[0] ($fit - 2) ($fit + 2))
    Key 0x34
    Check 'with fit selected again, 4 = four times the fit' (InRange (Green-Box $cx $cy 400)[0] ($fit * 4 - 5) ($fit * 4 + 5))
    Close-Viewer

    # ============================================================================================
    Write-Host "`n[2] Viewer controls on the left edge"
    $d = Sub-Dir 'bar'
    Make-Picture "$d\a.png" 400 300 20
    Make-Picture "$d\b.png" 400 300 20 ([System.Drawing.Color]::FromArgb(30, 30, 200))
    Make-Picture "$d\c.png" 400 300 20 ([System.Drawing.Color]::FromArgb(30, 200, 200))
    Use-Settings "$sandbox\bar.ini" "canvas = 204060"
    Check 'viewer opens b.png' (Open-Viewer "$d\b.png")
    $none = Signature
    $barPos = Bar-Button 0
    Click $barPos[0] $barPos[1]
    Check 'with the bar closed, a click where its button would be does not press it' ((Title) -like 'b.png*') (Title)
    Hover 2 $cy
    $open = Signature
    Check 'the pointer on the left edge opens the bar' ($open -ne $none)
    Bar-Click 0
    Check 'the Next button goes to c.png' ((Title) -like 'c.png*') (Title)
    Bar-Click 1
    Check 'the Previous button goes back to b.png' ((Title) -like 'b.png*') (Title)
    Hover ($scr.Width / 2) $cy
    Start-Sleep -Milliseconds 3600   # the toast fades
    Check 'the pointer leaving the bar closes it' ((Signature) -eq $none)
    Key 0x56
    Check 'V keeps the bar open without the pointer' ((Signature) -ne $none)
    Key 0x56
    Start-Sleep -Milliseconds 300
    Check 'V again closes it' ((Signature) -eq $none)
    # fit / 100% buttons toggle the base mode
    Hover 2 $cy
    Bar-Click 3
    Hover ($scr.Width / 2) $cy
    Start-Sleep -Milliseconds 300
    Check 'the 100% button: the 20 px marker is 20 px' (InRange (Green-Box $cx $cy 120)[0] 18 22)
    Key 0x32
    Check 'and 2 then doubles that to 40 px' (InRange (Green-Box $cx $cy 120)[0] 38 42)
    Close-Viewer

    # ============================================================================================
    Write-Host "`n[3] Rotation: +45 / -45, automatic save, undo"
    $d = Sub-Dir 'rot'
    Make-Picture "$d\a.png" 400 300 20
    Make-Picture "$d\b.png" 400 300 20
    Use-Settings "$sandbox\rot.ini" "canvas = 204060`nrotate_save = auto"
    Check 'viewer opens a.png' (Open-Viewer "$d\a.png")
    Key 0xDD
    $b45 = (Green-Box $cx $cy 150)
    Check "] turns it 45 degrees: the square's bounding box grows to about 28 px  [$($b45[0])x$($b45[1])]" ((InRange $b45[0] 26 31) -and (InRange $b45[1] 26 31))
    Key 0xDB
    Check '[ turns it back: 20 px again' (InRange (Green-Box $cx $cy 150)[0] 18 22)
    Key 0xDB
    Check '[ again: -45 degrees, also about 28 px' (InRange (Green-Box $cx $cy 150)[0] 26 31)
    Key 0xDD
    Key-Mods 0xDD 2
    $q = Green-Box $cx $cy 150
    Check 'Shift+] turns 90 degrees: a square looks the same, 20 px' (InRange $q[0] 18 22)
    Key-Mods 0xDB 2
    # real change then leave: auto save
    Key 0xDD
    Key 0x27
    Check 'moving on saves automatically and shows b.png' ((Title) -like 'b.png*') (Title)
    $sz = Img-Size "$d\a.png"
    Check "a.png was rewritten on a bigger canvas  [$($sz -join 'x')]" ((InRange $sz[0] 490 500) -and (InRange $sz[1] 490 500))
    $bak = Get-ChildItem (Join-Path $env:LOCALAPPDATA 'NuViewer\undo') -ErrorAction SilentlyContinue
    Check 'a backup of the original was kept' (@($bak).Count -ge 1)
    Key 0x25
    $a = Green-Box $cx $cy 150
    Check "back on a.png the saved turn is part of the picture: the marker is a diamond with a 28 px box  [$($a[0])]" (InRange $a[0] 26 31)
    Key-Mods 0x5A 1
    $sz = Img-Size "$d\a.png"
    Check "Ctrl+Z restores the original file  [$($sz -join 'x')]" (($sz[0] -eq 400) -and ($sz[1] -eq 300))
    Check 'and the restored picture is shown again (20 px marker)' (InRange (Green-Box $cx $cy 150)[0] 18 22)
    Close-Viewer

    Write-Host "`n[3b] Rotation: ask first (Cancel stays, No discards, Yes saves)"
    Use-Settings "$sandbox\rot2.ini" "canvas = 204060`nrotate_save = ask"
    Check 'viewer opens a.png' (Open-Viewer "$d\a.png")
    Key 0xDD
    Key 0x27 0
    Dialog-Answer 2 'Cancel'
    Check 'Cancel: still on a.png and still turned' (((Title) -like 'a.png*') -and (InRange (Green-Box $cx $cy 150)[0] 26 31)) (Title)
    Key 0x27 0
    Dialog-Answer 7 'No'
    Check 'No: moves on to b.png' ((Title) -like 'b.png*') (Title)
    $sz = Img-Size "$d\a.png"
    Check 'and a.png was not touched' (($sz[0] -eq 400) -and ($sz[1] -eq 300))
    Key 0x25
    Check 'back on a.png: the discarded turn is gone (20 px)' (InRange (Green-Box $cx $cy 150)[0] 18 22)
    Key 0xDD
    Key-Mods 0x53 1
    $sz = Img-Size "$d\a.png"
    Check "Ctrl+S saves without asking  [$($sz -join 'x')]" ((InRange $sz[0] 490 500) -and (InRange $sz[1] 490 500))
    Key 0x27 0
    Check 'afterwards nothing is pending: moving on does not ask' ((Wait-Until { (Title) -like 'b.png*' } 3000) -ne $null) (Title)
    Close-Viewer

    Write-Host "`n[3c] Closing a rotated picture asks"
    Use-Settings "$sandbox\rot3.ini" "canvas = 204060`nrotate_save = ask"
    Check 'viewer opens b.png' (Open-Viewer "$d\b.png")
    Key 0xDD
    Key 0x1B 0
    Dialog-Answer 2 'Esc on a rotated picture'
    Check 'Cancel keeps the viewer open' ($script:viewer -and -not $script:viewer.HasExited)
    Key 0x1B 0
    Dialog-Answer 7 'Esc again'
    $gone = Wait-Until { $script:viewer.Refresh(); $script:viewer.HasExited } 4000
    Check 'No quits without saving' ($null -ne $gone)
    Close-Viewer

    # ============================================================================================
    Write-Host "`n[4] 3D keys: wireframe, shadow, projection, texture are saved as settings and change the picture"
    $d = Sub-Dir 'm'
    # a cube (STL ascii)
    $v = @('0 0 0', '1 0 0', '1 1 0', '0 1 0', '0 0 1', '1 0 1', '1 1 1', '0 1 1')
    $quads = @(@(0, 3, 2, 1), @(4, 5, 6, 7), @(0, 1, 5, 4), @(2, 3, 7, 6), @(1, 2, 6, 5), @(0, 4, 7, 3))
    $sb = New-Object System.Text.StringBuilder; [void]$sb.AppendLine('solid cube')
    foreach ($qd in $quads) { foreach ($tri in @(@($qd[0], $qd[1], $qd[2]), @($qd[0], $qd[2], $qd[3]))) { [void]$sb.AppendLine('facet normal 0 0 0'); [void]$sb.AppendLine('outer loop'); foreach ($i in $tri) { [void]$sb.AppendLine("vertex $($v[$i])") }; [void]$sb.AppendLine('endloop'); [void]$sb.AppendLine('endfacet') } }
    [void]$sb.AppendLine('endsolid cube')
    Set-Content "$d\cube.stl" $sb.ToString() -Encoding ASCII
    $ini = "$sandbox\m.ini"
    Use-Settings $ini "canvas = 204060"
    Check 'STL opens' (Open-Viewer "$d\cube.stl")
    $base = Signature
    Key 0x57
    $wire = Signature
    Check 'W (wireframe) changes the picture' ($wire -ne $base)
    Check 'and is saved in the settings file' ((Get-Content $ini -Raw) -match 'model_wireframe = true')
    Key 0x57 400
    Start-Sleep -Milliseconds 3500   # the toast fades
    Check 'W again restores the picture' ((Signature) -eq $base)
    Key 0x4F
    Check 'O (orthographic) changes the picture' ((Signature) -ne $base)
    Check 'and is saved' ((Get-Content $ini -Raw) -match 'model_ortho = true')
    Key 0x4F
    Key-Mods 0x53 2
    Check 'Shift+S (shadow) changes the picture' ((Signature) -ne $base)
    Check 'and is saved' ((Get-Content $ini -Raw) -match 'model_shadow = true')
    Key-Mods 0x53 2
    Key-Mods 0x54 2
    Check 'Shift+T changes the shading' ((Signature) -ne $base)
    Check 'and is saved' ((Get-Content $ini -Raw) -match 'model_shading = diffuse')
    Key-Mods 0x54 2; Key-Mods 0x54 2; Key-Mods 0x54 2; Key-Mods 0x54 2
    Key 0x5A
    Check 'Z turns the model 90 degrees about Z' ((Get-Content $ini -Raw) -match 'model_turn_z = 1')
    Key-Mods 0x5A 2
    Check 'Shift+Z turns it back' ((Get-Content $ini -Raw) -match 'model_turn_z = 0')
    Key 0x54
    Check 'T (texture) switches off, saved' ((Get-Content $ini -Raw) -match 'model_texture = false')
    Key 0x4E
    Check 'N (normal map) switches off, saved' ((Get-Content $ini -Raw) -match 'model_normal_map = false')
    Close-Viewer

    # ============================================================================================
    Write-Host "`n[5] NuEditor mock-up"
    $d = Sub-Dir 'ed'
    Make-Picture "$d\a.png" 400 300 20
    Use-Settings "$sandbox\ed.ini" "canvas = 204060"
    Check 'viewer opens a.png' (Open-Viewer "$d\a.png")
    $before = Signature
    Key 0x45 1200
    Start-Sleep -Milliseconds 3500
    $edit = Signature
    Check 'E opens the editor (the left palette and the right inspector appear)' ($edit -ne $before)
    Key 0xDD
    Check 'the editor works on the picture: ] still rotates 45 degrees' (InRange (Green-Box $cx $cy 150)[0] 26 31)
    Key-Mods 0x5A 1
    Check 'Ctrl+Z takes the turn back' (InRange (Green-Box $cx $cy 150)[0] 18 22)
    Key 0x1B 800
    Start-Sleep -Milliseconds 3500
    Check 'Esc closes the editor' ((Signature) -eq $before)
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
