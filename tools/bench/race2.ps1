param(
  [Parameter(Mandatory)][string]$Exe,
  [Parameter(Mandatory)][string]$Folder,
  [Parameter(Mandatory)][ValidateSet('nv','fs')][string]$Kind,
  [int]$Seed = 7,
  [int]$PaceMs = 800,
  [string]$Filter = '*.png',
  [switch]$OpenOnly
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System; using System.Diagnostics; using System.Runtime.InteropServices; using System.Threading;
public static class R2 {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
  [DllImport("gdi32.dll")] static extern uint GetPixel(IntPtr dc, int x, int y);
  [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);

  [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleDC(IntPtr dc);
  [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleBitmap(IntPtr dc, int w, int h);
  [DllImport("gdi32.dll")] static extern IntPtr SelectObject(IntPtr dc, IntPtr o);
  [DllImport("gdi32.dll")] static extern bool BitBlt(IntPtr d, int x, int y, int w, int h, IntPtr s, int sx, int sy, uint rop);
  [StructLayout(LayoutKind.Sequential)] struct BMI { public int size, w, h; public short planes, bpp; public int comp, sizeImg, xp, yp, used, imp; }
  [DllImport("gdi32.dll")] static extern int GetDIBits(IntPtr dc, IntPtr bmp, uint start, uint lines, byte[] bits, ref BMI bi, uint usage);

  static IntPtr dc = GetDC(IntPtr.Zero);
  static IntPtr mdc = IntPtr.Zero, mbmp = IntPtr.Zero;
  static byte[] buf = null;
  const int RW = 640, RH = 360;
  public const int GX = 24, GY = 14;
  public static int PollCount = 0;
  public static double SigMs = 0;
  public static double LastSim = 0;

  public static int[] Sig(int cx, int cy) {
    var sw = Stopwatch.StartNew();
    if (mdc == IntPtr.Zero) {
      mdc = CreateCompatibleDC(dc); mbmp = CreateCompatibleBitmap(dc, RW, RH); SelectObject(mdc, mbmp); buf = new byte[RW*RH*4];
    }
    BitBlt(mdc, 0, 0, RW, RH, dc, cx - RW/2, cy - RH/2, 0x40CC0020);
    var bi = new BMI { size = 40, w = RW, h = -RH, planes = 1, bpp = 32, comp = 0 };
    GetDIBits(mdc, mbmp, 0, (uint)RH, buf, ref bi, 0);
    var s = new int[GX*GY]; int k = 0;
    for (int j = 0; j < GY; j++) for (int i = 0; i < GX; i++) {
      int x = 8 + i*(RW-16)/(GX-1), y = 8 + j*(RH-16)/(GY-1);
      int o = (y*RW + x)*4;
      s[k++] = buf[o] | (buf[o+1] << 8) | (buf[o+2] << 16);
    }
    PollCount++; SigMs += sw.Elapsed.TotalMilliseconds;
    return s;
  }
  static int D(int a, int b) {
    return Math.Abs((a&255)-(b&255)) + Math.Abs(((a>>8)&255)-((b>>8)&255)) + Math.Abs(((a>>16)&255)-((b>>16)&255));
  }
  // fraction of points within tol
  public static double Sim(int[] a, int[] b, int tol) {
    int ok = 0; for (int i = 0; i < a.Length; i++) if (D(a[i], b[i]) <= tol) ok++;
    return (double)ok / a.Length;
  }
  public static bool Changed(int[] a, int[] b) { return Sim(a, b, 10) < 0.93; }

  public static bool Centre(Process p, out int x, out int y, int sw, int sh) {
    x = sw/2; y = sh/2;
    try {
      p.Refresh(); RECT r;
      if (p.MainWindowHandle != IntPtr.Zero && GetWindowRect(p.MainWindowHandle, out r) && (r.R-r.L) > 400 && (r.B-r.T) > 400) {
        x = (r.L+r.R)/2; y = (r.T+r.B)/2; return true;
      }
    } catch {}
    return false;
  }
  // spawn, return ms until screen matches ref (>=92% of points), -1 on timeout
  public static double LooseMs = -1;
  public static double OpenUntil(string exe, string args, int[] reference, int sw, int sh, int timeoutMs) {
    LooseMs = -1;
    var t = Stopwatch.StartNew();
    var p = Process.Start(new ProcessStartInfo(exe, args) { UseShellExecute = false });
    IntPtr h = IntPtr.Zero; int n = 0;
    while (t.ElapsedMilliseconds < timeoutMs) {
      // the window cascades to a new position on every launch, so probe relative to its current centre
      if (h == IntPtr.Zero && (n++ % 4) == 0) { p.Refresh(); h = p.MainWindowHandle; }
      if (h == IntPtr.Zero) { Thread.Sleep(0); continue; }
      int cx = sw/2, cy = sh/2; RECT r;
      if (GetWindowRect(h, out r) && (r.R-r.L) > 400 && (r.B-r.T) > 400) { cx = (r.L+r.R)/2; cy = (r.T+r.B)/2; }
      var cur = Sig(cx, cy);
      if (LooseMs < 0 && Sim(cur, reference, 90) >= 0.90) LooseMs = t.Elapsed.TotalMilliseconds;
      LastSim = Sim(cur, reference, 36);
      if (LastSim >= 0.92) return t.Elapsed.TotalMilliseconds;
      Thread.Sleep(0);
    }
    return -1;
  }
  public static void Press(byte vk, bool ctrl) {
    if (ctrl) keybd_event(0x11, 0, 0, UIntPtr.Zero);
    keybd_event(vk, 0, 0, UIntPtr.Zero); keybd_event(vk, 0, 2, UIntPtr.Zero);
    if (ctrl) keybd_event(0x11, 0, 2, UIntPtr.Zero);
  }
  public static void Wheel(IntPtr h, int delta, int x, int y, bool ctrl) {
    long w = (((long)(delta & 0xFFFF)) << 16) + (ctrl ? 8L : 0L);
    long l = (((long)(y & 0xFFFF)) << 16) + (long)(x & 0xFFFF);
    PostMessage(h, 0x020A, (IntPtr)w, (IntPtr)l);
  }
  // act = 0 key, 1 wheel. Returns {firstChangeMs, settleMs}; quietMs = 0 -> return at first change.
  public static double[] Step(IntPtr hwnd, int act, byte vk, bool ctrl, int wheelDelta, int cx, int cy, int timeoutMs, int quietMs) {
    var prev = Sig(cx, cy);
    var t = Stopwatch.StartNew();
    if (act == 0) Press(vk, ctrl); else Wheel(hwnd, wheelDelta, cx, cy, ctrl);
    double first = -1, last = -1;
    while (t.ElapsedMilliseconds < timeoutMs) {
      var cur = Sig(cx, cy);
      double now = t.Elapsed.TotalMilliseconds;
      if (Changed(prev, cur)) { if (first < 0) first = now; last = now; prev = cur; if (quietMs == 0) return new[]{first, first}; }
      else if (first >= 0 && now - last > quietMs) break;
      Thread.Sleep(0);
    }
    return new[]{first, last};
  }
  public static void Focus(Process p) { p.Refresh(); if (p.MainWindowHandle != IntPtr.Zero) SetForegroundWindow(p.MainWindowHandle); }
  public static double Close(Process p, int timeoutMs) {
    keybd_event(0x12, 0, 0, UIntPtr.Zero); keybd_event(0x73, 0, 0, UIntPtr.Zero);
    var t = Stopwatch.StartNew();
    keybd_event(0x73, 0, 2, UIntPtr.Zero); keybd_event(0x12, 0, 2, UIntPtr.Zero);
    while (t.ElapsedMilliseconds < timeoutMs) { p.Refresh(); if (p.HasExited) return t.Elapsed.TotalMilliseconds; Thread.Sleep(0); }
    return -1;
  }
}
'@

$pn = [IO.Path]::GetFileNameWithoutExtension($Exe)
function Kill-All { Get-Process -Name $pn -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 700 }
function Fmt($a) { ($a | ForEach-Object { if ($_ -lt 0) { 'TIMEOUT' } else { '{0:N0}' -f $_ } }) -join ', ' }
function Med($a) { $v = @($a | Where-Object { $_ -ge 0 } | Sort-Object); if ($v.Count -eq 0) { return 'n/a' }; $m = $v.Count/2; if ($v.Count % 2) { '{0:N0}' -f $v[[int][math]::Floor($m)] } else { '{0:N0}' -f (($v[$m-1]+$v[$m])/2) } }
function Launch([string]$file) {
  $p = Start-Process $Exe -ArgumentList "`"$file`"" -PassThru
  Start-Sleep -Milliseconds 2500
  $procs = Get-Process -Name $pn -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 }
  $pp = if ($procs) { $procs | Select-Object -First 1 } else { $p }
  [R2]::Focus($pp); Start-Sleep -Milliseconds 300
  return $pp
}
$scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$all = Get-ChildItem $Folder -File -Filter $Filter | Sort-Object Name
Kill-All
"== $Kind ($pn), $($all.Count) files =="

# ---- 1. open 5 random images ----
$rand = if ($OpenOnly) { 1..10 | ForEach-Object { $all[63] } } else { $all | Get-Random -Count 5 -SetSeed $Seed }
if (-not $OpenOnly) { $rand = @($rand) * 2 }   # each random file twice: the 2nd pass shows per-file consistency
$opens = @(); $loose = @()
foreach ($f in $rand) {
  $pp = Launch $f.FullName
  $cx = 0; $cy = 0; [void][R2]::Centre($pp, [ref]$cx, [ref]$cy, $scr.Width, $scr.Height)
  $ref = [R2]::Sig($cx, $cy)
  Kill-All
  $t = [R2]::OpenUntil($Exe, "`"$($f.FullName)`"", $ref, $scr.Width, $scr.Height, 8000)
  $opens += $t
  $loose += [R2]::LooseMs
  if ($t -lt 0) { "   (open timeout: centre $cx,$cy last similarity {0:N2})" -f [R2]::LastSim }
  Kill-All
}
"1. OPEN random 5 (spawn -> picture matches final look) ms: " + (Fmt $opens) + "   median " + (Med $opens)
"1b. OPEN same, first time a recognisable picture is up (loose match) ms: " + (Fmt $loose) + "   median " + (Med $loose)
if ($OpenOnly) { exit }

# ---- 2. next 10 / back 10 ----
$start = $all[[math]::Min(60, [math]::Max(0, $all.Count - 25))]
$pp = Launch $start.FullName
$cx = 0; $cy = 0; [void][R2]::Centre($pp, [ref]$cx, [ref]$cy, $scr.Width, $scr.Height)
$h = $pp.MainWindowHandle
$nP = @(); $nS = @(); $bP = @(); $bS = @()
for ($i = 0; $i -lt 10; $i++) { Start-Sleep -Milliseconds $PaceMs; $r = [R2]::Step($h, 0, 0x27, $false, 0, $cx, $cy, 3000, 250); $nP += $r[0]; $nS += $r[1] }
for ($i = 0; $i -lt 10; $i++) { Start-Sleep -Milliseconds $PaceMs; $r = [R2]::Step($h, 0, 0x25, $false, 0, $cx, $cy, 3000, 250); $bP += $r[0]; $bS += $r[1] }
"2a. NEXT x10 paced ${PaceMs}ms, first-change ms: " + (Fmt $nP) + "   median " + (Med $nP) + "   | settled: median " + (Med $nS)
"2b. BACK x10 paced ${PaceMs}ms, first-change ms: " + (Fmt $bP) + "   median " + (Med $bP) + "   | settled: median " + (Med $bS)
$nR = @(); $bR = @()
for ($i = 0; $i -lt 10; $i++) { $r = [R2]::Step($h, 0, 0x27, $false, 0, $cx, $cy, 3000, 0); $nR += $r[0] }
for ($i = 0; $i -lt 10; $i++) { $r = [R2]::Step($h, 0, 0x25, $false, 0, $cx, $cy, 3000, 0); $bR += $r[0] }
"2c. NEXT x10 rapid (press as soon as previous shows), ms: " + (Fmt $nR) + "   median " + (Med $nR)
"2d. BACK x10 rapid, ms: " + (Fmt $bR) + "   median " + (Med $bR)

# ---- 3. zoom ----
Start-Sleep -Milliseconds 600
if ($Kind -eq 'nv') {
  $r1 = [R2]::Step($h, 0, 0x31, $false, 0, $cx, $cy, 3000, 250)                 # key 1 -> 100%
  Start-Sleep -Milliseconds 500
  $r2 = [R2]::Step($h, 1, 0, $false, 1382, $cx, $cy, 3000, 250)                 # wheel: 100% -> 500% (1.15^(1382/120) = 5.0)
  Start-Sleep -Milliseconds 500
  $r3 = [R2]::Step($h, 1, 0, $false, -1382, $cx, $cy, 3000, 250)                # wheel back to 100%
  Start-Sleep -Milliseconds 500
  $r4 = [R2]::Step($h, 0, 0x46, $false, 0, $cx, $cy, 3000, 250)                 # F -> fit
  "3. ZOOM (NuViewer) 1=100%: {0:N0}/{1:N0} ms | 100%->500%: {2:N0}/{3:N0} | 500%->100%: {4:N0}/{5:N0} | F fit: {6:N0}/{7:N0}   (first-change/settled)" -f $r1[0],$r1[1],$r2[0],$r2[1],$r3[0],$r3[1],$r4[0],$r4[1]
} else {
  # FastStone: probe what zoom inputs it reacts to, per-step latency
  $inK = @(); $outK = @()
  for ($i = 0; $i -lt 5; $i++) { $r = [R2]::Step($h, 0, 0x6B, $false, 0, $cx, $cy, 2000, 250); $inK += $r[0]; Start-Sleep -Milliseconds 300 }   # numpad +
  for ($i = 0; $i -lt 5; $i++) { $r = [R2]::Step($h, 0, 0x6D, $false, 0, $cx, $cy, 2000, 250); $outK += $r[0]; Start-Sleep -Milliseconds 300 }  # numpad -
  "3. ZOOM (FastStone) numpad+ x5 first-change ms: " + (Fmt $inK) + " | numpad- x5: " + (Fmt $outK) + "   (TIMEOUT = no visible reaction)"
}

# ---- 4. close ----
Start-Sleep -Milliseconds 600
$pp.Refresh()
$c = [R2]::Close($pp, 5000)
"4. CLOSE (Alt+F4 -> process gone) ms: " + ('{0:N0}' -f $c)
Kill-All
"polls: $([R2]::PollCount), avg capture ms: " + ('{0:N1}' -f ([R2]::SigMs / [math]::Max(1,[R2]::PollCount)))
