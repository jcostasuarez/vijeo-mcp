//! Smart objects (piezas «inteligentes» de la Toolchest: fecha, hora, login, gráficos…): un grupo
//! (`GroupObject`, BaseGroup.dll) con el bit 0 de las flags de su cola a 1, más la animación SmartObject
//! (SmartObj.ocx, ranura 9) en la extensión de la cabecera común. Gramática en `docs/objetos/smartobject.md`.
//!
//! Cola del grupo (lo que va tras la lista de hijos y su `u32 0`), según la versión `v` del grupo:
//! ```text
//! v == 1        u32
//! v > 2         u32 so_flags (las flags de guardado, 9; 0 si no es smart object)
//!               si so_flags & 1: CSOItem (la definición entera, con su código Java)
//! v >= 4        u32 n + n × propiedad de la instancia (los valores: variables, colores, combos…)
//! 5 <= v <= 7   u8 hay XML (sólo se lee 0)
//! v > 5         [v > 8: u32 flags2 (9)] u32 x, u32 n + n × (BSTR clave, [flags2 & 1: BSTR valor])
//! ```
//! Un grupo normal v9 lleva `0, 0, 9, 0, 0` (20 bytes).
//!
//! - `CSOItem::Save` (HMIManagers, v10): u32 versión, u32 VersionNumber, 9 BSTR (declaraciones y código Java
//!   con marcadores `$$Propiedad$$`), u32 n + n × u32, u32 n + n × (BSTR id + `CPlaceholder`), tamaños mínimo
//!   y máximo, descripción, versión de la pieza («6.1.1.225»), clase base («CgpSmartObject»)…
//! - `CPlaceholder::Save` (v9, en un `CArchive`): la definición de una propiedad: tipo, límites, nombres en 8
//!   idiomas, conexiones y elementos de los combos.
//! - Propiedad de la instancia (BaseGroup `FUN_606e646a`, v5): u32 5, u32 flags, u32 categoría, u32 tipo,
//!   BSTR id y el valor según el tipo (variables: BSTR + `CExpression` en BSTR).
//!
//! Los hijos son objetos normales (el dibujo inicial); las propiedades no se sustituyen en ellos sino en el
//! código Java de la pieza, que el runtime ejecuta.

use super::bin::{Reader, Writer, R};
use super::expr::{raw_ref, Expr};
use super::hmi::LanguageDataList;
use crate::objects::Node;

pub const ITEM_VERSION: u32 = 10;
pub const PLACEHOLDER_VERSION: u32 = 9;
pub const PROP_VERSION: u32 = 5;

// ---------------------------------------------------------------------------------------------------------------
// CArchive: cuentas y CStringArray
// ---------------------------------------------------------------------------------------------------------------

/// `CArchive::ReadCount`: u16, o 0xFFFF + u32.
fn read_count(r: &mut Reader) -> R<usize> {
    let n = r.u16()?;
    Ok(if n == 0xFFFF { r.u32()? as usize } else { n as usize })
}

fn write_count(w: &mut Writer, n: usize) {
    if n < 0xFFFF {
        w.u16(n as u16);
    } else {
        w.u16(0xFFFF);
        w.u32(n as u32);
    }
}

fn read_strings(r: &mut Reader) -> R<Vec<String>> {
    let at = r.p;
    let n = read_count(r)?;
    if n > 4096 {
        return Err(format!("CStringArray de {n} cadenas en {at:#x}"));
    }
    (0..n).map(|_| r.cstring()).collect()
}

fn write_strings(w: &mut Writer, v: &[String]) {
    write_count(w, v.len());
    for s in v {
        w.cstring(s);
    }
}

/// BSTR que puede ser nulo (`u32 0`, distinto de la cadena vacía `u32 2 + NUL`).
fn bstr_or_null(r: &mut Reader) -> R<Option<String>> {
    if r.b.get(r.p..r.p + 4) == Some(&[0u8, 0, 0, 0][..]) {
        r.p += 4;
        return Ok(None);
    }
    r.bstr().map(Some)
}

fn f32_of(r: &mut Reader) -> R<f32> {
    Ok(f32::from_bits(r.u32()?))
}

/// Lista de u32 precedida de su número (u32).
fn read_u32s(r: &mut Reader, what: &str) -> R<Vec<u32>> {
    let at = r.p;
    let n = r.u32()? as usize;
    if n > r.left() / 4 {
        return Err(format!("{what}: {n} elementos en {at:#x}"));
    }
    (0..n).map(|_| r.u32()).collect()
}

