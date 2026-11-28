//! Objetos de un panel tal como los copia y pega Vijeo: formato `63INDE_VIJEO_ObjectClipboard`
//! (inventario §13.2; prototipo en `tools/vdobj`).
//!
//! - El portapapeles es un compound file con el stream `ObjectData/ObjectData` y el stream `DetachedTagSet`
//!   (las variables que usan los objetos). Las piezas de la Toolchest son lo mismo con 8 bytes delante.
//! - `ObjectData`: ancla (2 × i32: esquina superior izquierda de la selección) y los objetos de nivel
//!   superior seguidos, cada uno con el GUID de su clase y sus datos, hasta el final.
//! - Cabecera común (`BaseGraphicalObject::Save`): versión u8 = 3, id u32, rectángulo 4 × i16 absoluto e
//!   inclusivo, nombre (u32 bytes con NUL + UTF-16), flags u8 (bit 0: extensión), extensión, marca
//!   `0x1CED50DA`. Detrás, los datos propios de la clase.
//! - Grupos (también los smart objects): u32 versión, hijos `u32 tamaño + 16 bytes + tamaño bytes`, u32 0 y una
//!   cola. Al editar un hijo se recalcula su tamaño al serializar.
//!
//! Los objetos de nivel superior no llevan tamaño: se separan buscando la siguiente cabecera válida, y
//! [`Clip::parse`] comprueba que releer lo que escribe da los mismos objetos.

use std::io::{Read, Write};

const END_MARK: u32 = 0x1CED50DA;
const OFF_RECT: usize = 16 + 1 + 4; // GUID, versión, id
const OFF_NAME: usize = OFF_RECT + 8;
const CFB_MAGIC: [u8; 4] = [0xD0, 0xCF, 0x11, 0xE0];

/// Nombre del formato de portapapeles de los objetos del editor de paneles (6.3).
pub const CLIPBOARD_FORMAT: &str = "63INDE_VIJEO_ObjectClipboard";
/// Formato que pone Vijeo al copiar un panel entero en el Navigator.
pub const PANEL_CLIPBOARD_FORMAT: &str = "63INDE_VIJEO_WINDOW_CLIPBOARD_COPY_DATA";

/// Error de los objetos con la cabecera antigua (versión 2, rectángulo en doubles): Vijeo los carga, pero
/// aquí no se leen (bibliotecas `Image Library` y algunas plantillas de la Toolchest).
pub const LEGACY: &str = "cabecera de objeto antigua (versión 2), no soportada";

/// Error de las piezas de la Toolchest que son un panel entero (`WindowObject` + `GraphicalObject`).
pub const PANEL: &str = "es un panel entero, no objetos sueltos";

/// `DetachedTagSet` sin variables (el de las piezas de la Toolchest).
pub const EMPTY_TAGS: [u8; 45] = [
    0x28, 0, 0, 0, 2, 0, 0, 0, 0xef, 0xef, 0xef, 0xef, 0, 0, 0, 0, 0xa2, 0x0f, 0, 0, 0xef, 0xef, 0xef, 0xef, 0, 0, 0, 0, 0x6f, 0, 0,
    0, 0xda, 0x50, 0xed, 0x1c, 0, 0, 0, 0, 0xda, 0x50, 0xed, 0x1c, 0,
];

/// Extensión de la cabecera sin animaciones, como la escribe Vijeo.
pub const EMPTY_EXTENSION: [u8; 8] = [3, 0, 0, 0, 0, 0, 0, 0];

/// GUID de clase en bytes (orden de `WriteClassStm`) a partir de su texto `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`.
pub fn guid_bytes(s: &str) -> Result<[u8; 16], String> {
    let h: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if h.len() != 32 {
        return Err(format!("GUID no válido: {s}"));
    }
    let byte = |i: usize| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap();
    let mut g = [0u8; 16];
    // Data1 (4 B), Data2 (2 B), Data3 (2 B) en little-endian; Data4 tal cual.
    for (dst, src) in [(0, 3), (1, 2), (2, 1), (3, 0), (4, 5), (5, 4), (6, 7), (7, 6)] {
        g[dst] = byte(src);
    }
    for i in 8..16 {
        g[i] = byte(i);
    }
    Ok(g)
}

fn rd_u32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}

fn rd_i16(b: &[u8], o: usize) -> Option<i16> {
    b.get(o..o + 2).map(|s| i16::from_le_bytes(s.try_into().unwrap()))
}

fn u16_at(v: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([v[i], v[i + 1]])
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()
}

pub fn guid_str(g: &[u8]) -> String {
    let d1 = u32::from_le_bytes(g[0..4].try_into().unwrap());
    let d2 = u16::from_le_bytes(g[4..6].try_into().unwrap());
    let d3 = u16::from_le_bytes(g[6..8].try_into().unwrap());
    format!("{{{d1:08X}-{d2:04X}-{d3:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}", g[8], g[9], g[10], g[11], g[12], g[13], g[14], g[15])
}

