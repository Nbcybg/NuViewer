# Test A: open one file of every format in every viewer; time until the picture stops changing.
param(
  [string[]]$AppList = @('nuviewer','irfan','glass','faststone','simple'),
  [string]$Only = '',          # label filter (regex)
  [string]$Csv = 'T:\bench-results\A-open.csv',
  [int]$QuietMs = 1200, [int]$MaxMsStill = 20000, [int]$Rot = 0
)
. (Join-Path $PSScriptRoot 'lib.ps1')
$B = 'T:\Benchmark-folder\'
$Files = [ordered]@{
 'jpg-pano-6MB'   = 'DSCF8394.JPG'
 'jpg-canon-18MP' = '_MG_6207.JPG'
 'jpg-damaged'    = '20231230_101344.jpg'
 'png-small'      = '20261005_1437_image.png'
 'png-14MB'       = 'Demon01.png'
 'png-56MB'       = '1794846485565.png_image.png'
 'webp'           = 'thoughts-on-this-paint-scheme-v0-1lu4c6i2jlhe1.webp'
 'avif'           = '20261112_090357_831227-3594295366.avif'
 'tga'            = 'Sunkiss01.tga'
 'psd-small'      = 'Mat\Head.psd'
 'psd-32MB'       = 'p.psd'
 'psd-293MB'      = 'Demon01.psd'
 'hdr'            = 'limehouse_4k.hdr'
 'exr-276MB'      = 'qr11-Canvas1-Beauty.exr'
 'raw-arw'        = '_DSC5222.ARW'
 'raw-cr2'        = '_MG_6207.CR2'
 'raw-raf'        = 'DSCF8453.RAF'
 'ai'             = 'airplane_Flat.ai'
 'pdf'            = 'DICIT.pdf'
 'sai'            = 'xxxx.sai'
 'oci'            = 'xxxccc.oci'
 'duf'            = 'ange.duf'
 'max'            = 'grunt.max'
 '3mf'            = 'Bin+bag+holder+all+parts.3mf'
 'glb'            = '1cf82e4d8f1a3790a1fd1639200ed16c.glb'
 'stl'            = 'e4dcdfb2b5bddb3a456f83fe6ba6b753.stl'
 'obj'            = 'bb0e4f7814dfd433ebf1de354d6b0479\71e238e82e44107c4e8c0549a100c6f3.obj'
 'mp4-87MB'       = '20240609_090923.mp4'
 'mov-66MB'       = 'bokeh_loop_alpha1.mov'
 'mp3'            = 'Audio\When we touch.mp3'
 'wav-53MB'       = 'Audio\Blade of the Storm (fixed v3 -14LUFS).wav'
 'mid'            = 'Audio\9005.mid'
}
New-Item -ItemType Directory -Force "$Out\A" | Out-Null
if (-not (Test-Path $Csv)) { 'label,app,file_mb,first_ms,done_ms,quiet_end,frames,nonblack_pct,alive,windows' | Set-Content $Csv }
[BH]::StartCover(); Start-Sleep -Milliseconds 700
$buf = New-Object byte[] (8MB)
$i = 0
foreach ($label in $Files.Keys) {
  if ($Only -and $label -notmatch $Only) { continue }
  $path = $B + $Files[$label]
  if (-not (Test-Path -LiteralPath $path)) { "missing $path"; continue }
  # same file-cache state for every viewer: read the whole file once
  $fs = [IO.File]::OpenRead($path); while ($fs.Read($buf, 0, $buf.Length) -gt 0) {}; $fs.Close()
  $mb = [math]::Round((Get-Item -LiteralPath $path).Length / 1MB, 1)
  # rotate the order of the viewers file by file so nobody always goes first
  $order = @(); for ($k = 0; $k -lt $AppList.Count; $k++) { $order += $AppList[($k + $i + $Rot) % $AppList.Count] }
  $i++
  $MaxMs = if ($label -match '^(mp4|mov|mp3|wav|mid)') { 6000 } else { $MaxMsStill }
  foreach ($app in $order) {
    foreach ($n in $Apps.Keys) { Stop-App $n }
    [BH]::CoverRaise(); Start-Sleep -Milliseconds 250
    $rec = New-Object BH+Rec; $rec.ChangePx = 500; $rec.Start(); Start-Sleep -Milliseconds 200
    $err = ''
    try { $t0 = Start-App $app $path } catch { $err = $_.Exception.Message; $t0 = [BH]::Now() }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($true) {
      Start-Sleep -Milliseconds 25
      if ($sw.ElapsedMilliseconds -ge $MaxMs) { break }
      if ($sw.ElapsedMilliseconds -ge 2500 -and $rec.QuietMs() -ge $QuietMs) { break }
    }
    $rec.Stop()
    $r = $rec.Analyse($t0, 500, 10)   # 500 of 230400 sampled pixels (0.2%): ignores small live overlays such as disk meters
    $fin = $rec.Last(); $nb = [math]::Round([BH]::NonBlack($fin), 1)
    $wins = @(App-Wins $app); $alive = if ($app -eq 'simple') { [bool](Get-Process SimpleViewer -ErrorAction SilentlyContinue | Where-Object { $script:Pre -notcontains $_.Id }) } else { [bool](Get-Process -Name $Apps[$app].Proc -ErrorAction SilentlyContinue | Where-Object { $script:Pre -notcontains $_.Id }) }
    $wt = ($wins | ForEach-Object { ($_.Cls + ':' + $_.Title) -replace '[,\r\n]',' ' } | Select-Object -First 4) -join ' ; '
    if ($wt.Length -gt 160) { $wt = $wt.Substring(0, 160) }
    [BH]::Shot("$Out\A\${label}__$app.png", 480)
    $stillChanging = ($sw.ElapsedMilliseconds -ge $MaxMs)
    $line = '{0},{1},{2},{3:F0},{4:F0},{5},{6},{7},{8},{9}' -f $label, $app, $mb, $r[0], $r[1], $(if ($stillChanging) { 'NO(max)' } else { 'yes' }), $r[2], $nb, $alive, ($wt + $err)
    Add-Content $Csv $line
    '{0,-15} {1,-9} first {2,6:N0}  done {3,6:N0}  {4}  nonblack {5}%  alive={6}' -f $label, $app, $r[0], $r[1], $(if ($stillChanging) { 'STILL-CHANGING' } else { '' }), $nb, $alive
    $rec = $null
  }
}
foreach ($n in $Apps.Keys) { Stop-App $n }
[BH]::StopCover()
