//! Servidor MCP: herramientas sobre `dyn Backend` (sólo conoce `vijeo-core`).

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{tool, tool_handler, tool_router, Json, ServerHandler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vijeo_core::*;

type ToolResult<T> = Result<Json<T>, CallToolResult>;

/// Qué capacidad necesita cada herramienta; las no soportadas no aparecen en `tools/list`.
const TOOL_CAPS: &[(&str, Capability)] = &[
    ("catalog_list_models", Capability::ModelCatalog),
    ("catalog_list_families", Capability::ModelCatalog),
    ("catalog_list_drivers", Capability::DriverCatalog),
    ("project_new", Capability::ProjectLifecycle),
    ("project_open", Capability::ProjectLifecycle),
    ("project_info", Capability::ProjectLifecycle),
    ("project_save", Capability::ProjectLifecycle),
    ("project_close", Capability::ProjectLifecycle),
    ("project_rename", Capability::ProjectLifecycle),
    ("target_list", Capability::TargetManage),
    ("target_add", Capability::TargetManage),
    ("target_delete", Capability::TargetManage),
    ("target_rename", Capability::TargetManage),
    ("panel_list", Capability::PanelList),
    ("project_build", Capability::Build),
    ("target_build", Capability::Build),
    ("project_messages", Capability::Build),
    ("target_config", Capability::ProjectConfig),
    ("target_config_set", Capability::ConfigEdit),
    ("project_export_xml", Capability::Reports),
    ("variable_list", Capability::Reports),
    ("panel_report", Capability::Reports),
    ("action_list", Capability::Reports),
    ("variable_lookup", Capability::VariableLookup),
    ("project_cross_references", Capability::CrossReferences),
    ("script_lint", Capability::ScriptLint),
    ("project_goto", Capability::PanelOpen),
    ("xml_validate", Capability::XmlValidate),
    ("variables_generate_csv", Capability::VariableCsv),
    ("variables_import", Capability::VariableImport),
    ("data_convert", Capability::DataConvert),
    ("panel_objects", Capability::PanelObjects),
    ("panel_edit", Capability::PanelObjects),
    ("panel_insert", Capability::PanelObjects),
    ("toolchest_list", Capability::Toolchest),
];

const INSTRUCTIONS: &str = "\
Controla Vijeo-Designer (editor de pantallas HMI de Schneider Electric) a través de su interfaz, como lo haría \
un usuario (menús, asistentes y Navigator), sin la API COM.
Flujo: project_open (un .vdz existente) o project_new → devuelven `project` (id) → target_*/panel_list/\
project_build/variable_* → project_save → project_close.
- Cada proyecto abierto es un proceso Vijeo-Frame.exe con ventana visible que trabaja sobre una copia \
(Vijeo-Manager\\MCP_TMP_*, se borra al cerrar); abrir tarda unos 10-30 s. El .vdz sólo cambia con project_save \
(y con target_config_set, que no abre Vijeo).
- Sólo se trabaja con .vdz y nunca dentro del workspace Vijeo-Manager ni de otras carpetas protegidas.
- Los modelos HMI se eligen con catalog_list_models (sólo los `creatable` se pueden crear; `probe` indica que \
se comprobó creando el target en esta instalación); project_new exige el primer target.
- project_build/target_build devuelven el texto de cada error y warning (con `path` compilan una copia de un \
.vdz sin abrirlo); target_config lee drivers, equipos e IP del target de un proyecto abierto o de cualquier .vdz.
- target_config_set cambia en un .vdz, sin abrir Vijeo, la IP/máscara/puerta de descarga del HMI y la IP, \
puerto y unit ID (Modbus TCP) de sus equipos. No escribe sobre proyectos abiertos: project_save + project_close \
antes, y project_build con `path` después para comprobarlo. La puerta de enlace tiene que ser de la red que Vijeo \
valida (en los modelos con `dual_ethernet`, la de la ETH2, que no se cambia).
- variable_list, panel_report, action_list, panel_list, variable_lookup y project_cross_references leen el XML \
del proyecto (Export Project, unos segundos la primera vez; se reutiliza mientras el proyecto no cambie). \
Necesitan SupportsXMLImportExport=1 en el registro de Vijeo.
- project_export_xml escribe el XML completo del proyecto (.vxml, public.xsd: pantallas, objetos, animaciones, \
acciones, variables, alarmas…) con sus imágenes. No hay herramienta para importarlo.
- Pantallas (paneles base de un proyecto abierto), con el portapapeles de Vijeo como Copiar/Pegar: panel_objects \
lista los objetos (ruta Grupo/Objeto, tipo, posición absoluta, textos y variables); panel_edit renombra, mueve, \
redimensiona, cambia textos, sustituye variables (Vijeo las vuelve a enlazar por nombre), borra objetos y cambia \
animaciones, acciones de botones y colores (dry_run para probar); panel_insert crea objetos desde cero (`create`: \
botones con acciones, lámparas, displays numéricos y de texto, textos, rectángulos, elipses y líneas, con animaciones \
de visibilidad, parpadeo, color, relleno y posición) o añade piezas de la Toolchest (toolchest_list, sin Vijeo) u \
objetos copiados de otro panel. Las variables tienen que existir en el target (se comprueban en el XML). \
Usan el portapapeles de Windows: el texto que tuviera se devuelve al terminar, el resto se pierde. Después: \
project_build para comprobar (las piezas de la Toolchest llegan sin variables) y project_save. Como project_goto, \
no funcionan tras un project_build en la misma instancia.
- script_lint revisa los scripts del proyecto (o un texto con `script`, antes de pegarlo en Vijeo) contra sus \
variables y la API de scripts: variables inexistentes o con otras mayúsculas, métodos, sintaxis. No compila: \
project_build sigue siendo la prueba definitiva. project_goto abre un panel base en la ventana de Vijeo; \
tras un project_build en la misma instancia Vijeo deja de atender el Navigator: úsalo antes de compilar o \
guarda, cierra y vuelve a abrir. Lo mismo vale para variables_import, que importa en un target abierto un CSV \
de variables (de variables_generate_csv o de Export Variables) o directamente una lista de ariables (como en \
variables_generate_csv), sin reemplazar las que ya existen salvo con replace_existing; luego project_save.
- Sin abrir Vijeo: xml_validate valida un .vxml contra el public.xsd de la instalación (los exports de Vijeo \
tampoco validan del todo: compara con uno); variables_generate_csv escribe un CSV para Variables › Import \
Variables (verificado importándolo en Vijeo; rechaza las palabras reservadas, que harían fallar la importación \
entera; con `project` comprueba nombres repetidos, grupos de escaneo e idioma; no importa: eso es variables_import); data_convert \
convierte recetas .rcp y registros .dat a CSV y CSV a .rcp con DataConverter.exe (ficheros ya descargados del HMI).
- Si Vijeo abre un diálogo desconocido o se cuelga, la operación falla (error modal_dialog/timeout/\
server_crashed) y el proyecto puede quedar inservible: guarda a menudo.
- No hay ninguna herramienta que actúe sobre HMIs reales (descarga).";

pub struct State {
    pub registry: BackendRegistry,
    pub installations: Vec<Installation>,
    pub backend: Option<Arc<dyn Backend>>,
    /// Por qué no hay backend (sin instalación, versión no soportada…).
    pub backend_error: Option<String>,
}

#[derive(Clone)]
pub struct VijeoServer {
    state: Arc<State>,
    tool_router: ToolRouter<Self>,
}

fn error_result(e: &VijeoError) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!("[{}] {e}", e.code()))])
}

