//! Conversión de texto a la página de códigos OEM del sistema, la del «ANSI CSV» que escribe Vijeo
//! (en un Windows en español, 437 u 850: `ñ` = 0xA4, `ó` = 0xA2).

use windows::core::{BOOL, PCSTR};
use windows::Win32::Globalization::{GetOEMCP, WideCharToMultiByte, CP_OEMCP, WC_NO_BEST_FIT_CHARS};

/// Página de códigos OEM del sistema.
pub fn oem_code_page() -> u32 {
    unsafe { GetOEMCP() }
}

/// `s` en la página OEM. El `bool` dice si algún carácter no existe en ella (sale `?`).
pub fn to_oem(s: &str) -> (Vec<u8>, bool) {
    let wide: Vec<u16> = s.encode_utf16().collect();
    if wide.is_empty() {
        return (Vec::new(), false);
    }
    let mut lossy = BOOL(0);
    let n = unsafe { WideCharToMultiByte(CP_OEMCP, WC_NO_BEST_FIT_CHARS, &wide, None, PCSTR::null(), Some(&mut lossy)) };
    let mut out = vec![0u8; n.max(0) as usize];
    let n = unsafe { WideCharToMultiByte(CP_OEMCP, WC_NO_BEST_FIT_CHARS, &wide, Some(&mut out), PCSTR::null(), Some(&mut lossy)) };
    out.truncate(n.max(0) as usize);
    (out, lossy.as_bool())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanish_text_fits_in_oem() {
        let (b, lossy) = to_oem("Año ñ ó");
        assert!(!lossy);
        assert_eq!(b.len(), 7);
        if matches!(oem_code_page(), 437 | 850) {
            assert_eq!(&b[4..], &[0xA4, b' ', 0xA2]);
        }
        let (_, lossy) = to_oem("日本");
        assert!(lossy);
    }
}
