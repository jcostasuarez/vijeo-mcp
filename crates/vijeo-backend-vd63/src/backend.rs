//! `Vd63Backend`: cada proyecto abierto es una instancia normal de Vijeo-Frame (D2, inventario §12.10),
//! con una copia del proyecto en `Vijeo-Manager\MCP_TMP_*` ([`UiProject`]).
//!
//! El MCP no usa la API COM: por COM Vijeo trabaja en modo SoMachine (§12.9), la validación añade a cada
//! target una red SoMachine sin dirección (falso `Error 1331`) y `Save` la guarda. Aquí todo pasa por la
//! interfaz: menús, asistentes, el Navigator y Export Project para leer variables, paneles y acciones del
//! `.vxml`. El nombre del proyecto es sólo el de la entrada `.SwxCF` del `.vdz`: lo pone el MCP al empaquetar.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::time::{Duration, Instant, SystemTime};

use vijeo_core::*;
use vijeo_ui::process;

use crate::catalog::{split_hmi_type, Catalog};
use crate::ui_project::{self, NewTarget, Timeouts, UiProject};
use crate::{dataconv, drivers, feedback, lint, objects, panel_ops, probe, reports, script, swx, toolchest, validate, varcsv, vdz, vxml, xsd};
use crate::widgets::build::{self, RefKind};

/// Esquema público del XML de proyecto, relativo a `Vijeo-Frame`.
const PUBLIC_XSD: &str = r"XML\public.xsd";

/// Modelos que el asistente New Target muestra con otro nombre que el del catálogo (los dos de tablets se
/// llaman «Tablet» en `Models_Tablets`; el nombre del asistente sale de sus recursos). Visto con --probe-models.
const WIZARD_MODEL_NAMES: &[(&str, &str)] = &[("Model_Tablets", "Android"), ("Model_iOS", "iOS"), ("TARGET_HMI_P6", "Harmony P6 Box PN")];

/// Mínimo para abrir una copia: incluye actualizarla de versión.
const UI_OPEN_TIMEOUT: Duration = Duration::from_secs(180);

struct Instance {
    id: ProjectId,
    pid: u32,
    /// `None` cuando ya se cerró.
    ui: Mutex<Option<UiProject>>,
    /// Carpeta de trabajo propia (exports); se borra al cerrar.
    work: PathBuf,
    path: Mutex<Option<PathBuf>>,
    name: Mutex<String>,
    /// Hay cambios que no están en el `.vdz` (la copia de trabajo se guarda tras cada operación).
    unsaved: AtomicBool,
    /// Último Export Project: (fecha del `.SwxCF` al exportar, texto del `.vxml`).
    export: Mutex<Option<(Option<SystemTime>, Arc<String>)>>,
}

impl Instance {
    fn path(&self) -> Option<PathBuf> {
        self.path.lock().unwrap().clone()
    }

    fn name(&self) -> String {
        self.name.lock().unwrap().clone()
    }

    fn mark_unsaved(&self) {
        self.unsaved.store(true, Ordering::SeqCst);
    }

    /// Ejecuta `f` con la instancia de Vijeo del proyecto (una operación a la vez).
    fn with_ui<R>(&self, op: &str, f: impl FnOnce(&mut UiProject) -> VijeoResult<R>) -> VijeoResult<R> {
        let mut guard = self.ui.lock().unwrap_or_else(|e| e.into_inner());
        let ui = guard.as_mut().ok_or(VijeoError::ProjectNotFound(self.id.0))?;
        if !ui.is_alive() {
            return Err(VijeoError::ServerCrashed {
                op: format!("{op}: Vijeo (PID {}) del proyecto {} ya no existe; ciérralo con project_close y vuelve a abrirlo", self.pid, self.id),
            });
        }
        f(ui)
    }

    /// Cierra Vijeo sin guardar y borra la copia de trabajo. Devuelve un aviso si quedó algo sin borrar.
    fn shutdown(&self) -> Option<String> {
        let ui = self.ui.lock().unwrap_or_else(|e| e.into_inner()).take();
        let warning = ui.and_then(UiProject::close);
        let _ = std::fs::remove_dir_all(&self.work);
        warning
    }
}

#[derive(Default)]
struct Pool {
    map: BTreeMap<ProjectId, Arc<Instance>>,
    starting: usize,
}

/// Plaza reservada en el pool para una instancia que arranca; se libera al soltarla.
struct Slot<'a>(&'a Mutex<Pool>);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.0.lock().unwrap().starting -= 1;
    }
}

pub struct Vd63Backend {
    inst: Installation,
    support: Support,
    cfg: BackendConfig,
    caps: CapabilitySet,
    catalog: Option<Catalog>,
    drivers: Vec<DriverInfo>,
    /// Fichero de `--probe-models` de esta instalación (`None` sin carpeta de caché).
    probe_cache: Option<PathBuf>,
    workspace: PathBuf,
    protected: Vec<PathBuf>,
    pool: Mutex<Pool>,
    /// Un arranque a la vez: varias instancias arrancando a la vez se estorban (diálogos, `Vijeo-Manager`).
    launching: Mutex<()>,
    next_id: AtomicU32,
    /// Bibliotecas de la Toolchest (se leen la primera vez que hacen falta).
    toolchest: OnceLock<Vec<toolchest::Library>>,
}

/// `%PUBLIC%\Documents\Vijeo-Designer <x.y>\Vijeo-Manager`: proyectos reales del usuario.
fn workspace_dir(v: Version) -> PathBuf {
    let public = std::env::var_os("PUBLIC").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"));
    public.join("Documents").join(format!("Vijeo-Designer {}", v.short())).join("Vijeo-Manager")
}

