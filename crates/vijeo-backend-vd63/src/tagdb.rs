//! Variables y carpetas de un proyecto leídas (y escritas) directamente en su `.vdz`, sin Vijeo.
//!
//! Formato de `Services/TagDatabase` y `Services/NameServer` tomado del MCP de 8OND007
//! (github.com/8OND007/vijeo-mcp, MIT, Vijeo 6.2) y comprobado aquí contra 6.3:
//!
//! ```text
//! TagDatabase  u32 8 | u32 longitud-9 | … tipos de datos (4003) … plantilla de variable (4004)
//!              u32 n | n variables por id ascendente | MARK | u32 siguiente id | MARK MARK | 4009 …
//!              4010 tablas de grupos (alarma, registro, escaneo) | 4011 carpetas: u32 último id, u32 n, n × 4012 …
//! variable     u32 id | u32 4005 | EF×4 | u32 n | n × (u32 prop, u32 tipo, valor) |
//!              u32 nº dinámicas | [dinámicas] | u32 nº hijos | hijos | MARK
//!              (instancia de estructura: un hijo por miembro, con el índice del miembro como id)
//! valor        tipo 2 = byte, 5 = double, 4/0x17 = u32 caracteres + UTF-16, 0x13 = 12 bytes, 0 = nada, resto u32
//! carpeta      u32 id | 4012 | EF×4 | u32 id | u32 padre (0 = raíz) | u32 nodo contenedor | u32 id de nombre | 0 | 1 | MARK
//! NameServer   árbol de nodos (`u16 1 | u32 grupo | u16 tipo | u16 n | n × (u32 id, u32 bytes, UTF-16+0) | u16 hijos`)
//!              y un u32 con el último id usado. Los nombres de variables y carpetas están en el nodo contenedor
//!              de su carpeta (el de la raíz, el que lista las carpetas de primer nivel).
//! ```

use std::collections::BTreeMap;

use vijeo_core::{VariableInfo, VijeoError, VijeoResult};

const EF4: [u8; 4] = [0xEF; 4];
/// Fin de registro.
const MARK: [u8; 4] = [0xDA, 0x50, 0xED, 0x1C];
const CLS_TYPE: u32 = 4003;
const CLS_TEMPLATE: u32 = 4004;
const CLS_VAR: u32 = 4005;
const CLS_AFTER_VARS: u32 = 4009;
const CLS_GROUPS: u32 = 4010;
const CLS_FOLDERS: u32 = 4011;
const CLS_FOLDER: u32 = 4012;

/// Ids de propiedad (contrastados con el CSV de Vijeo por 8OND007 y con el XML de 6.3 aquí).
pub mod prop {
    pub const TYPE: u32 = 1;
    pub const NAME: u32 = 4;
    pub const DESCRIPTION: u32 = 5;
    pub const SHARING: u32 = 6;
    pub const INITIAL: u32 = 8;
    pub const ALARM: u32 = 0x0A;
    pub const ALARM_MESSAGE: u32 = 0x0B;
    /// Condición de una alarma de BOOL (tipo 12): 2 = When Low; sin ella, When High.
    pub const ALARM_TRIGGER: u32 = 0x0C;
    /// Severidad de la alarma (tipo 3); sin ella, 1.
    pub const ALARM_SEVERITY: u32 = 0x10;
    pub const INPUT_RANGE: u32 = 0x14;
    pub const MIN: u32 = 0x15;
    pub const MAX: u32 = 0x16;
    /// Byte a 1 en toda variable con alarma de un proyecto 6.3 (Import Variables y SET03); no sale en el XML.
    pub const ALARM_FLAG: u32 = 0x26;
    pub const BYTES: u32 = 0x29;
    pub const ADDRESS: u32 = 0x2B;
    pub const ALARM_GROUP: u32 = 0x4D;
    pub const LOGGING_GROUP: u32 = 0x4E;
    pub const SCAN_GROUP: u32 = 0x4F;
    pub const PARENT: u32 = 0x51;
    pub const EXTERNAL: u32 = 0x56;
    pub const INDIRECT: u32 = 0x5F;
    pub const INDIRECT_ADDRESS: u32 = 0x60;
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn put_u32(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_u16(b: &mut [u8], i: usize, v: u16) {
    b[i..i + 2].copy_from_slice(&v.to_le_bytes());
}

fn utf16(b: &[u8]) -> Option<String> {
    if !b.len().is_multiple_of(2) {
        return None;
    }
    String::from_utf16(&b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>()).ok()
}

fn utf16_bytes(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Cabecera de objeto: `u32 clase | EF×4`.
fn class_header(cls: u32) -> [u8; 8] {
    let mut h = [0u8; 8];
    h[..4].copy_from_slice(&cls.to_le_bytes());
    h[4..].copy_from_slice(&EF4);
    h
}

fn find_all(b: &[u8], pat: &[u8]) -> Vec<usize> {
    b.windows(pat.len()).enumerate().filter(|(_, w)| *w == pat).map(|(i, _)| i).collect()
}

fn find_from(b: &[u8], pat: &[u8], from: usize) -> Option<usize> {
    b.get(from..)?.windows(pat.len()).position(|w| w == pat).map(|i| i + from)
}

// ---------- propiedades ----------

/// Propiedad de una variable: id, tipo de valor y valor tal cual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prop {
    pub id: u32,
    pub ty: u32,
    pub raw: Vec<u8>,
}

const STR_TYPES: [u32; 2] = [4, 0x17];

impl Prop {
    pub fn text(&self) -> Option<String> {
        STR_TYPES.contains(&self.ty).then(|| utf16(self.raw.get(4..)?)).flatten()
    }

    /// Valor numérico (byte o u32); `None` en textos, doubles y otros.
    pub fn int(&self) -> Option<u32> {
        match self.raw.len() {
            1 => Some(self.raw[0] as u32),
            4 if !STR_TYPES.contains(&self.ty) => u32_at(&self.raw, 0),
            _ => None,
        }
    }

    pub fn double(&self) -> Option<f64> {
        (self.ty == 5).then(|| Some(f64::from_le_bytes(self.raw.as_slice().try_into().ok()?))).flatten()
    }

    /// Valor como texto para mostrar.
    pub fn display(&self) -> String {
        if let Some(t) = self.text() {
            t
        } else if let Some(d) = self.double() {
            d.to_string()
        } else if let Some(i) = self.int() {
            i.to_string()
        } else {
            self.raw.iter().map(|b| format!("{b:02x}")).collect()
        }
    }

    pub fn text_value(id: u32, s: &str) -> Self {
        let mut raw = (s.encode_utf16().count() as u32).to_le_bytes().to_vec();
        raw.extend(utf16_bytes(s));
        Prop { id, ty: 4, raw }
    }

    pub fn u32_value(id: u32, ty: u32, v: u32) -> Self {
        Prop { id, ty, raw: v.to_le_bytes().to_vec() }
    }

    pub fn byte_value(id: u32, v: u8) -> Self {
        Prop { id, ty: 2, raw: vec![v] }
    }

    pub fn double_value(id: u32, v: f64) -> Self {
        Prop { id, ty: 5, raw: v.to_le_bytes().to_vec() }
    }

    fn encode(&self, out: &mut Vec<u8>) {
        out.extend(self.id.to_le_bytes());
        out.extend(self.ty.to_le_bytes());
        out.extend(&self.raw);
    }
}

fn value_size(b: &[u8], ty: u32, at: usize) -> Option<usize> {
    Some(match ty {
        4 | 0x17 => 4 + 2 * u32_at(b, at)? as usize,
        0 => 0,
        2 => 1,
        5 => 8,
        0x13 => 12,
        _ => 4,
    })
}

/// `u32 n | n × (u32 id, u32 tipo, valor)` en `p`: (propiedades, fin).
fn parse_props(b: &[u8], mut p: usize) -> Option<(Vec<Prop>, usize)> {
    let n = u32_at(b, p)?;
    if n > 256 {
        return None;
    }
    p += 4;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let (id, ty) = (u32_at(b, p)?, u32_at(b, p + 4)?);
        p += 8;
        let size = value_size(b, ty, p)?;
        out.push(Prop { id, ty, raw: b.get(p..p + size)?.to_vec() });
        p += size;
    }
    Some((out, p))
}

/// Propiedades ordenadas por id, como las guarda Vijeo.
fn encode_props(props: &[Prop]) -> Vec<u8> {
    let mut sorted: Vec<&Prop> = props.iter().collect();
    sorted.sort_by_key(|p| p.id);
    let mut out = (sorted.len() as u32).to_le_bytes().to_vec();
    for p in sorted {
        p.encode(&mut out);
    }
    out
}

fn get(props: &[Prop], id: u32) -> Option<&Prop> {
    props.iter().find(|p| p.id == id)
}

// ---------- NameServer ----------

/// Nodo de `NameServer`.
#[derive(Debug, Clone)]
pub struct NsNode {
    pub at: usize,
    pub group: u32,
    pub kind: u16,
    /// (id, nombre, posición de la entrada, bytes de la entrada).
    pub entries: Vec<(u32, String, usize, usize)>,
    /// Posición del `u16` con el número de hijos.
    pub kids_at: usize,
    /// Fin del nodo con todos sus hijos.
    pub end: usize,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct NameServer {
    pub nodes: Vec<NsNode>,
    pub counter_at: usize,
}

impl NameServer {
    pub fn parse(b: &[u8]) -> Option<Self> {
        fn node(b: &[u8], at: usize, parent: Option<usize>, depth: usize, out: &mut Vec<NsNode>) -> Option<usize> {
            if depth > 64 || u16_at(b, at)? != 1 {
                return None;
            }
            let (group, kind, n) = (u32_at(b, at + 2)?, u16_at(b, at + 6)?, u16_at(b, at + 8)?);
            let mut i = at + 10;
            let mut entries = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let id = u32_at(b, i)?;
                let len = u32_at(b, i + 4)? as usize;
                let body = b.get(i + 8..i + 8 + len)?;
                if len < 2 || !len.is_multiple_of(2) || body[len - 2..] != [0, 0] {
                    return None;
                }
                entries.push((id, utf16(&body[..len - 2])?, i, 8 + len));
                i += 8 + len;
            }
            let kids = u16_at(b, i)?;
            let me = out.len();
            out.push(NsNode { at, group, kind, entries, kids_at: i, end: 0, parent });
            let mut end = i + 2;
            for _ in 0..kids {
                end = node(b, end, Some(me), depth + 1, out)?;
            }
            out[me].end = end;
            Some(end)
        }
        if u16_at(b, 0)? != 1 {
            return None;
        }
        let mut nodes = Vec::new();
        let mut end = 4;
        for _ in 0..u16_at(b, 2)? {
            end = node(b, end, None, 0, &mut nodes)?;
        }
        u32_at(b, end)?;
        Some(NameServer { nodes, counter_at: end })
    }

    pub fn name(&self, id: u32) -> Option<&str> {
        self.nodes.iter().flat_map(|n| &n.entries).find(|e| e.0 == id).map(|e| e.1.as_str())
    }

    /// Nodo cuyo grupo es `group` (los contenedores de las carpetas).
    pub fn node_of_group(&self, group: u32) -> Option<usize> {
        self.nodes.iter().position(|n| n.group == group)
    }

