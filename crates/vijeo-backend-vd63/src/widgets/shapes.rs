//! Datos de clase (lo que va detrás de la cabecera común y la marca 0x1CED50DA) de las formas básicas de
//! Vijeo-Designer 6.3: Text (TextObject.dll, v11), Rectangle (RoundRect.dll, v2), Line (LineObject.dll, v2) y
//! Ellipse/Circle (CircObject.dll, v2). Sólo std + `bin.rs` (Reader/Writer). Gramática y fuentes en NOTAS.md.
//!
//! Todas las structs hacen ida y vuelta exacta (`parse` → `write` da los mismos bytes) y tienen un
//! constructor `default_*` para crear objetos nuevos, con los valores más comunes del corpus.
#![allow(dead_code)]

use super::bin::{Reader, Writer, R};

// ---------------------------------------------------------------------------------------------------------
// GUID de clase (tal como van en el stream, antes de la cabecera común)
// ---------------------------------------------------------------------------------------------------------

pub const GUID_TEXT: &str = "{CFD4B0CC-678F-4F78-980C-24C14CBE68E1}";
pub const GUID_RECTANGLE: &str = "{3BE64A91-D329-4653-9891-AA4EE97464C2}";
pub const GUID_LINE: &str = "{78914787-36A8-4096-9256-DF9BD6C180AA}";
pub const GUID_ELLIPSE: &str = "{22A41009-62E3-4612-B997-032976965397}";

// ---------------------------------------------------------------------------------------------------------
// Tipos comunes
// ---------------------------------------------------------------------------------------------------------

/// Bit de transparencia de un color (en el propio COLORREF y, en el XML, `BackColor="08000000"`).
pub const TRANSPARENT_BIT: u32 = 0x0800_0000;

/// Color tal como lo guarda Vijeo: COLORREF de Windows, `0x00BBGGRR` (R en el byte bajo), más banderas en el
/// byte alto: 0x08 = transparente; 0x02 (PALETTERGB) aparece en los colores de degradado.
/// El XML lo escribe como hexadecimal del valor (`"A4A09F"` = R 0x9F, G 0xA0, B 0xA4), en mayúsculas y
/// con un número par de cifras (`"00"`, `"FF00"`, `"0A9902"`, `"08000000"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color(pub u32);

impl Color {
    pub const BLACK: Color = Color(0);
    pub const WHITE: Color = Color(0x00FF_FFFF);
    pub fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color(r as u32 | (g as u32) << 8 | (b as u32) << 16)
    }
    /// Color transparente (lo que Vijeo guarda para "sin relleno": `08000000`).
    pub fn transparent() -> Color {
        Color(TRANSPARENT_BIT)
    }
    pub fn r(self) -> u8 {
        self.0 as u8
    }
    pub fn g(self) -> u8 {
        (self.0 >> 8) as u8
    }
    pub fn b(self) -> u8 {
        (self.0 >> 16) as u8
    }
    pub fn is_transparent(self) -> bool {
        self.0 & TRANSPARENT_BIT != 0
    }
    /// Desde el atributo del XML (`"A4A09F"`, `"08000000"`).
    pub fn from_xml(s: &str) -> R<Color> {
        u32::from_str_radix(s, 16).map(Color).map_err(|_| format!("color no válido: {s:?}"))
    }
    /// Como lo escribe el XML de Vijeo (con el bit de transparencia si `trans != 0`).
    pub fn to_xml(self, trans: u32) -> String {
        let v = if trans != 0 { self.0 | TRANSPARENT_BIT } else { self.0 };
        let s = format!("{v:X}");
        if s.len() % 2 == 1 {
            format!("0{s}")
        } else {
            s
        }
    }
}

/// Parpadeo (g_blinkMap de HMIManagers): None 0, Fast 1, Mid 2, Slow 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Blink {
    #[default]
    None,
    Fast,
    Mid,
    Slow,
    Other(u16),
}

impl Blink {
    pub fn from_u16(v: u16) -> Blink {
        match v {
            0 => Blink::None,
            1 => Blink::Fast,
            2 => Blink::Mid,
            3 => Blink::Slow,
            o => Blink::Other(o),
        }
    }
    pub fn to_u16(self) -> u16 {
        match self {
            Blink::None => 0,
            Blink::Fast => 1,
            Blink::Mid => 2,
            Blink::Slow => 3,
            Blink::Other(o) => o,
        }
    }
    pub fn xml(self) -> String {
        match self {
            Blink::None => "None".into(),
            Blink::Fast => "Fast".into(),
            Blink::Mid => "Mid".into(),
            Blink::Slow => "Slow".into(),
            Blink::Other(o) => o.to_string(),
        }
    }
}

/// Degradado de un color (relleno, línea o texto): estilo de sombreado (eShadingStyle; 0 = color liso) y los
/// colores adicionales (el primero de la lista interna es el propio color del objeto y no se guarda).
/// Se escribe: i16 estilo, u16 n, n × u32 color.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Gradient {
    pub style: i16,
    pub colors: Vec<Color>,
}

impl Gradient {
    /// Lo que Vijeo guarda en un objeto sin degradado: estilo 0 y un color extra (0x02FF0000 en el relleno,
    /// 0x02FFFFFF en la línea y en el texto).
    pub fn plain(extra: u32) -> Gradient {
        Gradient { style: 0, colors: vec![Color(extra)] }
    }
    fn parse(r: &mut Reader) -> R<Gradient> {
        let style = r.i16()?;
        let n = r.u16()? as usize;
        let mut colors = Vec::with_capacity(n);
        for _ in 0..n {
            colors.push(Color(r.u32()?));
        }
        Ok(Gradient { style, colors })
    }
    fn write(&self, w: &mut Writer) {
        w.i16(self.style);
        w.u16(self.colors.len() as u16);
        for c in &self.colors {
            w.u32(c.0);
        }
    }
}

