# 32 bits: aplica CXMLFileFormatUtility::TransformToPrivate/TransformToPublic de INDEXML.dll a un XML.
# Uso: C:\Windows\SysWOW64\WindowsPowerShell\v1.0\powershell.exe -File xml_transform32.ps1 -In public.xml -Out private.xml [-Public] [-Flag 1]
param([string]$In, [string]$Out, [switch]$Public, [int]$Flag = 1)
if ([IntPtr]::Size -ne 4) { throw 'ejecutar con PowerShell de 32 bits (SysWOW64)' }
$vf = 'C:\Program Files (x86)\Schneider Electric\Vijeo-Designer 6.3\Vijeo-Frame'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class X {
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern IntPtr LoadLibraryEx(string p, IntPtr h, uint f);
  [DllImport("INDEXML.dll", EntryPoint="??0CXMLDOMDocument2@@QAE@PAUIXMLDOMDocument2@@H@Z", CallingConvention=CallingConvention.ThisCall)]
  public static extern IntPtr DocCtor(IntPtr self, IntPtr doc, int addRef);
  [DllImport("INDEXML.dll", EntryPoint="?TransformToPrivate@CXMLFileFormatUtility@@SAXAAVCXMLDOMDocument2@@_N@Z", CallingConvention=CallingConvention.Cdecl)]
  public static extern void ToPrivate(IntPtr doc, [MarshalAs(UnmanagedType.U1)] bool flag);
  [DllImport("INDEXML.dll", EntryPoint="?TransformToPublic@CXMLFileFormatUtility@@SAXAAVCXMLDOMDocument2@@_N@Z", CallingConvention=CallingConvention.Cdecl)]
  public static extern void ToPublic(IntPtr doc, [MarshalAs(UnmanagedType.U1)] bool flag);
}
'@
$h = [X]::LoadLibraryEx("$vf\INDEXML.dll", [IntPtr]::Zero, 8)
if ($h -eq [IntPtr]::Zero) { throw "LoadLibraryEx: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
$doc = New-Object -ComObject 'Msxml2.DOMDocument.4.0'
$doc.async = $false
$doc.preserveWhiteSpace = $true
if (-not $doc.load($In)) { throw "load: $($doc.parseError.reason)" }
"cargado: raíz=$($doc.documentElement.nodeName)"
$unk = [Runtime.InteropServices.Marshal]::GetIUnknownForObject($doc)
$iid = [Guid]'2933BF95-7B36-11d2-B20E-00C04F983E60' # IXMLDOMDocument2
$p2 = [IntPtr]::Zero
$hr = [Runtime.InteropServices.Marshal]::QueryInterface($unk, [ref]$iid, [ref]$p2)
if ($hr -ne 0) { throw "QI IXMLDOMDocument2: 0x$('{0:X8}' -f $hr)" }
$wrap = [Runtime.InteropServices.Marshal]::AllocHGlobal(64)
[void][X]::DocCtor($wrap, $p2, 1)
$sw = [Diagnostics.Stopwatch]::StartNew()
if ($Public) { [X]::ToPublic($wrap, [bool]$Flag) } else { [X]::ToPrivate($wrap, [bool]$Flag) }
"transformado en $($sw.ElapsedMilliseconds) ms"
$res = [Runtime.InteropServices.Marshal]::ReadIntPtr($wrap, 4)
$obj = [Runtime.InteropServices.Marshal]::GetObjectForIUnknown($res)
"raíz resultado: $($obj.documentElement.nodeName)"
$obj.save($Out)
"guardado: $Out ($((Get-Item $Out).Length) bytes)"
