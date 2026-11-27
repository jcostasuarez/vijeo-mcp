//! Operaciones de panel_objects, panel_edit y panel_insert sobre los objetos ya leídos ([`Clip`]),
//! sin Vijeo. El backend copia, corta y pega con la interfaz; aquí se decide qué pegar.

use std::collections::BTreeMap;

use vijeo_core::{AnimationSpec, NewObject, PanelEditOp, PanelInsertItem, PanelObject, SmartProperty, VijeoError, VijeoResult};

use crate::objects::{Clip, Node};
use crate::widgets::build::{self, PanelLookup};
use crate::widgets::smartobj::{self, PropInput};

/// Cola leída, entradas de `set_props` y variables que usan.
type PropertyInputs = (smartobj::Tail, Vec<(String, PropInput)>, Vec<String>);

/// Entradas de `set_props` para las propiedades pedidas, según el tipo de cada una en el smart object `n`, y
/// las variables que usan (para comprobar que existen).
fn property_inputs(n: &Node, props: &BTreeMap<String, String>) -> Result<PropertyInputs, String> {
    let t = match smartobj::read(n) {
        None => return Err(format!("«{}» no es un smart object: no tiene propiedades", n.name())),
        Some(r) => r.map_err(|e| format!("no se entendieron las propiedades de «{}»: {e}", n.name()))?,
    };
    let (mut inputs, mut vars) = (Vec::new(), Vec::new());
    for (name, value) in props {
        let ids: Vec<&str> = t.props.iter().map(|p| p.id.as_str()).collect();
        let p = t.props.iter().find(|p| p.id.eq_ignore_ascii_case(name)).ok_or_else(|| format!("«{}» no tiene la propiedad «{name}» (tiene: {})", n.name(), ids.join(", ")))?;
        let input = match &p.value {
            smartobj::PropValue::Tag { .. } => {
                let v = value.trim();
                if !v.is_empty() {
                    vars.push(v.to_owned());
                }
                PropInput::Variable(v.to_owned())
            }
            smartobj::PropValue::Color(_) => PropInput::Color(value.clone()),
            _ => PropInput::Combo(value.clone()),
        };
        inputs.push((p.id.clone(), input));
    }
    Ok((t, inputs, vars))
}

/// Cambia las propiedades `props` del smart object `n`. Devuelve una línea por cambio.
pub fn set_properties(n: &mut Node, props: &BTreeMap<String, String>) -> Result<Vec<String>, String> {
    if props.is_empty() {
        return Err("no hay ninguna propiedad".into());
    }
    let (mut t, inputs, _) = property_inputs(n, props)?;
    let done = smartobj::set_props(&mut t, &inputs)?;
    smartobj::write(n, &t)?;
    Ok(done)
}

/// El único smart object de nivel superior de una pieza (al que van las `properties` de panel_insert).
fn smart_target(nodes: &[Node]) -> Result<usize, String> {
    let idx: Vec<usize> = nodes.iter().enumerate().filter(|(_, n)| n.kind() == "SmartObject").map(|(i, _)| i).collect();
    match idx.len() {
        1 => Ok(idx[0]),
        0 => Err("la pieza no es un smart object: `properties` no vale".into()),
        n => Err(format!("la pieza tiene {n} smart objects: `properties` sólo vale con uno")),
    }
}

/// Variables de las `properties` de un elemento de panel_insert sobre los objetos de su pieza.
pub fn insert_property_variables(nodes: &[Node], props: &BTreeMap<String, String>) -> Result<Vec<String>, String> {
    if props.is_empty() {
        return Ok(Vec::new());
    }
    let i = smart_target(nodes)?;
    Ok(property_inputs(&nodes[i], props)?.2)
}

