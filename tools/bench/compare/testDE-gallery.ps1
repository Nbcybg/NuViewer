# Test D (gallery: time until every visible thumbnail is there) and Test E (gallery -> viewer, viewer -> gallery).
# Folder of 60 pictures (50 PNG + 10 JPG). Cold = a fresh copy of the folder (new path, so no app has a thumbnail cache for it).
param(
  [string[]]$AppList = @('nuviewer','irfan','faststone','glass'),
  [string]$Src = 'T:\bench-gal\probe',
  [string]$Csv = 'T:\bench-results\DE-gallery.csv'
)
. (Join-Path $PSScriptRoot 'lib.ps1')
$VK_ENTER = 0x0D; $VK_ESC = 0x1B
New-Item -ItemType Directory -Force "$Out\DE" | Out-Null
if (-not (Test-Path $Csv)) { 'app,test,run,first_ms,done_ms,note' | Set-Content $Csv }
$first = (Get-ChildItem $Src -File | Sort-Object Name | Select-Object -First 1).Name
$buf = New-Object byte[] (8MB)
foreach ($f in (Get-ChildItem $Src -File)) { $fs = [IO.File]::OpenRead($f.FullName); while ($fs.Read($buf, 0, $buf.Length) -gt 0) {}; $fs.Close() }

