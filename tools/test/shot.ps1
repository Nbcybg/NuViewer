param([string]$Exe, [string]$File, [string]$Out, [int]$WaitMs = 2500, [string]$ExtraArgs = "")
# Starts a program on a file, waits, captures its window with PrintWindow (no real input is sent anywhere), closes it.
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Drawing; using System.Drawing.Imaging; using System.Text;
public class Cap {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr h);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  public struct RECT { public int L, T, R, B; }
  public static IntPtr Find(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h) && GetWindowTextLength(h) > 0) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
  }
  public static void Shot(IntPtr h, string path) {
    RECT r; GetWindowRect(h, out r);
    int w = r.R - r.L, hgt = r.B - r.T;
    using (Bitmap bmp = new Bitmap(w, hgt)) {
      using (Graphics g = Graphics.FromImage(bmp)) { IntPtr dc = g.GetHdc(); PrintWindow(h, dc, 2); g.ReleaseHdc(dc); }
      bmp.Save(path, ImageFormat.Png);
    }
  }
}
"@ -ReferencedAssemblies System.Drawing
$args2 = @($File)
if ($ExtraArgs -ne "") { $args2 += $ExtraArgs.Split(" ") }
$p = Start-Process -FilePath $Exe -ArgumentList $args2 -PassThru
Start-Sleep -Milliseconds $WaitMs
$h = [Cap]::Find([uint32]$p.Id)
if ($h -eq [IntPtr]::Zero) { "NO WINDOW (exited: $($p.HasExited))"; exit 1 }
[Cap]::Shot($h, $Out)
"captured $Out"
Stop-Process -Id $p.Id -Force
