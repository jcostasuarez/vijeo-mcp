//! Catálogo de familias y modelos HMI, descifrado en tiempo de ejecución de `TargetInfo\*.dat`.
//!
//! Regla de creabilidad (D5, §12.7 del inventario): la familia está activa en `Targets.dat` y la
//! `RequiredKey` del modelo está cubierta por la licencia. El MCP no puede leer la licencia; se asume la
//! estándar ([`ASSUMED_LICENSE_MASK`]). Los targets se crean con el asistente de la interfaz (modo normal):
//! el bloque `<SoMachine>` (`somachine`) sólo lo exigía la API COM, que trabaja en modo SoMachine.
//! Lo que dé `--probe-models` (crear de verdad cada modelo, [`crate::probe`]) manda sobre la regla.

use std::collections::HashMap;
use std::path::Path;

use roxmltree::{Document, Node};
use vijeo_core::{FamilyInfo, ModelFilter, ModelInfo, ModelProbe, ModelRef, ModelSelector, ProbeOutcome, ResolvedModel, VijeoError, VijeoResult};

/// Bits de licencia que se suponen presentes: todos salvo los de productos aparte (p.ej. Easy iPC,
/// `0x10000000`). Los SCU piden `0x200` y sí se crean.
pub const ASSUMED_LICENSE_MASK: u64 = 0x0FFF_FFFF;

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub families: Vec<FamilyInfo>,
    pub models: Vec<ModelInfo>,
    /// Motivo por el que no es creable, por `internal_name`.
    not_creatable: HashMap<String, String>,
}

struct Family {
    internal_name: String,
    name: String,
    default_model: Option<String>,
    model_list: Option<String>,
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.has_tag_name(name))
}

fn path<'a, 'i>(n: Node<'a, 'i>, p: &str) -> Option<Node<'a, 'i>> {
    p.split('/').try_fold(n, |cur, seg| child(cur, seg))
}

fn text(n: Node, p: &str) -> Option<String> {
    path(n, p).and_then(|c| c.text()).map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
}

fn parse_err(what: &str, e: roxmltree::Error) -> VijeoError {
    VijeoError::Internal(format!("catálogo {what} no es XML válido: {e}"))
}

fn parse_families(xml: &str) -> VijeoResult<Vec<Family>> {
    let doc = Document::parse(xml).map_err(|e| parse_err("Targets", e))?;
    Ok(doc
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("Target"))
        .filter_map(|t| {
            Some(Family {
                internal_name: text(t, "Identity/InternalName")?,
                name: text(t, "Identity/Name").unwrap_or_default(),
                default_model: text(t, "DefaultModel"),
                model_list: text(t, "ModelList"),
            })
        })
        .collect())
}

