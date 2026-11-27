//! Animaciones de un objeto gráfico de Vijeo-Designer 6.3 (formato del portapapeles, `ObjectData`).
//!
//! Van en la **extensión** de la cabecera común (BaseObject.dll): tras el byte de flags (bit 0 = hay extensión) y
//! antes de la marca `0x1CED50DA`. La escribe el `AnimationMgrObject` (AnimationManager.dll, `IPersistStream::Save`):
//!
//! ```text
//! u32 3                                   versión del gestor
//! { u32 n · GUID de clase (16 B) · n B }*  una por animación, en el orden de las ranuras (ver `Kind`)
//! u32 0                                   fin
//! ```
//!
//! Los `n` bytes de cada animación son el `DoPropExchange` del control MFC (.ocx) sobre un `CArchive`
//! (`CArchivePropExchange`): versión u32 (`ExchangeVersion`), `PX_Bool` = u8, `PX_Short` = i16, `PX_Long` = u32,
//! `PX_Double` = f64, `PX_String` = CString de MFC y `CExpression::DoExpressionPropExchange` (ver `Expr`).
//! Los controles que llaman a `COleControl::DoPropExchange` añaden antes la extensión OLE (ver `Extent`).
//! El gestor no salta al final del bloque tras cargar una animación: los datos tienen que ser exactos.
//!
//! Sólo std; usa `super::bin` (Reader/Writer).

use super::bin::{Reader, Writer, R};

pub const MANAGER_VERSION: u32 = 3;
pub const EXPR_VERSION: u32 = 0x3F3;
pub const MARKER: u32 = 0x1CED50DA;
/// Extensión sin animaciones (gestor presente y vacío), la forma habitual en los objetos sin animar.
pub const EMPTY_EXT: [u8; 8] = [3, 0, 0, 0, 0, 0, 0, 0];

pub type Guid = [u8; 16];

/// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` a los 16 bytes del stream (Data1, Data2 y Data3 en little-endian).
pub fn guid(s: &str) -> Guid {
    let h: Vec<u8> = s
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_digit(16).unwrap() as u8)
        .collect();
    assert_eq!(h.len(), 32, "GUID mal formado: {s}");
    let mut b = [0u8; 16];
    for i in 0..16 {
        b[i] = h[2 * i] << 4 | h[2 * i + 1];
    }
    let mut g = b;
    g[0..4].copy_from_slice(&[b[3], b[2], b[1], b[0]]);
    g[4..6].copy_from_slice(&[b[5], b[4]]);
    g[6..8].copy_from_slice(&[b[7], b[6]]);
    g
}

pub fn guid_str(g: &Guid) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        u32::from_le_bytes([g[0], g[1], g[2], g[3]]),
        u16::from_le_bytes([g[4], g[5]]),
        u16::from_le_bytes([g[6], g[7]]),
        g[8], g[9], g[10], g[11], g[12], g[13], g[14], g[15]
    )
}

// ---------------------------------------------------------------------------------------------------------------
// Clases (ranuras del gestor)
// ---------------------------------------------------------------------------------------------------------------

/// Las 10 ranuras del gestor, en el orden en que se guardan (tabla de AnimationManager.dll, nombres de
/// `HKLM\SOFTWARE\[WOW6432Node\]Schneider Electric\Vijeo-Designer63\Vijeo-Frame\Vijeo-Designer63\GraphicalEditor\Animations`).
/// Una animación de cada clase como mucho por objeto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Color = 0,
    Fill = 1,
    Size = 2,
    /// Sin clase registrada en 6.3.
    Slider = 3,
    Position = 4,
    Rotate = 5,
    Touch = 6,
    Value = 7,
    Visibility = 8,
    SmartObject = 9,
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Color,
        Kind::Fill,
        Kind::Size,
        Kind::Slider,
        Kind::Position,
        Kind::Rotate,
        Kind::Touch,
        Kind::Value,
        Kind::Visibility,
        Kind::SmartObject,
    ];

    pub fn slot(self) -> usize {
        self as usize
    }

    /// GUID tal como va en el stream (ya traducido por `CPFUtility::CLSIDReverseLookup`).
    pub fn guid_str(self) -> Option<&'static str> {
        Some(match self {
            Kind::Color => "{67888E8D-9486-4065-A5D5-31B725501695}",
            Kind::Fill => "{5626009D-9182-495C-B817-1164423C2A7C}",
            Kind::Size => "{11CC3AEA-E3EC-41F2-BBA3-A3DF553DFDAC}",
            Kind::Slider => return None,
            Kind::Position => "{23FFD5F6-7A66-4F71-99BA-66404EC9D0F3}",
            Kind::Rotate => "{372D2B71-1692-49E5-AB9B-49E320D6F8D6}",
            Kind::Touch => "{765B4F06-4A94-400A-85A7-20BC624D09BB}",
            Kind::Value => "{49B52EE2-DAB7-4D8A-BC69-417CD9037A61}",
            Kind::Visibility => "{521E8F5F-A2C3-4814-A903-A78A52A9C720}",
            Kind::SmartObject => "{ED248C23-F5AE-4FBB-BD00-811BA61B3B05}",
        })
    }

    pub fn guid(self) -> Option<Guid> {
        self.guid_str().map(guid)
    }

    pub fn from_guid(g: &Guid) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.guid().as_ref() == Some(g))
    }

    /// Versión que escribe Vijeo 6.3 (`ExchangeVersion`); `None` si no se tipa.
    pub fn version(self) -> Option<u32> {
        Some(match self {
            Kind::Color => 9,
            Kind::Fill => 5,
            Kind::Size => 4,
            Kind::Position => 4,
            Kind::Rotate => 4,
            Kind::Value => 7,
            Kind::Visibility => 4,
            Kind::SmartObject => 1,
            _ => return None,
        })
    }

    /// Nombre del elemento del XML (public.xsd) más parecido.
    pub fn xml_name(self) -> &'static str {
        match self {
            Kind::Color => "ForeColorAnimation/BackColorAnimation/LineColorAnimation/StateColorAnimation",
            Kind::Fill => "FillAnimation",
            Kind::Size => "SizeAnimation",
            Kind::Slider => "-",
            Kind::Position => "PositionAnimation",
            Kind::Rotate => "RotateAnimation",
            Kind::Touch => "TouchAnimation",
            Kind::Value => "ValueAnimation",
            Kind::Visibility => "VisibilityAnimation",
            Kind::SmartObject => "SmartObject",
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Primitivas PX_* sobre CArchive
// ---------------------------------------------------------------------------------------------------------------

fn rbool(r: &mut Reader) -> R<bool> {
    let at = r.p;
    match r.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        v => Err(format!("PX_Bool con valor {v} en {at:#x}")),
    }
}

fn wbool(w: &mut Writer, v: bool) {
    w.u8(v as u8);
}

/// Extensión OLE que escriben los controles que llaman a `COleControl::DoPropExchange` (Color, Size, Position,
/// Value): `PX_Long _ExtentX`, `PX_Long _ExtentY` (HIMETRIC) y `PX_ULong _StockProps` (siempre 0: sin propiedades de
/// serie; si no lo es, la animación se conserva en bruto).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extent {
    pub cx: i32,
    pub cy: i32,
}

impl Extent {
    /// 100 x 50 px: el tamaño por defecto del control, el que llevan todas las del corpus.
    pub const DEFAULT: Extent = Extent { cx: 2646, cy: 1323 };

    fn read(r: &mut Reader) -> R<Extent> {
        let cx = r.i32()?;
        let cy = r.i32()?;
        let at = r.p;
        let stock = r.u32()?;
        if stock != 0 {
            return Err(format!("propiedades de serie {stock:#x} en {at:#x}"));
        }
        Ok(Extent { cx, cy })
    }

    fn write(&self, w: &mut Writer) {
        w.i32(self.cx);
        w.i32(self.cy);
        w.u32(0);
    }
}

