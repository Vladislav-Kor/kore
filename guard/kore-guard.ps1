<#
  KORE guard: keeps the total CPU load of the PC at or below the limit from guard.conf.
  When the PC goes over the limit, the biggest background CPU users get a hard CPU cap through a Windows Job Object
  (JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, percent of the whole CPU). Caps go up again when there is headroom.
  Processes are never killed or suspended, only slowed down.
  What the user works with is left alone: the active window, the names in guard.conf (browsers) and the folders in
  guard.conf (games). A capped process that becomes the active window gets its cap removed at once.
  Runs elevated in the user's session from the task "KORE guard" (install-guard.ps1): session 0 has no active window.
  Settings live in guard.conf, the KORE app edits them and the guard rereads the file while running.
  State for the app goes to guard.status on every tick.
  Stop it with a file guard.stop next to this script: the guard removes every cap and exits. Killing the process
  instead leaves the caps in place until the capped processes restart.
#>
param(
    [double]$Limit = 75,      # used until guard.conf is read: total CPU load the PC must not exceed, %
    [double]$MinCap = 8,      # used until guard.conf is read: never cap a process below this, % of the whole CPU
    [int]$IntervalMs = 2000,
    [int]$Seconds = 0,        # stop after this many seconds (0 = run forever); for tests
    # Never capped, guard.conf cannot remove these: Windows system processes and Defender.
    [string]$AlwaysExempt = 'Idle,System,Registry,Secure System,Memory Compression,smss,csrss,wininit,winlogon,services,lsass,svchost,dwm,audiodg,fontdrvhost,explorer,MsMpEng,NisSrv'
)