fn parse_models(xml: &str, catalog: &str, family: Option<&Family>) -> VijeoResult<Vec<ModelInfo>> {
    let doc = Document::parse(xml).map_err(|e| parse_err(catalog, e))?;
    let mut out = Vec::new();
    for m in doc.root_element().children().filter(|n| n.has_tag_name("Model")) {
        let Some(internal) = text(m, "Identity/InternalName") else { continue };
        let units: Vec<(String, Option<u32>, Option<u32>)> = path(m, "Display/DisplayUnits")
            .map(|du| {
                du.children()
                    .filter(|c| c.has_tag_name("DisplayUnit"))
                    .filter_map(|u| {
                        Some((text(u, "Name")?, text(u, "ScreenWidth").and_then(|v| v.parse().ok()), text(u, "ScreenHeight").and_then(|v| v.parse().ok())))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // DefaultDisplayUnit es un índice base 0 (verificado: 2 → HMIDT651).
        let default_unit = text(m, "Display/DisplayUnits/DefaultDisplayUnit")
            .and_then(|i| i.parse::<usize>().ok())
            .and_then(|i| units.get(i))
            .or(units.first())
            .cloned();
        let (screen_width, screen_height) = match &default_unit {
            Some((_, w, h)) => (*w, *h),
            None => (text(m, "Display/ScreenWidth").and_then(|v| v.parse().ok()), text(m, "Display/ScreenHeight").and_then(|v| v.parse().ok())),
        };
        let download_types = path(m, "Download/DownloadTypes")
            .map(|d| d.children().filter(|c| c.is_element() && c.text().map(str::trim) == Some("true")).map(|c| c.tag_name().name().to_owned()).collect())
            .unwrap_or_default();
        let hmi_type = match &default_unit {
            Some((u, _, _)) => format!("{internal}:{u}"),
            None => internal.clone(),
        };
        out.push(ModelInfo {
            name: text(m, "Identity/Name").unwrap_or_else(|| internal.clone()),
            hmi_type,
            family: family.map(|f| f.internal_name.clone()).or_else(|| text(m, "Identity/Target")).unwrap_or_default(),
            family_name: family.map(|f| f.name.clone()),
            display_units: units.iter().map(|u| u.0.clone()).collect(),
            default_display: default_unit.map(|u| u.0),
            screen_width,
            screen_height,
            target_os: text(m, "Download/TargetOS"),
            download_types,
            somachine: child(m, "SoMachine").is_some(),
            dual_ethernet: text(m, "Ethernet/DualEthernetSupported").is_some_and(|v| v.eq_ignore_ascii_case("true")),
            required_key: text(m, "Registration/RequiredKey").and_then(|k| k.parse().ok()).unwrap_or(0),
            creatable: false,
            probe: None,
            internal_name: internal,
        });
    }
    Ok(out)
}

fn creatable_reason(m: &ModelInfo, supported: bool) -> Option<String> {
    if !supported {
        Some("su familia está desactivada en Targets.dat".into())
    } else if m.required_key & !ASSUMED_LICENSE_MASK != 0 {
        Some(format!("exige una licencia aparte (RequiredKey = 0x{:X})", m.required_key))
    } else {
        None
    }
}

fn norm(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

/// Trozos alfanuméricos de un nombre comercial: `HMIS5T(HMIS65/S85)` → `hmis5t`, `hmis65`, `s85`.
fn tokens(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_ascii_alphanumeric()).filter(|t| !t.is_empty()).map(|t| t.to_ascii_lowercase())
}

impl Catalog {
    /// Descifra y parsea `TargetInfo\Targets.dat` y `TargetInfo\Models_*.dat`.
    pub fn load(frame_dir: &Path) -> VijeoResult<Self> {
        let dir = frame_dir.join("TargetInfo");
        let read = |p: &Path| -> VijeoResult<String> {
            let raw = std::fs::read(p).map_err(|e| VijeoError::io(p, e))?;
            vijeo_dat::decrypt_dat(&raw).map_err(|e| VijeoError::io(p, e))
        };
        let targets = read(&dir.join("Targets.dat"))?;
        let mut catalogs = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| VijeoError::io(&dir, e))? {
            let p = entry.map_err(|e| VijeoError::io(&dir, e))?.path();
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned();
            if stem.starts_with("Models_") && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dat")) {
                catalogs.push((stem, read(&p)?));
            }
        }
        catalogs.sort();
        Self::from_xml(&targets, &catalogs)
    }

    /// `catalogs`: pares (nombre del catálogo sin extensión, XML descifrado).
    pub fn from_xml(targets_xml: &str, catalogs: &[(String, String)]) -> VijeoResult<Self> {
        let families = parse_families(targets_xml)?;
        // Vijeo enlaza familia → catálogo por <ModelList>; el <Identity/Target> del modelo no siempre coincide.
        let by_catalog: HashMap<&str, &Family> = families.iter().filter_map(|f| f.model_list.as_deref().map(|l| (l, f))).collect();
        let mut cat = Catalog::default();
        for (name, xml) in catalogs {
            let fam = by_catalog.get(name.as_str()).copied();
            for mut m in parse_models(xml, name, fam)? {
                match creatable_reason(&m, fam.is_some()) {
                    None => m.creatable = true,
                    Some(r) => {
                        cat.not_creatable.insert(m.internal_name.clone(), r);
                    }
                }
                cat.models.push(m);
            }
        }
        cat.models.sort_by(|a, b| (&a.family, &a.internal_name).cmp(&(&b.family, &b.internal_name)));
        cat.families = families
            .iter()
            .map(|f| FamilyInfo {
                internal_name: f.internal_name.clone(),
                name: f.name.clone(),
                default_model: f.default_model.clone(),
                model_count: cat.models.iter().filter(|m| m.family == f.internal_name).count(),
            })
            .collect();
        Ok(cat)
    }

    pub fn filter(&self, f: &ModelFilter) -> Vec<ModelInfo> {
        let fam = f.family.as_deref().map(norm);
        let txt = f.text.as_deref().map(norm);
        self.models
            .iter()
            .filter(|m| fam.as_ref().is_none_or(|x| norm(&m.family) == *x || m.family_name.as_deref().is_some_and(|n| norm(n) == *x)))
            .filter(|m| f.creatable.is_none_or(|c| m.creatable == c))
            .filter(|m| {
                txt.as_ref().is_none_or(|t| {
                    norm(&m.internal_name).contains(t.as_str())
                        || norm(&m.name).contains(t.as_str())
                        || m.display_units.iter().any(|u| norm(u).contains(t.as_str()))
                })
            })
            .cloned()
            .collect()
    }

    fn by_internal(&self, internal: &str) -> Option<&ModelInfo> {
        self.models.iter().find(|m| m.internal_name.eq_ignore_ascii_case(internal.trim()))
    }

    fn find(&self, query: &str) -> VijeoResult<&ModelInfo> {
        if let Some(m) = self.by_internal(query) {
            return Ok(m);
        }
        let q = norm(query);
        let exact: Vec<&ModelInfo> = self.models.iter().filter(|m| norm(&m.name) == q).collect();
        let candidates = if exact.is_empty() {
            self.models
                .iter()
                .filter(|m| {
                    tokens(&m.name).any(|t| t == q)
                        || m.internal_name.to_ascii_lowercase().strip_prefix("model_").is_some_and(|s| s == q)
                })
                .collect()
        } else {
            exact
        };
        match candidates.as_slice() {
            [m] => Ok(m),
            [] => Err(VijeoError::invalid("model", format!("no hay ningún modelo «{query}» en el catálogo; usa catalog_list_models para ver los disponibles"))),
            many => {
                // Si sólo uno es creable, es el que se quiere.
                let creatable: Vec<_> = many.iter().filter(|m| m.creatable).collect();
                if let [m] = creatable.as_slice() {
                    return Ok(m);
                }
                let names: Vec<String> = many.iter().map(|m| format!("{} ({})", m.internal_name, m.name)).collect();
                Err(VijeoError::invalid("model", format!("«{query}» es ambiguo: {}", names.join(", "))))
            }
        }
    }

    pub fn resolve(&self, sel: &ModelSelector) -> VijeoResult<ResolvedModel> {
        let (model_q, inline_display) = match sel.model.split_once(':') {
            Some((m, d)) => (m, Some(d.trim())),
            None => (sel.model.as_str(), None),
        };
        let display_q = match (inline_display, sel.display.as_deref()) {
            (Some(a), Some(b)) if !a.eq_ignore_ascii_case(b.trim()) => {
                return Err(VijeoError::invalid("display", format!("contradice la pantalla indicada en model («{a}»)")));
            }
            (a, b) => a.or(b.map(str::trim)).filter(|d| !d.is_empty()),
        };
        let m = self.find(model_q)?;
        if !m.creatable {
            let why = self.not_creatable.get(&m.internal_name).cloned().unwrap_or_default();
            return Err(VijeoError::invalid("model", format!("{} ({}) no se puede crear: {why}", m.internal_name, m.name)));
        }
        let hmi_type = if m.display_units.is_empty() {
            if let Some(d) = display_q {
                return Err(VijeoError::invalid("display", format!("{} no es modular y no admite pantalla («{d}»)", m.internal_name)));
            }
            m.internal_name.clone()
        } else {
            let unit = match display_q {
                None => m.default_display.clone().unwrap_or_else(|| m.display_units[0].clone()),
                Some(d) => {
                    let dn = norm(d);
                    let matches: Vec<&String> = match m.display_units.iter().find(|u| norm(u) == dn) {
                        Some(u) => vec![u],
                        None => m.display_units.iter().filter(|u| norm(u).starts_with(&dn)).collect(),
                    };
                    match matches.as_slice() {
                        [u] => (*u).clone(),
                        _ => {
                            return Err(VijeoError::invalid(
                                "display",
                                format!("«{d}» no identifica una única pantalla de {}: {}", m.internal_name, m.display_units.join(", ")),
                            ))
                        }
                    }
                }
            };
            format!("{}:{unit}", m.internal_name)
        };
        Ok(ResolvedModel { model: m.clone(), hmi_type })
    }

    /// Interpreta lo que devuelve `Target.Type()`: `Model_X`, `Model_X:Pantalla` o
    /// `Target=Model_X[:Pantalla],Control=True`.
    pub fn identify(&self, hmi_type: &str) -> Option<ModelRef> {
        let (internal, display) = split_hmi_type(hmi_type);
        let m = self.by_internal(internal)?;
        Some(ModelRef { internal_name: m.internal_name.clone(), name: m.name.clone(), display: display.map(str::to_owned) })
    }

    /// `hmi_type` de un target guardado: el modelo del fichero y, en los modulares, la única pantalla con
    /// esa resolución (`1280x800` es ambigua entre HMIDT651 y HMIDT652: entonces queda sólo el modelo).
    pub fn hmi_type_of(&self, model: &str, resolution: Option<&str>) -> String {
        let Some(m) = self.by_internal(model) else { return model.to_owned() };
        let Some(res) = resolution.map(|r| format!("({})", r.replace(' ', ""))) else { return m.internal_name.clone() };
        let units: Vec<&String> = m.display_units.iter().filter(|u| u.replace(' ', "").ends_with(&res)).collect();
        match units.as_slice() {
            [u] => format!("{}:{u}", m.internal_name),
            _ => m.internal_name.clone(),
        }
    }

    /// Aplica lo que dio `--probe-models`, que manda sobre la regla. Un `failed` no cambia `creatable`.
    pub fn apply_probes(&mut self, probes: &[ModelProbe]) {
        for p in probes {
            let Some(m) = self.models.iter_mut().find(|m| m.internal_name.eq_ignore_ascii_case(&p.internal_name)) else { continue };
            m.probe = Some(p.outcome);
            match p.outcome {
                ProbeOutcome::Created => {
                    m.creatable = true;
                    self.not_creatable.remove(&m.internal_name);
                }
                ProbeOutcome::Rejected => {
                    m.creatable = false;
                    let why = p.detail.as_deref().unwrap_or("sin detalle");
                    self.not_creatable.insert(m.internal_name.clone(), format!("Vijeo no lo creó al probarlo con --probe-models ({why})"));
                }
                ProbeOutcome::Failed => {}
            }
        }
    }

    /// Si el modelo (código interno) tiene segunda Ethernet; `None` si no está en el catálogo.
    pub fn dual_ethernet(&self, model: &str) -> Option<bool> {
        self.by_internal(model).map(|m| m.dual_ethernet)
    }

    /// Familia del modelo con el nombre que muestra el asistente de Vijeo (`HMIGTU Series`).
    pub fn family_name(&self, m: &ModelInfo) -> String {
        m.family_name.clone().or_else(|| self.families.iter().find(|f| f.internal_name == m.family).map(|f| f.name.clone())).unwrap_or_else(|| m.family.clone())
    }
}

/// Separa código interno y pantalla de un `hmi_type` (cualquiera de sus formas).
pub fn split_hmi_type(hmi_type: &str) -> (&str, Option<&str>) {
    let s = hmi_type.trim();
    let s = s.strip_prefix("Target=").map(|r| r.split(',').next().unwrap_or(r)).unwrap_or(s);
    match s.split_once(':') {
        Some((m, d)) => (m.trim(), Some(d.trim())),
        None => (s, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGETS: &str = r#"<?xml version="1.0"?>
<TargetList>
  <!-- <Target><Identity><InternalName>Old</InternalName></Identity><ModelList>Models_Old</ModelList></Target> -->
  <Target><Identity><InternalName>Target_HMIGTU</InternalName><Name>HMIGTU Series</Name></Identity>
    <DefaultModel>Model_HMIG3U</DefaultModel><ModelList>Models_HMIGTU</ModelList></Target>
  <Target><Identity><InternalName>Target_HMISTU</InternalName><Name>HMISTU Series</Name></Identity>
    <ModelList>Models_HMISTU</ModelList></Target>
</TargetList>"#;

    const GTU: &str = r#"<?xml version="1.0" encoding="utf-16"?>
<ModelList>
  <Model><Identity><InternalName>Model_HMIG3U</InternalName><Name>HMIG3U</Name><Target>Target_HMIGTU</Target></Identity>
    <SoMachine><TypeID>1</TypeID></SoMachine><Registration><RequiredKey>0</RequiredKey></Registration>
    <Display><DisplayUnits><DefaultDisplayUnit>1</DefaultDisplayUnit>
      <DisplayUnit><Name>HMIDT351 (800x480)</Name><ScreenWidth>800</ScreenWidth><ScreenHeight>480</ScreenHeight></DisplayUnit>
      <DisplayUnit><Name>HMIDT651 (1280x800)</Name><ScreenWidth>1280</ScreenWidth><ScreenHeight>800</ScreenHeight></DisplayUnit>
      <DisplayUnit><Name>HMIDT652 (1280x800)</Name><ScreenWidth>1280</ScreenWidth><ScreenHeight>800</ScreenHeight></DisplayUnit>
    </DisplayUnits></Display>
    <Ethernet><EthernetSupported>true</EthernetSupported><DualEthernetSupported>true</DualEthernetSupported></Ethernet>
    <Download><DownloadTypes><Ethernet>true</Ethernet><USB>false</USB></DownloadTypes><TargetOS>WinCE</TargetOS></Download></Model>
  <Model><Identity><InternalName>Model_HMIG5U</InternalName><Name>HMIG5U</Name></Identity>
    <Registration><RequiredKey>0</RequiredKey></Registration></Model>
</ModelList>"#;

    const STU: &str = r#"<ModelList>
  <!-- <Model><Identity><InternalName>Model_HMISTU855</InternalName></Identity></Model> -->
  <Model><Identity><InternalName>Model_HMIS5T</InternalName><Name>HMIS5T(HMIS65/S85)</Name></Identity><SoMachine/>
    <Registration><RequiredKey>512</RequiredKey></Registration><Display><ScreenWidth>800</ScreenWidth><ScreenHeight>480</ScreenHeight></Display></Model>
  <Model><Identity><InternalName>MODEL_EIPC</InternalName><Name>Easy iPC</Name></Identity><SoMachine/>
    <Registration><RequiredKey>268435456</RequiredKey></Registration></Model>
</ModelList>"#;

    const ORPHAN: &str = r#"<ModelList><Model><Identity><InternalName>Model_X</InternalName><Name>X</Name><Target>Target_X</Target></Identity><SoMachine/></Model></ModelList>"#;

    fn cat() -> Catalog {
        Catalog::from_xml(
            TARGETS,
            &[("Models_HMIGTU".into(), GTU.into()), ("Models_HMISTU".into(), STU.into()), ("Models_Orphan".into(), ORPHAN.into())],
        )
        .unwrap()
    }

    #[test]
    fn parses_families_and_models() {
        let c = cat();
        assert_eq!(c.families.len(), 2);
        assert_eq!(c.models.len(), 5);
        let g3u = c.by_internal("model_hmig3u").unwrap();
        assert_eq!(g3u.hmi_type, "Model_HMIG3U:HMIDT651 (1280x800)");
        assert_eq!(g3u.screen_width, Some(1280));
        assert_eq!(g3u.download_types, vec!["Ethernet"]);
        assert!(g3u.creatable);
        assert_eq!((c.dual_ethernet("Model_HMIG3U"), c.dual_ethernet("Model_HMIS5T"), c.dual_ethernet("Model_Nada")), (Some(true), Some(false), None));
        assert!(c.by_internal("Model_HMIG5U").unwrap().creatable, "sin <SoMachine>: se crea con el asistente");
        assert!(c.by_internal("Model_HMIS5T").unwrap().creatable, "512 cubierto por la licencia");
        assert!(!c.by_internal("MODEL_EIPC").unwrap().creatable, "licencia aparte");
        let x = c.by_internal("Model_X").unwrap();
        assert!(!x.creatable && x.family == "Target_X", "familia desactivada");
        assert!(c.by_internal("Model_HMISTU855").is_none(), "comentado");
    }

    #[test]
    fn resolves_selectors() {
        let c = cat();
        let r = |m: &str, d: Option<&str>| c.resolve(&ModelSelector { model: m.into(), display: d.map(Into::into) });
        assert_eq!(r("Model_HMIS5T", None).unwrap().hmi_type, "Model_HMIS5T");
        assert_eq!(r("hmis65", None).unwrap().hmi_type, "Model_HMIS5T");
        assert_eq!(r("HMIG3U", Some("HMIDT351")).unwrap().hmi_type, "Model_HMIG3U:HMIDT351 (800x480)");
        assert_eq!(r("Model_HMIG3U:HMIDT351 (800x480)", None).unwrap().hmi_type, "Model_HMIG3U:HMIDT351 (800x480)");
        assert_eq!(r("HMIG3U", None).unwrap().hmi_type, "Model_HMIG3U:HMIDT651 (1280x800)");
        assert!(r("HMIG3U", Some("HMIDT65")).is_err(), "prefijo ambiguo");
        assert!(r("HMIS5T", Some("X")).is_err(), "no modular");
        assert!(r("Easy iPC", None).is_err(), "no creable");
        assert!(r("nada", None).is_err());
    }

    #[test]
    fn hmi_type_from_file() {
        let c = cat();
        assert_eq!(c.hmi_type_of("Model_HMIG3U", Some("800x480")), "Model_HMIG3U:HMIDT351 (800x480)");
        assert_eq!(c.hmi_type_of("Model_HMIG3U", Some("1280x800")), "Model_HMIG3U", "dos pantallas con esa resolución");
        assert_eq!(c.hmi_type_of("Model_HMIS5T", Some("800x480")), "Model_HMIS5T");
        assert_eq!(c.hmi_type_of("Model_Nope", None), "Model_Nope");
    }

    #[test]
    fn identifies_type_forms() {
        let c = cat();
        let m = c.identify("Target=Model_HMIS5T,Control=False").unwrap();
        assert_eq!(m.internal_name, "Model_HMIS5T");
        let m = c.identify("Model_HMIG3U:HMIDT351 (800x480)").unwrap();
        assert_eq!(m.display.as_deref(), Some("HMIDT351 (800x480)"));
        assert!(c.identify("Model_Nope").is_none());
    }

    #[test]
    fn probes_override_the_rule() {
        let mut c = cat();
        let probe = |name: &str, outcome, detail: Option<&str>| ModelProbe { internal_name: name.into(), hmi_type: name.into(), outcome, detail: detail.map(Into::into), duration_ms: 1 };
        c.apply_probes(&[
            probe("model_hmis5t", ProbeOutcome::Rejected, Some("el asistente no lo ofrece")),
            probe("MODEL_EIPC", ProbeOutcome::Created, None),
            probe("Model_HMIG5U", ProbeOutcome::Failed, Some("timeout")),
            probe("Model_Nada", ProbeOutcome::Created, None),
        ]);
        let m = |n: &str| c.by_internal(n).unwrap().clone();
        assert!(!m("Model_HMIS5T").creatable && m("Model_HMIS5T").probe == Some(ProbeOutcome::Rejected));
        assert!(m("MODEL_EIPC").creatable, "la prueba manda sobre la licencia supuesta");
        assert!(m("Model_HMIG5U").creatable && m("Model_HMIG5U").probe == Some(ProbeOutcome::Failed), "un fallo no cambia la regla");
        let err = c.resolve(&ModelSelector { model: "HMIS5T".into(), display: None }).unwrap_err().to_string();
        assert!(err.contains("--probe-models") && err.contains("no lo ofrece"), "{err}");
        assert!(c.resolve(&ModelSelector { model: "Easy iPC".into(), display: None }).is_ok());
    }

    #[test]
    fn filters() {
        let c = cat();
        assert_eq!(c.filter(&ModelFilter { creatable: Some(true), ..Default::default() }).len(), 3);
        assert_eq!(c.filter(&ModelFilter { family: Some("hmigtu series".into()), ..Default::default() }).len(), 2);
        assert_eq!(c.filter(&ModelFilter { text: Some("dt351".into()), ..Default::default() }).len(), 1);
    }
}