/// BSTR que distingue el nulo (u32 0) de la cadena vacía (u32 2 + NUL).
fn rd_bstr(r: &mut Reader) -> R<Option<String>> {
    let at = r.p;
    let n = r.u32()?;
    if n == 0 {
        return Ok(None);
    }
    r.p = at;
    r.bstr().map(Some)
}

fn wr_bstr(w: &mut Writer, s: &Option<String>) {
    match s {
        None => w.u32(0),
        Some(s) => w.bstr(s),
    }
}

fn end(r: &Reader, what: &str) -> R<()> {
    if r.left() != 0 {
        return Err(format!("{what}: sobran {} bytes en {:#x}", r.left(), r.p));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------
// Rectangle (RoundRect.dll, CRoundRectObject, versión 2)
// ---------------------------------------------------------------------------------------------------------

/// Forma del rectángulo (g_rectShapeMap): Normal 0, Rounded 1, Cut (chaflán) 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecShape {
    #[default]
    Normal,
    Rounded,
    Cut,
    Other(u16),
}

impl RecShape {
    pub fn from_u16(v: u16) -> RecShape {
        match v {
            0 => RecShape::Normal,
            1 => RecShape::Rounded,
            2 => RecShape::Cut,
            o => RecShape::Other(o),
        }
    }
    pub fn to_u16(self) -> u16 {
        match self {
            RecShape::Normal => 0,
            RecShape::Rounded => 1,
            RecShape::Cut => 2,
            RecShape::Other(o) => o,
        }
    }
    pub fn xml(self) -> String {
        match self {
            RecShape::Normal => "Normal".into(),
            RecShape::Rounded => "Rounded".into(),
            RecShape::Cut => "Cut".into(),
            RecShape::Other(o) => o.to_string(),
        }
    }
}

/// Rectángulo (XML `<Rectangle>`, `<Style BackColor BackBlink LineColor LineBlink LineStyle LineWidth ForeColor
/// ForeBlink Pattern RecShape Corner>`). "Fore" es el relleno; "Back", el color de fondo del patrón.
#[derive(Debug, Clone, PartialEq)]
pub struct Rectangle {
    pub version: u32,
    pub fore_color: Color,
    pub line_color: Color,
    pub back_color: Color,
    pub fore_blink: Blink,
    pub line_blink: Blink,
    pub back_blink: Blink,
    /// Ancho de línea en píxeles (Vijeo lo guarda como double de un entero).
    pub line_width: f64,
    /// Estilo de línea (XML LineStyle: 0 continua…).
    pub line_style: u32,
    /// Patrón de relleno (XML Pattern; 1 = liso).
    pub pattern: u16,
    /// Radio de esquina relativo: Corner (px) = round(lado mayor × ratio), ver [`Rectangle::corner_px`].
    pub corner_ratio: f64,
    /// Banderas de transparencia (≠ 0: transparente; el XML lo suma al color como 0x08000000).
    pub back_trans: u32,
    pub fore_trans: u32,
    pub line_trans: u32,
    pub shape: RecShape,
    /// u32 que Vijeo escribe siempre a 0 y al leer ignora.
    pub reserved: u32,
    /// Degradados (sólo versión ≥ 2).
    pub fore_gradient: Option<Gradient>,
    pub line_gradient: Option<Gradient>,
}

impl Rectangle {
    pub fn parse(b: &[u8]) -> R<Rectangle> {
        let mut r = Reader::new(b);
        let version = r.u32()?;
        if version == 0 || version > 2 {
            return Err(format!("Rectangle: versión {version} no soportada"));
        }
        let fore_color = Color(r.u32()?);
        let line_color = Color(r.u32()?);
        let back_color = Color(r.u32()?);
        let fore_blink = Blink::from_u16(r.u16()?);
        let line_blink = Blink::from_u16(r.u16()?);
        let back_blink = Blink::from_u16(r.u16()?);
        let line_width = r.f64()?;
        let line_style = r.u32()?;
        let pattern = r.u16()?;
        let corner_ratio = r.f64()?;
        let back_trans = r.u32()?;
        let fore_trans = r.u32()?;
        let line_trans = r.u32()?;
        let shape = RecShape::from_u16(r.u16()?);
        let reserved = r.u32()?;
        let (fore_gradient, line_gradient) =
            if version >= 2 { (Some(Gradient::parse(&mut r)?), Some(Gradient::parse(&mut r)?)) } else { (None, None) };
        end(&r, "Rectangle")?;
        Ok(Rectangle {
            version,
            fore_color,
            line_color,
            back_color,
            fore_blink,
            line_blink,
            back_blink,
            line_width,
            line_style,
            pattern,
            corner_ratio,
            back_trans,
            fore_trans,
            line_trans,
            shape,
            reserved,
            fore_gradient,
            line_gradient,
        })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u32(self.version);
        w.u32(self.fore_color.0);
        w.u32(self.line_color.0);
        w.u32(self.back_color.0);
        w.u16(self.fore_blink.to_u16());
        w.u16(self.line_blink.to_u16());
        w.u16(self.back_blink.to_u16());
        w.f64(self.line_width);
        w.u32(self.line_style);
        w.u16(self.pattern);
        w.f64(self.corner_ratio);
        w.u32(self.back_trans);
        w.u32(self.fore_trans);
        w.u32(self.line_trans);
        w.u16(self.shape.to_u16());
        w.u32(self.reserved);
        if self.version >= 2 {
            self.fore_gradient.clone().unwrap_or_else(|| Gradient::plain(0x02FF_0000)).write(&mut w);
            self.line_gradient.clone().unwrap_or_else(|| Gradient::plain(0x02FF_FFFF)).write(&mut w);
        }
        w.o
    }

    /// Rectángulo nuevo con el estilo más común del corpus: relleno gris A4A09F, borde verde 309500 de 2 px,
    /// esquinas redondeadas de 3 px (para un objeto de `width` × `height`, como los de la cabecera común).
    pub fn default_rectangle(width: i32, height: i32) -> Rectangle {
        let mut r = Rectangle {
            version: 2,
            fore_color: Color(0x00A4_A09F),
            line_color: Color(0x0030_9500),
            back_color: Color::BLACK,
            fore_blink: Blink::None,
            line_blink: Blink::None,
            back_blink: Blink::None,
            line_width: 2.0,
            line_style: 0,
            pattern: 1,
            corner_ratio: 1.0 / 6.0,
            back_trans: 0,
            fore_trans: 0,
            line_trans: 0,
            shape: RecShape::Rounded,
            reserved: 0,
            fore_gradient: Some(Gradient::plain(0x02FF_0000)),
            line_gradient: Some(Gradient::plain(0x02FF_FFFF)),
        };
        r.set_corner_px(3, width, height);
        r
    }

    /// Radio de esquina en píxeles (atributo Corner del XML) para un objeto de `width` × `height` (ancho y
    /// alto del XML, `r - l + 1` de la cabecera). Mismo cálculo que RoundRect.dll: con dx = width - 1 y
    /// dy = height - 1, round(max(dx, dy) × ratio), limitado a la mitad (entera) del lado menor y a 599.
    pub fn corner_px(&self, width: i32, height: i32) -> i32 {
        corner_px(self.corner_ratio, width, height)
    }

    /// Fija el radio de esquina en píxeles (lo inverso de [`Rectangle::corner_px`]).
    pub fn set_corner_px(&mut self, px: i32, width: i32, height: i32) {
        let m = (width.max(height) - 1).max(1) as f64;
        self.corner_ratio = px as f64 / m;
    }
}

/// Corner (px) a partir del ratio guardado (FUN_616465b3 de RoundRect.dll). `w`/`h`: ancho y alto del objeto
/// (`r - l + 1`, como en el XML); Vijeo calcula con `r - l` y `b - t`.
pub fn corner_px(ratio: f64, w: i32, h: i32) -> i32 {
    let (dx, dy) = ((w - 1) as f64, (h - 1) as f64);
    let big = dx.max(dy);
    let mut c = (big * ratio + 0.5).floor() as i32;
    if dy < dx && dy * 0.5 < c as f64 {
        c = (dy as i32) / 2;
    }
    if dx < dy && dx * 0.5 < c as f64 {
        c = (dx as i32) / 2;
    }
    c.min(599)
}

// ---------------------------------------------------------------------------------------------------------
// Line (LineObject.dll, CLineLineObject, versión 2)
// ---------------------------------------------------------------------------------------------------------

/// Flechas de la línea (XML Direction): None 0, Start 1, End 2, Both 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Arrows {
    #[default]
    None,
    Start,
    End,
    Both,
    Other(u16),
}

