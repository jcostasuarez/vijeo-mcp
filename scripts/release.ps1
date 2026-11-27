<#
.SYNOPSIS
  Compila vijeo-mcp para Windows x64, arma el zip de entrega y (opcional) crea el release en GitHub.

.DESCRIPTION
  1. Lee la versión de Cargo.toml ([workspace.package]).
  2. cargo test (salvo -SkipTests): unitarias y humo, sin Vijeo ni datos de Schneider.
  3. cargo build --release -p vijeo-mcp. Si target\release\vijeo-mcp.exe está en uso (el MCP corriendo), lo renombra
     antes a vijeo-mcp.run-HHMMSS.exe, que se podrá borrar al reiniciar el MCP.
  4. Arma dist\vijeo-mcp\ (exe + README.md) y dist\vijeo-mcp-<versión>.zip + .sha256.
  5. Con -Publish: exige árbol limpio y sincronizado con origin y crea el tag v<versión> y el release con gh, sin
     assets. Los adjunta el workflow .github\workflows\release.yml, que corre este mismo script en GitHub Actions
     cada vez que se publica un release (también si se crea desde la web).

.EXAMPLE
  .\scripts\release.ps1                 # solo compila y empaqueta en dist\
  .\scripts\release.ps1 -SkipTests      # sin cargo test
  .\scripts\release.ps1 -Publish        # crea el release v<versión>; GitHub Actions sube el zip
  .\scripts\release.ps1 -Publish -Draft # lo deja como borrador (el workflow corre al publicarlo)
#>
[CmdletBinding()]
param(
    [switch]$SkipTests,
    [switch]$Publish,
    [switch]$Draft
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$Name = 'vijeo-mcp'

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments)
    # cargo y gh escriben progreso en stderr: en PowerShell 5.1 con 'Stop' eso cortaría el script.
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Exe @Arguments } finally { $ErrorActionPreference = $prev }
    if ($LASTEXITCODE -ne 0) { throw "Falló: $Exe $($Arguments -join ' ') (código $LASTEXITCODE)" }
}

# Corre un comando y devuelve su salida (stdout + stderr, recortada) y su código, sin cortar el script.
function Get-Native {
    param([string]$Exe, [string[]]$Arguments)
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & $Exe @Arguments 2>&1 | ForEach-Object { "$_" } } finally { $ErrorActionPreference = $prev }
    [pscustomobject]@{ Code = $LASTEXITCODE; Text = (($out | Out-String).Trim()) }
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "No se encontró 'cargo' en el PATH. Instalar Rust desde https://rustup.rs (toolchain MSVC)."
}

# --- Versión ---
$m = Select-String -Path (Join-Path $Root 'Cargo.toml') -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $m) { throw 'No se encontró la versión en Cargo.toml' }
$Version = $m.Matches[0].Groups[1].Value
$Tag = "v$Version"
Step "$Name $Version"

# --- Chequeos previos para publicar (antes de compilar, para fallar rápido) ---
if ($Publish) {
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw "No se encontró 'gh' (GitHub CLI)." }
    $dirty = Get-Native git @('status', '--porcelain')
    if ($dirty.Code -ne 0) { throw "git status falló:`n$($dirty.Text)" }
    if ($dirty.Text) { throw "Hay cambios sin commitear; commitear antes de publicar:`n$($dirty.Text)" }
    Invoke-Native git @('fetch', '--quiet', 'origin')
    $branch = (Get-Native git @('rev-parse', '--abbrev-ref', 'HEAD')).Text
    $local = (Get-Native git @('rev-parse', 'HEAD')).Text
    $remote = Get-Native git @('rev-parse', "origin/$branch")
    if ($remote.Code -ne 0 -or $local -ne $remote.Text) { throw "La rama '$branch' no está sincronizada con origin (hacer push primero)." }
    if ((Get-Native gh @('release', 'view', $Tag)).Code -eq 0) { throw "El release $Tag ya existe. Subir la versión en Cargo.toml." }
    $global:LASTEXITCODE = 0
}

# --- Pruebas (unitarias y humo; las de Vijeo en vivo van con la feature live-vijeo y no se corren aquí) ---
if (-not $SkipTests) {
    Step 'cargo test'
    Invoke-Native cargo @('test', '--workspace', '--quiet')
}

# --- Compilación ---
$Exe = Join-Path $Root "target\release\$Name.exe"
if (Test-Path $Exe) {
    try {
        [IO.File]::Open($Exe, 'Open', 'ReadWrite', 'None').Dispose()
    } catch {
        $old = "$Name.run-$(Get-Date -Format HHmmss).exe"
        Write-Host "$Name.exe está en uso (MCP corriendo): se renombra a $old" -ForegroundColor DarkGray
        Rename-Item $Exe $old
    }
}
Step 'Compilando x64'
Invoke-Native cargo @('build', '--release', '-p', $Name)
if (-not (Test-Path $Exe)) { throw "No se generó $Exe" }

# --- Paquete ---
Step 'Armando dist\'
$Dist = Join-Path $Root 'dist'
$Stage = Join-Path $Dist $Name
if (Test-Path $Stage) { Remove-Item $Stage -Recurse -Force }
New-Item -ItemType Directory -Force $Stage | Out-Null
Copy-Item $Exe, (Join-Path $Root 'README.md') $Stage

$Zip = Join-Path $Dist "$Name-$Version.zip"
if (Test-Path $Zip) { Remove-Item $Zip -Force }
Add-Type -AssemblyName System.IO.Compression.FileSystem
[IO.Compression.ZipFile]::CreateFromDirectory($Stage, $Zip, [IO.Compression.CompressionLevel]::Optimal, $true)

$Hash = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
$ShaFile = "$Zip.sha256"
[IO.File]::WriteAllText($ShaFile, "$Hash  $(Split-Path $Zip -Leaf)`n")

Write-Host ''
Write-Host "Paquete: $Zip" -ForegroundColor Green
Write-Host "SHA256:  $Hash"
Get-ChildItem $Stage -Recurse -File | ForEach-Object {
    '  {0,-40} {1,10:N0} bytes' -f $_.FullName.Substring($Stage.Length + 1), $_.Length
}

# --- Release (sin assets: los sube el workflow release.yml) ---
if ($Publish) {
    Step "Creando release $Tag"
    $notes = @"
Servidor MCP para Vijeo-Designer 6.3, versión $Version.

- ``$Name.exe``: Windows x64. Requiere Vijeo-Designer 6.3 instalado y una sesión de escritorio.

Descomprimir y registrar el exe en el agente (ver README). El zip y su SHA256 los adjunta GitHub Actions al publicar el release.
"@
    $ghArgs = @('release', 'create', $Tag, '--title', "$Name $Version", '--notes', $notes, '--target', $local)
    if ($Draft) { $ghArgs += '--draft' }
    Invoke-Native gh $ghArgs
    Write-Host "Release $Tag creado. Los assets los sube el workflow 'Release assets' (gh run list -w release.yml)." -ForegroundColor Green
}
