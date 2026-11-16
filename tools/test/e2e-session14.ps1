# End-to-end test of session 14: Western / Japanese reading, Dim, canvas (blur, picture), 360 view, double click -> gallery,
# the playback bar on a sound (waveform picture, seek track, volume), a real video if one is on this PC.
# Messages in, window pixels out (see lib.ps1): it never touches your real mouse or keyboard, and every scenario reads its
# own settings file.   tools\test\e2e-session14.ps1 [-Exe path\to\nuviewer.exe] [-Video path\to\some.mov]
param([string]$Exe, [string]$Video)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $Exe) { $Exe = Join-Path $target 'release\nuviewer.exe' }
if (-not (Test-Path $Exe)) { throw "missing $Exe" }
$script:Exe = $Exe
. (Join-Path $PSScriptRoot 'lib.ps1')

$sandbox = Join-Path $env:TEMP "nv_s14_$([Diagnostics.Process]::GetCurrentProcess().Id)"
New-Item -ItemType Directory -Force $sandbox | Out-Null
$realLocal = $env:LOCALAPPDATA
$env:LOCALAPPDATA = Join-Path $sandbox 'local'
New-Item -ItemType Directory -Force $env:LOCALAPPDATA | Out-Null
$scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$W = $scr.Width; $H = $scr.Height
function Sub-Dir([string]$name) { $d = Join-Path $sandbox $name; New-Item -ItemType Directory -Force $d | Out-Null; $d }
function Key-Mods([int]$vk, [int]$mods, [int]$wait = 450) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
function Hover([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0200, [IntPtr]0, (Pack $x $y)); Start-Sleep -Milliseconds 250 }
function Dpi { [In]::GetDpiForWindow($script:win) / 96.0 }
function Solid([string]$path, [int]$w, [int]$h, $c) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h; $g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear($c); $g.Dispose()
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
function Make-Wav([string]$path, [double]$seconds) {
    $rate = 44100; $n = [int]($rate * $seconds)
    $fs = [IO.File]::Create($path); $bw = New-Object IO.BinaryWriter $fs
    $bw.Write([Text.Encoding]::ASCII.GetBytes('RIFF')); $bw.Write([int](36 + $n * 2)); $bw.Write([Text.Encoding]::ASCII.GetBytes('WAVEfmt '))
    $bw.Write([int]16); $bw.Write([int16]1); $bw.Write([int16]1); $bw.Write([int]$rate); $bw.Write([int]($rate * 2)); $bw.Write([int16]2); $bw.Write([int16]16)
    $bw.Write([Text.Encoding]::ASCII.GetBytes('data')); $bw.Write([int]($n * 2))
    for ($i = 0; $i -lt $n; $i++) { $bw.Write([int16]([math]::Sin($i * 0.06) * 9000)) }
    $bw.Close(); $fs.Close()
}
function Is-Near($c, [int]$r, [int]$g, [int]$b, [int]$tol = 45) { ([math]::Abs($c.R - $r) -le $tol) -and ([math]::Abs($c.G - $g) -le $tol) -and ([math]::Abs($c.B - $b) -le $tol) }

