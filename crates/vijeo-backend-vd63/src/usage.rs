//! Uso de las variables leído del `.vdz`, sin Vijeo: dónde se nombra cada variable (pantallas y sus objetos,
//! acciones, alarmas…), cuáles no se usan y qué enlaces apuntan a variables que ya no existen.
//!
//! Los objetos de los paneles se leen del stream `GraphicalObject` (`objects::PanelObjects`); en ellos una variable
//! aparece como enlace por id (`{T,2:1.<id>,Nombre}`, `{T,3:1.<id>.<miembro>,Miembro}`) o por nombre en expresiones
//! y scripts (`TagDB.Carpeta.Var`, `Carpeta.Var.getIntValue()`). El resto de streams del target se buscan enteros.

use std::collections::{BTreeMap, BTreeSet};

use crate::objects::{strings_in, Node, PanelObjects};
use crate::tagdb::{prop, TagDb};
use crate::vxml::identifiers;

/// Un enlace por id que ya no lleva a ninguna variable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Broken {
    pub location: String,
    /// Nombre que guarda el enlace (el que tenía la variable).
    pub name: String,
    pub id: u32,
}

#[derive(Debug, Default)]
pub struct Usage {
    /// Variable (como en `vdz_variable_list`) → sitios donde se nombra.
    pub references: BTreeMap<String, BTreeSet<String>>,
    pub broken: BTreeSet<Broken>,
}

/// Panel al que pertenece un stream de `WindowList`/`PopupWindowList` y cómo llamarlo.
fn panel_label(path: &str, streams: &BTreeMap<String, Vec<u8>>, popups: &BTreeMap<String, String>) -> Option<String> {
    let dir = path.rsplit_once('/')?.0;
    let name = streams.get(&format!("{dir}/WindowObject")).and_then(|b| strings_in(b, 0).into_iter().next()).unwrap_or_else(|| dir.rsplit('/').next().unwrap_or(dir).to_owned());
    if dir.contains("/PopupWindowList/") {
        let group = dir.split("/PopupWindowList/").nth(1)?.split('/').next()?;
        let window = popups.get(group).cloned().unwrap_or_else(|| group.to_owned());
        return Some(format!("Window «{window}» › Panel «{name}»"));
    }
    if dir.contains("/MasterPanelsNodeStorage/") {
        return Some(format!("MasterPanel «{name}»"));
    }
    if dir.contains("/DefinitionNodeStorage/") {
        return Some(format!("Template «{name}»"));
    }
    Some(format!("Panel «{name}»"))
}

/// Nombre legible de un stream que no es de un panel.
fn stream_label(path: &str) -> String {
    const KNOWN: &[(&str, &str)] = &[
        ("ActionsManagerStream", "Actions"),
        ("AlarmUnits", "Alarms"),
        ("AlarmUnitManager", "Alarms"),
        ("RecipeGroupManager", "Recipes"),
        ("DataLoggingStream", "DataLogging"),
        ("SecurityManagerStream", "Security"),
        ("TouchFieldManagerStream", "TouchFields"),
        ("FunctionKeyManagerStream", "FunctionKeys"),
        ("MasterPanelManagerStream", "MasterPanels"),
        ("SOManagerData", "SmartObjects"),
        ("LangManagerData", "Texts"),
        ("EmailManager", "Email"),
        ("NativePartStream", "Native"),
        ("ResourceManagerStream", "Resources"),
    ];
    let last = path.rsplit('/').next().unwrap_or(path);
    KNOWN.iter().find(|(s, _)| *s == last).map(|(_, l)| (*l).to_owned()).unwrap_or_else(|| path.split("/Component 1/").last().unwrap_or(path).to_owned())
}

