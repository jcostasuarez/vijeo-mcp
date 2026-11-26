# Ejecuta Ghidra headless con un entorno aislado (JDK, ajustes, caché y temporales bajo C:\tools).
# Copia el binario a una ruta sin espacios/paréntesis (los .bat de Ghidra no los soportan) y
# ejecuta un script de tools/ghidra_scripts como post-script.
#
# Uso:
#   ghidra_headless.ps1 -Binary <exe/dll> -Project <nombre> -Out <fichero.c> -ScriptArgs 004F4100,004F73A8
#   ghidra_headless.ps1 ... -Script DecompileByString.java -ScriptArgs 'CVJDTarget_Internal::GetPanel'
#   [-Reuse]  reutiliza el proyecto ya analizado (-process -noanalysis) en vez de reimportar
param(
  [Parameter(Mandatory)] [string]$Binary,
  [Parameter(Mandatory)] [string]$Project,
  [Parameter(Mandatory)] [string]$Out,
  [Parameter(Mandatory)] [string[]]$ScriptArgs,
  [string]$Script = 'DecompileAt.java',
  [string]$GhidraDir = 'C:\tools\ghidra_12.2_PUBLIC',
  [string]$JavaHome = 'C:\tools\jdk-25.0.5+8',
  [string]$HomeDir = 'C:\tools\ghidra_home',
  [string]$ProjectsDir = 'C:\tools\ghidra_projects',
  [int]$AnalysisTimeout = 3000,
  [switch]$Reuse
)
$ErrorActionPreference = 'Stop'
foreach ($d in "$HomeDir\config", "$HomeDir\cache", "$HomeDir\tmp", "$ProjectsDir\bin\$Project", "$HomeDir\logs") {
  New-Item -ItemType Directory -Force $d | Out-Null
}
$env:JAVA_HOME = $JavaHome
$env:Path = "$JavaHome\bin;$env:Path"
$env:XDG_CONFIG_HOME = "$HomeDir\config"
$env:XDG_CACHE_HOME = "$HomeDir\cache"
$env:TEMP = "$HomeDir\tmp"
$env:TMP = "$HomeDir\tmp"

$name = Split-Path $Binary -Leaf
$local = Join-Path "$ProjectsDir\bin\$Project" $name
if (-not $Reuse) { Copy-Item $Binary $local -Force }

$stamp = Get-Date -Format 'yyyyMMdd_HHmmss'
$log = "$HomeDir\logs\${Project}_$stamp.log"
$scriptDir = Join-Path $PSScriptRoot 'ghidra_scripts'
$common = @('-log', $log, '-scriptPath', $scriptDir, '-postScript', $Script, $Out) + $ScriptArgs
if ($Reuse) {
  $argv = @($ProjectsDir, $Project, '-process', $name, '-noanalysis', '-readOnly') + $common
} else {
  $argv = @($ProjectsDir, $Project, '-import', $local, '-overwrite', '-analysisTimeoutPerFile', "$AnalysisTimeout") + $common
}
# Start-Process no entrecomilla, y cmd.exe (los .bat de Ghidra) separa argumentos también por = , ;
$argv = $argv | ForEach-Object { if ($_ -match '[\s=,;]') { '"' + $_ + '"' } else { $_ } }
$p = Start-Process -FilePath "$GhidraDir\support\analyzeHeadless.bat" -ArgumentList $argv `
  -RedirectStandardOutput "$log.out" -RedirectStandardError "$log.err" -PassThru -NoNewWindow -Wait
"exit: $($p.ExitCode)"
"log: $log"
Get-Content "$log.out" | Select-String 'ERROR|Exception|succeeded|took|REPORT' | Select-Object -Last 20 | ForEach-Object { $_.Line }
if (Test-Path $Out) { "out: $Out ($((Get-Item $Out).Length) bytes)" } else { "out: <no generado>" }