    /// Nodo que tiene la entrada `id`.
    pub fn node_with_entry(&self, id: u32) -> Option<usize> {
        self.nodes.iter().position(|n| n.entries.iter().any(|e| e.0 == id))
    }
}

// ---------- TagDatabase ----------

/// Carpeta de variables.
#[derive(Debug, Clone)]
pub struct Folder {
    pub id: u32,
    pub at: usize,
    pub parent: u32,
    /// Grupo del nodo de `NameServer` que lista lo que contiene.
    pub container: u32,
    /// Id de su nombre en `NameServer` (en el contenedor de la carpeta padre).
    pub name_id: u32,
    pub name: String,
    /// `Carpeta.Sub`.
    pub path: String,
}

/// Variable de primer nivel (también una instancia de estructura).
#[derive(Debug, Clone)]
pub struct Var {
    pub id: u32,
    pub at: usize,
    pub end: usize,
    pub props: Vec<Prop>,
    pub props_end: usize,
    /// `Carpeta.Variable`.
    pub name: String,
    /// Variable del sistema (`_CurPanelID`…): no sale en el XML ni se edita.
    pub system: bool,
}

/// Elemento (miembro) de una instancia de estructura.
#[derive(Debug, Clone)]
pub struct Element {
    pub index: u32,
    pub at: usize,
    pub end: usize,
    pub props: Vec<Prop>,
    pub props_end: usize,
}

/// Tipo de datos estructurado.
#[derive(Debug, Clone)]
pub struct DataType {
    pub key: u32,
    pub name: String,
    /// (índice, nombre, código de tipo) en el orden en que están.
    pub members: Vec<(u32, String, u32)>,
    /// Bytes tras el nombre: los tipos del sistema (`Authorization`) llevan ahí la clave que usan sus instancias.
    pub window: Vec<u8>,
}

pub struct TagDb {
    pub b: Vec<u8>,
    pub ns: Vec<u8>,
    pub names: NameServer,
    pub folders: Vec<Folder>,
    pub folder_header: usize,
    pub folders_end: usize,
    pub vars: Vec<Var>,
    pub count_at: usize,
    pub list_end: usize,
    pub next_id_at: usize,
    pub types: Vec<DataType>,
    /// Grupos de alarma, registro y escaneo: id → nombre.
    pub groups: BTreeMap<&'static str, BTreeMap<u32, String>>,
    /// Bloques de target que tiene la base (uno por target, en el orden de creación de los targets).
    pub targets: usize,
    /// Target cuyo bloque se lee y se edita (`select`): sus variables, carpetas y grupos están en los campos de arriba.
    pub active: usize,
    /// Textos del target activo (`LangManagerData`: id → texto) para los mensajes de alarma; vacío si no se cargaron.
    pub messages: BTreeMap<u32, String>,
}

fn bad(what: &str) -> VijeoError {
    VijeoError::Internal(format!("TagDatabase: {what} no reconocido; no se lee ni se edita sin Vijeo"))
}

impl TagDb {
    pub fn parse(tagdb: Vec<u8>, nameserver: Vec<u8>) -> VijeoResult<Self> {
        let names = NameServer::parse(&nameserver).ok_or_else(|| bad("NameServer"))?;
        let mut db = TagDb {
            b: tagdb,
            ns: nameserver,
            names,
            folders: Vec::new(),
            folder_header: 0,
            folders_end: 0,
            vars: Vec::new(),
            count_at: 0,
            list_end: 0,
            next_id_at: 0,
            types: Vec::new(),
            groups: BTreeMap::new(),
            targets: 0,
            active: 0,
            messages: BTreeMap::new(),
        };
        db.index()?;
        Ok(db)
    }

    /// Pasa a leer y editar el bloque del target `index` (0 = el primero creado).
    pub fn select(&mut self, index: usize) -> VijeoResult<()> {
        if index >= self.targets {
            return Err(bad(&format!("el target {index} (la base tiene {})", self.targets)));
        }
        self.active = index;
        self.messages.clear();
        self.index()
    }

    /// Con varios targets (verificado con dos): un contenedor `4001 | u32 nº de bloques` y, por target,
    /// `u32 k | 4004 (plantilla) … | u32 n | variables | MARK | u32 siguiente id | MARK`; detrás `MARK | 4009`. Los ids de
    /// variables y de carpetas empiezan en cada bloque; después de 4009 van, en el mismo orden, un par 4010 (grupos) /
    /// 4011 (carpetas) por target. Los tipos de datos (4003) son de todo el proyecto; NameServer también es uno, pero
    /// cada target tiene ahí su nodo de nombres de primer nivel y los de sus carpetas (los que usan las ediciones, que
    /// los buscan a partir de las variables y carpetas del bloque).
    fn index(&mut self) -> VijeoResult<()> {
        let b = &self.b;
        if u32_at(b, 0) != Some(8) || u32_at(b, 4).map(|n| n as usize + 9) != Some(b.len()) {
            return Err(bad("el campo de longitud"));
        }
        self.names = NameServer::parse(&self.ns).ok_or_else(|| bad("NameServer"))?;
        let blocks: Vec<usize> = find_all(b, &class_header(CLS_TEMPLATE));
        self.targets = blocks.len();
        let k = self.active;
        if blocks.is_empty() || k >= blocks.len() {
            return Err(bad("el bloque de variables del target"));
        }
        // Carpetas: 4011 | u32 último id | u32 n | n × (u32 id | 4012 | EF×4 | 6 × u32 | MARK).
        let fh = *find_all(b, &class_header(CLS_FOLDERS)).get(k).ok_or_else(|| bad("la lista de carpetas"))?;
        self.folder_header = fh + 8;
        let n = u32_at(b, fh + 12).ok_or_else(|| bad("la lista de carpetas"))?;
        let mut p = fh + 16;
        let mut folders = Vec::new();
        for _ in 0..n {
            let id = u32_at(b, p).ok_or_else(|| bad("una carpeta"))?;
            if b.get(p + 4..p + 12) != Some(&class_header(CLS_FOLDER)[..]) || b.get(p + 36..p + 40) != Some(&MARK[..]) {
                return Err(bad("una carpeta"));
            }
            let f = |k: usize| u32_at(b, p + 12 + 4 * k).unwrap_or_default();
            let name_id = f(3);
            folders.push(Folder {
                id,
                at: p,
                parent: f(1),
                container: f(2),
                name_id,
                name: self.names.name(name_id).unwrap_or("?").to_owned(),
                path: String::new(),
            });
            p += 40;
        }
        self.folders_end = p;
        let paths: Vec<String> = folders.iter().map(|f| folder_path(&folders, f.id)).collect();
        for (f, path) in folders.iter_mut().zip(paths) {
            f.path = path;
        }
        // Variables del bloque: u32 n | registros por id ascendente | MARK | u32 siguiente id | MARK, y luego el bloque
        // siguiente (`u32 k+1 | 4004`) o, en el último, `MARK | 4009`.
        let after = find_from(b, &class_header(CLS_AFTER_VARS), blocks[k]).ok_or_else(|| bad("el fin de la lista de variables"))?;
        let block_end = match blocks.get(k + 1) {
            Some(&next) if next >= 4 => next - 4,
            Some(_) => return Err(bad("el bloque de variables del target")),
            None => {
                if after < 4 || b[after - 4..after] != MARK {
                    return Err(bad("el fin de la lista de variables"));
                }
                after - 4
            }
        };
        let mut tops: Vec<usize> = find_all(&b[..block_end], &class_header(CLS_VAR))
            .into_iter()
            .filter(|&m| m >= 4 + 8 && m > blocks[k])
            .map(|m| m - 4)
            .filter(|&i| parse_props(b, i + 12).is_some_and(|(pr, _)| get(&pr, prop::NAME).is_some()))
            .collect();
        tops.sort_unstable();
        let first = *tops.first().ok_or_else(|| bad("la lista de variables"))?;
        self.count_at = first - 4;
        if u32_at(b, self.count_at) != Some(tops.len() as u32) {
            return Err(bad("el número de variables"));
        }
        if block_end < 12 || b[block_end - 4..block_end] != MARK {
            return Err(bad("el fin de la lista de variables"));
        }
        self.next_id_at = block_end - 8;
        self.list_end = self.next_id_at - 4;
        if b[self.list_end..self.next_id_at] != MARK {
            return Err(bad("el fin de la lista de variables"));
        }
        let mut vars = Vec::with_capacity(tops.len());
        let ends: Vec<usize> = tops.iter().skip(1).copied().chain([self.list_end]).collect();
        for (&at, &end) in tops.iter().zip(&ends) {
            let id = u32_at(b, at).unwrap_or_default();
            let (props, props_end) = parse_props(b, at + 12).ok_or_else(|| bad("una variable"))?;
            let name_id = get(&props, prop::NAME).and_then(Prop::int);
            let short = name_id.and_then(|n| self.names.name(n)).map(str::to_owned).unwrap_or_else(|| format!("#{id}"));
            let parent = get(&props, prop::PARENT).and_then(Prop::int).unwrap_or(0);
            let name = match folders.iter().find(|f| f.id == parent) {
                Some(f) => format!("{}.{short}", f.path),
                None => short,
            };
            // Las del sistema (`_CurPanelID`…) están en la raíz y empiezan por `_`, que Vijeo no admite en las del usuario.
            // (Comparten el nodo de NameServer con las variables de primer nivel del usuario.)
            let system = parent == 0 && name.starts_with('_');
            vars.push(Var { id, at, end, props, props_end, name, system });
        }
        let max_id = vars.iter().map(|v| v.id).max().unwrap_or(0);
        if u32_at(b, self.next_id_at).is_none_or(|n| n <= max_id) {
            return Err(bad("el contador de ids de variable"));
        }
        self.folders = folders;
        self.vars = vars;
        self.types = data_types(b, blocks[0]);
        self.groups = groups(b, k);
        Ok(())
    }

    /// Elementos de una instancia de estructura (vacío en las variables simples).
    pub fn elements(&self, v: &Var) -> Vec<Element> {
        let b = &self.b;
        let mut out = Vec::new();
        let q = v.props_end;
        let (Some(ndyn), Some(n)) = (u32_at(b, q), u32_at(b, q + 4)) else { return out };
        if ndyn != 0 {
            return out;
        }
        let mut q = q + 8;
        for _ in 0..n {
            let Some(index) = u32_at(b, q) else { break };
            let Some((props, props_end)) = parse_props(b, q + 12) else { break };
            let Some(end) = self.record_end(q) else { break };
            out.push(Element { index, at: q, end, props, props_end });
            q = end;
        }
        out
    }

    /// Fin de un registro de variable o elemento que empieza en `i`.
    fn record_end(&self, i: usize) -> Option<usize> {
        let b = &self.b;
        let (_, mut q) = parse_props(b, i + 12)?;
        let ndyn = u32_at(b, q)?;
        if ndyn == 0 {
            let n = u32_at(b, q + 4)?;
            q += 8;
            for _ in 0..n {
                q = self.record_end(q)?;
            }
            (b.get(q..q + 4)? == MARK).then_some(q + 4)
        } else {
            find_from(b, &MARK, q).map(|m| m + 4)
        }
    }

    /// Tipo por su código: simple, estructurado (por su clave) o del sistema (por la clave que lleva tras el nombre).
    pub fn data_type(&self, code: u32) -> Option<&DataType> {
        self.types.iter().find(|t| t.key == code).or_else(|| {
            let key = code.to_le_bytes();
            self.types.iter().find(|t| t.members.is_empty() && t.window.windows(4).any(|w| w == key))
        })
    }

    pub fn data_type_name(&self, code: Option<u32>) -> String {
        match code {
            None => "BOOL".into(),
            Some(c) => simple_type(c).map(str::to_owned).or_else(|| self.data_type(c).map(|t| t.name.clone())).unwrap_or_else(|| format!("#{c}")),
        }
    }

    fn group(&self, kind: &str, id: Option<u32>) -> Option<String> {
        let id = id?;
        Some(self.groups.get(kind).and_then(|g| g.get(&id)).cloned().unwrap_or_else(|| format!("#{id}")))
    }

