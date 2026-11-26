# Prueba del export XML de proyecto de la UI (comando 0xC147 de Vijeo-Frame.exe, FUN_004ae510 en 6.3).
# Lanza una instancia propia por COM, abre una COPIA del proyecto, envía WM_COMMAND al marco principal,
# rellena el diálogo "Guardar como" y vuelca todos los diálogos que aparezcan.
# No toca otras instancias de Vijeo-Frame (sólo actúa sobre el PID que crea).
#
# Uso: ui_export_probe.ps1 -ProjectPath <copia.vdz> -OutFile <salida.vxml|.zdat> [-Command 0xC147] [-DumpOnly]
param(
  [Parameter(Mandatory)] [string]$ProjectPath,
  [Parameter(Mandatory)] [string]$OutFile,
  [int]$Command = 0xC147,
  [int]$WaitSeconds = 60,
  [switch]$DumpOnly,
  [switch]$KeepOpen
)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class W {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetMenu(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetMenuString(IntPtr m, uint id, StringBuilder s, int n, uint flags);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, string l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] public static extern IntPtr SendMessageSb(IntPtr h, uint msg, IntPtr w, StringBuilder l);
  public static List<IntPtr> TopLevel(uint pid) {
    var r = new List<IntPtr>();
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) r.Add(h); return true; }, IntPtr.Zero);
    return r;
  }
  public static List<IntPtr> Children(IntPtr parent) {
    var r = new List<IntPtr>();
    EnumChildWindows(parent, (h, l) => { r.Add(h); return true; }, IntPtr.Zero);
    return r;
  }
  public static string Cls(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
  public static string Txt(IntPtr h) {
    var s = new StringBuilder(1024);
    SendMessageSb(h, 0x000D, (IntPtr)1024, s); // WM_GETTEXT
    return s.ToString();
  }
}
'@
function Log($m) { [Console]::Out.WriteLine(("[{0:HH:mm:ss.fff}] {1}" -f (Get-Date), $m)) }
function Dump-Window([IntPtr]$h, [string]$tag) {
  Log ("{0} hwnd=0x{1:X} class={2} text='{3}'" -f $tag, $h.ToInt64(), [W]::Cls($h), [W]::Txt($h))
  foreach ($c in [W]::Children($h)) {
    $t = [W]::Txt($c); if ($t.Length -gt 120) { $t = $t.Substring(0, 120) + '…' }
    Log ("    id={0,-6} class={1,-22} vis={2} text='{3}'" -f [W]::GetDlgCtrlID($c), [W]::Cls($c), [W]::IsWindowVisible($c), $t)
  }
}
function Dialogs([uint32]$procId) { @([W]::TopLevel($procId) | Where-Object { [W]::Cls($_) -eq '#32770' }) }

