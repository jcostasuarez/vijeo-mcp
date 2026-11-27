// Prototipo: lee, edita y genera el portapapeles de objetos de Vijeo-Designer 6.3
// (63INDE_VIJEO_ObjectClipboard). Formato en docs/vijeo-designer-6.3-api-inventory.md §13.2.
//
//   vdobj dump <fichero>                 árbol de objetos (clase, nombre, rectángulo, tamaños)
//   vdobj scan <fichero>...              una línea por fichero (ok/!!, objetos, clases)
//   vdobj roundtrip <fichero>...         reconstruye sin cambios y compara byte a byte
//   vdobj build <salida.cfb> <fichero>:<objeto|*> [op...] [<fichero>:<objeto> [op...]]...
//       op: sel=NOMBRE (las siguientes ops van a ese objeto, también dentro de grupos)
//           rename=NUEVO  text=VIEJO=>NUEVO  subst=VIEJO=>NUEVO  move=dx,dy  at=x,y  rect=l,t,r,b
//           tags=<fichero> (DetachedTagSet)  anchor=x,y
//
// <fichero>: compound file del portapapeles (volcado con tools/clip_ole_dump.ps1), una pieza de la
// Toolchest (TCParts.tcp/PartNNN, extraída del compound file) o el stream ObjectData suelto.
// Opcional: un CSV stream,real,name,dll con la tabla completa de GUID (sin él, tabla fija de classes.rs).
mod classes;

use std::collections::HashMap;
use std::io::Read;

const END_MARK: u32 = 0x1CED50DA;

fn rd_u32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}
fn rd_i16(b: &[u8], o: usize) -> Option<i16> {
    b.get(o..o + 2).map(|s| i16::from_le_bytes(s.try_into().unwrap()))
}

pub fn guid_str(g: &[u8]) -> String {
    let d1 = u32::from_le_bytes(g[0..4].try_into().unwrap());
    let d2 = u16::from_le_bytes(g[4..6].try_into().unwrap());
    let d3 = u16::from_le_bytes(g[6..8].try_into().unwrap());
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        d1, d2, d3, g[8], g[9], g[10], g[11], g[12], g[13], g[14], g[15]
    )
}

#[derive(Debug, Clone)]
pub struct Obj {
    pub off: usize,
    pub guid: String,
    pub class: String,
    pub name: String,
    pub rect: [i16; 4],
    pub flags: u8,
    pub ext: (usize, usize),
    pub base_end: usize,
    /// Fin conocido del objeto (hijos de grupo: por el tamaño; nivel superior: siguiente cabecera).
    pub end: usize,
    pub group: Option<Group>,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub version: u32,
    pub children: Vec<(usize, Obj)>, // (offset del campo tamaño, hijo)
    pub list_end: usize,             // tras el terminador 0
}

pub struct Ctx {
    /// GUID admitidos al buscar objetos de nivel superior (vacío: cualquiera con cabecera válida).
    pub classes: HashMap<String, (String, String)>,
    /// Nombres para mostrar.
    pub names: HashMap<String, (String, String)>,
}

impl Ctx {
    fn known(&self, g: &[u8]) -> bool {
        self.classes.contains_key(&guid_str(g))
    }
    fn class_name(&self, g: &[u8]) -> String {
        self.names.get(&guid_str(g)).map(|(n, d)| format!("{n} [{d}]")).unwrap_or_else(|| "?".into())
    }