    /// Descripción de una variable o elemento como en `vdz_variable_list` (lo que exporta Vijeo en el XML: las referencias
    /// indirectas son `Internal` con `Reference`, y el valor inicial sólo sale en las internas; en un BOOL sin valor,
    /// `Disabled`).
    fn describe(&self, target: &str, name: String, props: &[Prop], type_hint: Option<String>) -> VariableInfo {
        let int = |id| get(props, id).and_then(Prop::int);
        let data_type = type_hint.unwrap_or_else(|| self.data_type_name(int(prop::TYPE)));
        let indirect = int(prop::INDIRECT).is_some_and(|v| v != 0);
        let source = if get(props, prop::SCAN_GROUP).is_some() { "External" } else { "Internal" };
        let mut extra = BTreeMap::new();
        let mut put = |k: &str, v: Option<String>| {
            if let Some(v) = v.filter(|v| !v.is_empty()) {
                extra.insert(k.to_owned(), v);
            }
        };
        if source == "Internal" && !indirect {
            let signed = matches!(data_type.as_str(), "INT" | "DINT" | "Integer");
            let initial = get(props, prop::INITIAL).map(|p| match (signed, p.int(), p.raw.len()) {
                (true, Some(n), 4) => (n as i32).to_string(),
                _ => p.display(),
            });
            put("InitialValue", if data_type == "BOOL" { Some(if initial.as_deref().is_some_and(|v| v != "0") { "Enabled".into() } else { "Disabled".into() }) } else { initial });
        }
        put("ScanGroup", self.group("scan", int(prop::SCAN_GROUP)));
        // Como en el XML: alarmas definidas (con su mensaje), también las deshabilitadas (sin 0x0A). Verificado con BOOL.
        let alarms = props.iter().filter(|p| p.id == prop::ALARM_MESSAGE).count();
        put("Alarms", (alarms > 0).then(|| alarms.to_string()));
        if alarms > 0 {
            let enabled = int(prop::ALARM).is_some_and(|a| a != 0);
            put("Alarm", Some(if enabled { "Enabled" } else { "Disabled" }.into()));
            put("AlarmMessage", int(prop::ALARM_MESSAGE).and_then(|id| self.messages.get(&id).cloned()));
            // El XML sólo da condición, grupo y severidad de las alarmas activas.
            if enabled {
                put("AlarmGroup", Some(self.group("alarm", int(prop::ALARM_GROUP)).unwrap_or_else(|| "None".into())));
                put("AlarmSeverity", Some(int(prop::ALARM_SEVERITY).unwrap_or(1).to_string()));
                if data_type == "BOOL" {
                    put("AlarmTriggerCondition", Some(if int(prop::ALARM_TRIGGER) == Some(2) { "When Low" } else { "When High" }.into()));
                }
            }
        }
        put("LoggingGroup", self.group("logging", int(prop::LOGGING_GROUP)));
        put("NoOfBytes", int(prop::BYTES).map(|n| n.to_string()));
        put("Sharing", int(prop::SHARING).map(|s| if s == 2 { "Read Only".to_owned() } else { s.to_string() }));
        if indirect {
            put("Reference", get(props, prop::INDIRECT_ADDRESS).map(Prop::display));
        }
        if int(prop::INPUT_RANGE).is_some_and(|v| v != 0) {
            put("InputRange", Some(format!("{}..{}", get(props, prop::MIN).map(Prop::display).unwrap_or("0".into()), get(props, prop::MAX).map(Prop::display).unwrap_or("0".into()))));
        }
        VariableInfo {
            target: target.to_owned(),
            name,
            data_type,
            source: source.to_owned(),
            address: get(props, prop::ADDRESS).and_then(Prop::text),
            description: get(props, prop::DESCRIPTION).and_then(Prop::text),
            extra,
        }
    }

    /// Variables, instancias de estructura y (con `elements`) sus miembros, ordenadas por nombre; las del sistema (`_…`)
    /// sólo con `system`.
    pub fn variables(&self, target: &str, elements: bool, system: bool) -> Vec<VariableInfo> {
        let mut vars: Vec<&Var> = self.vars.iter().filter(|v| system || !v.system).collect();
        vars.sort_by(|a, b| a.name.cmp(&b.name));
        let mut out = Vec::new();
        for v in vars {
            let row = self.describe(target, v.name.clone(), &v.props, None);
            let code = get(&v.props, prop::TYPE).and_then(Prop::int);
            let ty = code.and_then(|c| self.types.iter().find(|t| t.key == c));
            let (source, scan) = (row.source.clone(), row.extra.get("ScanGroup").cloned());
            out.push(row);
            if let (true, Some(ty)) = (elements, ty) {
                for e in self.elements(v) {
                    let member = ty.members.iter().find(|m| m.0 == e.index);
                    let ename = format!("{}.{}", v.name, member.map(|m| m.1.clone()).unwrap_or_else(|| e.index.to_string()));
                    let hint = member.map(|m| self.data_type_name(Some(m.2)));
                    let mut row = self.describe(target, ename, &e.props, hint);
                    if row.source == "Internal" && source == "External" {
                        row.source = source.clone();
                        if let Some(s) = &scan {
                            row.extra.insert("ScanGroup".into(), s.clone());
                        }
                    }
                    out.push(row);
                }
            }
        }
        out
    }

    pub fn folder_paths(&self) -> Vec<String> {
        let mut v: Vec<String> = self.folders.iter().map(|f| f.path.clone()).collect();
        v.sort();
        v
    }
}

fn folder_path(folders: &[Folder], id: u32) -> String {
    let mut parts = Vec::new();
    let mut cur = id;
    for _ in 0..32 {
        let Some(f) = folders.iter().find(|f| f.id == cur) else { break };
        parts.push(f.name.clone());
        if f.parent == 0 {
            break;
        }
        cur = f.parent;
    }
    parts.reverse();
    parts.join(".")
}

/// Códigos de los tipos simples. Los de 6.2 (8OND007); los de 6.3 se comprueban contra el XML.
pub fn simple_type(code: u32) -> Option<&'static str> {
    Some(match code {
        1 => "BOOL",
        7 => "Integer",
        2 => "REAL",
        3 => "STRING",
        8 => "INT",
        9 => "UINT",
        10 => "DINT",
        11 => "UDINT",
        _ => return None,
    })
}

/// Tipos estructurados (4003, clase 5): `u32 5 | u32 caracteres | nombre | relleno | u32 clave | u32 siguiente índice |
/// u32 n | n × (u32 índice | u32 caracteres | nombre | u32 tipo | 8 bytes 0) | u32 n | n × índice | MARK`.
fn data_types(b: &[u8], limit: usize) -> Vec<DataType> {
    let mut out = Vec::new();
    let starts = find_all(&b[..limit.min(b.len())], &class_header(CLS_TYPE));
    for (k, &s) in starts.iter().enumerate() {
        let next = starts.get(k + 1).copied().unwrap_or(limit).min(b.len());
        let p = s + 8;
        let (Some(kind), Some(n)) = (u32_at(b, p), u32_at(b, p + 4)) else { continue };
        if !(kind == 5 || kind == 6) || !(1..64).contains(&n) {
            continue;
        }
        let Some(name) = b.get(p + 8..p + 8 + 2 * n as usize).and_then(utf16) else { continue };
        let mut q = p + 8 + 2 * n as usize;
        while b.get(q) == Some(&0) {
            q += 1;
        }
        let (Some(key), Some(_next), Some(count)) = (u32_at(b, q), u32_at(b, q + 4), u32_at(b, q + 8)) else { continue };
        let mut members = Vec::new();
        let mut m = q + 12;
        let mut ok = count < 1024;
        for _ in 0..count.min(1024) {
            let (Some(idx), Some(len)) = (u32_at(b, m), u32_at(b, m + 4)) else {
                ok = false;
                break;
            };
            let Some(mname) = b.get(m + 8..m + 8 + 2 * len as usize).and_then(utf16) else {
                ok = false;
                break;
            };
            let Some(code) = u32_at(b, m + 8 + 2 * len as usize) else {
                ok = false;
                break;
            };
            members.push((idx, mname, code));
            m += 20 + 2 * len as usize;
        }
        if ok && kind == 5 && u32_at(b, m) == Some(count) && b.get(m + 4 + 4 * count as usize..m + 8 + 4 * count as usize) == Some(&MARK[..]) {
            out.push(DataType { key, name, members, window: b[q..(q + 400).min(next).max(q)].to_vec() });
        } else if kind == 6 || !ok {
            // Tipos de sistema: sólo el nombre con su clave.
            out.push(DataType { key, name, members: Vec::new(), window: b[q..(q + 400).min(next).max(q)].to_vec() });
        }
    }
    out
}

/// Grupos de alarma (0x4D), registro (0x4E) y escaneo (0x4F) del bloque 4010: secciones `u32 id | 12 bytes 0`
/// seguidas de entradas `u32 id | u32 id de nombre | u32 caracteres | nombre`. Con varios targets, el `k`-ésimo par
/// 4010/4011.
fn groups(b: &[u8], k: usize) -> BTreeMap<&'static str, BTreeMap<u32, String>> {
    let mut out: BTreeMap<&'static str, BTreeMap<u32, String>> = [("alarm", BTreeMap::new()), ("logging", BTreeMap::new()), ("scan", BTreeMap::new())].into();
    let (Some(&g0), Some(&g1)) = (find_all(b, &class_header(CLS_GROUPS)).get(k), find_all(b, &class_header(CLS_FOLDERS)).get(k)) else { return out };
    if g1 <= g0 {
        return out;
    }
    let blk = &b[g0..g1];
    let mut section: Option<&'static str> = None;
    let mut p = 8;
    while p + 12 < blk.len() {
        let id = u32_at(blk, p).unwrap_or_default();
        let sec = match id {
            0x4D => Some("alarm"),
            0x4E => Some("logging"),
            0x4F => Some("scan"),
            _ => None,
        };
        if sec.is_some() && blk.get(p + 8..p + 20) == Some(&[0u8; 12][..]) {
            section = sec;
            p += 20;
            continue;
        }
        let n = u32_at(blk, p + 8).unwrap_or_default() as usize;
        if let Some(s) = section {
            if (1..64).contains(&n) && p + 12 + 2 * n <= blk.len() {
                if let Some(name) = utf16(&blk[p + 12..p + 12 + 2 * n]) {
                    if !name.is_empty() && name != "None" && name.chars().all(|c| !c.is_control()) {
                        out.get_mut(s).unwrap().insert(id, name);
                        p += 12 + 2 * n;
                        continue;
                    }
                }
            }
        }
        p += 1;
    }
    out
}

/// Para las escrituras: reemplaza `b[start..end]` y actualiza el campo de longitud.
fn splice(b: &mut Vec<u8>, start: usize, end: usize, new: &[u8]) {
    b.splice(start..end, new.iter().copied());
    let len = (b.len() - 9) as u32;
    put_u32(b, 4, len);
}

// ---------- escritura ----------

/// Ajustes de una variable al crearla o cambiarla. `None` = no se toca; texto vacío = se quita.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// `Internal` / `External`.
    pub source: Option<String>,
    pub scan_group: Option<String>,
    pub address: Option<String>,
    pub description: Option<String>,
    pub initial_value: Option<String>,
    pub logging_group: Option<String>,
    pub string_bytes: Option<u32>,
}

/// Alarma de una variable BOOL al crearla o cambiarla. `None` = no se toca (al crearla: activa, When High, severidad 1,
/// sin grupo).
#[derive(Debug, Clone, Default)]
pub struct AlarmSettings {
    pub enabled: Option<bool>,
    pub message: Option<String>,
    /// `when high` / `when low`.
    pub trigger: Option<String>,
    pub severity: Option<u32>,
    /// Grupo de alarmas; vacío o `None` = sin grupo.
    pub group: Option<String>,
}

/// Propiedades de la alarma de una variable.
const ALARM_PROPS: [u32; 6] = [prop::ALARM, prop::ALARM_MESSAGE, prop::ALARM_TRIGGER, prop::ALARM_SEVERITY, prop::ALARM_FLAG, prop::ALARM_GROUP];

/// Tipos que se pueden crear y su código.
pub const CREATABLE_TYPES: [(&str, u32); 8] = [("BOOL", 0), ("INT", 8), ("UINT", 9), ("DINT", 10), ("UDINT", 11), ("Integer", 7), ("REAL", 2), ("STRING", 3)];

/// Propiedades que dicen qué es la variable o cómo se configura: al tomar otra variable del mismo tipo como plantilla,
/// sólo se copian las demás (formato, límites por defecto…).
const SETTING_PROPS: [u32; 20] = [
    prop::TYPE,
    prop::NAME,
    prop::DESCRIPTION,
    prop::SHARING,
    prop::INITIAL,
    prop::ALARM,
    prop::ALARM_MESSAGE,
    prop::INPUT_RANGE,
    prop::MIN,
    prop::MAX,
    prop::ALARM_FLAG,
    prop::ALARM_TRIGGER,
    prop::ALARM_SEVERITY,
    prop::BYTES,
    prop::ADDRESS,
    prop::ALARM_GROUP,
    prop::LOGGING_GROUP,
    prop::SCAN_GROUP,
    prop::PARENT,
    prop::EXTERNAL,
];

impl TagDb {
    /// Streams `TagDatabase` y `NameServer` como quedan.
    pub fn into_streams(self) -> (Vec<u8>, Vec<u8>) {
        (self.b, self.ns)
    }

