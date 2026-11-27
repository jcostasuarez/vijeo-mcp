# Switch (botón) v6 y bloques compartidos de HMIManagers — formato del portapapeles de Vijeo-Designer 6.3

Estado (2026-11-27):

- **Ida y vuelta**: los 65 Switch del corpus (`S\clips\*.bin`, todos v6 con flags de guardado 9) se leen y se
  vuelven a escribir **byte a byte iguales (65/65)**: `probe <S>\clips swrt`.
- **Generación desde cero**: `Switch::button` reproduce byte a byte 4 botones reales (ButtonMan01, Page_ATV12,
  EnableGreen, MoreParameters) copiando sólo lo que depende del proyecto de origen (ids dentro de las referencias,
  id de cadena, InternalID, u32 «reserved», nº de imágenes por estado) y comparando las tablas de colores en orden
  canónico: `probe <S>\clips swgen` (incluye una autoprueba con todas las operaciones tipadas).
- Código (sólo std + `bin.rs`): `S\wA\probe\src\{bin,expr,action,hmi,switch}.rs`. Órdenes de prueba en
  `swtool.rs` (`swrt`, `swgen`, `swlist`, `swshow <clip> <nombre>`, `swstats`). Compilar con
  `CARGO_TARGET_DIR=S/wA/target cargo build` en `S\wA\probe`.
- Pseudocódigo consultado (no copiado aquí): `S\gh\*.c` y `S\wA\gh\*.c` (hmi1/hmi2/hmi3, widgets, actlist, npart, sw_xml).

Convenciones: todo little-endian; `BSTR` = u32 bytes (con NUL) + UTF-16 + NUL (vacío = `02 00 00 00 00 00`;
en el corpus no aparece ningún BSTR nulo de longitud 0); `CString` = `FF FE FF` + u8 n + UTF-16 sin NUL;
`save_flags` = 9 en el portapapeles (bit 0 = incrustar copias de bibliotecas, imágenes y la parte nativa).
`+0x…` son desplazamientos del miembro en el objeto C++ (para buscar en el pseudocódigo).

## 1. Objeto Switch completo

Cabecera común (BaseObject, la maneja ya el MCP): GUID de clase `{347CCED5-5E26-497A-A140-EA017D0272BA}`
(`d5 ce 7c 34 26 5e 7a 49 a1 40 ea 01 7d 02 72 ba`), u8 3, u32 id, rect l,t,r,b inclusivo, nombre, u8 flags = 1,
extensión vacía `03 00 00 00 00 00 00 00` (las animaciones de visibilidad del Switch van en los datos de clase, no en
la extensión), marca `0x1CED50DA`. Después, los datos de clase:

| # | Tipo | Condición | Campo (`switch.rs`) | Miembro | XML |
|---|------|-----------|---------------------|---------|-----|
| 1 | u32 = 6 | | `version` | | |
| 2 | u32 | | `save_flags` (9) | | |
| 3 | u16 máscara | | `mask` | | ver tabla de bits |
| 4 | CExpression | bit 0 | `lamp` | +0x240 | `LampControl`; `Style Mode="WithLamp"` |
| 5 | CExpression | bit 1 | `monitor` | +0x244 | `MonitorEnabled` / `MonitorTag` |
| 6 | CExpression | bit 3 | `interlock` | +0x248 | `InterlockEnabled` / `InterlockControl` |
| 7 | CExpression | bit 4 | `visibility` | +0x250 | `VisibilityAnimation VisibilityControl` |
| 8 | CExpression + u16 | bit 5 | `blink` | +0x24C, +0x1B8 | `BlinkControl`, `BlinkSpeed` |
| 9 | u16, u16, CExpression | bit 6 | `radio` | +0x1C8, +0x1D0, +0x254 | `RadioGroupID`, `RadioDelay`, `RadioTag` |
| 10 | u16 | siempre | `hold_delay` (1) | +0x1E4 | `HoldDelayValue` |
| 11 | BSTR | siempre | `security_group` ("") | +0x1F0 | `SecurityGroup` |
| 12 | u32 n + n CNativeAction | | `on_touch` | +0x258 | `<WhenTouch>` |
| 13 | ídem | | `on_release` | +0x2A0 | `<WhenRelease>` |
| 14 | ídem | | `while_pressed` | +0x2E8 | `<WhileTouch>` |
| 15 | bloque de color (§4) | | `colors` | +0xF0 | `<Colors>` |
| 16 | CLabelStorage (§5) | | `labels` | +0x330 | `<Labels>` |
| 17 | CImageStorage (§6) | | `images` | +0x114 | imágenes por estado (categoría Bitmap) |
| 18 | u16 + CImageInfo | | `use_overlay_on`, `overlay_on` | +0x3A0, +0x3A4 | `UseOverlayOn`, `<OverlayImage StateName="On">` |
| 19 | u16 + CImageInfo | | `use_overlay_off`, `overlay_off` | +0x38A, +0x38C | `UseOverlayOff`, `<OverlayImage StateName="Off">` |
| 20 | u16 | | `overlay_trans` | +0x3B8 | `IsOverlayTrans` |
| 21 | u32 | | `overlay_trans_color` (0x7B08FF) | +0x3BC | `OverlayTransColor` |
| 22 | CNativePart (§7) si save_flags & 1; si no, BSTR nombre del estilo | | `style` | +0x60/+0x90 del contenedor | `Style ObjectStyle` |

