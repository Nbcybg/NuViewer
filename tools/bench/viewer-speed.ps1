# Does a build make the viewer slower? Runs two nuviewer.exe builds (old = a release you trust, new = the one to check) over the
# same folder of 12 MP JPEGs, alternating old / new so drift in the machine hits both alike, and reports:
#   window    process start -> a visible window
#   first     the viewer's own "first image N ms" (process start -> first picture on screen), from its title
#   nav       the viewer's own "nav N ms" (key press -> next picture submitted), mean of the steps, from its title
#
#   tools\bench\viewer-speed.ps1 -Old dist\NuViewer-0.3.2-win64\nuviewer.exe -New R:\cargo-target\NuViewer\release\nuviewer.exe
#   tools\bench\viewer-speed.ps1 -Old ... -New ... -Folder D:\Photos -Runs 9
#
# Each exe is started once first and ignored (a program Windows has not seen is scanned on its first start: 0.4-0.75 s).
# The numbers are medians (min in brackets). Nothing in -Folder is written.
param(
    [Parameter(Mandatory)][string]$Old,
    [Parameter(Mandatory)][string]$New,
    [string]$Folder = '',
    [int]$Runs = 7,
    [int]$Steps = 12
)
$ErrorActionPreference = 'Stop'
if (-not $Folder) { $Folder = Join-Path $PSScriptRoot 'img12' }
$env:NUVIEWER_NO_PREWARM = '1'
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class B19 { [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, int m, IntPtr w, IntPtr l); }
'@
$first = (Get-ChildItem $Folder -Filter *.jpg | Sort-Object Name | Select-Object -First 1).FullName
if (-not $first) { throw "no .jpg in $Folder" }

function Measure-One([string]$exe) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.Arguments = "`"$first`" --windowed --new-window"
    $psi.UseShellExecute = $false
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $p = [System.Diagnostics.Process]::Start($psi)
    $win = -1; $firstMs = -1; $nav = @()
    try {
        while ($sw.ElapsedMilliseconds -lt 15000) {
            $p.Refresh()
            if ($win -lt 0 -and $p.MainWindowHandle -ne [IntPtr]::Zero) { $win = $sw.ElapsedMilliseconds }
            if ($p.MainWindowTitle -match 'first image (\d+) ms') { $firstMs = [int]$Matches[1]; break }
            Start-Sleep -Milliseconds 10
        }
        if ($firstMs -ge 0) {
            $h = $p.MainWindowHandle
            for ($i = 0; $i -lt $Steps; $i++) {
                [B19]::PostMessage($h, 0x8001, [IntPtr]0x27, [IntPtr]0) | Out-Null   # WM_NV_INJECT_KEY, VK_RIGHT
                Start-Sleep -Milliseconds 350
                $p.Refresh()
                if ($p.MainWindowTitle -match 'nav ([\d.]+) ms') { $nav += [double]$Matches[1] }
            }
        }
    } finally {
        if (-not $p.HasExited) { $p.Kill() }
        $p.WaitForExit(3000) | Out-Null
    }
    $navMean = if ($nav.Count) { ($nav | Measure-Object -Average).Average } else { -1 }
    return [pscustomobject]@{ Window = $win; First = $firstMs; Nav = $navMean }
}
function Median($a) { $s = @($a | Sort-Object); if ($s.Count -eq 0) { return -1 }; $s[[int][math]::Floor($s.Count / 2)] }
function Min($a) { @($a | Sort-Object)[0] }

$null = Measure-One $Old; $null = Measure-One $New   # warm-up
$res = @{ old = @(); new = @() }
for ($i = 0; $i -lt $Runs; $i++) {
    $res.old += Measure-One $Old
    $res.new += Measure-One $New
}
foreach ($k in 'old', 'new') {
    $r = $res[$k]
    '{0,-4} window {1,5:N0} ms ({2:N0})   first image {3,5:N0} ms ({4:N0})   nav {5,6:N1} ms ({6:N1})' -f $k, (Median ($r.Window)), (Min ($r.Window)), (Median ($r.First)), (Min ($r.First)), (Median ($r.Nav)), (Min ($r.Nav))
}
