# Session 21 (0.5.0): RAW + JPG grouping, XMP ratings and labels, a plug-in decoder, in the real viewer window.
# Scratch settings / plug-in folder / pictures; window messages in (WM_NV_INJECT_KEY), window title and files out; no real mouse or keyboard.
#   tools\test\e2e-session21.ps1 [-Dir <folder with nuviewer.exe>]
param([string]$Dir = 'R:\cargo-target\NuViewer\release')
. (Join-Path $PSScriptRoot 'lib.ps1')
$script:Exe = Join-Path $Dir 'nuviewer.exe'
if (-not (Test-Path $script:Exe)) { Write-Host "no nuviewer.exe in $Dir"; exit 2 }
if (Get-Process nuviewer -ErrorAction SilentlyContinue) { Write-Host 'NuViewer is already running (maybe yours): close it first'; exit 2 }

$work = Join-Path $env:TEMP 'nuv_s21'
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory $work | Out-Null
$pics = Join-Path $work 'pics'; New-Item -ItemType Directory $pics | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $work 'settings.ini'
Set-Content $env:NUVIEWER_SETTINGS "start_fullscreen=0`r`nconfirm_delete=0`r`n" -Encoding ASCII
$env:LOCALAPPDATA = Join-Path $work 'local'; New-Item -ItemType Directory $env:LOCALAPPDATA -Force | Out-Null
$env:NUEDITOR_BACKUP_DIR = Join-Path $work 'undo'
$env:NUVIEWER_PLUGINS = Join-Path $work 'plugins'

function MakeJpeg([string]$path, [int]$w, [int]$h, [int]$shade) {
    $b = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($b); $g.Clear([System.Drawing.Color]::FromArgb($shade, 90, 200 - $shade)); $g.Dispose()
    $b.Save($path, [System.Drawing.Imaging.ImageFormat]::Jpeg); $b.Dispose()
}
foreach ($n in 'A', 'B', 'C') { MakeJpeg (Join-Path $pics "$n.JPG") 64 48 (40 + 60 * [int][char]$n % 7) }
Copy-Item (Join-Path $pics 'A.JPG') (Join-Path $pics 'A.CR2')   # stands in for the RAW of the pair A (only listing and file operations matter here)
Copy-Item (Join-Path $pics 'B.JPG') (Join-Path $pics 'D.CR2')   # a RAW with no JPG
$hashA = (Get-FileHash (Join-Path $pics 'A.JPG')).Hash; $hashRaw = (Get-FileHash (Join-Path $pics 'A.CR2')).Hash

# a decoder plug-in: "make a picture" for .zzz by copying a fixed PNG
$plug = Join-Path $env:NUVIEWER_PLUGINS 'fake'; New-Item -ItemType Directory $plug -Force | Out-Null
$bmp = New-Object System.Drawing.Bitmap 40, 30; $bmp.Save((Join-Path $plug 'fixture.png'), [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
Set-Content (Join-Path $plug 'decode.cmd') "@copy /Y `"%~dp0fixture.png`" `"%~2`" >nul" -Encoding ASCII
Set-Content (Join-Path $plug 'plugin.ini') "name = Fake`r`napi = 1`r`ndecoder.extensions = zzz`r`ndecoder.exe = decode.cmd`r`n" -Encoding ASCII
Set-Content (Join-Path $work 'x.zzz') 'not a picture' -Encoding ASCII

function Inject([int]$vk, [int]$mods = 0, [int]$wait = 600) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
$SHIFT = 2; $CTRL = 1
function Pos { if ((Title) -match '(\d+)/(\d+)') { "$($Matches[1])/$($Matches[2])" } else { '?' } }

Write-Host "RAW + JPG"
$ok = Open-Viewer (Join-Path $pics 'A.JPG')
Check 'viewer opens A.JPG' $ok (Title)
Check 'factory: RAW and JPG are separate pictures (A.CR2 A.JPG B.JPG C.JPG D.CR2)' ((Pos) -eq '2/5') (Title)
Inject 0x51 $SHIFT
Check 'Shift+Q groups the pair: 4 pictures, still on A.JPG' ((Pos) -eq '1/4') (Title)
Check 'the choice is saved in settings.ini' ((Get-Content $env:NUVIEWER_SETTINGS -Raw) -match 'pair_group\s*=\s*true')

Write-Host "XMP ratings"
Inject 0x33 $SHIFT
$jx = Join-Path $pics 'A.JPG.xmp'; $rx = Join-Path $pics 'A.xmp'
Check 'Shift+3 wrote the JPG sidecar with 3 stars' ((Test-Path $jx) -and ((Get-Content $jx -Raw) -match 'xmp:Rating="3"'))
Check 'and the RAW sidecar (a grouped pair is rated as one)' ((Test-Path $rx) -and ((Get-Content $rx -Raw) -match 'xmp:Rating="3"'))
Check 'a picture that was not touched has no sidecar' (-not (Test-Path (Join-Path $pics 'B.JPG.xmp')))
Inject 0x36 $SHIFT
Check 'Shift+6 added the red label' ((Get-Content $jx -Raw) -match 'xmp:Label="Red"')
Inject 0x33 $SHIFT
Check 'Shift+3 again clears the stars' ((Get-Content $jx -Raw) -match 'xmp:Rating="0"')
Check 'the pictures are byte-for-byte unchanged' (((Get-FileHash (Join-Path $pics 'A.JPG')).Hash -eq $hashA) -and ((Get-FileHash (Join-Path $pics 'A.CR2')).Hash -eq $hashRaw))

Write-Host "Pair file operations"
Inject 0x2E 0 1200   # Delete (the question is off in the scratch settings)
Check 'Delete took both files of the pair' ((-not (Test-Path (Join-Path $pics 'A.JPG'))) -and (-not (Test-Path (Join-Path $pics 'A.CR2')))) (Title)
Check 'the next picture is on screen (3 left)' ((Pos) -match '/3$') (Title)
Inject 0x5A $CTRL 1500   # Ctrl+Z
Check 'one Ctrl+Z brought both back' ((Test-Path (Join-Path $pics 'A.JPG')) -and (Test-Path (Join-Path $pics 'A.CR2'))) (Title)
Check 'and the list is 4 pictures again' ((Pos) -match '/4$') (Title)
Inject 0x51 $SHIFT
Check 'Shift+Q again: separate (5 pictures)' ((Pos) -match '/5$') (Title)
Close-Viewer

Write-Host "Plug-in decoder"
$ok = Open-Viewer (Join-Path $work 'x.zzz')
Check 'a .zzz file opens through the plug-in' ($ok -and ((Title) -match '40x30')) (Title)
Close-Viewer

Write-Host ""
Write-Host "passed $($script:pass), failed $($script:fail), skipped $($script:skip)"
exit ([int]($script:fail -gt 0))
