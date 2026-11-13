# End-to-end test of session 9: CMYK pictures through the colour engine (a CMYK TIFF of four stripes: paper, cyan, black, 50% black), the
# assumed-profile notice, RAW developed from the sensor data (needs a RAW sample: skipped when it is not on this PC), and
# documents in the gallery. Messages in, window pixels out (see lib.ps1): it never touches your real mouse or keyboard,
# every scenario reads its own settings file, and the thumbnail cache is a scratch one.
#   tools\test\e2e-session9.ps1 [-Exe path\to\nuviewer.exe] [-Raw path\to\file.dng]
param([string]$Exe, [string]$Raw = 'P:\Downloads\DSC02765.dng')
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $Exe) { $Exe = Join-Path $target 'release\nuviewer.exe' }
if (-not (Test-Path $Exe)) { throw "missing $Exe" }
$script:Exe = $Exe
. (Join-Path $PSScriptRoot 'lib.ps1')

$sandbox = Join-Path $env:TEMP "nv_s9_$([Diagnostics.Process]::GetCurrentProcess().Id)"
New-Item -ItemType Directory -Force $sandbox | Out-Null
$realSettings = $env:NUVIEWER_SETTINGS
$realLocal = $env:LOCALAPPDATA
$env:LOCALAPPDATA = Join-Path $sandbox 'local'
New-Item -ItemType Directory -Force $env:LOCALAPPDATA | Out-Null
function Sub-Dir([string]$name) { $d = Join-Path $sandbox $name; New-Item -ItemType Directory -Force $d | Out-Null; $d }
function Near([System.Drawing.Color]$c, [int]$r, [int]$g, [int]$b, [int]$tol = 8) { ([math]::Abs($c.R - $r) -le $tol) -and ([math]::Abs($c.G - $g) -le $tol) -and ([math]::Abs($c.B - $b) -le $tol) }
function Fmt([System.Drawing.Color]$c) { "($($c.R),$($c.G),$($c.B))" }

try {
    # ============================================================================================
    Write-Host "`n[1] A CMYK TIFF is converted through FOGRA39 (assumed: it has no profile), relative colorimetric"
    $d = Sub-Dir 'cmyk'
    Copy-Item (Join-Path $PSScriptRoot 'cmyk_stripes_400x100.tif') "$d\ink.tif"
    Use-Settings "$sandbox\cmyk.ini" "canvas = 000000`nintent = relative`nbpc = false`ncmyk = fogra39"
    Check 'viewer opens the CMYK TIFF' (Open-Viewer "$d\ink.tif" @('--windowed'))
    # The picture is shown in the middle of a black window: find it on the middle row, then take the middle of each stripe.
    $b = Grab; $row = [int]($b.Height / 2)
    $xs = 0..($b.Width - 1) | Where-Object { $c = $b.GetPixel($_, $row); ($c.R + $c.G + $c.B) -gt 30 }
    $left = ($xs | Measure-Object -Minimum).Minimum; $count = $xs.Count
    $cols = 0..3 | ForEach-Object { $b.GetPixel([int]($left + $_ * [math]::Max(1, $count / 4) + [math]::Max(1, $count / 4) / 2 - 0.5), $row) }
    $b.Dispose()
    $step = [math]::Max(1, $count / 4)
    Check "found the four stripes  [x from $left, $count wide]" ($count -ge 300 -and $count -le 1700)
    Check "paper is white  $(Fmt $cols[0])" (Near $cols[0] 255 255 255)
    Check "100% cyan is FOGRA39 cyan (0,160,228)  $(Fmt $cols[1])" (Near $cols[1] 0 160 228)
    Check "100% black is (43,43,42)  $(Fmt $cols[2])" (Near $cols[2] 43 43 42)
    Check "50% black is (158,158,158)  $(Fmt $cols[3])" (Near $cols[3] 158 158 158)
    # The same picture with the SWOP profile assumed is another cyan.
    Close-Viewer
    Use-Settings "$sandbox\cmyk2.ini" "canvas = 000000`ncmyk = swop"
    if (Test-Path "$env:SystemRoot\System32\spool\drivers\color\RSWOP.icm") {
        Check 'viewer opens it again with SWOP' (Open-Viewer "$d\ink.tif" @('--windowed'))
        $b = Grab; $sw = $b.GetPixel([int]($left + $step * 1.5), $row); $b.Dispose()
        Check "SWOP cyan differs from FOGRA39 cyan  $(Fmt $sw)" (-not (Near $sw 0 160 228 6))
    } else { Skip 'SWOP' 'RSWOP.icm is not on this PC' }
    Close-Viewer

    # ============================================================================================
    Write-Host "`n[2] Camera RAW is developed from the sensor data"
    if (-not (Test-Path $Raw)) { Skip 'RAW' "$Raw is not on this PC" }
    else {
        $d = Sub-Dir 'raw'
        Copy-Item $Raw "$d\shot.dng"
        Use-Settings "$sandbox\raw_full.ini" "raw_mode = full"
        Check 'full: opens' (Open-Viewer "$d\shot.dng" @('--windowed'))
        $t = Title
        Check "full: the title shows the sensor size  [$t]" ($t -like '*6000x4000*' -or $t -like '*4000x6000*')
        Close-Viewer
        Use-Settings "$sandbox\raw_half.ini" "raw_mode = half"
        Check 'half: opens' (Open-Viewer "$d\shot.dng" @('--windowed'))
        $t = Title
        Check "half: a quarter of the pixels  [$t]" ($t -like '*3000x2000*' -or $t -like '*2000x3000*')
        Close-Viewer
    }

    # ============================================================================================
    Write-Host "`n[3] Documents in the gallery"
    $d = Sub-Dir 'docs'
    Make-Picture "$d\a.png" 64 48 0
    Make-Picture "$sandbox\page.png" 80 60 0
    Compress-Archive -Path "$sandbox\page.png" -DestinationPath "$d\book.zip" -Force
    Set-Content -Path "$d\notes.txt" -Value "First line of the notes`nsecond line" -Encoding ASCII
    Use-Settings "$sandbox\docs_off.ini" "documents = false"
    Check 'gallery opens (documents off)' (Open-Viewer "$d\a.png" @('--gallery', '--windowed'))
    Start-Sleep -Milliseconds 600
    Check "documents off: one picture is listed  [$(Title)]" ((Title) -like '*1/1*')
    Close-Viewer
    Use-Settings "$sandbox\docs_on.ini" "documents = true"
    Check 'gallery opens (documents on)' (Open-Viewer "$d\a.png" @('--gallery', '--windowed'))
    $ok = Wait-Until { (Title) -like '*thumbs 3/3*' } 8000
    Check "documents on: three tiles, all with thumbnails  [$(Title)]" ($null -ne $ok)
    Close-Viewer
}
finally {
    Close-Viewer
    $env:NUVIEWER_SETTINGS = $realSettings
    $env:LOCALAPPDATA = $realLocal
    Remove-Item -Recurse -Force $sandbox -ErrorAction SilentlyContinue
}
Write-Host "`n$script:pass passed, $script:fail failed, $script:skip skipped"
if ($script:fail -gt 0) { exit 1 }
