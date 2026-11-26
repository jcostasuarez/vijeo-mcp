//! El `.vdz` es un ZIP con `<nombre>.SwxCF`, `DataFilesBackup/` y `SymbolTables/`. El nombre del proyecto
//! es sólo el de la entrada `.SwxCF`: el MCP abre copias con un nombre temporal ([`TEMP_PREFIX`]), que
//! Vijeo desempaqueta en `Vijeo-Manager\<temporal>`, y al guardar empaqueta esa carpeta con el nombre final.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use vijeo_core::{VijeoError, VijeoResult};

/// Prefijo de los proyectos temporales del MCP en el workspace; sólo se borran carpetas así.
pub const TEMP_PREFIX: &str = "MCP_TMP_";

/// Nombre de proyecto temporal único (válido para el asistente New Project: ASCII, sin puntos).
pub fn temp_project_name() -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    format!("{TEMP_PREFIX}{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst))
}

/// Empaqueta la carpeta de un proyecto (`Vijeo-Manager\<stored>`) como `.vdz`: `<stored>.SwxCF` como
/// `<name>.SwxCF` (el nombre del proyecto), `DataFilesBackup/` y `SymbolTables/`.
pub fn package_vdz(project_dir: &Path, stored: &str, name: &str, dest: &Path) -> VijeoResult<()> {
    use zip::write::SimpleFileOptions;
    let swx = project_dir.join(format!("{stored}.SwxCF"));
    if !swx.is_file() {
        return Err(VijeoError::Internal(format!("Vijeo no guardó {}", swx.display())));
    }
    let file = std::fs::File::create(dest).map_err(|e| VijeoError::io(dest, e))?;
    let mut zw = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let io = |e: std::io::Error| VijeoError::io(dest, e);
    let zerr = |e: zip::result::ZipError| VijeoError::io(dest, e);
    zw.start_file(format!("{name}.SwxCF"), opts).map_err(zerr)?;
    std::io::copy(&mut std::fs::File::open(&swx).map_err(|e| VijeoError::io(&swx, e))?, &mut zw).map_err(io)?;
    for dir in ["DataFilesBackup", "SymbolTables"] {
        zw.add_directory(format!("{dir}/"), opts).map_err(zerr)?;
        add_tree(&mut zw, &project_dir.join(dir), dir, opts).map_err(io)?;
    }
    zw.finish().map_err(zerr)?;
    Ok(())
}

/// Copia `src` en `dest` con el proyecto llamado `name`. El nombre del proyecto es sólo el de la entrada
/// `<nombre>.SwxCF` del ZIP; las entradas se copian sin recomprimir.
pub fn copy_with_project_name(src: &Path, dest: &Path, name: &str) -> VijeoResult<()> {
    fn zerr(p: &Path) -> impl Fn(zip::result::ZipError) -> VijeoError + '_ {
        move |e| VijeoError::io(p, e)
    }
    let mut za = zip::ZipArchive::new(std::fs::File::open(src).map_err(|e| VijeoError::io(src, e))?).map_err(zerr(src))?;
    let is_swx = |n: &str| !n.contains('/') && n.to_ascii_lowercase().ends_with(".swxcf");
    let found = za.file_names().filter(|n| is_swx(n)).count();
    if found != 1 {
        return Err(VijeoError::invalid("path", format!("{} no parece un proyecto de Vijeo: tiene {found} ficheros .SwxCF en la raíz del ZIP", src.display())));
    }
    let mut zw = zip::ZipWriter::new(std::fs::File::create(dest).map_err(|e| VijeoError::io(dest, e))?);
    for i in 0..za.len() {
        let f = za.by_index_raw(i).map_err(zerr(src))?;
        let entry = f.name().to_owned();
        if is_swx(&entry) {
            zw.raw_copy_file_rename(f, format!("{name}.SwxCF")).map_err(zerr(dest))?;
        } else {
            zw.raw_copy_file(f).map_err(zerr(dest))?;
        }
    }
    zw.finish().map_err(zerr(dest))?;
    Ok(())
}

