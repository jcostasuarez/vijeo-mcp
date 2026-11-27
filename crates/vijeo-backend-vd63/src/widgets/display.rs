//! DataDisplay (DataDisplay.dll), datos de clase versión 8: displays numéricos (`NumericDisplay`), de
//! texto (`StringDisplay`), de fecha (`DateDisplay`) y de hora (`TimeDisplay`), con entrada opcional.
//! Gramática completa en NOTAS.md.
//!
//! ```text
//! u32 8                      versión de la clase
//! u32 save_flags             9 en el portapapeles
//! save_flags&1: CNativePart  estilo nativo («DD00001», «BlankArea»...); si no: BSTR nombre del estilo
//! u32 kind                   0 numérico, 1 texto, 2 fecha, 3 hora (+0xDC)
//! kind 0/1: CExpression      Variable
//! kind 0: u16 NumericDisplayVariableType (0 Integer, 1 Float), u32 ZeroSuppress, u32 DisplayZero,
//!         u32 DigitGrouping, u16 WholeDigits, u16 DecimalDigits, u32 (+0x3A8, siempre 1),
//!         u32 id de cadena de DisplayUnitLabel (-1: sin unidad) [+ CLanguageDataList si != -1 y save_flags&1]
//! kind 1: u16 DisplayLength
//! u16 formato                Format (Dec/Hex/Oct/Bin o Fixed/Auto/Scientific), DateFormat o TimeFormat
//! u32 InputModeEnabled
//! si hay entrada: u16 1 + u32 (+0x194/+0x198), u32 DisplayPopupKeypad, [kind 1: u16 KeypadFormat],
//!   u32 BuzzerOnTouch, u32 PasswordMode, [kind 0: u32 OverwriteInputRange + (CExpression máximo,
//!   CExpression mínimo)], u32 BarcodeEnabled + [CExpression], u32 ErrorStatusEnabled + [CExpression],
//!   [save_flags 0 o con bit 1: u16 +0x1D4, u16 +0x1DC], u32 UserInputTriggerEnabled + [CExpression],
//!   u32 InputStatusVarEnabled + [CExpression + u16 InputStatusMode]
//! 4 × (u32 + [CExpression])  HiHi, Hi, Lo, LoLo (Colors HiHiExpr...)
//! u32 + [CExpression]        VisibilityControl
//! u32 + [CExpression + u16]  BlinkControl + BlinkSpeed
//! u32 + [CExpression]        InterlockControl
//! CLabelStorage              fuente y alineación (Labels)
//! bloque de color            Colors (filas Normal, HiHi, Hi, Lo, LoLo)
//! BSTR                       SecurityGroup
//! u16, u16                   +0x450, +0x464 (0 en todo el corpus)
//! ```
//! Cuerpo: DataDisplay.dll FUN_60b45ad0; envoltorio (versión, save_flags, estilo): FUN_60b51d10.

use super::bin::{Reader, Writer, R};
use super::expr::Expr;
use super::hmi::{
    label_kind, ColorBlock, ColorLibrary, LabelStorage, LanguageDataList, LanguageFont, NativePart, PartBody, Row,
    Widget,
};
use super::lamp::{label, rm};

pub const DD_VERSION: u32 = 8;

/// GUID de clase de DataDisplay (primeros 16 bytes del objeto).
pub const DD_CLSID: [u8; 16] =
    [0xe2, 0xa5, 0xd5, 0x5f, 0x28, 0x24, 0x24, 0x40, 0x90, 0x29, 0xee, 0xf4, 0xa8, 0xff, 0x82, 0xa4];

/// Tipo de display (+0xDC).
pub mod kind {
    pub const NUMERIC: u32 = 0;
    pub const STRING: u32 = 1;
    pub const DATE: u32 = 2;
    pub const TIME: u32 = 3;
}

