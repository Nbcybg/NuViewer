# End-to-end test of the session-16 changes: damaged JPEGs open, paged documents stop at their last page (the wheel never
# carries on into the next file), ping-pong play of an animation, the RAW look toggle, and the new formats (HTML, SWF, FLA).
# Messages in, window pixels / titles out (see lib.ps1). Needs python with Pillow for the test files (skipped otherwise).
#   tools\test\e2e-session16.ps1 [-Exe path\to\nuviewer.exe]
param([string]$Exe)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $Exe) { $Exe = Join-Path $target 'release\nuviewer.exe' }
if (-not (Test-Path $Exe)) { throw "missing $Exe" }
$script:Exe = $Exe
. (Join-Path $PSScriptRoot 'lib.ps1')

$sandbox = Join-Path $env:TEMP "nv_s16_$([Diagnostics.Process]::GetCurrentProcess().Id)"
New-Item -ItemType Directory -Force $sandbox | Out-Null
$realLocal = $env:LOCALAPPDATA; $realApp = $env:APPDATA
$env:LOCALAPPDATA = Join-Path $sandbox 'local'; $env:APPDATA = Join-Path $sandbox 'roaming'
New-Item -ItemType Directory -Force $env:LOCALAPPDATA, $env:APPDATA | Out-Null
function Sub-Dir([string]$name) { $d = Join-Path $sandbox $name; New-Item -ItemType Directory -Force $d | Out-Null; $d }
function Key-Mods([int]$vk, [int]$mods, [int]$wait = 450) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
# NV_PYTHON = a python with Pillow (the `python` on PATH can be Windows' store stub, which starts an installer)
$python = $env:NV_PYTHON