/// GUID de clase tal como aparece en el stream (traducido por `CPFUtility::CLSIDReverseLookup`) y tipo de
/// objeto. Sólo para dar nombre a los objetos: la lectura no depende de esta tabla.
const KINDS: &[(&str, &str)] = &[
    ("{3E217838-CB27-4AE0-9CBC-F7C1D1336474}", "Group"),
    ("{A75C95D0-AA7D-11D2-AA42-00A0C9CE0C40}", "Group"),
    ("{3BE64A91-D329-4653-9891-AA4EE97464C2}", "Rectangle"),
    ("{F43A7170-B104-11D2-B86D-00A0C9A0F105}", "Rectangle"),
    ("{CFD4B0CC-678F-4F78-980C-24C14CBE68E1}", "Text"),
    ("{7AFD8809-AFD9-11D2-8CDB-00A0C9A0FF8F}", "Text"),
    ("{347CCED5-5E26-497A-A140-EA017D0272BA}", "Switch"),
    ("{C29C3B73-7AD6-4E52-A271-30A8376E05C2}", "Image"),
    ("{B926ADF7-AF05-11D2-8CDB-00A0C9A0FF8F}", "Image"),
    ("{5FD5A5E2-2428-4024-9029-EEF4A8FF82A4}", "DataDisplay"),
    ("{8CCD8CE7-3AC7-447A-BF36-C3A7A9FA73F1}", "Polyline"),
    ("{8FDB4A8B-C40F-4336-A1EA-BFDA2A4545B7}", "Polygon"),
    ("{78914787-36A8-4096-9256-DF9BD6C180AA}", "Line"),
    ("{22A41009-62E3-4612-B997-032976965397}", "Circle"),
    ("{94122865-3DE8-4419-9FC8-FDBFC6AD2092}", "Arc"),
    ("{C0E3BF5B-3D58-4D95-86B1-570E00856691}", "Pie"),
    ("{975F1095-4FAF-4F39-9E70-3AF003F4AC52}", "Dot"),
    ("{C3E49131-12E1-4B0B-B800-16EB9D8115F7}", "BezierCurve"),
    ("{F8D146D0-E08B-4233-ACF8-67AC80415EBA}", "Lamp"),
    ("{6334D9F0-692C-4022-B6AC-D89F10AA26BB}", "Lamp"),
    ("{25806192-B36E-11D1-956F-008029E62139}", "Scale"),
    ("{14A49847-15B6-4EEE-A41D-A4D5C716C886}", "Scale"),
    ("{1231AE6C-E4DB-415C-B71B-086B3BC2788B}", "BarGraph"),
    ("{EA3D9D75-06D9-416C-B794-7E520FD73B7A}", "Meter"),
    ("{7C30E94E-FE74-483D-A082-4E5636774390}", "TrendGraph"),
    ("{C1EA5369-C4ED-433D-9057-CCFA2DB2DB5B}", "JpegViewer"),
    ("{23969996-F01C-4DEA-9110-6EF30136B91D}", "NativeList"),
    ("{3030F383-8DB4-47E6-A77A-1E63AD52D861}", "TabletPart"),
];

fn kind_of(guid: &str) -> Option<&'static str> {
    KINDS.iter().find(|(g, _)| *g == guid).map(|(_, k)| *k)
}

/// Cabecera común de un objeto que empieza (con su GUID) en `o`. Devuelve dónde acaban la cabecera
/// (tras la marca de fin) y el nombre.
fn header(b: &[u8], o: usize) -> Option<(usize, String)> {
    b.get(o..o + 16)?;
    if *b.get(o + 16)? != 3 {
        return None;
    }
    let mut p = o + OFF_NAME;
    for i in 0..4 {
        rd_i16(b, o + OFF_RECT + 2 * i)?;
    }
    let n = rd_u32(b, p)? as usize;
    if n < 4 || n % 2 != 0 || n > 512 {
        return None;
    }
    p += 4;
    let s = b.get(p..p + n)?;
    if s[n - 2] != 0 || s[n - 1] != 0 {
        return None;
    }
    let units: Vec<u16> = s[..n - 2].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    if units.iter().any(|&c| c < 0x20) {
        return None;
    }
    let name = String::from_utf16(&units).ok()?;
    p += n;
    let flags = *b.get(p)?;
    p += 1;
    let mark = if flags & 1 == 0 {
        (rd_u32(b, p)? == END_MARK).then_some(p)?
    } else {
        // Extensión (IPersistStream): empieza por u32 3, es corta y termina en la marca.
        if rd_u32(b, p)? != 3 {
            return None;
        }
        (p + 4..p + 0x10000).find(|&q| rd_u32(b, q) == Some(END_MARK))?
    };
    Some((mark + 4, name))
}

/// Objeto serializado (GUID + datos). Los grupos se separan en cabecera (hasta la versión del grupo
/// incluida), hijos (16 bytes que Vijeo no usa + el hijo) y cola, para recalcular los tamaños al editar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Leaf(Vec<u8>),
    Group { head: Vec<u8>, children: Vec<([u8; 16], Node)>, tail: Vec<u8> },
}

impl Node {
    /// Lee el objeto que empieza en `o` y termina como mucho en `limit`. Devuelve el nodo y, si es un grupo,
    /// dónde acaba su lista de hijos (la cola no lleva tamaño; para un hijo, el final lo da su tamaño).
    fn parse(b: &[u8], o: usize, limit: usize, depth: usize) -> Option<(Node, Option<usize>)> {
        let (base_end, _) = header(b, o)?;
        if depth < 32 {
            if let Some((children, list_end)) = Self::parse_children(b, base_end, limit, depth) {
                return Some((Node::Group { head: b[o..base_end + 4].to_vec(), children, tail: Vec::new() }, Some(list_end)));
            }
        }
        Some((Node::Leaf(Vec::new()), None))
    }

    /// Lista de hijos de grupo en `p` (versión u32 + [tamaño, 16 bytes, GUID…]… + 0).
    fn parse_children(b: &[u8], p: usize, limit: usize, depth: usize) -> Option<(Vec<([u8; 16], Node)>, usize)> {
        let version = rd_u32(b, p)?;
        if version == 0 || version > 32 {
            return None;
        }
        let mut q = p + 4;
        let mut out = Vec::new();
        loop {
            let size = rd_u32(b, q)? as usize;
            if size == 0 {
                q += 4;
                break;
            }
            let start = q + 4 + 16;
            let end = start.checked_add(size)?;
            if end > limit {
                return None;
            }
            let junk: [u8; 16] = b.get(q + 4..start)?.try_into().ok()?;
            let (node, list_end) = Node::parse(b, start, end, depth + 1)?;
            out.push((junk, node.complete(b, start, end, list_end)));
            q = end;
        }
        (!out.is_empty()).then_some((out, q))
    }

