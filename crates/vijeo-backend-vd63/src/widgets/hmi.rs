//! Bloques compartidos de HMIManagers.dll que guardan los objetos nativos (Switch, Lamp, DataDisplay...):
//! bloque de color con su biblioteca (CColorLibrary), etiquetas (CLabelStorage, CLabelInfoBase,
//! CLanguageDataList, CLanguageFont, CFontLibrary, CTextLibrary), imágenes (CImageStorage, CImageInfo,
//! CImageLibrary) y el estilo nativo (CNativePart, vía CNPManager::SavePart).
//!
//! Todos se escriben con `IStream::Write` directo salvo CNativePart, que usa un CArchive de MFC
//! (mismo resultado byte a byte: enteros LE y CString con `FF FE FF` + longitud).
//! `save_flags` es el argumento de guardado del objeto (9 en el portapapeles): con el bit 0 se
//! incrustan copias de las bibliotecas del proyecto, las imágenes y la parte nativa.
//!
//! La gramática detallada está en NOTAS.md.

use super::bin::{Reader, Writer, R};

// ---------------------------------------------------------------------------------------------
// Tablas de recursos (CResourceManager::SaveRow) y widgets
// ---------------------------------------------------------------------------------------------

/// Valor de una celda (CBaseWidget). La clase la decide el byte alto de la columna
/// (`CResourceManager::CreateWidget`).
#[derive(Debug, Clone, PartialEq)]
pub enum Widget {
    /// 0x1xx CColorWidget: COLORREF (0x00BBGGRR).
    Color(u32),
    /// 0x2xx CBlinkWidget: modo de parpadeo (0 = None).
    Blink(u8),
    /// 0x3xx CDynamicWidget: cadena.
    Dynamic(String),
    /// 0x4xx CLabelWidget: id de cadena del proyecto, o textos por idioma.
    Label(LabelText),
    /// 0x5xx CFontIDWidget: nombre de fuente + copia opcional de la biblioteca de fuentes.
    FontId { name: String, library: Option<FontLibrary> },
    /// 0x6xx CImageWidget: fichero de imagen incrustado o referencia a biblioteca.
    Image(ImageWidget),
    /// 0x7xx CPatternWidget: trama (1 = sólida).
    Pattern(u8),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LabelText {
    StringId(u32),
    Texts(LanguageDataList),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImageWidget {
    /// u8 0, BSTR fichero, y si no está vacío u32 n + n bytes de imagen.
    File { name: String, data: Option<Vec<u8>> },
    /// u8 != 0, BSTR biblioteca, u32 id.
    Library { flag: u8, library: String, id: u32 },
}

impl Widget {
    pub fn parse(col: u32, r: &mut Reader) -> R<Widget> {
        let at = r.p;
        Ok(match col & 0xFF00 {
            0x100 => Widget::Color(r.u32()?),
            0x200 => Widget::Blink(r.u8()?),
            0x300 => Widget::Dynamic(r.bstr()?),
            0x400 => {
                if r.u8()? == 0 {
                    Widget::Label(LabelText::StringId(r.u32()?))
                } else {
                    Widget::Label(LabelText::Texts(LanguageDataList::parse(r)?))
                }
            }
            0x500 => {
                let name = r.bstr()?;
                let library = if r.u8()? != 0 { Some(FontLibrary::parse(r)?) } else { None };
                Widget::FontId { name, library }
            }
            0x600 => {
                let flag = r.u8()?;
                if flag == 0 {
                    let name = r.bstr()?;
                    let data = if name.is_empty() {
                        None
                    } else {
                        let n = r.u32()? as usize;
                        Some(r.bytes(n)?.to_vec())
                    };
                    Widget::Image(ImageWidget::File { name, data })
                } else {
                    let library = r.bstr()?;
                    Widget::Image(ImageWidget::Library { flag, library, id: r.u32()? })
                }
            }
            0x700 => Widget::Pattern(r.u8()?),
            _ => return Err(format!("columna {col:#x} desconocida en {at:#x}")),
        })
    }

    pub fn write(&self, w: &mut Writer) {
        match self {
            Widget::Color(c) => w.u32(*c),
            Widget::Blink(b) | Widget::Pattern(b) => w.u8(*b),
            Widget::Dynamic(s) => w.bstr(s),
            Widget::Label(LabelText::StringId(id)) => {
                w.u8(0);
                w.u32(*id);
            }
            Widget::Label(LabelText::Texts(l)) => {
                w.u8(1);
                l.write(w);
            }
            Widget::FontId { name, library } => {
                w.bstr(name);
                w.u8(library.is_some() as u8);
                if let Some(l) = library {
                    l.write(w);
                }
            }
            Widget::Image(ImageWidget::File { name, data }) => {
                w.u8(0);
                w.bstr(name);
                if let Some(d) = data {
                    w.u32(d.len() as u32);
                    w.bytes(d);
                }
            }
            Widget::Image(ImageWidget::Library { flag, library, id }) => {
                w.u8(*flag);
                w.bstr(library);
                w.u32(*id);
            }
        }
    }
}

/// Fila de una tabla de recursos: u32 fila (eRM_ROW) + u32 n + n × (u32 columna, widget).
/// El orden de las celdas es el de iteración del CMap de Vijeo: se conserva tal cual.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub row: u32,
    pub cells: Vec<(u32, Widget)>,
}

impl Row {
    pub fn parse(r: &mut Reader) -> R<Row> {
        let row = r.u32()?;
        let at = r.p;
        let n = r.u32()?;
        if n > 64 {
            return Err(format!("fila {row:#x} con {n} columnas en {at:#x}"));
        }
        let mut cells = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let col = r.u32()?;
            cells.push((col, Widget::parse(col, r)?));
        }
        Ok(Row { row, cells })
    }
    pub fn write(&self, w: &mut Writer) {
        w.u32(self.row);
        w.u32(self.cells.len() as u32);
        for (c, v) in &self.cells {
            w.u32(*c);
            v.write(w);
        }
    }
    pub fn get(&self, col: u32) -> Option<&Widget> {
        self.cells.iter().find(|(c, _)| *c == col).map(|(_, v)| v)
    }
    pub fn color(&self, col: u32) -> Option<u32> {
        match self.get(col) {
            Some(Widget::Color(c)) => Some(*c),
            _ => None,
        }
    }
}

