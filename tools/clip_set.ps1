# Pone el contenido de un fichero en el portapapeles con un formato registrado (por defecto el de objetos
# de Vijeo 6.3). Sustituye el portapapeles del usuario. Uso: clip_set.ps1 -File <bytes> [-Format <nombre>]
param([Parameter(Mandatory)][string]$File, [string]$Format = '63INDE_VIJEO_ObjectClipboard')
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class SC {
  [DllImport("user32.dll")] static extern bool OpenClipboard(IntPtr h);
  [DllImport("user32.dll")] static extern bool CloseClipboard();
  [DllImport("user32.dll")] static extern bool EmptyClipboard();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern uint RegisterClipboardFormat(string n);
  [DllImport("user32.dll")] static extern IntPtr SetClipboardData(uint f, IntPtr h);
  [DllImport("kernel32.dll")] static extern IntPtr GlobalAlloc(uint flags, UIntPtr n);
  [DllImport("kernel32.dll")] static extern IntPtr GlobalLock(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool GlobalUnlock(IntPtr h);
  public static string Set(string fmt, byte[] data){
    uint f = RegisterClipboardFormat(fmt);
    IntPtr h = GlobalAlloc(0x0002, (UIntPtr)data.Length);
    IntPtr p = GlobalLock(h); Marshal.Copy(data, 0, p, data.Length); GlobalUnlock(h);
    if(!OpenClipboard(IntPtr.Zero)) return "OpenClipboard failed";
    EmptyClipboard();
    IntPtr r = SetClipboardData(f, h);
    CloseClipboard();
    return string.Format("format {0} (id {1}) {2} bytes -> {3}", fmt, f, data.Length, r == IntPtr.Zero ? "FAILED" : "ok");
  }
}
'@
[SC]::Set($Format, [IO.File]::ReadAllBytes($File))