/// Variables de las operaciones `set_properties` de panel_edit, sobre el panel ya copiado.
pub fn edit_property_variables(clip: &Clip, ops: &[PanelEditOp]) -> VijeoResult<Vec<String>> {
    let mut v = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        if let PanelEditOp::SetProperties { object, properties } = op {
            // Un objeto que aún no existe (lo crea o renombra una operación anterior) se comprueba en `apply`.
            let Ok(p) = clip.resolve(object) else { continue };
            v.extend(property_inputs(clip.node(&p), properties).map_err(|e| bad(&format!("operations[{i}]"), e))?.2);
        }
    }
    Ok(v)
}

/// ¿Alguna operación cambia propiedades de smart objects?
pub fn has_property_edits(ops: &[PanelEditOp]) -> bool {
    ops.iter().any(|op| matches!(op, PanelEditOp::SetProperties { .. }))
}

/// Máximo de textos por objeto en panel_objects (las imágenes y los smart objects tienen muchos).
const MAX_STRINGS: usize = 40;

/// Textos y variables de las cadenas de un objeto. Las referencias a variables vienen en varias formas
/// (`TagDB.X`, `X`, `${F,…}$.{T,…}`): de `TagDB.X` sale el nombre y las demás se omiten.
fn classify(strings: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut vars: Vec<String> = Vec::new();
    for s in &strings {
        if let Some(v) = s.strip_prefix("TagDB.") {
            if !vars.iter().any(|x| x == v) {
                vars.push(v.to_owned());
            }
        }
    }
    let mut texts: Vec<String> = Vec::new();
    for s in strings {
        let internal = s.starts_with("TagDB.") || s.contains("{T,") || s.contains("{F,") || vars.iter().any(|v| *v == s);
        if !internal && !s.trim().is_empty() && !texts.contains(&s) {
            texts.push(s);
        }
    }
    texts.truncate(MAX_STRINGS);
    (texts, vars)
}

/// Propiedades de un smart object para panel_objects.
fn smart_properties(t: &smartobj::Tail) -> Vec<SmartProperty> {
    smartobj::describe(t)
        .into_iter()
        .map(|(name, kind, label, value, options)| SmartProperty { label: label.filter(|l| *l != name), name, kind, value, options })
        .collect()
}

fn to_object(n: &Node, prefix: &str) -> PanelObject {
    let path = if prefix.is_empty() { n.name() } else { format!("{prefix}/{}", n.name()) };
    let r = n.rect();
    // En los smart objects, las cadenas de la cola son el código Java de la pieza: se muestran sus propiedades.
    let (strings, variables, smart_object, properties) = match smartobj::read(n) {
        Some(Ok(t)) => {
            let vars = t.props.iter().filter_map(|p| p.variable()).collect();
            (Vec::new(), vars, smartobj::item_id(n), smart_properties(&t))
        }
        Some(Err(e)) => {
            tracing::warn!(object = %path, "propiedades del smart object no leídas: {e}");
            (Vec::new(), Vec::new(), smartobj::item_id(n), Vec::new())
        }
        None => {
            let (s, v) = classify(n.strings());
            (s, v, None, Vec::new())
        }
    };
    PanelObject {
        smart_object,
        properties,
        children: n.children().iter().map(|(_, c)| to_object(c, &path)).collect(),
        path,
        name: n.name(),
        kind: n.kind(),
        left: r[0],
        top: r[1],
        width: r[2] - r[0] + 1,
        height: r[3] - r[1] + 1,
        strings,
        variables,
    }
}

pub fn to_objects(clip: &Clip) -> Vec<PanelObject> {
    clip.objects.iter().map(|n| to_object(n, "")).collect()
}

pub fn count(objs: &[PanelObject]) -> usize {
    objs.iter().map(|o| 1 + count(&o.children)).sum()
}

fn bad(field: &str, reason: impl Into<String>) -> VijeoError {
    VijeoError::invalid(field, reason)
}

fn resolve(clip: &Clip, object: &str) -> VijeoResult<Vec<usize>> {
    clip.resolve(object).map_err(|e| bad("object", e))
}

