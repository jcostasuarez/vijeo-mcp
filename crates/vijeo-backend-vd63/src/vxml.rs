//! Lectura del XML público de un proyecto (`.vxml` de Export Project, public.xsd): variables, paneles,
//! acciones y referencias cruzadas. Sustituye a los informes HTML de `GenerateAllReports`, que sin COM
//! Vijeo sólo muestra en su propia ventana.
//!
//! Estructura (6.3.0.103): `<Project Name>` → `<Target Name Model Type>` → `Variables` (`Folder` anidadas y
//! `Variable Name Type Source DeviceAddress…` con `<Description>`), `Actions/Action` (`Trigger` + operaciones:
//! `Script`, `Bit`, `Decision`…) y `GraphicalPanels` (`BasePanels/Panel`, `MasterPanels/MasterPanel`,
//! `PopupWindows/Window`, con `Folder` posibles y `Actions` propias en cada panel).

use std::collections::{BTreeMap, BTreeSet};

use roxmltree::{Document, Node};
use vijeo_core::{ActionInfo, CrossReference, PanelReport, PanelRef, VariableInfo, VijeoError, VijeoResult};

pub fn parse(text: &str) -> VijeoResult<Document<'_>> {
    Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() })
        .map_err(|e| VijeoError::Internal(format!("el XML exportado por Vijeo no es válido: {e}")))
}

fn elements<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(|c| c.is_element())
}

fn child<'a, 'i>(n: Node<'a, 'i>, tag: &str) -> Option<Node<'a, 'i>> {
    elements(n).find(|c| c.has_tag_name(tag))
}

fn attr(n: Node, name: &str) -> Option<String> {
    n.attribute(name).map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned)
}

