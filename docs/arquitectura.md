# Arquitectura del MCP de Vijeo-Designer

> Estado: **aprobada con cambios** (D1–D5 resueltas, ver §13).
> Base empírica: `docs/vijeo-designer-6.3-api-inventory.md` (§12 validación, §12.7 catálogos, §12.9 modo SoMachine, §12.10 instancia normal sin COM y §12.11 import XML sin COM).

## 1. Objetivos

1. Exponer Vijeo-Designer como servidor MCP (stdio) escrito en Rust con `rmcp` 3.8.
2. **Multi-versión**: soportar 6.3 hoy y añadir versiones futuras sin tocar la capa MCP.
3. **Fiable**: sin bloqueos por diálogos modales, sin dejar procesos ni carpetas huérfanas, errores propios y explícitos (Vijeo no los da).
4. **Seguro**: nada que actúe sobre HMIs reales o borre datos sin opt-in explícito; nunca tocar el workspace `Vijeo-Manager` del usuario.

## 2. Capas y crates

```
            ┌───────────────────────────────────────────────┐
  stdio ◄──►│ vijeo-mcp      servidor rmcp, herramientas,   │
            │                sesión, configuración          │
            └──────────────┬────────────────────────────────┘
                           │ dyn Backend (traits de vijeo-core)
            ┌──────────────▼────────────────────────────────┐
            │ vijeo-core     dominio, Capability, errores,  │
            │                traits, detección de versión   │
            └──────────────▲────────────────────────────────┘
                           │ implementa
   ┌───────────────────────┴─────────┐    ┌─────────────────────┐
   │ vijeo-backend-vd63              │    │ vijeo-backend-vdXX  │  (futuras)
   │ reglas 6.3 + catálogo           │    └─────────────────────┘
   └───────┬──────────────────┬──────┘
           │                  │
   ┌───────▼───────┐  ┌───────▼───────┐
   │ vijeo-ui      │  │ vijeo-dat     │
   │ ventana, menús│  │ descifrado    │
   │ Navigator     │  │ catálogos     │
   └───────────────┘  └───────────────┘
```

| Crate | Responsabilidad | Depende de Windows |
|---|---|---|
| `vijeo-core` | Tipos de dominio, `Capability`, `VijeoError`, traits de backend, detección de instalaciones (interfaz) | No |
| `vijeo-ui` | Sin COM. Control de la ventana de Vijeo: `ui` (`UiInstance`, `UiDialog`), diálogos (`dialog`), Feedback Zone (`feedback`), Navigator (`tree`), procesos (`process`: localizar, vigilar y matar) e instalaciones (`install`) | Sí |
| `vijeo-dat` | Descifrado TEA de `.dat` (ya hecho, con tests) | No |
| `vijeo-backend-vd63` | Implementación 6.3: cada proyecto abierto es una instancia normal de Vijeo manejada por la interfaz (`ui_project`); lecturas del `.vxml` (`vxml`) y del `.SwxCF` (`swx`); empaquetado del `.vdz` (`vdz`); validaciones y catálogo de modelos | Sí |
| `vijeo-mcp` | Servidor rmcp, registro de herramientas por capacidades, estado de sesión, config | No directamente |

Regla: **la capa MCP sólo conoce `vijeo-core`**. Todo lo específico de versión vive en su backend.

## 3. Detección de versión y selección de backend

```rust
pub struct Installation {
    pub product_version: Version,      // FileVersion de Vijeo-Frame.exe (6.3.0.103)
    pub frame_dir: PathBuf,            // ...\Vijeo-Frame
    pub registry_key: String,          // ...\Vijeo-Designer63\06.30
}

pub trait BackendFactory: Send + Sync {
    fn id(&self) -> &'static str;                         // "vd63"
    fn supports(&self, inst: &Installation) -> Support;  // Exact | Compatible | No
    fn create(&self, inst: &Installation, cfg: &BackendConfig) -> Result<Box<dyn Backend>, VijeoError>;
}
```

- Se enumeran instalaciones en `HKLM\SOFTWARE\WOW6432Node\Schneider Electric\Vijeo-Designer*\<NN.NN>`.
- Un `BackendRegistry` (lista estática de factorías) elige la de mejor `Support`; `--vijeo-version` lo fuerza.
- `Compatible` permite arrancar una versión no probada con el backend más cercano, marcando las capacidades no verificadas como desactivadas por defecto.
- Desde la séptima tanda, `Vd63Factory::supports` sólo exige la versión 6.x y `Vijeo-Frame.exe`; ya no hace falta ningún ProgID registrado.

## 4. Capacidades

```rust
pub enum Capability {
    ProjectLifecycle,   // new/open/save/close/rename del proyecto (instancia normal)
    TargetManage,       // listar, añadir, renombrar y borrar targets
    Build,
    Reports,            // Export Project (.vxml) y lecturas de variables, paneles y acciones
    CrossReferences,
    VariableLookup,     // existencia y tipo de una variable, del .vxml
    PanelList,          // paneles base de un target, del .vxml
    ModelCatalog,       // catálogos descifrados
    DriverCatalog,      // drivers instalados (HMI IO\*_PDL.dll)
    ProjectConfig,      // drivers, equipos y descarga, leídos del .SwxCF
    ConfigEdit,         // IP de descarga y dirección de los equipos, escritas en el .SwxCF (sin Vijeo)
    XmlValidate,        // .vxml contra public.xsd (sin Vijeo)
    ScriptLint,         // scripts del .vxml contra sus variables y la API de scripts
    VariableCsv,        // CSV de Import Variables (sin Vijeo)
    DataConvert,        // DataConverter.exe (sin Vijeo)
    PanelOpen,          // abrir un panel en la ventana de Vijeo (Navigator)
    VariableImport,     // Import Variables de un CSV en un target abierto
    // Download / RetrieveSource: fuera de la primera versión (D3).
}
```

Cada backend devuelve un `CapabilitySet` con estado por capacidad: `Supported`, `Gated` (existe pero requiere opt-in de configuración) o `Unsupported { reason }`. El servidor registra todas las herramientas y desactiva con `ToolRouter::with_disabled` las que no aplican, así `tools/list` sólo muestra lo que funciona en la versión instalada.

## 5. Traits de backend

Segmentados por capacidad, para que una versión pueda implementar sólo parte:

```rust
pub trait Backend: Send + Sync {
    fn info(&self) -> BackendInfo;                 // versión, id, ruta
    fn capabilities(&self) -> &CapabilitySet;
    fn status(&self) -> BackendStatus;                     // instancias y carpetas temporales huérfanas
    fn projects(&self) -> Option<&dyn ProjectOps> { None }
    fn targets(&self)  -> Option<&dyn TargetOps>  { None }
    fn build(&self)    -> Option<&dyn BuildOps>   { None }
    fn reports(&self)  -> Option<&dyn ReportOps>  { None }
    fn variables(&self)-> Option<&dyn VariableOps>{ None }
    fn catalog(&self)  -> Option<&dyn CatalogOps> { None }
    fn config(&self) -> Option<&dyn ConfigOps> { None }
    fn files(&self) -> Option<&dyn FileOps> { None }   // validate_xml, variables_csv, convert_data (sin Vijeo)
    fn shutdown(&self) -> Result<(), VijeoError>;           // cierra los proyectos sin guardar y sus instancias
}

// Métodos síncronos (sin async_trait): la capa MCP los ejecuta con spawn_blocking.
pub trait ProjectOps {
    fn new_project(&self, name: &str, save_as: &Path, overwrite: bool, target: &str, model: &ModelSelector) -> Result<ProjectInfo, VijeoError>;
    fn open(&self, vdz: &Path) -> Result<ProjectInfo, VijeoError>;
    fn save(&self, p: ProjectId, to: Option<&Path>, overwrite: bool) -> Result<ProjectInfo, VijeoError>;
    fn close(&self, p: ProjectId, discard: bool) -> Result<(), VijeoError>;
    fn info(&self, p: ProjectId) -> Result<ProjectInfo, VijeoError>;
    fn list(&self) -> Vec<ProjectSummary>;
    fn rename(&self, p: ProjectId, name: &str) -> Result<ProjectInfo, VijeoError>;
}
pub trait TargetOps {
    fn list(&self, p: ProjectId) -> Result<Vec<TargetInfo>, VijeoError>;
    fn add(&self, p: ProjectId, name: &str, model: &ModelSelector) -> Result<TargetInfo, VijeoError>;
    fn delete(&self, p: ProjectId, target: &str) -> Result<(), VijeoError>;
    fn rename(&self, p: ProjectId, target: &str, new_name: &str) -> Result<TargetInfo, VijeoError>;
    fn panels(&self, p: ProjectId, target: &str) -> Result<Vec<PanelRef>, VijeoError>;
    fn open_panel(&self, p: ProjectId, target: &str, panel: &str) -> Result<PanelRef, VijeoError>;
}
// BuildOps, ConfigOps, ReportOps (con lint_scripts), VariableOps, CatalogOps y FileOps análogos.
```