    /// Cabecera común (BaseGraphicalObject::Save) en `o` (donde empieza el GUID).
    pub fn header(&self, b: &[u8], o: usize) -> Option<Obj> {
        let g = b.get(o..o + 16)?;
        if *b.get(o + 16)? != 3 {
            return None;
        }
        let mut p = o + 17 + 4;
        let mut rect = [0i16; 4];
        for r in rect.iter_mut() {
            *r = rd_i16(b, p)?;
            p += 2;
        }
        let n = rd_u32(b, p)? as usize;
        if n < 2 || n % 2 != 0 || n > 512 {
            return None;
        }
        p += 4;
        let s = b.get(p..p + n)?;
        if s[n - 2] != 0 || s[n - 1] != 0 {
            return None;
        }
        let u: Vec<u16> = s[..n - 2].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        if u.iter().any(|&c| c == 0 || c < 0x20) {
            return None;
        }
        let name = String::from_utf16(&u).ok()?;
        p += n;
        let flags = *b.get(p)?;
        p += 1;
        let ext_start = p;
        // Sin extensión la marca va justo detrás; con extensión se busca la marca.
        let mark = if flags & 1 == 0 {
            if rd_u32(b, p)? != END_MARK {
                return None;
            }
            p
        } else {
            // Extensión (IPersistStream): empieza por u32 3 y es corta; termina en la marca.
            if rd_u32(b, p)? != 3 {
                return None;
            }
            let mut q = p + 4;
            loop {
                if rd_u32(b, q)? == END_MARK {
                    break q;
                }
                q += 1;
                if q > p + 0x10000 {
                    return None;
                }
            }
        };
        Some(Obj {
            off: o,
            guid: guid_str(g),
            class: self.class_name(g),
            name,
            rect,
            flags,
            ext: (ext_start, mark),
            base_end: mark + 4,
            end: 0,
            group: None,
        })
    }

    /// ¿Hay en `p` una lista de hijos de grupo (versión u32 + [tamaño, 16 bytes, GUID...])?
    fn group_at(&self, b: &[u8], p: usize, limit: usize, depth: usize) -> Option<Group> {
        let version = rd_u32(b, p)?;
        if version == 0 || version > 32 {
            return None;
        }
        let mut q = p + 4;
        let mut children = Vec::new();
        loop {
            let size = rd_u32(b, q)? as usize;
            if size == 0 {
                q += 4;
                break;
            }
            let start = q + 4 + 16;
            let end = start.checked_add(size)?;
            if end > limit || b.get(start..start + 16).is_none() {
                return None;
            }
            let mut child = self.header(b, start)?;
            child.end = end;
            self.fill(b, &mut child, end, depth + 1);
            children.push((q, child));
            q = end;
        }
        if children.is_empty() {
            return None;
        }
        Some(Group { version, children, list_end: q })
    }

    fn fill(&self, b: &[u8], o: &mut Obj, limit: usize, depth: usize) {
        if depth > 32 {
            return;
        }
        o.group = self.group_at(b, o.base_end, limit, depth);
    }

    /// Siguiente cabecera de objeto a partir de `from` (búsqueda de un GUID conocido + cabecera válida).
    fn next_header(&self, b: &[u8], from: usize) -> Option<usize> {
        let mut o = from;
        while o + 16 < b.len() {
            if (self.classes.is_empty() || self.known(&b[o..o + 16])) && self.header(b, o).is_some() {
                return Some(o);
            }
            o += 1;
        }
        None
    }

    pub fn parse_top(&self, b: &[u8]) -> (i32, i32, Vec<Obj>) {
        let x = rd_u32(b, 0).unwrap_or(0) as i32;
        let y = rd_u32(b, 4).unwrap_or(0) as i32;
        let mut out = Vec::new();
        let mut o = 8;
        while let Some(mut obj) = self.header(b, o) {
            self.fill(b, &mut obj, b.len(), 0);
            let scan_from = obj.group.as_ref().map(|g| g.list_end).unwrap_or(obj.base_end);
            let next = self.next_header(b, scan_from);
            obj.end = next.unwrap_or(b.len());
            out.push(obj);
            match next {
                Some(n) => o = n,
                None => break,
            }
        }
        (x, y, out)
    }
}

// ---------------------------------------------------------------- árbol editable

/// Objeto serializado (GUID + datos). Los grupos se separan en cabecera, hijos y cola para
/// poder recalcular los tamaños de los hijos al editar.
#[derive(Clone, Debug)]
pub enum Node {
    Leaf(Vec<u8>),
    Group { head: Vec<u8>, children: Vec<([u8; 16], Node)>, tail: Vec<u8> },
}

const OFF_RECT: usize = 16 + 1 + 4; // GUID, versión, id
const OFF_NAME: usize = OFF_RECT + 8;