impl Arrows {
    pub fn from_u16(v: u16) -> Arrows {
        match v {
            0 => Arrows::None,
            1 => Arrows::Start,
            2 => Arrows::End,
            3 => Arrows::Both,
            o => Arrows::Other(o),
        }
    }
    pub fn to_u16(self) -> u16 {
        match self {
            Arrows::None => 0,
            Arrows::Start => 1,
            Arrows::End => 2,
            Arrows::Both => 3,
            Arrows::Other(o) => o,
        }
    }
    pub fn xml(self) -> String {
        match self {
            Arrows::None => "None".into(),
            Arrows::Start => "Start".into(),
            Arrows::End => "End".into(),
            Arrows::Both => "Both".into(),
            Arrows::Other(o) => o.to_string(),
        }
    }
}

/// Línea (XML `<Line>` con `<StartPoint X Y>`, `<EndPoint X Y>` relativos al objeto y `<Style BackColor BackBlink
/// LineColor LineBlink LineStyle LineWidth Direction>`).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub version: u32,
    pub line_color: Color,
    /// Color de fondo (relleno de las flechas / del patrón de línea).
    pub back_color: Color,
    pub line_blink: Blink,
    pub back_blink: Blink,
    pub line_width: f64,
    pub line_style: u32,
    /// Extremos en coordenadas absolutas del panel (double); la cabecera común lleva su rectángulo envolvente.
    pub start: (f64, f64),
    pub end: (f64, f64),
    pub back_trans: u32,
    pub line_trans: u32,
    pub arrows: Arrows,
    /// u32 siempre 0 (Vijeo lo ignora al leer).
    pub reserved: u32,
    /// Estilo de sombreado de la línea (sólo versión ≥ 2; 0 en todo el corpus).
    pub shading: Option<i16>,
}

