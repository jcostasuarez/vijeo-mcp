//! Lamp (Lamp.dll), datos de clase versión 2: lámparas de 2 estados (`LampType="Lamp"`) y de N estados
//! (`LampType="NStateLamp"`), con colores, etiquetas e imágenes por estado. Gramática completa en NOTAS.md.
//!
//! ```text
//! u32 2                    versión de la clase
//! u32 save_flags           9 en el portapapeles (bit 0: incrusta bibliotecas, imágenes y estilo nativo)
//! u8  flags                bit 0 NStateLamp (+0x1BE), bit 1 VisibilityEnabled, bit 2 BlinkEnabled,
//!                          bit 3 InvisibleWhenInvalid (+0x124)
//! u16 states               NumOfStates (+0x268; 2 en LampType="Lamp")
//! u16 control              +0xE8: 0 bits (Variable / Bit), 1 WordByBitVariable, 2 WordByValueVariable
//! control 0: CExpression bit0 [+ bit1 si states > 2, bit2 si > 4, bit3 si > 8, bit4 si > 16]
//! control≠0: CExpression valor de la palabra
//! flags&2: CExpression     VisibilityControl (+0x16C)
//! flags&4: CExpression + u16   BlinkControl (+0x168) + BlinkSpeed (+0x140)
//! bloque de color          hmi::ColorBlock (FUN_610b1a20; colores por estado)
//! CLabelStorage            hmi::LabelStorage (etiquetas por estado)
//! CImageStorage            hmi::ImageStorage (imágenes por estado)
//! save_flags&1: CNativePart (estilo nativo, `hmi::NativePart`); si no: BSTR nombre del estilo
//! ```
//! Cuerpo: Lamp.dll FUN_610b5c80; envoltorio (versión, save_flags, estilo): FUN_610c06f0.

use super::bin::{Reader, Writer, R};
use super::expr::Expr;
use super::hmi::{
    label_kind, ColorBlock, ColorLibrary, ImageInfo, ImageStorage, LabelInfo, LabelStorage, LanguageData,
    LanguageDataList, LanguageFont, NativePart, PartBody, Row, Widget,
};

pub const LAMP_VERSION: u32 = 2;

/// GUID de clase de Lamp (primeros 16 bytes del objeto).
pub const LAMP_CLSID: [u8; 16] =
    [0xf0, 0xd9, 0x34, 0x63, 0x2c, 0x69, 0x22, 0x40, 0xb6, 0xac, 0xd8, 0x9f, 0x10, 0xaa, 0x26, 0xbb];

/// Filas (eRM_ROW) y columnas (eRM_COLUMN) de las bibliotecas de colores, según las tablas de
/// HMIManagers.dll (`CResourceManager::SaveXMLRow` y la tabla nombre↔valor de `StateType`).
pub mod rm {
    pub const ROW_INVALID: u32 = 0;
    pub const ROW_NORMAL: u32 = 1;
    pub const ROW_MAX: u32 = 2;
    pub const ROW_HIHI: u32 = 3;
    pub const ROW_HI: u32 = 4;
    pub const ROW_LO: u32 = 5;
    pub const ROW_LOLO: u32 = 6;
    pub const ROW_MIN: u32 = 7;
    /// Filas «de estado» (segunda lista de la biblioteca, una por estado de NStateLamp).
    pub const ROW_STATE: u32 = 8;
    pub const ROW_ON_UP: u32 = 9;
    pub const ROW_OFF_UP: u32 = 10;
    pub const ROW_ON_DOWN: u32 = 11;
    pub const ROW_OFF_DOWN: u32 = 12;

    pub const COL_TEXT: u32 = 0x100;
    /// Ojo: 0x101 es Color3D y 0x109 BackColor (verificado con SwitchCMD_08 de MB_Start: Color3D=C0C0C0).
    pub const COL_COLOR_3D: u32 = 0x101;
    pub const COL_INDICATOR: u32 = 0x102;
    pub const COL_IND_BACK: u32 = 0x103;
    pub const COL_FRAME: u32 = 0x104;
    pub const COL_PLATE: u32 = 0x105;
    pub const COL_SCALE: u32 = 0x106;
    pub const COL_THRESHOLD: u32 = 0x107;
    pub const COL_FORE: u32 = 0x108;
    pub const COL_BACK: u32 = 0x109;
    pub const COL_BLINK: u32 = 0x200;
    pub const COL_PATTERN: u32 = 0x700;