fn write_u32s(w: &mut Writer, v: &[u32]) {
    w.u32(v.len() as u32);
    for x in v {
        w.u32(*x);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Tipos de propiedad
// ---------------------------------------------------------------------------------------------------------------

/// Tipo de una propiedad (`CPlaceholder::GetXMLName`, `PH_*Type`).
pub fn type_name(t: u32) -> &'static str {
    match t {
        1 => "bool",
        2 => "byte",
        3 => "char",
        4 => "short",
        5 => "int",
        6 => "long",
        7 => "float",
        8 => "double",
        9 => "string",
        10 => "variable",
        0xb => "color",
        0xc => "text",
        0xd => "font",
        0x10 => "text_file",
        0x11 => "parent",
        0x12 => "pattern",
        0x13 => "style",
        0x14 => "line_width",
        0x15 => "alarm_group",
        0x16 => "recipe_control",
        0x17 => "block_variable",
        0x1b => "xy_graph_variable",
        0x1c => "parison_group",
        _ => "unknown",
    }
}

fn is_tag(t: u32) -> bool {
    matches!(t, 10 | 0x17 | 0x1b)
}

fn is_int(t: u32) -> bool {
    matches!(t, 2 | 4 | 5)
}

// ---------------------------------------------------------------------------------------------------------------
// Definición: CPlaceholder y CSOItem
// ---------------------------------------------------------------------------------------------------------------

/// Límites de una propiedad numérica o de texto.
#[derive(Debug, Clone, PartialEq)]
pub enum Limits {
    None,
    Int(i32, i32),
    Float(f32, f32),
    Double(f64, f64),
    /// Cadena: longitud máxima.
    MaxLen(u32),
}

/// `CComboItem::Save`: u32 1, u32 valor, CStringArray (etiqueta en cada idioma), u32 n + n × u32.
#[derive(Debug, Clone, PartialEq)]
pub struct ComboItem {
    pub value: u32,
    pub labels: Vec<String>,
    pub extra: Vec<u32>,
}

/// `CPropertyConnection::Save`: u32 1, CString, u32.
#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub name: String,
    pub value: u32,
}

/// Definición de una propiedad (`CPlaceholder::Save`, v9).
#[derive(Debug, Clone, PartialEq)]
pub struct Placeholder {
    /// Tipo (`type_name`).
    pub ptype: u32,
    pub f8: u32,
    pub fc: u32,
    pub f10: u32,
    /// Variables: tipos que acepta (1 LoggingGroup, 2 Integer, 4 Float, 8 Boolean, 0x10 String, 0x20 …).
    pub tag_kinds: Option<u32>,
    pub limits: Limits,
    /// Nombre visible en cada idioma (8: inglés, francés, alemán, italiano, español, portugués, chino…).
    pub names: Vec<String>,
    /// Segundo CStringArray (descripciones o valores por idioma).
    pub texts: Vec<String>,
    pub connections: Vec<Connection>,
    pub combo: Vec<ComboItem>,
}

impl Placeholder {
    pub fn parse(r: &mut Reader) -> R<Placeholder> {
        r.expect_u32(PLACEHOLDER_VERSION, "CPlaceholder")?;
        let ptype = r.u32()?;
        let (f8, fc, f10) = (r.u32()?, r.u32()?, r.u32()?);
        let tag_kinds = if is_tag(ptype) { Some(r.u32()?) } else { None };
        let limits = match ptype {
            t if is_int(t) => Limits::Int(r.i32()?, r.i32()?),
            7 => Limits::Float(f32_of(r)?, f32_of(r)?),
            8 => Limits::Double(r.f64()?, r.f64()?),
            9 => Limits::MaxLen(r.u32()?),
            _ => Limits::None,
        };
        let names = read_strings(r)?;
        let texts = read_strings(r)?;
        let n = r.u32()?;
        let mut connections = Vec::new();
        for _ in 0..n {
            r.expect_u32(1, "CPropertyConnection")?;
            connections.push(Connection { name: r.cstring()?, value: r.u32()? });
        }
        let at = r.p;
        let n = r.u32()?;
        if n > 1024 {
            return Err(format!("{n} elementos de combo en {at:#x}"));
        }
        let mut combo = Vec::new();
        for _ in 0..n {
            r.expect_u32(1, "CComboItem")?;
            let value = r.u32()?;
            let labels = read_strings(r)?;
            combo.push(ComboItem { value, labels, extra: read_u32s(r, "CComboItem")? });
        }
        Ok(Placeholder { ptype, f8, fc, f10, tag_kinds, limits, names, texts, connections, combo })
    }

