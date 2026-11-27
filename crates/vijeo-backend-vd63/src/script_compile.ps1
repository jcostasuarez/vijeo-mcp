# Compila scripts de acción de Vijeo con su propio compilador (ScriptEngine.dll, COM 32 bits, en proceso).
# Lo lanza script.rs con el PowerShell de 32 bits (SysWOW64) y -EncodedCommand.
# Entrada (variables de entorno):
#   VIJEO_MCP_SCRIPT_IN   fichero JSON {"symbols": [[nombre, tipo], ...], "scripts": [fuente, ...]}
#                         tipo: BOOL | INTEGER | REAL | STRING | BaseTag (instancia de estructura)
#   VIJEO_MCP_MATH_XML    MathMethodDefine.xml de la instalación (métodos de Math)
# Salida (stdout, una línea JSON): [{"compiled": "...", "errors": [{code,line,column,message}], "warnings": [...]}]
# ScriptEngine pregunta por los nombres a un IObjectMethodValidate. El de Vijeo necesita el proyecto abierto, así que
# se responde aquí: variables y carpetas (IsTag), el objeto Sys (LSys) y Math, y las firmas de sus métodos.
$ErrorActionPreference = 'Stop'
$cs = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Web.Script.Serialization;
using System.Xml;

[ComImport, Guid("df1bac2b-c11d-49b4-accc-128b4f933559"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IObjectMethodValidate {
  [PreserveSig] int Initialize(IntPtr a, IntPtr b, IntPtr c, IntPtr d);
  [PreserveSig] int IsObject([MarshalAs(UnmanagedType.BStr)] string name, IntPtr type);
  [PreserveSig] int IsTag([MarshalAs(UnmanagedType.BStr)] string name, IntPtr type, IntPtr indirect, IntPtr accessible, IntPtr system, IntPtr inFolder);
  [PreserveSig] int GetObjectNames([MarshalAs(UnmanagedType.BStr)] string t, IntPtr names, IntPtr n);
  [PreserveSig] int GetFuncSignature([MarshalAs(UnmanagedType.BStr)] string t, [MarshalAs(UnmanagedType.BStr)] string m, IntPtr sigs, IntPtr n);
  [PreserveSig] int GetTypeFieldMethodNames([MarshalAs(UnmanagedType.BStr)] string t, IntPtr f, IntPtr m, IntPtr p, IntPtr nf, IntPtr nm, IntPtr np);
  [PreserveSig] int IsField([MarshalAs(UnmanagedType.BStr)] string t, [MarshalAs(UnmanagedType.BStr)] string f, IntPtr type);
  [PreserveSig] int AddObjects(IntPtr a, IntPtr b, int n);
  [PreserveSig] int AddTags(IntPtr a, IntPtr b, int n);
  [PreserveSig] int AddMethods(IntPtr a, IntPtr b, int n);
  [PreserveSig] int AddFields(IntPtr a, IntPtr b, int n);
  [PreserveSig] int RemoveMethods(IntPtr a, IntPtr b, int n);
  [PreserveSig] int RemoveFields(IntPtr a, IntPtr b, int n);
  [PreserveSig] int RemoveObjects(IntPtr a, IntPtr b, int n);
  [PreserveSig] int RemoveTags(IntPtr a, int n);
  [PreserveSig] int RemoveAll();
  [PreserveSig] int IsSubclass([MarshalAs(UnmanagedType.BStr)] string a, [MarshalAs(UnmanagedType.BStr)] string b);
  [PreserveSig] int GetTargetName(IntPtr name);
  [PreserveSig] int GetCGPanelName(IntPtr name);
  [PreserveSig] int SetGlobalUpdateLock(int l);
  [PreserveSig] int GetTagType([MarshalAs(UnmanagedType.BStr)] string name, IntPtr t);
  [PreserveSig] int IsTagMethod([MarshalAs(UnmanagedType.BStr)] string m);
  [PreserveSig] int GetBuildTargetName([MarshalAs(UnmanagedType.BStr)] string a, IntPtr b);
  [PreserveSig] int IsTagType([MarshalAs(UnmanagedType.BStr)] string t);
}

// Interfaz interna (no está en la typelib): ScriptEngine la pide al validador antes de registrar cada variable
// local declarada y sólo la registra si Ext28 devuelve distinto de 0. Mismo orden que IObjectMethodValidate + 3 ranuras.
[ComVisible(true), Guid("b134325e-cc50-4974-a437-2b41c0c04a7c"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IObjectMethodValidate2 {
  [PreserveSig] int Initialize(IntPtr a, IntPtr b, IntPtr c, IntPtr d);
  [PreserveSig] int IsObject([MarshalAs(UnmanagedType.BStr)] string name, IntPtr type);
  [PreserveSig] int IsTag([MarshalAs(UnmanagedType.BStr)] string name, IntPtr type, IntPtr indirect, IntPtr accessible, IntPtr system, IntPtr inFolder);
  [PreserveSig] int GetObjectNames([MarshalAs(UnmanagedType.BStr)] string t, IntPtr names, IntPtr n);
  [PreserveSig] int GetFuncSignature([MarshalAs(UnmanagedType.BStr)] string t, [MarshalAs(UnmanagedType.BStr)] string m, IntPtr sigs, IntPtr n);
  [PreserveSig] int GetTypeFieldMethodNames([MarshalAs(UnmanagedType.BStr)] string t, IntPtr f, IntPtr m, IntPtr p, IntPtr nf, IntPtr nm, IntPtr np);
  [PreserveSig] int IsField([MarshalAs(UnmanagedType.BStr)] string t, [MarshalAs(UnmanagedType.BStr)] string f, IntPtr type);
  [PreserveSig] int AddObjects(IntPtr a, IntPtr b, int n);
  [PreserveSig] int AddTags(IntPtr a, IntPtr b, int n);
  [PreserveSig] int AddMethods(IntPtr a, IntPtr b, int n);
  [PreserveSig] int AddFields(IntPtr a, IntPtr b, int n);
  [PreserveSig] int RemoveMethods(IntPtr a, IntPtr b, int n);
  [PreserveSig] int RemoveFields(IntPtr a, IntPtr b, int n);
  [PreserveSig] int RemoveObjects(IntPtr a, IntPtr b, int n);
  [PreserveSig] int RemoveTags(IntPtr a, int n);
  [PreserveSig] int RemoveAll();
  [PreserveSig] int IsSubclass([MarshalAs(UnmanagedType.BStr)] string a, [MarshalAs(UnmanagedType.BStr)] string b);
  [PreserveSig] int GetTargetName(IntPtr name);
  [PreserveSig] int GetCGPanelName(IntPtr name);
  [PreserveSig] int SetGlobalUpdateLock(int l);
  [PreserveSig] int GetTagType([MarshalAs(UnmanagedType.BStr)] string name, IntPtr t);
  [PreserveSig] int IsTagMethod([MarshalAs(UnmanagedType.BStr)] string m);
  [PreserveSig] int GetBuildTargetName([MarshalAs(UnmanagedType.BStr)] string a, IntPtr b);
  [PreserveSig] int IsTagType([MarshalAs(UnmanagedType.BStr)] string t);
  [PreserveSig] int Ext27(IntPtr a, IntPtr b, IntPtr c);
  [PreserveSig] int Ext28();
  [PreserveSig] int Ext29(IntPtr v);
}
[ComImport, Guid("8890a1b1-c4a6-41c7-ad6b-44f7746c3d76"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IScriptValidate {
  [PreserveSig] int Validate([MarshalAs(UnmanagedType.BStr)] string src, short mode, IObjectMethodValidate v, out int o1,
    out IntPtr s1, out IntPtr compiled, out IntPtr s3, out IntPtr s4, out int b1, out int b2, ushort a11);
}

[ComImport, Guid("9e100f5b-98f3-46f2-88f1-10fc2c1da2ca"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IScriptValidateEvent {
  [PreserveSig] int ReportError(int code, int line, int column, [MarshalAs(UnmanagedType.BStr)] string msg, int d, [MarshalAs(UnmanagedType.BStr)] string e);
  [PreserveSig] int ReportWarning(int code, int line, int column, [MarshalAs(UnmanagedType.BStr)] string msg, int d, [MarshalAs(UnmanagedType.BStr)] string e);
}

[ComVisible(true), ClassInterface(ClassInterfaceType.None)]
public class Sink : IScriptValidateEvent {
  public List<Dictionary<string, object>> Errors = new List<Dictionary<string, object>>();
  public List<Dictionary<string, object>> Warnings = new List<Dictionary<string, object>>();
  static Dictionary<string, object> M(int code, int line, int column, string msg) {
    var d = new Dictionary<string, object>(); d["code"] = code; d["line"] = line; d["column"] = column; d["message"] = msg; return d;
  }
  public int ReportError(int code, int line, int column, string msg, int d, string e) { Errors.Add(M(code, line, column, msg)); return 0; }
  public int ReportWarning(int code, int line, int column, string msg, int d, string e) { Warnings.Add(M(code, line, column, msg)); return 0; }
}

[ComVisible(true), ClassInterface(ClassInterfaceType.None)]
public class Validator : IObjectMethodValidate, IObjectMethodValidate2 {
  public int Ext27(IntPtr a, IntPtr b, IntPtr c) { return unchecked((int)0x80004001); }
  public int Ext28() { return 1; }
  public int Ext29(IntPtr v) { return unchecked((int)0x80004001); }
  public Dictionary<string, string> Tags = new Dictionary<string, string>();
  public HashSet<string> Folders = new HashSet<string>();
  // tipo (BOOL/INTEGER/REAL/STRING/BaseTag, LSys, LMath, "" = funciones globales) -> método -> firmas
  public Dictionary<string, Dictionary<string, List<string>>> Sigs = new Dictionary<string, Dictionary<string, List<string>>>();
  static readonly Dictionary<string, string> Objects = new Dictionary<string, string> { { "Sys", "LSys" }, { "Math", "LMath" }, { "String", "LString" } };

  // VIJEO_MCP_SCRIPT_LOG=<fichero>: anota cada consulta del motor (para depurar qué pregunta).
  static readonly string LogPath = Environment.GetEnvironmentVariable("VIJEO_MCP_SCRIPT_LOG");
  public static int L(int hr, string what) { if (!string.IsNullOrEmpty(LogPath)) System.IO.File.AppendAllText(LogPath, what + " -> " + hr + "\r\n"); return hr; }
  static void Str(IntPtr p, string s) { if (p != IntPtr.Zero) Marshal.WriteIntPtr(p, Marshal.StringToBSTR(s)); }
  static void Int(IntPtr p, int v) { if (p != IntPtr.Zero) Marshal.WriteInt32(p, v); }
  public void Add(string type, string method, string sig) {
    Dictionary<string, List<string>> m; if (!Sigs.TryGetValue(type, out m)) Sigs[type] = m = new Dictionary<string, List<string>>();
    List<string> l; if (!m.TryGetValue(method, out l)) m[method] = l = new List<string>();
    if (!l.Contains(sig)) l.Add(sig);
  }
  // Tipos como los da el IsTag de Vijeo (cadenas globales de ScriptEngine): LTag.BOOL, LTag.INTEGER, LTag.REAL,
  // LTag.STRING, LBaseTag (estructura) y LFolder.
  public void AddTag(string name, string type) {
    Tags[name] = type == "BaseTag" ? "LBaseTag" : "LTag." + type;
    for (int i = name.IndexOf('.'); i > 0; i = name.IndexOf('.', i + 1)) Folders.Add(name.Substring(0, i));
  }

  public int Initialize(IntPtr a, IntPtr b, IntPtr c, IntPtr d) { return 0; }
  public int IsObject(string name, IntPtr type) { string t; bool ok = Objects.TryGetValue(name, out t); Str(type, ok ? t : ""); return L(ok ? 0 : 1, "IsObject(" + name + ")"); }
  public int IsTag(string name, IntPtr type, IntPtr indirect, IntPtr accessible, IntPtr system, IntPtr inFolder) {
    string t; Int(indirect, 0); Int(system, 0);
    if (Tags.TryGetValue(name, out t)) { Str(type, t); Int(accessible, 1); Int(inFolder, name.IndexOf('.') > 0 ? 1 : 0); return L(0, "IsTag(" + name + ")=" + t); }
    Int(accessible, 0); Int(inFolder, 0);
    // Una instancia de estructura es una variable (BaseTag) aunque tenga miembros; el resto de prefijos son carpetas.
    if (Folders.Contains(name)) { Str(type, "LFolder"); return L(0, "IsTag(" + name + ")=LFolder"); }
    Str(type, ""); return L(1, "IsTag(" + name + ")");
  }
  public int GetObjectNames(string t, IntPtr names, IntPtr n) { Int(n, 0); if (names != IntPtr.Zero) Marshal.WriteIntPtr(names, IntPtr.Zero); return L(1, "GetObjectNames(" + t + ")"); }
  public int GetFuncSignature(string t, string m, IntPtr sigs, IntPtr n) {
    Dictionary<string, List<string>> mm; List<string> l;
    if (!Sigs.TryGetValue(t ?? "", out mm) || !mm.TryGetValue(m, out l)) return L(1, "GetFuncSignature(" + t + ", " + m + ")");
    L(0, "GetFuncSignature(" + t + ", " + m + ")=" + string.Join("|", l.ToArray()));
    if (sigs != IntPtr.Zero && n != IntPtr.Zero) {
      IntPtr a = Marshal.AllocCoTaskMem(IntPtr.Size * l.Count);
      for (int i = 0; i < l.Count; i++) Marshal.WriteIntPtr(a, i * IntPtr.Size, Marshal.StringToBSTR(l[i]));
      Marshal.WriteIntPtr(sigs, a); Marshal.WriteInt32(n, l.Count);
    }
    return 0;
  }
  public int GetTypeFieldMethodNames(string t, IntPtr f, IntPtr m, IntPtr p, IntPtr nf, IntPtr nm, IntPtr np) {
    foreach (var q in new[] { f, m, p }) if (q != IntPtr.Zero) Marshal.WriteIntPtr(q, IntPtr.Zero);
    Int(nf, 0); Int(nm, 0); Int(np, 0); return L(1, "GetTypeFieldMethodNames(" + t + ")");
  }
  public int IsField(string t, string f, IntPtr type) { Str(type, ""); return L(1, "IsField(" + t + ", " + f + ")"); }
  public int AddObjects(IntPtr a, IntPtr b, int n) { return 0; }
  public int AddTags(IntPtr a, IntPtr b, int n) { return 0; }
  public int AddMethods(IntPtr a, IntPtr b, int n) { return 0; }
  public int AddFields(IntPtr a, IntPtr b, int n) { return 0; }
  public int RemoveMethods(IntPtr a, IntPtr b, int n) { return 0; }
  public int RemoveFields(IntPtr a, IntPtr b, int n) { return 0; }
  public int RemoveObjects(IntPtr a, IntPtr b, int n) { return 0; }
  public int RemoveTags(IntPtr a, int n) { return 0; }
  public int RemoveAll() { return 0; }
  public int IsSubclass(string a, string b) { return L(1, "IsSubclass(" + a + ", " + b + ")"); }
  public int GetTargetName(IntPtr name) { Str(name, "Target1"); return 0; }
  public int GetCGPanelName(IntPtr name) { Str(name, "Panel1"); return 0; }
  public int SetGlobalUpdateLock(int l) { return 0; }
  public int GetTagType(string name, IntPtr t) { if (t != IntPtr.Zero) Marshal.WriteInt16(t, 0); return L(1, "GetTagType(" + name + ")"); }
  public int IsTagMethod(string m) {
    foreach (var t in new[] { "LTag.BOOL", "LTag.INTEGER", "LTag.REAL", "LTag.STRING" }) { Dictionary<string, List<string>> mm; if (Sigs.TryGetValue(t, out mm) && mm.ContainsKey(m)) return L(0, "IsTagMethod(" + m + ")"); }
    return L(1, "IsTagMethod(" + m + ")");
  }
  public int GetBuildTargetName(string a, IntPtr b) { Str(b, a); return 0; }
  public int IsTagType(string t) { return L(1, "IsTagType(" + t + ")"); }
}

public static class Compiler {
  // Firmas de la tabla interna de ScriptEngine (FUN_61720bec), con comas como las pide GetFuncSignature:
  // I int, J long, F float, D double, Z boolean, X String, V void.
  static readonly string[] TagMethods = {
    "write=(I)V", "write=(F)V", "write=(LString)V", "getIntValue=()I", "getFloatValue=()F", "getStringValue=()LString",
    "flushToStorage=()Z", "toString=()LString", "isDataQualityGood=()Z", "getIntValues=()I", "getFloatValues=()F",
    "getSize=()I", "sendData=()V", "add=()V", "ack=()Z", "setLabel=(LString)Z" };
  static readonly string[] IntTagMethods = { "X=(I)Z", "XW=(I,Z)V", "getMin=()I", "getMax=()I", "enableScaling=(I,I,I,I)V", "setMinMaxValues=(I,I)V" };
  static readonly string[] FloatTagMethods = { "getMin=()F", "getMax=()F", "enableScaling=(F,F,F,F)V", "setMinMaxValues=(F,F)V" };
  static readonly string[] SysMethods = {
    "createProcess=(LString,LString)I", "removeProcess=(I)Z", "isPlatform=(I)Z", "exitRuntime=()V", "setTime=(I,I,I)Z", "setDate=(I,I,I)Z",
    "vibrationOn=()V", "vibrationOff=()V", "systemWindowShow=(Z)V", "beep=()V", "logout=()V", "flushAllDataLogging=()V",
    "getLastFieldRequested=()I", "getLastFieldUpdated=()I", "getInfoString=(I)LString", "getInfoInt=(I)I", "controllerStart=()V",
    "controllerStop=()V", "login=(LString,LString)V", "clearBiometricSwitch=()I", "getTemperature=()I" };
  // String y StringBuffer de Java: la tabla interna de ScriptEngine sólo trae los nombres (con firma «()V»); las
  // firmas son las de java.lang (C char). El motor pregunta GetFuncSignature("LString", método) para una local String
  // y ("LStringBuffer", "StringBuffer") para el constructor.
  static readonly string[] StringMethods = {
    "String=()V", "String=(LString)V", "length=()I", "charAt=(I)C",
    "indexOf=(I)I", "indexOf=(LString)I", "indexOf=(I,I)I", "indexOf=(LString,I)I",
    "lastIndexOf=(I)I", "lastIndexOf=(LString)I", "lastIndexOf=(I,I)I", "lastIndexOf=(LString,I)I",
    "substring=(I)LString", "substring=(I,I)LString", "equals=(LString)Z", "equalsIgnoreCase=(LString)Z",
    "startsWith=(LString)Z", "startsWith=(LString,I)Z", "endsWith=(LString)Z", "compareTo=(LString)I",
    "concat=(LString)LString", "replace=(C,C)LString", "toUpperCase=()LString", "toLowerCase=()LString", "trim=()LString",
    "toString=()LString", "hashCode=()I", "intern=()LString",
    "regionMatches=(I,LString,I,I)Z", "regionMatches=(Z,I,LString,I,I)Z",
    "valueOf=(I)LString", "valueOf=(J)LString", "valueOf=(F)LString", "valueOf=(D)LString", "valueOf=(Z)LString", "valueOf=(C)LString" };
  static readonly string[] StringBufferMethods = {
    "StringBuffer=()V", "StringBuffer=(I)V", "StringBuffer=(LString)V",
    "append=(LString)LStringBuffer", "append=(I)LStringBuffer", "append=(J)LStringBuffer", "append=(F)LStringBuffer",
    "append=(D)LStringBuffer", "append=(Z)LStringBuffer", "append=(C)LStringBuffer",
    "insert=(I,LString)LStringBuffer", "insert=(I,I)LStringBuffer", "insert=(I,J)LStringBuffer", "insert=(I,F)LStringBuffer",
    "insert=(I,D)LStringBuffer", "insert=(I,Z)LStringBuffer", "insert=(I,C)LStringBuffer",
    "length=()I", "capacity=()I", "ensureCapacity=(I)V", "setLength=(I)V", "setCharAt=(I,C)V", "charAt=(I)C",
    "reverse=()LStringBuffer", "toString=()LString" };

  static void AddAll(Validator v, string type, string[] list) {
    foreach (var s in list) { int i = s.IndexOf('='); v.Add(type, s.Substring(0, i), s.Substring(i + 1)); }
  }
  static string Code(string t) {
    switch (t) { case "Int": return "I"; case "Long": return "J"; case "Float": return "F"; case "Double": return "D";
      case "Boolean": return "Z"; case "String": return "LString"; case "Void": return "V"; default: return "I"; }
  }
  // Math (y sus funciones globales sin «Math.») desde MathMethodDefine.xml.
  static void AddMath(Validator v, string path) {
    if (string.IsNullOrEmpty(path) || !System.IO.File.Exists(path)) return;
    var doc = new XmlDocument(); doc.Load(path);
    foreach (XmlElement holder in doc.SelectNodes("//GlobalMethods | //ClassDefine[@ClassName='Math']")) {
      string type = holder.Name == "GlobalMethods" ? "" : "LMath";
      foreach (XmlElement m in holder.SelectNodes("Methods/Method")) {
        foreach (XmlElement o in m.SelectNodes("Overloads/Overload")) {
          var ps = new List<string>();
          foreach (XmlElement p in o.SelectNodes("Parameters/Parameter")) if (p.GetAttribute("Type") != "Void") ps.Add(Code(p.GetAttribute("Type")));
          v.Add(type, m.GetAttribute("MethodName"), "(" + string.Join(",", ps.ToArray()) + ")" + Code(o.GetAttribute("ReturnType")));
        }
      }
    }
  }

  public static string Run(string inputPath, string mathXml) {
    var js = new JavaScriptSerializer { MaxJsonLength = int.MaxValue };
    var input = js.Deserialize<Dictionary<string, object>>(System.IO.File.ReadAllText(inputPath));
    var v = new Validator();
    foreach (object o in (System.Collections.ArrayList)input["symbols"]) {
      var p = (System.Collections.ArrayList)o; v.AddTag((string)p[0], (string)p[1]);
    }
    foreach (var t in new[] { "LTag.BOOL", "LTag.INTEGER", "LTag.REAL", "LTag.STRING", "LBaseTag" }) AddAll(v, t, TagMethods);
    AddAll(v, "LTag.BOOL", IntTagMethods); AddAll(v, "LTag.INTEGER", IntTagMethods); AddAll(v, "LTag.REAL", FloatTagMethods);
    AddAll(v, "LSys", SysMethods);
    AddAll(v, "LString", StringMethods); AddAll(v, "LStringBuffer", StringBufferMethods);
    AddMath(v, mathXml);
    var results = new List<object>();
    var engine = (IScriptValidate)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("79575a22-58cd-4980-890b-803e6b29ccea")));
    var cpc = (System.Runtime.InteropServices.ComTypes.IConnectionPointContainer)engine;
    System.Runtime.InteropServices.ComTypes.IConnectionPoint cp;
    var iid = typeof(IScriptValidateEvent).GUID;
    cpc.FindConnectionPoint(ref iid, out cp);
    foreach (object o in (System.Collections.ArrayList)input["scripts"]) {
      // Vijeo compila la fuente con un salto de línea más al final (comprobado con el corpus).
      string src = ((string)o).Replace("\r\n", "\n").Replace("\n", "\r\n") + "\r\n";
      Validator.L(0, "=== " + src.Replace("\r\n", "\\n"));
      var sink = new Sink(); int cookie; cp.Advise(sink, out cookie);
      int o1, b1, b2; IntPtr s1, compiled, s3, s4;
      int hr = engine.Validate(src, 0, v, out o1, out s1, out compiled, out s3, out s4, out b1, out b2, 0);
      cp.Unadvise(cookie);
      var r = new Dictionary<string, object>();
      r["hresult"] = hr;
      r["compiled"] = hr == 0 && compiled != IntPtr.Zero && sink.Errors.Count == 0 ? Marshal.PtrToStringBSTR(compiled) : null;
      r["errors"] = sink.Errors; r["warnings"] = sink.Warnings;
      results.Add(r);
      foreach (var p in new[] { s1, compiled, s3, s4 }) if (p != IntPtr.Zero) Marshal.FreeBSTR(p);
    }
    return js.Serialize(results);
  }
}
'@
Add-Type -TypeDefinition $cs -Language CSharp -ReferencedAssemblies System.Web.Extensions, System.Xml
[Console]::OutputEncoding = [Text.Encoding]::UTF8
[Console]::Out.Write([Compiler]::Run($env:VIJEO_MCP_SCRIPT_IN, $env:VIJEO_MCP_MATH_XML))
