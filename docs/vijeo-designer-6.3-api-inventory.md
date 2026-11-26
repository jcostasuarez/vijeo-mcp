# Inventario de APIs públicas — Vijeo-Designer 6.3

> Instalación analizada: `C:\Program Files (x86)\Schneider Electric\Vijeo-Designer 6.3`
> Build: **6.3.0.103** · Binarios **32 bits** (MFC/ATL, COM) · Registro: `HKLM\SOFTWARE\WOW6432Node\Schneider Electric\Vijeo-Designer63\06.30`
> Fecha del análisis: 2026-11-01
> Sexta tanda (2026-11-01): el MCP ya no usa COM para los proyectos abiertos; sólo lo usa para `LoadXMLCommand`. Ver §12.10.
> Séptima tanda (2026-11-02): el MCP ya no usa COM. Se quitaron `LoadXMLCommand` y `project_import_xml`, y no hay vía de import XML sin COM. Ver §12.11.
> Octava tanda (2026-11-02): `target_config_set` escribe la configuración de comunicación en el `.vdz` sin Vijeo. Ver §12.12.

Referencias generadas a partir de esta instalación:

> El pseudocódigo descompilado (`docs/reference/vd-6.3/decompiled/`), los volcados de type libraries, `MathMethodDefine.xml` y `tools/vdat_decrypt32.ps1` son material de Schneider Electric o derivado de sus binarios: no se publican en el repositorio. Se regeneran en local con las herramientas de `tools/` sobre una instalación propia.

- `docs/reference/vd-6.3/typelibs/*.idl.txt` — volcado de las 90 type libraries COM (+ `_summary.txt`).
- `docs/reference/vd-6.3/MathMethodDefine.xml` — definición de la API de scripts que usa el editor.
- `tools/dump_typelibs.ps1` — script para regenerar los volcados sobre otra versión (`-Root <Vijeo-Frame> -OutDir <dir>`).

---

## 0. Resumen: superficies de integración

| # | Superficie | Tipo | Requiere VD abierto | Utilidad para el MCP |
|---|---|---|---|---|
| 1 | **Automatización COM `VijeoFrame`** (`Vijeo-Frame.tlb`) | Servidor COM out-of-proc (`LocalServer32` → `Vijeo-Frame.exe`), `IDispatch` | Sí (lo lanza COM) | ⭐ Principal: proyectos, targets, paneles, build, download, símbolos, informes |
| 2 | **Import/Export XML** (`public.xsd`) | Esquema XML + `GenericCall LoadXML` | Parcial | ⭐ Generación/edición masiva de proyecto (variables, paneles, alarmas…) |
| 3 | **CLI `DataTransferTool.exe`** | Ejecutable | No | Get/Put de recetas, logs, alarmas, backups `.vdz` sobre Ethernet |
| 4 | **CLI `DataConverter.exe`** | Ejecutable | No | Conversión `.rcp`/`.dat` ↔ `.csv`/`.txt` |
| 5 | **CLI `RuntimeLoader.exe`** (PDPLoad) | Ejecutable | No | Carga de runtime/core a targets (serie/Ethernet/USB) |
| 6 | **Línea de comandos `Vijeo-Frame.exe /import`** | Ejecutable | — | Abrir/importar `.vdz` |
| 7 | **API de scripts Java del runtime** (`gpClasses`, `MathMethodDefine.xml`) | API dentro del HMI | — | Generar/validar scripts de usuario |
| 8 | **Lectores de tablas de símbolos** (`symb*.dll`) | DLLs nativas | — | Formatos de importación de variables de PLC |
| 9 | **Catálogos** (`TargetInfo/*.dat`, `HMI IO/*_PDL.dll`) | Ficheros de datos | No | Listar modelos HMI y drivers disponibles |
| 10 | Type libraries internas (plugins del framework) | COM in-proc | Sí, dentro del proceso | ⚠ No utilizables desde fuera; sólo referencia |

> Nota (sexta tanda): el MCP ya no usa la fila 1 para los proyectos abiertos. Sólo la usa para `LoadXMLCommand` (§12.6, §12.8). Los proyectos se manejan con una instancia normal de Vijeo (§12.10).

> Obsoleto desde la séptima tanda: ya no la usa nada, y la fila 2 no sirve para importar sin COM (§12.11).

---

## 1. API de automatización COM `VijeoFrame` ⭐

**TypeLib** `{8B45E955-75FD-46A4-9733-4E6C1285D046}` v1.0 — `Vijeo-Frame.tlb`.
Es la API que usa SoMachine / Machine Expert para pilotar Vijeo-Designer. Está registrada como servidor **out-of-process**, por lo que un proceso Rust de **64 bits** puede consumirla por `IDispatch` sin problemas de bitness (COM hace el marshalling).

### 1.1 ProgIDs registrados

| ProgID | CLSID | Notas |
|---|---|---|
| `VijeoFrame.VijeoDesigner` (`.1`) | `{0CA1AFB8-4444-474D-AA1C-45775585CAE6}` | API v1 |
| `VijeoFrame.VijeoDesigner2` (`.1`) | `{C845ACCE-C797-46C0-AA68-F386589443DA}` | **API v2 (preferente)** |
| `VijeoFrame.VijeoDesigner_Internal` / `_Internal2` | `{C3206E41-…}` / `{02B4C31C-…}` | Misma firma; uso interno de SoMachine |
| `VijeoFrame.VJDProject[2]`, `VJDTarget[2]`, `VJDPanel[2]` (+ `_Internal[2]`) | ver typelib | Objetos devueltos por la jerarquía; no se crean directamente |

> La existencia de interfaces **v1 y v2** en paralelo (`IVijeoDesigner` / `IVijeoDesigner2`, etc.) es justo el patrón que el MCP debe abstraer con traits: una versión futura puede añadir `IVijeoDesigner3` o cambiar ProgIDs.

### 1.2 Jerarquía de objetos

```
IVijeoDesigner2                          (raíz, CoCreateInstance)
 └─ IVJDProject2                         (NewProject / OpenProject)
     ├─ IVJDTarget2                      (NewTarget / GetTarget)
     │   └─ IVJDPanel2                   (GetPanel / ImportPanel)
     └─ IMSDProject2  ◄── callback       (Advise: lo implementa el cliente, p.ej. SoMachine)
         └─ IMSDControllerDevice2        (PLC: símbolos, aplicaciones)
```

### 1.3 Métodos — `IVijeoDesigner2`

| DISPID | Firma | Descripción / herramienta MCP candidata |
|---|---|---|
| 1 | `void Exit()` | Cerrar Vijeo-Designer → `vijeo_exit` |
| 2 | `IVJDProject2* NewProject(BSTR name, BSTR hmi_name, BSTR hmi_type)` | Crear proyecto → `project_new` |
| 3 | `IVJDProject2* OpenProject(BSTR fullPathname)` | Abrir proyecto → `project_open` |
| 4 | `I4 RetrieveSourceProject(BSTR targetAddress, BSTR fullPathname)` | Subir el proyecto fuente desde un HMI → `project_upload_from_target` |
| 5 | `void SetMode(BSTR mode)` | Modo de operación (p.ej. modo MSD/SoMachine) |
| 6 | `I4 IsAvailable()` | Health-check → `vijeo_status` |
| 7 | `I4 ChangeUILanguage(BSTR language)` | Idioma de la UI |
| 8 | `BSTR GetLastErrorMessage()` | Último error (sólo v2) |
| 9 | `BSTR GenericCall(BSTR xml)` | Llamada genérica extensible (ver §1.8) (sólo v2) |

`IVijeoDesigner` (v1) = DISPIDs 1–7.

### 1.4 Métodos — `IVJDProject2`

| DISPID | Firma | Herramienta MCP candidata |
|---|---|---|
| 1 | `void Advise(IMSDProject2*)` | Registrar callback de controladores (avanzado) |
| 2 | `void Unadvise()` | — |
| 3 | `void Save(BSTR fullPathname)` | `project_save` / save-as |
| 4 | `void Close()` | `project_close` |
| 5 | `I4 Build(out I4* count_warnings)` | `project_build` (devuelve nº errores + warnings) |
| 6 | `I4 IsDirty()` | `project_status` |
| 7 | `IVJDTarget2* NewTarget(BSTR name, BSTR hmi_type)` | `target_new` |
| 8 | `VARIANT GetTargetNames()` | `target_list` (SAFEARRAY de BSTR) |
| 9 | `IVJDTarget2* GetTarget(BSTR name)` | — |
| 10 | `BSTR Name` (get/put) | `project_rename` |
| 11 | `I4 Download(VARIANT* targetAddresses)` | `project_download` (varios targets) |
| 12 | `void ShowMessages()` | Mostrar ventana de mensajes |
| 13 | `void Notify(I4 category, VARIANT data)` | Notificaciones |
| 14 | `void GenerateAllReports(BSTR fullPathname)` | `project_reports` (v2) |
| 15 | `void GetCrossReferences(BSTR fullPathname)` | `project_cross_references` (v2) |
| 16 | `void GotoContext(BSTR referenceId)` | Navegar en la UI a un elemento (v2) |
| 17 | `BSTR GetLastErrorMessage()` | (v2) |
| 18 | `BSTR GenericCall(BSTR xml)` | (v2) |

### 1.5 Métodos — `IVJDTarget2`

| DISPID | Firma | Herramienta MCP candidata |
|---|---|---|
| 1 | `void Delete()` | `target_delete` |
| 2 | `BSTR Name` (get/put) | `target_rename` |
| 3 | `IVJDPanel2* ImportPanel(BSTR fullPathname, BSTR path)` | `panel_import` |
| 4 | `VARIANT GetPanelNames()` | `panel_list` |
| 5 | `IVJDPanel2* GetPanel(BSTR name)` | — |
| 6 | `I4 Build(out I4* count_warnings)` | `target_build` |
| 7 | `I4 Download(BSTR targetAddress, BSTR fullPathname)` | `target_download` (a IP o a carpeta/CF) |
| 8 | `void Show()` | Activar target en la UI |
| 9 | `BSTR GetBindingOfVariable(BSTR variableName)` | `variable_get_binding` |
| 10 | `BSTR Type()` | Modelo HMI del target |
| 11 | `BSTR GetLastErrorMessage()` | (v2) |
| 12 | `BSTR GenericCall(BSTR xml)` | (v2) — comando `LookupTag` |

### 1.6 Métodos — `IVJDPanel2`

| DISPID | Firma | Herramienta MCP candidata |
|---|---|---|
| 1 | `BSTR Name` (get/put) | `panel_rename` |
| 2 | `void Export(BSTR fullPathname)` | `panel_export` |
| 3 | `VARIANT GetVariables()` | `panel_variables` |
| 4 | `I4 Bind(BSTR template_variable, IMSDControllerDevice2*, BSTR newSymbol)` | Re-binding de plantillas a símbolos de PLC |
| 5 | `void FinalizeBinding()` | — |
| 6 | `BSTR GetLastErrorMessage()` | (v2) |
| 7 | `BSTR GenericCall(BSTR xml)` | (v2) |

### 1.7 Interfaces de callback (las implementa el cliente)

`IMSDProject2`: `Notify`, `GetControllerDeviceNames`, `GetControllerDevice`, `GetLastErrorMessage`, `GenericCall`.
`IMSDControllerDevice2`: `Name`, `Address`, `GetSymbols`, `AddSymbol`, `DeleteSymbol`, `RenameSymbol`, `GetApplicationNames`, `GetLogicBinary`, `GetVariableContainers`, `GetTypeNames`, `GetSymbolFiles`, `AddSymbolsFromFile`, `GetLastErrorMessage`, `GenericCall`.

Sirven para que Vijeo **consulte símbolos de un PLC** al editor anfitrión (SoMachine). El MCP podría implementarlas (servidor COM en Rust) para exponer un PLC "virtual" desde un CSV/XML → fase avanzada; no imprescindible.

Vijeo invoca de vuelta (`GenericCall` saliente) con estos comandos: **`GetCoDeSysAddress`**, **`GetConnectorsOf`** (dispositivos/conectores/gateway).

### 1.8 Protocolo `GenericCall` (validado, ver §12)

**Entrante** (cliente → Vijeo): el XML tiene como **raíz el nombre del comando** y los parámetros como elementos hijo (XPath interno `/{Comando}/{Param}`). Sin declaración XML ni envoltorio:

```xml
<LookupTagCommand><TagName>MyController.Alarms.xAlarmEStop_1</TagName></LookupTagCommand>
```

| Comando | Objeto que lo atiende | Petición | Respuesta observada |
|---|---|---|---|
| `LookupTagCommand` | **`IVJDTarget2`** | `<TagName>` | `<Result><Exists>true</Exists><Type>BOOL</Type></Result>` / `<Result><Exists>false</Exists><Type></Type></Result>` |
| `LoadXMLCommand` | **`IVijeoDesigner2`** | `<XMLFile>`, `<ErrorFile>` | `<Result/>` siempre; los errores van a `ErrorFile`. ⚠ Espera formato **privado** (`VijeoDesignerProject`); un XML inválido corrompe la base de tags (§12.6) |

- Comando desconocido, formato incorrecto o enviado al objeto equivocado → `UNHANDLED COMMAND`.
- `IVJDProject2.GenericCall` devuelve `null` para todo lo probado (sin handler en 6.3).

**Saliente** (Vijeo → cliente que implementa `IMSDProject2`): éste sí usa el envoltorio `<Generic Version="1"><Call><{Cmd}>…</{Cmd}></Call></Generic>` y lee `/Generic/Result/{campo}` (comandos `GetCoDeSysAddress`, `GetConnectorsOf`).

**Este mecanismo es el punto natural de extensión por versión**: una versión futura puede añadir comandos sin cambiar la typelib, así que el MCP debe descubrirlos/negociarlos por versión (`UNHANDLED COMMAND` = no soportado).

---

## 2. Import/Export XML de proyecto ⭐

- `Vijeo-Frame\XML\public.xsd` (412 KB) — esquema **público** de import/export. `private.xsd` + transformaciones `public_to_private.dat` / `private_to_public.dat` (XSLT) para convertir al formato interno.
- Registro `Vijeo-Frame\Services\…\SupportsXMLImportExport` y `SupportVXDExport` indican qué servicios participan.

Elementos raíz del esquema público (lo que se puede importar/exportar):