/// Busca los usos en los streams de un target (rutas con `/`, de `swx::all_streams` filtradas al target).
pub fn scan(db: &TagDb, streams: &[(String, Vec<u8>)]) -> Usage {
    let streams: BTreeMap<String, Vec<u8>> = streams.iter().map(|(p, b)| (p.replace('\\', "/"), b.clone())).collect();
    // Nombres que se pueden citar: variables de primer nivel y miembros de las instancias de estructura.
    let rows = db.variables("", true, true);
    let names: BTreeSet<&str> = rows.iter().map(|v| v.name.as_str()).collect();
    let by_id: BTreeMap<u32, &str> = db.vars.iter().map(|v| (v.id, v.name.as_str())).collect();
    let popups = popup_names(&streams);
    let mut u = Usage::default();

    let look = |strings: &[String], here: &str, u: &mut Usage| {
        for s in strings {
            for (kind, id, member, stored) in links(s) {
                match by_id.get(&id) {
                    Some(v) => {
                        let name = match (kind, member) {
                            ('3', Some(m)) => format!("{v}.{m}"),
                            _ => (*v).to_owned(),
                        };
                        let name = if names.contains(name.as_str()) { name } else { (*v).to_owned() };
                        u.references.entry(name).or_default().insert(here.to_owned());
                    }
                    None => {
                        u.broken.insert(Broken { location: here.to_owned(), name: stored, id });
                    }
                }
            }
            let text = s.replace('$', "");
            for token in identifiers(&text) {
                let token = token.strip_prefix("TagDB.").unwrap_or(token);
                let mut cut = token.len();
                loop {
                    let cand = &token[..cut];
                    if let Some(n) = names.get(cand) {
                        u.references.entry((*n).to_owned()).or_default().insert(here.to_owned());
                        break;
                    }
                    match cand.rfind('.') {
                        Some(i) => cut = i,
                        None => break,
                    }
                }
            }
        }
    };

    for (path, b) in &streams {
        if path.ends_with("/TagDatabase") || path.ends_with("/NameServer") || path.ends_with("/WindowObject") || path.ends_with("/StatusFlags") {
            continue;
        }
        if path.ends_with("/GraphicalObject") {
            let Some(panel) = panel_label(path, &streams, &popups) else { continue };
            match PanelObjects::parse(b) {
                Ok(p) => {
                    fn go(n: &Node, prefix: &str, out: &mut Vec<(String, Vec<String>)>) {
                        let here = format!("{prefix} › {} «{}»", n.kind(), n.name());
                        out.push((here.clone(), strings_in(&n.own_bytes(), 0)));
                        for (_, c) in n.children() {
                            go(c, &here, out);
                        }
                    }
                    let mut objs = Vec::new();
                    for (_, n) in &p.objects {
                        go(n, &panel, &mut objs);
                    }
                    for (here, strings) in objs {
                        look(&strings, &here, &mut u);
                    }
                }
                Err(_) => look(&strings_in(b, 0), &panel, &mut u),
            }
            continue;
        }
        let here = match panel_label(path, &streams, &popups) {
            Some(p) if path.contains("/WindowList/") || path.contains("/PopupWindowList/") => p,
            _ => stream_label(path),
        };
        look(&strings_in(b, 0), &here, &mut u);
    }
    // En la propia base: referencias indirectas (`Carpeta.%s.Var` con su parámetro) y alarmas de cada variable.
    for v in &db.vars {
        let here = format!("Variables › Variable «{}»", v.name);
        let texts: Vec<String> = v.props.iter().filter(|p| p.id != prop::NAME && p.id != prop::DESCRIPTION).filter_map(|p| p.text()).collect();
        // Tras las propiedades va el parámetro de una referencia indirecta (como enlace `{T,2:1.<id>,…}` y `TagDB.…`).
        let after = strings_in(db.b.get(v.props_end..v.end).unwrap_or_default(), 0);
        let mut own = Usage::default();
        look(&texts, &here, &mut own);
        let indirect = v.props.iter().any(|p| p.id == prop::INDIRECT);
        look(&after, &if indirect { format!("{here} › Parameter") } else { here.clone() }, &mut own);
        own.references.remove(&v.name);
        for (name, at) in own.references {
            u.references.entry(name).or_default().extend(at);
        }
        u.broken.extend(own.broken);
        let alarms = v.props.iter().filter(|p| p.id == prop::ALARM_MESSAGE).count();
        if alarms > 0 {
            u.references.entry(v.name.clone()).or_default().insert(format!("{here} › Alarm ({alarms})"));
        }
    }
    u
}