Tipos de dominio (todos `Serialize + JsonSchema`): `ProjectId`, `ProjectInfo { id, name, path, dirty, targets, notes }`, `TargetInfo { name, hmi_type, model }`, `ModelInfo` (el de `models.json`), `ModelSelector` (código interno, nombre comercial o `modelo + pantalla`), `BuildResult { errors, warnings, duration_ms, messages, output, output_note, notes }`, `VariableInfo { target, name, data_type, source, address, description, extra }`, `PanelRef { path: "BP:\\Start" }`.

## 6. Capa de interfaz

### 6.1 Instancia normal por proyecto (sexta tanda)

- **Una instancia de Vijeo (un proceso `Vijeo-Frame.exe`) por proyecto abierto**, lanzada como ejecutable: `/Import <copia>` para abrir y sin argumentos para crear. Sin `-Embedding` no hay modo SoMachine (§12.9–§12.10). `Vd63Backend` guarda `ProjectId → Instance` en un `Pool`; un `Slot` reserva la plaza de `--max-instances` mientras la instancia arranca, y los arranques van de uno en uno.
- **`UiProject`** (`ui_project.rs`) es la sesión de una instancia. Su copia de trabajo vive en `Vijeo-Manager\MCP_TMP_<pid>_<n>`: `save` manda `Save Project` (`0xC098`) y `package` empaqueta la copia como `.vdz`; `close` manda `Close Project` (`0xC09A`) y responde No a «save changes?».
- **Control** (`vijeo-ui::ui`): los comandos de menú van como `WM_COMMAND` al marco; los botones se pulsan con `BM_CLICK` (los de fichero con `WM_COMMAND BN_CLICKED`); los combos se eligen con `CB_SETCURSEL` y sus notificaciones; los campos de texto se escriben con `WM_SETTEXT` y `EN_KILLFOCUS`. El Navigator (`vijeo-ui::tree`) y la Feedback Zone (`vijeo-ui::feedback`) se leen y se editan entre procesos.
- **Lecturas**: Export Project escribe el `.vxml` y `vxml.rs` lo parsea. El texto se cachea mientras no cambie el `.SwxCF` de la copia.
- **Diálogos** (`classify`): se responden los conocidos (actualizar versión → Sí; modelo no soportado → No; tres o más instancias → Aceptar). Los asistentes y el diálogo de fichero se esperan. Un MessageBox desconocido se cierra tras `dialog_grace` y la operación falla con `modal_dialog`; otro diálogo con botones falla tras 30 s y queda abierto. La tabla completa está en el inventario, §12.10.

### 6.2 Sin COM (séptima tanda)

- El MCP no usa COM: no hay instancias `-Embedding`, ni `ComServer`, ni hilo STA, ni `IDispatch`, ni conversión `VARIANT` ↔ `Value`. `vijeo-com` pasó a ser `vijeo-ui`, que sólo maneja la ventana, los diálogos, la Feedback Zone, el Navigator, los procesos y las instalaciones.
- Los diálogos se vigilan y se responden sólo desde la interfaz (`classify`, §6.1).
- El import XML (`LoadXMLCommand`, en una instancia COM propia) se eliminó con `project_import_xml`. Sin COM no hay otra vía (§12.11), así que el MCP no crea ni edita pantallas.

## 7. Backend 6.3 (`vijeo-backend-vd63`) — reglas incorporadas

| Regla (verificada) | Implementación |
|---|---|
| Por COM, Vijeo arranca en modo SoMachine (MSD): la validación añade `SoMachineNetwork01` sin dirección (falso `Error 1331`) y `Save` lo guarda (§12.9) | Los proyectos abiertos son instancias normales (`/Import`, sin `-Embedding`). El backend no guarda ni compila por COM (§12.10) |
| Nombre inválido → `MessageBox` modal | validar `\/:*?"<>\|#,;.` y sólo ASCII **antes** de abrir el asistente o la copia |
| ≥ 32 targets → modal | `validate::MAX_TARGETS` (32) en `add`, antes de abrir el asistente |
| Familia y modelo se eligen en el asistente por nombre comercial | `Catalog::resolve` y `family_name` dan los nombres del asistente; `find_item` busca cada uno en su combo |
| `hmi_type` interno + pantalla exacta | resolver con el catálogo; tras crear, `check_new_target` comprueba en el `.SwxCF` el modelo y la resolución, y deshace el target si no coinciden |
| Vijeo puede cambiar el nombre de un target nuevo | `check_new_target` lo renombra en el Navigator; si no lo acepta, error |
| Creables: familia activa y licencia estándar (ya no hace falta `<SoMachine>`) | `Catalog::creatable_reason`; 56 modelos en 6.3 (§13, D5). Con la caché de `--probe-models` manda lo probado: 39 en 6.3 (novena tanda) |
| Nombres de los targets de un `.vdz` sin abrir | `swx::target_names`: primer nodo de `/Services/NameServer`, en orden de creación (= carpetas `Target N`) |
| `hmi_type` de un target guardado | `Catalog::hmi_type_of`: modelo y pantalla sólo si la resolución identifica una pantalla del modelo |
| Sólo `.vdz` | rechazar otras extensiones (`check_vdz_input`, `validate::vdz_path`) |
| `GetLastErrorMessage` es stub | no se usa (ya no hay COM); los errores son propios de `VijeoError` |
| `GetPanel`, `ImportPanel`, `GetBindingOfVariable` stubs/rotos | sin capacidad `PanelObject` (quitada en la séptima tanda); el MCP no crea ni edita pantallas |
| `SetMode` puede abrir modal | nunca se llama |
| `Project.Download` ignora direcciones | nunca se expone |
| Una instancia sobrevive al MCP si éste muere de golpe | Cada `Vijeo-Frame.exe` lanzado entra en un job object con `KILL_ON_JOB_CLOSE` (`process::tie_to_this_process`): Windows lo termina al terminar el MCP (novena tanda) |
| `MCP_TMP_<pid>_*` queda si el proceso muere (`M5DTemPHolder<PID>` era de COM y ya no se crea) | `vijeo_status` las lista como huérfanas si su proceso ya no existe; no se borran |
| `Close` antes de salir | `shutdown()` cierra cada proyecto sin guardar (`Close Project`, respuesta No) y su instancia; también en `Drop` y ante Ctrl+C |
| Export Project sólo ofrece `.vxml` con `SupportsXMLImportExport = 1` | error `unsupported` que indica la clave |

Catálogo: al crear el backend se descifran `TargetInfo\*.dat` con `vijeo-dat` desde la instalación y se parsean con `roxmltree` (D4). Se cachea en memoria. La creabilidad: familia activa y licencia estándar (§13, D5), salvo que `--probe-models` haya probado el modelo en esta instalación (novena tanda).

Las reglas de COM sobre `OpenProject`, `NewProject`, `Save` y `Build` (§12.1–§12.2, §12.9) ya no se aplican a los proyectos abiertos (§12.10), y desde la séptima tanda el MCP no usa COM en absoluto (§12.11).

## 8. Herramientas MCP (iniciales)

