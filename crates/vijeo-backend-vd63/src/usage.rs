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
    // Almacén de cada panel → cómo se llama en los sitios (`Folder «F» › Panel «P»`).
    let panels: Vec<(String, String)> = crate::screens::screens(&streams).into_iter().map(|s| (format!("{}/", s.dir), s.label())).collect();
    let panel_of = |path: &str| panels.iter().find(|(d, _)| path.starts_with(d.as_str())).map(|(_, l)| l.clone());
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
            let Some(panel) = panel_of(path) else {
                look(&strings_in(b, 0), &stream_label(path), &mut u);
                continue;
            };
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
        let here = panel_of(path).unwrap_or_else(|| stream_label(path));
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

/// `s` con la variable `id` renombrada de `old` a `new` (nombres completos, misma carpeta): los enlaces
/// `{T,2:1.<id>,Viejo}` y el nombre completo donde aparece como identificador (`TagDB.Carpeta.Viejo`,
/// `Carpeta.Viejo.getIntValue()`, `Carpeta.Viejo.miembro`), no dentro de otro nombre (`Carpeta.Viejo2`).
pub fn rename_text(s: &str, id: u32, old: &str, new: &str) -> Option<String> {
    rename_ident(s, 'T', id, old, new)
}

/// `s` con la carpeta `id` renombrada de `old` a `new` (rutas completas, mismo padre): los enlaces `{F,2:1.<id>,Vieja}`
/// y la ruta al principio de los nombres de lo que contiene (`TagDB.Vieja.Var` → `TagDB.Nueva.Var`).
pub fn rename_folder_text(s: &str, id: u32, old: &str, new: &str) -> Option<String> {
    rename_ident(s, 'F', id, old, new)
}

/// `s` con la variable `id` movida de `old` a `new` (nombres completos, mismo nombre en otra carpeta): la cadena de
/// carpetas de su enlace (`{F,2:1.1,A}.{T,2:1.<id>,Var}` → `{F,2:1.7,B}.{T,2:1.<id>,Var}`, con las cadenas de
/// `TagDb::folder_chain`) y el nombre completo como identificador, como en [`rename_text`].
pub fn move_text(s: &str, id: u32, old: &str, new: &str, old_chain: &str, new_chain: &str) -> Option<String> {
    let leaf = old.rsplit('.').next().unwrap_or(old);
    let link = format!("{{T,2:1.{id},{leaf}}}");
    // El enlace con la cadena vieja delante; sin carpetas (raíz), el enlace solo, sin una `}.` de carpeta justo antes.
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(&link) {
        let (head, tail) = rest.split_at(i);
        if head.ends_with(old_chain) && (!old_chain.is_empty() || !head.ends_with("}.")) {
            out.push_str(&head[..head.len() - old_chain.len()]);
            out.push_str(new_chain);
        } else {
            out.push_str(head);
        }
        out.push_str(&link);
        rest = &tail[link.len()..];
    }
    out.push_str(rest);
    let out = rename_ident(&out, 'T', id, old, new).unwrap_or(out);
    (out != s).then_some(out)
}

fn rename_ident(s: &str, kind: char, id: u32, old: &str, new: &str) -> Option<String> {
    let leaf = |n: &str| n.rsplit('.').next().unwrap_or(n).to_owned();
    let (old_leaf, new_leaf) = (leaf(old), leaf(new));
    let out = s.replace(&format!("{{{kind},2:1.{id},{old_leaf}}}"), &format!("{{{kind},2:1.{id},{new_leaf}}}"));
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut rest = out.as_str();
    let mut before = String::new();
    while let Some(i) = rest.find(old) {
        let (head, tail) = rest.split_at(i);
        before.push_str(head);
        let prev = before.chars().last();
        let after = tail[old.len()..].chars().next();
        // Dentro de un enlace (`{T,2:1.9,xDone}`) va sólo el nombre corto: lo cambia el reemplazo del enlace, no este.
        let open = before.rfind('{').filter(|&i| before.rfind('}') < Some(i));
        let in_link = open.is_some_and(|i| before[i + 1..].starts_with("T,") || before[i + 1..].starts_with("F,"));
        let starts = !in_link
            && match prev {
                None => true,
                Some('.') => before.ends_with("TagDB."),
                Some(c) => !ident(c),
            };
        // Una variable acaba ahí (o sigue un método o un miembro); una carpeta siempre va seguida de lo que contiene.
        let ends = if kind == 'F' { after == Some('.') } else { after.is_none_or(|c| !ident(c)) };
        if starts && ends {
            before.push_str(new);
        } else {
            before.push_str(old);
        }
        rest = &tail[old.len()..];
    }
    before.push_str(rest);
    (before != s).then_some(before)
}