    fn reindex(&mut self) -> Result<(), String> {
        self.index().map_err(|e| format!("tras el cambio, {e}"))
    }

    pub fn find_var(&self, name: &str) -> Option<&Var> {
        self.vars.iter().find(|v| !v.system && v.name.eq_ignore_ascii_case(name))
    }

    pub fn find_folder(&self, path: &str) -> Option<&Folder> {
        self.folders.iter().find(|f| f.path.eq_ignore_ascii_case(path))
    }

    fn group_id(&self, kind: &str, name: &str) -> Result<u32, String> {
        let all = self.groups.get(kind).cloned().unwrap_or_default();
        all.iter()
            .find(|(_, n)| n.eq_ignore_ascii_case(name.trim()))
            .map(|(id, _)| *id)
            .ok_or_else(|| format!("no hay grupo de {} «{name}» (hay: {})", group_label(kind), all.values().cloned().collect::<Vec<_>>().join(", ")))
    }

    /// Nodo de NameServer con los nombres de primer nivel (variables y carpetas de la raíz, también las del sistema).
    fn root_container(&self) -> Result<usize, String> {
        self.vars
            .iter()
            .filter(|v| get(&v.props, prop::PARENT).is_none())
            .filter_map(|v| get(&v.props, prop::NAME).and_then(Prop::int))
            .chain(self.folders.iter().filter(|f| f.parent == 0).map(|f| f.name_id))
            .find_map(|id| self.names.node_with_entry(id))
            .ok_or_else(|| "no se encontró el nodo de nombres de la raíz en NameServer".into())
    }

    /// Nodo de NameServer donde van los nombres de lo que contiene la carpeta (`0` = raíz).
    fn container_of(&self, folder: u32) -> Result<usize, String> {
        if folder == 0 {
            return self.root_container();
        }
        let f = self.folders.iter().find(|f| f.id == folder).ok_or("carpeta inexistente")?;
        self.names.node_of_group(f.container).ok_or_else(|| format!("no está el nodo {} de la carpeta «{}» en NameServer", f.container, f.path))
    }

    /// ¿Hay ya algo llamado `short` (variable o carpeta) en la carpeta?
    fn taken(&self, node: usize, short: &str) -> bool {
        self.names.nodes[node].entries.iter().any(|e| e.1.eq_ignore_ascii_case(short))
    }

    fn new_name_id(&mut self) -> u32 {
        let at = self.names.counter_at;
        let id = u32_at(&self.ns, at).unwrap_or_default() + 1;
        put_u32(&mut self.ns, at, id);
        id
    }

    fn ns_add_entry(&mut self, node: usize, id: u32, name: &str) -> Result<(), String> {
        let (at, kids_at) = (self.names.nodes[node].at, self.names.nodes[node].kids_at);
        let mut text = utf16_bytes(name);
        text.extend([0, 0]);
        let mut entry = id.to_le_bytes().to_vec();
        entry.extend((text.len() as u32).to_le_bytes());
        entry.extend(text);
        self.ns.splice(kids_at..kids_at, entry);
        let n = u16_at(&self.ns, at + 8).unwrap_or_default() + 1;
        put_u16(&mut self.ns, at + 8, n);
        self.names = NameServer::parse(&self.ns).ok_or("NameServer quedó ilegible")?;
        Ok(())
    }

    fn ns_remove_entry(&mut self, id: u32) -> Result<(), String> {
        let node = self.names.node_with_entry(id).ok_or_else(|| format!("el nombre {id} no está en NameServer"))?;
        let at = self.names.nodes[node].at;
        let &(_, _, pos, size) = self.names.nodes[node].entries.iter().find(|e| e.0 == id).unwrap();
        self.ns.drain(pos..pos + size);
        let n = u16_at(&self.ns, at + 8).unwrap_or_default() - 1;
        put_u16(&mut self.ns, at + 8, n);
        self.names = NameServer::parse(&self.ns).ok_or("NameServer quedó ilegible")?;
        Ok(())
    }

    /// Nodo contenedor nuevo y vacío para una carpeta: hijo del mismo nodo que los de las demás carpetas (el del target),
    /// al final.
    fn ns_add_container(&mut self, group: u32) -> Result<(), String> {
        let parent = self
            .folders
            .iter()
            .find_map(|f| self.names.node_of_group(f.container))
            .and_then(|i| self.names.nodes[i].parent)
            .unwrap_or(0);
        let (end, kids_at) = (self.names.nodes[parent].end, self.names.nodes[parent].kids_at);
        let mut node = 1u16.to_le_bytes().to_vec();
        node.extend(group.to_le_bytes());
        node.extend([0u8; 6]); // tipo 0, 0 nombres, 0 hijos
        self.ns.splice(end..end, node);
        let kids = u16_at(&self.ns, kids_at).unwrap_or_default() + 1;
        put_u16(&mut self.ns, kids_at, kids);
        self.names = NameServer::parse(&self.ns).ok_or("NameServer quedó ilegible")?;
        Ok(())
    }

    fn ns_remove_container(&mut self, group: u32) -> Result<(), String> {
        let i = self.names.node_of_group(group).ok_or_else(|| format!("no está el nodo {group} en NameServer"))?;
        let n = &self.names.nodes[i];
        if !n.entries.is_empty() || n.end != n.kids_at + 2 {
            return Err(format!("el nodo {group} de NameServer no está vacío"));
        }
        let (at, end, parent) = (n.at, n.end, n.parent);
        self.ns.drain(at..end);
        if let Some(p) = parent {
            let kids_at = self.names.nodes[p].kids_at;
            let kids = u16_at(&self.ns, kids_at).unwrap_or_default() - 1;
            put_u16(&mut self.ns, kids_at, kids);
        }
        self.names = NameServer::parse(&self.ns).ok_or("NameServer quedó ilegible")?;
        Ok(())
    }

    /// Crea la carpeta `path` (`A.B`) y las que falten por encima. `false` si ya existía.
    pub fn add_folder(&mut self, path: &str) -> Result<bool, String> {
        let path = path.trim();
        if self.find_folder(path).is_some() {
            return Ok(false);
        }
        let (parent_path, short) = match path.rsplit_once('.') {
            Some((p, s)) => (Some(p), s),
            None => (None, path),
        };
        let parent = match parent_path {
            Some(p) => {
                self.add_folder(p)?;
                self.find_folder(p).map(|f| f.id).unwrap_or(0)
            }
            None => 0,
        };
        let node = self.container_of(parent)?;
        if self.taken(node, short) {
            return Err(format!("ya hay una variable «{path}»"));
        }
        let last = u32_at(&self.b, self.folder_header).unwrap_or_default();
        let id = self.folders.iter().map(|f| f.id).max().unwrap_or(0).max(last) + 1;
        let container = self.new_name_id();
        let name_id = self.new_name_id();
        let mut rec = id.to_le_bytes().to_vec();
        rec.extend(class_header(CLS_FOLDER));
        for v in [id, parent, container, name_id, 0, 1] {
            rec.extend(v.to_le_bytes());
        }
        rec.extend(MARK);
        let (at, header) = (self.folders_end, self.folder_header);
        splice(&mut self.b, at, at, &rec);
        put_u32(&mut self.b, header, id);
        let n = u32_at(&self.b, header + 4).unwrap_or_default() + 1;
        put_u32(&mut self.b, header + 4, n);
        self.ns_add_entry(node, name_id, short)?;
        self.ns_add_container(container)?;
        self.reindex()?;
        Ok(true)
    }

    /// Borra una carpeta vacía.
    pub fn delete_folder(&mut self, path: &str) -> Result<(), String> {
        let f = self.find_folder(path.trim()).cloned().ok_or_else(|| format!("no hay carpeta «{path}»"))?;
        let inside: Vec<String> = self
            .vars
            .iter()
            .filter(|v| get(&v.props, prop::PARENT).and_then(Prop::int) == Some(f.id))
            .map(|v| v.name.clone())
            .chain(self.folders.iter().filter(|s| s.parent == f.id).map(|s| s.path.clone()))
            .collect();
        if !inside.is_empty() {
            return Err(format!("la carpeta «{}» no está vacía: {}", f.path, inside.join(", ")));
        }
        splice(&mut self.b, f.at, f.at + 40, &[]);
        let header = self.folder_header;
        let n = u32_at(&self.b, header + 4).unwrap_or_default() - 1;
        put_u32(&mut self.b, header + 4, n);
        self.ns_remove_entry(f.name_id)?;
        self.ns_remove_container(f.container)?;
        self.reindex()
    }

    /// Propiedades base de una variable nueva: las no configurables (formato, límites por defecto…) de otra del mismo tipo,
    /// mejor de la misma fuente; si el proyecto no tiene ninguna de ese tipo, las que llevan siempre las de Vijeo.
    fn template(&self, code: u32, external: bool) -> Vec<Prop> {
        let ty = |v: &Var| get(&v.props, prop::TYPE).and_then(Prop::int).unwrap_or(0);
        let same: Vec<&Var> = self.vars.iter().filter(|v| !v.system && ty(v) == code && get(&v.props, prop::INDIRECT).is_none()).collect();
        let pick = same.iter().find(|v| get(&v.props, prop::SCAN_GROUP).is_some() == external).or(same.first());
        match pick {
            Some(v) => v.props.iter().filter(|p| !SETTING_PROPS.contains(&p.id) && p.id != prop::INDIRECT && p.id != prop::INDIRECT_ADDRESS).cloned().collect(),
            // UDINT e Integer llevan `0x30 = 1` (sin ella un UDINT da «[Variable] [X] [ScanGroup] invalid selection»).
            None if matches!(code, 7 | 11) => vec![Prop::u32_value(0x30, 0xf, 1)],
            None => Vec::new(),
        }
    }

    /// Crea una variable simple (`Carpeta.Nombre`; las carpetas que falten se crean).
    pub fn add_variable(&mut self, name: &str, data_type: &str, s: &Settings) -> Result<(), String> {
        let name = name.trim();
        let (ty_name, code) = CREATABLE_TYPES
            .iter()
            .find(|(t, _)| t.eq_ignore_ascii_case(data_type.trim()))
            .copied()
            .ok_or_else(|| format!("tipo «{data_type}» no admitido (hay: {})", CREATABLE_TYPES.map(|t| t.0).join(", ")))?;
        if self.find_var(name).is_some() || self.find_folder(name).is_some() {
            return Err(format!("ya existe «{name}»"));
        }
        let (folder_path, short) = match name.rsplit_once('.') {
            Some((p, s)) => (Some(p), s),
            None => (None, name),
        };
        let folder = match folder_path {
            Some(p) => {
                self.add_folder(p)?;
                self.find_folder(p).map(|f| f.id).unwrap_or(0)
            }
            None => 0,
        };
        let node = self.container_of(folder)?;
        if self.taken(node, short) {
            return Err(format!("ya existe «{name}»"));
        }
        let external = s.source.as_deref().is_some_and(|x| x.eq_ignore_ascii_case("External"));
        let mut props = self.template(code, external);
        let name_id = self.new_name_id();
        props.push(Prop::u32_value(prop::NAME, 0x10, name_id));
        if code != 0 {
            props.push(Prop::u32_value(prop::TYPE, 1, code));
        }
        if folder != 0 {
            props.push(Prop::u32_value(prop::PARENT, 0x15, folder));
        }
        let mut s = s.clone();
        s.source.get_or_insert_with(|| "Internal".into());
        if ty_name == "STRING" && s.string_bytes.is_none() {
            s.string_bytes = Some(20);
        }
        self.apply(&mut props, ty_name, &s)?;
        // Una numérica interna sin valor inicial no compila (Vijeo: «[Variable] [X] [ScanGroup] invalid selection», verificado
        // con un UDINT): las de Vijeo lo llevan siempre, aunque sea 0.
        let internal = get(&props, prop::SCAN_GROUP).is_none();
        if internal && !matches!(ty_name, "BOOL" | "STRING") && get(&props, prop::INITIAL).is_none() {
            props.push(initial(ty_name, "0")?);
        }
        let id = u32_at(&self.b, self.next_id_at).unwrap_or_default();
        let mut rec = id.to_le_bytes().to_vec();
        rec.extend(class_header(CLS_VAR));
        rec.extend(encode_props(&props));
        rec.extend([0u8; 8]); // sin dinámicas, sin hijos
        rec.extend(MARK);
        let at = self.list_end;
        splice(&mut self.b, at, at, &rec);
        let next_at = self.next_id_at + rec.len();
        put_u32(&mut self.b, next_at, id + 1);
        let count_at = self.count_at;
        let n = u32_at(&self.b, count_at).unwrap_or_default() + 1;
        put_u32(&mut self.b, count_at, n);
        self.ns_add_entry(node, name_id, short)?;
        self.reindex()
    }