    pub fn write(&self, w: &mut Writer) {
        w.u32(PLACEHOLDER_VERSION);
        w.u32(self.ptype);
        w.u32(self.f8);
        w.u32(self.fc);
        w.u32(self.f10);
        if let Some(k) = self.tag_kinds {
            w.u32(k);
        }
        match self.limits {
            Limits::None => {}
            Limits::Int(a, b) => {
                w.i32(a);
                w.i32(b);
            }
            Limits::Float(a, b) => {
                w.u32(a.to_bits());
                w.u32(b.to_bits());
            }
            Limits::Double(a, b) => {
                w.f64(a);
                w.f64(b);
            }
            Limits::MaxLen(n) => w.u32(n),
        }
        write_strings(w, &self.names);
        write_strings(w, &self.texts);
        w.u32(self.connections.len() as u32);
        for c in &self.connections {
            w.u32(1);
            w.cstring(&c.name);
            w.u32(c.value);
        }
        w.u32(self.combo.len() as u32);
        for c in &self.combo {
            w.u32(1);
            w.u32(c.value);
            write_strings(w, &c.labels);
            write_u32s(w, &c.extra);
        }
    }

    /// Nombre visible en inglés (o el primero que haya).
    pub fn label(&self) -> Option<&str> {
        self.names.iter().find(|s| !s.is_empty()).map(String::as_str)
    }
}

/// Definición del smart object (`CSOItem::Save`, v10). Se lee entera para llegar a las propiedades de la
/// instancia, que van detrás sin tamaño.
#[derive(Debug, Clone, PartialEq)]
pub struct SoItem {
    pub version_number: u32,
    /// Las 9 cadenas del código (declaraciones, eventos y dibujo), en el orden en que se guardan.
    pub code: Vec<String>,
    pub ints: Vec<u32>,
    pub placeholders: Vec<(String, Placeholder)>,
    pub min_width: u32,
    pub min_height: u32,
    pub component_objects_not_needed: u32,
    pub description: String,
    /// Versión de la pieza («6.1.1.225»).
    pub item_version: String,
    pub f40: u32,
    pub f50: u32,
    /// Clase Java base («CgpSmartObject»).
    pub base_class: String,
    pub only_bitmap_objects_needed: u32,
    pub max_width: u32,
    pub max_height: u32,
}

impl SoItem {
    pub fn parse(r: &mut Reader) -> R<SoItem> {
        r.expect_u32(ITEM_VERSION, "CSOItem")?;
        let version_number = r.u32()?;
        let code = (0..9).map(|_| r.bstr()).collect::<R<Vec<_>>>()?;
        let ints = read_u32s(r, "CSOItem")?;
        let at = r.p;
        let n = r.u32()?;
        if n > 1024 {
            return Err(format!("{n} propiedades en la definición ({at:#x})"));
        }
        let mut placeholders = Vec::new();
        for _ in 0..n {
            let id = r.bstr()?;
            let p = Placeholder::parse(r).map_err(|e| format!("propiedad «{id}» de la definición: {e}"))?;
            placeholders.push((id, p));
        }
        Ok(SoItem {
            version_number,
            code,
            ints,
            placeholders,
            min_width: r.u32()?,
            min_height: r.u32()?,
            component_objects_not_needed: r.u32()?,
            description: r.bstr()?,
            item_version: r.bstr()?,
            f40: r.u32()?,
            f50: r.u32()?,
            base_class: r.bstr()?,
            only_bitmap_objects_needed: r.u32()?,
            max_width: r.u32()?,
            max_height: r.u32()?,
        })
    }

    pub fn write(&self, w: &mut Writer) {
        w.u32(ITEM_VERSION);
        w.u32(self.version_number);
        for s in &self.code {
            w.bstr(s);
        }
        write_u32s(w, &self.ints);
        w.u32(self.placeholders.len() as u32);
        for (id, p) in &self.placeholders {
            w.bstr(id);
            p.write(w);
        }
        w.u32(self.min_width);
        w.u32(self.min_height);
        w.u32(self.component_objects_not_needed);
        w.bstr(&self.description);
        w.bstr(&self.item_version);
        w.u32(self.f40);
        w.u32(self.f50);
        w.bstr(&self.base_class);
        w.u32(self.only_bitmap_objects_needed);
        w.u32(self.max_width);
        w.u32(self.max_height);
    }

