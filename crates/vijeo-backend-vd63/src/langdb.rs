//! Tabla de textos de un target (`<target>/…/LangManager/LangManagerData`), leída y escrita sin Vijeo.
//!
//! Ahí están los textos de las pantallas y los mensajes de alarma de las variables (la propiedad 0x0B de cada variable
//! es el id de su texto). Formato visto en 6.3 (ejemplos de Machine Expert Basic, proyectos reales y uno hecho con
//! Import Variables):
//!
//! ```text
//! u32 4 | u32 ? | u32 nº de idiomas (1) | u32 1 | u32 2 | u32 siguiente id de texto | 6 × u32 |
//! CLanguageFont (fuente del idioma) | BSTR nombre del idioma | u32 2 | u16 0 | u32 n |
//! n × (u32 id | CLanguageString: u32 2 | BSTR texto | u8 hay fuente | [CLanguageFont]) | cola
//! ```
//!
//! BSTR = `u32 bytes con el NUL | UTF-16 + NUL` (`u32 0` = sin texto). CLanguageFont v4 = `u32 4 | u32 tipo | u32
//! estilo | i32 alto | i32 ancho | BSTR cara | i32 alto GP | i32 ancho GP | u32 cara GP | u8 charset | i32 tamaño AGFA |
//! u32 cara AGFA` (`objetos/formas.md`). Las entradas no van ordenadas (salen de un mapa); los mensajes de alarma que
//! crea Vijeo van sin fuente propia y con un id nuevo del contador de la cabecera.

/// Posición del siguiente id de texto en la cabecera.
const NEXT_ID_AT: usize = 0x14;
/// Posición de la fuente del idioma en la cabecera.
const FONT_AT: usize = 0x30;
/// Bytes entre el nombre del idioma y el número de textos.
const BEFORE_COUNT: [u8; 6] = [2, 0, 0, 0, 0, 0];

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

fn put_u32(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_le_bytes());
}

/// BSTR en `i`: (texto, fin).
fn bstr(b: &[u8], i: usize) -> Option<(String, usize)> {
    let n = u32_at(b, i)? as usize;
    if n == 0 {
        return Some((String::new(), i + 4));
    }
    if n < 2 || !n.is_multiple_of(2) {
        return None;
    }
    let body = b.get(i + 4..i + 4 + n)?;
    if body[n - 2..] != [0, 0] {
        return None;
    }
    let units: Vec<u16> = body[..n - 2].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    Some((String::from_utf16(&units).ok()?, i + 4 + n))
}

fn bstr_bytes(s: &str) -> Vec<u8> {
    let mut text: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    text.extend([0, 0]);
    let mut out = (text.len() as u32).to_le_bytes().to_vec();
    out.extend(text);
    out
}

/// Fin de una CLanguageFont v4 que empieza en `i`.
fn font_end(b: &[u8], i: usize) -> Option<usize> {
    if u32_at(b, i)? != 4 {
        return None;
    }
    let (_, q) = bstr(b, i + 20)?;
    // alto, ancho y cara GP, charset, tamaño y cara AGFA.
    let end = q + 12 + 1 + 8;
    (end <= b.len()).then_some(end)
}

type Cfb = cfb::CompoundFile<std::io::Cursor<Vec<u8>>>;

/// Ruta de la tabla de textos del target `k` (en orden de creación): `/Targets/Target N/Component M/LangManager/…`.
pub fn stream_path(c: &Cfb, k: usize) -> Option<String> {
    let storage = crate::swx::target_storages(c).into_iter().nth(k)?;
    let base = format!("/Targets/{storage}");
    c.walk_storage(&base)
        .ok()?
        .filter(|e| e.is_stream() && e.name() == "LangManagerData")
        .map(|e| e.path().to_string_lossy().replace('\\', "/"))
        .find(|p| p.ends_with("/LangManager/LangManagerData"))
}

/// Tabla de textos del target `k`: `None` si no la tiene; error si no se reconoce.
pub fn load(c: &mut Cfb, k: usize) -> Option<(String, Result<LangTable, String>)> {
    let path = stream_path(c, k)?;
    let b = crate::swx::stream(c, &path)?;
    Some((path, LangTable::parse(b)))
}

/// Un texto de la tabla.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: u32,
    pub at: usize,
    /// Posición del BSTR.
    pub text_at: usize,
    /// Fin del BSTR (empieza el `u8` de la fuente).
    pub text_end: usize,
    pub end: usize,
    pub text: String,
}

pub struct LangTable {
    pub b: Vec<u8>,
    pub languages: u32,
    pub language: String,
    count_at: usize,
    pub entries: Vec<Entry>,
    entries_end: usize,
}

fn bad(what: &str) -> String {
    format!("LangManagerData: {what} no reconocido; los textos no se leen ni se editan sin Vijeo")
}