impl Node {
    pub fn from_obj(o: &Obj, b: &[u8]) -> Node {
        match &o.group {
            Some(g) => Node::Group {
                head: b[o.off..o.base_end + 4].to_vec(),
                children: g
                    .children
                    .iter()
                    .map(|(so, c)| (b[so + 4..so + 20].try_into().unwrap(), Node::from_obj(c, b)))
                    .collect(),
                tail: b[g.list_end..o.end].to_vec(),
            },
            None => Node::Leaf(b[o.off..o.end].to_vec()),
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Node::Leaf(v) => v.clone(),
            Node::Group { head, children, tail } => {
                let mut out = head.clone();
                for (junk, c) in children {
                    let cb = c.bytes();
                    out.extend_from_slice(&(cb.len() as u32).to_le_bytes());
                    out.extend_from_slice(junk);
                    out.extend_from_slice(&cb);
                }
                out.extend_from_slice(&0u32.to_le_bytes());
                out.extend_from_slice(tail);
                out
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
    pub fn name(&self) -> String {
        let h = self.own();
        let n = rd_u32(h, OFF_NAME).unwrap() as usize;
        let u: Vec<u16> = h[OFF_NAME + 4..OFF_NAME + 2 + n].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&u)
    }
    pub fn rect(&self) -> [i16; 4] {
        let h = self.own();
        let mut r = [0i16; 4];
        for (i, v) in r.iter_mut().enumerate() {
            *v = rd_i16(h, OFF_RECT + 2 * i).unwrap();
        }
        r
    }
    pub fn set_rect(&mut self, r: [i16; 4]) {
        let h = self.own_mut();
        for (i, v) in r.iter().enumerate() {
            h[OFF_RECT + 2 * i..OFF_RECT + 2 * i + 2].copy_from_slice(&v.to_le_bytes());
        }
    }
    pub fn rename(&mut self, new: &str) {
        let h = self.own_mut();
        let n = rd_u32(h, OFF_NAME).unwrap() as usize;
        let mut enc = Vec::new();
        let u: Vec<u16> = new.encode_utf16().chain(std::iter::once(0)).collect();
        enc.extend_from_slice(&((u.len() * 2) as u32).to_le_bytes());
        for c in u {
            enc.extend_from_slice(&c.to_le_bytes());
        }
        h.splice(OFF_NAME..OFF_NAME + 4 + n, enc);
    }
    /// Desplaza el rectángulo del objeto y de todos sus hijos.
    pub fn shift(&mut self, dx: i16, dy: i16) {
        let r = self.rect();
        self.set_rect([r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy]);
        if let Node::Group { children, .. } = self {
            for (_, c) in children {
                c.shift(dx, dy);
            }
        }
    }
    pub fn find_mut(&mut self, name: &str) -> Option<&mut Node> {
        if self.name() == name {
            return Some(self);
        }
        if let Node::Group { children, .. } = self {
            for (_, c) in children {
                if let Some(f) = c.find_mut(name) {
                    return Some(f);
                }
            }
        }
        None
    }
    /// Sustituye un texto en los datos propios del objeto (no en los hijos). Formatos:
    /// u32 bytes (con NUL) + UTF-16 + NUL, y CString de MFC (FF FE FF len8 + UTF-16, sin NUL).
    pub fn replace_text(&mut self, old: &str, new: &str) -> usize {
        let start = OFF_NAME + 4 + rd_u32(self.own(), OFF_NAME).unwrap() as usize;
        let mut count = replace_in(self.own_mut(), start, old, new);
        if let Node::Group { tail, .. } = self {
            count += replace_in(tail, 0, old, new);
        }
        count
    }
}

impl Node {
    /// Sustituye una subcadena dentro de las cadenas del objeto (no en los hijos), corrigiendo la
    /// longitud de cada cadena. Devuelve (cadenas cambiadas, apariciones fuera de una cadena conocida).
    pub fn subst(&mut self, old: &str, new: &str) -> (usize, usize) {
        let start = OFF_NAME + 4 + rd_u32(self.own(), OFF_NAME).unwrap() as usize;
        let mut r = subst_in(self.own_mut(), start, old, new);
        if let Node::Group { tail, .. } = self {
            let t = subst_in(tail, 0, old, new);
            r = (r.0 + t.0, r.1 + t.1);
        }
        r
    }
}

fn u16_at(v: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([v[i], v[i + 1]])
}

fn subst_in(v: &mut Vec<u8>, from: usize, old: &str, new: &str) -> (usize, usize) {
    let o16 = utf16(old);
    let (mut done, mut orphan) = (0, 0);
    let mut i = from;
    while i + o16.len() <= v.len() {
        if v[i..i + o16.len()] != o16[..] {
            i += 1;
            continue;
        }
        // Principio de la cadena: retroceder mientras haya caracteres no nulos.
        let mut s = i;
        while s >= from + 2 && u16_at(v, s - 2) >= 0x0A && s - 2 >= 4 {
            s -= 2;
        }
        // ¿u32 bytes (con NUL) + UTF-16 + NUL? Probar cada inicio posible desde `s` hasta `i`.
        let mut handled = false;
        let mut cand = s;
        while cand <= i {
            if cand >= 4 {
                let n = rd_u32(v, cand - 4).unwrap() as usize;
                let end = cand + n;
                if n >= 2 && n % 2 == 0 && end <= v.len() && end >= i + o16.len() + 2 && u16_at(v, end - 2) == 0 {
                    let text: Vec<u16> = v[cand..end - 2].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                    if !text.contains(&0) {
                        let s_old = String::from_utf16_lossy(&text);
                        let s_new = s_old.replace(old, new);
                        let mut enc = utf16(&s_new);
                        enc.extend_from_slice(&[0, 0]);
                        let mut rep = (enc.len() as u32).to_le_bytes().to_vec();
                        rep.extend_from_slice(&enc);
                        v.splice(cand - 4..end, rep.iter().cloned());
                        i = cand - 4 + rep.len();
                        done += 1;
                        handled = true;
                        break;
                    }
                }
                // CString de MFC: FF FE FF + longitud (caracteres, 1 byte) + UTF-16 sin NUL.
                if v[cand - 4..cand - 1] == [0xFF, 0xFE, 0xFF] {
                    let n = v[cand - 1] as usize;
                    let end = cand + 2 * n;
                    if end <= v.len() && end >= i + o16.len() {
                        let text: Vec<u16> = v[cand..end].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                        let s_new = String::from_utf16_lossy(&text).replace(old, new);
                        let enc = utf16(&s_new);
                        assert!(enc.len() / 2 < 255, "CString demasiado larga");
                        let mut rep = vec![0xFF, 0xFE, 0xFF, (enc.len() / 2) as u8];
                        rep.extend_from_slice(&enc);
                        v.splice(cand - 4..end, rep.iter().cloned());
                        i = cand - 4 + rep.len();
                        done += 1;
                        handled = true;
                        break;
                    }
                }
            }
            cand += 2;
        }
        if !handled {
            orphan += 1;
            i += o16.len();
        }
    }
    (done, orphan)
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()
}

fn replace_in(v: &mut Vec<u8>, from: usize, old: &str, new: &str) -> usize {
    let (o16, n16) = (utf16(old), utf16(new));
    let mut count = 0;
    // u32 longitud en bytes con NUL
    let mut pat = ((o16.len() + 2) as u32).to_le_bytes().to_vec();
    pat.extend_from_slice(&o16);
    pat.extend_from_slice(&[0, 0]);
    let mut rep = ((n16.len() + 2) as u32).to_le_bytes().to_vec();
    rep.extend_from_slice(&n16);
    rep.extend_from_slice(&[0, 0]);
    count += splice_all(v, from, &pat, &rep);
    // CString de MFC (longitud < 255)
    if o16.len() / 2 < 255 && n16.len() / 2 < 255 {
        let mut pat = vec![0xFF, 0xFE, 0xFF, (o16.len() / 2) as u8];
        pat.extend_from_slice(&o16);
        let mut rep = vec![0xFF, 0xFE, 0xFF, (n16.len() / 2) as u8];
        rep.extend_from_slice(&n16);
        count += splice_all(v, from, &pat, &rep);
    }
    count
}

fn splice_all(v: &mut Vec<u8>, from: usize, pat: &[u8], rep: &[u8]) -> usize {
    let mut i = from;
    let mut n = 0;
    while i + pat.len() <= v.len() {
        if &v[i..i + pat.len()] == pat {
            v.splice(i..i + pat.len(), rep.iter().cloned());
            i += rep.len();
            n += 1;
        } else {
            i += 1;
        }
    }
    n
}

fn write_cfb(path: &str, objdata: &[u8], tags: &[u8]) {
    use std::io::Write;
    let cur = std::io::Cursor::new(Vec::new());
    let mut c = cfb::CompoundFile::create_with_version(cfb::Version::V3, cur).unwrap();
    c.create_storage("/ObjectData").unwrap();
    c.create_stream("/ObjectData/ObjectData").unwrap().write_all(objdata).unwrap();
    c.create_stream("/DetachedTagSet").unwrap().write_all(tags).unwrap();
    c.flush().unwrap();
    let bytes = c.into_inner().into_inner();
    std::fs::write(path, bytes).unwrap();
}

const EMPTY_TAGS: [u8; 45] = [
    0x28, 0, 0, 0, 2, 0, 0, 0, 0xef, 0xef, 0xef, 0xef, 0, 0, 0, 0, 0xa2, 0x0f, 0, 0, 0xef, 0xef, 0xef, 0xef, 0, 0, 0, 0, 0x6f, 0, 0,
    0, 0xda, 0x50, 0xed, 0x1c, 0, 0, 0, 0, 0xda, 0x50, 0xed, 0x1c, 0,
];

/// build <out> [tags=<fichero>] [anchor=x,y] <fichero>:<objeto|*> [rename=N] [text=VIEJO=>NUEVO] [move=dx,dy] [at=x,y] ...
fn build(ctx: &Ctx, args: &[String]) {
    let out = &args[0];
    let mut nodes: Vec<Node> = Vec::new();
    let mut tags: Option<Vec<u8>> = None;
    let mut anchor: Option<(i32, i32)> = None;
    let mut last: Vec<usize> = Vec::new();
    let mut target: Option<String> = None;
    fn each<F: FnMut(&mut Node)>(nodes: &mut [Node], last: &[usize], target: &Option<String>, mut f: F) {
        let mut hits = 0;
        for &i in last {
            let n = match target {
                Some(t) => nodes[i].find_mut(t),
                None => Some(&mut nodes[i]),
            };
            if let Some(n) = n {
                f(n);
                hits += 1;
            }
        }
        assert!(hits > 0, "ningún objeto para {target:?}");
    }
    for a in &args[1..] {
        if a.ends_with(".csv") {
            continue;
        }
        if let Some(t) = a.strip_prefix("tags=") {
            tags = read_input(t).1;
        } else if let Some(v) = a.strip_prefix("anchor=") {
            let p: Vec<i32> = v.split(',').map(|s| s.parse().unwrap()).collect();
            anchor = Some((p[0], p[1]));
        } else if let Some(v) = a.strip_prefix("sel=") {
            // Las operaciones siguientes se aplican a este objeto (buscado también dentro de grupos).
            target = Some(v.to_string());
        } else if let Some(v) = a.strip_prefix("rename=") {
            each(&mut nodes, &last, &target, |n| n.rename(v));
            // La selección sigue al objeto renombrado.
            if target.is_some() {
                target = Some(v.to_string());
            }
        } else if let Some(v) = a.strip_prefix("text=") {
            let (o, n) = v.split_once("=>").expect("text=VIEJO=>NUEVO");
            let (o, n) = (o.replace("\\n", "\n"), n.replace("\\n", "\n"));
            each(&mut nodes, &last, &target, |x| {
                let k = x.replace_text(&o, &n);
                eprintln!("text: {} sustituciones en {}", k, x.name());
            });
        } else if let Some(v) = a.strip_prefix("subst=") {
            let (o, n) = v.split_once("=>").expect("subst=VIEJO=>NUEVO");
            each(&mut nodes, &last, &target, |x| {
                let (k, orphan) = x.subst(o, n);
                eprintln!("subst: {k} cadenas cambiadas en {} ({orphan} apariciones fuera de cadenas conocidas)", x.name());
            });
        } else if let Some(v) = a.strip_prefix("move=") {
            let p: Vec<i16> = v.split(',').map(|s| s.parse().unwrap()).collect();
            each(&mut nodes, &last, &target, |n| n.shift(p[0], p[1]));
        } else if let Some(v) = a.strip_prefix("at=") {
            let p: Vec<i16> = v.split(',').map(|s| s.parse().unwrap()).collect();
            each(&mut nodes, &last, &target, |n| {
                let r = n.rect();
                n.shift(p[0] - r[0], p[1] - r[1]);
            });
        } else if let Some(v) = a.strip_prefix("rect=") {
            // Sólo el rectángulo propio (no los hijos): para probar si la clase guarda más geometría.
            let p: Vec<i16> = v.split(',').map(|s| s.parse().unwrap()).collect();
            each(&mut nodes, &last, &target, |n| n.set_rect([p[0], p[1], p[2], p[3]]));
        } else if let Some((file, name)) = a.rsplit_once(':').filter(|(f, _)| f.len() > 1) {
            target = None;
            let (b, dt) = read_input(file);
            if tags.is_none() {
                tags = dt;
            }
            let (_, _, objs) = ctx.parse_top(&b);
            let mut tops: Vec<Node> = objs.iter().map(|o| Node::from_obj(o, &b)).collect();
            last.clear();
            if name == "*" {
                for n in tops {
                    last.push(nodes.len());
                    nodes.push(n);
                }
            } else {
                let found = tops.iter_mut().find_map(|t| t.find_mut(name)).unwrap_or_else(|| panic!("no está {name} en {file}"));
                last.push(nodes.len());
                nodes.push(found.clone());
            }
        } else {
            panic!("argumento no reconocido: {a}");
        }
    }
    let (ax, ay) = anchor.unwrap_or_else(|| {
        let x = nodes.iter().map(|n| n.rect()[0] as i32).min().unwrap_or(0);
        let y = nodes.iter().map(|n| n.rect()[1] as i32).min().unwrap_or(0);
        (x, y)
    });
    let mut od = Vec::new();
    od.extend_from_slice(&ax.to_le_bytes());
    od.extend_from_slice(&ay.to_le_bytes());
    for n in &nodes {
        od.extend_from_slice(&n.bytes());
        eprintln!("+ {} {:?} ", n.name(), n.rect());
    }
    // Comprobación: el resultado tiene que volver a leerse igual.
    let (_, _, back) = ctx.parse_top(&od);
    assert_eq!(back.len(), nodes.len(), "relectura: número de objetos");
    assert_eq!(back.last().map(|o| o.end), Some(od.len()), "relectura: fin");
    let tags = tags.unwrap_or_else(|| EMPTY_TAGS.to_vec());
    write_cfb(out, &od, &tags);
    eprintln!("{out}: ObjectData {} B, ancla ({ax}, {ay}), {} objetos, DetachedTagSet {} B", od.len(), nodes.len(), tags.len());
}

fn print(o: &Obj, depth: usize, b: &[u8]) {
    let ind = "  ".repeat(depth);
    let ext = if o.flags & 1 != 0 { format!(" ext={}B", o.ext.1 - o.ext.0) } else { String::new() };
    let cls_ver = rd_u32(b, o.base_end).unwrap_or(0);
    println!(
        "{ind}@{:#08x}..{:#08x} {:<28} {:?} {} ({} B, v{}{ext}) {}",
        o.off,
        o.end,
        o.name,
        o.rect,
        o.guid,
        o.end - o.off,
        cls_ver,
        o.class
    );
    if let Some(g) = &o.group {
        println!("{ind}  [grupo v{} {} hijos, lista hasta {:#x}, resto {} B]", g.version, g.children.len(), g.list_end, o.end.saturating_sub(g.list_end));
        for (_, c) in &g.children {
            print(c, depth + 1, b);
        }
    }
}

fn load_map(path: &str) -> HashMap<String, (String, String)> {
    let mut m = HashMap::new();
    if let Ok(t) = std::fs::read_to_string(path) {
        for l in t.lines().skip(1) {
            let f: Vec<&str> = l.splitn(4, ',').collect();
            if f.len() == 4 {
                let name = f[2].trim_matches('"').to_string();
                m.insert(f[0].to_string(), (if name.is_empty() { f[1].to_string() } else { name }, f[3].to_string()));
            }
        }
    }
    m
}

fn read_input(path: &str) -> (Vec<u8>, Option<Vec<u8>>) {
    let mut raw = std::fs::read(path).expect("no se puede leer");
    // Las partes de la Toolchest (TCParts.tcp/PartNNN) llevan 8 bytes delante del compound file.
    if raw.get(8..12) == Some(&[0xD0, 0xCF, 0x11, 0xE0]) {
        raw.drain(..8);
    }
    if raw.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        let mut c = cfb::CompoundFile::open(std::io::Cursor::new(raw)).expect("cfb");
        let mut od = Vec::new();
        c.open_stream("/ObjectData/ObjectData").expect("sin ObjectData").read_to_end(&mut od).unwrap();
        let mut dts = Vec::new();
        let dt = c.open_stream("/DetachedTagSet").ok().map(|mut s| {
            s.read_to_end(&mut dts).unwrap();
            dts
        });
        (od, dt)
    } else {
        (raw, None)
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("uso: vdobj dump <fichero> [guidmap.csv]");
        std::process::exit(2);
    }
    // Con un CSV (stream,real,name,dll) se usa esa tabla completa; sin él, la tabla fija de clases
    // gráficas y la búsqueda de objetos de nivel superior sólo por el patrón de la cabecera.
    let map = match a.iter().find(|s| s.ends_with(".csv")) {
        Some(s) => load_map(s),
        None => classes::CLASSES.iter().map(|(g, n, d)| (g.to_string(), (n.to_string(), d.to_string()))).collect(),
    };
    let strict = a.iter().any(|s| s.ends_with(".csv"));
    let ctx = Ctx { classes: if strict { map.clone() } else { HashMap::new() }, names: map };
    match a[1].as_str() {
        "dump" => {
            let (b, dt) = read_input(&a[2]);
            let (x, y, objs) = ctx.parse_top(&b);
            println!("ObjectData {} B, ancla ({x}, {y}), {} objetos de nivel superior, DetachedTagSet {:?} B", b.len(), objs.len(), dt.map(|d| d.len()));
            for o in &objs {
                print(o, 0, &b);
            }
            let last = objs.last().map(|o| o.end).unwrap_or(8);
            if last != b.len() {
                println!("!! fin del último objeto {last:#x} != tamaño {:#x}", b.len());
            }
        }
        "build" => build(&ctx, &a[2..]),
        "roundtrip" => {
            // Lee, reconstruye con el árbol y compara byte a byte (sin cambios tiene que salir igual).
            for f in &a[2..] {
                if f.ends_with(".csv") {
                    continue;
                }
                let (b, _) = read_input(f);
                let (_, _, objs) = ctx.parse_top(&b);
                let mut od = b[..8].to_vec();
                for o in &objs {
                    od.extend_from_slice(&Node::from_obj(o, &b).bytes());
                }
                println!("{}\t{f}", if od == b { "igual" } else { "DISTINTO" });
            }
        }
        "scan" => {
            // Una línea por fichero: ok/!!, objetos de nivel superior y clases (recursivo).
            fn walk(o: &Obj, acc: &mut Vec<String>) {
                acc.push(o.class.split(" [").next().unwrap_or("?").replace(" Class", ""));
                if let Some(g) = &o.group {
                    for (_, c) in &g.children {
                        walk(c, acc);
                    }
                }
            }
            for f in &a[2..] {
                if f.ends_with(".csv") {
                    continue;
                }
                let (b, _) = read_input(f);
                let (x, y, objs) = ctx.parse_top(&b);
                let last = objs.last().map(|o| o.end).unwrap_or(8);
                let mut cls = Vec::new();
                for o in &objs {
                    walk(o, &mut cls);
                }
                let st = if !objs.is_empty() && last == b.len() { "ok" } else { "!!" };
                let names: Vec<&str> = objs.iter().map(|o| o.name.as_str()).collect();
                println!("{st}\t{f}\t{}B\t({x},{y})\t{}\t{}\t{}", b.len(), objs.len(), names.join("|"), cls.join(","));
            }
        }
        _ => eprintln!("orden desconocida"),
    }
}
