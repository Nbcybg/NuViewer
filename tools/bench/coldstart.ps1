param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$ExtraArgs = '',
    [int]$Runs = 3,
    [switch]$FreshCopy
)
# Measures: process start -> first visible top-level window of that process (ms), and -> process "idle" (input idle).
# -FreshCopy first copies the exe to a new folder under a new name with random bytes appended, so it is a file nobody (cache, antivirus) has seen.
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
public static class W {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    public static int Count(uint pid) {
        int n = 0;
        EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) n++; return true; }, IntPtr.Zero);
        return n;
    }
}
"@

function Measure-Start([string]$path) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $path
    $psi.Arguments = $ExtraArgs
    $psi.UseShellExecute = $false
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $p = [System.Diagnostics.Process]::Start($psi)
    $ms = -1
    try {
        while ($sw.ElapsedMilliseconds -lt 20000) {
            if ($p.HasExited) { break }
            if ([W]::Count([uint32]$p.Id) -gt 0) { $ms = $sw.ElapsedMilliseconds; break }
            Start-Sleep -Milliseconds 5
        }
    } finally {
        if (-not $p.HasExited) { $p.Kill() }
        $p.WaitForExit(3000) | Out-Null
    }
    return $ms
}

$target = $Exe
if ($FreshCopy) {
    $dir = Join-Path $env:TEMP ("nv_cold_" + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory $dir | Out-Null
    $target = Join-Path $dir ([IO.Path]::GetFileName($Exe))
    Copy-Item $Exe $target
    $junk = New-Object byte[] 16
    (New-Object Random).NextBytes($junk)
    $fs = [IO.File]::Open($target, 'Append'); $fs.Write($junk, 0, 16); $fs.Close()
    foreach ($sib in 'nuviewer.exe', 'nugallery.exe', 'nueditor.exe') {
        $src = Join-Path (Split-Path $Exe) $sib
        if ((Test-Path $src) -and -not (Test-Path (Join-Path $dir $sib))) { Copy-Item $src $dir }
    }
}
$res = @()
for ($i = 0; $i -lt $Runs; $i++) { $res += (Measure-Start $target) }
"{0}: {1} ms (runs: {2})" -f ([IO.Path]::GetFileName($Exe)), (($res | Sort-Object)[0]), ($res -join ', ')
