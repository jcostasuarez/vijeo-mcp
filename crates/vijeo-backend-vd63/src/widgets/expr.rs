//! CExpression (PFUtility.dll, `CExpression::Save`): referencia a una variable o expresión
//! tal como la guarda Vijeo-Designer 6.3 dentro de los objetos (portapapeles y .vdz).
//!
//! Gramática (todo little-endian):
//! ```text
//! u32   0x3F3                      versión / marca
//! BSTR  raw                        texto fuente: {F,2:2.<id>,Carpeta}.{T,2:2.<id>,Var}, con $..$ si hay placeholder
//! BSTR  compiled                   expresión compilada (TagDB.X..., o la constante)
//! BSTR  compiled2                  igual que compiled; en las constantes, la expresión de contexto
//! u16   kind                       0 en todo el corpus
//! u32   l24, l28, l2c              atributos de tipo (ver `Use`)
//! 7×BSTR lists                     listas separadas por '\n': nombres, TagDB.nombres, "", TagDB.nombres, "", "", ""
//! u32   l30                        tipo de dato del resultado
//! u8    flags                      6 por defecto; los bits altos (0x10..0x80) varían en el corpus
//! u32   n + n×(BSTR, BSTR)         placeholders (nombre, valor)
//! ```
//! Vijeo vuelve a enlazar por nombre al pegar: los ids de `raw` pueden ir a 0.

use super::bin::{Reader, Writer, R};

pub const EXPR_MAGIC: u32 = 0x3F3;

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub raw: String,
    pub compiled: String,
    pub compiled2: String,
    pub kind: u16,
    pub l24: u32,
    pub l28: u32,
    pub l2c: u32,
    pub lists: [String; 7],
    pub l30: u32,
    pub flags: u8,
    pub placeholders: Vec<(String, String)>,
}

/// Uso de una referencia, con los atributos (l24, l28, l2c, l30) vistos en el corpus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Use {
    /// Condición booleana (lámpara, habilitación, visibilidad): `(TagDB.X.getIntValue(0) != 0)`.
    Cond,
    /// Destino de una operación de bit: `TagDB.X`.
    BitDest,
    /// Destino de una operación de palabra: `TagDB.X`.
    WordDest,
    /// Valor entero: `(TagDB.X.getIntValue(0))`.
    IntValue,
    /// Valor real: `(TagDB.X.getFloatValue(0))`.
    FloatValue,
}

impl Use {
    pub fn attrs(self) -> (u32, u32, u32, u32) {
        match self {
            Use::Cond => (0x20, 8, 2, 0x20),
            Use::BitDest => (2, 2, 2, 2),
            Use::WordDest => (0x40c, 2, 2, 4),
            Use::IntValue => (0x40, 0x1005, 2, 0x40),
            Use::FloatValue => (0x80, 0x1005, 2, 0x80),
        }
    }
}

/// `Carpeta.Sub.Var` → `{F,2:2.0,Carpeta}.{F,2:2.0,Sub}.{T,2:2.0,Var}` (ids a 0: Vijeo los corrige al pegar).
pub fn raw_ref(path: &str) -> String {
    let parts: Vec<&str> = path.split('.').collect();
    let n = parts.len();
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{{{},2:2.0,{p}}}", if i + 1 == n { "T" } else { "F" }))
        .collect::<Vec<_>>()
        .join(".")
}

impl Expr {
    pub fn parse(r: &mut Reader) -> R<Expr> {
        r.expect_u32(EXPR_MAGIC, "CExpression")?;
        let raw = r.bstr()?;
        let compiled = r.bstr()?;
        let compiled2 = r.bstr()?;
        let kind = r.u16()?;
        let (l24, l28, l2c) = (r.u32()?, r.u32()?, r.u32()?);
        let mut lists: [String; 7] = Default::default();
        for s in lists.iter_mut() {
            *s = r.bstr()?;
        }
        let l30 = r.u32()?;
        let flags = r.u8()?;
        let at = r.p;
        let n = r.u32()?;
        if n > 1024 {
            return Err(format!("CExpression: {n} placeholders en {at:#x}"));
        }
        let mut placeholders = Vec::with_capacity(n as usize);
        for _ in 0..n {
            placeholders.push((r.bstr()?, r.bstr()?));
        }
        Ok(Expr { raw, compiled, compiled2, kind, l24, l28, l2c, lists, l30, flags, placeholders })
    }

    pub fn write(&self, w: &mut Writer) {
        w.u32(EXPR_MAGIC);
        w.bstr(&self.raw);
        w.bstr(&self.compiled);
        w.bstr(&self.compiled2);
        w.u16(self.kind);
        w.u32(self.l24);
        w.u32(self.l28);
        w.u32(self.l2c);
        for s in &self.lists {
            w.bstr(s);
        }
        w.u32(self.l30);
        w.u8(self.flags);
        w.u32(self.placeholders.len() as u32);
        for (k, v) in &self.placeholders {
            w.bstr(k);
            w.bstr(v);
        }
    }

    /// Referencia a la variable `path` (`Carpeta.Var`) para el uso indicado.
    pub fn var(path: &str, u: Use) -> Expr {
        let tag = format!("TagDB.{path}");
        let c = match u {
            Use::Cond => format!("({tag}.getIntValue(0) != 0)"),
            Use::BitDest | Use::WordDest => tag.clone(),
            Use::IntValue => format!("({tag}.getIntValue(0))"),
            Use::FloatValue => format!("({tag}.getFloatValue(0))"),
        };
        let (l24, l28, l2c, l30) = u.attrs();
        Expr {
            raw: raw_ref(path),
            compiled: c.clone(),
            compiled2: c,
            kind: 0,
            l24,
            l28,
            l2c,
            lists: [format!("{path}\n"), format!("{tag}\n"), String::new(), format!("{tag}\n"), String::new(), String::new(), String::new()],
            l30,
            flags: 6,
            placeholders: Vec::new(),
        }
    }
    pub fn cond(path: &str) -> Expr {
        Expr::var(path, Use::Cond)
    }
    pub fn bit_dest(path: &str) -> Expr {
        Expr::var(path, Use::BitDest)
    }
    pub fn word_dest(path: &str) -> Expr {
        Expr::var(path, Use::WordDest)
    }
    pub fn int_value(path: &str) -> Expr {
        Expr::var(path, Use::IntValue)
    }
    pub fn float_value(path: &str) -> Expr {
        Expr::var(path, Use::FloatValue)
    }
    /// Constante numérica (`"1000"`); `context` es la expresión compilada a la que acompaña
    /// (p. ej. el destino de la operación) y va en la tercera cadena. Atributos (0x40, 5, 1, 0x40).
    pub fn constant(value: &str, context: &str) -> Expr {
        Expr {
            raw: value.to_string(),
            compiled: value.to_string(),
            compiled2: context.to_string(),
            kind: 0,
            l24: 0x40,
            l28: 5,
            l2c: 1,
            lists: Default::default(),
            l30: 0x40,
            flags: 6,
            placeholders: Vec::new(),
        }
    }
}
