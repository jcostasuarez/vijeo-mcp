# Animaciones de los objetos de Vijeo-Designer 6.3 (portapapeles / ObjectData)

Código: `anim.rs` (sólo std + `bin.rs`). Pruebas: crate `probe/` (órdenes `anim`, `animcmp`, `animnew`, `animlist`,
`animexprs`, `animdump`; ver al final). Todo lo de aquí está comprobado contra el corpus salvo donde se dice
**sin verificar**.

Fuentes (sólo nombres/direcciones; nada copiado):
- BaseObject.dll (`baseobj`): guardado de la cabecera común `FUN_60736ab0`, carga `FUN_60741576`.
- AnimationManager.dll (`wc_animmgr`): guardado `FUN_6066a3b0`, carga v3 `FUN_60666a5a`, carga antigua
  `FUN_6066669c`, tabla de ranuras `FUN_6066a997` (nombres en `0x60676e54`).
- Un `DoPropExchange` por control: Visib.ocx `FUN_61f161b0` (+ `FUN_61f13ac0`), PosAnimation.ocx `FUN_61de6cc0`
  (+ `FUN_61de3150`), ValueAnimation.ocx `FUN_61edcac0` (+ `FUN_61eda630`), ColorAnimation.ocx `FUN_61cc7b80`
  (+ `FUN_61cceb20`, `FUN_61ce58d0`), FillAnimation.ocx `FUN_61d493a0` (+ `FUN_61d41fc0`), SizeAnimation.ocx
  `FUN_61e4a060` (+ `FUN_61e47de0`), Rotate.ocx `FUN_61e15820` (+ `FUN_61e12d00`).
- PFUtility: `CExpression::DoExpressionPropExchange` (`0x1002c830`, ya en `S\gh\pfu_expr.c`).
- Enumerados del XML: tablas {cadena, valor} de cada .ocx (`gh\enumrefs.ps1`, `va_table.ps1`).
- Descompilados completos de los .ocx en `wC\gh\wc_*_all.c` (proyectos Ghidra `wc_*`).

## 1. La extensión: el gestor de animaciones

En la cabecera común: `... u8 flags · [extensión si flags & 1] · u32 0x1CED50DA`. BaseObject pone el bit 0 si el
objeto tiene gestor de animaciones (todos los objetos actuales del corpus lo tienen; 759 piezas antiguas de la
Toolchest no) y llama a `IPersistStream::Save` del `AnimationMgrObject`, que escribe:

```
u32 3                         versión del gestor
repetido, una por animación:
  u32 n                       tamaño de los datos (sin contar GUID)
  GUID de clase (16 B)        traducido (CPFUtility::CLSIDReverseLookup), el de classes.rs
  n bytes                     IPersistStream::Save del control (ver §3)
u32 0                         fin
```

Sin animaciones: `03 00 00 00 00 00 00 00`. Ejemplo (MCBStatus_OK de ATV12.bin, visibilidad):
`01 | 03000000 | f6020000 | 5f8f1e52 c3a2 1448 a903 a78a52a9c720 | 04000000 00 f3030000 ... | 00000000 | da50ed1c`.

**Orden**: el gestor guarda sus 10 ranuras en este orden (tabla del registro
`HKLM\SOFTWARE\WOW6432Node\Schneider Electric\Vijeo-Designer63\Vijeo-Frame\Vijeo-Designer63\GraphicalEditor\Animations`);
una animación por clase como mucho:

| ranura | clase | GUID en el stream | .ocx | versión 6.3 | tipada |
|---|---|---|---|---|---|
| 0 | Color | {67888E8D-9486-4065-A5D5-31B725501695} | ColorAnimation.ocx | 9 | sí |
| 1 | Fill | {5626009D-9182-495C-B817-1164423C2A7C} | FillAnimation.ocx | 5 | sí |
| 2 | Size | {11CC3AEA-E3EC-41F2-BBA3-A3DF553DFDAC} | SizeAnimation.ocx | 4 | sí (**sin ejemplos**) |
| 3 | Slider | (sin clase registrada) | – | – | – |
| 4 | Position | {23FFD5F6-7A66-4F71-99BA-66404EC9D0F3} | PosAnimation.ocx | 4 | sí |
| 5 | Rotate | {372D2B71-1692-49E5-AB9B-49E320D6F8D6} | Rotate.ocx | 4 | sí (la de las líneas) |
| 6 | Touch | {765B4F06-4A94-400A-85A7-20BC624D09BB} | Touch.ocx | – | en bruto |
| 7 | Value | {49B52EE2-DAB7-4D8A-BC69-417CD9037A61} | ValueAnimation.ocx | 7 | sí |
| 8 | Visibility | {521E8F5F-A2C3-4814-A903-A78A52A9C720} | Visib.ocx | 4 | sí |
| 9 | SmartObject | {ED248C23-F5AE-4FBB-BD00-811BA61B3B05} | SmartObj.ocx | – | en bruto |

Carga (`FUN_60666a5a`): lee la versión; mientras `n != 0`: GUID → ranura; si la reconoce llama a
`IPersistStream::Load` **sin volver a colocarse con `n`** (los datos tienen que ser exactos); si no, salta `n` bytes.
`write_ext` ordena por ranura y rechaza dos de la misma clase; `set`/`remove` sustituyen o quitan por clase.

## 2. Primitivas (MFC `CArchivePropExchange` sobre el stream)

Cada control es un `COleControl` de MFC: su `IPersistStream::Save` hace `DoPropExchange` sobre un `CArchive`.
- `ExchangeVersion`: u32 versión (la tabla de arriba).
- `PX_Bool`: **u8** (0/1). `PX_Short`: i16. `PX_Long`/`PX_ULong`: u32. `PX_Double`: f64. `PX_Color`: u32.
- `PX_String`: CString de MFC (`FF FE FF` + longitud u8, o `FF`+u16, o `FF FFFF`+u32 + UTF-16 sin NUL).
- `PX_Blob`: u32 n + n bytes.
- `COleControl::DoPropExchange` (sólo Color, Size, Position y Value): `_ExtentX` i32, `_ExtentY` i32 (HIMETRIC del
  control, siempre 2646 × 1323 = 100 × 50 px, no es el tamaño del objeto) y `_StockProps` u32 (siempre 0; si no lo
  fuera seguirían las propiedades de serie: `anim.rs` deja entonces la animación en bruto). Tipo `Extent`.

## 3. Referencias a variables: CExpression en forma CArchive

Las animaciones guardan sus expresiones con `CExpression::DoExpressionPropExchange` (mismos campos que
`CExpression::Save` en BSTR, pero CString y dos campos en i16). Tipo `Expr`:

```
u32  Version = 0x3F3
CStr RawString          lo que escribe el usuario, con las variables como {F,..}/{T,..}
CStr BuildString        compilada
CStr CfgBuildString     = BuildString (en constantes compiladas a veces es un resto de la expresión anterior)
i16  ValidBuild         0 compilada; -1 sin compilar o casilla vacía; 1 (destinos de color sin usar)
u32  ExpressionDataType 0x02 bit, 0x10, 0x20 booleana, 0x40 entera, 0x80 real
u32  ExpressionType     2 destino, 4 valor, 5 constante/límite de rango, 8 condición
u32  LiteralExprType    2 una variable, 8 expresión compuesta, 1 número, 0x20 vacía/sin compilar
CStr m_mUsedTagName                    "Carpeta.Var\n" por variable
CStr m_mTagListForCGDependence         "TagDB.Carpeta.Var\n"
CStr m_mArrayTagListForCGDataQuality   ""
CStr m_mSingleTagListForCGDataQuality  "TagDB.Carpeta.Var\n"
CStr m_mArrayTagNameList               ""
CStr m_mRealArrayTagNameList           ""
CStr m_mArrayIndexStringList           ""
u32  LastValidatedExprDataType   = dt en las compiladas; 0x20 en las vacías/sin compilar
i16  PlaceHolderFlagValue        nibble bajo siempre 6; el alto varía al azar (0..F) en el corpus → se escribe 0x06
i16  n marcadores; n × (CStr marcador en forma raw, CStr descripción)
```

