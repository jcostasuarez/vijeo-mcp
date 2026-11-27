//! Toolchest de la instalación (`Vijeo-Frame\Toolchest`): cada biblioteca es un `TCParts.tcp`, un
//! compound file con un stream `PartNNN` por pieza, que es el mismo contenido del portapapeles de
//! objetos con 8 bytes delante (inventario §13.2). Las de `Image_Library` usan una cabecera de objeto
//! antigua (versión 2) que no se lee: se cuentan aparte.

use std::io::Read;
use std::path::{Path, PathBuf};

use vijeo_core::{ToolchestLibrary, ToolchestPart};

use crate::objects::Clip;

const FILE: &str = "TCParts.tcp";

#[derive(Debug, Clone)]
pub struct Library {
    /// Carpeta dentro de `Toolchest`, con `\` (`Graph\BarGraph\Vertical`).
    pub name: String,
    pub file: PathBuf,
    /// Streams `PartNNN`, en orden.
    pub parts: Vec<String>,
}

pub fn root(frame_dir: &Path) -> PathBuf {
    frame_dir.join("Toolchest")
}

/// Bibliotecas de la Toolchest (sólo lee el directorio de cada compound file).
pub fn libraries(frame_dir: &Path) -> Vec<Library> {
    let root = root(frame_dir);
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(FILE)) {
                let Ok(c) = cfb::open(&p) else { continue };
                let mut parts: Vec<String> = c
                    .walk()
                    .filter(|e| e.is_stream())
                    .map(|e| e.name().to_owned())
                    .filter(|n| n.starts_with("Part") && n[4..].chars().all(|c| c.is_ascii_digit()))
                    .collect();
                parts.sort_by_key(|n| n[4..].parse::<u32>().unwrap_or(0));
                let name = dir.strip_prefix(&root).unwrap_or(&dir).to_string_lossy().replace('/', "\\");
                out.push(Library { name, file: p, parts });
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

type Tcp = cfb::CompoundFile<std::io::Cursor<Vec<u8>>>;

/// Abre la biblioteca en memoria (leerla del disco sector a sector es mucho más lento).
fn open(lib: &Library) -> Result<Tcp, String> {
    let bytes = std::fs::read(&lib.file).map_err(|e| format!("no se pudo leer {}: {e}", lib.file.display()))?;
    cfb::CompoundFile::open(std::io::Cursor::new(bytes)).map_err(|e| format!("{} no es un compound file válido: {e}", lib.file.display()))
}

fn read_from(c: &mut Tcp, lib: &Library, part: &str) -> Result<Clip, String> {
    let mut raw = Vec::new();
    c.open_stream(format!("/{part}")).map_err(|e| format!("no está {part} en {}: {e}", lib.name))?.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    Clip::from_bytes(&raw)
}

pub fn read_part(lib: &Library, part: &str) -> Result<Clip, String> {
    read_from(&mut open(lib)?, lib, part)
}

/// Todas las piezas de la biblioteca, abriéndola una vez.
pub fn read_all(lib: &Library) -> Vec<(String, Result<Clip, String>)> {
    match open(lib) {
        Ok(mut c) => lib.parts.iter().map(|p| (p.clone(), read_from(&mut c, lib, p))).collect(),
        Err(e) => lib.parts.iter().map(|p| (p.clone(), Err(e.clone()))).collect(),
    }
}

pub fn summary(lib: &Library) -> ToolchestLibrary {
    ToolchestLibrary { library: lib.name.clone(), parts: lib.parts.len() }
}

/// Descripción de una pieza ya leída.
pub fn describe(lib: &Library, part: &str, clip: &Clip) -> ToolchestPart {
    let mut kinds: Vec<String> = Vec::new();
    for (path, _) in clip.walk() {
        let k = clip.node(&path).kind();
        if !kinds.contains(&k) {
            kinds.push(k);
        }
    }
    let rects: Vec<[i32; 4]> = clip.objects.iter().map(|n| n.rect()).collect();
    let (l, t) = (rects.iter().map(|r| r[0]).min().unwrap_or(0), rects.iter().map(|r| r[1]).min().unwrap_or(0));
    let (r, b) = (rects.iter().map(|r| r[2]).max().unwrap_or(0), rects.iter().map(|r| r[3]).max().unwrap_or(0));
    ToolchestPart {
        id: format!("{}\\{part}", lib.name),
        library: lib.name.clone(),
        name: clip.objects.iter().map(|n| n.name()).collect::<Vec<_>>().join(", "),
        kinds,
        width: r - l + 1,
        height: b - t + 1,
    }
}

/// Busca la pieza `id` (`Biblioteca\PartNNN` o `Biblioteca\<nombre de su objeto>`) y la lee.
pub fn find(libs: &[Library], id: &str) -> Result<(ToolchestPart, Clip), String> {
    let id = id.trim().replace('/', "\\");
    let (lib_name, part) = id.rsplit_once('\\').ok_or_else(|| format!("«{id}» no es Biblioteca\\Pieza (usa el id de toolchest_list)"))?;
    let lib = libs
        .iter()
        .find(|l| l.name.eq_ignore_ascii_case(lib_name))
        .ok_or_else(|| format!("no hay ninguna biblioteca «{lib_name}» en la Toolchest (toolchest_list las enumera)"))?;
    if let Some(p) = lib.parts.iter().find(|p| p.eq_ignore_ascii_case(part)) {
        let clip = read_part(lib, p)?;
        return Ok((describe(lib, p, &clip), clip));
    }
    let mut found = Vec::new();
    for (p, clip) in read_all(lib) {
        if let Ok(clip) = clip {
            if clip.objects.iter().any(|n| n.name().eq_ignore_ascii_case(part)) {
                found.push((describe(lib, &p, &clip), clip));
            }
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(format!("no hay ninguna pieza «{part}» en «{}»", lib.name)),
        _ => Err(format!("hay {} piezas «{part}» en «{}»: usa su id ({})", found.len(), lib.name, found.iter().map(|(d, _)| d.id.as_str()).collect::<Vec<_>>().join(", "))),
    }
}
