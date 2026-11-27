# Vuelca el portapapeles OLE (OleGetClipboard): cada formato con su medio (HGLOBAL, IStream o IStorage) a
# ficheros. Vijeo ofrece sus formatos 63INDE_VIJEO_* en diferido: GetClipboardData no los da.
# Uso: clip_ole_dump.ps1 -OutDir <dir>   (§13.1 del inventario de APIs)
param([Parameter(Mandatory)][string]$OutDir)
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Collections.Generic;
using System.Text;
[ComImport, Guid("0000000B-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IStorageX {
  [PreserveSig] int CreateStream(); [PreserveSig] int OpenStream(); [PreserveSig] int CreateStorage(); [PreserveSig] int OpenStorage();
  [PreserveSig] int CopyTo(int ciidExclude, IntPtr rgiid, IntPtr snb, IStorageX dest);
  [PreserveSig] int MoveElementTo(); [PreserveSig] int Commit(int flags); [PreserveSig] int Revert();
}
public static class OC {
  [DllImport("ole32.dll")] static extern int OleGetClipboard(out IDataObject d);
  [DllImport("ole32.dll")] static extern int StgCreateDocfile([MarshalAs(UnmanagedType.LPWStr)] string name, int mode, int res, out IStorageX stg);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClipboardFormatName(uint f, StringBuilder s, int n);
  [DllImport("kernel32.dll")] static extern IntPtr GlobalLock(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool GlobalUnlock(IntPtr h);
  [DllImport("kernel32.dll")] static extern UIntPtr GlobalSize(IntPtr h);
  [DllImport("ole32.dll")] static extern void ReleaseStgMedium(ref STGMEDIUM m);
  static string Name(short f){ var s=new StringBuilder(256); int n=GetClipboardFormatName((uint)(ushort)f,s,256); return n>0?s.ToString():("std_"+(ushort)f); }
  static string Safe(string s){ var sb=new StringBuilder(); foreach(var c in s) sb.Append(char.IsLetterOrDigit(c)||c=='-'||c=='.'?c:'_'); return sb.ToString(); }
  public static List<string> Dump(string dir){
    var log=new List<string>(); Directory.CreateDirectory(dir);
    IDataObject d; int hr=OleGetClipboard(out d); log.Add("OleGetClipboard hr=0x"+hr.ToString("X8"));
    IEnumFORMATETC en=d.EnumFormatEtc(DATADIR.DATADIR_GET);
    var arr=new FORMATETC[1]; var got=new int[1];
    var list=new List<FORMATETC>();
    while(en.Next(1,arr,got)==0) list.Add(arr[0]);
    foreach(var fe in list){
      string name=Name(fe.cfFormat);
      log.Add(string.Format("format {0} ({1}) tymed={2} aspect={3} lindex={4}",name,(ushort)fe.cfFormat,fe.tymed,fe.dwAspect,fe.lindex));
      foreach(TYMED t in new[]{TYMED.TYMED_HGLOBAL,TYMED.TYMED_ISTREAM,TYMED.TYMED_ISTORAGE}){
        if((fe.tymed & t)==0) continue;
        var f2=fe; f2.tymed=t; STGMEDIUM m;
        try { d.GetData(ref f2, out m); } catch(Exception ex){ log.Add("  GetData "+t+" failed: "+ex.Message); continue; }
        string path=Path.Combine(dir,Safe(name));
        if(m.tymed==TYMED.TYMED_HGLOBAL){ var p=GlobalLock(m.unionmember); int n=(int)GlobalSize(m.unionmember).ToUInt64(); var b=new byte[n]; Marshal.Copy(p,b,0,n); GlobalUnlock(m.unionmember); File.WriteAllBytes(path+".hglobal.bin",b); log.Add("  HGLOBAL "+n+" bytes"); }
        else if(m.tymed==TYMED.TYMED_ISTREAM){ var s=(IStream)Marshal.GetObjectForIUnknown(m.unionmember); s.Seek(0,0,IntPtr.Zero); var ms=new MemoryStream(); var buf=new byte[65536]; IntPtr pr=Marshal.AllocHGlobal(4); while(true){ s.Read(buf,buf.Length,pr); int r=Marshal.ReadInt32(pr); if(r<=0) break; ms.Write(buf,0,r);} Marshal.FreeHGlobal(pr); File.WriteAllBytes(path+".stream.bin",ms.ToArray()); log.Add("  ISTREAM "+ms.Length+" bytes"); }
        else if(m.tymed==TYMED.TYMED_ISTORAGE){ var src=(IStorageX)Marshal.GetObjectForIUnknown(m.unionmember); IStorageX dst; int h1=StgCreateDocfile(path+".storage.cfb",0x1012,0,out dst); int h2=src.CopyTo(0,IntPtr.Zero,IntPtr.Zero,dst); dst.Commit(0); Marshal.ReleaseComObject(dst); log.Add("  ISTORAGE -> "+path+".storage.cfb create=0x"+h1.ToString("X8")+" copy=0x"+h2.ToString("X8")); }
        try { ReleaseStgMedium(ref m); } catch {}
      }
    }
    return log;
  }
}
'@
[OC]::Dump($OutDir)
