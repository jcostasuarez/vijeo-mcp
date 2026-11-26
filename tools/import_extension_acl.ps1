# Da a un usuario permiso de escritura en la clave "Import Extension" de Vijeo-Designer 6.3 (ejecutar como administrador).
#
# INDEFileManager.dll (FUN_6343d1a7) abre "Import Extension" y cada subclave con acceso 0x2001F (lectura +
# SetValue + CreateSubKey). Desde un Vijeo-Frame sin elevar esa apertura falla con ERROR_ACCESS_DENIED y la
# extensión registrada (tools\import_extension_xml.reg) queda sin extensiones ni filtro: Import Project sólo
# acepta .vdz. Con este permiso sobre la clave (heredado por sus subclaves) la extensión funciona.
#
# Uso:    powershell -ExecutionPolicy Bypass -File tools\import_extension_acl.ps1 [-User DOMINIO\usuario]
# Deshacer: powershell -ExecutionPolicy Bypass -File tools\import_extension_acl.ps1 -Remove
param(
  [string]$User = "$env:USERDOMAIN\$env:USERNAME",
  [switch]$Remove
)
$ErrorActionPreference = 'Stop'
$key = 'HKLM:\SOFTWARE\WOW6432Node\Schneider Electric\Vijeo-Designer63\Vijeo-Frame\Import Extension'
if (-not (Test-Path $key)) { throw "no existe $key: aplica antes tools\import_extension_xml.reg" }
$rights = [System.Security.AccessControl.RegistryRights]'ReadKey, SetValue, CreateSubKey'
$rule = New-Object System.Security.AccessControl.RegistryAccessRule($User, $rights, 'ContainerInherit', 'None', 'Allow')
$acl = Get-Acl $key
if ($Remove) { [void]$acl.RemoveAccessRule($rule) } else { $acl.AddAccessRule($rule) }
Set-Acl -Path $key -AclObject $acl
(Get-Acl $key).Access | Where-Object { $_.IdentityReference -like "*$($User.Split('\')[-1])*" } | Format-Table IdentityReference, RegistryRights, InheritanceFlags -AutoSize