fn temp_dir(kind: &str) -> VijeoResult<PathBuf> {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join("vijeo-mcp").join(format!("{kind}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&d).map_err(|e| VijeoError::io(&d, e))?;
    Ok(d)
}

fn check_vdz_input(field: &str, vdz: &Path) -> VijeoResult<()> {
    if !vdz.is_absolute() || !vdz.extension().is_some_and(|e| e.eq_ignore_ascii_case("vdz")) {
        return Err(VijeoError::invalid(field, "debe ser la ruta absoluta de un .vdz"));
    }
    if !vdz.is_file() {
        return Err(VijeoError::invalid(field, format!("{} no existe", vdz.display())));
    }
    Ok(())
}

/// Resolución de una pantalla (`HMIDT351 (800x480)` → `800x480`).
fn display_resolution(display: &str) -> Option<String> {
    let inner = display.rsplit_once('(')?.1.strip_suffix(')')?;
    Some(inner.replace(' ', ""))
}

/// Deja sólo el target pedido (sin distinguir mayúsculas).
fn select_target(targets: Vec<TargetConfig>, target: Option<&str>) -> VijeoResult<Vec<TargetConfig>> {
    let Some(t) = target else { return Ok(targets) };
    let found: Vec<TargetConfig> = targets.into_iter().filter(|x| x.name.eq_ignore_ascii_case(t)).collect();
    if found.is_empty() {
        return Err(VijeoError::TargetNotFound(t.to_owned()));
    }
    Ok(found)
}

/// Cuenta errores y warnings de una compilación (de todo el proyecto o sólo de `target`).
fn build_result(lines: &[String], target: Option<&str>, duration_ms: u64, notes: Vec<String>) -> BuildResult {
    let log = feedback::parse(lines);
    let count = |m: &[BuildMessage], s| m.iter().filter(|x| x.severity == s).count() as i32;
    let (messages, (errors, warnings)) = match target {
        Some(tg) => {
            let m = feedback::only_target(log.messages, tg);
            let c = (count(&m, MessageSeverity::Error), count(&m, MessageSeverity::Warning));
            (m, c)
        }
        None => {
            let c = feedback::summary(lines)
                .map(|s| (s.errors, s.warnings))
                .unwrap_or_else(|| (count(&log.messages, MessageSeverity::Error), count(&log.messages, MessageSeverity::Warning)));
            (log.messages, c)
        }
    };
    BuildResult { errors, warnings, duration_ms, messages, output: log.output, output_note: None, notes }
}

/// Pone `name` en el `<Project Name>` de un `.vxml` exportado de la copia `temp`.
fn rename_exported_project(xml: &str, temp: &str, name: &str) -> String {
    let attr = format!("Name=\"{temp}\"");
    match (xml.find("<Project"), xml.find(&attr)) {
        (Some(p), Some(a)) if a > p && !xml[p..a].contains('>') => {
            format!("{}Name=\"{}\"{}", &xml[..a], validate::xml_escape(name), &xml[a + attr.len()..])
        }
        _ => xml.to_owned(),
    }
}

impl Vd63Backend {
    pub fn new(inst: &Installation, support: Support, cfg: &BackendConfig) -> VijeoResult<Self> {
        if !inst.frame_dir.join("Vijeo-Frame.exe").is_file() {
            return Err(VijeoError::NotInstalled(format!("no está Vijeo-Frame.exe en {}", inst.frame_dir.display())));
        }
        let probe_cache = cfg.cache_dir.clone().or_else(probe::default_dir).map(|d| probe::cache_file(&d, inst));
        let mut catalog = Catalog::load(&inst.frame_dir);
        if let (Ok(c), Some(file)) = (&mut catalog, &probe_cache) {
            let probes = probe::load(file, inst);
            if !probes.is_empty() {
                tracing::info!(file = %file.display(), models = probes.len(), "creabilidad de modelos según --probe-models");
                c.apply_probes(&probes);
            }
        }
        let workspace = workspace_dir(inst.product_version);
        let mut protected = vec![workspace.clone(), inst.root_dir()];
        protected.extend(cfg.protected_dirs.iter().cloned());

        // Menús, diálogos y formatos verificados sólo con 6.3.
        let gate = |caps: CapabilitySet, cap| match support {
            Support::Exact => caps.supported(cap),
            _ => caps.with(cap, CapabilityState::Gated { option: "versión no verificada: usa --vijeo-version para forzar el backend 6.3".into() }),
        };
        let mut caps = CapabilitySet::new();
        for cap in [
            Capability::ProjectLifecycle,
            Capability::TargetManage,
            Capability::Build,
            Capability::Reports,
            Capability::CrossReferences,
            Capability::VariableLookup,
            Capability::PanelList,
            // El formato de lo que se escribe sólo está verificado con 6.3.
            Capability::ConfigEdit,
            Capability::VariableCsv,
            Capability::ScriptLint,
            Capability::PanelOpen,
            Capability::VariableImport,
            // Formato binario de los objetos y comandos del editor verificados con 6.3 (inventario §13.2).
            Capability::PanelObjects,
        ] {
            caps = gate(caps, cap);
        }
        // Herramientas y esquema de la propia instalación: valen para la versión que sea.
        caps = if inst.frame_dir.join(PUBLIC_XSD).is_file() {
            caps.supported(Capability::XmlValidate)
        } else {
            caps.unsupported(Capability::XmlValidate, format!("no está {PUBLIC_XSD} en la instalación"))
        };
        caps = if inst.frame_dir.join(dataconv::EXE).is_file() {
            caps.supported(Capability::DataConvert)
        } else {
            caps.unsupported(Capability::DataConvert, format!("no está {} en la instalación", dataconv::EXE))
        };
        caps = match &catalog {
            Ok(_) => caps.supported(Capability::ModelCatalog),
            Err(e) => caps.unsupported(Capability::ModelCatalog, format!("no se pudo leer TargetInfo: {e}")),
        };
        // Sin catálogo no se sabe qué familia y modelo elegir en el asistente.
        if catalog.is_err() {
            caps = caps.unsupported(Capability::TargetManage, "sin catálogo de modelos no se pueden elegir los targets");
        }
        // Se lee del .vdz, sin Vijeo: no depende de que la versión esté verificada.
        caps = caps.supported(Capability::ProjectConfig);
        // Las piezas usan el formato de objetos de 6.3: sólo con esa versión.
        caps = if !toolchest::root(&inst.frame_dir).is_dir() {
            caps.unsupported(Capability::Toolchest, "no está la carpeta Toolchest en la instalación")
        } else {
            gate(caps, Capability::Toolchest)
        };
        let drivers = drivers::load(&inst.frame_dir);
        caps = match &drivers {
            Ok(d) if !d.is_empty() => caps.supported(Capability::DriverCatalog),
            Ok(_) => caps.unsupported(Capability::DriverCatalog, "no hay ningún *_PDL.dll en HMI IO"),
            Err(e) => caps.unsupported(Capability::DriverCatalog, format!("no se pudo leer HMI IO: {e}")),
        };
        Ok(Self {
            inst: inst.clone(),
            support,
            cfg: cfg.clone(),
            caps,
            catalog: catalog.map_err(|e| tracing::warn!("catálogo no disponible: {e}")).ok(),
            drivers: drivers.unwrap_or_default(),
            probe_cache,
            workspace,
            protected,
            pool: Mutex::new(Pool::default()),
            launching: Mutex::new(()),
            next_id: AtomicU32::new(1),
            toolchest: OnceLock::new(),
        })
    }

    fn require(&self, cap: Capability) -> VijeoResult<()> {
        match self.caps.state(cap) {
            CapabilityState::Supported => Ok(()),
            CapabilityState::Gated { option } => Err(VijeoError::Unsupported { capability: cap, version: self.inst.product_version.to_string(), reason: option }),
            CapabilityState::Unsupported { reason } => Err(VijeoError::Unsupported { capability: cap, version: self.inst.product_version.to_string(), reason }),
        }
    }

    fn catalog_ref(&self) -> VijeoResult<&Catalog> {
        self.require(Capability::ModelCatalog)?;
        self.catalog.as_ref().ok_or_else(|| VijeoError::Internal("catálogo no cargado".into()))
    }

    fn timeouts(&self) -> Timeouts {
        Timeouts { open: self.cfg.start_timeout.max(UI_OPEN_TIMEOUT), build: self.cfg.build_timeout, call: self.cfg.call_timeout, dialog_grace: self.cfg.dialog_grace }
    }

    fn get(&self, p: ProjectId) -> VijeoResult<Arc<Instance>> {
        self.pool.lock().unwrap().map.get(&p).cloned().ok_or(VijeoError::ProjectNotFound(p.0))
    }

    /// Reserva una instancia de Vijeo más (`--max-instances`) para `what`.
    fn reserve(&self, what: &str) -> VijeoResult<Slot<'_>> {
        let mut pool = self.pool.lock().unwrap();
        let used = pool.map.len() + pool.starting;
        if used >= self.cfg.max_instances {
            return Err(VijeoError::Limit(format!(
                "{what} necesita una instancia de Vijeo más y ya hay {used} (máximo {}, --max-instances); cierra algún proyecto con project_close",
                self.cfg.max_instances
            )));
        }
        pool.starting += 1;
        Ok(Slot(&self.pool))
    }

    /// Nombre temporal libre en el workspace.
    fn temp_name(&self) -> VijeoResult<String> {
        let name = vdz::temp_project_name();
        if self.workspace.join(&name).exists() {
            return Err(VijeoError::Internal(format!("ya existe Vijeo-Manager\\{name}")));
        }
        Ok(name)
    }

    /// Arranca Vijeo con `start(carpeta de trabajo, nombre temporal, tiempos)` y registra el proyecto.
    fn launch(&self, path: Option<PathBuf>, name: String, start: impl FnOnce(&Path, &str, Timeouts) -> VijeoResult<UiProject>) -> VijeoResult<Arc<Instance>> {
        let _slot = self.reserve("abrir un proyecto")?;
        let _one_at_a_time = self.launching.lock().unwrap_or_else(|e| e.into_inner());
        let temp = self.temp_name()?;
        let work = temp_dir("project")?;
        let ui = match start(&work, &temp, self.timeouts()) {
            Ok(u) => u,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&work);
                return Err(e);
            }
        };
        let id = ProjectId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let inst = Arc::new(Instance {
            id,
            pid: ui.pid(),
            ui: Mutex::new(Some(ui)),
            work,
            path: Mutex::new(path),
            name: Mutex::new(name),
            unsaved: AtomicBool::new(false),
            export: Mutex::new(None),
        });
        self.pool.lock().unwrap().map.insert(id, inst.clone());
        tracing::info!(project = %id, pid = inst.pid, "proyecto abierto");
        Ok(inst)
    }

    /// Quita el proyecto del pool y cierra su Vijeo.
    fn discard(&self, inst: &Instance) {
        self.pool.lock().unwrap().map.remove(&inst.id);
        if let Some(w) = inst.shutdown() {
            tracing::warn!("{w}");
        }
    }

    fn hmi_type(&self, model: &str, resolution: Option<&str>) -> String {
        self.catalog.as_ref().map(|c| c.hmi_type_of(model, resolution)).unwrap_or_else(|| model.to_owned())
    }

    fn target_info(&self, t: &TargetConfig) -> TargetInfo {
        let hmi_type = t.hmi_type.clone().unwrap_or_default();
        let model = self.catalog.as_ref().and_then(|c| c.identify(&hmi_type));
        TargetInfo { name: t.name.clone(), hmi_type, model }
    }

    /// Targets de la copia de trabajo (lo último guardado por Vijeo: las operaciones del MCP guardan al terminar).
    fn read_project(&self, inst: &Instance, op: &str) -> VijeoResult<Vec<TargetConfig>> {
        let mut targets = inst.with_ui(op, |u| {
            let mut c = swx::open_swx(&u.swx())?;
            Ok(swx::read_targets(&mut c, None))
        })?;
        for t in &mut targets {
            t.hmi_type = t.model.as_deref().map(|m| self.hmi_type(m, t.resolution.as_deref()));
        }
        Ok(targets)
    }

    fn targets_of(&self, inst: &Instance) -> VijeoResult<Vec<TargetConfig>> {
        self.read_project(inst, "leer los targets")
    }

    fn info_of(&self, inst: &Instance) -> VijeoResult<ProjectInfo> {
        let targets = self.read_project(inst, "project_info")?;
        Ok(ProjectInfo {
            id: inst.id,
            name: inst.name(),
            path: inst.path(),
            dirty: inst.unsaved.load(Ordering::SeqCst),
            targets: targets.iter().map(|t| self.target_info(t)).collect(),
            notes: Vec::new(),
        })
    }

    /// Lo que se pide al asistente para el modelo `resolved`.
    fn new_target(&self, name: &str, resolved: &ResolvedModel) -> VijeoResult<NewTarget> {
        let display = (!resolved.model.display_units.is_empty()).then(|| split_hmi_type(&resolved.hmi_type).1.map(str::to_owned)).flatten();
        let model = WIZARD_MODEL_NAMES
            .iter()
            .find(|(internal, _)| internal.eq_ignore_ascii_case(&resolved.model.internal_name))
            .map(|(_, shown)| (*shown).to_owned())
            .unwrap_or_else(|| resolved.model.name.clone());
        Ok(NewTarget { name: name.to_owned(), family: self.catalog_ref()?.family_name(&resolved.model), model, display })
    }

    /// Comprueba el target que acaba de crear el asistente (el que no estaba en `before`): le pone `name`
    /// si Vijeo no lo tomó y verifica modelo y pantalla.
    fn check_new_target(&self, inst: &Instance, before: &[String], name: &str, resolved: &ResolvedModel) -> VijeoResult<TargetInfo> {
        let targets = self.targets_of(inst)?;
        let new: Vec<&TargetConfig> = targets.iter().filter(|t| !before.iter().any(|b| b.eq_ignore_ascii_case(&t.name))).collect();
        let [t] = new.as_slice() else {
            let names: Vec<&str> = targets.iter().map(|t| t.name.as_str()).collect();
            return Err(VijeoError::Internal(format!("el asistente de Vijeo no creó el target (targets: {})", names.join(", "))));
        };
        let mut t = (*t).clone();
        if t.name != name {
            let got = inst.with_ui("poner el nombre del target", |u| u.rename_target(&t.name, name))?;
            if got != name {
                return Err(VijeoError::invalid("name", format!("Vijeo creó el target como «{}» y no aceptó «{name}»", t.name)));
            }
            t.name = got;
        }
        // El `Document` de los iPC no tiene las propiedades en el orden conocido y no da el modelo: no se
        // puede comprobar, pero Vijeo creó el target con lo elegido en el asistente (--probe-models).
        if t.model.is_none() {
            tracing::warn!(target = %t.name, model = %resolved.model.internal_name, "no se pudo leer el modelo del target nuevo en el .SwxCF: no se comprueba");
            return Ok(self.target_info(&t));
        }
        let model_ok = t.model.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(&resolved.model.internal_name));
        let want_res = split_hmi_type(&resolved.hmi_type).1.filter(|_| !resolved.model.display_units.is_empty()).and_then(display_resolution);
        let display_ok = match (&want_res, &t.resolution) {
            (Some(w), Some(got)) => w == got,
            _ => true,
        };
        if !model_ok || !display_ok {
            return Err(VijeoError::invalid(
                "model",
                format!(
                    "Vijeo creó el target «{}» como {} ({}) en vez de {}",
                    t.name,
                    t.model.as_deref().unwrap_or("?"),
                    t.resolution.as_deref().unwrap_or("?"),
                    resolved.hmi_type
                ),
            ));
        }
        Ok(self.target_info(&t))
    }

    /// Proyecto nuevo (asistente New Project) con su primer target ya comprobado; todavía sin `.vdz`.
    fn launch_new(&self, name: &str, target: &str, resolved: &ResolvedModel) -> VijeoResult<Arc<Instance>> {
        let nt = self.new_target(target, resolved)?;
        let (frame, ws) = (self.inst.frame_dir.clone(), self.workspace.clone());
        let inst = self.launch(None, name.to_owned(), |_, temp, t| UiProject::create(&frame, &ws, temp, &nt, t))?;
        let mut check = self.check_new_target(&inst, &[], target, resolved);
        // En frío, el primer Save puede llegar antes de que Vijeo acabe de crear el proyecto y dejar el
        // `.SwxCF` sin el target (visto una vez con HMIG3U en --probe-models): se guarda otra vez y se mira.
        if matches!(check, Err(VijeoError::Internal(_))) && self.targets_of(&inst).is_ok_and(|t| t.is_empty()) {
            tracing::warn!("el proyecto nuevo se guardó sin targets: se guarda otra vez");
            check = inst.with_ui("project_new", |u| u.save()).and_then(|_| self.check_new_target(&inst, &[], target, resolved));
        }
        if let Err(e) = check {
            self.discard(&inst);
            return Err(e);
        }
        Ok(inst)
    }

    /// New Target con un modelo ya resuelto. Si el target no queda como se pidió, se deshace.
    fn add_resolved(&self, inst: &Instance, name: &str, resolved: &ResolvedModel) -> VijeoResult<TargetInfo> {
        let before: Vec<String> = self.targets_of(inst)?.into_iter().map(|t| t.name).collect();
        if before.len() >= validate::MAX_TARGETS {
            return Err(VijeoError::Limit(format!("el proyecto ya tiene {} targets (Vijeo admite {})", before.len(), validate::MAX_TARGETS)));
        }
        if before.iter().any(|x| x.eq_ignore_ascii_case(name)) {
            return Err(VijeoError::invalid("name", format!("ya existe un target «{name}»")));
        }
        let nt = self.new_target(name, resolved)?;
        inst.with_ui("New Target", |u| u.add_target(&nt))?;
        inst.mark_unsaved();
        match self.check_new_target(inst, &before, name, resolved) {
            Ok(t) => Ok(t),
            Err(e) => {
                // Se deshace: el target nuevo es el que no estaba antes.
                if let Ok(ts) = self.targets_of(inst) {
                    for t in ts.iter().filter(|t| !before.iter().any(|b| b.eq_ignore_ascii_case(&t.name))) {
                        let _ = inst.with_ui("deshacer New Target", |u| u.delete_target(&t.name));
                    }
                }
                Err(e)
            }
        }
    }

    /// Prueba un modelo: crea un proyecto con él si no hay ninguno (`project`), o le añade un target y lo
    /// borra. Tras un error que no sea del modelo, el proyecto se descarta: puede haber quedado a medias.
    fn probe_one(&self, project: &mut Option<Arc<Instance>>, m: &ModelInfo) -> ModelProbe {
        const PROJECT: &str = "MCP_PROBE";
        const BASE: &str = "Base";
        const TRIAL: &str = "Prueba";
        let resolved = ResolvedModel { model: m.clone(), hmi_type: m.hmi_type.clone() };
        let t0 = Instant::now();
        let r = match project.clone() {
            None => self.launch_new(PROJECT, BASE, &resolved).map(|inst| *project = Some(inst)),
            Some(inst) => self.add_resolved(&inst, TRIAL, &resolved).and_then(|_| {
                // Si no se puede borrar, el modelo sí se creó: sólo se descarta el proyecto.
                if let Err(e) = inst.with_ui("borrar el target de prueba", |u| u.delete_target(TRIAL)) {
                    tracing::warn!(model = %m.internal_name, "no se pudo borrar el target de prueba: {e}");
                    self.discard(&inst);
                    *project = None;
                }
                Ok(())
            }),
        };
        let (outcome, detail) = match r {
            Ok(()) => (ProbeOutcome::Created, None),
            // Vijeo no ofrece el modelo, crea otro o lo rechaza con un aviso.
            Err(e @ (VijeoError::InvalidArgument { .. } | VijeoError::ModalDialog { .. })) => {
                if matches!(e, VijeoError::ModalDialog { .. }) {
                    self.drop_probe_project(project);
                }
                (ProbeOutcome::Rejected, Some(e.to_string()))
            }
            Err(e) => {
                self.drop_probe_project(project);
                (ProbeOutcome::Failed, Some(e.to_string()))
            }
        };
        ModelProbe { internal_name: m.internal_name.clone(), hmi_type: m.hmi_type.clone(), outcome, detail, duration_ms: t0.elapsed().as_millis() as u64 }
    }

    fn drop_probe_project(&self, project: &mut Option<Arc<Instance>>) {
        if let Some(inst) = project.take() {
            self.discard(&inst);
        }
    }

    /// Texto del `.vxml` del proyecto (Export Project). Se reutiliza mientras la copia de trabajo no cambie
    /// (las operaciones del MCP la guardan al terminar; lo que se edite a mano en la ventana no cuenta).
    fn vxml_text(&self, inst: &Instance) -> VijeoResult<Arc<String>> {
        inst.with_ui("Export Project", |u| {
            let stamp = std::fs::metadata(u.swx()).and_then(|m| m.modified()).ok();
            let mut cache = inst.export.lock().unwrap();
            if let Some((s, text)) = cache.as_ref() {
                if stamp.is_some() && *s == stamp {
                    return Ok(text.clone());
                }
            }
            let dir = inst.work.join("export");
            std::fs::create_dir_all(&dir).map_err(|e| VijeoError::io(&dir, e))?;
            let out = dir.join("project.vxml");
            u.export_xml(&out)?;
            let bytes = std::fs::read(&out).map_err(|e| VijeoError::io(&out, e))?;
            let text = Arc::new(reports::decode_text(&bytes));
            *cache = Some((stamp, text.clone()));
            Ok(text)
        })
    }

    /// Aplica `f` a cada target elegido del XML del proyecto.
    fn from_vxml<T>(&self, p: ProjectId, target: Option<&str>, f: impl Fn(roxmltree::Node) -> Vec<T>) -> VijeoResult<Vec<T>> {
        let inst = self.get(p)?;
        let text = self.vxml_text(&inst)?;
        let doc = vxml::parse(&text)?;
        Ok(vxml::select_targets(&doc, target)?.into_iter().flat_map(f).collect())
    }

    /// Carpetas temporales que quedaron de procesos que ya no existen: `M5DTemPHolder*` de Vijeo (las borra
    /// en su próximo arranque) y `MCP_TMP_<pid del MCP>_*` del MCP.
    fn orphan_temp_dirs(&self) -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(&self.workspace) else { return Vec::new() };
        rd.filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let pid = match (name.strip_prefix("M5DTemPHolder"), name.strip_prefix(vdz::TEMP_PREFIX)) {
                    (Some(hex), _) => u32::from_str_radix(hex, 16).ok()?,
                    (_, Some(rest)) => rest.split('_').next()?.parse().ok()?,
                    _ => return None,
                };
                (!process::is_alive(pid)).then(|| e.path())
            })
            .collect()
    }
}