impl Default for Extent {
    fn default() -> Self {
        Extent::DEFAULT
    }
}

// ---------------------------------------------------------------------------------------------------------------
// CExpression en forma CArchive (CExpression::DoExpressionPropExchange, versión 0x3F3)
// ---------------------------------------------------------------------------------------------------------------

/// Referencia a variable o expresión, tal como la guarda `CExpression::DoExpressionPropExchange` en un CArchive.
/// Mismos campos que `CExpression::Save` (BSTR), pero con CString, y `PlaceHolderFlagValue` y el tamaño del mapa
/// de marcadores como i16.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    /// `RawString`: `{F,2:2.<id>,Carpeta}.{T,2:2.<id>,Var}` (con `$...$` alrededor del marcador de estructura).
    pub raw: String,
    /// `BuildString`: compilada, p. ej. `(TagDB.X.getIntValue(0) != 0)`.
    pub build: String,
    /// `CfgBuildString`: normalmente igual a `build` (en las constantes, a veces un resto de otra expresión).
    pub cfg_build: String,
    /// `ValidBuild`: 0 compilada; -1 (y 1) sin compilar/no usada.
    pub valid_build: i16,
    /// `ExpressionDataType`: 0x02 bit, 0x10, 0x20 booleana, 0x40 entera, 0x80 real...
    pub data_type: u32,
    /// `ExpressionType`: 2 destino, 4 valor, 5 constante, 8 condición, 0x1005 valor de un display...
    pub expr_type: u32,
    /// `LiteralExprType`: 2 referencia a variable, 1 literal compilado, 0x20 vacía o literal sin compilar.
    pub literal_type: u32,
    /// `m_mUsedTagName`: variables usadas (`Carpeta.Var\n`).
    pub used_tags: String,
    /// `m_mTagListForCGDependence` (`TagDB.Carpeta.Var\n`).
    pub cg_dependence: String,
    /// `m_mArrayTagListForCGDataQuality`.
    pub cg_array_quality: String,
    /// `m_mSingleTagListForCGDataQuality` (`TagDB.Carpeta.Var\n`).
    pub cg_single_quality: String,
    /// `m_mArrayTagNameList`.
    pub array_tags: String,
    /// `m_mRealArrayTagNameList`.
    pub real_array_tags: String,
    /// `m_mArrayIndexStringList`.
    pub array_index: String,
    /// `LastValidatedExprDataType`.
    pub last_validated_type: u32,
    /// `PlaceHolderFlagValue` (byte de flags de la expresión; 0x06 / 0x26 / 0x36 / 0x46 / 0x66 / 0x76 / 0x86 en el corpus).
    pub ph_flags: u16,
    /// Mapa de marcadores (`$...$`): (marcador en forma raw, descripción).
    pub placeholders: Vec<(String, String)>,
}

impl Expr {
    pub fn read(r: &mut Reader) -> R<Expr> {
        r.expect_u32(EXPR_VERSION, "CExpression")?;
        let raw = r.cstring()?;
        let build = r.cstring()?;
        let cfg_build = r.cstring()?;
        let valid_build = r.i16()?;
        let data_type = r.u32()?;
        let expr_type = r.u32()?;
        let literal_type = r.u32()?;
        let used_tags = r.cstring()?;
        let cg_dependence = r.cstring()?;
        let cg_array_quality = r.cstring()?;
        let cg_single_quality = r.cstring()?;
        let array_tags = r.cstring()?;
        let real_array_tags = r.cstring()?;
        let array_index = r.cstring()?;
        let last_validated_type = r.u32()?;
        let ph_flags = r.u16()?;
        let at = r.p;
        let n = r.i16()?;
        if !(0..=256).contains(&n) {
            return Err(format!("mapa de marcadores con {n} entradas en {at:#x}"));
        }
        let mut placeholders = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let k = r.cstring()?;
            let v = r.cstring()?;
            placeholders.push((k, v));
        }
        Ok(Expr {
            raw,
            build,
            cfg_build,
            valid_build,
            data_type,
            expr_type,
            literal_type,
            used_tags,
            cg_dependence,
            cg_array_quality,
            cg_single_quality,
            array_tags,
            real_array_tags,
            array_index,
            last_validated_type,
            ph_flags,
            placeholders,
        })
    }

    pub fn write(&self, w: &mut Writer) {
        w.u32(EXPR_VERSION);
        w.cstring(&self.raw);
        w.cstring(&self.build);
        w.cstring(&self.cfg_build);
        w.i16(self.valid_build);
        w.u32(self.data_type);
        w.u32(self.expr_type);
        w.u32(self.literal_type);
        for s in [
            &self.used_tags,
            &self.cg_dependence,
            &self.cg_array_quality,
            &self.cg_single_quality,
            &self.array_tags,
            &self.real_array_tags,
            &self.array_index,
        ] {
            w.cstring(s);
        }
        w.u32(self.last_validated_type);
        w.u16(self.ph_flags);
        w.i16(self.placeholders.len() as i16);
        for (k, v) in &self.placeholders {
            w.cstring(k);
            w.cstring(v);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    // --- Constructores (valores copiados de las expresiones del corpus; ver NOTAS) ------------------------------

    /// Casilla sin usar, como la escribe Vijeo: sólo el tipo de dato y de uso (`ValidBuild` -1, `LiteralExprType`
    /// y `LastValidatedExprDataType` 0x20). `PlaceHolderFlagValue` 0x06: el nibble alto varía al azar en el corpus.
    pub fn unused(data_type: u32, expr_type: u32) -> Expr {
        Expr {
            raw: String::new(),
            build: String::new(),
            cfg_build: String::new(),
            valid_build: -1,
            data_type,
            expr_type,
            literal_type: 0x20,
            used_tags: String::new(),
            cg_dependence: String::new(),
            cg_array_quality: String::new(),
            cg_single_quality: String::new(),
            array_tags: String::new(),
            real_array_tags: String::new(),
            array_index: String::new(),
            last_validated_type: 0x20,
            ph_flags: 0x06,
            placeholders: Vec::new(),
        }
    }

    /// Condición sin usar (parpadeo desactivado, `InterlockTag`...): dt 0x20, et 8.
    pub fn unused_cond() -> Expr {
        Expr::unused(0x20, 8)
    }

    /// Control numérico sin usar (eje de posición/tamaño/relleno desactivado): dt 0x40, et 4.
    pub fn unused_value() -> Expr {
        Expr::unused(0x40, 4)
    }

    /// Constante numérica compilada (rangos de un eje activo, ángulos): `ValidBuild` 0, dt 0x40, et 5, lt 1,
    /// raw = compilada = `CfgBuildString` (en el corpus esta última suele ser un resto de la expresión anterior).
    pub fn number(v: f64) -> Expr {
        let s = fmt_num(v);
        let mut e = Expr::unused(0x40, 5);
        e.raw = s.clone();
        e.build = s.clone();
        e.cfg_build = s;
        e.valid_build = 0;
        e.literal_type = 1;
        e.last_validated_type = 0x40;
        e
    }

    /// Constante sin compilar (rangos de un eje desactivado: "0", "100"): sólo raw, `ValidBuild` -1, lt 0x20.
    pub fn number_unbuilt(v: f64) -> Expr {
        let mut e = Expr::unused(0x40, 5);
        e.raw = fmt_num(v);
        e
    }

    /// Valor de un estado de una animación de color ("0", "1"...): sólo raw, `ValidBuild` -1, lt 1, lv 0x40.
    pub fn state_value(v: f64) -> Expr {
        let mut e = Expr::unused(0x40, 5);
        e.raw = fmt_num(v);
        e.literal_type = 1;
        e.last_validated_type = 0x40;
        e
    }

    /// Referencia a una variable (`Carpeta.Sub.Var`) con el uso indicado. Los ids de la forma raw van a 0: Vijeo
    /// vuelve a enlazar por nombre al pegar (verificado con las expresiones BSTR de los botones, no con éstas).
    pub fn var(path: &str, u: Use) -> Expr {
        Expr::var_raw(&raw_ref(path), path, u)
    }

    /// Como `var`, con la forma raw ya hecha (p. ej. `raw_ref_member`); `path` es el nombre con puntos.
    pub fn var_raw(raw: &str, path: &str, u: Use) -> Expr {
        let tag = format!("TagDB.{path}");
        let (build, dt, et) = match u {
            Use::Cond => (format!("({tag}.getIntValue(0) != 0)"), 0x20, 8),
            Use::Int => (format!("({tag}.getIntValue(0))"), 0x40, 4),
            Use::Float => (format!("({tag}.getFloatValue(0))"), 0x80, 4),
            Use::Range => (format!("({tag}.getIntValue(0))"), 0x40, 5),
        };
        let mut e = Expr::unused(dt, et);
        e.raw = raw.to_string();
        e.cfg_build = build.clone();
        e.build = build;
        e.valid_build = 0;
        e.literal_type = 2;
        e.used_tags = format!("{path}\n");
        e.cg_dependence = format!("{tag}\n");
        e.cg_single_quality = format!("{tag}\n");
        e.last_validated_type = dt;
        e
    }

    /// Expresión compuesta ya compilada (`LiteralExprType` 8), como las del corpus: `raw` con las variables en forma
    /// `{T,2:2.0,Var}`, `build` la compilada (`(TagDB.X.getIntValue(0) != 0)` para un booleano,
    /// `(TagDB.X.getIntValue(0))` dentro de una comparación, `!` delante para negar) y `vars` las variables usadas
    /// (`Carpeta.Var`), cada una una vez. Ejemplo del corpus: raw `{F,..}.{T,..,iPopupControl}==1`,
    /// build `(TagDB.MyController.LXM28.iPopupControl.getIntValue(0))==1`.
    pub fn composite(raw: &str, build: &str, vars: &[&str], data_type: u32, expr_type: u32) -> Expr {
        let mut e = Expr::unused(data_type, expr_type);
        e.raw = raw.to_string();
        e.build = build.to_string();
        e.cfg_build = build.to_string();
        e.valid_build = 0;
        e.literal_type = 8;
        e.used_tags = vars.iter().map(|v| format!("{v}\n")).collect();
        e.cg_dependence = vars.iter().map(|v| format!("TagDB.{v}\n")).collect();
        e.cg_single_quality = e.cg_dependence.clone();
        e.last_validated_type = data_type;
        e
    }

    /// Expresión libre sin compilar (`raw` en la sintaxis del editor, con las variables en forma `{T,2:2.0,Var}`;
    /// ver `raw_ref`). Queda como las casillas que Vijeo aún no ha validado (`ValidBuild` -1, sin compilada).
    /// No verificado en Vijeo: que la compile al pegar o al compilar el proyecto.
    pub fn expression(raw: &str, data_type: u32, expr_type: u32) -> Expr {
        let mut e = Expr::unused(data_type, expr_type);
        e.raw = raw.to_string();
        e
    }
}

/// Uso de una referencia a variable dentro de una animación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// Condición booleana (visibilidad, parpadeo, color discreto): `(TagDB.X.getIntValue(0) != 0)`, dt 0x20, et 8.
    Cond,
    /// Valor entero (control de posición, tamaño, relleno, rotación; valor entero): `(TagDB.X.getIntValue(0))`,
    /// dt 0x40, et 4. La posición usa `getIntValue` también con variables reales.
    Int,
    /// Valor real (animación de valor de una variable real): `(TagDB.X.getFloatValue(0))`, dt 0x80, et 4.
    Float,
    /// Variable como límite de un rango (`...ValueRangeFromExpr`): como `Int` pero et 5.
    Range,
}