Atributos por uso (comprobados en las 346 expresiones de animación del corpus; `vb dt et lt lv`):

| uso | vb | dt | et | lt | lv | build | constructor |
|---|---|---|---|---|---|---|---|
| condición con una variable (visibilidad, parpadeo, color discreto) | 0 | 0x20 | 8 | 2 | 0x20 | `(TagDB.X.getIntValue(0) != 0)` | `Expr::var(p, Use::Cond)` |
| condición compuesta / negada | 0 | 0x20 | 8 | 8 | 0x20 | p. ej. `!(TagDB.X.getIntValue(0) != 0)` | `Expr::composite`, `invert_cond` |
| control entero (posición, relleno, rotación, valor entero) | 0 | 0x40 | 4 | 2 | 0x40 | `(TagDB.X.getIntValue(0))` | `Use::Int` |
| valor real (animación de valor) | 0 | 0x80 | 4 | 2 | 0x80 | `(TagDB.X.getFloatValue(0))` | `Use::Float` |
| variable como límite de rango | 0 | 0x40 | 5 | 2 | 0x40 | `(TagDB.X.getIntValue(0))` | `Use::Range` |
| número de un eje activo / ángulo | 0 | 0x40 | 5 | 1 | 0x40 | = raw (`40`, `-200`) | `Expr::number` |
| número de un eje desactivado (`0`,`100`) | -1 | 0x40 | 5 | 0x20 | 0x20 | "" | `Expr::number_unbuilt` |
| valor de un estado de color (`0`,`1`) | -1 | 0x40 | 5 | 1 | 0x40 | "" | `Expr::state_value` |
| casilla vacía | -1 | según casilla | según casilla | 0x20 | 0x20 | "" | `Expr::unused(dt, et)` |

La posición usa `getIntValue` (dt 0x40) aunque la variable sea real.

**Forma raw de una referencia** (`raw_ref`, `raw_ref_member`):
- `{F,<n>:<ids>,Carpeta}.{T,<n>:<ids>,Var}`: `F` carpeta, `T` variable; `<n>` = número de componentes del id
  (`2:2.231`, `2:1.30`; el primero cambia de un proyecto a otro: 2 en Hardwired, 1 en Modbus/TC).
- Variable de sistema sin carpeta: `{T,2:1.1,_CurPanelID}`.
- Miembro de estructura: la variable de estructura es `T` y el miembro lleva 3 componentes:
  `{F,2:1.1,SoM}.{T,2:1.30,IpWdgtPara}.{T,3:1.30.10,rLvl4}` → build `TagDB.SoM.IpWdgtPara.rLvl4`.
- Marcador de estructura (plantillas): `$...$` alrededor de la parte sustituible,
  `${F,2:2.1,MyController}.{F,2:2.2,ATV12}$.{T,2:2.231,xStatMCB}`; entonces la lista de marcadores lleva
  (`{F,2:2.1,MyController}.{F,2:2.2,ATV12}`, descripción). Siempre coinciden: con `$` hay marcadores y sin `$` no.
  En el XML queda `$MyController.ATV12$.xStatMCB`.
- Expresiones: `${..}$.{T,..,iAxisStatus}<2` → `(TagDB.MyController.LXM28.iAxisStatus.getIntValue(0))<2`;
  `A && !B` → `(…A… != 0) &&!(…B… != 0)` (la compilada pierde espacios). Cada variable una vez en las listas.
- Restos sin compilar en ejes desactivados de una plantilla: nombres planos sin llaves
  (`MyController.Application.POU_MED_L1_1....intBeamPower`, vb -1).
- Ids: `anim.rs` escribe 0 (`2:2.0`, `3:2.0.0`). Que Vijeo vuelve a enlazar por nombre al pegar está verificado para
  las expresiones BSTR de los botones; para éstas (misma clase CExpression) **sin verificar**, y para miembros de
  estructura con ids 0 tampoco.