impl Drop for Vd63Backend {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

impl Backend for Vd63Backend {
    fn info(&self) -> BackendInfo {
        BackendInfo {
            id: "vd63".into(),
            product_version: self.inst.product_version,
            support: self.support,
            frame_dir: self.inst.frame_dir.clone(),
            protected_dirs: self.protected.clone(),
        }
    }

    fn capabilities(&self) -> &CapabilitySet {
        &self.caps
    }

    fn status(&self) -> BackendStatus {
        let pool = self.pool.lock().unwrap();
        let mut instances: Vec<InstanceStatus> = pool
            .map
            .values()
            .map(|i| InstanceStatus {
                project: Some(i.id),
                project_name: Some(i.name()),
                pid: Some(i.pid),
                state: match i.ui.try_lock() {
                    Err(TryLockError::WouldBlock) => InstanceState::Busy,
                    Err(TryLockError::Poisoned(_)) => InstanceState::Crashed,
                    Ok(mut g) => match g.as_mut().map(UiProject::is_alive) {
                        Some(true) => InstanceState::Ready,
                        _ => InstanceState::Crashed,
                    },
                },
            })
            .collect();
        instances.extend((0..pool.starting).map(|_| InstanceStatus { project: None, project_name: None, pid: None, state: InstanceState::Starting }));
        BackendStatus { instances, max_instances: self.cfg.max_instances, orphan_temp_dirs: self.orphan_temp_dirs() }
    }

    fn projects(&self) -> Option<&dyn ProjectOps> {
        Some(self)
    }
    fn targets(&self) -> Option<&dyn TargetOps> {
        Some(self)
    }
    fn build(&self) -> Option<&dyn BuildOps> {
        Some(self)
    }
    fn reports(&self) -> Option<&dyn ReportOps> {
        Some(self)
    }
    fn variables(&self) -> Option<&dyn VariableOps> {
        Some(self)
    }
    fn catalog(&self) -> Option<&dyn CatalogOps> {
        Some(self)
    }
    fn config(&self) -> Option<&dyn ConfigOps> {
        Some(self)
    }
    fn files(&self) -> Option<&dyn FileOps> {
        Some(self)
    }
    fn panels(&self) -> Option<&dyn PanelOps> {
        Some(self)
    }

    fn shutdown(&self) -> VijeoResult<()> {
        let all: Vec<Arc<Instance>> = std::mem::take(&mut self.pool.lock().unwrap().map).into_values().collect();
        for inst in all {
            tracing::info!(project = %inst.id, "cerrando instancia (sin guardar)");
            if let Some(w) = inst.shutdown() {
                tracing::warn!("{w}");
            }
        }
        Ok(())
    }
}

impl ProjectOps for Vd63Backend {
    fn new_project(&self, name: &str, save_as: &Path, overwrite: bool, target: &str, model: &ModelSelector) -> VijeoResult<ProjectInfo> {
        self.require(Capability::ProjectLifecycle)?;
        self.require(Capability::TargetManage)?;
        validate::name("name", name)?;
        validate::name("target.name", target)?;
        let dest = validate::vdz_path("save_as", save_as, &self.protected)?;
        if dest.exists() && !overwrite {
            return Err(VijeoError::invalid("save_as", format!("{} ya existe (usa overwrite=true para reemplazarlo)", dest.display())));
        }
        if !dest.parent().is_some_and(Path::is_dir) {
            return Err(VijeoError::invalid("save_as", "la carpeta de destino no existe"));
        }
        let resolved = self.catalog_ref()?.resolve(model)?;
        let inst = self.launch_new(name, target, &resolved)?;
        let finish = (|| {
            inst.with_ui("project_save", |u| u.package(&dest, name))?;
            *inst.path.lock().unwrap() = Some(dest.clone());
            Ok(())
        })();
        if let Err(e) = finish {
            self.discard(&inst);
            return Err(e);
        }
        self.info_of(&inst)
    }

    fn open(&self, vdz: &Path) -> VijeoResult<ProjectInfo> {
        self.require(Capability::ProjectLifecycle)?;
        let path = validate::vdz_path("path", vdz, &self.protected)?;
        if !path.is_file() {
            return Err(VijeoError::invalid("path", format!("{} no existe", path.display())));
        }
        let name = swx::project_name(&path)?;
        let (frame, ws, src) = (self.inst.frame_dir.clone(), self.workspace.clone(), path.clone());
        let inst = self.launch(Some(path), name, |work, temp, t| UiProject::open(&frame, &ws, &src, work, temp, t))?;
        let mut info = self.info_of(&inst)?;
        info.notes = inst.with_ui("project_open", |u| Ok(u.notes().to_vec()))?;
        Ok(info)
    }

    fn save(&self, p: ProjectId, to: Option<&Path>, overwrite: bool) -> VijeoResult<ProjectInfo> {
        self.require(Capability::ProjectLifecycle)?;
        let inst = self.get(p)?;
        let current = inst.path();
        let dest = match to {
            Some(t) => {
                let d = validate::vdz_path("save_as", t, &self.protected)?;
                let same = current.as_deref().is_some_and(|c| c.to_string_lossy().eq_ignore_ascii_case(&d.to_string_lossy()));
                if d.exists() && !same && !overwrite {
                    return Err(VijeoError::invalid("save_as", format!("{} ya existe (usa overwrite=true para reemplazarlo)", d.display())));
                }
                d
            }
            None => current.ok_or_else(|| VijeoError::invalid("save_as", "el proyecto no tiene ruta todavía: indica save_as"))?,
        };
        // Revalida por si la ruta actual vino de otra configuración.
        validate::vdz_path("save_as", &dest, &self.protected)?;
        if !dest.parent().is_some_and(Path::is_dir) {
            return Err(VijeoError::invalid("save_as", format!("la carpeta de {} no existe", dest.display())));
        }
        let name = inst.name();
        inst.with_ui("project_save", |u| {
            // Lo que el usuario haya cambiado a mano en la ventana de Vijeo también se guarda.
            u.save()?;
            u.package(&dest, &name)
        })?;
        *inst.path.lock().unwrap() = Some(dest);
        inst.unsaved.store(false, Ordering::SeqCst);
        self.info_of(&inst)
    }

    fn close(&self, p: ProjectId, discard: bool) -> VijeoResult<()> {
        let inst = self.get(p)?;
        // Sólo cuenta lo que cambió el MCP: lo editado a mano en la ventana de Vijeo no se detecta.
        if !discard && inst.unsaved.load(Ordering::SeqCst) {
            return Err(VijeoError::Refused("el proyecto tiene cambios sin guardar: guárdalo con project_save o ciérralo con discard=true".into()));
        }
        self.discard(&inst);
        Ok(())
    }

    fn info(&self, p: ProjectId) -> VijeoResult<ProjectInfo> {
        let inst = self.get(p)?;
        self.info_of(&inst)
    }