/// Aplica `ops` en orden. Devuelve qué hizo cada una.
pub fn apply(clip: &mut Clip, ops: &[PanelEditOp], panels: PanelLookup) -> VijeoResult<Vec<String>> {
    let mut changes = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        let field = format!("operations[{i}]");
        let err = |e: String| bad(&field, e);
        let done = match op {
            PanelEditOp::Rename { object, new_name } => {
                let p = resolve(clip, object)?;
                let new_name = new_name.trim();
                if clip.walk().iter().any(|(q, path)| *q != p && path.rsplit('/').next() == Some(new_name)) {
                    return Err(err(format!("ya hay otro objeto «{new_name}» en el panel")));
                }
                let old = clip.node(&p).name();
                clip.node_mut(&p).rename(new_name).map_err(err)?;
                format!("«{old}» → «{new_name}»")
            }
            PanelEditOp::Move { object, dx, dy } => {
                let p = resolve(clip, object)?;
                clip.node_mut(&p).shift(*dx, *dy).map_err(err)?;
                clip.refit_ancestors(&p).map_err(err)?;
                format!("«{object}» desplazado ({dx}, {dy})")
            }
            PanelEditOp::MoveTo { object, x, y } => {
                let p = resolve(clip, object)?;
                let r = clip.node(&p).rect();
                clip.node_mut(&p).shift(x - r[0], y - r[1]).map_err(err)?;
                clip.refit_ancestors(&p).map_err(err)?;
                format!("«{object}» llevado a ({x}, {y})")
            }
            PanelEditOp::Resize { object, width, height } => {
                let p = resolve(clip, object)?;
                if clip.node(&p).is_group() {
                    return Err(err(format!("«{object}» es un grupo: cambia el tamaño de sus objetos")));
                }
                if *width < 1 || *height < 1 {
                    return Err(err("el ancho y el alto tienen que ser al menos 1".into()));
                }
                let r = clip.node(&p).rect();
                clip.node_mut(&p).resize([r[0], r[1], r[0] + width - 1, r[1] + height - 1]).map_err(err)?;
                clip.refit_ancestors(&p).map_err(err)?;
                format!("«{object}» ahora mide {width} × {height}")
            }
            PanelEditOp::SetText { object, old, new } => {
                let p = resolve(clip, object)?;
                let n = clip.node_mut(&p).set_text(old, new).map_err(err)?;
                if n == 0 {
                    let (texts, _) = classify(clip.node(&p).strings());
                    return Err(err(format!("«{object}» no tiene el texto «{old}» (tiene: {texts:?})")));
                }
                format!("«{object}»: texto «{old}» → «{new}»")
            }
            PanelEditOp::Replace { object, old, new } => {
                let n = match object {
                    Some(o) => {
                        let p = resolve(clip, o)?;
                        clip.node_mut(&p).replace_all(old, new).map_err(err)?
                    }
                    None => {
                        let mut n = 0;
                        for o in &mut clip.objects {
                            n += o.replace_all(old, new).map_err(err)?;
                        }
                        n
                    }
                };
                if n == 0 {
                    return Err(err(format!("«{old}» no aparece en {}", object.as_deref().map(|o| format!("«{o}»")).unwrap_or("el panel".into()))));
                }
                format!("«{old}» → «{new}» en {n} cadena(s) de {}", object.as_deref().unwrap_or("el panel"))
            }
            PanelEditOp::Delete { object } => {
                let p = resolve(clip, object)?;
                clip.remove(&p).map_err(err)?;
                format!("«{object}» borrado")
            }
            PanelEditOp::Animate { object, animations } => {
                if animations.is_empty() {
                    return Err(err("no hay ninguna animación".into()));
                }
                let p = resolve(clip, object)?;
                let done = build::animate(clip.node_mut(&p), animations).map_err(err)?;
                format!("«{object}»: {}", done.join(", "))
            }
            PanelEditOp::RemoveAnimations { object, kinds } => {
                let p = resolve(clip, object)?;
                let n = build::remove_animations(clip.node_mut(&p), kinds).map_err(err)?;
                if n == 0 {
                    return Err(err(format!("«{object}» no tiene esas animaciones")));
                }
                format!("«{object}»: {n} animación(es) quitada(s)")
            }
            PanelEditOp::SetActions { object, on_touch, on_release, while_pressed } => {
                let p = resolve(clip, object)?;
                if on_touch.is_none() && on_release.is_none() && while_pressed.is_none() {
                    return Err(err("indica `on_touch`, `on_release` o `while_pressed`".into()));
                }
                build::set_actions(clip.node_mut(&p), on_touch.as_deref(), on_release.as_deref(), while_pressed.as_deref(), panels).map_err(err)?;
                format!("«{object}»: acciones cambiadas")
            }
            PanelEditOp::SetColors { object, fill, line, text, on, off } => {
                let p = resolve(clip, object)?;
                let c = build::ColorChange { fill: fill.as_deref(), line: line.as_deref(), text: text.as_deref(), on: on.as_deref(), off: off.as_deref() };
                build::set_colors(clip.node_mut(&p), &c).map_err(err)?;
                format!("«{object}»: colores cambiados")
            }
            PanelEditOp::SetProperties { object, properties } => {
                let p = resolve(clip, object)?;
                let done = set_properties(clip.node_mut(&p), properties).map_err(err)?;
                format!("«{object}»: {}", done.join(", "))
            }
        };
        changes.push(done);
    }
    clip.reanchor();
    Ok(changes)
}