## 4. Cada clase, campo a campo

Todas empiezan por la versión u32. Los nombres son los de `DoPropExchange`; entre corchetes, el atributo del XML.

### Visibility (v4, sin extensión OLE) — `VisibilityAnimation`
```
u8   VisibBlk      parpadeo activo            [presencia de BlinkControl]
Expr (parpadeo)                               [BlinkControl]
i16  VisibSpeed    0 Fast, 1 Slow (también sin parpadeo)  [BlinkSpeed]
u8   VisibVisib    visibilidad activa         [presencia de VisibilityControl]
Expr (visibilidad) visible mientras sea cierta [VisibilityControl]
```
No hay campo de invertir: se niega la expresión (`!X`, `X==0`; corpus: DriveRun_OFF, Status_OK02...).
`InvisibleWhenInvalid` **no** es de esta clase: en el XML aparece en los MessageDisplay (datos de su clase, extensión
vacía) y en `StateColorAnimation` (clase Color). Corpus: 285 visibilidades (68 en los clips), todas con VisibVisib=1;
11 con parpadeo (speed 1 = Slow; AlarmIndicator speed 0 = Fast, confirmado con la tabla `Fast 0, Slow 1` de Visib.ocx).
Crear: `Visibility::by_var(p, invertir)`, `Visibility::when(expr)`, `.with_blink(cond, rápido)`. 454 B con una
variable de una carpeta.

### Position (v4) — `PositionAnimation` con `Vertical`/`Horizontal`
```
Extent (cx, cy, 0)
u8   PosVerticanEnable   [hay <Vertical>]
Expr PosVertical         [VerticalPositionControl]
u8   PosHorizontalEnable [hay <Horizontal>]
Expr PosHorizontal       [HorizontalPositionControl]
Expr PosVerticalFrom   [VerticalValueRangeFromExpr]    Expr PosVerticalTo  [VerticalValueRangeToExpr]
Expr PosVerticalStart  [VerticalPositionRangeStartExpr] Expr PosVerticalEnd [VerticalPositionRangeEndExpr]
Expr PosHorizontalFrom/To/Start/End (igual)
```
Start/End en px de desplazamiento (pueden ser negativos). Eje desactivado: control vacío y `0`,`100`,`0`,`100` sin
compilar. Crear: `Position::new(PosAxis::by_var(var, vmin, vmax, pini, pfin), PosAxis::off())`; los límites pueden ser
variables (`Expr::var(p, Use::Range)`, como RectangleL0..4).

### Size (v4) — `SizeAnimation` (**sin ejemplos: sólo del descompilado**)
```
Extent
u8 SizeVerticalEnable  [VerticalSizeEnabled]   u8 SizeHorizontalEnable [HorizontalSizeEnabled]
Expr SizeVertical [VerticalSizeControl]        Expr SizeHorizontal [HorizontalSizeControl]
Expr SizeVerticalFrom [VerticalValueRangeFromExpr]  Expr SizeVerticalTo [VerticalValueRangeToExpr]
Expr SizeVerticalMax [VerticalDisplayRangeMaxExpr]  Expr SizeVerticalMin [VerticalDisplayRangeMinExpr]   (¡Max antes que Min!)
i16  SizeVerticalAnchor (defecto 1) [VerticalAnchor: Top 0, Middle 1, Bottom 2]
Expr SizeHorizontalFrom, To, Max, Min
i16  SizeHorizontalAnchor (defecto 1) [HorizontalAnchor: Left 0, Middle 1, Right 2]
```
Crear: `Size::new(SizeAxis::by_var(var, vmin, vmax, pct_min, pct_max, anchor), SizeAxis::off())`. Los valores de un
eje desactivado se copian de Position/Fill (sin verificar).