    fn list(&self) -> Vec<ProjectSummary> {
        self.pool.lock().unwrap().map.values().map(|i| ProjectSummary { id: i.id, name: i.name(), path: i.path(), pid: Some(i.pid) }).collect()
    }

    fn rename(&self, p: ProjectId, name: &str) -> VijeoResult<ProjectInfo> {
        self.require(Capability::ProjectLifecycle)?;
        validate::name("name", name)?;
        let inst = self.get(p)?;
        // El nombre sólo existe en el .vdz (la entrada .SwxCF): se aplica al guardar.
        if inst.name() != name {
            *inst.name.lock().unwrap() = name.to_owned();
            inst.mark_unsaved();
        }
        self.info_of(&inst)
    }
}

impl TargetOps for Vd63Backend {
    fn list(&self, p: ProjectId) -> VijeoResult<Vec<TargetInfo>> {
        self.require(Capability::TargetManage)?;
        let inst = self.get(p)?;
        Ok(self.targets_of(&inst)?.iter().map(|t| self.target_info(t)).collect())
    }

    fn add(&self, p: ProjectId, name: &str, model: &ModelSelector) -> VijeoResult<TargetInfo> {
        self.require(Capability::TargetManage)?;
        validate::name("name", name)?;
        let resolved = self.catalog_ref()?.resolve(model)?;
        let inst = self.get(p)?;
        self.add_resolved(&inst, name, &resolved)
    }

    fn delete(&self, p: ProjectId, target: &str) -> VijeoResult<()> {
        self.require(Capability::TargetManage)?;
        let inst = self.get(p)?;
        let t = select_target(self.targets_of(&inst)?, Some(target))?.remove(0);
        inst.with_ui("Delete Target", |u| u.delete_target(&t.name))?;
        inst.mark_unsaved();
        Ok(())
    }

    fn rename(&self, p: ProjectId, target: &str, new_name: &str) -> VijeoResult<TargetInfo> {
        self.require(Capability::TargetManage)?;
        validate::name("new_name", new_name)?;
        let inst = self.get(p)?;
        let targets = self.targets_of(&inst)?;
        let old = select_target(targets.clone(), Some(target))?.remove(0);
        if old.name == new_name {
            return Ok(self.target_info(&old));
        }
        if targets.iter().any(|x| x.name != old.name && x.name.eq_ignore_ascii_case(new_name)) {
            return Err(VijeoError::invalid("new_name", format!("ya existe un target «{new_name}»")));
        }
        let got = inst.with_ui("Rename Target", |u| u.rename_target(&old.name, new_name))?;
        if got != new_name {
            return Err(VijeoError::invalid("new_name", format!("Vijeo dejó el nombre «{got}» en vez de «{new_name}»")));
        }
        inst.mark_unsaved();
        let t = select_target(self.targets_of(&inst)?, Some(new_name))?.remove(0);
        Ok(self.target_info(&t))
    }

    fn panels(&self, p: ProjectId, target: &str) -> VijeoResult<Vec<PanelRef>> {
        self.require(Capability::PanelList)?;
        self.from_vxml(p, Some(target), vxml::base_panel_refs)
    }

