//! De los tipos del dominio (`NewObject`, `AnimationSpec`, `SwitchAction`) a objetos de Vijeo: lo que usan
//! panel_insert (crear) y panel_edit (animaciones, acciones y colores).
//!
//! Los colores del dominio son `RRGGBB` (como en un editor de imágenes); Vijeo guarda COLORREF `0x00BBGGRR`.

use vijeo_core::{
    AnimationKind, AnimationSpec, Arrows, ColorPart, FillDirection, FontSize, HAlign, LampShape, NewObject, SwitchAction,
};

use super::action::{Action, Op};
use super::anim::{self, Animation, Kind, Use};
use super::display::{self, Bound, DataDisplay, DisplaySpec};
use super::expr::Expr;
use super::hmi::{col, ColorBlock, LanguageFont, StateColors, Widget};
use super::lamp::{self, rm, Lamp, LampColors, LampLabels, LampSpec};
use super::shapes::{self, paint, Color, Ellipse, Line, RecShape, Rectangle, Text};
use super::switch::{ButtonSpec, Style, Switch};
use crate::objects::{guid_bytes, Node, EMPTY_EXTENSION};

pub const GUID_SWITCH: &str = "{347CCED5-5E26-497A-A140-EA017D0272BA}";

/// Lo que las acciones necesitan sacar del proyecto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// Paneles base («cambiar de panel»): su PanelID.
    Base,
    /// Ventanas emergentes (abrir y cerrar popup): su PanelID.
    Popup,
    /// Script de acción: su Java compilado (`crate::script`), a partir de la fuente normalizada.
    Script,
}

/// Referencias de las acciones: PanelID de un panel como texto (`P2` → `2`, `P10001` → `10001`) a partir de su nombre o
/// ruta, o el Java compilado de un script a partir de su fuente.
pub type PanelLookup<'a> = &'a dyn Fn(RefKind, &str) -> Result<String, String>;

/// `RRGGBB` → COLORREF; `none` (o `transparent`) → `None`.
pub fn color(s: &str) -> Result<Option<u32>, String> {
    let t = s.trim().trim_start_matches('#');
    if t.eq_ignore_ascii_case("none") || t.eq_ignore_ascii_case("transparent") {
        return Ok(None);
    }
    if t.len() != 6 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("color no válido «{s}»: se espera RRGGBB en hexadecimal (p. ej. 00FF00) o `none`"));
    }
    let v = u32::from_str_radix(t, 16).unwrap();
    let (r, g, b) = (v >> 16, (v >> 8) & 0xFF, v & 0xFF);
    Ok(Some(r | g << 8 | b << 16))
}

/// Color obligatorio (sin `none`).
fn solid(s: &str) -> Result<u32, String> {
    color(s)?.ok_or_else(|| format!("«{s}»: aquí no vale un color transparente"))
}

fn solid_or(s: &Option<String>, default: u32) -> Result<u32, String> {
    s.as_deref().map(solid).transpose().map(|c| c.unwrap_or(default))
}

/// Cara GP, ancho y alto de cada tamaño de fuente.
fn gp(size: FontSize) -> (u32, i32, i32) {
    match size {
        FontSize::Small => (9, 6, 10),
        FontSize::Medium => (10, 8, 13),
        FontSize::Large => (12, 13, 23),
    }
}

/// Fuente de las etiquetas de botones, lámparas y displays.
fn label_font(size: FontSize, bold: bool) -> LanguageFont {
    match size {
        FontSize::Medium => lamp::font_8x13(bold),
        _ => {
            let (face, w, h) = gp(size);
            LanguageFont::gp(face, w, h, bold)
        }
    }
}

fn halign(a: Option<HAlign>, default: u32) -> u32 {
    match a {
        None => default,
        Some(HAlign::Left) => 0,
        Some(HAlign::Center) => 1,
        Some(HAlign::Right) => 2,
    }
}

fn check_var(field: &str, v: &str) -> Result<(), String> {
    let v = v.trim();
    if v.is_empty() || v.contains(|c: char| c.is_whitespace() || "(){}$\"'".contains(c)) {
        return Err(format!("{field}: «{v}» no es un nombre de variable (Carpeta.Variable)"));
    }
    Ok(())
}

/// Variables de una acción (para comprobar que existen).
pub fn action_variables(a: &SwitchAction) -> Vec<&str> {
    use SwitchAction::*;
    match a {
        Set { variable } | Reset { variable } | Toggle { variable } | MomentaryOn { variable } | MomentaryOff { variable } => vec![variable],
        WordSet { variable, value } | WordAdd { variable, value } | WordSubtract { variable, value } => {
            if value.trim().parse::<f64>().is_ok() {
                vec![variable]
            } else {
                vec![variable, value]
            }
        }
        // Las variables de un script las comprueba el compilador.
        ChangePanel { .. } | PreviousPanel | OpenPopup { .. } | ClosePopup { .. } | Script { .. } => vec![],
    }
}

/// ¿Alguna acción necesita buscar paneles (los ids salen del XML)?
pub fn uses_panels(l: &[SwitchAction]) -> bool {
    l.iter().any(|a| matches!(a, SwitchAction::ChangePanel { .. } | SwitchAction::OpenPopup { .. } | SwitchAction::ClosePopup { .. }))
}