    pub fn placeholder(&self, id: &str) -> Option<&Placeholder> {
        self.placeholders.iter().find(|(k, _)| k == id).map(|(_, p)| p)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Propiedades de la instancia
// ---------------------------------------------------------------------------------------------------------------

/// Valor de una propiedad de la instancia, según su tipo.
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    /// Texto o fuente (`text`, `font`): id de cadena y, con flags & 1, la lista de textos/fuentes por idioma.
    Texts { string_id: u32, texts: Option<LanguageDataList> },
    /// COLORREF 0x00BBGGRR.
    Color(u32),
    /// `parent`: sin datos.
    Empty,
    /// `alarm_group`: id y, con flags & 1, el nombre (`None` dentro: BSTR nulo, el de las piezas sin configurar).
    AlarmGroup { id: u32, name: Option<Option<String>> },
    /// Variable: BSTR (vacío en el corpus) y la referencia.
    Tag { value: String, expr: Expr },
    /// El resto (combo, números, cadenas, booleanos…) como texto.
    Text(String),
}

/// Propiedad de la instancia (BaseGroup, versión 5).
#[derive(Debug, Clone, PartialEq)]
pub struct Prop {
    pub flags: u32,
    /// Categoría (la del XML: 1 Item, 2 Boolean, 3 Color, 4 Combo, 5 Tag, 6 Text, 7 Font…).
    pub category: u32,
    pub ptype: u32,
    pub id: String,
    pub value: PropValue,
}

impl Prop {
    pub fn parse(r: &mut Reader) -> R<Prop> {
        r.expect_u32(PROP_VERSION, "propiedad del smart object")?;
        let flags = r.u32()?;
        let category = r.u32()?;
        let ptype = r.u32()?;
        let id = r.bstr()?;
        let value = match ptype {
            0xc | 0xd => {
                let string_id = r.u32()?;
                let texts = if flags & 1 != 0 { Some(LanguageDataList::parse(r)?) } else { None };
                PropValue::Texts { string_id, texts }
            }
            0xb => PropValue::Color(r.u32()?),
            0x11 => PropValue::Empty,
            0x15 => {
                let id = r.u32()?;
                PropValue::AlarmGroup { id, name: if flags & 1 != 0 { Some(bstr_or_null(r)?) } else { None } }
            }
            t if is_tag(t) => PropValue::Tag { value: r.bstr()?, expr: Expr::parse(r)? },
            _ => PropValue::Text(r.bstr()?),
        };
        Ok(Prop { flags, category, ptype, id, value })
    }

    pub fn write(&self, w: &mut Writer) {
        w.u32(PROP_VERSION);
        w.u32(self.flags);
        w.u32(self.category);
        w.u32(self.ptype);
        w.bstr(&self.id);
        match &self.value {
            PropValue::Texts { string_id, texts } => {
                w.u32(*string_id);
                if let Some(t) = texts {
                    t.write(w);
                }
            }
            PropValue::Color(c) => w.u32(*c),
            PropValue::Empty => {}
            PropValue::AlarmGroup { id, name } => {
                w.u32(*id);
                match name {
                    Some(Some(n)) => w.bstr(n),
                    Some(None) => w.u32(0),
                    None => {}
                }
            }
            PropValue::Tag { value, expr } => {
                w.bstr(value);
                expr.write(w);
            }
            PropValue::Text(s) => w.bstr(s),
        }
    }

