# Text, Rectangle, Line y Ellipse de Vijeo-Designer 6.3: datos de clase en el portapapeles

Lo que sigue a la cabecera común (GUID + `u8 3` … marca `0x1CED50DA`). Todo little-endian, `IStream::Write`
directo. Código: `shapes.rs` (structs, parse/write, constructores por defecto); órdenes en
`probe/src/shapes_cli.rs`.

## Fuentes

| Clase (XML) | GUID en el stream | DLL | Save / Load / SaveXML |
|---|---|---|---|
| Text v11 | `{CFD4B0CC-678F-4F78-980C-24C14CBE68E1}` | TextObject.dll | `FUN_61aa57a5` / `FUN_61aa9869` / `FUN_61aa60a6` |
| Rectangle v2 | `{3BE64A91-D329-4653-9891-AA4EE97464C2}` | RoundRect.dll | `FUN_6164ce8c` / `FUN_6165119f` / `FUN_6164f111`; Corner `FUN_616465b3` |
| Line v2 | `{78914787-36A8-4096-9256-DF9BD6C180AA}` | LineObject.dll | `FUN_61237e85` / `FUN_6123c0f5` / `FUN_6123a22d` |
| Ellipse v2 | `{22A41009-62E3-4612-B997-032976965397}` | CircObject.dll | `FUN_60ae7008` / `FUN_60aebc30` / `FUN_60ae9efd` |

