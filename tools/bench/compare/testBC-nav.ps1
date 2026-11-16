# Test B (paced next/back, one step at a time, wait for the full picture) and
# Test C (rapid: 5x next in one burst, then 5x back in one burst).  Real keystrokes (SendInput), same for every viewer.
param(
  [string[]]$AppList = @('nuviewer','irfan','glass','faststone','simple'),
  [string]$Mix = 'T:\bench-mix',
  [int]$Passes = 3,
  [string]$Csv = 'T:\bench-results\BC-nav.csv'
)
. (Join-Path $PSScriptRoot 'lib.ps1')
$VK_RIGHT = 0x27; $VK_LEFT = 0x25
$files = @(Get-ChildItem $Mix -File | Sort-Object Name)
if ($files.Count -lt 7) { throw "need 7+ files in $Mix" }
New-Item -ItemType Directory -Force "$Out\BC" | Out-Null
if (-not (Test-Path $Csv)) { 'pass,app,step,file,first_ms,done_ms,ok,note' | Set-Content $Csv }
$buf = New-Object byte[] (8MB)
foreach ($f in $files) { $fs = [IO.File]::OpenRead($f.FullName); while ($fs.Read($buf, 0, $buf.Length) -gt 0) {}; $fs.Close() }   # equal cache state for everybody
[BH]::StartCover(); Start-Sleep -Milliseconds 700
$TOL = 1500   # sampled pixels that may differ when we say "same picture as before" (0.65%)

function Step($label, $fname, $keyfn, $maxMs, $ref) {
  $rec = New-Object BH+Rec; $rec.ChangePx = 500; $rec.Start(); Start-Sleep -Milliseconds 180
  $t0 = & $keyfn
  $sw = [Diagnostics.Stopwatch]::StartNew()
  while ($true) {
    Start-Sleep -Milliseconds 20
    if ($sw.ElapsedMilliseconds -ge $maxMs) { break }
    if ($sw.ElapsedMilliseconds -ge 500 -and $rec.QuietMs() -ge 600) { break }
  }
  $rec.Stop(); $r = $rec.Analyse($t0, 500, 10); $last = $rec.Last()
  $ok = 'n/a'; if ($ref) { $ok = if ([BH]::Diff($last, $ref, 10) -le $TOL) { 'same' } else { 'DIFFERENT' } }
  if ($r[0] -lt 0) { $ok = 'NO-REACTION' }
  [pscustomobject]@{ first = $r[0]; done = $r[1]; ok = $ok; last = $last }
}

[BH]::MoveMouse(1280, 900)
for ($pass = 1; $pass -le $Passes; $pass++) {
  $rot = $pass - 1
  $order = @(); for ($k = 0; $k -lt $AppList.Count; $k++) { $order += $AppList[($k + $rot) % $AppList.Count] }
  foreach ($app in $order) {
    foreach ($n in $Apps.Keys) { Stop-App $n }
    [BH]::CoverRaise(); Start-Sleep -Milliseconds 300
    $rec = New-Object BH+Rec; $rec.ChangePx = 500; $rec.Start(); Start-Sleep -Milliseconds 200
    $t0 = Start-App $app $files[0].FullName
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($true) { Start-Sleep -Milliseconds 25; if ($sw.ElapsedMilliseconds -ge 15000) { break }; if ($sw.ElapsedMilliseconds -ge 2500 -and $rec.QuietMs() -ge 1200) { break } }
    $rec.Stop(); $r = $rec.Analyse($t0, 500, 10); $ref = @{}; $ref[0] = $rec.Last()
    Add-Content $Csv ("{0},{1},open,{2},{3:F0},{4:F0},,first image" -f $pass, $app, $files[0].Name, $r[0], $r[1])
    Start-Sleep -Milliseconds 500
    $w = @(App-Wins $app) | Sort-Object { ($_.R.R - $_.R.L) * ($_.R.B - $_.R.T) } -Descending | Select-Object -First 1
    if (-not $w) { Add-Content $Csv "$pass,$app,ALL,,,,FAIL,no window"; "$app pass $pass : no window"; continue }
    $foc = $false; for ($t = 0; $t -lt 5 -and -not $foc; $t++) { $foc = [BH]::Focus($w.H) }
    if (-not $foc) { Add-Content $Csv "$pass,$app,ALL,,,,SKIPPED,could not take focus (nothing sent)"; "$app pass $pass : no focus, skipped"; continue }
    [BH]::MoveMouse(1280, 900)
    "--- pass $pass $app : first image done at {0:N0} ms" -f $r[1]
    # B: paced, forward 5 then back 5
    for ($k = 1; $k -le 5; $k++) {
      $s = Step "F$k" $files[$k].Name { [BH]::Keys(@($VK_RIGHT)) } 8000 $null
      $ref[$k] = $s.last
      Add-Content $Csv ("{0},{1},F{2},{3},{4:F0},{5:F0},{6}," -f $pass, $app, $k, $files[$k].Name, $s.first, $s.done, $s.ok)
      '   F{0} {1,-28} first {2,6:N0}  done {3,6:N0}  {4}' -f $k, $files[$k].Name, $s.first, $s.done, $s.ok
    }
    for ($k = 4; $k -ge 0; $k--) {
      $s = Step "B$k" $files[$k].Name { [BH]::Keys(@($VK_LEFT)) } 8000 $ref[$k]
      Add-Content $Csv ("{0},{1},B{2},{3},{4:F0},{5:F0},{6}," -f $pass, $app, (5 - $k), $files[$k].Name, $s.first, $s.done, $s.ok)
      '   B{0} {1,-28} first {2,6:N0}  done {3,6:N0}  {4}' -f (5 - $k), $files[$k].Name, $s.first, $s.done, $s.ok
    }
    # C: rapid burst forward x5 (one SendInput), check we land on picture 5; then burst back x5 -> picture 0
    $s = Step 'RF' $files[5].Name { [BH]::Keys(@($VK_RIGHT, $VK_RIGHT, $VK_RIGHT, $VK_RIGHT, $VK_RIGHT)) } 10000 $ref[5]
    Add-Content $Csv ("{0},{1},RAPID-FWD5,{2},{3:F0},{4:F0},{5}," -f $pass, $app, $files[5].Name, $s.first, $s.done, $s.ok)
    '   RAPID fwd5  first {0,6:N0}  done {1,6:N0}  {2}' -f $s.first, $s.done, $s.ok
    $s = Step 'RB' $files[0].Name { [BH]::Keys(@($VK_LEFT, $VK_LEFT, $VK_LEFT, $VK_LEFT, $VK_LEFT)) } 10000 $ref[0]
    Add-Content $Csv ("{0},{1},RAPID-BACK5,{2},{3:F0},{4:F0},{5}," -f $pass, $app, $files[0].Name, $s.first, $s.done, $s.ok)
    '   RAPID back5 first {0,6:N0}  done {1,6:N0}  {2}' -f $s.first, $s.done, $s.ok
    if ($pass -eq 1) { [BH]::Shot("$Out\BC\end-$app.png", 480) }
  }
}
foreach ($n in $Apps.Keys) { Stop-App $n }
[BH]::StopCover()