fn text_of(n: Node) -> Option<String> {
    let t: String = n.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect();
    let t = t.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// Nombre del proyecto (`<Project Name>`).
pub fn project_name(doc: &Document) -> Option<String> {
    attr(doc.root_element(), "Name")
}

/// Targets del proyecto, en el orden del XML.
pub fn targets<'a, 'i>(doc: &'a Document<'i>) -> Vec<Node<'a, 'i>> {
    let root = doc.root_element();
    // Algunos XML agrupan los targets en <Targets>.
    let holder = child(root, "Targets").unwrap_or(root);
    elements(holder).filter(|n| n.has_tag_name("Target")).collect()
}

pub fn target_name(t: Node) -> String {
    attr(t, "Name").unwrap_or_default()
}

/// Targets elegidos: todos o el de nombre `wanted` (sin distinguir mayúsculas).
pub fn select_targets<'a, 'i>(doc: &'a Document<'i>, wanted: Option<&str>) -> VijeoResult<Vec<Node<'a, 'i>>> {
    let all = targets(doc);
    let Some(w) = wanted else { return Ok(all) };
    let found: Vec<_> = all.into_iter().filter(|t| target_name(*t).eq_ignore_ascii_case(w)).collect();
    if found.is_empty() {
        return Err(VijeoError::TargetNotFound(w.to_owned()));
    }
    Ok(found)
}

// ---------- variables ----------

/// Variables del target con el nombre completo (`Carpeta.Subcarpeta.Variable`, como en los scripts).
pub fn variables(t: Node) -> Vec<VariableInfo> {
    let target = target_name(t);
    let mut out = Vec::new();
    if let Some(vars) = child(t, "Variables") {
        collect_variables(vars, "", &target, &mut out);
    }
    out
}

/// Variables del target con su tipo para el compilador de scripts (`crate::script`): las simples (`BOOL`, `INTEGER`,
/// `REAL`, `STRING`), las instancias de estructura (`BaseTag`) y sus miembros con el tipo de `<DataTypes>`. Las de otros
/// tipos no se dan: un script que las use no compila.
pub fn script_symbols(t: Node) -> Vec<(String, String)> {
    let mut structs: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let root = t.ancestors().find(|a| a.has_tag_name("Project"));
    if let Some(dt) = root.and_then(|r| child(r, "DataTypes")) {
        for s in elements(dt).filter(|s| s.has_tag_name("StructureType")) {
            let members = elements(s)
                .filter(|e| e.has_tag_name("StructureElement"))
                .filter_map(|e| Some((attr(e, "Name")?, attr(e, "Type").unwrap_or_default())))
                .collect();
            if let Some(n) = attr(s, "Name") {
                structs.insert(n, members);
            }
        }
    }
    let mut out = Vec::new();
    for v in variables(t) {
        if let Some(ms) = structs.get(&v.data_type) {
            out.push((v.name.clone(), "BaseTag".to_owned()));
            out.extend(ms.iter().filter_map(|(m, ty)| Some((format!("{}.{m}", v.name), crate::script::symbol_type(ty)?.to_owned()))));
        } else if let Some(ty) = crate::script::symbol_type(&v.data_type) {
            out.push((v.name, ty.to_owned()));
        }
    }
    out
}

/// Nombres que se pueden usar como variable en un objeto: las variables del target y los miembros de las
/// estructuradas (`Var.miembro`), tanto los que van dentro de la variable como los de su tipo en `<DataTypes>`.
pub fn variable_names(t: Node) -> Vec<String> {
    let mut structs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let root = t.ancestors().find(|a| a.has_tag_name("Project"));
    if let Some(dt) = root.and_then(|r| child(r, "DataTypes")) {
        for s in elements(dt).filter(|s| s.has_tag_name("StructureType")) {
            let members = elements(s).filter(|e| e.has_tag_name("StructureElement")).filter_map(|e| attr(e, "Name")).collect();
            if let Some(n) = attr(s, "Name") {
                structs.insert(n, members);
            }
        }
    }
    let mut out = Vec::new();
    for v in variables(t) {
        if let Some(ms) = v.extra.get("Members") {
            out.extend(ms.split(", ").map(|m| format!("{}.{m}", v.name)));
        }
        if let Some(ms) = structs.get(&v.data_type) {
            out.extend(ms.iter().map(|m| format!("{}.{m}", v.name)));
        }
        out.push(v.name);
    }
    out
}

fn collect_variables(n: Node, prefix: &str, target: &str, out: &mut Vec<VariableInfo>) {
    for c in elements(n) {
        let Some(name) = attr(c, "Name") else { continue };
        let full = if prefix.is_empty() { name } else { format!("{prefix}.{name}") };
        if c.has_tag_name("Folder") {
            collect_variables(c, &full, target, out);
        } else if c.has_tag_name("Variable") {
            out.push(variable(c, full, target));
        }
    }
}

/// Atributos que tienen campo propio o que no aportan (valores por defecto).
const VARIABLE_FIELDS: [&str; 5] = ["Name", "Type", "Source", "DeviceAddress", "LogUserOperations"];

fn variable(v: Node, name: String, target: &str) -> VariableInfo {
    let mut extra = BTreeMap::new();
    for a in v.attributes() {
        let (k, val) = (a.name(), a.value().trim());
        let default = matches!((k, val), ("Sharing", "None") | ("Retentive", "Disabled") | (_, ""));
        if !VARIABLE_FIELDS.contains(&k) && !default {
            extra.insert(k.to_owned(), val.to_owned());
        }
    }
    // Variables estructuradas (p.ej. `Authorization`): sus miembros son <Variable> sin tipo.
    let members: Vec<String> = elements(v).filter(|c| c.has_tag_name("Variable")).filter_map(|c| attr(c, "Name")).collect();
    if !members.is_empty() {
        extra.insert("Members".into(), members.join(", "));
    }
    let refs: Vec<String> = elements(v).filter(|c| c.has_tag_name("Reference")).filter_map(|c| attr(c, "ReferenceString")).collect();
    if !refs.is_empty() {
        extra.insert("Reference".into(), refs.join("; "));
    }
    let alarms = elements(v).filter(|c| c.has_tag_name("Alarm")).count();
    if alarms > 0 {
        extra.insert("Alarms".into(), alarms.to_string());
    }
    VariableInfo {
        target: target.to_owned(),
        name,
        data_type: attr(v, "Type").unwrap_or_default(),
        source: attr(v, "Source").unwrap_or_default(),
        address: attr(v, "DeviceAddress"),
        description: child(v, "Description").and_then(text_of),
        extra,
    }
}

// ---------- acciones ----------

/// Acciones globales del target.
pub fn global_actions(t: Node) -> Vec<ActionInfo> {
    actions_of(t, &target_name(t))
}

fn actions_of(n: Node, target: &str) -> Vec<ActionInfo> {
    let Some(list) = child(n, "Actions") else { return Vec::new() };
    elements(list).filter(|a| a.has_tag_name("Action")).enumerate().map(|(i, a)| action(a, i as u32 + 1, target)).collect()
}

fn action(a: Node, index: u32, target: &str) -> ActionInfo {
    let trigger = child(a, "Trigger");
    let trigger_text = trigger
        .map(|t| {
            let kind = attr(t, "TriggerType").unwrap_or_default();
            let when = attr(t, "TriggerCondition").or_else(|| attr(t, "TriggerEvent"));
            let mut s = match when {
                Some(w) if !kind.is_empty() => format!("{kind} - {w}"),
                Some(w) => w,
                None => kind,
            };
            if let Some(v) = attr(t, "TriggerVariable") {
                s.push_str(&format!(", {v}"));
            }
            s
        })
        .unwrap_or_default();
    let ops: Vec<Node> = elements(a).filter(|c| !c.has_tag_name("Trigger")).collect();
    let kind = (!ops.is_empty()).then(|| ops.iter().map(|o| o.tag_name().name()).collect::<Vec<_>>().join(", "));
    let body = (!ops.is_empty()).then(|| ops.iter().map(|o| operation(*o)).collect::<Vec<_>>().join("\n"));
    ActionInfo {
        target: target.to_owned(),
        index: Some(index),
        trigger: trigger_text,
        description: trigger.and_then(|t| attr(t, "Description")),
        interlock: attr(a, "InterlockVariable"),
        kind,
        body,
    }
}

/// Texto de una operación: el código si es un script; si no, `Etiqueta(atributo=valor, …)` con sus hijas.
fn operation(o: Node) -> String {
    if o.has_tag_name("Script") {
        return o.text().map(|s| s.trim_end().to_owned()).unwrap_or_default();
    }
    let attrs: Vec<String> = o.attributes().filter(|a| a.name() != "InternalID").map(|a| format!("{}={}", a.name(), a.value())).collect();
    let mut s = format!("{}({})", o.tag_name().name(), attrs.join(", "));
    let kids: Vec<String> = elements(o).map(operation).collect();
    if !kids.is_empty() {
        s.push_str(&format!(" {{ {} }}", kids.join("; ")));
    }
    s
}

// ---------- paneles ----------

/// (contenedor, elemento, grupo con el nombre del informe de Vijeo, prefijo de `vdz_panel_list`).
const PANEL_GROUPS: [(&str, &str, &str, Option<&str>); 3] = [
    ("BasePanels", "Panel", "Base Panels", Some("BP:")),
    ("MasterPanels", "MasterPanel", "Master Panels", None),
    ("PopupWindows", "Window", "Popup Windows", None),
];

/// Paneles base, maestros y ventanas emergentes con sus acciones de panel.
pub fn panels(t: Node) -> Vec<PanelReport> {
    let target = target_name(t);
    let mut out = Vec::new();
    let Some(gp) = child(t, "GraphicalPanels") else { return out };
    for (holder, tag, group, prefix) in PANEL_GROUPS {
        if let Some(h) = child(gp, holder) {
            collect_panels(h, tag, "", &mut |path, p| {
                out.push(PanelReport {
                    target: target.clone(),
                    group: group.to_owned(),
                    name: attr(p, "Name").unwrap_or_default(),
                    path: path.to_owned(),
                    panel_ref: prefix.map(|x| format!("{x}\\{path}")),
                    actions: actions_of(p, &target),
                })
            });
        }
    }
    out
}

fn collect_panels<'a, 'i>(n: Node<'a, 'i>, tag: &str, prefix: &str, f: &mut dyn FnMut(&str, Node<'a, 'i>)) {
    for c in elements(n) {
        let Some(name) = attr(c, "Name") else { continue };
        let path = if prefix.is_empty() { name } else { format!("{prefix}\\{name}") };
        // Vijeo exporta las carpetas de pantallas como `<PanelFolder>` (comprobado con `live_offline_panel_list`).
        if c.tag_name().name().ends_with("Folder") {
            collect_panels(c, tag, &path, f);
        } else if c.has_tag_name(tag) {
            f(&path, c);
        }
    }
}

/// Panel con su id interno (`ID="P2"` en el XML), que es lo que guardan las acciones Change Panel y las de las
/// ventanas emergentes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelId {
    /// `Base Panels`, `Master Panels` o `Popup Windows`.
    pub group: String,
    /// Ruta dentro del grupo (`Carpeta\Panel`).
    pub path: String,
    pub name: String,
    /// `P2`, `P10001`…
    pub id: String,
    /// Ventanas emergentes: nombre de la ventana (`<Window Name=…>`) que contiene el panel; vacío en el resto.
    pub window: String,
}