`Project`, `Target`, `DataTypes`, `ArrayType`, `Variables`, `Variable`, `Alarm`, `Actions`, `Script`, `Condition`, `State`, `VisibilityAnimation`, `DataScaling`, `InputRange`, `Format`, `Font`, `Language`, `LanguageFont`, `Panel`, `PanelFolder`, `MasterPanel`, `MasterPanelFolder`, `DefinitionPanel`, `DefinitionPanelFolder`, `Folder`, `FunctionKey`, `Files`, `UserDatabase`, `BatchPrinting`, `SaveToFile`, `Reference`.

Herramientas MCP candidatas: `xml_validate` (contra `public.xsd`, sin Vijeo), `xml_generate_variables` (desde CSV/tabla de PLC), `xml_import` (vía `GenericCall LoadXML`), `panel_export`.

---

## 3. CLI `DataTransferTool.exe` (runtime ↔ PC, sólo Ethernet)

```
DataTransferTool -ip <host>:6001 (-user <u> -pwd <p> | -ua) (-get | -put)
                 (-primary | -secondary) -remotedatafolder <Log|Recipe|Alarm|…>
                 -localfolder "<ruta>" [-r] [-delete] <patrón>
```

- **Get**: recetas `.rcp`, data logging `.dat`, alarmas `.csv`, vídeo `.mpeg`, capturas `.jpeg`, backup `.vdz`.
- **Put**: `.rcp`, `.csv`, `.txt`.
- Retorno `0` = OK, `>0` = error.

Herramientas: `runtime_get_files`, `runtime_put_recipes`, `runtime_backup_project`.

## 4. CLI `DataConverter.exe`

```
DataConverter -i "<entrada>" -o "<salida>" [-s] [-b] [-cw] [-cf] [-ch] [-ct|-ct0|-ct1] [-rp "<rcp>"] [-ccr] [-v]
```

`.rcp`/`.dat` → `.csv`/`.txt`; y `-ccr` para `.csv`/`.txt` → `.rcp` (validando contra `-rp`). Herramientas: `data_convert_logs`, `recipe_csv_to_rcp`.

## 5. CLI `RuntimeLoader.exe` (PDPLoad)

```
PDPLoad.exe [/T:{GP2000|PSP|GP3000|iPC Series|Target_XBT_OT|Target_XBTGT1005|Target_HMISTO|Target_HMISTU|Target_HMISCU}]
            [/D:{SERIAL"Com:Baud" | ETHER"IP[|mask][@gw][:Port][;DataPort]" | USB | MSD}]
            [/E:{Y|N}] [/V] [/F] [/I] [/DEBUG] {/Core:{Up|Down} "fichero"} {/USER:"u"} {/PSWD:"p"} [/Som]
```

Actualiza runtime/core en el target. Herramienta (con confirmación explícita, es destructiva): `runtime_install`.

## 6. Otros ejecutables

| Ejecutable | Uso |
|---|---|
| `Vijeo-Frame.exe /import "<fichero.vdz>"` | Asociación de `.vdz`; abre/importa proyecto |
| `SimulationTool.exe`, `Koohi.exe` (HMI Runtime), `KoohiKill.exe` | Simulación local del runtime |
| `DataManager.exe` | GUI de transferencia (equivalente gráfico de §3/§4) |
| `Vijeo-Runtime\Tools\IPCAppLoader.exe`, `VJDRTSD.exe` | Carga de aplicación en iPC |
| `VijeoLanguageSelection.exe` | Idioma de instalación |
| `CoDeSysControl.exe` | Control runtime embebido (CODESYS 3.5.16.10) |

Extensiones registradas: `.vdz` → `Vijeo-Designer.import`, `.VJDDataTypes`, `.VJDPrintDef`.

---

## 7. API de scripts del runtime (Java) — para generar/validar scripts

Los scripts de usuario del HMI son Java compilado (`jikes113.exe`, `BuildJava.dll`) contra `Vijeo-Runtime\Public\gpClasses`. La ayuda (`Scripts.chm`) documenta la API pública:

| Objeto | Métodos |
|---|---|
| **Variable** (`Tag`, `IntTag`, `FloatTag`) | `getIntValue`, `getFloatValue`, `getStringValue`, `write`, `toString`, `X(bit)`, `XW`, `getMax`, `getMin`, `setMinMaxValues`, `enableScaling`, `isDataQualityGood`, `setLabel`, `ack`, `add`, `flushToStorage`; bloques: `getIntValues`, `getFloatValues`, `getSize`, `sendData` |
| **Sys** | `getInfoString`, `getInfoInt`, `beep`, `exitRuntime`, `login`, `logout`, `validateUser`, `createProcess`, `removeProcess`, `setDate`, `setTime`, `setTargetIPAddress`, `isPlatform`, `dataLoggingFlushAll`, `clearAllDataLoggingInMemory`, `clearBiometricSwitch`, `getLastFieldRequested`, `getLastFieldUpdated` |
| **PopupWindow** | `open`, `close`, `show`, `move`, `moveOffset`, `changePanel`, `previousPanel`, `getPanelID`, `getStatus` |
| **AlarmSummary** | `ack`, `ackAll`, `clear`, `clearAll`, `delete`, `deleteAll`, `cursorUp/Down/On/Off`, `pageUp/Down`, `moveTop/Bottom`, `jumpTo`, `showSubDisplay` |
| **AlarmGroup** | `ackAll`, `getCurrent`, `getSum`, `clearSum` |
| **Equipment** | `openConfig`, `getProperty`, `setProperty`, `commitConfig` (direccionamiento en runtime) |
| **Script Driver (serie)** | `read`, `write` |
| **DiagBuffer** | `getStatusWords`, `decodeStatusWords`, `updateStatusWords`, `getDfbName` |
| **Sound** / **videoDisplay** | `play`, `loop`, `stop` / `play`, `pause`, `stop`, `record`, `capture`, `rewind`, `fastForward`, `setCurFramePercent`, `switchWindows` |
| **Text File**, **String**, **StringBuffer**, **Math** | E/S de texto; utilidades estándar Java; `Math.*` (20 funciones) |

Tipos de disparadores (`userScripts`): Startup, Shutdown, Periodic, Daily, Weekly, Scheduled, DataChange, Key/HoldKey, WhenTouch, OnOpen/OnClose (panel), AlarmGroup, VariableAlarm, Error.

`MathMethodDefine.xml` es la definición que usa el editor para autocompletado/validación (`Math`, `Tag`, `IntTag`, `FloatTag`, `Sys`) — útil como fuente machine-readable para una herramienta `script_lint`.

---

## 8. Lectores de tablas de símbolos (formatos importables)

`symbAsc` (ASC), `symbcsv` (CSV), `symbxml` (XML), `symbstu` (Unity Pro `.stu`/XVM), `symbconcn`/`symbcpts` (Concept), `symbpl7` (PL7), `symbmods` (Modsoft), `symbtwido` (Twido). Orquestados por `SymbolTables.dll`. Indican qué formatos acepta "Importar variables" — el MCP puede generar CSV/XML compatibles.

## 9. Catálogos de datos (lectura sin Vijeo)

- **Modelos HMI**: `Vijeo-Frame\TargetInfo\Models_*.dat` + `Targets.dat` (HMIGTO, HMIGTU, HMIGTUX, HMISTU, HMISCU, HMIGK, XBTGC, XBTGH, XBT_OT, iPC, Tablets…). Necesario para validar `hmi_type` en `NewProject`/`NewTarget`.
- **Drivers de comunicación** (`Vijeo-Frame\HMI IO\*_PDL.dll`, 64): Schneider (Modbus SIO/TCP/Slave/Plus, Uni-Telway, XWay, FIPIO/FIPWAY, Jbus, Zelio, PacDrive, OMS), Siemens (MPI, ISOonTCP, TCP, TIA, RK512, 505), Rockwell (EtherNet/IP, DF1, DH485), Mitsubishi (Melsec A/Q/QnU/L/iQ-R/FX), Omron (FINS/Sysmac), GE (SNPX, SRTP), Panasonic, Toyopuc, ABB Comli, Emerson ROC, genéricos (EtherNet/IP explícito, barcode, serial, referencia).
- **Plantillas de proyecto**: `ProjectCfg{AT,CE,GP,GP3000}.ini`.
- **Toolchest** (librería de objetos): Browser, Configuration, DateTime, Diagnostic, Graph, Image Library, Keypad, Lamps, Meter, Recipe, Security, SoMachine, Templates, TextDisplay…

Herramientas: `catalog_list_hmi_models`, `catalog_list_drivers`.

## 10. Type libraries internas (no usables externamente)

90 typelibs en total. Las de `Vijeo-Frame.exe` del §1 son las únicas expuestas como servidor. El resto son componentes in-proc del framework "TIS/FrameworX" que sólo funcionan **dentro** del proceso de Vijeo (`ITISFrameWorkSink`, `ITISDocument`, `ITISTarget`, `ISwxStorage`…):

| DLL | Interfaces relevantes |
|---|---|
| `TaylorWBCOM.dll` | `ITISWorkbenchDoc[2,3]`, `ITISProjectInfo`, `IINDE_BaseTarget`, `INDE_DataSharing`, `INDE_RuntimeMode`, `INDE_ProWeb` |
| `MainDocument.dll` | `ITIS_PWDocument`, `ITargetConfig`, `IVariableUI` |
| `EditorDocument.dll` | `ITIS_GE_Editor`, `ITIS_GOb_*` (objetos gráficos), `ITIS_PW_WindowList` |
| `TISIOManagers.dll` | `ITISMMIIO`, `ITISDriver`, `ITISProtocol`, `ITISDevice`, `ITISIOConfiguration` |
| `INDEAlarmGroup.dll` / `RecipeGroups.dll` / `DataLogging.dll` | Alarmas, recetas, logging |
| `ScriptEngine.dll` | `IScriptValidate`, `I_PDSL_Keywords` (validador de scripts) |
| `ExprMgr.dll` | `ITIS_PW_Expression_Mgr`, `ITIS_PWTagCreator` |
| `SwxLoc.dll` | `ISwxCompoundFileManager` — el proyecto interno es un **Compound File (OLE Structured Storage)** |
| Objetos gráficos (`Switch`, `Lamp`, `Meter`, `BarGraph`, `TrendGraph`, `Text`, …) | Propiedades de cada objeto — útil como referencia para generar XML de paneles |

Se usan sólo como **referencia de propiedades** para construir XML de `public.xsd`.

---

## 11. Implicaciones para el diseño en Rust (multi-versión)

1. **Detección de versión**: leer `HKLM\SOFTWARE\WOW6432Node\Schneider Electric\Vijeo-Designer*\<NN.NN>\Path` + ProgIDs disponibles (`VijeoFrame.VijeoDesigner2` vs `…VijeoDesigner`) + `FileVersion` de `Vijeo-Frame.exe`.
2. **Trait por capacidad, no por versión**, p. ej.:
   - `trait VijeoBackend { fn version(); fn capabilities() -> CapabilitySet; fn open_project(); … }`
   - `trait ProjectOps`, `TargetOps`, `PanelOps`, `RuntimeTransfer`, `XmlImport`, `Catalog`.
   - Implementaciones: `Vd63ComBackend` (usa v2 con fallback a v1), `CliTools` (DataTransfer/DataConverter, independientes de COM), `XmlSchema` (cargado desde la ruta de instalación, no embebido).
   - Las herramientas MCP se registran según `capabilities()` → una versión que no tenga `GenerateAllReports` simplemente no expone `project_reports`.
3. **COM desde Rust**: crate `windows` (`Win32::System::Com`, `IDispatch::Invoke` con late binding por nombre vía `GetIDsOfNames`), un **hilo STA dedicado** con canal de comandos (el servidor es STA/MFC; `rmcp` es async/tokio).
4. **Seguridad**: `Download`, `RuntimeLoader`, `DataTransferTool -put/-delete`, `Exit`, `Target.Delete` son acciones con efectos en máquinas reales → marcar como destructivas (anotaciones MCP `destructiveHint`) y requerir confirmación.
5. **Toolchain**: en esta VM **no hay Rust instalado** (`cargo` no encontrado). Hace falta `rustup` con target `x86_64-pc-windows-msvc` (o `i686-…` si alguna vez se quisiera cargar DLLs in-proc, que no es el caso para el COM out-of-proc).

---

## 12. Validación empírica de la API COM (2026-11-01)

Probado con `tools/com_probe.ps1` (PowerShell 5.1 x64 → `Vijeo-Frame.exe` x86 out-of-proc) sobre **copias** en el scratchpad: el ejemplo de Machine Expert Basic `Compact_Hardwired_M221.vdz` y el sample `Manual_GXU`. Los proyectos reales de `Vijeo-Manager` no se han abierto ni modificado. **No se han probado en vivo** `Download` (de target y de proyecto) ni `RetrieveSourceProject`, porque actúan sobre HMIs reales; su comportamiento está descrito a partir del descompilado (§12.6). `ImportPanel` y `Target.Delete` sí se han probado, sobre copias.

### 12.1 Ciclo de vida

