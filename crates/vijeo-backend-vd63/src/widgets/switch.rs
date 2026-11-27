//! Datos de clase del Switch (botón) versión 6 de Vijeo-Designer 6.3 (Switch.dll FUN_618ad920 →
//! FUN_6189fb20): lo que va tras la cabecera común del objeto (marca 0x1CED50DA).
//! Gramática y correspondencia con el XML en NOTAS.md.

use super::action::{self, Action, Op};
use super::bin::{Reader, Writer, R};
use super::expr::Expr;
use super::hmi::{ColorBlock, ImageInfo, ImageStorage, LabelStorage, LanguageFont, NativePart};

pub const SWITCH_VERSION: u32 = 6;
/// Flags de guardado del portapapeles (bit 0: incrustar bibliotecas, imágenes y parte nativa; bit 3).
pub const CLIPBOARD_SAVE_FLAGS: u32 = 9;

/// Bits de la máscara u16 que sigue a los flags de guardado (nombres de los atributos del XML;
/// los marcados «sin corpus» se deducen de Switch.dll, no hay ejemplos).
pub mod mask {
    /// Hay `LampControl` (Mode="WithLamp").
    pub const LAMP: u16 = 1 << 0;
    /// Hay `MonitorTag` (MonitorEnabled). Sin corpus.
    pub const MONITOR: u16 = 1 << 1;
    /// `ReverseOnTouch` (sin datos). Sin corpus (siempre false).
    pub const REVERSE_ON_TOUCH: u16 = 1 << 2;
    /// Hay `InterlockControl` (InterlockEnabled). Sin corpus en los clips.
    pub const INTERLOCK: u16 = 1 << 3;
    /// Hay `VisibilityAnimation VisibilityControl`.
    pub const VISIBILITY: u16 = 1 << 4;
    /// Hay `BlinkControl` + u16 `BlinkSpeed`. Sin corpus.
    pub const BLINK: u16 = 1 << 5;
    /// Botón de radio: u16 RadioGroupID, u16 RadioDelay, `RadioTag`. Sin corpus.
    pub const RADIO: u16 = 1 << 6;
    /// RadioDelayCheck. Sin corpus.
    pub const RADIO_DELAY: u16 = 1 << 7;
    /// `BuzzerOnTouchEnabled`.
    pub const BUZZER: u16 = 1 << 8;
    /// `HoldDelayEnabled` (el valor va siempre en `hold_delay`).
    pub const HOLD_DELAY: u16 = 1 << 9;
}