/// Paneles base, maestros y ventanas emergentes con su id. Una ventana emergente (`<Window WindowID="W1">`) no
/// tiene id de panel: lo tienen sus paneles (`<Panels><Panel ID="P10001">`), que son los que abren y cierran las acciones;
/// su ruta es `Carpeta\Ventana\Panel`.
pub fn panel_ids(t: Node) -> Vec<PanelId> {
    let mut out = Vec::new();
    let Some(gp) = child(t, "GraphicalPanels") else { return out };
    for (holder, tag, group, _) in PANEL_GROUPS {
        if let Some(h) = child(gp, holder) {
            collect_panels(h, tag, "", &mut |path, p| {
                let name = attr(p, "Name").unwrap_or_default();
                if let Some(id) = attr(p, "ID") {
                    out.push(PanelId { group: group.to_owned(), path: path.to_owned(), name, id, window: String::new() });
                } else if tag == "Window" {
                    // <Window><Panels><Panel ID="P10001">…
                    for c in child(p, "Panels").into_iter().flat_map(elements).filter(|c| c.has_tag_name("Panel")) {
                        if let (Some(id), Some(pn)) = (attr(c, "ID"), attr(c, "Name")) {
                            out.push(PanelId { group: group.to_owned(), path: format!("{path}\\{pn}"), name: pn, id, window: name.clone() });
                        }
                    }
                }
            });
        }
    }
    out
}
/// Pantalla que se puede renombrar o borrar desde el Navigator: un panel base o una ventana emergente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub popup: bool,
    /// Ruta dentro del grupo (`Carpeta\Panel`; en los popups, `Carpeta\Ventana`).
    pub path: String,
    pub name: String,
    /// Número del Navigator (`21: RFID_1`, `1: ComErrorCodes`): PanelID de los paneles base, WindowID de los popups.
    pub number: u32,
    /// Ids que guardan las acciones (`P21`; en los popups, los de sus paneles: `P10001`).
    pub panel_ids: Vec<String>,
}

