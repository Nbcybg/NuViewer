// Benchmark helper: screen recorder + real input + app launch, identical for every viewer.
// Time base = Stopwatch ticks (QueryPerformanceCounter). Nothing here knows about any viewer.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class BH
{
    // ---------------------------------------------------------------- native
    [DllImport("user32.dll")] static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
    [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleDC(IntPtr dc);
    [DllImport("gdi32.dll")] static extern IntPtr CreateDIBSection(IntPtr dc, ref BMI bi, uint usage, out IntPtr bits, IntPtr sect, uint off);
    [DllImport("gdi32.dll")] static extern IntPtr SelectObject(IntPtr dc, IntPtr o);
    [DllImport("gdi32.dll")] static extern bool StretchBlt(IntPtr d, int dx, int dy, int dw, int dh, IntPtr s, int sx, int sy, int sw, int sh, uint rop);
    [DllImport("gdi32.dll")] static extern int SetStretchBltMode(IntPtr dc, int mode);
    [StructLayout(LayoutKind.Sequential)] struct BMI { public int size, w, h; public short planes, bpp; public int comp, sizeImg, xp, yp, used, imp; }
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);

    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inp, int size);
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Explicit)] struct INPUTU { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public INPUTU u; }
    [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern bool GetCursorPos(out POINT p);
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }

    public delegate bool EP(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EP f, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr h);

    // ---------------------------------------------------------------- setup
    public static int SW, SH;           // screen size in pixels
    public const int RW = 640, RH = 360; // recorded size (1/4 of 2560x1440)
    static IntPtr sdc, mdc, bits;
    static byte[] raw = new byte[RW * RH * 4];

    public static void Init()
    {
        SetProcessDPIAware();
        SW = GetSystemMetrics(0); SH = GetSystemMetrics(1);
        sdc = GetDC(IntPtr.Zero);
        mdc = CreateCompatibleDC(sdc);
        var bi = new BMI { size = 40, w = RW, h = -RH, planes = 1, bpp = 32 };
        IntPtr bmp = CreateDIBSection(sdc, ref bi, 0, out bits, IntPtr.Zero, 0);
        SelectObject(mdc, bmp);
        SetStretchBltMode(mdc, 3); // COLORONCOLOR: nearest, no averaging, cheap
    }

    // One frame: whole screen, nearest-neighbour 1/4, as gray bytes.
    public static byte[] Grab()
    {
        StretchBlt(mdc, 0, 0, RW, RH, sdc, 0, 0, SW, SH, 0x00CC0020);
        Marshal.Copy(bits, raw, 0, raw.Length);
        var g = new byte[RW * RH];
        for (int i = 0, o = 0; i < g.Length; i++, o += 4)
            g[i] = (byte)((raw[o] + raw[o + 1] * 2 + raw[o + 2]) >> 2);
        return g;
    }

    public static int Diff(byte[] a, byte[] b, int thr)
    {
        int n = 0;
        for (int i = 0; i < a.Length; i++) { int d = a[i] - b[i]; if (d > thr || d < -thr) n++; }
        return n;
    }

    public static double NonBlack(byte[] f) { int n = 0; for (int i = 0; i < f.Length; i++) if (f[i] > 24) n++; return 100.0 * n / f.Length; }
    public static double Ms(long ticks) { return ticks * 1000.0 / Stopwatch.Frequency; }
    public static long Now() { return Stopwatch.GetTimestamp(); }

    // Full-resolution screenshot to a (scaled) PNG, for the human review sheets.
    public static void Shot(string path, int w)
    {
        using (var bmp = new Bitmap(SW, SH, PixelFormat.Format24bppRgb))
        {
            using (var g = Graphics.FromImage(bmp)) g.CopyFromScreen(0, 0, 0, 0, new Size(SW, SH));
            int h = SH * w / SW;
            using (var sm = new Bitmap(w, h, PixelFormat.Format24bppRgb))
            {
                using (var g = Graphics.FromImage(sm)) { g.InterpolationMode = System.Drawing.Drawing2D.InterpolationMode.HighQualityBicubic; g.DrawImage(bmp, 0, 0, w, h); }
                sm.Save(path, ImageFormat.Png);
            }
        }
    }

    // ---------------------------------------------------------------- recorder
    public class Rec
    {
        public List<long> T = new List<long>();
        public List<byte[]> F = new List<byte[]>();
        volatile bool stop; Thread th;
        public long LastChange;               // ticks of the last frame that differed from its predecessor
        public int ChangeThr = 10, ChangePx = 30;
        public void Start()
        {
            LastChange = Now();
            th = new Thread(Loop) { IsBackground = true, Priority = ThreadPriority.AboveNormal };
            th.Start();
        }
        void Loop()
        {
            byte[] prev = null;
            while (!stop)
            {
                var f = Grab(); long t = Now();
                if (prev != null && Diff(prev, f, ChangeThr) > ChangePx) LastChange = t;
                prev = f;
                lock (T) { T.Add(t); F.Add(f); }
            }
        }
        public void Stop() { stop = true; th.Join(); }
        public double QuietMs() { return Ms(Now() - LastChange); }
        public int Count { get { lock (T) return T.Count; } }
        public double FrameMs() { lock (T) { if (T.Count < 20) return 0; return Ms(T[T.Count - 1] - T[T.Count - 11]) / 10.0; } }

        // t0 = moment of the action. Returns { firstChangeMs, doneMs, framesAfter, finalDiffPx, quiet(1/0) }
        // doneMs = time of the first frame after which no frame differs (> px pixels) from the final frame.
        public double[] Analyse(long t0, int px, int thr)
        {
            lock (T)
            {
                int n = T.Count; if (n < 3) return new double[] { -1, -1, 0, 0, 0 };
                int pre = 0; for (int i = 0; i < n; i++) if (T[i] <= t0) pre = i; else break;
                var fin = F[n - 1];
                int last = -1;                       // last frame that still differs from final
                for (int i = pre; i < n - 1; i++) if (Diff(F[i], fin, thr) > px) last = i;
                int first = -1;                      // first frame that differs from the pre-action frame
                for (int i = pre + 1; i < n; i++) if (Diff(F[i], F[pre], thr) > px) { first = i; break; }
                double firstMs = first < 0 ? -1 : Ms(T[first] - t0);
                double doneMs;
                if (last < 0) doneMs = first < 0 ? 0 : firstMs;   // never differed from final (instant) -> first change
                else doneMs = Ms(T[last + 1] - t0);
                double quiet = Ms(T[n - 1] - T[Math.Max(pre, Math.Max(last, 0))]) ;
                return new double[] { firstMs, doneMs, n - pre, first < 0 ? -1 : Diff(F[pre], fin, thr), last < 0 ? 0 : 1 };
            }
        }
        // Distinct "looks" after t0: list of {ms, px-diff-to-final} every ~step ms, for the report.
        public string Trace(long t0, int stepMs, int thr)
        {
            lock (T)
            {
                int n = T.Count; var sb = new StringBuilder(); var fin = F[n - 1]; double next = 0;
                for (int i = 0; i < n; i++)
                {
                    double ms = Ms(T[i] - t0); if (ms < next) continue; next = ms + stepMs;
                    sb.Append(((int)ms) + ":" + Diff(F[i], fin, thr) + " ");
                    if (ms > 6000) break;
                }
                return sb.ToString();
            }
        }
        public double Mean(int a, int b) { return 0; }
        // Compare frame closest to time 'ms' after t0 against the final frame -> px
        public int Fin(int thr) { lock (T) { int n = T.Count; return Diff(F[n - 1], F[Math.Max(0, n - 20)], thr); } }
        public byte[] Last() { lock (T) { return F[F.Count - 1]; } }
    }

    // ---------------------------------------------------------------- input
    const uint KEYEVENTF_KEYUP = 2, KEYEVENTF_EXT = 1;
    static bool IsExt(int vk) { return vk == 0x25 || vk == 0x26 || vk == 0x27 || vk == 0x28 || vk == 0x21 || vk == 0x22 || vk == 0x23 || vk == 0x24 || vk == 0x2D || vk == 0x2E; }

    static INPUT Key(int vk, bool up)
    {
        var i = new INPUT { type = 1 };
        i.u.ki = new KEYBDINPUT { vk = (ushort)vk, scan = (ushort)MapVirtualKey((uint)vk, 0), flags = (up ? KEYEVENTF_KEYUP : 0) | (IsExt(vk) ? KEYEVENTF_EXT : 0) };
        return i;
    }
    // Press-and-release each key in one SendInput call, all in a single batch (no gap). Returns timestamp taken right before.
    public static long Keys(int[] vks)
    {
        var l = new List<INPUT>();
        foreach (int vk in vks) { l.Add(Key(vk, false)); l.Add(Key(vk, true)); }
        long t = Now();
        SendInput((uint)l.Count, l.ToArray(), Marshal.SizeOf(typeof(INPUT)));
        return t;
    }
    // Same keys but paced (gap ms between them) - for the "human fast" variant.
    public static long KeysPaced(int[] vks, int gapMs)
    {
        long t = Now();
        foreach (int vk in vks) { Keys(new int[] { vk }); if (gapMs > 0) Thread.Sleep(gapMs); }
        return t;
    }
    public static long KeyCombo(int mod, int vk)
    {
        var l = new List<INPUT> { Key(mod, false), Key(vk, false), Key(vk, true), Key(mod, true) };
        long t = Now(); SendInput((uint)l.Count, l.ToArray(), Marshal.SizeOf(typeof(INPUT))); return t;
    }
    public static void MoveMouse(int x, int y) { SetCursorPos(x, y); }
    public static bool MouseAt(int x, int y) { POINT p; GetCursorPos(out p); return Math.Abs(p.X - x) < 3 && Math.Abs(p.Y - y) < 3; }
    public static long Click(int x, int y, bool dbl)
    {
        SetCursorPos(x, y); Thread.Sleep(60);
        var l = new List<INPUT>();
        for (int k = 0; k < (dbl ? 2 : 1); k++)
        {
            var d = new INPUT { type = 0 }; d.u.mi = new MOUSEINPUT { flags = 2 };
            var u = new INPUT { type = 0 }; u.u.mi = new MOUSEINPUT { flags = 4 };
            l.Add(d); l.Add(u);
        }
        long t = Now(); SendInput((uint)l.Count, l.ToArray(), Marshal.SizeOf(typeof(INPUT))); return t;
    }

    // ---------------------------------------------------------------- windows
    public class Win { public IntPtr H; public uint Pid; public string Cls, Title; public RECT R; public override string ToString() { return Pid + " | " + Cls + " | " + Title + " | " + R.L + "," + R.T + " " + (R.R - R.L) + "x" + (R.B - R.T); } }
    public static List<Win> Windows(int minW)
    {
        var l = new List<Win>();
        EnumWindows((h, x) =>
        {
            if (!IsWindowVisible(h)) return true;
            RECT r; GetWindowRect(h, out r); if (r.R - r.L < minW || r.B - r.T < 50) return true;
            var s = new StringBuilder(256); GetWindowText(h, s, 256); var c = new StringBuilder(128); GetClassName(h, c, 128);
            uint pid; GetWindowThreadProcessId(h, out pid);
            l.Add(new Win { H = h, Pid = pid, Cls = c.ToString(), Title = s.ToString(), R = r }); return true;
        }, IntPtr.Zero);
        return l;
    }
    public static IntPtr Foreground() { return GetForegroundWindow(); }
    public static uint PidOf(IntPtr h) { uint p; GetWindowThreadProcessId(h, out p); return p; }
    public static string ClassOf(IntPtr h) { var c = new StringBuilder(128); GetClassName(h, c, 128); return c.ToString(); }
    public static string TitleOf(IntPtr h) { var c = new StringBuilder(256); GetWindowText(h, c, 256); return c.ToString(); }

    public static bool Focus(IntPtr h)
    {
        if (IsIconic(h)) ShowWindow(h, 9);
        IntPtr fg = GetForegroundWindow();
        if (fg == h) return true;   // already in front: send nothing (an Alt tap would open the viewer's menu bar)
        uint pid; uint ft = GetWindowThreadProcessId(fg, out pid); uint me = GetCurrentThreadId();
        keybd_alt();
        if (ft != me) AttachThreadInput(me, ft, true);
        BringWindowToTop(h); SetForegroundWindow(h);
        if (ft != me) AttachThreadInput(me, ft, false);
        Thread.Sleep(30);
        return GetForegroundWindow() == h;
    }
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    static void keybd_alt() { keybd_event(0x12, 0, 0, UIntPtr.Zero); keybd_event(0x12, 0, 2, UIntPtr.Zero); } // lets SetForegroundWindow through
    public static void Close(IntPtr h) { PostMessage(h, 0x0010, IntPtr.Zero, IntPtr.Zero); }

    // ---------------------------------------------------------------- black backdrop: hides everything behind the viewers
    public class CoverForm : System.Windows.Forms.Form
    {
        // Activatable on purpose: the harness owns the foreground, so every viewer it starts is allowed to come to the front.
        protected override System.Windows.Forms.CreateParams CreateParams { get { var cp = base.CreateParams; cp.ExStyle |= 0x80; return cp; } }
    }
    static CoverForm cover;
    public static void StartCover()
    {
        var t = new Thread(() =>
        {
            cover = new CoverForm { FormBorderStyle = System.Windows.Forms.FormBorderStyle.None, StartPosition = System.Windows.Forms.FormStartPosition.Manual, Bounds = new Rectangle(0, 0, SW, SH), BackColor = Color.Black, ShowInTaskbar = false };
            System.Windows.Forms.Application.Run(cover);
        });
        t.SetApartmentState(ApartmentState.STA); t.IsBackground = true; t.Start();
    }
    public static void StopCover() { if (cover != null) cover.BeginInvoke((Action)(() => cover.Close())); }
    // Put the backdrop on top of everything (before launching an app, so the app opens above it).
    public static void CoverRaise() { if (cover != null) SetWindowPos(cover.Handle, new IntPtr(0), 0, 0, 0, 0, 0x13); }
    public static void Maximize(IntPtr h) { ShowWindow(h, 3); }
    public static bool CoverFocus() { if (cover == null) return false; SetWindowPos(cover.Handle, new IntPtr(0), 0, 0, 0, 0, 0x13); return Focus(cover.Handle); }
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint f);

    // ---------------------------------------------------------------- UWP activation (for Image Simple Viewer)
    [ComImport, Guid("2e941141-7f97-4756-ba1d-9decde894a3d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IApplicationActivationManager
    {
        int ActivateApplication([In, MarshalAs(UnmanagedType.LPWStr)] string appUserModelId, [In, MarshalAs(UnmanagedType.LPWStr)] string arguments, [In] int options, out uint processId);
        int ActivateForFile([In, MarshalAs(UnmanagedType.LPWStr)] string appUserModelId, IntPtr itemArray, [In, MarshalAs(UnmanagedType.LPWStr)] string verb, out uint processId);
        int ActivateForProtocol([In, MarshalAs(UnmanagedType.LPWStr)] string appUserModelId, IntPtr itemArray, out uint processId);
    }
    [ComImport, Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C")] class ApplicationActivationManager { }
    [DllImport("shell32.dll", CharSet = CharSet.Unicode, PreserveSig = false)] static extern void SHCreateItemFromParsingName(string path, IntPtr bc, ref Guid riid, out IntPtr ppv);
    [DllImport("shell32.dll", PreserveSig = false)] static extern void SHCreateShellItemArrayFromShellItem(IntPtr item, ref Guid riid, out IntPtr ppv);

    public static uint ActivateFile(string aumid, string path)
    {
        Guid iShellItem = new Guid("43826d1e-e718-42ee-bc55-a1e261c37bfe");
        Guid iArray = new Guid("b63ea76d-1f85-456f-a19c-48159efa858b");
        IntPtr item, arr; uint pid;
        SHCreateItemFromParsingName(path, IntPtr.Zero, ref iShellItem, out item);
        SHCreateShellItemArrayFromShellItem(item, ref iArray, out arr);
        var mgr = (IApplicationActivationManager)new ApplicationActivationManager();
        int hr = mgr.ActivateForFile(aumid, arr, "open", out pid);
        if (hr != 0) throw new Exception("ActivateForFile hr=0x" + hr.ToString("X"));
        return pid;
    }
    public static uint Activate(string aumid)
    {
        uint pid; var mgr = (IApplicationActivationManager)new ApplicationActivationManager();
        int hr = mgr.ActivateApplication(aumid, "", 0, out pid);
        if (hr != 0) throw new Exception("ActivateApplication hr=0x" + hr.ToString("X"));
        return pid;
    }
}