impl Line {
    pub fn parse(b: &[u8]) -> R<Line> {
        let mut r = Reader::new(b);
        let version = r.u32()?;
        if version == 0 || version > 2 {
            return Err(format!("Line: versión {version} no soportada"));
        }
        let line_color = Color(r.u32()?);
        let back_color = Color(r.u32()?);
        let line_blink = Blink::from_u16(r.u16()?);
        let back_blink = Blink::from_u16(r.u16()?);
        let line_width = r.f64()?;
        let line_style = r.u32()?;
        let start = (r.f64()?, r.f64()?);
        let end_ = (r.f64()?, r.f64()?);
        let back_trans = r.u32()?;
        let line_trans = r.u32()?;
        let arrows = Arrows::from_u16(r.u16()?);
        let reserved = r.u32()?;
        let shading = if version >= 2 { Some(r.i16()?) } else { None };
        end(&r, "Line")?;
        Ok(Line {
            version,
            line_color,
            back_color,
            line_blink,
            back_blink,
            line_width,
            line_style,
            start,
            end: end_,
            back_trans,
            line_trans,
            arrows,
            reserved,
            shading,
        })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u32(self.version);
        w.u32(self.line_color.0);
        w.u32(self.back_color.0);
        w.u16(self.line_blink.to_u16());
        w.u16(self.back_blink.to_u16());
        w.f64(self.line_width);
        w.u32(self.line_style);
        w.f64(self.start.0);
        w.f64(self.start.1);
        w.f64(self.end.0);
        w.f64(self.end.1);
        w.u32(self.back_trans);
        w.u32(self.line_trans);
        w.u16(self.arrows.to_u16());
        w.u32(self.reserved);
        if self.version >= 2 {
            w.i16(self.shading.unwrap_or(0));
        }
        w.o
    }

    /// Línea nueva de (x1, y1) a (x2, y2) en coordenadas del panel, con el estilo más común del corpus
    /// (verde 319400 de 2 px, sin flechas). La cabecera común debe llevar el rectángulo
    /// (min x, min y, max x, max y) — ver [`Line::bounds`].
    pub fn default_line(x1: i32, y1: i32, x2: i32, y2: i32) -> Line {
        Line {
            version: 2,
            line_color: Color(0x0031_9400),
            back_color: Color::BLACK,
            line_blink: Blink::None,
            back_blink: Blink::None,
            line_width: 2.0,
            line_style: 0,
            start: (x1 as f64, y1 as f64),
            end: (x2 as f64, y2 as f64),
            back_trans: 0,
            line_trans: 0,
            arrows: Arrows::None,
            reserved: 0,
            shading: Some(0),
        }
    }

    /// Rectángulo inclusivo (l, t, r, b) que corresponde a los extremos (el de la cabecera común).
    pub fn bounds(&self) -> [i32; 4] {
        let (x1, y1, x2, y2) = (self.start.0 as i32, self.start.1 as i32, self.end.0 as i32, self.end.1 as i32);
        [x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2)]
    }

    /// Desplaza los extremos (al mover el objeto hay que mover también la cabecera).
    pub fn shift(&mut self, dx: f64, dy: f64) {
        self.start = (self.start.0 + dx, self.start.1 + dy);
        self.end = (self.end.0 + dx, self.end.1 + dy);
    }
}

// ---------------------------------------------------------------------------------------------------------
// Ellipse / Circle (CircObject.dll, CCircleObject, versión 2)
// ---------------------------------------------------------------------------------------------------------

/// Elipse (XML `<Ellipse>`, `<Style BackColor BackBlink LineColor LineBlink LineStyle LineWidth ForeColor
/// ForeBlink Pattern>`). Ocupa el rectángulo de la cabecera común.
#[derive(Debug, Clone, PartialEq)]
pub struct Ellipse {
    pub version: u32,
    pub fore_color: Color,
    pub line_color: Color,
    pub back_color: Color,
    pub fore_blink: Blink,
    pub line_blink: Blink,
    pub back_blink: Blink,
    pub line_width: f64,
    /// Estilo de línea (aquí u16, en el rectángulo u32).
    pub line_style: u16,
    pub pattern: u16,
    pub line_trans: u32,
    pub fore_trans: u32,
    pub back_trans: u32,
    /// u32 que Vijeo pone a 0 antes de escribirlo.
    pub reserved: u32,
    pub fore_gradient: Option<Gradient>,
    pub line_gradient: Option<Gradient>,
}

impl Ellipse {
    pub fn parse(b: &[u8]) -> R<Ellipse> {
        let mut r = Reader::new(b);
        let version = r.u32()?;
        if version == 0 || version > 2 {
            return Err(format!("Ellipse: versión {version} no soportada"));
        }
        let fore_color = Color(r.u32()?);
        let line_color = Color(r.u32()?);
        let back_color = Color(r.u32()?);
        let fore_blink = Blink::from_u16(r.u16()?);
        let line_blink = Blink::from_u16(r.u16()?);
        let back_blink = Blink::from_u16(r.u16()?);
        let line_width = r.f64()?;
        let line_style = r.u16()?;
        let pattern = r.u16()?;
        let line_trans = r.u32()?;
        let fore_trans = r.u32()?;
        let back_trans = r.u32()?;
        let reserved = r.u32()?;
        let (fore_gradient, line_gradient) =
            if version >= 2 { (Some(Gradient::parse(&mut r)?), Some(Gradient::parse(&mut r)?)) } else { (None, None) };
        end(&r, "Ellipse")?;
        Ok(Ellipse {
            version,
            fore_color,
            line_color,
            back_color,
            fore_blink,
            line_blink,
            back_blink,
            line_width,
            line_style,
            pattern,
            line_trans,
            fore_trans,
            back_trans,
            reserved,
            fore_gradient,
            line_gradient,
        })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u32(self.version);
        w.u32(self.fore_color.0);
        w.u32(self.line_color.0);
        w.u32(self.back_color.0);
        w.u16(self.fore_blink.to_u16());
        w.u16(self.line_blink.to_u16());
        w.u16(self.back_blink.to_u16());
        w.f64(self.line_width);
        w.u16(self.line_style);
        w.u16(self.pattern);
        w.u32(self.line_trans);
        w.u32(self.fore_trans);
        w.u32(self.back_trans);
        w.u32(self.reserved);
        if self.version >= 2 {
            self.fore_gradient.clone().unwrap_or_else(|| Gradient::plain(0x02FF_0000)).write(&mut w);
            self.line_gradient.clone().unwrap_or_else(|| Gradient::plain(0x02FF_FFFF)).write(&mut w);
        }
        w.o
    }