// ---------- parámetros ----------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectParam {
    /// Id devuelto por project_open/project_new.
    pub project: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TargetParam {
    pub project: u32,
    /// Nombre del target (sin distinguir mayúsculas).
    pub target: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NewTargetSpec {
    /// Nombre del target: ASCII, sin \ / : * ? " < > | # , ; .
    pub name: String,
    /// Código interno (Model_HMIS5T), nombre comercial (HMIGTO5310) o hmi_type completo.
    pub model: String,
    /// Pantalla en modelos modulares (HMIGTU): nombre o prefijo, p.ej. HMIDT351.
    #[serde(default)]
    pub display: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectNewParams {
    /// Nombre del proyecto: ASCII, sin \ / : * ? " < > | # , ; .
    pub name: String,
    /// Ruta absoluta del .vdz que se crea.
    pub save_as: PathBuf,
    /// Reemplazar el fichero si ya existe.
    #[serde(default)]
    pub overwrite: bool,
    /// Primer target (Vijeo no crea proyectos sin target).
    pub target: NewTargetSpec,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectOpenParams {
    /// Ruta absoluta de un .vdz.
    pub path: PathBuf,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectSaveParams {
    pub project: u32,
    /// Guardar como (ruta absoluta .vdz). Sin él, guarda en la ruta actual.
    #[serde(default)]
    pub save_as: Option<PathBuf>,
    /// Reemplazar save_as si ya existe.
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectCloseParams {
    pub project: u32,
    /// Cerrar aunque haya cambios sin guardar (se pierden).
    #[serde(default)]
    pub discard: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TargetAddParams {
    pub project: u32,
    #[serde(flatten)]
    pub target: NewTargetSpec,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportXmlParams {
    pub project: u32,
    /// Carpeta absoluta donde se escriben <proyecto>.vxml y la carpeta <proyecto> con las imágenes (se crea si no existe).
    pub dir: PathBuf,
    /// Reemplazar el .vxml y las imágenes si ya existen.
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VariableListParams {
    pub project: u32,
    /// Sólo este target (por defecto, todos).
    #[serde(default)]
    pub target: Option<String>,
    /// Texto a buscar en nombre o descripción (sin distinguir mayúsculas).
    #[serde(default)]
    pub text: Option<String>,
    /// Internal / External.
    #[serde(default)]
    pub source: Option<String>,
    /// Incluir las variables de sistema (las que empiezan por «_»).
    #[serde(default)]
    pub include_system: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectRenameParams {
    pub project: u32,
    /// Nombre nuevo: ASCII, sin \ / : * ? " < > | # , ; . (no cambia el nombre del .vdz).
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TargetRenameParams {
    pub project: u32,
    /// Nombre actual (sin distinguir mayúsculas).
    pub target: String,
    /// Nombre nuevo: ASCII, sin \ / : * ? " < > | # , ; .
    pub new_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReportTargetParams {
    pub project: u32,
    /// Sólo este target (por defecto, todos).
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BuildParams {
    /// Proyecto abierto: compila su estado actual, aunque no esté guardado en el .vdz. Indica `project` o `path`.
    #[serde(default)]
    pub project: Option<u32>,
    /// Ruta absoluta de un .vdz: compila una copia en una instancia de Vijeo aparte y la descarta; el fichero no
    /// se modifica. Tarda más (abrir, compilar y cerrar).
    #[serde(default)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TargetBuildParams {
    /// Proyecto abierto. Indica `project` o `path`. Compila todo el proyecto y devuelve sólo los mensajes del target.
    #[serde(default)]
    pub project: Option<u32>,
    /// Ruta absoluta de un .vdz: compila una copia de todo el proyecto y devuelve sólo los mensajes de este target.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Nombre del target (sin distinguir mayúsculas).
    pub target: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TargetConfigParams {
    /// Proyecto abierto (se lee su copia de trabajo, con los cambios aún no guardados en el .vdz). Indica `project` o `path`.
    #[serde(default)]
    pub project: Option<u32>,
    /// Ruta absoluta de un .vdz cualquiera, que se lee sin abrir Vijeo. Indica `project` o `path`.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Sólo este target (por defecto, todos).
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TargetConfigSetParams {
    /// Ruta absoluta del .vdz que se cambia (se lee sin abrir Vijeo). El fichero que se escribe no puede estar
    /// abierto con project_open.
    pub path: PathBuf,
    /// Guardar el resultado en otro .vdz (ruta absoluta). Sin él, se reescribe `path`.
    #[serde(default)]
    pub save_as: Option<PathBuf>,
    /// Reemplazar save_as si ya existe.
    #[serde(default)]
    pub overwrite: bool,
    /// Sólo calcular los cambios, sin escribir nada.
    #[serde(default)]
    pub dry_run: bool,
    /// Target a cambiar (sin distinguir mayúsculas); puede faltar si el proyecto sólo tiene uno.
    #[serde(default)]
    pub target: Option<String>,
    #[serde(flatten)]
    pub changes: TargetConfigChanges,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScriptLintParams {
    pub project: u32,
    /// Sólo este target (por defecto, todos). Obligatorio con `script` si hay varios.
    #[serde(default)]
    pub target: Option<String>,
    /// Revisar este código (un script aún no puesto en el proyecto) en vez de los scripts del proyecto.
    #[serde(default)]
    pub script: Option<String>,
    /// Incluir también los scripts sin problemas en la lista.
    #[serde(default)]
    pub include_clean: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GotoParams {
    pub project: u32,
    pub target: String,
    /// Panel base: ruta de panel_list (`BP:\Carpeta\Panel`), ruta sin `BP:\` o sólo el nombre si no se repite.
    pub panel: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct XmlValidateParams {
    /// Ruta absoluta del XML (.vxml de project_export_xml u otro con formato public.xsd). Sólo se lee.
    pub path: PathBuf,
    /// Máximo de errores en `issues` (por defecto 50; `summary` los agrupa todos).
    #[serde(default)]
    pub max_issues: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VariableLookupParams {
    pub project: u32,
    pub target: String,
    /// Nombre completo de la variable (p.ej. MyController.Alarms.xAlarmEStop_1).
    pub name: String,
}

// ---------- resultados ----------

#[derive(Debug, Serialize, JsonSchema)]
pub struct InstallationOut {
    #[serde(flatten)]
    pub installation: Installation,
    /// Backend que la atendería.
    pub backend: Option<String>,
    pub support: Support,
    /// Es la instalación que usa este servidor.
    pub selected: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct InstallationsOut {
    pub installations: Vec<InstallationOut>,
    pub backends: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusOut {
    pub backend: BackendInfo,
    pub capabilities: CapabilitySet,
    pub status: BackendStatus,
    pub projects: Vec<ProjectSummary>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelsOut {
    pub count: usize,
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FamiliesOut {
    pub families: Vec<FamilyInfo>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ClosedOut {
    pub closed: u32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TargetsOut {
    pub targets: Vec<TargetInfo>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DeletedOut {
    pub deleted: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PanelsOut {
    pub panels: Vec<PanelRef>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct VariablesOut {
    pub count: usize,
    pub variables: Vec<VariableInfo>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DriversOut {
    pub count: usize,
    pub drivers: Vec<DriverInfo>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PanelReportsOut {
    pub count: usize,
    pub panels: Vec<PanelReport>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ActionsOut {
    pub count: usize,
    pub actions: Vec<ActionInfo>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct XrefOut {
    /// Variables que se usan en algún sitio.
    pub count: usize,
    pub references: Vec<CrossReference>,
    /// Variables que no aparecen en ningún panel, acción, alarma… (`Target/Variable`).
    pub unused: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ScriptLintOut {
    pub scripts_checked: usize,
    pub errors: usize,
    pub warnings: usize,
    /// Scripts con algún problema (o todos con `include_clean`).
    pub scripts: Vec<ScriptLint>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GotoOut {
    /// Panel abierto en la ventana de Vijeo.
    pub opened: PanelRef,
}

// ---------- servidor ----------

impl VijeoServer {
    pub fn new(state: State) -> Self {
        let mut router = Self::tool_router();
        let caps = state.backend.as_ref().map(|b| b.capabilities().clone()).unwrap_or_default();
        for (tool, cap) in TOOL_CAPS {
            if !caps.is_supported(*cap) {
                router = router.with_disabled(*tool);
            }
        }
        Self { state: Arc::new(state), tool_router: router }
    }

    pub fn backend(&self) -> Option<Arc<dyn Backend>> {
        self.state.backend.clone()
    }

    /// Ejecuta una operación síncrona del backend fuera del runtime de tokio.
    async fn blocking<T, F>(&self, f: F) -> ToolResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&dyn Backend) -> VijeoResult<T> + Send + 'static,
    {
        let Some(b) = self.state.backend.clone() else {
            let msg = self.state.backend_error.clone().unwrap_or_else(|| "no hay backend".into());
            return Err(error_result(&VijeoError::NotInstalled(msg)));
        };
        match tokio::task::spawn_blocking(move || f(b.as_ref())).await {
            Ok(Ok(v)) => Ok(Json(v)),
            Ok(Err(e)) => {
                tracing::warn!("{e}");
                Err(error_result(&e))
            }
            Err(e) => Err(error_result(&VijeoError::Internal(format!("tarea abortada: {e}")))),
        }
    }
}

fn ops<'a, T: ?Sized>(o: Option<&'a T>, cap: Capability, b: &dyn Backend) -> VijeoResult<&'a T> {
    o.ok_or_else(|| VijeoError::Unsupported { capability: cap, version: b.info().product_version.to_string(), reason: "no implementada".into() })
}

#[tool_router]
impl VijeoServer {
    /// Instalaciones de Vijeo-Designer encontradas y backend elegido. No lanza Vijeo.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn vijeo_installations(&self) -> Json<InstallationsOut> {
        let selected = self.state.backend.as_ref().map(|b| b.info().frame_dir);
        let installations = self
            .state
            .installations
            .iter()
            .map(|i| {
                let d = self.state.registry.describe(i);
                InstallationOut {
                    backend: d.map(|(id, _)| id.to_owned()),
                    support: d.map(|(_, s)| s).unwrap_or(Support::No),
                    selected: selected.as_deref() == Some(i.frame_dir.as_path()),
                    installation: i.clone(),
                }
            })
            .collect();
        Json(InstallationsOut {
            installations,
            backends: self.state.registry.ids().into_iter().map(String::from).collect(),
            error: self.state.backend_error.clone(),
        })
    }

    /// Estado del backend: capacidades, instancias de Vijeo (lista/ocupada/caída) y proyectos abiertos.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn vijeo_status(&self) -> ToolResult<StatusOut> {
        self.blocking(|b| {
            Ok(StatusOut {
                backend: b.info(),
                capabilities: b.capabilities().clone(),
                status: b.status(),
                projects: b.projects().map(|p| p.list()).unwrap_or_default(),
            })
        })
        .await
    }

    /// Modelos HMI del catálogo de la instalación. Filtra por familia, `creatable` o texto.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn catalog_list_models(&self, Parameters(filter): Parameters<ModelFilter>) -> ToolResult<ModelsOut> {
        self.blocking(move |b| {
            let models = ops(b.catalog(), Capability::ModelCatalog, b)?.models(&filter);
            Ok(ModelsOut { count: models.len(), models })
        })
        .await
    }

    /// Familias de HMI del catálogo.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn catalog_list_families(&self) -> ToolResult<FamiliesOut> {
        self.blocking(|b| Ok(FamiliesOut { families: ops(b.catalog(), Capability::ModelCatalog, b)?.families() })).await
    }

    /// Drivers de comunicación instalados (HMI IO). Fabricante, protocolo y medio se deducen del nombre del fichero.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn catalog_list_drivers(&self, Parameters(filter): Parameters<DriverFilter>) -> ToolResult<DriversOut> {
        self.blocking(move |b| {
            let drivers = ops(b.catalog(), Capability::DriverCatalog, b)?.drivers(&filter);
            Ok(DriversOut { count: drivers.len(), drivers })
        })
        .await
    }

    /// Crea un proyecto nuevo con su primer target (asistente New Project), lo guarda en `save_as` (.vdz) y lo deja abierto.
    #[tool(annotations(destructive_hint = false, open_world_hint = false))]
    async fn project_new(&self, Parameters(p): Parameters<ProjectNewParams>) -> ToolResult<ProjectInfo> {
        self.blocking(move |b| {
            let sel = ModelSelector { model: p.target.model, display: p.target.display };
            ops(b.projects(), Capability::ProjectLifecycle, b)?.new_project(&p.name, &p.save_as, p.overwrite, &p.target.name, &sel)
        })
        .await
    }

    /// Abre un proyecto .vdz en una instancia nueva de Vijeo y devuelve su id.
    /// No modifica el .vdz, pero lanza Vijeo y crea una copia de trabajo en Vijeo-Manager.
    #[tool(annotations(destructive_hint = false, open_world_hint = false))]
    async fn project_open(&self, Parameters(p): Parameters<ProjectOpenParams>) -> ToolResult<ProjectInfo> {
        self.blocking(move |b| ops(b.projects(), Capability::ProjectLifecycle, b)?.open(&p.path)).await
    }

    /// Nombre, ruta, cambios sin guardar y targets de un proyecto abierto.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn project_info(&self, Parameters(p): Parameters<ProjectParam>) -> ToolResult<ProjectInfo> {
        self.blocking(move |b| ops(b.projects(), Capability::ProjectLifecycle, b)?.info(ProjectId(p.project))).await
    }

    /// Guarda el proyecto en su ruta o en `save_as`. Sobrescribe el .vdz de destino.
    #[tool(annotations(destructive_hint = true, open_world_hint = false))]
    async fn project_save(&self, Parameters(p): Parameters<ProjectSaveParams>) -> ToolResult<ProjectInfo> {
        self.blocking(move |b| ops(b.projects(), Capability::ProjectLifecycle, b)?.save(ProjectId(p.project), p.save_as.as_deref(), p.overwrite)).await
    }

    /// Cierra el proyecto y su instancia de Vijeo. Falla si hay cambios sin guardar salvo con `discard=true`.
    #[tool(annotations(destructive_hint = true, open_world_hint = false))]
    async fn project_close(&self, Parameters(p): Parameters<ProjectCloseParams>) -> ToolResult<ClosedOut> {
        self.blocking(move |b| {
            ops(b.projects(), Capability::ProjectLifecycle, b)?.close(ProjectId(p.project), p.discard)?;
            Ok(ClosedOut { closed: p.project })
        })
        .await
    }

    /// Cambia el nombre del proyecto abierto (queda pendiente de project_save; el .vdz no se renombra).
    #[tool(annotations(destructive_hint = false, idempotent_hint = true, open_world_hint = false))]
    async fn project_rename(&self, Parameters(p): Parameters<ProjectRenameParams>) -> ToolResult<ProjectInfo> {
        self.blocking(move |b| ops(b.projects(), Capability::ProjectLifecycle, b)?.rename(ProjectId(p.project), &p.name)).await
    }

    /// Targets del proyecto con su modelo (hmi_type canónico).
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn target_list(&self, Parameters(p): Parameters<ProjectParam>) -> ToolResult<TargetsOut> {
        self.blocking(move |b| Ok(TargetsOut { targets: ops(b.targets(), Capability::TargetManage, b)?.list(ProjectId(p.project))? })).await
    }

    /// Añade un target (HMI) al proyecto. El modelo se valida contra el catálogo antes de llamar a Vijeo.
    #[tool(annotations(destructive_hint = false, open_world_hint = false))]
    async fn target_add(&self, Parameters(p): Parameters<TargetAddParams>) -> ToolResult<TargetInfo> {
        self.blocking(move |b| {
            let sel = ModelSelector { model: p.target.model, display: p.target.display };
            ops(b.targets(), Capability::TargetManage, b)?.add(ProjectId(p.project), &p.target.name, &sel)
        })
        .await
    }

    /// Borra un target y todo su contenido (paneles, variables) del proyecto abierto.
    #[tool(annotations(destructive_hint = true, open_world_hint = false))]
    async fn target_delete(&self, Parameters(p): Parameters<TargetParam>) -> ToolResult<DeletedOut> {
        self.blocking(move |b| {
            ops(b.targets(), Capability::TargetManage, b)?.delete(ProjectId(p.project), &p.target)?;
            Ok(DeletedOut { deleted: p.target })
        })
        .await
    }

    /// Cambia el nombre de un target (queda pendiente de project_save).
    #[tool(annotations(destructive_hint = false, idempotent_hint = true, open_world_hint = false))]
    async fn target_rename(&self, Parameters(p): Parameters<TargetRenameParams>) -> ToolResult<TargetInfo> {
        self.blocking(move |b| ops(b.targets(), Capability::TargetManage, b)?.rename(ProjectId(p.project), &p.target, &p.new_name)).await
    }

    /// Paneles de un target (rutas BP:\Panel o BP:\Carpeta\Panel).
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn panel_list(&self, Parameters(p): Parameters<TargetParam>) -> ToolResult<PanelsOut> {
        self.blocking(move |b| Ok(PanelsOut { panels: ops(b.targets(), Capability::PanelList, b)?.panels(ProjectId(p.project), &p.target)? })).await
    }

    /// Compila todo el proyecto (abierto o un .vdz por ruta). Devuelve nº de errores y warnings y el texto de cada uno (ventana de salida de Vijeo).
    #[tool(annotations(destructive_hint = false, open_world_hint = false))]
    async fn project_build(&self, Parameters(p): Parameters<BuildParams>) -> ToolResult<BuildResult> {
        self.blocking(move |b| {
            let ops = ops(b.build(), Capability::Build, b)?;
            match (p.project, &p.path) {
                (Some(id), None) => ops.build_project(ProjectId(id)),
                (None, Some(path)) => ops.build_file(path, None),
                _ => Err(VijeoError::invalid("project/path", "indica exactamente uno de los dos")),
            }
        })
        .await
    }

    /// Compila un target (de un proyecto abierto, o de un .vdz por ruta). Devuelve nº de errores y warnings y el texto de cada uno (ventana de salida de Vijeo).
    #[tool(annotations(destructive_hint = false, open_world_hint = false))]
    async fn target_build(&self, Parameters(p): Parameters<TargetBuildParams>) -> ToolResult<BuildResult> {
        self.blocking(move |b| {
            let ops = ops(b.build(), Capability::Build, b)?;
            match (p.project, &p.path) {
                (Some(id), None) => ops.build_target(ProjectId(id), &p.target),
                (None, Some(path)) => ops.build_file(path, Some(&p.target)),
                _ => Err(VijeoError::invalid("project/path", "indica exactamente uno de los dos")),
            }
        })
        .await
    }

    /// Contenido actual de la ventana de salida (Feedback Zone) de la instancia del proyecto: errores y warnings de la última compilación.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn project_messages(&self, Parameters(p): Parameters<ProjectParam>) -> ToolResult<FeedbackLog> {
        self.blocking(move |b| ops(b.build(), Capability::Build, b)?.feedback(ProjectId(p.project))).await
    }

    /// Drivers de comunicación, equipos (IP, puerto, unit ID, parámetros) y propiedades de descarga de cada target, leídos del .vdz.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn target_config(&self, Parameters(p): Parameters<TargetConfigParams>) -> ToolResult<ProjectConfig> {
        self.blocking(move |b| {
            let c = ops(b.config(), Capability::ProjectConfig, b)?;
            match (p.project, &p.path) {
                (Some(id), None) => c.project_config(ProjectId(id), p.target.as_deref()),
                (None, Some(path)) => c.file_config(path, p.target.as_deref()),
                _ => Err(VijeoError::invalid("project/path", "indica exactamente uno de los dos")),
            }
        })
        .await
    }

    /// Cambia en un .vdz, sin abrir Vijeo, la IP, máscara y puerta de enlace de descarga del HMI y la IP, puerto y
    /// unit ID (sólo Modbus TCP) de sus equipos. Devuelve cada cambio (antes/después) y el target tal como queda;
    /// el fichero se relee y sólo se reemplaza si quedó como se esperaba.
    #[tool(annotations(destructive_hint = true, idempotent_hint = true, open_world_hint = false))]
    async fn target_config_set(&self, Parameters(p): Parameters<TargetConfigSetParams>) -> ToolResult<ConfigEditResult> {
        self.blocking(move |b| {
            ops(b.config(), Capability::ConfigEdit, b)?.edit_file_config(&p.path, p.target.as_deref(), &p.changes, p.save_as.as_deref(), p.overwrite, p.dry_run)
        })
        .await
    }

    /// Exporta el proyecto abierto como XML (Export Project): <dir>\<proyecto>.vxml (public.xsd: targets, paneles con sus
    /// objetos, animaciones y acciones, variables, alarmas…) y <dir>\<proyecto>\ con las imágenes.
    #[tool(annotations(destructive_hint = false, open_world_hint = false))]
    async fn project_export_xml(&self, Parameters(p): Parameters<ExportXmlParams>) -> ToolResult<XmlExportResult> {
        self.blocking(move |b| ops(b.reports(), Capability::Reports, b)?.export_xml(ProjectId(p.project), &p.dir, p.overwrite)).await
    }

    /// Variables del proyecto (nombre completo con carpetas, tipo, origen, dirección, descripción), leídas del XML del proyecto.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn variable_list(&self, Parameters(p): Parameters<VariableListParams>) -> ToolResult<VariablesOut> {
        self.blocking(move |b| {
            let text = p.text.as_deref().map(str::to_lowercase);
            let variables: Vec<VariableInfo> = ops(b.reports(), Capability::Reports, b)?
                .variables(ProjectId(p.project), p.target.as_deref())?
                .into_iter()
                .filter(|v| p.include_system || !v.name.starts_with('_'))
                .filter(|v| p.source.as_deref().is_none_or(|s| v.source.eq_ignore_ascii_case(s)))
                .filter(|v| {
                    text.as_deref().is_none_or(|t| {
                        v.name.to_lowercase().contains(t) || v.description.as_deref().is_some_and(|d| d.to_lowercase().contains(t))
                    })
                })
                .collect();
            Ok(VariablesOut { count: variables.len(), variables })
        })
        .await
    }

    /// Paneles base, maestros y ventanas emergentes de cada target con sus acciones de panel, leídos del XML del proyecto.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn panel_report(&self, Parameters(p): Parameters<ReportTargetParams>) -> ToolResult<PanelReportsOut> {
        self.blocking(move |b| {
            let panels = ops(b.reports(), Capability::Reports, b)?.panels(ProjectId(p.project), p.target.as_deref())?;
            Ok(PanelReportsOut { count: panels.len(), panels })
        })
        .await
    }

    /// Acciones globales (disparador, descripción, enclavamiento, script u operaciones), leídas del XML del proyecto.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn action_list(&self, Parameters(p): Parameters<ReportTargetParams>) -> ToolResult<ActionsOut> {
        self.blocking(move |b| {
            let actions = ops(b.reports(), Capability::Reports, b)?.actions(ProjectId(p.project), p.target.as_deref())?;
            Ok(ActionsOut { count: actions.len(), actions })
        })
        .await
    }

    /// Comprueba si existe una variable en un target (nombre completo, sin distinguir mayúsculas) y su tipo.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn variable_lookup(&self, Parameters(p): Parameters<VariableLookupParams>) -> ToolResult<VariableLookup> {
        self.blocking(move |b| ops(b.variables(), Capability::VariableLookup, b)?.lookup(ProjectId(p.project), &p.target, &p.name)).await
    }

    /// Dónde se usa cada variable (paneles y sus objetos, acciones, alarmas, registro de datos…) y qué variables no se usan,
    /// buscando su nombre completo en atributos, scripts y expresiones del XML del proyecto.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn project_cross_references(&self, Parameters(p): Parameters<ReportTargetParams>) -> ToolResult<XrefOut> {
        self.blocking(move |b| {
            let all = ops(b.reports(), Capability::CrossReferences, b)?.cross_references(ProjectId(p.project), p.target.as_deref())?;
            let (references, unused): (Vec<CrossReference>, Vec<CrossReference>) = all.into_iter().partition(|r| !r.references.is_empty());
            Ok(XrefOut { count: references.len(), references, unused: unused.into_iter().map(|r| format!("{}/{}", r.target, r.variable)).collect() })
        })
        .await
    }

    /// Revisa los scripts del proyecto (o el código de `script`) sin compilar: variables que no existen o con otras
    /// mayúsculas, métodos que no son de la API, variables usadas sin getIntValue(), paréntesis y comillas sin cerrar…
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn script_lint(&self, Parameters(p): Parameters<ScriptLintParams>) -> ToolResult<ScriptLintOut> {
        self.blocking(move |b| {
            let all = ops(b.reports(), Capability::ScriptLint, b)?.lint_scripts(ProjectId(p.project), p.target.as_deref(), p.script.as_deref())?;
            let count = |s| all.iter().flat_map(|l| &l.issues).filter(|i| i.severity == s).count();
            let (errors, warnings) = (count(MessageSeverity::Error), count(MessageSeverity::Warning));
            let scripts_checked = all.len();
            let scripts = all.into_iter().filter(|l| p.include_clean || !l.issues.is_empty()).collect();
            Ok(ScriptLintOut { scripts_checked, errors, warnings, scripts })
        })
        .await
    }

    /// Abre un panel base del proyecto en la ventana de Vijeo (como el doble clic en el Navigator), para que el
    /// usuario lo vea o lo edite. No cambia el proyecto.
    #[tool(annotations(destructive_hint = false, idempotent_hint = true, open_world_hint = false))]
    async fn project_goto(&self, Parameters(p): Parameters<GotoParams>) -> ToolResult<GotoOut> {
        self.blocking(move |b| Ok(GotoOut { opened: ops(b.targets(), Capability::PanelOpen, b)?.open_panel(ProjectId(p.project), &p.target, &p.panel)? })).await
    }

    /// Valida un XML de proyecto (.vxml) contra el public.xsd de la instalación, sin Vijeo. Devuelve los errores con
    /// línea y columna y un resumen agrupado por mensaje.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn xml_validate(&self, Parameters(p): Parameters<XmlValidateParams>) -> ToolResult<XmlValidation> {
        self.blocking(move |b| ops(b.files(), Capability::XmlValidate, b)?.validate_xml(&p.path, p.max_issues.unwrap_or(50))).await
    }

    /// Escribe un CSV de variables con el formato de Export Variables de Vijeo, listo para Variables › Import Variables.
    /// Valida tipos, direcciones, valores iniciales y alarmas; con `project` avisa de nombres que ya existen y grupos de
    /// escaneo desconocidos. No importa nada en el proyecto.
    #[tool(annotations(destructive_hint = false, idempotent_hint = true, open_world_hint = false))]
    async fn variables_generate_csv(&self, Parameters(p): Parameters<VariableCsvRequest>) -> ToolResult<VariableCsvResult> {
        self.blocking(move |b| ops(b.files(), Capability::VariableCsv, b)?.variables_csv(&p)).await
    }

    /// Importa variables en un target del proyecto abierto con Variables › Import Variables: un CSV (`csv`, de
    /// variables_generate_csv o de Export Variables) o la lista `variables` (como en variables_generate_csv, se genera
    /// el CSV). Las que ya existen se conservan salvo con `replace_existing`. Devuelve lo que dice Vijeo; el .vdz no
    /// cambia hasta project_save. Tras un project_build en la misma instancia no funciona (guarda, cierra y reabre).
    #[tool(annotations(destructive_hint = true, idempotent_hint = false, open_world_hint = false))]
    async fn variables_import(&self, Parameters(p): Parameters<VariableImportRequest>) -> ToolResult<VariableImportResult> {
        self.blocking(move |b| ops(b.variables(), Capability::VariableImport, b)?.import_csv(&p)).await
    }

    /// Objetos de un panel base del proyecto abierto (Select All + Copy en su editor): ruta para panel_edit
    /// (`Grupo/Objeto`), tipo, posición y tamaño absolutos en el panel, textos (etiquetas…) y variables, con los
    /// objetos de cada grupo. No cambia el proyecto; usa el portapapeles de Windows y devuelve el texto que tenía.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn panel_objects(&self, Parameters(p): Parameters<GotoParams>) -> ToolResult<PanelObjectsResult> {
        self.blocking(move |b| ops(b.panels(), Capability::PanelObjects, b)?.objects(ProjectId(p.project), &p.target, &p.panel)).await
    }

    /// Edita los objetos de un panel base: rename, move, move_to, resize, set_text (un texto entero), replace (dentro
    /// de todas las cadenas, también variables: Vijeo las vuelve a enlazar por nombre), delete, animate (visibilidad,
    /// parpadeo, color, relleno, posición), remove_animations, set_actions (botones) y set_colors. Se comprueban antes
    /// de tocar nada; luego se cortan todos los objetos, se editan y se pegan, y se comprueba el resultado (si el panel
    /// quedara vacío, se vuelve a pegar el original). Con dry_run sólo devuelve cómo quedaría. Luego project_save.
    #[tool(annotations(destructive_hint = true, idempotent_hint = false, open_world_hint = false))]
    async fn panel_edit(&self, Parameters(p): Parameters<PanelEditRequest>) -> ToolResult<PanelChangeResult> {
        self.blocking(move |b| ops(b.panels(), Capability::PanelObjects, b)?.edit(&p)).await
    }

    /// Añade a un panel base objetos nuevos (`create`: switch, lamp, numeric_display, string_display, text, rectangle,
    /// ellipse, line; colores RRGGBB; variables con su ruta completa, que tienen que existir), piezas de la Toolchest
    /// (`toolchest`: id de toolchest_list) u objetos copiados de otro panel base del mismo target (`from_panel` +
    /// `object`), con su esquina superior izquierda en x, y; opcionalmente con otro nombre, `animations`, `visibility`
    /// y (salvo con `create`) sustituciones de texto o variables. Sin `name`, los nuevos se llaman como en Vijeo
    /// (`Switch01`…). Devuelve los objetos del panel después y avisa si Vijeo cambió algún
    /// nombre (no admite repetidos) o posición. Luego project_build (las piezas de la Toolchest llegan sin variables) y
    /// project_save.
    #[tool(annotations(destructive_hint = false, idempotent_hint = false, open_world_hint = false))]
    async fn panel_insert(&self, Parameters(p): Parameters<PanelInsertRequest>) -> ToolResult<PanelChangeResult> {
        self.blocking(move |b| ops(b.panels(), Capability::PanelObjects, b)?.insert(&p)).await
    }

    /// Toolchest de la instalación, sin abrir Vijeo: sin filtros, sus bibliotecas; con `library` o `text`, sus piezas
    /// (id para panel_insert, nombre, tipos de objeto y tamaño).
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn toolchest_list(&self, Parameters(f): Parameters<ToolchestFilter>) -> ToolResult<ToolchestResult> {
        self.blocking(move |b| ops(b.panels(), Capability::Toolchest, b)?.toolchest(&f)).await
    }

    /// Convierte con DataConverter.exe recetas (.rcp) y registros de datos (.dat) descargados del HMI a CSV/TXT, o un
    /// CSV a .rcp validándolo contra las recetas originales. Devuelve los ficheros escritos y la salida del programa.
    #[tool(annotations(destructive_hint = true, open_world_hint = false))]
    async fn data_convert(&self, Parameters(p): Parameters<DataConvertRequest>) -> ToolResult<DataConvertResult> {
        self.blocking(move |b| ops(b.files(), Capability::DataConvert, b)?.convert_data(&p)).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for VijeoServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("vijeo-mcp", env!("CARGO_PKG_VERSION")).with_title("Vijeo-Designer"))
            .with_instructions(INSTRUCTIONS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_capability_or_is_global() {
        let names: Vec<String> = VijeoServer::tool_router().list_all().into_iter().map(|t| t.name.to_string()).collect();
        for n in &names {
            let global = n == "vijeo_installations" || n == "vijeo_status";
            assert!(global || TOOL_CAPS.iter().any(|(t, _)| t == n), "{n} sin capacidad en TOOL_CAPS");
        }
        for (t, _) in TOOL_CAPS {
            assert!(names.iter().any(|n| n == t), "{t} en TOOL_CAPS no existe");
        }
    }

    #[test]
    fn without_backend_only_global_tools() {
        let s = VijeoServer::new(State { registry: BackendRegistry::new(), installations: vec![], backend: None, backend_error: Some("x".into()) });
        let names: Vec<String> = s.tool_router.list_all().into_iter().map(|t| t.name.to_string()).collect();
        assert_eq!(names, vec!["vijeo_installations", "vijeo_status"]);
    }
}