Textos y fuentes: HMIManagers.dll `CLanguageDataList::Save(IStream*)` (630d2af0), `CLanguageFont::Save` (630c5130),
`CLanguageString::Save` (630cd9b0), `CLanguageFont::SaveXML` (630c57c0). Enumeraciones sacadas de las tablas
{cadena, valor} de las DLL (g_blinkMap, g_horzAlignMap, g_vertAlignMap, g_rectShapeMap, Direction, fuentes).
Proyectos Ghidra: `wd_rrect`, `wd_line`, `wd_circ`, `wd_text` (+ `hmimgr`). Los ficheros `.c` de `gh\` son
material de Schneider: sólo para consulta.

Ninguna de estas clases escribe nada de estilo en BaseObject.dll: todo el estilo va en los datos de la clase.

## Tipos comunes

- **Color** = COLORREF de Windows `0x00BBGGRR` (u32; R en el byte bajo). En el XML se escribe el valor en
  hexadecimal, mayúsculas, con número **par** de cifras: `"A4A09F"` = bytes `9F A0 A4 00` (R=9F G=A0 B=A4),
  `"00"` = 0 (negro), `"FF0000"` = azul, `"FF00"` = verde, `"0A9902"`.
- **Transparencia**: bit `0x08000000` del color **y** una bandera u32 aparte por color (`*_trans`, 0/1). El
  XML escribe `color | (bandera ? 0x08000000 : 0)`: `BackColor="08000000"` = transparente (color 0),
  `"08FFFFFF"`/`"08A4A09F"` = transparente conservando el RGB anterior. En el corpus bit y bandera van juntos
  (2 829 de 2 838 colores; las 9 excepciones son piezas antiguas de la Toolchest), así que para crear un
  transparente se ponen los dos (`paint(None)` en shapes.rs). La carga usa el bit del color para el degradado.
- **Parpadeo** (Blink, u16): None 0, Fast 1, Mid 2, Slow 3 (tabla g_blinkMap). En el corpus todos son None.
- **Degradado** (Gradient): `i16 estilo` (eShadingStyle; 0 = liso, el único visto) + `u16 n` + `n × u32` color.
  La lista interna tiene n+1 colores: el 0 es el propio color del objeto y no se guarda. Sin degradado Vijeo
  guarda n=1 con `0x02FF0000` (relleno del rectángulo/elipse), `0x02FFFFFF` (líneas y texto) y `0x02000000`
  (fondo del texto).
- **BSTR**: u32 bytes (con NUL) + UTF-16 + NUL; u32 0 = BSTR nulo (se distingue de `""` = `02 00 00 00 00 00`).
- Los anchos de línea de Rectangle/Line/Ellipse son `f64` de un entero; en Text, u32.

## Rectangle v2 (78 bytes fijos)

| Campo | Tipo | XML (`<Style>`) |
|---|---|---|
| versión | u32 = 2 | |
| fore_color | u32 color | ForeColor (relleno) |
| line_color | u32 color | LineColor |
| back_color | u32 color | BackColor (fondo del patrón) |
| fore_blink, line_blink, back_blink | 3 × u16 | ForeBlink, LineBlink, BackBlink |
| line_width | f64 | LineWidth |
| line_style | u32 | LineStyle (número; 0 continua; la Toolchest usa también 2 y 5) |
| pattern | u16 | Pattern (1 = liso) |
| corner_ratio | f64 | Corner (ver abajo) |
| back_trans, fore_trans, line_trans | 3 × u32 | bit 08000000 de BackColor/ForeColor/LineColor |
| shape | u16 | RecShape: Normal 0, Rounded 1, Cut 2 |
| reservado | u32 = 0 | (se lee y se ignora) |
| fore_gradient, line_gradient | Gradient × 2 | (sólo v ≥ 2) |

**Corner** (px, sólo con Rounded/Cut) = con dx = Width−1, dy = Height−1 (los `r−l`, `b−t` de la cabecera):
`round(max(dx,dy) × ratio)`, y si supera la mitad del lado menor, `lado_menor/2` (entero); tope 599.
Comprobado en los 49 rectángulos (con Width en lugar de Width−1 fallaban 3). Para fijar N px:
`ratio = N / max(dx, dy)`. El valor inicial de Vijeo es 1/6 (constante en RoundRect.dll y en 8 objetos).

## Ellipse v2 (66 bytes fijos)

`u32 2`, fore_color, line_color, back_color (u32), fore/line/back blink (3 × u16), line_width f64,
**line_style u16**, pattern u16, **line_trans, fore_trans, back_trans** (3 × u32, en este orden, distinto del
rectángulo), `u32` reservado = 0, fore_gradient, line_gradient. XML `<Ellipse>`: `<Style BackColor BackBlink
LineColor LineBlink LineStyle LineWidth ForeColor ForeBlink Pattern>`; ocupa el rectángulo de la cabecera.

## Line v2 (76 bytes fijos)

| Campo | Tipo | XML |
|---|---|---|
| versión | u32 = 2 | |
| line_color, back_color | 2 × u32 | Style LineColor, BackColor |
| line_blink, back_blink | 2 × u16 | LineBlink, BackBlink |
| line_width | f64 | LineWidth |
| line_style | u32 | LineStyle |
| start x, start y, end x, end y | 4 × f64, **absolutas** del panel | StartPoint X/Y, EndPoint X/Y = punto − (Left, Top) de la cabecera |
| back_trans, line_trans | 2 × u32 | |
| arrows | u16 | Direction (flechas): None 0, Start 1, End 2, Both 3 |
| reservado | u32 = 0 | |
| shading | i16 (sólo v ≥ 2) | estilo de sombreado de la línea (0) |

La cabecera común lleva el envolvente inclusivo de los extremos (2 031 de 2 033 líneas). **Ojo al mover**: los
puntos van en coordenadas absolutas dentro de los datos de la clase, así que desplazar sólo la cabecera
(`Node::shift`/`set_rect` del MCP) los deja atrás. Es lo que pasa en las dos líneas de `TC_anims` (cabecera
desplazada +20,+20 respecto a los puntos) y Vijeo lo exporta tal cual: `StartPoint X="9" Y="-20"`. Hay que
mover también los puntos (`Line::shift`); Polyline/Polygon probablemente tengan lo mismo.

## Text v11

| Campo | Tipo | XML |
|---|---|---|
| versión | u32 = 11 | |
| text_blink, back_blink, blink_3d | 3 × u16 | Style TextBlink, BackBlink, Blink3D |
| direction | u16 | (propiedad CDirection; no sale en el XML; 0 en todo el corpus) |
| line_spacing | u16 | Style TextLineSpacing |
| h_align | u16 | `<Text HAlign>`: Left 0, Center 1, Right 2 |
| v_align | u16 | `<Text VAlign>`: Top 0, Middle 1, Bottom 2 |
| rotation | u32 | Style TextRotation = 90 × valor |
| text_color, back_color, color_3d | 3 × u32 | TextColor, BackColor, Color3D |
| text_trans, back_trans, trans_3d | 3 × u32 | (bit 08000000 de los anteriores) |
| string_id | u32 | id de la cadena del proyecto (al pegar con la lista incluida, Vijeo lo pone a −1 y crea otra: puede ir 0) |
| save_flags | u32 = 9 | bit 0: lista de textos incluida; bit 3: al leer se reinician GroupNumber/OrderNumber |
| si flags & 1: hay_lista | u32 (0/1) | |
| si hay_lista: CLanguageDataList | | `<LanguageString>` + `<Font>` por idioma |
| line_color, line_trans | 2 × u32 | Style LineColor (borde del cuadro) |
| line_blink | u16 | LineBlink |
| line_width | **u32** | LineWidth |
| line_style | u32 | LineStyle |
| bitmap_display | u16 (bool 0/FFFF) | BitmapDisplay |
| group_number, order_number | 2 × i16 | GroupNumber (−1), OrderNumber (0) |
| si bitmap_display: CLanguageString | | |
| back_gradient, text_gradient, line_gradient | 3 × Gradient | |

**CLanguageDataList** (v3): `u32 3`, `u32 n`, y por idioma: BSTR nombre del idioma (`"Language1"`), BSTR texto,
CLanguageFont (fuente por defecto del idioma, la del proyecto: GPFontModern8x13 en los tres proyectos),
`u32` hay fuente propia (0/1) + CLanguageFont (la del texto; es la que sale en `<Font>`), `u32` UserLanguageID
(1 = `"L1"`), `u32` sin identificar (0; un 2 en una pieza de la Toolchest).

**CLanguageFont** (v4): `u32 4`, `u32 FontType` (GP 0, TrueType 1, AGFA 2), `u32 FontStyle` (Normal 0, Bold 1,
3D 2), `i32 tt_height`, `i32 tt_width`, BSTR cara TrueType, `i32 gp_height`, `i32 gp_width`, `u32 gp_face`,
`u8 charset`, `i32 agfa_size` (px), `u32 agfa_face`. El XML sólo muestra los del tipo activo: GP →
`FontFace=GP_FACES[gp_face] FontWidth=gp_width FontHeightOrSize=gp_height`; TrueType →
`TrueTypeFontFace FontWidth=tt_width FontHeightOrSize=tt_height`; AGFA → `FontFace FontSize` (pt). Caras GP:
SansSerif 0, JGothic 1, JMincho 2, KGothic 3, KMincho 4, SongTi 5, FangSong 6, TaiSongTi 7, TaiMingLiu 8,
Modern6x10 9, Modern8x13 10, Modern8x13F 11, Modern13x23 12, Modern13x23F 13, Modern28x50 14, Cyrillic 15
(AGFA 0–9: Japanese, Korean, SmpChinese, TrdChinese, Latin, Segment, UtahWGL, AndaleWGL, Arabic, Hebrew). Con
una fuente GP, Vijeo rellena los campos TrueType con Lucida Console −8 (6x10) u −11 (8x13, 8x13F, SansSerif),
peso 400/700 (normal/negrita), charset 0, agfa_size = tt_height, agfa_face 4.

**CLanguageString** (v2, sólo con BitmapDisplay; no aparece en el corpus): `u32 2`, BSTR, `u8` hay fuente,
[CLanguageFont].

**Texto multilínea**: las líneas se separan con LF solo (`0A 00`), sin CR; en el XML es un salto de línea
dentro de `<LanguageString>` (el fichero .vxml tiene CRLF, pero eso es el fin de línea del fichero). 8 textos
del corpus lo comprueban (`"Axis\nnot homed!"`…).

## Comprobación contra el corpus

- Ida y vuelta (`probe ALL rt`): Text 164/164, Rectangle 49/49, Line 12/12, Circle 3/3 iguales byte a byte.
  Toolchest (`probe TC rt`): Rectangle 776/776 (v1 y v2), Line 2 021/2 021 (v1 y v2), Circle 33/33 (v1),
  Text 29/29 v11 (las 439 v9/v10 no se soportan).
- Contra el XML exportado (`probe ALL xml`, objeto por Name + Width + Height): los 228 objetos coinciden en
  todos los atributos de `<Style>`, `HAlign/VAlign`, `<LanguageString>`, `<Font>` y `StartPoint/EndPoint`,
  incluido Corner; ningún atributo de esos elementos queda sin derivar.
- Constructores por defecto (`probe ALL defaults`): `default_text` da exactamente 30 de los 164 textos (con su
  texto), `default_rectangle` 12 de 49 (estilo y Corner 3), `default_line` 2 de 12, `default_ellipse` 3 de 3.
- `probe X newclip <out.bin>` escribe un portapapeles con un texto (dos líneas, 8x13 negrita, centrado), un
  rectángulo, una línea y una elipse verde creados desde cero (`nuevas_formas.bin`; se relee bien). **No se ha
  pegado en Vijeo** (prohibido tocarlo en esta tarea).

Versiones 1 (Toolchest): Rectangle y Ellipse v1 = v2 sin los dos degradados; Line v1 = v2 sin `shading`.

## Dudas

- Sin ejemplos en el corpus de: parpadeos ≠ None, degradados (estilos ≠ 0 y sus nombres), Cut, flechas End/Both,
  BitmapDisplay (CLanguageString), varios idiomas, fuentes TrueType/AGFA propias de un texto, `direction` ≠ 0,
  rotación ≠ 0, interlineado ≠ 0: la gramática sale del código, los valores no están comprobados en el XML.
- Significado de los números de LineStyle y Pattern (el XML también los da como número).
- El último u32 de cada idioma (0) y los valores TrueType "a la sombra" para caras GP no vistas.
- Si Vijeo acepta un transparente con sólo la bandera o sólo el bit (se recomienda poner los dos).
