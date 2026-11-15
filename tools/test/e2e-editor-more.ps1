# NuEditor, second round: presets, the heal tools (clone, undo), zoom on a turned picture, the keystone sliders (Pitch) and the
# Export section. Messages only (no real mouse / keyboard), scratch files / settings / preset folder.
# The editor is started with NUEDITOR_TEST=1, which puts a line of its internal state in the window title.
#
#   powershell -File tools\test\e2e-editor-more.ps1 [-Exe path\to\nueditor.exe]
param([string]$Exe = 'T:\cargo-target\NuViewer\release\nueditor.exe')
. (Join-Path $PSScriptRoot 'lib.ps1')

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Em {
    [StructLayout(LayoutKind.Sequential)] public struct RC { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RC r);
}
'@

$scratch = Join-Path ([IO.Path]::GetTempPath()) ("nueditor-more-" + [Diagnostics.Process]::GetCurrentProcess().Id)
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
$env:NUVIEWER_SETTINGS = Join-Path $scratch 'settings.ini'
Set-Content -Path $env:NUVIEWER_SETTINGS -Value "" -Encoding ASCII
$env:NUEDITOR_BACKUP_DIR = Join-Path $scratch 'backups'
$env:NUEDITOR_PRESET_DIR = Join-Path $scratch 'presets'
$env:NUEDITOR_TEST = '1'
New-Item -ItemType Directory -Force -Path $env:NUEDITOR_PRESET_DIR | Out-Null
# One preset made by hand: the pair-file text with the invert flag.
Set-Content -Path (Join-Path $env:NUEDITOR_PRESET_DIR 'Negative.nupreset') -Encoding ASCII -Value @'
nuedit version="1" source="" width="0" height="0" size="0" mtime="0" auto="off"
flags invert="1"
'@
if (-not (Test-Path $Exe)) { Write-Host "no $Exe"; exit 2 }

