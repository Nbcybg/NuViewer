param(
  [Parameter(Mandatory)][string]$Exe,
  [Parameter(Mandatory)][string[]]$Files,
  [int]$RecordMs = 7000,
  [string]$ExtraArgs = ''
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Diagnostics; using System.Runtime.InteropServices; using System.Threading;
public static class R3 {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
  [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleDC(IntPtr dc);
  [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleBitmap(IntPtr dc, int w, int h);
  [DllImport("gdi32.dll")] static extern IntPtr SelectObject(IntPtr dc, IntPtr o);
  [DllImport("gdi32.dll")] static extern bool BitBlt(IntPtr d, int x, int y, int w, int h, IntPtr s, int sx, int sy, uint rop);
  [StructLayout(LayoutKind.Sequential)] struct BMI { public int size, w, h; public short planes, bpp; public int comp, sizeImg, xp, yp, used, imp; }
  [DllImport("gdi32.dll")] static extern int GetDIBits(IntPtr dc, IntPtr bmp, uint start, uint lines, byte[] bits, ref BMI bi, uint usage);

  static IntPtr dc = GetDC(IntPtr.Zero), mdc = IntPtr.Zero, mbmp = IntPtr.Zero;
  static byte[] buf;
  const int RW = 640, RH = 360, GX = 24, GY = 14;

  static int[] Sig(int cx, int cy) {
    if (mdc == IntPtr.Zero) { mdc = CreateCompatibleDC(dc); mbmp = CreateCompatibleBitmap(dc, RW, RH); SelectObject(mdc, mbmp); buf = new byte[RW*RH*4]; }
    BitBlt(mdc, 0, 0, RW, RH, dc, cx - RW/2, cy - RH/2, 0x40CC0020);
    var bi = new BMI { size = 40, w = RW, h = -RH, planes = 1, bpp = 32 };
    GetDIBits(mdc, mbmp, 0, (uint)RH, buf, ref bi, 0);
    var s = new int[GX*GY]; int k = 0;
    for (int j = 0; j < GY; j++) for (int i = 0; i < GX; i++) {
      int x = 8 + i*(RW-16)/(GX-1), y = 8 + j*(RH-16)/(GY-1), o = (y*RW + x)*4;
      s[k++] = buf[o] | (buf[o+1] << 8) | (buf[o+2] << 16);
    }
    return s;
  }
  static int D(int a, int b) { return Math.Abs((a&255)-(b&255)) + Math.Abs(((a>>8)&255)-((b>>8)&255)) + Math.Abs(((a>>16)&255)-((b>>16)&255)); }
  static double Sim(int[] a, int[] b, int tol) { int ok = 0; for (int i = 0; i < a.Length; i++) if (D(a[i], b[i]) <= tol) ok++; return (double)ok / a.Length; }

  // Spawn, record the picture area for recordMs, then say when it stopped changing.
  // Returns {settledStrictMs, settledLooseMs, samples}
  public static double[] Run(string exe, string args, int sw, int sh, int recordMs) {
    var T = new List<double>(); var S = new List<int[]>();
    var t = Stopwatch.StartNew();
    var p = Process.Start(new ProcessStartInfo(exe, args) { UseShellExecute = false });
    IntPtr h = IntPtr.Zero; int n = 0;
    while (t.ElapsedMilliseconds < recordMs) {
      if (h == IntPtr.Zero && (n++ % 4) == 0) { p.Refresh(); h = p.MainWindowHandle; }
      if (h == IntPtr.Zero) { Thread.Sleep(0); continue; }
      int cx = sw/2, cy = sh/2; RECT r;
      if (GetWindowRect(h, out r) && (r.R-r.L) > 400 && (r.B-r.T) > 400) { cx = (r.L+r.R)/2; cy = (r.T+r.B)/2; }
      S.Add(Sig(cx, cy)); T.Add(t.Elapsed.TotalMilliseconds);
    }
    if (S.Count < 3) return new double[]{-1, -1, S.Count};
    var fin = S[S.Count-1];
    return new double[]{ Settle(T, S, fin, 36, 0.92), Settle(T, S, fin, 90, 0.90), S.Count };
  }
  static double Settle(List<double> T, List<int[]> S, int[] fin, int tol, double frac) {
    int last = -1;
    for (int i = 0; i < S.Count; i++) if (Sim(S[i], fin, tol) < frac) last = i;
    if (last == S.Count-1) return -1;
    return T[last+1];
  }
}
'@

$pn = [IO.Path]::GetFileNameWithoutExtension($Exe)
function Kill-All { Get-Process -Name $pn -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 800 }
$scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
Kill-All
foreach ($f in $Files) {
  $name = [IO.Path]::GetFileName($f)
  $mb = (Get-Item $f).Length / 1MB
  $cold = [R3]::Run($Exe, ("`"$f`" " + $ExtraArgs).Trim(), $scr.Width, $scr.Height, $RecordMs)
  Kill-All
  $warm = [R3]::Run($Exe, ("`"$f`" " + $ExtraArgs).Trim(), $scr.Width, $scr.Height, $RecordMs)
  Kill-All
  "{0,-13} {1,5:N1} MB  COLD: picture final at {2,6:N0} ms (recognisable {3,6:N0})   WARM: {4,5:N0} ms (recognisable {5,5:N0})" -f $name, $mb, $cold[0], $cold[1], $warm[0], $warm[1]
}