    /// Cambia los ajustes de una variable o de un elemento de estructura (`Instancia.Miembro`: dirección, descripción,
    /// valor inicial).
    pub fn set_variable(&mut self, name: &str, s: &Settings) -> Result<(), String> {
        let name = name.trim();
        if let Some(v) = self.find_var(name).cloned() {
            if get(&v.props, prop::INDIRECT).is_some() {
                return Err(format!("«{}» es una referencia indirecta: no se edita sin Vijeo", v.name));
            }
            let ty = self.data_type_name(get(&v.props, prop::TYPE).and_then(Prop::int));
            let mut props = v.props.clone();
            self.apply(&mut props, &ty, s)?;
            splice(&mut self.b, v.at + 12, v.props_end, &encode_props(&props));
            return self.reindex();
        }
        let (inst, member) = name.rsplit_once('.').ok_or_else(|| format!("no hay ninguna variable «{name}»"))?;
        let v = self.find_var(inst).cloned().ok_or_else(|| format!("no hay ninguna variable «{name}»"))?;
        let ty = get(&v.props, prop::TYPE).and_then(Prop::int).and_then(|c| self.data_type(c)).cloned().ok_or_else(|| format!("no hay ninguna variable «{name}»"))?;
        let m = ty.members.iter().find(|m| m.1.eq_ignore_ascii_case(member)).cloned().ok_or_else(|| format!("«{inst}» ({}) no tiene el miembro «{member}»", ty.name))?;
        if s.source.is_some() || s.scan_group.is_some() || s.logging_group.is_some() || s.string_bytes.is_some() {
            return Err("en un elemento de estructura sólo se cambian address, description e initial_value (la fuente y el grupo de escaneo son de la instancia)".into());
        }
        let e = self.elements(&v).into_iter().find(|e| e.index == m.0).ok_or_else(|| format!("«{name}» no está en la instancia"))?;
        let mut props = e.props.clone();
        let ety = self.data_type_name(Some(m.2));
        if let Some(a) = &s.address {
            set_text(&mut props, prop::ADDRESS, a);
        }
        let only = Settings { description: s.description.clone(), initial_value: s.initial_value.clone(), ..Default::default() };
        self.apply(&mut props, &ety, &only)?;
        splice(&mut self.b, e.at + 12, e.props_end, &encode_props(&props));
        self.reindex()
    }

    /// Cambia el nombre de una variable de primer nivel dentro de su carpeta (`Carpeta.Viejo` → `Carpeta.<new_leaf>`): sólo
    /// su entrada de `NameServer`; el id no cambia. Devuelve el nombre completo nuevo.
    pub fn rename_variable(&mut self, name: &str, new_leaf: &str) -> Result<String, String> {
        let v = self.find_var(name.trim()).cloned().ok_or_else(|| format!("no hay ninguna variable «{name}» (los elementos de una estructura no se renombran)"))?;
        let new_leaf = new_leaf.trim();
        if new_leaf.contains('.') {
            return Err("new_name es sólo el nombre, sin carpetas: la variable se queda en su carpeta".into());
        }
        let prefix = v.name.rsplit_once('.').map(|(p, _)| format!("{p}.")).unwrap_or_default();
        let full = format!("{prefix}{new_leaf}");
        if full == v.name {
            return Err(format!("«{}» ya se llama así", v.name));
        }
        let taken = self.vars.iter().any(|o| o.id != v.id && o.name.eq_ignore_ascii_case(&full)) || self.find_folder(&full).is_some();
        if taken {
            return Err(format!("ya hay una variable o carpeta «{full}»"));
        }
        let name_id = get(&v.props, prop::NAME).and_then(Prop::int).ok_or("variable sin nombre")?;
        self.ns_set_entry(name_id, new_leaf)?;
        self.reindex()?;
        Ok(full)
    }

    /// Cambia el nombre de una carpeta dentro de su carpeta padre (`A.Vieja` → `A.<new_leaf>`): sólo su entrada de
    /// `NameServer`; el id no cambia y lo que contiene pasa a llamarse `A.<new_leaf>.…`. Devuelve la ruta nueva.
    pub fn rename_folder(&mut self, path: &str, new_leaf: &str) -> Result<String, String> {
        let f = self.find_folder(path.trim().trim_matches('.')).cloned().ok_or_else(|| format!("no hay ninguna carpeta «{}»", path.trim()))?;
        let new_leaf = new_leaf.trim();
        if new_leaf.contains('.') {
            return Err("new_name es sólo el nombre, sin carpetas: la carpeta se queda donde está".into());
        }
        let prefix = f.path.rsplit_once('.').map(|(p, _)| format!("{p}.")).unwrap_or_default();
        let full = format!("{prefix}{new_leaf}");
        if full == f.path {
            return Err(format!("«{}» ya se llama así", f.path));
        }
        let taken = self.folders.iter().any(|o| o.id != f.id && o.path.eq_ignore_ascii_case(&full)) || self.vars.iter().any(|v| v.name.eq_ignore_ascii_case(&full));
        if taken {
            return Err(format!("ya hay una variable o carpeta «{full}»"));
        }
        self.ns_set_entry(f.name_id, new_leaf)?;
        self.reindex()?;
        Ok(full)
    }

    /// Referencia indirecta (`Carpeta.%s.Var`) de otra variable a la que puede apuntar `name` (una variable o, con
    /// `folder`, cualquier cosa dentro de esa carpeta): (variable que la tiene, plantilla). Al renombrar, la plantilla,
    /// que es texto y no un enlace, dejaría de llevar a ella.
    pub fn indirect_hit(&self, name: &str, folder: bool) -> Option<(String, String)> {
        let lower = name.to_lowercase();
        let fits = |n: &str, parts: &[String]| match parts {
            [one] => n == one,
            [first, .., last] => n.starts_with(first.as_str()) && n.ends_with(last.as_str()) && n.len() >= first.len() + last.len(),
            [] => false,
        };
        let inside: Vec<String> = if folder {
            let pre = format!("{lower}.");
            self.vars.iter().map(|v| v.name.to_lowercase()).filter(|n| n.starts_with(&pre)).collect()
        } else {
            vec![lower.clone()]
        };
        self.vars.iter().find_map(|v| {
            let t = get(&v.props, prop::INDIRECT_ADDRESS).and_then(Prop::text)?;
            let parts: Vec<String> = t.to_lowercase().split("%s").map(str::to_owned).collect();
            // La parte fija del principio nombra la carpeta, o alguna variable de dentro (o la propia) encaja.
            let hit = (folder && parts[0].starts_with(&format!("{lower}."))) || inside.iter().any(|n| fits(n, &parts));
            hit.then(|| (v.name.clone(), t))
        })
    }

    /// Mueve una variable de primer nivel a la carpeta `to` (`""` = la raíz; las carpetas que falten se crean), con el
    /// mismo nombre y el mismo id: su carpeta padre (prop `0x51`) y su entrada de `NameServer`, que pasa al nodo de la
    /// carpeta nueva. Devuelve el nombre completo nuevo.
    pub fn move_variable(&mut self, name: &str, to: &str) -> Result<String, String> {
        let v = self.find_var(name.trim()).cloned().ok_or_else(|| format!("no hay ninguna variable «{}» (los elementos de una estructura no se mueven)", name.trim()))?;
        let to = to.trim().trim_matches('.');
        let leaf = v.name.rsplit('.').next().unwrap_or(&v.name).to_owned();
        let full = if to.is_empty() { leaf.clone() } else { format!("{to}.{leaf}") };
        let from = get(&v.props, prop::PARENT).and_then(Prop::int).unwrap_or(0);
        if !to.is_empty() && self.find_var(to).is_some() {
            return Err(format!("«{to}» es una variable, no una carpeta"));
        }
        if !to.is_empty() {
            self.add_folder(to)?;
        }
        let dest = if to.is_empty() { 0 } else { self.find_folder(to).map(|f| f.id).ok_or_else(|| format!("no se pudo crear la carpeta «{to}»"))? };
        if dest == from {
            return Err(format!("«{}» ya está en esa carpeta", v.name));
        }
        if self.taken(self.container_of(dest)?, &leaf) {
            return Err(format!("ya hay una variable o carpeta «{full}»"));
        }
        let name_id = get(&v.props, prop::NAME).and_then(Prop::int).ok_or("variable sin nombre")?;
        // Con la carpeta nueva creada, la variable puede haber cambiado de sitio en la base.
        let v = self.vars.iter().find(|x| x.id == v.id).cloned().ok_or("la variable desapareció al crear la carpeta")?;
        let mut props: Vec<Prop> = v.props.iter().filter(|p| p.id != prop::PARENT).cloned().collect();
        if dest != 0 {
            props.push(Prop::u32_value(prop::PARENT, 0x15, dest));
        }
        splice(&mut self.b, v.at + 12, v.props_end, &encode_props(&props));
        self.ns_remove_entry(name_id)?;
        let node = self.container_of(dest)?;
        self.ns_add_entry(node, name_id, &leaf)?;
        self.reindex()?;
        Ok(full)
    }

    /// Mueve una carpeta, con todo lo que contiene, dentro de la carpeta `to` (`""` = la raíz; las que falten se crean),
    /// con el mismo nombre y el mismo id: su padre (en su registro, +16) y su entrada de `NameServer`, que pasa al nodo de
    /// la carpeta nueva (el nodo con lo que contiene no cambia). Devuelve la ruta nueva.
    pub fn move_folder(&mut self, path: &str, to: &str) -> Result<String, String> {
        let f = self.find_folder(path.trim().trim_matches('.')).cloned().ok_or_else(|| format!("no hay ninguna carpeta «{}»", path.trim()))?;
        let to = to.trim().trim_matches('.');
        if to.eq_ignore_ascii_case(&f.path) || to.to_lowercase().starts_with(&format!("{}.", f.path.to_lowercase())) {
            return Err(format!("no se puede mover «{}» dentro de sí misma", f.path));
        }
        if !to.is_empty() && self.find_var(to).is_some() {
            return Err(format!("«{to}» es una variable, no una carpeta"));
        }
        if !to.is_empty() {
            self.add_folder(to)?;
        }
        let dest = if to.is_empty() { 0 } else { self.find_folder(to).map(|d| d.id).ok_or_else(|| format!("no se pudo crear la carpeta «{to}»"))? };
        if dest == f.parent {
            return Err(format!("«{}» ya está en esa carpeta", f.path));
        }
        let full = if to.is_empty() { f.name.clone() } else { format!("{to}.{}", f.name) };
        if self.taken(self.container_of(dest)?, &f.name) {
            return Err(format!("ya hay una variable o carpeta «{full}»"));
        }
        // Con la carpeta nueva creada, el registro puede haber cambiado de sitio.
        let at = self.folders.iter().find(|x| x.id == f.id).map(|x| x.at).ok_or("la carpeta desapareció al crear la otra")?;
        put_u32(&mut self.b, at + 16, dest);
        self.ns_remove_entry(f.name_id)?;
        let node = self.container_of(dest)?;
        self.ns_add_entry(node, f.name_id, &f.name)?;
        self.reindex()?;
        Ok(full)
    }

    /// La carpeta `folder` (`0` = raíz, ninguna) y las que la contienen, de la raíz hacia abajo, como (id, nombre): las de los
    /// enlaces que van delante del de algo que está en ella (`{F,2:1.1,MyController}.{F,2:1.4,System}.`).
    pub fn ancestors(&self, folder: u32) -> Vec<(u32, String)> {
        let mut out = Vec::new();
        let mut at = folder;
        while let Some(f) = self.folders.iter().find(|f| f.id == at) {
            out.push((f.id, f.name.clone()));
            at = f.parent;
            if out.len() > 64 {
                break;
            }
        }
        out.reverse();
        out
    }