| Herramienta | Capacidad | Anotaciones | Notas |
|---|---|---|---|
| `vijeo_installations` | — | read-only | Sin Vijeo. Instalaciones y backend elegido |
| `vijeo_status` | — | read-only | Capacidades, instancias (`starting`, `ready`, `busy`, `crashed`), proyectos abiertos y carpetas temporales huérfanas |
| `catalog_list_models` | ModelCatalog | read-only | Filtros: familia, `creatable`, texto |
| `catalog_list_families` | ModelCatalog | read-only | Familias del catálogo |
| `catalog_list_drivers` | DriverCatalog | read-only | Sin Vijeo: `HMI IO\*_PDL.dll`; filtros por fabricante, medio y texto |
| `project_new` | ProjectLifecycle | no destructiva | Asistente New Project con el primer target (obligatorio); guarda en `save_as`; falla si existe salvo `overwrite=true` |
| `project_open` | ProjectLifecycle | read-only | Sólo `.vdz`; abre su instancia de Vijeo sobre una copia |
| `project_info` | ProjectLifecycle | read-only | Nombre, ruta, `dirty` (cambios del MCP sin guardar) y targets |
| `project_save` | ProjectLifecycle | destructiva | Guarda la copia y empaqueta el `.vdz` en su ruta o en `save_as` (`overwrite` para reemplazar) |
| `project_close` | ProjectLifecycle | destructiva | Cierra sin guardar; falla con cambios sin guardar salvo `discard=true` |
| `project_rename` | ProjectLifecycle | no destructiva, idempotente | Nombre que se usa al empaquetar; queda pendiente de `project_save` |
| `target_list` | TargetManage | read-only | Nombre + `hmi_type` canónico, de la copia de trabajo |
| `target_add` | TargetManage | no destructiva | Asistente New Target; modelo por código, nombre comercial o modelo+pantalla |
| `target_rename` | TargetManage | no destructiva, idempotente | Editando la etiqueta en el Navigator; queda pendiente de `project_save` |
| `target_delete` | TargetManage | **destructiva** | Navigator (Supr) y confirmación Yes |
| `panel_list` | PanelList | read-only | Rutas `BP:\...` del `.vxml` |
| `project_build` / `target_build` | Build | no destructiva | Build All; nº de errores y warnings y el texto de cada uno (Feedback Zone). `project` compila el estado actual del proyecto abierto; `path` compila una copia del `.vdz` sin tocarlo |
| `project_messages` | Build | read-only | Contenido actual de la Feedback Zone de la instancia |
| `target_config` | ProjectConfig | read-only | Drivers, equipos y propiedades de descarga: de la copia de trabajo (`project`) o de cualquier `.vdz` (`path`) |
| `target_config_set` | ConfigEdit | destructiva, idempotente | Sin Vijeo: IP, máscara y puerta de enlace de descarga del HMI, e IP, puerto y unit ID de los equipos de un `.vdz`. Reescribe `path` o, con `save_as`, escribe otro; `dry_run` sólo calcula. Rechaza un destino abierto con `project_open` |
| `project_export_xml` | Reports | no destructiva | Export Project: `<dir>\<proyecto>.vxml` y su carpeta de imágenes |
| `variable_list` | Reports | read-only | Variables del `.vxml` (nombre completo, tipo, origen, dirección, descripción) |
| `panel_report` | Reports | read-only | Paneles base, maestros y emergentes con sus acciones de panel, del `.vxml` |
| `action_list` | Reports | read-only | Acciones globales, del `.vxml` |
| `variable_lookup` | VariableLookup | read-only | Existencia y tipo de una variable, del `.vxml` |
| `project_cross_references` | CrossReferences | read-only | `{count, references, unused}`; `target` opcional |
| `script_lint` | ScriptLint | read-only | Scripts del `.vxml` (o el texto de `script`) contra las variables del target y la API de scripts; sólo lista los que tienen avisos salvo `include_clean` |
| `project_goto` | PanelOpen | no destructiva, idempotente | Abre un panel base en la ventana de Vijeo (Navigator + Intro); acepta `BP:\…`, la ruta o el nombre si no se repite |
| `xml_validate` | XmlValidate | read-only | Sin Vijeo: `System.Xml` contra `XML\public.xsd` (corregido en memoria); errores con línea y columna y resumen por mensaje |
| `variables_generate_csv` | VariableCsv | no destructiva, idempotente | Sin Vijeo: CSV de Export/Import Variables (OEM o Unicode); con `project`, avisa de nombres repetidos, grupos de escaneo desconocidos e idioma |
| `variables_import` | VariableImport | destructiva | Import Variables en un target del proyecto abierto, de un CSV (`csv`) o de la lista `variables` (se genera el CSV OEM en la carpeta de trabajo); conserva las existentes salvo con `replace_existing`; devuelve `imported` y la salida de Vijeo. No funciona tras un `project_build` en la misma instancia |
| `data_convert` | DataConvert | destructiva | Sin Vijeo: `DataConverter.exe` (`.rcp`/`.dat` → CSV/TXT, CSV → `.rcp` con `recipe_dir`); devuelve los ficheros escritos y la salida |

`ProjectId` se devuelve en `project_new/open` y se pasa al resto; el servidor mantiene la tabla de sesión (proyecto → instancia). Varios proyectos pueden estar abiertos a la vez, cada uno en su instancia. La descarga a HMI queda fuera de la primera versión (D3).

## 9. Errores

`VijeoError` (en `vijeo-core`) con variantes estables, mapeadas a resultados de herramienta con `is_error = true` y un `code` legible por el modelo:

`NotInstalled`, `Timeout { op, ms }`, `ModalDialog { title, text }`, `InvalidArgument { field, reason }`, `Unsupported { capability, version, reason }`, `ServerCrashed { op }`, `ProjectNotFound`, `TargetNotFound`, `Limit`, `Refused`, `Io` e `Internal`. El `code` que ve el modelo es `not_installed`, `timeout`, `modal_dialog`, `invalid_argument`, `unsupported`, `server_crashed`, `project_not_found`, `target_not_found`, `limit`, `refused`, `io` e `internal`. Desde la séptima tanda se quitaron `NotAvailable`, `ComFailure` y `NullResult` (y sus códigos), que sólo producía COM.

`kills_instance()` (`timeout`, `modal_dialog` y `server_crashed`) lo usaba sólo la capa COM y se borró en la séptima tanda. En la capa de interfaz, `timeout` y `modal_dialog` no matan la instancia: queda como estaba (un aviso ya cerrado, o una operación o un diálogo en curso), y `project_close` la cierra.

## 10. Seguridad y efectos

