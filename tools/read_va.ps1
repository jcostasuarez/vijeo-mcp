# Lee la cadena (ASCII y UTF-16LE) situada en una VA de un PE de 32 bits.
# Uso: read_va.ps1 -File <pe> -Address 0x5D634C[,...]
param([string]$File, [string[]]$Address)
$b = [IO.File]::ReadAllBytes($File)
$peOff = [BitConverter]::ToInt32($b, 0x3C)
$nSec = [BitConverter]::ToUInt16($b, $peOff + 6)
$optSize = [BitConverter]::ToUInt16($b, $peOff + 20)
$imageBase = [BitConverter]::ToUInt32($b, $peOff + 24 + 28)
$secOff = $peOff + 24 + $optSize
function ToOff([uint32]$va) {
  for ($i = 0; $i -lt $nSec; $i++) {
    $o = $secOff + 40 * $i
    $sva = $imageBase + [BitConverter]::ToUInt32($b, $o + 12); $vs = [BitConverter]::ToUInt32($b, $o + 8); $rp = [BitConverter]::ToUInt32($b, $o + 20)
    if ($va -ge $sva -and $va -lt $sva + $vs) { return [int]($rp + ($va - $sva)) }
  }
  return -1
}
foreach ($v in $Address) {
  $addr = [Convert]::ToUInt32(($v -replace '^0x', '' -replace 'h$', ''), 16)
  $off = ToOff $addr
  if ($off -lt 0) { "0x{0:X8}`t<no mapeada>" -f $addr; continue }
  $a = New-Object Text.StringBuilder; for ($i = $off; $i -lt $b.Length -and $b[$i] -ne 0 -and $a.Length -lt 200; $i++) { [void]$a.Append([char]$b[$i]) }
  $u = New-Object Text.StringBuilder; for ($i = $off; $i -lt $b.Length - 1 -and ($b[$i] -ne 0 -or $b[$i + 1] -ne 0) -and $u.Length -lt 200; $i += 2) { [void]$u.Append([char]($b[$i] + 256 * $b[$i + 1])) }
  $hex = ($b[$off..($off + 15)] | ForEach-Object { $_.ToString('X2') }) -join ' '
  "0x{0:X8}`tA='{1}'`tU='{2}'`t[{3}]" -f $addr, $a, $u, $hex
}
