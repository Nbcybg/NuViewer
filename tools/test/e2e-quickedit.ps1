# Quick edit (turn / flip a JPEG or PNG in place) in the viewer and in the gallery, and that what is on screen follows the file.
# Scratch settings / state / thumbnail cache / undo folder; window messages in (WM_NV_INJECT_KEY), window pixels and files out;
# never the real mouse or keyboard.
#   tools\test\e2e-quickedit.ps1 [-Dir <folder with nuviewer.exe and nugallery.exe>]
param([string]$Dir = 'R:\cargo-target\NuViewer\debug')
. (Join-Path $PSScriptRoot 'lib.ps1')
$script:Exe = Join-Path $Dir 'nuviewer.exe'
$gal = Join-Path $Dir 'nugallery.exe'
if (-not (Test-Path $script:Exe) -or -not (Test-Path $gal)) { Write-Host "need nuviewer.exe and nugallery.exe in $Dir"; exit 2 }
if ((Get-Process nuviewer -ErrorAction SilentlyContinue) -or (Get-Process nugallery -ErrorAction SilentlyContinue)) { Write-Host 'NuViewer / NuGallery is already running (maybe yours): close it first'; exit 2 }

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Drawing; using System.Drawing.Imaging;
public static class Px {
    // Centre of the red-ish and of the blue-ish pixels: {rx, ry, rn, bx, by, bn}.
    public static double[] Centres(Bitmap b) {
        var d = b.LockBits(new Rectangle(0, 0, b.Width, b.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
        double rx = 0, ry = 0, rn = 0, bx = 0, by = 0, bn = 0;
        unsafe_free(d, b, ref rx, ref ry, ref rn, ref bx, ref by, ref bn);
        b.UnlockBits(d);
        return new double[] { rn > 0 ? rx / rn : -1, rn > 0 ? ry / rn : -1, rn, bn > 0 ? bx / bn : -1, bn > 0 ? by / bn : -1, bn };
    }
    static void unsafe_free(BitmapData d, Bitmap b, ref double rx, ref double ry, ref double rn, ref double bx, ref double by, ref double bn) {
        var buf = new byte[d.Stride * b.Height];
        System.Runtime.InteropServices.Marshal.Copy(d.Scan0, buf, 0, buf.Length);
        for (int y = 0; y < b.Height; y += 2) for (int x = 0; x < b.Width; x += 2) {
            int i = y * d.Stride + x * 4; int B = buf[i], G = buf[i + 1], R = buf[i + 2];
            if (R > 150 && G < 80 && B < 80) { rx += x; ry += y; rn++; }
            else if (B > 150 && R < 80 && G < 80) { bx += x; by += y; bn++; }
        }
    }
}
'@

$work = Join-Path $env:TEMP 'nuv_quickedit'
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory $work | Out-Null
$pics = Join-Path $work 'pics'; New-Item -ItemType Directory $pics | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $work 'settings.ini'
Set-Content $env:NUVIEWER_SETTINGS "start_fullscreen=0`r`nconfirm_delete=0`r`nfit_enlarge=0`r`n" -Encoding ASCII
$env:LOCALAPPDATA = Join-Path $work 'local'; New-Item -ItemType Directory $env:LOCALAPPDATA -Force | Out-Null
$env:NUGALLERY_STATE = Join-Path $work 'state.ini'
$undo = Join-Path $work 'undo'; $env:NUEDITOR_BACKUP_DIR = $undo

function Make-Halves([string]$path) {   # 400x200: red left, blue right
    $b = New-Object System.Drawing.Bitmap 400, 200
    $g = [System.Drawing.Graphics]::FromImage($b)
    $g.FillRectangle((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(200, 30, 30))), 0, 0, 200, 200)
    $g.FillRectangle((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(30, 30, 200))), 200, 0, 200, 200)
    $g.Dispose(); $b.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $b.Dispose()
}
function Disk-Size([string]$path) { $ms = New-Object IO.MemoryStream (, [IO.File]::ReadAllBytes($path)); $i = [System.Drawing.Image]::FromStream($ms); $r = @($i.Width, $i.Height); $i.Dispose(); $r }
function Disk-Left([string]$path) { $ms = New-Object IO.MemoryStream (, [IO.File]::ReadAllBytes($path)); $i = New-Object System.Drawing.Bitmap $ms; $c = $i.GetPixel(10, 10); $i.Dispose(); $c }
function Screen { $b = Grab; $c = [Px]::Centres($b); $b.Dispose(); $c }
# "red left of blue", "red above blue", "blue left of red": from the centres on screen.
function Red-Left($c) { ($c[2] -gt 200) -and ($c[5] -gt 200) -and ($c[0] -lt $c[3] - 20) -and ([math]::Abs($c[1] - $c[4]) -lt 40) }
function Red-Top($c)  { ($c[2] -gt 200) -and ($c[5] -gt 200) -and ($c[1] -lt $c[4] - 20) -and ([math]::Abs($c[0] - $c[3]) -lt 40) }
function Blue-Left($c) { ($c[2] -gt 200) -and ($c[5] -gt 200) -and ($c[3] -lt $c[0] - 20) -and ([math]::Abs($c[1] - $c[4]) -lt 40) }