    fn open_panel(&self, p: ProjectId, target: &str, panel: &str) -> VijeoResult<PanelRef> {
        self.require(Capability::PanelOpen)?;
        let (inst, t, found) = self.resolve_panel(p, target, panel, "panel")?;
        let path = panel_path(&found);
        inst.with_ui("abrir el panel", |u| u.open_panel(&t, &path))?;
        Ok(found)
    }
}

/// Ruta de un panel base sin `BP:\` (`Carpeta\Panel`), como la usa el Navigator.
fn panel_path(r: &PanelRef) -> String {
    r.path.trim_start_matches("BP:\\").to_owned()
}

/// PanelID como texto (`P2` → `2`, `P10001` → `10001`) de un panel base («cambiar de panel») o de una ventana
/// emergente (abrir/cerrar popup), buscado por ruta (`Carpeta\Panel`; en los popups `Carpeta\Ventana\Panel`) o por
/// nombre si no hay otro igual. Un popup también se encuentra por el nombre de su ventana si ésta tiene un solo panel.
fn find_panel(ids: &[vxml::PanelId], kind: RefKind, want: &str) -> Result<String, String> {
    let (group, what) = match kind {
        RefKind::Base => ("Base Panels", "un panel base"),
        RefKind::Popup => ("Popup Windows", "un panel de ventana emergente"),
        RefKind::Script => return Err("find_panel no busca scripts".into()),
    };
    let w = want.trim();
    let w = if kind == RefKind::Base { w.trim_start_matches("BP:").trim_start_matches("bp:") } else { w }.trim_matches('\\');
    let all: Vec<&vxml::PanelId> = ids.iter().filter(|p| p.group == group).collect();
    let unique = |f: &dyn Fn(&vxml::PanelId) -> bool| {
        let m: Vec<&vxml::PanelId> = all.iter().copied().filter(|p| f(p)).collect();
        (m.len() == 1).then(|| m[0])
    };
    let hit = all
        .iter()
        .copied()
        .find(|p| p.path.eq_ignore_ascii_case(w))
        .or_else(|| unique(&|p| p.name.eq_ignore_ascii_case(w)))
        .or_else(|| unique(&|p| !p.window.is_empty() && p.window.eq_ignore_ascii_case(w)));
    match hit {
        Some(p) => p.id.strip_prefix('P').map(str::to_owned).ok_or_else(|| format!("id de panel inesperado «{}»", p.id)),
        None if all.is_empty() => Err(format!("«{want}» no es {what}: el target no tiene ninguna")),
        None => Err(format!("«{want}» no es {what} (hay: {})", all.iter().map(|p| p.path.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

/// Referencias de las acciones: paneles por su id ([`find_panel`]) y scripts por su Java ya compilado.
fn resolve_ref(ids: &[vxml::PanelId], compiled: &BTreeMap<String, String>, kind: RefKind, want: &str) -> Result<String, String> {
    match kind {
        RefKind::Script => compiled.get(want).cloned().ok_or_else(|| "script sin compilar".to_owned()),
        _ => find_panel(ids, kind, want),
    }
}

fn uses_change_panel(l: &[SwitchAction]) -> bool {
    build::uses_panels(l)
}

/// El portapapeles es uno para todo el sistema: una operación de objetos a la vez.
static CLIPBOARD: Mutex<()> = Mutex::new(());

fn clip_error(e: String) -> VijeoError {
    VijeoError::Internal(format!("no se entendió el contenido del portapapeles de Vijeo: {e}"))
}

/// Comprueba que existen `vars` entre las variables `known` del target (del XML). Las del sistema (`_CurPanelID`…)
/// no salen en el XML y los índices de matriz (`Var[3]`) no se comprueban.
fn check_variables(known: &[String], vars: &[String], target: &str) -> VijeoResult<()> {
    let exists = |v: &str| known.iter().any(|k| *k == v.split('[').next().unwrap_or(v));
    let missing: Vec<String> = vars
        .iter()
        .filter(|v| !v.starts_with('_') && !exists(v))
        .map(|v| match known.iter().find(|k| k.eq_ignore_ascii_case(v)) {
            Some(k) => format!("{v} (¿{k}?)"),
            None => v.to_string(),
        })
        .collect();
    if !missing.is_empty() {
        return Err(VijeoError::invalid("variables", format!("no existen en «{target}»: {} (variable_list las lista)", missing.join(", "))));
    }
    Ok(())
}

fn read_clip(raw: Option<Vec<u8>>) -> VijeoResult<Option<objects::Clip>> {
    raw.map(|b| objects::Clip::from_bytes(&b).map_err(clip_error)).transpose()
}

impl Vd63Backend {
    /// Proyecto, nombre real del target y panel base (buscado en el XML para dar un error claro y usar las
    /// mayúsculas reales). `field` es el argumento que da el panel, para el error.
    fn resolve_panel(&self, p: ProjectId, target: &str, panel: &str, field: &str) -> VijeoResult<(Arc<Instance>, String, PanelRef)> {
        let inst = self.get(p)?;
        let t = select_target(self.targets_of(&inst)?, Some(target))?.remove(0);
        let want = panel.trim().trim_start_matches("BP:").trim_start_matches("bp:").trim_matches('\\').to_lowercase();
        let panels = self.from_vxml(p, Some(&t.name), vxml::base_panel_refs)?;
        let found = panels.iter().find(|r| r.path.trim_start_matches("BP:\\").eq_ignore_ascii_case(&want)).or_else(|| {
            // Sólo el nombre, si no hay otro igual en otra carpeta.
            let by_name: Vec<&PanelRef> = panels.iter().filter(|r| r.path.rsplit('\\').next().is_some_and(|n| n.eq_ignore_ascii_case(&want))).collect();
            (by_name.len() == 1).then(|| by_name[0])
        });
        let Some(found) = found.cloned() else {
            let all: Vec<&str> = panels.iter().map(|r| r.path.as_str()).collect();
            return Err(VijeoError::invalid(field, format!("«{panel}» no es un panel base de «{}» (hay: {})", t.name, all.join(", "))));
        };
        Ok((inst, t.name, found))
    }

    /// Comprueba en el XML del proyecto que existen `vars` y devuelve los paneles (para «cambiar de panel») si
    /// `need_panels`. Sin nada que comprobar no exporta el XML.
    fn object_context(&self, p: ProjectId, target: &str, vars: &[String], need_panels: bool) -> VijeoResult<Vec<vxml::PanelId>> {
        // Las variables del sistema (`_CurPanelID`…) no salen en el XML.
        if vars.iter().any(|v| !v.starts_with('_')) {
            check_variables(&self.from_vxml(p, Some(target), vxml::variable_names)?, vars, target)?;
        }
        if need_panels {
            self.from_vxml(p, Some(target), vxml::panel_ids)
        } else {
            Ok(Vec::new())
        }
    }

    /// Compila con el compilador de Vijeo los scripts de acción de una operación (fuente normalizada → Java), con las
    /// variables del target sacadas del XML. Si alguno no compila, nada se toca y se devuelven sus errores.
    fn compile_scripts(&self, p: ProjectId, target: &str, sources: &[String]) -> VijeoResult<BTreeMap<String, String>> {
        let mut out = BTreeMap::new();
        if sources.is_empty() {
            return Ok(out);
        }
        let symbols = self.from_vxml(p, Some(target), vxml::script_symbols)?;
        let results = script::compile(&self.inst.frame_dir, &symbols, sources)?;
        let mut bad = Vec::new();
        for (i, (src, r)) in sources.iter().zip(results).enumerate() {
            match r.compiled {
                Some(c) if r.errors.is_empty() => {
                    out.insert(src.clone(), c);
                }
                _ => bad.push(format!("script {}: {}", i + 1, script::describe(&r.errors))),
            }
        }
        if !bad.is_empty() {
            return Err(VijeoError::invalid("script", format!("no compila con el compilador de Vijeo: {}", bad.join(" | "))));
        }
        Ok(out)
    }

    fn toolchest_libs(&self) -> VijeoResult<&[toolchest::Library]> {
        self.require(Capability::Toolchest)?;
        Ok(self.toolchest.get_or_init(|| toolchest::libraries(&self.inst.frame_dir)))
    }
}

/// Lo que Vijeo desplaza cada Paste respecto al anterior después de un Copy (verificado en vivo: +5, +10…;
/// tras un Cut, el primer Paste no se desplaza).
const PASTE_STEP: i32 = 5;

/// Piezas como mucho en una respuesta de toolchest_list.
const MAX_TOOLCHEST_PARTS: usize = 300;

impl PanelOps for Vd63Backend {
    fn objects(&self, p: ProjectId, target: &str, panel: &str) -> VijeoResult<PanelObjectsResult> {
        self.require(Capability::PanelObjects)?;
        let (inst, t, found) = self.resolve_panel(p, target, panel, "panel")?;
        let path = panel_path(&found);
        let _one = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
        let mut backup = vijeo_ui::clipboard::TextBackup::take();
        let raw = inst.with_ui("copiar los objetos del panel", |u| u.copy_objects(&t, &path, false));
        backup.restore();
        let objects = read_clip(raw?)?.map(|c| panel_ops::to_objects(&c)).unwrap_or_default();
        let mut notes = Vec::new();
        if objects.is_empty() {
            notes.push("el panel no tiene objetos".into());
        }
        Ok(PanelObjectsResult { target: t, panel: found.path, count: panel_ops::count(&objects), objects, notes })
    }

    fn edit(&self, req: &PanelEditRequest) -> VijeoResult<PanelChangeResult> {
        self.require(Capability::PanelObjects)?;
        if req.operations.is_empty() {
            return Err(VijeoError::invalid("operations", "no hay ninguna operación"));
        }
        let (inst, t, found) = self.resolve_panel(req.project, &req.target, &req.panel, "panel")?;
        let path = panel_path(&found);
        let need_panels = req.operations.iter().any(|op| match op {
            PanelEditOp::SetActions { on_touch, on_release, while_pressed, .. } => {
                [on_touch, on_release, while_pressed].into_iter().flatten().any(|l| uses_change_panel(l))
            }
            _ => false,
        });
        let ids = self.object_context(req.project, &t, &panel_ops::edit_variables(&req.operations), need_panels)?;
        let sources: Vec<String> = req
            .operations
            .iter()
            .flat_map(|op| match op {
                PanelEditOp::SetActions { on_touch, on_release, while_pressed, .. } => {
                    [on_touch, on_release, while_pressed].into_iter().flatten().flat_map(|l| build::scripts(l)).collect()
                }
                _ => Vec::new(),
            })
            .collect();
        let compiled = self.compile_scripts(req.project, &t, &sources)?;
        let lookup = |k: RefKind, p: &str| resolve_ref(&ids, &compiled, k, p);
        // Las propiedades de los smart objects sólo se sabe si son variables tras copiar el panel: se comprueban
        // entonces contra las variables del XML, leídas antes de tocar la interfaz.
        let known = if panel_ops::has_property_edits(&req.operations) { Some(self.from_vxml(req.project, Some(&t), vxml::variable_names)?) } else { None };
        let _one = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
        let mut backup = vijeo_ui::clipboard::TextBackup::take();
        let mut touched = false;
        let r = inst.with_ui("editar los objetos del panel", |u| {
            // 1. Copiar y aplicar las operaciones sin tocar el panel: si alguna no vale, no cambia nada.
            let mut check = read_clip(u.copy_objects(&t, &path, false)?)?.ok_or_else(|| VijeoError::invalid("panel", "el panel no tiene objetos"))?;
            if let Some(known) = &known {
                check_variables(known, &panel_ops::edit_property_variables(&check, &req.operations)?, &t)?;
            }
            let changes = panel_ops::apply(&mut check, &req.operations, &lookup)?;
            if req.dry_run {
                return Ok((false, changes, panel_ops::to_objects(&check), Vec::new()));
            }
            // 2. Cortar todo, aplicar las operaciones a lo cortado y pegarlo.
            touched = true;
            let cut = u.copy_objects(&t, &path, true)?.ok_or_else(|| VijeoError::Internal("Vijeo no cortó los objetos del panel".into()))?;
            let original = objects::Clip::from_bytes(&cut).map_err(clip_error)?;
            let mut edited = original.clone();
            if let Err(e) = panel_ops::apply(&mut edited, &req.operations, &lookup) {
                u.paste_objects(&cut)?;
                return Err(e);
            }
            let pasted = edited.to_cfb().map_err(clip_error).and_then(|b| u.paste_objects(&b));
            // 3. Comprobar lo que quedó; si el panel quedó vacío, se vuelve a pegar el original.
            let after = u.copy_objects(&t, &path, false)?;
            let Some(after) = read_clip(after)? else {
                u.paste_objects(&cut)?;
                return Err(pasted.err().unwrap_or_else(|| VijeoError::Internal("Vijeo no pegó los objetos editados: se volvió a pegar el contenido original del panel".into())));
            };
            pasted?;
            let warnings = panel_ops::differences(&edited.objects, &after.objects);
            u.save()?;
            Ok((true, changes, panel_ops::to_objects(&after), warnings))
        });
        if touched {
            inst.mark_unsaved();
        }
        backup.restore();
        let (applied, changes, objects, warnings) = r?;
        let mut notes = Vec::new();
        if applied {
            notes.push("Vijeo renumera los ObjectID de los objetos del panel al pegarlos".into());
            notes.push("el proyecto queda con cambios sin guardar en el .vdz: project_save para conservarlos".into());
        }
        Ok(PanelChangeResult { target: t, panel: found.path, applied, changes, objects, warnings, notes })
    }

    fn insert(&self, req: &PanelInsertRequest) -> VijeoResult<PanelChangeResult> {
        self.require(Capability::PanelObjects)?;
        if req.items.is_empty() {
            return Err(VijeoError::invalid("items", "no hay nada que insertar"));
        }
        let (inst, t, found) = self.resolve_panel(req.project, &req.target, &req.panel, "panel")?;
        let path = panel_path(&found);
        // Lo que no necesita Vijeo se resuelve antes: piezas de la Toolchest y rutas de los paneles de origen.
        enum Source {
            Part(String, objects::Clip),
            Panel(String, String),
            New,
        }
        let mut sources = Vec::new();
        let mut vars = Vec::new();
        let mut need_panels = false;
        for (i, item) in req.items.iter().enumerate() {
            let field = format!("items[{i}]");
            vars.extend(panel_ops::insert_variables(item));
            let s = match (&item.create, &item.toolchest, &item.from_panel, &item.object) {
                (Some(o), None, None, None) => {
                    if let NewObject::Switch { on_touch, on_release, while_pressed, .. } = o {
                        need_panels |= uses_change_panel(on_touch) || uses_change_panel(on_release) || uses_change_panel(while_pressed);
                    }
                    Source::New
                }
                (None, Some(id), None, None) => {
                    let (d, clip) = toolchest::find(self.toolchest_libs()?, id).map_err(|e| VijeoError::invalid(&field, e))?;
                    vars.extend(panel_ops::insert_property_variables(&clip.objects, &item.properties).map_err(|e| VijeoError::invalid(format!("{field}.properties"), e))?);
                    Source::Part(d.id, clip)
                }
                (None, None, Some(panel), Some(object)) => {
                    let (_, _, src) = self.resolve_panel(req.project, &t, panel, &format!("{field}.from_panel"))?;
                    Source::Panel(panel_path(&src), object.clone())
                }
                _ => return Err(VijeoError::invalid(&field, "indica `create`, `toolchest`, o `from_panel` y `object`")),
            };
            if item.create.is_none() && (item.width.is_some() || item.height.is_some()) {
                return Err(VijeoError::invalid(&field, "`width` y `height` sólo valen con `create`"));
            }
            if item.toolchest.is_none() && !item.properties.is_empty() {
                return Err(VijeoError::invalid(&field, "`properties` sólo vale con `toolchest` (para otro smart object, panel_edit con set_properties)"));
            }
            sources.push(s);
        }
        let ids = self.object_context(req.project, &t, &vars, need_panels)?;
        let scripts: Vec<String> = req
            .items
            .iter()
            .filter_map(|i| match &i.create {
                Some(NewObject::Switch { on_touch, on_release, while_pressed, .. }) => {
                    Some([on_touch, on_release, while_pressed].into_iter().flat_map(|l| build::scripts(l)).collect::<Vec<_>>())
                }
                _ => None,
            })
            .flatten()
            .collect();
        let compiled = self.compile_scripts(req.project, &t, &scripts)?;
        let lookup = |k: RefKind, p: &str| resolve_ref(&ids, &compiled, k, p);
        let _one = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
        let mut backup = vijeo_ui::clipboard::TextBackup::take();
        let mut touched = false;
        let r = inst.with_ui("insertar objetos en el panel", |u| {
            let mut slots: Vec<Option<objects::Clip>> = Vec::new();
            let mut changes: Vec<Option<String>> = Vec::new();
            let mut copied: Vec<(String, objects::Clip)> = Vec::new();
            for (i, (item, src)) in req.items.iter().zip(sources).enumerate() {
                let field = format!("items[{i}]");
                let (nodes, tags, what) = match src {
                    Source::New => {
                        slots.push(None);
                        changes.push(None);
                        continue;
                    }
                    Source::Part(id, clip) => (clip.objects, clip.tags, format!("pieza {id}")),
                    Source::Panel(src_path, object) => {
                        if !copied.iter().any(|(p, _)| *p == src_path) {
                            let c = read_clip(u.copy_objects(&t, &src_path, false)?)?.ok_or_else(|| VijeoError::invalid(format!("{field}.from_panel"), "el panel no tiene objetos"))?;
                            copied.push((src_path.clone(), c));
                        }
                        let c = &copied.iter().find(|(p, _)| *p == src_path).unwrap().1;
                        let at = c.resolve(&object).map_err(|e| VijeoError::invalid(format!("{field}.object"), e))?;
                        (vec![c.node(&at).clone()], c.tags.clone(), format!("«{object}» de {src_path}"))
                    }
                };
                let clip = panel_ops::prepare_insert(nodes, tags, item, &field)?;
                let names: Vec<String> = clip.objects.iter().map(|n| n.name()).collect();
                changes.push(Some(format!("{what} insertada como «{}» en ({}, {})", names.join(", "), item.x, item.y)));
                slots.push(Some(clip));
            }
            // Lo que ya hay en el panel (copiado al final: el panel de destino queda activo para pegar).
            let current = read_clip(u.copy_objects(&t, &path, false)?)?;
            let before = current.as_ref().map(|c| c.objects.len()).unwrap_or(0);
            let mut taken: Vec<String> = current.as_ref().map(|c| c.walk().into_iter().map(|(_, p)| p.rsplit('/').next().unwrap_or(&p).to_owned()).collect()).unwrap_or_default();
            for (i, item) in req.items.iter().enumerate() {
                if slots[i].is_none() {
                    let field = format!("items[{i}]");
                    let clip = panel_ops::create_insert(item, &field, &lookup, &mut taken)?;
                    let o = &clip.objects[0];
                    changes[i] = Some(format!("{} nuevo «{}» en ({}, {})", o.kind(), o.name(), item.x, item.y));
                    slots[i] = Some(clip);
                }
            }
            let prepared: Vec<objects::Clip> = slots.into_iter().flatten().collect();
            let changes: Vec<String> = changes.into_iter().flatten().collect();
            touched = true;
            // Tras un Copy, Vijeo desplaza cada Paste 5 px más que el anterior (+5, +10…): se compensa.
            for (i, clip) in prepared.iter().enumerate() {
                let k = PASTE_STEP * (i as i32 + 1);
                u.paste_objects(&panel_ops::shifted(clip, -k, -k)?.to_cfb().map_err(clip_error)?)?;
            }
            let want: Vec<objects::Node> = prepared.iter().flat_map(|c| c.objects.iter().cloned()).collect();
            let mut after = read_clip(u.copy_objects(&t, &path, false)?)?.ok_or_else(|| VijeoError::Internal("tras pegar, el panel no tiene objetos".into()))?;
            let mut notes = Vec::new();
            if after.objects.len() >= before && !panel_ops::differences(&want, &after.objects[before..]).is_empty() {
                // Si aún quedó algo fuera de sitio: cortar todo, recolocarlo y pegarlo de una vez (tras un Cut, el
                // primer Paste no se desplaza).
                let cut = u.copy_objects(&t, &path, true)?.ok_or_else(|| VijeoError::Internal("Vijeo no cortó los objetos del panel".into()))?;
                let mut fixed = objects::Clip::from_bytes(&cut).map_err(clip_error)?;
                if panel_ops::realign(&mut fixed, before, &want)? {
                    notes.push("Vijeo dejó desplazado lo pegado y se recolocó (cortando y pegando el panel)".to_string());
                    let pasted = fixed.to_cfb().map_err(clip_error).and_then(|b| u.paste_objects(&b));
                    match read_clip(u.copy_objects(&t, &path, false)?)? {
                        Some(c) => after = c,
                        None => {
                            u.paste_objects(&cut)?;
                            return Err(pasted.err().unwrap_or_else(|| VijeoError::Internal("Vijeo no pegó los objetos recolocados: se volvió a pegar lo que había".into())));
                        }
                    }
                    pasted?;
                } else {
                    u.paste_objects(&cut)?;
                    after = read_clip(u.copy_objects(&t, &path, false)?)?.unwrap_or(fixed);
                }
            }
            let warnings = if after.objects.len() >= before { panel_ops::differences(&want, &after.objects[before..]) } else { vec!["el panel tiene menos objetos que antes de pegar".into()] };
            u.save()?;
            Ok((changes, panel_ops::to_objects(&after), warnings, notes))
        });
        if touched {
            inst.mark_unsaved();
        }
        backup.restore();
        let (changes, objects, warnings, mut notes) = r?;
        if req.items.iter().any(|i| i.toolchest.is_some() && i.properties.is_empty()) {
            notes.push("las piezas de la Toolchest sin `properties` llegan como al arrastrarlas (los smart objects, con las variables vacías): panel_objects muestra sus propiedades y panel_edit set_properties las cambia".into());
        }
        if req.items.iter().any(|i| !i.properties.is_empty()) {
            notes.push("las variables de las propiedades se enlazan por nombre al pegar: project_build confirma que el smart object compila".into());
        }
        if req.items.iter().any(|i| i.create.is_some()) {
            notes.push("los objetos nuevos enlazan sus variables por nombre al pegarlos: project_build confirma que todo compila".into());
        }
        notes.push("el proyecto queda con cambios sin guardar en el .vdz: project_save para conservarlos".into());
        Ok(PanelChangeResult { target: t, panel: found.path, applied: true, changes, objects, warnings, notes })
    }

    fn toolchest(&self, f: &ToolchestFilter) -> VijeoResult<ToolchestResult> {
        let libs = self.toolchest_libs()?;
        let lib_f = f.library.as_deref().map(|s| s.trim().replace('/', "\\").to_lowercase()).filter(|s| !s.is_empty());
        let text_f = f.text.as_deref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
        let mut notes = Vec::new();
        if lib_f.is_none() && text_f.is_none() {
            notes.push("indica `library` o `text` para ver las piezas".into());
            return Ok(ToolchestResult { libraries: libs.iter().map(toolchest::summary).collect(), parts: Vec::new(), notes });
        }
        let (mut parts, mut legacy, mut panels, mut failed) = (Vec::new(), 0, 0, 0);
        for lib in libs.iter().filter(|l| lib_f.as_deref().is_none_or(|w| l.name.to_lowercase().contains(w))) {
            for (p, r) in toolchest::read_all(lib) {
                match r {
                    Ok(clip) => {
                        let d = toolchest::describe(lib, &p, &clip);
                        let hay = format!("{} {} {}", d.library, d.name, d.kinds.join(" ")).to_lowercase();
                        if text_f.as_deref().is_none_or(|w| hay.contains(w)) {
                            parts.push(d);
                        }
                    }
                    Err(e) if e == objects::LEGACY => legacy += 1,
                    Err(e) if e == objects::PANEL => panels += 1,
                    Err(e) => {
                        failed += 1;
                        tracing::warn!(library = %lib.name, part = %p, "pieza de la Toolchest no leída: {e}");
                    }
                }
            }
        }
        if legacy > 0 {
            notes.push(format!("{legacy} pieza(s) con un formato antiguo (Image Library y algunas plantillas) no se listan: no se pueden insertar"));
        }
        if panels > 0 {
            notes.push(format!("{panels} pieza(s) son paneles enteros: no se listan"));
        }
        if failed > 0 {
            notes.push(format!("{failed} pieza(s) no se pudieron leer"));
        }
        if parts.len() > MAX_TOOLCHEST_PARTS {
            notes.push(format!("hay {} piezas: se muestran las {MAX_TOOLCHEST_PARTS} primeras (afina con library o text)", parts.len()));
            parts.truncate(MAX_TOOLCHEST_PARTS);
        }
        Ok(ToolchestResult { libraries: Vec::new(), parts, notes })
    }
}

impl Vd63Backend {
    /// Controles de los diálogos que abre el comando de menú `cmd` con el nodo `node` del target seleccionado
    /// (diagnóstico para las pruebas en vivo; los diálogos se cancelan).
    pub fn probe_command(&self, p: ProjectId, target: &str, node: &str, cmd: u16) -> VijeoResult<Vec<String>> {
        self.get(p)?.with_ui("probar un comando", |u| u.probe_command(target, node, cmd))
    }

    /// Navigator bajo un target (`depth` niveles), para diagnosticar `open_panel` en las pruebas en vivo.
    pub fn navigator_outline(&self, p: ProjectId, target: &str, depth: usize) -> VijeoResult<Vec<String>> {
        self.get(p)?.with_ui("Navigator", |u| u.navigator_outline(target, depth))
    }

    /// Contenido del portapapeles de objetos (compound file) tras Select All + Copy en el panel `panel`, sin
    /// interpretar: diagnóstico para las pruebas en vivo y para ampliar el corpus. Guarda y devuelve el texto
    /// del portapapeles como `panel_objects`.
    #[doc(hidden)]
    pub fn copy_panel_raw(&self, p: ProjectId, target: &str, panel: &str) -> VijeoResult<Option<Vec<u8>>> {
        let (inst, t, found) = self.resolve_panel(p, target, panel, "panel")?;
        let path = panel_path(&found);
        let _one = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
        let mut backup = vijeo_ui::clipboard::TextBackup::take();
        let raw = inst.with_ui("copiar los objetos del panel", |u| u.copy_objects(&t, &path, false));
        backup.restore();
        raw
    }

    /// Variables y grupos de escaneo del target `target` del proyecto `p` (o del único que tenga).
    fn csv_context(&self, p: ProjectId, target: Option<&str>) -> VijeoResult<(Option<String>, Vec<VariableInfo>, Vec<String>)> {
        let inst = self.get(p)?;
        let text = self.vxml_text(&inst)?;
        let doc = vxml::parse(&text)?;
        let ts = vxml::targets(&doc);
        let t = match target {
            Some(n) => vxml::select_targets(&doc, Some(n))?.remove(0),
            None if ts.len() == 1 => ts[0],
            None => {
                let names: Vec<String> = ts.iter().map(|t| vxml::target_name(*t)).collect();
                return Err(VijeoError::invalid("target", format!("el proyecto tiene {} targets ({}): indica cuál", ts.len(), names.join(", "))));
            }
        };
        let name = vxml::target_name(t);
        let mut groups: Vec<String> = Vec::new();
        for c in select_target(self.read_project(&inst, "leer los equipos")?, Some(&name))? {
            for d in c.drivers {
                for e in d.equipment {
                    groups.push(e.name.clone());
                    groups.extend(e.scan_groups);
                }
            }
        }
        Ok((vxml::first_language(&doc), vxml::variables(t), groups))
    }
}

/// CSV de variables listo para escribir, con lo que se vio al compararlo con el proyecto.
struct GeneratedCsv {
    bytes: Vec<u8>,
    variables: usize,
    folders: usize,
    language: String,
    /// Variables que ya existen en el target.
    repeated: Vec<String>,
    warnings: Vec<String>,
    notes: Vec<String>,
}

impl Vd63Backend {
    /// Genera el CSV de `variables`; con `project` toma de él el idioma y avisa de nombres repetidos y de
    /// grupos de escaneo que no son de ningún equipo del target.
    fn generate_csv(
        &self,
        variables: &[VariableSpec],
        language: Option<&str>,
        encoding: CsvEncoding,
        project: Option<(ProjectId, Option<&str>)>,
    ) -> VijeoResult<GeneratedCsv> {
        let mut warnings = Vec::new();
        let mut notes = Vec::new();
        let context = match project {
            Some((p, target)) => Some(self.csv_context(p, target)?),
            None => None,
        };
        let language = match (language, context.as_ref().and_then(|c| c.0.clone())) {
            (Some(l), Some(pl)) if !l.trim().eq_ignore_ascii_case(&pl) => {
                warnings.push(format!("el primer idioma del proyecto es «{pl}» y se pidió «{}»", l.trim()));
                l.trim().to_owned()
            }
            (Some(l), _) => l.trim().to_owned(),
            (None, Some(pl)) => pl,
            (None, None) => {
                notes.push("sin project, la columna del idioma se llama «Idioma1 ID»: si el proyecto tiene otro idioma (p.ej. Español), indícalo en language".into());
                "Idioma1".into()
            }
        };
        let v = self.inst.product_version;
        let csv = varcsv::build(variables, &language, &format!("{}.{}.{}", v.major, v.minor, v.patch))?;
        let mut repeated = Vec::new();
        if let Some((_, existing, groups)) = &context {
            let names: std::collections::BTreeSet<String> = existing.iter().map(|v| v.name.to_lowercase()).collect();
            repeated = csv.rows.iter().filter(|r| names.contains(&r.name.to_lowercase())).map(|r| r.name.clone()).collect();
            let mut unknown: Vec<&str> = csv.rows.iter().filter_map(|r| r.scan_group.as_deref()).filter(|g| !groups.iter().any(|x| x.eq_ignore_ascii_case(g))).collect();
            unknown.sort();
            unknown.dedup();
            if !unknown.is_empty() {
                warnings.push(format!("grupos de escaneo que no son de ningún equipo del target (Vijeo pedirá a cuál asignarlos): {}", unknown.join(", ")));
            }
        }
        let (bytes, lossy) = varcsv::encode(&csv.text, encoding == CsvEncoding::Unicode);
        if lossy {
            warnings.push(format!(
                "algunos caracteres no existen en la página de códigos OEM {} y se escribieron como «?»: usa encoding=unicode",
                vijeo_ui::text::oem_code_page()
            ));
        }
        Ok(GeneratedCsv { bytes, variables: csv.rows.len(), folders: csv.folders.len(), language, repeated, warnings, notes })
    }
}

impl FileOps for Vd63Backend {
    fn validate_xml(&self, path: &Path, max_issues: usize) -> VijeoResult<XmlValidation> {
        self.require(Capability::XmlValidate)?;
        // Sólo se lee: puede estar en cualquier carpeta.
        if !path.is_absolute() || !path.is_file() {
            return Err(VijeoError::invalid("path", format!("{} no es la ruta absoluta de un fichero que exista", path.display())));
        }
        let mut r = xsd::validate(path, &self.inst.frame_dir.join(PUBLIC_XSD), max_issues.clamp(1, 1000))?;
        if !r.valid && r.fatal.is_none() {
            r.notes.push(
                "los .vxml que exporta Vijeo 6.3 tampoco validan del todo contra public.xsd (p.ej. Download sin DHCP/UseNAT/DNS, PopPanel sin Condition, \
                 atributos RTN/UNACK/ACK de las alarmas): compara con el XML exportado del mismo proyecto antes de dar un error por bueno"
                    .into(),
            );
        }
        Ok(r)
    }

    fn variables_csv(&self, req: &VariableCsvRequest) -> VijeoResult<VariableCsvResult> {
        self.require(Capability::VariableCsv)?;
        if !req.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("csv")) {
            return Err(VijeoError::invalid("path", "tiene que ser un .csv"));
        }
        let dest = validate::output_dir("path", &req.path, &self.protected)?;
        if dest.exists() && !req.overwrite {
            return Err(VijeoError::invalid("path", format!("{} ya existe (usa overwrite=true para reemplazarlo)", dest.display())));
        }
        if !dest.parent().is_some_and(Path::is_dir) {
            return Err(VijeoError::invalid("path", "la carpeta de destino no existe"));
        }
        let project = match req.project {
            Some(p) => Some((p, req.target.as_deref())),
            None if req.target.is_some() => return Err(VijeoError::invalid("target", "sólo con project")),
            None => None,
        };
        let g = self.generate_csv(&req.variables, req.language.as_deref(), req.encoding, project)?;
        let mut warnings = g.warnings;
        let mut notes = g.notes;
        if !g.repeated.is_empty() {
            warnings.push(format!(
                "{} variable(s) ya existen en el target y sólo se reemplazan si al importar eliges «Overwrite existing variables»: {}",
                g.repeated.len(),
                g.repeated.iter().take(20).cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        let tmp = dest.with_file_name(format!("{}.mcp-tmp", dest.file_name().unwrap_or_default().to_string_lossy()));
        std::fs::write(&tmp, &g.bytes).and_then(|_| std::fs::rename(&tmp, &dest)).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            VijeoError::io(&dest, e)
        })?;
        notes.push("impórtalo con variables_import (csv) o en Vijeo con clic derecho en Variables › Import Variables (tipo CSV, delimitador coma)".into());
        if req.encoding == CsvEncoding::Unicode {
            notes.push("el CSV Unicode (UTF-16 con BOM) no se ha probado a importar; el OEM es el formato que exporta Vijeo".into());
        }
        Ok(VariableCsvResult { path: dest, variables: g.variables, folders: g.folders, encoding: req.encoding, language: g.language, warnings, notes })
    }

    fn convert_data(&self, req: &DataConvertRequest) -> VijeoResult<DataConvertResult> {
        self.require(Capability::DataConvert)?;
        if !req.input.is_absolute() || !req.input.exists() {
            return Err(VijeoError::invalid("input", format!("{} no es una ruta absoluta que exista", req.input.display())));
        }
        let output = validate::output_dir("output", &req.output, &self.protected)?;
        if let Some(rp) = &req.recipe_dir {
            if !rp.is_absolute() || !rp.is_dir() {
                return Err(VijeoError::invalid("recipe_dir", format!("{} no es una carpeta que exista", rp.display())));
            }
            if same_path(rp, &output) {
                return Err(VijeoError::invalid("recipe_dir", "tiene que ser distinta de output (DataConverter valida una contra otra)"));
            }
        }
        std::fs::create_dir_all(&output).map_err(|e| VijeoError::io(&output, e))?;
        let req = DataConvertRequest { output, ..req.clone() };
        dataconv::run(&self.inst.frame_dir.join(dataconv::EXE), &req, self.cfg.build_timeout)
    }
}

impl Vd63Backend {
    fn build_open(&self, p: ProjectId, target: Option<&str>) -> VijeoResult<BuildResult> {
        self.require(Capability::Build)?;
        let inst = self.get(p)?;
        if let Some(t) = target {
            select_target(self.targets_of(&inst)?, Some(t))?;
        }
        let t0 = Instant::now();
        // Build All: los mensajes de un target se separan por su sección en la salida.
        let lines = inst.with_ui("Build All", |u| u.build())?;
        Ok(build_result(&lines, target, t0.elapsed().as_millis() as u64, Vec::new()))
    }
}

impl BuildOps for Vd63Backend {
    fn build_project(&self, p: ProjectId) -> VijeoResult<BuildResult> {
        self.build_open(p, None)
    }

    fn build_target(&self, p: ProjectId, target: &str) -> VijeoResult<BuildResult> {
        self.build_open(p, Some(target))
    }

    fn build_file(&self, vdz: &Path, target: Option<&str>) -> VijeoResult<BuildResult> {
        self.require(Capability::Build)?;
        // Se compila una copia: el original sólo se lee y puede estar en cualquier carpeta.
        check_vdz_input("path", vdz)?;
        if let Some(t) = target {
            select_target(swx::read_targets(&mut swx::open_vdz(vdz)?, None), Some(t))?;
        }
        let name = self.temp_name()?;
        let _slot = self.reserve("compilar un .vdz")?;
        let t0 = Instant::now();
        let b = temp_dir("build").and_then(|work| {
            let r = ui_project::build_copy(&self.inst.frame_dir, &self.workspace, vdz, &work, &name, self.timeouts());
            let _ = std::fs::remove_dir_all(&work);
            r
        })?;
        Ok(build_result(&b.lines, target, t0.elapsed().as_millis() as u64, b.notes))
    }

    fn feedback(&self, p: ProjectId) -> VijeoResult<FeedbackLog> {
        self.require(Capability::Build)?;
        self.get(p)?.with_ui("project_messages", |u| u.feedback())
    }
}

impl ConfigOps for Vd63Backend {
    fn project_config(&self, p: ProjectId, target: Option<&str>) -> VijeoResult<ProjectConfig> {
        self.require(Capability::ProjectConfig)?;
        let inst = self.get(p)?;
        let targets = self.read_project(&inst, "target_config")?;
        let source = inst.with_ui("target_config", |u| Ok(u.swx()))?;
        Ok(ProjectConfig { source, unsaved_changes: false, targets: select_target(targets, target)? })
    }

    fn file_config(&self, vdz: &Path, target: Option<&str>) -> VijeoResult<ProjectConfig> {
        self.require(Capability::ProjectConfig)?;
        // Sólo lectura y sin Vijeo: se admite cualquier .vdz, también fuera de las carpetas de trabajo.
        check_vdz_input("path", vdz)?;
        let mut cfb = swx::open_vdz(vdz)?;
        let mut targets = swx::read_targets(&mut cfb, None);
        for t in &mut targets {
            t.hmi_type = t.model.as_deref().map(|m| self.hmi_type(m, t.resolution.as_deref()));
        }
        Ok(ProjectConfig { source: vdz.to_path_buf(), unsaved_changes: false, targets: select_target(targets, target)? })
    }

    fn edit_file_config(
        &self,
        vdz: &Path,
        target: Option<&str>,
        changes: &TargetConfigChanges,
        save_as: Option<&Path>,
        overwrite: bool,
        dry_run: bool,
    ) -> VijeoResult<ConfigEditResult> {
        self.require(Capability::ConfigEdit)?;
        check_vdz_input("path", vdz)?;
        if changes.download.is_none() && changes.equipment.is_empty() {
            return Err(VijeoError::invalid("download/equipment", "no hay nada que cambiar"));
        }
        let dest = match save_as {
            Some(s) => {
                let d = validate::vdz_path("save_as", s, &self.protected)?;
                if d.exists() && !same_path(&d, vdz) && !overwrite {
                    return Err(VijeoError::invalid("save_as", format!("{} ya existe (usa overwrite=true para reemplazarlo)", d.display())));
                }
                d
            }
            None => validate::vdz_path("path", vdz, &self.protected)?,
        };
        if !dest.parent().is_some_and(Path::is_dir) {
            return Err(VijeoError::invalid("save_as", format!("la carpeta de {} no existe", dest.display())));
        }
        // Un proyecto abierto sobre el destino lo pisaría al guardar, y su ventana no vería el cambio.
        let mut notes = Vec::new();
        for i in self.pool.lock().unwrap().map.values() {
            let Some(p) = i.path() else { continue };
            if same_path(&p, &dest) {
                return Err(VijeoError::Refused(format!(
                    "{} está abierto como proyecto {}: guárdalo si hace falta, ciérralo con project_close y vuelve a abrirlo después",
                    dest.display(),
                    i.id
                )));
            }
            if same_path(&p, vdz) && i.unsaved.load(Ordering::SeqCst) {
                notes.push(format!("el proyecto {} (abierto sobre este .vdz) tiene cambios sin guardar que no están en el resultado", i.id));
            }
        }

        let mut cfb = swx::open_vdz(vdz)?;
        let targets = swx::read_targets(&mut cfb, None);
        let t = match target {
            Some(n) => select_target(targets.clone(), Some(n))?.remove(0),
            None if targets.len() == 1 => targets[0].clone(),
            None => {
                let names: Vec<&str> = targets.iter().map(|t| t.name.as_str()).collect();
                return Err(VijeoError::invalid("target", format!("el proyecto tiene {} targets ({}): indica cuál", targets.len(), names.join(", "))));
            }
        };
        // Los modelos con ETH2 (HMIG2U, HMIG3U, HMIG3X) están todos en el catálogo. Uno que no esté (el
        // HMISTO531_532 de los ejemplos de Machine Expert) se trata como de una sola Ethernet.
        let dual = t.model.as_deref().and_then(|m| self.catalog.as_ref()?.dual_ethernet(m)).unwrap_or(false);
        let edit = swx::edit_target(&mut cfb, &t, changes, dual)?;
        let with_hmi_type = |mut t: TargetConfig| {
            t.hmi_type = t.model.as_deref().map(|m| self.hmi_type(m, t.resolution.as_deref()));
            t
        };
        if edit.changes.is_empty() || dry_run {
            notes.push(if edit.changes.is_empty() { "ya tenía esos valores: no se escribió nada".into() } else { "dry_run: no se escribió nada".into() });
            return Ok(ConfigEditResult { path: dest, written: false, changes: edit.changes, target: with_hmi_type(edit.expected), notes });
        }

        // Se escribe al lado del destino y sólo se reemplaza si al releerlo todo es como se esperaba.
        let tmp = dest.with_file_name(format!("{}.mcp-tmp", dest.file_name().unwrap_or_default().to_string_lossy()));
        let written = (|| {
            vdz::replace_swx(vdz, &tmp, &swx::into_bytes(cfb)?)?;
            let after = swx::read_targets(&mut swx::open_vdz(&tmp)?, None);
            let mut want = targets.clone();
            if let Some(w) = want.iter_mut().find(|x| x.storage == t.storage) {
                *w = edit.expected.clone();
            }
            let json = |v: &[TargetConfig]| serde_json::to_value(v).unwrap_or_default();
            if json(&after) != json(&want) {
                return Err(VijeoError::Internal("al releer el fichero escrito la configuración no es la esperada: no se reemplazó el destino".into()));
            }
            std::fs::rename(&tmp, &dest).map_err(|e| VijeoError::io(&dest, e))
        })();
        if let Err(e) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        notes.push("compílalo con project_build (path) para comprobar que Vijeo lo acepta".into());
        Ok(ConfigEditResult { path: dest, written: true, changes: edit.changes, target: with_hmi_type(edit.expected), notes })
    }
}

/// Misma ruta, sin distinguir mayúsculas ni el tipo de barra.
fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\");
    norm(a).eq_ignore_ascii_case(&norm(b))
}

impl ReportOps for Vd63Backend {
    fn export_xml(&self, p: ProjectId, dir: &Path, overwrite: bool) -> VijeoResult<XmlExportResult> {
        self.require(Capability::Reports)?;
        let dir = validate::output_dir("dir", dir, &self.protected)?;
        std::fs::create_dir_all(&dir).map_err(|e| VijeoError::io(&dir, e))?;
        let inst = self.get(p)?;
        let name = inst.name();
        let out = dir.join(format!("{name}.vxml"));
        let images_dir = dir.join(&name);
        if (out.exists() || images_dir.exists()) && !overwrite {
            return Err(VijeoError::invalid("dir", format!("ya existe {} o la carpeta {} (usa overwrite=true)", out.display(), images_dir.display())));
        }
        let temp = inst.with_ui("Export Project", |u| {
            u.export_xml(&out)?;
            Ok(u.temp_name().to_owned())
        })?;
        // La copia de trabajo se llama como el proyecto temporal: el XML lleva el nombre del proyecto.
        let text = reports::decode_text(&std::fs::read(&out).map_err(|e| VijeoError::io(&out, e))?);
        let text = rename_exported_project(&text, &temp, &name);
        std::fs::write(&out, text.as_bytes()).map_err(|e| VijeoError::io(&out, e))?;
        let doc = vxml::parse(&text)?;
        let targets = vxml::targets(&doc);
        let images = std::fs::read_dir(&images_dir).map(|rd| rd.filter_map(Result::ok).filter(|e| e.path().is_file()).count()).unwrap_or(0);
        Ok(XmlExportResult {
            path: out,
            images_dir: images_dir.is_dir().then_some(images_dir),
            images,
            targets: targets.len(),
            panels: targets.iter().map(|t| vxml::base_panel_refs(*t).len()).sum(),
        })
    }

    fn variables(&self, p: ProjectId, target: Option<&str>) -> VijeoResult<Vec<VariableInfo>> {
        self.require(Capability::Reports)?;
        self.from_vxml(p, target, vxml::variables)
    }

    fn panels(&self, p: ProjectId, target: Option<&str>) -> VijeoResult<Vec<PanelReport>> {
        self.require(Capability::Reports)?;
        self.from_vxml(p, target, vxml::panels)
    }

    fn actions(&self, p: ProjectId, target: Option<&str>) -> VijeoResult<Vec<ActionInfo>> {
        self.require(Capability::Reports)?;
        self.from_vxml(p, target, vxml::global_actions)
    }

    fn cross_references(&self, p: ProjectId, target: Option<&str>) -> VijeoResult<Vec<CrossReference>> {
        self.require(Capability::CrossReferences)?;
        self.from_vxml(p, target, vxml::cross_references)
    }

    fn lint_scripts(&self, p: ProjectId, target: Option<&str>, script: Option<&str>) -> VijeoResult<Vec<ScriptLint>> {
        self.require(Capability::ScriptLint)?;
        let inst = self.get(p)?;
        let text = self.vxml_text(&inst)?;
        let doc = vxml::parse(&text)?;
        let targets = vxml::select_targets(&doc, target)?;
        if script.is_some() && targets.len() > 1 {
            return Err(VijeoError::invalid("target", "el proyecto tiene varios targets: indica contra cuál revisar el script"));
        }
        let mut out = Vec::new();
        for t in targets {
            let ctx = vxml::lint_context(t);
            let scripts = match script {
                Some(code) => vec![("script".to_owned(), None, code.to_owned())],
                None => vxml::scripts(t),
            };
            // Todos los scripts del target en una sola llamada al compilador de Vijeo (ScriptEngine.dll).
            let sources: Vec<String> = scripts.iter().map(|s| script::normalize(&s.2)).collect();
            let compiled = script::compile(&self.inst.frame_dir, &vxml::script_symbols(t), &sources);
            for (i, (location, script_type, code)) in scripts.into_iter().enumerate() {
                let mut issues = lint::lint(&code, &ctx);
                let (compiles, compiler_note) = match &compiled {
                    Ok(results) => {
                        let check = lint::merge_compiler(&issues, &results[i], &ctx);
                        issues.extend(check.issues);
                        issues.sort_by_key(|x| (x.line, x.column));
                        (check.compiles, check.note)
                    }
                    Err(e) => (None, Some(format!("no se pudo usar el compilador de Vijeo: {e}"))),
                };
                out.push(ScriptLint { target: vxml::target_name(t), location, script_type, lines: code.lines().count(), compiles, compiler_note, issues });
            }
        }
        Ok(out)
    }
}

impl VariableOps for Vd63Backend {
    fn lookup(&self, p: ProjectId, target: &str, name: &str) -> VijeoResult<VariableLookup> {
        self.require(Capability::VariableLookup)?;
        let found = self.from_vxml(p, Some(target), |t| vxml::variables(t).into_iter().filter(|v| v.name.eq_ignore_ascii_case(name.trim())).collect())?;
        let v = found.into_iter().next();
        Ok(VariableLookup { name: name.to_owned(), exists: v.is_some(), data_type: v.map(|v| v.data_type).filter(|t| !t.is_empty()) })
    }

    fn import_csv(&self, req: &VariableImportRequest) -> VijeoResult<VariableImportResult> {
        self.require(Capability::VariableImport)?;
        let inst = self.get(req.project)?;
        let targets = self.targets_of(&inst)?;
        let t = match req.target.as_deref() {
            Some(n) => select_target(targets, Some(n))?.remove(0),
            None if targets.len() == 1 => targets.into_iter().next().unwrap(),
            None => {
                let names: Vec<&str> = targets.iter().map(|t| t.name.as_str()).collect();
                return Err(VijeoError::invalid("target", format!("el proyecto tiene {} targets ({}): indica cuál", names.len(), names.join(", "))));
            }
        };
        let mut warnings = Vec::new();
        let mut notes = Vec::new();
        let (csv, expected) = match (&req.csv, &req.variables) {
            (Some(_), Some(_)) => return Err(VijeoError::invalid("csv", "indica csv o variables, no los dos")),
            (None, None) => return Err(VijeoError::invalid("variables", "falta csv o variables")),
            (Some(path), None) => {
                if req.language.is_some() {
                    return Err(VijeoError::invalid("language", "sólo con variables"));
                }
                if !path.is_absolute() || !path.is_file() || !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("csv")) {
                    return Err(VijeoError::invalid("csv", format!("{} no es la ruta absoluta de un .csv que exista", path.display())));
                }
                (path.clone(), None)
            }
            (None, Some(vars)) => {
                // El CSV OEM es el único formato probado a importar.
                let g = self.generate_csv(vars, req.language.as_deref(), CsvEncoding::Oem, Some((req.project, Some(&t.name))))?;
                warnings.extend(g.warnings);
                notes.extend(g.notes);
                if !g.repeated.is_empty() {
                    let what = if req.replace_existing { "se reemplazan" } else { "no se tocan (replace_existing=false)" };
                    warnings.push(format!(
                        "{} variable(s) ya existen en el target y {what}: {}",
                        g.repeated.len(),
                        g.repeated.iter().take(20).cloned().collect::<Vec<_>>().join(", ")
                    ));
                }
                let dir = inst.work.join("import");
                std::fs::create_dir_all(&dir).map_err(|e| VijeoError::io(&dir, e))?;
                let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or_default();
                let path = dir.join(format!("variables-{stamp}.csv"));
                std::fs::write(&path, &g.bytes).map_err(|e| VijeoError::io(&path, e))?;
                (path, Some(g.variables))
            }
        };
        let r = inst.with_ui("Import Variables", |u| u.import_variables(&t.name, &csv, req.replace_existing));
        // Aunque falle, Vijeo puede haber cambiado algo antes de pararse.
        inst.mark_unsaved();
        let feedback: Vec<String> = r?.lines().map(str::to_owned).collect();
        let imported = feedback.iter().find_map(|l| l.strip_suffix(" variables were imported.").or_else(|| l.strip_suffix(" variable was imported.")).and_then(|n| n.trim().parse().ok()));
        if let (Some(n), Some(want)) = (imported, expected) {
            if n != want {
                warnings.push(format!("se pidieron {want} variables y Vijeo dice haber importado {n}"));
            }
        }
        if imported.is_none() {
            warnings.push("Vijeo no dijo cuántas variables importó: compruébalo con variable_list".into());
        }
        notes.push("el proyecto queda con cambios sin guardar en el .vdz: project_save para conservarlos".into());
        Ok(VariableImportResult { target: t.name, csv, imported, feedback, warnings, notes })
    }
}

impl CatalogOps for Vd63Backend {
    fn families(&self) -> Vec<FamilyInfo> {
        self.catalog.as_ref().map(|c| c.families.clone()).unwrap_or_default()
    }

    fn models(&self, filter: &ModelFilter) -> Vec<ModelInfo> {
        self.catalog.as_ref().map(|c| c.filter(filter)).unwrap_or_default()
    }

    fn resolve(&self, selector: &ModelSelector) -> VijeoResult<ResolvedModel> {
        self.catalog_ref()?.resolve(selector)
    }

    fn identify(&self, hmi_type: &str) -> Option<ModelRef> {
        self.catalog.as_ref()?.identify(hmi_type)
    }

    fn drivers(&self, filter: &DriverFilter) -> Vec<DriverInfo> {
        drivers::filter(&self.drivers, filter)
    }

    fn probe_models(&self, filter: &ModelFilter, progress: &mut dyn FnMut(&ModelProbe)) -> VijeoResult<ModelProbeReport> {
        self.require(Capability::TargetManage)?;
        let cache = self.probe_cache.clone().ok_or_else(|| VijeoError::invalid("cache_dir", "no hay carpeta de caché (%LOCALAPPDATA% no está definida): usa --cache-dir"))?;
        let t0 = Instant::now();
        // `creatable` no cuenta: se prueban también los que la regla descarta por licencia.
        let models = self.catalog_ref()?.filter(&ModelFilter { creatable: None, ..filter.clone() });
        // El asistente sólo ofrece las familias activas en Targets.dat (las únicas con nombre de familia).
        let (todo, inactive): (Vec<ModelInfo>, Vec<ModelInfo>) = models.into_iter().partition(|m| m.family_name.is_some());
        let skipped = inactive.iter().map(|m| format!("{}: su familia está desactivada en Targets.dat", m.internal_name)).collect();
        let mut project = None;
        let mut probed = Vec::new();
        for m in &todo {
            let mut p = self.probe_one(&mut project, m);
            if p.outcome == ProbeOutcome::Failed {
                // Un fallo no dice nada del modelo: otra vez, ya con un proyecto nuevo.
                tracing::warn!(model = %m.internal_name, "la prueba falló ({}): se repite", p.detail.as_deref().unwrap_or(""));
                p = self.probe_one(&mut project, m);
            }
            progress(&p);
            probed.push(p);
            // Lo que va saliendo queda guardado aunque la prueba no termine.
            if let Err(e) = probe::store(&cache, &self.inst, &probed) {
                tracing::warn!("no se pudo guardar la caché de --probe-models: {e}");
            }
        }
        self.drop_probe_project(&mut project);
        probe::store(&cache, &self.inst, &probed)?;
        Ok(ModelProbeReport { cache, probed, skipped, duration_ms: t0.elapsed().as_millis() as u64 })
    }
}

/// Factoría del backend 6.3.
pub struct Vd63Factory;

impl BackendFactory for Vd63Factory {
    fn id(&self) -> &'static str {
        "vd63"
    }

    fn supports(&self, inst: &Installation) -> Support {
        let v = inst.product_version;
        match (v.major, v.minor) {
            (6, 3) => Support::Exact,
            (6, m) if m > 3 => Support::Compatible,
            _ => Support::No,
        }
    }

    fn create(&self, inst: &Installation, support: Support, cfg: &BackendConfig) -> VijeoResult<Arc<dyn Backend>> {
        Ok(Arc::new(Vd63Backend::new(inst, support, cfg)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_exported_project() {
        let xml = "<?xml version=\"1.0\"?>\n<Project Name=\"MCP_TMP_1_0\" Version=\"6.3\"><Target Name=\"MCP_TMP_1_0\"/></Project>";
        assert_eq!(
            rename_exported_project(xml, "MCP_TMP_1_0", "A&B"),
            "<?xml version=\"1.0\"?>\n<Project Name=\"A&amp;B\" Version=\"6.3\"><Target Name=\"MCP_TMP_1_0\"/></Project>"
        );
        assert_eq!(rename_exported_project("<Project Version=\"1\"><X Name=\"T\"/></Project>", "T", "N"), "<Project Version=\"1\"><X Name=\"T\"/></Project>");
    }

    #[test]
    fn display_resolutions() {
        assert_eq!(display_resolution("HMIDT351 (800x480)").as_deref(), Some("800x480"));
        assert_eq!(display_resolution("HMIDT351"), None);
    }
}
