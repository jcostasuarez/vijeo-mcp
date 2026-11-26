//! Texto de los ficheros que escribe Vijeo (XML exportado, log del import).

/// Decodifica un fichero de Vijeo según su BOM (sin BOM, UTF-8: el `.vxml` de Export Project).
pub fn decode_text(bytes: &[u8]) -> String {
    let utf16 = |b: &[u8], le: bool| {
        let units: Vec<u16> = b.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::decode_text;

    #[test]
    fn boms() {
        let mut le = vec![0xFF, 0xFE];
        le.extend("<tr>ñ".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode_text(&le), "<tr>ñ");
        let mut be = vec![0xFE, 0xFF];
        be.extend("ab".encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(decode_text(&be), "ab");
        assert_eq!(decode_text(b"\xEF\xBB\xBFx"), "x");
        assert_eq!(decode_text(b"plain"), "plain");
    }
}
