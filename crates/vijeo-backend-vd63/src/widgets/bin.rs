//! Lectura y escritura de los tipos primitivos que usa Vijeo al serializar objetos (`IStream::Write` directo).
//! Común a todos los módulos de clases: cópialo tal cual (sólo std).

#[derive(Debug, Clone)]
pub struct Reader<'a> {
    pub b: &'a [u8],
    pub p: usize,
}

pub type R<T> = Result<T, String>;

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Reader { b, p: 0 }
    }
    pub fn at(b: &'a [u8], p: usize) -> Self {
        Reader { b, p }
    }
    pub fn left(&self) -> usize {
        self.b.len().saturating_sub(self.p)
    }
    pub fn bytes(&mut self, n: usize) -> R<&'a [u8]> {
        let s = self.b.get(self.p..self.p + n).ok_or_else(|| format!("fin de datos en {:#x} (faltan {n} B)", self.p))?;
        self.p += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> R<u8> {
        Ok(self.bytes(1)?[0])
    }
    pub fn u16(&mut self) -> R<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    pub fn i16(&mut self) -> R<i16> {
        Ok(i16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> R<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn f64(&mut self) -> R<f64> {
        Ok(f64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
    /// Comprueba un valor fijo.
    pub fn expect_u32(&mut self, v: u32, what: &str) -> R<()> {
        let at = self.p;
        let got = self.u32()?;
        if got != v {
            return Err(format!("{what}: se esperaba {v:#x} y hay {got:#x} en {at:#x}"));
        }
        Ok(())
    }
    /// BSTR: u32 bytes (con el NUL) + UTF-16 + NUL. `None` si la longitud es 0 (BSTR nulo).
    pub fn bstr(&mut self) -> R<String> {
        let at = self.p;
        let n = self.u32()? as usize;
        if n == 0 {
            return Ok(String::new());
        }
        if n % 2 != 0 || n > 0x100000 {
            return Err(format!("BSTR con longitud {n} en {at:#x}"));
        }
        let s = self.bytes(n)?;
        let u: Vec<u16> = s[..n - 2].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        if s[n - 2] != 0 || s[n - 1] != 0 {
            return Err(format!("BSTR sin NUL final en {at:#x}"));
        }
        String::from_utf16(&u).map_err(|_| format!("BSTR con UTF-16 inválido en {at:#x}"))
    }
    /// CString de MFC (CArchive): FF FE FF + longitud (u8, o FF + u16, o FFFF + u32) + UTF-16 sin NUL.
    pub fn cstring(&mut self) -> R<String> {
        let at = self.p;
        let m = self.bytes(3)?;
        if m != [0xFF, 0xFE, 0xFF] {
            return Err(format!("CString sin marca FF FE FF en {at:#x}"));
        }
        let mut n = self.u8()? as usize;
        if n == 0xFF {
            n = self.u16()? as usize;
            if n == 0xFFFF {
                n = self.u32()? as usize;
            }
        }
        let s = self.bytes(2 * n)?;
        let u: Vec<u16> = s.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16(&u).map_err(|_| format!("CString con UTF-16 inválido en {at:#x}"))
    }
}

#[derive(Debug, Default, Clone)]
pub struct Writer {
    pub o: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Writer { o: Vec::new() }
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.o.extend_from_slice(b);
    }
    pub fn u8(&mut self, v: u8) {
        self.o.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn i16(&mut self, v: i16) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.bytes(&v.to_le_bytes());
    }
    /// BSTR (una cadena vacía se escribe como `02 00 00 00 00 00`, como hace Vijeo).
    pub fn bstr(&mut self, s: &str) {
        let u: Vec<u16> = s.encode_utf16().collect();
        self.u32((u.len() as u32 + 1) * 2);
        for c in u {
            self.u16(c);
        }
        self.u16(0);
    }
    pub fn cstring(&mut self, s: &str) {
        let u: Vec<u16> = s.encode_utf16().collect();
        self.bytes(&[0xFF, 0xFE, 0xFF]);
        if u.len() < 0xFF {
            self.u8(u.len() as u8);
        } else if u.len() < 0xFFFF {
            self.u8(0xFF);
            self.u16(u.len() as u16);
        } else {
            self.u8(0xFF);
            self.u16(0xFFFF);
            self.u32(u.len() as u32);
        }
        for c in u {
            self.u16(c);
        }
    }
}