/// Fuentes de los scripts de unas acciones, normalizadas como las guarda Vijeo (`\r\n`).
pub fn scripts(l: &[SwitchAction]) -> Vec<String> {
    l.iter().filter_map(|a| if let SwitchAction::Script { script } = a { Some(crate::script::normalize(script)) } else { None }).collect()
}

/// Coordenada de un popup: constante entera (`PopupPanelPosX="20"` en los ejemplos). Su `compiled2` en los ejemplos
/// es el contexto de la acción anterior; Vijeo lo acepta vacío.
fn popup_coord(v: i32) -> Expr {
    Expr::constant(&v.to_string(), "")
}

/// Variables que usa un objeto nuevo.
pub fn object_variables(o: &NewObject) -> Vec<&str> {
    let mut v: Vec<&str> = Vec::new();
    match o {
        NewObject::Switch { on_touch, on_release, while_pressed, lamp, interlock, .. } => {
            for a in on_touch.iter().chain(on_release).chain(while_pressed) {
                v.extend(action_variables(a));
            }
            v.extend(lamp.as_deref());
            v.extend(interlock.as_deref());
        }
        NewObject::Lamp { variable, .. } | NewObject::StringDisplay { variable, .. } => v.push(variable),
        NewObject::NumericDisplay { variable, min, max, .. } => {
            v.push(variable);
            for b in [min, max].into_iter().flatten() {
                if b.trim().parse::<f64>().is_err() {
                    v.push(b);
                }
            }
        }
        _ => {}
    }
    v
}

pub fn animation_variables(a: &AnimationSpec) -> &str {
    match a {
        AnimationSpec::Visibility { variable, .. }
        | AnimationSpec::Blink { variable, .. }
        | AnimationSpec::Color { variable, .. }
        | AnimationSpec::Fill { variable, .. }
        | AnimationSpec::Position { variable, .. } => variable,
    }
}

fn word_operand(dest: &str, value: &str) -> Expr {
    let v = value.trim();
    if v.parse::<f64>().is_ok() {
        Expr::constant(v, &format!("TagDB.{dest}"))
    } else {
        Expr::int_value(v)
    }
}

/// Acción del dominio → acción de Vijeo.
pub fn op(a: &SwitchAction, panels: PanelLookup) -> Result<Op, String> {
    use SwitchAction::*;
    for v in action_variables(a) {
        check_var("acción", v)?;
    }
    Ok(match a {
        Set { variable } => Op::BitSet(Expr::bit_dest(variable.trim())),
        Reset { variable } => Op::BitReset(Expr::bit_dest(variable.trim())),
        Toggle { variable } => Op::BitToggle(Expr::bit_dest(variable.trim())),
        MomentaryOn { variable } => Op::BitMomentaryOn(Expr::bit_dest(variable.trim())),
        MomentaryOff { variable } => Op::BitMomentaryOff(Expr::bit_dest(variable.trim())),
        WordSet { variable, value } => Op::WordSet { dest: Expr::word_dest(variable.trim()), value: word_operand(variable.trim(), value) },
        WordAdd { variable, value } => Op::WordAdd { dest: Expr::word_dest(variable.trim()), value: word_operand(variable.trim(), value) },
        WordSubtract { variable, value } => {
            Op::WordSubtract { dest: Expr::word_dest(variable.trim()), value: word_operand(variable.trim(), value) }
        }
        ChangePanel { panel } => Op::ChangePanel(panels(RefKind::Base, panel)?),
        PreviousPanel => Op::PreviousPanel,
        OpenPopup { popup, x, y } => {
            let target = panels(RefKind::Popup, popup)?;
            match (x, y) {
                (Some(x), Some(y)) => Op::PopupOpen { x: popup_coord(*x), y: popup_coord(*y), target },
                (None, None) => Op::PopupOpenCentered(target),
                _ => return Err("open_popup: indica `x` e `y` (las dos) o ninguna (centrado)".into()),
            }
        }
        // «-1» (cerrar el popup en el que está el botón) da el error 2010 en un panel base, que es donde trabajan
        // panel_insert y panel_edit: sólo se cierra un popup por su nombre.
        ClosePopup { popup } => Op::PopupClose(panels(RefKind::Popup, popup)?),
        Script { script } => {
            let source = crate::script::normalize(script);
            if source.trim().is_empty() {
                return Err("script: el script está vacío".into());
            }
            let compiled = panels(RefKind::Script, &source)?;
            Op::Script { source, compiled }
        }
    })
}

fn ops(l: &[SwitchAction], panels: PanelLookup) -> Result<Vec<Op>, String> {
    l.iter().map(|a| op(a, panels)).collect()
}

/// Tamaño por defecto de cada tipo (ancho, alto).
fn default_size(o: &NewObject) -> (i32, i32) {
    match o {
        NewObject::Switch { .. } => (58, 18),
        NewObject::Lamp { .. } => (40, 20),
        NewObject::NumericDisplay { .. } => (60, 20),
        NewObject::StringDisplay { .. } => (100, 20),
        NewObject::Text { text, font, .. } => {
            let (_, cw, ch) = gp(*font);
            let lines: Vec<&str> = text.split('\n').collect();
            let chars = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1).max(1) as i32;
            (chars * cw + 4, lines.len() as i32 * ch + 4)
        }
        NewObject::Rectangle { .. } => (60, 40),
        NewObject::Ellipse { .. } => (30, 30),
        NewObject::Line { .. } => (1, 1),
    }
}