impl LangTable {
    pub fn parse(b: Vec<u8>) -> Result<Self, String> {
        if u32_at(&b, 0) != Some(4) {
            return Err(bad("la versión"));
        }
        let languages = u32_at(&b, 8).ok_or_else(|| bad("la cabecera"))?;
        let q = font_end(&b, FONT_AT).ok_or_else(|| bad("la fuente del idioma"))?;
        let (language, q) = bstr(&b, q).ok_or_else(|| bad("el nombre del idioma"))?;
        if b.get(q..q + 6) != Some(&BEFORE_COUNT[..]) {
            return Err(bad("la cabecera de la lista de textos"));
        }
        let count_at = q + 6;
        let n = u32_at(&b, count_at).ok_or_else(|| bad("el número de textos"))?;
        let mut p = count_at + 4;
        let mut entries = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let id = u32_at(&b, p).ok_or_else(|| bad("un texto"))?;
            if u32_at(&b, p + 4) != Some(2) {
                return Err(bad("un texto"));
            }
            let text_at = p + 8;
            let (text, text_end) = bstr(&b, text_at).ok_or_else(|| bad("un texto"))?;
            let end = match b.get(text_end) {
                Some(0) => text_end + 1,
                Some(1) => font_end(&b, text_end + 1).ok_or_else(|| bad("la fuente de un texto"))?,
                _ => return Err(bad("un texto")),
            };
            entries.push(Entry { id, at: p, text_at, text_end, end, text });
            p = end;
        }
        let next = u32_at(&b, NEXT_ID_AT).ok_or_else(|| bad("la cabecera"))?;
        if entries.iter().any(|e| e.id >= next) {
            return Err(bad("el contador de ids de texto"));
        }
        Ok(LangTable { b, languages, language, count_at, entries, entries_end: p })
    }

    pub fn text(&self, id: u32) -> Option<&str> {
        self.entries.iter().find(|e| e.id == id).map(|e| e.text.as_str())
    }

    pub fn has(&self, id: u32) -> bool {
        self.entries.iter().any(|e| e.id == id)
    }

    /// Ids → textos.
    pub fn texts(&self) -> std::collections::BTreeMap<u32, String> {
        self.entries.iter().map(|e| (e.id, e.text.clone())).collect()
    }

    fn editable(&self) -> Result<(), String> {
        if self.languages != 1 {
            return Err(format!("el target tiene {} idiomas: el MCP sólo cambia textos de proyectos de un idioma; hazlo en Vijeo", self.languages));
        }
        Ok(())
    }

    fn reparse(&mut self) -> Result<(), String> {
        *self = LangTable::parse(std::mem::take(&mut self.b)).map_err(|e| format!("tras el cambio, {e}"))?;
        Ok(())
    }

    /// Texto nuevo (sin fuente propia, como los mensajes de alarma de Vijeo) con el siguiente id: devuelve el id.
    pub fn add(&mut self, text: &str) -> Result<u32, String> {
        self.editable()?;
        let id = u32_at(&self.b, NEXT_ID_AT).unwrap_or_default();
        let mut entry = id.to_le_bytes().to_vec();
        entry.extend(2u32.to_le_bytes());
        entry.extend(bstr_bytes(text));
        entry.push(0);
        let at = self.entries_end;
        self.b.splice(at..at, entry);
        put_u32(&mut self.b, NEXT_ID_AT, id + 1);
        let n = self.entries.len() as u32 + 1;
        put_u32(&mut self.b, self.count_at, n);
        self.reparse()?;
        Ok(id)
    }

    /// Cambia el texto `id` (conserva su fuente).
    pub fn set(&mut self, id: u32, text: &str) -> Result<(), String> {
        self.editable()?;
        let e = self.entries.iter().find(|e| e.id == id).cloned().ok_or_else(|| format!("no hay ningún texto {id}"))?;
        self.b.splice(e.text_at..e.text_end, bstr_bytes(text));
        self.reparse()
    }

    /// Quita el texto `id` (el contador no baja, como en Vijeo).
    pub fn remove(&mut self, id: u32) -> Result<(), String> {
        self.editable()?;
        let e = self.entries.iter().find(|e| e.id == id).cloned().ok_or_else(|| format!("no hay ningún texto {id}"))?;
        self.b.drain(e.at..e.end);
        let n = self.entries.len() as u32 - 1;
        put_u32(&mut self.b, self.count_at, n);
        self.reparse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODBUS: &str = r"C:\Program Files (x86)\Schneider Electric\EcoStruxure Machine Expert - Basic\Examples\Compact_Modbus SL_M221_Book.vdz";

    fn modbus() -> Option<LangTable> {
        let p = std::path::Path::new(MODBUS);
        if !p.is_file() {
            return None;
        }
        let mut c = crate::swx::open_vdz(p).unwrap();
        Some(LangTable::parse(crate::swx::stream(&mut c, "/Targets/Target 1/Component 1/LangManager/LangManagerData").unwrap()).unwrap())
    }

    #[test]
    fn reads_the_alarm_messages_of_the_example() {
        let Some(t) = modbus() else { return };
        assert_eq!(t.entries.len(), 362);
        assert_eq!(t.languages, 1);
        assert_eq!(t.language, "Language1");
        assert_eq!(t.text(825), Some("E-Stop function is tripped."));
        assert_eq!(t.text(2), Some("Motor 1"));
    }

    #[test]
    fn adds_changes_and_removes_texts() {
        let Some(mut t) = modbus() else { return };
        let before = t.b.clone();
        let id = t.add("Nueva alarma ñ").unwrap();
        assert_eq!(id, 996);
        assert_eq!(t.text(id), Some("Nueva alarma ñ"));
        t.set(id, "Otra").unwrap();
        t.set(825, "Paro").unwrap();
        assert_eq!(t.text(id), Some("Otra"));
        assert_eq!(t.text(825), Some("Paro"));
        assert_eq!(t.text(826), Some("E-Stop module not able to start."));
        t.set(825, "E-Stop function is tripped.").unwrap();
        t.remove(id).unwrap();
        assert_eq!(t.entries.len(), 362);
        // Sólo cambia el contador.
        let mut want = before;
        put_u32(&mut want, NEXT_ID_AT, 997);
        assert_eq!(t.b, want);
        // Un texto con fuente propia conserva la fuente.
        let with_font = t.entries.iter().find(|e| t.b[e.text_end] == 1).unwrap().clone();
        let font = t.b[with_font.text_end..with_font.end].to_vec();
        t.set(with_font.id, "x").unwrap();
        let e = t.entries.iter().find(|e| e.id == with_font.id).unwrap();
        assert_eq!(t.b[e.text_end..e.end], font[..]);
    }
}