fn parse_rows(r: &mut Reader) -> R<Vec<Row>> {
    let at = r.p;
    let n = r.u32()?;
    if n > 256 {
        return Err(format!("{n} filas en {at:#x}"));
    }
    (0..n).map(|_| Row::parse(r)).collect()
}

fn write_rows(w: &mut Writer, rows: &[Row]) {
    w.u32(rows.len() as u32);
    for row in rows {
        row.write(w);
    }
}

/// Columnas de color (eRM_COLUMN) usadas por los Switch, con su atributo en el XML (`<State .../>`).
/// Comprobado con el orden de atributos del XML, que es el mismo que el de las celdas del binario
/// (ButtonOnYellow: Pattern, TextColor, Color3D, FrameColor, ForeColor, BackColor = 700,100,101,104,108,109).
pub mod col {
    pub const TEXT_COLOR: u32 = 0x100;
    pub const COLOR_3D: u32 = 0x101;
    pub const FRAME_COLOR: u32 = 0x104;
    pub const FORE_COLOR: u32 = 0x108;
    pub const BACK_COLOR: u32 = 0x109;
    pub const PATTERN: u32 = 0x700;
}

/// Colores de un estado de Switch (`<State StateType="OnUp|OffUp" .../>`). COLORREF = 0x00BBGGRR,
/// que es lo que el XML escribe en hexadecimal (FF00 = verde, 696462 = gris oscuro).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateColors {
    pub pattern: u8,
    pub back: u32,
    pub fore: u32,
    pub frame: u32,
    pub color_3d: u32,
    pub text: u32,
}

impl StateColors {
    /// Fila con las celdas en el orden más común del corpus (700, 109, 108, 104, 101, 100).
    pub fn row(&self, row: u32) -> Row {
        Row {
            row,
            cells: vec![
                (col::PATTERN, Widget::Pattern(self.pattern)),
                (col::BACK_COLOR, Widget::Color(self.back)),
                (col::FORE_COLOR, Widget::Color(self.fore)),
                (col::FRAME_COLOR, Widget::Color(self.frame)),
                (col::COLOR_3D, Widget::Color(self.color_3d)),
                (col::TEXT_COLOR, Widget::Color(self.text)),
            ],
        }
    }
    /// Lee una fila de Switch (en cualquier orden de celdas).
    pub fn from_row(r: &Row) -> Option<StateColors> {
        let pattern = match r.get(col::PATTERN)? {
            Widget::Pattern(p) => *p,
            _ => return None,
        };
        Some(StateColors {
            pattern,
            back: r.color(col::BACK_COLOR)?,
            fore: r.color(col::FORE_COLOR)?,
            frame: r.color(col::FRAME_COLOR)?,
            color_3d: r.color(col::COLOR_3D)?,
            text: r.color(col::TEXT_COLOR)?,
        })
    }
    /// Base de las bibliotecas estándar de los ejemplos (marco 707070, texto 696462, fondo y 3D negros).
    pub const fn standard(fore: u32) -> StateColors {
        StateColors { pattern: 1, back: 0, fore, frame: 0x707070, color_3d: 0, text: 0x696462 }
    }
}

/// Colores de las bibliotecas que traen los proyectos de ejemplo (mismo nombre en los tres).
pub mod palette {
    use super::StateColors;
    /// (nombre, encendido OnUp, apagado OffUp)
    pub const BUTTON_ON_GREEN: (&str, StateColors, StateColors) =
        ("ButtonOnGreen", StateColors::standard(0x00FF00), StateColors::standard(0xC0C0C0));
    pub const BUTTON_ON_RED: (&str, StateColors, StateColors) =
        ("ButtonOnRed", StateColors::standard(0x8080FF), StateColors::standard(0xC0C0C0));
    pub const BUTTON_ON_YELLOW: (&str, StateColors, StateColors) =
        ("ButtonOnYellow", StateColors::standard(0x00FFFF), StateColors::standard(0xC0C0C0));
    pub const NAVIGATION_BUTTON_WHITE: (&str, StateColors, StateColors) =
        ("NavigationButtonWhite", StateColors::standard(0xFFFFFF), StateColors::standard(0xFFFFFF));
    pub const NAVIGATION_BUTTON_GREY: (&str, StateColors, StateColors) =
        ("NavigationButtonGrey", StateColors::standard(0xC0C0C0), StateColors::standard(0xC0C0C0));
}