/// Prepara un objeto que se va a insertar: lo lleva a (`x`, `y`), le pone el nombre y aplica las
/// sustituciones y las animaciones. `nodes` son los objetos de nivel superior de la fuente.
pub fn prepare_insert(mut nodes: Vec<Node>, tags: Vec<u8>, item: &PanelInsertItem, field: &str) -> VijeoResult<Clip> {
    let err = |e: String| bad(field, e);
    if nodes.is_empty() {
        return Err(err("la fuente no tiene objetos".into()));
    }
    let left = nodes.iter().map(|n| n.rect()[0]).min().unwrap();
    let top = nodes.iter().map(|n| n.rect()[1]).min().unwrap();
    for n in &mut nodes {
        n.shift(item.x - left, item.y - top).map_err(err)?;
    }
    if let Some(name) = item.name.as_deref() {
        if nodes.len() != 1 {
            return Err(err(format!("la pieza tiene {} objetos: `name` sólo vale con uno", nodes.len())));
        }
        nodes[0].rename(name.trim()).map_err(err)?;
    }
    for r in &item.replace {
        let mut n = 0;
        for node in &mut nodes {
            n += node.replace_all(&r.old, &r.new).map_err(err)?;
        }
        if n == 0 {
            return Err(err(format!("«{}» no aparece en el objeto", r.old)));
        }
    }
    if !item.properties.is_empty() {
        let i = smart_target(&nodes).map_err(err)?;
        set_properties(&mut nodes[i], &item.properties).map_err(err)?;
    }
    animate_insert(&mut nodes, item, item.visibility.as_deref(), field)?;
    let mut clip = Clip { anchor: (item.x, item.y), objects: nodes, tags };
    clip.reanchor();
    Ok(clip)
}

/// Animaciones de un objeto insertado (`visibility` incluida, salvo en los botones nuevos, que la llevan dentro).
fn animate_insert(nodes: &mut [Node], item: &PanelInsertItem, visibility: Option<&str>, field: &str) -> VijeoResult<()> {
    let mut specs = item.animations.clone();
    if let Some(v) = visibility {
        specs.insert(0, AnimationSpec::Visibility { variable: v.to_owned(), invert: false });
    }
    if specs.is_empty() {
        return Ok(());
    }
    if nodes.len() != 1 || nodes[0].is_group() {
        return Err(bad(field, "las animaciones (y `visibility`) sólo valen para un objeto suelto, no para grupos ni piezas de varios objetos"));
    }
    build::animate(&mut nodes[0], &specs).map_err(|e| bad(field, e))?;
    Ok(())
}