/// Nombre de cada grupo de ventanas emergentes (`UniqueNameK` → nombre de la ventana).
fn popup_names(streams: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (p, b) in streams {
        if let Some(rest) = p.split("/PopupWindowList/").nth(1) {
            if let Some((group, "PageListProperties")) = rest.split_once('/') {
                if let Some(name) = strings_in(b, 0).into_iter().next() {
                    out.insert(group.to_owned(), name);
                }
            }
        }
    }
    out
}

/// Enlaces `{T,2:1.<id>,Nombre}` y `{T,3:1.<id>.<miembro>,Miembro}` de un texto: (tipo, id, miembro, nombre guardado).
fn links(s: &str) -> Vec<(char, u32, Option<String>, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find("{T,") {
        rest = &rest[i + 3..];
        let Some(end) = rest.find('}') else { break };
        let body = &rest[..end];
        rest = &rest[end..];
        // `2:1.157,Var` o `3:1.157.2,Miembro`
        let Some((kind, tail)) = body.split_once(":1.") else { continue };
        let Some((ids, stored)) = tail.split_once(',') else { continue };
        let mut parts = ids.split('.');
        let Some(id) = parts.next().and_then(|x| x.parse().ok()) else { continue };
        let k = kind.chars().next().unwrap_or('2');
        let member = (k == '3').then(|| stored.to_owned());
        out.push((k, id, member, stored.to_owned()));
    }
    out
}