    /// Variable de una propiedad de tipo variable (`Carpeta.Var`), sin los `$` de los marcadores.
    pub fn variable(&self) -> Option<String> {
        match &self.value {
            PropValue::Tag { expr, .. } => {
                let tag = expr.compiled.strip_prefix("TagDB.").unwrap_or(&expr.compiled);
                (!tag.is_empty()).then(|| tag.to_string())
            }
            _ => None,
        }
    }
}

/// Referencia de una propiedad de tipo variable a `path`, con los atributos que guarda Vijeo en las 36
/// propiedades configuradas de los ejemplos: compilada `TagDB.X`, `l24` 1, `l28` 2, `l2c` 2 (una variable),
/// `l30` 1. Los ids van a 0: Vijeo vuelve a enlazar por nombre al pegar.
pub fn tag_expr(path: &str) -> Expr {
    let tag = format!("TagDB.{path}");
    Expr {
        raw: raw_ref(path),
        compiled: tag.clone(),
        compiled2: tag.clone(),
        kind: 0,
        l24: 1,
        l28: 2,
        l2c: 2,
        lists: [format!("{path}\n"), format!("{tag}\n"), String::new(), format!("{tag}\n"), String::new(), String::new(), String::new()],
        l30: 1,
        flags: 6,
        placeholders: Vec::new(),
    }
}

/// Propiedad de variable vacía, como las de los ejemplos y de la Toolchest sin configurar (`kind` -1,
/// `l2c` y `l30` 0x20).
pub fn empty_tag_expr() -> Expr {
    Expr {
        raw: String::new(),
        compiled: String::new(),
        compiled2: String::new(),
        kind: 0xFFFF,
        l24: 1,
        l28: 2,
        l2c: 0x20,
        lists: Default::default(),
        l30: 0x20,
        flags: 6,
        placeholders: Vec::new(),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Cola completa
// ---------------------------------------------------------------------------------------------------------------

/// Cola de un grupo que es un smart object (o un grupo normal).
#[derive(Debug, Clone, PartialEq)]
pub struct Tail {
    /// Versión del grupo (la que va antes de los hijos).
    pub version: u32,
    pub v1: Option<u32>,
    pub so_flags: u32,
    pub item: Option<SoItem>,
    pub props: Vec<Prop>,
    pub flags2: Option<u32>,
    pub x: Option<u32>,
    pub map: Vec<(String, Option<String>)>,
}

impl Tail {
    pub fn parse(version: u32, b: &[u8]) -> R<Tail> {
        let mut r = Reader::new(b);
        let v = version;
        if matches!(v, 2 | 3) {
            return Err(format!("grupo de versión {v}: cola no soportada"));
        }
        let v1 = if v == 1 { Some(r.u32()?) } else { None };
        let so_flags = if v > 2 { r.u32()? } else { 0 };
        let item = if so_flags & 1 != 0 { Some(SoItem::parse(&mut r)?) } else { None };
        let mut props = Vec::new();
        if v >= 4 {
            let at = r.p;
            let n = r.u32()?;
            if n > 1024 {
                return Err(format!("{n} propiedades en {at:#x}"));
            }
            for i in 0..n {
                props.push(Prop::parse(&mut r).map_err(|e| format!("propiedad {i}: {e}"))?);
            }
        }
        if (5..=7).contains(&v) {
            let at = r.p;
            if r.u8()? != 0 {
                return Err(format!("grupo v{v} con XML incrustado en {at:#x}: no soportado"));
            }
        }
        let (mut flags2, mut x, mut map) = (None, None, Vec::new());
        if v > 5 {
            let f2 = if v > 8 { r.u32()? } else { 0 };
            flags2 = (v > 8).then_some(f2);
            x = Some(r.u32()?);
            let at = r.p;
            let n = r.u32()?;
            if n > 4096 {
                return Err(format!("{n} pares en {at:#x}"));
            }
            for _ in 0..n {
                let k = r.bstr()?;
                map.push((k, if f2 & 1 != 0 { Some(r.bstr()?) } else { None }));
            }
        }
        if r.left() != 0 {
            return Err(format!("sobran {} B al final de la cola del grupo ({:#x})", r.left(), r.p));
        }
        Ok(Tail { version, v1, so_flags, item, props, flags2, x, map })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut w = Writer::new();
        let v = self.version;
        if let Some(x) = self.v1 {
            w.u32(x);
        }
        if v > 2 {
            w.u32(self.so_flags);
        }
        if let Some(i) = &self.item {
            i.write(&mut w);
        }
        if v >= 4 {
            w.u32(self.props.len() as u32);
            for p in &self.props {
                p.write(&mut w);
            }
        }
        if (5..=7).contains(&v) {
            w.u8(0);
        }
        if v > 5 {
            if let Some(f) = self.flags2 {
                w.u32(f);
            }
            w.u32(self.x.unwrap_or(0));
            w.u32(self.map.len() as u32);
            for (k, val) in &self.map {
                w.bstr(k);
                if let Some(s) = val {
                    w.bstr(s);
                }
            }
        }
        w.o
    }

    pub fn prop(&self, id: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.id == id)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Sobre los nodos
// ---------------------------------------------------------------------------------------------------------------

fn group_version(n: &Node) -> Option<u32> {
    n.class_data().get(..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// Cola de un grupo que es un smart object; `None` si el nodo no lo es.
pub fn read(n: &Node) -> Option<R<Tail>> {
    let Node::Group { tail, .. } = n else { return None };
    if n.kind() != "SmartObject" {
        return None;
    }
    Some(group_version(n).ok_or_else(|| "grupo sin versión".to_string()).and_then(|v| Tail::parse(v, tail)))
}

/// Id de la pieza (`SOItemID` de la animación SmartObject).
pub fn item_id(n: &Node) -> Option<String> {
    let anims = super::anim::parse_ext(n.extension()?).ok()?;
    anims.into_iter().find_map(|a| match a {
        super::anim::Animation::SmartObject(s) => Some(s.item_id),
        _ => None,
    })
}

/// Sustituye la cola de un smart object.
pub fn write(n: &mut Node, t: &Tail) -> R<()> {
    match n {
        Node::Group { tail, .. } => {
            *tail = t.write();
            Ok(())
        }
        Node::Leaf(_) => Err("no es un grupo".into()),
    }
}

/// Lo que se pide para una propiedad.
#[derive(Debug, Clone, PartialEq)]
pub enum PropInput {
    /// Variable (`Carpeta.Var`), o vacía para quitarla.
    Variable(String),
    /// Color `RRGGBB`.
    Color(String),
    /// Combo: índice o etiqueta (en cualquier idioma).
    Combo(String),
    /// Número, cadena o booleano.
    Value(String),
}

/// Color `RRGGBB` → COLORREF 0x00BBGGRR.
fn colorref(s: &str) -> R<u32> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("color «{s}» no válido: usa RRGGBB"));
    }
    let v = u32::from_str_radix(h, 16).unwrap();
    Ok(((v & 0xFF) << 16) | (v & 0xFF00) | ((v >> 16) & 0xFF))
}

/// COLORREF → `RRGGBB`.
pub fn rgb(c: u32) -> String {
    format!("{:02X}{:02X}{:02X}", c & 0xFF, (c >> 8) & 0xFF, (c >> 16) & 0xFF)
}

/// Cambia las propiedades `sets` (por id, sin distinguir mayúsculas) de la cola. Devuelve una línea por cambio.
/// Comprueba el tipo de cada una y, en combos y números, los valores que admite la definición.
pub fn set_props(t: &mut Tail, sets: &[(String, PropInput)]) -> R<Vec<String>> {
    let mut out = Vec::new();
    for (name, input) in sets {
        let ids: Vec<String> = t.props.iter().map(|p| p.id.clone()).collect();
        let i = t
            .props
            .iter()
            .position(|p| p.id.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("el smart object no tiene la propiedad «{name}» (tiene: {})", ids.join(", ")))?;
        let def = t.item.as_ref().and_then(|it| it.placeholder(&t.props[i].id)).cloned();
        let p = &mut t.props[i];
        let kind = type_name(p.ptype);
        let bad = |what: &str| format!("«{}» es de tipo {kind}: {what}", p.id);
        match (input, &mut p.value) {
            (PropInput::Variable(v), PropValue::Tag { expr, .. }) => {
                let v = v.trim();
                *expr = if v.is_empty() { empty_tag_expr() } else { tag_expr(v) };
                out.push(format!("{} = {}", p.id, if v.is_empty() { "(sin variable)" } else { v }));
            }
            (PropInput::Color(c), PropValue::Color(old)) => {
                *old = colorref(c)?;
                out.push(format!("{} = {}", p.id, rgb(*old)));
            }
            (PropInput::Combo(s) | PropInput::Value(s), PropValue::Text(old)) => {
                let s = s.trim();
                let combo = def.as_ref().map(|d| d.combo.as_slice()).unwrap_or(&[]);
                // El valor de un combo es el `value` de la opción (el ComboIndex del XML), no su posición.
                let new = if !combo.is_empty() {
                    let by_value = s.parse::<u32>().ok().and_then(|v| combo.iter().find(|c| c.value == v));
                    let by_label = || combo.iter().find(|c| c.labels.iter().any(|l| l.trim_matches('"').eq_ignore_ascii_case(s.trim_matches('"'))));
                    match by_value.or_else(by_label) {
                        Some(c) => c.value.to_string(),
                        None => return Err(bad(&format!("«{s}» no es una opción ({})", combo_options(combo).join(", ")))),
                    }
                } else {
                    check_value(p.ptype, s, def.as_ref().map(|d| &d.limits)).map_err(|e| bad(&e))?
                };
                *old = new.clone();
                out.push(format!("{} = {new}", p.id));
            }
            (_, PropValue::Tag { .. }) => return Err(bad("indica una variable")),
            (_, PropValue::Color(_)) => return Err(bad("indica un color RRGGBB")),
            (_, PropValue::Text(_)) => return Err(bad("indica un valor")),
            _ => return Err(bad("no se puede cambiar (todavía)")),
        }
    }
    Ok(out)
}

/// Opciones de un combo: `valor «etiqueta»`.
pub fn combo_options(combo: &[ComboItem]) -> Vec<String> {
    combo.iter().map(|c| format!("{} «{}»", c.value, c.labels.first().map(String::as_str).unwrap_or("").trim_matches('"'))).collect()
}

/// Valor de una propiedad numérica, booleana o de cadena, normalizado como lo guarda Vijeo (en la Toolchest:
/// booleanos `true`/`false`, `char` de un carácter, números en texto).
fn check_value(ptype: u32, s: &str, limits: Option<&Limits>) -> Result<String, String> {
    match ptype {
        1 => match s.to_lowercase().as_str() {
            "1" | "true" | "sí" | "si" | "yes" => Ok("true".into()),
            "0" | "false" | "no" => Ok("false".into()),
            _ => Err(format!("«{s}» no es un booleano")),
        },
        3 => {
            if s.chars().count() == 1 {
                Ok(s.to_string())
            } else {
                Err(format!("«{s}» no es un carácter"))
            }
        }
        t if is_int(t) || t == 6 => {
            let v: i64 = s.parse().map_err(|_| format!("«{s}» no es un entero"))?;
            if let Some(Limits::Int(a, b)) = limits {
                if a < b && (v < *a as i64 || v > *b as i64) {
                    return Err(format!("{v} fuera de [{a}, {b}]"));
                }
            }
            Ok(v.to_string())
        }
        7 | 8 => {
            let v: f64 = s.parse().map_err(|_| format!("«{s}» no es un número"))?;
            let range = match limits {
                Some(Limits::Float(a, b)) => Some((*a as f64, *b as f64)),
                Some(Limits::Double(a, b)) => Some((*a, *b)),
                _ => None,
            };
            if let Some((a, b)) = range {
                if a < b && (v < a || v > b) {
                    return Err(format!("{v} fuera de [{a}, {b}]"));
                }
            }
            Ok(s.to_string())
        }
        9 => {
            if let Some(Limits::MaxLen(n)) = limits {
                if *n > 0 && s.chars().count() > *n as usize {
                    return Err(format!("más de {n} caracteres"));
                }
            }
            Ok(s.to_string())
        }
        _ => Err("no se puede cambiar (todavía)".into()),
    }
}

/// (id, tipo, nombre visible, valor, opciones del combo).
pub type PropDesc = (String, String, Option<String>, String, Vec<String>);

/// Descripción de cada propiedad para mostrarla.
pub fn describe(t: &Tail) -> Vec<PropDesc> {
    t.props
        .iter()
        .map(|p| {
            let def = t.item.as_ref().and_then(|it| it.placeholder(&p.id));
            let combo: Vec<String> = def.map(|d| combo_options(&d.combo)).unwrap_or_default();
            let kind = if !combo.is_empty() { "combo" } else { type_name(p.ptype) };
            let value = match &p.value {
                PropValue::Tag { .. } => p.variable().unwrap_or_default(),
                PropValue::Color(c) => rgb(*c),
                PropValue::Text(s) => s.clone(),
                PropValue::Texts { texts, .. } => texts
                    .as_ref()
                    .and_then(|l| l.items.first())
                    .map(|d| if p.ptype == 0xc { d.text.clone() } else { d.font.as_ref().unwrap_or(&d.base_font).tt_face.clone() })
                    .unwrap_or_default(),
                PropValue::AlarmGroup { name, id } => name.clone().flatten().unwrap_or_else(|| id.to_string()),
                PropValue::Empty => String::new(),
            };
            (p.id.clone(), kind.to_string(), def.and_then(|d| d.label()).map(str::to_owned), value, combo)
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn placeholder(ptype: u32, combo: &[&str]) -> Placeholder {
        Placeholder {
            ptype,
            f8: 3,
            fc: u32::MAX,
            f10: 0,
            tag_kinds: is_tag(ptype).then_some(0),
            limits: if is_int(ptype) { Limits::Int(0, 0) } else { Limits::None },
            names: vec!["Nombre".into(), String::new()],
            texts: vec![String::new(), String::new()],
            connections: vec![],
            // Valores con huecos, como DateFormat (0, 2, 4…): el valor no es la posición.
            combo: combo.iter().enumerate().map(|(i, l)| ComboItem { value: 2 * i as u32, labels: vec![l.to_string()], extra: vec![] }).collect(),
        }
    }

    pub(crate) fn sample(version: u32) -> Tail {
        let tag = Prop { flags: 9, category: 5, ptype: 10, id: "DayVariable".into(), value: PropValue::Tag { value: String::new(), expr: empty_tag_expr() } };
        Tail {
            version,
            v1: None,
            so_flags: 9,
            item: Some(SoItem {
                version_number: 1,
                code: (0..9).map(|i| format!("int $$P{i}$$;")).collect(),
                ints: vec![1, 2],
                placeholders: vec![
                    ("DayVariable".into(), placeholder(10, &[])),
                    ("Format".into(), placeholder(5, &["mm/dd/yy", "dd/mm/yy", "yy/mm/dd"])),
                    ("Color".into(), placeholder(0xb, &[])),
                ],
                min_width: 100,
                min_height: 20,
                component_objects_not_needed: 1,
                description: "Prueba".into(),
                item_version: "6.1.1.225".into(),
                f40: 1,
                f50: 1,
                base_class: "CgpSmartObject".into(),
                only_bitmap_objects_needed: 0,
                max_width: 3000,
                max_height: 3000,
            }),
            props: vec![
                tag,
                Prop { flags: 9, category: 4, ptype: 5, id: "Format".into(), value: PropValue::Text("0".into()) },
                Prop { flags: 9, category: 3, ptype: 0xb, id: "Color".into(), value: PropValue::Color(0x4A4A4A) },
                Prop { flags: 9, category: 6, ptype: 0x11, id: "Parent".into(), value: PropValue::Empty },
            ],
            flags2: (version > 8).then_some(9),
            x: (version > 5).then_some(0),
            map: vec![],
        }
    }

    #[test]
    fn roundtrip_versions() {
        for v in [4, 7, 8, 9] {
            let t = sample(v);
            let b = t.write();
            assert_eq!(Tail::parse(v, &b).unwrap(), t, "v{v}");
        }
        // Grupo normal v9: 20 bytes.
        let plain = [0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let t = Tail::parse(9, &plain).unwrap();
        assert!(t.item.is_none() && t.props.is_empty());
        assert_eq!(t.write(), plain);
        assert!(Tail::parse(9, &plain[..16]).is_err());
    }

    #[test]
    fn sets_properties() {
        let mut t = sample(9);
        let ch = set_props(
            &mut t,
            &[
                ("dayvariable".into(), PropInput::Variable("Carpeta.iDia".into())),
                ("Format".into(), PropInput::Combo("DD/MM/YY".into())),
                ("Color".into(), PropInput::Color("FF8000".into())),
            ],
        )
        .unwrap();
        assert_eq!(ch, ["DayVariable = Carpeta.iDia", "Format = 2", "Color = FF8000"]);
        assert_eq!(set_props(&mut t, &[("Format".into(), PropInput::Combo("4".into()))]).unwrap(), ["Format = 4"]);
        assert!(set_props(&mut t, &[("Format".into(), PropInput::Combo("1".into()))]).is_err(), "1 no es un valor del combo");
        set_props(&mut t, &[("Format".into(), PropInput::Combo("2".into()))]).unwrap();
        let back = Tail::parse(9, &t.write()).unwrap();
        assert_eq!(back.prop("DayVariable").unwrap().variable().as_deref(), Some("Carpeta.iDia"));
        let PropValue::Tag { expr, .. } = &back.prop("DayVariable").unwrap().value else { panic!() };
        assert_eq!((expr.l24, expr.l28, expr.l2c, expr.l30, expr.kind), (1, 2, 2, 1, 0));
        assert_eq!(expr.raw, "{F,2:2.0,Carpeta}.{T,2:2.0,iDia}");
        assert_eq!(back.prop("Color").unwrap().value, PropValue::Color(0x0080FF));
        let d = describe(&back);
        assert_eq!(d[1].1, "combo");
        assert_eq!(d[1].4, ["0 «mm/dd/yy»", "2 «dd/mm/yy»", "4 «yy/mm/dd»"]);
        assert_eq!(d[1].3, "2");
        assert_eq!(d[2].3, "FF8000");
        // Errores: tipo equivocado, opción que no existe, propiedad que no existe.
        assert!(set_props(&mut t, &[("Color".into(), PropInput::Variable("X".into()))]).is_err());
        assert!(set_props(&mut t, &[("Format".into(), PropInput::Combo("yy".into()))]).unwrap_err().contains("no es una opción"));
        assert!(set_props(&mut t, &[("Nada".into(), PropInput::Value("1".into()))]).unwrap_err().contains("no tiene"));
        // Quitar la variable.
        set_props(&mut t, &[("DayVariable".into(), PropInput::Variable(String::new()))]).unwrap();
        assert_eq!(t.prop("DayVariable").unwrap().variable(), None);
    }

    #[test]
    fn colors() {
        assert_eq!(colorref("FF8000").unwrap(), 0x0080FF);
        assert_eq!(rgb(0x0080FF), "FF8000");
        assert!(colorref("XYZ").is_err());
    }
}