/// `Carpeta.Sub.Var` → `{F,2:2.0,Carpeta}.{F,2:2.0,Sub}.{T,2:2.0,Var}` (carpetas `F`, variable `T`).
pub fn raw_ref(path: &str) -> String {
    let parts: Vec<&str> = path.split('.').collect();
    let n = parts.len();
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{{{},2:2.0,{p}}}", if i + 1 == n { "T" } else { "F" }))
        .collect::<Vec<_>>()
        .join(".")
}

/// Miembro de una variable de estructura: `Carpeta.Estr` + `miembro` →
/// `{F,2:2.0,Carpeta}.{T,2:2.0,Estr}.{T,3:2.0.0,miembro}` (el miembro lleva 3 componentes de id).
/// Forma deducida del corpus (`{T,2:1.30,IpWdgtPara}.{T,3:1.30.10,rLvl4}`); con ids 0, sin verificar en Vijeo.
pub fn raw_ref_member(struct_path: &str, member: &str) -> String {
    format!("{}.{{T,3:2.0.0,{member}}}", raw_ref(struct_path))
}

/// Número como lo escribe el editor (`40`, `-200`, `0.5`).
pub fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Visibilidad (Visib.ocx, versión 4)
// ---------------------------------------------------------------------------------------------------------------

/// Animación SmartObject (SmartObj.ocx, v1): la llevan los grupos que son smart objects. `DoPropExchange`:
/// extensión OLE, `PX_Bool SmartObject` y `PX_String SOItemID` (el id de la pieza, p. ej.
/// `DATEDISP220804184201`; el XML lo exporta como `<SmartObject SOItem=…>`).
#[derive(Debug, Clone, PartialEq)]
pub struct SmartObjectAnim {
    pub extent: Extent,
    pub enabled: bool,
    pub item_id: String,
}

impl SmartObjectAnim {
    fn read(r: &mut Reader) -> R<Self> {
        Ok(SmartObjectAnim { extent: Extent::read(r)?, enabled: rbool(r)?, item_id: r.cstring()? })
    }

    fn write(&self, w: &mut Writer) {
        self.extent.write(w);
        wbool(w, self.enabled);
        w.cstring(&self.item_id);
    }
}

/// `VisibilityAnimation`. Sin campo de invertir: se invierte con la expresión (`!X`, `X == 0`).
#[derive(Debug, Clone, PartialEq)]
pub struct Visibility {
    /// `VisibBlk` (XML: hay `BlinkControl`).
    pub blink: bool,
    /// XML `BlinkControl`.
    pub blink_expr: Expr,
    /// `VisibSpeed` (XML `BlinkSpeed`): 0 `Fast`, 1 `Slow` (por defecto; también sin parpadeo).
    pub blink_speed: i16,
    /// `VisibVisib` (XML: hay `VisibilityControl`).
    pub visible: bool,
    /// XML `VisibilityControl`: visible mientras sea verdadera.
    pub visible_expr: Expr,
}

impl Visibility {
    pub const VERSION: u32 = 4;

    fn read(r: &mut Reader) -> R<Self> {
        Ok(Visibility {
            blink: rbool(r)?,
            blink_expr: Expr::read(r)?,
            blink_speed: r.i16()?,
            visible: rbool(r)?,
            visible_expr: Expr::read(r)?,
        })
    }

    fn write(&self, w: &mut Writer) {
        wbool(w, self.blink);
        self.blink_expr.write(w);
        w.i16(self.blink_speed);
        wbool(w, self.visible);
        self.visible_expr.write(w);
    }

    /// Visible mientras `cond` sea verdadera.
    pub fn when(cond: Expr) -> Self {
        Visibility { blink: false, blink_expr: Expr::unused_cond(), blink_speed: 1, visible: true, visible_expr: cond }
    }

    /// Visible mientras la variable booleana valga 1 (o 0 si `invert`).
    pub fn by_var(path: &str, invert: bool) -> Self {
        let mut e = Expr::var(path, Use::Cond);
        if invert {
            invert_cond(&mut e);
        }
        Visibility::when(e)
    }

    /// Añade parpadeo mientras `cond` sea verdadera (`fast`: `BlinkSpeed="Fast"`, si no `Slow`).
    pub fn with_blink(mut self, cond: Expr, fast: bool) -> Self {
        self.blink = true;
        self.blink_expr = cond;
        self.blink_speed = if fast { 0 } else { 1 };
        self
    }
}

