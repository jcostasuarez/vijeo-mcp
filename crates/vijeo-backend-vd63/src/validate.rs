//! Validaciones previas a llamar a Vijeo: lo que Vijeo rechaza abriendo un diálogo modal hay que
//! rechazarlo antes aquí.

use std::path::{Component, Path, PathBuf};

use vijeo_core::{VijeoError, VijeoResult};

/// Caracteres que `NewProject` rechaza con un `MessageBox` (`SpanExcluding`, §12.6).
const FORBIDDEN: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|', '#', ',', ';', '.'];

/// Máximo de targets: con 32 o más, `NewTarget` abre un `AfxMessageBox` modal.
pub const MAX_TARGETS: usize = 32;

/// Nombre de proyecto o de target: no vacío, sólo ASCII imprimible y sin [`FORBIDDEN`].
pub fn name(field: &str, value: &str) -> VijeoResult<()> {
    if value.trim().is_empty() {
        return Err(VijeoError::invalid(field, "no puede estar vacío"));
    }
    if value.len() > 64 {
        return Err(VijeoError::invalid(field, "admite como mucho 64 caracteres"));
    }
    if let Some(c) = value.chars().find(|c| !c.is_ascii() || c.is_ascii_control()) {
        return Err(VijeoError::invalid(field, format!("sólo admite caracteres ASCII imprimibles (encontrado {c:?})")));
    }
    if let Some(c) = value.chars().find(|c| FORBIDDEN.contains(c)) {
        return Err(VijeoError::invalid(field, format!("no puede contener «{c}» (prohibidos: \\ / : * ? \" < > | # , ; .)")));
    }
    Ok(())
}

/// Nombre de objeto del proyecto (panel, driver, equipo) con la convención de Vijeo: una letra y luego letras,
/// dígitos o `_` (si no, Vijeo da el error 2003 «doesn't follow naming convention»: verificado con paneles base
/// llamados «Mot ores», «1Motor», «Motor-2» y «_Motor»).
pub fn identifier(field: &str, value: &str) -> VijeoResult<()> {
    name(field, value)?;
    let mut cs = value.chars();
    let ok = cs.next().is_some_and(|c| c.is_ascii_alphabetic()) && cs.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(VijeoError::invalid(field, format!("«{value}» no sigue la convención de nombres de Vijeo: una letra y luego letras, dígitos o «_», sin espacios ni guiones")))
    }
}

/// Normaliza una ruta absoluta sin tocar el disco (resuelve `.` y `..`).
fn lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn lower(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// `p` está dentro de alguna carpeta protegida (comparación sin mayúsculas, sobre la ruta real si existe).
pub fn is_protected(p: &Path, protected: &[PathBuf]) -> bool {
    let real = |x: &Path| std::fs::canonicalize(x).map(|c| PathBuf::from(c.to_string_lossy().trim_start_matches(r"\\?\"))).unwrap_or_else(|_| lexical(x));
    // La ruta puede no existir aún: se canonicaliza el primer antepasado que exista.
    let mut base = p.to_path_buf();
    let mut rest = Vec::new();
    while !base.exists() {
        match (base.file_name().map(|f| f.to_owned()), base.parent()) {
            (Some(f), Some(parent)) => {
                rest.push(f);
                base = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut full = real(&base);
    for f in rest.into_iter().rev() {
        full.push(f);
    }
    let full = lower(&full);
    protected.iter().any(|d| {
        let d = lower(&real(d));
        full == d || full.starts_with(&format!("{d}\\"))
    })
}

/// Ruta de un `.vdz`: absoluta, con extensión `.vdz` y fuera de las carpetas protegidas.
pub fn vdz_path(field: &str, p: &Path, protected: &[PathBuf]) -> VijeoResult<PathBuf> {
    if !p.is_absolute() {
        return Err(VijeoError::invalid(field, "tiene que ser una ruta absoluta"));
    }
    if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("vdz")) {
        return Err(VijeoError::invalid(field, "el MCP sólo trabaja con ficheros .vdz"));
    }
    let p = lexical(p);
    if is_protected(&p, protected) {
        return Err(VijeoError::Refused(format!("{} está en una carpeta protegida (workspace de Vijeo-Manager u otra configurada)", p.display())));
    }
    Ok(p)
}

/// Carpeta de salida (informes): absoluta y fuera de las carpetas protegidas.
pub fn output_dir(field: &str, p: &Path, protected: &[PathBuf]) -> VijeoResult<PathBuf> {
    if !p.is_absolute() {
        return Err(VijeoError::invalid(field, "tiene que ser una ruta absoluta"));
    }
    let p = lexical(p);
    if is_protected(&p, protected) {
        return Err(VijeoError::Refused(format!("{} está en una carpeta protegida", p.display())));
    }
    Ok(p)
}

/// Escapa texto para un elemento XML de `GenericCall`.
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(name("name", "Proyecto_1 A-b").is_ok());
        for bad in ["", "  ", "a.b", "a/b", "a#b", "año", "a;b", "a\tb"] {
            assert!(name("name", bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn protected_paths() {
        let prot = vec![PathBuf::from(r"C:\Users\Public\Documents\Vijeo-Designer 6.3\Vijeo-Manager")];
        assert!(is_protected(Path::new(r"c:\users\public\documents\vijeo-designer 6.3\vijeo-manager\x\y.vdz"), &prot));
        assert!(is_protected(Path::new(r"C:\Users\Public\Documents\Vijeo-Designer 6.3\Vijeo-Manager"), &prot));
        assert!(!is_protected(Path::new(r"C:\Users\Public\Documents\Vijeo-Designer 6.3\Vijeo-Manager2\a.vdz"), &prot));
        assert!(vdz_path("p", Path::new(r"C:\tmp\..\Users\Public\Documents\Vijeo-Designer 6.3\Vijeo-Manager\a.vdz"), &prot).is_err());
        assert!(vdz_path("p", Path::new(r"C:\tmp\a.vdz"), &prot).is_ok());
        assert!(vdz_path("p", Path::new(r"C:\tmp\a.SwxCF"), &prot).is_err());
        assert!(vdz_path("p", Path::new(r"a.vdz"), &prot).is_err());
    }

    #[test]
    fn escape() {
        assert_eq!(xml_escape("a<b>&\"'"), "a&lt;b&gt;&amp;&quot;&apos;");
    }
}