Bits de la máscara (Switch.dll FUN_6189fb20; nombres sacados de sus SaveXML):

| bit | significado | datos | corpus |
|-----|-------------|-------|--------|
| 0 | hay `LampControl` | expr #4 | 37 |
| 1 | `MonitorEnabled` | expr #5 | 0 |
| 2 | `ReverseOnTouch` | — | 0 (siempre false) |
| 3 | `InterlockEnabled` | expr #6 | 0 en los clips (sí hay 6 en los XML) |
| 4 | `VisibilityEnabled` | expr #7 | 14 |
| 5 | `BlinkEnabled` | expr + u16 #8 | 0 |
| 6 | botón de radio | #9 | 0 |
| 7 | `RadioDelayCheck` | — | 0 |
| 8 | `BuzzerOnTouchEnabled` | — | 13 |
| 9 | `HoldDelayEnabled` | — (el valor #10 va siempre) | 0 |

Máscaras del corpus: 0x0000 (24), 0x0001 (14), 0x0011 (13), 0x0101 (10), 0x0100 (3), 0x0010 (1). Los bits 1, 2, 3,
5, 6, 7 y 9 no tienen ejemplos: su posición sale del código de Save, su significado de las funciones SaveXML.

## 2. CExpression (PFUtility `CExpression::Save`) — `expr.rs`

`u32 0x3F3; BSTR raw; BSTR compiled; BSTR compiled2; u16 kind (0); u32 l24; u32 l28; u32 l2c; 7 × BSTR listas;
u32 l30; u8 flags; u32 n + n × (BSTR, BSTR) placeholders`.

- `raw`: `{F,2:2.<id>,Carpeta}.{T,2:2.<id>,Var}` (con `$…$` alrededor del prefijo si viene de un placeholder);
  los ids pueden ir a 0 (`raw_ref`).
- Atributos por uso (l24, l28, l2c, l30), comprobados en el corpus: condición booleana (lámpara, visibilidad,
  enclavamiento) `(TagDB.X.getIntValue(0) != 0)` = (0x20, 8, 2, 0x20); destino de bit `TagDB.X` = (2, 2, 2, 2);
  destino de palabra = (0x40C, 2, 2, 4); valor entero = (0x40, 0x1005, 2, 0x40); real = (0x80, 0x1005, 2, 0x80);
  constante (raw = compiled = «1000», compiled2 = expresión del destino, listas vacías) = (0x40, 5, 1, 0x40).
  Las expresiones con operadores (`a||b`, `x<2`) llevan l2c = 8.
- `flags` = 6 en las referencias simples; con placeholders aparecen 0x36, 0x76, 0xA6, 0xE6, 0xF6 (no descifrado:
  se conserva).
- Constructores: `Expr::cond`, `bit_dest`, `word_dest`, `int_value`, `float_value`, `constant(valor, contexto)`.

## 3. CNativeAction (HMIManagers `CNativeAction::Save/Load`) — `action.rs`

`u32 versión (7); u32 «reserved»; u32 tipo; cuerpo según tipo; u32 InternalID`.

- «reserved»: Load lo lee y lo descarta. En el corpus vale 0 (50) o 1 (19); para generar, 0.
- InternalID = atributo `InternalID` del XML: numeración de las acciones dentro del objeto (en el corpus empieza
  en 1, 2, 3, 4 o 9 según el historial del objeto). Para generar: 1, 2, 3… en el orden de las listas.
- Cuerpo según el tipo (tabla de `CNativeAction::Load`; `layout()` en el código):
  - 1 expresión (destino): bit `BitSet` 0, `Reset` 1, `MomentaryOn` 2, `MomentaryOff` 3, `Toggle` 4; 100,
    0x70–0x7C, 0x81–0x86, 0x88, 0x8A–0x8C, 0xEC.
  - 2 expresiones (destino, valor): `WordSet` 10, `WordAdd` 11, `Subtract` 12, `AND` 13, `OR` 14, `XOR` 15,
    `Overwrite` 0x6E, `StringAdd` 0x6F.
  - nada: `PreviousPanel` 0x14, 0x3E, 0x50–0x57 (`RestartRuntime` 80, `ExitRuntime` 81, `Configuration` 82,
    `EventLog` 83, `LoginScreen` 84, `LogOut` 85, `LockInput` 86, `UnlockInput` 87), 0xE6, 0xE7, 0xED, 0xEE, 0xFA, 0xFB.
  - 1 BSTR: `ChangePanel` 0x15 (número del panel: «11» = P11), `Close` (popup) 0x22, 0x1E, 0x23, 0x28–0x2D,
    0x2F–0x37 (cursor, alarmas…), 0x3C, 0x3D (sonido), 0x46, 0x47 (idioma), 0xC8–0xD1, 0xDC–0xE3, 0xEB, 0xF2–0xF4, 0xF6.
    0xE0 lleva además una expresión delante.
  - 1 expresión + BSTR: 0x1F, 0x2E, 0xF5, 0xF9. 2 expresiones + BSTR: `Open` (popup) 0x20, 0xEF, 0xF0, 0xF1.
    3 expresiones + BSTR: 0x21, 0xF8, 0xFC. 2 BSTR: script 0x5A.
  - `Decision` 0xF7: expresión (condición) + 2 CActionList (`u8 n` + n CNativeAction): lista «si» y lista «si no»
    (`CActionList::Save`). Hay uno en el corpus (LXM28 ChangeView) y sale igual.
- Tipado: `Op::{BitSet, BitReset, BitToggle, BitMomentaryOn, BitMomentaryOff, WordSet, WordAdd, WordSubtract,
  ChangePanel, PreviousPanel, PopupOpen, PopupClose, Decision}`; cualquier otro tipo de la tabla queda en
  `Op::Other { kind, exprs, strings }` sin perder bytes. Tipos fuera de la tabla → error.
- Tipos del corpus: MomentaryOn 0x2 (41), ChangePanel 0x15 (22), Toggle 0x4 (4), Configuration 0x52 (1),
  Decision 0xF7 (1; dentro: WordAdd y WordSet con constante).

## 4. Bloque de color (Switch.dll FUN_6189bbb0) y CColorLibrary — `hmi.rs`

Bloque: `u8 2; BSTR biblioteca del proyecto; u8 hay copia; [si hay copia: u8 flag_c; u8 flag_d; CColorLibrary]`.

- `resource` = XML `Colors ColorResourceID` ("" si el objeto tiene colores propios, `<Colors><State …/>`).
- `flag_c` = 1 exactamente cuando `resource` ≠ "" (53/65), `flag_d` = 0 siempre. Con save_flags 9 la copia está
  siempre (65/65).

`CColorLibrary::Save`: `u32 5; BSTR nombre (= resource, o ""); u32 v28 (16); u32 n + n filas; u32 m + m filas de
estado (id 8; 0 en los Switch)`.

Fila (`CResourceManager::SaveRow`): `u32 fila; u32 n; n × (u32 columna, widget)`. El widget lo decide el byte alto
de la columna (`CResourceManager::CreateWidget`): 0x1xx `CColorWidget` u32 COLORREF; 0x2xx `CBlinkWidget` u8;
0x3xx `CDynamicWidget` BSTR; 0x4xx `CLabelWidget` u8 + (u32 id de cadena | CLanguageDataList); 0x5xx
`CFontIDWidget` BSTR + u8 + [CFontLibrary]; 0x6xx `CImageWidget` u8 + (BSTR fichero + [u32 n + n B] | BSTR
biblioteca + u32); 0x7xx `CPatternWidget` u8.

Filas y columnas del Switch (enum del XML volcado de HMIManagers: OnUp 9, OffUp 10, OnDown 11, OffDown 12,
State 8):

| fila | XML `StateType` | | columna | XML | widget |
|------|-----------------|-|---------|-----|--------|
| 9 | OnUp (lámpara encendida) | | 0x700 | `Pattern` | u8 (1) |
| 10 | OffUp (apagada) | | 0x109 | `BackColor` | COLORREF |
| | | | 0x108 | `ForeColor` | COLORREF |
| | | | 0x104 | `FrameColor` | COLORREF |
| | | | 0x101 | `Color3D` | COLORREF |
| | | | 0x100 | `TextColor` | COLORREF |

COLORREF = 0x00BBGGRR, y el XML escribe ese número en hexadecimal (`FF00` verde, `696462` gris del texto). La
correspondencia de columnas se comprobó con el orden de atributos del XML, que es el mismo que el de las celdas.

**Orden de filas y celdas**: es el de iteración de un `CMap` de MFC. Las claves 9/10 y 0x100–0x109 caen en el mismo
cubo del hash (clave >> 4), así que el orden es el inverso de la inserción y se invierte en cada carga/guardado: en el
corpus salen 9,10 (42) o 10,9 (23) y celdas 700,109,108,104,101,100 (84) o 700,100,101,104,108,109 (46); 0x700
siempre primero (otro cubo). Es una deducción coherente con todo el corpus; para Vijeo el orden no importa (se
vuelve a meter en un mapa). El código conserva el orden leído y genera el más común.

## 5. Etiquetas: CLabelStorage, CLabelInfoBase, CLanguageDataList, CLanguageFont — `hmi.rs`

`CLabelStorage::Save`: `u8 2; u8 tipo (+4); u8 numérica (+8 bit 0); u32 HAlign (+0x30); u32 VAlign (+0x34);
[si numérica: ref. a CFontLibrary]; i16 n + n CLabelInfoBase; [tipos 3 y 4: ref. a CTextLibrary];
[tipos 1 y 2: ref. a CFontLibrary]`.

- Tipo (+4): 0 sin etiqueta, 1 Auto, 2 Floating, 3 UserDefined, 4 On/Off, 5 estática. Corpus: 5 (59), 0 (6).
- HAlign: Left 0, Center 1, Right 2; VAlign: Top 0, Middle 1, Bottom 2 (tablas de HMIManagers). Corpus: 1/1 siempre
  (XML `Labels HAlign="Center" VAlign="Middle"`); también con tipo 0.
- «Ref. a biblioteca»: `u8 hay nombre; [BSTR nombre; si save_flags & 1: u8 hay copia; [copia]]`. No aparece en el
  corpus (tipo 5 no la lleva).

`CLabelInfoBase::Save`: `u8 1; u32 id de cadena; [si id ≠ 0xFFFFFFFF y save_flags & 1: CLanguageDataList]`.
El id es el de la cadena en el gestor de idiomas del proyecto de origen (243, 315…). `CLabelInfoBase::Load`, al leer
la lista, pone el id a -1 y después `SaveAllToLangMgr` crea una cadena nueva: el valor sólo tiene que ser ≠ -1
(el generador pone 0). Deducido del código; no comprobado pegando en Vijeo.

`CLanguageDataList::Save`: `u32 3; u32 n; n × { BSTR idioma («Language1» = L1); BSTR texto (XML
`LanguageString`, con `\n` si hay salto de línea); CLanguageFont base; u32 hay fuente propia (0/1);
[CLanguageFont propia]; u32 (1); u32 (0) }`.

`CLanguageFont::Save`: `u32 4; u32 FontType (+0: 0 GP, 1 TrueType, 2 PSP/Agfa); u32 FontStyle (+4: Normal 0,
Bold 1, 3D 2); i32 altura TT (+0xC); i32 anchura/peso TT (+0x10); BSTR cara TT (+0x14); i32 FontHeightOrSize GP
(+0x18); i32 FontWidth GP (+0x1C); u32 FontFace GP (+0x20: GPFontModern6x10 = 9, GPFontModern8x13 = 10, 8x13F 11,
13x23 12, …); u8 (+8, 0); i32 tamaño PSP en píxeles (+0x24); u32 cara PSP (+0x28: 4 = AgfaFontLatin)`.

- La fuente que exporta el XML (`<Font FontType="GP" …>`) es la **propia** (segunda). La base es siempre
  GP 8x13 normal + Arial -19/400 + PSP -19/4 (59/59).
- Fuentes propias del corpus: GPFontModern6x10 normal (55; TT «Lucida Console» -8/400, PSP -8/4) y
  GPFontModern8x13 negrita (4; TT «Arial» -19/700, PSP -19/4). `LanguageFont::gp(cara, ancho, alto, negrita)`
  reproduce ambas.
- `CFontLibrary::Save`: `u32 5; BSTR nombre; u8 hay textos; CLanguageDataList | u32`.
  `CTextLibrary::Save`: `u32 5; BSTR; u32; u8; filas; filas de estado` (como §4). Ninguna aparece en los Switch
  del corpus; implementadas según el código.

## 6. Imágenes: CImageStorage, CImageInfo, CImageLibrary — `hmi.rs`

`CImageStorage::Save`: `u8 flags (bits 0–1 propiedades, bit 2 hay biblioteca, bit 3 copia incrustada, bit 4
siempre); u32 color transparente (0x7B08FF); u32 v14 (16); sin biblioteca: i16 n + n CImageInfo; con biblioteca:
BSTR nombre + (bit 3 = 0: i16 n + n × u32 | bit 3 = 1: CImageLibrary + u16 n)`.
Corpus: flags 0x10, sin biblioteca, n = 2 (36) o 4 (29) imágenes vacías (depende del historial; ambos valen).

`CImageInfo::Save` con save_flags & 1: `BSTR fichero; [si no está vacío: u32 n + n bytes]`. Los bytes son un
**fichero BMP completo** (`BM…`, el PNG original convertido; 39 270 B en SwitchCMD_10/11, imagen superpuesta «Off»
`Arrow_left.png`). Sin el bit 0: `u32 1; BSTR id; u32`.

`CImageLibrary::Save`: `u32 5; BSTR; u32; u8; u32; u32; filas; filas de estado` (no aparece en el corpus).

## 7. Estilo nativo: CNativePart (CNPManager::SavePart → CNativePart::Save, por CArchive) — `hmi.rs`

`u32 4; CString StyleID (+0x38); u32 tipo (+4: 1 Switch, 2 Lamp, 4 sin cuerpo, 8/0x10 selector, 0x20 medidor,
0x40 barra, 0x80/0x100 lista); u32 LabelType (+0x34); u32 +0x18; u32 +0x1C; u32 BlinkType (+0x20); cuerpo`.

Cuerpo Switch: `i16 n listas (3); por lista i16 m + m × u32 tipo de acción; u32 ReverseOnTouch (+8);
u32 Interlock (+0xC); u32 Lamp (+0x10); u32 +0x14; u32 +0x24; u32 HoldDelay (+0x28)`.

- Es una copia de la definición de `Vijeo-Frame\NativeStyles\<Category>\<StyleID>.NPS` (XML UTF-16) más
  indicadores que **repiten el objeto**: las listas llevan los tipos de las acciones de WhenTouch/WhenRelease/WhileTouch
  (65/65), LabelType = 1 si hay etiqueta (59/59), +0x18 = 1 si hay visibilidad (14/14), Lamp = 1 si hay lámpara
  (37/37). +0x1C, +0x14, +0x24, BlinkType, ReverseOnTouch, Interlock y HoldDelay: 0 en todo el corpus.
- StyleID del corpus: Square03 (59, XML `ObjectStyle="00003"`), Square05 (4, «00005»), BlankArea (2, «000000»),
  todos en `NativeStyles\Primitive` (`Category="Primitive"`). Sin save_flags & 1 sólo iría el nombre (BSTR).

## 8. Correspondencia rápida XML → binario

| XML | binario |
|-----|---------|
| `Switch Name/Left/Top/Width/Height` | cabecera común (rect inclusivo: r = Left+Width-1) |
| `LampControl` / `Style Mode="WithLamp"` | máscara bit 0 + expr cond + `NativePart.Lamp = 1` |
| `ReverseOnTouch` | máscara bit 2 (y `NativePart.ReverseOnTouch`, sin corpus) |
| `BuzzerOnTouchEnabled` | máscara bit 8 |
| `InterlockControl` | máscara bit 3 + expr (sin corpus en clips) |
| `VisibilityAnimation VisibilityControl` | máscara bit 4 + expr cond + `NativePart` +0x18 = 1 |
| `VisibilityAnimation BlinkControl/BlinkSpeed` | máscara bit 5 + expr + u16 (sin corpus) |
| `Style SwitchType="Switch" Category ObjectStyle` | `NativePart` (tipo 1, StyleID) |
| `WhenTouch` / `WhenRelease` / `WhileTouch` | 3 listas de CNativeAction |
| `Bit BitOperation DestinationVariable InternalID` | tipo 0–4 + expr destino de bit + InternalID |
| `Word WordOperation SourceExpression DestinationVariable` | tipo 10–15 + destino de palabra + valor |
| `Special SpecialFunction="ChangePanel" ChangePanelID="P11"` | tipo 0x15 + BSTR «11» |
| `Special SpecialFunction="PreviousPanel"` / `Configuration` | tipo 0x14 / 0x52, sin cuerpo |
| `PopPanel PopupPanelAction="Open|Close"` | tipo 0x20 (2 expr + BSTR) / 0x22 (BSTR) — sin corpus |
| `Decision` + `DecisionTrueList/FalseList` | tipo 0xF7 |
| `Colors ColorResourceID="X"` | bloque de color: resource «X», flag_c 1, copia de la biblioteca |
| `Colors` + `State StateType Pattern BackColor ForeColor FrameColor Color3D TextColor` | resource "", flag_c 0, filas 9/10 |
| `Labels HAlign VAlign` | CLabelStorage tipo 5, HAlign/VAlign |
| `LanguageString UserLanguageID="L1"` | CLanguageDataList: idioma «Language1», texto |
| `Font FontType FontStyle FontFace FontWidth FontHeightOrSize` | CLanguageFont propia |
| `OverlayImage StateName FileName` | overlay_on/off + u16 de uso + imagen BMP |

## 9. Botón por defecto (propuesta, `Switch::button` + `ButtonSpec`)

Lo más común del corpus (65 clips y 269 Switch de los XML):

- Cabecera: GUID del Switch, tamaño **58 × 18** (66/269; otros frecuentes 40×15, 52×20, 40×20), nombre propio,
  flags 1 y extensión vacía.
- `save_flags` 9; `hold_delay` 1; `security_group` ""; sin monitor/enclavamiento/parpadeo/radio; zumbador apagado
  (239/269).
- Acción: una en «al tocar», InternalID 1, reserved 0. Lo habitual: `BitMomentaryOn(Expr::bit_dest(var))` (comando)
  o `ChangePanel("N")` (navegación); también Toggle, Set/Reset, WordSet/Add/Subtract.
- Lámpara opcional: `lamp = Some(var)` → `Expr::cond(var)`, máscara bit 0, `NativePart.Lamp = 1`
  (XML `Mode="WithLamp"`).
- Etiqueta: estática (tipo 5), Center/Middle, idioma «Language1», fuente propia GPFontModern6x10 normal y base
  GPFontModern8x13 (§5), id de cadena 0 (se reasigna al pegar). Sin etiqueta: `LabelStorage::none()` (tipo 0, 1/1).
- Colores: o bien **propios** (`ColorBlock::own(on, off)`: autocontenido, no depende del proyecto) o una biblioteca
  del proyecto (`ColorBlock::resource(nombre, on, off)`). Valores de las bibliotecas de los ejemplos en
  `hmi::palette`: fondo 0, marco 0x707070, 3D 0, texto 0x696462 y ForeColor encendido/apagado: ButtonOnGreen
  0x00FF00/0xC0C0C0, ButtonOnRed 0x8080FF/0xC0C0C0, ButtonOnYellow 0x00FFFF/0xC0C0C0, NavigationButtonWhite
  0xFFFFFF/0xFFFFFF, NavigationButtonGrey 0xC0C0C0/0xC0C0C0.
- Imágenes: `ImageStorage::empty(2)` (flags 0x10, 0x7B08FF, 16, dos imágenes vacías), sin superposición,
  color transparente 0x7B08FF.
- Estilo: `NativePart::switch("Square03", tipos de acción por lista, etiqueta, lámpara, visibilidad, enclavamiento)`.

Dependencias del proyecto / instalación:

- **Biblioteca de colores por nombre** (`ColorResourceID`): el objeto lleva una copia, pero el nombre apunta a la
  biblioteca del proyecto de destino; si no existe allí, no se sabe qué hace Vijeo al pegar (¿se queda con la copia,
  la crea, o deja la referencia rota?). Mientras no se compruebe, para generar lo seguro es `ColorBlock::own`.
  Si se usa `resource`, los colores deben ser los de la biblioteca del proyecto (se leen de su XML
  `ResourceLibrary/ColorLibrary`).
- **Estilo nativo**: StyleID tiene que existir en `Vijeo-Frame\NativeStyles\Primitive` (Square01–71, Round01–13,
  Circular01–12, BlankArea…). La copia incrustada sólo lleva el nombre y los indicadores; el dibujo sale del .NPS
  instalado.
- **Variables**: las referencias se generan con ids 0 (`{F,2:2.0,…}`); Vijeo las vuelve a enlazar por nombre al pegar
  (verificado antes para CExpression), así que las variables tienen que existir en el proyecto de destino.
- **Idioma**: el texto va en «Language1»; un proyecto con más idiomas recibiría la etiqueta sólo en el primero.

## 10. Dudas y partes opacas

- Sin ejemplos en el corpus (formato deducido del código, no probado): bits de máscara 1, 2, 3, 5, 6, 7 y 9
  (monitor, ReverseOnTouch, enclavamiento, parpadeo, radio, HoldDelayEnabled), PopupOpen/Close (qué expresiones
  son X e Y y qué va en el BSTR), etiquetas de tipos 1–4 y numéricas (refs. a CFontLibrary/CTextLibrary),
  CImageLibrary y CImageWidget (algunas ramas de su Save las eliminó el descompilador), filas OnDown/OffDown y filas
  de estado (id 8), botones de categoría Bitmap (`ObjectStyle="20113"`).
- Campos con valor fijo en el corpus y significado desconocido: `flag_d` del bloque de color (0), `v28` de
  CColorLibrary (16), `v14` de CImageStorage (16), los dos u32 finales de cada idioma (1, 0), el u8 +8 de
  CLanguageFont (0), +0x1C/+0x14/+0x24 de la parte nativa (0), los bits altos de `flags` de CExpression, y el u32
  «reserved» de CNativeAction (0/1; Load lo ignora).
- Nº de imágenes por estado (2 o 4): no se ve de qué depende; Load acepta cualquiera.
- No comprobado en Vijeo (regla: no tocarlo): que un botón generado se pegue bien, la reasignación del id de cadena
  y el comportamiento con una `ColorResourceID` inexistente. Siguiente paso natural: pegar un `Switch::button` en un
  proyecto de prueba y exportar su XML.