| Aspecto | Resultado |
|---|---|
| `CoCreateInstance("VijeoFrame.VijeoDesigner2")` desde un proceso x64 | ✅ ~200 ms. Lanza `Vijeo-Frame.exe` **con ventana visible** (título `Vijeo-Frame`) |
| Licencia / registro | No apareció ningún diálogo modal en esta VM |
| `IsAvailable()` | Devuelve `0` durante ~2,6–3,7 s y después `1`. **Antes de que pase a 1, cualquier llamada devuelve `E_FAIL`** → el backend debe hacer polling con timeout |
| `Exit()` | ✅ cierra el proceso. También se cierra solo al liberar la última referencia COM |
| Efectos colaterales | En cada arranque reescribe `Vijeo-Manager\Info.IFM` (compound file con `FolderInfo`, inocuo). El proyecto abierto o creado vive en `Vijeo-Manager\M5DTemPHolder<PID en hex>\` (formato `"%s%X"`, verificado: PID 5468 → `M5DTemPHolder155C`) y se borra al `Close()`. **Si se hace `Exit()` sin `Close()`, o Vijeo cae, la carpeta se queda**; Vijeo borra todas las `M5DTemPHolder*` en el siguiente arranque (`FUN_00490f30` → `ForceDeleteFolder`, verificado dos veces) |
| Varios proyectos | **Una instancia = un proyecto.** En la misma instancia, un 2º `OpenProject` devuelve el proyecto ya abierto y `NewProject` con uno abierto abre el modal «Unable to rename project or folder…» y deja inválidos los abiertos. Cada `CoCreateInstance` lanza **un proceso nuevo**: dos instancias trabajan en paralelo con proyectos aislados (verificado, `probe_multi2.log`) |
| `GetLastErrorMessage()` | **Stub** en las tres clases (Designer/Project/Target): `*out = NULL; return S_OK` (§12.6). No hay forma de obtener mensajes de Vijeo; el MCP genera los suyos |

> Nota (sexta tanda): `M5DTemPHolder`, `Info.IFM` y el arranque con `-Embedding` son efectos de COM. Las instancias normales (§12.10) guardan su proyecto en `Vijeo-Manager\MCP_TMP_*`.

### 12.2 Proyecto / target / panel

> Nota (sexta tanda): el MCP ya no llama a estos métodos para los proyectos abiertos, sólo a `LoadXMLCommand`. Esto incluye `NewProject`, `NewTarget`, `Save`, `Build`, `GenerateAllReports` y `GetCrossReferences`. Su sustitución está en §12.10; los resultados siguen valiendo como referencia.

> Obsoleto desde la séptima tanda: ya no se llama ni a estos métodos ni a `LoadXMLCommand` (§12.11).

| Llamada | Resultado |
|---|---|
| `NewProject(name, hmi_name, hmi_type)` | ✅ pero **sólo usa `name`**: `hmi_name`/`hmi_type` se descartan (descompilado, §12.6) y el proyecto nace sin targets. **Solución verificada**: `NewProject(name,'','')` + `NewTarget(nombre, 'Model_…')` + `Save` → al reabrir, el target persiste. ⚠ Si `name` tiene caracteres de `\/:*?"<>\|#,;.` o no-ASCII, Vijeo abre un **`MessageBox` modal** y la llamada COM se bloquea → validar antes |
| `OpenProject(ruta.vdz)` | ✅ ~2–3,5 s |
| `OpenProject(ruta.SwxCF)` | ❌ `E_FAIL` tras ~8 s → **la API sólo trabaja con `.vdz`** |
| `Project.Name`, `IsDirty()` | ✅ (`IsDirty` pasa a 1 tras `NewTarget`) |
| `GetTargetNames()` | ✅ SAFEARRAY de BSTR (`HMISTU855`); `VT_EMPTY` si no hay targets |
| `NewTarget(name, hmi_type)` | ✅ con el **código interno** del catálogo (`Model_HMIS5T`, `Model_HMIGTO5310`…); los modelos modulares exigen `"<modelo>:<pantalla>"` (`Model_HMIG3U:HMIDT351 (800x480)`). Sólo **29 de 58** modelos son creables por COM: la API aplica el filtro del modo SoMachine (§12.7). Un código no válido devuelve **`null` sin error**. Tabla completa: `docs/reference/vd-6.3/models.md` |
| `Target.Type()` | ✅ devuelve el código interno (`Model_HMIS5T`) → es la fuente para construir la tabla nombre comercial ↔ código |
| `Target.GetPanelNames()` | ✅ SAFEARRAY con rutas prefijadas: `BP:\Start`, `BP:\Alarms`… (`BP:` = Base Panel) |
| `Target.GetPanel(name)` | ❌ **Roto en 6.3 por construcción** (ver §12.5): la implementación devuelve siempre `E_FAIL`, incluso cuando encuentra el panel |
| `Target.ImportPanel(file, path)` | **Stub** (v1 y v2): `*ppPanel = NULL; return S_OK`. Verificado en vivo: devuelve `null` |
| `Target.GetBindingOfVariable(var)` | **Stub**: la implementación (`004F2230`, slot 16 de la vtable) es `return 0` → siempre `null` |
| `Target.Delete()` | ✅ verificado sobre copia: el target desaparece de `GetTargetNames`; el objeto queda inválido (`Name` → `null`) |
| `NewTarget` (límite) | El descompilado muestra un `AfxMessageBox` **modal** al superar 32 targets → el MCP debe comprobar `GetTargetNames().len() < 32` antes |
| `Target.Build(out warnings)` | ✅ `ret=0 warnings=2`, ~7 s. `ret` = nº de errores |
| `Project.GenerateAllReports(dir)` | ✅ ~1,3 s. Genera `ReportNames.xml` y `<Target>\{Panels,Actions,Variables}\index.html` + `.emf` de cada panel. **El informe de variables lista nombre, tipo, origen (Internal/External), dirección (`%M203`) y descripción** → fuente útil para un tool `variable_list` |
| `Project.GetCrossReferences(file)` | ✅ pero devolvió `<VijeoDesignerXRefReport Version="1"/>` vacío en este proyecto |
| `Project.Save(ruta.vdz)` | ✅ ZIP con `<Proyecto>.SwxCF` (OLE Structured Storage) + `DataFilesBackup/` + `SymbolTables/` |
| `SetMode(mode)` | Sólo acepta `"OMS V1.0"`, `"MSD V1.0"` o `"MSD V<x.y>"` (por eso `'MSD'` daba `E_FAIL`). Activa el **modo integrado en SoMachine** y en ese camino puede abrir el `MessageBox` modal *"Vijeo-Designer is ready. Press OK to continue"* → **el MCP no lo llama** |
| `Project.Close()` | ✅; un segundo `Close()` devuelve `E_FAIL` (flag interno `+3Eh`) |
| `GenericCall(LoadXMLCommand)` | Ver §12.6: el XML debe tener la raíz **privada** `VijeoDesignerProject`; con un XML válido no produce cambios visibles fuera del modo SoMachine; con un XML inválido **deja corrupta la base de tags y la siguiente llamada tumba Vijeo** (AV en `DDTTagDatabase.dll`) |

### 12.3 Implicaciones para el backend Rust

> Nota (sexta tanda): el backend ya no abre, crea, guarda, compila ni busca variables por COM (§12.10). Los puntos 1, 2 y 8 valen sólo para `LoadXMLCommand`. En el punto 6, `LookupTagCommand` ya no se usa. Los puntos 5 y 7 se cumplen ahora con la instancia normal.

> Obsoleto desde la séptima tanda: el MCP ya no usa COM, así que los puntos 1, 2, 6 y 8 ya no aplican (§12.11).

1. **Hilo STA dedicado** que hace `CoCreateInstance`, espera `IsAvailable()==1` (timeout configurable, por defecto 30 s) y serializa las llamadas. El servidor es MFC/STA y la UI es visible.
2. **Validar los resultados nulos**: `NewTarget` → `null` = modelo no válido. `GetLastErrorMessage` no informa de nada.
3. **Tabla de modelos por versión** (`Model_HMIS5T` ↔ HMISTU855…). Se puede construir sondeando `NewTarget` con candidatos y leyendo `Type()` en un proyecto temporal, y debe ir en el módulo de versión 6.3.
4. **Trabajar siempre sobre `.vdz`**. `project_open` acepta `.vdz`. Para editar un proyecto de `Vijeo-Manager` habría que exportarlo antes (no expuesto por la API; queda pendiente).
5. **Limpieza**: al cerrar el MCP o ante un panic, **siempre `Close()` antes de `Exit()`**. Si queda `M5DTemPHolder<PID>` de un PID que ya no existe, informar: Vijeo la borra solo en el siguiente arranque, así que el MCP no borra nada.
6. `GenericCall` se envía al objeto correcto: `LookupTagCommand` → Target, `LoadXMLCommand` → Designer.
7. **Evitar diálogos modales** (bloquean el hilo STA sin timeout posible desde COM): validar nombres de proyecto (sin `\/:*?"<>|#,;.`, sólo ASCII), no superar 32 targets, `ErrorFile` de `LoadXMLCommand` siempre en ruta escribible (si no, `"Can't open LOG file"` modal), no llamar a `SetMode`. Como red de seguridad, un **watchdog** que detecte una ventana `#32770` del PID de Vijeo-Frame y aborte la llamada (matar el proceso y marcar el backend como caído).
8. **Tras un error de `LoadXMLCommand`** (el `ErrorFile` contiene `ERROR`), reiniciar Vijeo antes de cualquier otra llamada.

### 12.4 Pendientes

- [x] ~~`GetPanel` / `Panel.Export` / `GetVariables`~~: no hay precondición que cumplir; `GetPanel` está roto en 6.3 (§12.5). Las operaciones a nivel de panel deben salir por otra vía (informes HTML, XML de `public.xsd`, lectura del `.SwxCF`).
- [x] ~~`GetLastErrorMessage`~~: stub, el MCP genera sus errores (§12.6).
- [x] ~~`NewProject` sin targets~~: `NewProject` + `NewTarget(Model_…)` verificado.
- [x] ~~`SetMode`, `GetBindingOfVariable`~~: valores de `SetMode` identificados (modo SoMachine, no se usa); `GetBindingOfVariable` es un stub.
- [x] ~~`M5DTemPHolder` huérfana~~: causa (`Exit` sin `Close`, o cuelgue) y limpieza automática de Vijeo verificadas.
- [x] ~~`ImportPanel`, `Target.Delete`~~: probados sobre copia.
- [x] ~~`LoadXMLCommand` con XML real~~: probado (§12.6). Formato privado aceptado sin efecto visible; formato público rompe Vijeo. En 6.3 se declara no soportado.
- [x] ~~Mapeo completo de modelos comerciales → `Model_*`~~: catálogos descifrados (`tools/vjd-dat`) y tabla validada en vivo (§12.7).
- [ ] `Download` / `RetrieveSourceProject`: analizados estáticamente (§12.6); falta probarlos contra un HMI o simulador de pruebas que se nos indique explícitamente.
- [ ] Cómo abrir o exportar los proyectos del workspace `Vijeo-Manager` (`.SwxCF`) vía API.

### 12.5 Análisis estático de `GetPanel` (Ghidra 12.2)

Herramientas: `tools/ghidra_headless.ps1` (Ghidra headless con JDK 25 y entorno aislado en `C:\tools\ghidra_home`) + `tools/ghidra_scripts/DecompileAt.java`. Proyecto analizado: `C:\tools\ghidra_projects\vd63`. Pseudocódigo guardado en `docs/reference/vd-6.3/decompiled/`.

**Cadena de llamadas** (`Vijeo-Frame.exe` 6.3.0.103, sin ASLR, base `0x400000`):

```
CVJDTarget2::GetPanel        004F1900  wrapper: comprueba app/objeto vivo → inner->vtbl[+30h]
 └ CVJDTarget::GetPanel      004F0BB0  wrapper idéntico → inner->vtbl[+30h]
    └ CVJDTarget_Internal::GetPanel   004F4100   (variante v2: CVJDTarget_Internal2::GetPanel 004F7370)
```

Sólo si la función interna devuelve `S_OK` el wrapper crea el `IVJDPanel2` externo; en otro caso propaga el HRESULT.

**Lógica de `CVJDTarget_Internal::GetPanel(name, ppPanel)`**:

1. `ppPanel == NULL` → `E_POINTER`. Si no, `*ppPanel = NULL`.
2. Prefijo obligatorio: `Format("%s%s", "BP:", "\")` y `CompareNoCase(name.Mid(0, 4), "BP:\")`. Si no coincide → `E_FAIL`.
3. `FUN_004f2cb0`: obtiene, por el `targetID` del objeto, el documento del target (misma comprobación `app+0x2E20` que `NewTarget`).
4. Busca el documento `ProcessWindowDoc` (CLSID `{8545EB2D-48D3-48D8-A7C8-3D36B424DDD1}`, MainDocument.dll), QI a `ITIS_PW_DocInternals` `{FA185ABE-…}` y `GetChild(0)`; QI del hijo a la interfaz privada `{06493F48-0EC8-4170-AE58-46863305724E}` (presente en casi todas las DLL, sin typelib) y llama a su método `+78h(5, child, &container)`.
5. `FUN_004f26d0(container, name.Mid(4), &this->m_panel)`: búsqueda **recursiva** del panel. Compara el nombre de cada hijo sin mayúsculas y, si la ruta contiene `\`, desciende a la subcarpeta (`Carpeta\Panel`). **El resultado se guarda en un miembro del objeto (`this+1Ch`), no en `*ppPanel`.**
6. **Epílogo**: todos los caminos terminan en `004F44DE: mov eax, 80004005h`, el de éxito incluido. Los únicos otros valores de retorno son los dos `E_POINTER` (`004F418D`, `004F41D6`). En la variante v2 (`004F7370`) pasa lo mismo: `E_POINTER` en `004F73FD`/`004F7446` y `E_FAIL` en `004F774E` como único retorno restante.

**Conclusión**: en 6.3 `IVJDTarget[2].GetPanel` devuelve siempre `E_FAIL` y nunca rellena `*ppPanel`. Por tanto `IVJDPanel2` (`Export`, `GetVariables`, `Bind`, `GenericCall`) **no es alcanzable** desde COM. `ImportPanel` (`004F3AC0`) es un stub (`*ppPanel = NULL; return S_OK`). El formato de nombre correcto, por si una versión futura lo arregla, es el que devuelve `GetPanelNames`: `BP:\<Panel>` o `BP:\<Carpeta>\<Panel>`.

**Implicaciones para el MCP**:

- El backend 6.3 declara la capacidad `panel_object = false`. Las herramientas `panel_export`/`panel_variables` no se registran para 6.3 (o se implementan por otra vía).
- Alternativas para datos de panel en 6.3: `GenerateAllReports` (HTML por panel con imagen `.emf`) y, a más largo plazo, leer el `.SwxCF` (OLE Compound File) directamente desde Rust. `LoadXMLCommand` **no** sirve en 6.3 (§12.6).
- Para versiones futuras: repetir este análisis con `ghidra_headless.ps1` sobre el nuevo `Vijeo-Frame.exe` (buscar la cadena `CVJDTarget_Internal::GetPanel` y revisar el epílogo) antes de habilitar la capacidad.

### 12.6 Resolución de los demás pendientes (Ghidra + validación en vivo)

Scripts usados: `DecompileByString.java` (localiza los métodos COM por su cadena de traza `CClase::Metodo`, sin depender de direcciones), `DecompileVtable.java` (métodos de una clase por su vtable RTTI) y `DecompileAt.java`. Logs de las pruebas en vivo: `docs/reference/vd-6.3/com_probe_logs/`.

| Método (`*_Internal2`) | Dirección | Qué hace realmente | Consecuencia para el MCP |
|---|---|---|---|
| `GetLastErrorMessage` (Designer/Project/Target) | `004DDC90` / `004F0330` / `004F8630` | `*out = NULL; return S_OK` | Errores propios del MCP |
| `NewProject(name, hmi_name, hmi_type)` | `004DC8B0` | Valida `name` con `SpanExcluding` (`\/:*?"<>\|#,;.`) e `IsExtASCII`; si falla → `MessageBox` modal + `E_INVALIDARG`. `hmi_name`/`hmi_type` no se usan | Validar `name` antes; añadir target con `NewTarget` |
| `NewTarget` | `004E8BD0` | Valida el nombre con `CNameServer` (renombra si está duplicado o no es válido); con ≥ 32 targets → `AfxMessageBox` modal | Comprobar el límite antes |
| `SetMode(mode)` | `004DD470` | Acepta `OMS V1.0` / `MSD V1.0` / `MSD V<x.y>` (activa `SetIsSoMVersion1_1` si `x.y ≥` umbral); puede mostrar *"Vijeo-Designer is ready. Press OK to continue"* | No usar en modo autónomo |
| `OpenProject(path)` | `004DCD10` | `SetForegroundWindow` + carga vía `FUN_004b57e0` (método `+A8h` del gestor de documentos) | Sólo `.vdz` |
| `GetBindingOfVariable` | `004F2230` | `return 0` | Capacidad no disponible en 6.3 |
| `ImportPanel` (v1 y v2) | `004F3AC0` / `004F6D30` | `*ppPanel = NULL; return S_OK` | Capacidad no disponible en 6.3 |
| `Target.Delete` | `004F6650` | Documento del target → método `+88h` | ✅ usable (destructivo: confirmación) |
| `Project.Close` | `004EC8E0` | Marca `+3Eh` y cierra; un segundo `Close` → `E_FAIL` | Llamar una vez, siempre antes de `Exit` |
| `Target.Download(address, path)` | `004F7910` | `address` no vacío, formato SoMachine: prefijo `msdstart:`/`msdstop:` + modo `SingleTarget`/`FirstTarget`/`NextTarget`/`LastTarget` y parámetros separados por `,`. Si `path` no está vacío, debe existir. Después `Sleep(max(10 s, FTPServerBootTime))` | Pendiente de probar con HMI de pruebas; timeout ≥ 10 s |
| `Project.Download(addresses)` | `004EE710` | **Ignora `addresses`** y lanza la descarga de la UI (`FUN_004af620`) a las direcciones **configuradas en cada target** | **Peligroso**: no exponerlo; usar `Target.Download` |
| `RetrieveSourceProject(address, path)` | `004DD070` | Ambos no vacíos; método `+B8h` del gestor de documentos; `out=0` si va bien | Pendiente de probar con HMI de pruebas |
| `M5DTemPHolder` | `0041A180` / `00490F30` | Nombre `"M5DTemPHolder%X"` con `GetCurrentProcessId()`; al arrancar, Vijeo borra todas las `M5DTemPHolder*` de `Vijeo-Manager` | Verificado: `Close` antes de `Exit`; las huérfanas se limpian solas |

