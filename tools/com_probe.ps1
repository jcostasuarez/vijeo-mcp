# Sondeo empírico de la API COM de automatización de Vijeo-Designer.
# Uso: powershell -File com_probe.ps1 -Step <basic|matrix|project|open> [-WorkDir <dir>] [-ProjectPath <ruta>]
# No ejecuta Download ni RetrieveSourceProject (actúan sobre HMIs reales).
param(
  [string]$Step = 'basic',
  [string]$WorkDir = "$env:TEMP\vjd_probe",
  [string]$ProgId = 'VijeoFrame.VijeoDesigner2',
  [string]$HmiName = 'Target1',
  [string]$HmiType = 'Model_HMISTU855',
  [string]$ProjectPath = '',
  [int]$WaitSeconds = 90,
  [switch]$NoBuild,
  [switch]$Keep
)
$ErrorActionPreference = 'Continue'
function Log($m) { [Console]::Out.WriteLine(("[{0:HH:mm:ss.fff}] {1}" -f (Get-Date), $m)) }
function Format-Val($v) {
  if ($null -eq $v) { return '<null>' }
  if ($v -is [array]) { return "array[" + $v.Count + "]: " + (($v | Select-Object -First 30 | ForEach-Object { Format-Val $_ }) -join ' | ') }
  if ($v -is [__ComObject]) { return "<ComObject>" }
  $s = "$v"; if ($s.Length -gt 600) { $s = $s.Substring(0, 600) + "…(" + $s.Length + " chars)" }
  return "[" + $v.GetType().Name + "] " + $s
}
function Try-Call([string]$label, [scriptblock]$sb) {
  # Nombres con prefijo para no tapar variables del llamador (ámbito dinámico de PowerShell).
  $__sw = [Diagnostics.Stopwatch]::StartNew()
  try { $__r = & $sb; Log ("OK   {0} ({1} ms) -> {2}" -f $label, $__sw.ElapsedMilliseconds, (Format-Val $__r)); return , $__r }
  catch { Log ("FAIL {0} ({1} ms) -> {2}" -f $label, $__sw.ElapsedMilliseconds, $_.Exception.Message); return $null }
}
function Generic([string]$cmd, [string]$body) {
  '<?xml version="1.0" encoding="UTF-16"?><Generic xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema" Version="1"><Call><' + $cmd + '>' + $body + '</' + $cmd + '></Call></Generic>'
}

function Inspect-Target($t, [string]$tn) {
  Try-Call "  [$tn] Type" { $t.Type() } | Out-Null
  $pn = Try-Call "  [$tn] GetPanelNames" { $t.GetPanelNames() }
  $first = $true
  foreach ($pname in @($pn)) {
    if (-not $pname) { continue }
    if (-not $first) { continue }   # inspecciona sólo el primer panel en detalle
    $first = $false
    $pp = Try-Call "    GetPanel($pname)" { $t.GetPanel($pname) }
    if ($pp) {
      $vars = Try-Call "    Panel.GetVariables" { $pp.GetVariables() }
      $pf = Join-Path $WorkDir ("panel_{0}_{1}.xml" -f $tn, ($pname -replace '[\\/:*?"<>|]', '_'))
      Try-Call "    Panel.Export($pf)" { $pp.Export($pf) } | Out-Null
      if (Test-Path $pf) { Log ("    exported {0} bytes; head: {1}" -f (Get-Item $pf).Length, ((Get-Content $pf -TotalCount 5) -join ' ')) }
      Try-Call "    Panel.GetLastErrorMessage" { $pp.GetLastErrorMessage() } | Out-Null
      $v0 = @($vars) | Where-Object { $_ } | Select-Object -First 1
      if ($v0) {
        Try-Call "  [$tn] GetBindingOfVariable($v0)" { $t.GetBindingOfVariable($v0) } | Out-Null
        Try-Call "  [$tn] GenericCall(LookupTag $v0, elem)" { $t.GenericCall((Generic 'LookupTag' "<TagName>$v0</TagName>")) } | Out-Null
      }
    }
  }
  Try-Call "  [$tn] GenericCall(LookupTag missing, elem)" { $t.GenericCall((Generic 'LookupTag' '<TagName>__NoSuchTag__</TagName>')) } | Out-Null
  Try-Call "  [$tn] GenericCall(LookupTag empty)" { $t.GenericCall((Generic 'LookupTag' '')) } | Out-Null
  if (-not $NoBuild) {
    $w = 0
    Try-Call "  [$tn] Build" { $r = $t.Build([ref]$w); "ret=$r warnings=$w" } | Out-Null
    Try-Call "  [$tn] GetLastErrorMessage" { $t.GetLastErrorMessage() } | Out-Null
  }
}
function Inspect-Project($p) {
  Try-Call 'Project.Name' { $p.Name } | Out-Null
  Try-Call 'Project.IsDirty' { $p.IsDirty() } | Out-Null
  $names = Try-Call 'Project.GetTargetNames' { $p.GetTargetNames() }
  foreach ($tn in @($names)) {
    if (-not $tn) { continue }
    $t = Try-Call "GetTarget($tn)" { $p.GetTarget($tn) }
    if ($t) { Inspect-Target $t $tn }
  }
  return $names
}

