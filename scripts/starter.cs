using System; using System.Diagnostics; using System.IO; using System.Windows.Forms;
// Starts start.ps1, which always points at the current build.
// Build: csc /target:winexe /r:System.Windows.Forms.dll starter.cs  (put the exe next to start.ps1)
static class P {
  static readonly string D = AppDomain.CurrentDomain.BaseDirectory.TrimEnd('\\'), S = D + @"\start.ps1";
  static bool Box(string t) { MessageBox.Show(t, "GTA VI starten"); return false; }
  static bool Running() {
    foreach (var p in Process.GetProcesses())
      if (p.ProcessName.StartsWith("victory-", StringComparison.OrdinalIgnoreCase)) return true;
    return false;
  }
  static Process Go(string sh, bool check) {
    string a = check ? "-Command \"exit 7\"" : "-File \"" + S + "\"";
    return Process.Start(new ProcessStartInfo(sh, "-NoProfile -ExecutionPolicy Bypass " + a)
      { UseShellExecute = false, CreateNoWindow = true, WorkingDirectory = D });
  }
  [STAThread] static int Main(string[] args) {
    bool check = args.Length > 0 && args[0] == "--check", mine;
    using (var m = new System.Threading.Mutex(true, "Local\\victory-starter", out mine)) {
    if (!File.Exists(S)) { if (!check) Box("Niet gevonden: " + S); return 2; }
    if (!mine || Running()) { if (!check) Box("Het spel draait al."); return 3; }
    var t = Stopwatch.StartNew(); Process p;
    string pw = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), @"Microsoft\WindowsApps\pwsh.exe");
    try { p = Go(pw, check); } catch { p = Go("powershell.exe", check); }
    p.WaitForExit();
    if (check) return p.ExitCode;
    if (t.Elapsed.TotalSeconds < 20) Box("Het spel sloot binnen 20 seconden.\nZie " + D + @"\start.log");
    return 0;
    }
  }
}
