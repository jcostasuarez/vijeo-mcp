# Localiza cadenas (ASCII o UTF-16LE) en un PE de 32 bits y devuelve su VA.
# Uso: find_string_va.ps1 -File <pe> -Text <cadena> [-Unicode]
param([string]$File, [string[]]$Text, [switch]$Unicode)
$b = [IO.File]::ReadAllBytes($File)
$peOff = [BitConverter]::ToInt32($b, 0x3C)
$nSec = [BitConverter]::ToUInt16($b, $peOff + 6)
$optSize = [BitConverter]::ToUInt16($b, $peOff + 20)
$imageBase = [BitConverter]::ToUInt32($b, $peOff + 24 + 28)
$secOff = $peOff + 24 + $optSize
$secs = for ($i = 0; $i -lt $nSec; $i++) {
  $o = $secOff + 40 * $i
  [pscustomobject]@{
    Name = [Text.Encoding]::ASCII.GetString($b, $o, 8).Trim([char]0)
    VA = [BitConverter]::ToUInt32($b, $o + 12); RawSize = [BitConverter]::ToUInt32($b, $o + 16); RawPtr = [BitConverter]::ToUInt32($b, $o + 20)
  }
}
function ToVA([int]$off) {
  foreach ($s in $secs) { if ($off -ge $s.RawPtr -and $off -lt $s.RawPtr + $s.RawSize) { return $imageBase + $s.VA + ($off - $s.RawPtr) } }
  return $null
}
foreach ($t in $Text) {
  $enc = if ($Unicode) { [Text.Encoding]::Unicode } else { [Text.Encoding]::ASCII }
  # Coincidencia exacta terminada en NUL
  $pat = $enc.GetBytes($t) + $(if ($Unicode) { @(0, 0) } else { @(0) })
  $first = $pat[0]
  for ($i = [Array]::IndexOf($b, $first); $i -ge 0 -and $i -lt $b.Length - $pat.Length; $i = [Array]::IndexOf($b, $first, $i + 1)) {
    $ok = $true
    for ($j = 1; $j -lt $pat.Length; $j++) { if ($b[$i + $j] -ne $pat[$j]) { $ok = $false; break } }
    if ($ok -and ($i -eq 0 -or $b[$i - 1] -eq 0)) {
      $va = ToVA $i
      "{0}`t0x{1:X8}`toff=0x{2:X}" -f $t, $va, $i
    }
  }
}
