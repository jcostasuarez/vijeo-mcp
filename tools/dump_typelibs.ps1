param([string]$Root, [string]$OutDir)
$src = @"
using System;
using System.Text;
using System.Runtime.InteropServices.ComTypes;
using Marshal = System.Runtime.InteropServices.Marshal;
using DllImportAttribute = System.Runtime.InteropServices.DllImportAttribute;
using CharSet = System.Runtime.InteropServices.CharSet;
using VarEnum = System.Runtime.InteropServices.VarEnum;
public static class TlbDump {
  [DllImport("oleaut32.dll", CharSet=CharSet.Unicode, PreserveSig=false)]
  static extern void LoadTypeLibEx(string file, int regkind, out ITypeLib tl);
  static string VtName(ITypeInfo ti, TYPEDESC td) {
    VarEnum vt = (VarEnum)td.vt;
    if (vt == VarEnum.VT_PTR) { TYPEDESC inner = (TYPEDESC)Marshal.PtrToStructure(td.lpValue, typeof(TYPEDESC)); return VtName(ti, inner) + "*"; }
    if (vt == VarEnum.VT_SAFEARRAY) { TYPEDESC inner = (TYPEDESC)Marshal.PtrToStructure(td.lpValue, typeof(TYPEDESC)); return "SAFEARRAY(" + VtName(ti, inner) + ")"; }
    if (vt == VarEnum.VT_USERDEFINED) { try { ITypeInfo r; ti.GetRefTypeInfo((int)td.lpValue.ToInt64(), out r); string n,d,h; int c; r.GetDocumentation(-1, out n, out d, out c, out h); return n; } catch { return "?udt"; } }
    return vt.ToString().Replace("VT_","");
  }
  public static string Dump(string file) {
    ITypeLib tl;
    try { LoadTypeLibEx(file, 2, out tl); } catch { return null; }
    var sb = new StringBuilder();
    string ln, ld, lh; int lc; tl.GetDocumentation(-1, out ln, out ld, out lc, out lh);
    IntPtr pla; tl.GetLibAttr(out pla); TYPELIBATTR la = (TYPELIBATTR)Marshal.PtrToStructure(pla, typeof(TYPELIBATTR)); tl.ReleaseTLibAttr(pla);
    sb.AppendFormat("LIBRARY {0} {{{1}}} v{2}.{3}  // {4}\n", ln, la.guid, la.wMajorVerNum, la.wMinorVerNum, ld);
    int n = tl.GetTypeInfoCount();
    for (int i = 0; i < n; i++) {
      ITypeInfo ti; tl.GetTypeInfo(i, out ti);
      string tn, td, th; int tc; ti.GetDocumentation(-1, out tn, out td, out tc, out th);
      IntPtr pta; ti.GetTypeAttr(out pta); TYPEATTR ta = (TYPEATTR)Marshal.PtrToStructure(pta, typeof(TYPEATTR));
      sb.AppendFormat("\n  {0} {1} {{{2}}}{3}\n", ta.typekind.ToString().Replace("TKIND_",""), tn, ta.guid, string.IsNullOrEmpty(td) ? "" : "  // " + td);
      if (ta.typekind == TYPEKIND.TKIND_COCLASS) {
        for (int k = 0; k < ta.cImplTypes; k++) { int h; ti.GetRefTypeOfImplType(k, out h); ITypeInfo r; ti.GetRefTypeInfo(h, out r); string rn,rd,rh; int rc; r.GetDocumentation(-1, out rn, out rd, out rc, out rh); IMPLTYPEFLAGS f; ti.GetImplTypeFlags(k, out f); sb.AppendFormat("    implements {0} {1}\n", rn, f); }
      }
      for (int f = 0; f < ta.cFuncs; f++) {
        IntPtr pfd; ti.GetFuncDesc(f, out pfd); FUNCDESC fd = (FUNCDESC)Marshal.PtrToStructure(pfd, typeof(FUNCDESC));
        string[] names = new string[fd.cParams + 1]; int got; ti.GetNames(fd.memid, names, names.Length, out got);
        string fn2, fdoc, fh; int fc; ti.GetDocumentation(fd.memid, out fn2, out fdoc, out fc, out fh);
        var ps = new StringBuilder();
        int sz = Marshal.SizeOf(typeof(ELEMDESC));
        for (int p = 0; p < fd.cParams; p++) {
          ELEMDESC ed = (ELEMDESC)Marshal.PtrToStructure(new IntPtr(fd.lprgelemdescParam.ToInt64() + p * sz), typeof(ELEMDESC));
          PARAMFLAG pf = ed.desc.paramdesc.wParamFlags;
          string dir = ((pf & PARAMFLAG.PARAMFLAG_FOUT) != 0 ? "out " : "") + ((pf & PARAMFLAG.PARAMFLAG_FRETVAL) != 0 ? "retval " : "") + ((pf & PARAMFLAG.PARAMFLAG_FOPT) != 0 ? "opt " : "");
          if (p > 0) ps.Append(", ");
          ps.AppendFormat("{0}{1} {2}", dir, VtName(ti, ed.tdesc), p + 1 < got ? names[p + 1] : "p" + p);
        }
        string kind = fd.invkind == INVOKEKIND.INVOKE_FUNC ? "" : fd.invkind.ToString().Replace("INVOKE_","") + " ";
        sb.AppendFormat("    [{0}] {1}{2} {3}({4}){5}\n", fd.memid, kind, VtName(ti, fd.elemdescFunc.tdesc), names[0], ps, string.IsNullOrEmpty(fdoc) ? "" : "  // " + fdoc);
        ti.ReleaseFuncDesc(pfd);
      }
      for (int v = 0; v < ta.cVars; v++) {
        IntPtr pvd; ti.GetVarDesc(v, out pvd); VARDESC vd = (VARDESC)Marshal.PtrToStructure(pvd, typeof(VARDESC));
        string[] names = new string[1]; int got; ti.GetNames(vd.memid, names, 1, out got);
        string val = "";
        if (vd.varkind == VARKIND.VAR_CONST) { try { val = " = " + Marshal.GetObjectForNativeVariant(vd.desc.lpvarValue); } catch {} }
        sb.AppendFormat("    var {0} {1}{2}\n", VtName(ti, vd.elemdescVar.tdesc), names[0], val);
        ti.ReleaseVarDesc(pvd);
      }
      ti.ReleaseTypeAttr(pta);
    }
    return sb.ToString();
  }
}
"@
Add-Type -TypeDefinition $src
New-Item -ItemType Directory -Force $OutDir | Out-Null
$files = Get-ChildItem $Root -Recurse -File | Where-Object { $_.Extension -in '.dll','.ocx','.exe','.tlb' }
$summary = @()
foreach ($f in $files) {
  $txt = [TlbDump]::Dump($f.FullName)
  if ($txt) {
    $out = Join-Path $OutDir ($f.Name + ".idl.txt")
    $txt | Out-File -Encoding utf8 $out
    $summary += "{0}`t{1}" -f $f.FullName.Replace($Root,''), ($txt -split "`n")[0]
  }
}
$summary | Out-File -Encoding utf8 (Join-Path $OutDir "_summary.txt")
$summary
