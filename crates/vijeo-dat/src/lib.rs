//! Descifrado de los ficheros `.dat` de Vijeo-Designer (`TargetInfo\*.dat`, `XML\*.dat`).
//!
//! Formato, obtenido de `INDEXML.dll` 6.3 (`CXMLFileFormatUtility::DecryptBuffer` / `TEAn`):
//! - TEA en la variante `tean` de Wheeler y Needham, 8 ciclos, clave fija `"InDE eLeCtRoNiCs"`
//!   (con espacio; Ghidra la muestra como `s_InDE_eLeCtRoNiCs` porque sus etiquetas no admiten espacios).
//! - Bloques independientes de 8 bytes (ECB); el tamaño se rellena con ceros hasta múltiplo de 8.
//! - El contenido descifrado es texto UTF-16LE que empieza en el byte 2 y termina en NUL.

const DELTA: u32 = 0x9E37_79B9;
const ROUNDS: u32 = 8;
const KEY: &[u8; 16] = b"InDE eLeCtRoNiCs";

fn key_words() -> [u32; 4] {
    let mut k = [0u32; 4];
    for (i, w) in k.iter_mut().enumerate() {
        *w = u32::from_le_bytes(KEY[i * 4..i * 4 + 4].try_into().unwrap());
    }
    k
}

fn tean_decode(y: &mut u32, z: &mut u32, k: &[u32; 4]) {
    let mut sum = DELTA.wrapping_mul(ROUNDS);
    while sum != 0 {
        *z = z.wrapping_sub(
            ((*y >> 5 ^ *y << 4).wrapping_add(*y)) ^ k[(sum >> 11 & 3) as usize].wrapping_add(sum),
        );
        sum = sum.wrapping_sub(DELTA);
        *y = y.wrapping_sub(((*z >> 5 ^ *z << 4).wrapping_add(*z)) ^ k[(sum & 3) as usize].wrapping_add(sum));
    }
}

fn tean_encode(y: &mut u32, z: &mut u32, k: &[u32; 4]) {
    let limit = DELTA.wrapping_mul(ROUNDS);
    let mut sum = 0u32;
    while sum != limit {
        *y = y.wrapping_add(((*z >> 5 ^ *z << 4).wrapping_add(*z)) ^ k[(sum & 3) as usize].wrapping_add(sum));
        sum = sum.wrapping_add(DELTA);
        *z = z.wrapping_add(
            ((*y >> 5 ^ *y << 4).wrapping_add(*y)) ^ k[(sum >> 11 & 3) as usize].wrapping_add(sum),
        );
    }
}

fn for_each_block(buf: &mut [u8], f: impl Fn(&mut u32, &mut u32, &[u32; 4])) {
    let k = key_words();
    for block in buf.chunks_exact_mut(8) {
        let mut y = u32::from_le_bytes(block[0..4].try_into().unwrap());
        let mut z = u32::from_le_bytes(block[4..8].try_into().unwrap());
        f(&mut y, &mut z, &k);
        block[0..4].copy_from_slice(&y.to_le_bytes());
        block[4..8].copy_from_slice(&z.to_le_bytes());
    }
}

/// Descifra en el sitio los bloques completos de 8 bytes (como `DecryptBuffer`).
pub fn decrypt_in_place(buf: &mut [u8]) {
    for_each_block(buf, tean_decode);
}

/// Cifra en el sitio los bloques completos de 8 bytes (inverso de [`decrypt_in_place`]).
pub fn encrypt_in_place(buf: &mut [u8]) {
    for_each_block(buf, tean_encode);
}

#[derive(Debug)]
pub enum DatError {
    TooShort,
    InvalidUtf16,
}

impl std::fmt::Display for DatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatError::TooShort => write!(f, "fichero demasiado corto"),
            DatError::InvalidUtf16 => write!(f, "el contenido descifrado no es UTF-16 válido"),
        }
    }
}

impl std::error::Error for DatError {}

/// Descifra el contenido completo de un `.dat` y devuelve el texto (XML) que contiene.
pub fn decrypt_dat(raw: &[u8]) -> Result<String, DatError> {
    if raw.len() < 8 {
        return Err(DatError::TooShort);
    }
    let mut buf = raw.to_vec();
    buf.resize(raw.len().div_ceil(8) * 8, 0);
    decrypt_in_place(&mut buf);
    let units: Vec<u16> = buf[2..]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16(&units).map_err(|_| DatError::InvalidUtf16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut data: Vec<u8> = (0u8..64).collect();
        let orig = data.clone();
        encrypt_in_place(&mut data);
        assert_ne!(data, orig);
        decrypt_in_place(&mut data);
        assert_eq!(data, orig);
    }

    #[test]
    fn decrypt_text() {
        // Construye un .dat como lo haría Vijeo: 2 bytes iniciales + UTF-16LE + NUL, cifrado.
        let text = "<Targets/>";
        let mut plain = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            plain.extend_from_slice(&u.to_le_bytes());
        }
        plain.extend_from_slice(&[0, 0]);
        plain.resize(plain.len().div_ceil(8) * 8, 0);
        encrypt_in_place(&mut plain);
        assert_eq!(decrypt_dat(&plain).unwrap(), text);
    }
}