$script:ed = $null
function Open-Editor([string]$file) {
    $name = [IO.Path]::GetFileName($file)
    $p = Start-Process $Exe -ArgumentList @("`"$file`"", '--new-window', '--windowed') -PassThru
    $w = Wait-Until { $p.Refresh(); if ($p.MainWindowHandle -ne 0 -and $p.MainWindowTitle -like "*$name*" -and $p.MainWindowTitle -like '*first image*') { $p.MainWindowHandle } } 15000
    Start-Sleep -Milliseconds 600
    $script:ed = $p; $script:win = $w
    return ($null -ne $w)
}
function Ed-Title { $script:ed.Refresh(); $script:ed.MainWindowTitle }
function State([string]$key) { if ((Ed-Title) -match "$key=([^ \]]+)") { $Matches[1] } else { '' } }
function Client { $r = New-Object Em+RC; [void][Em]::GetClientRect($script:win, [ref]$r); $r }
function Scale { [double][In]::GetDpiForWindow($script:win) / 96.0 }
function Centre { $d = Scale; $c = Client; @([int](($c.R - 300.0 * $d) / 2), [int]($c.B / 2)) }
function Win-Pixel([int]$cx, [int]$cy) {
    $b = Grab; $r = New-Object In+RECT; [void][In]::GetWindowRect($script:win, [ref]$r)
    $off = [Math]::Max(0, [int](($r.R - $r.L) - (Client).R) / 2); $top = ($r.B - $r.T) - (Client).B - $off
    $c = $b.GetPixel($cx + $off, $cy + $top); $b.Dispose(); $c
}
function Press-Key([int]$vk, [int]$mods = 0) { [void][In]::PostMessage($script:win, 0x8001, [IntPtr]($vk -bor ($mods -shl 16)), [IntPtr]::Zero); Start-Sleep -Milliseconds 400 }
function Stop-Editor { if ($script:ed -and -not $script:ed.HasExited) { $script:ed.Kill(); [void]$script:ed.WaitForExit(3000) }; $script:ed = $null }
function Panel-Left { (Client).R - 300.0 * (Scale) }
# The y of the middle of header i when every group is closed and the panel is not scrolled.
function Header-Y([int]$i) { (56 + 30 * $i + 15) * (Scale) }
function Pic-Rect { $c = Centre; $cl = Client; $d = Scale; $aw = $cl.R - 300.0 * $d
    $s = [Math]::Min([Math]::Min($aw / 1200.0, $cl.B / 800.0), 1.0); $w = 1200 * $s; $h = 800 * $s
    @(($c[0] - $w / 2), ($c[1] - $h / 2), $w, $h) }

try {
    Write-Host "== NuEditor round 2 e2e =="
    $a = Join-Path $scratch 'a.png'; Make-Picture $a 1200 800 300
    Check 'opens' (Open-Editor $a)
    $c = Centre; $d = Scale; $pl = Panel-Left
    $bg = @(($c[0] - 450), ($c[1] - 300))
    Check 'the state line is in the title' ((Ed-Title) -match 'strokes=0 mode=None') (Ed-Title)
    Click ([int]($pl + 60 * $d)) ([int](101 * $d))   # close the Light group, which starts open

    # ---- zoom on a turned picture ----
    Press-Key 0xBE      # . = turn 45 degrees
    Press-Key 0x31      # 1 = 100%
    Check 'a turned picture zooms to 100%' ((Ed-Title) -match ' 100% ') (Ed-Title)
    Press-Key 0x46      # F = fit
    Check 'and comes back to fit' ((Ed-Title) -match 'fit ') (Ed-Title)
    Press-Key 0x30      # 0 = reset all
    $m = Win-Pixel $c[0] $c[1]
    Check 'reset all: the green square is in the middle again' ($m.G -gt 200 -and $m.R -lt 80) "$m"

    # ---- presets ----
    Click ([int]($pl + 60 * $d)) ([int](Header-Y 9))
    Check 'the Presets section lists the preset file' ((State 'presets') -eq '1') (Ed-Title)
    $first = (56 + 30 * 10 + 14) * $d
    Click ([int]($pl + 60 * $d)) ([int]$first)
    Start-Sleep -Milliseconds 300
    $bgPix = Win-Pixel $bg[0] $bg[1]
    Check 'a preset is applied: the red background is inverted' ($bgPix.R -lt 90 -and $bgPix.G -gt 180) "$bgPix"
    Press-Key 0x5A 1    # Ctrl+Z
    $bgPix = Win-Pixel $bg[0] $bg[1]
    Check 'Ctrl+Z takes the preset back' ($bgPix.R -gt 150 -and $bgPix.G -lt 80) "$bgPix"

    # ---- heal tools: clone ----
    Click ([int]($pl + 60 * $d)) ([int](Header-Y 9))   # close Presets
    Press-Key 0x4B      # K = clone
    Check 'K turns the clone tool on' ((State 'mode') -like 'Heal(Clone*') (Ed-Title)
    $pr = Pic-Rect
    # Ctrl+click the red background to copy from, then paint across the green square.
    Btn-Down ([int]($pr[0] + 40)) ([int]($pr[1] + 40)) $true; Btn-Up ([int]($pr[0] + 40)) ([int]($pr[1] + 40))
    Btn-Down ($c[0] - 40) $c[1]
    foreach ($dx in -20, 0, 20, 40) { Btn-Move ($c[0] + $dx) $c[1] $true }
    Btn-Up ($c[0] + 40) $c[1]
    Start-Sleep -Milliseconds 400
    Check 'a stroke was made' ((State 'strokes') -eq '1') (Ed-Title)
    $m = Win-Pixel $c[0] $c[1]
    Check 'the clone covered the green with the red it copied' ($m.R -gt 150 -and $m.G -lt 90) "$m"
    Check 'the picture counts as edited' ((Ed-Title).StartsWith([string][char]0x25CF)) (Ed-Title)
    Press-Key 0x5A 1
    $m = Win-Pixel $c[0] $c[1]
    Check 'Ctrl+Z takes the stroke back' ((State 'strokes') -eq '0' -and $m.G -gt 200) "$m $(Ed-Title)"
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero); Start-Sleep -Milliseconds 400
    Check 'Esc leaves the tool, not the editor' ((State 'mode') -eq 'None' -and -not $script:ed.HasExited) (Ed-Title)

    # ---- the remove tool: a stroke on a plain area leaves the picture as it was ----
    Press-Key 0x4D      # M = remove
    Btn-Down ($pr[0] + 30) ($pr[1] + 30); Btn-Move ($pr[0] + 60) ($pr[1] + 30) $true; Btn-Up ($pr[0] + 60) ($pr[1] + 30)
    Start-Sleep -Milliseconds 300
    $bgPix = Win-Pixel ($pr[0] + 45) ($pr[1] + 30)
    Check 'remove on a plain red area keeps it red' ($bgPix.R -gt 150 -and $bgPix.G -lt 90 -and (State 'strokes') -eq '1') "$bgPix $(Ed-Title)"
    Press-Key 0x5A 1
    [void][In]::PostMessage($script:win, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero); Start-Sleep -Milliseconds 300

    # ---- keystone ----
    Click ([int]($pl + 60 * $d)) ([int](Header-Y 4))   # Composition
    $top = (56 + 30 * 5) * $d
    $pitchTrack = $top + 44 * $d + 30 * $d             # second row: Pitch
    Click ([int]($pl + 284 * $d)) ([int]$pitchTrack)   # far right: +100
    Start-Sleep -Milliseconds 400
    $pr = Pic-Rect
    $tl = Win-Pixel ([int]($pr[0] + 4)) ([int]($pr[1] + 4))
    $bl = Win-Pixel ([int]($pr[0] + 4)) ([int]($pr[1] + $pr[3] - 4))
    $dark = { param($p) $p.R -lt 60 -and $p.G -lt 60 -and $p.B -lt 60 }
    Check 'Pitch +100 tips the picture: one pair of corners is empty, the other is picture' ((& $dark $tl) -ne (& $dark $bl)) "top-left $tl bottom-left $bl"
    Press-Key 0x30
    $tl = Win-Pixel ([int]($pr[0] + 4)) ([int]($pr[1] + 4))
    Check 'reset all brings the corners back' (-not (& $dark $tl)) "$tl"

    # ---- the Export section ----
    Click ([int]($pl + 60 * $d)) ([int](Header-Y 4))   # close Composition
    Click ([int]($pl + 60 * $d)) ([int](Header-Y 10))
    $scroll = [double](State 'scroll')
    Check 'Export is open (and the panel scrolled so it is in view when it is long)' ((State 'export') -like 'Same/*') (Ed-Title)
    $formatY = (56 + 30 * 11 + 14) * $d - $scroll
    Click ([int]($pl + 60 * $d)) ([int]$formatY)
    Check 'a click on Format picks the next one (JPEG)' ((State 'export') -like 'Jpeg/*') (Ed-Title)
    Click ([int]($pl + 60 * $d)) ([int]$formatY)
    Check 'and the next (PNG)' ((State 'export') -like 'Png/*') (Ed-Title)
}
catch { Write-Host "  ERROR  $_" -ForegroundColor Red; $script:fail++ }
finally {
    Stop-Editor
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
Write-Host ""
Write-Host "passed $script:pass, failed $script:fail"
if ($script:fail -gt 0) { exit 1 }