    /// Tipo de objeto de la biblioteca (campo `v28` de CColorLibrary y `v14` de CImageStorage).
    pub const OBJ_SWITCH: u32 = 16;
    pub const OBJ_LAMP: u32 = 32;
    pub const OBJ_DATA_DISPLAY: u32 = 64;

    /// Parpadeo (Blink / BlinkSpeed): None 0, Slow 1, Fast 2.
    pub const BLINK_NONE: u8 = 0;
    pub const BLINK_SLOW: u8 = 1;
    pub const BLINK_FAST: u8 = 2;
}

/// Origen del estado (u16 `control`, +0xE8).
pub mod control {
    /// Una expresión por bit del número de estado: LampTag (2 estados) o elementos `<Bit Variable=…>`.
    pub const BITS: u16 = 0;
    /// `WordByBitVariable`: el estado es el número del bit activo de una palabra.
    pub const WORD_BY_BIT: u16 = 1;
    /// `WordByValueVariable`: el estado es el valor de una palabra.
    pub const WORD_BY_VALUE: u16 = 2;
}

/// Origen del estado de la lámpara.
#[derive(Debug, Clone, PartialEq)]
pub enum LampSource {
    /// `control` 0: una expresión por bit del número de estado (bit 0 primero). Con 2 estados, una sola
    /// condición booleana (XML `Variable`).
    Bits(Vec<Expr>),
    /// `control` 1 (WordByBit) o 2 (WordByValue): valor entero de una palabra.
    Word { control: u16, expr: Expr },
}