- Nunca se abre ni guarda nada dentro de `Vijeo-Manager`; `project_open` sólo acepta `.vdz` y `project_save` rechaza rutas bajo el workspace. La única excepción son las copias de trabajo `MCP_TMP_<pid>_<n>` que Vijeo crea allí, que se borran al cerrar. Las copias de trabajo van a `%TEMP%\vijeo-mcp\`.
- `target_config_set` escribe un `.vdz` sin Vijeo, con la misma validación de ruta que `project_save`. Escribe `<destino>.mcp-tmp` junto al destino, relee el fichero y compara la configuración de todos los targets con la esperada; sólo si coincide renombra el temporal sobre el destino (si no, lo borra y falla). Rechaza un destino abierto con `project_open`. Si el origen está abierto con cambios sin guardar, lo indica en `notes`.
- Herramientas destructivas marcadas con `destructive_hint`; no hay ninguna herramienta que actúe sobre HMIs reales en la primera versión.
- Arranque perezoso: Vijeo sólo se lanza con la primera herramienta que lo necesita; `vijeo_installations` y el catálogo funcionan sin lanzar Vijeo.
- Logs (`tracing`) a stderr/fichero, nunca a stdout (stdout es el canal MCP).

## 11. Añadir una versión nueva

1. Volcar typelibs (`tools/dump_typelibs.ps1`) y comparar con 6.3.
2. Ghidra headless con `DecompileByString.java` sobre los mismos métodos (`CVJDTarget_Internal2::GetPanel`, etc.) para ver qué se arregló o rompió.
3. Validar en vivo con `tools/com_probe.ps1` (pasos `basic`, `fixes`, `models`).
4. Crear `vijeo-backend-vdXX` (o reutilizar `vd63` con una tabla de diferencias si los cambios son menores) e implementar `BackendFactory::supports`.
5. Registrar la factoría en `BackendRegistry`. La capa MCP no cambia.
6. Si cambian los ids de menú, de asistentes o de controles (§12.10), actualizar las constantes de `ui_project.rs` y verificarlas en vivo.

## 12. Pruebas

- Unitarias: `vijeo-dat` (hecho) y validaciones y selección de modelo en `vd63`.
- Integración en vivo (feature `live-vijeo`, desactivada por defecto): sobre copias de los `.vdz` de ejemplo de Machine Expert Basic en un directorio temporal.
- Humo MCP: lanzar el binario, enviar `initialize` + `tools/list` + `tools/call` por stdio y verificar respuestas.

## 12bis. Estado de la implementación (2026-11-06)

Implementado con 6.3.0.103: los cinco crates de §2, las 27 herramientas de §8 y las pruebas de §12 (unitarias, `live-vijeo` y humo MCP por stdio). Cada tanda indica lo verificado en vivo. Desde la séptima tanda el MCP no usa COM: las menciones de `CoCreateInstance`, `ComServer`, `LoadXMLCommand` o `vijeo-com` de las tandas anteriores describen el estado de entonces.

Hallazgos y decisiones tomadas al implementar:

- **Instalaciones**: la ruta de `Vijeo-Frame` está en `HKLM\SOFTWARE\Schneider Electric\Vijeo-Designer63\06.30\Path\Path` (vista de 32 bits). Un ProgID sólo cuenta para una instalación si su `LocalServer32` apunta a ese `Vijeo-Frame.exe`.
- **PID de la instancia**: diferencia de procesos `Vijeo-Frame.exe` antes y después de `CoCreateInstance`, con los arranques serializados y contando sólo los hijos del lanzador COM (ver la segunda tanda).
- **Vigilante**: reconoce un `MessageBox` por el `Static` con id `0xFFFF` y los botones con ids estándar (así no confunde los diálogos de progreso). Verificado con `NewProject("mal.nombre")`: detecta el diálogo, lee su texto y cierra la instancia en ~1,5 s.
- **Hilo STA**: bombea mensajes mientras espera trabajo.
- **`GenerateAllReports`** devuelve `E_FAIL` si la carpeta no existe: el backend la crea antes. Los informes HTML están en **UTF-16LE con BOM**.
- **Guardar**: se guarda en un `.vdz` temporal junto al destino y se renombra encima. Nunca queda un fichero a medias y no se depende de un posible diálogo de «sobrescribir».
- **D5 (creabilidad)**: `<SoMachine>` + familia activa + `RequiredKey` dentro de la licencia supuesta `0x0FFFFFFF` (todo salvo productos aparte como Easy iPC, `0x10000000`). Da exactamente los 29 modelos validados en vivo. `--probe-models` queda pendiente. *(Hecho en la novena tanda.)*
- **Instancia caída**: el proyecto se pierde. Las herramientas devuelven `server_crashed` hasta que se llama a `project_close`; no se reabre solo.

Segunda tanda (2026-11-01): `project_rename`, `target_rename`, `catalog_list_drivers`, `panel_report` y `action_list`, verificadas en vivo.

- **`put Project.Name`** funciona y persiste al guardar, pero **deja `IsDirty()` a 0** aunque haya cambios pendientes, incluidos los anteriores. El backend lleva una marca propia (`Instance::unsaved`) que suman `project_info` y `project_close` y que limpia el guardado. Por si acaso, renombrar un target también la activa.
- **`put Target.Name`** funciona. Cambiar sólo mayúsculas (`uno` → `UNO`) devuelve `E_FAIL`, porque Vijeo compara sin distinguir mayúsculas y lo ve como duplicado: se rechaza antes con `invalid_argument`. Si Vijeo devolviera otro nombre, se restaura el anterior.
- **Drivers**: el fabricante y el nombre comercial están en tablas de strings de `<id>_PDL_Lang.<idioma>` (DLL de recursos), pero con IDs distintos en cada driver (1106/1107 en Modbus TCP, 1227/1228 en Siemens TCP, 1013/1012 en los USB, y en otros no hay fabricante). Por eso el fabricante, el protocolo y el medio se deducen del nombre del fichero. La compatibilidad entre driver y modelo vive en condiciones XPath compiladas en cada `*_PDL.dll` (`FINDCURRENT/IO/Ports/...` contra el bloque `<IO><Ports>` de `Models_*.xml`); no se expone.
- **Informes de paneles**: un bloque por panel separado por `<hr>`, con el título `Grupo\Ruta` (`Base Panels\Start`, `Master Panels\LXM`) y la miniatura `.emf`, seguido de sus acciones de panel o de `No Actions`. Las rutas de los paneles base coinciden con `GetPanelNames` cambiando `Base Panels\` por `BP:\` (verificado). Ningún ejemplo disponible trae acciones de panel: se parsean con el mismo formato que el informe de acciones, que no está verificado.
- **Informe de acciones**: una tabla por acción con `<n> | <disparador>`, filas `Description:` e `Interlock:`, el tipo (`Script`) y el código en `<pre>`, con `<br>` como salto de línea.
- **`GenerateAllReports` devuelve `E_FAIL` si el proyecto no compila**. Un target recién creado por COM tiene 1 error; `Compact_Modbus SL_M221_Book.vdz` tiene 3. Sin targets sí funciona. Ante ese `E_FAIL`, el backend compila y lo explica en el error. En `Packaging_TempCtrl_HW_M221.vdz` abre además un `MessageBox` («Error 1130: [Equipment] [SOM_Target1] [Name] is invalid. Name already in use.»), que el vigilante cierra (`modal_dialog`).
- **Arranques**: con tres `project_open` simultáneos, uno acabó en `not_available` y otro en `timeout`. Ahora el backend serializa el arranque completo (`launching`), además del `CoCreateInstance` que ya serializaba `vijeo-com`. Dos aperturas en paralelo funcionan. Con una instancia interactiva del usuario abierta, la tercera instancia del MCP dio timeout en `CoCreateInstance` (posible límite de procesos, sin verificar).
- **Instancias propias**: un `Vijeo-Frame.exe` lanzado por COM tiene como padre `svchost.exe` (DcomLaunch) y la línea de comandos `-Embedding`; uno abierto por el usuario cuelga de `explorer.exe` (verificado). `ComServer::start` sólo cuenta como nuevos, para el PID y para matar tras un timeout, los procesos cuyo padre es `svchost.exe` (`process::com_launched`). Un Vijeo que el usuario abra durante la espera ya no se toca. Si el padre ya no existe, el proceso no cuenta.

Tercera tanda (2026-11-01): mensajes de compilación (`project_build`/`target_build` con texto, `project_messages`) y `target_config`, verificadas en vivo y con copias de proyectos reales.

- **Mensajes de compilación**: Vijeo no los escribe en disco ni los da por COM (`GetLastErrorMessage` es un stub). La Feedback Zone es un `ListBox` estándar (`LBS_HASSTRINGS`, id 102) dentro de la barra de id 711; `LB_GETTEXT` funciona entre procesos sin inyectar nada (`vijeo_com::feedback`, con `SendMessageTimeout`). Se busca por ids porque el título depende del idioma. **Vijeo la vacía al empezar cada compilación** (23 líneas → 18 al recompilar), así que tras `Build` todo su contenido es de esa compilación. Cada línea lleva un marcador de tipo: `\x01\x02` información, `\x01\x0D` warning, `\x01\x0B` error; la severidad sale de ahí y no del idioma. Las cabeceras `Target - X` / `X - HMI` / `X - I/O` dan el `context` de cada mensaje.
- **Formato del proyecto**: el `.vdz` es un ZIP con `<Proyecto>.SwxCF` (OLE compuesto, crate `cfb`). Por target (`Targets\Target N`):
  - `PrimaryTarget\Document`: `CArchive` de MFC (cadenas `FF FE FF <len>` + UTF-16). Orden verificado en GTO, STO y GTU: familia, descripción, modelo, IP, máscara y puerta de enlace de descarga (coinciden con `Download/@IPAddress`, `@SubnetMask`, `@DefaultGateway` de `XML\private.xsd`), ruta de descarga, puerto serie y, justo detrás, el baud rate (`u32`). El resto (segundo puerto Ethernet de los GTU, NTP…) va sin etiquetar en `other_values`.
  - `Component 2` es el I/O Manager: `DriverList` (nombre, fabricante, id `*_PDL_*`, carpeta `DriverK`), `DriverK\DriverConfig` (`Clave=Valor\r\n` en UTF-16), `DriverK\ProtocolList` (nombre, protocolo `*_PCL_*`, carpeta `EquipmentJ`) y `EquipmentJ\ProtocolConfig` (`[4]T:0:<IP>:<puerto>:<unit id>:...` en Modbus TCP). Las subcarpetas de cada equipo son sus grupos de escaneo.
  - `Services\NameServer` empieza por el nombre del primer target; con varios targets y sin Vijeo, el nombre queda como la carpeta (`Target 2`). Con el proyecto abierto se emparejan por orden con `GetTargetNames`.
- `target_config` lee el `.vdz` guardado: con cambios sin guardar lo indica (`unsaved_changes`). *(Desde la sexta tanda, `unsaved_changes` va siempre a `false`.)* El modelo del fichero puede no coincidir con `Target.Type()`: el ejemplo Modbus guarda `Model_HMISTO531_532` y Vijeo lo abre como `Model_HMIGTO6310`; se devuelven los dos.

Cuarta tanda (2026-11-01): `project_import_xml`, verificada en vivo (test `live_import_xml_roundtrip`, 5 ejecuciones seguidas sin fallos). Detalle del formato y del análisis en el inventario, §12.8. Obsoleto desde la séptima tanda: `project_import_xml` se eliminó (§12.11).

- **Flujo** (`xml_import.rs` + `Vd63Backend::import_steps`): se inspecciona el XML con `roxmltree` (formato, nombre, paneles, imágenes) y se rechaza si falta alguna imagen. El helper `xml_to_private.ps1`, embebido con `include_str!` y ejecutado con el PowerShell de SysWOW64, convierte a privado con `INDEXML!TransformToPrivate`, pone el nombre temporal `MCP_TMP_<pid>_<n>` y copia las imágenes a `img\`. Una instancia propia sin proyecto hace `LoadXMLCommand` y sale. Después se empaqueta `Vijeo-Manager\MCP_TMP_*` como `.vdz` (como `Project.Save`: `<nombre>.SwxCF`, `DataFilesBackup/`, `SymbolTables/`), se borra esa carpeta, y el `.vdz` se abre en otra instancia para devolverle el nombre del XML y guardarlo en `save_as`. Son unos 19 s con el ejemplo de 12 paneles.
- **Fallo intermitente de Vijeo**: `FUN_004d0420` hace `local_100 |= 1` cuando `RelativePaths` es true sobre una variable que nunca inicializa. Con basura en el bit 0 trata las rutas absolutas como relativas a la carpeta del XML y falla con el modal «Error 2500: Unable to open file <carpeta del XML>\<ruta absoluta>». Pasó en 3 de 4 ejecuciones del test y en 1 de unas 10 desde PowerShell. Con rutas relativas y `RelativePaths="true"` el bit queda siempre a 1.
- **Export**: no se implementó en esta tanda (obsoleto desde la sexta: el MCP exporta desde una instancia normal, §12.10). El diálogo «Export Project» (`WM_COMMAND 0xC147`) sólo ofrece `.vxml` si `Options\SupportsXMLImportExport` ≠ 0 **y** el indicador de la aplicación `+0x1C8` ≠ 1, pero `InitInstance` lo pone a 1 cuando Vijeo arranca con `-Embedding` (desensamblado en `00493D49`), como cualquier instancia COM. Abrir Vijeo como proceso normal (`/import`) importa el proyecto al workspace con su nombre real y pregunta si se convierte de versión. Además, el diálogo rechazó en silencio el guardado en `.vxml` pulsado por programa.
- **`restore_from`** (2026-11-01, test `live_import_xml_restore`): el `.vdz` original se lee con `swx` antes del import; `xml_import::restore_patch` lo convierte en un XML `<Restore>` que el helper aplica sobre el privado (sólo rellena valores vacíos; equipos por driver y nombre, o por nombre si es único). Después se lee el `.vdz` guardado y `xml_import::compare` (incluida la resolución de pantalla), `compare_data_files` y `compare_texts` devuelven `differences`. `compare_texts` compara, por zona del `.SwxCF` (`swx::TEXT_AREAS`: seguridad, alarmas, LangManager, TagDatabase, NameServer, recursos, recetas…), los textos del original que no están en el importado. Los paneles y la biblioteca de imágenes quedan fuera porque Vijeo renumera sus almacenes y regenera sus IDs, así que el resultado hay que compilarlo. Con `ConfigString` relleno, Vijeo ignora `IECAddressMode` y deja el modo IEC del equipo en −1 (las variables `%MW…` dan `Error 4330`), así que, tras empaquetar el `.vdz` temporal y antes de abrirlo, `swx::copy_protocol_configs` copia del original el `ProtocolConfig` de los equipos que el helper rellenó (líneas `EQUIPMENT` de su salida). Verificado con PLANTA_SET3: sin errores 4330 (inventario, §12.8). En el mismo punto, `swx::restore_target_documents` recupera del original el display (los bytes no textuales de `PrimaryTarget\Document`, si la resolución cambió) y las cadenas que el import deja vacías o por defecto, sólo si los dos documentos tienen la misma forma. El helper pasa además los usuarios de seguridad sin `UserKey` (proyectos creados con 6.1) al formato de 6.3: `UserKey` + `/Project/UserDatabase`. Siempre, también sin `restore_from`, `swx::fix_alarm_messages` corrige los mensajes de alarma de las variables. `LoadXMLCommand` los deja desplazados respecto a sus textos de `LangManagerData`: el desplazamiento se calcula y se comprueba con los mensajes del XML (`XmlSummary::alarm_messages`), y lo guardado se compara con ellos (`check_alarm_messages`).

Quinta tanda (2026-11-01): compilar un `.vdz` sin COM (`project_build`/`target_build` con `path`), verificado en vivo (test `live_build_file_without_com`) y con una copia de PLANTA_SET3. Detalle en el inventario, §12.9.

- **Por qué**: toda instancia COM arranca con `-Embedding`, e `InitInstance` pone el modo `eIVJDMode_MSD` (SoMachine). En ese modo la validación (compilar, generar informes) añade a cada target un driver `SoMachineNetwork01` sin dirección. Ese driver da `Error 1331` al compilar y `Project.Save` lo guarda en el `.vdz`. Una instancia lanzada como ejecutable queda en modo normal.
- **Flujo** (`vijeo_com::ui::UiInstance` + `ui_build.rs`; desde la sexta tanda `ui_build.rs` se borró y su lógica está en `ui_project.rs`): la copia lleva un nombre temporal (`xml_import::copy_with_project_name` sólo renombra la entrada `.SwxCF` del ZIP), porque `/Import` la desempaqueta en `Vijeo-Manager\<nombre>`. Se lanza `Vijeo-Frame.exe /Import <copia>` (stdio a null) y se espera a que el título del marco empiece por `<nombre> - `. Se responden tres diálogos conocidos, y cada uno se avisa en `notes`:
  - la actualización de versión (casilla 10030 → Yes);
  - el modelo no soportado (texto «Target Type» → No = convertir);
  - «3 instance(s) of Vijeo-Frame running» → OK.
- **Compilación**: Build All es `WM_COMMAND 0xC09E` y la salida se lee de la Feedback Zone, como en COM. La compilación está acabada cuando el último resumen y la salida llevan 2 s quietos y no queda ningún diálogo. El resumen se reconoce por su forma (`Main.ENU`: `… - %1 %2(s), %3 %4(s)` o, sin errores, `… - %1 %2(s)`). Si no empieza por `Build ` (otro idioma, o es el de validación) se esperan 20 s.
- **Cierre**: Close Project (`0xC09A`) → «save changes?» → No → `WM_CLOSE`. Ante cualquier imprevisto se termina el proceso (también en `Drop`), y la carpeta temporal se borra siempre.
- **Diálogos desconocidos**: un `MessageBox` desconocido aborta tras `dialog_grace` (`modal_dialog`); cualquier otro diálogo con botones, tras 30 s. El proceso cuelga del MCP, así que `process::com_launched` no lo confunde con uno COM.
- **Resultados**: el ejemplo Modbus da 2 errores y 2 warnings (por COM, 3 errores con el `1331`); PLANTA_SET3 da 0 errores y 7 warnings en ~25 s, con runtime generado. Cuenta para `max_instances` mientras dura. Con `target`, compila todo y deja sólo los mensajes cuyo `context` es de ese target.
- **Guardar por COM sin el driver** (test `live_save_drops_somachine_driver`, ya eliminado; obsoleto desde la sexta tanda, el MCP ya no guarda por COM):
  - Al abrir (o importar), cada instancia guarda una `swx::IoBaseline` del `.vdz` de partida: sus drivers por target y el siguiente id libre de `NameServer`. Un `project_new` la toma tras su primer guardado.
  - En cada `save_to`, sobre el temporal y antes de renombrarlo, `swx::remove_added_somachine_drivers` quita los drivers SoMachine sin dirección que no estaban en esa base. Con ellos van su registro en `DriverList` y sus nombres en `NameServer`, que tienen que ser posteriores a la base.
  - Lo quitado, o lo que no se pudo comprobar, va en `ProjectInfo.notes` (`project_save`) o en los `warnings` del import. Si el formato no encaja, el fichero no se toca.
  - La E/S de cada target se busca en el `Component N` que tiene `DriverList` (`swx::io_path`), no siempre en el 2. Esto arregla también `target_config` con el ejemplo Hardwired.

Sexta tanda (2026-11-01): backend sin COM. Los proyectos abiertos son instancias normales de Vijeo manejadas por la interfaz; COM quedaba sólo para `LoadXMLCommand` (obsoleto desde la séptima tanda, que lo quitó). Detalle y ids en el inventario, §12.10.

- **Por qué**: toda instancia COM arranca en modo SoMachine (§12.9). La validación añadía a cada target un `SoMachineNetwork01` sin dirección (falso `Error 1331`), y `Save` lo guardaba en el `.vdz`. Un `Vijeo-Frame.exe` lanzado como ejecutable queda en modo normal.
- **Ciclo de vida** (`ui_project.rs`, `Vd63Backend`): `project_open` copia el `.vdz` como `MCP_TMP_<pid>_<n>.vdz` (el nombre del proyecto es sólo el de la entrada `.SwxCF`), lo abre con `Vijeo-Frame.exe /Import` y guarda una vez. `project_new` crea el proyecto con el asistente New Project. `project_save` guarda la copia y la empaqueta (`vdz::package_vdz`) en `save_as`. `project_close` cierra sin guardar. `project_rename` sólo cambia el nombre que se usa al empaquetar. Añadir, renombrar o borrar targets guarda la copia de trabajo al terminar.
- **Asistentes, Navigator y combos**: New Project es `0xC095` y New Target `0xC418`, con los ids de §12.10. Los combos se eligen con `CB_SETCURSEL` y `CBN_SELENDOK`, `CBN_SELCHANGE` y `CBN_CLOSEUP`. Renombrar y borrar targets se hace en el Navigator (`SysTreeView32` 2001): editando la etiqueta, o seleccionando el nodo y pulsando Supr.
- **Hallazgo**: si se abre el asistente nada más aparecer la ventana de Vijeo, Finish no hace nada. Hay que esperar unos 5 s con la ventana quieta (`STARTUP_SETTLE`).
- **Alta de targets**: `target_add` valida el modelo contra el catálogo, abre New Target y comprueba en el `.SwxCF` el target nuevo (nombre, modelo y resolución). Si no cuadra, lo deshace. Verificado creando un HMIG5U, que no tiene `<SoMachine>`.
- **Nombres sin abrir**: `swx::target_names` toma los nombres de las entradas del primer nodo de `/Services/NameServer`, en orden de creación, que coincide con las carpetas `Targets\Target N`. Verificado creando tres targets, borrando el segundo y añadiendo otro.
- **Modelos**: `Catalog::hmi_type_of` deduce el `hmi_type` del modelo guardado y de la resolución; en los modulares, la pantalla sólo se pone si es la única del catálogo con esa resolución. `creatable` ya no exige `<SoMachine>`, que era de la API COM: basta con la familia activa y la licencia estándar. Hay 29 modelos con `<SoMachine>` y 56 creables en total.
- **Compilación**: Build All es `0xC09E` y la salida se lee de la Feedback Zone. Vijeo la vacía al empezar, así que recompilar sin cambios acaba con las mismas líneas: la espera detecta el cambio. `target_build` filtra los mensajes por el target. Verificado con Compact_Modbus SL_M221_Book: 2 errores y 2 warnings, sin el `1331`.
- **Dirty**: `dirty` refleja sólo los cambios hechos por el MCP. El `*` del título no sirve: con Compact_Hardwired_M221 aparece justo después de guardar, y con el ejemplo Modbus desaparece al guardar (compilar lo pone).
- **Lecturas del `.vxml`**: Export Project (`0xC147`) necesita `SupportsXMLImportExport = 1`; sin él no hay filtro `.vxml` y la herramienta devuelve `unsupported`. Hay que elegir el tipo `.vxml`. La caché se invalida cuando cambia la fecha del `.SwxCF` de la copia. `vxml.rs` lee variables, paneles, acciones globales y referencias cruzadas.
- **`target_config`**: con `project` lee la copia de trabajo. El campo `unsaved_changes` va siempre a `false`: no se calcula.
- **Herramientas**: `project_reports` desaparece y la sustituye `project_export_xml`. `project_cross_references` devuelve `{count, references, unused}` y acepta `target`. El total era 27 (26 desde la séptima tanda).
- **Qué queda de COM**: sólo `LoadXMLCommand` (obsoleto desde la séptima tanda: ya no queda COM). El guardado ya no llama a `remove_added_somachine_drivers` (sigue en `swx.rs`), y `com.rs` y `ui_build.rs` se borraron del backend. `vijeo_status` lista como huérfanas las carpetas `MCP_TMP_<pid>_*` de procesos que ya no existen; no las borra.

Séptima tanda (2026-11-02): sin COM. Se eliminó la API COM del MCP y, con ella, el import XML. La búsqueda de una vía alternativa está en el inventario, §12.11.

- **Por qué**: el import XML (`LoadXMLCommand`) era lo único que usaba COM, y sin COM no hay otra vía. El menú Import Project sólo acepta `.vdz` salvo que una comprobación de licencia (bit `0x1000`) habilite su importador XML, y la extensión legacy exige escribir en `HKLM` (§12.11). Sin COM desaparecen `-Embedding`, `ComServer`, el hilo STA e `IDispatch`.
- **Quitado**: `project_import_xml`, con `restore_from`, `differences`, el helper de 32 bits `xml_to_private.ps1` (`INDEXML.dll`/`TransformToPrivate`), `LoadXMLCommand`, `XmlImportResult` y el trait `XmlOps`. Son 26 herramientas (antes 27). Se mantienen `project_export_xml` y las lecturas del `.vxml` (`variable_list`, `panel_report`, `action_list`, `panel_list`, `variable_lookup` y `project_cross_references`). **El MCP ya no crea ni edita pantallas.**
- **`vijeo-com` → `vijeo-ui`** (`crates\vijeo-ui`): sólo `ui` (`UiInstance`/`UiDialog`), `dialog`, `feedback`, `tree` (Navigator), `process` (localizar, vigilar y matar procesos; se quitó `com_launched`) e `install` (`discover_installations`, sin buscar ProgIDs). Se borraron `server.rs`, `sta.rs`, `value.rs`, `variant.rs` y sus tests en vivo, y las features `Win32_System_Com/Ole/Variant` ya no se usan. Donde las tandas anteriores dicen `vijeo-com` o `vijeo_com`, hoy es `vijeo-ui`.
- **`vijeo-core`**: se quitaron `Capability::XmlImport` y `Capability::PanelObject`, los errores `NotAvailable`, `ComFailure` y `NullResult` (códigos `not_available`, `com_failure` y `null_result`), y el campo `Installation.progids`. `Vd63Factory::supports` ya no exige el ProgID `VijeoFrame.VijeoDesigner2`: basta con la versión 6.x y `Vijeo-Frame.exe`.
- **Backend**: `xml_import.rs` se reemplazó por `vdz.rs` (`TEMP_PREFIX`, `temp_project_name`, `package_vdz`, `copy_with_project_name` y `remove_temp_project`).
- **Pruebas en vivo**: `cargo test --workspace --features vijeo-backend-vd63/live-vijeo,vijeo-mcp/live-vijeo -- --test-threads=1`. `vijeo-ui` ya no tiene la feature `live-vijeo`.
- **Herramienta de análisis**: `tools\ghidra_scripts\ScalarRefs.java`, que lista las funciones que usan un valor inmediato (p. ej. ids de recursos).

Pendiente: `--probe-models` (D5), atar las instancias a un Job object para que mueran si el MCP muere de golpe, y `Download` (D3). *(Los dos primeros, hechos en la novena tanda.)*

Octava tanda (2026-11-02): `target_config_set`, que cambia la configuración de comunicación de un `.vdz` sin abrir Vijeo. Formato y comprobaciones en el inventario, §12.12.

- **Qué cambia**: la IP, la máscara y la puerta de enlace de descarga del HMI (`download`), y la IP, el puerto y el unit ID de los equipos (`equipment`; puerto y unit ID sólo en Modbus TCP). `driver` sólo hace falta si hay dos equipos con el mismo nombre. No cambia la segunda Ethernet de los GTU ni `DriverConfig`.
- **Red del target**: el `Document` tiene, sin etiquetar, otra terna IP / puerta / máscara (`swx::second_network_at`). Vijeo valida contra ella la puerta de enlace de descarga. Si repite la IP de descarga (STO, una sola Ethernet) se cambia junto con la descarga (`item: "ethernet"` en `changes`). Si no (segunda Ethernet de un GTU), se deja, y una puerta fuera de esa red se rechaza antes de escribir, porque Vijeo daría `1126` (inventario §12.12).
- **Cómo escribe**: valida todo antes de escribir. Escribe `<destino>.mcp-tmp` junto al destino con `vdz::replace_swx`, relee el fichero y compara la configuración de todos los targets con la esperada (en JSON). Sólo si coincide renombra el temporal sobre el destino; si no, lo borra y falla. Los valores que ya coinciden no cuentan, y sin cambios o con `dry_run` no escribe (`written: false`).
- **Rechazos**: un destino abierto con `project_open` (`refused`), las mismas reglas de ruta que `project_save` y un `save_as` que ya existe sin `overwrite`.
- **Gate**: `ConfigEdit` sólo queda `Supported` con una instalación 6.3 (`Support::Exact`). Con otra versión, aunque se fuerce con `--vijeo-version`, queda `Gated`. `ProjectConfig` (lectura) no tiene ese límite.
- **Pruebas**: unitarias en `swx.rs` (`mfc_encoding_round_trips`, `edits_equipment_addresses`, `rewrites_protocol_config_keeping_the_tail`, `ipv4_and_masks`, `edits_a_target_and_reads_back_what_was_expected` y `updates_the_network_copy_of_single_ethernet_targets`) y el test en vivo `live_config_set_build_and_reopen` (`crates\vijeo-backend-vd63\tests\live.rs`; con `VIJEO_MCP_LIVE_EXTRA=<.vdz>` repite la edición con una copia de otro proyecto).
- **Verificado en vivo** (2026-11-02): con el ejemplo Modbus (IP, máscara y puerta de descarga y el equipo cambiados) compila con los mismos 2/2 que el original. Vijeo lo abre, y al guardarlo él mismo las IP nuevas siguen ahí. Con una copia de SET03 (IP y máscara de descarga y un equipo) compila con 0/7, como el original, y la puerta `10.0.0.1` se rechaza con la explicación.

Novena tanda (2026-11-06): instancias atadas al MCP con un job object y `--probe-models` (D5).

- **Job object** (`vijeo_ui::process::Job`, `tie_to_this_process`): al lanzar cada `Vijeo-Frame.exe`, `UiInstance::launch` lo mete en un job con `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. El job vive en un `static` que no se suelta nunca, así que Windows cierra su handle (y termina las instancias) cuando termina el MCP, también si muere de golpe. Si no se puede crear o asignar, se avisa en el log y se sigue como antes. Necesita las features `Win32_System_JobObjects` y `Win32_Security`. Test `closing_the_job_kills_its_processes`. **Verificado en vivo**: al matar a la fuerza un `vijeo-mcp.exe` con su Vijeo abierto, Windows terminó también el Vijeo. La carpeta `MCP_TMP_*` sí puede quedar (Vijeo no llega a borrarla) y `vijeo_status` la sigue listando como huérfana.
- **`--probe-models`** (`main.rs`, `CatalogOps::probe_models`, `probe.rs`): no arranca el servidor. Crea un proyecto temporal con el primer modelo y, para cada uno de los demás, añade un target con el asistente, lo comprueba (`check_new_target`) y lo borra. Se prueban todos los modelos de familias activas, también los que la regla descarta por licencia. Los de familias desactivadas en `Targets.dat` se saltan (`skipped`). El resultado de cada modelo es `created`, `rejected` (el asistente no ofrece la familia o el modelo, Vijeo crea otro o sale un aviso) o `failed` (tiempo, caída de Vijeo; no dice nada del modelo y se repite una vez con un proyecto nuevo). Tras un `failed` o un aviso, el proyecto se descarta. `--probe-only <texto>` prueba sólo los modelos que contienen el texto. El resultado va en JSON a stdout y el progreso a stderr.
- **Caché**: `%LOCALAPPDATA%\vijeo-mcp\models-<versión>.json` (o `--cache-dir`). Lleva la versión, `frame_dir` y una huella de los `TargetInfo\*.dat` (nombre, tamaño y fecha); si algo cambia, no se usa. Se escribe tras cada modelo, así que una prueba cortada no pierde lo hecho. Las pruebas se suman: un modelo probado otra vez se reemplaza, salvo que ahora dé `failed` y antes no. Al crear el backend, `Catalog::apply_probes` aplica la caché: `created` → creable, `rejected` → no creable (`resolve` da el motivo), `failed` → la regla. `ModelInfo.probe` dice qué salió.
- **Arreglos que salieron de la prueba**:
  - `project_new` en frío: una vez el primer Save dejó el `.SwxCF` sin el target (HMIG3U, no se repitió). `launch_new` guarda otra vez si el proyecto nuevo sale sin targets.
  - Familia «Tablet» del catálogo = «Tablets and Smartphones» en el asistente: `find_family` acepta el único elemento que empieza por el nombre del catálogo.
  - Nombres del asistente que no están en el catálogo (`WIZARD_MODEL_NAMES`): los dos modelos de tablets se llaman «Tablet» en `Models_Tablets` y el asistente los muestra como «Android» (`Model_Tablets`) e «iOS» (`Model_iOS`); `TARGET_HMI_P6` («Harmony HMIP6» en el catálogo) es «Harmony P6 Box PN» (verificado: el `.SwxCF` guarda `TARGET_HMI_P6`).
  - iPC: su `Document` guarda la familia como `iPC Series` (no `Target_*`) y `is_labelled` lo descartaba, así que no se leía el modelo y `target_add` deshacía el target. Ahora se reconoce por el código del modelo (`Model_*` o `Target_*`, el del P6) en la tercera cadena (test `ipc_documents_are_labelled_by_model` y el test en vivo `live_ipc_target_is_kept`, con HMIRSO, HMIPUx6 y P6). Si aun así el modelo no se puede leer, `check_new_target` acepta el target sin comprobarlo y lo avisa en el log.