/// Valores de los enumerados (tablas `g_numericFormat`, `g_dateFormat`... de DataDisplay.dll).
pub mod fmt {
    /// NumericDisplayVariableType.
    pub const INTEGER: u16 = 0;
    pub const FLOAT: u16 = 1;
    /// Format de enteros.
    pub const DEC: u16 = 0;
    pub const HEX: u16 = 1;
    pub const OCT: u16 = 2;
    pub const BIN: u16 = 3;
    /// Format de reales.
    pub const FIXED: u16 = 0;
    pub const AUTO: u16 = 1;
    pub const SCIENTIFIC: u16 = 2;
    /// DateFormat.
    pub const DATE_YYYY_MM_DD: u16 = 0;
    pub const DATE_YYYY_DD_MM: u16 = 1;
    pub const DATE_MM_DD_YYYY: u16 = 2;
    pub const DATE_DD_MM_YYYY: u16 = 3;
    pub const DATE_YY_DD_MM: u16 = 4;
    pub const DATE_YY_MM_DD: u16 = 5;
    pub const DATE_MM_DD_YY: u16 = 6;
    pub const DATE_DD_MM_YY: u16 = 7;
    pub const DATE_MM_DD: u16 = 8;
    pub const DATE_DD_MM: u16 = 15;
    /// TimeFormat.
    pub const TIME_24_HMS: u16 = 9;
    pub const TIME_24_HM: u16 = 10;
    pub const TIME_12_HMS: u16 = 11;
    pub const TIME_12_HMS_AM: u16 = 12;
    pub const TIME_12_HM: u16 = 13;
    pub const TIME_12_HM_AM: u16 = 14;
    /// KeypadFormat.
    pub const STRING_KEYPAD: u16 = 0;
    pub const DECIMAL_KEYPAD: u16 = 1;
    /// InputStatusMode.
    pub const ON_OFF_REVERSE: u16 = 0;
    pub const ON_TRIGGER: u16 = 1;
    pub const OFF_TRIGGER: u16 = 2;
    pub const INPUT_STATUS_RECOGNITION: u16 = 3;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Style {
    Native(NativePart),
    Name(String),
}

/// Parte propia del display numérico (kind 0).
#[derive(Debug, Clone, PartialEq)]
pub struct Numeric {
    /// NumericDisplayVariableType (+0x124): `fmt::INTEGER` o `fmt::FLOAT`.
    pub data_type: u16,
    pub zero_suppress: u32,
    pub display_zero: u32,
    pub digit_grouping: u32,
    pub whole_digits: u16,
    pub decimal_digits: u16,
    /// +0x3A8 (1 en todo el corpus).
    pub v3a8: u32,
    /// DisplayUnitLabel: id de cadena (-1 sin unidad) y, en el portapapeles, sus textos.
    pub unit_id: u32,
    pub unit_texts: Option<LanguageDataList>,
}

/// Opciones de entrada (InputMode), presentes si la entrada está habilitada.
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    /// Objeto +0x194: u16 versión (1) + u32 (+0x198, 0 en el corpus).
    pub field_ver: u16,
    pub field: u32,
    pub popup_keypad: u32,
    /// Sólo en displays de texto: KeypadFormat.
    pub keypad_format: Option<u16>,
    pub buzzer: u32,
    pub password: u32,
    /// Sólo en numéricos: OverwriteInputRange; si != 0, (MaxValueVariable, MinValueVariable), en ese orden.
    pub range: Option<(u32, Option<(Expr, Expr)>)>,
    pub barcode: Opt,
    pub error_status: Opt,
    /// Sólo con save_flags 0 o con el bit 1 (nunca en el portapapeles): u16 +0x1D4, u16 +0x1DC.
    pub extra: Option<(u16, u16)>,
    /// UserInputTriggerExpr.
    pub trigger: Opt,
    /// InputStatusVariable + InputStatusMode.
    pub status: OptMode,
}

/// u32 habilitado + CExpression si != 0.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Opt {
    pub enabled: u32,
    pub expr: Option<Expr>,
}

/// u32 habilitado + (CExpression + u16) si != 0.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OptMode {
    pub enabled: u32,
    pub expr: Option<(Expr, u16)>,
}

impl Opt {
    pub fn some(e: Expr) -> Opt {
        Opt { enabled: 1, expr: Some(e) }
    }
}