try {
    $d = Sub-Dir 'pics'
    if ($python) {
        $py = Join-Path $sandbox 'make.py'
        @"
import os, random
from PIL import Image
d = r'$d'
random.seed(2)
im = Image.new('RGB', (320, 240))
px = im.load()
for y in range(240):
    for x in range(320):
        px[x, y] = (x * 255 // 320, y * 255 // 240, random.randint(0, 255))
im.save(os.path.join(d, 'a_prog.jpg'), quality=90, progressive=True)
data = open(os.path.join(d, 'a_prog.jpg'), 'rb').read()
open(os.path.join(d, 'b_cut.jpg'), 'wb').write(data[: len(data) // 2])
frames = [Image.new('RGB', (64, 48), c) for c in [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0)]]
frames[0].save(os.path.join(d, 'c_anim.gif'), save_all=True, append_images=frames[1:], duration=200, loop=0)
pages = [Image.new('RGB', (200, 280), c) for c in [(250, 250, 250), (200, 220, 250), (250, 220, 200)]]
pages[0].save(os.path.join(d, 'd_doc.pdf'), save_all=True, append_images=pages[1:])
Image.new('RGB', (100, 100), (10, 120, 10)).save(os.path.join(d, 'e_after.png'))
"@ | Set-Content -Path $py -Encoding ASCII
        & $python -I $py
    }

    # ================================================================================================
    Write-Host "`n[1] A progressive JPEG that stops half way opens (it used to be 'Exhausted data in the image')"
    if (-not (Test-Path "$d\b_cut.jpg")) { Skip 'damaged jpeg' 'no python / Pillow for the test files' }
    else {
        Use-Settings "$sandbox\s1.ini" "fullscreen = false"
        Check 'the cut-off file opens' (Open-Viewer "$d\b_cut.jpg")
        $bmp = Grab; $c = $bmp.GetPixel([int]($bmp.Width / 2), [int]($bmp.Height / 2)); $bmp.Dispose()
        Check 'the middle of the window shows picture, not the dark error panel' (($c.R + $c.G + $c.B) -gt 90) "pixel $($c.R),$($c.G),$($c.B)"
        Close-Viewer
    }

    # ================================================================================================
    Write-Host "`n[2] A document with pages: the wheel stops at the last page, Ctrl+Right leaves it"
    if (-not (Test-Path "$d\d_doc.pdf")) { Skip 'pdf paging' 'no test pdf' }
    else {
        Use-Settings "$sandbox\s2.ini" "fullscreen = false"
        Check 'the PDF opens' (Open-Viewer "$d\d_doc.pdf")
        Check 'it has 3 pages' ((Title) -like '*page 1/3*') (Title)
        1..8 | ForEach-Object { Wheel -120 300 300 }   # a fast spin forward
        Check 'ends on page 3 of the same file' (((Title) -like '*d_doc.pdf*') -and ((Title) -like '*page 3/3*')) (Title)
        Key-Mods 0x27 1 900                              # Ctrl+Right = next file
        Check 'Ctrl+Right goes to the next file' ((Title) -like '*e_after.png*') (Title)
        Close-Viewer
    }

    # ================================================================================================
    Write-Host "`n[3] Ping-pong: the frames go up, then down"
    if (-not (Test-Path "$d\c_anim.gif")) { Skip 'ping-pong' 'no test gif' }
    else {
        Use-Settings "$sandbox\s3.ini" "fullscreen = false`nplay_pingpong = true`nplay_loop = true"
        Check 'the GIF opens' (Open-Viewer "$d\c_anim.gif")
        $seq = @(); $sw = [Diagnostics.Stopwatch]::StartNew()
        while ($sw.ElapsedMilliseconds -lt 3600) {
            $t = Title; if ($t -match 'frame (\d)/4') { $f = [int]$Matches[1]; if ($seq.Count -eq 0 -or $seq[-1] -ne $f) { $seq += $f } }
            Start-Sleep -Milliseconds 40
        }
        Write-Host "    frames seen: $($seq -join ' ')"
        $up = $false; $down = $false
        for ($i = 1; $i -lt $seq.Count; $i++) { if ($seq[$i] -gt $seq[$i - 1]) { $up = $true }; if ($seq[$i] -lt $seq[$i - 1] -and $seq[$i - 1] - $seq[$i] -eq 1) { $down = $true } }
        Check 'the frames count up' $up
        Check 'and then count down one by one (no jump back to 1)' $down ($seq -join ' ')
        Close-Viewer
        Use-Settings "$sandbox\s3b.ini" "fullscreen = false`nplay_pingpong = false`nplay_loop = true"
        Check 'the GIF opens again' (Open-Viewer "$d\c_anim.gif")
        $seq = @(); $sw = [Diagnostics.Stopwatch]::StartNew()
        while ($sw.ElapsedMilliseconds -lt 2600) {
            $t = Title; if ($t -match 'frame (\d)/4') { $f = [int]$Matches[1]; if ($seq.Count -eq 0 -or $seq[-1] -ne $f) { $seq += $f } }
            Start-Sleep -Milliseconds 40
        }
        $backward = $false
        for ($i = 1; $i -lt $seq.Count; $i++) { if ($seq[$i - 1] - $seq[$i] -eq 1) { $backward = $true } }
        Check 'without ping-pong it never steps backward' (-not $backward) ($seq -join ' ')
        Close-Viewer
        # Shift+K turns it on from the keyboard (wParam = vk | shift << 16)
        Check 'the GIF opens a third time' (Open-Viewer "$d\c_anim.gif")
        Key-Mods 0x4B 2 400
        $ini = Get-Content "$sandbox\s3b.ini" -Raw
        Check 'Shift+K saved play_pingpong = true' ($ini -match 'play_pingpong\s*=\s*true') $ini
        Close-Viewer
    }

    # ================================================================================================
    Write-Host "`n[4] The RAW look toggle (Shift+R) changes the shared setting"
    Use-Settings "$sandbox\s4.ini" "fullscreen = false`nraw_mode = full"
    Check 'a picture opens' (Open-Viewer "$d\e_after.png")
    Key-Mods 0x52 2 600
    $ini = Get-Content "$sandbox\s4.ini" -Raw
    Check 'Shift+R: raw_mode = preview (camera look)' ($ini -match 'raw_mode\s*=\s*preview') $ini
    Key-Mods 0x52 2 600
    $ini = Get-Content "$sandbox\s4.ini" -Raw
    Check 'Shift+R again: raw_mode = full (sensor data)' ($ini -match 'raw_mode\s*=\s*full') $ini
    Close-Viewer

    # ================================================================================================
    Write-Host "`n[5] New formats: HTML, SWF, FLA"
    $f = Sub-Dir 'formats'
    Set-Content "$f\a_page.html" '<!doctype html><title>T</title><body style="background:#2a8"><h1>Hello</h1>' -Encoding UTF8
    # a vector-only Flash movie (CWS, 550 x 400, 24 fps), made by python
    if ($python) {
        $py2 = Join-Path $sandbox 'swf.py'
        $swfPath = "$f\b_movie.swf"
        @"
import struct, zlib
acc = []
def push(v, n):
    for b in range(n - 1, -1, -1): acc.append((v >> b) & 1)
push(15, 5)
for v in (0, 11000, 0, 8000): push(v, 15)
while len(acc) % 8: acc.append(0)
head = bytes(int(''.join(map(str, acc[i:i + 8])), 2) for i in range(0, len(acc), 8)) + bytes([0, 24, 120, 0])
body = head + struct.pack('<H', (2 << 6) | 4) + bytes(4) + bytes(2)
open(r'$swfPath', 'wb').write(b'CWS' + bytes([10]) + struct.pack('<I', len(body) + 8) + zlib.compress(body))
"@ | Set-Content -Path $py2 -Encoding ASCII
        & $python -I $py2
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zipPath = "$f\c_project.fla"
    $zip = [IO.Compression.ZipFile]::Open($zipPath, 'Create')
    $e = $zip.CreateEntry('DOMDocument.xml'); $w = New-Object IO.StreamWriter($e.Open()); $w.Write('<DOMDocument width="800" height="600" frameRate="30"></DOMDocument>'); $w.Dispose()
    $zip.Dispose()
    Use-Settings "$sandbox\s5.ini" "fullscreen = false"
    Check 'an HTML page opens (drawn by the browser, or a text card)' (Open-Viewer "$f\a_page.html")
    Close-Viewer
    if (Test-Path "$f\b_movie.swf") { Check 'a SWF opens (a card for a movie without bitmaps)' (Open-Viewer "$f\b_movie.swf"); Close-Viewer } else { Skip 'swf' 'no python' }
    Check 'an FLA opens (a card)' (Open-Viewer "$f\c_project.fla")
    Close-Viewer
}
finally {
    Close-Viewer
    $env:LOCALAPPDATA = $realLocal; $env:APPDATA = $realApp
    Remove-Item Env:\NUVIEWER_SETTINGS -ErrorAction SilentlyContinue
}
Write-Host "`n$script:pass passed, $script:fail failed, $script:skip skipped"
if ($script:fail -gt 0) { exit 1 }
