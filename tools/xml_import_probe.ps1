# Prueba del import XML: instancia propia por COM, SIN proyecto abierto, GenericCall(LoadXMLCommand) con un
# XML privado (raíz VijeoDesignerProject). Vijeo crea y guarda el proyecto en Vijeo-Manager\<ProjectName>;
# el script lo empaqueta como .vdz (ZIP con <nombre>.SwxCF, DataFilesBackup/ y SymbolTables/) y después
# borra SÓLO las carpetas de Vijeo-Manager que aparecieron durante esta ejecución.
# El ProjectName del XML debe ser un nombre temporal que no exista en Vijeo-Manager.
# Uso: xml_import_probe.ps1 -XmlFile <privado.xml> -SaveAs <salida.vdz> [-KeepWorkspace]
param(
  [Parameter(Mandatory)] [string]$XmlFile,
  [Parameter(Mandatory)] [string]$SaveAs,
  [int]$WaitSeconds = 60,
  [switch]$KeepWorkspace
)
$ErrorActionPreference = 'Stop'
function Log($m) { [Console]::Out.WriteLine(("[{0:HH:mm:ss.fff}] {1}" -f (Get-Date), $m)) }
$wm = 'C:\Users\Public\Documents\Vijeo-Designer 6.3\Vijeo-Manager'
$name = ([xml](Get-Content -Raw -Encoding UTF8 $XmlFile)).SelectSingleNode('/VijeoDesignerProject/ProjectProperties/ProjectName').InnerText
if (-not $name) { throw 'el XML no tiene ProjectProperties/ProjectName' }
if (Test-Path (Join-Path $wm $name)) { throw "ya existe Vijeo-Manager\${name}: usar otro ProjectName" }
$wmBefore = @(Get-ChildItem $wm -Force | ForEach-Object Name)
$before = @(Get-Process Vijeo-Frame -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
Log "Vijeo-Frame previos (no se tocan): $($before -join ',')"
$vd = New-Object -ComObject 'VijeoFrame.VijeoDesigner2'
$sw = [Diagnostics.Stopwatch]::StartNew(); $avail = 0
while ($sw.Elapsed.TotalSeconds -lt $WaitSeconds) { try { $avail = $vd.IsAvailable() } catch {}; if ($avail -ne 0) { break }; Start-Sleep -Milliseconds 500 }
$mine = @(Get-Process Vijeo-Frame | Where-Object { $before -notcontains $_.Id })
Log "instancia propia PID=$($mine[0].Id) IsAvailable=$avail"
$err = [IO.Path]::ChangeExtension($SaveAs, '.import_log.txt')
try {
  $req = "<LoadXMLCommand><XMLFile>$XmlFile</XMLFile><ErrorFile>$err</ErrorFile></LoadXMLCommand>"
  $sw.Restart()
  $r = $vd.GenericCall($req)
  Log ("LoadXMLCommand -> '{0}' en {1:n1}s" -f $r.Trim(), $sw.Elapsed.TotalSeconds)
  if ((Test-Path $err) -and (Select-String -Path $err -Pattern 'ERROR' -Quiet)) { Get-Content $err | Where-Object { $_ -match 'ERROR' } | ForEach-Object { Log "  | $_" } }
}
finally {
  try { $vd.Exit() } catch { Log "Exit: $($_.Exception.Message)" }
  Start-Sleep -Seconds 2
}
$src = Join-Path $wm $name
if (-not (Test-Path "$src\$name.SwxCF")) { throw "no se creó Vijeo-Manager\$name\$name.SwxCF" }
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
if (Test-Path $SaveAs) { Remove-Item $SaveAs }
$z = [IO.Compression.ZipFile]::Open($SaveAs, 'Create')
try {
  [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($z, "$src\$name.SwxCF", "$name.SwxCF")
  [void]$z.CreateEntry('SymbolTables/')
  [void]$z.CreateEntry('DataFilesBackup/')
  if (Test-Path "$src\DataFilesBackup") {
    $base = "$src\".Length
    Get-ChildItem "$src\DataFilesBackup" -Recurse | ForEach-Object {
      $rel = $_.FullName.Substring($base).Replace('\', '/')
      if ($_.PSIsContainer) { [void]$z.CreateEntry("$rel/") } else { [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($z, $_.FullName, $rel) }
    }
  }
}
finally { $z.Dispose() }
Log "empaquetado: $SaveAs ($((Get-Item $SaveAs).Length) bytes)"
$new = @(Get-ChildItem $wm -Force | ForEach-Object Name | Where-Object { $wmBefore -notcontains $_ })
Log "Vijeo-Manager: nuevas entradas: $(if ($new) { $new -join ', ' } else { '(ninguna)' })"
if (-not $KeepWorkspace) {
  foreach ($n in $new) { Remove-Item -Recurse -Force (Join-Path $wm $n) -Confirm:$false; Log "  borrada Vijeo-Manager\$n" }
}