$before = @(Get-Process Vijeo-Frame -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
Log "Vijeo-Frame previos (no se tocan): $($before -join ',')"
$vd = New-Object -ComObject 'VijeoFrame.VijeoDesigner2'
$sw = [Diagnostics.Stopwatch]::StartNew(); $avail = 0
while ($sw.Elapsed.TotalSeconds -lt $WaitSeconds) { try { $avail = $vd.IsAvailable() } catch {}; if ($avail -ne 0) { break }; Start-Sleep -Milliseconds 500 }
$mine = @(Get-Process Vijeo-Frame | Where-Object { $before -notcontains $_.Id })
if ($mine.Count -ne 1) { throw "no se identifica la instancia propia: $($mine.Id -join ',')" }
$procId = [uint32]$mine[0].Id
Log "instancia propia PID=$procId IsAvailable=$avail"
try {
  $p = $vd.OpenProject($ProjectPath)
  Log "OpenProject -> $($p.Name)"
  Start-Sleep -Seconds 2
  $frames = @([W]::TopLevel($procId) | Where-Object { [W]::Cls($_) -ne '#32770' -and [W]::Txt($_) -ne '' })
  foreach ($f in $frames) {
    Log ("ventana: 0x{0:X} class={1} '{2}'" -f $f.ToInt64(), [W]::Cls($f), [W]::Txt($f))
    $m = [W]::GetMenu($f)
    if ($m -ne [IntPtr]::Zero) { $s = New-Object Text.StringBuilder 256; [void][W]::GetMenuString($m, [uint32]$Command, $s, 256, 0); Log "  menú 0x$('{0:X}' -f $Command): '$s'" }
  }
  $main = $frames | Select-Object -First 1
  Log ("WM_COMMAND 0x{0:X} -> 0x{1:X}" -f $Command, $main.ToInt64())
  [void][W]::PostMessage($main, 0x0111, [IntPtr]$Command, [IntPtr]::Zero)
  $handled = @{}
  $sw.Restart()
  while ($sw.Elapsed.TotalSeconds -lt $WaitSeconds) {
    Start-Sleep -Milliseconds 700
    foreach ($d in (Dialogs $procId)) {
      $key = $d.ToInt64(); if ($handled[$key]) { continue }; $handled[$key] = $true
      Dump-Window $d 'DIALOGO'
      if ($DumpOnly) { continue }
      $kids = [W]::Children($d)
      $fname = $kids | Where-Object { [W]::GetDlgCtrlID($_) -eq 1148 -and [W]::Cls($_) -eq 'Edit' } | Select-Object -First 1
      if (-not $fname) { $fname = $kids | Where-Object { [W]::Cls($_) -eq 'Edit' } | Select-Object -First 1 }
      if ($fname -and ([W]::Txt($d) -match '(?i)save|guardar|export')) {
        $filter = $kids | Where-Object { [W]::GetDlgCtrlID($_) -eq 1136 } | Select-Object -First 1
        if ($filter) {
          $n = [W]::SendMessage($filter, 0x0146, [IntPtr]::Zero, [IntPtr]::Zero).ToInt32() # CB_GETCOUNT
          $ext = [IO.Path]::GetExtension($OutFile).TrimStart('.')
          for ($i = 0; $i -lt $n; $i++) {
            $sb = New-Object Text.StringBuilder 512; [void][W]::SendMessageSb($filter, 0x0148, [IntPtr]$i, $sb) # CB_GETLBTEXT
            Log "  filtro[$i] = '$sb'"
            if ($sb.ToString() -match [regex]::Escape($ext)) { [void][W]::SendMessage($filter, 0x014E, [IntPtr]$i, [IntPtr]::Zero) } # CB_SETCURSEL
          }
        }
        [void][W]::SendMessage($fname, 0x000C, [IntPtr]::Zero, $OutFile) # WM_SETTEXT
        $ok = $kids | Where-Object { [W]::GetDlgCtrlID($_) -eq 1 -and [W]::Cls($_) -eq 'Button' } | Select-Object -First 1
        Log "  nombre -> $OutFile ; clic en Guardar"
        [void][W]::PostMessage($ok, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero) # BM_CLICK
      }
    }
    if ((Test-Path $OutFile) -and (Dialogs $procId).Count -eq 0) { Start-Sleep -Seconds 2; if ((Dialogs $procId).Count -eq 0) { break } }
  }
  if (Test-Path $OutFile) { Log "SALIDA: $OutFile ($((Get-Item $OutFile).Length) bytes)" } else { Log "SALIDA: no generada" }
  foreach ($d in (Dialogs $procId)) { Dump-Window $d 'DIALOGO PENDIENTE' }
}
finally {
  if (-not $KeepOpen) {
    if ((Dialogs $procId).Count -gt 0) {
      Log "quedan diálogos modales: se mata la instancia propia $procId"
      Stop-Process -Id $procId -Force
    } else {
      try { $p.Close() } catch { Log "Close: $($_.Exception.Message)" }
      try { $vd.Exit() } catch { Log "Exit: $($_.Exception.Message)" }
    }
  }
}