$pic = Join-Path $pics 'halves.png'; Make-Halves $pic

Write-Host '--- the viewer ---'
if (-not (Open-Viewer $pic)) { Write-Host 'viewer did not open'; exit 1 }
Check 'opens with red on the left' (Red-Left (Screen)) ((Screen) -join ',')
function Inject([int]$vk, [int]$mods = 0, [int]$wait = 900) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
Inject 0xDD 5   # Ctrl+Alt+]  : turn right
$s = Disk-Size $pic
Check 'turn right: the file is 200 x 400' (($s[0] -eq 200) -and ($s[1] -eq 400)) ($s -join 'x')
Check 'turn right: the picture on screen follows (red on top)' (Red-Top (Screen)) ((Screen) -join ',')
Check 'the original is in the undo folder' ((Get-ChildItem $undo -ErrorAction SilentlyContinue | Measure-Object).Count -ge 1)
Inject 0x5A 1   # Ctrl+Z
$s = Disk-Size $pic
Check 'undo: the file is 400 x 200 again' (($s[0] -eq 400) -and ($s[1] -eq 200)) ($s -join 'x')
Check 'undo: the picture on screen follows (red on the left)' (Red-Left (Screen)) ((Screen) -join ',')
Inject 0x48 5   # Ctrl+Alt+H  : flip left-right
$c = Disk-Left $pic
Check 'flip: the left of the file is blue' (($c.B -gt 150) -and ($c.R -lt 80)) "$c"
Check 'flip: the picture on screen follows (blue on the left)' (Blue-Left (Screen)) ((Screen) -join ',')
Close-Viewer

Write-Host '--- the gallery ---'
Make-Halves $pic
$gp = Start-Process $gal -ArgumentList @("`"$pics`"", '--windowed', '--new-window') -PassThru
$script:win = Wait-Until { $gp.Refresh(); if ($gp.MainWindowHandle -ne 0 -and $gp.MainWindowTitle -like '*NuGallery*') { $gp.MainWindowHandle } } 15000
Start-Sleep -Milliseconds 1800
$before = Screen
Check 'the thumbnail shows red on the left' (Red-Left $before) ($before -join ',')
Inject 0x41 1 400   # Ctrl+A
Inject 0xFFF7 0 2500   # Quick edit > Rotate right
$s = Disk-Size $pic
Check 'gallery: the file is 200 x 400' (($s[0] -eq 200) -and ($s[1] -eq 400)) ($s -join 'x')
$after = Screen
Check 'gallery: the thumbnail follows the file (red on top)' (Red-Top $after) ($after -join ',')
if (-not $gp.HasExited) { $gp.Kill() }

Write-Host ("`n{0} passed, {1} failed" -f $script:pass, $script:fail)
exit $(if ($script:fail -eq 0) { 0 } else { 1 })
