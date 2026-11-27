//! Objetos de pantalla generados desde cero: la serialización propia de cada clase (lo que va detrás de la
//! cabecera común de [`crate::objects`]) en el formato del portapapeles de Vijeo 6.3 (flags de guardado 9).
//!
//! Descifrado con Ghidra y comprobado con los objetos copiados de los proyectos de ejemplo: cada módulo lee y
//! vuelve a escribir byte a byte todos los objetos de su clase del corpus (inventario §13.3).
//!
//! - [`bin`]: tipos primitivos (`IStream::Write` directo, BSTR, CString de MFC).
//! - [`expr`]: `CExpression` (PFUtility), las referencias a variables y expresiones.
//! - [`action`]: `CNativeAction` (HMIManagers), las acciones de los botones.
//! - [`hmi`]: bloques compartidos de HMIManagers: colores, etiquetas y fuentes, imágenes y estilo nativo.
//! - [`switch`]: el botón (`Switch`, versión 6).

pub mod action;
pub mod bin;
pub mod expr;
pub mod hmi;
pub mod switch;
pub mod anim;
pub mod display;
pub mod lamp;
pub mod shapes;

#[cfg(test)]
mod corpus {
    //! Ida y vuelta con objetos reales copiados de Vijeo: `VIJEO_MCP_CORPUS` = carpeta con portapapeles
    //! (`*.bin`, compound files con `/ObjectData/ObjectData`). Sin la variable, la prueba no hace nada.
    use crate::objects::{Clip, Node};

    fn class_roundtrip(kind: &str, data: &[u8]) -> Option<Result<Vec<u8>, String>> {
        Some(match kind {
            "Switch" => super::switch::Switch::parse(data).map(|s| s.to_bytes()),
            "Lamp" => super::lamp::Lamp::parse(data).map(|s| s.to_bytes()),
            "DataDisplay" => super::display::DataDisplay::parse(data).map(|s| s.to_bytes()),
            "Text" => super::shapes::Text::parse(data).map(|s| s.write()),
            "Rectangle" => super::shapes::Rectangle::parse(data).map(|s| s.write()),
            "Line" => super::shapes::Line::parse(data).map(|s| s.write()),
            "Circle" => super::shapes::Ellipse::parse(data).map(|s| s.write()),
            _ => return None,
        })
    }

    fn visit(n: &Node, file: &str, stats: &mut std::collections::BTreeMap<String, (usize, usize)>, bad: &mut Vec<String>) {
        for (_, c) in n.children() {
            visit(c, file, stats, bad);
        }
        if n.is_group() {
            return;
        }
        if let Some(ext) = n.extension() {
            let e = stats.entry("(animaciones)".into()).or_default();
            e.0 += 1;
            match super::anim::parse_ext(ext).and_then(|a| super::anim::write_ext(&a)) {
                Ok(b) if b == ext => e.1 += 1,
                Ok(_) => bad.push(format!("{file} {}: extensión distinta", n.name())),
                Err(err) => bad.push(format!("{file} {}: extensión: {err}", n.name())),
            }
        }
        let kind = n.kind();
        let data = n.class_data();
        if let Some(r) = class_roundtrip(&kind, data) {
            let e = stats.entry(kind.clone()).or_default();
            e.0 += 1;
            match r {
                Ok(b) if b == data => e.1 += 1,
                Ok(_) => bad.push(format!("{file} {} ({kind}): distinto", n.name())),
                Err(err) => bad.push(format!("{file} {} ({kind}): {err}", n.name())),
            }
        }
    }

    #[test]
    fn corpus_roundtrip() {
        let Ok(dir) = std::env::var("VIJEO_MCP_CORPUS") else { return };
        let mut stats = std::collections::BTreeMap::new();
        let mut bad = Vec::new();
        for f in std::fs::read_dir(&dir).unwrap() {
            let p = f.unwrap().path();
            if p.extension().and_then(|e| e.to_str()) != Some("bin") {
                continue;
            }
            let file = p.file_name().unwrap().to_string_lossy().to_string();
            let clip = Clip::from_bytes(&std::fs::read(&p).unwrap()).unwrap();
            for n in &clip.objects {
                visit(n, &file, &mut stats, &mut bad);
            }
        }
        for (k, (n, ok)) in &stats {
            eprintln!("{k}: {ok}/{n}");
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }
}
pub mod build;