/// Nombre base de los objetos nuevos de cada tipo (Vijeo usa el tipo y un número: `Switch01`).
fn base_name(o: &NewObject) -> &'static str {
    match o {
        NewObject::Switch { .. } => "Switch",
        NewObject::Lamp { .. } => "Lamp",
        NewObject::NumericDisplay { .. } => "NumericDisplay",
        NewObject::StringDisplay { .. } => "StringDisplay",
        NewObject::Text { .. } => "Text",
        NewObject::Rectangle { .. } => "Rectangle",
        NewObject::Ellipse { .. } => "Ellipse",
        NewObject::Line { .. } => "Line",
    }
}

/// Primer `<base>NN` que no está en `taken` (y lo añade).
pub fn free_name(base: &str, taken: &mut Vec<String>) -> String {
    let name = (1..).map(|i| format!("{base}{i:02}")).find(|n| !taken.iter().any(|t| t.eq_ignore_ascii_case(n))).unwrap();
    taken.push(name.clone());
    name
}

/// Objeto nuevo de `item.create`. `taken`: nombres que ya hay en el panel (para elegir uno libre).
pub fn create_insert(item: &PanelInsertItem, field: &str, panels: PanelLookup, taken: &mut Vec<String>) -> VijeoResult<Clip> {
    let o = item.create.as_ref().ok_or_else(|| bad(field, "falta `create`"))?;
    if !item.replace.is_empty() {
        return Err(bad(field, "`replace` no vale con `create`: indica directamente los textos y variables"));
    }
    let name = match item.name.as_deref().map(str::trim) {
        Some(n) if !n.is_empty() => {
            taken.push(n.to_owned());
            n.to_owned()
        }
        _ => free_name(base_name(o), taken),
    };
    let is_switch = matches!(o, NewObject::Switch { .. });
    let in_class = if is_switch { item.visibility.as_deref() } else { None };
    let node = build::new_object(o, &name, item.x, item.y, item.width, item.height, in_class, panels).map_err(|e| bad(field, e))?;
    let mut nodes = vec![node];
    animate_insert(&mut nodes, item, if is_switch { None } else { item.visibility.as_deref() }, field)?;
    let mut clip = Clip { anchor: (item.x, item.y), objects: nodes, tags: crate::objects::EMPTY_TAGS.to_vec() };
    clip.reanchor();
    Ok(clip)
}

/// Variables que usa un elemento de panel_insert (para comprobar que existen).
pub fn insert_variables(item: &PanelInsertItem) -> Vec<String> {
    let mut v: Vec<String> = item.create.as_ref().map(build::object_variables).unwrap_or_default().into_iter().map(|s| s.trim().to_owned()).collect();
    v.extend(item.visibility.iter().map(|s| s.trim().to_owned()));
    v.extend(item.animations.iter().map(|a| build::animation_variables(a).trim().to_owned()));
    v
}

/// Variables que usan las operaciones de panel_edit.
pub fn edit_variables(ops: &[PanelEditOp]) -> Vec<String> {
    let mut v = Vec::new();
    for op in ops {
        match op {
            PanelEditOp::Animate { animations, .. } => v.extend(animations.iter().map(|a| build::animation_variables(a).trim().to_owned())),
            PanelEditOp::SetActions { on_touch, on_release, while_pressed, .. } => {
                for a in [on_touch, on_release, while_pressed].into_iter().flatten().flatten() {
                    v.extend(build::action_variables(a).into_iter().map(|s| s.trim().to_owned()));
                }
            }
            _ => {}
        }
    }
    v
}

