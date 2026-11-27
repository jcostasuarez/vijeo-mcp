# Inventario de los binarios PE de una carpeta (por defecto, todo Schneider Electric): bits, .NET o nativo,
# versión, exports e imports vigilados (portapapeles, DnD, DDE, tuberías, COM, structured storage). Usa dumpbin.
# Uso: pe_inventory.ps1 -Out <fichero.json> [-Root <carpeta>]   (§13 del inventario de APIs)
param(
  [string]$Root = 'C:\Program Files (x86)\Schneider Electric',
  [Parameter(Mandatory)][string]$Out
)
$dumpbin = 'C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\dumpbin.exe'
$watch = @('RegisterClipboardFormatA','RegisterClipboardFormatW','SetClipboardData','GetClipboardData','OleSetClipboard','OleGetClipboard',
  'DoDragDrop','RegisterDragDrop','DdeInitializeA','DdeInitializeW','CreateNamedPipeA','CreateNamedPipeW','RegisterWindowMessageA','RegisterWindowMessageW',
  'CoRegisterClassObject','CreateFileMappingA','CreateFileMappingW','StgOpenStorage','StgCreateDocfile','StgOpenStorageEx','CommandLineToArgvW','GetCommandLineW','GetCommandLineA',
  'WSAStartup','socket','bind','listen','LoadTypeLib','LoadTypeLibEx','RegisterTypeLib')
$files = Get-ChildItem -Path $Root -Recurse -File -Include *.exe,*.dll,*.ocx,*.ax,*.cpl -ErrorAction SilentlyContinue
$result = New-Object System.Collections.Generic.List[object]
$i = 0
foreach ($f in $files) {
  $i++
  $rel = $f.FullName.Substring($Root.Length + 1)
  $bits = $null; $managed = $false
  try {
    $fs = [IO.File]::OpenRead($f.FullName); $buf = New-Object byte[] 4096; [void]$fs.Read($buf, 0, 4096); $fs.Close()
    $pe = [BitConverter]::ToInt32($buf, 0x3C)
    if ($pe -gt 0 -and $pe -lt 3900) {
      $magic = [BitConverter]::ToUInt16($buf, $pe + 24)
      $bits = if ($magic -eq 0x20b) { 64 } else { 32 }
      $dd = if ($magic -eq 0x20b) { $pe + 24 + 112 } else { $pe + 24 + 96 }
      $managed = ([BitConverter]::ToUInt32($buf, $dd + 14 * 8) -ne 0)
    }
  } catch {}
  $vi = $f.VersionInfo
  $exports = @(); $imports = @()
  if (-not ($rel -match '\\(MUI|[a-z]{2}(-[A-Za-z]{2,4})?)\\[^\\]+\.resources\.dll$')) {
    $txt = & $dumpbin /nologo /exports /imports $f.FullName 2>$null
    $mode = ''
    foreach ($line in $txt) {
      if ($line -match '^\s+Section contains the following exports') { $mode = 'exp'; continue }
      if ($line -match '^\s+Section contains the following imports') { $mode = 'imp'; continue }
      if ($mode -eq 'exp' -and $line -match '^\s+\d+\s+[0-9A-F]+\s+(?:[0-9A-F]{8}\s+)?(\S+)') { $exports += $Matches[1] }
      elseif ($mode -eq 'imp' -and $line -match '^\s+[0-9A-F]+\s+(\S+)$') { if ($watch -contains $Matches[1]) { $imports += $Matches[1] } }
    }
  }
  $result.Add([pscustomobject]@{
    rel = $rel; size = $f.Length; bits = $bits; managed = $managed
    company = $vi.CompanyName; product = $vi.ProductName; description = $vi.FileDescription; version = $vi.FileVersion
    exports = $exports; watched_imports = @($imports | Sort-Object -Unique)
  })
  if ($i % 50 -eq 0) { Write-Output "$i / $($files.Count)" }
}
$result | ConvertTo-Json -Depth 4 | Out-File -FilePath $Out -Encoding utf8
Write-Output "done: $($result.Count) -> $Out"