/// Paneles base y ventanas emergentes del target, con sus ids.
pub fn screens(t: Node) -> Vec<Screen> {
    let mut out = Vec::new();
    let Some(gp) = child(t, "GraphicalPanels") else { return out };
    let num = |s: Option<String>, prefix: char| s.and_then(|s| s.strip_prefix(prefix).and_then(|n| n.parse().ok()));
    if let Some(h) = child(gp, "BasePanels") {
        collect_panels(h, "Panel", "", &mut |path, p| {
            if let (Some(id), Some(number)) = (attr(p, "ID"), num(attr(p, "ID"), 'P')) {
                out.push(Screen { popup: false, path: path.to_owned(), name: attr(p, "Name").unwrap_or_default(), number, panel_ids: vec![id] });
            }
        });
    }
    if let Some(h) = child(gp, "PopupWindows") {
        collect_panels(h, "Window", "", &mut |path, w| {
            if let Some(number) = num(attr(w, "WindowID"), 'W') {
                let panel_ids = child(w, "Panels").into_iter().flat_map(elements).filter(|c| c.has_tag_name("Panel")).filter_map(|c| attr(c, "ID")).collect();
                out.push(Screen { popup: true, path: path.to_owned(), name: attr(w, "Name").unwrap_or_default(), number, panel_ids });
            }
        });
    }
    out
}