/// Sitios que impiden borrar `name`: los de la variable y los de sus miembros, salvo su propia definición (sus alarmas).
pub fn uses_of(u: &Usage, name: &str) -> Vec<String> {
    let own = format!("Variables › Variable «{name}»");
    let member = format!("{name}.");
    u.references
        .iter()
        .filter(|(v, _)| v.as_str() == name || v.starts_with(&member))
        .flat_map(|(_, at)| at.iter())
        .filter(|at| !at.starts_with(&own))
        .cloned()
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLES: &str = r"C:\Program Files (x86)\Schneider Electric\EcoStruxure Machine Expert - Basic\Examples";
    const CORPUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), r"\..\..\docs\reference\vd-6.3\corpus\xml");
    /// (ejemplo de la instalación, su XML exportado por Vijeo en el corpus).
    const PAIRS: [(&str, &str); 4] = [
        ("Compact_Modbus SL_M221_Book", "Compact_Modbus SL_M221 Book"),
        ("Compact_Hardwired_M221", "Compact_Hardwired_M221"),
        ("MaterialWork_Drill_HW_M221", "Material Working_Drill machine_H"),
        ("Packaging_TempCtrl_HW_M221", "Packaging_TempCtrl_HW_M221"),
    ];

    type Streams = Vec<(String, Vec<u8>)>;

    fn open(name: &str) -> Option<(TagDb, Streams)> {
        let p = std::path::Path::new(EXAMPLES).join(format!("{name}.vdz"));
        if !p.is_file() {
            return None;
        }
        let mut c = crate::swx::open_vdz(&p).unwrap();
        let db = TagDb::parse(crate::swx::stream(&mut c, "/Services/TagDatabase").unwrap(), crate::swx::stream(&mut c, "/Services/NameServer").unwrap()).unwrap();
        Some((db, crate::swx::all_streams(&mut c)))
    }

    /// Sitio sin tipos de objeto ni atributo, para comparar con el XML: `Panel «A» › Switch «B» › Bit @Var` → `«A» › «B»`;
    /// las acciones globales y las alarmas, sólo la sección.
    fn normalize(loc: &str) -> String {
        let loc = loc.strip_prefix("GraphicalPanels › ").unwrap_or(loc);
        for section in ["Actions", "Alarms"] {
            if loc == section || loc.starts_with(&format!("{section} ›")) {
                return section.to_owned();
            }
        }
        let loc = loc.split(" @").next().unwrap();
        let mut parts: Vec<&str> = loc.split(" › ").collect();
        while parts.len() > 1 && !parts.last().unwrap().contains('«') && !parts.last().unwrap().starts_with("Alarm (") && *parts.last().unwrap() != "Parameter" {
            parts.pop();
        }
        parts.iter().map(|p| p.find('«').map(|i| &p[i..]).unwrap_or(p)).collect::<Vec<_>>().join(" › ")
    }

    /// Los objetos de todos los paneles de los ejemplos se leen y se vuelven a escribir idénticos.
    #[test]
    fn panel_objects_roundtrip() {
        let mut panels = 0;
        for (name, _) in PAIRS {
            let Some((_, streams)) = open(name) else { return };
            for (path, b) in streams.iter().filter(|(p, _)| p.ends_with("GraphicalObject")) {
                let p = PanelObjects::parse(b).unwrap_or_else(|e| panic!("{path}: {e}"));
                assert_eq!(p.stream(), *b);
                panels += 1;
            }
        }
        assert_eq!(panels, 63);
    }

    /// Los usos que se leen del `.vdz` son los mismos que da el XML que exporta Vijeo, objeto por objeto (sin las
    /// variables del sistema, que el XML no tiene).
    #[test]
    fn matches_the_exported_xml() {
        for (name, xml) in PAIRS {
            let Some((db, streams)) = open(name) else { return };
            let Ok(text) = std::fs::read(std::path::Path::new(CORPUS).join(format!("{xml}.vxml"))) else { return };
            let text = crate::reports::decode_text(&text);
            let doc = crate::vxml::parse(&text).unwrap();
            let mut expected = BTreeSet::new();
            for t in crate::vxml::targets(&doc) {
                for r in crate::vxml::cross_references(t) {
                    expected.extend(r.references.iter().map(|l| (r.variable.clone(), normalize(l))));
                }
            }
            let u = scan(&db, &streams);
            let got: BTreeSet<(String, String)> =
                u.references.iter().filter(|(v, _)| !v.starts_with('_')).flat_map(|(v, ls)| ls.iter().map(|l| (v.clone(), normalize(l)))).collect();
            let missing: Vec<_> = expected.difference(&got).collect();
            let extra: Vec<_> = got.difference(&expected).collect();
            assert!(missing.is_empty() && extra.is_empty(), "{name}: faltan {missing:?}; sobran {extra:?}");
            assert!(u.broken.is_empty(), "{name}: {:?}", u.broken);
            assert!(expected.len() > 20, "{name}: {}", expected.len());
        }
    }

    /// Al borrar de la base una variable que usan las pantallas, sus enlaces quedan rotos y se dicen.
    #[test]
    fn reports_links_to_deleted_variables() {
        let Some((mut db, streams)) = open("Compact_Modbus SL_M221_Book") else { return };
        let before = scan(&db, &streams);
        let gone = "MyController.Energy.reTotalEnergy";
        let at = before.references[gone].clone();
        assert!(at.contains("Panel «Energy» › DataDisplay «TotalEnergy»"), "{at:?}");
        assert_eq!(uses_of(&before, gone), at.iter().cloned().collect::<Vec<_>>());
        let id = db.find_var(gone).unwrap().id;
        db.delete_variable(gone).unwrap();
        let after = scan(&db, &streams);
        assert!(!after.references.contains_key(gone));
        let broken: Vec<&Broken> = after.broken.iter().filter(|b| b.id == id).collect();
        assert!(!broken.is_empty());
        assert!(broken.iter().all(|b| b.name == "reTotalEnergy"), "{broken:?}");
        assert!(broken.iter().any(|b| b.location == "Panel «Energy» › DataDisplay «TotalEnergy»"), "{broken:?}");
    }

    #[test]
    fn reads_links() {
        assert_eq!(links("{F,2:1.56,Carp}.{T,2:1.157,Var}"), vec![('2', 157, None, "Var".to_owned())]);
        assert_eq!(links("{T,3:1.12.2,Miembro}"), vec![('3', 12, Some("Miembro".to_owned()), "Miembro".to_owned())]);
        assert!(links("sin enlaces {T,").is_empty());
    }
}