    /// Cambia el texto de la entrada `id` de `NameServer` (en su sitio, con el mismo id).
    fn ns_set_entry(&mut self, id: u32, name: &str) -> Result<(), String> {
        let node = self.names.node_with_entry(id).ok_or_else(|| format!("el nombre {id} no está en NameServer"))?;
        let &(_, _, pos, size) = self.names.nodes[node].entries.iter().find(|e| e.0 == id).unwrap();
        let mut text = utf16_bytes(name);
        text.extend([0, 0]);
        let mut entry = id.to_le_bytes().to_vec();
        entry.extend((text.len() as u32).to_le_bytes());
        entry.extend(text);
        self.ns.splice(pos..pos + size, entry);
        self.names = NameServer::parse(&self.ns).ok_or("NameServer quedó ilegible")?;
        Ok(())
    }

    /// Borra una variable de primer nivel (con sus elementos, si es una instancia de estructura).
    /// Ids de los mensajes de alarma de una variable y de sus elementos (los textos que hay que quitar al borrarla).
    pub fn alarm_text_ids(&self, v: &Var) -> Vec<u32> {
        let ids = |props: &[Prop]| props.iter().filter(|p| p.id == prop::ALARM_MESSAGE).filter_map(Prop::int).collect::<Vec<_>>();
        let mut out = ids(&v.props);
        for e in self.elements(v) {
            out.extend(ids(&e.props));
        }
        out.retain(|&id| id != u32::MAX);
        out
    }

    /// ¿Hay otra variable (u otro elemento) del target, aparte de `except`, cuyo mensaje de alarma es el texto `id`?
    pub fn text_shared(&self, id: u32, except: u32) -> bool {
        self.vars.iter().filter(|v| v.id != except).any(|v| self.alarm_text_ids(v).contains(&id))
    }

    /// Crea o cambia la alarma de una variable BOOL (las de límites de los numéricos todavía no). El mensaje va a la tabla
    /// de textos del target (`lang`). Como Import Variables de Vijeo 6.3: 0x0A activa, 0x0B id del mensaje, 0x0C = 2
    /// When Low, 0x10 severidad (sin ella, 1), 0x26 = 1, 0x4D grupo (sin él, None). Devuelve qué se hizo.
    pub fn set_alarm(&mut self, name: &str, a: &AlarmSettings, lang: &mut crate::langdb::LangTable) -> Result<String, String> {
        let v = self.find_var(name.trim()).cloned().ok_or_else(|| format!("no hay ninguna variable «{}» (las alarmas de elementos de estructura no se cambian sin Vijeo)", name.trim()))?;
        let ty = self.data_type_name(get(&v.props, prop::TYPE).and_then(Prop::int));
        if ty != "BOOL" {
            return Err(format!("«{}» es {ty}: sin Vijeo sólo se definen alarmas de variables BOOL (las de límites de los numéricos todavía no)", v.name));
        }
        if get(&v.props, prop::INDIRECT).is_some_and(|p| p.int() != Some(0)) {
            return Err(format!("«{}» es una referencia indirecta: no se edita sin Vijeo", v.name));
        }
        let mut props = v.props.clone();
        let old = get(&props, prop::ALARM_MESSAGE).and_then(Prop::int).filter(|&id| id != u32::MAX && lang.has(id));
        let creating = old.is_none();
        let message = match &a.message {
            Some(m) => {
                let m = m.trim();
                if m.is_empty() || m.chars().count() > 160 || m.chars().any(char::is_control) {
                    return Err("el mensaje de alarma no puede estar vacío, tiene como mucho 160 caracteres y va en una línea".into());
                }
                Some(m.to_owned())
            }
            None if creating => return Err(format!("«{}» no tiene alarma: para crearla hace falta alarm.message", v.name)),
            None => None,
        };
        // Primero lo que puede fallar.
        let trigger = match a.trigger.as_deref().map(|t| t.trim().to_ascii_lowercase()) {
            None => None,
            Some(t) if t == "when high" || t == "high" => Some(false),
            Some(t) if t == "when low" || t == "low" => Some(true),
            Some(t) => return Err(format!("alarm.trigger «{t}»: when high o when low")),
        };
        if let Some(s) = a.severity {
            if !(1..=999).contains(&s) {
                return Err("alarm.severity: de 1 (máxima) a 999".into());
            }
        }
        let group = match a.group.as_deref().map(str::trim) {
            None => None,
            Some(g) if g.is_empty() || g.eq_ignore_ascii_case("None") => Some(None),
            Some(g) => Some(Some(self.group_id("alarm", g)?)),
        };
        let mut done = Vec::new();
        match (old, &message) {
            (Some(id), Some(m)) if lang.text(id) != Some(m.as_str()) => {
                if self.text_shared(id, v.id) {
                    let new = lang.add(m)?;
                    set(&mut props, Prop::u32_value(prop::ALARM_MESSAGE, 6, new));
                } else {
                    lang.set(id, m)?;
                }
                done.push(format!("mensaje «{m}»"));
            }
            (None, Some(m)) => {
                let id = lang.add(m)?;
                set(&mut props, Prop::u32_value(prop::ALARM_MESSAGE, 6, id));
                set(&mut props, Prop::byte_value(prop::ALARM_FLAG, 1));
                done.push(format!("mensaje «{m}»"));
            }
            _ => {}
        }
        if a.enabled.unwrap_or(creating) {
            if get(&props, prop::ALARM).and_then(Prop::int) != Some(1) {
                done.push("activada".into());
            }
            set(&mut props, Prop::byte_value(prop::ALARM, 1));
        } else if a.enabled == Some(false) {
            if get(&props, prop::ALARM).is_some() {
                done.push("desactivada (conserva mensaje, condición, grupo y severidad)".into());
            }
            props.retain(|p| p.id != prop::ALARM);
        }
        match trigger {
            Some(true) => {
                set(&mut props, Prop::u32_value(prop::ALARM_TRIGGER, 12, 2));
                done.push("condición When Low".into());
            }
            Some(false) => {
                props.retain(|p| p.id != prop::ALARM_TRIGGER);
                done.push("condición When High".into());
            }
            None => {}
        }
        match a.severity {
            Some(1) => props.retain(|p| p.id != prop::ALARM_SEVERITY),
            Some(s) => set(&mut props, Prop::u32_value(prop::ALARM_SEVERITY, 3, s)),
            None => {}
        }
        if let Some(s) = a.severity {
            done.push(format!("severidad {s}"));
        }
        match group {
            Some(Some(id)) => {
                set(&mut props, Prop::u32_value(prop::ALARM_GROUP, 0x14, id));
                done.push(format!("grupo {}", self.groups["alarm"][&id]));
            }
            Some(None) => {
                props.retain(|p| p.id != prop::ALARM_GROUP);
                done.push("sin grupo (None)".into());
            }
            None => {}
        }
        self.messages = lang.texts();
        splice(&mut self.b, v.at + 12, v.props_end, &encode_props(&props));
        self.reindex()?;
        Ok(if done.is_empty() { "sin cambios".into() } else { done.join(", ") })
    }

    /// Quita la alarma de una variable (y su mensaje de la tabla de textos, si nadie más lo usa).
    pub fn delete_alarm(&mut self, name: &str, lang: &mut crate::langdb::LangTable) -> Result<(), String> {
        let v = self.find_var(name.trim()).cloned().ok_or_else(|| format!("no hay ninguna variable «{}»", name.trim()))?;
        if get(&v.props, prop::ALARM_MESSAGE).is_none() && get(&v.props, prop::ALARM).is_none() {
            return Err(format!("«{}» no tiene alarma", v.name));
        }
        let ty = self.data_type_name(get(&v.props, prop::TYPE).and_then(Prop::int));
        if ty != "BOOL" {
            return Err(format!("«{}» es {ty}: sin Vijeo sólo se quitan alarmas de variables BOOL", v.name));
        }
        let text = get(&v.props, prop::ALARM_MESSAGE).and_then(Prop::int).filter(|&id| lang.has(id) && !self.text_shared(id, v.id));
        let mut props = v.props.clone();
        props.retain(|p| !ALARM_PROPS.contains(&p.id));
        if let Some(id) = text {
            lang.remove(id)?;
        }
        self.messages = lang.texts();
        splice(&mut self.b, v.at + 12, v.props_end, &encode_props(&props));
        self.reindex()
    }

    pub fn delete_variable(&mut self, name: &str) -> Result<(), String> {
        let v = self.find_var(name.trim()).cloned().ok_or_else(|| format!("no hay ninguna variable «{name}» (los elementos se borran con su instancia)"))?;
        let name_id = get(&v.props, prop::NAME).and_then(Prop::int).ok_or("variable sin nombre")?;
        splice(&mut self.b, v.at, v.end, &[]);
        let count_at = self.count_at;
        let n = u32_at(&self.b, count_at).unwrap_or_default() - 1;
        put_u32(&mut self.b, count_at, n);
        self.ns_remove_entry(name_id)?;
        self.reindex()
    }

    fn apply(&self, props: &mut Vec<Prop>, ty: &str, s: &Settings) -> Result<(), String> {
        if let Some(d) = &s.description {
            set_text(props, prop::DESCRIPTION, d);
        }
        let external = match s.source.as_deref().map(str::trim) {
            Some(x) if x.eq_ignore_ascii_case("External") => true,
            Some(x) if x.eq_ignore_ascii_case("Internal") => false,
            Some(x) => return Err(format!("source «{x}»: Internal o External")),
            None => get(props, prop::SCAN_GROUP).is_some(),
        };
        if external {
            if let Some(g) = &s.scan_group {
                let id = self.group_id("scan", g)?;
                set(props, Prop::u32_value(prop::SCAN_GROUP, 0x14, id));
            } else if get(props, prop::SCAN_GROUP).is_none() {
                let scan = self.groups.get("scan").cloned().unwrap_or_default();
                match scan.len() {
                    1 => set(props, Prop::u32_value(prop::SCAN_GROUP, 0x14, *scan.keys().next().unwrap())),
                    0 => return Err("el proyecto no tiene grupos de escaneo: crea uno en Vijeo (equipo de comunicación)".into()),
                    _ => return Err(format!("indica scan_group (hay: {})", scan.values().cloned().collect::<Vec<_>>().join(", "))),
                }
            }
            set(props, Prop::u32_value(prop::EXTERNAL, 7, 1));
            if let Some(a) = &s.address {
                set_text(props, prop::ADDRESS, a);
            }
            if get(props, prop::ADDRESS).is_none() {
                return Err("una variable External necesita address (p.ej. %MW10)".into());
            }
        } else {
            if s.scan_group.is_some() || s.address.is_some() {
                return Err("scan_group y address son de variables External (source: External)".into());
            }
            // Verificado compilando: un UDINT interno da «[Variable] [X] [ScanGroup] invalid selection» (1110); los demás
            // tipos internos compilan.
            if ty == "UDINT" {
                return Err("en Vijeo un UDINT sólo puede ser External (interno no compila: error 1110); usa DINT o UINT".into());
            }
            props.retain(|p| ![prop::SCAN_GROUP, prop::EXTERNAL, prop::ADDRESS].contains(&p.id));
        }
        if let Some(iv) = s.initial_value.as_deref().map(str::trim) {
            if iv.is_empty() {
                props.retain(|p| p.id != prop::INITIAL);
            } else if external {
                return Err("initial_value es de variables Internal".into());
            } else {
                set(props, initial(ty, iv)?);
            }
        }
        if let Some(g) = &s.logging_group {
            if g.trim().is_empty() {
                props.retain(|p| p.id != prop::LOGGING_GROUP);
            } else {
                let id = self.group_id("logging", g)?;
                set(props, Prop::u32_value(prop::LOGGING_GROUP, 0x14, id));
            }
        }
        if let Some(n) = s.string_bytes {
            if ty != "STRING" {
                return Err("string_bytes es de variables STRING".into());
            }
            if !(1..=1024).contains(&n) {
                return Err("string_bytes: de 1 a 1024".into());
            }
            set(props, Prop::u32_value(prop::BYTES, 3, n));
        }
        Ok(())
    }
}

fn group_label(kind: &str) -> &'static str {
    match kind {
        "scan" => "escaneo",
        "logging" => "registro",
        _ => "alarma",
    }
}

fn set(props: &mut Vec<Prop>, p: Prop) {
    props.retain(|x| x.id != p.id);
    props.push(p);
}