$G = @{
  nuviewer  = @{ Exe = 'C:\Users\noobc\AppData\Local\Programs\NuViewer\nugallery.exe'; Arg = { param($d) "`"$d`"" }; Procs = @('nugallery','nuviewer'); GalCls = 'NuGalleryWindow'; ViewCls = 'NuViewerWindow' }
  irfan     = @{ Exe = $Apps.irfan.Exe;     Arg = { param($d) "`"$d`" /thumbs" }; Procs = @('i_view64'); GalCls = 'IrfanViewThumbnails'; ViewCls = '^IrfanView$' }
  faststone = @{ Exe = $Apps.faststone.Exe; Arg = { param($d) "`"$d`"" };          Procs = @('FSViewer'); GalCls = 'FastStoneImageViewerMainForm'; ViewCls = 'TFullScreenWindow' }
  glass     = @{ Exe = $Apps.glass.Exe;     Arg = { param($d) "`"$d\$first`"" };   Procs = @('ImageGlass'); GalCls = 'Avalonia'; ViewCls = 'Avalonia' }
}
function Kill-G($app) { foreach ($p in $G[$app].Procs) { Get-Process -Name $p -ErrorAction SilentlyContinue | Where-Object { $script:Pre -notcontains $_.Id } | Stop-Process -Force -ErrorAction SilentlyContinue }; Start-Sleep -Milliseconds 500 }
function Win-Of($app, $cls) {
  $ids = @($G[$app].Procs | ForEach-Object { Get-Process -Name $_ -ErrorAction SilentlyContinue } | Where-Object { $script:Pre -notcontains $_.Id } | ForEach-Object { $_.Id })
  @([BH]::Windows(300) | Where-Object { $ids -contains $_.Pid -and $_.Cls -match $cls } | Sort-Object { ($_.R.R - $_.R.L) * ($_.R.B - $_.R.T) } -Descending)
}
function Settle($rec, $t0, $minMs, $maxMs, $quietMs) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  while ($true) { Start-Sleep -Milliseconds 25; if ($sw.ElapsedMilliseconds -ge $maxMs) { break }; if ($sw.ElapsedMilliseconds -ge $minMs -and $rec.QuietMs() -ge $quietMs) { break } }
  $rec.Stop(); $rec.Analyse($t0, 500, 10)
}
function Launch-Gallery($app, $dir) {
  $rec = New-Object BH+Rec; $rec.ChangePx = 500
  for ($i = 0; $i -lt 6; $i++) { if ([BH]::CoverFocus()) { break }; Start-Sleep -Milliseconds 100 }
  $rec.Start(); Start-Sleep -Milliseconds 200
  $psi = New-Object System.Diagnostics.ProcessStartInfo($G[$app].Exe, (& $G[$app].Arg $dir)); $psi.UseShellExecute = $false
  $t0 = [BH]::Now(); [void][System.Diagnostics.Process]::Start($psi)
  $r = Settle $rec $t0 3000 20000 1500
  return $r
}
[BH]::StartCover(); Start-Sleep -Milliseconds 700
$colds = @{}
foreach ($app in $AppList) {
  Kill-G $app
  # --- D cold x2 (two fresh copies), then E round trips in the first session
  for ($run = 1; $run -le 2; $run++) {
    $dir = "T:\bench-gal\cold-$app-$run"; Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue; Copy-Item $Src $dir -Recurse
    foreach ($f in (Get-ChildItem $dir -File)) { $fs = [IO.File]::OpenRead($f.FullName); while ($fs.Read($buf, 0, $buf.Length) -gt 0) {}; $fs.Close() }
    $colds[$app] = $dir
    $r = Launch-Gallery $app $dir
    Add-Content $Csv ("{0},gallery-cold,{1},{2:F0},{3:F0}," -f $app, $run, $r[0], $r[1])
    "{0,-10} gallery COLD run {1}: first {2,6:F0}  all thumbnails {3,6:F0} ms" -f $app, $run, $r[0], $r[1]
    [BH]::Shot("$Out\DE\gal-cold-$app-$run.png", 640)
    if ($run -eq 1 -and $app -ne 'glass') {
      for ($rt = 1; $rt -le 3; $rt++) {
        # gallery -> viewer
        $gw = Win-Of $app $G[$app].GalCls | Select-Object -First 1
        if (-not $gw -or -not [BH]::Focus($gw.H)) { Add-Content $Csv "$app,gallery-to-viewer,$rt,,,no focus"; break }
        [BH]::MoveMouse(1280, 1000)
        $rec = New-Object BH+Rec; $rec.ChangePx = 500; $rec.Start(); Start-Sleep -Milliseconds 200
        if ($app -eq 'irfan') { $t0 = [BH]::Click(820, 135, $true) } elseif ($app -eq 'nuviewer') { $t0 = [BH]::Keys(@(0x20)) } else { $t0 = [BH]::Keys(@($VK_ENTER)) }   # NuGallery: Space = view; IrfanView: double-click first thumbnail
        $r = Settle $rec $t0 600 8000 900
        Add-Content $Csv ("{0},gallery-to-viewer,{1},{2:F0},{3:F0}," -f $app, $rt, $r[0], $r[1])
        "{0,-10} gallery -> viewer #{1}: first {2,6:F0}  picture complete {3,6:F0} ms" -f $app, $rt, $r[0], $r[1]
        [BH]::Shot("$Out\DE\g2v-$app-$rt.png", 480)
        # viewer -> gallery
        $vw = Win-Of $app $G[$app].ViewCls | Select-Object -First 1
        if (-not $vw -or -not [BH]::Focus($vw.H)) { Add-Content $Csv "$app,viewer-to-gallery,$rt,,,no viewer window/focus"; break }
        $rec = New-Object BH+Rec; $rec.ChangePx = 500; $rec.Start(); Start-Sleep -Milliseconds 200
        $key = switch ($app) { 'nuviewer' { 0x47 } 'irfan' { 0x1B } 'faststone' { 0x1B } }
        $t0 = [BH]::Keys(@($key))
        $r = Settle $rec $t0 600 10000 1200
        Add-Content $Csv ("{0},viewer-to-gallery,{1},{2:F0},{3:F0}," -f $app, $rt, $r[0], $r[1])
        "{0,-10} viewer -> gallery #{1}: first {2,6:F0}  gallery complete {3,6:F0} ms" -f $app, $rt, $r[0], $r[1]
        [BH]::Shot("$Out\DE\v2g-$app-$rt.png", 480)
      }
    }
    Kill-G $app
  }
  # --- D warm x3 (same folder again: the app's own cache is warm now)
  for ($run = 1; $run -le 3; $run++) {
    $r = Launch-Gallery $app $colds[$app]
    Add-Content $Csv ("{0},gallery-warm,{1},{2:F0},{3:F0}," -f $app, $run, $r[0], $r[1])
    "{0,-10} gallery WARM run {1}: first {2,6:F0}  all thumbnails {3,6:F0} ms" -f $app, $run, $r[0], $r[1]
    if ($run -eq 1) { [BH]::Shot("$Out\DE\gal-warm-$app.png", 640) }
    Kill-G $app
  }
}
[BH]::StopCover()