- **Resultado en 6.3.0.103** (2026-11-06, ~15 s por modelo con el proyecto ya abierto, ~35 s el primero): **39 creables y 19 no** de 58 probados; `GP2401-T` no se prueba (familia desactivada). La regla daba 56 creables. En modo normal el asistente no ofrece las familias HMISCU (`Model_HMISAC`, `Model_HMISBC`, que por COM sí se creaban), XBTGTC (4), XBT OT (5), Easy Harmony iPC (2, ya descartados por licencia), Generic Windows PC, Pro-face iPC (2) ni HMIBMW, y dentro de sus familias no ofrece `Model_OctolisHMISTU855` ni `Model_IPC_MPCNx50NNN20N`. Se crean todos los GTO, GTU, GTUX, GK, STU (HMIS5T), XBTGH, los 18 iPC y las dos tablets.
- Los tests en vivo usan una caché vacía propia: ven la regla, no la caché del usuario.

Décima tanda (2026-11-06): las cinco herramientas de bajo riesgo. Son 32 herramientas, con cinco capacidades nuevas (`XmlValidate`, `ScriptLint`, `VariableCsv`, `DataConvert` y `PanelOpen`) y el trait `FileOps` (`Backend::files`) para lo que se hace sin Vijeo.

- **`xml_validate`** (`xsd.rs` + `xsd_validate.ps1`, incrustado): en Rust no hay un validador XSD completo, así que se valida con `System.Xml` de .NET desde `powershell.exe -EncodedCommand`, con las rutas en variables de entorno. `public.xsd` 6.3 no es XSD 1.0 válido en dos sitios, y el script los corrige en memoria (lo dice en `notes`): un `xs:group` dentro de un `xs:all` (pasa a ser un `xs:choice` repetible) y `DefinitionObjectID` declarado dos veces en el mismo tipo. **Los exports de Vijeo tampoco validan del todo**: el `.vxml` de SET03 da 23 errores y 25 warnings (Download sin DHCP/UseNAT/DNS, PopPanel sin Condition, RTN/UNACK/ACK de las alarmas…). Por eso el resultado agrupa los errores por mensaje (`summary`) y aconseja comparar con el export del mismo proyecto. Tarda unos 4 s.
- **`variables_generate_csv`** (`varcsv.rs`): el formato sale de los `Tags-*.CSV` que exportó Vijeo 6.3 (22 ficheros, unas 12 000 filas). La línea 1 es `'5.1.0, Vijeo-Designer 6.3.0 CSV output`, la línea 2 trae 46 columnas (la 11 es `<idioma> ID`) y cada fila tiene 47 campos. Las carpetas van primero (`Folder,A.B,,,,"",`) y luego las variables con su ruta con puntos. Qué columnas lleva cada tipo (BOOL/enteros/REAL/STRING, interna/externa, con alarma) y cuáles van entre comillas se copió de esas filas; los tests comparan filas reales carácter a carácter. Por defecto, la codificación es la página **OEM** (el «ANSI CSV» de Vijeo se escribe en OEM: `ñ` = 0xA4 con OEMCP 437, `vijeo_ui::text::to_oem`, feature `Win32_Globalization`). `unicode` (UTF-16 LE con BOM) no se ha probado a importar. Sólo hay alarmas de BOOL (las de límite no están en los CSV de referencia). Con `project` toma el idioma del proyecto (`UserLanguage` L1) y avisa de nombres que ya existen y de grupos de escaneo que no son de ningún equipo. Rechaza las palabras reservadas de Vijeo (ayuda, apéndice 3 «Reserved Words»: `int`, `real`, `read`, `top`, `HMI`, `PI`, `E`…) en cualquier parte del nombre. **Verificado importándolo** (2026-11-25, `live_generated_csv_imports`, ejemplo Modbus): las 14 variables entran como se pidieron (los 8 tipos, internas y externas, bit `%MW212:X3`, valores iniciales, retentiva, Read Only, STRING con comillas, dos alarmas, descripción con `«año» ñ ó` en OEM) y el proyecto compila igual (2/2).
- **`data_convert`** (`dataconv.rs`): `DataConverter.exe -i … -o … [-csr|-crf|-ccr|-cfr] [-rp] [-s] [-b] [-cw] [-ch] [-ctN] [-dX] -v`, con las opciones de la ayuda §20.5.3. Es de consola: devuelve 0 si fue bien y escribe en stdout. Los ficheros escritos se sacan comparando la carpeta de salida antes y después. El proceso queda atado al job del MCP (`process::tie_child`). `csv_to_rcp` exige `recipe_dir`, que tiene que ser distinta de `output`. **Verificado**: con un `.dat` que no es válido termina con 0, escribe `<fichero>:  Not a valid DAT file` y deja un `.csv`. Por eso `ok` también mira su salida, y las líneas de fallo van en `errors`. **Sólo probado con entradas que no existen o rotas**: en el equipo no hay ningún `.rcp` ni `.dat` real.
- **`script_lint`** (`lint.rs`; scripts y contexto de `vxml::scripts` y `vxml::lint_context`): un lexer de Java (comentarios, cadenas, números) y comprobaciones de la ayuda de scripts (§18.3-18.4). Mira:
  - paréntesis, llaves y comillas sin cerrar;
  - llamadas `Var.metodo()` sobre variables que no existen (con la más parecida como sugerencia), con otras mayúsculas (los scripts las distinguen) o con métodos que no son de las variables (§18.9.1.6);
  - `X`/`XW` fuera de los enteros y métodos de `Sys` desconocidos;
  - una variable usada sin método en una expresión, o como nombre de una local;
  - miembros de estructura que no existen;
  - `float x = 0.5;` sin cast y decimales `.5`.

  Los objetos del target con nombre (equipos, ventanas, grupos de alarmas…) y las clases de Java no se revisan. Con `script` revisa un texto suelto contra las variables del target. **Verificado en vivo**: Hardwired y SET03 (2 scripts cada uno) sin avisos, y el texto suelto `NoExiste_Var.write(a)` da un error.
