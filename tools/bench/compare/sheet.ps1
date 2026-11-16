# Contact sheets for Test A: one row per file, one column per viewer (final screenshot of each run), to judge "did it open?" by eye.
param([string]$Dir = 'T:\bench-results\A', [string[]]$AppList = @('nuviewer','irfan','glass','faststone','simple'), [int]$PerSheet = 6, [int]$W = 300)
Add-Type -AssemblyName System.Drawing
$labels = Get-ChildItem $Dir -Filter *__nuviewer.png | ForEach-Object { $_.BaseName -replace '__nuviewer$','' }
$h = [int]($W * 9 / 16); $cols = $AppList.Count
$f = New-Object System.Drawing.Font 'Arial', 10
for ($s = 0; $s * $PerSheet -lt $labels.Count; $s++) {
  $part = @($labels | Select-Object -Skip ($s * $PerSheet) -First $PerSheet)
  $bmp = New-Object System.Drawing.Bitmap ($cols * ($W + 3)), ($part.Count * ($h + 18))
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear([System.Drawing.Color]::DimGray)
  for ($r = 0; $r -lt $part.Count; $r++) {
    for ($c = 0; $c -lt $cols; $c++) {
      $p = Join-Path $Dir ($part[$r] + '__' + $AppList[$c] + '.png')
      $x = $c * ($W + 3); $y = $r * ($h + 18)
      if (Test-Path $p) { $im = [System.Drawing.Image]::FromFile($p); $g.DrawImage($im, $x, $y + 16, $W, $h); $im.Dispose() }
      $g.DrawString($part[$r] + '  [' + $AppList[$c] + ']', $f, [System.Drawing.Brushes]::Yellow, $x + 2, $y + 1)
    }
  }
  $bmp.Save((Join-Path $Dir ('..\sheetA-{0}.jpg' -f $s)), [System.Drawing.Imaging.ImageFormat]::Jpeg)
  $g.Dispose(); $bmp.Dispose()
}
"sheets: " + [math]::Ceiling($labels.Count / $PerSheet)