try {
    # ================================================================================================
    Write-Host "`n[1] Dim: Shift+D darkens the picture by the setting, again restores it"
    $d = Sub-Dir 'dim'
    Solid "$d\a.png" 4000 3000 ([System.Drawing.Color]::FromArgb(200, 40, 40))
    Use-Settings "$sandbox\dim.ini" "canvas = 204060`ndim_pct = 50"
    Check 'viewer opens' (Open-Viewer "$d\a.png")
    $before = Pixel ([int]($W / 2)) ([int]($H / 2))
    Key-Mods 0x44 2 600
    $dim = Pixel ([int]($W / 2)) ([int]($H / 2))
    Check 'dimmed: red channel is about half' ($dim.R -lt $before.R * 0.75 -and $dim.R -gt $before.R * 0.25) "before $($before.R) after $($dim.R)"
    Key-Mods 0x44 2 600
    $after = Pixel ([int]($W / 2)) ([int]($H / 2))
    Check 'dim off restores it' ([math]::Abs($after.R - $before.R) -lt 12) "after $($after.R)"
    Close-Viewer

    # ================================================================================================
    Write-Host "`n[2] Canvas: blurred picture, and a picture file of the user's own"
    $d = Sub-Dir 'canvas'
    Solid "$d\small.png" 300 200 ([System.Drawing.Color]::FromArgb(220, 30, 30))
    Solid "$d\bg.png" 800 600 ([System.Drawing.Color]::FromArgb(30, 30, 230))
    Use-Settings "$sandbox\blur.ini" "canvas = 204060`ncanvas_mode = blur`ncanvas_shade = 20"
    Check 'viewer opens (blur)' (Open-Viewer "$d\small.png")
    $c = Pixel 12 12
    Check 'corner shows the blurred (red) picture, not the plain colour' ($c.R -gt 90 -and $c.R -gt $c.B + 40) "got $($c.R),$($c.G),$($c.B)"
    Close-Viewer
    Use-Settings "$sandbox\img.ini" "canvas = 204060`ncanvas_mode = image`ncanvas_shade = 0`ncanvas_image = $d\bg.png"
    Check 'viewer opens (picture)' (Open-Viewer "$d\small.png")
    Start-Sleep -Milliseconds 800
    $c = Pixel 12 12
    Check 'corner shows the chosen (blue) picture' ($c.B -gt 150 -and $c.R -lt 90) "got $($c.R),$($c.G),$($c.B)"
    Close-Viewer
    Use-Settings "$sandbox\solid.ini" "canvas = 204060"
    Check 'viewer opens (plain)' (Open-Viewer "$d\small.png")
    $c = Pixel 12 12
    Check 'plain canvas is the colour from the settings' (Is-Near $c 0x20 0x40 0x60 18) "got $($c.R),$($c.G),$($c.B)"
    Close-Viewer

    # ================================================================================================
    Write-Host "`n[3] Reading style: Western puts the first page left, Japanese puts it right (Ctrl+B)"
    $d = Sub-Dir 'read'
    Solid "$d\p1.png" 400 600 ([System.Drawing.Color]::FromArgb(220, 30, 30))
    Solid "$d\p2.png" 400 600 ([System.Drawing.Color]::FromArgb(30, 30, 220))
    Use-Settings "$sandbox\read.ini" "canvas = 204060`nread_start = 1"
    Check 'viewer opens (read)' (Open-Viewer "$d\p1.png")
    Key 0x42 700   # B
    $l = Pixel ([int]($W * 0.4)) ([int]($H / 2)); $r = Pixel ([int]($W * 0.6)) ([int]($H / 2))
    Check 'Western: red page on the left, blue on the right' ($l.R -gt 150 -and $r.B -gt 150) "L $($l.R),$($l.B)  R $($r.R),$($r.B)"
    Key-Mods 0x42 1 700   # Ctrl+B
    $l = Pixel ([int]($W * 0.4)) ([int]($H / 2)); $r = Pixel ([int]($W * 0.6)) ([int]($H / 2))
    Check 'Japanese: blue page on the left, red on the right' ($l.B -gt 150 -and $r.R -gt 150) "L $($l.R),$($l.B)  R $($r.R),$($r.B)"
    Key-Mods 0x42 1 400
    Close-Viewer

    # ================================================================================================
    Write-Host "`n[4] 360 view: Shift+P looks into an equirectangular picture, dragging turns it"
    $d = Sub-Dir 'pano'
    $bmp = New-Object System.Drawing.Bitmap 2000, 1000; $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.Clear([System.Drawing.Color]::FromArgb(220, 30, 30))
    $g.FillRectangle((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(30, 220, 30))), 900, 0, 200, 1000)
    $g.FillRectangle((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(30, 30, 220))), 1100, 0, 900, 1000)
    $g.Dispose(); $bmp.Save("$d\pano.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    Use-Settings "$sandbox\pano.ini" "canvas = 204060"
    Check 'viewer opens (pano)' (Open-Viewer "$d\pano.png")
    Key-Mods 0x50 2 900   # Shift+P
    $c = Pixel ([int]($W / 2)) ([int]($H / 2))
    Check '360: the middle of the window looks at the green band' ($c.G -gt 150 -and $c.R -lt 100) "got $($c.R),$($c.G),$($c.B)"
    Btn-Down ([int]($W / 2)) ([int]($H / 2)); Btn-Move ([int]($W * 0.9)) ([int]($H / 2)); Btn-Up ([int]($W * 0.9)) ([int]($H / 2))
    $c = Pixel ([int]($W / 2)) ([int]($H / 2))
    Check 'dragging right turns the view to the left: red now' ($c.R -gt 150 -and $c.B -lt 100) "got $($c.R),$($c.G),$($c.B)"
    Key-Mods 0x50 2 600
    $c = Pixel ([int]($W / 2)) ([int]($H / 2))
    Check 'Shift+P again leaves the 360 view (flat picture: the green band is only a sliver in the middle)' ($c.R -gt 100 -or $c.B -gt 100 -or $c.G -gt 100) ''
    Close-Viewer

    # ================================================================================================
    Write-Host "`n[5] Sound: the waveform is the picture, the bar plays it"
    $d = Sub-Dir 'sound'
    Make-Wav "$d\tone.wav" 6
    Use-Settings "$sandbox\snd.ini" "canvas = 204060`ncontrols_always = true`nvolume = 0"
    Check 'viewer opens a WAV' (Open-Viewer "$d\tone.wav")
    Start-Sleep -Milliseconds 1500
    $dpi = Dpi
    $barW = [math]::Min([math]::Max($W * 0.78, 520 * $dpi), 1100 * $dpi); $barW = [math]::Min($barW, $W - 24 * $dpi)
    $x0 = ($W - $barW) / 2; $y0 = $H - 48 * $dpi - 30 * $dpi
    $seekL = $x0 + 8 * $dpi + 3 * (34 * $dpi + 4 * $dpi) + 6 * $dpi
    $r = $x0 + $barW - 8 * $dpi - 78 * $dpi - 36 * $dpi - 36 * $dpi - 50 * $dpi - 124 * $dpi - 8 * $dpi
    $mid = [int]($y0 + 24 * $dpi)
    $bar = Pixel ([int]($x0 + 4)) ([int]($y0 + 4))
    Check 'the bar is up (dark panel)' ($bar.R -lt 60 -and $bar.G -lt 60) "got $($bar.R),$($bar.G),$($bar.B)"
    $played = Pixel ([int]($seekL + 6)) $mid; $rest = Pixel ([int]($r - 8)) $mid
    Check 'it is playing: the start of the track is the accent colour' ($played.B -gt 180 -and $played.R -lt 110) "got $($played.R),$($played.G),$($played.B)"
    Check 'the end of the track is still grey' ($rest.B -lt 120 -and [math]::Abs($rest.R - $rest.B) -lt 40) "got $($rest.R),$($rest.G),$($rest.B)"
    # Pause with Space, then seek to the middle by clicking the track.
    Key 0x20 500
    $mid_x = [int](($seekL + $r) / 2)
    Click $mid_x $mid
    Start-Sleep -Milliseconds 600
    $p1 = Pixel ([int]($seekL + ($r - $seekL) * 0.25)) $mid; $p2 = Pixel ([int]($seekL + ($r - $seekL) * 0.75)) $mid
    Check 'a click on the track seeks there (left of it filled, right of it grey)' ($p1.B -gt 180 -and $p2.B -lt 120) "left $($p1.R),$($p1.B) right $($p2.R),$($p2.B)"
    Close-Viewer

    # ================================================================================================
    Write-Host "`n[5b] Double click on the picture opens the gallery (and the viewer hands over)"
    $galleryExe = Join-Path (Split-Path $Exe) 'nugallery.exe'
    if (-not (Test-Path $galleryExe)) { Skip 'double click' 'nugallery.exe is not next to nuviewer.exe' }
    elseif (Get-Process nugallery -ErrorAction SilentlyContinue) { Skip 'double click' 'a NuGallery is already running (maybe yours); close it to run this check' }
    else {
        $d = Sub-Dir 'dbl'
        Solid "$d\a.png" 600 400 ([System.Drawing.Color]::FromArgb(200, 40, 40))
        Solid "$d\b.png" 600 400 ([System.Drawing.Color]::FromArgb(40, 40, 200))
        Use-Settings "$sandbox\dbl.ini" "canvas = 204060"
        Check 'viewer opens' (Open-Viewer "$d\a.png")
        $v = $script:viewer
        [void][In]::PostMessage($script:win, 0x0203, [IntPtr]1, (Pack ([int]($W / 2)) ([int]($H / 2))))
        $gone = Wait-Until { $v.Refresh(); $v.HasExited } 6000
        $g = Get-Process nugallery -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $galleryExe }
        Check 'the viewer handed over: it is gone and NuGallery is up' ($gone -and $null -ne $g) "exited=$gone gallery=$($null -ne $g)"
        $g | ForEach-Object { try { $_.Kill() } catch {} }
        $script:viewer = $null
    }

    # ================================================================================================
    Write-Host "`n[6] A real video (if there is one on this PC)"
    if (-not $Video) { $Video = Get-ChildItem 'F:\Photo' -Recurse -Filter *.mov -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty FullName }
    if (-not $Video -or -not (Test-Path $Video)) { Skip 'video' 'no .mov / .mp4 given or found' }
    else {
        Use-Settings "$sandbox\vid.ini" "canvas = 204060`ncontrols_always = true`nvolume = 0"
        Check 'viewer opens a video' (Open-Viewer $Video)
        Start-Sleep -Milliseconds 2500
        $dpi = Dpi
        $barW = [math]::Min([math]::Max($W * 0.78, 520 * $dpi), 1100 * $dpi); $barW = [math]::Min($barW, $W - 24 * $dpi)
        $x0 = ($W - $barW) / 2; $y0 = $H - 48 * $dpi - 30 * $dpi
        $seekL = $x0 + 8 * $dpi + 3 * (34 * $dpi + 4 * $dpi) + 6 * $dpi
        $mid = [int]($y0 + 24 * $dpi)
        $played = Pixel ([int]($seekL + 3)) $mid
        Check 'the video is playing: the first pixels of the track are filled' ($played.B -gt 180 -and $played.R -lt 110) "got $($played.R),$($played.G),$($played.B)"
        Close-Viewer
    }
}
finally {
    Close-Viewer
    $env:LOCALAPPDATA = $realLocal
    Remove-Item -Recurse -Force $sandbox -ErrorAction SilentlyContinue
}
Write-Host "`n$script:pass passed, $script:fail failed, $script:skip skipped"
if ($script:fail -gt 0) { exit 1 }