fn leaf(guid: &str, name: &str, rect: [i32; 4], data: &[u8]) -> Result<Node, String> {
    Node::new_leaf(guid_bytes(guid)?, name, rect, &EMPTY_EXTENSION, data)
}

/// Objeto nuevo en (`x`, `y`). `name`: nombre del objeto (Vijeo cambia los repetidos). `visibility` va dentro del
/// botón; en el resto la pone quien llama como animación.
#[allow(clippy::too_many_arguments)]
pub fn new_object(
    o: &NewObject,
    name: &str,
    x: i32,
    y: i32,
    width: Option<i32>,
    height: Option<i32>,
    visibility: Option<&str>,
    panels: PanelLookup,
) -> Result<Node, String> {
    let (dw, dh) = default_size(o);
    let (w, h) = (width.unwrap_or(dw), height.unwrap_or(dh));
    if !matches!(o, NewObject::Line { .. }) && (w < 2 || h < 2) {
        return Err(format!("tamaño no válido: {w}×{h}"));
    }
    let rect = [x, y, w, h];
    match o {
        NewObject::Switch {
            label,
            on_touch,
            on_release,
            while_pressed,
            lamp,
            interlock,
            on_color,
            off_color,
            text_color,
            font,
            bold,
            style,
            buzzer,
        } => {
            for v in lamp.iter().chain(interlock).map(|s| s.as_str()).chain(visibility) {
                check_var("variable", v)?;
            }
            let text = solid_or(text_color, 0x696462)?;
            let on = StateColors { text, ..StateColors::standard(solid_or(on_color, 0x00FF00)?) };
            let off = StateColors { text, ..StateColors::standard(solid_or(off_color, 0xC0C0C0)?) };
            let style = style.as_deref().unwrap_or("Square03");
            if style.is_empty() || style.contains(['\\', '/', '.']) {
                return Err(format!("estilo no válido «{style}»"));
            }
            let spec = ButtonSpec {
                label: (!label.is_empty()).then_some(label.as_str()),
                font: Some(label_font(*font, *bold)),
                on_touch: ops(on_touch, panels)?,
                on_release: ops(on_release, panels)?,
                while_pressed: ops(while_pressed, panels)?,
                lamp: lamp.as_deref().map(str::trim),
                interlock: interlock.as_deref().map(str::trim),
                visibility: visibility.map(str::trim),
                colors: ColorBlock::own(on, off),
                style,
                buzzer: *buzzer,
            };
            leaf(GUID_SWITCH, name, rect, &Switch::button(&spec).to_bytes())
        }
        NewObject::Lamp { variable, label, on_label, off_label, on_color, off_color, text_color, font, bold, shape } => {
            check_var("variable", variable)?;
            let text = solid_or(text_color, 0)?;
            let st = |fore| LampColors { text, ..LampColors::fill(fore) };
            let labels = match (label, on_label, off_label) {
                (None, None, None) => LampLabels::None,
                (Some(t), None, None) => LampLabels::Static(t),
                (None, on, off) => LampLabels::PerState(vec![off.as_deref().unwrap_or(""), on.as_deref().unwrap_or("")]),
                _ => return Err("indica `label` o bien `on_label`/`off_label`, no los dos".into()),
            };
            let style = match shape {
                LampShape::BlankArea => lamp::STYLE_BLANK_AREA,
                LampShape::Square => lamp::STYLE_SQUARE70,
                LampShape::Round => lamp::STYLE_ROUND04,
            };
            if *shape == LampShape::Round && !matches!(labels, LampLabels::None) {
                return Err("la lámpara redonda no lleva texto".into());
            }
            let mut spec = LampSpec::bool(variable.trim());
            spec.states = vec![st(solid_or(off_color, 0xC0C0C0)?), st(solid_or(on_color, 0x00FF00)?)];
            spec.labels = labels;
            spec.font = label_font(*font, *bold);
            spec.style = style;
            let guid = crate::objects::guid_str(&lamp::LAMP_CLSID);
            leaf(&guid, name, rect, &Lamp::new_bool(&spec).to_bytes())
        }
        NewObject::NumericDisplay { variable, float, digits, decimals, input, min, max, plate_color, text_color, font, bold, align } => {
            check_var("variable", variable)?;
            let mut s = DisplaySpec::new(variable.trim());
            s.input = *input;
            fn bound(b: &str) -> Result<Bound<'_>, String> {
                let b = b.trim();
                if b.parse::<f64>().is_ok() {
                    Ok(Bound::Const(b))
                } else {
                    check_var("límite", b)?;
                    Ok(Bound::Var(b))
                }
            }
            s.range = match (min, max) {
                (None, None) => None,
                (Some(a), Some(b)) if *input => Some((bound(a)?, bound(b)?)),
                (Some(_), Some(_)) => return Err("`min` y `max` sólo valen con `input`".into()),
                _ => return Err("indica `min` y `max` juntos".into()),
            };
            display_style(&mut s, plate_color, text_color, *font, *bold, *align)?;
            let d = DataDisplay::numeric(&s, *float, digits.unwrap_or(5), decimals.unwrap_or(if *float { 2 } else { 0 }));
            leaf(&crate::objects::guid_str(&display::DD_CLSID), name, rect, &d.to_bytes())
        }
        NewObject::StringDisplay { variable, length, input, plate_color, text_color, font, bold, align } => {
            check_var("variable", variable)?;
            let mut s = DisplaySpec::new(variable.trim());
            s.input = *input;
            display_style(&mut s, plate_color, text_color, *font, *bold, *align)?;
            let n = length.unwrap_or(10);
            if !(1..=300).contains(&n) {
                return Err("`length` va de 1 a 300".into());
            }
            leaf(&crate::objects::guid_str(&display::DD_CLSID), name, rect, &DataDisplay::string(&s, n).to_bytes())
        }
        NewObject::Text { text, color: c, background, border, font, bold, align } => {
            if text.is_empty() {
                return Err("el texto está vacío".into());
            }
            let mut t = Text::default_text(text);
            t.text_color = Color(solid_or(c, 0)?);
            (t.back_color, t.back_trans) = paint(background.as_deref().map(color).transpose()?.flatten().map(Color));
            if let Some(b) = border.as_deref().map(color).transpose()?.flatten() {
                (t.line_color, t.line_trans) = (Color(b), 0);
            }
            t.h_align = halign(*align, 0) as u16;
            if let Some(f) = t.font_mut() {
                *f = shapes::Font::gp(gp(*font).0, *bold);
            }
            leaf(shapes::GUID_TEXT, name, rect, &t.write())
        }
        NewObject::Rectangle { fill, line, line_width, corner } => {
            let mut r = Rectangle::default_rectangle(w, h);
            set_fill_line(&mut r.fore_color, &mut r.fore_trans, fill)?;
            set_fill_line(&mut r.line_color, &mut r.line_trans, line)?;
            if let Some(lw) = line_width {
                r.line_width = *lw as f64;
            }
            match corner {
                Some(0) => r.shape = RecShape::Normal,
                Some(px) => r.set_corner_px(*px as i32, w, h),
                None => {}
            }
            leaf(shapes::GUID_RECTANGLE, name, rect, &r.write())
        }
        NewObject::Ellipse { fill, line, line_width } => {
            let mut e = Ellipse::default_ellipse();
            set_fill_line(&mut e.fore_color, &mut e.fore_trans, fill)?;
            set_fill_line(&mut e.line_color, &mut e.line_trans, line)?;
            if let Some(lw) = line_width {
                e.line_width = *lw as f64;
            }
            leaf(shapes::GUID_ELLIPSE, name, rect, &e.write())
        }
        NewObject::Line { end_x, end_y, color: c, width: lw, arrows } => {
            if width.is_some() || height.is_some() {
                return Err("una línea va de (x, y) a (end_x, end_y): sin `width` ni `height`".into());
            }
            let mut l = Line::default_line(x, y, *end_x, *end_y);
            l.line_color = Color(solid_or(c, 0x319400)?);
            if let Some(lw) = lw {
                l.line_width = *lw as f64;
            }
            l.arrows = match arrows {
                None | Some(Arrows::None) => shapes::Arrows::None,
                Some(Arrows::Start) => shapes::Arrows::Start,
                Some(Arrows::End) => shapes::Arrows::End,
                Some(Arrows::Both) => shapes::Arrows::Both,
            };
            let b = l.bounds();
            leaf(shapes::GUID_LINE, name, [b[0], b[1], b[2] - b[0] + 1, b[3] - b[1] + 1], &l.write())
        }
    }
}