    /// Rellena los bytes del nodo sabiendo dónde acaba (`end`).
    fn complete(self, b: &[u8], start: usize, end: usize, list_end: Option<usize>) -> Node {
        match (self, list_end) {
            (Node::Group { head, children, .. }, Some(le)) => Node::Group { head, children, tail: b[le..end].to_vec() },
            _ => Node::Leaf(b[start..end].to_vec()),
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Node::Leaf(v) => out.extend_from_slice(v),
            Node::Group { head, children, tail } => {
                out.extend_from_slice(head);
                for (junk, c) in children {
                    let cb = c.bytes();
                    out.extend_from_slice(&(cb.len() as u32).to_le_bytes());
                    out.extend_from_slice(junk);
                    out.extend_from_slice(&cb);
                }
                out.extend_from_slice(&0u32.to_le_bytes());
                out.extend_from_slice(tail);
            }
        }
    }

    fn own(&self) -> &Vec<u8> {
        match self {
            Node::Leaf(v) => v,
            Node::Group { head, .. } => head,
        }
    }

    fn own_mut(&mut self) -> &mut Vec<u8> {
        match self {
            Node::Leaf(v) => v,
            Node::Group { head, .. } => head,
        }
    }

    /// Objeto nuevo sin hijos: cabecera común (id 0: Vijeo asigna uno al pegar) y datos de la clase.
    /// `rect` = left, top, ancho, alto. `extension` son las animaciones (sin ellas, [`EMPTY_EXTENSION`]).
    pub fn new_leaf(guid: [u8; 16], name: &str, rect: [i32; 4], extension: &[u8], class_data: &[u8]) -> Result<Node, String> {
        let mut v = guid.to_vec();
        v.push(3);
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&[0u8; 8]);
        v.extend_from_slice(&4u32.to_le_bytes());
        v.extend_from_slice(&[b'X', 0, 0, 0]);
        v.push(1);
        v.extend_from_slice(extension);
        v.extend_from_slice(&END_MARK.to_le_bytes());
        v.extend_from_slice(class_data);
        let mut n = Node::Leaf(v);
        n.rename(name)?;
        let [l, t, w, h] = rect;
        if w < 1 || h < 1 {
            return Err(format!("tamaño no válido: {w}×{h}"));
        }
        n.set_rect([l, t, l + w - 1, t + h - 1])?;
        Ok(n)
    }

    /// Posición del byte de flags y del final de la cabecera común (tras la marca) en `own()`.
    fn layout(&self) -> (usize, usize) {
        let h = self.own();
        let n = rd_u32(h, OFF_NAME).unwrap() as usize;
        let (end, _) = header(h, 0).expect("cabecera ya validada al leer");
        (OFF_NAME + 4 + n, end)
    }

    /// Extensión de la cabecera (las animaciones), si la hay.
    pub fn extension(&self) -> Option<&[u8]> {
        let (fl, end) = self.layout();
        let h = self.own();
        (h[fl] & 1 == 1).then(|| &h[fl + 1..end - 4])
    }

    /// Sustituye la extensión (las animaciones).
    pub fn set_extension(&mut self, ext: &[u8]) -> Result<(), String> {
        if rd_u32(ext, 0) != Some(3) {
            return Err("la extensión tiene que empezar por u32 3".into());
        }
        let (fl, end) = self.layout();
        let h = self.own_mut();
        let flags = h[fl] | 1;
        let mut field = vec![flags];
        field.extend_from_slice(ext);
        h.splice(fl..end - 4, field);
        Ok(())
    }

    /// Datos propios de la clase (lo que va detrás de la cabecera común). En un grupo, la versión del grupo.
    pub fn class_data(&self) -> &[u8] {
        let (_, end) = self.layout();
        &self.own()[end..]
    }

    /// Sustituye los datos de la clase de un objeto que no es un grupo.
    pub fn set_class_data(&mut self, data: &[u8]) -> Result<(), String> {
        if self.is_group() {
            return Err("los datos de un grupo no se sustituyen enteros".into());
        }
        let (_, end) = self.layout();
        let h = self.own_mut();
        h.truncate(end);
        h.extend_from_slice(data);
        Ok(())
    }

    pub fn guid(&self) -> String {
        guid_str(&self.own()[..16])
    }

    /// Tipo de objeto (`Switch`, `Text`…); `SmartObject` para los grupos que son smart objects y el GUID
    /// para las clases que no están en la tabla.
    pub fn kind(&self) -> String {
        let g = self.guid();
        match (kind_of(&g), self) {
            (Some("Group"), Node::Group { tail, .. }) if rd_u32(tail, 0).is_some_and(|f| f & 1 != 0) => "SmartObject".into(),
            (Some(k), _) => k.into(),
            (None, Node::Group { .. }) => "Group".into(),
            (None, _) => g,
        }
    }

    pub fn is_group(&self) -> bool {
        matches!(self, Node::Group { .. })
    }

    pub fn children(&self) -> &[([u8; 16], Node)] {
        match self {
            Node::Group { children, .. } => children,
            Node::Leaf(_) => &[],
        }
    }

    /// Sustituye `old` por `new` dentro de las cadenas del objeto y de todo lo que contiene.
    /// Cambia con `f` las cadenas del objeto y de todos sus hijos; devuelve cuántas cambiaron.
    pub fn edit_all_strings(&mut self, f: &dyn Fn(&str) -> Option<String>) -> Result<usize, String> {
        let mut n = self.edit_strings(f)?;
        if let Node::Group { children, .. } = self {
            for (_, c) in children {
                n += c.edit_all_strings(f)?;
            }
        }
        Ok(n)
    }

    pub fn replace_all(&mut self, old: &str, new: &str) -> Result<usize, String> {
        let mut n = self.replace(old, new)?;
        if let Node::Group { children, .. } = self {
            for (_, c) in children {
                n += c.replace_all(old, new)?;
            }
        }
        Ok(n)
    }

    pub fn name(&self) -> String {
        let h = self.own();
        let n = rd_u32(h, OFF_NAME).unwrap() as usize;
        let units: Vec<u16> = h[OFF_NAME + 4..OFF_NAME + 2 + n].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    }

    /// left, top, right, bottom (inclusivos).
    pub fn rect(&self) -> [i32; 4] {
        let h = self.own();
        std::array::from_fn(|i| rd_i16(h, OFF_RECT + 2 * i).unwrap() as i32)
    }

    pub fn set_rect(&mut self, r: [i32; 4]) -> Result<(), String> {
        let mut v = [0i16; 4];
        for (i, x) in r.iter().enumerate() {
            v[i] = i16::try_from(*x).map_err(|_| format!("coordenada fuera de rango: {x}"))?;
        }
        if v[2] < v[0] || v[3] < v[1] {
            return Err(format!("rectángulo vacío: {r:?}"));
        }
        let h = self.own_mut();
        for (i, x) in v.iter().enumerate() {
            h[OFF_RECT + 2 * i..OFF_RECT + 2 * i + 2].copy_from_slice(&x.to_le_bytes());
        }
        Ok(())
    }

    pub fn rename(&mut self, new: &str) -> Result<(), String> {
        if new.is_empty() || new.chars().any(|c| (c as u32) < 0x20) || new.encode_utf16().count() > 250 {
            return Err(format!("nombre de objeto no válido: «{new}»"));
        }
        let h = self.own_mut();
        let n = rd_u32(h, OFF_NAME).unwrap() as usize;
        let mut enc = utf16(new);
        enc.extend_from_slice(&[0, 0]);
        let mut field = (enc.len() as u32).to_le_bytes().to_vec();
        field.extend_from_slice(&enc);
        h.splice(OFF_NAME..OFF_NAME + 4 + n, field);
        Ok(())
    }

    /// Desplaza el objeto y todos sus hijos.
    pub fn shift(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        let r = self.rect();
        self.set_rect([r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy])?;
        if let Node::Group { children, .. } = self {
            for (_, c) in children {
                c.shift(dx, dy)?;
            }
        }
        self.map_line(|x, y| (x + dx as f64, y + dy as f64))
    }

    /// Cambia el rectángulo (left, top, right, bottom inclusivos) de un objeto suelto. Las líneas llevan sus
    /// extremos en coordenadas absolutas dentro de los datos de la clase: se llevan al rectángulo nuevo.
    pub fn resize(&mut self, r: [i32; 4]) -> Result<(), String> {
        let old = self.rect();
        self.set_rect(r)?;
        let scale = |v: f64, o0: i32, o1: i32, n0: i32, n1: i32| {
            if o1 == o0 {
                n0 as f64 + (v - o0 as f64)
            } else {
                n0 as f64 + (v - o0 as f64) * (n1 - n0) as f64 / (o1 - o0) as f64
            }
        };
        self.map_line(|x, y| (scale(x, old[0], old[2], r[0], r[2]), scale(y, old[1], old[3], r[1], r[3])))
    }

    /// Aplica `f` a los extremos si el objeto es una línea.
    fn map_line(&mut self, f: impl Fn(f64, f64) -> (f64, f64)) -> Result<(), String> {
        if !matches!(self, Node::Leaf(_)) || self.guid() != crate::widgets::shapes::GUID_LINE {
            return Ok(());
        }
        let mut l = crate::widgets::shapes::Line::parse(self.class_data())?;
        l.start = f(l.start.0, l.start.1);
        l.end = f(l.end.0, l.end.1);
        self.set_class_data(&l.write())
    }

    /// Ajusta el rectángulo de un grupo (y de sus subgrupos) a la unión del de sus hijos, como lo guarda Vijeo.
    pub fn refit(&mut self) -> Result<(), String> {
        if let Node::Group { children, .. } = self {
            let mut u = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
            for (_, c) in children.iter_mut() {
                c.refit()?;
                let r = c.rect();
                u = [u[0].min(r[0]), u[1].min(r[1]), u[2].max(r[2]), u[3].max(r[3])];
            }
            if !children.is_empty() {
                self.set_rect(u)?;
            }
        }
        Ok(())
    }

    /// Principio de los datos propios del objeto tras el nombre.
    fn data_start(&self) -> usize {
        OFF_NAME + 4 + rd_u32(self.own(), OFF_NAME).unwrap() as usize
    }

    /// Cadenas de los datos propios (no de los hijos), en orden.
    /// Bytes propios del objeto, sin los de sus hijos (cabecera y cola de un grupo).
    pub fn own_bytes(&self) -> Vec<u8> {
        match self {
            Node::Leaf(v) => v.clone(),
            Node::Group { head, tail, .. } => [head.as_slice(), tail.as_slice()].concat(),
        }
    }

    pub fn strings(&self) -> Vec<String> {
        let mut out = strings_in(self.own(), self.data_start());
        if let Node::Group { tail, .. } = self {
            out.extend(strings_in(tail, 0));
        }
        out
    }

    /// Sustituye una cadena completa `old` por `new` en los datos propios. Devuelve cuántas cambió.
    pub fn set_text(&mut self, old: &str, new: &str) -> Result<usize, String> {
        self.edit_strings(&|s| (s == old).then(|| new.to_owned()))
    }

    /// Sustituye `old` por `new` dentro de las cadenas de los datos propios. Devuelve cuántas cambió.
    pub fn replace(&mut self, old: &str, new: &str) -> Result<usize, String> {
        if old.is_empty() {
            return Err("el texto a sustituir está vacío".into());
        }
        self.edit_strings(&|s| s.contains(old).then(|| s.replace(old, new)))
    }

    fn edit_strings(&mut self, f: &dyn Fn(&str) -> Option<String>) -> Result<usize, String> {
        let start = self.data_start();
        let mut n = edit_strings_in(self.own_mut(), start, f)?;
        if let Node::Group { tail, .. } = self {
            n += edit_strings_in(tail, 0, f)?;
        }
        Ok(n)
    }
}