    /// Elipse nueva: relleno naranja 80FF con borde blanco de 1 px (las del corpus).
    pub fn default_ellipse() -> Ellipse {
        Ellipse {
            version: 2,
            fore_color: Color(0x0000_80FF),
            line_color: Color::WHITE,
            back_color: Color::BLACK,
            fore_blink: Blink::None,
            line_blink: Blink::None,
            back_blink: Blink::None,
            line_width: 1.0,
            line_style: 0,
            pattern: 1,
            line_trans: 0,
            fore_trans: 0,
            back_trans: 0,
            reserved: 0,
            fore_gradient: Some(Gradient::plain(0x02FF_0000)),
            line_gradient: Some(Gradient::plain(0x02FF_FFFF)),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Fuentes y textos multilingües (HMIManagers.dll: CLanguageFont, CLanguageString, CLanguageDataList)
// ---------------------------------------------------------------------------------------------------------

/// Tipo de fuente (XML FontType): GP 0 (fuentes de mapa de bits del terminal), TrueType 1, AGFA 2.
pub const FONT_GP: u32 = 0;
pub const FONT_TRUETYPE: u32 = 1;
pub const FONT_AGFA: u32 = 2;
/// Estilo (XML FontStyle): Normal 0, Bold 1, 3D 2.
pub const STYLE_NORMAL: u32 = 0;
pub const STYLE_BOLD: u32 = 1;
pub const STYLE_3D: u32 = 2;
/// Caras GP (XML FontFace con FontType="GP").
pub const GP_FACES: [&str; 16] = [
    "GPFontSansSerif",
    "GPFontJGothic",
    "GPFontJMincho",
    "GPFontKGothic",
    "GPFontKMincho",
    "GPFontSongTi",
    "GPFontFangSong",
    "GPFontTaiSongTi",
    "GPFontTaiMingLiu",
    "GPFontModern6x10",
    "GPFontModern8x13",
    "GPFontModern8x13F",
    "GPFontModern13x23",
    "GPFontModern13x23F",
    "GPFontModern28x50",
    "GPFontCyrillic",
];
/// Caras AGFA (XML FontFace con FontType="AGFA").
pub const AGFA_FACES: [&str; 10] = [
    "AgfaFontJapanese",
    "AgfaFontKorean",
    "AgfaFontSmpChinese",
    "AgfaFontTrdChinese",
    "AgfaFontLatin",
    "AgfaFontSegment",
    "AgfaFontUtahWGL",
    "AgfaFontAndaleWGL",
    "AgfaFontArabic",
    "AgfaFontHebrew",
];

/// CLanguageFont (versión 4). Guarda a la vez los datos de los tres tipos de fuente; el XML sólo muestra los
/// del tipo activo: GP → FontFace=`GP_FACES[gp_face]`, FontWidth=`gp_width`, FontHeightOrSize=`gp_height`;
/// TrueType → TrueTypeFontFace=`tt_face`, FontWidth=`tt_width`, FontHeightOrSize=`tt_height`;
/// AGFA → FontFace=`AGFA_FACES[agfa_face]`, FontSize=puntos(`agfa_size` px).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    pub version: u32,
    pub font_type: u32,
    pub style: u32,
    /// TrueType: alto (lfHeight, negativo = alto de carácter) y segundo valor (FontWidth en el XML).
    pub tt_height: i32,
    pub tt_width: i32,
    pub tt_face: Option<String>,
    pub gp_height: i32,
    pub gp_width: i32,
    pub gp_face: u32,
    pub charset: u8,
    pub agfa_size: i32,
    pub agfa_face: u32,
}

impl Font {
    fn parse(r: &mut Reader) -> R<Font> {
        let version = r.u32()?;
        if version != 4 {
            return Err(format!("CLanguageFont: versión {version} en {:#x}", r.p - 4));
        }
        Ok(Font {
            version,
            font_type: r.u32()?,
            style: r.u32()?,
            tt_height: r.i32()?,
            tt_width: r.i32()?,
            tt_face: rd_bstr(r)?,
            gp_height: r.i32()?,
            gp_width: r.i32()?,
            gp_face: r.u32()?,
            charset: r.u8()?,
            agfa_size: r.i32()?,
            agfa_face: r.u32()?,
        })
    }
    fn write(&self, w: &mut Writer) {
        w.u32(self.version);
        w.u32(self.font_type);
        w.u32(self.style);
        w.i32(self.tt_height);
        w.i32(self.tt_width);
        wr_bstr(w, &self.tt_face);
        w.i32(self.gp_height);
        w.i32(self.gp_width);
        w.u32(self.gp_face);
        w.u8(self.charset);
        w.i32(self.agfa_size);
        w.u32(self.agfa_face);
    }

    /// Fuente GP propia de un texto, como las del corpus. `face`: índice en [`GP_FACES`]
    /// (9 = GPFontModern6x10, 10 = GPFontModern8x13, 11 = GPFontModern8x13F, 0 = GPFontSansSerif);
    /// `bold`: FontStyle Bold. Tamaño: el fijo de la cara ([`gp_size`]).
    pub fn gp(face: u32, bold: bool) -> Font {
        let (w, h) = gp_size(face);
        Font::gp_sized(face, bold, w, h)
    }

    /// Igual que [`Font::gp`] con ancho y alto explícitos (GPFontSansSerif admite 8×8 y 8×16 en el corpus).
    /// Los campos TrueType/AGFA "a la sombra" llevan lo que Vijeo guarda junto a la fuente GP: Lucida Console
    /// de -8 (6x10) o -11 (8x13, 8x13F, SansSerif), peso 400/700, y la misma altura en `agfa_size`.
    pub fn gp_sized(face: u32, bold: bool, width: i32, height: i32) -> Font {
        let tt_h = if face == 9 { -8 } else { -11 };
        Font {
            version: 4,
            font_type: FONT_GP,
            style: if bold { STYLE_BOLD } else { STYLE_NORMAL },
            tt_height: tt_h,
            tt_width: if bold { 700 } else { 400 },
            tt_face: Some("Lucida Console".into()),
            gp_height: height,
            gp_width: width,
            gp_face: face,
            charset: 0,
            agfa_size: tt_h,
            agfa_face: 4,
        }
    }

    /// Valores de FontType/FontStyle/FontFace/FontWidth/FontHeightOrSize del XML.
    pub fn xml(&self) -> Vec<(&'static str, String)> {
        let ty = match self.font_type {
            0 => "GP".to_string(),
            1 => "TrueType".to_string(),
            2 => "AGFA".to_string(),
            o => o.to_string(),
        };
        let st = match self.style {
            0 => "Normal".to_string(),
            1 => "Bold".to_string(),
            2 => "3D".to_string(),
            o => o.to_string(),
        };
        let mut v = vec![("FontType", ty), ("FontStyle", st)];
        match self.font_type {
            FONT_GP => {
                v.push(("FontFace", GP_FACES.get(self.gp_face as usize).map(|s| s.to_string()).unwrap_or_default()));
                v.push(("FontWidth", self.gp_width.to_string()));
                v.push(("FontHeightOrSize", self.gp_height.to_string()));
            }
            FONT_TRUETYPE => {
                v.push(("TrueTypeFontFace", self.tt_face.clone().unwrap_or_default()));
                v.push(("FontWidth", self.tt_width.to_string()));
                v.push(("FontHeightOrSize", self.tt_height.to_string()));
            }
            _ => {
                v.push(("FontFace", AGFA_FACES.get(self.agfa_face as usize).map(|s| s.to_string()).unwrap_or_default()));
            }
        }
        v
    }
}

/// Ancho y alto en píxeles de las caras GP (los de su nombre; SansSerif: 8×16, lo más común de la Toolchest;
/// las asiáticas y la cirílica no aparecen en el corpus: 8×16 por defecto, sin comprobar).
pub fn gp_size(face: u32) -> (i32, i32) {
    match face {
        9 => (6, 10),
        10 | 11 => (8, 13),
        12 | 13 => (13, 23),
        14 => (28, 50),
        _ => (8, 16),
    }
}

/// (color, bandera) para un color opcional: `None` = transparente (`08000000` y bandera 1, como guarda
/// Vijeo: el bit en el color y la bandera van juntos en el 98 % del corpus). Sirve para cualquier pareja
/// color/`*_trans` de las cuatro clases.
pub fn paint(c: Option<Color>) -> (Color, u32) {
    match c {
        None => (Color::transparent(), 1),
        Some(c) => (Color(c.0 & !TRANSPARENT_BIT), 0),
    }
}

/// Entrada de un idioma en CLanguageDataList.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageEntry {
    /// Nombre del idioma del proyecto ("Language1").
    pub language: Option<String>,
    /// Texto. Varias líneas separadas por LF (`\n`, sin CR; en el XML, salto de línea).
    pub text: Option<String>,
    /// Fuente por defecto del idioma (la del proyecto; no sale en el XML).
    pub language_font: Font,
    /// Fuente propia del texto (la del XML `<Font>`). `None` = usa la del idioma.
    pub font: Option<Font>,
    /// UserLanguageID del XML (1 = "L1").
    pub language_id: u32,
    /// u32 sin identificar (0 en todo el corpus).
    pub extra: u32,
}

/// CLanguageDataList (versión 3): textos por idioma.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageDataList {
    pub version: u32,
    pub entries: Vec<LanguageEntry>,
}