New-Item -ItemType Directory -Force $WorkDir | Out-Null
$vd = Try-Call "CreateObject($ProgId)" { New-Object -ComObject $ProgId }
if (-not $vd) { exit 1 }
$sw = [Diagnostics.Stopwatch]::StartNew(); $avail = 0
while ($sw.Elapsed.TotalSeconds -lt $WaitSeconds) {
  try { $avail = $vd.IsAvailable() } catch { Log "IsAvailable threw: $($_.Exception.Message)" }
  if ($avail -ne 0) { break }
  Start-Sleep -Milliseconds 1000
}
Log ("IsAvailable={0} after {1:n1}s" -f $avail, $sw.Elapsed.TotalSeconds)
$fp = Get-Process Vijeo-Frame -ErrorAction SilentlyContinue | ForEach-Object { "pid=$($_.Id) title='$($_.MainWindowTitle)' visible=$($_.MainWindowHandle -ne 0)" }
Log "frame: $fp"

switch ($Step) {
  'basic' {
    Try-Call 'GetLastErrorMessage' { $vd.GetLastErrorMessage() } | Out-Null
    Try-Call 'GenericCall(<invalid>)' { $vd.GenericCall('<x/>') } | Out-Null
    Try-Call 'GenericCall(Foo)' { $vd.GenericCall((Generic 'Foo' '')) } | Out-Null
  }
  'matrix' {
    $combos = @(@('Target1', 'Model_HMISTU855'), @('Target_HMISTU', 'Model_HMISTU855'), @('Target1', 'HMISTU855'), @('Target1', 'HMISTU 855'), @('Target1', 'Target_HMISTU'))
    $i = 0
    foreach ($c in $combos) {
      $i++
      $p = Try-Call "NewProject('M$i','$($c[0])','$($c[1])')" { $vd.NewProject("M$i", $c[0], $c[1]) }
      if ($p) { Try-Call '  GetTargetNames' { $p.GetTargetNames() } | Out-Null; Try-Call '  Close' { $p.Close() } | Out-Null }
    }
  }
  'project' {
    $p = Try-Call "NewProject('ProbeProj','$HmiName','$HmiType')" { $vd.NewProject('ProbeProj', $HmiName, $HmiType) }
    if ($p) {
      foreach ($ty in @('Model_HMISTU855', 'HMISTU855', 'Target_HMISTU', 'bogus_model')) {
        $nt = Try-Call "NewTarget('T_$($ty -replace '\W','')','$ty')" { $p.NewTarget("T_$($ty -replace '\W','')", $ty) }
        Try-Call '  Project.GetLastErrorMessage' { $p.GetLastErrorMessage() } | Out-Null
      }
      Inspect-Project $p | Out-Null
      $file = Join-Path $WorkDir 'ProbeProj.vdz'
      Try-Call "Project.Save($file)" { $p.Save($file) } | Out-Null
      if (-not $Keep) { Try-Call 'Project.Close' { $p.Close() } | Out-Null }
    }
  }
  'open' {
    $p = Try-Call "OpenProject($ProjectPath)" { $vd.OpenProject($ProjectPath) }
    Try-Call 'GetLastErrorMessage' { $vd.GetLastErrorMessage() } | Out-Null
    if ($p) {
      $names = Inspect-Project $p
      $rep = Join-Path $WorkDir 'reports'; New-Item -ItemType Directory -Force $rep | Out-Null
      Try-Call "Project.GenerateAllReports($rep)" { $p.GenerateAllReports($rep) } | Out-Null
      $xr = Join-Path $WorkDir 'crossref.xml'
      Try-Call "Project.GetCrossReferences($xr)" { $p.GetCrossReferences($xr) } | Out-Null
      Try-Call 'Project.GetLastErrorMessage' { $p.GetLastErrorMessage() } | Out-Null
      $nt = Try-Call "NewTarget('ProbeT','Model_HMISTU855')" { $p.NewTarget('ProbeT', 'Model_HMISTU855') }
      Try-Call 'Project.GetLastErrorMessage' { $p.GetLastErrorMessage() } | Out-Null
      Try-Call 'Project.GetTargetNames (after NewTarget)' { $p.GetTargetNames() } | Out-Null
      Try-Call 'Project.IsDirty (after NewTarget)' { $p.IsDirty() } | Out-Null
      $sv = Join-Path $WorkDir 'saved_copy.vdz'
      Try-Call "Project.Save($sv)" { $p.Save($sv) } | Out-Null
      Try-Call 'Project.Close' { $p.Close() } | Out-Null
    }
  }
  'open2' {
    $p = Try-Call "OpenProject($ProjectPath)" { $vd.OpenProject($ProjectPath) }
    if ($p) {
      $tn = @($p.GetTargetNames())[0]
      $t = Try-Call "GetTarget($tn)" { $p.GetTarget($tn) }
      foreach ($pn in @('Start', '\Start', 'BP:Start', 'Panel:Start', 'BP:\Start')) {
        $pp = Try-Call "GetPanel('$pn')" { $t.GetPanel($pn) }
        Try-Call "  Target.GetLastErrorMessage" { $t.GetLastErrorMessage() } | Out-Null
        if ($pp) {
          Try-Call '  Panel.Name' { $pp.Name } | Out-Null
          Try-Call '  Panel.GetVariables' { $pp.GetVariables() } | Out-Null
          $pf = Join-Path $WorkDir 'panel_Start.xml'
          Try-Call "  Panel.Export($pf)" { $pp.Export($pf) } | Out-Null
          if (Test-Path $pf) { Log ("  exported {0} bytes; head: {1}" -f (Get-Item $pf).Length, ((Get-Content $pf -TotalCount 3) -join ' ')) }
          break
        }
      }
      $var = 'MyController.Alarms.xAlarmEStop_1'
      Try-Call "GetBindingOfVariable($var)" { $t.GetBindingOfVariable($var) } | Out-Null
      foreach ($cmd in @('LookupTagCommand', 'LookupTag')) {
        foreach ($body in @("<TagName>$var</TagName>", "<TagName>__NoSuchTag__</TagName>")) {
          Try-Call "Target.GenericCall($cmd $body)" { $t.GenericCall((Generic $cmd $body)) } | Out-Null
          Try-Call "Designer.GenericCall($cmd $body)" { $vd.GenericCall((Generic $cmd $body)) } | Out-Null
          Try-Call "Project.GenericCall($cmd $body)" { $p.GenericCall((Generic $cmd $body)) } | Out-Null
        }
      }
      $err = Join-Path $WorkDir 'loadxml_err.txt'
      $missing = Join-Path $WorkDir 'no_such.xml'
      foreach ($cmd in @('LoadXMLCommand', 'LoadXML')) {
        Try-Call "Designer.GenericCall($cmd missing file)" { $vd.GenericCall((Generic $cmd "<XMLFile>$missing</XMLFile><ErrorFile>$err</ErrorFile>")) } | Out-Null
      }
      foreach ($ty in @('Model_HMIS5T', 'Model_HMIGTO5310', 'Model_HMISTU855')) {
        $nt = Try-Call "NewTarget('P_$($ty -replace '\W','')','$ty')" { $p.NewTarget("P_$($ty -replace '\W','')", $ty) }
        if ($nt) { Try-Call '  NewTarget.Type' { $nt.Type() } | Out-Null; Try-Call '  NewTarget.Name' { $nt.Name } | Out-Null }
      }
      Try-Call 'GetTargetNames (after NewTarget)' { $p.GetTargetNames() } | Out-Null
      Try-Call 'Project.Close' { $p.Close() } | Out-Null
    }
  }
  'open3' {
    $p = Try-Call "OpenProject($ProjectPath)" { $vd.OpenProject($ProjectPath) }
    if ($p) {
      $tn = @($p.GetTargetNames())[0]
      $t = $p.GetTarget($tn)
      $var = 'MyController.Alarms.xAlarmEStop_1'
      foreach ($tag in @($var, '__NoSuchTag__', '_Hour')) {
        $x = "<LookupTagCommand><TagName>$tag</TagName></LookupTagCommand>"
        Try-Call "Target.GenericCall($x)" { $t.GenericCall($x) } | Out-Null
        Try-Call "Designer.GenericCall($x)" { $vd.GenericCall($x) } | Out-Null
      }
      Try-Call "Target.GenericCall(LookupTagCommand attr)" { $t.GenericCall("<LookupTagCommand TagName=`"$var`"/>") } | Out-Null
      # LoadXML: fichero inexistente (sólo para ver el formato de respuesta/error)
      $err = Join-Path $WorkDir 'loadxml_err.txt'
      $missing = Join-Path $WorkDir 'no_such.xml'
      $x = "<LoadXMLCommand><XMLFile>$missing</XMLFile><ErrorFile>$err</ErrorFile></LoadXMLCommand>"
      Try-Call "Designer.GenericCall(LoadXMLCommand missing)" { $vd.GenericCall($x) } | Out-Null
      Try-Call "Target.GenericCall(LoadXMLCommand missing)" { $t.GenericCall($x) } | Out-Null
      if (Test-Path $err) { Log ("  errorfile: " + (Get-Content $err -Raw)) }
      foreach ($mode in @('MSD', 'SoMachine')) {
        Try-Call "SetMode('$mode')" { $vd.SetMode($mode) } | Out-Null
        foreach ($pn in @('Start', 'BP:\Start')) {
          $pp = Try-Call "  GetPanel('$pn')" { $t.GetPanel($pn) }
          if ($pp) { Try-Call '    Panel.Name' { $pp.Name } | Out-Null; Try-Call '    Panel.GetVariables' { $pp.GetVariables() } | Out-Null }
        }
      }
      Try-Call 'Project.Close' { $p.Close() } | Out-Null
    }
  }
  'multi' {
    # D2: dos proyectos abiertos a la vez. -ProjectPath = "<a.vdz>|<b.vdz>" (copias).
    $paths = $ProjectPath -split '\|'
    $pa = Try-Call "A: OpenProject($($paths[0]))" { $vd.OpenProject($paths[0]) }
    if ($pa) { Try-Call 'A: Name' { $pa.Name } | Out-Null; Try-Call 'A: GetTargetNames' { $pa.GetTargetNames() } | Out-Null }
    $pb = Try-Call "B: OpenProject($($paths[1]))" { $vd.OpenProject($paths[1]) }
    if ($pb) { Try-Call 'B: Name' { $pb.Name } | Out-Null; Try-Call 'B: GetTargetNames' { $pb.GetTargetNames() } | Out-Null }
    # ¿Sigue siendo válido A después de abrir B?
    if ($pa) {
      Try-Call 'A (tras abrir B): Name' { $pa.Name } | Out-Null
      Try-Call 'A (tras abrir B): GetTargetNames' { $pa.GetTargetNames() } | Out-Null
      Try-Call 'A (tras abrir B): IsDirty' { $pa.IsDirty() } | Out-Null
    }
    $pc = Try-Call "C: NewProject('MCPMultiC','','')" { $vd.NewProject('MCPMultiC', '', '') }
    if ($pc) { Try-Call 'C: Name' { $pc.Name } | Out-Null }
    if ($pb) { Try-Call 'B (tras C): Name' { $pb.Name } | Out-Null }
    if ($pa) { Try-Call 'A (tras C): Name' { $pa.Name } | Out-Null }
    Log ("procesos Vijeo-Frame: " + ((Get-Process Vijeo-Frame -ErrorAction SilentlyContinue | ForEach-Object { $_.Id }) -join ','))
    foreach ($x in @(@('C', $pc), @('B', $pb), @('A', $pa))) {
      if ($x[1]) { Try-Call "$($x[0]): Close" { $x[1].Close() } | Out-Null }
    }
    # Segunda instancia COM: ¿mismo proceso u otro?
    $vd2 = Try-Call "CreateObject #2($ProgId)" { New-Object -ComObject $ProgId }
    if ($vd2) {
      Start-Sleep -Seconds 2
      Try-Call '#2 IsAvailable' { $vd2.IsAvailable() } | Out-Null
      Log ("procesos Vijeo-Frame con 2 objetos: " + ((Get-Process Vijeo-Frame -ErrorAction SilentlyContinue | ForEach-Object { $_.Id }) -join ','))
      $p2 = Try-Call "#2 OpenProject($($paths[1]))" { $vd2.OpenProject($paths[1]) }
      $p1 = Try-Call "#1 OpenProject($($paths[0]))" { $vd.OpenProject($paths[0]) }
      if ($p1) { Try-Call '#1 proyecto: Name' { $p1.Name } | Out-Null }
      if ($p2) { Try-Call '#2 proyecto: Name' { $p2.Name } | Out-Null }
      if ($p2) { Try-Call '#2 proyecto: Close' { $p2.Close() } | Out-Null }
      if ($p1) { Try-Call '#1 proyecto: Close' { $p1.Close() } | Out-Null }
      Try-Call '#2 Exit' { $vd2.Exit() } | Out-Null
    }
  }
  'multi2' {
    # D2 (2): dos instancias COM (dos procesos Vijeo-Frame), un proyecto en cada una.
    $paths = $ProjectPath -split '\|'
    $vd2 = Try-Call "CreateObject #2($ProgId)" { New-Object -ComObject $ProgId }
    $sw2 = [Diagnostics.Stopwatch]::StartNew(); $a2 = 0
    while ($vd2 -and $sw2.Elapsed.TotalSeconds -lt $WaitSeconds) { try { $a2 = $vd2.IsAvailable() } catch {}; if ($a2 -ne 0) { break }; Start-Sleep -Milliseconds 1000 }
    Log ("#2 IsAvailable={0} tras {1:n1}s; procesos: {2}" -f $a2, $sw2.Elapsed.TotalSeconds, ((Get-Process Vijeo-Frame -ErrorAction SilentlyContinue | ForEach-Object { $_.Id }) -join ','))
    $p1 = Try-Call "#1 OpenProject(A)" { $vd.OpenProject($paths[0]) }
    $p2 = if ($vd2) { Try-Call "#2 OpenProject(B)" { $vd2.OpenProject($paths[1]) } }
    if ($p1) { Try-Call '#1 Name' { $p1.Name } | Out-Null; Try-Call '#1 GetTargetNames' { $p1.GetTargetNames() } | Out-Null }
    if ($p2) { Try-Call '#2 Name' { $p2.Name } | Out-Null; Try-Call '#2 GetTargetNames' { $p2.GetTargetNames() } | Out-Null }
    # Cambios independientes: un target nuevo en cada proyecto
    if ($p1) { Try-Call '#1 NewTarget(Extra1)' { $p1.NewTarget('Extra1', 'Model_HMIGTO5310') } | Out-Null; Try-Call '#1 GetTargetNames' { $p1.GetTargetNames() } | Out-Null }
    if ($p2) { Try-Call '#2 GetTargetNames (no debe ver Extra1)' { $p2.GetTargetNames() } | Out-Null }
    if ($p1) { Try-Call '#1 Save' { $p1.Save((Join-Path $WorkDir 'A_saved.vdz')) } | Out-Null; Try-Call '#1 Close' { $p1.Close() } | Out-Null }
    if ($p2) { Try-Call '#2 Save' { $p2.Save((Join-Path $WorkDir 'B_saved.vdz')) } | Out-Null; Try-Call '#2 Close' { $p2.Close() } | Out-Null }
    Log ("carpetas temporales: " + ((Get-ChildItem 'C:\Users\Public\Documents\Vijeo-Designer 6.3\Vijeo-Manager' -Directory -Filter 'M5D*' | ForEach-Object { $_.Name }) -join ','))
    if ($vd2) { Try-Call '#2 Exit' { $vd2.Exit() } | Out-Null }
  }
  'models' {
    # -ProjectPath = models.json (tools/extract_models.ps1). Crea y borra un target por modelo
    # (nunca más de 2 a la vez: con 32 Vijeo abre un MessageBox modal) y escribe models_validation.csv.
    $models = (Get-Content $ProjectPath -Raw | ConvertFrom-Json).models | Where-Object { $_.supported }
    $p = Try-Call "NewProject('MCPModels','','')" { $vd.NewProject('MCPModels', '', '') }
    $rows = @()
    $i = 0
    foreach ($m in $models) {
      $i++
      $name = 'M{0:D2}' -f $i
      $sw = [Diagnostics.Stopwatch]::StartNew()
      $t = $null; $type = $null; $err = $null
      $hmiType = if ($m.hmi_type) { $m.hmi_type } else { $m.internal_name }
      try { $t = $p.NewTarget($name, $hmiType) } catch { $err = $_.Exception.Message }
      if ($t) { try { $type = $t.Type() } catch { $err = $_.Exception.Message } }
      # SCU (con controlador) devuelve la forma compuesta "Target=<modelo>,Control=True".
      $ok = [bool]$t -and ($type -eq $hmiType -or $type -like "Target=$hmiType,*")
      Log ("{0} {1,-40} -> {2} type={3} ({4} ms) {5}" -f ($(if ($ok) { 'OK  ' } else { 'FAIL' })), $hmiType, ($(if ($t) { 'obj' } else { 'null' })), $type, $sw.ElapsedMilliseconds, $err)
      $rows += [pscustomobject]@{ internal_name = $m.internal_name; hmi_type = $hmiType; name = $m.name; family = $m.family; created = [bool]$t; type = $type; ok = $ok; ms = $sw.ElapsedMilliseconds; error = $err }
      if ($t) { try { $t.Delete() } catch { Log "  Delete falló: $($_.Exception.Message)" } }
    }
    $rows | Export-Csv -NoTypeInformation -Encoding UTF8 (Join-Path $WorkDir 'models_validation.csv')
    Log ("resumen: {0} OK / {1} total" -f @($rows | Where-Object { $_.ok }).Count, $rows.Count)
    Try-Call 'GetTargetNames (final)' { $p.GetTargetNames() } | Out-Null
    Try-Call 'Close' { $p.Close() } | Out-Null
  }
  'hmitype' {
    # Prueba variantes de hmi_type para NewTarget (familias modulares con DisplayUnits / Control).
    $variants = @(
      'Model_HMIG3U', 'Model_HMIG3U:HMIDT351', 'Model_HMIG3U:HMIDT351 (800x480)',
      'HMIDT351:Model_HMIG3U', 'Target_HMIGTU:Model_HMIG3U',
      'Target=Model_HMIG3U,Control=False', 'Target=Model_HMIG3U,Control=True',
      'Target=Model_HMIG3U:HMIDT351,Control=False',
      'Target=Model_HMISAC,Control=False', 'Target=Model_HMISAC,Control=True',
      'Model_HMIS5T,Resize=True', 'Target=Model_HMIS5T,Control=False'
    )
    $p = Try-Call "NewProject('MCPHmiType','','')" { $vd.NewProject('MCPHmiType', '', '') }
    $i = 0
    foreach ($v in $variants) {
      $i++
      $t = Try-Call "NewTarget('V$i','$v')" { $p.NewTarget("V$i", $v) }
      if ($t) {
        Try-Call '  Type' { $t.Type() } | Out-Null
        Try-Call '  Delete' { $t.Delete() } | Out-Null
      }
    }
    Try-Call 'Close' { $p.Close() } | Out-Null
  }
  'loadxml' {
    # -ProjectPath = XML (formato privado) a importar con LoadXMLCommand en un proyecto nuevo.
    $p = Try-Call "NewProject('MCPXmlHost','','')" { $vd.NewProject('MCPXmlHost', '', '') }
    if ($p -and $HmiName) {
      $pre = Try-Call "pre NewTarget('$HmiName','$HmiType')" { $p.NewTarget($HmiName, $HmiType) }
      if ($pre) { Try-Call "  pre LookupTag(MCP_X)" { $pre.GenericCall('<LookupTagCommand><TagName>MCP_X</TagName></LookupTagCommand>') } | Out-Null }
    }
    $ef = Join-Path $WorkDir ((Split-Path $ProjectPath -Leaf) + '.err.txt')
    Try-Call "LoadXMLCommand($ProjectPath)" { $vd.GenericCall("<LoadXMLCommand><XMLFile>$ProjectPath</XMLFile><ErrorFile>$ef</ErrorFile></LoadXMLCommand>") } | Out-Null
    $errText = if (Test-Path $ef) { (Get-Content $ef -Raw) -replace '\s+', ' ' } else { '<sin errorfile>' }
    Log "  errorfile: $errText"
    if ($p) {
      Try-Call '  host.Name' { $p.Name } | Out-Null
      $names = Try-Call '  host.GetTargetNames' { $p.GetTargetNames() }
      if ($errText -notmatch 'ERROR') {
        foreach ($tn in @($names)) {
          if (-not $tn) { continue }
          $t = $p.GetTarget($tn)
          Try-Call "  [$tn] Type" { $t.Type() } | Out-Null
          Try-Call "  [$tn] LookupTag(MCP_X)" { $t.GenericCall('<LookupTagCommand><TagName>MCP_X</TagName></LookupTagCommand>') } | Out-Null
        }
        $sv = Join-Path $WorkDir ((Split-Path $ProjectPath -Leaf) + '.vdz')
        Try-Call "  Save($sv)" { $p.Save($sv) } | Out-Null
      } else { Log '  (import con errores: no se tocan más objetos para evitar el AV de DDTTagDatabase)' }
      # Cerrar antes de Exit: salir con un proyecto abierto deja Vijeo-Manager\M5DTemPHolder<PID>.
      Try-Call '  Close' { $p.Close() } | Out-Null
    }
  }
  'fixes' {
    $vm = 'C:\Users\Public\Documents\Vijeo-Designer 6.3\Vijeo-Manager'
    $frame = Get-Process Vijeo-Frame -ErrorAction SilentlyContinue | Select-Object -First 1
    # 1) NewProject + NewTarget con código interno, guardar y reabrir
    $p = Try-Call "NewProject('MCPFix','','')" { $vd.NewProject('MCPFix', '', '') }
    if ($p) {
      $t = Try-Call "NewTarget('HMI1','Model_HMIS5T')" { $p.NewTarget('HMI1', 'Model_HMIS5T') }
      if ($t) { Try-Call '  Type' { $t.Type() } | Out-Null }
      Try-Call 'GetTargetNames' { $p.GetTargetNames() } | Out-Null
      $np = Join-Path $WorkDir 'MCPFix.vdz'
      Try-Call "Save($np)" { $p.Save($np) } | Out-Null
      Try-Call 'Close' { $p.Close() } | Out-Null
      Try-Call 'Close (2nd)' { $p.Close() } | Out-Null
      $p2 = Try-Call "OpenProject($np)" { $vd.OpenProject($np) }
      if ($p2) { Try-Call '  reopened GetTargetNames' { $p2.GetTargetNames() } | Out-Null; Try-Call '  Close' { $p2.Close() } | Out-Null }
    }
    # 2) Proyecto de ejemplo (copia)
    $p = Try-Call "OpenProject($ProjectPath)" { $vd.OpenProject($ProjectPath) }
    if ($p) {
      if ($frame) {
        $holder = 'M5DTemPHolder{0:X}' -f $frame.Id
        Log ("temp holder esperado: $holder existe=" + (Test-Path (Join-Path $vm $holder)))
      }
      $tn = @($p.GetTargetNames())[0]
      $t = $p.GetTarget($tn)
      Try-Call "ImportPanel(no existe)" { $t.ImportPanel((Join-Path $WorkDir 'nope.xml'), '') } | Out-Null
      Try-Call "GetBindingOfVariable" { $t.GetBindingOfVariable('MyController.Alarms.xAlarmEStop_1') } | Out-Null
      $dm = Try-Call "NewTarget('DelMe','Model_HMIGTO5310')" { $p.NewTarget('DelMe', 'Model_HMIGTO5310') }
      Try-Call 'GetTargetNames (con DelMe)' { $p.GetTargetNames() } | Out-Null
      if ($dm) { Try-Call 'DelMe.Delete' { $dm.Delete() } | Out-Null }
      Try-Call 'GetTargetNames (tras Delete)' { $p.GetTargetNames() } | Out-Null
      if ($dm) { Try-Call 'DelMe.Name tras Delete' { $dm.Name } | Out-Null }
      # 3) LoadXMLCommand con distintos XML
      $variants = [ordered]@{
        'A_variables' = '<?xml version="1.0" encoding="utf-8"?><Variables><Variable Name="MCP_A" Type="INT" Source="Internal"/></Variables>'
        'B_target'    = '<?xml version="1.0" encoding="utf-8"?><Target ID="T1" Type="HMISTU Series" Model="HMISTU855" Name="' + $tn + '"><Variables><Variable Name="MCP_B" Type="INT" Source="Internal"/></Variables></Target>'
        'C_project'   = '<?xml version="1.0" encoding="utf-8"?><Project Name="' + $p.Name + '"><Target ID="T1" Type="HMISTU Series" Model="HMISTU855" Name="' + $tn + '"><Variables><Variable Name="MCP_C" Type="INT" Source="Internal"/></Variables></Target></Project>'
      }
      foreach ($k in $variants.Keys) {
        $xf = Join-Path $WorkDir "$k.xml"; $ef = Join-Path $WorkDir "$k.err.txt"
        [IO.File]::WriteAllText($xf, $variants[$k], (New-Object Text.UTF8Encoding $false))
        Try-Call "LoadXMLCommand($k)" { $vd.GenericCall("<LoadXMLCommand><XMLFile>$xf</XMLFile><ErrorFile>$ef</ErrorFile></LoadXMLCommand>") } | Out-Null
        if (Test-Path $ef) { Log ("  errorfile: " + ((Get-Content $ef -Raw) -replace '\s+', ' ')) }
        $var = 'MCP_' + $k.Substring(0, 1)
        Try-Call "  LookupTag($var)" { $t.GenericCall("<LookupTagCommand><TagName>$var</TagName></LookupTagCommand>") } | Out-Null
        Try-Call '  GetTargetNames' { $p.GetTargetNames() } | Out-Null
      }
      Try-Call 'IsDirty' { $p.IsDirty() } | Out-Null
      $sv = Join-Path $WorkDir 'after_fixes.vdz'
      Try-Call "Save($sv)" { $p.Save($sv) } | Out-Null
      Try-Call 'Close' { $p.Close() } | Out-Null
    }
  }
}
Log ("workdir: " + ((Get-ChildItem $WorkDir -Recurse -File | ForEach-Object { "$($_.FullName.Replace($WorkDir,''))($($_.Length))" }) -join ', '))
if (-not $Keep) { Try-Call 'Exit' { $vd.Exit() } | Out-Null }
if ($vd -is [__ComObject]) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($vd) }
Log 'done'
