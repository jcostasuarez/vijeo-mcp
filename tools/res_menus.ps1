# Vuelca los menús (MENU y MENUEX) y aceleradores de binarios de recursos (p.ej. Vijeo-Frame\Main.ENU),
# con el id de cada comando para mandarlo con WM_COMMAND.
# Uso: res_menus.ps1 -Out <fichero.txt> [-Files <dll/ENU>...]   (§13 del inventario de APIs)
param([string[]]$Files = @('C:\Program Files (x86)\Schneider Electric\Vijeo-Designer 6.3\Vijeo-Frame\Main.ENU'), [Parameter(Mandatory)][string]$Out)
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class R {
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern IntPtr LoadLibraryEx(string f, IntPtr h, uint flags);
  public delegate bool EnumNameProc(IntPtr m, IntPtr type, IntPtr name, IntPtr l);
  [DllImport("kernel32.dll")] public static extern bool EnumResourceNames(IntPtr m, IntPtr type, EnumNameProc p, IntPtr l);
  [DllImport("kernel32.dll")] public static extern IntPtr FindResource(IntPtr m, IntPtr name, IntPtr type);
  [DllImport("kernel32.dll")] public static extern IntPtr LoadResource(IntPtr m, IntPtr r);
  [DllImport("kernel32.dll")] public static extern IntPtr LockResource(IntPtr r);
  [DllImport("kernel32.dll")] public static extern uint SizeofResource(IntPtr m, IntPtr r);
  public static List<IntPtr> Names(IntPtr m, int type){ var r=new List<IntPtr>(); EnumResourceNames(m,(IntPtr)type,(a,b,n,l)=>{ r.Add(n); return true; },IntPtr.Zero); return r; }
  public static byte[] Get(IntPtr m, IntPtr name, int type){ var h=FindResource(m,name,(IntPtr)type); if(h==IntPtr.Zero) return null; var g=LoadResource(m,h); var p=LockResource(g); var n=(int)SizeofResource(m,h); var b=new byte[n]; Marshal.Copy(p,b,0,n); return b; }
  static string WStr(byte[] b, ref int o){ var sb=new StringBuilder(); while(o+1<b.Length){ char c=(char)BitConverter.ToUInt16(b,o); o+=2; if(c==0) break; sb.Append(c);} return sb.ToString(); }
  // Parse standard MENU template (version 0) and MENUEX (version 1)
  public static List<string> ParseMenu(byte[] b){
    var r=new List<string>(); ushort ver=BitConverter.ToUInt16(b,0); ushort off=BitConverter.ToUInt16(b,2);
    if(ver==0){ int o=4+off; var depth=new Stack<bool>(); ParseStd(b,ref o,0,r); }
    else { int o=4+off; ParseEx(b,ref o,0,r); }
    return r;
  }
  static void ParseStd(byte[] b, ref int o, int lvl, List<string> r){
    while(o<b.Length){
      ushort flags=BitConverter.ToUInt16(b,o); o+=2;
      if((flags&0x10)!=0){ string t=WStr(b,ref o); r.Add(new string(' ',lvl*2)+"[+] "+t); ParseStd(b,ref o,lvl+1,r); }
      else { ushort id=BitConverter.ToUInt16(b,o); o+=2; string t=WStr(b,ref o); r.Add(new string(' ',lvl*2)+string.Format("0x{0:X4} {1}",id,t)); }
      if((flags&0x80)!=0) return;
    }
  }
  static void ParseEx(byte[] b, ref int o, int lvl, List<string> r){
    while(o<b.Length){
      uint type=BitConverter.ToUInt32(b,o); uint state=BitConverter.ToUInt32(b,o+4); uint id=BitConverter.ToUInt32(b,o+8); ushort res=BitConverter.ToUInt16(b,o+12); o+=14;
      string t=WStr(b,ref o); o=(o+3)&~3;
      if((res&1)!=0){ o+=4; r.Add(new string(' ',lvl*2)+"[+] "+t); ParseEx(b,ref o,lvl+1,r);} else r.Add(new string(' ',lvl*2)+string.Format("0x{0:X4} {1}",id,t));
      if((res&0x80)!=0) return;
    }
  }
  public static List<string> ParseAccel(byte[] b){ var r=new List<string>(); for(int o=0;o+8<=b.Length;o+=8){ ushort f=BitConverter.ToUInt16(b,o); ushort k=BitConverter.ToUInt16(b,o+2); ushort id=BitConverter.ToUInt16(b,o+4); string m=((f&8)!=0?"Ctrl+":"")+((f&4)!=0?"Shift+":"")+((f&0x10)!=0?"Alt+":""); string key=(f&1)!=0?("VK_0x"+k.ToString("X2")+((k>=0x30&&k<=0x5A)?"('"+(char)k+"')":"")):("'"+(char)k+"'"); r.Add(string.Format("0x{0:X4} {1}{2}",id,m,key)); if((f&0x80)!=0) break; } return r; }
}
'@
$sb = New-Object System.Text.StringBuilder
foreach ($f in $Files) {
  $m = [R]::LoadLibraryEx($f, [IntPtr]::Zero, 0x2)
  if ($m -eq [IntPtr]::Zero) { [void]$sb.AppendLine("!! cannot load $f"); continue }
  [void]$sb.AppendLine("##### $f")
  foreach ($n in [R]::Names($m, 4)) {
    $nm = if ($n.ToInt64() -lt 0x10000) { $n.ToInt64() } else { [Runtime.InteropServices.Marshal]::PtrToStringUni($n) }
    [void]$sb.AppendLine("=== MENU $nm")
    try { foreach ($l in [R]::ParseMenu([R]::Get($m, $n, 4))) { [void]$sb.AppendLine($l) } } catch { [void]$sb.AppendLine("  parse error: $_") }
  }
  foreach ($n in [R]::Names($m, 9)) {
    $nm = if ($n.ToInt64() -lt 0x10000) { $n.ToInt64() } else { [Runtime.InteropServices.Marshal]::PtrToStringUni($n) }
    [void]$sb.AppendLine("=== ACCEL $nm")
    foreach ($l in [R]::ParseAccel([R]::Get($m, $n, 9))) { [void]$sb.AppendLine("  $l") }
  }
}
$sb.ToString() | Out-File -FilePath $Out -Encoding utf8
"written $Out"
