# NuGallery: PDF from pictures, compress, unpack, an archive opened as a folder (plain zip, 7z, password zip with the dialog).
# Windowed, own scratch settings / state / caches; messages in, window titles and files out (never the real mouse or keyboard).
#   tools\test\e2e-gallery-archives.ps1 [-Dir <folder with nuviewer.exe and nugallery.exe>]
param([string]$Dir = $(if ($env:CARGO_TARGET_DIR) { Join-Path $env:CARGO_TARGET_DIR 'release' } else { Join-Path $PSScriptRoot '..\..\target\release' }))
. (Join-Path $PSScriptRoot 'lib.ps1')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class Top14 {
    public delegate bool EP(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EP cb, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    public static IntPtr Form(uint pid) { IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var c = new StringBuilder(64); GetClassName(h, c, 64); if (c.ToString() == "NuViewerForm") { found = h; return false; } } return true; }, IntPtr.Zero); return found; }
}
'@
$g = Join-Path $Dir 'nugallery.exe'
if (-not (Test-Path $g)) { throw "missing $g" }
if (Get-Process nugallery -ErrorAction SilentlyContinue) { Write-Host 'a NuGallery is already running (maybe yours): close it first so the test gets its own window'; exit 2 }
$root = Join-Path $env:TEMP 'nug_arc'
if (Test-Path $root) { Remove-Item $root -Recurse -Force }
New-Item -ItemType Directory "$root\src" | Out-Null
$cols = @([System.Drawing.Color]::FromArgb(200,30,30),[System.Drawing.Color]::FromArgb(30,160,60),[System.Drawing.Color]::FromArgb(40,70,200))
for ($i = 1; $i -le 4; $i++) { Make-Picture (Join-Path "$root\src" ("pic{0:00}.png" -f $i)) (300 + $i * 20) 200 60 $cols[$i % 3] }
# Archives made by other tools.
Compress-Archive -Path "$root\src\*" -DestinationPath "$root\plain.zip"
$sz = 'C:\Program Files\7-Zip\7z.exe'
$have7z = Test-Path $sz
if ($have7z) {
    & $sz a -t7z "$root\solid.7z" "$root\src\*" | Out-Null
    & $sz a -tzip -pSecret1 -mem=ZipCrypto "$root\locked.zip" "$root\src\*" | Out-Null
}
$st = Join-Path $env:TEMP 'nug_arc_state'; New-Item -ItemType Directory $st -Force | Out-Null
$env:NUGALLERY_STATE = Join-Path $st 'state.ini'; Remove-Item $env:NUGALLERY_STATE -ErrorAction SilentlyContinue
$env:NUVIEWER_SETTINGS = Join-Path $st 'settings.ini'
Set-Content $env:NUVIEWER_SETTINGS "start_fullscreen=0`r`narchive_format=zip`r`n" -Encoding ASCII
$env:LOCALAPPDATA = Join-Path $st 'local'; New-Item -ItemType Directory $env:LOCALAPPDATA -Force | Out-Null
$gp = Start-Process $g -ArgumentList @("`"$root`"", '--windowed', '--new-window') -PassThru
try {
    $script:win = Wait-Until { $gp.Refresh(); if ($gp.MainWindowHandle -ne 0 -and $gp.MainWindowTitle -like '*NuGallery*') { $gp.MainWindowHandle } } 15000
    Start-Sleep -Milliseconds 1200
    function GT { $gp.Refresh(); $gp.MainWindowTitle }
    function Inject([int]$vk, [int]$mods = 0, [int]$wait = 500) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds $wait }
    function Typed([string]$s) { foreach ($c in $s.ToCharArray()) { [void][In]::PostMessage($script:win, 0x0102, [IntPtr][int]$c, [IntPtr]::Zero); Start-Sleep -Milliseconds 40 }; Start-Sleep -Milliseconds 200 }
    function Items { if ((GT) -match '. (\d+) items') { [int]$Matches[1] } else { -1 } }
    function Search([string]$text) { Inject 0x46 1 300; Typed $text; Inject 0x0D 700 }   # Ctrl+F, text, Enter
    function Select-First-Named([string]$name) { Search $name; Inject 0x24 0 300 }       # the search shows only that file; Home puts the cursor on it
    function Clear-Search { Inject 0x1B 0 300; Inject 0x1B 0 300 }
    function Form-Wait { Wait-Until { $f = [Top14]::Form($gp.Id); if ($f -ne [IntPtr]::Zero) { $f } } 8000 }
    function Set-Edit([IntPtr]$form, [int]$id, [string]$text) { $c = [In]::Children($form) | Where-Object { $_.Id -eq $id } | Select-Object -First 1; [void][In]::SendMessage($c.H, 0x0C, [IntPtr]::Zero, $text) }
    function Press([IntPtr]$form, [int]$id) { $c = [In]::Children($form) | Where-Object { $_.Id -eq $id } | Select-Object -First 1; [void][In]::PostMessage($c.H, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero); Start-Sleep -Milliseconds 700 }

    Write-Host "`n[1] PDF from a folder's pictures (nothing selected = this folder; here the sub-folder 'src')"
    Inject 0x57 1 100 | Out-Null   # (no-op: keep the window busy-free)
    # Go into src: search for it, open it with Space.
    Select-First-Named 'src'; Inject 0x20 0 900; Clear-Search
    Check 'inside src: 4 pictures' ((Items) -eq 4) (GT)
    Inject 0xFFF9 0 1500
    $pdf = Get-ChildItem "$root\src" -Filter *.pdf | Select-Object -First 1
    Check 'a PDF was made next to the pictures' ($null -ne $pdf) ((Get-ChildItem "$root\src").Name -join ',')
    if ($pdf) {
        $b = [IO.File]::ReadAllBytes($pdf.FullName)
        $txt = [Text.Encoding]::ASCII.GetString($b)
        $m = [regex]::Match($txt, '/Count (\d+)')
        Check 'it is a PDF with 4 pages' ($m.Success -and $m.Groups[1].Value -eq '4') "found /Count $($m.Groups[1].Value), $($b.Length) bytes"
    }
    Inject 0x08 0 900      # Backspace: up

    Write-Host "`n[2] Compress the selected files (zip), then unpack the zip"
    Inject 0x41 1 300      # Ctrl+A
    Inject 0xFFF5 0 1500
    Check 'a zip was made' ((Get-ChildItem $root -Filter 'nug_arc*.zip').Count -ge 1 -or (Get-ChildItem $root -Filter '*.zip').Count -ge 2) ((Get-ChildItem $root).Name -join ',')
    Clear-Search

    Write-Host "`n[3] A plain zip opens as a folder"
    Select-First-Named 'plain.zip'
    Inject 0x20 0 2500
    Check 'the title is the archive, with its 4 pictures' ((GT) -like '*plain.zip*' -and (Items) -eq 4) (GT)
    Inject 0x08 0 900
    Check 'Backspace leads back to the folder the archive is in' ((GT) -like '*nug_arc*') (GT)

    if ($have7z) {
        Write-Host "`n[4] A 7z opens as a folder (7-Zip does the work)"
        Clear-Search; Select-First-Named 'solid.7z'
        Inject 0x20 0 3500
        Check '7z: 4 pictures inside' ((GT) -like '*solid.7z*' -and (Items) -eq 4) (GT)
        Inject 0x08 0 900

        Write-Host "`n[5] A zip with a password asks for it"
        Clear-Search; Select-First-Named 'locked.zip'
        Inject 0x20 0 1000
        $f = Form-Wait
        Check 'the password dialog appears' ($null -ne $f)
        if ($f) {
            Set-Edit $f 10 'wrong'; Press $f 1
            $f2 = Form-Wait
            Check 'a wrong password asks again' ($null -ne $f2)
            if ($f2) { Set-Edit $f2 10 'Secret1'; Press $f2 1 }
            Start-Sleep -Milliseconds 2500
            Check 'the right password opens it with 4 pictures' ((GT) -like '*locked.zip*' -and (Items) -eq 4) (GT)
        }
    } else { Skip '7z and password archives' '7-Zip is not installed here' }
}
finally {
    if (-not $gp.HasExited) { $gp.Kill() }
    Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host "`n$script:pass passed, $script:fail failed, $script:skip skipped"
if ($script:fail -gt 0) { exit 1 }