/// Dónde se usa alguno de los ids de panel `ids` (`P21`): atributos del target con ese valor exacto
/// (`ChangePanelID`, `PopupPanelID`, `InitialPanelID`, seguridad…) fuera de los propios paneles: lo que está dentro
/// (el botón del popup que lo cierra) se borra con ellos.
pub fn panel_id_references(t: Node, ids: &[String]) -> Vec<String> {
    let own = |n: &Node| n.ancestors().any(|a| a.has_tag_name("Panel") && a.attribute("ID").is_some_and(|id| ids.iter().any(|x| x == id)));
    let mut out = Vec::new();
    for n in t.descendants().filter(|n| n.is_element() && !own(n)) {
        for a in n.attributes() {
            if a.name() != "ID" && ids.iter().any(|id| id == a.value()) {
                out.push(format!("{} @{}", location(n), a.name()));
            }
        }
    }
    out
}


/// Rutas de los paneles base (`BP:\Panel`, `BP:\Carpeta\Panel`).
pub fn base_panel_refs(t: Node) -> Vec<PanelRef> {
    panels(t).into_iter().filter_map(|p| p.panel_ref).map(|path| PanelRef { path }).collect()
}

// ---------- referencias cruzadas ----------

/// Atributos que nunca son referencias a variables.
const NOT_REFERENCES: [&str; 6] = ["Name", "ID", "InternalID", "Type", "Description", "ResourceID"];

/// Dónde aparece cada variable del target: en atributos y textos (scripts, expresiones) del resto del
/// XML, como identificador completo (`Carpeta.Variable`, seguido o no de `.método()`).
pub fn cross_references(t: Node) -> Vec<CrossReference> {
    let target = target_name(t);
    let vars: Vec<VariableInfo> = variables(t);
    let names: BTreeSet<&str> = vars.iter().map(|v| v.name.as_str()).collect();
    let mut refs: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut add = |text: &str, here: &dyn Fn() -> String| {
        // `$Carpeta.Sub$.miembro` (placeholder de los objetos) es `Carpeta.Sub.miembro`. Las plantillas
        // `Carpeta.%s.miembro` de las referencias se resuelven en runtime y no se cuentan.
        let text = text.replace('$', "");
        for token in identifiers(&text) {
            // `Carpeta.Var.getIntValue` → la variable es el prefijo más largo que exista.
            let mut cut = token.len();
            loop {
                let cand = &token[..cut];
                if let Some(n) = names.get(cand) {
                    refs.entry(n).or_default().insert(here());
                    break;
                }
                match cand.rfind('.') {
                    Some(i) => cut = i,
                    None => break,
                }
            }
        }
    };
    for n in t.descendants().filter(|n| n.is_element()) {
        // El nombre de la propia variable o carpeta no es un uso.
        let own_definition = n.has_tag_name("Variable") || n.has_tag_name("Folder");
        for a in n.attributes() {
            if NOT_REFERENCES.contains(&a.name()) || (own_definition && a.name() == "Name") {
                continue;
            }
            add(a.value(), &|| format!("{} @{}", location(n), a.name()));
        }
        for txt in n.children().filter(|c| c.is_text()) {
            if let Some(s) = txt.text() {
                add(s, &|| location(n));
            }
        }
    }
    vars.iter()
        .map(|v| {
            let mut references: Vec<String> = refs.get(v.name.as_str()).map(|s| s.iter().cloned().collect()).unwrap_or_default();
            if let Some(n) = v.extra.get("Alarms") {
                references.push(format!("Variables › Variable «{}» › Alarm ({n})", v.name));
            }
            CrossReference { target: target.clone(), variable: v.name.clone(), references }
        })
        .collect()
}

// ---------- scripts ----------

/// Scripts del target (acciones globales y de panel, objetos, alarmas…): (dónde, `ScriptType`, código).
pub fn scripts(t: Node) -> Vec<(String, Option<String>, String)> {
    t.descendants()
        .filter(|n| n.is_element() && n.has_tag_name("Script"))
        .map(|n| (location(n), attr(n, "ScriptType"), n.text().unwrap_or_default().to_owned()))
        .collect()
}