- **`project_goto`** (`TargetOps::open_panel`, `UiProject::open_panel`): busca el panel en el `.vxml` (`BP:\…`, la ruta o el nombre si es único) y lo abre en el Navigator. En 6.3 el árbol es `<target>` › `Graphical Panels` › `Base Panels` › `1: Start`: los paneles base llevan delante su número (`navigator_label_is`). Selecciona el nodo, le da el foco y pulsa Intro (`RemoteTree::press_enter`, la misma vía que Supr en `target_delete`). Termina cuando el título del marco muestra `[<target> - <panel> - <idioma>]`. **Verificado en vivo** con Hardwired: Start → Cabinet (por nombre) → `BP:\Start`.
- **Arreglos que salieron**:
  - Recién abierto un proyecto con `/Import`, el árbol 2001 del Navigator puede estar todavía vacío: `target_items` espera hasta 15 s a que tenga raíz (antes sólo se había usado con proyectos recién creados).
  - Export Project espera el diálogo hasta 180 s (`EXPORT_DIALOG_TIMEOUT`): con una copia de PROYC (8,8 MB) no apareció en los 30 s de antes. Con 180 s se exporta y `script_lint` revisa su único script, sin avisos (la prueba con PROYC tarda unos 9 min, casi todo en abrir y exportar).
