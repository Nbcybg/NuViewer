param([string]$Csv = 'T:\bench-results\BC-nav.csv')
$d = Import-Csv $Csv
function Med($a) { $a = @($a | Sort-Object); if ($a.Count -eq 0) { return $null }; $a[[int][math]::Floor(($a.Count - 1) / 2)] }
function Avg($a) { $a = @($a); if ($a.Count -eq 0) { return $null }; ($a | Measure-Object -Average).Average }
$apps = $d | Select-Object -ExpandProperty app -Unique
'{0,-10} {1,8} | {2,22} | {3,22} | {4,10} {5,10} | {6}' -f 'app','open','paced fwd5 (ms/step)','paced back5 (ms/step)','rapid fwd5','rapid back5','verification'
foreach ($a in $apps) {
  $r = $d | Where-Object { $_.app -eq $a }
  $open = Med ($r | Where-Object { $_.step -eq 'open' } | ForEach-Object { [double]$_.done_ms })
  $fw = $r | Where-Object { $_.step -match '^F\d$' -and $_.ok -ne 'NO-REACTION' }
  $bk = $r | Where-Object { $_.step -match '^B\d$' -and $_.ok -ne 'NO-REACTION' }
  $rf = $r | Where-Object { $_.step -eq 'RAPID-FWD5' }
  $rb = $r | Where-Object { $_.step -eq 'RAPID-BACK5' }
  $bad = @($r | Where-Object { $_.ok -match 'DIFFERENT|NO-REACTION|FAIL|SKIPPED' }).Count
  $tot = @($r | Where-Object { $_.step -ne 'open' }).Count
  '{0,-10} {1,8:F0} | med {2,5:F0} avg {3,5:F0} max {4,5:F0} | med {5,5:F0} avg {6,5:F0} max {7,5:F0} | {8,10:F0} {9,10:F0} | wrong/no-reaction {10}/{11}' -f $a, $open,
    (Med ($fw | ForEach-Object { [double]$_.done_ms })), (Avg ($fw | ForEach-Object { [double]$_.done_ms })), (($fw | ForEach-Object { [double]$_.done_ms } | Measure-Object -Maximum).Maximum),
    (Med ($bk | ForEach-Object { [double]$_.done_ms })), (Avg ($bk | ForEach-Object { [double]$_.done_ms })), (($bk | ForEach-Object { [double]$_.done_ms } | Measure-Object -Maximum).Maximum),
    (Med ($rf | ForEach-Object { [double]$_.done_ms })), (Med ($rb | ForEach-Object { [double]$_.done_ms })), $bad, $tot
}
''
'Per-step median (ms) over the 3 passes, F = next, B = back:'
$steps = 'F1','F2','F3','F4','F5','B1','B2','B3','B4','B5','RAPID-FWD5','RAPID-BACK5'
'{0,-10}{1}' -f 'app', (($steps | ForEach-Object { '{0,7}' -f ($_ -replace 'RAPID-','R') }) -join '')
foreach ($a in $apps) { $line = '{0,-10}' -f $a; foreach ($s in $steps) { $v = Med ($d | Where-Object { $_.app -eq $a -and $_.step -eq $s } | ForEach-Object { [double]$_.done_ms }); $line += '{0,7:F0}' -f $v }; $line }
''
'Anything that went wrong:'
$d | Where-Object { $_.ok -match 'DIFFERENT|NO-REACTION|FAIL|SKIPPED' } | Group-Object app,step,ok | ForEach-Object { '  {0}  x{1}' -f $_.Name, $_.Count }
