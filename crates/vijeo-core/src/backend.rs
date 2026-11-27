use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use schemars::JsonSchema;
use serde::Serialize;

use crate::*;

/// Configuración común a todos los backends (viene de la línea de comandos del MCP).
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Máximo de proyectos abiertos a la vez (una instancia de Vijeo por proyecto).
    pub max_instances: usize,
    /// Espera máxima a que Vijeo arranque (abrir un proyecto tiene además un mínimo propio en el backend).
    pub start_timeout: Duration,
    /// Tiempo límite por defecto de una operación (guardar, exportar, un asistente…).
    pub call_timeout: Duration,
    /// Tiempo límite de las compilaciones.
    pub build_timeout: Duration,
    /// Tiempo que un `MessageBox` puede seguir abierto antes de abortar la llamada.
    pub dialog_grace: Duration,
    /// Carpetas donde nunca se abre ni se guarda nada (además del workspace de Vijeo-Manager).
    pub protected_dirs: Vec<PathBuf>,
    /// Carpeta de caché (p.ej. resultado de validar modelos en vivo).
    pub cache_dir: Option<PathBuf>,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            max_instances: 3,
            start_timeout: Duration::from_secs(30),
            call_timeout: Duration::from_secs(60),
            build_timeout: Duration::from_secs(600),
            dialog_grace: Duration::from_millis(1500),
            protected_dirs: Vec::new(),
            cache_dir: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BackendInfo {
    /// Identificador del backend (`vd63`).
    pub id: String,
    pub product_version: Version,
    pub support: Support,
    pub frame_dir: PathBuf,
    /// Carpetas protegidas efectivas (workspace de Vijeo-Manager incluido).
    pub protected_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstanceState {
    Starting,
    Ready,
    Busy,
    Crashed,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct InstanceStatus {
    pub project: Option<ProjectId>,
    pub project_name: Option<String>,
    pub pid: Option<u32>,
    pub state: InstanceState,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BackendStatus {
    pub instances: Vec<InstanceStatus>,
    pub max_instances: usize,
    /// Carpetas `M5DTemPHolder*` de procesos que ya no existen (Vijeo las borra en su próximo arranque).
    pub orphan_temp_dirs: Vec<PathBuf>,
}

/// Un backend implementa una versión concreta de Vijeo. Los métodos son síncronos: la capa MCP
/// los ejecuta con `spawn_blocking`. Cada grupo de operaciones es opcional según sus capacidades.
pub trait Backend: Send + Sync {
    fn info(&self) -> BackendInfo;
    fn capabilities(&self) -> &CapabilitySet;
    fn status(&self) -> BackendStatus;
    fn projects(&self) -> Option<&dyn ProjectOps> {
        None
    }
    fn targets(&self) -> Option<&dyn TargetOps> {
        None
    }
    fn build(&self) -> Option<&dyn BuildOps> {
        None
    }
    fn reports(&self) -> Option<&dyn ReportOps> {
        None
    }
    fn variables(&self) -> Option<&dyn VariableOps> {
        None
    }
    fn catalog(&self) -> Option<&dyn CatalogOps> {
        None
    }
    fn config(&self) -> Option<&dyn ConfigOps> {
        None
    }
    fn files(&self) -> Option<&dyn FileOps> {
        None
    }
    fn panels(&self) -> Option<&dyn PanelOps> {
        None
    }
    /// Cierra todos los proyectos (sin guardar) y termina todas las instancias de Vijeo.
    fn shutdown(&self) -> VijeoResult<()>;
}

pub trait ProjectOps: Send + Sync {
    /// Crea un proyecto con un target (Vijeo no admite proyectos sin target) y lo guarda en `save_as` (`.vdz`).
    fn new_project(&self, name: &str, save_as: &Path, overwrite: bool, target: &str, model: &ModelSelector) -> VijeoResult<ProjectInfo>;
    fn open(&self, vdz: &Path) -> VijeoResult<ProjectInfo>;
    /// Guarda en la ruta actual o en `to` (guardar como).
    fn save(&self, p: ProjectId, to: Option<&Path>, overwrite: bool) -> VijeoResult<ProjectInfo>;
    /// Cierra el proyecto y su instancia. Falla si hay cambios sin guardar salvo con `discard`.
    fn close(&self, p: ProjectId, discard: bool) -> VijeoResult<()>;
    fn info(&self, p: ProjectId) -> VijeoResult<ProjectInfo>;
    fn list(&self) -> Vec<ProjectSummary>;
    /// Cambia el nombre del proyecto (no el del `.vdz`).
    fn rename(&self, p: ProjectId, name: &str) -> VijeoResult<ProjectInfo>;
}

pub trait TargetOps: Send + Sync {
    fn list(&self, p: ProjectId) -> VijeoResult<Vec<TargetInfo>>;
    fn add(&self, p: ProjectId, name: &str, model: &ModelSelector) -> VijeoResult<TargetInfo>;
    fn delete(&self, p: ProjectId, target: &str) -> VijeoResult<()>;
    fn rename(&self, p: ProjectId, target: &str, new_name: &str) -> VijeoResult<TargetInfo>;
    fn panels(&self, p: ProjectId, target: &str) -> VijeoResult<Vec<PanelRef>>;
    /// Abre el panel en la ventana de Vijeo, como al hacer doble clic en el Navigator. `panel` es la ruta de
    /// panel_list (`BP:\Carpeta\Panel`) o la ruta sin `BP:\`.
    fn open_panel(&self, p: ProjectId, target: &str, panel: &str) -> VijeoResult<PanelRef>;
}

pub trait BuildOps: Send + Sync {
    fn build_project(&self, p: ProjectId) -> VijeoResult<BuildResult>;
    fn build_target(&self, p: ProjectId, target: &str) -> VijeoResult<BuildResult>;
    /// Compila una copia de un `.vdz` en una instancia de Vijeo sin COM y la descarta (el original no se
    /// toca). Con `target`, sólo cuentan los mensajes de ese target.
    fn build_file(&self, vdz: &Path, target: Option<&str>) -> VijeoResult<BuildResult>;
    /// Contenido actual de la ventana de salida de la instancia del proyecto.
    fn feedback(&self, p: ProjectId) -> VijeoResult<FeedbackLog>;
}

pub trait ConfigOps: Send + Sync {
    /// Configuración del proyecto abierto, leída de su `.vdz` (el último guardado).
    fn project_config(&self, p: ProjectId, target: Option<&str>) -> VijeoResult<ProjectConfig>;
    /// Configuración de un `.vdz` cualquiera, sin abrirlo en Vijeo.
    fn file_config(&self, vdz: &Path, target: Option<&str>) -> VijeoResult<ProjectConfig>;
    /// Aplica `changes` a un target de `vdz` sin Vijeo y guarda el resultado en `save_as` (o en `vdz`).
    /// `target` puede faltar si el proyecto sólo tiene uno. Con `dry_run` no escribe nada.
    fn edit_file_config(
        &self,
        vdz: &Path,
        target: Option<&str>,
        changes: &TargetConfigChanges,
        save_as: Option<&Path>,
        overwrite: bool,
        dry_run: bool,
    ) -> VijeoResult<ConfigEditResult>;
}

/// Lecturas del proyecto abierto a partir de su XML (Export Project).
pub trait ReportOps: Send + Sync {
    /// Exporta el proyecto como `<nombre>.vxml` (y la carpeta de imágenes) en `dir`.
    fn export_xml(&self, p: ProjectId, dir: &Path, overwrite: bool) -> VijeoResult<XmlExportResult>;
    /// Variables de uno o de todos los targets.
    fn variables(&self, p: ProjectId, target: Option<&str>) -> VijeoResult<Vec<VariableInfo>>;
    /// Paneles base, maestros y ventanas emergentes con sus acciones de panel.
    fn panels(&self, p: ProjectId, target: Option<&str>) -> VijeoResult<Vec<PanelReport>>;
    /// Acciones globales.
    fn actions(&self, p: ProjectId, target: Option<&str>) -> VijeoResult<Vec<ActionInfo>>;
    /// Dónde se usa cada variable (incluye las que no se usan, con `references` vacío).
    fn cross_references(&self, p: ProjectId, target: Option<&str>) -> VijeoResult<Vec<CrossReference>>;
    /// Revisa los scripts del proyecto o, con `script`, ese texto contra las variables del target.
    fn lint_scripts(&self, p: ProjectId, target: Option<&str>, script: Option<&str>) -> VijeoResult<Vec<ScriptLint>>;
}

/// Ficheros que se tratan sin abrir Vijeo (con herramientas o datos de la instalación).
pub trait FileOps: Send + Sync {
    /// Valida un XML contra el `public.xsd` de la instalación. `max_issues` limita la lista `issues`.
    fn validate_xml(&self, path: &Path, max_issues: usize) -> VijeoResult<XmlValidation>;
    /// Escribe un CSV de variables en el formato de Export/Import Variables de Vijeo.
    fn variables_csv(&self, req: &VariableCsvRequest) -> VijeoResult<VariableCsvResult>;
    /// Convierte recetas o registros de datos con `DataConverter.exe`.
    fn convert_data(&self, req: &DataConvertRequest) -> VijeoResult<DataConvertResult>;
}

/// Objetos de los paneles base de un proyecto abierto (por el portapapeles de Vijeo) y piezas de la Toolchest.
pub trait PanelOps: Send + Sync {
    /// Objetos de un panel base, con sus grupos y lo que contienen.
    fn objects(&self, p: ProjectId, target: &str, panel: &str) -> VijeoResult<PanelObjectsResult>;
    /// Aplica operaciones (renombrar, mover, cambiar textos o variables, borrar) a los objetos de un panel base.
    fn edit(&self, req: &PanelEditRequest) -> VijeoResult<PanelChangeResult>;
    /// Añade a un panel base piezas de la Toolchest u objetos copiados de otro panel.
    fn insert(&self, req: &PanelInsertRequest) -> VijeoResult<PanelChangeResult>;
    /// Bibliotecas de la Toolchest de la instalación o, con filtro, sus piezas.
    fn toolchest(&self, filter: &ToolchestFilter) -> VijeoResult<ToolchestResult>;
}

pub trait VariableOps: Send + Sync {
    fn lookup(&self, p: ProjectId, target: &str, name: &str) -> VijeoResult<VariableLookup>;
    /// Import Variables de un CSV (o de las variables de la petición) en un target del proyecto abierto.
    fn import_csv(&self, req: &VariableImportRequest) -> VijeoResult<VariableImportResult>;
}

pub trait CatalogOps: Send + Sync {
    fn families(&self) -> Vec<FamilyInfo>;
    fn models(&self, filter: &ModelFilter) -> Vec<ModelInfo>;
    fn resolve(&self, selector: &ModelSelector) -> VijeoResult<ResolvedModel>;
    /// Modelo del catálogo para un `hmi_type` devuelto por `Target.Type()`.
    fn identify(&self, hmi_type: &str) -> Option<ModelRef>;
    fn drivers(&self, filter: &DriverFilter) -> Vec<DriverInfo>;
    /// Crea y borra en un proyecto temporal un target de cada modelo de `filter` (sin mirar `creatable`) y
    /// guarda el resultado en la caché, que manda sobre la regla de `creatable` desde el próximo arranque.
    /// `progress` recibe cada resultado según sale.
    fn probe_models(&self, filter: &ModelFilter, progress: &mut dyn FnMut(&ModelProbe)) -> VijeoResult<ModelProbeReport>;
}

/// Crea backends para las instalaciones que soporta.
pub trait BackendFactory: Send + Sync {
    fn id(&self) -> &'static str;
    fn supports(&self, inst: &Installation) -> Support;
    fn create(&self, inst: &Installation, support: Support, cfg: &BackendConfig) -> VijeoResult<Arc<dyn Backend>>;
}

/// Elección de instalación + factoría.
pub struct Selection<'a> {
    pub installation: Installation,
    pub factory: &'a dyn BackendFactory,
    pub support: Support,
}

/// Lista estática de factorías conocidas.
#[derive(Default)]
pub struct BackendRegistry {
    factories: Vec<Box<dyn BackendFactory>>,
}

impl BackendRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(mut self, f: Box<dyn BackendFactory>) -> Self {
        self.factories.push(f);
        self
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.factories.iter().map(|f| f.id()).collect()
    }

    /// Elige la mejor pareja instalación/backend: mayor `Support` y, a igualdad, la versión más alta.
    /// `version` (p.ej. `"6.3"`) restringe las instalaciones candidatas.
    pub fn select(&self, installs: &[Installation], version: Option<&str>) -> Option<Selection<'_>> {
        let wanted = version.and_then(Version::parse);
        let mut best: Option<Selection<'_>> = None;
        for inst in installs {
            if let Some(w) = wanted {
                let v = inst.product_version;
                if v.major != w.major || v.minor != w.minor {
                    continue;
                }
            }
            for f in &self.factories {
                let support = f.supports(inst);
                if support == Support::No {
                    continue;
                }
                let better = match &best {
                    None => true,
                    Some(b) => (support, inst.product_version) > (b.support, b.installation.product_version),
                };
                if better {
                    best = Some(Selection { installation: inst.clone(), factory: f.as_ref(), support });
                }
            }
        }
        best
    }

    /// Qué backend usaría cada instalación (para `vijeo_installations`).
    pub fn describe(&self, inst: &Installation) -> Option<(&'static str, Support)> {
        self.factories
            .iter()
            .map(|f| (f.id(), f.supports(inst)))
            .filter(|(_, s)| *s != Support::No)
            .max_by_key(|(_, s)| *s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(&'static str, u16);
    impl BackendFactory for Fake {
        fn id(&self) -> &'static str {
            self.0
        }
        fn supports(&self, inst: &Installation) -> Support {
            match inst.product_version.minor {
                m if m == self.1 => Support::Exact,
                m if m == self.1 + 1 => Support::Compatible,
                _ => Support::No,
            }
        }
        fn create(&self, _: &Installation, _: Support, _: &BackendConfig) -> VijeoResult<Arc<dyn Backend>> {
            unreachable!()
        }
    }

    fn inst(v: &str) -> Installation {
        Installation {
            product_version: Version::parse(v).unwrap(),
            frame_dir: PathBuf::from(format!("C:\\vd{v}\\Vijeo-Frame")),
            registry_key: String::new(),
        }
    }

    #[test]
    fn prefers_exact_then_newest() {
        let reg = BackendRegistry::new().register(Box::new(Fake("vd63", 3)));
        let installs = [inst("6.3.0.103"), inst("6.4.0.1"), inst("6.1.0.0")];
        let s = reg.select(&installs, None).unwrap();
        assert_eq!(s.installation.product_version.minor, 3);
        assert_eq!(s.support, Support::Exact);

        let s = reg.select(&installs, Some("6.4")).unwrap();
        assert_eq!(s.support, Support::Compatible);

        assert!(reg.select(&installs, Some("6.1")).is_none());
    }
}