/// Estilo de la lámpara: copia de la parte nativa (portapapeles) o sólo el nombre.
#[derive(Debug, Clone, PartialEq)]
pub enum Style {
    Native(NativePart),
    Name(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lamp {
    pub save_flags: u32,
    /// LampType: false «Lamp» (2 estados), true «NStateLamp».
    pub n_state: bool,
    /// InvisibleWhenInvalid (sólo NStateLamp).
    pub invisible_when_invalid: bool,
    /// NumOfStates.
    pub states: u16,
    pub source: LampSource,
    /// VisibilityControl (flags bit 1).
    pub visibility: Option<Expr>,
    /// BlinkControl + BlinkSpeed (flags bit 2).
    pub blink: Option<(Expr, u16)>,
    pub colors: ColorBlock,
    pub labels: LabelStorage,
    pub images: ImageStorage,
    pub style: Style,
}

/// Número de expresiones de bit para `states` estados (umbrales 2, 4, 8, 16 de Lamp.dll).
pub fn bit_count(states: u16) -> usize {
    1 + [2u16, 4, 8, 16].iter().filter(|&&t| states > t).count()
}

impl Lamp {
    pub fn parse(b: &[u8]) -> R<Lamp> {
        let mut r = Reader::new(b);
        let l = Lamp::read(&mut r)?;
        if r.left() != 0 {
            return Err(format!("sobran {} B tras la lámpara en {:#x}", r.left(), r.p));
        }
        Ok(l)
    }

    pub fn read(r: &mut Reader) -> R<Lamp> {
        r.expect_u32(LAMP_VERSION, "versión de Lamp")?;
        let save_flags = r.u32()?;
        let at = r.p;
        let flags = r.u8()?;
        if flags & 0xF0 != 0 {
            return Err(format!("flags de Lamp {flags:#x} desconocidos en {at:#x}"));
        }
        let states = r.u16()?;
        let control = r.u16()?;
        let source = if control == control::BITS {
            let mut v = Vec::new();
            for _ in 0..bit_count(states) {
                v.push(Expr::parse(r)?);
            }
            LampSource::Bits(v)
        } else {
            LampSource::Word { control, expr: Expr::parse(r)? }
        };
        let visibility = if flags & 2 != 0 { Some(Expr::parse(r)?) } else { None };
        let blink = if flags & 4 != 0 { Some((Expr::parse(r)?, r.u16()?)) } else { None };
        let colors = ColorBlock::parse(r)?;
        let labels = LabelStorage::parse(r, save_flags)?;
        let images = ImageStorage::parse(r, save_flags)?;
        let style = if save_flags & 1 != 0 { Style::Native(NativePart::parse(r)?) } else { Style::Name(r.bstr()?) };
        Ok(Lamp {
            save_flags,
            n_state: flags & 1 != 0,
            invisible_when_invalid: flags & 8 != 0,
            states,
            source,
            visibility,
            blink,
            colors,
            labels,
            images,
            style,
        })
    }

    pub fn flags(&self) -> u8 {
        self.n_state as u8
            | (self.visibility.is_some() as u8) << 1
            | (self.blink.is_some() as u8) << 2
            | (self.invisible_when_invalid as u8) << 3
    }

    pub fn write(&self, w: &mut Writer) {
        w.u32(LAMP_VERSION);
        w.u32(self.save_flags);
        w.u8(self.flags());
        w.u16(self.states);
        match &self.source {
            LampSource::Bits(v) => {
                w.u16(control::BITS);
                for e in v {
                    e.write(w);
                }
            }
            LampSource::Word { control, expr } => {
                w.u16(*control);
                expr.write(w);
            }
        }
        if let Some(e) = &self.visibility {
            e.write(w);
        }
        if let Some((e, v)) = &self.blink {
            e.write(w);
            w.u16(*v);
        }
        self.colors.write(w);
        self.labels.write(w);
        self.images.write(w);
        match &self.style {
            Style::Native(p) => p.write(w),
            Style::Name(s) => w.bstr(s),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        self.write(&mut w);
        w.o
    }
}

// ---------------------------------------------------------------------------------------------
// Creación de lámparas nuevas
// ---------------------------------------------------------------------------------------------

/// Colores de un estado de lámpara (atributos de `<State>` del XML; COLORREF 0x00BBGGRR).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LampColors {
    pub fore: u32,
    pub back: u32,
    pub frame: u32,
    pub text: u32,
    pub color_3d: u32,
    /// Pattern (1 = sólido).
    pub pattern: u8,
    /// Blink (0 None, 1 Slow, 2 Fast).
    pub blink: u8,
}

impl LampColors {
    /// Color de relleno `fore` con marco 707070 y texto negro (como las lámparas del corpus).
    pub fn fill(fore: u32) -> LampColors {
        LampColors { fore, back: 0, frame: 0x707070, text: 0, color_3d: 0, pattern: 1, blink: 0 }
    }
    /// Estado inválido por defecto: gris C0C0C0, texto blanco.
    pub fn invalid() -> LampColors {
        LampColors { fore: 0xC0C0C0, back: 0, frame: 0x707070, text: 0xFFFFFF, color_3d: 0, pattern: 1, blink: 0 }
    }
    /// Fila de la biblioteca, con las celdas en el orden del corpus (PK_Parameters Lamp01).
    pub fn row(&self, row: u32) -> Row {
        Row {
            row,
            cells: vec![
                (rm::COL_PATTERN, Widget::Pattern(self.pattern)),
                (rm::COL_BLINK, Widget::Blink(self.blink)),
                (rm::COL_TEXT, Widget::Color(self.text)),
                (rm::COL_COLOR_3D, Widget::Color(self.color_3d)),
                (rm::COL_FRAME, Widget::Color(self.frame)),
                (rm::COL_FORE, Widget::Color(self.fore)),
                (rm::COL_BACK, Widget::Color(self.back)),
            ],
        }
    }
}

/// Fuente del corpus para etiquetas: GP «GPFontModern8x13» (cara 10) con Arial -19 de respaldo,
/// `bold` = FontStyle Bold.
pub fn font_8x13(bold: bool) -> LanguageFont {
    LanguageFont {
        font_type: 0,
        style: bold as u32,
        tt_height: -19,
        tt_width: if bold { 700 } else { 400 },
        tt_face: "Arial".into(),
        gp_height: 13,
        gp_width: 8,
        gp_face: 10,
        b8: 0,
        psp_size: -19,
        psp_face: 4,
    }
}

/// Fuente base de los textos (la primera de cada `LanguageData`): Arial -19, GP 8x13, en todo el corpus.
pub fn base_font() -> LanguageFont {
    font_8x13(false)
}

/// Un texto en el idioma 1; `None` = BSTR nulo (etiqueta vacía, como el estado inválido).
pub fn label(text: Option<&str>, font: &LanguageFont, string_id: u32) -> LabelInfo {
    LabelInfo {
        string_id,
        texts: Some(LanguageDataList {
            items: vec![LanguageData {
                language: "Language1".into(),
                text: text.unwrap_or("").into(),
                text_null: text.is_none(),
                base_font: base_font(),
                font: Some(font.clone()),
                tail_a: 1,
                tail_b: 0,
            }],
        }),
    }
}

/// Etiquetas de una lámpara nueva.
#[derive(Debug, Clone)]
pub enum LampLabels<'a> {
    /// Sin etiqueta.
    None,
    /// Una etiqueta para todos los estados (label_kind STATIC, XML `<Labels><Label>`).
    Static(&'a str),
    /// Texto por estado (2 estados: ON_OFF, XML `State StateName="Off"/"On"`; N estados: USER_DEFINED,
    /// XML `StateName="0".."N-1"`, más una etiqueta vacía para «Invalid»).
    PerState(Vec<&'a str>),
}

/// Estilo nativo de una lámpara nueva: nombre del .NPS y el u32 `v34` que lo acompaña en el corpus.
#[derive(Debug, Clone, Copy)]
pub struct LampStyle {
    pub name: &'static str,
    pub v34: u32,
}

/// Rectángulo con etiqueta (XML ObjectStyle 000000): MB_RFID_Overview, NStateLamp*.
pub const STYLE_BLANK_AREA: LampStyle = LampStyle { name: "BlankArea", v34: 2 };
/// Cuadrado (XML ObjectStyle 20057): PK_Parameters Lamp01..04.
pub const STYLE_SQUARE70: LampStyle = LampStyle { name: "Square70", v34: 1 };
/// Piloto redondo sin etiqueta: MB_Wireless.
pub const STYLE_ROUND04: LampStyle = LampStyle { name: "Round04", v34: 0 };

/// Parámetros de una lámpara nueva.
#[derive(Debug, Clone)]
pub struct LampSpec<'a> {
    /// Variable (`Carpeta.Var`).
    pub variable: &'a str,
    /// Colores por estado: 2 estados = [Off, On]; N estados = uno por valor 0..N-1.
    pub states: Vec<LampColors>,
    pub invalid: LampColors,
    pub labels: LampLabels<'a>,
    pub font: LanguageFont,
    /// HAlign 0 Left, 1 Center, 2 Right; VAlign 0 Top, 1 Middle, 2 Bottom.
    pub halign: u32,
    pub valign: u32,
    pub style: LampStyle,
    /// Primer id de cadena para las etiquetas (Vijeo los reasigna al pegar; ver NOTAS.md).
    pub string_id: u32,
}

impl<'a> LampSpec<'a> {
    /// Lámpara de bit: apagada gris, encendida verde, sin etiqueta, estilo BlankArea.
    pub fn bool(variable: &'a str) -> LampSpec<'a> {
        LampSpec {
            variable,
            states: vec![LampColors::fill(0xC0C0C0), LampColors::fill(0x00FF00)],
            invalid: LampColors::invalid(),
            labels: LampLabels::None,
            font: font_8x13(false),
            halign: 1,
            valign: 1,
            style: STYLE_BLANK_AREA,
            string_id: 0,
        }
    }
}

/// Valor entero de una palabra para las lámparas WordByValue/WordByBit: atributos (0x40, 0x804, 2, 0x40).
pub fn word_value(path: &str) -> Expr {
    let mut e = Expr::int_value(path);
    e.l28 = 0x804;
    e
}

fn library(rows: Vec<Row>, states: Vec<Row>) -> ColorBlock {
    ColorBlock {
        resource: String::new(),
        flag_c: 0,
        flag_d: 0,
        library: Some(ColorLibrary { name: String::new(), v28: rm::OBJ_LAMP, rows, states }),
    }
}

fn label_storage(kind: u8, labels: Vec<LabelInfo>, s: &LampSpec) -> LabelStorage {
    LabelStorage {
        kind,
        numeric: 0,
        halign: s.halign,
        valign: s.valign,
        numeric_font: None,
        labels,
        text_library: None,
        font_library: None,
    }
}

fn native(style: LampStyle, control: u16, states: u16) -> Style {
    Style::Native(NativePart {
        name: style.name.into(),
        part_type: 2,
        label_type: style.v34,
        visibility: 0,
        v1c: 0,
        blink_type: 0,
        body: PartBody::Lamp(control, states),
    })
}

fn images(flags: u8, n: usize) -> ImageStorage {
    ImageStorage { flags, transparent: 0x7B08FF, v14: rm::OBJ_LAMP, images: vec![ImageInfo::empty(); n], library: None }
}

impl Lamp {
    /// Lámpara de 2 estados (LampType «Lamp») sobre una variable booleana: encendida si `!= 0`.
    /// `s.states` = [apagada, encendida]. Etiquetas: None, Static o PerState([off, on]).
    pub fn new_bool(s: &LampSpec) -> Lamp {
        assert_eq!(s.states.len(), 2, "una lámpara de bit tiene 2 estados");
        let (kind, labels) = match &s.labels {
            LampLabels::None => (label_kind::NONE, vec![]),
            LampLabels::Static(t) => (label_kind::STATIC, vec![label(Some(t), &s.font, s.string_id)]),
            LampLabels::PerState(v) => {
                assert_eq!(v.len(), 2, "On/Off: dos textos");
                (
                    label_kind::ON_OFF,
                    v.iter().enumerate().map(|(i, t)| label(Some(t), &s.font, s.string_id + i as u32)).collect(),
                )
            }
        };
        Lamp {
            save_flags: 9,
            n_state: false,
            invisible_when_invalid: false,
            states: 2,
            source: LampSource::Bits(vec![Expr::cond(s.variable)]),
            visibility: None,
            blink: None,
            colors: library(
                vec![s.states[0].row(rm::ROW_OFF_UP), s.states[1].row(rm::ROW_ON_UP), s.invalid.row(rm::ROW_INVALID)],
                vec![],
            ),
            labels: label_storage(kind, labels, s),
            images: images(0x10, 2),
            style: native(s.style, control::BITS, 2),
        }
    }

    /// Lámpara de N estados (NStateLamp) sobre el valor de una palabra entera (WordByValueVariable):
    /// el estado k se muestra cuando la variable vale k. `s.states` = colores de los estados 0..N-1;
    /// etiquetas: None, Static o PerState (un texto por estado; se añade la etiqueta vacía de «Invalid»).
    pub fn new_word_by_value(s: &LampSpec) -> Lamp {
        let n = s.states.len();
        assert!((2..=255).contains(&n), "NumOfStates entre 2 y 255");
        let (kind, labels) = match &s.labels {
            LampLabels::None => (label_kind::NONE, vec![]),
            LampLabels::Static(t) => (label_kind::STATIC, vec![label(Some(t), &s.font, s.string_id)]),
            LampLabels::PerState(v) => {
                assert_eq!(v.len(), n, "un texto por estado");
                let mut l: Vec<LabelInfo> =
                    v.iter().enumerate().map(|(i, t)| label(Some(t), &s.font, s.string_id + i as u32)).collect();
                l.push(label(None, &font_8x13(false), s.string_id + n as u32));
                (label_kind::USER_DEFINED, l)
            }
        };
        Lamp {
            save_flags: 9,
            n_state: true,
            invisible_when_invalid: false,
            states: n as u16,
            source: LampSource::Word { control: control::WORD_BY_VALUE, expr: word_value(s.variable) },
            visibility: None,
            blink: None,
            colors: library(
                vec![
                    s.invalid.row(rm::ROW_INVALID),
                    LampColors::fill(0x0000FF).row(rm::ROW_ON_UP),
                    LampColors::fill(0x00FF00).row(rm::ROW_OFF_UP),
                ],
                s.states.iter().map(|c| c.row(rm::ROW_STATE)).collect(),
            ),
            labels: label_storage(kind, labels, s),
            images: images(0x12, n + 1),
            style: native(s.style, control::WORD_BY_VALUE, n as u16),
        }
    }
}
