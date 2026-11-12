# End-to-end test of loading big pictures: the loading panel, window responsiveness while a 108-megapixel picture is
# decoded and copied to the GPU, and the formats / 3D textures added with it. Messages in, window pixels out (see
# lib.ps1): it never touches your real mouse or keyboard.
#   tools\test\e2e-loading.ps1 [-Exe path\to\nuviewer.exe] [-Baseline path\to\older\nuviewer.exe] [-ShotDir folder]
# -Baseline runs the responsiveness measurement for an older build too, so the improvement is visible.
# -ShotDir saves window screenshots of the loading panel and the textured model there (the test pictures are generated).
# The big test picture is ~330 MB of noise: it goes to TEST_DRIVE_DIR, else T:\ if that exists, else %TEMP%.
param([string]$Exe, [string]$Baseline, [string]$ShotDir)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $Exe) { $Exe = Join-Path $target 'release\nuviewer.exe' }
if (-not (Test-Path $Exe)) { throw "missing $Exe" }
# NB: PowerShell variables are case-insensitive, so $Exe and $script:Exe are the same variable: keep our own copy.
$newExe = $Exe
$script:Exe = $newExe
. (Join-Path $PSScriptRoot 'lib.ps1')

Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class Lag {
    [DllImport("user32.dll", SetLastError = true)] static extern IntPtr SendMessageTimeout(IntPtr h, uint m, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    // Pings the window with WM_NULL for `ms` milliseconds and returns the longest time one ping took (-1 = hung).
    public static double MaxPing(IntPtr h, int ms) {
        var all = Stopwatch.StartNew(); double worst = 0;
        while (all.ElapsedMilliseconds < ms) {
            var sw = Stopwatch.StartNew(); IntPtr r;
            IntPtr ok = SendMessageTimeout(h, 0, IntPtr.Zero, IntPtr.Zero, 2, 3000, out r);
            sw.Stop();
            if (ok == IntPtr.Zero) return -1;
            if (sw.Elapsed.TotalMilliseconds > worst) worst = sw.Elapsed.TotalMilliseconds;
            System.Threading.Thread.Sleep(2);
        }
        return worst;
    }
}
'@

$baseDir = if ($env:TEST_DRIVE_DIR) { $env:TEST_DRIVE_DIR } elseif (Test-Path 'T:\') { 'T:\' } else { $env:TEMP }
$sandbox = Join-Path $baseDir "nv_load_$([Diagnostics.Process]::GetCurrentProcess().Id)"
New-Item -ItemType Directory -Force $sandbox | Out-Null
if ($ShotDir) { New-Item -ItemType Directory -Force $ShotDir | Out-Null }
$scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$cx = [int]($scr.Width / 2); $cy = [int]($scr.Height / 2)
$env:NUVIEWER_SETTINGS = Join-Path $sandbox 'settings.ini'
Set-Content -Path $env:NUVIEWER_SETTINGS -Value "canvas = 000000`ntop_bar = false" -Encoding ASCII
function Shot([string]$name) { if ($ShotDir) { $b = Grab; $b.Save((Join-Path $ShotDir "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png); $b.Dispose() } }

try {
    # --- a folder: a small picture, six fillers, then a 108 MP picture of noise (last, so End reaches it cold) ---
    $d = Join-Path $sandbox 'pics'; New-Item -ItemType Directory -Force $d | Out-Null
    Make-Picture "$d\a_small.png" 800 600 80
    foreach ($n in 'b', 'c', 'd', 'e', 'f', 'g') { Make-Picture "$d\${n}_filler.png" 800 600 80 }
    $gen = Join-Path $target 'release\examples\gen_big.exe'
    if (-not (Test-Path $gen)) { throw "build the generator first: cargo build --release -p nv-codecs --example gen_big" }
    & $gen png-noise "$d\z_big.png" | Out-Null
    Write-Host ("big picture: {0:N0} MB on disk" -f ((Get-Item "$d\z_big.png").Length / 1MB))

    # ============================================================================================
    Write-Host "`n[1] Loading panel while a big picture loads"
    Check 'viewer opens the small picture' (Open-Viewer "$d\a_small.png")
    $before = Pixel $cx $cy
    Check "small picture: the green marker is in the middle  [$($before.R),$($before.G),$($before.B)]" (($before.G -gt 180) -and ($before.R -lt 90))
    Key 0x23 0   # End: the big picture, never prefetched
    $stages = @{}; $sw = [Diagnostics.Stopwatch]::StartNew(); $shot = $false
    while ((Title) -like '*loading*' -and $sw.ElapsedMilliseconds -lt 90000) {
        $t = Title
        foreach ($w in 'waiting for a free', 'opening', 'reading', 'decoding', 'sending', 'waiting to send') { if ($t -match $w) { $stages[$w] = $true } }
        if (-not $shot -and $sw.ElapsedMilliseconds -gt 900) {
            $shot = $true
            $mid = Pixel ($cx - 185) $cy            # inside the panel's left padding (the text is centred)
            $side = Pixel ($cx - 330) ($cy - 230)   # inside the 800x600 picture, outside the panel
            Write-Host "  (title while loading: $t)"
            Shot 'loading-panel'
            Check 'title says it is loading and what the loader is doing' ($t -match 'reading|decoding|opening|waiting|sending')
            Check "the old picture is dimmed (red background darker)  [picture pixel $($side.R),$($side.G),$($side.B)]" (($side.R -gt 60) -and ($side.R -lt 165) -and ($side.G -lt 40))
            Check "the progress panel sits in the middle  [panel pixel $($mid.R),$($mid.G),$($mid.B)]" (($mid.R -lt 45) -and ($mid.G -lt 45) -and ($mid.B -lt 45))
        }
        Start-Sleep -Milliseconds 150
    }
    Check 'the loading panel was up long enough to be seen (load took over 1 s)' $shot
    Write-Host ("  stages seen in the title: {0}   (load took {1:N1} s)" -f (($stages.Keys | Sort-Object) -join ', '), ($sw.ElapsedMilliseconds / 1000))
    Check 'the big picture finishes loading' ((Title) -notlike '*loading*') (Title)
    Start-Sleep -Milliseconds 500
    Write-Host "  (title when done: $(Title))"
    Check 'title shows the size 12000x9000' ((Title) -match '12000x9000')
    $after = Pixel $cx $cy
    Check "the panel is gone and the noise picture is shown undimmed  [centre $($after.R),$($after.G),$($after.B)]" (($after.R -gt 90) -and ($after.G -gt 90) -and ($after.B -gt 90))
    Shot 'loaded'

    # ============================================================================================
    Write-Host "`n[2] The window keeps answering while a big picture loads"
    $runs = @(@{ Name = 'new'; Exe = $newExe })
    if ($Baseline) { $runs += @{ Name = 'baseline'; Exe = $Baseline } }
    $lags = @{}
    foreach ($r in $runs) {
        $script:Exe = $r.Exe
        [void](Open-Viewer "$d\a_small.png")
        Start-Sleep -Milliseconds 300
        Key 0x23 0
        $worst = [Lag]::MaxPing($script:win, 16000)   # covers decode + upload
        $lags[$r.Name] = $worst
        Write-Host ("  {0}: longest time the window took to answer a ping: {1:N0} ms" -f $r.Name, $worst)
        [void](Wait-Until { (Title) -notlike '*loading*' } 60000)
    }
    $script:Exe = $newExe
    Check 'the window never hangs (longest ping answered in under 250 ms)' (($lags['new'] -ge 0) -and ($lags['new'] -lt 250)) "worst = $($lags['new']) ms"

    # ============================================================================================
    Write-Host "`n[3] Moving on while a big picture is still loading"
    [void](Open-Viewer "$d\a_small.png")
    Key 0x23 0           # to the big one
    Start-Sleep -Milliseconds 400
    $sw = [Diagnostics.Stopwatch]::StartNew()
    Key 0x24 0           # Home: back to the small one
    $back = Wait-Until { (Title) -like 'a_small.png*' -and (Title) -notlike '*loading*' } 4000
    Check "going back to a small picture while the big one loads is quick  [$($sw.ElapsedMilliseconds) ms]" (($null -ne $back) -and ($sw.ElapsedMilliseconds -lt 2500)) (Title)
    Start-Sleep -Milliseconds 500
    Key 0x27 0           # next: b_filler (a neighbour, normal prefetch)
    Check 'stepping on to the next small picture works' ((Wait-Until { (Title) -like 'b_filler.png*' -and (Title) -notlike '*loading*' } 4000) -ne $null) (Title)

    # ============================================================================================
    Write-Host "`n[4] New formats open"
    $f = Join-Path $sandbox 'formats'; New-Item -ItemType Directory -Force $f | Out-Null
    # 4x2 TGA: top row red, bottom row green (top-left origin, uncompressed 24 bit).
    $tga = New-Object System.Collections.Generic.List[byte]
    $hdr = New-Object byte[] 18; $hdr[2] = 2; $hdr[12] = 4; $hdr[14] = 2; $hdr[16] = 24; $hdr[17] = 0x20
    $tga.AddRange($hdr)
    1..4 | ForEach-Object { $tga.AddRange([byte[]](0, 0, 255)) }
    1..4 | ForEach-Object { $tga.AddRange([byte[]](0, 255, 0)) }
    [IO.File]::WriteAllBytes("$f\sample.tga", $tga.ToArray())
    $ppm = [byte[]]([Text.Encoding]::ASCII.GetBytes("P6`n2 1`n255`n")) + [byte[]](255, 0, 0, 0, 0, 255)
    [IO.File]::WriteAllBytes("$f\sample.ppm", $ppm)
    $opened = Open-Viewer "$f\sample.tga"
    if (-not $opened) { Write-Host "  (title: $(Title); files: $((Get-ChildItem $f | ForEach-Object Name) -join ', '))" }
    Check 'a .tga opens (sample.tga)' $opened
    if ($opened) {
        Check 'the TGA title shows its size 4x2' ((Title) -match '4x2') (Title)
        $c1 = Pixel $cx ($cy - 1)
        $c2 = Pixel $cx $cy
        Check "TGA top row is red, bottom row green, as stored  [$($c1.R),$($c1.G),$($c1.B) / $($c2.R),$($c2.G),$($c2.B)]" (($c1.R -gt 200) -and ($c1.G -lt 60) -and ($c2.G -gt 200) -and ($c2.R -lt 60))
    }
    $opened = Open-Viewer "$f\sample.ppm"
    Check 'a .ppm opens' $opened
    if ($opened) { Check 'the PPM title shows its size 2x1' ((Title) -match '2x1') (Title) }

    # ============================================================================================
    Write-Host "`n[5] 3D model textures"
    $m = Join-Path $sandbox 'model'; New-Item -ItemType Directory -Force $m | Out-Null
    Make-Picture "$m\paint.png" 64 64 0 ([System.Drawing.Color]::FromArgb(255, 0, 255))   # magenta: not a colour the grey surface has
    Set-Content "$m\quad.obj" "mtllib quad.mtl`nv -1 -1 0`nv 1 -1 0`nv 1 1 0`nv -1 1 0`nvt 0 0`nvt 1 0`nvt 1 1`nvt 0 1`nvn 0 0 1`nusemtl paint`nf 1/1/1 2/2/1 3/3/1 4/4/1" -Encoding ASCII
    Set-Content "$m\quad.mtl" "newmtl paint`nKd 1 1 1`nmap_Kd paint.png" -Encoding ASCII
    Use-Settings "$sandbox\tex_on.ini" "canvas = 000000`nmodel_textures = true"
    $opened = Open-Viewer "$m\quad.obj"
    Check 'the textured quad opens' $opened
    if ($opened) {
        Check 'the title counts the texture' ((Title) -match '1 texture') (Title)
        $on = Pixel $cx $cy
        Shot 'model-textured'
        Check "textures on: the quad is coloured by the picture (magenta)  [$($on.R),$($on.G),$($on.B)]" (($on.R -gt 40) -and ($on.B -gt 40) -and ($on.G -lt 25) -and ([math]::Abs($on.R - $on.B) -lt 25))
    }
    Use-Settings "$sandbox\tex_off.ini" "canvas = 000000`nmodel_textures = false"
    $opened = Open-Viewer "$m\quad.obj"
    if ($opened) {
        $off = Pixel $cx $cy
        Shot 'model-plain'
        Check "textures off: plain grey surface  [$($off.R),$($off.G),$($off.B)]" (([math]::Abs($off.R - $off.B) -lt 25) -and ($off.G -gt 30))
        Check 'no texture mentioned in the title when the setting is off' ((Title) -notmatch 'texture')
    } else { Check 'the plain quad opens' $false }
}
finally {
    Close-Viewer
    Remove-Item -Recurse -Force $sandbox -ErrorAction SilentlyContinue
}
Write-Host ("`n{0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip)
if ($script:fail -gt 0) { exit 1 }
