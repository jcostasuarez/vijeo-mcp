# Sonda del portapapeles de Vijeo (Win32): lista las ventanas del marco, manda WM_COMMAND al marco de la
# primera instancia de Vijeo-Frame y vuelca los formatos del portapapeles. No distingue instancias: úsalo con
# una sola abierta (nunca con la del usuario). Uso: clip_ui.ps1 -Action windows|cmd|dump [-Command 0xC03D] [-OutDir <dir>]
param(
  [ValidateSet('copy','dump','cmd','windows')][string]$Action = 'dump',
  [int]$Command = 0,
  [string]$OutDir = "$env:TEMP\vijeo-clip"
)
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class W {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumProc p, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool OpenClipboard(IntPtr h);
  [DllImport("user32.dll")] public static extern bool CloseClipboard();
  [DllImport("user32.dll")] public static extern uint EnumClipboardFormats(uint f);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClipboardFormatName(uint f, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern IntPtr GetClipboardData(uint f);
  [DllImport("user32.dll")] public static extern IntPtr GetClipboardOwner();
  [DllImport("kernel32.dll")] public static extern IntPtr GlobalLock(IntPtr h);
  [DllImport("kernel32.dll")] public static extern bool GlobalUnlock(IntPtr h);
  [DllImport("kernel32.dll")] public static extern UIntPtr GlobalSize(IntPtr h);
  public static string Cls(IntPtr h){ var s=new StringBuilder(256); GetClassName(h,s,256); return s.ToString(); }
  public static string Txt(IntPtr h){ var s=new StringBuilder(512); GetWindowText(h,s,512); return s.ToString(); }
  public static List<IntPtr> TopOf(uint pid){ var r=new List<IntPtr>(); EnumWindows((h,l)=>{ uint p; GetWindowThreadProcessId(h,out p); if(p==pid && IsWindowVisible(h)) r.Add(h); return true; }, IntPtr.Zero); return r; }
  public static List<IntPtr> Children(IntPtr parent){ var r=new List<IntPtr>(); EnumChildWindows(parent,(h,l)=>{ r.Add(h); return true; }, IntPtr.Zero); return r; }
}
'@
$proc = Get-Process Vijeo-Frame -ErrorAction SilentlyContinue | Select-Object -First 1
$frame = [IntPtr]::Zero
if ($proc) { foreach ($h in [W]::TopOf([uint32]$proc.Id)) { if ([W]::Cls($h) -like 'Afx*Frame*' -or [W]::Txt($h) -like '*Vijeo*') { $frame = $h } } }
switch ($Action) {
  'windows' {
    "frame: $frame '$([W]::Txt($frame))' $([W]::Cls($frame))"
    foreach ($c in [W]::Children($frame)) { if ([W]::IsWindowVisible($c)) { "{0,-10} id={1,-6} {2,-40} {3}" -f $c, [W]::GetDlgCtrlID($c), [W]::Cls($c), [W]::Txt($c) } }
  }
  'cmd' { "WM_COMMAND 0x{0:X} -> {1}" -f $Command, $frame; [void][W]::SendMessage($frame, 0x0111, [IntPtr]$Command, [IntPtr]::Zero) }
  'dump' {
    New-Item -ItemType Directory -Force $OutDir | Out-Null
    if (-not [W]::OpenClipboard([IntPtr]::Zero)) { throw 'OpenClipboard failed' }
    try {
      $owner = [W]::GetClipboardOwner(); $op = 0; [void][W]::GetWindowThreadProcessId($owner, [ref]$op)
      "owner hwnd=$owner pid=$op class=$([W]::Cls($owner))"
      $f = 0
      while (($f = [W]::EnumClipboardFormats($f)) -ne 0) {
        $sb = New-Object System.Text.StringBuilder 256
        $n = [W]::GetClipboardFormatName($f, $sb, 256)
        $name = if ($n -gt 0) { $sb.ToString() } else { "std_$f" }
        $h = [W]::GetClipboardData($f)
        $size = 0
        if ($h -ne [IntPtr]::Zero -and $f -notin 2,3,8,14,17) {
          $p = [W]::GlobalLock($h)
          if ($p -ne [IntPtr]::Zero) {
            $size = [int][W]::GlobalSize($h).ToUInt64()
            $bytes = New-Object byte[] $size
            [Runtime.InteropServices.Marshal]::Copy($p, $bytes, 0, $size)
            [void][W]::GlobalUnlock($h)
            $safe = ($name -replace '[^\w\-\.]', '_')
            [IO.File]::WriteAllBytes((Join-Path $OutDir "$f-$safe.bin"), $bytes)
          }
        }
        "{0,6} {1,-55} handle={2} size={3}" -f $f, $name, $h, $size
      }
    } finally { [void][W]::CloseClipboard() }
  }
}
