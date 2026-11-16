# Shared harness for the viewer comparison. ASCII only (PowerShell 5.1).
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
if (-not ('BH' -as [type])) {
  Add-Type -Path (Join-Path $PSScriptRoot 'BH.cs') -ReferencedAssemblies System.Drawing,System.Windows.Forms
}
[BH]::Init()

$Out = 'T:\bench-results'
New-Item -ItemType Directory -Force $Out | Out-Null

# ---- the five viewers: launch command, how to find the window, how to close it
$Apps = [ordered]@{
  nuviewer = @{ Exe=$(if ($env:NV_EXE) { $env:NV_EXE } else { 'C:\Users\noobc\AppData\Local\Programs\NuViewer\nuviewer.exe' }); Proc='nuviewer'; Args='--fullscreen';  Cls='NuViewerWindow' }
  irfan    = @{ Exe='C:\Program Files\IrfanView\i_view64.exe'; Proc='i_view64'; Args='/fs'; Cls='IrfanView' }
  glass    = @{ Exe='C:\Users\noobc\AppData\Local\Microsoft\WindowsApps\ImageGlass.exe'; Proc='ImageGlass'; Args=''; Cls='Avalonia' }
  faststone= @{ Exe='C:\Program Files (x86)\FastStone Image Viewer\FSViewer.exe'; Proc='FSViewer'; Args=''; Cls='' }
  simple   = @{ Aumid='27382RodrigoG.deQueiroz.ImageSimpleViewer_95gv24eqr8d4w!App'; Proc='SimpleViewer'; Cls='ApplicationFrameWindow' }
}

function Lst-Win { [BH]::Windows(200) }

# processes that were already running when the harness loaded (the user's own, never touched)
$script:Pre = @($Apps.Values | ForEach-Object { Get-Process -Name $_.Proc -ErrorAction SilentlyContinue } | ForEach-Object { $_.Id })
$script:Started = @()
function Stop-App($name) {
  $a = $Apps[$name]
  Get-Process -Name $a.Proc -ErrorAction SilentlyContinue | Where-Object { $script:Pre -notcontains $_.Id } | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 400
}

# Start an app on a file. Returns the launch timestamp (taken immediately before the call).
function Start-App($name, $file, $extra='') {
  $a = $Apps[$name]
  for ($i = 0; $i -lt 6; $i++) { if ([BH]::CoverFocus()) { break }; Start-Sleep -Milliseconds 100 }   # harness = foreground owner, so the viewer may come to the front
  if ($a.Aumid) {
    $t0 = [BH]::Now()
    if ($file) { $procId = [BH]::ActivateFile($a.Aumid, $file) } else { $procId = [BH]::Activate($a.Aumid) }
    $script:Started += $procId
    return $t0
  }
  $arg = if ($file) { ('"' + $file + '" ' + $extra + ' ' + $a.Args).Trim() } else { ($extra + ' ' + $a.Args).Trim() }
  $psi = New-Object System.Diagnostics.ProcessStartInfo($a.Exe, $arg)
  $psi.UseShellExecute = $false
  $t0 = [BH]::Now()
  $p = [System.Diagnostics.Process]::Start($psi)
  $script:Started += $p.Id
  return $t0
}

# Windows of an app (by process name) that are big enough to be a viewer window
function App-Wins($name) {
  $a = $Apps[$name]
  $ids = @(Get-Process -Name $a.Proc -ErrorAction SilentlyContinue | Where-Object { $script:Pre -notcontains $_.Id } | ForEach-Object { $_.Id })
  if ($a.Aumid) { return @([BH]::Windows(300) | Where-Object { $_.Cls -eq 'ApplicationFrameWindow' -and $_.Title -match 'Image Simple Viewer' }) }
  @([BH]::Windows(300) | Where-Object { $ids -contains $_.Pid })
}

function Wait-Quiet($rec, $quietMs, $maxMs) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  while ($sw.ElapsedMilliseconds -lt $maxMs) {
    if ($rec.QuietMs() -ge $quietMs -and $sw.ElapsedMilliseconds -gt 300) { return $true }
    Start-Sleep -Milliseconds 20
  }
  return $false
}
