//! Caché de `--probe-models` (D5): qué modelos crea de verdad el asistente New Target de esta instalación.
//!
//! La regla de `creatable` supone la licencia estándar; la prueba en vivo crea y borra un target de cada
//! modelo y su resultado manda sobre la regla. Se guarda por instalación y sólo vale mientras no cambien
//! los catálogos (`TargetInfo\*.dat`): una actualización de Vijeo la invalida.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use vijeo_core::{Installation, ModelProbe, ProbeOutcome, VijeoError, VijeoResult};

#[derive(Debug, Serialize, Deserialize)]
struct ProbeCache {
    product_version: String,
    frame_dir: PathBuf,
    /// Nombre, tamaño y fecha de los `TargetInfo\*.dat` al probar.
    catalog: String,
    /// Segundos desde 1970 de la última prueba.
    probed_at: u64,
    models: Vec<ModelProbe>,
}

/// Carpeta por defecto: `%LOCALAPPDATA%\vijeo-mcp`.
pub fn default_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("vijeo-mcp"))
}

pub fn cache_file(dir: &Path, inst: &Installation) -> PathBuf {
    dir.join(format!("models-{}.json", inst.product_version))
}

/// Huella de los catálogos de la instalación.
pub fn catalog_stamp(frame_dir: &Path) -> String {
    let Ok(rd) = std::fs::read_dir(frame_dir.join("TargetInfo")) else { return String::new() };
    let mut parts: Vec<String> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("dat")))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            let secs = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
            Some(format!("{}:{}:{secs}", e.file_name().to_string_lossy(), m.len()))
        })
        .collect();
    parts.sort();
    parts.join(";")
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().trim_end_matches('\\').eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches('\\'))
}

/// Resultados guardados para `inst`; nada si no hay caché o es de otra instalación o de otros catálogos.
pub fn load(file: &Path, inst: &Installation) -> Vec<ModelProbe> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    match serde_json::from_str::<ProbeCache>(&text) {
        Ok(c) if c.product_version == inst.product_version.to_string() && same_dir(&c.frame_dir, &inst.frame_dir) && c.catalog == catalog_stamp(&inst.frame_dir) => c.models,
        Ok(_) => {
            tracing::info!(file = %file.display(), "la caché de --probe-models es de otra instalación o de otros catálogos: no se usa");
            Vec::new()
        }
        Err(e) => {
            tracing::warn!(file = %file.display(), "caché de --probe-models ilegible: {e}");
            Vec::new()
        }
    }
}

/// Junta `new` con lo que ya había: un modelo probado otra vez se reemplaza, salvo que ahora fallara la
/// prueba (`failed`) y antes no.
pub fn merge(mut models: Vec<ModelProbe>, new: &[ModelProbe]) -> Vec<ModelProbe> {
    for n in new {
        match models.iter_mut().find(|m| m.internal_name.eq_ignore_ascii_case(&n.internal_name)) {
            Some(old) if n.outcome == ProbeOutcome::Failed && old.outcome != ProbeOutcome::Failed => {}
            Some(old) => *old = n.clone(),
            None => models.push(n.clone()),
        }
    }
    models.sort_by(|a, b| a.internal_name.cmp(&b.internal_name));
    models
}

/// Añade `new` a la caché de `inst` y la escribe (temporal + renombrar).
pub fn store(file: &Path, inst: &Installation, new: &[ModelProbe]) -> VijeoResult<()> {
    let cache = ProbeCache {
        product_version: inst.product_version.to_string(),
        frame_dir: inst.frame_dir.clone(),
        catalog: catalog_stamp(&inst.frame_dir),
        probed_at: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        models: merge(load(file, inst), new),
    };
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| VijeoError::io(dir, e))?;
    }
    let json = serde_json::to_string_pretty(&cache).map_err(|e| VijeoError::Internal(format!("caché de --probe-models: {e}")))?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| VijeoError::io(&tmp, e))?;
    std::fs::rename(&tmp, file).map_err(|e| VijeoError::io(file, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vijeo_core::Version;

    fn probe(name: &str, outcome: ProbeOutcome) -> ModelProbe {
        ModelProbe { internal_name: name.into(), hmi_type: name.into(), outcome, detail: None, duration_ms: 0 }
    }

    #[test]
    fn a_failure_does_not_erase_what_was_known() {
        let old = vec![probe("Model_A", ProbeOutcome::Created), probe("Model_B", ProbeOutcome::Failed), probe("Model_C", ProbeOutcome::Created)];
        let got = merge(old, &[probe("model_a", ProbeOutcome::Failed), probe("Model_B", ProbeOutcome::Rejected), probe("Model_D", ProbeOutcome::Created)]);
        let outcomes: Vec<(&str, ProbeOutcome)> = got.iter().map(|m| (m.internal_name.as_str(), m.outcome)).collect();
        assert_eq!(
            outcomes,
            [("Model_A", ProbeOutcome::Created), ("Model_B", ProbeOutcome::Rejected), ("Model_C", ProbeOutcome::Created), ("Model_D", ProbeOutcome::Created)]
        );
    }

    #[test]
    fn round_trips_and_checks_the_installation() {
        let dir = std::env::temp_dir().join(format!("vijeo-mcp-probe-test-{}", std::process::id()));
        let frame = dir.join("Vijeo-Frame");
        std::fs::create_dir_all(frame.join("TargetInfo")).unwrap();
        std::fs::write(frame.join("TargetInfo").join("Targets.dat"), b"x").unwrap();
        let inst = Installation { product_version: Version::parse("6.3.0.103").unwrap(), frame_dir: frame.clone(), registry_key: String::new() };
        let file = cache_file(&dir, &inst);
        assert!(load(&file, &inst).is_empty());
        store(&file, &inst, &[probe("Model_A", ProbeOutcome::Created)]).unwrap();
        store(&file, &inst, &[probe("Model_B", ProbeOutcome::Rejected)]).unwrap();
        assert_eq!(load(&file, &inst).len(), 2);
        let other = Installation { product_version: Version::parse("6.3.0.104").unwrap(), ..inst.clone() };
        assert!(load(&file, &other).is_empty(), "otra versión");
        std::fs::write(frame.join("TargetInfo").join("Targets.dat"), b"xy").unwrap();
        assert!(load(&file, &inst).is_empty(), "catálogo cambiado");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