### Fill (v5, sin extensión OLE) — `FillAnimation`
```
u8 FillVerticalEnabled [VerticalFillEnabled]; Expr FillVertical [VerticalFillControl]
i16 FillVerticalAnchor [VerticalAnchor: Top 0, CenterbothSides 1, CenterOneSide 2, Bottom 3] (defecto 3)
Expr FillVerticalMinPercent [VerticalDisplayRangeMinExpr]; Expr FillVerticalMaxPercent [VerticalDisplayRangeMaxExpr]
Expr FillVerticalFromRange [VerticalValueRangeFromExpr];   Expr FillVerticalToRange [VerticalValueRangeToExpr]
u8 FillHorizontalEnabled; Expr FillHorizontal; i16 FillHorizontalAnchor [Left 0, CenterBothSides 1, CenterOneSide 2, Right 3] (defecto 0)
Expr FillHorizontalMinPercent, MaxPercent, FromRange, ToRange
u32 FillPpgForeColor   [BackAreaColor]  COLORREF 0x00BBGGRR; 0x08000000 = transparente (corpus 0x0800FF00)
u32 FillPpgBackColor   (sin XML; corpus 0x0000FF00)
u32 FillBackAreaPattern (corpus 1)
i16 Fillforeblk, i16 Fillbackblk (corpus 0)
```
Crear: `Fill::by_var(var, vertical, vmin, vmax)` (llena 0..100 %, zona sin rellenar transparente).

### Rotate (v4, sin extensión OLE) — `RotateAnimation` (la de las líneas, `{372D2B71-…}`)
```
u8 Rotation [RotateEnabled]; Expr Tag [RotateControl]
Expr StartAngle [AngleStartExpr]; Expr EndAngle [AngleEndExpr]
Expr From [ValueRangeFromExpr];   Expr To [ValueRangeToExpr]
f64 X [OffSetX]; f64 Y [OffSetY]; i16 Angleunit (corpus -1, sin XML)
```
Crear: `Rotate::by_var(var, vmin, vmax, ang_ini, ang_fin)` (X = Y = 0, Angleunit -1 como el corpus).

### Value (v7) — `ValueAnimation` (texto que muestra una variable)
```
Extent
u8 ValueEnable; i16 DataType [DataType: 0 discreto (OnLabel/OffLabel), 1 Integer, 2 Real, 3 String]
CStr OnLabel; CStr OffLabel; i16 Align; u8 InputMode [InputModeEnabled]
f64 RangeFrom, RangeTo, RangeMin, RangeMax   (corpus 0, 1, 0, 1)
i16 Format [Format: Dec 0, Hex 1, Oct 2, Bin 3]; u8 DisplayDigit
i16 Integer [IntegerDigitRange; en reales sale como FloatDigitRange]; i16 Fraction [DecimalDigitRange]
u8 ZeroSuppress [ZeroSuppressCheck]; u8 RoundOff; i16 TriggerRedraw
u8 ClearMode; u8 BarCodeInput; u8 CheckDigit; u8 DisplayLengthCheck; i16 DisplayLength [DisplayLength]
u8 ShowHideKeypad [PopupKeypadEnabled]
Expr (valor) [ValueExpr]
CStr PopupKeypad; u8 InputStatus; u8 KeypadBeep [BeepOnTouch]
Expr [InputStatusTag] (vacía: dt 2, et 2); Expr (sin XML; vacía: dt 0x10, et 2)
u8 Interlock; Expr [InterlockTag] (vacía: condición)
```
Crear: `Value::by_var(p, real, cifras_enteras, decimales, supresion_ceros)`; el resto, como `Number` y `TextL0..4`.