/// Filas (eRM_ROW) de la biblioteca de colores de un Switch (StateType del XML).
pub mod row {
    pub const ON_UP: u32 = 9;
    pub const OFF_UP: u32 = 10;
    /// Filas «de estado» (segunda lista): siempre se escriben con el id 8.
    pub const STATE: u32 = 8;
}

// ---------------------------------------------------------------------------------------------
// Bibliotecas: colores, textos, fuentes, imágenes
// ---------------------------------------------------------------------------------------------

/// CColorLibrary::Save: u32 5, BSTR nombre, u32 `v28`, filas, u32 m + m filas de estado (id 8).
#[derive(Debug, Clone, PartialEq)]
pub struct ColorLibrary {
    pub name: String,
    /// Campo +0x28 (16 en todo el corpus; Load cambia 0xFFFF por 0xF en versiones < 2).
    pub v28: u32,
    pub rows: Vec<Row>,
    pub states: Vec<Row>,
}

impl ColorLibrary {
    pub fn parse(r: &mut Reader) -> R<ColorLibrary> {
        r.expect_u32(5, "CColorLibrary")?;
        Ok(ColorLibrary { name: r.bstr()?, v28: r.u32()?, rows: parse_rows(r)?, states: parse_rows(r)? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.u32(5);
        w.bstr(&self.name);
        w.u32(self.v28);
        write_rows(w, &self.rows);
        write_rows(w, &self.states);
    }
    pub fn row(&self, id: u32) -> Option<&Row> {
        self.rows.iter().find(|r| r.row == id)
    }
    /// Biblioteca de un Switch: filas OnUp (9) y OffUp (10), sin filas de estado.
    pub fn switch(name: &str, on: StateColors, off: StateColors) -> ColorLibrary {
        ColorLibrary { name: name.into(), v28: 16, rows: vec![on.row(row::ON_UP), off.row(row::OFF_UP)], states: Vec::new() }
    }
}

/// CTextLibrary::Save: u32 5, BSTR nombre, u32 `v28`, u8 `v2c`, filas, filas de estado.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLibrary {
    pub name: String,
    pub v28: u32,
    pub v2c: u8,
    pub rows: Vec<Row>,
    pub states: Vec<Row>,
}

impl TextLibrary {
    pub fn parse(r: &mut Reader) -> R<TextLibrary> {
        r.expect_u32(5, "CTextLibrary")?;
        Ok(TextLibrary { name: r.bstr()?, v28: r.u32()?, v2c: r.u8()?, rows: parse_rows(r)?, states: parse_rows(r)? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.u32(5);
        w.bstr(&self.name);
        w.u32(self.v28);
        w.u8(self.v2c);
        write_rows(w, &self.rows);
        write_rows(w, &self.states);
    }
}

/// CImageLibrary::Save: u32 5, BSTR nombre, u32 `v28`, u8 `v38`, u32 `v2c`, u32 `v30`, filas, filas de estado.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageLibrary {
    pub name: String,
    pub v28: u32,
    pub v38: u8,
    pub v2c: u32,
    pub v30: u32,
    pub rows: Vec<Row>,
    pub states: Vec<Row>,
}

impl ImageLibrary {
    pub fn parse(r: &mut Reader) -> R<ImageLibrary> {
        r.expect_u32(5, "CImageLibrary")?;
        Ok(ImageLibrary {
            name: r.bstr()?,
            v28: r.u32()?,
            v38: r.u8()?,
            v2c: r.u32()?,
            v30: r.u32()?,
            rows: parse_rows(r)?,
            states: parse_rows(r)?,
        })
    }
    pub fn write(&self, w: &mut Writer) {
        w.u32(5);
        w.bstr(&self.name);
        w.u32(self.v28);
        w.u8(self.v38);
        w.u32(self.v2c);
        w.u32(self.v30);
        write_rows(w, &self.rows);
        write_rows(w, &self.states);
    }
}

/// CFontLibrary::Save: u32 5, BSTR nombre, u8 hay textos; textos por idioma, o u32 id de fuente.
#[derive(Debug, Clone, PartialEq)]
pub struct FontLibrary {
    pub name: String,
    pub value: LabelText,
}

impl FontLibrary {
    pub fn parse(r: &mut Reader) -> R<FontLibrary> {
        r.expect_u32(5, "CFontLibrary")?;
        let name = r.bstr()?;
        let value = if r.u8()? != 0 { LabelText::Texts(LanguageDataList::parse(r)?) } else { LabelText::StringId(r.u32()?) };
        Ok(FontLibrary { name, value })
    }
    pub fn write(&self, w: &mut Writer) {
        w.u32(5);
        w.bstr(&self.name);
        match &self.value {
            LabelText::StringId(id) => {
                w.u8(0);
                w.u32(*id);
            }
            LabelText::Texts(l) => {
                w.u8(1);
                l.write(w);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Bloque de color del objeto (Switch.dll FUN_6189bbb0)
// ---------------------------------------------------------------------------------------------

/// u8 2, BSTR biblioteca del proyecto ("" = colores propios), u8 hay copia;
/// si hay copia: u8 `flag_c` (bit 0 de +0xC), u8 `flag_d`, CColorLibrary.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorBlock {
    /// Nombre de la biblioteca de colores del proyecto (XML `Colors ColorResourceID`); "" si los colores son propios.
    pub resource: String,
    pub flag_c: u8,
    pub flag_d: u8,
    /// Copia de la biblioteca (o los colores propios, con nombre ""). Siempre presente en el portapapeles.
    pub library: Option<ColorLibrary>,
}

impl ColorBlock {
    pub fn parse(r: &mut Reader) -> R<ColorBlock> {
        let at = r.p;
        let v = r.u8()?;
        if v != 2 {
            return Err(format!("bloque de color con versión {v} en {at:#x}"));
        }
        let resource = r.bstr()?;
        let has = r.u8()?;
        if has == 0 {
            return Ok(ColorBlock { resource, flag_c: 0, flag_d: 0, library: None });
        }
        let flag_c = r.u8()?;
        let flag_d = r.u8()?;
        Ok(ColorBlock { resource, flag_c, flag_d, library: Some(ColorLibrary::parse(r)?) })
    }
    /// Colores propios del objeto (XML `<Colors><State .../></Colors>`).
    pub fn own(on: StateColors, off: StateColors) -> ColorBlock {
        ColorBlock { resource: String::new(), flag_c: 0, flag_d: 0, library: Some(ColorLibrary::switch("", on, off)) }
    }
    /// Biblioteca de colores del proyecto (XML `Colors ColorResourceID`), con su copia incrustada.
    /// Los colores deben ser los de la biblioteca del proyecto (ver NOTAS.md).
    pub fn resource(name: &str, on: StateColors, off: StateColors) -> ColorBlock {
        ColorBlock { resource: name.into(), flag_c: 1, flag_d: 0, library: Some(ColorLibrary::switch(name, on, off)) }
    }

    pub fn write(&self, w: &mut Writer) {
        w.u8(2);
        w.bstr(&self.resource);
        w.u8(self.library.is_some() as u8);
        if let Some(l) = &self.library {
            w.u8(self.flag_c);
            w.u8(self.flag_d);
            l.write(w);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Textos por idioma y fuentes
// ---------------------------------------------------------------------------------------------

/// CLanguageFont::Save: u32 4 + campos. `font_type`: 0 GP (fuente de mapa de bits del terminal),
/// 1 TrueType (Windows), 2 PSP.
#[derive(Debug, Clone, PartialEq)]
pub struct LanguageFont {
    pub font_type: u32,
    /// FontStyle del XML (0 Normal, 1 Bold...).
    pub style: u32,
    /// TrueType: altura (FontHeightOrSize) y anchura/peso (FontWidth).
    pub tt_height: i32,
    pub tt_width: i32,
    pub tt_face: String,
    /// GP: FontHeightOrSize, FontWidth, FontFace (enum eGPFontFace: 9 = GPFontModern6x10).
    pub gp_height: i32,
    pub gp_width: i32,
    pub gp_face: u32,
    /// Byte en +8 (0 en todo el corpus).
    pub b8: u8,
    /// PSP: tamaño en píxeles y cara.
    pub psp_size: i32,
    pub psp_face: u32,
}

impl LanguageFont {
    pub fn parse(r: &mut Reader) -> R<LanguageFont> {
        r.expect_u32(4, "CLanguageFont")?;
        Ok(LanguageFont {
            font_type: r.u32()?,
            style: r.u32()?,
            tt_height: r.i32()?,
            tt_width: r.i32()?,
            tt_face: r.bstr()?,
            gp_height: r.i32()?,
            gp_width: r.i32()?,
            gp_face: r.u32()?,
            b8: r.u8()?,
            psp_size: r.i32()?,
            psp_face: r.u32()?,
        })
    }
    pub fn write(&self, w: &mut Writer) {
        w.u32(4);
        w.u32(self.font_type);
        w.u32(self.style);
        w.i32(self.tt_height);
        w.i32(self.tt_width);
        w.bstr(&self.tt_face);
        w.i32(self.gp_height);
        w.i32(self.gp_width);
        w.u32(self.gp_face);
        w.u8(self.b8);
        w.i32(self.psp_size);
        w.u32(self.psp_face);
    }

    /// Fuente GP (`FontType="GP"`). Los campos TrueType/PSP que acompañan son los del corpus:
    /// GPFontModern6x10 (cara 9) → «Lucida Console» -8 y PSP -8; GPFontModern8x13 (cara 10) → «Arial» -19
    /// y PSP -19; peso 700 si es negrita (FontStyle="Bold" = 1), 400 si no.
    pub fn gp(face: u32, width: i32, height: i32, bold: bool) -> LanguageFont {
        let (tt_face, tt_height) = if face == GP_MODERN_6X10 { ("Lucida Console", -8) } else { ("Arial", -19) };
        LanguageFont {
            font_type: 0,
            style: bold as u32,
            tt_height,
            tt_width: if bold { 700 } else { 400 },
            tt_face: tt_face.into(),
            gp_height: height,
            gp_width: width,
            gp_face: face,
            b8: 0,
            psp_size: tt_height,
            psp_face: 4,
        }
    }
    /// GPFontModern6x10 normal: la fuente de 55 de las 59 etiquetas del corpus.
    pub fn modern_6x10() -> LanguageFont {
        LanguageFont::gp(GP_MODERN_6X10, 6, 10, false)
    }
    /// GPFontModern8x13 normal: la «fuente base» que acompaña a todos los textos del corpus.
    pub fn modern_8x13() -> LanguageFont {
        LanguageFont::gp(GP_MODERN_8X13, 8, 13, false)
    }
}

/// Caras GP (eGPFontFace) vistas en el corpus.
pub const GP_MODERN_6X10: u32 = 9;
pub const GP_MODERN_8X13: u32 = 10;

/// Texto de un idioma: BSTR idioma, BSTR texto, fuente base, u32 hay fuente propia + fuente propia, u32, u32.
#[derive(Debug, Clone, PartialEq)]
pub struct LanguageData {
    /// Nombre interno del idioma («Language1» = L1).
    pub language: String,
    pub text: String,
    /// El texto es un BSTR nulo (`00 00 00 00`), no una cadena vacía: displays y lámparas sin texto.
    pub text_null: bool,
    /// Primera fuente (la del idioma por defecto).
    pub base_font: LanguageFont,
    /// Segunda fuente, la que exporta el XML (`<Font .../>`), si la hay.
    pub font: Option<LanguageFont>,
    /// Dos u32 finales (1 y 0 en todo el corpus).
    pub tail_a: u32,
    pub tail_b: u32,
}

/// CLanguageDataList::Save: u32 3, u32 n, n × LanguageData.
#[derive(Debug, Clone, PartialEq)]
pub struct LanguageDataList {
    pub items: Vec<LanguageData>,
}

impl LanguageDataList {
    pub fn parse(r: &mut Reader) -> R<LanguageDataList> {
        r.expect_u32(3, "CLanguageDataList")?;
        let at = r.p;
        let n = r.u32()?;
        if n > 64 {
            return Err(format!("{n} idiomas en {at:#x}"));
        }
        let mut items = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let language = r.bstr()?;
            let text_null = r.b.get(r.p..r.p + 4) == Some(&[0u8, 0, 0, 0][..]);
            let text = r.bstr()?;
            let base_font = LanguageFont::parse(r)?;
            let at = r.p;
            let has = r.u32()?;
            let font = match has {
                0 => None,
                1 => Some(LanguageFont::parse(r)?),
                _ => return Err(format!("CLanguageDataList: indicador de fuente {has} en {at:#x}")),
            };
            items.push(LanguageData { language, text, text_null, base_font, font, tail_a: r.u32()?, tail_b: r.u32()? });
        }
        Ok(LanguageDataList { items })
    }
    pub fn write(&self, w: &mut Writer) {
        w.u32(3);
        w.u32(self.items.len() as u32);
        for d in &self.items {
            w.bstr(&d.language);
            if d.text_null && d.text.is_empty() {
                w.u32(0);
            } else {
                w.bstr(&d.text);
            }
            d.base_font.write(w);
            w.u32(d.font.is_some() as u32);
            if let Some(f) = &d.font {
                f.write(w);
            }
            w.u32(d.tail_a);
            w.u32(d.tail_b);
        }
    }
}

/// CLabelInfoBase::Save: u8 1, u32 id de cadena; si id != -1 y save_flags & 1, la lista de textos.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelInfo {
    pub string_id: u32,
    pub texts: Option<LanguageDataList>,
}

impl LabelInfo {
    pub fn parse(r: &mut Reader, save_flags: u32) -> R<LabelInfo> {
        let at = r.p;
        let v = r.u8()?;
        if v != 1 {
            return Err(format!("CLabelInfoBase con marca {v} en {at:#x}"));
        }
        let string_id = r.u32()?;
        let texts =
            if string_id != u32::MAX && save_flags & 1 != 0 { Some(LanguageDataList::parse(r)?) } else { None };
        Ok(LabelInfo { string_id, texts })
    }
    pub fn write(&self, w: &mut Writer) {
        w.u8(1);
        w.u32(self.string_id);
        if let Some(t) = &self.texts {
            t.write(w);
        }
    }
}

/// Referencia opcional a una biblioteca del proyecto (fuente o texto) dentro de CLabelStorage:
/// u8 hay nombre; si hay: BSTR nombre, y con save_flags & 1: u8 hay copia + copia.
#[derive(Debug, Clone, PartialEq)]
pub struct LibRef<T> {
    pub name: String,
    pub copy: Option<Option<T>>,
}

fn parse_libref<T>(r: &mut Reader, save_flags: u32, f: fn(&mut Reader) -> R<T>) -> R<Option<LibRef<T>>> {
    if r.u8()? == 0 {
        return Ok(None);
    }
    let name = r.bstr()?;
    let copy = if save_flags & 1 != 0 { Some(if r.u8()? != 0 { Some(f(r)?) } else { None }) } else { None };
    Ok(Some(LibRef { name, copy }))
}

fn write_libref<T>(w: &mut Writer, l: &Option<LibRef<T>>, f: fn(&T, &mut Writer)) {
    match l {
        None => w.u8(0),
        Some(l) => {
            w.u8(1);
            w.bstr(&l.name);
            if let Some(c) = &l.copy {
                w.u8(c.is_some() as u8);
                if let Some(c) = c {
                    f(c, w);
                }
            }
        }
    }
}

/// Tipo de etiqueta (CLabelStorage +4).
pub mod label_kind {
    pub const NONE: u8 = 0;
    pub const AUTO: u8 = 1;
    pub const FLOATING: u8 = 2;
    pub const USER_DEFINED: u8 = 3;
    pub const ON_OFF: u8 = 4;
    pub const STATIC: u8 = 5;
}

/// CLabelStorage::Save.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelStorage {
    /// `label_kind` (Switch: 0 sin etiqueta, 5 estática, 4 On/Off).
    pub kind: u8,
    /// Bit 0 de +8: etiqueta numérica habilitada.
    pub numeric: u8,
    /// HAlign: 0 Left, 1 Center, 2 Right. VAlign: 0 Top, 1 Middle, 2 Bottom.
    pub halign: u32,
    pub valign: u32,
    /// Con `numeric`: fuente de la parte numérica.
    pub numeric_font: Option<LibRef<FontLibrary>>,
    pub labels: Vec<LabelInfo>,
    /// Tipos 3 y 4: biblioteca de textos.
    pub text_library: Option<LibRef<TextLibrary>>,
    /// Tipos 1 y 2: biblioteca de fuentes.
    pub font_library: Option<LibRef<FontLibrary>>,
}

impl LabelStorage {
    fn text_supported(&self) -> bool {
        self.kind == label_kind::ON_OFF || self.kind == label_kind::USER_DEFINED
    }
    fn auto_or_floating(&self) -> bool {
        self.kind == label_kind::AUTO || self.kind == label_kind::FLOATING
    }

    pub fn parse(r: &mut Reader, save_flags: u32) -> R<LabelStorage> {
        let at = r.p;
        let v = r.u8()?;
        if v != 2 {
            return Err(format!("CLabelStorage versión {v} en {at:#x}"));
        }
        let mut s = LabelStorage {
            kind: r.u8()?,
            numeric: r.u8()?,
            halign: r.u32()?,
            valign: r.u32()?,
            numeric_font: None,
            labels: Vec::new(),
            text_library: None,
            font_library: None,
        };
        if s.numeric & 1 != 0 {
            s.numeric_font = parse_libref(r, save_flags, FontLibrary::parse)?;
        }
        let at = r.p;
        let n = r.i16()?;
        if !(0..=256).contains(&n) {
            return Err(format!("CLabelStorage con {n} etiquetas en {at:#x}"));
        }
        for _ in 0..n {
            s.labels.push(LabelInfo::parse(r, save_flags)?);
        }
        if s.text_supported() {
            s.text_library = parse_libref(r, save_flags, TextLibrary::parse)?;
        }
        if s.auto_or_floating() {
            s.font_library = parse_libref(r, save_flags, FontLibrary::parse)?;
        }
        Ok(s)
    }

    pub fn write(&self, w: &mut Writer) {
        w.u8(2);
        w.u8(self.kind);
        w.u8(self.numeric);
        w.u32(self.halign);
        w.u32(self.valign);
        if self.numeric & 1 != 0 {
            write_libref(w, &self.numeric_font, FontLibrary::write);
        }
        w.i16(self.labels.len() as i16);
        for l in &self.labels {
            l.write(w);
        }
        if self.text_supported() {
            write_libref(w, &self.text_library, TextLibrary::write);
        }
        if self.auto_or_floating() {
            write_libref(w, &self.font_library, FontLibrary::write);
        }
    }

    /// Etiqueta estática con un texto en el idioma 1 (`Language1`) y la fuente indicada.
    /// `string_id` es el id de cadena del proyecto: Vijeo lo vuelve a asignar al pegar (ver NOTAS.md).
    pub fn static_text(text: &str, font: LanguageFont, halign: u32, valign: u32, string_id: u32) -> LabelStorage {
        LabelStorage {
            kind: label_kind::STATIC,
            numeric: 0,
            halign,
            valign,
            numeric_font: None,
            labels: vec![LabelInfo {
                string_id,
                texts: Some(LanguageDataList {
                    items: vec![LanguageData {
                        language: "Language1".into(),
                        text: text.into(),
                        text_null: false,
                        base_font: LanguageFont::modern_8x13(),
                        font: Some(font),
                        tail_a: 1,
                        tail_b: 0,
                    }],
                }),
            }],
            text_library: None,
            font_library: None,
        }
    }

    pub fn none() -> LabelStorage {
        LabelStorage {
            kind: label_kind::NONE,
            numeric: 0,
            halign: 1,
            valign: 1,
            numeric_font: None,
            labels: Vec::new(),
            text_library: None,
            font_library: None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Imágenes
// ---------------------------------------------------------------------------------------------

/// CImageInfo::Save. Con save_flags & 1: BSTR fichero y, si no está vacío, u32 n + n bytes de imagen
/// (CxImage). Sin el bit 0: u32 1, BSTR id en el gestor de mapas de bits, u32.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageInfo {
    Embedded { name: String, data: Option<Vec<u8>> },
    Managed { tag: u32, id: String, value: u32 },
}

impl ImageInfo {
    pub fn empty() -> ImageInfo {
        ImageInfo::Embedded { name: String::new(), data: None }
    }
    pub fn parse(r: &mut Reader, save_flags: u32) -> R<ImageInfo> {
        if save_flags & 1 != 0 {
            let name = r.bstr()?;
            let data = if name.is_empty() {
                None
            } else {
                let at = r.p;
                let n = r.u32()? as usize;
                if n > r.left() {
                    return Err(format!("imagen de {n} B en {at:#x}"));
                }
                Some(r.bytes(n)?.to_vec())
            };
            Ok(ImageInfo::Embedded { name, data })
        } else {
            Ok(ImageInfo::Managed { tag: r.u32()?, id: r.bstr()?, value: r.u32()? })
        }
    }
    pub fn write(&self, w: &mut Writer) {
        match self {
            ImageInfo::Embedded { name, data } => {
                w.bstr(name);
                if let Some(d) = data {
                    w.u32(d.len() as u32);
                    w.bytes(d);
                }
            }
            ImageInfo::Managed { tag, id, value } => {
                w.u32(*tag);
                w.bstr(id);
                w.u32(*value);
            }
        }
    }
}

/// Imágenes por estado cuando vienen de una biblioteca de imágenes del proyecto.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageLibRef {
    /// Bit 3 a 0: u16 n + n × u32 (id de la imagen en la biblioteca).
    Ids(Vec<u32>),
    /// Bit 3 a 1: copia de la biblioteca + u16 n.
    Copy(ImageLibrary, u16),
}

/// CImageStorage::Save: u8 flags (bit 0 y 1: propiedades; bit 2: hay biblioteca; bit 3: copia
/// incrustada; bit 4: siempre), u32 color transparente (0x7B08FF), u32 `v14`, y las imágenes.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageStorage {
    pub flags: u8,
    pub transparent: u32,
    pub v14: u32,
    /// Sin biblioteca: u16 n + n × CImageInfo (Switch: 2, una por estado).
    pub images: Vec<ImageInfo>,
    /// Con biblioteca (bit 2): BSTR nombre + `ImageLibRef`.
    pub library: Option<(String, ImageLibRef)>,
}

impl ImageStorage {
    pub fn parse(r: &mut Reader, save_flags: u32) -> R<ImageStorage> {
        let flags = r.u8()?;
        let transparent = r.u32()?;
        let v14 = r.u32()?;
        let mut s = ImageStorage { flags, transparent, v14, images: Vec::new(), library: None };
        if flags & 4 == 0 {
            let at = r.p;
            let n = r.i16()?;
            if !(0..=64).contains(&n) {
                return Err(format!("CImageStorage con {n} imágenes en {at:#x}"));
            }
            for _ in 0..n {
                s.images.push(ImageInfo::parse(r, save_flags)?);
            }
        } else {
            let name = r.bstr()?;
            let l = if flags & 8 == 0 {
                let n = r.i16()?;
                ImageLibRef::Ids((0..n).map(|_| r.u32()).collect::<R<Vec<u32>>>()?)
            } else {
                let lib = ImageLibrary::parse(r)?;
                ImageLibRef::Copy(lib, r.u16()?)
            };
            s.library = Some((name, l));
        }
        Ok(s)
    }
    pub fn write(&self, w: &mut Writer) {
        w.u8(self.flags);
        w.u32(self.transparent);
        w.u32(self.v14);
        match &self.library {
            None => {
                w.i16(self.images.len() as i16);
                for i in &self.images {
                    i.write(w);
                }
            }
            Some((name, l)) => {
                w.bstr(name);
                match l {
                    ImageLibRef::Ids(ids) => {
                        w.i16(ids.len() as i16);
                        for i in ids {
                            w.u32(*i);
                        }
                    }
                    ImageLibRef::Copy(lib, n) => {
                        lib.write(w);
                        w.u16(*n);
                    }
                }
            }
        }
    }

    /// Sin imágenes: flags 0x10, transparente 0x7B08FF, v14 16 y `n` imágenes vacías (Switch: 2).
    pub fn empty(n: usize) -> ImageStorage {
        ImageStorage { flags: 0x10, transparent: 0x7B08FF, v14: 16, images: vec![ImageInfo::empty(); n], library: None }
    }
}


// ---------------------------------------------------------------------------------------------
// Estilo nativo (CNativePart::Save, por CArchive)
// ---------------------------------------------------------------------------------------------

/// Cuerpo de la parte nativa según su tipo (+4).
#[derive(Debug, Clone, PartialEq)]
pub enum PartBody {
    /// Tipo 1 (Switch). `actions`: u16 n listas (al tocar, al soltar, mientras se pulsa) + por lista
    /// u16 m + m × u32 tipo de acción (eNP_ACTION_TYPE): repite los tipos de las acciones del objeto.
    /// Después 6 × u32: ReverseOnTouch (+8), Interlock (+0xC), Lamp (+0x10), +0x14, +0x24, HoldDelay (+0x28).
    Switch { actions: Vec<Vec<u32>>, reverse_on_touch: u32, interlock: u32, lamp: u32, v14: u32, v24: u32, hold_delay: u32 },
    /// Tipo 2 (Lamp): u16 +0x2C (tipo de control), u16 +0x30 (estados).
    Lamp(u16, u16),
    /// Tipo 4: sin cuerpo.
    Empty,
    /// Otros tipos (selector 8/0x10, medidor 0x20, barra 0x40, lista 0x80/0x100): bytes sin descifrar
    /// hasta el final del objeto.
    Raw(Vec<u8>),
}

/// Copia de la parte nativa (estilo de `Vijeo-Frame\NativeStyles\<Categoría>\<Nombre>.NPS`):
/// u32 4, CString StyleID (+0x38), u32 tipo (+4), u32 LabelType (+0x34), u32 +0x18, u32 +0x1C,
/// u32 BlinkType (+0x20), cuerpo.
#[derive(Debug, Clone, PartialEq)]
pub struct NativePart {
    /// Nombre del estilo (fichero .NPS sin extensión): «Square03», «Square05», «BlankArea»...
    pub name: String,
    pub part_type: u32,
    /// eNP_LABEL_TYPE: 1 con etiqueta, 0 sin etiqueta (en el corpus coincide con CLabelStorage).
    pub label_type: u32,
    /// +0x18: 1 si el objeto tiene animación de visibilidad (máscara bit 4) en todo el corpus.
    pub visibility: u32,
    pub v1c: u32,
    pub blink_type: u32,
    pub body: PartBody,
}

impl NativePart {
    /// Va al final del objeto: `Raw` se queda con todo lo que falte.
    pub fn parse(r: &mut Reader) -> R<NativePart> {
        r.expect_u32(4, "CNativePart")?;
        let name = r.cstring()?;
        let part_type = r.u32()?;
        let (label_type, visibility, v1c, blink_type) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
        let body = match part_type {
            1 => {
                let n = r.i16()?;
                let mut actions = Vec::new();
                for _ in 0..n.max(0) {
                    let m = r.i16()?;
                    actions.push((0..m.max(0)).map(|_| r.u32()).collect::<R<Vec<u32>>>()?);
                }
                PartBody::Switch {
                    actions,
                    reverse_on_touch: r.u32()?,
                    interlock: r.u32()?,
                    lamp: r.u32()?,
                    v14: r.u32()?,
                    v24: r.u32()?,
                    hold_delay: r.u32()?,
                }
            }
            2 => PartBody::Lamp(r.u16()?, r.u16()?),
            4 => PartBody::Empty,
            _ => PartBody::Raw(r.bytes(r.left())?.to_vec()),
        };
        Ok(NativePart { name, part_type, label_type, visibility, v1c, blink_type, body })
    }

    pub fn write(&self, w: &mut Writer) {
        w.u32(4);
        w.cstring(&self.name);
        w.u32(self.part_type);
        w.u32(self.label_type);
        w.u32(self.visibility);
        w.u32(self.v1c);
        w.u32(self.blink_type);
        match &self.body {
            PartBody::Switch { actions, reverse_on_touch, interlock, lamp, v14, v24, hold_delay } => {
                w.i16(actions.len() as i16);
                for l in actions {
                    w.i16(l.len() as i16);
                    for a in l {
                        w.u32(*a);
                    }
                }
                for x in [reverse_on_touch, interlock, lamp, v14, v24, hold_delay] {
                    w.u32(*x);
                }
            }
            PartBody::Lamp(a, b) => {
                w.u16(*a);
                w.u16(*b);
            }
            PartBody::Empty => {}
            PartBody::Raw(b) => w.bytes(b),
        }
    }

    /// Parte nativa de un Switch con el estilo `name` (p. ej. «Square03»): los indicadores repiten
    /// los del objeto (tipos de acción por lista, lámpara, visibilidad, etiqueta).
    /// `interlock` sigue la misma lógica que `lamp` (SetInterlock); no hay enclavamientos en los clips del corpus.
    pub fn switch(name: &str, action_kinds: [Vec<u32>; 3], label: bool, lamp: bool, visibility: bool, interlock: bool) -> NativePart {
        let [a, b, c] = action_kinds;
        NativePart {
            name: name.into(),
            part_type: 1,
            label_type: label as u32,
            visibility: visibility as u32,
            v1c: 0,
            blink_type: 0,
            body: PartBody::Switch {
                actions: vec![a, b, c],
                reverse_on_touch: 0,
                interlock: interlock as u32,
                lamp: lamp as u32,
                v14: 0,
                v24: 0,
                hold_delay: 0,
            },
        }
    }
}
