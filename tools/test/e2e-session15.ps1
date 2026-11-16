# End-to-end test of version 0.2.0: quick convert (command line and queue), frame stepping, video snapshot, the back / close
# buttons of the menu bar, the Settings sidebar and its new pages, the language file, JSON and 3D files in the viewer, and
# NuImporter (clipboard, a PowerPoint, a screenshot, a watched folder).
# Messages in, window pixels out (see lib.ps1); every scenario reads its own settings file and its own %LOCALAPPDATA%.
#   tools\test\e2e-session15.ps1 [-Exe path\to\nuviewer.exe]
param([string]$Exe)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $Exe) { $Exe = Join-Path $target 'release\nuviewer.exe' }
if (-not (Test-Path $Exe)) { throw "missing $Exe" }
$script:Exe = $Exe
$bin = Split-Path $Exe
. (Join-Path $PSScriptRoot 'lib.ps1')

$sandbox = Join-Path $env:TEMP "nv_s15_$([Diagnostics.Process]::GetCurrentProcess().Id)"
New-Item -ItemType Directory -Force $sandbox | Out-Null
$realLocal = $env:LOCALAPPDATA; $realApp = $env:APPDATA
$env:LOCALAPPDATA = Join-Path $sandbox 'local'; $env:APPDATA = Join-Path $sandbox 'roaming'
New-Item -ItemType Directory -Force $env:LOCALAPPDATA, $env:APPDATA | Out-Null
$scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$ScrW = $scr.Width; $ScrH = $scr.Height
function Sub-Dir([string]$name) { $d = Join-Path $sandbox $name; New-Item -ItemType Directory -Force $d | Out-Null; $d }
function Key-Mods([int]$vk, [int]$mods, [int]$wait = 450) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
function Solid([string]$path, [int]$w, [int]$h, $c, $fmt = [System.Drawing.Imaging.ImageFormat]::Png) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h; $g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear($c); $g.Dispose()
    $bmp.Save($path, $fmt); $bmp.Dispose()
}
function Hover([int]$x, [int]$y) { [void][In]::PostMessage($script:win, 0x0200, [IntPtr]0, (Pack $x $y)); Start-Sleep -Milliseconds 250 }
function Window-Of([System.Diagnostics.Process]$p, [string]$like = '*', [int]$ms = 10000) {
    Wait-Until { $p.Refresh(); if ($p.MainWindowHandle -ne 0 -and $p.MainWindowTitle -like $like) { $p.MainWindowHandle } } $ms
}
function Hash-Of([IntPtr]$h) {
    $r = New-Object In+RECT; [void][In]::GetWindowRect($h, [ref]$r)
    $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
    $g = [System.Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc(); [void][In]::PrintWindow($h, $dc, 2); $g.ReleaseHdc($dc); $g.Dispose()
    $ms = New-Object IO.MemoryStream; $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Bmp); $bmp.Dispose()
    $sha = [Security.Cryptography.SHA1]::Create(); [BitConverter]::ToString($sha.ComputeHash($ms.ToArray()))
}
$ffmpeg = (Get-Command ffmpeg -ErrorAction SilentlyContinue).Source
$started = @()