/// Enlaces a la variable `id` que guardan otro nombre que `leaf` (sitio y nombre guardado). El análisis de uso los da por
/// buenos (resuelve por id), pero tras renombrar o mover tienen que llevar el nombre nuevo.
pub fn stale_links(streams: &[(String, Vec<u8>)], id: u32, leaf: &str) -> Vec<String> {
    streams
        .iter()
        .filter(|(p, _)| !p.ends_with("TagDatabase") && !p.ends_with("NameServer"))
        .flat_map(|(p, b)| strings_in(b, 0).into_iter().flat_map(|s| links(&s)).filter(|l| l.0 == '2' && l.1 == id && l.3 != leaf).map(move |l| format!("{p}: {}", l.3)))
        .collect()
}

/// Streams que no se reescriben al renombrar: los textos que ve el usuario y las cachés de Vijeo.
const RENAME_SKIP: [&str; 5] = ["/TagDatabase", "/NameServer", "/LangManagerData", "/StatusFlags", "/WindowObject"];

/// Aplica `f` (un renombrado) a los streams del target (`GraphicalObject` objeto por objeto; el resto, sus cadenas).
/// Devuelve las rutas de los streams que cambiaron.
pub fn rename_in_streams(streams: &mut [(String, Vec<u8>)], f: &dyn Fn(&str) -> Option<String>) -> Result<Vec<String>, String> {
    let mut changed = Vec::new();
    for (path, b) in streams.iter_mut() {
        let p = path.replace('\\', "/");
        if RENAME_SKIP.iter().any(|x| p.ends_with(x)) {
            continue;
        }
        let n = if p.ends_with("/GraphicalObject") {
            match PanelObjects::parse(b) {
                Ok(mut panel) => {
                    let mut n = 0;
                    for (_, o) in &mut panel.objects {
                        n += o.edit_all_strings(f).map_err(|e| format!("{path}: {e}"))?;
                    }
                    if n > 0 {
                        *b = panel.stream();
                    }
                    n
                }
                Err(e) => return Err(format!("{path}: {e}")),
            }
        } else {
            crate::objects::edit_strings_in(b, 0, f).map_err(|e| format!("{path}: {e}"))?
        };
        if n > 0 {
            changed.push(path.clone());
        }
    }
    Ok(changed)
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
    fn renames_whole_identifiers_only() {
        let r = |s: &str| rename_text(s, 157, "Carp.Var", "Carp.Nueva");
        assert_eq!(r("{F,2:1.56,Carp}.{T,2:1.157,Var}").as_deref(), Some("{F,2:1.56,Carp}.{T,2:1.157,Nueva}"));
        assert_eq!(r("{F,2:1.56,Carp}.{T,2:1.158,Var}"), None, "otro id");
        assert_eq!(r("(TagDB.Carp.Var.getIntValue(0) != 0)").as_deref(), Some("(TagDB.Carp.Nueva.getIntValue(0) != 0)"));
        assert_eq!(r("Carp.Var\n").as_deref(), Some("Carp.Nueva\n"));
        assert_eq!(r("x = Carp.Var2 + Otra.Carp.Var + Carp.VarX;"), None);
        assert_eq!(r("Carp.Var.miembro; Carp.Var").as_deref(), Some("Carp.Nueva.miembro; Carp.Nueva"));
        let root = |s: &str| rename_text(s, 9, "xDone", "xHecho");
        assert_eq!(root("!xDone").as_deref(), Some("!xHecho"));
        assert_eq!(root("TagDB.xDone; A.xDone; xDoneX"), Some("TagDB.xHecho; A.xDone; xDoneX".to_owned()));
    }

    /// Renombrar en el ejemplo Modbus una variable de pantalla y otra que usan una acción global y su script: los mismos
    /// sitios con el nombre nuevo, ninguno con el viejo y los paneles siguen leyéndose.
    #[test]
    fn renames_in_panels_and_actions() {
        let Some((mut db, mut streams)) = open("Compact_Modbus SL_M221_Book") else { return };
        for (old, leaf) in [("MyController.Energy.reTotalEnergy", "reEnergiaTotal"), ("MyController.DateAndTime.iGetRTCSec", "iSegundosRTC"), ("xRTCSyncDone", "xSincronizado")] {
            let before = uses_of(&scan(&db, &streams), old);
            assert!(!before.is_empty(), "{old}");
            let id = db.find_var(old).unwrap().id;
            let new = db.rename_variable(old, leaf).unwrap();
            let changed = rename_in_streams(&mut streams, &|s| rename_text(s, id, old, &new)).unwrap();
            assert!(!changed.is_empty(), "{old}");
            let after = scan(&db, &streams);
            assert_eq!(uses_of(&after, &new), before, "{old}");
            assert!(uses_of(&after, old).is_empty() && after.broken.is_empty(), "{old}: {:?}", after.broken);
            assert!(stale_links(&streams, id, leaf).is_empty(), "{old}");
        }
        let actions = streams.iter().find(|(p, _)| p.ends_with("ActionsManagerStream")).unwrap();
        let texts = crate::objects::strings_in(&actions.1, 0).join("\n");
        assert!(texts.contains("{T,2:1.112,iSegundosRTC}") && texts.contains("xSincronizado") && !texts.contains("iGetRTCSec"), "{texts}");
        for (p, b) in streams.iter().filter(|(p, _)| p.ends_with("GraphicalObject")) {
            PanelObjects::parse(b).unwrap_or_else(|e| panic!("{p}: {e}"));
        }
        assert!(db.rename_variable("MyController.Energy.reEnergiaTotal", "iTimeSec").unwrap_err().contains("ya hay"));
        assert!(db.rename_variable("MyController.Energy.reEnergiaTotal", "A.b").unwrap_err().contains("sin carpetas"));
    }

    #[test]
    fn renames_folder_prefixes_only() {
        let r = |s: &str| rename_folder_text(s, 4, "Ctl.Sys", "Ctl.Sistema");
        assert_eq!(r("{F,2:1.1,Ctl}.{F,2:1.4,Sys}.{T,2:1.124,xPulse}").as_deref(), Some("{F,2:1.1,Ctl}.{F,2:1.4,Sistema}.{T,2:1.124,xPulse}"));
        assert_eq!(r("(TagDB.Ctl.Sys.xPulse.getIntValue(0) != 0)").as_deref(), Some("(TagDB.Ctl.Sistema.xPulse.getIntValue(0) != 0)"));
        assert_eq!(r("Ctl.Sys2.x; Ctl.Sys; A.Ctl.Sys.x"), None, "otra carpeta, la carpeta sola, otra ruta");
        assert_eq!(r("{F,2:1.5,Sys}"), None, "otro id");
    }

    /// Renombrar en el ejemplo Modbus una subcarpeta y la carpeta raíz de casi todo: cada variable de dentro conserva sus
    /// usos con su nombre nuevo.
    #[test]
    fn renames_folders_in_panels_and_actions() {
        let Some((mut db, mut streams)) = open("Compact_Modbus SL_M221_Book") else { return };
        for (old, leaf) in [("MyController.Energy", "Energia"), ("MyController", "Controlador")] {
            let pre = format!("{old}.");
            let vars: Vec<String> = db.vars.iter().filter(|v| v.name.starts_with(&pre)).map(|v| v.name.clone()).collect();
            let before = scan(&db, &streams);
            let uses: Vec<Vec<String>> = vars.iter().map(|n| uses_of(&before, n)).collect();
            assert!(uses.iter().any(|u| !u.is_empty()), "{old}");
            let id = db.find_folder(old).unwrap().id;
            let new = db.rename_folder(old, leaf).unwrap();
            rename_in_streams(&mut streams, &|s| rename_folder_text(s, id, old, &new)).unwrap();
            let after = scan(&db, &streams);
            for (n, u) in vars.iter().zip(&uses) {
                let moved = format!("{new}{}", &n[old.len()..]);
                assert_eq!(&uses_of(&after, &moved), u, "{n} → {moved}");
                assert!(uses_of(&after, n).is_empty(), "{n}");
            }
            assert!(after.broken.is_empty());
        }
        let actions = streams.iter().find(|(p, _)| p.ends_with("ActionsManagerStream")).unwrap();
        let texts = crate::objects::strings_in(&actions.1, 0).join("\n");
        assert!(texts.contains("TagDB.Controlador.DateAndTime.iGetRTCSec") && !texts.contains("MyController"), "{texts}");
        assert!(db.rename_folder("Controlador", "Alarms").is_ok(), "no choca con la carpeta Controlador.Alarms, que es de otro nivel");
        assert!(db.rename_folder("NoExiste", "x").unwrap_err().contains("no hay"));
        assert!(db.rename_folder("Alarms.Alarms", "Energia").unwrap_err().contains("ya hay"), "hermana con ese nombre");
    }

    #[test]
    fn moves_the_folder_chain_of_links() {
        let (a, b) = ("{F,2:1.1,Ctl}.{F,2:1.4,Sys}.", "{F,2:1.9,Otra}.");
        let m = |s: &str| move_text(s, 124, "Ctl.Sys.xPulse", "Otra.xPulse", a, b);
        assert_eq!(m("${F,2:1.1,Ctl}.{F,2:1.4,Sys}.{T,2:1.124,xPulse}$").as_deref(), Some("${F,2:1.9,Otra}.{T,2:1.124,xPulse}$"));
        assert_eq!(m("(TagDB.Ctl.Sys.xPulse.getIntValue(0) != 0)").as_deref(), Some("(TagDB.Otra.xPulse.getIntValue(0) != 0)"));
        assert_eq!(m("{F,2:1.1,Ctl}.{F,2:1.4,Sys}.{T,2:1.125,xPulse}"), None, "otro id");
        // De la raíz a una carpeta y de una carpeta a la raíz.
        assert_eq!(move_text("!{T,2:1.9,xDone}", 9, "xDone", "A.xDone", "", "{F,2:1.3,A}.").as_deref(), Some("!{F,2:1.3,A}.{T,2:1.9,xDone}"));
        assert_eq!(move_text("{F,2:1.3,A}.{T,2:1.9,xDone}; TagDB.A.xDone", 9, "A.xDone", "xDone", "{F,2:1.3,A}.", "").as_deref(), Some("{T,2:1.9,xDone}; TagDB.xDone"));
    }

    /// Mover en el ejemplo Modbus: entre dos carpetas, de la raíz a una carpeta nueva y de una carpeta a la raíz.
    #[test]
    fn moves_variables_in_panels_and_actions() {
        let Some((mut db, mut streams)) = open("Compact_Modbus SL_M221_Book") else { return };
        for (old, to) in [("MyController.Energy.reTotalEnergy", "MyController.DateAndTime"), ("xRTCSyncDone", "Mcp.Sub"), ("MyController.DateAndTime.iGetRTCSec", "")] {
            let before = uses_of(&scan(&db, &streams), old);
            assert!(!before.is_empty(), "{old}");
            let id = db.find_var(old).unwrap().id;
            let from = old.rsplit_once('.').and_then(|(p, _)| db.find_folder(p)).map(|f| f.id).unwrap_or(0);
            let old_chain = db.folder_chain(from);
            let new = db.move_variable(old, to).unwrap();
            let dest = new.rsplit_once('.').and_then(|(p, _)| db.find_folder(p)).map(|f| f.id).unwrap_or(0);
            let new_chain = db.folder_chain(dest);
            rename_in_streams(&mut streams, &|s| move_text(s, id, old, &new, &old_chain, &new_chain)).unwrap();
            let after = scan(&db, &streams);
            assert_eq!(uses_of(&after, &new), before, "{old} → {new}");
            assert!(uses_of(&after, old).is_empty() && after.broken.is_empty(), "{old}: {:?}", after.broken);
            assert!(stale_links(&streams, id, new.rsplit('.').next().unwrap()).is_empty(), "{old}");
            assert_eq!(db.find_var(&new).unwrap().id, id);
        }
        assert!(db.find_var("Mcp.Sub.xRTCSyncDone").is_some() && db.find_var("iGetRTCSec").is_some());
        assert!(db.move_variable("Mcp.Sub.xRTCSyncDone", "Mcp.Sub").unwrap_err().contains("ya está"));
        assert!(db.move_variable("MyController.DateAndTime.reTotalEnergy", "MyController.Energy").is_ok());
        assert!(db.move_variable("iGetRTCSec", "MyController.Energy.reTotalEnergy").unwrap_err().contains("no una carpeta"));
    }

    #[test]
    fn reads_links() {
        assert_eq!(links("{F,2:1.56,Carp}.{T,2:1.157,Var}"), vec![('2', 157, None, "Var".to_owned())]);
        assert_eq!(links("{T,3:1.12.2,Miembro}"), vec![('3', 12, Some("Miembro".to_owned()), "Miembro".to_owned())]);
        assert!(links("sin enlaces {T,").is_empty());
    }
}