/// (ruta, rectángulo) de todos los objetos, para comparar lo pegado con lo que se quería.
pub fn summary(nodes: &[Node]) -> Vec<(String, [i32; 4])> {
    fn go(n: &Node, prefix: &str, out: &mut Vec<(String, [i32; 4])>) {
        let path = if prefix.is_empty() { n.name() } else { format!("{prefix}/{}", n.name()) };
        out.push((path.clone(), n.rect()));
        for (_, c) in n.children() {
            go(c, &path, out);
        }
    }
    let mut out = Vec::new();
    for n in nodes {
        go(n, "", &mut out);
    }
    out
}

/// Copia de `clip` desplazada (`dx`, `dy`), con el ancla recalculada.
pub fn shifted(clip: &Clip, dx: i32, dy: i32) -> VijeoResult<Clip> {
    let mut c = clip.clone();
    for n in &mut c.objects {
        n.shift(dx, dy).map_err(VijeoError::Internal)?;
    }
    c.reanchor();
    Ok(c)
}

/// Devuelve a su sitio los objetos pegados (los `want.len()` siguientes a los `before` primeros) que Vijeo
/// dejó desplazados. `true` si movió alguno.
pub fn realign(clip: &mut Clip, before: usize, want: &[Node]) -> VijeoResult<bool> {
    if clip.objects.len() != before + want.len() {
        return Ok(false);
    }
    let mut moved = false;
    for (got, w) in clip.objects[before..].iter_mut().zip(want) {
        let (g, w) = (got.rect(), w.rect());
        if (g[0], g[1]) != (w[0], w[1]) {
            got.shift(w[0] - g[0], w[1] - g[1]).map_err(VijeoError::Internal)?;
            moved = true;
        }
    }
    clip.reanchor();
    Ok(moved)
}