$code = @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class KorCpuGuard
{
    [DllImport("kernel32.dll")] static extern bool GetSystemTimes(out long idle, out long kernel, out long user);
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")] static extern bool GetProcessTimes(IntPtr process, out long created, out long exited, out long kernel, out long user);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern IntPtr CreateJobObject(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr job, int infoClass, ref CpuRateInfo info, int size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool QueryInformationJobObject(IntPtr job, int infoClass, out Accounting info, int size, IntPtr returned);
    [DllImport("kernel32.dll")] static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern bool QueryFullProcessImageName(IntPtr process, int flags, StringBuilder name, ref int size);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);

    const uint PROCESS_TERMINATE = 0x0001, PROCESS_SET_QUOTA = 0x0100, PROCESS_QUERY_LIMITED = 0x1000;
    const int JobBasicAccounting = 1, JobCpuRateControl = 15;
    const uint RATE_ENABLE = 0x1, RATE_HARD_CAP = 0x4;

    [StructLayout(LayoutKind.Sequential)] struct CpuRateInfo { public uint Flags; public uint Rate; }
    [StructLayout(LayoutKind.Sequential)] struct Accounting
    {
        public long TotalUserTime, TotalKernelTime, ThisPeriodUserTime, ThisPeriodKernelTime;
        public uint PageFaults, TotalProcesses, ActiveProcesses, TerminatedProcesses;
    }

    // One job object = one capped process plus the children it starts later.
    class Group { public IntPtr Job; public string Name; public string Path; public int Pid; public double Cap; public long LastCpu; public double Usage; public bool Active; }
    class Sample { public int Pid; public string Name; public long Created; public double Usage; }
    class Unit { public Group G; public Sample S; public double Usage; }

    // Settings from guard.conf. "exempt" and "protected_paths" add to the built-in list, they cannot shrink it.
    class Settings
    {
        public bool Enabled = true, ProtectForeground = true;
        public double Limit, MinCap;
        public HashSet<string> Exempt = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        public List<string> Paths = new List<string>();
    }

    static readonly List<Group> groups = new List<Group>();
    static readonly HashSet<string> failed = new HashSet<string>();
    static Dictionary<int, long[]> prevCpu = new Dictionary<int, long[]>();
    static string logPath;
    static HashSet<string> always;
    static Settings settings;
    static DateTime confStamp = DateTime.MinValue;

    public static void Run(double limit, double minCap, int intervalMs, string exemptCsv, string log, string stopPath, string confPath, string statusPath, int seconds)
    {
        logPath = log;
        bool fresh;
        Mutex single;
        try { single = new Mutex(true, @"Global\KorCpuGuard", out fresh); }
        catch (Exception) { single = new Mutex(true, @"Local\KorCpuGuard", out fresh); }
        if (!fresh) { Log("уже запущен другой экземпляр - выхожу"); return; }
        try { Process.GetCurrentProcess().PriorityClass = ProcessPriorityClass.AboveNormal; } catch (Exception) { }
        if (File.Exists(stopPath)) File.Delete(stopPath);

        always = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (string e in exemptCsv.Split(',')) if (e.Trim().Length > 0) always.Add(e.Trim());
        settings = new Settings { Limit = limit, MinCap = minCap };
        settings.Exempt.UnionWith(always);
        ReloadSettings(confPath);
        int self = Process.GetCurrentProcess().Id;
        int ncpu = Environment.ProcessorCount;
        int session = Process.GetCurrentProcess().SessionId;
        Log(string.Format("старт: предел {0}%, минимальный потолок процесса {1}%, логических ядер {2}, сессия {3}", settings.Limit, settings.MinCap, ncpu, session));
        if (session == 0) Log("сессия 0: активное окно не видно, защищаю только по списку и папкам");

        long idle0, kern0, user0;
        GetSystemTimes(out idle0, out kern0, out user0);
        var clock = Stopwatch.StartNew();
        long wall0 = 0;
        SampleProcesses(0, self);
        int cooldown = 0, calm = 0;
        DateTime nextBeat = DateTime.Now.AddMinutes(10);
        double peak = 0;

        while (!File.Exists(stopPath) && (seconds <= 0 || clock.Elapsed.TotalSeconds < seconds))
        {
            Thread.Sleep(intervalMs);
            try
            {
                long idle, kern, user;
                GetSystemTimes(out idle, out kern, out user);
                long all = (kern - kern0) + (user - user0), free = idle - idle0;  // kernel time includes idle time
                idle0 = idle; kern0 = kern; user0 = user;
                double total = all > 0 ? 100.0 * (all - free) / all : 0;
                peak = Math.Max(peak, total);
                long wall = clock.Elapsed.Ticks;  // 100 ns units, like FILETIME
                double span = (double)(wall - wall0) * ncpu;
                wall0 = wall;
                ReloadSettings(confPath);
                double lim = settings.Limit;
                int active = settings.ProtectForeground ? ForegroundPid() : 0;
                ProtectActive(active);
                List<Sample> procs = SampleProcesses(span, self);
                UpdateGroups(span);

                if (cooldown > 0) cooldown--;
                if (!settings.Enabled) { ReleaseAll(); calm = 0; }
                else if (total > lim - 3)
                {
                    calm = 0;
                    if (cooldown == 0) { Cut(procs, total, total - (lim - 8), settings.MinCap, active); cooldown = 1; }
                }
                else if (total < lim - 10)
                {
                    if (++calm >= 2) { Raise((lim - 7) - total); calm = 0; }
                }
                else calm = 0;

                WriteStatus(statusPath, total, peak, session, active);
                if (DateTime.Now >= nextBeat)
                {
                    Log(string.Format("жив: сейчас {0:0}%, пик за 10 мин {1:0}%, ограничено: {2}", total, peak, Describe()));
                    nextBeat = DateTime.Now.AddMinutes(10);
                    peak = 0;
                }
            }
            catch (Exception ex) { Log("ошибка: " + ex.Message); }
        }
        foreach (Group g in groups) { SetCap(g, 0); CloseHandle(g.Job); }
        groups.Clear();
        if (File.Exists(stopPath)) File.Delete(stopPath);
        if (File.Exists(statusPath)) File.Delete(statusPath);
        Log("остановлен, все потолки сняты");
        single.ReleaseMutex();
    }

    // Rereads guard.conf when it changes. A broken file keeps the previous settings: one bad save must not disarm the guard.
    static void ReloadSettings(string path)
    {
        if (!File.Exists(path)) return;
        DateTime stamp = File.GetLastWriteTimeUtc(path);
        if (stamp == confStamp) return;
        var next = new Settings { Enabled = settings.Enabled, ProtectForeground = settings.ProtectForeground, Limit = settings.Limit, MinCap = settings.MinCap };
        next.Exempt.UnionWith(always);
        try
        {
            foreach (string raw in File.ReadAllLines(path, Encoding.UTF8))
            {
                string line = raw.Trim();
                int eq = line.IndexOf('=');
                if (eq < 1 || line[0] == '#') continue;
                string key = line.Substring(0, eq).Trim(), value = line.Substring(eq + 1).Trim();
                if (key == "enabled") next.Enabled = value == "1";
                else if (key == "protect_foreground") next.ProtectForeground = value == "1";
                else if (key == "limit") next.Limit = Bound(value, 30, 100);
                else if (key == "min_cap") next.MinCap = Bound(value, 1, 50);
                else if (key == "exempt") { foreach (string e in value.Split(',')) if (e.Trim().Length > 0) next.Exempt.Add(e.Trim()); }
                else if (key == "protected_paths") { foreach (string e in value.Split(',')) if (e.Trim().Length > 0) next.Paths.Add(e.Trim()); }
            }
        }
        catch (Exception ex) { confStamp = stamp; Log("guard.conf не прочитан, оставляю прежние настройки: " + ex.Message); return; }
        bool first = confStamp == DateTime.MinValue;
        confStamp = stamp;
        settings = next;
        if (first) return;
        Log(string.Format("настройки: {0}, предел {1}%, минимальный потолок {2}%, активное окно {3}", next.Enabled ? "включён" : "выключен", next.Limit, next.MinCap, next.ProtectForeground ? "защищено" : "без защиты"));
        foreach (Group g in groups)
            if (g.Cap > 0 && IsSpared(g.Name, g.Path) && SetCap(g, 0)) Log(string.Format("в исключениях: {0} ({1}) потолок снят", g.Name, g.Pid));
    }

    static double Bound(string text, double low, double high)
    {
        return Math.Max(low, Math.Min(high, double.Parse(text, System.Globalization.CultureInfo.InvariantCulture)));
    }

    static bool IsSpared(string name, string path)
    {
        if (settings.Exempt.Contains(name)) return true;
        if (string.IsNullOrEmpty(path)) return false;
        foreach (string part in settings.Paths) if (path.IndexOf(part, StringComparison.OrdinalIgnoreCase) >= 0) return true;
        return false;
    }

    // 0 when nothing is active or the guard sits in session 0, where there is no desktop to look at.
    static int ForegroundPid()
    {
        IntPtr window = GetForegroundWindow();
        if (window == IntPtr.Zero) return 0;
        uint pid;
        GetWindowThreadProcessId(window, out pid);
        return (int)pid;
    }

    // The active window never stays capped: a game can land in a job through the launcher that started it.
    static void ProtectActive(int pid)
    {
        foreach (Group g in groups) g.Active = false;
        if (pid == 0 || groups.Count == 0) return;
        IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED, false, pid);
        if (h == IntPtr.Zero) return;
        foreach (Group g in groups)
        {
            bool inside;
            if (!IsProcessInJob(h, g.Job, out inside) || !inside) continue;
            g.Active = true;
            if (g.Cap > 0 && SetCap(g, 0)) Log(string.Format("активное окно: {0} ({1}) потолок снят", g.Name, g.Pid));
        }
        CloseHandle(h);
    }

    static void ReleaseAll()
    {
        foreach (Group g in groups)
            if (g.Cap > 0 && SetCap(g, 0)) Log(string.Format("сторож выключен: {0} ({1}) потолок снят", g.Name, g.Pid));
    }

    static string ImagePath(int pid)
    {
        IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED, false, pid);
        if (h == IntPtr.Zero) return "";
        var name = new StringBuilder(1024);
        int size = name.Capacity;
        bool ok = QueryFullProcessImageName(h, 0, name, ref size);
        CloseHandle(h);
        return ok ? name.ToString() : "";
    }

    // Takes the excess away from the biggest users first; nobody goes below minCap.
    static void Cut(List<Sample> procs, double total, double excess, double minCap, int active)
    {
        var units = new List<Unit>();
        foreach (Group g in groups) if (!g.Active && !IsSpared(g.Name, g.Path)) units.Add(new Unit { G = g, Usage = g.Usage });
        foreach (Sample s in procs)
            if (s.Usage >= 3 && s.Pid != active && !failed.Contains(Key(s)) && !InGroup(s.Pid) && !IsSpared(s.Name, ImagePath(s.Pid)))
                units.Add(new Unit { S = s, Usage = s.Usage });
        units.Sort(delegate(Unit a, Unit b) { return b.Usage.CompareTo(a.Usage); });
        foreach (Unit u in units)
        {
            if (excess < 1) break;
            double cut = Math.Min(excess, u.Usage - minCap);
            if (cut < 1 || (u.G != null && u.G.Cap > 0 && u.Usage - cut > u.G.Cap - 1)) continue;
            Group g = u.G ?? Capture(u.S);
            if (g == null || !SetCap(g, u.Usage - cut)) continue;
            excess -= cut;
            Log(string.Format("ЦП {0:0}% > предела: {1} ({2}) ел {3:0}% -> потолок {4:0}%", total, g.Name, g.Pid, u.Usage, g.Cap));
        }
    }

    // Gives headroom back to capped processes that are pressing against their cap.
    static void Raise(double room)
    {
        var capped = new List<Group>();
        foreach (Group g in groups) if (g.Cap > 0 && g.Usage >= g.Cap * 0.85) capped.Add(g);
        capped.Sort(delegate(Group a, Group b) { return a.Cap.CompareTo(b.Cap); });
        foreach (Group g in capped)
        {
            if (room < 1) break;
            double up = Math.Min(10, room);
            double old = g.Cap;
            if (!SetCap(g, g.Cap + up >= 95 ? 0 : g.Cap + up)) continue;
            room -= up;
            Log(g.Cap == 0
                ? string.Format("есть запас: {0} ({1}) потолок снят", g.Name, g.Pid)
                : string.Format("есть запас: {0} ({1}) потолок {2:0}% -> {3:0}%", g.Name, g.Pid, old, g.Cap));
        }
    }

    static List<Sample> SampleProcesses(double span, int self)
    {
        var list = new List<Sample>();
        var seen = new Dictionary<int, long[]>();
        foreach (Process p in Process.GetProcesses())
        {
            int pid = p.Id;
            string name = p.ProcessName;
            p.Dispose();
            if (pid == 0 || pid == 4 || pid == self || settings.Exempt.Contains(name)) continue;
            IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED, false, pid);
            if (h == IntPtr.Zero) continue;
            long created, exited, kernel, user;
            bool ok = GetProcessTimes(h, out created, out exited, out kernel, out user);
            CloseHandle(h);
            if (!ok) continue;
            long cpu = kernel + user;
            long[] prev;
            double usage = 0;
            if (span > 0 && prevCpu.TryGetValue(pid, out prev) && prev[0] == created) usage = 100.0 * (cpu - prev[1]) / span;
            seen[pid] = new long[] { created, cpu };
            list.Add(new Sample { Pid = pid, Name = name, Created = created, Usage = usage });
        }
        prevCpu = seen;
        return list;
    }

    static void UpdateGroups(double span)
    {
        for (int i = groups.Count - 1; i >= 0; i--)
        {
            Group g = groups[i];
            Accounting a;
            if (!QueryInformationJobObject(g.Job, JobBasicAccounting, out a, Marshal.SizeOf(typeof(Accounting)), IntPtr.Zero)) continue;
            if (a.ActiveProcesses == 0) { CloseHandle(g.Job); groups.RemoveAt(i); continue; }
            long cpu = a.TotalUserTime + a.TotalKernelTime;
            g.Usage = span > 0 ? 100.0 * (cpu - g.LastCpu) / span : 0;
            g.LastCpu = cpu;
        }
    }

    static Group Capture(Sample s)
    {
        IntPtr h = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED, false, s.Pid);
        if (h == IntPtr.Zero) { Fail(s, Marshal.GetLastWin32Error()); return null; }
        long created, exited, kernel, user;
        if (!GetProcessTimes(h, out created, out exited, out kernel, out user) || created != s.Created) { CloseHandle(h); return null; }
        IntPtr job = CreateJobObject(IntPtr.Zero, null);
        bool ok = job != IntPtr.Zero && AssignProcessToJobObject(job, h);
        int err = Marshal.GetLastWin32Error();
        CloseHandle(h);
        if (!ok) { if (job != IntPtr.Zero) CloseHandle(job); Fail(s, err); return null; }
        var g = new Group { Job = job, Name = s.Name, Path = ImagePath(s.Pid), Pid = s.Pid };
        Accounting a;
        if (QueryInformationJobObject(job, JobBasicAccounting, out a, Marshal.SizeOf(typeof(Accounting)), IntPtr.Zero))
            g.LastCpu = a.TotalUserTime + a.TotalKernelTime;
        groups.Add(g);
        return g;
    }

    // cap = percent of the whole CPU; 0 removes the cap.
    static bool SetCap(Group g, double cap)
    {
        var info = new CpuRateInfo();
        if (cap > 0)
        {
            info.Flags = RATE_ENABLE | RATE_HARD_CAP;
            info.Rate = (uint)Math.Max(1, Math.Min(10000, Math.Round(cap * 100)));
        }
        if (!SetInformationJobObject(g.Job, JobCpuRateControl, ref info, Marshal.SizeOf(typeof(CpuRateInfo))))
        {
            Log(string.Format("не смог поставить потолок {0} ({1}): ошибка {2}", g.Name, g.Pid, Marshal.GetLastWin32Error()));
            return false;
        }
        g.Cap = cap > 0 ? info.Rate / 100.0 : 0;
        return true;
    }

    static bool InGroup(int pid)
    {
        if (groups.Count == 0) return false;
        IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED, false, pid);
        if (h == IntPtr.Zero) return false;
        bool found = false;
        foreach (Group g in groups)
        {
            bool inside;
            if (IsProcessInJob(h, g.Job, out inside) && inside) { found = true; break; }
        }
        CloseHandle(h);
        return found;
    }

    static void Fail(Sample s, int err)
    {
        failed.Add(Key(s));
        Log(string.Format("нельзя ограничить {0} ({1}): ошибка {2}{3}", s.Name, s.Pid, err,
            err == 5 ? " - нет прав, нужен запуск от SYSTEM/администратора" : ""));
    }

    static string Key(Sample s) { return s.Pid + ":" + s.Created; }

    static string Describe()
    {
        var parts = new List<string>();
        foreach (Group g in groups) if (g.Cap > 0) parts.Add(string.Format("{0} {1:0}%", g.Name, g.Cap));
        return parts.Count > 0 ? string.Join(", ", parts.ToArray()) : "никто";
    }

    // A snapshot for the KORE app; a missed write only leaves the old values for one tick.
    static void WriteStatus(string path, double total, double peak, int session, int active)
    {
        string name = active != 0 ? Path.GetFileNameWithoutExtension(ImagePath(active)) : "";
        var parts = new List<string>();
        foreach (Group g in groups) if (g.Cap > 0) parts.Add(string.Format(System.Globalization.CultureInfo.InvariantCulture, "{0} {1:0}", g.Name, g.Cap));
        string text = string.Format(System.Globalization.CultureInfo.InvariantCulture,
            "time={0}\nversion=2\nenabled={1}\nlimit={2:0}\nmin_cap={3:0}\ntotal={4:0}\npeak={5:0}\nsession={6}\nforeground={7}\ncapped={8}\n",
            DateTime.Now.ToString("yyyy-MM-ddTHH:mm:ss"), settings.Enabled ? 1 : 0, settings.Limit, settings.MinCap, total, peak, session, name, string.Join(";", parts.ToArray()));
        try { File.WriteAllText(path, text, new UTF8Encoding(false)); } catch (Exception) { }
    }

    static void Log(string line)
    {
        try
        {
            var f = new FileInfo(logPath);
            if (f.Exists && f.Length > 1000000) File.Copy(logPath, logPath + ".old", true);
            if (f.Exists && f.Length > 1000000) File.Delete(logPath);
            File.AppendAllText(logPath, DateTime.Now.ToString("dd.MM HH:mm:ss ") + line + Environment.NewLine);
        }
        catch (Exception) { }
    }
}
'@

Add-Type -TypeDefinition $code -Language CSharp
[KorCpuGuard]::Run($Limit, $MinCap, $IntervalMs, $AlwaysExempt, (Join-Path $PSScriptRoot 'guard.log'),
    (Join-Path $PSScriptRoot 'guard.stop'), (Join-Path $PSScriptRoot 'guard.conf'), (Join-Path $PSScriptRoot 'guard.status'), $Seconds)
