param([string]$Exe, [string]$Vmf, [string]$Out, [int]$Attente = 20)
# Opens $Vmf in Hammer++, waits, captures its 3D view (top-left view pane) into $Out, then closes it with WM_CLOSE.
# Dialogs: the recovery question after a forced close gets "No"; load progress bars are left alone; any other dialog
# is reported (title and text) and gets "OK". Prints one line per event; the last line is "vue3d <w>x<h>" or "echec ...".
Add-Type -ReferencedAssemblies System.Drawing @"
using System; using System.Runtime.InteropServices; using System.Drawing; using System.Drawing.Imaging; using System.Text; using System.Collections.Generic;
public static class HmHpp {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  public delegate bool CB(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
  [DllImport("user32.dll")] public static extern bool EnumWindows(CB cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, CB cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  static string Texte(IntPtr h) { var s = new StringBuilder(1024); GetWindowText(h, s, 1024); return s.ToString(); }
  static string Classe(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
  public static List<Tuple<IntPtr, string, string>> Dialogues(uint pid) {
    var l = new List<Tuple<IntPtr, string, string>>();
    EnumWindows((h, x) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h) && Classe(h) == "#32770") {
        var sb = new StringBuilder();
        EnumChildWindows(h, (c, y) => { var t = Texte(c); if (t.Length > 0) sb.Append(t.Replace("\r", " ").Replace("\n", " ") + " | "); return true; }, IntPtr.Zero);
        l.Add(Tuple.Create(h, Texte(h), sb.ToString()));
      } return true; }, IntPtr.Zero);
    return l;
  }
  // the 3D view: the top-left view pane (untitled "Afx:" child of at least 200x150)
  public static IntPtr Vue3d(IntPtr main) {
    IntPtr best = IntPtr.Zero; long cle = long.MaxValue;
    EnumChildWindows(main, (h, x) => { RECT r; GetWindowRect(h, out r);
      if (IsWindowVisible(h) && Classe(h).StartsWith("Afx:") && Texte(h).Length == 0 && r.R - r.L >= 200 && r.B - r.T >= 150) {
        long k = (long)r.T * 100000 + r.L; if (k < cle) { cle = k; best = h; } }
      return true; }, IntPtr.Zero);
    return best;
  }
  public static string Prendre(IntPtr h, string f) {
    RECT r; GetWindowRect(h, out r);
    var b = new Bitmap(Math.Max(1, r.R - r.L), Math.Max(1, r.B - r.T), PixelFormat.Format32bppArgb);
    using (var g = Graphics.FromImage(b)) { var dc = g.GetHdc(); PrintWindow(h, dc, 2); g.ReleaseHdc(dc); }
    b.Save(f, ImageFormat.Png);
    return b.Width + "x" + b.Height;
  }
}
"@
$nom = [IO.Path]::GetFileName($Vmf)
$vus = @{}
function Dialogues($p) {
  foreach ($d in [HmHpp]::Dialogues([uint32]$p.Id)) {
    if ($d.Item2 -like "Reading*" -or $d.Item3 -match '\d+%') { continue }
    if ($d.Item3 -like "*did not shut down correctly*") { [HmHpp]::SendMessage($d.Item1, 0x0111, [IntPtr]7, [IntPtr]::Zero) | Out-Null; "recuperation : non"; continue }
    if (-not $vus.ContainsKey([string]$d.Item1)) { $vus[[string]$d.Item1] = 1; "dialogue : $($d.Item2) : $($d.Item3)" }
    # "save changes?" when closing: No (the copy is a temporary file anyway)
    $reponse = if ($d.Item3 -match '(?i)save') { 7 } else { 1 }
    [HmHpp]::SendMessage($d.Item1, 0x0111, [IntPtr]$reponse, [IntPtr]::Zero) | Out-Null
  }
}
$t0 = Get-Date
$p = Start-Process $Exe -ArgumentList "`"$Vmf`"" -WorkingDirectory (Split-Path $Exe) -PassThru
while ($true) {
  Start-Sleep -Milliseconds 500
  $p.Refresh()
  if ($p.HasExited) { "echec : Hammer++ s'est ferme"; exit 1 }
  Dialogues $p
  if ($p.MainWindowTitle -like "*$nom*") { break }
  if (((Get-Date) - $t0).TotalSeconds -gt 240) { "echec : la carte ne s'ouvre pas ('$($p.MainWindowTitle)')"; Stop-Process -Id $p.Id -Force; exit 1 }
}
"ouverte en $([int]((Get-Date) - $t0).TotalSeconds) s"
$fin = (Get-Date).AddSeconds($Attente)
while ((Get-Date) -lt $fin) { Start-Sleep -Milliseconds 500; Dialogues $p }
# the 3D view is only drawn (and captured) once the window is shown in front
[HmHpp]::ShowWindow($p.MainWindowHandle, 3) | Out-Null
[HmHpp]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
Start-Sleep -Seconds 3
Dialogues $p
$v = [HmHpp]::Vue3d($p.MainWindowHandle)
$taille = if ($v -ne [IntPtr]::Zero) { [HmHpp]::Prendre($v, $Out) } else { "" }
[HmHpp]::PostMessage($p.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
$t1 = Get-Date
while (-not $p.HasExited -and ((Get-Date) - $t1).TotalSeconds -lt 15) { Start-Sleep -Milliseconds 300; Dialogues $p; $p.Refresh() }
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force; "fermeture forcee" }
if ($taille) { "vue3d $taille" } else { "echec : vue 3D introuvable" }