- **Import Variables** (2026-11-25; `UiProject::import_variables`, desde la undécima tanda detrás de `variables_import`): comando `0xC419` (menú Variable de `MENU 1008`; en el contextual del nodo Variables, `MENU 3550`, con Ctrl+I), con el nodo `<target>` › `Variables` seleccionado y el foco en el Navigator (`RemoteTree::focus`). Abre un diálogo de fichero con controles propios: tipo (`1136`: CSV, TXT o XML), delimitador (`6092`: `, Comma` / `; Semicolon`), «Do not overwrite» / «Overwrite existing variables» (`6087`/`6088`), «Allow Unassigned Scan Groups» (`6075`) y «Do not import Alarm Messages» (`6076`). **No hay aviso final** (la ayuda dice lo contrario): Vijeo sólo escribe en la Feedback Zone `Importing variables from <csv>...`, `<n> variables were imported.` e `Import complete.` Un nombre no válido para la importación entera (`Import operation halted. Invalid Variable name was found in file … at line N`) y no importa nada. Con un solo grupo de escaneo en el proyecto, Vijeo asigna a él el `ScanGroup1` del CSV (el `.vxml` lo muestra como `M221_Controller`).
- **Tras un Build All, Vijeo deja de atender el Navigator** en esa instancia (verificado con el ejemplo Modbus, que compila con errores). Import Variables ya no abre su diálogo, ni mandado al marco ni a la barra del Navigator, y el Intro del Navigator ya no abre paneles (`project_goto`). Tampoco se arregla esperando 30 s, guardando o activando la vista del panel con `WM_MOUSEACTIVATE`, y no hay ningún modal oculto: el marco sigue habilitado. Export Project y Save siguen funcionando. La causa no se conoce. `project_goto` e `import_variables` lo explican en el error si la instancia ya compiló (`UiProject::built`), y la salida es guardar, cerrar y volver a abrir. `live_probe_import_variables_dialog` lo deja registrado.
- **Diagnóstico**: `RemoteTree::describe_all` (todos los `SysTreeView32` de la instancia) y `Vd63Backend::navigator_outline`, que usa el test en vivo `live_goto_and_lint` (con `VIJEO_MCP_LIVE_LINT=<.vdz>` revisa también los scripts de otro proyecto). El smoke `smoke_file_tools` prueba las tres herramientas sin Vijeo a través del MCP.