/// Cadena que empieza en `i`: (inicio de los caracteres, fin de la cadena, texto).
/// Formatos: u32 bytes (con NUL) + UTF-16 + NUL, y CString de MFC (`FF FE FF` + longitud u8 + UTF-16).
fn string_at(v: &[u8], i: usize) -> Option<(usize, usize, String, bool)> {
    let printable = |u: u16| u >= 0x20 || u == 0x09 || u == 0x0A || u == 0x0D;
    if let Some(n) = rd_u32(v, i).map(|n| n as usize) {
        if (4..=0x4000).contains(&n) && n % 2 == 0 && i + 4 + n <= v.len() && u16_at(v, i + 4 + n - 2) == 0 {
            let units: Vec<u16> = v[i + 4..i + 2 + n].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            if units.iter().all(|&u| printable(u)) {
                return Some((i + 4, i + 4 + n, String::from_utf16_lossy(&units), false));
            }
        }
    }
    if v.get(i..i + 3) == Some(&[0xFF, 0xFE, 0xFF]) {
        let n = *v.get(i + 3)? as usize;
        if n > 0 && n < 0xFF && i + 4 + 2 * n <= v.len() {
            let units: Vec<u16> = v[i + 4..i + 4 + 2 * n].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            if units.iter().all(|&u| printable(u)) {
                return Some((i + 4, i + 4 + 2 * n, String::from_utf16_lossy(&units), true));
            }
        }
    }
    None
}