**`LoadXMLCommand` en detalle** (`CLoadXMLCommand::vftable` @ `005D31D4`, `Execute` = `004D3450`):

1. Abre `ErrorFile` para escritura y escribe una línea de guiones; si no puede → `AfxMessageBox("Can't open LOG file")` **modal**.
2. Llama a la importación del proyecto (`FUN_004d0420`): carga el XML con MSXML (`SelectionLanguage=XPath`) y busca la raíz **privada** `VijeoDesignerProject` con `ProjectProperties/ProjectName`, `UserDatabase`, `InputSynchronization` y `Targets` (esquema `XML\private.xsd`, no `public.xsd`).
3. Responde siempre `<Result/>`; el detalle sólo está en `ErrorFile`.

Pruebas en vivo (cada variante en una sesión de Vijeo nueva):

| XML | `ErrorFile` | Efecto |
|---|---|---|
| Raíz pública `<Variables>` | `XML ERROR NULL XML Node` | **La siguiente llamada COM (`LookupTagCommand`) tumbó `Vijeo-Frame.exe`**: AV `0xC0000005` en `DDTTagDatabase.dll` (visor de eventos). Quedó `M5DTemPHolder155C`, que Vijeo borró al arrancar de nuevo |
| Privado mínimo (`ProjectProperties` + `XMLFileProperties SchemaVersion="1.0"` + `<Targets/>`) | sólo la línea separadora | Ninguno: el proyecto abierto mantiene su nombre |
| Privado con `Target`/`TargetProperties` (`TargetID="T1"`, `Model_HMIS5T`) + `VariableData` con `MCP_X` | sólo la línea separadora | Ninguno, ni en un proyecto vacío ni con el target creado antes: `LookupTagCommand(MCP_X)` → `Exists=false` |

Conclusión: en modo autónomo, `LoadXMLCommand` no sirve para crear ni modificar contenido en 6.3. Parece pensado para el flujo de SoMachine (modo MSD con `IMSDProject2`). Capacidad `xml_import = false` para 6.3. Si se reactivara en otra versión, el MCP debe validar el XML contra `private.xsd` antes y reiniciar Vijeo si el `ErrorFile` contiene `ERROR`.

**Modelos**: COM usa códigos internos (`Model_HMIS5T`, `Model_HMIGTO5310`) y el XSD público nombres comerciales (`HMISTU855`, `HMIGTO5310/5315`). En claro, en los binarios, sólo hay 16 códigos `Model_*` (`Model_HMIG2U`, `Model_HMIG3U`, `Model_HMIG3X`, `Model_HMIG5U`, `Model_HMIG5U2`, `Model_HMIG5U22`, `Model_HMISAC`, `Model_HMISBC`, `Model_HMISTO511`, `Model_HMISTO531_532`, `Model_HMISTU655`, `Model_HMISTU855`, `Model_IPC_FLEX_KPM`, `Model_OctolisHMISTU855`, `Model_XBTGT1335`, `Model_XBTGT2110`). La tabla completa sale de los `.dat` descifrados (§12.7).

### 12.7 Catálogos cifrados y tabla de modelos

**Formato de los `.dat`** (`TargetInfo\*.dat` y `XML\*.dat`), obtenido de `INDEXML.dll` (`CXMLFileFormatUtility::DecryptBuffer` @ `1001B920`, `TEAn` @ `1001B840`, `CEncryptedXMLFileUtility::LoadDocument` @ `10017CB0`):

- Cifrado **TEA, variante `tean` de Wheeler y Needham**, 8 ciclos (`N = -8` para descifrar), `DELTA = 0x9E3779B9`, aritmética de 32 bits sin signo.
- Clave fija de 16 bytes **`"InDE eLeCtRoNiCs"`**, con espacio. Ghidra la muestra como `s_InDE_eLeCtRoNiCs` porque sus etiquetas no admiten espacios; con el guion bajo no descifra.
- Bloques independientes de 8 bytes (ECB). El buffer se rellena con ceros hasta un múltiplo de 8.
- El texto en claro es XML **UTF-16LE** desde el byte 2, terminado en NUL.

Implementación: `tools/vjd-dat` (crate Rust sin dependencias; `lib.rs` con `decrypt_dat`, `decrypt_in_place` y `encrypt_in_place`, más tests). Uso: `vjd-dat <dir|fichero.dat>... -o <salida>`. Descifra los 33 ficheros de la 6.3: 18 catálogos de `TargetInfo` y 15 de `XML`, entre ellos las XSLT `public_to_private` / `private_to_public`. El MCP debería descifrarlos **en tiempo de ejecución** desde la instalación, en lugar de copiarlos al repositorio: así funciona con cualquier versión y no redistribuye datos de Schneider.

**Estructura de los catálogos**:

- `Targets.xml` (`TargetList/Target`): las familias, con `InternalName` (p.ej. `Target_HMISTU`), `Name`, `DefaultModel` y `ModelList`, que apunta al catálogo `Models_*.xml`.
- `Models_*.xml` (`ModelList/Model`): por modelo, `Identity/InternalName` (el código que usa COM), `Name` comercial, `RuntimeModelID`, `Display` (resolución y, en los modulares, `DisplayUnits` + `DefaultDisplayUnit` como índice base 0), `Download/DownloadTypes`, `Download/TargetOS`, `Registration/RequiredKey` (máscara de licencia) y, opcionalmente, un bloque `<SoMachine>` (`TypeID`, `DeviceFamilyID`).
- Los modelos descatalogados están **comentados** en el XML. Por ejemplo, `Model_HMISTU655`/`Model_HMISTU855` lo están, y su sustituto es `Model_HMIS5T`, «HMIS5T(HMIS65/S85)». Por eso `NewTarget('Model_HMISTU855')` fallaba.

**Qué modelos acepta `NewTarget`** (`CVJDTarget_Internal2::NewTarget` → `CTargets::Init(máscara, true)` → `CTargets::BuildAllowedLists` en `Targets.dll`):

- Un modelo es válido si `(RequiredKey & máscaraLicencia) == RequiredKey`.
- La API COM llama **siempre** con el modo SoMachine activo, que añade un requisito: el modelo debe tener un bloque `<SoMachine>` (`count(FINDCURRENT/SoMachine) > 0`).
- Verificado en vivo con los 58 modelos soportados (crear target → `Type()` → `Delete()`): **29 creables**. Fallan 27 por no tener `<SoMachine>` (Generic PC, Tablets, XBT OT, XBTGC, HMIBMW, Proface iPC, varios iPC MPC, `Model_HMIG5U`, Octolis) y 2 por licencia (Easy iPC, `RequiredKey = 0x10000000`).

> Nota (sexta tanda): ese criterio es el de la API COM, en modo SoMachine. Desde la sexta tanda el catálogo marca como creables los modelos de familia activa y licencia estándar, sin `<SoMachine>`: 56 en 6.3. `Model_HMIG5U` se creó con el asistente de la interfaz (verificado en vivo).

**Sintaxis de `hmi_type`** (verificada):

| Forma | Ejemplo | Resultado |
|---|---|---|
| `<modelo>` | `Model_HMIGTO5310` | ✅ modelos no modulares |
| `<modelo>:<pantalla>` | `Model_HMIG3U:HMIDT351 (800x480)` | ✅ obligatoria en los modulares (HMIGTU/GTUX). El nombre de pantalla tiene que ser **exacto**: `Model_HMIG3U:HMIDT351` se crea, pero **cae en silencio a la pantalla por defecto** (`HMIDT651 (1280x800)`) → el MCP debe comparar `Type()` con lo pedido |
| `Target=<modelo>[:<pantalla>],Control=True\|False` | `Target=Model_HMIS5T,Control=False` | ✅ forma compuesta. En SCU, `Control` se fuerza a `True` |
| `Target.Type()` | — | Devuelve la forma canónica (`Model_HMIG3U:HMIDT351 (800x480)`, `Target=Model_HMISAC,Control=True`) |

Referencias generadas:

- `docs/reference/vd-6.3/models.json`: 15 familias y 59 modelos, con `hmi_type` canónico, `display_units`, `somachine`, `required_key`, resolución, SO y tipos de descarga, más `com_creatable` (validación en vivo).
- `docs/reference/vd-6.3/models.md`: la tabla legible.
- `docs/reference/vd-6.3/com_probe_logs/models_validation.csv`: resultado de la validación modelo a modelo.
- Para regenerarlos en otra versión: `vjd-dat` → `tools/extract_models.ps1 -ValidationCsv …` tras `com_probe.ps1 -Step models`.

### 12.8 Creación y edición de paneles: export/import XML de proyecto (2026-11-01)

> Obsoleto desde la séptima tanda: `project_import_xml` y `LoadXMLCommand` se eliminaron del MCP, y sin COM no hay vía de import (§12.11). El análisis del export y del formato sigue siendo válido.

COM no tiene ninguna vía para crear o editar paneles (§12.5). Lo que sí existe es un **export/import del proyecto completo en el XML público** (`public.xsd`), con paneles, objetos, animaciones y acciones. Pseudocódigo en `docs/reference/vd-6.3/decompiled/{frame_xml_*,filemanager_*,pfutility_*,indexml_transform_pipeline}.c`.

**Qué cubre el XML público** (verificado con el export de `Compact_Hardwired_M221`): `Panel` (`Name`, `ID`, `BackColor`, `MasterPanel`) con objetos `Switch`, `IlluminatedSwitch`, `Lamp`, `Rectangle`, `Ellipse`, `Line`, `Polygon`, `Text`, `Image`, `GroupObject`/`Children`, `NumericDisplay`, `Meter`, `BarGraph`, `TrendGraph`, `SmartObject`…; geometría (`Left`, `Top`, `Width`, `Height`), `Style`, `Labels` multiidioma, acciones táctiles (`WhenTouch`/`WhileTouch` → `Special SpecialFunction="ChangePanel"`, `Bit`, `Word`, `Script`…) y animaciones (`VisibilityAnimation`, `ColorAnimation`, `PositionAnimation`, `SizeAnimation`, `RotateAnimation`, `FillAnimation`, `ValueAnimation`, `TouchAnimation`). También variables, alarmas, scripts, drivers, idiomas, `MasterPanel` y `DefinitionPanel`.

**Export** (`Vijeo-Frame.exe`):

| Pieza | Dirección | Detalle |
|---|---|---|
| Comando de menú **`0xC147`** "Export Project" | `FUN_004c5de0` → `FUN_004ae510` | `WM_COMMAND 0xC147` al marco `AfxMDIFrame80u` abre un `CINDEFileDialog` "Export Project" (modal, Save, edit id 1148, filtro id 1136, botón id 1). Verificado con `tools/ui_export_probe.ps1`: hay que pulsar con `BM_CLICK` (un `WM_COMMAND IDOK` no basta). ~14 s para el ejemplo |
| Filtros del diálogo | `FUN_004a8c30` | `.zdat` siempre. `.vxml` sólo si `HKLM\SOFTWARE\WOW6432Node\Schneider Electric\Vijeo-Designer63\Vijeo-Frame\Options\SupportsXMLImportExport` (DWORD) ≠ 0. En esta VM sólo existen `IndirectVariable` y `Version` |
| Escritura | `FUN_004d2010` | `FUN_004d1470` genera el XML privado → `INDEXML!TransformToPublic` → `.vxml` en claro, o `.zdat` = ZIP con `<nombre>.vdat` + imágenes (`<Target>/<PanelID>/*.bmp` y planas) |
| `.vdat` | `FUN_004d2010` + `PFUtility!CCipherKey` | 2 bytes `07 B2` + IV aleatorio de 16 bytes + XML cifrado con **Twofish-256 CBC** (implementación de referencia de Counterpane, firma `"FISH"`; no es Rijndael aunque las tablas estén). Clave fija de 32 bytes en `FUN_004d2010` (no se publica). Relleno: bytes aleatorios hasta múltiplo de 32 (≥ 5) y los 4 últimos bytes = longitud en claro (LE). Descifrado verificado con `tools/vdat_decrypt32.ps1` (llama a `CCipherKey::Decrypt` en un PowerShell de 32 bits): 483 962 bytes de XML `<Project …>` |