Undécima tanda (2026-11-26): **`variables_import`** (33 herramientas; capacidad `VariableImport`, `VariableOps::import_csv`). Usa `UiProject::import_variables` (arriba). Acepta `csv` (ruta absoluta de un `.csv`, sólo se lee: puede estar en cualquier carpeta) o `variables` (los mismos `VariableSpec` de `variables_generate_csv`, con sus comprobaciones contra el target y el idioma del proyecto; el CSV se escribe en OEM en `<trabajo>\import\`). `replace_existing` elige «Overwrite existing variables»; si no, las que ya existen no se tocan y salen en `warnings`. `imported` se lee de `N variables were imported.`; si no coincide con las pedidas, lo avisa. Un `Import operation halted` es error `invalid_argument` (no se importó nada). Tras importar, Vijeo guarda la copia de trabajo y el proyecto queda con cambios sin guardar en el `.vdz` (`project_save`). `live_generated_csv_imports` importa las 14 variables por `csv` y luego otra nueva y una repetida (con otro tipo) por `variables`: la nueva entra y la repetida conserva su tipo.

## 13. Decisiones

- **D1. Ventana visible** — se mantiene visible en la primera versión. Desde la sexta tanda es imprescindible: el MCP maneja la ventana de cada proyecto (§6.1). Queda como idea a estudiar: arrancar sin ventana y, a petición del usuario, abrir una copia guardada del proyecto en una instancia aparte para consultarla. Vijeo no tiene modo de sólo lectura real (los cambios en esa copia no llegarían al MCP), y el descompilado muestra que Vijeo trae su ventana al frente en varias operaciones, así que habría que probarlo antes de comprometerse.
- **D2. Concurrencia** — **verificado** (`docs/reference/vd-6.3/com_probe_logs/probe_multi*.log`):
  - Misma instancia: un 2º `OpenProject` devuelve el proyecto ya abierto; `NewProject` con un proyecto abierto se bloquea en un diálogo modal y deja inválidos los proyectos abiertos.
  - Dos `CoCreateInstance`: **dos procesos independientes**, cada uno con su proyecto, cambios aislados, guardado y cierre correctos, sin carpetas huérfanas.
  - Decisión: **una instancia por proyecto abierto** (pool, §6). Desde la sexta tanda, esa instancia es normal, sin COM (§6.1).
- **D3. Descarga a HMI** — **fuera de la primera versión**.
- **D4. Parser XML** — `roxmltree` para catálogos y para el `.vxml` (`vxml.rs`). Desde la sexta tanda ya no se usa el HTML de `GenerateAllReports`.
- **D5. Qué modelos se pueden crear** — un modelo es creable si tiene `<SoMachine>` en su catálogo **y** su `RequiredKey` está cubierta por la licencia instalada; el MCP puede leer la clave que pide cada modelo pero no la licencia del equipo (la calcula Vijeo internamente). Por defecto el catálogo marca como creables los modelos con `<SoMachine>` y clave 0 (instantáneo, pero puede fallar en los pocos que piden licencia: aquí los SCU piden 512 y sí funcionan). Opcionalmente, `--probe-models` valida en vivo (crear y borrar un target por modelo en un proyecto temporal, ~1 min) y guarda el resultado en caché por instalación para no repetirlo. **Actualizado en la sexta tanda**: ya no se exige `<SoMachine>` (era de la API COM). Es creable si la familia está activa y la `RequiredKey` cabe en la licencia estándar `0x0FFFFFFF` (`Catalog::creatable_reason`): 56 modelos en 6.3. **Novena tanda**: `--probe-models` implementado. Tarda unos 15 minutos para los 58 modelos (no ~1 min), guarda la caché en `%LOCALAPPDATA%\vijeo-mcp` y su resultado manda sobre la regla: 39 creables en 6.3 (§12bis).