/// Estilo del botón: con save_flags & 1 la copia de la parte nativa; si no, sólo su nombre.
#[derive(Debug, Clone, PartialEq)]
pub enum Style {
    Part(NativePart),
    Name(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Radio {
    pub group_id: u16,
    pub delay: u16,
    pub tag: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Switch {
    pub version: u32,
    pub save_flags: u32,
    /// Máscara (ver `mask`). Los bits de datos se recalculan al escribir; el resto se escribe tal cual.
    pub mask: u16,
    pub lamp: Option<Expr>,
    pub monitor: Option<Expr>,
    pub interlock: Option<Expr>,
    pub visibility: Option<Expr>,
    /// (BlinkControl, BlinkSpeed)
    pub blink: Option<(Expr, u16)>,
    pub radio: Option<Radio>,
    /// HoldDelayValue (1 en todo el corpus; se escribe aunque el bit HOLD_DELAY esté a 0).
    pub hold_delay: u16,
    /// SecurityGroup ("" en todo el corpus).
    pub security_group: String,
    /// WhenTouch, WhenRelease y WhileTouch del XML (en el flujo van WhenTouch, WhileTouch, WhenRelease).
    pub on_touch: Vec<Action>,
    pub on_release: Vec<Action>,
    pub while_pressed: Vec<Action>,
    pub colors: ColorBlock,
    pub labels: LabelStorage,
    /// Imágenes por estado (vacías salvo en botones de categoría Bitmap).
    pub images: ImageStorage,
    /// UseOverlayOn + imagen superpuesta «On».
    pub use_overlay_on: u16,
    pub overlay_on: ImageInfo,
    /// UseOverlayOff + imagen superpuesta «Off» (XML `<OverlayImage StateName="Off">`).
    pub use_overlay_off: u16,
    pub overlay_off: ImageInfo,
    /// IsOverlayTrans + OverlayTransColor (por defecto 0 y 0x7B08FF).
    pub overlay_trans: u16,
    pub overlay_trans_color: u32,
    pub style: Style,
}

impl Switch {
    pub fn parse(b: &[u8]) -> R<Switch> {
        let mut r = Reader::new(b);
        let s = Switch::parse_from(&mut r)?;
        if r.left() != 0 {
            return Err(format!("sobran {} B en {:#x}", r.left(), r.p));
        }
        Ok(s)
    }

    pub fn parse_from(r: &mut Reader) -> R<Switch> {
        r.expect_u32(SWITCH_VERSION, "versión del Switch")?;
        let save_flags = r.u32()?;
        let m = r.u16()?;
        let opt = |r: &mut Reader, bit: u16| -> R<Option<Expr>> {
            if m & bit != 0 {
                Ok(Some(Expr::parse(r)?))
            } else {
                Ok(None)
            }
        };
        let lamp = opt(r, mask::LAMP)?;
        let monitor = opt(r, mask::MONITOR)?;
        let interlock = opt(r, mask::INTERLOCK)?;
        let visibility = opt(r, mask::VISIBILITY)?;
        let blink = if m & mask::BLINK != 0 { Some((Expr::parse(r)?, r.u16()?)) } else { None };
        let radio = if m & mask::RADIO != 0 {
            Some(Radio { group_id: r.u16()?, delay: r.u16()?, tag: Expr::parse(r)? })
        } else {
            None
        };
        let hold_delay = r.u16()?;
        let security_group = r.bstr()?;
        let on_touch = action::parse_list(r)?;
        // Orden del flujo comprobado en Vijeo: la segunda lista es WhileTouch y la tercera WhenRelease.
        let while_pressed = action::parse_list(r)?;
        let on_release = action::parse_list(r)?;
        let colors = ColorBlock::parse(r)?;
        let labels = LabelStorage::parse(r, save_flags)?;
        let images = ImageStorage::parse(r, save_flags)?;
        let use_overlay_on = r.u16()?;
        let overlay_on = ImageInfo::parse(r, save_flags)?;
        let use_overlay_off = r.u16()?;
        let overlay_off = ImageInfo::parse(r, save_flags)?;
        let overlay_trans = r.u16()?;
        let overlay_trans_color = r.u32()?;
        let style = if save_flags & 1 != 0 { Style::Part(NativePart::parse(r)?) } else { Style::Name(r.bstr()?) };
        Ok(Switch {
            version: SWITCH_VERSION,
            save_flags,
            mask: m,
            lamp,
            monitor,
            interlock,
            visibility,
            blink,
            radio,
            hold_delay,
            security_group,
            on_touch,
            on_release,
            while_pressed,
            colors,
            labels,
            images,
            use_overlay_on,
            overlay_on,
            use_overlay_off,
            overlay_off,
            overlay_trans,
            overlay_trans_color,
            style,
        })
    }

    /// Máscara efectiva: los bits con datos salen de los campos presentes; el resto, de `mask`.
    pub fn effective_mask(&self) -> u16 {
        let data = mask::LAMP | mask::MONITOR | mask::INTERLOCK | mask::VISIBILITY | mask::BLINK | mask::RADIO;
        let mut m = self.mask & !data;
        for (bit, on) in [
            (mask::LAMP, self.lamp.is_some()),
            (mask::MONITOR, self.monitor.is_some()),
            (mask::INTERLOCK, self.interlock.is_some()),
            (mask::VISIBILITY, self.visibility.is_some()),
            (mask::BLINK, self.blink.is_some()),
            (mask::RADIO, self.radio.is_some()),
        ] {
            if on {
                m |= bit;
            }
        }
        m
    }

    pub fn write(&self, w: &mut Writer) {
        w.u32(self.version);
        w.u32(self.save_flags);
        w.u16(self.effective_mask());
        for e in [&self.lamp, &self.monitor, &self.interlock, &self.visibility].into_iter().flatten() {
            e.write(w);
        }
        if let Some((e, speed)) = &self.blink {
            e.write(w);
            w.u16(*speed);
        }
        if let Some(r) = &self.radio {
            w.u16(r.group_id);
            w.u16(r.delay);
            r.tag.write(w);
        }
        w.u16(self.hold_delay);
        w.bstr(&self.security_group);
        action::write_list(w, &self.on_touch);
        action::write_list(w, &self.while_pressed);
        action::write_list(w, &self.on_release);
        self.colors.write(w);
        self.labels.write(w);
        self.images.write(w);
        w.u16(self.use_overlay_on);
        self.overlay_on.write(w);
        w.u16(self.use_overlay_off);
        self.overlay_off.write(w);
        w.u16(self.overlay_trans);
        w.u32(self.overlay_trans_color);
        match &self.style {
            Style::Part(p) => p.write(w),
            Style::Name(n) => w.bstr(n),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        self.write(&mut w);
        w.o
    }

    /// Botón nuevo con los valores más comunes del corpus (ver «Botón por defecto» en NOTAS.md).
    pub fn button(spec: &ButtonSpec) -> Switch {
        let label = spec.label.map(|t| {
            LabelStorage::static_text(t, spec.font.clone().unwrap_or_else(LanguageFont::modern_6x10), 1, 1, 0)
        });
        let kinds = |l: &[Op]| l.iter().map(|o| o.kind()).collect::<Vec<u32>>();
        let acts = |l: &[Op], first: u32| {
            l.iter().enumerate().map(|(i, o)| Action::new(o.clone(), first + i as u32)).collect::<Vec<_>>()
        };
        let on_touch = acts(&spec.on_touch, 1);
        let on_release = acts(&spec.on_release, 1 + on_touch.len() as u32);
        let while_pressed = acts(&spec.while_pressed, 1 + (on_touch.len() + on_release.len()) as u32);
        let mut m = 0;
        if spec.buzzer {
            m |= mask::BUZZER;
        }
        let mut s = Switch {
            version: SWITCH_VERSION,
            save_flags: CLIPBOARD_SAVE_FLAGS,
            mask: m,
            lamp: spec.lamp.map(Expr::cond),
            monitor: None,
            interlock: spec.interlock.map(Expr::cond),
            visibility: spec.visibility.map(Expr::cond),
            blink: None,
            radio: None,
            hold_delay: 1,
            security_group: String::new(),
            on_touch,
            on_release,
            while_pressed,
            colors: spec.colors.clone(),
            labels: label.clone().unwrap_or_else(LabelStorage::none),
            images: ImageStorage::empty(2),
            use_overlay_on: 0,
            overlay_on: ImageInfo::empty(),
            use_overlay_off: 0,
            overlay_off: ImageInfo::empty(),
            overlay_trans: 0,
            overlay_trans_color: 0x7B08FF,
            style: Style::Part(NativePart::switch(
                spec.style,
                [kinds(&spec.on_touch), kinds(&spec.while_pressed), kinds(&spec.on_release)],
                label.is_some(),
                spec.lamp.is_some(),
                spec.visibility.is_some(),
                spec.interlock.is_some(),
            )),
        };
        s.mask = s.effective_mask();
        s
    }
}

/// Parámetros de un botón nuevo (`Switch::button`).
#[derive(Debug, Clone)]
pub struct ButtonSpec<'a> {
    /// Texto de la etiqueta (idioma 1); `None` sin etiqueta.
    pub label: Option<&'a str>,
    /// Fuente de la etiqueta; por defecto GPFontModern6x10.
    pub font: Option<LanguageFont>,
    pub on_touch: Vec<Op>,
    pub on_release: Vec<Op>,
    pub while_pressed: Vec<Op>,
    /// Variable de la lámpara (`LampControl`), p. ej. `MyController.System.xStatManMode`.
    pub lamp: Option<&'a str>,
    /// Variable de enclavamiento (`InterlockControl`).
    pub interlock: Option<&'a str>,
    /// Variable de visibilidad (`VisibilityControl`).
    pub visibility: Option<&'a str>,
    pub colors: ColorBlock,
    /// Estilo nativo (fichero de `Vijeo-Frame\NativeStyles\Primitive`): «Square03» por defecto.
    pub style: &'a str,
    pub buzzer: bool,
}