/// Niega una condición creada con `Expr::var(.., Use::Cond)`, como las negaciones del corpus
/// (`!{F,...}.{T,...,xStatError}` / `!(TagDB....getIntValue(0) != 0)`, `LiteralExprType` 8 = expresión compuesta).
pub fn invert_cond(e: &mut Expr) {
    e.raw = format!("!{}", e.raw);
    e.build = format!("!{}", e.build);
    e.cfg_build = e.build.clone();
    e.literal_type = 8;
}

// ---------------------------------------------------------------------------------------------------------------
// Posición (PosAnimation.ocx, versión 4)
// ---------------------------------------------------------------------------------------------------------------

/// Un eje de `PositionAnimation` (`Vertical`/`Horizontal`).
#[derive(Debug, Clone, PartialEq)]
pub struct PosAxis {
    /// `PosVerticanEnable` / `PosHorizontalEnable` (XML: hay elemento `Vertical`/`Horizontal`).
    pub enable: bool,
    /// XML `VerticalPositionControl`.
    pub control: Expr,
    /// XML `VerticalValueRangeFromExpr`.
    pub value_from: Expr,
    /// XML `VerticalValueRangeToExpr`.
    pub value_to: Expr,
    /// XML `VerticalPositionRangeStartExpr` (desplazamiento en px).
    pub pos_start: Expr,
    /// XML `VerticalPositionRangeEndExpr`.
    pub pos_end: Expr,
}

impl PosAxis {
    /// Eje desactivado, como lo deja Vijeo (control vacío, rangos 0..100 → 0..100).
    pub fn off() -> Self {
        PosAxis {
            enable: false,
            control: Expr::unused_value(),
            value_from: Expr::number_unbuilt(0.0),
            value_to: Expr::number_unbuilt(100.0),
            pos_start: Expr::number_unbuilt(0.0),
            pos_end: Expr::number_unbuilt(100.0),
        }
    }

    /// La variable `var` en [`v_from`, `v_to`] desplaza el objeto de `p_start` a `p_end` px.
    pub fn by_var(var: Expr, v_from: f64, v_to: f64, p_start: f64, p_end: f64) -> Self {
        PosAxis {
            enable: true,
            control: var,
            value_from: Expr::number(v_from),
            value_to: Expr::number(v_to),
            pos_start: Expr::number(p_start),
            pos_end: Expr::number(p_end),
        }
    }
}

/// `PositionAnimation`.
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    pub extent: Extent,
    pub vertical: PosAxis,
    pub horizontal: PosAxis,
}

impl Position {
    pub const VERSION: u32 = 4;

    fn read(r: &mut Reader) -> R<Self> {
        let extent = Extent::read(r)?;
        let v_enable = rbool(r)?;
        let v_control = Expr::read(r)?;
        let h_enable = rbool(r)?;
        let h_control = Expr::read(r)?;
        let mut e: Vec<Expr> = Vec::with_capacity(8);
        for _ in 0..8 {
            e.push(Expr::read(r)?);
        }
        let mut it = e.into_iter();
        let mut n = || it.next().unwrap();
        let vertical = PosAxis { enable: v_enable, control: v_control, value_from: n(), value_to: n(), pos_start: n(), pos_end: n() };
        let horizontal = PosAxis { enable: h_enable, control: h_control, value_from: n(), value_to: n(), pos_start: n(), pos_end: n() };
        Ok(Position { extent, vertical, horizontal })
    }

    fn write(&self, w: &mut Writer) {
        self.extent.write(w);
        let (v, h) = (&self.vertical, &self.horizontal);
        wbool(w, v.enable);
        v.control.write(w);
        wbool(w, h.enable);
        h.control.write(w);
        for e in [&v.value_from, &v.value_to, &v.pos_start, &v.pos_end, &h.value_from, &h.value_to, &h.pos_start, &h.pos_end] {
            e.write(w);
        }
    }

    pub fn new(vertical: PosAxis, horizontal: PosAxis) -> Self {
        Position { extent: Extent::DEFAULT, vertical, horizontal }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Tamaño (SizeAnimation.ocx, versión 4) — sin ejemplos en el corpus
// ---------------------------------------------------------------------------------------------------------------

/// Un eje de `SizeAnimation`.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeAxis {
    /// `SizeVerticalEnable` (XML `VerticalSizeEnabled`).
    pub enable: bool,
    /// XML `VerticalSizeControl`.
    pub control: Expr,
    /// `SizeVerticalFrom` (XML `VerticalValueRangeFromExpr`).
    pub value_from: Expr,
    /// `SizeVerticalTo` (XML `VerticalValueRangeToExpr`).
    pub value_to: Expr,
    /// `SizeVerticalMin` (XML `VerticalDisplayRangeMinExpr`, %).
    pub size_min: Expr,
    /// `SizeVerticalMax` (XML `VerticalDisplayRangeMaxExpr`, %).
    pub size_max: Expr,
    /// `SizeVerticalAnchor` (XML `VerticalAnchor`; por defecto 1).
    pub anchor: i16,
}

impl SizeAxis {
    pub fn off() -> Self {
        SizeAxis {
            enable: false,
            control: Expr::unused_value(),
            value_from: Expr::number_unbuilt(0.0),
            value_to: Expr::number_unbuilt(100.0),
            size_min: Expr::number_unbuilt(0.0),
            size_max: Expr::number_unbuilt(100.0),
            anchor: 1,
        }
    }