try {
    # ================================================================================================
    Write-Host "`n[1] Quick convert from the command line (what Explorer's right-click entries run)"
    $d = Sub-Dir 'convert'
    $env:NUVIEWER_QUEUE_DIR = Join-Path $d 'queue'
    Solid "$d\a.png" 64 48 ([System.Drawing.Color]::FromArgb(20, 180, 90))
    Solid "$d\b.jpg" 64 48 ([System.Drawing.Color]::FromArgb(180, 40, 40)) ([System.Drawing.Imaging.ImageFormat]::Jpeg)
    (Get-Item "$d\b.jpg").LastWriteTime = [datetime]'2020-05-06 07:08:09'
    $p = Start-Process $Exe -ArgumentList "--to-jpg", "`"$d\a.png`"" -PassThru -Wait
    Check 'png -> jpg made a.jpg next to it' (Test-Path "$d\a.jpg")
    $bytes = [IO.File]::ReadAllBytes("$d\a.jpg")
    Check 'it is a real JPEG' ($bytes[0] -eq 0xFF -and $bytes[1] -eq 0xD8)
    $img = [System.Drawing.Image]::FromFile("$d\a.jpg"); Check 'same size' ($img.Width -eq 64 -and $img.Height -eq 48); $img.Dispose()
    $p = Start-Process $Exe -ArgumentList "--to-png", "`"$d\b.jpg`"" -PassThru -Wait
    Check 'jpg -> png made b.png' (Test-Path "$d\b.png")
    $bytes = [IO.File]::ReadAllBytes("$d\b.png"); Check 'it is a real PNG' ($bytes[0] -eq 0x89 -and $bytes[1] -eq 0x50)
    Check 'the date of the original is kept' ((Get-Item "$d\b.png").LastWriteTime.Year -eq 2020)
    Start-Process $Exe -ArgumentList "--to-png", "`"$d\b.jpg`"" -Wait
    Check 'a second conversion never overwrites: b (2).png' (Test-Path "$d\b (2).png")
    # several files at once: one process each, as Explorer does it
    1..5 | ForEach-Object { Solid "$d\m$_.png" 32 32 ([System.Drawing.Color]::FromArgb(10 * $_, 60, 60)) }
    $procs = 1..5 | ForEach-Object { Start-Process $Exe -ArgumentList "--to-jpg", "`"$d\m$_.png`"" -PassThru }
    $procs | ForEach-Object { [void]$_.WaitForExit(20000) }
    $made = (1..5 | Where-Object { Test-Path "$d\m$_.jpg" }).Count
    Check 'five files started together are all converted' ($made -eq 5) "made $made"
    Remove-Item Env:\NUVIEWER_QUEUE_DIR

    # ================================================================================================
    Write-Host "`n[2] A video: one frame forward / back (exact), and a snapshot"
    if (-not $ffmpeg) { Skip 'video tests' 'ffmpeg is not installed' }
    else {
        $d = Sub-Dir 'video'
        & $ffmpeg -v error -y -f lavfi -i "testsrc2=duration=3:size=320x240:rate=25" -pix_fmt yuv420p -c:v libx264 -g 5 "$d\t.mp4"
        Check 'test video made' (Test-Path "$d\t.mp4")
        $shots = Sub-Dir 'shots'
        Use-Settings "$sandbox\video.ini" "autoplay = false`nsnapshot_dir = $shots`nsnapshot_format = png`nsnapshot_name = frame {time}"
        Check 'viewer opens the video' (Open-Viewer "$d\t.mp4")
        Start-Sleep -Milliseconds 900
        $a0 = Hash-Of $script:win
        Key 0xBE 700; $a1 = Hash-Of $script:win
        Key 0xBE 700; $a2 = Hash-Of $script:win
        Check '"." shows the next frame (picture changes)' ($a1 -ne $a0 -and $a2 -ne $a1)
        Key 0xBC 900; $b1 = Hash-Of $script:win
        Check '"," goes back exactly one frame (identical picture)' ($b1 -eq $a1)
        Key 0xBC 900; $b0 = Hash-Of $script:win
        Check '"," again is exactly the first frame' ($b0 -eq $a0)
        Key 0xBC 900; $b00 = Hash-Of $script:win
        Check 'at the first frame "," stays' ($b00 -eq $a0)
        Key 0xBE 600; Key 0xBE 600; Key 0xBE 600
        Key-Mods 0x53 3 2500   # Ctrl+Shift+S
        $png = Get-ChildItem $shots -Filter 'frame*.png' -ErrorAction SilentlyContinue | Select-Object -First 1
        Check 'Ctrl+Shift+S saved a frame as PNG' ($null -ne $png)
        if ($png) {
            $im = [System.Drawing.Image]::FromFile($png.FullName); Check 'the snapshot is the full 320x240 frame' ($im.Width -eq 320 -and $im.Height -eq 240) "$($im.Width)x$($im.Height)"; $im.Dispose()
            Check 'its name carries the time of the frame (3 frames in = 120 ms)' ($png.Name -like '*00m00s120*') $png.Name
        }
        Close-Viewer
    }

    # ================================================================================================
    Write-Host "`n[3] The back and close buttons at the ends of the menu bar"
    $d = Sub-Dir 'nav'
    Solid "$d\a.png" 800 600 ([System.Drawing.Color]::FromArgb(40, 60, 200))
    Use-Settings "$sandbox\nav.ini" "canvas = 101010"
    Check 'viewer opens' (Open-Viewer "$d\a.png")
    Hover 200 1; Hover 200 2
    $w = $script:viewer.MainWindowHandle
    Click ($ScrW - 12) 8
    $gone = Wait-Until { $script:viewer.Refresh(); $script:viewer.HasExited } 4000
    Check 'the x button at the right end closes the viewer' ($true -eq $gone)
    Close-Viewer
    Check 'viewer opens again' (Open-Viewer "$d\a.png")
    Hover 200 1; Hover 20 2
    $before = @(Get-Process nugallery -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
    Click 14 8
    $gone = Wait-Until { $script:viewer.Refresh(); $script:viewer.HasExited } 4000
    Check 'the < button leaves the viewer' ($true -eq $gone)
    $g = Wait-Until { Get-Process nugallery -ErrorAction SilentlyContinue | Where-Object { $before -notcontains $_.Id } | Select-Object -First 1 } 8000
    Check 'and brings up the gallery' ($null -ne $g)
    if ($g) { $started += $g }
    Close-Viewer
    if ($g) { try { $g.Kill() } catch {} }

    # ================================================================================================
    Write-Host "`n[4] JSON and new 3D formats in the viewer"
    $d = Sub-Dir 'formats'
    Set-Content "$d\data.json" '{"name":"NuViewer","list":[1,2,3],"nested":{"ok":true,"none":null}}' -Encoding UTF8
    Check 'a JSON file opens (coloured page)' (Open-Viewer "$d\data.json")
    $c = Pixel ([int]($ScrW / 2)) ([int]($ScrH / 3))
    Check 'the page is dark (code theme)' ($c.R -lt 80 -and $c.G -lt 80)
    Close-Viewer
    Set-Content "$d\tri.dxf" "0`nSECTION`n2`nENTITIES`n0`n3DFACE`n10`n0`n20`n0`n30`n0`n11`n1`n21`n0`n31`n0`n12`n1`n22`n1`n32`n0`n13`n0`n23`n1`n33`n0`n0`nENDSEC`n0`nEOF`n" -Encoding ASCII
    Check 'a DXF with a 3D face opens as a 3D model' (Open-Viewer "$d\tri.dxf")
    Close-Viewer
    Set-Content "$d\scene.c4d" 'not a real Cinema 4D file' -Encoding ASCII
    Check 'a closed-format scene opens with its message (no crash)' (Open-Viewer "$d\scene.c4d")
    Close-Viewer

    # ================================================================================================
    Write-Host "`n[5] The Settings window: sidebar, new pages, language"
    $env:NUVIEWER_SETTINGS = Join-Path $sandbox 'set.ini'
    Set-Content $env:NUVIEWER_SETTINGS 'language = en' -Encoding ASCII
    $p = Start-Process $Exe -ArgumentList '--settings' -PassThru
    $w = Window-Of $p '*Settings*' 8000
    Check 'the Settings window opens' ($null -ne $w)
    if ($w) {
        $kids = [In]::Children($w)
        $side = @($kids | Where-Object { $_.Cls -eq 'Button' -and (($_.Style -band 0xF) -eq 0x0B) })
        Check 'the sidebar has its 14 owner-drawn entries' ($side.Count -ge 14) "found $($side.Count)"
        [void][In]::PostMessage($w, 0x0111, [IntPtr]21, [IntPtr]::Zero); Start-Sleep -Milliseconds 400   # Video snapshot
        $combo = @([In]::Children($w) | Where-Object { $_.Id -eq 1201 })
        Check 'the Video snapshot page has its format box' ($combo.Count -eq 1 -and (([In]::GetWindowLong($combo[0].H, -16)) -band 0x10000000) -ne 0)
        [void][In]::PostMessage($w, 0x0111, [IntPtr]22, [IntPtr]::Zero); Start-Sleep -Milliseconds 400   # Import
        $after = @([In]::Children($w) | Where-Object { $_.Id -eq 1301 })
        Check 'the Import page has its "then" box' ($after.Count -eq 1 -and (([In]::GetWindowLong($after[0].H, -16)) -band 0x10000000) -ne 0)
        [void][In]::PostMessage($w, 0x0111, [IntPtr]2, [IntPtr]::Zero)
        [void]$p.WaitForExit(4000)
        if (-not $p.HasExited) { $p.Kill() }
    }
    $lang = Sub-Dir 'langdir'
    Copy-Item (Join-Path $root 'crates\nv-text\lang\th.txt') "$lang\th.txt"
    $env:NUVIEWER_LANG_DIR = $lang
    Set-Content $env:NUVIEWER_SETTINGS 'language = th' -Encoding UTF8
    $p = Start-Process $Exe -ArgumentList '--settings' -PassThru
    $w = Window-Of $p '*' 8000
    Check 'with language = th the Settings title is Thai' ($null -ne $w -and (@($p.MainWindowTitle.ToCharArray() | Where-Object { [int]$_ -ge 0x0E00 -and [int]$_ -le 0x0E7F }).Count -ge 3)) $p.MainWindowTitle
    if ($w) { [void][In]::PostMessage($w, 0x0111, [IntPtr]2, [IntPtr]::Zero); [void]$p.WaitForExit(4000) }
    if (-not $p.HasExited) { $p.Kill() }
    Remove-Item Env:\NUVIEWER_LANG_DIR

    # ================================================================================================
    Write-Host "`n[6] NuImporter"
    $imp = Join-Path $bin 'nuimporter.exe'
    if (-not (Test-Path $imp)) { Skip 'importer tests' "no $imp" }
    else {
        $out = Sub-Dir 'imports'
        Set-Content "$sandbox\imp.ini" "import_after = save`nimport_dir = $out`nimport_format = png`nimport_name = {source} test {n}" -Encoding ASCII
        $env:NUVIEWER_SETTINGS = "$sandbox\imp.ini"
        # clipboard picture
        $bmp = New-Object System.Drawing.Bitmap 40, 30; $g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear([System.Drawing.Color]::FromArgb(250, 120, 10)); $g.Dispose()
        [System.Windows.Forms.Clipboard]::SetImage($bmp); $bmp.Dispose()
        $p = Start-Process $imp -PassThru
        $w = Window-Of $p '*NuImporter*' 8000
        Check 'NuImporter opens' ($null -ne $w)
        if ($w) {
            [void][In]::PostMessage($w, 0x0111, [IntPtr]103, [IntPtr]::Zero)   # From the clipboard
            $f = Wait-Until { Get-ChildItem $out -Filter 'Clipboard*.png' -ErrorAction SilentlyContinue | Select-Object -First 1 } 6000
            Check 'a picture on the clipboard is saved at once as PNG' ($null -ne $f)
            if ($f) { $im = [System.Drawing.Bitmap]::FromFile($f.FullName); $px = $im.GetPixel(5, 5); Check 'with its colours and size' ($im.Width -eq 40 -and $im.Height -eq 30 -and [math]::Abs($px.R - 250) -lt 4 -and [math]::Abs($px.G - 120) -lt 4); $im.Dispose() }
            [void][In]::PostMessage($w, 0x0111, [IntPtr]2, [IntPtr]::Zero)
        }
        if (-not $p.HasExited) { [void]$p.WaitForExit(3000) }
        if (-not $p.HasExited) { $p.Kill() }
        # a screenshot of the whole screen, started the way NuQuick's hot key starts it
        Get-ChildItem $out -Filter 'Screenshot*' | Remove-Item -Force
        $p = Start-Process $imp -ArgumentList '--shot', 'full' -PassThru
        $f = Wait-Until { Get-ChildItem $out -Filter 'Screenshot*.png' -ErrorAction SilentlyContinue | Select-Object -First 1 } 12000
        Check '--shot full saves a screenshot' ($null -ne $f)
        if ($f) { $im = [System.Drawing.Image]::FromFile($f.FullName); Check 'it is as big as the screen' ($im.Width -ge $ScrW -and $im.Height -ge $ScrH) "$($im.Width)x$($im.Height) vs ${ScrW}x$ScrH"; $im.Dispose() }
        if (-not $p.HasExited) { $p.Kill() }
        # a PowerPoint file (a zip with a picture in ppt/media)
        $pp = "$sandbox\deck.pptx"
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        Solid "$sandbox\slide.png" 120 90 ([System.Drawing.Color]::FromArgb(10, 200, 220))
        if (Test-Path $pp) { Remove-Item $pp }
        $zip = [System.IO.Compression.ZipFile]::Open($pp, 'Create')
        [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, "$sandbox\slide.png", 'ppt/media/image1.png')
        $e = $zip.CreateEntry('ppt/slides/slide1.xml'); $sw = New-Object IO.StreamWriter($e.Open()); $sw.Write('<p/>'); $sw.Close()
        $zip.Dispose()
        $env:NUIMPORTER_FILE = $pp
        $p = Start-Process $imp -PassThru
        $w = Window-Of $p '*NuImporter*' 8000
        if ($w) {
            [void][In]::PostMessage($w, 0x0111, [IntPtr]106, [IntPtr]::Zero)   # From a file
            $f = Wait-Until { Get-ChildItem $out -Filter 'Import*.png' -ErrorAction SilentlyContinue | Select-Object -First 1 } 8000
            Check 'the picture inside a .pptx is imported' ($null -ne $f)
        } else { Check 'importer opens for the pptx' $false }
        if (-not $p.HasExited) { $p.Kill() }
        Remove-Item Env:\NUIMPORTER_FILE
        # a watched folder (tether)
        $tether = Sub-Dir 'tether'
        $env:NUIMPORTER_TETHER_DIR = $tether
        $p = Start-Process $imp -PassThru
        $w = Window-Of $p '*NuImporter*' 8000
        if ($w) {
            [void][In]::PostMessage($w, 0x0111, [IntPtr]105, [IntPtr]::Zero)   # Camera: watch a folder
            Start-Sleep -Milliseconds 1200
            Solid "$tether\IMG_0001.png" 50 50 ([System.Drawing.Color]::FromArgb(90, 90, 250))
            $f = Wait-Until { Get-ChildItem $out -Filter 'Camera*.png' -ErrorAction SilentlyContinue | Select-Object -First 1 } 8000
            Check 'a picture that appears in the watched folder is imported' ($null -ne $f)
        } else { Check 'importer opens for the tether' $false }
        if (-not $p.HasExited) { $p.Kill() }
        Remove-Item Env:\NUIMPORTER_TETHER_DIR
        Remove-Item Env:\NUVIEWER_SETTINGS
    }
}
finally {
    Close-Viewer
    $env:LOCALAPPDATA = $realLocal; $env:APPDATA = $realApp
    Remove-Item Env:\NUVIEWER_SETTINGS -ErrorAction SilentlyContinue
    try { Remove-Item $sandbox -Recurse -Force -ErrorAction SilentlyContinue } catch {}
    Write-Host "`n$script:pass passed, $script:fail failed, $script:skip skipped"
    if ($script:fail -gt 0) { exit 1 }
}