/// Diferencias entre lo que se quería pegar y lo que quedó en el panel.
pub fn differences(want: &[Node], got: &[Node]) -> Vec<String> {
    let (w, g) = (summary(want), summary(got));
    if w == g {
        return Vec::new();
    }
    if w.len() != g.len() {
        return vec![format!("se pegaron {} objetos y en el panel hay {} donde se esperaban", w.len(), g.len())];
    }
    w.iter()
        .zip(&g)
        .filter(|(a, b)| a != b)
        .take(20)
        .map(|((pa, ra), (pb, rb))| {
            if pa != pb {
                format!("«{pa}» quedó como «{pb}» (Vijeo no admite nombres repetidos)")
            } else {
                format!("«{pa}» quedó en ({}, {}) en vez de ({}, {})", rb[0], rb[1], ra[0], ra[1])
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_panels(p: &str) -> Result<String, String> {
        Err(format!("no hay panel {p}"))
    }

    fn clip() -> Clip {
        // Un Text suelto y un grupo con un Rectangle y un Text, como en objects::tests.
        let od = crate::objects::tests_support::sample();
        Clip::parse(&od, Vec::new()).unwrap()
    }

    /// El grupo del ejemplo convertido en smart object (cola y animación sintéticas).
    fn smart_clip() -> Clip {
        use crate::widgets::anim::{self, Animation, Extent, SmartObjectAnim};
        let mut c = clip();
        let g = &mut c.objects[0];
        smartobj::write(g, &smartobj::tests::sample(9)).unwrap();
        let a = Animation::SmartObject(SmartObjectAnim { extent: Extent::DEFAULT, enabled: true, item_id: "PRUEBA01".into() });
        g.set_extension(&anim::write_ext(&[a]).unwrap()).unwrap();
        Clip::parse(&c.stream(), Vec::new()).unwrap()
    }

    #[test]
    fn smart_object_properties() {
        let mut c = smart_clip();
        let objs = to_objects(&c);
        let g = &objs[0];
        assert_eq!((g.kind.as_str(), g.smart_object.as_deref()), ("SmartObject", Some("PRUEBA01")));
        assert!(g.strings.is_empty(), "sin el código Java: {:?}", g.strings);
        let names: Vec<(&str, &str)> = g.properties.iter().map(|p| (p.name.as_str(), p.kind.as_str())).collect();
        assert_eq!(names, [("DayVariable", "variable"), ("Format", "combo"), ("Color", "color"), ("Parent", "parent")]);
        let props: BTreeMap<String, String> = [("DayVariable", "Carpeta.iDia"), ("Format", "yy/mm/dd"), ("Color", "00FF00")].into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        let ops = vec![PanelEditOp::SetProperties { object: "Grupo01".into(), properties: props.clone() }];
        assert_eq!(edit_property_variables(&c, &ops).unwrap(), ["Carpeta.iDia"]);
        let ch = apply(&mut c, &ops, &no_panels).unwrap();
        assert_eq!(ch, ["«Grupo01»: Color = 00FF00, DayVariable = Carpeta.iDia, Format = 4"]);
        let g = &to_objects(&Clip::parse(&c.stream(), Vec::new()).unwrap())[0];
        assert_eq!(g.variables, ["Carpeta.iDia"]);
        assert_eq!(g.properties[1].value, "4");
        // Errores: no es un smart object, propiedad que no existe, tipo equivocado.
        let bad_ops = |o: &str, k: &str, v: &str| vec![PanelEditOp::SetProperties { object: o.into(), properties: [(k.to_string(), v.to_string())].into() }];
        assert!(apply(&mut c, &bad_ops("Suelto", "X", "1"), &no_panels).unwrap_err().to_string().contains("no es un smart object"));
        assert!(apply(&mut c, &bad_ops("Grupo01", "Nada", "1"), &no_panels).unwrap_err().to_string().contains("no tiene la propiedad"));
        assert!(apply(&mut c, &bad_ops("Grupo01", "Color", "rojo"), &no_panels).is_err());
        // panel_insert: `properties` van al smart object de la pieza; las variables se sacan antes de pegar.
        let nodes = smart_clip().objects;
        assert_eq!(insert_property_variables(&nodes[..1], &props).unwrap(), ["Carpeta.iDia"]);
        assert!(insert_property_variables(&nodes[1..], &props).unwrap_err().contains("no es un smart object"));
        let item: PanelInsertItem = serde_json::from_value(serde_json::json!({"toolchest": "X\\Y", "x": 50, "y": 60, "properties": {"DayVariable": "Carpeta.iMes"}})).unwrap();
        let ins = prepare_insert(nodes[..1].to_vec(), Vec::new(), &item, "items[0]").unwrap();
        assert_eq!(to_objects(&ins)[0].variables, ["Carpeta.iMes"]);
        assert_eq!(ins.objects[0].rect()[..2], [50, 60]);
    }

    #[test]
    fn lists_objects_with_texts_and_variables() {
        let objs = to_objects(&clip());
        assert_eq!(count(&objs), 4);
        let g = &objs[0];
        assert_eq!((g.kind.as_str(), g.left, g.top, g.width, g.height), ("Group", 5, 35, 310, 147));
        let t = &g.children[1];
        assert_eq!(t.path, "Grupo01/Text07");
        assert_eq!(t.strings, ["Language1", "Date and Time", "abc"]);
        assert_eq!(t.variables, ["Carpeta.xVar"]);
    }

    #[test]
    fn applies_operations_in_order() {
        let mut c = clip();
        let ops: Vec<PanelEditOp> = serde_json::from_value(serde_json::json!([
            {"op": "rename", "object": "Text07", "new_name": "Titulo"},
            {"op": "set_text", "object": "Titulo", "old": "Date and Time", "new": "Fecha"},
            {"op": "replace", "object": "Grupo01", "old": "xVar", "new": "xOtra"},
            {"op": "move_to", "object": "Suelto", "x": 10, "y": 220},
            {"op": "resize", "object": "Suelto", "width": 50, "height": 10},
            {"op": "delete", "object": "Grupo01/Rect03"}
        ]))
        .unwrap();
        let changes = apply(&mut c, &ops, &no_panels).unwrap();
        assert_eq!(changes.len(), 6);
        let objs = to_objects(&c);
        assert_eq!(objs[0].children.len(), 1);
        assert_eq!(objs[0].children[0].strings[1], "Fecha");
        assert_eq!(objs[0].children[0].variables, ["Carpeta.xOtra"]);
        assert_eq!((objs[1].left, objs[1].top, objs[1].width, objs[1].height), (10, 220, 50, 10));
        // El grupo se ajusta a lo que le queda y el ancla a la esquina de todo.
        assert_eq!((objs[0].left, objs[0].top), (12, 35));
        assert_eq!(c.anchor, (10, 35));
        assert!(Clip::parse(&c.stream(), Vec::new()).is_ok());
    }

    #[test]
    fn rejects_bad_operations() {
        let mut c = clip();
        let one = |v: serde_json::Value| -> Vec<PanelEditOp> { serde_json::from_value(serde_json::json!([v])).unwrap() };
        assert!(apply(&mut c, &one(serde_json::json!({"op": "rename", "object": "Text07", "new_name": "Suelto"})), &no_panels).is_err());
        assert!(apply(&mut c, &one(serde_json::json!({"op": "resize", "object": "Grupo01", "width": 5, "height": 5})), &no_panels).is_err());
        assert!(apply(&mut c, &one(serde_json::json!({"op": "set_text", "object": "Text07", "old": "nada", "new": "x"})), &no_panels).is_err());
        assert!(apply(&mut c, &one(serde_json::json!({"op": "replace", "old": "zzz", "new": "x"})), &no_panels).is_err());
        assert!(apply(&mut c, &one(serde_json::json!({"op": "move", "object": "NoExiste", "dx": 1, "dy": 1})), &no_panels).is_err());
    }

    #[test]
    fn prepares_inserts_and_compares() {
        let c = clip();
        let item: PanelInsertItem = serde_json::from_value(serde_json::json!({"x": 100, "y": 50, "name": "Nuevo", "replace": [{"old": "Hola", "new": "Adiós"}]})).unwrap();
        let ins = prepare_insert(vec![c.objects[1].clone()], Vec::new(), &item, "items[0]").unwrap();
        assert_eq!(ins.anchor, (100, 50));
        let o = &to_objects(&ins)[0];
        assert_eq!((o.name.as_str(), o.left, o.top), ("Nuevo", 100, 50));
        assert_eq!(o.strings[1], "Adiós\nmundo");
        assert!(differences(&ins.objects, &ins.objects).is_empty());
        let mut moved = ins.objects.clone();
        moved[0].shift(5, 5).unwrap();
        assert_eq!(differences(&ins.objects, &moved), ["«Nuevo» quedó en (105, 55) en vez de (100, 50)"]);
        let two: PanelInsertItem = serde_json::from_value(serde_json::json!({"x": 0, "y": 0, "name": "X"})).unwrap();
        assert!(prepare_insert(c.objects.clone(), Vec::new(), &two, "items[0]").is_err());
    }

    #[test]
    fn realigns_offset_pastes() {
        // Lo que Vijeo deja tras pegar con su desplazamiento: el grupo original y el Text 10 px más allá.
        let c = clip();
        let want = vec![c.objects[1].clone()];
        let mut got = shifted(&c, 0, 0).unwrap();
        got.objects[1].shift(10, 10).unwrap();
        assert_eq!(differences(&want, &got.objects[1..]).len(), 1);
        assert!(realign(&mut got, 1, &want).unwrap());
        assert!(differences(&want, &got.objects[1..]).is_empty());
        assert_eq!(got.objects[0].rect(), c.objects[0].rect(), "lo que ya había no se toca");
        assert!(!realign(&mut got, 1, &want).unwrap());
        assert_eq!(shifted(&c, -5, -5).unwrap().anchor, (0, 30));
    }
}