impl LanguageDataList {
    fn parse(r: &mut Reader) -> R<LanguageDataList> {
        let version = r.u32()?;
        if version != 3 {
            return Err(format!("CLanguageDataList: versión {version} en {:#x}", r.p - 4));
        }
        let n = r.u32()? as usize;
        if n > 1000 {
            return Err(format!("CLanguageDataList: {n} idiomas"));
        }
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            let language = rd_bstr(r)?;
            let text = rd_bstr(r)?;
            let language_font = Font::parse(r)?;
            let has = r.u32()?;
            let font = match has {
                0 => None,
                1 => Some(Font::parse(r)?),
                o => return Err(format!("CLanguageDataList: bandera de fuente {o}")),
            };
            let language_id = r.u32()?;
            let extra = r.u32()?;
            entries.push(LanguageEntry { language, text, language_font, font, language_id, extra });
        }
        Ok(LanguageDataList { version, entries })
    }
    fn write(&self, w: &mut Writer) {
        w.u32(self.version);
        w.u32(self.entries.len() as u32);
        for e in &self.entries {
            wr_bstr(w, &e.language);
            wr_bstr(w, &e.text);
            e.language_font.write(w);
            match &e.font {
                None => w.u32(0),
                Some(f) => {
                    w.u32(1);
                    f.write(w);
                }
            }
            w.u32(e.language_id);
            w.u32(e.extra);
        }
    }
}