/// Copia `src` en `dest` con otro contenido para su `.SwxCF` (mismo nombre de entrada); las demás entradas
/// se copian sin recomprimir.
pub fn replace_swx(src: &Path, dest: &Path, swx: &[u8]) -> VijeoResult<()> {
    use std::io::Write;
    fn zerr(p: &Path) -> impl Fn(zip::result::ZipError) -> VijeoError + '_ {
        move |e| VijeoError::io(p, e)
    }
    let mut za = zip::ZipArchive::new(std::fs::File::open(src).map_err(|e| VijeoError::io(src, e))?).map_err(zerr(src))?;
    let is_swx = |n: &str| !n.contains('/') && n.to_ascii_lowercase().ends_with(".swxcf");
    let mut zw = zip::ZipWriter::new(std::fs::File::create(dest).map_err(|e| VijeoError::io(dest, e))?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut replaced = false;
    for i in 0..za.len() {
        let f = za.by_index_raw(i).map_err(zerr(src))?;
        if is_swx(f.name()) && !replaced {
            let name = f.name().to_owned();
            drop(f);
            zw.start_file(name, opts).map_err(zerr(dest))?;
            zw.write_all(swx).map_err(|e| VijeoError::io(dest, e))?;
            replaced = true;
        } else {
            zw.raw_copy_file(f).map_err(zerr(dest))?;
        }
    }
    if !replaced {
        return Err(VijeoError::io(src, "el .vdz no contiene ningún .SwxCF"));
    }
    zw.finish().map_err(zerr(dest))?;
    Ok(())
}

fn add_tree(zw: &mut zip::ZipWriter<std::fs::File>, dir: &Path, prefix: &str, opts: zip::write::SimpleFileOptions) -> std::io::Result<()> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Ok(()) };
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let rel = format!("{prefix}/{}", e.file_name().to_string_lossy());
        if e.file_type()?.is_dir() {
            zw.add_directory(format!("{rel}/"), opts)?;
            add_tree(zw, &e.path(), &rel, opts)?;
        } else {
            zw.start_file(rel, opts)?;
            std::io::copy(&mut std::fs::File::open(e.path())?, zw)?;
        }
    }
    Ok(())
}

/// Borra la carpeta temporal del workspace. Sólo acepta `<workspace>\MCP_TMP_*`; reintenta un rato porque
/// Vijeo puede tardar en soltar los ficheros al salir.
pub fn remove_temp_project(workspace: &Path, name: &str) -> VijeoResult<()> {
    if !name.starts_with(TEMP_PREFIX) || name.contains(['\\', '/', '.']) {
        return Err(VijeoError::Internal(format!("se rechazó borrar «{name}» del workspace: no es un proyecto temporal del MCP")));
    }
    let dir = workspace.join(name);
    let mut last = None;
    for _ in 0..20 {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(VijeoError::io(&dir, last.map(|e| e.to_string()).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_names_are_valid_and_unique() {
        let (a, b) = (temp_project_name(), temp_project_name());
        assert_ne!(a, b);
        assert!(a.starts_with(TEMP_PREFIX));
        crate::validate::name("name", &a).unwrap();
    }

    #[test]
    fn only_temp_projects_are_removed() {
        let ws = std::env::temp_dir().join("vijeo-mcp-test-ws");
        for bad in ["PLANTA_SET3", "MCP_TMP_..\\x", "", "MCP_TMP_a.b"] {
            assert!(remove_temp_project(&ws, bad).is_err(), "{bad}");
        }
        assert!(remove_temp_project(&ws, "MCP_TMP_no_existe").is_ok());
    }

    #[test]
    fn renames_project_entry_only() {
        use std::io::{Read, Write};
        use zip::write::SimpleFileOptions;
        let dir = std::env::temp_dir().join(format!("vijeo-mcp-rename-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (src, dest) = (dir.join("src.vdz"), dir.join("dest.vdz"));
        let write = |path: &Path, entries: &[(&str, &[u8])]| {
            let mut zw = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
            let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (n, data) in entries {
                if n.ends_with('/') {
                    zw.add_directory(*n, opts).unwrap();
                } else {
                    zw.start_file(*n, opts).unwrap();
                    zw.write_all(data).unwrap();
                }
            }
            zw.finish().unwrap();
        };
        write(&src, &[("Compact_Modbus SL_M221 Book.SwxCF", b"cfb"), ("DataFilesBackup/", b""), ("DataFilesBackup/T_1/Splash.jpg", b"jpg")]);
        copy_with_project_name(&src, &dest, "MCP_TMP_B1").unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
        assert_eq!(z.file_names().collect::<std::collections::BTreeSet<_>>(), ["DataFilesBackup/", "DataFilesBackup/T_1/Splash.jpg", "MCP_TMP_B1.SwxCF"].into());
        let mut s = String::new();
        z.by_name("MCP_TMP_B1.SwxCF").unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "cfb");

        // Sin .SwxCF en la raíz no es un proyecto.
        write(&src, &[("sub/x.SwxCF", b"cfb")]);
        assert!(copy_with_project_name(&src, &dest, "MCP_TMP_B1").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