/// Texto: vacío = se quita.
fn set_text(props: &mut Vec<Prop>, id: u32, text: &str) {
    if text.is_empty() {
        props.retain(|x| x.id != id);
    } else {
        set(props, Prop::text_value(id, text));
    }
}

/// Valor inicial como lo guarda Vijeo: entero (tipo 3, también BOOL 0/1), double (REAL) o texto (STRING).
fn initial(ty: &str, v: &str) -> Result<Prop, String> {
    match ty {
        "STRING" => Ok(Prop::text_value(prop::INITIAL, v)),
        "REAL" => v.parse::<f64>().map(|d| Prop::double_value(prop::INITIAL, d)).map_err(|_| format!("initial_value «{v}» no es un número")),
        "BOOL" => match v.to_ascii_lowercase().as_str() {
            "1" | "true" | "enabled" | "on" => Ok(Prop::u32_value(prop::INITIAL, 3, 1)),
            "0" | "false" | "disabled" | "off" => Ok(Prop::u32_value(prop::INITIAL, 3, 0)),
            _ => Err(format!("initial_value «{v}» de un BOOL: 0/1, true/false")),
        },
        _ => {
            let n: i64 = v.parse().map_err(|_| format!("initial_value «{v}» no es un entero"))?;
            let (lo, hi) = match ty {
                "INT" => (i16::MIN as i64, i16::MAX as i64),
                "UINT" => (0, u16::MAX as i64),
                "UDINT" => (0, u32::MAX as i64),
                _ => (i32::MIN as i64, i32::MAX as i64),
            };
            if !(lo..=hi).contains(&n) {
                return Err(format!("initial_value {n} fuera de rango para {ty} ({lo}..{hi})"));
            }
            Ok(Prop::u32_value(prop::INITIAL, 3, n as u32))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODBUS: &str = r"C:\Program Files (x86)\Schneider Electric\EcoStruxure Machine Expert - Basic\Examples\Compact_Modbus SL_M221_Book.vdz";

    fn modbus() -> Option<TagDb> {
        let p = std::path::Path::new(MODBUS);
        if !p.is_file() {
            return None;
        }
        let mut c = crate::swx::open_vdz(p).unwrap();
        Some(TagDb::parse(crate::swx::stream(&mut c, "/Services/TagDatabase").unwrap(), crate::swx::stream(&mut c, "/Services/NameServer").unwrap()).unwrap())
    }

    fn find<'a>(all: &'a [VariableInfo], name: &str) -> &'a VariableInfo {
        all.iter().find(|v| v.name == name).unwrap_or_else(|| panic!("falta {name}"))
    }

    /// Las plantillas de las referencias indirectas (`MyController.%s.wComErrorCode` de `wRefComErrorCode`) frenan
    /// renombrar lo que pueden nombrar.
    #[test]
    fn finds_what_indirect_references_can_reach() {
        let Some(db) = modbus() else { return };
        let target = db.vars.iter().find(|v| v.name.ends_with(".wComErrorCode")).unwrap().name.clone();
        let folder = target.rsplit_once('.').unwrap().0.to_owned();
        assert_eq!(db.indirect_hit(&target, false).map(|h| h.0).as_deref(), Some("wRefComErrorCode"));
        assert!(db.indirect_hit(&folder, true).is_some(), "{folder}");
        assert!(db.indirect_hit("MyController", true).is_some(), "la parte fija la nombra");
        assert!(db.indirect_hit("MyController.Energy", true).is_none());
        assert!(db.indirect_hit("MyController.Energy.reTotalEnergy", false).is_none());
    }

    /// Lectura del ejemplo Modbus de la instalación (sin Vijeo): coincide con lo que exporta Vijeo en el XML
    /// (comprobado variable por variable con `compare_with_exported_xml`).
    #[test]
    fn reads_the_modbus_example() {
        let Some(db) = modbus() else { return };
        let all = db.variables("HMI", true, false);
        assert_eq!(all.len(), 183);
        let e = find(&all, "MyController.Energy.reTotalEnergy");
        assert_eq!((e.data_type.as_str(), e.source.as_str(), e.address.as_deref()), ("REAL", "External", Some("%MF16")));
        assert_eq!(e.extra.get("ScanGroup").map(String::as_str), Some("M221_Controller"));
        let s = find(&all, "sRefStringForComErrCodes");
        assert_eq!((s.data_type.as_str(), s.extra.get("InitialValue").map(String::as_str), s.extra.get("NoOfBytes").map(String::as_str)), ("STRING", Some("ATV_Motor_67"), Some("12")));
        assert_eq!(find(&all, "diNumberofActAlarms").data_type, "Integer");
        assert_eq!(find(&all, "wRefComErrorCode").extra.get("Reference").map(String::as_str), Some("MyController.%s.wComErrorCode"));
        assert!(!all.iter().any(|v| v.name.starts_with('_')), "sin las del sistema");
        assert_eq!(db.folders.len(), 15);
    }

    /// Carpetas y variables nuevas, cambios y borrados en memoria: se releen como se pidieron y el resto no cambia.
    #[test]
    fn edits_round_trip() {
        let Some(mut db) = modbus() else { return };
        let before = db.variables("HMI", true, false);
        let ext = |a: &str| Settings { source: Some("External".into()), address: Some(a.into()), ..Default::default() };
        assert!(db.add_folder("Mcp.Sub").unwrap());
        assert!(!db.add_folder("Mcp").unwrap());
        db.add_variable("Mcp.Contador", "INT", &Settings { initial_value: Some("-5".into()), description: Some("de prueba".into()), ..Default::default() }).unwrap();
        db.add_variable("Mcp.Sub.Marcha", "BOOL", &ext("%M300")).unwrap();
        db.add_variable("Mcp.Nivel", "REAL", &Settings { initial_value: Some("1.5".into()), ..Default::default() }).unwrap();
        db.add_variable("Mcp.Texto", "STRING", &Settings { initial_value: Some("hola".into()), ..Default::default() }).unwrap();
        db.add_variable("McpRaiz", "DINT", &Settings::default()).unwrap();
        assert!(db.add_variable("Mcp.Udint", "UDINT", &Settings::default()).unwrap_err().contains("External"));
        db.add_variable("MyController.Energy.iNueva", "UINT", &ext("%MW400")).unwrap();
        db.set_variable("MyController.Energy.iTimeSec", &Settings { address: Some("%MW99".into()), description: Some("segundos".into()), ..Default::default() }).unwrap();
        db.set_variable("sRefStringForComErrCodes", &Settings { string_bytes: Some(30), ..Default::default() }).unwrap();
        for (name, ty, s) in [
            ("Mcp.Contador", "INT", Settings::default()),
            ("Mcp.Malo", "WORD", Settings::default()),
            ("Mcp.Ext", "INT", Settings { source: Some("External".into()), ..Default::default() }),
            ("Mcp.Ini", "INT", Settings { initial_value: Some("70000".into()), ..Default::default() }),
            ("Mcp.Grupo", "INT", Settings { source: Some("External".into()), address: Some("%MW1".into()), scan_group: Some("NoExiste".into()), ..Default::default() }),
        ] {
            assert!(db.add_variable(name, ty, &s).is_err(), "{name}");
        }
        db.delete_variable("xRTCSyncDone").unwrap();
        assert!(db.delete_folder("Mcp").is_err(), "no está vacía");
        db.add_folder("Vacia").unwrap();
        db.delete_folder("Vacia").unwrap();
        // Releer desde los bytes, como al abrir el .vdz guardado.
        let (b, ns) = db.into_streams();
        let db = TagDb::parse(b, ns).unwrap();
        let after = db.variables("HMI", true, false);
        let c = find(&after, "Mcp.Contador");
        assert_eq!((c.data_type.as_str(), c.source.as_str(), c.description.as_deref(), c.extra.get("InitialValue").map(String::as_str)), ("INT", "Internal", Some("de prueba"), Some("-5")));
        let m = find(&after, "Mcp.Sub.Marcha");
        assert_eq!((m.data_type.as_str(), m.source.as_str(), m.address.as_deref(), m.extra.get("ScanGroup").map(String::as_str)), ("BOOL", "External", Some("%M300"), Some("M221_Controller")));
        assert_eq!(find(&after, "Mcp.Nivel").extra.get("InitialValue").map(String::as_str), Some("1.5"));
        assert_eq!(find(&after, "Mcp.Texto").extra.get("NoOfBytes").map(String::as_str), Some("20"));
        assert_eq!(find(&after, "McpRaiz").data_type, "DINT");
        assert_eq!(find(&after, "MyController.Energy.iNueva").address.as_deref(), Some("%MW400"));
        let t = find(&after, "MyController.Energy.iTimeSec");
        assert_eq!((t.address.as_deref(), t.description.as_deref()), (Some("%MW99"), Some("segundos")));
        assert_eq!(find(&after, "sRefStringForComErrCodes").extra.get("NoOfBytes").map(String::as_str), Some("30"));
        assert!(!after.iter().any(|v| v.name == "xRTCSyncDone"));
        assert!(db.find_folder("Mcp.Sub").is_some() && db.find_folder("Vacia").is_none());
        // El resto, igual.
        let touched = ["MyController.Energy.iTimeSec", "sRefStringForComErrCodes", "xRTCSyncDone"];
        for v in before.iter().filter(|v| !touched.contains(&v.name.as_str())) {
            let a = find(&after, &v.name);
            assert_eq!((&a.data_type, &a.source, &a.address, &a.description, &a.extra), (&v.data_type, &v.source, &v.address, &v.description, &v.extra), "{}", v.name);
        }
        assert_eq!(after.len(), before.len() + 6 - 1);
    }

    /// Lee el `.vdz` de `TAGDB_VDZ` y compara sus variables con las del XML exportado por Vijeo (`TAGDB_XML`).
    #[test]
    #[ignore]
    fn compare_with_exported_xml() {
        let vdz = std::path::PathBuf::from(std::env::var("TAGDB_VDZ").unwrap());
        let xml = std::fs::read_to_string(std::env::var("TAGDB_XML").unwrap()).unwrap();
        let mut c = crate::swx::open_vdz(&vdz).unwrap();
        let db = TagDb::parse(crate::swx::stream(&mut c, "/Services/TagDatabase").unwrap(), crate::swx::stream(&mut c, "/Services/NameServer").unwrap()).unwrap();
        eprintln!("carpetas {:?}", db.folder_paths());
        eprintln!("tipos {:?}", db.types.iter().map(|t| format!("{}={} ({} miembros)", t.key, t.name, t.members.len())).collect::<Vec<_>>());
        eprintln!("grupos {:?}", db.groups);
        let doc = crate::vxml::parse(&xml).unwrap();
        let t = crate::vxml::targets(&doc)[0];
        let want = crate::vxml::variables(t);
        let got = db.variables("", false, false);
        eprintln!("XML {} variables, TagDatabase {}", want.len(), got.len());
        let mut types: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        let mut diffs = 0;
        for w in &want {
            let Some(g) = got.iter().find(|g| g.name == w.name) else {
                eprintln!("FALTA {}", w.name);
                diffs += 1;
                continue;
            };
            let code = db.vars.iter().find(|v| v.name == w.name).and_then(|v| get(&v.props, prop::TYPE)).map(|p| p.display()).unwrap_or("-".into());
            *types.entry(w.data_type.clone()).or_default().entry(format!("{} (código {code})", g.data_type)).or_default() += 1;
            let mut d = Vec::new();
            if g.source != w.source {
                d.push(format!("source {} ≠ {}", g.source, w.source));
            }
            if g.address != w.address {
                d.push(format!("address {:?} ≠ {:?}", g.address, w.address));
            }
            if g.description != w.description {
                d.push(format!("description {:?} ≠ {:?}", g.description, w.description));
            }
            for k in ["ScanGroup", "InitialValue", "NoOfBytes"] {
                if g.extra.get(k) != w.extra.get(k) {
                    d.push(format!("{k} {:?} ≠ {:?}", g.extra.get(k), w.extra.get(k)));
                }
            }
            if !d.is_empty() {
                diffs += 1;
                if diffs < 40 {
                    eprintln!("{}: {}", w.name, d.join("; "));
                }
            }
        }
        for g in &got {
            if !want.iter().any(|w| w.name == g.name) {
                eprintln!("SOBRA {}", g.name);
            }
        }
        eprintln!("tipos XML → TagDatabase: {types:#?}");
        eprintln!("diferencias: {diffs}");
    }


    /// Los cuatro ejemplos de Machine Expert Basic contra su XML (corpus de `docs/`): mismas variables y el mismo `extra`
    /// campo por campo, salvo `AlarmGroup` (sólo lo da la lectura offline) y el formato que Vijeo exporta por defecto en
    /// las externas de tipo `Integer` (no está en la base). Sin la instalación o el corpus, no hace nada.
    #[test]
    fn extra_matches_the_exported_xml() {
        let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/vd-6.3/corpus/xml");
        let examples = std::path::Path::new(MODBUS).parent().unwrap();
        let integer_format = ["BitWidth", "DataFormat", "DataLength", "OffsetBitNo", "Signed"];
        let mine = corpus.parent().unwrap().to_owned();
        let mut checked_alarms = 0;
        for (v, x) in [
            ("Compact_Hardwired_M221", "Compact_Hardwired_M221"),
            ("Compact_Modbus SL_M221_Book", "Compact_Modbus SL_M221 Book"),
            ("MaterialWork_Drill_HW_M221", "Material Working_Drill machine_H"),
            ("Packaging_TempCtrl_HW_M221", "Packaging_TempCtrl_HW_M221"),
            ("McpAlarmas", "McpAlarmas"),
        ] {
            let dir = if v.starts_with("Mcp") { &mine } else { examples };
            let (vdz, xml) = (dir.join(format!("{v}.vdz")), corpus.join(format!("{x}.vxml")));
            if !vdz.is_file() || !xml.is_file() {
                continue;
            }
            let mut c = crate::swx::open_vdz(&vdz).unwrap();
            let mut db = TagDb::parse(crate::swx::stream(&mut c, "/Services/TagDatabase").unwrap(), crate::swx::stream(&mut c, "/Services/NameServer").unwrap()).unwrap();
            db.messages = crate::langdb::load(&mut c, 0).unwrap().1.unwrap().texts();
            let text = crate::reports::decode_text(&std::fs::read(&xml).unwrap());
            let doc = crate::vxml::parse(&text).unwrap();
            let want = crate::vxml::variables(crate::vxml::targets(&doc)[0]);
            let got = db.variables("", false, false);
            assert_eq!(want.len(), got.len(), "{v}");
            for w in &want {
                let g = got.iter().find(|g| g.name == w.name).unwrap_or_else(|| panic!("{v}: falta {}", w.name));
                for k in w.extra.keys().chain(g.extra.keys()) {
                    let skip = w.data_type == "Integer" && w.source == "External" && integer_format.contains(&k.as_str());
                    assert!(skip || w.extra.get(k) == g.extra.get(k), "{v} {} {k}: XML {:?} offline {:?}", w.name, w.extra.get(k), g.extra.get(k));
                }
                checked_alarms += w.extra.contains_key("AlarmMessage") as usize;
            }
        }
        if examples.is_dir() {
            assert!(checked_alarms > 50, "{checked_alarms} alarmas comparadas");
        }
    }

    /// Proyecto hecho con Import Variables (`docs/`): AlHigh, AlLow (When Low), AlSev5 (severidad 5), AlLow999 y
    /// SinAlarma, todas internas y sin grupo.
    fn alarm_project() -> Option<(TagDb, crate::langdb::LangTable)> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/vd-6.3/corpus/McpAlarmas.vdz");
        if !p.is_file() {
            return None;
        }
        let mut c = crate::swx::open_vdz(&p).unwrap();
        let mut db = TagDb::parse(crate::swx::stream(&mut c, "/Services/TagDatabase").unwrap(), crate::swx::stream(&mut c, "/Services/NameServer").unwrap()).unwrap();
        let lang = crate::langdb::load(&mut c, 0).unwrap().1.unwrap();
        db.messages = lang.texts();
        Some((db, lang))
    }

    fn reread(db: TagDb, lang: &crate::langdb::LangTable) -> TagDb {
        let (b, ns) = db.into_streams();
        let mut again = TagDb::parse(b, ns).unwrap();
        again.messages = crate::langdb::LangTable::parse(lang.b.clone()).unwrap().texts();
        again
    }

    /// Crear una alarma como la deja Import Variables: mismas propiedades que AlLow999 y el mensaje en la tabla.
    #[test]
    fn creates_alarms_like_import_variables() {
        let Some((mut db, mut lang)) = alarm_project() else { return };
        let want: Vec<Prop> = db.find_var("AlLow999").unwrap().props.iter().filter(|p| p.id != prop::NAME && p.id != prop::ALARM_MESSAGE).cloned().collect();
        let a = AlarmSettings { message: Some("Nueva por bajo".into()), trigger: Some("when low".into()), severity: Some(999), ..Default::default() };
        db.set_alarm("SinAlarma", &a, &mut lang).unwrap();
        let got = db.find_var("SinAlarma").unwrap().props.clone();
        assert_eq!(got.iter().filter(|p| p.id != prop::NAME && p.id != prop::ALARM_MESSAGE).cloned().collect::<Vec<_>>(), want);
        let id = get(&got, prop::ALARM_MESSAGE).and_then(Prop::int).unwrap();
        assert_eq!(id, 20, "el siguiente id de texto del proyecto");
        let db = reread(db, &lang);
        let all = db.variables("", false, false);
        let v = find(&all, "SinAlarma");
        assert_eq!(v.extra.get("AlarmMessage").map(String::as_str), Some("Nueva por bajo"));
        assert_eq!(v.extra.get("AlarmTriggerCondition").map(String::as_str), Some("When Low"));
        assert_eq!(v.extra.get("AlarmSeverity").map(String::as_str), Some("999"));
        assert_eq!(v.extra.get("AlarmGroup").map(String::as_str), Some("None"));
        assert_eq!(v.extra.get("Alarm").map(String::as_str), Some("Enabled"));
    }

    /// Cambiar, desactivar y quitar; con un grupo que no existe, error sin tocar nada.
    #[test]
    fn changes_disables_and_deletes_alarms() {
        let Some((mut db, mut lang)) = alarm_project() else { return };
        let original = db.b.clone();
        let texts = lang.entries.len();
        assert!(db.set_alarm("AlHigh", &AlarmSettings { group: Some("NoExiste".into()), ..Default::default() }, &mut lang).unwrap_err().contains("AlarmGroup1"));
        assert!(db.set_alarm("SinAlarma", &AlarmSettings::default(), &mut lang).unwrap_err().contains("message"));
        assert_eq!(db.b, original);
        let a = AlarmSettings { message: Some("Cambiado".into()), trigger: Some("when low".into()), severity: Some(1), group: Some("AlarmGroup1".into()), ..Default::default() };
        db.set_alarm("AlSev5", &a, &mut lang).unwrap();
        db.set_alarm("AlLow", &AlarmSettings { enabled: Some(false), ..Default::default() }, &mut lang).unwrap();
        db.delete_alarm("AlHigh", &mut lang).unwrap();
        assert_eq!(lang.entries.len(), texts - 1);
        assert!(db.delete_alarm("AlHigh", &mut lang).is_err());
        let db = reread(db, &lang);
        let all = db.variables("", false, false);
        let s5 = &find(&all, "AlSev5").extra;
        assert_eq!((s5["AlarmMessage"].as_str(), s5["AlarmTriggerCondition"].as_str(), s5["AlarmSeverity"].as_str(), s5["AlarmGroup"].as_str()), ("Cambiado", "When Low", "1", "AlarmGroup1"));
        let low = &find(&all, "AlLow").extra;
        assert_eq!((low["Alarm"].as_str(), low["AlarmMessage"].as_str()), ("Disabled", "Alarma por bajo"));
        assert!(!low.contains_key("AlarmTriggerCondition"));
        assert!(!find(&all, "AlHigh").extra.keys().any(|k| k.starts_with("Alarm")));
        // Volver a activarla deja la condición que tenía.
        let mut db = db;
        db.set_alarm("AlLow", &AlarmSettings { enabled: Some(true), ..Default::default() }, &mut lang).unwrap();
        assert_eq!(find(&db.variables("", false, false), "AlLow").extra["AlarmTriggerCondition"], "When Low");
    }

    /// Sin Vijeo sólo alarmas de BOOL.
    #[test]
    fn refuses_alarms_of_numeric_variables() {
        let Some((mut db, mut lang)) = alarm_project() else { return };
        db.add_variable("Numero", "INT", &Settings::default()).unwrap();
        assert!(db.set_alarm("Numero", &AlarmSettings { message: Some("x".into()), ..Default::default() }, &mut lang).unwrap_err().contains("BOOL"));
    }

    /// Proyecto de prueba con dos targets (Primero y Segundo, hecho con Vijeo): `Comun` en los dos (INT y DINT) y una
    /// carpeta propia en cada uno. Está en `docs/` (local); sin él, las pruebas no hacen nada.
    fn two_targets() -> Option<TagDb> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/vd-6.3/corpus/McpDosTargets.vdz");
        if !p.is_file() {
            return None;
        }
        let mut c = crate::swx::open_vdz(&p).unwrap();
        Some(TagDb::parse(crate::swx::stream(&mut c, "/Services/TagDatabase").unwrap(), crate::swx::stream(&mut c, "/Services/NameServer").unwrap()).unwrap())
    }

    fn summary(db: &TagDb, target: &str) -> Vec<(String, String)> {
        db.variables(target, true, false).into_iter().map(|v| (v.name, v.data_type)).collect()
    }

    #[test]
    fn reads_each_target_of_a_two_target_project() {
        let Some(mut db) = two_targets() else { return };
        assert_eq!(db.targets, 2);
        let pairs = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert_eq!(summary(&db, "Primero"), pairs(&[("CarpUno.UnoDentro", "BOOL"), ("Comun", "INT"), ("UnoRaiz", "INT")]));
        assert_eq!(db.folder_paths(), ["CarpUno"]);
        assert!(db.variables("Primero", false, true).iter().any(|v| v.name == "_CurPanelID"));
        db.select(1).unwrap();
        assert_eq!(summary(&db, "Segundo"), pairs(&[("CarpDos.DosDentro", "BOOL"), ("Comun", "DINT"), ("DosRaiz", "REAL")]));
        assert_eq!(db.folder_paths(), ["CarpDos"]);
        assert!(db.select(2).is_err());
    }

    /// Cambios en un target: se releen en él y el otro queda igual.
    #[test]
    fn edits_one_target_and_leaves_the_other() {
        for k in 0..2 {
            let Some(mut db) = two_targets() else { return };
            let names = ["Primero", "Segundo"];
            db.select(1 - k).unwrap();
            let other = summary(&db, names[1 - k]);
            let other_groups = db.groups.clone();
            db.select(k).unwrap();
            let groups = db.groups.clone();
            db.add_folder("Nueva.Sub").unwrap();
            db.add_variable("Nueva.Sub.Marcha", "BOOL", &Settings::default()).unwrap();
            db.add_variable("Comun2", "REAL", &Settings { initial_value: Some("2.5".into()), ..Default::default() }).unwrap();
            db.set_variable("Comun", &Settings { description: Some("cambiada".into()), ..Default::default() }).unwrap();
            let (b, ns) = db.into_streams();
            let mut again = TagDb::parse(b, ns).unwrap();
            again.select(k).unwrap();
            let all = again.variables(names[k], true, false);
            assert!(all.iter().any(|v| v.name == "Nueva.Sub.Marcha" && v.data_type == "BOOL"), "{all:?}");
            assert!(all.iter().any(|v| v.name == "Comun2" && v.data_type == "REAL"));
            assert_eq!(find(&all, "Comun").description.as_deref(), Some("cambiada"));
            assert_eq!(again.groups, groups);
            again.delete_variable("Nueva.Sub.Marcha").unwrap();
            again.delete_folder("Nueva.Sub").unwrap();
            again.delete_folder("Nueva").unwrap();
            let (b, ns) = again.into_streams();
            let mut again = TagDb::parse(b, ns).unwrap();
            again.select(1 - k).unwrap();
            assert_eq!(summary(&again, names[1 - k]), other);
            assert_eq!(again.groups, other_groups);
            assert!(again.variables(names[1 - k], true, false).iter().all(|v| v.description.as_deref() != Some("cambiada")));
        }
    }
}