/// Variables (con los miembros de las estructuras) y nombres del resto de objetos del target, para
/// revisar sus scripts.
pub fn lint_context(t: Node) -> crate::lint::Context {
    let mut ctx = crate::lint::Context::default();
    for v in variables(t) {
        if let Some(members) = v.extra.get("Members") {
            for m in members.split(", ") {
                ctx.add_variable(&format!("{}.{m}", v.name), "");
            }
        }
        ctx.add_variable(&v.name, &v.data_type);
    }
    let in_variables = |n: Node| n.ancestors().any(|a| a.has_tag_name("Variables"));
    for n in t.descendants().filter(|n| n.is_element() && !in_variables(*n)) {
        if let Some(name) = attr(n, "Name") {
            ctx.add_object(&name);
        }
    }
    ctx
}

/// Nombre del primer idioma del proyecto (`<UserLanguage Name="Español" UserLanguageID="L1">`).
pub fn first_language(doc: &Document) -> Option<String> {
    let langs: Vec<Node> = doc.descendants().filter(|n| n.has_tag_name("UserLanguage")).collect();
    langs.iter().find(|n| n.attribute("UserLanguageID") == Some("L1")).or(langs.first()).and_then(|n| attr(*n, "Name"))
}

/// Identificadores con puntos (`a.b.c`) de un texto.
pub(crate) fn identifiers(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).map(|t| t.trim_matches('.')).filter(|t| t.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_'))
}

