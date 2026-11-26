# Genera la tabla de modelos HMI a partir de los catálogos descifrados con vjd-dat.
# Uso: extract_models.ps1 -DecryptedDir <dir con Targets.xml y Models_*.xml> -OutJson <models.json>
#        [-OutMd <models.md>] [-ValidationCsv <models_validation.csv de com_probe.ps1 -Step models>]
param(
  [Parameter(Mandatory)] [string]$DecryptedDir,
  [Parameter(Mandatory)] [string]$OutJson,
  [string]$OutMd,
  [string]$ValidationCsv
)
$ErrorActionPreference = 'Stop'
function Load([string]$path) { $x = New-Object Xml.XmlDocument; $x.LoadXml((Get-Content $path -Raw)); $x }
function Txt($node, [string]$xpath) { $n = $node.SelectSingleNode($xpath); if ($n) { $n.InnerText.Trim() } else { $null } }

$families = @{}
foreach ($t in (Load (Join-Path $DecryptedDir 'Targets.xml')).SelectNodes('/TargetList/Target')) {
  $internal = Txt $t 'Identity/InternalName'
  $families[$internal] = [ordered]@{
    internal_name = $internal
    name          = Txt $t 'Identity/Name'
    type          = Txt $t 'Identity/Type'
    default_model = Txt $t 'DefaultModel'
    model_list    = Txt $t 'ModelList'
    hmi           = (Txt $t 'ComponentSupport/HMI') -eq 'true'
    control       = (Txt $t 'ComponentSupport/Control') -eq 'true'
  }
}

# Vijeo enlaza familia → catálogo por <ModelList>; el <Identity/Target> del modelo no siempre coincide
# (p.ej. Tablets declara "Target_Tablets" y la familia es "Tablet Series").
$byCatalog = @{}
foreach ($fam in $families.Values) { if ($fam.model_list) { $byCatalog[$fam.model_list] = $fam } }

$models = New-Object System.Collections.Generic.List[object]
foreach ($f in Get-ChildItem $DecryptedDir -Filter 'Models_*.xml' | Sort-Object Name) {
  $fam = $byCatalog[$f.BaseName]
  foreach ($m in (Load $f.FullName).SelectNodes('/ModelList/Model')) {
    $dl = $m.SelectSingleNode('Download/DownloadTypes')
    $types = if ($dl) { @($dl.ChildNodes | Where-Object { $_.NodeType -eq 'Element' -and $_.InnerText -eq 'true' } | ForEach-Object { $_.Name }) } else { @() }
    # Modelos modulares (HMIGTU…): NewTarget exige "<modelo>:<nombre completo de la pantalla>".
    # DefaultDisplayUnit es un índice base 0 en la lista (verificado: índice 2 → HMIDT651).
    $du = $m.SelectSingleNode('Display/DisplayUnits')
    $units = if ($du) { @($du.ChildNodes | Where-Object { $_.NodeType -eq 'Element' -and $_.Name -ne 'DefaultDisplayUnit' } | ForEach-Object { $_.Name }) } else { @() }
    $defaultUnit = $null
    if ($units.Count) { $idx = [int](Txt $du 'DefaultDisplayUnit'); if ($idx -lt $units.Count) { $defaultUnit = $units[$idx] } }
    $internal = Txt $m 'Identity/InternalName'
    $models.Add([ordered]@{
      internal_name    = $internal
      hmi_type         = if ($defaultUnit) { "${internal}:$defaultUnit" } else { $internal }
      display_units    = $units
      default_display  = $defaultUnit
      somachine        = [bool]$m.SelectSingleNode('SoMachine')
      required_key     = [int64](Txt $m 'Registration/RequiredKey')
      name             = Txt $m 'Identity/Name'
      family           = if ($fam) { $fam.internal_name } else { Txt $m 'Identity/Target' }
      family_name      = if ($fam) { $fam.name } else { $null }
      supported        = [bool]$fam
      runtime_model_id = Txt $m 'Identity/RuntimeModelID'
      screen_width     = [int](Txt $m 'Display/ScreenWidth')
      screen_height    = [int](Txt $m 'Display/ScreenHeight')
      target_os        = Txt $m 'Download/TargetOS'
      download_types   = $types
      catalog          = $f.BaseName
    })
  }
}

if ($ValidationCsv) {
  $validated = @{}
  Import-Csv $ValidationCsv | ForEach-Object { $validated[$_.internal_name] = ($_.ok -eq 'True') }
  foreach ($m in $models) { $m['com_creatable'] = if ($validated.ContainsKey($m.internal_name)) { $validated[$m.internal_name] } else { $false } }
}

$doc = [ordered]@{
  source   = 'Vijeo-Designer 6.3 TargetInfo (descifrado con tools/vjd-dat)'
  families = @($families.Values | Sort-Object { $_.internal_name })
  models   = @($models | Sort-Object { $_.family }, { $_.internal_name })
}
$doc | ConvertTo-Json -Depth 5 | Out-File -Encoding utf8 $OutJson
"familias: $($families.Count)  modelos: $($models.Count)"

if ($OutMd) {
  $lines = @('| `hmi_type` para `NewTarget` | Nombre comercial | Familia | Resolución | SO runtime | Descarga | COM |', '|---|---|---|---|---|---|---|')
  foreach ($m in $doc.models | Where-Object { $_.supported }) {
    $com = if ($m.Contains('com_creatable')) { if ($m.com_creatable) { '✅' } elseif (-not $m.somachine) { '❌ sin `<SoMachine>`' } else { '❌ licencia' } } else { '?' }
    $res = if ($m.screen_width) { '{0}x{1}' -f $m.screen_width, $m.screen_height } else { '—' }
    $lines += '| `{0}` | {1} | {2} | {3} | {4} | {5} | {6} |' -f $m.hmi_type, $m.name, $m.family_name, $res, $m.target_os, ($m.download_types -join ', '), $com
  }
  $orphans = @($doc.models | Where-Object { -not $_.supported })
  if ($orphans.Count) {
    $lines += ''
    $lines += 'Modelos en catálogos cuya familia está desactivada en `Targets.dat` (no se pueden crear): ' + (($orphans | ForEach-Object { '`' + $_.internal_name + '`' }) -join ', ')
  }
  $lines | Out-File -Encoding utf8 $OutMd
}