    /// La variable en [`v_from`, `v_to`] lleva el tamaño de `pct_min` a `pct_max` % del original.
    pub fn by_var(var: Expr, v_from: f64, v_to: f64, pct_min: f64, pct_max: f64, anchor: i16) -> Self {
        SizeAxis {
            enable: true,
            control: var,
            value_from: Expr::number(v_from),
            value_to: Expr::number(v_to),
            size_min: Expr::number(pct_min),
            size_max: Expr::number(pct_max),
            anchor,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Size {
    pub extent: Extent,
    pub vertical: SizeAxis,
    pub horizontal: SizeAxis,
}

impl Size {
    pub const VERSION: u32 = 4;

    fn read(r: &mut Reader) -> R<Self> {
        let extent = Extent::read(r)?;
        let v_enable = rbool(r)?;
        let h_enable = rbool(r)?;
        let v_control = Expr::read(r)?;
        let h_control = Expr::read(r)?;
        let v_from = Expr::read(r)?;
        let v_to = Expr::read(r)?;
        let v_max = Expr::read(r)?;
        let v_min = Expr::read(r)?;
        let v_anchor = r.i16()?;
        let h_from = Expr::read(r)?;
        let h_to = Expr::read(r)?;
        let h_max = Expr::read(r)?;
        let h_min = Expr::read(r)?;
        let h_anchor = r.i16()?;
        Ok(Size {
            extent,
            vertical: SizeAxis { enable: v_enable, control: v_control, value_from: v_from, value_to: v_to, size_min: v_min, size_max: v_max, anchor: v_anchor },
            horizontal: SizeAxis { enable: h_enable, control: h_control, value_from: h_from, value_to: h_to, size_min: h_min, size_max: h_max, anchor: h_anchor },
        })
    }

    fn write(&self, w: &mut Writer) {
        let (v, h) = (&self.vertical, &self.horizontal);
        self.extent.write(w);
        wbool(w, v.enable);
        wbool(w, h.enable);
        v.control.write(w);
        h.control.write(w);
        v.value_from.write(w);
        v.value_to.write(w);
        v.size_max.write(w);
        v.size_min.write(w);
        w.i16(v.anchor);
        h.value_from.write(w);
        h.value_to.write(w);
        h.size_max.write(w);
        h.size_min.write(w);
        w.i16(h.anchor);
    }

    pub fn new(vertical: SizeAxis, horizontal: SizeAxis) -> Self {
        Size { extent: Extent::DEFAULT, vertical, horizontal }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Relleno (FillAnimation.ocx, versión 5; sin extensión OLE)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct FillAxis {
    /// `FillVerticalEnabled` (XML `VerticalFillEnabled`).
    pub enable: bool,
    /// XML `VerticalFillControl`.
    pub control: Expr,
    /// `FillVerticalAnchor` (XML `VerticalAnchor`; por defecto 3 en vertical y 0 en horizontal).
    pub anchor: i16,
    /// `FillVerticalMinPercent` (XML `VerticalDisplayRangeMinExpr`).
    pub fill_min: Expr,
    /// `FillVerticalMaxPercent` (XML `VerticalDisplayRangeMaxExpr`).
    pub fill_max: Expr,
    /// `FillVerticalFromRange` (XML `VerticalValueRangeFromExpr`).
    pub value_from: Expr,
    /// `FillVerticalToRange` (XML `VerticalValueRangeToExpr`).
    pub value_to: Expr,
}

impl FillAxis {
    fn read(r: &mut Reader) -> R<Self> {
        Ok(FillAxis {
            enable: rbool(r)?,
            control: Expr::read(r)?,
            anchor: r.i16()?,
            fill_min: Expr::read(r)?,
            fill_max: Expr::read(r)?,
            value_from: Expr::read(r)?,
            value_to: Expr::read(r)?,
        })
    }

    fn write(&self, w: &mut Writer) {
        wbool(w, self.enable);
        self.control.write(w);
        w.i16(self.anchor);
        self.fill_min.write(w);
        self.fill_max.write(w);
        self.value_from.write(w);
        self.value_to.write(w);
    }

    pub fn off(anchor: i16) -> Self {
        FillAxis {
            enable: false,
            control: Expr::unused_value(),
            anchor,
            fill_min: Expr::number_unbuilt(0.0),
            fill_max: Expr::number_unbuilt(100.0),
            value_from: Expr::number_unbuilt(0.0),
            value_to: Expr::number_unbuilt(100.0),
        }
    }

    pub fn by_var(var: Expr, v_from: f64, v_to: f64, pct_min: f64, pct_max: f64, anchor: i16) -> Self {
        FillAxis {
            enable: true,
            control: var,
            anchor,
            fill_min: Expr::number(pct_min),
            fill_max: Expr::number(pct_max),
            value_from: Expr::number(v_from),
            value_to: Expr::number(v_to),
        }
    }
}

/// `FillAnimation`.
#[derive(Debug, Clone, PartialEq)]
pub struct Fill {
    pub vertical: FillAxis,
    pub horizontal: FillAxis,
    /// `FillPpgForeColor` = XML `BackAreaColor` (color de la zona sin rellenar; COLORREF 0x00BBGGRR, el bit
    /// 0x08000000 = transparente: `0800FF00` en el corpus).
    pub area_color: u32,
    /// `FillPpgBackColor` (sin atributo en el XML; 0x0000FF00 en el corpus).
    pub ppg_back_color: u32,
    /// `FillBackAreaPattern` (1 en el corpus).
    pub area_pattern: u32,
    /// `Fillforeblk`.
    pub fore_blink: i16,
    /// `Fillbackblk`.
    pub back_blink: i16,
}

impl Fill {
    pub const VERSION: u32 = 5;
    /// Bit de transparencia de los colores de Vijeo (`08` delante del color en el XML).
    pub const TRANSPARENT: u32 = 0x0800_0000;

    fn read(r: &mut Reader) -> R<Self> {
        Ok(Fill {
            vertical: FillAxis::read(r)?,
            horizontal: FillAxis::read(r)?,
            area_color: r.u32()?,
            ppg_back_color: r.u32()?,
            area_pattern: r.u32()?,
            fore_blink: r.i16()?,
            back_blink: r.i16()?,
        })
    }

    fn write(&self, w: &mut Writer) {
        self.vertical.write(w);
        self.horizontal.write(w);
        w.u32(self.area_color);
        w.u32(self.ppg_back_color);
        w.u32(self.area_pattern);
        w.i16(self.fore_blink);
        w.i16(self.back_blink);
    }

    /// Relleno vertical (desde abajo) o horizontal (desde la izquierda) por una variable: `v_from..v_to` llena
    /// del 0 al 100 %. Zona sin rellenar transparente, como en el corpus (`Rectangle13`, `Rectangle18`).
    pub fn by_var(var: Expr, vertical: bool, v_from: f64, v_to: f64) -> Self {
        let on = |anchor| FillAxis::by_var(var.clone(), v_from, v_to, 0.0, 100.0, anchor);
        Fill {
            vertical: if vertical { on(3) } else { FillAxis::off(3) },
            horizontal: if vertical { FillAxis::off(0) } else { on(0) },
            area_color: Fill::TRANSPARENT | 0x00FF00,
            ppg_back_color: 0x00FF00,
            area_pattern: 1,
            fore_blink: 0,
            back_blink: 0,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Rotación (Rotate.ocx, versión 4; sin extensión OLE)
// ---------------------------------------------------------------------------------------------------------------

/// `RotateAnimation`.
#[derive(Debug, Clone, PartialEq)]
pub struct Rotate {
    /// `Rotation` (XML `RotateEnabled`).
    pub enable: bool,
    /// `Tag` (XML `RotateControl`).
    pub control: Expr,
    /// `StartAngle` (XML `AngleStartExpr`).
    pub angle_start: Expr,
    /// `EndAngle` (XML `AngleEndExpr`).
    pub angle_end: Expr,
    /// `From` (XML `ValueRangeFromExpr`).
    pub value_from: Expr,
    /// `To` (XML `ValueRangeToExpr`).
    pub value_to: Expr,
    /// `X` (XML `OffSetX`): centro de giro relativo.
    pub x: f64,
    /// `Y` (XML `OffSetY`).
    pub y: f64,
    /// `Angleunit`.
    pub angle_unit: i16,
}

impl Rotate {
    pub const VERSION: u32 = 4;

    fn read(r: &mut Reader) -> R<Self> {
        Ok(Rotate {
            enable: rbool(r)?,
            control: Expr::read(r)?,
            angle_start: Expr::read(r)?,
            angle_end: Expr::read(r)?,
            value_from: Expr::read(r)?,
            value_to: Expr::read(r)?,
            x: r.f64()?,
            y: r.f64()?,
            angle_unit: r.i16()?,
        })
    }

    fn write(&self, w: &mut Writer) {
        wbool(w, self.enable);
        self.control.write(w);
        self.angle_start.write(w);
        self.angle_end.write(w);
        self.value_from.write(w);
        self.value_to.write(w);
        w.f64(self.x);
        w.f64(self.y);
        w.i16(self.angle_unit);
    }

    /// La variable en [`v_from`, `v_to`] gira el objeto de `a_start` a `a_end` grados.
    pub fn by_var(var: Expr, v_from: f64, v_to: f64, a_start: f64, a_end: f64) -> Self {
        Rotate {
            enable: true,
            control: var,
            angle_start: Expr::number(a_start),
            angle_end: Expr::number(a_end),
            value_from: Expr::number(v_from),
            value_to: Expr::number(v_to),
            x: 0.0,
            y: 0.0,
            angle_unit: -1,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Valor (ValueAnimation.ocx, versión 7): texto que muestra una variable
// ---------------------------------------------------------------------------------------------------------------

/// `ValueAnimation`.
#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub extent: Extent,
    /// `ValueEnable`.
    pub enable: bool,
    /// `DataType` (XML `DataType`): 0 `Discrete` (OnLabel/OffLabel), 1 `Integer`, 2 `Real`, 3 `String`.
    pub data_type: i16,
    pub on_label: String,
    pub off_label: String,
    pub align: i16,
    /// `InputMode` (XML `InputModeEnabled`).
    pub input_mode: bool,
    pub range_from: f64,
    pub range_to: f64,
    pub range_min: f64,
    pub range_max: f64,
    /// `Format` (XML `Format`, enteros): 0 `Dec`.
    pub format: i16,
    pub display_digit: bool,
    /// `Integer` (XML `IntegerDigitRange`).
    pub integer: i16,
    /// `Fraction` (XML `DecimalDigitRange`).
    pub fraction: i16,
    /// `ZeroSuppress` (XML `ZeroSuppressCheck`).
    pub zero_suppress: bool,
    pub round_off: bool,
    pub trigger_redraw: i16,
    pub clear_mode: bool,
    pub barcode_input: bool,
    pub check_digit: bool,
    pub display_length_check: bool,
    /// `DisplayLength` (XML `DisplayLength`, cadenas).
    pub display_length: i16,
    /// `ShowHideKeypad` (XML `PopupKeypadEnabled`).
    pub show_hide_keypad: bool,
    /// XML `ValueExpr`.
    pub value_expr: Expr,
    pub popup_keypad: String,
    /// `InputStatus` (hay `InputStatusTag`).
    pub input_status: bool,
    /// `KeypadBeep` (XML `BeepOnTouch`).
    pub keypad_beep: bool,
    /// XML `InputStatusTag` (destino de bit).
    pub input_status_tag: Expr,
    /// Tercera expresión del control (sin atributo en el XML; destino, tipo 0x10).
    pub expr4: Expr,
    /// `Interlock` (hay `InterlockTag`).
    pub interlock: bool,
    /// XML `InterlockTag` (condición).
    pub interlock_tag: Expr,
}

impl Value {
    pub const VERSION: u32 = 7;

    fn read(r: &mut Reader) -> R<Self> {
        Ok(Value {
            extent: Extent::read(r)?,
            enable: rbool(r)?,
            data_type: r.i16()?,
            on_label: r.cstring()?,
            off_label: r.cstring()?,
            align: r.i16()?,
            input_mode: rbool(r)?,
            range_from: r.f64()?,
            range_to: r.f64()?,
            range_min: r.f64()?,
            range_max: r.f64()?,
            format: r.i16()?,
            display_digit: rbool(r)?,
            integer: r.i16()?,
            fraction: r.i16()?,
            zero_suppress: rbool(r)?,
            round_off: rbool(r)?,
            trigger_redraw: r.i16()?,
            clear_mode: rbool(r)?,
            barcode_input: rbool(r)?,
            check_digit: rbool(r)?,
            display_length_check: rbool(r)?,
            display_length: r.i16()?,
            show_hide_keypad: rbool(r)?,
            value_expr: Expr::read(r)?,
            popup_keypad: r.cstring()?,
            input_status: rbool(r)?,
            keypad_beep: rbool(r)?,
            input_status_tag: Expr::read(r)?,
            expr4: Expr::read(r)?,
            interlock: rbool(r)?,
            interlock_tag: Expr::read(r)?,
        })
    }

    fn write(&self, w: &mut Writer) {
        self.extent.write(w);
        wbool(w, self.enable);
        w.i16(self.data_type);
        w.cstring(&self.on_label);
        w.cstring(&self.off_label);
        w.i16(self.align);
        wbool(w, self.input_mode);
        w.f64(self.range_from);
        w.f64(self.range_to);
        w.f64(self.range_min);
        w.f64(self.range_max);
        w.i16(self.format);
        wbool(w, self.display_digit);
        w.i16(self.integer);
        w.i16(self.fraction);
        wbool(w, self.zero_suppress);
        wbool(w, self.round_off);
        w.i16(self.trigger_redraw);
        wbool(w, self.clear_mode);
        wbool(w, self.barcode_input);
        wbool(w, self.check_digit);
        wbool(w, self.display_length_check);
        w.i16(self.display_length);
        wbool(w, self.show_hide_keypad);
        self.value_expr.write(w);
        w.cstring(&self.popup_keypad);
        wbool(w, self.input_status);
        wbool(w, self.keypad_beep);
        self.input_status_tag.write(w);
        self.expr4.write(w);
        wbool(w, self.interlock);
        self.interlock_tag.write(w);
    }

    /// Muestra el valor de una variable entera (`real` = false) o real, con `integer` cifras enteras y
    /// `fraction` decimales (sólo reales). Valores por defecto copiados del corpus (`Number`, `TextL0..4`).
    pub fn by_var(path: &str, real: bool, integer: i16, fraction: i16, zero_suppress: bool) -> Self {
        Value {
            extent: Extent::DEFAULT,
            enable: true,
            data_type: if real { 2 } else { 1 },
            on_label: "On".into(),
            off_label: "Off".into(),
            align: 0,
            input_mode: false,
            range_from: 0.0,
            range_to: 1.0,
            range_min: 0.0,
            range_max: 1.0,
            format: 0,
            display_digit: true,
            integer,
            fraction,
            zero_suppress,
            round_off: false,
            trigger_redraw: 0,
            clear_mode: false,
            barcode_input: false,
            check_digit: false,
            display_length_check: true,
            display_length: 10,
            show_hide_keypad: false,
            value_expr: Expr::var(path, if real { Use::Float } else { Use::Int }),
            popup_keypad: String::new(),
            input_status: false,
            keypad_beep: false,
            input_status_tag: Expr::unused(2, 2),
            expr4: Expr::unused(0x10, 2),
            interlock: false,
            interlock_tag: Expr::unused_cond(),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Color (ColorAnimation.ocx, versión 9)
// ---------------------------------------------------------------------------------------------------------------

/// Un estado (fila de la rejilla) de una animación de color: XML `ValueRange`.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorRow {
    /// `...GridColor%d` (COLORREF 0x00BBGGRR; el XML lo escribe en hex tal cual, p. ej. `4080FF`).
    pub color: u32,
    /// `...GridTran%d`: transparente (el XML antepone `08` al color: `08FFFFFF`).
    pub transparent: bool,
    /// `...GridOrig%d` (XML `OriginalColorChecked`).
    pub original: bool,
    /// `...GridBlink%d` (XML `Blink`, `g_blinkMap`): 0 `None`, 1 `Fast`, 2 `Mid`, 3 `Slow`.
    pub blink: i16,
    /// Valor o condición del estado (XML `ValueRangeExpr`).
    pub value: Expr,
    /// `InternalID` (XML `InternalID`): 1, 2, 3...
    pub internal_id: u32,
}

impl ColorRow {
    fn read(r: &mut Reader) -> R<Self> {
        Ok(ColorRow {
            color: r.u32()?,
            transparent: rbool(r)?,
            original: rbool(r)?,
            blink: r.i16()?,
            value: Expr::read(r)?,
            internal_id: r.u32()?,
        })
    }

    fn write(&self, w: &mut Writer) {
        w.u32(self.color);
        wbool(w, self.transparent);
        wbool(w, self.original);
        w.i16(self.blink);
        self.value.write(w);
        w.u32(self.internal_id);
    }
}

/// Color de primer plano, de fondo o de línea.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorTarget {
    /// `ForeEnable`/`BackEnable`/`LineEnable`.
    pub enable: bool,
    /// `ForeDataType`... (XML `DataType`, `g_dataTypeMap`): 1 `Discrete` (condición; Vijeo guarda como mucho
    /// 2 filas), 0 `Integer` (por valor; control dt 0x40).
    pub data_type: i16,
    /// `ForeTag`... (XML `DataExpr`).
    pub control: Expr,
    pub rows: Vec<ColorRow>,
}

fn color_row(v: f64, c: Option<u32>, id: u32) -> ColorRow {
    ColorRow {
        color: c.unwrap_or(0xFFFFFF),
        transparent: c.is_none(),
        original: false,
        blink: 0,
        value: Expr::state_value(v),
        internal_id: id,
    }
}

impl ColorTarget {
    /// Destino sin usar, como lo deja Vijeo (control vacío con `ValidBuild` 1, `Discrete`, sin filas).
    pub fn off() -> Self {
        let mut e = Expr::unused_cond();
        e.valid_build = 1;
        ColorTarget { enable: false, data_type: 1, control: e, rows: Vec::new() }
    }

    /// Color discreto por una variable booleana: `off` con 0 y `on` con 1 (COLORREF 0x00BBGGRR; `None` =
    /// transparente). Igual que `Ellipse02`/`Ellipse04` del corpus.
    pub fn discrete(path: &str, off: Option<u32>, on: Option<u32>) -> Self {
        ColorTarget { enable: true, data_type: 1, control: Expr::var(path, Use::Cond), rows: vec![color_row(0.0, off, 1), color_row(1.0, on, 2)] }
    }

    /// Color por valor de una variable entera: una fila por (valor, color). Sin ejemplos en el corpus: forma
    /// deducida de ColorAnimation.ocx (control dt 0x40, filas et 5 / dt 0x40); sin verificar en Vijeo.
    pub fn by_value(path: &str, rows: &[(f64, Option<u32>)]) -> Self {
        let rows = rows.iter().enumerate().map(|(i, (v, c))| color_row(*v, *c, i as u32 + 1)).collect();
        ColorTarget { enable: true, data_type: 0, control: Expr::var(path, Use::Int), rows }
    }
}

/// `ForeColorAnimation` / `BackColorAnimation` / `LineColorAnimation` y `StateColorAnimation`: una sola clase con
/// los tres destinos por valor y el bloque de estados con biblioteca de colores.
#[derive(Debug, Clone, PartialEq)]
pub struct Color {
    pub extent: Extent,
    pub fore: ColorTarget,
    pub back: ColorTarget,
    pub line: ColorTarget,
    /// `GridMode`.
    pub grid_mode: i16,
    /// `Enable`: `StateColorAnimation` activa (colores por estado de una biblioteca de colores).
    pub enable: bool,
    /// `CtrlType` (XML `StateColorAnimation/@DataType`, `g_stateDataTypeMap`: 0 `Integer`, 1 `Discrete`, 2 `String`).
    pub ctrl_type: i16,
    /// `CtrlTag` (XML `StateExpression`).
    pub ctrl_tag: Expr,
    /// `ColorLibraryID` (XML `ColorResourceID`).
    pub color_library_id: String,
    /// Valores de los estados (XML `Value` de cada estado; `NumOfStates` cadenas; "0".."4" por defecto).
    pub states: Vec<String>,
    /// `Invisible` (XML `StateColorAnimation/@InvisibleWhenInvalid`).
    pub invisible: bool,
    /// Sólo si `color_library_id` no está vacío: `ColorLibrarySize` y, si es > 0, `ColorLibraryData`
    /// (PX_Blob: u32 n + n bytes de `CColorLibrary::Save`).
    pub color_library: Option<(u32, Vec<u8>)>,
}

impl Color {
    pub const VERSION: u32 = 9;

    fn read(r: &mut Reader) -> R<Self> {
        let extent = Extent::read(r)?;
        let en = [rbool(r)?, rbool(r)?, rbool(r)?];
        let dt = [r.i16()?, r.i16()?, r.i16()?];
        let ctl = [Expr::read(r)?, Expr::read(r)?, Expr::read(r)?];
        let mut rows: Vec<Vec<ColorRow>> = Vec::new();
        for _ in 0..3 {
            let at = r.p;
            let n = r.i16()?;
            if !(0..=256).contains(&n) {
                return Err(format!("{n} filas de color en {at:#x}"));
            }
            let mut v = Vec::new();
            for _ in 0..n {
                v.push(ColorRow::read(r)?);
            }
            rows.push(v);
        }
        let grid_mode = r.i16()?;
        let enable = rbool(r)?;
        let ctrl_type = r.i16()?;
        let at = r.p;
        let n_states = r.i16()?;
        if !(0..=1024).contains(&n_states) {
            return Err(format!("{n_states} estados de color en {at:#x}"));
        }
        let ctrl_tag = Expr::read(r)?;
        let color_library_id = r.cstring()?;
        let mut states = Vec::new();
        for _ in 0..n_states {
            states.push(r.cstring()?);
        }
        let invisible = rbool(r)?;
        let color_library = if color_library_id.is_empty() {
            None
        } else {
            let size = r.u32()?;
            let data = if size > 0 {
                let n = r.u32()? as usize;
                r.bytes(n)?.to_vec()
            } else {
                Vec::new()
            };
            Some((size, data))
        };
        let [fe, be, le] = en;
        let [fc, bc, lc] = ctl;
        let mut rows = rows.into_iter();
        Ok(Color {
            extent,
            fore: ColorTarget { enable: fe, data_type: dt[0], control: fc, rows: rows.next().unwrap() },
            back: ColorTarget { enable: be, data_type: dt[1], control: bc, rows: rows.next().unwrap() },
            line: ColorTarget { enable: le, data_type: dt[2], control: lc, rows: rows.next().unwrap() },
            grid_mode,
            enable,
            ctrl_type,
            ctrl_tag,
            color_library_id,
            states,
            invisible,
            color_library,
        })
    }

    fn write(&self, w: &mut Writer) {
        self.extent.write(w);
        let t = [&self.fore, &self.back, &self.line];
        for x in t {
            wbool(w, x.enable);
        }
        for x in t {
            w.i16(x.data_type);
        }
        for x in t {
            x.control.write(w);
        }
        for x in t {
            w.i16(x.rows.len() as i16);
            for row in &x.rows {
                row.write(w);
            }
        }
        w.i16(self.grid_mode);
        wbool(w, self.enable);
        w.i16(self.ctrl_type);
        w.i16(self.states.len() as i16);
        self.ctrl_tag.write(w);
        w.cstring(&self.color_library_id);
        for s in &self.states {
            w.cstring(s);
        }
        wbool(w, self.invisible);
        if !self.color_library_id.is_empty() {
            let (size, data) = self.color_library.clone().unwrap_or((0, Vec::new()));
            w.u32(size);
            if size > 0 {
                w.u32(data.len() as u32);
                w.bytes(&data);
            }
        }
    }

    /// Animación de color con sólo el primer plano (`fore`), el fondo (`back`) o la línea (`line`) activos;
    /// el resto como lo deja Vijeo. Bloque de estados copiado del corpus (`Ellipse02`).
    pub fn new(fore: Option<ColorTarget>, back: Option<ColorTarget>, line: Option<ColorTarget>) -> Self {
        let mut ctrl = Expr::unused(0x40, 4);
        ctrl.last_validated_type = 0x40;
        Color {
            extent: Extent::DEFAULT,
            fore: fore.unwrap_or_else(ColorTarget::off),
            back: back.unwrap_or_else(ColorTarget::off),
            line: line.unwrap_or_else(ColorTarget::off),
            grid_mode: 0,
            enable: false,
            ctrl_type: 0,
            ctrl_tag: ctrl,
            color_library_id: String::new(),
            states: (0..5).map(|i| i.to_string()).collect(),
            invisible: false,
            color_library: None,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Animación (typed o en bruto) y extensión completa
// ---------------------------------------------------------------------------------------------------------------

/// Una animación de un objeto. Las clases sin tipar (Touch, SmartObject, versiones antiguas, propiedades de serie
/// no vacías...) se conservan en bruto con su GUID.
#[derive(Debug, Clone, PartialEq)]
pub enum Animation {
    Color(Color),
    Fill(Fill),
    Size(Size),
    Position(Position),
    Rotate(Rotate),
    Value(Value),
    Visibility(Visibility),
    SmartObject(SmartObjectAnim),
    Raw { guid: Guid, data: Vec<u8> },
}

impl Animation {
    pub fn kind(&self) -> Option<Kind> {
        Some(match self {
            Animation::Color(_) => Kind::Color,
            Animation::Fill(_) => Kind::Fill,
            Animation::Size(_) => Kind::Size,
            Animation::Position(_) => Kind::Position,
            Animation::Rotate(_) => Kind::Rotate,
            Animation::Value(_) => Kind::Value,
            Animation::Visibility(_) => Kind::Visibility,
            Animation::SmartObject(_) => Kind::SmartObject,
            Animation::Raw { guid, .. } => return Kind::from_guid(guid),
        })
    }

    pub fn guid(&self) -> Guid {
        match self {
            Animation::Raw { guid, .. } => *guid,
            a => a.kind().and_then(Kind::guid).unwrap(),
        }
    }

    pub fn is_raw(&self) -> bool {
        matches!(self, Animation::Raw { .. })
    }

    /// Datos de la animación (lo que va tras el GUID).
    pub fn data(&self) -> Vec<u8> {
        let mut w = Writer::new();
        if let Some(v) = (!self.is_raw()).then(|| self.kind().and_then(Kind::version)).flatten() {
            w.u32(v);
        }
        match self {
            Animation::Color(a) => a.write(&mut w),
            Animation::Fill(a) => a.write(&mut w),
            Animation::Size(a) => a.write(&mut w),
            Animation::Position(a) => a.write(&mut w),
            Animation::Rotate(a) => a.write(&mut w),
            Animation::Value(a) => a.write(&mut w),
            Animation::Visibility(a) => a.write(&mut w),
            Animation::SmartObject(a) => a.write(&mut w),
            Animation::Raw { data, .. } => w.bytes(data),
        }
        w.o
    }

    /// Tipa los datos de una animación si se conoce su clase y versión y se consumen exactos; si no, en bruto.
    pub fn from_data(guid: Guid, data: &[u8]) -> Animation {
        Animation::typed(&guid, data).unwrap_or_else(|_| Animation::Raw { guid, data: data.to_vec() })
    }

    /// Como `from_data`, pero devuelve el motivo si no se puede tipar.
    pub fn typed(guid: &Guid, data: &[u8]) -> R<Animation> {
        let kind = Kind::from_guid(guid).ok_or_else(|| format!("clase desconocida {}", guid_str(guid)))?;
        let want = kind.version().ok_or_else(|| format!("{kind:?} sin tipar"))?;
        let mut r = Reader::new(data);
        let v = r.u32()?;
        if v != want {
            return Err(format!("{kind:?} versión {v} (se tipa la {want})"));
        }
        let a = match kind {
            Kind::Color => Animation::Color(Color::read(&mut r)?),
            Kind::Fill => Animation::Fill(Fill::read(&mut r)?),
            Kind::Size => Animation::Size(Size::read(&mut r)?),
            Kind::Position => Animation::Position(Position::read(&mut r)?),
            Kind::Rotate => Animation::Rotate(Rotate::read(&mut r)?),
            Kind::Value => Animation::Value(Value::read(&mut r)?),
            Kind::Visibility => Animation::Visibility(Visibility::read(&mut r)?),
            Kind::SmartObject => Animation::SmartObject(SmartObjectAnim::read(&mut r)?),
            _ => unreachable!(),
        };
        if r.left() != 0 {
            return Err(format!("{kind:?}: sobran {} B en {:#x}", r.left(), r.p));
        }
        Ok(a)
    }
}

/// Lee la extensión completa (desde el u32 3 hasta el u32 0 final, incluidos).
pub fn read_ext(r: &mut Reader) -> R<Vec<Animation>> {
    r.expect_u32(MANAGER_VERSION, "versión del gestor de animaciones")?;
    let mut out = Vec::new();
    loop {
        let at = r.p;
        let n = r.u32()? as usize;
        if n == 0 {
            break;
        }
        if n > r.left() {
            return Err(format!("animación de {n} B en {at:#x} (quedan {})", r.left()));
        }
        let g: Guid = r.bytes(16)?.try_into().unwrap();
        let data = r.bytes(n)?;
        out.push(Animation::from_data(g, data));
    }
    Ok(out)
}

/// Extensión completa; tiene que consumirse entera.
pub fn parse_ext(b: &[u8]) -> R<Vec<Animation>> {
    let mut r = Reader::new(b);
    let v = read_ext(&mut r)?;
    if r.left() != 0 {
        return Err(format!("sobran {} B tras la extensión", r.left()));
    }
    Ok(v)
}

/// Escribe la extensión. Ordena por ranura (como el gestor) y rechaza dos animaciones de la misma clase.
pub fn write_ext(anims: &[Animation]) -> R<Vec<u8>> {
    let mut v: Vec<&Animation> = anims.iter().collect();
    v.sort_by_key(|a| a.kind().map(Kind::slot).unwrap_or(usize::MAX));
    for p in v.windows(2) {
        if p[0].guid() == p[1].guid() {
            return Err(format!("dos animaciones de la clase {}", guid_str(&p[0].guid())));
        }
    }
    let mut w = Writer::new();
    w.u32(MANAGER_VERSION);
    for a in v {
        let d = a.data();
        w.u32(d.len() as u32);
        w.bytes(&a.guid());
        w.bytes(&d);
    }
    w.u32(0);
    Ok(w.o)
}

/// Pone (o sustituye) la animación de su clase en la lista.
pub fn set(anims: &mut Vec<Animation>, a: Animation) {
    let g = a.guid();
    anims.retain(|x| x.guid() != g);
    anims.push(a);
    anims.sort_by_key(|a| a.kind().map(Kind::slot).unwrap_or(usize::MAX));
}

/// Quita la animación de una clase; devuelve si había.
pub fn remove(anims: &mut Vec<Animation>, k: Kind) -> bool {
    let n = anims.len();
    anims.retain(|x| x.kind() != Some(k));
    anims.len() != n
}

// ---------------------------------------------------------------------------------------------------------------
// Sobre los bytes de un objeto (GUID de clase + cabecera común + datos de clase)
// ---------------------------------------------------------------------------------------------------------------

/// Posiciones de la extensión dentro de un objeto: byte de flags, inicio y fin de la extensión (la marca
/// 0x1CED50DA va en `end`). Sin el bit 0, `start == end`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtSpan {
    pub flags_at: usize,
    pub start: usize,
    pub end: usize,
}

/// Localiza la extensión: GUID (16) · u8 3 · u32 id · 4×i16 · nombre (u32 bytes + UTF-16 con NUL) · u8 flags.
pub fn ext_span(obj: &[u8]) -> R<ExtSpan> {
    let mut r = Reader::at(obj, 16);
    let v = r.u8()?;
    if v != 3 {
        return Err(format!("cabecera común versión {v}"));
    }
    r.bytes(4 + 8)?;
    let n = r.u32()? as usize;
    r.bytes(n)?;
    let flags_at = r.p;
    let flags = r.u8()?;
    let start = r.p;
    if flags & 1 != 0 {
        read_ext(&mut r)?;
    }
    let end = r.p;
    let m = r.u32()?;
    if m != MARKER {
        return Err(format!("sin marca 0x1CED50DA en {end:#x} (hay {m:#x})"));
    }
    Ok(ExtSpan { flags_at, start, end })
}

/// Animaciones de un objeto.
pub fn get_animations(obj: &[u8]) -> R<Vec<Animation>> {
    let s = ext_span(obj)?;
    if s.start == s.end {
        return Ok(Vec::new());
    }
    parse_ext(&obj[s.start..s.end])
}

/// Devuelve el objeto con estas animaciones (lista vacía = extensión vacía `03 00 00 00 00 00 00 00`, como Vijeo).
/// Si el objeto contiene hijos con tamaño (grupos), el tamaño de éste lo gestiona quien lo contenga.
pub fn set_animations(obj: &[u8], anims: &[Animation]) -> R<Vec<u8>> {
    let s = ext_span(obj)?;
    let ext = write_ext(anims)?;
    let mut o = Vec::with_capacity(obj.len() + ext.len());
    o.extend_from_slice(&obj[..s.flags_at]);
    o.push(obj[s.flags_at] | 1);
    o.extend_from_slice(&ext);
    o.extend_from_slice(&obj[s.end..]);
    Ok(o)
}