**Import**:

| Pieza | Detalle |
|---|---|
| Público → privado | `INDEXML!CXMLFileFormatUtility::TransformToPrivate(doc, true)`: cadena de XSLT cifradas de `XML\` (`public_to_private`, `attribute_lookup_table`, `attribute_value_lookup_table`, `attribute_value_reprocess_import`, `attribute_renaming_import`, `make_absolute_groups`, `make_absolute_points`, `base64decoder`) sobre MSXML 4.0. Verificado con `tools/xml_transform32.ps1`: ~4,8 s, raíz `VijeoDesignerProject` |
| Privado → proyecto | `FUN_004d0420` (lo usan `LoadXMLCommand` y el método `+74h` de la interfaz interna que llama `INDEXMLImport.dll`). Crea un **proyecto nuevo** con el `ProjectName` del XML (por eso no hacía nada con un proyecto ya abierto, §12.6). El descifrado que contiene es el de la base de usuarios, no el del `.zdat` |
| Import desde el Navigator | `INDEFileManager!CXmlProjectImporter` acepta un XML cuya raíz sea `<Project ` (público), aplica `TransformToPrivate`, lo guarda en `%TEMP%` y llama a `INDEXMLImport.dll` (`InitVDImportExtensionDLL`) → `FUN_004d0420`. Lista de importadores: `CZipProjectImporter` (`.vdz`), `CXmlProjectImporter`, `CLegacyProjectImporter` |

**Round-trip verificado en vivo** con `Compact_Hardwired_M221`:

1. Export a `.zdat` (`tools/ui_export_probe.ps1`) y descifrado (`tools/vdat_decrypt32.ps1`).
2. Edición del XML público: `Project/@Name` = `MCP_TMP_RT1`, imágenes extraídas del `.zdat` con los `FileName` reescritos (178) y un `Rectangle` + un `Text` nuevos en el panel `Cabinet`.
3. `TransformToPrivate` (`tools/xml_transform32.ps1`, ~3,3 s).
4. `tools/xml_import_probe.ps1`: `LoadXMLCommand` sin proyecto abierto (1,6 s, sin errores en `ErrorFile`) → Vijeo **crea y guarda** `Vijeo-Manager\MCP_TMP_RT1\MCP_TMP_RT1.SwxCF` (+ `DataFilesBackup\`, `AutoBackup001.bkm`, `Desc.MAP`, `.ini`) → se empaqueta como `.vdz` (ZIP con `<nombre>.SwxCF`, `DataFilesBackup/`, `SymbolTables/`) → se borra la carpeta del workspace. Unos 7 s en total.
5. `OpenProject` del `.vdz` resultante y nuevo export: los 12 paneles, el target `HMISTU855` y los dos objetos nuevos están, con sus propiedades. Diferencias: 2 731 → 2 732 nodos (un `Column` más) y los colores se normalizan como números (`0000FF` → `FF`).

Detalles del import:

- Si una imagen referenciada no existe, Vijeo abre el modal **"Error 2500: Unable to open file …"** y la llamada COM se queda bloqueada. Los `FileName` del export apuntan a una carpeta temporal que Vijeo borra al terminar, así que hay que extraer las imágenes del `.zdat` y reescribir las rutas antes de importar.
- Después de `LoadXMLCommand` **no queda proyecto abierto**: `OpenProject(x.vdz)` abre `x.vdz`, no devuelve el importado. El resultado se lee del workspace.
- ⚠ **`FUN_004d0420` crea el proyecto en `Vijeo-Manager\<ProjectName>`**, no en `M5DTemPHolder<PID>`. El MCP debe usar siempre un `ProjectName` temporal que no exista (si coincidiera con un proyecto real del usuario, podría pisarlo o abrir un modal), empaquetar el `.vdz` fuera del workspace y borrar sólo la carpeta que apareció en esa ejecución. El script lo hace así y se niega si el nombre ya existe.

**Hallazgos posteriores** (implementación de `project_import_xml`):

- **`.vxml` y COM**: `FUN_004a8c30` sólo lee `SupportsXMLImportExport` si el miembro `+0x1C8` de la aplicación ≠ 1. `InitInstance` (`FUN_00493620`, `00493D49`) lo pone a 1 cuando Vijeo arranca con `-Embedding`, y `SetMode`, `Save` y `put_Name` del proyecto también lo escriben. En instancias COM el diálogo sólo ofrece `.zdat` aunque el valor de registro esté puesto (verificado). En una instancia normal (`Vijeo-Frame.exe /import x.vdz`) sí aparece `Vijeo-Frame XML Files (*.vxml)`, pero `/import` copia el proyecto a `Vijeo-Manager\<nombre del proyecto>`, pregunta si se convierte de versión, y el guardado en `.vxml` pulsado por programa no se completó.
  - Nota (sexta tanda): desde una instancia normal el guardado en `.vxml` sí se completa: se elige el filtro `*.vxml`, se escribe la ruta y se pulsa Aceptar (§12.10).
- **`RelativePaths` sin inicializar**: en `FUN_004d0420` las opciones viven en `local_100`, que nunca se inicializa; sólo se hace `local_100 |= 1` si `ProjectProperties/@RelativePaths` es true. Con basura en el bit 0, Vijeo antepone la carpeta del XML a rutas absolutas y falla al azar con «Error 2500: Unable to open file <carpeta del XML>\C:\…» (3 de 4 ejecuciones en una tanda, ninguna en otra). Con rutas de imagen **relativas a la carpeta del XML** y `RelativePaths="true"` el comportamiento es determinista (5 de 5).
- **Lo que el `.vxml` no conserva** (verificado con un proyecto real de 54 paneles y 14 equipos Modbus TCP): pantallas y objetos sí (los 2039 objetos con nombre están en su panel tras reimportar), pero el export deja vacío `Equipment/EquipConfig/ConfigString`, así que al reimportar todos los equipos quedan con la dirección por defecto (`T:0:0.0.0.0:255:120:0:4:3:0:1`: IP 0.0.0.0, puerto 255, unit ID 120). Tampoco viajan la máscara de descarga (`255.255.255.0` → `255.255.0.0`), la descripción del target ni el splash. **El import sí respeta `ConfigString`**: con la cadena de `ProtocolConfig` del original (`[4]T:0:10.0.0.101:502:255:0:0.0.0.0:120:1:6:3:0:0:0`) el equipo queda idéntico.
- **Restauración desde el `.vdz` original** (`restore_from`, verificado con PLANTA_SET3): el helper rellena sobre el XML privado `Target/TargetProperties/Download/@SubnetMask|@DefaultGateway|@IPAddress` (los dos primeros el export no los escribe) y `IOManager/Driver/Equipment/EquipConfig/ConfigString` vacíos; los 14 equipos, la máscara y la puerta de enlace quedan iguales que en el original. El splash **sí** se exporta (`StartupOptions/@SplashFile`, absoluto, con el fichero en `<vxml>\Splash.jpg`), pero se perdía porque el import sólo copiaba `FileName`: con `RelativePaths="true"` un `SplashFile` absoluto se descarta sin aviso y falta `DataFilesBackup/T_1/Splash.jpg`. Copiado como las imágenes, vuelve. Sin campo en `public.xsd` ni `private.xsd`: la descripción del target. Tampoco vuelven dos valores sin etiquetar del `Document` del target (`D:\` y una segunda terna IP/máscara/puerta, `10.0.0.120/255.255.255.0/0.0.0.0` → `…/255.255.0.0/…`).
- **El round-trip rompía las variables IEC** (PLANTA_SET3, compilado en ambos lados): el original da 1 error (`1331`, un driver `SoMachineNetwork01` que Vijeo añade al abrir y deja el proyecto sucio). El reimportado daba ≥100 errores `4330 [Variable] [X] Invalid address: Mismatch [Data Length] is a [16] bit register` en variables con dirección IEC (`%M2000` BOOL, `%MW6108:X0` BOOL, `%MW6000` UINT, `%MW6200` REAL). Los registros de las variables en `Services\TagDatabase` son idénticos byte a byte salvo identificadores (que se corresponden 1:1). La causa está en el equipo:
  - **Formato de `EquipmentJ\ProtocolConfig`** (escritor `FUN_637f3d5f`, lector `FUN_637f2c8b` de `TISIOManagers.dll`): `u32 versión (8)`, cadena (`u32 bytes + UTF-16`), `u32` del `CDLLinfo` del driver (el modo de direccionamiento IEC + 3: `CPDLUI::SetIECAddressMode` hace `+0x820 = modo + 3` y `GetIECAddressMode` resta 3), segunda cadena (vacía) y `u32` contador (0).
  - **Import del equipo** (`FUN_637fdaaf`): si `EquipConfig/ConfigString` viene **vacío**, construye la cadena a partir de los atributos de `EquipConfig` (plantilla del driver `[%d]Protocol=IPProtocol:0*:%s:502*:Station:EnableSecondaryIPAddress=Boolean:SecondaryIPAddress:FrameLength:UsesIECSyntax=Boolean:WordOrder=ByteAndWordOrder:ByteOrder=ByteAndWordOrder:PreloadDataDictonary=Boolean:HotStandbyInfo=Boolean`, de modo que el 9.º campo de `[4]T:0:10.0.0.101:502:255:0:0.0.0.0:120:1:6:3:0:0:0` es `UsesIECSyntax=1`) y aplica `IECAddressMode`. Si viene **relleno**, usa la cadena e **ignora `IECAddressMode`**: el modo queda en −1 (se serializa 2) y las variables IEC no validan.
  - **Export** (`FUN_637fd144`): intenta convertir la cadena a atributos; con la cadena versionada `[4]…` no saca ninguno y deja `ConfigString` vacío con `IECAddressMode` = modo guardado (0).
  - Verificado: `IECAddressMode="1"` con `ConfigString` vacío compila sin 4330 (serializa 4), pero con IP 0.0.0.0; `ConfigString` relleno (con o sin `[4]`, con `IECAddressMode` 0 o 1) da 2 y falla; copiar el `ProtocolConfig` completo del original al `.vdz` (serializa 3, como el original) compila sin 4330 y con las IP. Es lo que hace `restore_from` (`swx::copy_protocol_configs`, sobre el `.vdz` temporal antes de abrirlo en Vijeo).
- **Comparación completa del round-trip** (PLANTA_SET3 original contra el reimportado con `restore_from`, 1129 contra 924 streams; herramientas de scratchpad `swxdiff`/`strdiff`/`tgtres`). Las rutas no sirven para comparar: Vijeo renumera `PanelN`, `UniqueNameN`, `DriverN`, `EquipmentN` y los nombres de la biblioteca de imágenes. Se compararon los textos de cada zona (cadenas MFC y `u32 bytes + UTF-16`, sin espacios, sin repetir) y se buscó cada texto que falta en el `.vxml` para saber si se pierde en el export o en el import:
  - **Se pierde en el import** (está en el XML y no en el `.vdz`):
    - **los usuarios de seguridad** (arreglado, ver más abajo): `Security/Users/User` (5 usuarios con su hash) llega al XML privado, pero `SecurityManagerStream` sólo guarda la clave y los 5 grupos (984 → 363 bytes);
    - **el display** (arreglado con `restore_from`, ver más abajo): `TargetProperties/@DisplayUnit` se ignora y el target queda con el de por defecto del modelo. La resolución está en `PrimaryTarget\Document`, dos `u16` 6 bytes después de la puerta de enlace (1024x768 → 1280x800; verificado en 20 proyectos: 320x240, 800x600, 1024x768, 1280x800);
    - las etiquetas (`<Label>`) de los grupos de alarmas (`GrupoDeAlarmas1`, `AlarmGroup1`);
    - **los mensajes de alarma** (arreglado, ver más abajo): avisos 2033 y error 1109 aparte, todas las alarmas apuntan a otro texto.
  - **No está en el XML**:
    - la descripción del target;
    - `D:\` y la segunda máscara del `Document` del target;
    - 48 nombres del NameServer (miembros de un DDT de Hot Standby);
    - 77 textos de `LangManagerData` que ningún objeto del XML usa (unidades como `kW.` o `seg.`, y textos de otro proyecto como «HABILITACIONES TRITURADOR PRIMARIO»: probablemente huérfanos);
    - en `ResourceManagerStream`, `Arial` y `Lucida Console` (fuentes de Windows con las que el editor muestra las fuentes del HMI);
    - nombres internos de objetos nativos (`NativePartStream`: `AlarmAck`, `Display08`… pasan a `Display_0007`…);
    - en `MainDocumentData` (815 → 271 bytes), dos campos sin identificar (`+0x08`: 0 → 6; `+0x18`: 3428, un ID interno, → −1). Lo demás son restos de guardados anteriores detrás del bloque útil: la zona horaria (`City of Buenos Aires`) se conserva.
  - **Sin cambios**: los 41 paneles base con los mismos nombres (`panel_list`), los 9 maestros y los 55 emergentes; las variables (`TagDatabase`: ningún texto falta); los grupos de seguridad; recetas, registro de datos, acciones globales, objetos inteligentes, teclas de función y formularios. Las 2 acciones de panel con script se conservan; el original guarda además el texto fuente tras `end:{`.
  - **No se comparan** (cambian en cada import): dentro de los paneles, las expresiones compiladas (`(TagDB.X.getIntValue(0) != 0)`), las referencias `!{T,2:1.<id>,X}` y los `StatusFlags` `(1)<id>`, que llevan IDs de variable; la biblioteca de imágenes, que guarda las del proyecto (369 → 117 nombres; las no usadas no vuelven); las miniaturas `JpegList`; y `IlluminatedSwitchStorage`, que pasa de uno por panel a uno global.
  - **Usuarios: causa y arreglo.** El `/Version` del original dice que se creó con 6.1 (build 393) y se guardó con 6.3. El reimportado se crea con 6.3 (build 103), y el formato de los usuarios cambia:
    - En el formato antiguo, `SecurityManagerStream` guarda, tras los grupos, un contador y una terna por usuario (nombre, hash y grupo).
    - En el de 6.3, guarda un contador y pares (`UserKey u32`, grupo), y el nombre y el hash van en el `/Document` raíz (PROYC, creado con 6.3, lo guarda así).
    - El XML sigue este cambio: en 6.3 cada `Security/User` lleva `UserKey`, y existe `/Project/UserDatabase/UserTable/User` (`UserKey`, `UserID`, `UserPwdHash`), sólo en `public.xsd`. El export de un proyecto antiguo escribe los usuarios sin `UserKey` y sin `UserDatabase`, y el import los guarda todos con la misma clave: queda uno solo (el último), sin nombre.
    - Copiar el `SecurityManagerStream` del original hace caer Vijeo al abrir, aunque se copie también `CommonSecurity`.
    - **Verificado:** con `UserKey` 1…n en cada usuario y la `UserDatabase` (`UserID` = `UserName`, `UserPwdHash` = `PasswordEncrypted`), el import crea los 5 usuarios con su grupo y el mismo hash. Lo hace el helper siempre, sobre el XML público antes de `TransformToPrivate`.
    - Sin probar: que esas contraseñas funcionen en el runtime.
  - **Display: causa y arreglo.**
    - `DisplayUnit` llega al XML privado y el import no lo aplica: tampoco con `HMIDT642` sin la resolución, y `Model="Model_HMIG3U:HMIDT642 (1024x768)"` lo rechaza (`Vijeo-Designer Import Error`).
    - El display lo fijan la resolución y dos bytes más del `PrimaryTarget\Document`: `+40` del hueco antes de la 7.ª cadena (0 / 4) y `+43` del hueco antes de la 20.ª (4 / 2). Cambiando sólo la resolución sigue siendo HMIDT651; con todos los bytes no textuales del original (y las cadenas del importado) abre como HMIDT642.
    - Antes de que Vijeo abra el `.vdz` temporal, la resolución es provisional (320x240).
    - `swx::restore_target_documents` (dentro de `restore_from`, sobre el `.vdz` temporal) parte los dos `Document` en cadenas MFC y los bytes entre ellas. Exige la misma forma: mismo número de cadenas y huecos de la misma longitud. Si la resolución cambió, toma todos los bytes no textuales del original.
    - De las cadenas, recupera las vacías en el importado (la descripción y `D:\`) y las IP por defecto (`0.0.0.0`, `255.255.0.0`) a partir de la 9.ª cadena (la segunda máscara). Las demás, como la IP de descarga, se quedan como las trae el XML.
    - **Verificado:** abre como HMIDT642 y el bloque queda igual que el original salvo el nombre del splash; compila con los mismos errores (`1109` ×2, `1331`).
    - Efecto secundario: si el display cambió, un ajuste binario del target cambiado en el XML vuelve al valor del original.
  - **Mensajes de alarma: causa y arreglo.**
    - **Cómo se guardan:** cada variable de `Services\TagDatabase` guarda propiedades `[u32 id][u32 tipo][valor]`. El mensaje de alarma es la `0x0B` (tipo 6, `u32`), detrás de la `0x0A` (tipo 2, 1 byte: alarma activa). Es el ID de una entrada de `LangManagerData`: `u32 id`, `u32 2`, `u32 bytes`, UTF-16 terminado en 0.
    - **El fallo:** en el original el valor es el ID del texto (`0x30C5`). En el reimportado, los textos de los mensajes existen (IDs 1608…), pero las variables guardan esos IDs menos 1607 (1, 2, 3…). Esos IDs bajos son los textos de los paneles, que se numeran primero. El import numera los mensajes desde 1 y no aplica el desplazamiento.
    - **Resultado medido** (PLANTA_SET3, 423 alarmas): ninguna apuntaba a su mensaje, 223 mostraban otro texto («Clave», «Usuario», «CONTROL DE ACCESOS»…) y 46 caían en textos vacíos o largos (los avisos `2033` y el `1109`).
    - **Verificado:** las referencias siguen el orden de las variables con `<Alarm><Message>` del XML. Con +1607 coinciden todas (34 sólo difieren en espacios finales, que el import recorta). Hay además un registro con la alarma desactivada y el valor `0xFFFFFFFF`, igual en el original.
    - **Arreglo:** `swx::fix_alarm_messages`, sobre el `.vdz` temporal antes de abrirlo y sin necesitar `restore_from`.
      - Toma como desplazamiento el más votado entre «ID del texto esperado (si es único) − valor guardado».
      - Lo aplica sólo si todas las referencias resultantes existen y al menos el 90 % coincide con el XML. Si no, avisa.
      - Tras guardar, `check_alarm_messages` compara lo guardado con el XML.
    - **Tras el arreglo:** la compilación sólo da el `1331` del original, sin ningún 2033 ni 1109. Ese `1331` (`[Driver] [SoMachineNetwork01] requires Equipment Address`) no es del proyecto: lo provoca el modo MSD de las instancias COM, y un `Save` por COM lo guardaría en el `.vdz` (§12.9).
    - No se ha comprobado con proyectos de varios idiomas o varios targets. Si el número de referencias no cuadra con el XML, no se toca nada y se avisa.
  - `restore_from` informa de todo esto en `differences`: `xml_import::compare` (target, incluida la resolución) y `compare_texts` sobre `swx::texts_by_area`, que sólo mira las zonas de la lista `TEXT_AREAS` y descarta los hashes de contraseña.
- **Tras `LoadXMLCommand` no queda proyecto abierto**: el resultado se lee de `Vijeo-Manager\<ProjectName>\<ProjectName>.SwxCF`, que se empaqueta como `.vdz`.

**Flujo resultante para el MCP** (todo en instancias propias): `OpenProject(.vdz)` → `WM_COMMAND 0xC147` + diálogo → `.zdat` → descomprimir + descifrar Twofish → XML público → editar/generar paneles, objetos, animaciones y acciones → `ProjectName` temporal + rutas de imágenes → `TransformToPrivate` → otra instancia sin proyecto → `LoadXMLCommand` → empaquetar `Vijeo-Manager\<temp>` como `.vdz` → borrar la carpeta temporal. Implementado como `project_import_xml` sólo la parte del import (el XML de entrada lo exporta el usuario desde la interfaz de Vijeo como `.vxml`); `TransformToPrivate` se ejecuta con un helper de 32 bits (INDEXML.dll + MSXML 4). Falta comprobar si el formato de color es RGB o BGR, y cómo se añade un panel nuevo (IDs `P…`, referencias desde el target).

### 12.9 Las instancias COM siempre arrancan en modo SoMachine (MSD) (2026-11-01)

> Nota (sexta tanda): el MCP ya no usa COM para los proyectos abiertos. Lo que sigue describe el problema y su origen; la solución está en §12.10.

> Obsoleto desde la séptima tanda: el MCP ya no usa COM en absoluto, y `LoadXMLCommand` también se eliminó (§12.11).

**Síntoma:** al compilar PLANTA_SET3 por COM sale `Error 1331: [Driver] [SoMachineNetwork01] requires Equipment Address.`, también en el original. Pero el `.vdz` no tiene ese driver: ni el backup del usuario (`C:\Trend\BKs\HMI\PLANTA_SET3_VJD6.3_20261028.vdz`) ni el restaurado tras el import, que se guardó por COM. Su `.SwxCF` sólo tiene `Component 2\Driver4` (`MODBUS_TCP`, 14 equipos) y ningún stream SoMachine/MSD.

**Cadena (Ghidra, `Vijeo-Frame.exe` 6.3.0.103 y `TISIOManagers.dll`):**

1. `InitInstance` (`FUN_00493620`): después de `CWinApp::ParseCommandLine`, si `m_bRunEmbedded` o `m_bRunAutomated` están activos pone `CWinApp+0x124 = 1`. Más adelante, si ese indicador vale 1, llama a `FUN_004a45b0(1)`, que guarda `1` en `+0x1C8` del documento del framework. COM siempre lanza Vijeo con `-Embedding`, así que toda instancia del MCP queda en ese modo.
2. Lo lee `ITISFrameWorkSinkEx::GetIVijeoDesignerMode` (slot 30, `FUN_004bc3a0` → `FUN_004a45c0`, que devuelve `+0x1C8`). El enum `eIVijeoDesignerMode` tiene dos valores: `eIVJDMode_None = 0` y `eIVJDMode_MSD = 1`. `SetMode("OMS V1.0" | "MSD V…")` (`FUN_004db2c0`/`FUN_004dd470`) pone el mismo valor, 1, y no hay ninguna vía para volver a 0. `put_Name` de `CVJDProject_Internal`/`_Internal2` (`FUN_004e9950`/`FUN_004edff0`) también lo escribe, con un valor que no viene del cliente.
3. Los componentes consultan ese modo por el slot +0x78. Por ejemplo, `TISIOManagers`: `IsSoMachineDriver` (`FUN_637e25fe`) sólo devuelve 1 en modo MSD y para `Schneider_PDL_OMS_XBTGC`/`Schneider_PDL_OMS_Gateway3`; `FUN_63804517` y `FUN_637e8f65` usan la misma condición. El driver es el *SoMachine - Network* de `HMI IO\Schneider_OMS_PDL.Dll` (también *SoMachine - Combo*), y su nombre por defecto es `SoMachineNetwork01`. `MainDocument.dll` expone `IsSoMComboAndNetworkAdded`.

**Conclusión:** en modo MSD, Vijeo añade a cada target la red SoMachine que en la integración real configura SoMachine/Machine Expert. Fuera de SoMachine nadie le da una dirección, de ahí el `1331`. La función concreta que crea el driver no está localizada (`MainDocument.dll` sin analizar).

**⚠ La validación por COM la añade y `Project.Save` la guarda en el `.vdz`.** No la añade `OpenProject`: la añade la validación, que hacen `Build`/`BuildTarget` y `GenerateAllReports`. Con SET03 un `OpenProject` + `Save` sin compilar no la añade; un `Build` + `Save` sí. Verificado con el ejemplo `Compact_Modbus SL_M221_Book.vdz`, que en disco sólo tiene `Driver2` (`ModbusTCP`). Tras pasar por COM y `Save`, el `.vdz` gana `Driver6`: `SoMachineNetwork01`, `Schneider_PDL_OMS_Gateway3`, `DLMName=OMS.dlm`, un equipo `SOM_<target>` (`Schneider_PCL_OMS_Gateway3`) sin `ProtocolConfig`. Entre los `.vdz` de los scratchpads, los que pasaron por `Save` de COM tienen dos drivers (`MCP_TMP_RT1*.vdz`, `check_a.vdz`). Los creados por `LoadXMLCommand` (se empaquetan desde la carpeta de `Vijeo-Manager`, sin `Save`) y los `.vdz` del usuario en `Documents\SET03` sólo tienen el suyo. Así, cualquier `project_save` del MCP sobre un proyecto ya existente le añade un driver que no compila. (El cambio de `Model_HMISTO531_532` a `Model_HMIGTO6310` del mismo ejemplo es otra cosa: el modelo no existe en 6.3 y Vijeo lo convierte también desde la interfaz.)

**Instancia normal, verificado el 2026-11-01** con una copia limpia del ejemplo. La copia se renombró a `MCP_TMP_NORMAL2` cambiando sólo el nombre de la entrada `.SwxCF` del ZIP, que es todo lo que define el nombre del proyecto.
- **Arranque:** `Vijeo-Frame.exe /Import "<copia.vdz>"`, igual que la asociación `.vdz`. El proceso cuelga de quien lo lanza y la ventana sale en ~4 s. `/Import` desempaqueta el proyecto en `Vijeo-Manager\<nombre de la entrada .SwxCF>`, no en `M5DTemPHolder`, así que hay que usar un nombre temporal y borrar la carpeta después.
- **Diálogos al abrir** (`#32770`, según el proyecto):
  - *"Opening the project will update it to the current Vijeo Designer version…"* (Yes = 6 / No = 7 / Help), con casilla id 10030.
  - *"The project was created with a Target Type that is not supported…"*: Yes = introducir una referencia de licencia; No = convertir a un modelo equivalente.
  - Al cerrar con cambios: *"Do you wish to save the changes you made to <proyecto>?"* (Yes = 6 / No = 7 / Cancel = 2).
  - Con 3 o más Vijeo-Frame ya abiertos (de cualquier tipo): *"You already have 3 instance(s) of Vijeo-Frame running. You may experience slower performance…"* [OK = 1]. Es probablemente lo que hacía agotar el tiempo a la tercera instancia COM.
  - Los botones se pulsan con `BM_CLICK`.
- **Menú:** el marco `AfxMDIFrame80u` usa la barra de menús BCG y no tiene `HMENU`. Los ids salen del recurso `MENU 1008` de `Main.ENU`: Build All `0xC09E`, Build Target `0xC09D`, Validate All `0xC0A0`, Validate Target `0xC09F`, Clean All `0xC142`, Save Project `0xC098`, Save As `0xC0A6`, Close Project `0xC09A`. Se mandan con `PostMessage(WM_COMMAND)` al marco. La Feedback Zone (ListBox 102 bajo la barra 711) se lee igual que en las instancias COM. Al terminar se cierra con `WM_CLOSE` al marco.
- **Resultado:** `Build Failed - 2 Error(s), 2 Warning(s)`: los dos `2841` de Backlight Color y los warnings 2133/2131, sin ningún `1331`. La sección `HMISTO531_532 - I/O` queda vacía y el SRAM ya no reserva `Control: 1 KB`. Con la misma copia por COM: 3 errores (`1331` de `SoMachineNetwork01` incluido) y `SRAM … Control: 1 KB`.
- Con el usuario usando su instancia interactiva, otra instancia normal funcionó sin interferir (la huérfana COM 4624 también seguía viva).

**Otros efectos del mismo modo, ya vistos:** sólo 29 de 58 modelos son creables (§12.7), el export `.vxml` no aparece en instancias `-Embedding` (§12.8) y `LoadXMLCommand` está pensado para el flujo MSD (§12.6).

**Alternativas:**
- Desde COM no se puede salir del modo MSD: el indicador depende de la línea de comandos que pone DCOM, y `SetMode` sólo activa el modo.
- Instancia normal (`/Import`, `eIVJDMode_None`): **implementado** como `project_build`/`target_build` con `path` (`docs/arquitectura.md`, quinta tanda). Desde la sexta tanda, la misma instancia normal sirve también para los proyectos abiertos (§12.10). PLANTA_SET3 compila así con 0 errores y 7 warnings (~25 s, runtime generado), sin el `1331`. Las líneas de resumen tienen tres formatos en `Main.ENU`: `Build Failed - %1 %2(s), %3 %4(s)`, `Build Complete - %1 %2(s)` (sólo warnings, con marcador `\x01\x0D`) y `Build Complete`. Tras el resumen de validación vienen `Generating Code`/`Compiling`/`Romizing` y una línea con el tamaño del runtime.
- Guardar por COM: **implementado**. Cada `Save` del MCP (`project_save`, `project_new`, `project_import_xml`) quita del `.vdz` recién guardado el driver añadido, antes de renombrar el temporal al destino (`swx::remove_added_somachine_drivers`). Sólo se quita un driver si cumple todo esto:
  - tiene id `Schneider_PDL_OMS_Gateway3`/`_XBTGC`;
  - el `.vdz` de partida no tenía ese driver (target, nombre);
  - ningún equipo suyo tiene `ProtocolConfig` con dirección;
  - sus nombres en `NameServer` tienen un id ≥ el siguiente id libre del fichero de partida.

  Si algo no encaja con el formato, no se toca el fichero y `project_save` lo dice en `notes`. Marcar los `1331` de drivers que no estaban en el original habría arreglado el informe, no el fichero.
  > Obsoleto desde la sexta tanda: el MCP ya no guarda por COM, así que `swx::remove_added_somachine_drivers` ya no se llama (sigue en `swx.rs`) y `project_save` no devuelve avisos de este tipo en `notes`.
  - Verificado en vivo con el ejemplo Modbus: Build + `Save` por COM, y el `.vdz` guardado tiene los mismos drivers que el original. Compilado sin COM da 2/2, como el original. Abierto otra vez por COM y compilado, vuelve a salir `SoMachineNetwork01`: no quedan nombres huérfanos.
  - SET03 (`built_saved`) limpio compila como el original (0/7).
  - `Compact_Hardwired_M221.vdz` trae de SoMachine una red `SoMachineNetwork01` **con dirección** (`127.0.0.1:1217`), en `Component 4`: no se toca (hash idéntico).

**Formatos de lo que añade (en el `Component N` del target que tiene `DriverList`: suele ser el 2, el ejemplo Hardwired usa el 4):**
- `DriverList`: cabecera de 26 bytes, con `u32 6`, `u32 0`, `u32 nº de drivers`, `u32 × 3` (ids de los grupos de `NameServer` de drivers, equipos y grupos de escaneo) y `u16` (siguiente nº de carpeta `DriverK`). Después, por driver: 4 cadenas (nombre, fabricante, id, carpeta) y un `u32 0`. Cada cadena es `u32 bytes` + UTF-16 con el 0 final. Ocupa exactamente el stream.
- `/Services/NameServer`: `u16 1`, `u16 nº de raíces` (2), los árboles de nodos y un `u32` con el siguiente id libre. Puede quedar basura detrás, de un guardado anterior más largo. Cada nodo es `u16 1`, `u32 grupo`, `u16 tipo`, `u16 n`, n × (`u32 id` + cadena), `u16 nº de hijos` y los hijos. El driver fantasma añade tres nombres, cada uno en su grupo de la cabecera de `DriverList`: el del driver, el del equipo `SOM_<target>` y el del grupo de escaneo (`SoMachineEquipment01SG01`, de su `DeviceList`).
- `TagDatabase` también cambia al guardar, pero sólo es el estado de filtros del editor de variables. El primer byte de `Component N/Status` pasa de 00 a 04 con cualquier `Save`. Ninguno de los dos tiene que ver con el driver.

**Sigue pendiente:** `project_reports` por COM falla con SET03. `GenerateAllReports` valida, ve el `1331` y devuelve `E_FAIL`, y el plan B (`com::build`) también valida por COM.

> Obsoleto desde la sexta tanda: `project_reports` se eliminó. `project_export_xml` lee el XML desde la instancia normal (§12.10).

### 12.10 Instancia normal sin COM (proyectos abiertos) (2026-11-01)

Desde la sexta tanda el MCP no abre, crea, guarda, compila ni busca variables de los proyectos por COM. Cada proyecto abierto es un `Vijeo-Frame.exe` lanzado como ejecutable, sin `-Embedding`: queda en modo normal (`eIVJDMode_None`, §12.9) y no añade el driver `SoMachineNetwork01`. El MCP lo maneja como lo haría el usuario: comandos de menú, asistentes, diálogos, Navigator y Export Project. Código: `ui_project.rs` (`UiProject`) y `vijeo-ui` (`ui`, `dialog`, `feedback`, `tree`; hasta la séptima tanda, `vijeo-com`). Lo marcado como **verificado** se probó en vivo; el resto está implementado.

**Abrir, guardar y cerrar**

| Paso | Qué hace el MCP |
|---|---|
| Abrir (`project_open`) | Copia el `.vdz` a `%TEMP%\vijeo-mcp\project-<pid>-<n>\MCP_TMP_<pid>_<n>.vdz`, con la entrada `.SwxCF` renombrada al nombre temporal. Lanza `Vijeo-Frame.exe /Import <copia>`, que desempaqueta el proyecto en `Vijeo-Manager\MCP_TMP_<pid>_<n>`. Espera a que el título empiece por `<nombre> - ` y a que no haya diálogos con botones; guarda una vez y borra la copia. |
| Crear (`project_new`) | Lanza `Vijeo-Frame.exe` sin argumentos y usa el asistente New Project (abajo). El proyecto nace en `Vijeo-Manager\MCP_TMP_*`. |
| Guardar (`project_save`) | `Save Project` (`0xC098`). Termina cuando el `.SwxCF` ha cambiado y lleva 2 s quieto, sin diálogos. Si Vijeo no lo reescribe porque no había nada que guardar, basta con 8 s. |
| Empaquetar | `vdz::package_vdz`: ZIP con `<nombre>.SwxCF` (el nombre real), `DataFilesBackup/` y `SymbolTables/`, en un `.vdz` temporal junto al destino, que luego se renombra. |
| Cerrar (`project_close`) | `Close Project` (`0xC09A`). Ante «Do you wish to save the changes…» se responde No. Después `WM_CLOSE` al marco (20 s). Si no sale, se termina el proceso. Se borran la carpeta de `Vijeo-Manager` y la copia de trabajo. |

**Asistentes, combos y campos**

- New Project: `0xC095`. Página del nombre: nombre del proyecto `7032` y Next `12324`. Página de target: nombre `7072`, familia (combo) `7061`, modelo (combo) `7062`, pantalla de los modulares (otro combo de la página, `13551`) y Finish `12325`.
- New Target: `0xC418`, con los mismos ids.
- Combos: `CB_SETCURSEL` y después `CBN_SELENDOK`, `CBN_SELCHANGE` y `CBN_CLOSEUP`, como lo haría el usuario. Tras elegir familia y modelo, el MCP espera 0,5 s a que el combo siguiente se rellene.
- Campos de texto: `WM_SETTEXT` y `EN_KILLFOCUS` enviado al padre. Es donde los asistentes leen el nombre.
- Los nombres son los comerciales del catálogo (`HMIGTO1300 (320x240)`). Si el asistente añade la resolución al nombre, se acepta el único elemento que empieza por `<nombre> (`.
- **Verificado**: si el asistente se abre nada más aparecer la ventana de Vijeo, Finish no hace nada. Hay que esperar unos 5 s con la ventana quieta (`STARTUP_SETTLE`).
- **Verificado**: el asistente creó un HMIG5U, que no tiene `<SoMachine>`.

**Navigator**

- `SysTreeView32` con id 2001: la raíz es el proyecto y sus hijos son los targets.
- Renombrar: `TVM_EDITLABELW` abre el editor de la etiqueta, `WM_SETTEXT` escribe el nombre y `TVM_ENDEDITLABELNOW` lo confirma. El `TVITEMW` de 32 bits se escribe en memoria de Vijeo (`VirtualAllocEx`), porque `Vijeo-Frame.exe` 6.3 es de 32 bits (`vijeo-ui/src/tree.rs`). Si Vijeo no acepta el nombre, abre un aviso: el MCP lo cierra y devuelve su texto como error.
- Borrar: se selecciona el nodo, se da el foco al árbol (`AttachThreadInput` + `SetFocus`), se envían `WM_KEYDOWN` y `WM_KEYUP` con `VK_DELETE` y se responde Yes (`IDYES`) a «Delete Target».
- **Verificado**: creando tres targets, borrando el segundo y añadiendo otro, el orden de las entradas de `/Services/NameServer` coincide con el de las carpetas `Targets\Target N` (`swx::target_names`).

**Compilación y Feedback Zone**

- Build All: `0xC09E`. Vijeo vacía la Feedback Zone al empezar, así que el MCP espera a que la salida cambie y después a que se quede quieta: 2 s tras un resumen reconocido, o 20 s si no se reconoce (otro idioma, o el resumen de validación). Tampoco puede haber diálogos abiertos.
- `project_build` devuelve todo el proyecto. `target_build` compila todo y deja los mensajes cuyo `context` es ese target.
- Con `path`, el `.vdz` se abre en una copia, se compila con Build All y se cierra sin guardar (quinta tanda).
- **Verificado**: Compact_Modbus SL_M221_Book da 2 errores y 2 warnings, sin el `1331`.
- La Feedback Zone es un `ListBox` (`LBS_HASSTRINGS`) con id 102 dentro de la barra 711. `LB_GETTEXT` la lee entre procesos (`vijeo-ui::feedback`).

**Export Project y lecturas**

- Export Project: `0xC147`. Abre el diálogo de fichero (tipo, combo `1136`; nombre, `1148`). El MCP elige el filtro `*.vxml`, escribe la ruta y pulsa Aceptar (`IDOK`). Vijeo sólo escribe `.vxml` si ese filtro es el primero (FUN_004ae510), y deja al lado una carpeta `<nombre>` con las imágenes.
- Sin `SupportsXMLImportExport = 1` el diálogo sólo ofrece `.zdat`, y el MCP devuelve `unsupported` con la clave que falta. El `0xC0A7` del recurso `MENU 1008` no abre nada.
- El texto del `.vxml` se cachea mientras no cambie la fecha del `.SwxCF` de la copia de trabajo.
- `vxml.rs` lee las variables (nombre completo con carpetas, tipo, origen, dirección y `<Description>`), los paneles (`BasePanels/Panel` como `BP:\ruta`, `MasterPanels`, `PopupWindows` y carpetas `Folder`), las acciones globales (`Trigger` y operaciones) y las referencias cruzadas (busca el nombre completo de cada variable en atributos y textos del resto del XML).

**Diálogos**

| Diálogo | Cómo se reconoce | Respuesta |
|---|---|---|
| Actualizar la versión del proyecto | Casilla `10030` y botón Yes (`6`) | Yes (`IDYES`) |
| Modelo no soportado | MessageBox con Yes y No y el texto «Target Type» | No (`IDNO`): Vijeo convierte el modelo |
| Tres o más instancias de Vijeo | MessageBox con OK y «instance(s) of Vijeo-Frame running» | OK (`IDOK`) |
| Asistentes y diálogo de fichero | Controles de los asistentes, `1148` y `1136` | Se esperan: los rellena el MCP |
| Progreso o pantalla de inicio | Sin botones | Se espera |
| Otro MessageBox | MessageBox sin respuesta prevista | Aceptar o Cancelar tras `--dialog-grace` (1,5 s por defecto); la operación falla con `modal_dialog` |
| Otro diálogo con botones | Cualquier otro | Tras 30 s la operación falla con `modal_dialog`; el diálogo queda abierto |

- Un MessageBox se reconoce por el `Static` con id `0xFFFF` y los botones con ids estándar (`dialog.rs`). Así no se confunde con los diálogos de progreso.
- Los botones de los MessageBox se pulsan con `BM_CLICK`. Los de los diálogos de fichero no atienden `BM_CLICK` y se pulsan con `WM_COMMAND BN_CLICKED`. Un `WM_COMMAND IDOK` no basta en los diálogos de Vijeo (§12.8).

**Dirty y limitaciones**

- `dirty` lo lleva el MCP. Cambia al renombrar el proyecto y al añadir, renombrar o borrar un target, y se limpia con `project_save`.
- El `*` del título no sirve para saberlo. Con Compact_Hardwired_M221 aparece justo después de guardar; con el ejemplo Modbus desaparece al guardar, y compilar lo pone.
- Lo que el usuario edite a mano en la ventana no se detecta, aunque `project_save` lo guarda.

**Qué queda de COM**

> Obsoleto desde la séptima tanda: ya no queda nada de COM en el MCP. `LoadXMLCommand` se eliminó y el import XML no tiene vía sin COM (§12.11).

- Sólo `LoadXMLCommand` (import): una instancia COM propia, sin proyecto abierto, que no valida y por tanto no añade el driver SoMachine (§12.6, §12.8).
- `swx::remove_added_somachine_drivers` (`swx.rs`) ya no se llama, porque el MCP no guarda por COM. `com.rs` y `ui_build.rs` se borraron del backend.
- `vijeo_status` lista como huérfanas las carpetas `MCP_TMP_<pid>_*` de procesos del MCP que ya no existen. No las borra.

### 12.11 Import XML sin COM: no viable (2026-11-02)

Séptima tanda. `LoadXMLCommand` sólo se podía llamar por COM, así que hacía falta una vía de import XML que no lo use. Se revisaron las que ofrece la propia interfaz de Vijeo (Import Project y las extensiones de importación). Ninguna sirve al MCP sin COM: el importador XML exige una licencia y la vía legacy exige escribir en el registro. Por eso se quitó `project_import_xml`.

- **Menú Import Project**: `WM_COMMAND 0xC094` al marco (recurso `MENU 1008` de `Main.ENU`; el `0xC0A7` de ese menú es Export Project, y el que funciona para exportar es `0xC147`). Abre un `CINDEFileDialog` «Import Project» (nombre `1148`, tipo `1136`, Abrir = `1`, casilla «Open Project After Import» id `6136`). El filtro sale de la lista de importadores de `INDEFileManager` (`FUN_63435666`, lista en `+0xd8`; cada importador aporta su slot 2). Si ningún importador acepta el fichero, aparece el MessageBox `0x180C` «Failed to import the Project. The format of the specified file may be invalid.» (título `0x11D4` «Import Project»; el de éxito es `0x11EE`, «The project %s imported successfully.»). `Vijeo-Frame.exe /Import <fichero.xml>` da el mismo error.
  - Verificado en vivo: con Vijeo normal sólo aparece «Vijeo-Designer Archive Files (*.vdz; *.zip)», y un `.vxml` o un `.xml` falla.
- **Lista de importadores** (`FUN_63431881`, en `INDEFileManager.dll`):
  - `CZipProjectImporter`: siempre (`.vdz`).
  - Un `CLegacyProjectImporter` por cada subclave de `HKLM\SOFTWARE\Schneider Electric\Vijeo-Designer63\Vijeo-Frame\Import Extension`, con los valores `Extensions` (`"*.ext;..."`), `Text_<idioma>` (el idioma es `CPFUtility::GetForceLanguage` en minúsculas, aquí `enu`) y `DLLPath`. Su Import llama a `FUN_634322d5`, que carga la DLL y su `InitVDImportExtensionDLL`.
  - `CXmlProjectImporter`, que acepta una raíz `<Project ` pública y hace `TransformToPrivate` por su cuenta, **sólo** si `ITISFrameWorkSinkEx` (IID `9d99c888-6b7d-48e7-88d7-2312a72a78cc`) devuelve `LicIsRegistered` ≠ 0 (slot +0x50) y `LicGetUpgradeLicence` con el bit `0x1000` (slot +0x4c). Es una comprobación de licencia: no se intenta eludirla.
- **`INDEXMLImport.dll` como extensión legacy**: exporta `InitVDImportExtensionDLL` y lee el XML **privado** (`//VijeoDesignerProject/ProjectProperties/ProjectName`). Escribe `<fichero>_LOG.TXT` junto al XML y termina en el método `+0x74` de una interfaz interna del marco (→ `FUN_004d0420`).
  - Se probó registrándola en `Import Extension\MCP_XML` (`tools/import_extension_xml.reg`). El constructor del legacy (`FUN_6343d1a7`) abre la clave y la subclave con acceso `0x2001F` (lectura, SetValue y CreateSubKey). Sin elevar falla con `ERROR_ACCESS_DENIED` (verificado con un exe x86 sin manifiesto), y el importador queda sin extensiones ni filtro. La virtualización de registro de Vijeo-Frame (token allowed=1, enabled=1) no lo arregla.
  - Funcionaría dando al usuario escritura sobre esa clave (`tools/import_extension_acl.ps1`), opción que el usuario descartó.
  - Al importar el `.reg`: `reg import ... /reg:32` con una ruta que ya incluye `WOW6432Node` deja la clave en `WOW6432Node\WOW6432Node`. Usar `/reg:64` o doble clic.
- **Conclusión**: sin COM, sin la licencia con el bit `0x1000` y sin cambiar permisos, no hay forma de importar un XML. `project_export_xml` y las lecturas del `.vxml` no dependen de ello y se mantienen.
- **Herramienta nueva**: `tools/ghidra_scripts/ScalarRefs.java`, que lista las funciones que usan un valor inmediato (p. ej. ids de recursos).

### 12.12 Escritura de la configuración de comunicación en el .vdz (2026-11-02)

Octava tanda. `target_config_set` cambia en un `.vdz`, sin abrir Vijeo, la IP de descarga del HMI y la IP, el puerto y el unit ID de los equipos. Escribe el mismo formato que describen §12.8 y §12.9. Código: `swx::edit_target` (`swx.rs`) y `vdz::replace_swx` (`vdz.rs`). Verificado en vivo (ver al final).

**Formato que se escribe**

- **Descarga**: en `Targets\Target N\PrimaryTarget\Document`, las cadenas 4.ª, 5.ª y 6.ª (índices 3, 4 y 5) son la IP, la máscara y la puerta de enlace. Son `CString` de MFC: `FF FE FF`, la longitud en caracteres (`u8`, o `FF` + `u16`, o `FF FF FF` + `u32` si es muy larga) y UTF-16. Se sustituyen de atrás adelante y la longitud puede cambiar. El `Document` no tiene offsets absolutos, igual que en `restore_target_documents` (§12.8).
- **Equipos**: en `Component N\DriverK\EquipmentJ\ProtocolConfig` (el `Component N` con `DriverList`, §12.9) sólo cambia la cadena `[4]T:0:<IP>:<puerto>:<unit ID>:...`, con su longitud. La versión y el resto (modo IEC + 3, segunda cadena vacía y contador; §12.8) se copian byte a byte, y la cadena sigue terminada en 0 si lo estaba. En Modbus TCP se cambian la primera IPv4 y los dos campos que la siguen (puerto y unit ID). En los demás protocolos sólo se acepta la IP.
- **Red del target**: después de las propiedades conocidas, el `Document` tiene otra terna IP, puerta de enlace y máscara: las tres primeras IPv4 seguidas a partir de la 9.ª cadena, cuya IP no es `0.0.0.0` (`swx::second_network_at`). En los modelos con dos Ethernet es la ETH2 (ver «Otros modelos» más abajo). Son las cadenas 21-23 en el ejemplo Modbus y en SET03, y no están en el XML. En el STO del ejemplo, de una sola Ethernet, repite la descarga (`172.20.10.168 / 0.0.0.0 / 255.255.0.0`). En el GTU de SET03 es otra red (`192.168.1.125 / 0.0.0.0 / 255.255.0.0`, la segunda Ethernet).
- **Sin tocar**: `DriverConfig` (`Clave=Valor`), la segunda Ethernet de los GTU y el resto de campos de los equipos. Son ampliaciones posibles, no de esta tanda.

**Validación de la puerta de enlace (sondas con `project_build path`, 2026-11-02)**

| Proyecto | Cambio en el `Document` | Resultado |
|---|---|---|
| Ejemplo Modbus (STO) | sólo la IP, sólo la máscara o sólo la puerta (`172.20.10.1`) | 2/2, como el original |
| Ejemplo Modbus (STO) | IP, máscara y puerta (`192.168.50.20/24`, `192.168.50.1`) | 4/2: `1126 [Subnet Mask] is invalid` y `1126 [Default Gateway] is invalid` |
| Ejemplo Modbus (STO) | lo mismo y también la terna de red | 2/2 |
| SET03 (GTU) | sólo la IP, sólo la máscara, o IP y máscara | 0/7, como el original |
| SET03 (GTU) | sólo la puerta `10.0.0.1`, o las tres (`10.0.0.126/24`, `10.0.0.1`) | 2/7: los mismos dos `1126` |
| SET03 (GTU) | la puerta `192.168.1.1`, sola o con las otras dos | 0/7 |

Conclusión: Vijeo valida la puerta de descarga contra la IP y la máscara de la terna de red, no contra las de descarga. Por eso `edit_target`:
- si la terna repite la IP de descarga antigua, la cambia con la descarga (IP, puerta, máscara; `item: "ethernet"`);
- si no, la deja, y rechaza (`invalid_argument`) una puerta distinta de `0.0.0.0` que no esté en su red;
- sin terna, exige que la puerta esté en la red de descarga.

Confirmado por el usuario (2026-11-02): en el HMI de SET03 (GTU), esa terna es la ETH2 que muestra la interfaz de Vijeo (`192.168.1.125 / 255.255.0.0`).

**Otros modelos (2026-11-02)**

Proyecto de prueba con un target por familia (GTO5310, GK5310, G3X, G3U, G5U, XBTGH2460 y S5T), creado con `project_new` y `target_add`. El SCU y el XBTGC no los ofrece el asistente porque piden licencia, y el iPC `Model_IPC_XBT_MPC56` se creó como `?`.
- **Recién creados**: la terna está a cero en todos (`0.0.0.0 / 0.0.0.0 / 255.255.0.0`). En la línea base, sin IP, el G3U y el G3X dan dos `1125 IP Address is empty` (dos Ethernet), el G5U ninguno y los demás uno.
- **Puerta de enlace con la terna a cero**: con la descarga `192.168.50.20/24` y la puerta `192.168.50.1` en los 7 targets (la terna no se toca porque está a cero), la compilación da 0 errores y 17 warnings. No aparece ningún 1126, y el 1125 desaparece en todos menos en el G3U y el G3X, que siguen dando uno: la ETH2 vacía. Si la terna está a cero, Vijeo valida la puerta contra la red de descarga, como ya hace `edit_target`.
- **Backups del usuario** (copias en el scratchpad):

| Proyecto | Modelo | Descarga | Terna (IP / puerta / máscara) |
|---|---|---|---|
| PROY-A1, PROY-A2 | S5T (STU) | 192.168.11.1 / 255.255.255.0 | 192.168.11.1 / 0.0.0.0 / 255.255.0.0 |
| ProyectoB | GTO6310 | 10.6.6.201 / 255.255.255.0 | 10.6.6.201 / 0.0.0.0 / 255.255.0.0 |
| PLANTA SET1 y SET3 | G3U (GTU) | 10.0.0.103 / 255.255.255.0 | 192.168.1.125 / 0.0.0.0 / 255.255.0.0 |
| PROYC | G3U (GTU) | 10.20.30.84 / 255.255.254.0, puerta 10.20.30.1 | **10.20.30.84** / 111.111.111.111 / 255.255.0.0 |

Conclusiones:
- En los modelos de una sola Ethernet, la terna repite la IP de descarga pero no su máscara: se queda en 255.255.0.0. Así que Vijeo no copia la máscara, y `edit_target` sí lo hace.
- **Fallo** (arreglado el mismo día): en PROYC, un GTU, la ETH2 tiene la misma IP que la descarga, y `edit_target` la tomaba por una copia. Un `dry_run` que sólo cambiaba la IP de descarga cambiaba también la IP, la puerta (111.111.111.111 → 10.20.30.1) y la máscara de la ETH2.
- **Corrección**: el catálogo (`Models_*.dat`, bloque `<Ethernet>`) marca con `<DualEthernetSupported>true` sólo el HMIG2U, el HMIG3U y el HMIG3X. Coincide con los dos 1125 de la compilación. `ModelInfo.dual_ethernet` lo expone en `catalog_list_models`, y `edit_target` recibe `dual_ethernet`: en esos modelos la terna es la ETH2, nunca se toma por copia y sólo sirve para validar la puerta. Un modelo que no está en el catálogo, como el `HMISTO531_532` de los ejemplos de Machine Expert, se trata como de una sola Ethernet.
- **Verificado**: tests unitarios (`keeps_eth2_of_dual_ethernet_targets_with_the_download_ip`); `live_config_set_dual_ethernet` con una copia de PROYC (`VIJEO_MCP_LIVE_DUAL`), donde sólo cambia la IP de descarga y la ETH2 queda igual; y `live_config_set_build_and_reopen` con el STO, que sigue actualizando su copia.
- **Otro hallazgo**: el bloque `<Ethernet>` también trae `SubnetValidation` (`AGP` o `None`; en los G5U y los iPC es `None`, y `GatewayRequired=false`). Sin resolver.

**Proyectos con historial de revisiones (2026-11-02, PROYC)**

- **Al guardar**: Save Project no guarda directamente. Abre un diálogo «Vijeo-Frame» (no es un MessageBox) con el Edit 11216, el Static 11217 «Revision History Comment:», el Static 11218 «Do you wish to save the changes you made to <proyecto>?» y los botones Yes/Cancel/No. `project_open` y `project_build path` fallaban con `modal_dialog`, porque abrir guarda la copia.
- **Respuesta del MCP** (`ui_project::classify`, `Answer::Fill`): escribe «Guardado por Vijeo MCP» en el comentario y pulsa Yes. Lo avisa en `notes`, porque cada guardado añade una entrada al historial del proyecto. Al cerrar sin guardar, `shutdown` responde No también a este diálogo.
- **Progreso de compilación**: en PROYC, Build All tarda más de 30 s y su diálogo «Building All» (barra `msctls_progress32` y Cancel) se tomaba por desconocido. Ahora un diálogo con barra de progreso se espera.
- **Verificado**: `live_config_set_build_and_reopen` con `VIJEO_MCP_LIVE_EXTRA` = copia de PROYC. Compila con 0/3 antes y después de cambiar la descarga y un equipo; la ETH2 no se toca y la puerta 10.0.0.1 se rechaza contra la ETH2.

**Dónde aparece cada IP**

Comprobación previa (2026-11-02, con una copia del backup PLANTA_SET3): cada IP aparece en un solo stream. La del HMI, y la segunda Ethernet de los GTU (192.168.1.125), sólo están en `PrimaryTarget\Document`. La de cada equipo sólo está en su `ProtocolConfig`. Por eso, cambiar esos streams no deja la IP antigua en otro stream.

**Por qué la vía sin Vijeo es segura**

- **Validación previa**: direcciones IPv4 válidas, máscara de bits contiguos, puerto distinto de 0, equipo identificado sin ambigüedad (por nombre y, si hace falta, driver) y puerto o unit ID sólo en Modbus TCP. Si algo falla, no se escribe ningún fichero.
- **Temporal**: el resultado va a `<destino>.mcp-tmp`, en la misma carpeta que el destino. Las demás entradas del ZIP se copian sin recomprimir.
- **Relectura y comparación**: se relee el temporal y se compara en JSON la configuración de todos los targets con la esperada, no sólo la del target editado. Sólo si coincide se renombra sobre el destino. Si no coincide, se borra el temporal y se devuelve `internal`.
- **Proyecto abierto**: un destino abierto con `project_open` se rechaza (`refused`), porque al guardar Vijeo lo sobrescribiría con su copia de trabajo, sin el cambio. Si sólo el origen está abierto con cambios sin guardar, la respuesta lo indica en `notes`.
- **Lo que no comprueba**: que Vijeo acepte el fichero. Eso se ve compilándolo con `project_build` y `path`, como indica la nota de la respuesta.

**Verificado en vivo** (`live_config_set_build_and_reopen`, en `crates\vijeo-backend-vd63\tests\live.rs`):
- Ejemplo Modbus, con las tres de descarga y el equipo cambiados (7 cambios con la terna de red): compila con 2/2, como el original. `project_open` del resultado funciona, y `project_save` (guardado por Vijeo) conserva las IP, la puerta y el unit ID nuevos. Repetir la misma petición no escribe nada. Con `save_as`, el origen no cambia.
- Copia de SET03 (`VIJEO_MCP_LIVE_EXTRA`), con la IP y la máscara de descarga y la IP de un equipo cambiadas: compila con 0/7 antes y después.