fn display_style(
    s: &mut DisplaySpec,
    plate: &Option<String>,
    text: &Option<String>,
    font: FontSize,
    bold: bool,
    align: Option<HAlign>,
) -> Result<(), String> {
    s.colors.plate = solid_or(plate, s.colors.plate)?;
    s.colors.text = solid_or(text, s.colors.text)?;
    s.font = label_font(font, bold);
    s.halign = halign(align, 1);
    Ok(())
}

fn set_fill_line(c: &mut Color, trans: &mut u32, v: &Option<String>) -> Result<(), String> {
    if let Some(v) = v {
        (*c, *trans) = paint(color(v)?.map(Color));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------
// Animaciones
// ---------------------------------------------------------------------------------------------------------

fn kind_of(k: AnimationKind) -> Kind {
    match k {
        AnimationKind::Visibility => Kind::Visibility,
        AnimationKind::Color => Kind::Color,
        AnimationKind::Fill => Kind::Fill,
        AnimationKind::Position => Kind::Position,
        AnimationKind::Size => Kind::Size,
        AnimationKind::Rotate => Kind::Rotate,
        AnimationKind::Value => Kind::Value,
    }
}

/// Animaciones actuales de un objeto (vacías si no tiene extensión).
pub fn animations_of(n: &Node) -> Result<Vec<Animation>, String> {
    match n.extension() {
        Some(e) => anim::parse_ext(e),
        None => Ok(Vec::new()),
    }
}

/// Aplica `specs` sobre las animaciones del objeto: cada una sustituye a la de su clase; `visibility` y `blink`
/// cambian cada una su parte de la animación de visibilidad.
pub fn animate(n: &mut Node, specs: &[AnimationSpec]) -> Result<Vec<String>, String> {
    if n.is_group() {
        return Err("los grupos no se animan: anima sus objetos".into());
    }
    let mut list = animations_of(n)?;
    let mut done = Vec::new();
    for s in specs {
        let var = animation_variables(s).trim();
        check_var("animación", var)?;
        match s {
            AnimationSpec::Visibility { invert, .. } | AnimationSpec::Blink { fast: invert, .. } => {
                let mut v = match list.iter().find_map(|a| if let Animation::Visibility(v) = a { Some(v.clone()) } else { None }) {
                    Some(v) => v,
                    None => {
                        let mut v = anim::Visibility::when(anim::Expr::unused_cond());
                        v.visible = false;
                        v
                    }
                };
                if matches!(s, AnimationSpec::Visibility { .. }) {
                    let w = anim::Visibility::by_var(var, *invert);
                    (v.visible, v.visible_expr) = (w.visible, w.visible_expr);
                    done.push(format!("visible con {}{var}", if *invert { "!" } else { "" }));
                } else {
                    v = v.with_blink(anim::Expr::var(var, Use::Cond), *invert);
                    done.push(format!("parpadea con {var}"));
                }
                anim::set(&mut list, Animation::Visibility(v));
            }
            AnimationSpec::Color { on_color, off_color, part, .. } => {
                let t = anim::ColorTarget::discrete(var, color(off_color)?, color(on_color)?);
                let c = match part {
                    ColorPart::Fill => anim::Color::new(Some(t), None, None),
                    ColorPart::Background => anim::Color::new(None, Some(t), None),
                    ColorPart::Line => anim::Color::new(None, None, Some(t)),
                };
                anim::set(&mut list, Animation::Color(c));
                done.push(format!("color según {var}"));
            }
            AnimationSpec::Fill { min, max, direction, .. } => {
                if min == max {
                    return Err("relleno: `min` y `max` no pueden ser iguales".into());
                }
                let f = anim::Fill::by_var(anim::Expr::var(var, Use::Int), *direction == FillDirection::Up, *min, *max);
                anim::set(&mut list, Animation::Fill(f));
                done.push(format!("relleno según {var}"));
            }
            AnimationSpec::Position { min, max, dx, dy, .. } => {
                if min == max || (*dx == 0 && *dy == 0) {
                    return Err("posición: hace falta `min` ≠ `max` y algún desplazamiento (`dx`, `dy`)".into());
                }
                let axis = |d: i32| {
                    if d == 0 {
                        anim::PosAxis::off()
                    } else {
                        anim::PosAxis::by_var(anim::Expr::var(var, Use::Int), *min, *max, 0.0, d as f64)
                    }
                };
                anim::set(&mut list, Animation::Position(anim::Position::new(axis(*dy), axis(*dx))));
                done.push(format!("posición según {var}"));
            }
        }
    }
    n.set_extension(&anim::write_ext(&list)?)?;
    Ok(done)
}

/// Quita animaciones (todas sin `kinds`). Devuelve cuántas quitó.
pub fn remove_animations(n: &mut Node, kinds: &[AnimationKind]) -> Result<usize, String> {
    let mut list = animations_of(n)?;
    let before = list.len();
    if kinds.is_empty() {
        list.retain(|a| matches!(a.kind(), Some(Kind::Touch | Kind::SmartObject)));
    } else {
        for k in kinds {
            anim::remove(&mut list, kind_of(*k));
        }
    }
    let removed = before - list.len();
    if removed > 0 || n.extension().is_none() {
        n.set_extension(&anim::write_ext(&list)?)?;
    }
    Ok(removed)
}

// ---------------------------------------------------------------------------------------------------------
// Acciones y colores de objetos existentes
// ---------------------------------------------------------------------------------------------------------

/// Sustituye las listas de acciones indicadas de un botón (y lo que su estilo repite de ellas).
pub fn set_actions(
    n: &mut Node,
    on_touch: Option<&[SwitchAction]>,
    on_release: Option<&[SwitchAction]>,
    while_pressed: Option<&[SwitchAction]>,
    panels: PanelLookup,
) -> Result<(), String> {
    if n.kind() != "Switch" {
        return Err(format!("es un {}: las acciones son de los botones (Switch)", n.kind()));
    }
    let mut s = Switch::parse(n.class_data())?;
    let lists = [(&mut s.on_touch, on_touch), (&mut s.on_release, on_release), (&mut s.while_pressed, while_pressed)];
    for (dst, src) in lists {
        if let Some(src) = src {
            *dst = ops(src, panels)?.into_iter().map(|o| Action::new(o, 0)).collect();
        }
    }
    // InternalID 1, 2… en el orden del XML (al tocar, al soltar, mientras se pulsa), como los botones nuevos.
    let mut id = 1;
    for l in [&mut s.on_touch, &mut s.on_release, &mut s.while_pressed] {
        for a in l.iter_mut() {
            a.internal_id = id;
            id += 1;
        }
    }
    if let Style::Part(p) = &mut s.style {
        if let super::hmi::PartBody::Switch { actions, .. } = &mut p.body {
            let kinds = |l: &[Action]| l.iter().map(|a| a.op.kind()).collect::<Vec<u32>>();
            *actions = vec![kinds(&s.on_touch), kinds(&s.while_pressed), kinds(&s.on_release)];
        }
    }
    n.set_class_data(&s.to_bytes())
}

/// Pone `c` en la celda `col` de las filas `rows` de la biblioteca de colores del objeto, que pasa a ser
/// propia (sin depender de una biblioteca del proyecto).
fn set_cells(b: &mut ColorBlock, rows: &[u32], col: u32, c: u32) -> Result<(), String> {
    let lib = b.library.as_mut().ok_or("el objeto no lleva sus colores")?;
    let mut n = 0;
    for r in lib.rows.iter_mut().filter(|r| rows.contains(&r.row)) {
        if let Some((_, w)) = r.cells.iter_mut().find(|(k, _)| *k == col) {
            *w = Widget::Color(c);
            n += 1;
        }
    }
    if n == 0 {
        return Err("el objeto no tiene ese color".into());
    }
    b.resource.clear();
    b.flag_c = 0;
    lib.name.clear();
    Ok(())
}

pub struct ColorChange<'a> {
    pub fill: Option<&'a str>,
    pub line: Option<&'a str>,
    pub text: Option<&'a str>,
    pub on: Option<&'a str>,
    pub off: Option<&'a str>,
}

/// Cambia los colores de un objeto según su tipo.
pub fn set_colors(n: &mut Node, c: &ColorChange) -> Result<(), String> {
    let kind = n.kind();
    let not = |what: &str| Err(format!("un {kind} no tiene color `{what}`"));
    let given: Vec<(&str, &str)> =
        [("fill", c.fill), ("line", c.line), ("text", c.text), ("on", c.on), ("off", c.off)].into_iter().filter_map(|(k, v)| v.map(|v| (k, v))).collect();
    if given.is_empty() {
        return Err("no se indicó ningún color".into());
    }
    let data = n.class_data().to_vec();
    let out = match kind.as_str() {
        "Switch" => {
            let mut s = Switch::parse(&data)?;
            for (k, v) in given {
                let (rows, column) = match k {
                    "on" => (&[super::hmi::row::ON_UP][..], col::FORE_COLOR),
                    "off" => (&[super::hmi::row::OFF_UP][..], col::FORE_COLOR),
                    "text" => (&[super::hmi::row::ON_UP, super::hmi::row::OFF_UP][..], col::TEXT_COLOR),
                    _ => return not(k),
                };
                set_cells(&mut s.colors, rows, column, solid(v)?)?;
            }
            s.to_bytes()
        }
        "Lamp" => {
            let mut l = Lamp::parse(&data)?;
            for (k, v) in given {
                let (rows, column) = match k {
                    "on" => (&[rm::ROW_ON_UP][..], rm::COL_FORE),
                    "off" => (&[rm::ROW_OFF_UP][..], rm::COL_FORE),
                    "text" => (&[rm::ROW_ON_UP, rm::ROW_OFF_UP][..], rm::COL_TEXT),
                    _ => return not(k),
                };
                set_cells(&mut l.colors, rows, column, solid(v)?)?;
            }
            l.to_bytes()
        }
        "DataDisplay" => {
            let mut d = DataDisplay::parse(&data)?;
            for (k, v) in given {
                let column = match k {
                    "fill" => rm::COL_PLATE,
                    "text" => rm::COL_TEXT,
                    _ => return not(k),
                };
                set_cells(&mut d.colors, &[rm::ROW_NORMAL], column, solid(v)?)?;
            }
            d.to_bytes()
        }
        "Text" => {
            let mut t = Text::parse(&data)?;
            for (k, v) in given {
                match k {
                    "text" => t.text_color = Color(solid(v)?),
                    "fill" => (t.back_color, t.back_trans) = paint(color(v)?.map(Color)),
                    "line" => (t.line_color, t.line_trans) = paint(color(v)?.map(Color)),
                    _ => return not(k),
                }
            }
            t.write()
        }
        "Rectangle" => {
            let mut r = Rectangle::parse(&data)?;
            for (k, v) in given {
                match k {
                    "fill" => (r.fore_color, r.fore_trans) = paint(color(v)?.map(Color)),
                    "line" => (r.line_color, r.line_trans) = paint(color(v)?.map(Color)),
                    _ => return not(k),
                }
            }
            r.write()
        }
        "Circle" => {
            let mut e = Ellipse::parse(&data)?;
            for (k, v) in given {
                match k {
                    "fill" => (e.fore_color, e.fore_trans) = paint(color(v)?.map(Color)),
                    "line" => (e.line_color, e.line_trans) = paint(color(v)?.map(Color)),
                    _ => return not(k),
                }
            }
            e.write()
        }
        "Line" => {
            let mut l = Line::parse(&data)?;
            for (k, v) in given {
                match k {
                    "line" => l.line_color = Color(solid(v)?),
                    _ => return not(k),
                }
            }
            l.write()
        }
        _ => return Err(format!("los colores de un {kind} no se pueden cambiar todavía")),
    };
    n.set_class_data(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panels(k: RefKind, p: &str) -> Result<String, String> {
        match (k, p) {
            (RefKind::Base, "Cabinet") => Ok("2".into()),
            (RefKind::Popup, "Errores") => Ok("10001".into()),
            (RefKind::Script, src) => Ok(format!("end:{{{src}}}")),
            _ => Err(format!("no hay panel {p}")),
        }
    }

    fn obj(v: serde_json::Value) -> NewObject {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn colors_are_rrggbb() {
        assert_eq!(color("00FF00").unwrap(), Some(0x00FF00));
        assert_eq!(color("#FF0000").unwrap(), Some(0x0000FF));
        assert_eq!(color("626469").unwrap(), Some(0x696462));
        assert_eq!(color("none").unwrap(), None);
        assert!(color("F00").is_err());
    }

    #[test]
    fn creates_every_kind_and_reads_it_back() {
        let all = [
            serde_json::json!({"type": "switch", "label": "Marcha", "on_touch": [{"op": "momentary_on", "variable": "A.xCmd"}],
                "on_release": [{"op": "change_panel", "panel": "Cabinet"}], "lamp": "A.xStat", "on_color": "FF0000"}),
            serde_json::json!({"type": "lamp", "variable": "A.xStat", "on_label": "ON", "off_label": "OFF"}),
            serde_json::json!({"type": "lamp", "variable": "A.xStat", "shape": "round"}),
            serde_json::json!({"type": "numeric_display", "variable": "A.rTemp", "float": true, "input": true, "min": "0", "max": "A.rMax"}),
            serde_json::json!({"type": "string_display", "variable": "A.sName", "length": 20}),
            serde_json::json!({"type": "text", "text": "Hola\nmundo", "font": "medium", "bold": true, "background": "FFFFFF"}),
            serde_json::json!({"type": "rectangle", "fill": "none", "corner": 0}),
            serde_json::json!({"type": "ellipse", "fill": "00FF00"}),
            serde_json::json!({"type": "line", "end_x": 5, "end_y": 80, "arrows": "end"}),
        ];
        for (i, v) in all.into_iter().enumerate() {
            let o = obj(v);
            let n = new_object(&o, &format!("Obj{i}"), 100, 50, None, None, None, &panels).unwrap();
            let data = n.class_data();
            let back = match n.kind().as_str() {
                "Switch" => Switch::parse(data).map(|s| s.to_bytes()),
                "Lamp" => Lamp::parse(data).map(|s| s.to_bytes()),
                "DataDisplay" => DataDisplay::parse(data).map(|s| s.to_bytes()),
                "Text" => Text::parse(data).map(|s| s.write()),
                "Rectangle" => Rectangle::parse(data).map(|s| s.write()),
                "Circle" => Ellipse::parse(data).map(|s| s.write()),
                "Line" => Line::parse(data).map(|s| s.write()),
                k => panic!("tipo {k}"),
            }
            .unwrap();
            assert_eq!(back, data, "{}", n.kind());
            assert_eq!(n.name(), format!("Obj{i}"));
            let r = n.rect();
            if n.kind() == "Line" {
                assert_eq!(r, [5, 50, 100, 80]);
            } else {
                assert_eq!((r[0], r[1]), (100, 50));
            }
        }
    }

    #[test]
    fn rejects_bad_objects() {
        let bad = [
            serde_json::json!({"type": "switch", "label": "X", "on_touch": [{"op": "change_panel", "panel": "Nada"}]}),
            serde_json::json!({"type": "switch", "label": "X", "on_touch": [{"op": "set", "variable": "mal nombre"}]}),
            serde_json::json!({"type": "lamp", "variable": "A.x", "shape": "round", "label": "no"}),
            serde_json::json!({"type": "numeric_display", "variable": "A.x", "min": "0", "max": "10"}),
            serde_json::json!({"type": "rectangle", "fill": "verde"}),
        ];
        for v in bad {
            assert!(new_object(&obj(v.clone()), "X", 0, 0, None, None, None, &panels).is_err(), "{v}");
        }
    }

    #[test]
    fn script_action() {
        let json = serde_json::json!([{"op": "script", "script": "Sys.beep();\nA.x.write(1);"}]);
        let a: Vec<SwitchAction> = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(scripts(&a), ["Sys.beep();\r\nA.x.write(1);"]);
        let o = obj(serde_json::json!({"type": "switch", "label": "S", "on_touch": json}));
        let n = new_object(&o, "B", 0, 0, None, None, None, &panels).unwrap();
        let s = Switch::parse(n.class_data()).unwrap();
        assert_eq!(s.to_bytes(), n.class_data());
        assert_eq!(
            s.on_touch[0].op,
            Op::Script { source: "Sys.beep();\r\nA.x.write(1);".into(), compiled: "end:{Sys.beep();\r\nA.x.write(1);}".into() }
        );
        let empty = obj(serde_json::json!({"type": "switch", "label": "S", "on_touch": [{"op": "script", "script": "  \n"}]}));
        assert!(new_object(&empty, "B", 0, 0, None, None, None, &panels).is_err());
    }

    #[test]
    fn popup_actions() {
        let o = obj(serde_json::json!({"type": "switch", "label": "Pop",
            "on_touch": [{"op": "open_popup", "popup": "Errores", "x": 30, "y": 25}, {"op": "open_popup", "popup": "Errores"}],
            "on_release": [{"op": "close_popup", "popup": "Errores"}]}));
        let n = new_object(&o, "B", 0, 0, None, None, None, &panels).unwrap();
        let s = Switch::parse(n.class_data()).unwrap();
        assert_eq!(s.to_bytes(), n.class_data());
        let touch: Vec<&Op> = s.on_touch.iter().map(|a| &a.op).collect();
        match touch[..] {
            [Op::PopupOpen { x, y, target }, Op::PopupOpenCentered(c)] => {
                assert_eq!((x.raw.as_str(), y.raw.as_str(), target.as_str(), c.as_str()), ("30", "25", "10001", "10001"));
                assert_eq!((x.l24, x.l28, x.l2c, x.l30), (0x40, 5, 1, 0x40));
            }
            _ => panic!("{touch:?}"),
        }
        assert_eq!(s.on_release[0].op, Op::PopupClose("10001".into()));
        assert_eq!(s.on_touch.iter().chain(&s.on_release).map(|a| a.internal_id).collect::<Vec<_>>(), [1, 2, 3]);
        let bad = [
            serde_json::json!({"op": "open_popup", "popup": "Errores", "x": 30}),
            serde_json::json!({"op": "open_popup", "popup": "Cabinet"}),
            serde_json::json!({"op": "close_popup"}),
        ];
        for a in bad {
            let o = serde_json::from_value::<NewObject>(serde_json::json!({"type": "switch", "label": "X", "on_touch": [a.clone()]}));
            assert!(o.map_err(|e| e.to_string()).and_then(|o| new_object(&o, "X", 0, 0, None, None, None, &panels).map(|_| ())).is_err(), "{a}");
        }
    }

    #[test]
    fn animations_merge_and_remove() {
        let o = obj(serde_json::json!({"type": "rectangle"}));
        let mut n = new_object(&o, "R", 0, 0, None, None, None, &panels).unwrap();
        let specs: Vec<AnimationSpec> = serde_json::from_value(serde_json::json!([
            {"type": "visibility", "variable": "A.xVis"},
            {"type": "blink", "variable": "A.xAlarm", "fast": true},
            {"type": "color", "variable": "A.xRun", "on_color": "00FF00", "off_color": "none"},
            {"type": "fill", "variable": "A.iLevel", "min": 0, "max": 100},
            {"type": "position", "variable": "A.iPos", "min": 0, "max": 1000, "dx": 200}
        ]))
        .unwrap();
        assert_eq!(animate(&mut n, &specs).unwrap().len(), 5);
        let list = animations_of(&n).unwrap();
        let kinds: Vec<Kind> = list.iter().filter_map(|a| a.kind()).collect();
        assert_eq!(kinds, [Kind::Color, Kind::Fill, Kind::Position, Kind::Visibility]);
        let Animation::Visibility(v) = &list[3] else { panic!() };
        assert!(v.visible && v.blink && v.blink_speed == 0);
        assert!(n.strings().iter().any(|s| s.contains("A.xAlarm")));
        // Se vuelve a leer igual (lo que Vijeo carga sin colocarse con el tamaño).
        assert_eq!(anim::write_ext(&list).unwrap(), n.extension().unwrap());
        assert_eq!(remove_animations(&mut n, &[AnimationKind::Fill]).unwrap(), 1);
        assert_eq!(remove_animations(&mut n, &[]).unwrap(), 3);
        assert_eq!(n.extension().unwrap(), EMPTY_EXTENSION);
    }

    #[test]
    fn edits_actions_and_colors() {
        let o = obj(serde_json::json!({"type": "switch", "label": "X", "on_touch": [{"op": "set", "variable": "A.x"}]}));
        let mut n = new_object(&o, "B", 0, 0, None, None, None, &panels).unwrap();
        let rel: Vec<SwitchAction> = serde_json::from_value(serde_json::json!([{"op": "word_add", "variable": "A.i", "value": "5"}])).unwrap();
        set_actions(&mut n, None, Some(&rel), None, &panels).unwrap();
        let s = Switch::parse(n.class_data()).unwrap();
        assert_eq!((s.on_touch.len(), s.on_release.len()), (1, 1));
        assert_eq!(s.on_release[0].internal_id, 2);
        // Igual que si se hubiera creado con esas acciones.
        let o2 = obj(serde_json::json!({"type": "switch", "label": "X", "on_touch": [{"op": "set", "variable": "A.x"}],
            "on_release": [{"op": "word_add", "variable": "A.i", "value": "5"}]}));
        let n2 = new_object(&o2, "B", 0, 0, None, None, None, &panels).unwrap();
        assert_eq!(n.class_data(), n2.class_data());
        set_colors(&mut n, &ColorChange { fill: None, line: None, text: Some("FFFFFF"), on: Some("0000FF"), off: None }).unwrap();
        let s = Switch::parse(n.class_data()).unwrap();
        let lib = s.colors.library.unwrap();
        let on = StateColors::from_row(lib.rows.iter().find(|r| r.row == super::super::hmi::row::ON_UP).unwrap()).unwrap();
        assert_eq!((on.fore, on.text), (0xFF0000, 0xFFFFFF));
        assert!(set_colors(&mut n, &ColorChange { fill: Some("FFFFFF"), line: None, text: None, on: None, off: None }).is_err());
    }
}
