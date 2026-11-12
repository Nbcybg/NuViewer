param([string]$Out, [int]$W = 4000, [int]$H = 3000, [int]$Count = 8, [string]$Prefix = "p")
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force $Out | Out-Null
# well-separated centre colours so a screen-pixel probe can tell which file is showing
$cols = @(
  @(230,30,30), @(30,200,30), @(30,30,230), @(230,220,30),
  @(30,210,210), @(220,30,220), @(240,140,20), @(140,30,230),
  @(30,120,60), @(120,200,250), @(250,150,170), @(100,60,20)
)
$jpeg = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq 'image/jpeg' }
$ep = New-Object System.Drawing.Imaging.EncoderParameters(1)
$ep.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter([System.Drawing.Imaging.Encoder]::Quality, [long]90)
for ($i = 0; $i -lt $Count; $i++) {
  $rnd = New-Object System.Random($i + 1)
  $bmp = New-Object System.Drawing.Bitmap($W, $H, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $br = New-Object System.Drawing.Drawing2D.LinearGradientBrush((New-Object System.Drawing.Rectangle(0,0,$W,$H)), [System.Drawing.Color]::FromArgb(40,50,70), [System.Drawing.Color]::FromArgb(180,170,150), 35.0)
  $g.FillRectangle($br, 0, 0, $W, $H)
  # texture so the JPEG has realistic entropy (photo-like size / decode cost)
  for ($k = 0; $k -lt 6000; $k++) {
    $c = [System.Drawing.Color]::FromArgb(70, $rnd.Next(256), $rnd.Next(256), $rnd.Next(256))
    $b = New-Object System.Drawing.SolidBrush($c)
    $s = $rnd.Next(8, 120)
    $g.FillEllipse($b, $rnd.Next(0, $W), $rnd.Next(0, $H), $s, $s)
    $b.Dispose()
  }
  $cc = $cols[$i % $cols.Count]
  $cb = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb($cc[0], $cc[1], $cc[2]))
  $g.FillRectangle($cb, [int]($W/2 - 300), [int]($H/2 - 300), 600, 600)
  $g.Dispose()
  $name = "{0}{1:D2}.jpg" -f $Prefix, $i
  $bmp.Save((Join-Path $Out $name), $jpeg, $ep)
  $bmp.Dispose()
}
Get-ChildItem $Out -Filter "$Prefix*.jpg" | ForEach-Object { "{0} {1:N1} MB" -f $_.Name, ($_.Length/1MB) }