/// CLanguageString (versión 2): texto + fuente opcional. En Text sólo aparece si BitmapDisplay está activo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageString {
    pub version: u32,
    pub text: Option<String>,
    pub font: Option<Font>,
}

impl LanguageString {
    fn parse(r: &mut Reader) -> R<LanguageString> {
        let version = r.u32()?;
        let text = rd_bstr(r)?;
        let has = r.u8()?;
        let font = if has != 0 { Some(Font::parse(r)?) } else { None };
        Ok(LanguageString { version, text, font })
    }
    fn write(&self, w: &mut Writer) {
        w.u32(self.version);
        wr_bstr(w, &self.text);
        w.u8(self.font.is_some() as u8);
        if let Some(f) = &self.font {
            f.write(w);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Text (TextObject.dll, CTextObject, versión 11)
// ---------------------------------------------------------------------------------------------------------

/// Alineación horizontal (g_horzAlignMap): Left 0, Center 1, Right 2.
pub const H_LEFT: u16 = 0;
pub const H_CENTER: u16 = 1;
pub const H_RIGHT: u16 = 2;
/// Alineación vertical (g_vertAlignMap): Top 0, Middle 1, Bottom 2.
pub const V_TOP: u16 = 0;
pub const V_MIDDLE: u16 = 1;
pub const V_BOTTOM: u16 = 2;
pub const H_ALIGN_XML: [&str; 3] = ["Left", "Center", "Right"];
pub const V_ALIGN_XML: [&str; 3] = ["Top", "Middle", "Bottom"];

/// Texto (XML `<Text HAlign VAlign>` con `<LanguageString>`, `<Font>` y `<Style BackColor BackBlink LineColor
/// LineBlink LineStyle LineWidth TextColor TextBlink Color3D Blink3D TextLineSpacing GroupNumber OrderNumber
/// TextRotation>`).
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub version: u32,
    pub text_blink: Blink,
    pub back_blink: Blink,
    pub blink_3d: Blink,
    /// Propiedad CDirection (dirección del texto; no sale en el XML; 0 en todo el corpus).
    pub direction: u16,
    /// Interlineado en píxeles (XML TextLineSpacing).
    pub line_spacing: u16,
    pub h_align: u16,
    pub v_align: u16,
    /// Giro en cuartos de vuelta (XML TextRotation = 90 × valor).
    pub rotation: u32,
    pub text_color: Color,
    /// Fondo del cuadro de texto (transparente: `Color::transparent()` y `back_trans = 1`).
    pub back_color: Color,
    /// Color del relieve 3D (estilo de fuente 3D).
    pub color_3d: Color,
    pub text_trans: u32,
    pub back_trans: u32,
    pub trans_3d: u32,
    /// Id de la cadena en el gestor de idiomas del proyecto (Vijeo lo reasigna al pegar).
    pub string_id: u32,
    /// Banderas de guardado: 9 en el portapapeles (bit 0: los textos van incluidos; bit 3: al leer se
    /// reinician GroupNumber/OrderNumber).
    pub save_flags: u32,
    /// Textos por idioma (presentes si `save_flags & 1`; `None` si Vijeo escribió "no hay lista").
    pub languages: Option<LanguageDataList>,
    /// Borde del cuadro de texto.
    pub line_color: Color,
    pub line_trans: u32,
    pub line_blink: Blink,
    pub line_width: u32,
    pub line_style: u32,
    /// BitmapDisplay (bool de Vijeo: 0 / 0xFFFF).
    pub bitmap_display: u16,
    /// Grupo y orden de tabulación (XML GroupNumber/OrderNumber; -1/0 = sin grupo).
    pub group_number: i16,
    pub order_number: i16,
    /// Texto de mapa de bits (sólo si `bitmap_display != 0`).
    pub bitmap_string: Option<LanguageString>,
    pub back_gradient: Gradient,
    pub text_gradient: Gradient,
    pub line_gradient: Gradient,
}

impl Text {
    pub fn parse(b: &[u8]) -> R<Text> {
        let mut r = Reader::new(b);
        let version = r.u32()?;
        if version != 11 {
            return Err(format!("Text: versión {version} no soportada (sólo 11)"));
        }
        let text_blink = Blink::from_u16(r.u16()?);
        let back_blink = Blink::from_u16(r.u16()?);
        let blink_3d = Blink::from_u16(r.u16()?);
        let direction = r.u16()?;
        let line_spacing = r.u16()?;
        let h_align = r.u16()?;
        let v_align = r.u16()?;
        let rotation = r.u32()?;
        let text_color = Color(r.u32()?);
        let back_color = Color(r.u32()?);
        let color_3d = Color(r.u32()?);
        let text_trans = r.u32()?;
        let back_trans = r.u32()?;
        let trans_3d = r.u32()?;
        let string_id = r.u32()?;
        let save_flags = r.u32()?;
        let languages = if save_flags & 1 != 0 {
            match r.u32()? {
                0 => None,
                1 => Some(LanguageDataList::parse(&mut r)?),
                o => return Err(format!("Text: bandera de textos {o}")),
            }
        } else {
            None
        };
        let line_color = Color(r.u32()?);
        let line_trans = r.u32()?;
        let line_blink = Blink::from_u16(r.u16()?);
        let line_width = r.u32()?;
        let line_style = r.u32()?;
        let bitmap_display = r.u16()?;
        let group_number = r.i16()?;
        let order_number = r.i16()?;
        let bitmap_string = if bitmap_display != 0 { Some(LanguageString::parse(&mut r)?) } else { None };
        let back_gradient = Gradient::parse(&mut r)?;
        let text_gradient = Gradient::parse(&mut r)?;
        let line_gradient = Gradient::parse(&mut r)?;
        end(&r, "Text")?;
        Ok(Text {
            version,
            text_blink,
            back_blink,
            blink_3d,
            direction,
            line_spacing,
            h_align,
            v_align,
            rotation,
            text_color,
            back_color,
            color_3d,
            text_trans,
            back_trans,
            trans_3d,
            string_id,
            save_flags,
            languages,
            line_color,
            line_trans,
            line_blink,
            line_width,
            line_style,
            bitmap_display,
            group_number,
            order_number,
            bitmap_string,
            back_gradient,
            text_gradient,
            line_gradient,
        })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u32(self.version);
        w.u16(self.text_blink.to_u16());
        w.u16(self.back_blink.to_u16());
        w.u16(self.blink_3d.to_u16());
        w.u16(self.direction);
        w.u16(self.line_spacing);
        w.u16(self.h_align);
        w.u16(self.v_align);
        w.u32(self.rotation);
        w.u32(self.text_color.0);
        w.u32(self.back_color.0);
        w.u32(self.color_3d.0);
        w.u32(self.text_trans);
        w.u32(self.back_trans);
        w.u32(self.trans_3d);
        w.u32(self.string_id);
        w.u32(self.save_flags);
        if self.save_flags & 1 != 0 {
            match &self.languages {
                None => w.u32(0),
                Some(l) => {
                    w.u32(1);
                    l.write(&mut w);
                }
            }
        }
        w.u32(self.line_color.0);
        w.u32(self.line_trans);
        w.u16(self.line_blink.to_u16());
        w.u32(self.line_width);
        w.u32(self.line_style);
        w.u16(self.bitmap_display);
        w.i16(self.group_number);
        w.i16(self.order_number);
        if self.bitmap_display != 0 {
            match &self.bitmap_string {
                Some(s) => s.write(&mut w),
                None => LanguageString { version: 2, text: None, font: None }.write(&mut w),
            }
        }
        self.back_gradient.write(&mut w);
        self.text_gradient.write(&mut w);
        self.line_gradient.write(&mut w);
        w.o
    }

    /// Texto (de un idioma) en el primer idioma, si lo hay.
    pub fn text(&self) -> Option<&str> {
        self.languages.as_ref()?.entries.first()?.text.as_deref()
    }

    /// Cambia el texto del idioma `language_id` (1 = L1). Las líneas se separan con LF (`\n`), como guarda
    /// Vijeo; un CRLF se convierte en LF.
    pub fn set_text(&mut self, language_id: u32, text: &str) -> R<()> {
        let norm = text.replace("\r\n", "\n");
        let l = self.languages.as_mut().ok_or("el texto no lleva lista de idiomas")?;
        let e = l.entries.iter_mut().find(|e| e.language_id == language_id).ok_or("idioma no encontrado")?;
        e.text = Some(norm);
        Ok(())
    }

    /// Texto nuevo en el idioma 1 ("Language1") con el estilo más común del corpus: negro sobre fondo
    /// transparente, sin borde, GPFontModern6x10 normal, alineado a la izquierda y centrado en vertical.
    pub fn default_text(text: &str) -> Text {
        let norm = text.replace("\r\n", "\n");
        Text {
            version: 11,
            text_blink: Blink::None,
            back_blink: Blink::None,
            blink_3d: Blink::None,
            direction: 0,
            line_spacing: 0,
            h_align: H_LEFT,
            v_align: V_MIDDLE,
            rotation: 0,
            text_color: Color::BLACK,
            back_color: Color::transparent(),
            color_3d: Color(0x00FF_0000),
            text_trans: 0,
            back_trans: 1,
            trans_3d: 0,
            string_id: 0,
            save_flags: 9,
            languages: Some(LanguageDataList {
                version: 3,
                entries: vec![LanguageEntry {
                    language: Some("Language1".into()),
                    text: Some(norm),
                    language_font: Font::language_default(),
                    font: Some(Font::gp(9, false)),
                    language_id: 1,
                    extra: 0,
                }],
            }),
            line_color: Color(0x08FF_FFFF),
            line_trans: 1,
            line_blink: Blink::None,
            line_width: 1,
            line_style: 0,
            bitmap_display: 0,
            group_number: -1,
            order_number: 0,
            bitmap_string: None,
            back_gradient: Gradient::plain(0x0200_0000),
            text_gradient: Gradient::plain(0x02FF_FFFF),
            line_gradient: Gradient::plain(0x02FF_FFFF),
        }
    }

    /// Fuente propia del primer idioma (la del XML).
    pub fn font_mut(&mut self) -> Option<&mut Font> {
        self.languages.as_mut()?.entries.first_mut()?.font.as_mut()
    }
}

impl Font {
    /// Fuente por defecto del idioma "Language1" de los proyectos del corpus (GPFontModern8x13 normal).
    pub fn language_default() -> Font {
        Font {
            version: 4,
            font_type: FONT_GP,
            style: STYLE_NORMAL,
            tt_height: -19,
            tt_width: 400,
            tt_face: Some("Arial".into()),
            gp_height: 13,
            gp_width: 8,
            gp_face: 10,
            charset: 0,
            agfa_size: -19,
            agfa_face: 4,
        }
    }
}