### Color (v9) — `ForeColorAnimation`/`BackColorAnimation`/`LineColorAnimation` + `StateColorAnimation`
```
Extent
u8 ForeEnable, BackEnable, LineEnable
i16 ForeDataType, BackDataType, LineDataType   [DataType: Integer 0, Discrete 1]
Expr ForeTag, BackTag, LineTag                 [DataExpr]
3 veces (fore, back, line): i16 n filas; por fila [ValueRange]:
    u32 color [ColorValue: hex del COLORREF tal cual, p. ej. 4080FF; transparente → "08" delante]
    u8 Tran (transparente); u8 Orig [OriginalColorChecked]; i16 Blink [Blink: None 0, Fast 1, Mid 2, Slow 3]
    Expr valor [ValueRangeExpr]; u32 InternalID [InternalID: 1, 2...]
i16 GridMode; u8 Enable (StateColorAnimation); i16 CtrlType [StateColorAnimation DataType: Integer 0, Discrete 1, String 2]
i16 NumOfStates; Expr CtrlTag [StateExpression]; CStr ColorLibraryID [ColorResourceID]
NumOfStates × CStr (valores de los estados; por defecto "0".."4")
u8 Invisible [InvisibleWhenInvalid]
si ColorLibraryID != "": u32 ColorLibrarySize; si > 0: PX_Blob ColorLibraryData (u32 n + CColorLibrary::Save)
```
Destino sin usar: control con vb **1**, `Discrete`, 0 filas. En discreto Vijeo guarda como mucho 2 filas.
Crear: `Color::new(Some(ColorTarget::discrete(p, color_0, color_1)), None, None)` (fore / back / line según la
posición). `ColorTarget::by_value` (dt 0, por valor) y la biblioteca de colores **sin ejemplos**.

### Touch y SmartObject
En bruto (`Animation::Raw`): SmartObject aparece en 434 piezas de la Toolchest y 12 de los ejemplos .vdz; Touch en
ninguno.

## 5. Ida y vuelta

`probe "<clip1>;<clip2>;…" anim`: para cada objeto (también grupos) lee la extensión con `parse_ext`, la reescribe con
`write_ext` y compara byte a byte; además pasa el objeto por `get_animations`/`set_animations`.
- 18 clips del corpus: **540/540 extensiones iguales** (449 vacías), 96 animaciones, todas tipadas
  (68 Visibility, 13 Position, 8 Value, 3 Color, 2 Fill, 2 Rotate).
- 4 proyectos de ejemplo (`S\ex\*.vdz`): 1413/1413 iguales (217 visibilidades tipadas, 12 SmartObject en bruto).
- Toolchest instalada (`probe TC anim`): 4387/4387 iguales; en bruto Color v8 (8), una Value con CExpression 0x3F2
  y SmartObject (434): versiones antiguas que no se tipan.

`probe "<clips>" animcmp`: reconstruye cada animación del corpus con los constructores (mismas variables y valores)
y la compara campo a campo quitando lo irreproducible (ids y `$` del raw, marcadores, nibble alto de los flags y el
resto en `CfgBuildString` de las constantes): **96/96 iguales** en los clips (54 + 14 visibilidades, 13 posiciones,
8 valores, 3 colores, 2 rellenos, 2 rotaciones) y 217/217 visibilidades en los .vdz.

`probe <clip> animnew`: cada constructor se relee igual; una extensión con 6 clases sale ordenada por ranura; puesta en
un objeto sin animar del clip (`set_animations` = `Node::set_extension` del MCP), escrita como portapapeles con el
backend del MCP y releída: iguales; quitar todas devuelve el objeto original byte a byte.

## 6. Dudas

- Nada se ha pegado en Vijeo (regla de la tarea): falta probar que pegue objetos con animaciones nuevas, en especial
  los ids 0 en estas expresiones CString y en miembros de estructura.
- `Expr::expression` (expresión libre sin compilar, vb -1) no tiene equivalente activo en el corpus; mejor
  `Expr::composite` con la compilada escrita a mano siguiendo las reglas de §3.
- Size y Color por valor/biblioteca de colores: sólo del descompilado. Touch y SmartObject sin tipar.
- Significado de `Angleunit` (-1), de la expresión sin XML de Value (dt 0x10), de `FillPpgBackColor` y de los bits de
  `PlaceHolderFlagValue` (0x02 parece "tiene marcadores"; el nibble alto parece memoria sin inicializar).
- `CfgBuildString` de las constantes compiladas: en el corpus arrastra la compilada de la expresión anterior; se
  escribe igual a la constante.