fn read_opt(r: &mut Reader) -> R<Opt> {
    let enabled = r.u32()?;
    let expr = if enabled != 0 { Some(Expr::parse(r)?) } else { None };
    Ok(Opt { enabled, expr })
}

fn write_opt(w: &mut Writer, o: &Opt) {
    w.u32(o.enabled);
    if let Some(e) = &o.expr {
        e.write(w);
    }
}

fn read_opt_mode(r: &mut Reader) -> R<OptMode> {
    let enabled = r.u32()?;
    let expr = if enabled != 0 { Some((Expr::parse(r)?, r.u16()?)) } else { None };
    Ok(OptMode { enabled, expr })
}

fn write_opt_mode(w: &mut Writer, o: &OptMode) {
    w.u32(o.enabled);
    if let Some((e, m)) = &o.expr {
        e.write(w);
        w.u16(*m);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataDisplay {
    pub save_flags: u32,
    pub style: Style,
    pub kind: u32,
    /// Variable mostrada (kind 0 y 1).
    pub variable: Option<Expr>,
    pub numeric: Option<Numeric>,
    /// DisplayLength (kind 1).
    pub display_length: Option<u16>,
    /// Format / DateFormat / TimeFormat (+0x164).
    pub format: u16,
    /// InputModeEnabled (+0x18C).
    pub input_enabled: u32,
    pub input: Option<Input>,
    pub hihi: Opt,
    pub hi: Opt,
    pub lo: Opt,
    pub lolo: Opt,
    /// VisibilityAnimation VisibilityControl (+0x294 / +0x300).
    pub visibility: Opt,
    /// VisibilityAnimation BlinkControl + BlinkSpeed (+0x29C / +0x304 / +0x2A4).
    pub blink: OptMode,
    /// InterlockControl (+0x2B0 / +0x2FC).
    pub interlock: Opt,
    pub labels: LabelStorage,
    pub colors: ColorBlock,
    pub security_group: String,
    pub v450: u16,
    pub v464: u16,
}

impl DataDisplay {
    pub fn parse(b: &[u8]) -> R<DataDisplay> {
        let mut r = Reader::new(b);
        let d = DataDisplay::read(&mut r)?;
        if r.left() != 0 {
            return Err(format!("sobran {} B tras el display en {:#x}", r.left(), r.p));
        }
        Ok(d)
    }

    pub fn read(r: &mut Reader) -> R<DataDisplay> {
        r.expect_u32(DD_VERSION, "versión de DataDisplay")?;
        let save_flags = r.u32()?;
        let style = if save_flags & 1 != 0 { Style::Native(NativePart::parse(r)?) } else { Style::Name(r.bstr()?) };
        let at = r.p;
        let kind = r.u32()?;
        if kind > kind::TIME {
            return Err(format!("tipo de display {kind} en {at:#x}"));
        }
        let variable = if kind != kind::DATE && kind != kind::TIME { Some(Expr::parse(r)?) } else { None };
        let numeric = if kind == kind::NUMERIC {
            let data_type = r.u16()?;
            let zero_suppress = r.u32()?;
            let display_zero = r.u32()?;
            let digit_grouping = r.u32()?;
            let whole_digits = r.u16()?;
            let decimal_digits = r.u16()?;
            let v3a8 = r.u32()?;
            let unit_id = r.u32()?;
            let unit_texts =
                if unit_id != u32::MAX && save_flags & 1 != 0 { Some(LanguageDataList::parse(r)?) } else { None };
            Some(Numeric {
                data_type,
                zero_suppress,
                display_zero,
                digit_grouping,
                whole_digits,
                decimal_digits,
                v3a8,
                unit_id,
                unit_texts,
            })
        } else {
            None
        };
        let display_length = if kind == kind::STRING { Some(r.u16()?) } else { None };
        let format = r.u16()?;
        let input_enabled = r.u32()?;
        let input = if input_enabled != 0 {
            let field_ver = r.u16()?;
            let field = r.u32()?;
            let popup_keypad = r.u32()?;
            let keypad_format = if kind == kind::STRING { Some(r.u16()?) } else { None };
            let buzzer = r.u32()?;
            let password = r.u32()?;
            let range = if kind == kind::NUMERIC {
                let v = r.u32()?;
                let e = if v != 0 { Some((Expr::parse(r)?, Expr::parse(r)?)) } else { None };
                Some((v, e))
            } else {
                None
            };
            let barcode = read_opt(r)?;
            let error_status = read_opt(r)?;
            let extra = if save_flags == 0 || save_flags & 2 != 0 { Some((r.u16()?, r.u16()?)) } else { None };
            let trigger = read_opt(r)?;
            let status = read_opt_mode(r)?;
            Some(Input {
                field_ver,
                field,
                popup_keypad,
                keypad_format,
                buzzer,
                password,
                range,
                barcode,
                error_status,
                extra,
                trigger,
                status,
            })
        } else {
            None
        };
        let hihi = read_opt(r)?;
        let hi = read_opt(r)?;
        let lo = read_opt(r)?;
        let lolo = read_opt(r)?;
        let visibility = read_opt(r)?;
        let blink = read_opt_mode(r)?;
        let interlock = read_opt(r)?;
        let labels = LabelStorage::parse(r, save_flags)?;
        let colors = ColorBlock::parse(r)?;
        let security_group = r.bstr()?;
        let v450 = r.u16()?;
        let v464 = r.u16()?;
        Ok(DataDisplay {
            save_flags,
            style,
            kind,
            variable,
            numeric,
            display_length,
            format,
            input_enabled,
            input,
            hihi,
            hi,
            lo,
            lolo,
            visibility,
            blink,
            interlock,
            labels,
            colors,
            security_group,
            v450,
            v464,
        })
    }

    pub fn write(&self, w: &mut Writer) {
        w.u32(DD_VERSION);
        w.u32(self.save_flags);
        match &self.style {
            Style::Native(p) => p.write(w),
            Style::Name(s) => w.bstr(s),
        }
        w.u32(self.kind);
        if let Some(e) = &self.variable {
            e.write(w);
        }
        if let Some(n) = &self.numeric {
            w.u16(n.data_type);
            w.u32(n.zero_suppress);
            w.u32(n.display_zero);
            w.u32(n.digit_grouping);
            w.u16(n.whole_digits);
            w.u16(n.decimal_digits);
            w.u32(n.v3a8);
            w.u32(n.unit_id);
            if let Some(t) = &n.unit_texts {
                t.write(w);
            }
        }
        if let Some(l) = self.display_length {
            w.u16(l);
        }
        w.u16(self.format);
        w.u32(self.input_enabled);
        if let Some(i) = &self.input {
            w.u16(i.field_ver);
            w.u32(i.field);
            w.u32(i.popup_keypad);
            if let Some(k) = i.keypad_format {
                w.u16(k);
            }
            w.u32(i.buzzer);
            w.u32(i.password);
            if let Some((v, e)) = &i.range {
                w.u32(*v);
                if let Some((max, min)) = e {
                    max.write(w);
                    min.write(w);
                }
            }
            write_opt(w, &i.barcode);
            write_opt(w, &i.error_status);
            if let Some((a, b)) = i.extra {
                w.u16(a);
                w.u16(b);
            }
            write_opt(w, &i.trigger);
            write_opt_mode(w, &i.status);
        }
        write_opt(w, &self.hihi);
        write_opt(w, &self.hi);
        write_opt(w, &self.lo);
        write_opt(w, &self.lolo);
        write_opt(w, &self.visibility);
        write_opt_mode(w, &self.blink);
        write_opt(w, &self.interlock);
        self.labels.write(w);
        self.colors.write(w);
        w.bstr(&self.security_group);
        w.u16(self.v450);
        w.u16(self.v464);
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        self.write(&mut w);
        w.o
    }
}

// ---------------------------------------------------------------------------------------------
// Creación de displays nuevos
// ---------------------------------------------------------------------------------------------

/// Colores de un estado del display (atributos de `<State>`: PlateColor, FrameColor, Color3D, TextColor, Blink).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayColors {
    pub plate: u32,
    pub frame: u32,
    pub color_3d: u32,
    pub text: u32,
    pub blink: u8,
}

impl DisplayColors {
    /// Normal del corpus (PK_Parameters): placa B0B0B0, marco 707070, texto negro.
    pub fn normal() -> DisplayColors {
        DisplayColors { plate: 0xB0B0B0, frame: 0x707070, color_3d: 0, text: 0, blink: 0 }
    }
    /// HiHi/Hi/Lo/LoLo por defecto: igual que Normal con texto blanco.
    pub fn alarm() -> DisplayColors {
        DisplayColors { text: 0xFFFFFF, ..DisplayColors::normal() }
    }
    /// Fila de la biblioteca con las celdas en el orden del corpus (PK_Parameters SetTemp02).
    pub fn row(&self, row: u32) -> Row {
        Row {
            row,
            cells: vec![
                (rm::COL_BLINK, Widget::Blink(self.blink)),
                (rm::COL_TEXT, Widget::Color(self.text)),
                (rm::COL_COLOR_3D, Widget::Color(self.color_3d)),
                (rm::COL_FRAME, Widget::Color(self.frame)),
                (rm::COL_PLATE, Widget::Color(self.plate)),
            ],
        }
    }
}

/// Bloque de color de un display: filas Normal, HiHi, Hi, Lo, LoLo (las cinco siempre presentes).
pub fn display_colors(normal: DisplayColors) -> ColorBlock {
    let a = DisplayColors::alarm();
    ColorBlock {
        resource: String::new(),
        flag_c: 0,
        flag_d: 0,
        library: Some(ColorLibrary {
            name: String::new(),
            v28: rm::OBJ_DATA_DISPLAY,
            rows: vec![normal.row(rm::ROW_NORMAL), a.row(rm::ROW_HIHI), a.row(rm::ROW_HI), a.row(rm::ROW_LO), a.row(rm::ROW_LOLO)],
            states: vec![],
        }),
    }
}

/// Límite de entrada (OverwriteInputRange): constante o variable.
#[derive(Debug, Clone)]
pub enum Bound<'a> {
    Const(&'a str),
    Var(&'a str),
}

/// Parámetros comunes de un display nuevo.
#[derive(Debug, Clone)]
pub struct DisplaySpec<'a> {
    /// Variable (`Carpeta.Var`); no se usa en fecha y hora.
    pub variable: &'a str,
    /// Entrada habilitada (InputMode, teclado emergente).
    pub input: bool,
    /// Sólo numéricos con entrada: (mínimo, máximo).
    pub range: Option<(Bound<'a>, Bound<'a>)>,
    /// HAlign 0 Left, 1 Center, 2 Right; VAlign 0 Top, 1 Middle, 2 Bottom.
    pub halign: u32,
    pub valign: u32,
    pub font: LanguageFont,
    pub colors: DisplayColors,
    /// Nombre del estilo nativo (DataDisplay.dll: «DD00001» en la mayoría del corpus).
    pub style: &'a str,
    /// Id de cadena de la etiqueta (Vijeo lo reasigna al pegar; ver NOTAS.md).
    pub string_id: u32,
}

impl<'a> DisplaySpec<'a> {
    pub fn new(variable: &'a str) -> DisplaySpec<'a> {
        DisplaySpec {
            variable,
            input: false,
            range: None,
            halign: 1,
            valign: 1,
            font: super::lamp::font_8x13(false),
            colors: DisplayColors::normal(),
            style: "DD00001",
            string_id: 0,
        }
    }
}

/// Variable de texto: `(TagDB.X.getStringValue(0))`, atributos (0x100, 0x1005, 2, 0x100).
pub fn string_value(path: &str) -> Expr {
    let mut e = Expr::int_value(path);
    let c = format!("(TagDB.{path}.getStringValue(0))");
    e.compiled = c.clone();
    e.compiled2 = c;
    e.l24 = 0x100;
    e.l30 = 0x100;
    e
}

fn bound(b: &Bound, float: bool, context: &str) -> Expr {
    match b {
        // En el corpus sólo hay constantes enteras sobre variables enteras: (0x40, 5, 1, 0x40).
        Bound::Const(v) => {
            let mut e = Expr::constant(v, context);
            if float && v.contains(['.', 'e', 'E']) {
                // Sin verificar: real literal como valor real.
                e.l24 = 0x80;
                e.l30 = 0x80;
            }
            e
        }
        // Sin verificar en el corpus: variable como valor (como la propia variable del display).
        Bound::Var(p) => {
            if float {
                Expr::float_value(p)
            } else {
                Expr::int_value(p)
            }
        }
    }
}

fn base(s: &DisplaySpec, kind: u32, variable: Option<Expr>, format: u16) -> DataDisplay {
    DataDisplay {
        save_flags: 9,
        style: Style::Native(NativePart {
            name: s.style.into(),
            part_type: 4,
            label_type: 0,
            visibility: 0,
            v1c: 0,
            blink_type: 0,
            body: PartBody::Empty,
        }),
        kind,
        variable,
        numeric: None,
        display_length: None,
        format,
        input_enabled: 0,
        input: None,
        hihi: Opt::default(),
        hi: Opt::default(),
        lo: Opt::default(),
        lolo: Opt::default(),
        visibility: Opt::default(),
        blink: OptMode::default(),
        interlock: Opt::default(),
        labels: LabelStorage {
            kind: label_kind::AUTO,
            numeric: 0,
            halign: s.halign,
            valign: s.valign,
            numeric_font: None,
            labels: vec![label(None, &s.font, s.string_id)],
            text_library: None,
            font_library: None,
        },
        colors: display_colors(s.colors),
        security_group: String::new(),
        v450: 0,
        v464: 0,
    }
}

fn input(kind: u32, range: Option<(u32, Option<(Expr, Expr)>)>) -> Input {
    Input {
        field_ver: 1,
        field: 0,
        popup_keypad: 1,
        keypad_format: (kind == kind::STRING).then_some(fmt::STRING_KEYPAD),
        buzzer: 0,
        password: 0,
        range,
        barcode: Opt::default(),
        error_status: Opt::default(),
        extra: None,
        trigger: Opt::default(),
        status: OptMode::default(),
    }
}

impl DataDisplay {
    /// Display numérico: variable entera (`float` = false, formato Dec) o real (Fixed), `whole` dígitos
    /// enteros y `decimal` decimales, con ZeroSuppress y DisplayZero activados (como todo el corpus).
    /// Con `s.input`, entrada por teclado emergente y, si hay `s.range`, límites (mínimo, máximo).
    pub fn numeric(s: &DisplaySpec, float: bool, whole: u16, decimal: u16) -> DataDisplay {
        let var = if float { Expr::float_value(s.variable) } else { Expr::int_value(s.variable) };
        let ctx = var.compiled.clone();
        let mut d = base(s, kind::NUMERIC, Some(var), 0);
        d.numeric = Some(Numeric {
            data_type: if float { fmt::FLOAT } else { fmt::INTEGER },
            zero_suppress: 1,
            display_zero: 1,
            digit_grouping: 0,
            whole_digits: whole,
            decimal_digits: decimal,
            v3a8: 1,
            unit_id: u32::MAX,
            unit_texts: None,
        });
        if s.input {
            let range = match &s.range {
                None => (0, None),
                // Orden en el flujo: máximo y después mínimo.
                Some((min, max)) => (1, Some((bound(max, float, &ctx), bound(min, float, &ctx)))),
            };
            d.input_enabled = 1;
            d.input = Some(input(kind::NUMERIC, Some(range)));
        }
        d
    }

    /// Display de texto de `length` caracteres (DisplayLength 1..300), entrada opcional (StringKeypad).
    pub fn string(s: &DisplaySpec, length: u16) -> DataDisplay {
        let mut d = base(s, kind::STRING, Some(string_value(s.variable)), 0);
        d.display_length = Some(length);
        if s.input {
            d.input_enabled = 1;
            d.input = Some(input(kind::STRING, None));
        }
        d
    }

    /// Display de fecha (`fmt::DATE_*`) o de hora (`fmt::TIME_*`) del HMI: sin variable ni entrada.
    pub fn date_time(s: &DisplaySpec, time: bool, format: u16) -> DataDisplay {
        base(s, if time { kind::TIME } else { kind::DATE }, None, format)
    }
}