pub fn strings_in(v: &[u8], from: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = from;
    while i + 4 <= v.len() {
        match string_at(v, i) {
            Some((_, end, s, _)) => {
                out.push(s);
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

fn encode_string(s: &str, cstring: bool) -> Result<Vec<u8>, String> {
    let units = utf16(s);
    if cstring {
        let n = units.len() / 2;
        if n == 0 || n >= 0xFF {
            return Err(format!("la cadena «{s}» tiene que tener entre 1 y 254 caracteres"));
        }
        let mut out = vec![0xFF, 0xFE, 0xFF, n as u8];
        out.extend_from_slice(&units);
        Ok(out)
    } else {
        let mut out = ((units.len() + 2) as u32).to_le_bytes().to_vec();
        out.extend_from_slice(&units);
        out.extend_from_slice(&[0, 0]);
        Ok(out)
    }
}

pub fn edit_strings_in(v: &mut Vec<u8>, from: usize, f: &dyn Fn(&str) -> Option<String>) -> Result<usize, String> {
    let mut n = 0;
    let mut i = from;
    while i + 4 <= v.len() {
        match string_at(v, i) {
            Some((_, end, s, cstring)) => match f(&s) {
                Some(new) => {
                    let enc = encode_string(&new, cstring)?;
                    let len = enc.len();
                    v.splice(i..end, enc);
                    i += len;
                    n += 1;
                }
                None => i = end,
            },
            None => i += 1,
        }
    }
    Ok(n)
}

/// Contenido del portapapeles de objetos: ancla, objetos de nivel superior y `DetachedTagSet`.
#[derive(Debug, Clone)]
pub struct Clip {
    pub anchor: (i32, i32),
    pub objects: Vec<Node>,
    pub tags: Vec<u8>,
}

/// Índices desde el nivel superior hasta un objeto.
pub type ObjPath = Vec<usize>;

impl Clip {
    /// Lee el compound file del portapapeles, una pieza de la Toolchest (8 bytes + compound file) o el
    /// stream `ObjectData` suelto.
    pub fn from_bytes(raw: &[u8]) -> Result<Clip, String> {
        let raw = if raw.get(8..12) == Some(&CFB_MAGIC) { &raw[8..] } else { raw };
        if !raw.starts_with(&CFB_MAGIC) {
            return Self::parse(raw, EMPTY_TAGS.to_vec());
        }
        let mut c = cfb::CompoundFile::open(std::io::Cursor::new(raw)).map_err(|e| format!("no es un compound file válido: {e}"))?;
        if c.is_stream("/ObjectData/WindowObject") {
            return Err(PANEL.into());
        }
        let mut od = Vec::new();
        c.open_stream("/ObjectData/ObjectData").map_err(|e| format!("no tiene ObjectData/ObjectData: {e}"))?.read_to_end(&mut od).map_err(|e| e.to_string())?;
        let mut tags = Vec::new();
        match c.open_stream("/DetachedTagSet") {
            Ok(mut s) => {
                s.read_to_end(&mut tags).map_err(|e| e.to_string())?;
            }
            Err(_) => tags = EMPTY_TAGS.to_vec(),
        }
        Self::parse(&od, tags)
    }

    /// Lee el stream `ObjectData`.
    pub fn parse(b: &[u8], tags: Vec<u8>) -> Result<Clip, String> {
        let x = rd_u32(b, 0).ok_or("ObjectData demasiado corto")? as i32;
        let y = rd_u32(b, 4).ok_or("ObjectData demasiado corto")? as i32;
        if b.get(8 + 16) == Some(&2) {
            return Err(LEGACY.into());
        }
        let mut objects = Vec::new();
        let mut o = 8;
        while o < b.len() {
            let (node, list_end) = Node::parse(b, o, b.len(), 0).ok_or_else(|| format!("no hay una cabecera de objeto válida en el byte {o}"))?;
            let (base_end, _) = header(b, o).unwrap();
            // Fin: la siguiente cabecera válida tras la cola del grupo o los datos del objeto.
            let from = list_end.unwrap_or(base_end);
            let next = (from..b.len().saturating_sub(16)).find(|&q| header(b, q).is_some()).unwrap_or(b.len());
            objects.push(node.complete(b, o, next, list_end));
            o = next;
        }
        let clip = Clip { anchor: (x, y), objects, tags };
        if clip.stream() != b {
            return Err("los objetos leídos no reproducen el stream (formato no reconocido)".into());
        }
        Ok(clip)
    }

    /// Stream `ObjectData`.
    pub fn stream(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.anchor.0.to_le_bytes());
        out.extend_from_slice(&self.anchor.1.to_le_bytes());
        for n in &self.objects {
            n.write(&mut out);
        }
        out
    }

    /// Compound file para el portapapeles (v3, como el que crea Vijeo con `StgCreateDocfileOnILockBytes`).
    pub fn to_cfb(&self) -> Result<Vec<u8>, String> {
        let cur = std::io::Cursor::new(Vec::new());
        let mut c = cfb::CompoundFile::create_with_version(cfb::Version::V3, cur).map_err(|e| e.to_string())?;
        c.create_storage("/ObjectData").map_err(|e| e.to_string())?;
        c.create_stream("/ObjectData/ObjectData").map_err(|e| e.to_string())?.write_all(&self.stream()).map_err(|e| e.to_string())?;
        c.create_stream("/DetachedTagSet").map_err(|e| e.to_string())?.write_all(&self.tags).map_err(|e| e.to_string())?;
        c.flush().map_err(|e| e.to_string())?;
        Ok(c.into_inner().into_inner())
    }

    /// Recalcula el ancla: esquina superior izquierda de todos los objetos (Vijeo comprueba que caiga en el panel).
    pub fn reanchor(&mut self) {
        if self.objects.is_empty() {
            return;
        }
        let x = self.objects.iter().map(|n| n.rect()[0]).min().unwrap();
        let y = self.objects.iter().map(|n| n.rect()[1]).min().unwrap();
        self.anchor = (x, y);
    }

    pub fn node(&self, path: &[usize]) -> &Node {
        let mut n = &self.objects[path[0]];
        for &i in &path[1..] {
            n = &n.children()[i].1;
        }
        n
    }

    pub fn node_mut(&mut self, path: &[usize]) -> &mut Node {
        let mut n = &mut self.objects[path[0]];
        for &i in &path[1..] {
            n = match n {
                Node::Group { children, .. } => &mut children[i].1,
                Node::Leaf(_) => unreachable!("ruta a través de un objeto que no es un grupo"),
            };
        }
        n
    }

    /// Todos los objetos con su ruta de índices y de nombres (`Grupo/Objeto`).
    pub fn walk(&self) -> Vec<(ObjPath, String)> {
        fn go(n: &Node, idx: ObjPath, prefix: &str, out: &mut Vec<(ObjPath, String)>) {
            let path = if prefix.is_empty() { n.name() } else { format!("{prefix}/{}", n.name()) };
            out.push((idx.clone(), path.clone()));
            for (i, (_, c)) in n.children().iter().enumerate() {
                let mut ci = idx.clone();
                ci.push(i);
                go(c, ci, &path, out);
            }
        }
        let mut out = Vec::new();
        for (i, n) in self.objects.iter().enumerate() {
            go(n, vec![i], "", &mut out);
        }
        out
    }

    /// Busca un objeto por ruta de nombres (`Grupo/Objeto`) o por un nombre que no se repita.
    pub fn resolve(&self, want: &str) -> Result<ObjPath, String> {
        let want = want.trim().trim_matches('/');
        let all = self.walk();
        let exact: Vec<&(ObjPath, String)> = all.iter().filter(|(_, p)| p == want).collect();
        if exact.len() == 1 {
            return Ok(exact[0].0.clone());
        }
        let by_name: Vec<&(ObjPath, String)> = if want.contains('/') { exact } else { all.iter().filter(|(_, p)| p.rsplit('/').next() == Some(want)).collect() };
        match by_name.len() {
            1 => Ok(by_name[0].0.clone()),
            0 => {
                let some: Vec<&str> = all.iter().take(40).map(|(_, p)| p.as_str()).collect();
                Err(format!("no hay ningún objeto «{want}» en el panel (hay: {})", some.join(", ")))
            }
            _ => {
                let paths: Vec<&str> = by_name.iter().map(|(_, p)| p.as_str()).collect();
                Err(format!("«{want}» se repite: indica la ruta ({})", paths.join(", ")))
            }
        }
    }

    /// Ajusta los grupos que contienen el objeto `path` a sus hijos.
    pub fn refit_ancestors(&mut self, path: &[usize]) -> Result<(), String> {
        for len in (1..path.len()).rev() {
            let n = self.node_mut(&path[..len]);
            // Sólo este nivel: los hijos ya están bien.
            if let Node::Group { children, .. } = n {
                let mut u = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
                for (_, c) in children.iter() {
                    let r = c.rect();
                    u = [u[0].min(r[0]), u[1].min(r[1]), u[2].max(r[2]), u[3].max(r[3])];
                }
                n.set_rect(u)?;
            }
        }
        Ok(())
    }

    /// Quita el objeto `path`. No deja grupos vacíos.
    pub fn remove(&mut self, path: &[usize]) -> Result<Node, String> {
        if path.len() == 1 {
            return Ok(self.objects.remove(path[0]));
        }
        let parent = self.node_mut(&path[..path.len() - 1]);
        let Node::Group { children, .. } = parent else { unreachable!() };
        if children.len() == 1 {
            return Err(format!("es el único objeto del grupo «{}»: borra el grupo", parent.name()));
        }
        let (_, n) = children.remove(*path.last().unwrap());
        self.refit_ancestors(&path[..path.len() - 1].iter().copied().chain([0]).collect::<Vec<_>>())?;
        Ok(n)
    }
}

/// Objetos de un panel tal como los guarda el `.vdz` (stream `…/PanelN/GraphicalObject`): un byte de versión, la
/// lista `u32 tamaño | 16 bytes | objeto` de los hijos de un grupo (acabada en `u32 0`) y un pie.
#[derive(Debug, Clone)]
pub struct PanelObjects {
    pub version: u8,
    pub objects: Vec<([u8; 16], Node)>,
    pub footer: Vec<u8>,
}

impl PanelObjects {
    pub fn parse(b: &[u8]) -> Result<PanelObjects, String> {
        let version = *b.first().ok_or("GraphicalObject vacío")?;
        let mut q = 1;
        let mut objects = Vec::new();
        loop {
            let size = rd_u32(b, q).ok_or_else(|| format!("GraphicalObject cortado en el byte {q}"))? as usize;
            if size == 0 {
                q += 4;
                break;
            }
            let start = q + 4 + 16;
            let end = start.checked_add(size).filter(|&e| e <= b.len()).ok_or_else(|| format!("objeto de {size} bytes fuera del stream en el byte {q}"))?;
            let junk: [u8; 16] = b[q + 4..start].try_into().unwrap();
            let (node, list_end) = Node::parse(b, start, end, 1).ok_or_else(|| format!("no hay una cabecera de objeto válida en el byte {start}"))?;
            objects.push((junk, node.complete(b, start, end, list_end)));
            q = end;
        }
        let p = PanelObjects { version, objects, footer: b[q..].to_vec() };
        if p.stream() != b {
            return Err("los objetos leídos no reproducen el stream del panel (formato no reconocido)".into());
        }
        Ok(p)
    }

    pub fn stream(&self) -> Vec<u8> {
        let mut out = vec![self.version];
        for (junk, n) in &self.objects {
            let bytes = n.bytes();
            out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(junk);
            out.extend_from_slice(&bytes);
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&self.footer);
        out
    }

    /// Todos los objetos (también los de dentro de los grupos) con su ruta de nombres (`Grupo/Objeto`).
    pub fn walk(&self) -> Vec<(String, &Node)> {
        fn go<'a>(n: &'a Node, prefix: &str, out: &mut Vec<(String, &'a Node)>) {
            let path = if prefix.is_empty() { n.name() } else { format!("{prefix}/{}", n.name()) };
            out.push((path.clone(), n));
            for (_, c) in n.children() {
                go(c, &path, out);
            }
        }
        let mut out = Vec::new();
        for (_, n) in &self.objects {
            go(n, "", &mut out);
        }
        out
    }
}

/// Objetos sintéticos para los tests (con la misma forma que los de Vijeo, sin datos de Schneider).
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    const RECT: [u8; 16] = [0x91, 0x4a, 0xe6, 0x3b, 0x29, 0xd3, 0x53, 0x46, 0x98, 0x91, 0xaa, 0x4e, 0xe9, 0x74, 0x64, 0xc2];
    const TEXT: [u8; 16] = [0xcc, 0xb0, 0xd4, 0xcf, 0x8f, 0x67, 0x78, 0x4f, 0x98, 0x0c, 0x24, 0xc1, 0x4c, 0xbe, 0x68, 0xe1];
    const GROUP: [u8; 16] = [0x38, 0x78, 0x21, 0x3e, 0x27, 0xcb, 0xe0, 0x4a, 0x9c, 0xbc, 0xf7, 0xc1, 0xd1, 0x33, 0x64, 0x74];

    fn base(guid: [u8; 16], name: &str, r: [i16; 4]) -> Vec<u8> {
        let mut v = guid.to_vec();
        v.push(3);
        v.extend_from_slice(&0u32.to_le_bytes());
        for x in r {
            v.extend_from_slice(&x.to_le_bytes());
        }
        v.extend_from_slice(&encode_string(name, false).unwrap());
        v.push(1);
        v.extend_from_slice(&3u32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&END_MARK.to_le_bytes());
        v
    }

    fn text(name: &str, r: [i16; 4], s: &str) -> Vec<u8> {
        let mut v = base(TEXT, name, r);
        v.extend_from_slice(&11u32.to_le_bytes());
        v.extend_from_slice(&[0; 7]);
        v.extend_from_slice(&encode_string("Language1", false).unwrap());
        v.extend_from_slice(&encode_string(s, false).unwrap());
        v.extend_from_slice(&encode_string("TagDB.Carpeta.xVar", false).unwrap());
        v.extend_from_slice(&[0xFF, 0xFE, 0xFF, 3, b'a', 0, b'b', 0, b'c', 0]);
        v.extend_from_slice(&[0; 5]);
        v
    }

    fn rect(name: &str, r: [i16; 4]) -> Vec<u8> {
        let mut v = base(RECT, name, r);
        v.extend_from_slice(&2u32.to_le_bytes());
        v.extend_from_slice(&[0x9f, 0xa0, 0xa4, 0, 0, 0x95, 0x30, 0, 1, 2, 3]);
        v
    }

    fn group(name: &str, r: [i16; 4], children: &[Vec<u8>]) -> Vec<u8> {
        let mut v = base(GROUP, name, r);
        v.extend_from_slice(&9u32.to_le_bytes());
        for c in children {
            v.extend_from_slice(&(c.len() as u32).to_le_bytes());
            v.extend_from_slice(&[0xAB; 16]);
            v.extend_from_slice(c);
        }
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        v
    }

    /// Un grupo con un Rectangle y un Text, y un Text suelto.
    pub fn sample() -> Vec<u8> {
        let mut od = Vec::new();
        od.extend_from_slice(&5i32.to_le_bytes());
        od.extend_from_slice(&35i32.to_le_bytes());
        od.extend(group("Grupo01", [5, 35, 314, 181], &[rect("Rect03", [5, 42, 314, 181]), text("Text07", [12, 35, 120, 49], "Date and Time")]));
        od.extend(text("Suelto", [5, 200, 113, 214], "Hola\nmundo"));
        od
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::sample;
    use super::*;

    #[test]
    fn parses_and_roundtrips() {
        let od = sample();
        let c = Clip::parse(&od, EMPTY_TAGS.to_vec()).unwrap();
        assert_eq!(c.anchor, (5, 35));
        assert_eq!(c.objects.len(), 2);
        assert_eq!(c.objects[0].kind(), "Group");
        assert_eq!(c.objects[0].children().len(), 2);
        assert_eq!(c.objects[1].name(), "Suelto");
        assert_eq!(c.objects[1].rect(), [5, 200, 113, 214]);
        assert_eq!(c.stream(), od);
        let paths: Vec<String> = c.walk().into_iter().map(|(_, p)| p).collect();
        assert_eq!(paths, ["Grupo01", "Grupo01/Rect03", "Grupo01/Text07", "Suelto"]);
        assert_eq!(c.objects[1].strings(), ["Language1", "Hola\nmundo", "TagDB.Carpeta.xVar", "abc"]);
    }

    #[test]
    fn cfb_roundtrip() {
        let c = Clip::parse(&sample(), EMPTY_TAGS.to_vec()).unwrap();
        let bytes = c.to_cfb().unwrap();
        let back = Clip::from_bytes(&bytes).unwrap();
        assert_eq!(back.stream(), c.stream());
        assert_eq!(back.tags, EMPTY_TAGS);
        // Pieza de la Toolchest: 8 bytes delante.
        let mut part = vec![2, 0, 0, 0, 0, 0, 1, 0];
        part.extend_from_slice(&bytes);
        assert_eq!(Clip::from_bytes(&part).unwrap().stream(), c.stream());
    }

    #[test]
    fn edits_keep_the_format() {
        let mut c = Clip::parse(&sample(), EMPTY_TAGS.to_vec()).unwrap();
        let p = c.resolve("Text07").unwrap();
        assert_eq!(p, vec![0, 1]);
        c.node_mut(&p).rename("Titulo largo").unwrap();
        assert_eq!(c.node_mut(&p).set_text("Date and Time", "Fecha y hora").unwrap(), 1);
        assert_eq!(c.node_mut(&p).replace("xVar", "xOtraVariable").unwrap(), 1);
        assert_eq!(c.node_mut(&p).replace("bc", "XYZ").unwrap(), 1, "también en CString");
        c.node_mut(&p).shift(0, -10).unwrap();
        c.refit_ancestors(&p).unwrap();
        // El grupo se ajusta a sus hijos: ahora empieza 10 px más arriba.
        assert_eq!(c.objects[0].rect(), [5, 25, 314, 181]);
        let s = c.stream();
        let back = Clip::parse(&s, EMPTY_TAGS.to_vec()).unwrap();
        let t = back.node(&back.resolve("Grupo01/Titulo largo").unwrap());
        assert_eq!(t.strings(), ["Language1", "Fecha y hora", "TagDB.Carpeta.xOtraVariable", "aXYZ"]);
        assert_eq!(t.rect(), [12, 25, 120, 39]);
        // Mover el grupo mueve a sus hijos.
        let mut back = back;
        back.objects[0].shift(100, 0).unwrap();
        assert_eq!(back.objects[0].children()[0].1.rect(), [105, 42, 414, 181]);
    }

    #[test]
    fn resolve_and_remove() {
        let mut c = Clip::parse(&sample(), EMPTY_TAGS.to_vec()).unwrap();
        assert!(c.resolve("Nada").unwrap_err().contains("no hay"));
        let p = c.resolve("Grupo01/Rect03").unwrap();
        c.remove(&p).unwrap();
        assert_eq!(c.objects[0].children().len(), 1);
        assert_eq!(c.objects[0].rect(), [12, 35, 120, 49]);
        let p = c.resolve("Text07").unwrap();
        assert!(c.remove(&p).unwrap_err().contains("único"));
        c.remove(&[0]).unwrap();
        c.reanchor();
        assert_eq!(c.anchor, (5, 200));
        assert_eq!(Clip::parse(&c.stream(), EMPTY_TAGS.to_vec()).unwrap().objects.len(), 1);
    }

    #[test]
    fn new_leaf_extension_and_class_data() {
        let g = guid_bytes("{347CCED5-5E26-497A-A140-EA017D0272BA}").unwrap();
        assert_eq!(guid_str(&g), "{347CCED5-5E26-497A-A140-EA017D0272BA}");
        let mut n = Node::new_leaf(g, "Boton1", [10, 20, 80, 30], &EMPTY_EXTENSION, &[6, 0, 0, 0, 9]).unwrap();
        assert_eq!(n.kind(), "Switch");
        assert_eq!(n.name(), "Boton1");
        assert_eq!(n.rect(), [10, 20, 89, 49]);
        assert_eq!(n.extension(), Some(&EMPTY_EXTENSION[..]));
        assert_eq!(n.class_data(), &[6, 0, 0, 0, 9]);
        // Se relee como un objeto válido dentro de un portapapeles.
        let mut od = vec![0u8; 8];
        od.extend_from_slice(&n.bytes());
        let c = Clip::parse(&od, EMPTY_TAGS.to_vec()).unwrap();
        assert_eq!(c.objects[0], n);
        let ext = [3, 0, 0, 0, 4, 0, 0, 0, 1, 2, 3, 4];
        n.set_extension(&ext).unwrap();
        assert_eq!(n.extension(), Some(&ext[..]));
        assert_eq!(n.name(), "Boton1");
        n.set_class_data(&[6, 0, 0, 0, 1]).unwrap();
        assert_eq!(n.class_data(), &[6, 0, 0, 0, 1]);
        assert_eq!(n.extension(), Some(&ext[..]));
        assert!(n.set_extension(&[1, 0, 0, 0]).is_err());
        assert!(Node::new_leaf(g, "X", [0, 0, 0, 5], &EMPTY_EXTENSION, &[]).is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(Clip::parse(&[0; 40], EMPTY_TAGS.to_vec()).is_err());
        let mut c = Clip::parse(&sample(), EMPTY_TAGS.to_vec()).unwrap();
        assert!(c.objects[1].rename("").is_err());
        assert!(c.objects[1].set_rect([10, 10, 5, 20]).is_err());
        assert!(c.objects[1].shift(40000, 0).is_err());
    }
}