/// Ruta legible de un elemento dentro de su target: sección y elementos con nombre
/// (`GraphicalPanels › Panel «Start» › Switch «Switch1»`); las acciones sin nombre van numeradas.
fn location(n: Node) -> String {
    let mut chain: Vec<Node> = n.ancestors().filter(|a| a.is_element()).take_while(|a| !a.has_tag_name("Target")).collect();
    chain.reverse();
    let mut parts = Vec::new();
    for (i, a) in chain.iter().enumerate() {
        let tag = a.tag_name().name();
        if let Some(name) = attr(*a, "Name") {
            parts.push(format!("{tag} «{name}»"));
        } else if tag == "Action" {
            // `prev_siblings` empieza por el propio nodo.
            let k = a.prev_siblings().filter(|s| s.has_tag_name("Action")).count();
            parts.push(format!("Action {k}"));
        } else if i == 0 || i + 1 == chain.len() {
            parts.push(tag.to_owned());
        }
    }
    parts.join(" › ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0"?>
<Project Name="Demo" Version="6.3.0.103">
  <Target ID="T1" Model="HMIGTO6310/6315" Name="HMI1" Type="HMIGTO Series">
    <Actions>
      <Action PublishTo="HMIRuntime" InterlockVariable="!Done">
        <Trigger TriggerType="Conditional" TriggerCondition="OnDataChange" TriggerVariable="Ctl.Sec" Description="Sync"></Trigger>
        <Script ScriptType="GlobalActionScript" InternalID="1">Sys.setTime(Ctl.Sec.getIntValue());
</Script>
        <Bit BitOperation="BitSet" DestinationVariable="Done" InternalID="2"></Bit>
      </Action>
    </Actions>
    <GraphicalPanels>
      <BasePanels>
        <Panel Name="Start" ID="P1"><Switch Name="Switch1"><Animation Variable="Ctl.Run"/><Bit DestinationVariable="$Ctl$.Alm"/></Switch>
          <Actions><Action><Trigger TriggerType="Event" TriggerEvent="OnOpen" Description="Open"/><Script ScriptType="WindowScript">Done.write(1);</Script></Action></Actions>
        </Panel>
        <PanelFolder Name="Sub"><Panel Name="Two" ID="P2"/></PanelFolder>
      </BasePanels>
      <MasterPanels><MasterPanel Name="Std" ID="P1000001"/></MasterPanels>
      <PopupWindows><Window Name="Pop" WindowID="W1"><Panels><Panel Name="Panel1" ID="P10001"/></Panels></Window></PopupWindows>
    </GraphicalPanels>
    <Variables>
      <Folder Name="Ctl">
        <Variable Name="Sec" Type="INT" Source="External" Sharing="None" ScanGroup="PLC" DeviceAddress="%MW11" LogUserOperations="Disabled"/>
        <Variable Name="Run" Type="BOOL" Source="External" ScanGroup="PLC" DeviceAddress="%M1"><Description>marcha</Description></Variable>
        <Variable Name="Alm" Type="BOOL" Source="External" ScanGroup="PLC" DeviceAddress="%M2"><Alarm/></Variable>
      </Folder>
      <Variable Name="Done" Type="BOOL" Source="Internal" Sharing="None" InitialValue="0" Retentive="Disabled"/>
      <Variable Name="Unused" Type="DINT" Source="Internal"/>
      <Variable Name="Auth" Type="Authorization" Source="Internal"><Variable Name="UserName"/><Variable Name="Result"/></Variable>
    </Variables>
  </Target>
</Project>"#;

    #[test]
    fn reads_variables() {
        let doc = parse(XML).unwrap();
        let t = targets(&doc)[0];
        let v = variables(t);
        let names: Vec<&str> = v.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["Ctl.Sec", "Ctl.Run", "Ctl.Alm", "Done", "Unused", "Auth"]);
        assert_eq!((v[0].data_type.as_str(), v[0].source.as_str(), v[0].address.as_deref()), ("INT", "External", Some("%MW11")));
        assert_eq!(v[0].extra.get("ScanGroup").map(String::as_str), Some("PLC"));
        assert!(!v[0].extra.contains_key("Sharing"));
        assert_eq!(v[1].description.as_deref(), Some("marcha"));
        assert_eq!(v[3].extra.get("InitialValue").map(String::as_str), Some("0"));
        assert_eq!(v[5].extra.get("Members").map(String::as_str), Some("UserName, Result"));
    }

    #[test]
    fn reads_actions_and_panels() {
        let doc = parse(XML).unwrap();
        let t = targets(&doc)[0];
        let a = global_actions(t);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].trigger, "Conditional - OnDataChange, Ctl.Sec");
        assert_eq!((a[0].description.as_deref(), a[0].interlock.as_deref(), a[0].kind.as_deref()), (Some("Sync"), Some("!Done"), Some("Script, Bit")));
        assert_eq!(a[0].body.as_deref(), Some("Sys.setTime(Ctl.Sec.getIntValue());\nBit(BitOperation=BitSet, DestinationVariable=Done)"));
        let p = panels(t);
        let summary: Vec<(&str, &str, Option<&str>)> = p.iter().map(|p| (p.group.as_str(), p.path.as_str(), p.panel_ref.as_deref())).collect();
        assert_eq!(
            summary,
            [("Base Panels", "Start", Some("BP:\\Start")), ("Base Panels", "Sub\\Two", Some("BP:\\Sub\\Two")), ("Master Panels", "Std", None), ("Popup Windows", "Pop", None)]
        );
        assert_eq!(p[0].actions[0].trigger, "Event - OnOpen");
        assert_eq!(base_panel_refs(t).len(), 2);
        // Las ventanas emergentes no tienen id: lo tienen sus paneles (<Window><Panels><Panel ID>).
        let all = panel_ids(t);
        let ids: Vec<(&str, &str, &str, &str)> = all.iter().map(|p| (p.group.as_str(), p.path.as_str(), p.id.as_str(), p.window.as_str())).collect();
        assert_eq!(
            ids,
            [
                ("Base Panels", "Start", "P1", ""),
                ("Base Panels", "Sub\\Two", "P2", ""),
                ("Master Panels", "Std", "P1000001", ""),
                ("Popup Windows", "Pop\\Panel1", "P10001", "Pop")
            ]
        );
    }

    #[test]
    fn finds_cross_references() {
        let doc = parse(XML).unwrap();
        let x = cross_references(targets(&doc)[0]);
        let get = |n: &str| x.iter().find(|r| r.variable == n).unwrap().references.clone();
        assert_eq!(get("Ctl.Run"), ["GraphicalPanels › Panel «Start» › Switch «Switch1» › Animation @Variable"]);
        assert_eq!(get("Ctl.Sec"), ["Actions › Action 1 › Script", "Actions › Action 1 › Trigger @TriggerVariable"]);
        assert_eq!(
            get("Done"),
            ["Actions › Action 1 @InterlockVariable", "Actions › Action 1 › Bit @DestinationVariable", "GraphicalPanels › Panel «Start» › Action 1 › Script"]
        );
        assert!(get("Unused").is_empty());
        assert_eq!(
            get("Ctl.Alm"),
            ["GraphicalPanels › Panel «Start» › Switch «Switch1» › Bit @DestinationVariable", "Variables › Variable «Ctl.Alm» › Alarm (1)"],
            "placeholder $Carpeta$.miembro y alarma propia"
        );
    }

    #[test]
    fn lints_project_scripts() {
        let doc = parse(XML).unwrap();
        let t = targets(&doc)[0];
        let s = scripts(t);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].0, "Actions › Action 1 › Script");
        assert_eq!(s[0].1.as_deref(), Some("GlobalActionScript"));
        let ctx = lint_context(t);
        for (_, _, code) in &s {
            assert!(crate::lint::lint(code, &ctx).is_empty(), "{code}");
        }
        assert!(crate::lint::lint("Auth.UserName.getStringValue();\nSwitch1.write(1);", &ctx).is_empty(), "miembros y objetos con nombre");
        assert_eq!(crate::lint::lint("Ctl.Sex.getIntValue();", &ctx).len(), 1);
    }

    #[test]
    fn selects_targets() {
        let doc = parse(XML).unwrap();
        assert_eq!(select_targets(&doc, Some("hmi1")).unwrap().len(), 1);
        assert!(matches!(select_targets(&doc, Some("X")), Err(VijeoError::TargetNotFound(_))));
        assert_eq!(project_name(&doc).as_deref(), Some("Demo"));
    }

    #[test]
    fn screens_and_their_references() {
        let xml = r#"<Project Name="Demo"><Target ID="T1" Name="HMI1">
  <TargetProperties InitialPanelID="P1"/>
  <GraphicalPanels>
    <BasePanels>
      <Panel Name="Start" ID="P1"><Switch Name="Ir"><WhenPress><Special SpecialFunction="ChangePanel" ChangePanelID="P21"/></WhenPress></Switch>
        <Switch Name="Pop"><WhenPress><PopPanel PopupPanelAction="Open" PopupPanelID="P10001"/></WhenPress></Switch></Panel>
      <PanelFolder Name="Sub"><Panel Name="RFID_1" ID="P21"/></PanelFolder>
    </BasePanels>
    <PopupWindows><Folder Name="F"><Window Name="ComErrorCodes" WindowID="W1"><Panels><Panel Name="Panel1" ID="P10001"><Switch Name="X"><WhenPress><PopPanel PopupPanelAction="Close" PopupPanelID="P10001"/></WhenPress></Switch></Panel></Panels></Window></Folder></PopupWindows>
  </GraphicalPanels></Target></Project>"#;
        let doc = parse(xml).unwrap();
        let t = targets(&doc)[0];
        let s = screens(t);
        let summary: Vec<(bool, &str, u32, Vec<&str>)> = s.iter().map(|s| (s.popup, s.path.as_str(), s.number, s.panel_ids.iter().map(String::as_str).collect())).collect();
        assert_eq!(summary, [(false, "Start", 1, vec!["P1"]), (false, r"Sub\RFID_1", 21, vec!["P21"]), (true, r"F\ComErrorCodes", 1, vec!["P10001"])]);
        assert_eq!(panel_id_references(t, &s[0].panel_ids), ["TargetProperties @InitialPanelID"]);
        let r = panel_id_references(t, &s[1].panel_ids);
        assert_eq!(r.len(), 1);
        assert!(r[0].contains("Switch «Ir»") && r[0].ends_with("@ChangePanelID"), "{r:?}");
        let r = panel_id_references(t, &s[2].panel_ids);
        assert!(r.len() == 1 && r[0].contains("Switch «Pop»"), "{r:?}");
    }
}
