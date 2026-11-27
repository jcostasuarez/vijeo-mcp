//! Proyecto abierto en una instancia normal de Vijeo-Frame, sin COM (inventario §12.10).
//!
//! Las instancias COM trabajan siempre en modo SoMachine (MSD, §12.9). Aquí Vijeo se lanza como
//! ejecutable y se maneja como lo haría el usuario: comandos de menú (`WM_COMMAND` al marco), los
//! diálogos de cada comando y el árbol del Navigator.
//!
//! El proyecto vive en `Vijeo-Manager\<nombre temporal>` (`MCP_TMP_*`): `/Import` desempaqueta ahí una
//! copia con ese nombre, y New Project lo crea ahí. `save` lo guarda en esa carpeta y `package`
//! empaqueta el `.vdz` con el nombre final (el nombre del proyecto es sólo el de la entrada `.SwxCF`).
//! La carpeta se borra al cerrar, pase lo que pase.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use vijeo_ui::clipboard;
use vijeo_ui::feedback::feedback_lines;
use vijeo_ui::tree::{RemoteTree, TreeItem};
use vijeo_ui::ui::{UiDialog, UiInstance};
use vijeo_core::{FeedbackLog, VijeoError, VijeoResult};

use crate::{feedback, objects, vdz};

/// Ids del recurso `MENU 1008` de `Main.ENU` (6.3.0.103).
const CMD_NEW_PROJECT: u16 = 0xC095;
const CMD_SAVE: u16 = 0xC098;
const CMD_CLOSE_PROJECT: u16 = 0xC09A;
const CMD_BUILD_ALL: u16 = 0xC09E;
const CMD_NEW_TARGET: u16 = 0xC418;
/// Variable › Import Variables… (también en el menú contextual del nodo Variables, `MENU 3550`).
const CMD_IMPORT_VARIABLES: u16 = 0xC419;
/// Export Project del menú Import/Export cuando `SupportsXMLImportExport` está activo (el `0xC0A7` del
/// recurso no abre nada).
const CMD_EXPORT_PROJECT: u16 = 0xC147;
/// Edición en el editor de paneles (`Main.ENU`, inventario §13.1). Los ids estándar de MFC no hacen nada.
const CMD_CUT: u16 = 0xC03C;
const CMD_COPY: u16 = 0xC03D;
const CMD_PASTE: u16 = 0xC03E;
const CMD_SELECT_ALL: u16 = 0xC043;
/// New Panel (recurso `Main.ENU`; `0xC066` hace lo mismo): con «Base Panels» seleccionado crea un panel base.
const CMD_NEW_PANEL: u16 = 0xC065;
/// «Insert» del Navigator (menús 4802/4803 de `Main.ENU`): New Driver con «IO Manager» seleccionado, New
/// Equipment con un driver seleccionado.
const CMD_INSERT: u16 = 0xC066;
/// Configuration… de un equipo (menú 4814).
const CMD_EQUIPMENT_CONFIG: u16 = 0xC424;
const IO_MANAGER_NODE: &str = "IO Manager";
/// Diálogo New Driver: combo de fabricantes, lista de drivers y lista de tipos de equipo (también en Equipment Selection).
const NEW_DRIVER_MANUFACTURER: i32 = 4817;
const NEW_DRIVER_LIST: i32 = 2584;
const EQUIPMENT_LIST: i32 = 4826;
/// Equipment Configuration de un equipo de red (Modbus TCP): IP (`SysIPAddress32`) y Unit ID.
const EQUIP_IP: i32 = 1000;
const EQUIP_UNIT_ID: i32 = 1002;
/// Lo que se espera a que termine un Copy/Cut/Paste antes de mirar si abrió un diálogo.
const EDIT_COMMAND_WAIT: Duration = Duration::from_secs(10);

/// Casilla «Do not show project conversion help topics» del aviso de actualizar de versión.
const CONVERSION_CHECKBOX: i32 = 10030;
/// Campo «Revision History Comment:» del diálogo que abre Save Project en los proyectos con historial de
/// revisiones (Edit 11216, Static 11217 y la pregunta «Do you wish to save the changes…?» en 11218; Yes/Cancel/No).
const REVISION_COMMENT: i32 = 11216;
const REVISION_LABEL: i32 = 11217;
/// Comentario que deja el MCP en el historial de revisiones al guardar.
const REVISION_TEXT: &str = "Guardado por Vijeo MCP";
const IDOK: i32 = 1;
const IDCANCEL: i32 = 2;
const IDYES: i32 = 6;
const IDNO: i32 = 7;

/// Diálogo de fichero (`Save As`/`Open`): nombre, tipo y botón principal.
const FILE_NAME: i32 = 1148;
const FILE_TYPE: i32 = 1136;
/// Controles propios del diálogo de fichero de Import Variables: delimitador (`, Comma` / `; Semicolon`) y
/// los radios «Do not overwrite existing variables» / «Overwrite existing variables».
const IMPORT_DELIMITER: i32 = 6092;
const IMPORT_KEEP_EXISTING: i32 = 6087;
const IMPORT_OVERWRITE: i32 = 6088;
/// Asistentes «Create New Project» / «Create New Target».
const WIZ_PROJECT_NAME: i32 = 7032;
const WIZ_TARGET_NAME: i32 = 7072;
const WIZ_TARGET_TYPE: i32 = 7061;
const WIZ_MODEL: i32 = 7062;
const WIZ_NEXT: i32 = 12324;
const WIZ_FINISH: i32 = 12325;
/// Árbol de la pestaña de proyecto del Navigator: raíz = proyecto, hijos = targets.
const NAVIGATOR_TREE: i32 = 2001;
/// Nodo de los paneles base bajo cada target (`<target>` › `Graphical Panels` › `Base Panels`).
const BASE_PANELS_NODE: [&str; 1] = ["Base Panels"];
/// Nodo del Navigator con las ventanas emergentes (`1: ComErrorCodes` › `10001: Panel1`).
const POPUP_WINDOWS_NODE: &str = "Popup Windows";
/// Lo que puede tardar el Navigator en llenarse tras abrir un proyecto.
const NAVIGATOR_FILL: Duration = Duration::from_secs(15);
/// Tras un Build All, Vijeo deja de atender el Navigator (Intro) e Import Variables en esa instancia
/// (verificado con el ejemplo Modbus; la causa no se conoce).
const AFTER_BUILD_HINT: &str = ". Esta instancia ya compiló el proyecto y, tras un Build All, Vijeo deja de atender el Navigator: \
guarda con project_save, ciérralo con project_close y vuelve a abrirlo";
/// Lo que puede tardar el editor en mostrar un panel tras abrirlo en el Navigator.
const OPEN_PANEL_TIMEOUT: Duration = Duration::from_secs(15);

const POLL: Duration = Duration::from_millis(500);
/// La salida tiene que quedarse quieta este tiempo, con el resumen ya escrito, para dar la compilación por acabada.
const SETTLE: Duration = Duration::from_secs(2);
/// Lo mismo si el último resumen no se reconoce como el de la compilación (otro idioma, o el de validación).
const UNRECOGNIZED_SETTLE: Duration = Duration::from_secs(20);
/// Un fichero que Vijeo escribe (guardar, exportar) está acabado cuando lleva este tiempo sin cambiar.
const FILE_SETTLE: Duration = Duration::from_secs(2);
/// Save Project sin cambios que guardar: tiempo tras el que se da por hecho aunque Vijeo no reescriba el fichero.
const UNCHANGED_SAVE: Duration = Duration::from_secs(8);
/// Vijeo arrancado sin proyecto: tiempo que la ventana tiene que estar quieta antes de abrir New Project.
const STARTUP_SETTLE: Duration = Duration::from_secs(5);
/// Para cerrar el proyecto y salir de Vijeo.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(20);
const OTHER_DIALOG_GRACE: Duration = Duration::from_secs(30);
/// Lo que puede tardar un diálogo en aparecer tras su comando.
const DIALOG_TIMEOUT: Duration = Duration::from_secs(30);
/// Export Project prepara el proyecto antes de abrir el diálogo de fichero: en los grandes tarda más.
const EXPORT_DIALOG_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// Hasta que el proyecto queda abierto (incluye actualizar la versión de la copia).
    pub open: Duration,
    pub build: Duration,
    /// Guardar, exportar y los asistentes.
    pub call: Duration,
    /// Tiempo que un diálogo desconocido puede seguir abierto antes de abortar.
    pub dialog_grace: Duration,
}

/// Qué hacer con un diálogo.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    /// Pulsar el botón y apuntar la nota.
    Click(i32, String),
    /// Escribir el texto en el campo (id) y luego pulsar el botón y apuntar la nota.
    Fill(i32, &'static str, i32, String),
    /// Sin botones: progreso o pantalla de inicio.
    Wait,
    Unknown,
}

fn sorted_buttons(d: &UiDialog) -> Vec<i32> {
    let mut b = d.button_ids();
    b.sort();
    b
}

fn classify(d: &UiDialog) -> Answer {
    let buttons = d.button_ids();
    if buttons.is_empty() {
        return Answer::Wait;
    }
    // Los asistentes y el diálogo de fichero sólo los abre el MCP, que los está rellenando: mientras
    // cambian de página o se cierran no son un aviso (tienen estáticos con id 0xFFFF, como un MessageBox).
    let wizard = d.has_control(WIZ_NEXT) && d.has_control(WIZ_FINISH);
    let file = d.has_control(FILE_NAME) && d.has_control(FILE_TYPE);
    // Un progreso con «Cancel» («Building All» en proyectos grandes como PROYC, más de 30 s): se espera.
    let progress = d.controls.iter().any(|c| c.class.eq_ignore_ascii_case("msctls_progress32"));
    if wizard || file || progress {
        return Answer::Wait;
    }
    // «Opening the project will update it to the current Vijeo Designer version…» (Yes/No/Help + casilla).
    if d.has_control(CONVERSION_CHECKBOX) && buttons.contains(&IDYES) {
        return Answer::Click(IDYES, "Vijeo actualizó la copia a la versión actual (el original no cambia hasta guardar)".into());
    }
    // Save Project con historial de revisiones: pide un comentario antes de guardar. Yes = guardar con él.
    if d.has_control(REVISION_COMMENT) && d.has_control(REVISION_LABEL) && buttons.contains(&IDYES) {
        return Answer::Fill(
            REVISION_COMMENT,
            REVISION_TEXT,
            IDYES,
            format!("el proyecto tiene activado el historial de revisiones: cada guardado añade una entrada con el comentario «{REVISION_TEXT}»"),
        );
    }
    let b = sorted_buttons(d);
    // «The project was created with a Target Type that is not supported…»: No = convertir a un modelo equivalente.
    if d.info.message_box && b == [IDYES, IDNO] && d.info.text.contains("Target Type") {
        return Answer::Click(IDNO, "un target tenía un modelo que esta versión no soporta y Vijeo lo convirtió a un modelo equivalente (como al abrirlo en la interfaz)".into());
    }
    // «You already have 3 instance(s) of Vijeo-Frame running. You may experience slower performance…» [OK].
    // El id de su único botón no es fijo (un MessageBox de sólo OK puede usar IDCANCEL): se pulsa ése.
    if d.info.message_box && b.len() == 1 && d.info.text.contains("instance(s) of Vijeo-Frame") {
        return Answer::Click(b[0], "había otras 3 o más instancias de Vijeo abiertas (Vijeo avisa de que puede ir más lento)".into());
    }
    Answer::Unknown
}

fn alive(ui: &mut UiInstance, op: &str) -> VijeoResult<()> {
    if ui.is_alive() {
        Ok(())
    } else {
        Err(VijeoError::ServerCrashed { op: op.into() })
    }
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

pub struct UiProject {
    ui: UiInstance,
    temp: String,
    workspace: PathBuf,
    t: Timeouts,
    unknown_since: Option<Instant>,
    /// Lo que respondió el MCP en los diálogos conocidos, sin repetir.
    notes: Vec<String>,
    /// Ya se hizo un Build All en esta instancia (ver [`AFTER_BUILD_HINT`]).
    built: bool,
}

impl UiProject {
    /// Abre una copia de `vdz` con el nombre temporal `temp` (que no debe existir en `workspace`).
    pub fn open(frame_dir: &Path, workspace: &Path, vdz: &Path, work: &Path, temp: &str, t: Timeouts) -> VijeoResult<Self> {
        let copy = work.join(format!("{temp}.vdz"));
        vdz::copy_with_project_name(vdz, &copy, temp)?;
        let ui = UiInstance::launch(&frame_dir.join("Vijeo-Frame.exe"), [std::ffi::OsStr::new("/Import"), copy.as_os_str()])?;
        let mut p = Self::new(ui, workspace, temp, t);
        p.wait_open()?;
        // La carpeta tiene ya el proyecto (convertido si hacía falta): se guarda para leer de ahí.
        p.save()?;
        let _ = std::fs::remove_file(&copy);
        Ok(p)
    }

    /// Proyecto nuevo `temp` con un target (asistente New Project).
    pub fn create(frame_dir: &Path, workspace: &Path, temp: &str, target: &NewTarget, t: Timeouts) -> VijeoResult<Self> {
        let ui = UiInstance::launch(&frame_dir.join("Vijeo-Frame.exe"), std::iter::empty::<&str>())?;
        let mut p = Self::new(ui, workspace, temp, t);
        // Con el asistente abierto nada más aparecer la ventana, Finish no hace nada (verificado): Vijeo
        // sigue arrancando. Se espera a que la ventana lleve un rato quieta y sin diálogos.
        let mut since: Option<Instant> = None;
        p.wait_until("arrancar Vijeo", t.open, |p| {
            if p.ui.title().is_none() || !p.ui.dialogs().is_empty() {
                since = None;
                return None;
            }
            (since.get_or_insert_with(Instant::now).elapsed() >= STARTUP_SETTLE).then_some(())
        })?;
        p.command(CMD_NEW_PROJECT)?;
        let d = p.wait_dialog("New Project", |d| d.has_control(WIZ_PROJECT_NAME))?;
        if !d.enter_text(WIZ_PROJECT_NAME, temp) || !d.press(WIZ_NEXT) {
            return Err(VijeoError::Internal("no se pudo rellenar el nombre en el asistente New Project".into()));
        }
        p.fill_target_page(target)?;
        p.wait_open()?;
        p.save()?;
        Ok(p)
    }

    fn new(ui: UiInstance, workspace: &Path, temp: &str, t: Timeouts) -> Self {
        Self { ui, temp: temp.to_owned(), workspace: workspace.to_path_buf(), t, unknown_since: None, notes: Vec::new(), built: false }
    }

    pub fn pid(&self) -> u32 {
        self.ui.pid()
    }

    pub fn is_alive(&mut self) -> bool {
        self.ui.is_alive()
    }

    /// Notas de los diálogos que se respondieron (actualizar la versión, convertir un modelo…).
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Nombre del proyecto dentro de Vijeo (`MCP_TMP_*`).
    pub fn temp_name(&self) -> &str {
        &self.temp
    }

    pub fn folder(&self) -> PathBuf {
        self.workspace.join(&self.temp)
    }

    /// `.SwxCF` del proyecto tal como quedó en el último `save`.
    pub fn swx(&self) -> PathBuf {
        self.folder().join(format!("{}.SwxCF", self.temp))
    }

    fn command(&self, id: u16) -> VijeoResult<()> {
        if self.ui.command(id) {
            Ok(())
        } else {
            Err(VijeoError::Internal("no se encontró la ventana principal de Vijeo".into()))
        }
    }

    /// Responde los diálogos conocidos; un desconocido que siga abierto más de `dialog_grace` es un error.
    /// `expected` son los que espera quien llama: no se tocan.
    fn handle_dialogs(&mut self, expected: &dyn Fn(&UiDialog) -> bool) -> VijeoResult<()> {
        let mut unknown = None;
        for d in self.ui.dialogs() {
            if expected(&d) {
                continue;
            }
            match classify(&d) {
                Answer::Click(id, note) => {
                    tracing::info!(title = %d.info.title, button = id, "diálogo de Vijeo respondido");
                    if d.click(id) && !self.notes.contains(&note) {
                        self.notes.push(note);
                    }
                }
                Answer::Fill(field, text, id, note) => {
                    tracing::info!(title = %d.info.title, field, button = id, "diálogo de Vijeo rellenado y respondido");
                    if d.enter_text(field, text) && d.click(id) && !self.notes.contains(&note) {
                        self.notes.push(note);
                    }
                }
                Answer::Wait => {}
                Answer::Unknown => unknown = Some(d),
            }
        }
        match unknown {
            None => self.unknown_since = None,
            Some(d) => {
                let since = *self.unknown_since.get_or_insert_with(Instant::now);
                // Un diálogo con botones que no es un MessageBox puede ser un progreso con «Cancelar».
                let grace = if d.info.message_box { self.t.dialog_grace } else { self.t.dialog_grace.max(OTHER_DIALOG_GRACE) };
                if since.elapsed() >= grace {
                    self.unknown_since = None;
                    if d.info.message_box {
                        // Se cierra para que la instancia siga usable; el error lleva el texto.
                        let _ = d.click(if d.button_ids().contains(&IDOK) { IDOK } else { IDCANCEL });
                    }
                    return Err(VijeoError::ModalDialog { title: d.info.title, text: format!("{} [{}]", d.info.text, d.info.buttons.join("/")) });
                }
            }
        }
        Ok(())
    }

    /// Espera a que `f` devuelva algo, respondiendo los diálogos conocidos mientras tanto.
    fn wait_until<R>(&mut self, op: &str, timeout: Duration, mut f: impl FnMut(&mut Self) -> Option<R>) -> VijeoResult<R> {
        let start = Instant::now();
        loop {
            alive(&mut self.ui, op)?;
            if let Some(r) = f(self) {
                return Ok(r);
            }
            self.handle_dialogs(&|_| false)?;
            if start.elapsed() >= timeout {
                return Err(VijeoError::Timeout { op: format!("{op} (Vijeo)"), ms: timeout.as_millis() as u64 });
            }
            std::thread::sleep(POLL);
        }
    }

    /// Espera el diálogo que reconoce `is`, sin tocarlo, y lo devuelve.
    fn wait_dialog(&mut self, op: &str, is: impl Fn(&UiDialog) -> bool) -> VijeoResult<UiDialog> {
        self.wait_dialog_for(op, DIALOG_TIMEOUT, is)
    }

    fn wait_dialog_for(&mut self, op: &str, timeout: Duration, is: impl Fn(&UiDialog) -> bool) -> VijeoResult<UiDialog> {
        let start = Instant::now();
        loop {
            alive(&mut self.ui, op)?;
            if self.ui.dialogs().iter().any(&is) {
                // Los controles se terminan de crear justo después de que el diálogo se haga visible.
                std::thread::sleep(Duration::from_millis(300));
                return self.ui.dialogs().into_iter().find(|d| is(d)).ok_or_else(|| VijeoError::Internal(format!("el diálogo de {op} se cerró solo")));
            }
            self.handle_dialogs(&is)?;
            if start.elapsed() >= timeout {
                return Err(VijeoError::Timeout { op: format!("{op}: Vijeo no abrió el diálogo"), ms: timeout.as_millis() as u64 });
            }
            std::thread::sleep(POLL);
        }
    }

    /// Hasta que el título del marco es el del proyecto y no queda ningún diálogo con botones.
    fn wait_open(&mut self) -> VijeoResult<()> {
        let prefix = format!("{} - ", self.temp);
        self.wait_until("abrir el proyecto", self.t.open, |p| {
            (p.ui.title().is_some_and(|s| s.starts_with(&prefix)) && p.ui.dialogs().iter().all(|d| d.button_ids().is_empty())).then_some(())
        })
    }

    /// Save Project: Vijeo reescribe `<temp>.SwxCF` en la carpeta del proyecto. Termina cuando el fichero
    /// lleva [`FILE_SETTLE`] sin cambiar y no hay diálogos; si no había nada que guardar puede que Vijeo no
    /// lo reescriba, y basta con que pase [`UNCHANGED_SAVE`]. El `*` del título no sirve para saberlo: con
    /// Compact_Hardwired aparece justo después de guardar y con el ejemplo Modbus desaparece (verificado).
    pub fn save(&mut self) -> VijeoResult<()> {
        let f = self.swx();
        let before = mtime(&f);
        let start = Instant::now();
        self.command(CMD_SAVE)?;
        let mut last: Option<(Option<SystemTime>, Instant)> = None;
        self.wait_until("guardar el proyecto", self.t.call, |p| {
            let now = mtime(&f);
            match last {
                Some((m, since)) if m == now => {
                    let written = now.is_some() && (now != before || start.elapsed() >= UNCHANGED_SAVE);
                    (written && since.elapsed() >= FILE_SETTLE && p.ui.dialogs().is_empty()).then_some(())
                }
                _ => {
                    last = Some((now, Instant::now()));
                    None
                }
            }
        })
    }

    /// Empaqueta lo guardado como `dest`, con el proyecto llamado `name`, sin dejar un `.vdz` a medias.
    pub fn package(&self, dest: &Path, name: &str) -> VijeoResult<()> {
        let dir = dest.parent().ok_or_else(|| VijeoError::invalid("save_as", "ruta sin carpeta"))?;
        let stem = dest.file_stem().and_then(|s| s.to_str()).unwrap_or("project");
        let tmp = dir.join(format!("{stem}.mcp-tmp-{}.vdz", std::process::id()));
        let r = vdz::package_vdz(&self.folder(), &self.temp, name, &tmp).and_then(|_| std::fs::rename(&tmp, dest).map_err(|e| VijeoError::io(dest, e)));
        if r.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        r
    }

    /// Build All. Devuelve las líneas de la Feedback Zone tal cual (con los marcadores de tipo).
    pub fn build(&mut self) -> VijeoResult<Vec<String>> {
        let pid = self.pid();
        let before = feedback_lines(pid).unwrap_or_default();
        self.command(CMD_BUILD_ALL)?;
        self.built = true;
        let mut last: Option<(Vec<String>, Instant)> = None;
        // Vijeo vacía la salida al empezar: recompilar sin cambios acaba con las mismas líneas que antes.
        let mut started = false;
        self.wait_until("Build All", self.t.build, |p| {
            let lines = feedback_lines(pid).unwrap_or_default();
            started |= lines != before;
            let busy = !p.ui.dialogs().is_empty();
            // Tras el resumen de validación sigue la generación de código, que puede pasar un rato sin escribir.
            let settle = match feedback::summary(&lines) {
                Some(s) if s.build => Some(SETTLE),
                Some(_) => Some(UNRECOGNIZED_SETTLE),
                None => None,
            };
            match &last {
                Some((prev, since)) if *prev == lines => (started && !busy && settle.is_some_and(|s| since.elapsed() >= s)).then(|| lines.clone()),
                _ => {
                    last = Some((lines, Instant::now()));
                    None
                }
            }
        })
    }

    pub fn feedback(&self) -> VijeoResult<FeedbackLog> {
        let pid = self.pid();
        let lines = feedback_lines(pid).ok_or_else(|| VijeoError::Internal(format!("no se pudo leer la ventana de salida (Feedback Zone) de Vijeo-Frame (PID {pid})")))?;
        Ok(feedback::parse(&lines))
    }

    /// Export Project a `out` (`.vxml`, XML público). Vijeo deja al lado una carpeta `<nombre>` con las
    /// imágenes. Necesita `SupportsXMLImportExport` en el registro: sin él sólo ofrece `.zdat`.
    pub fn export_xml(&mut self, out: &Path) -> VijeoResult<()> {
        if out.exists() {
            std::fs::remove_file(out).map_err(|e| VijeoError::io(out, e))?;
        }
        self.command(CMD_EXPORT_PROJECT)?;
        let is_save = |d: &UiDialog| d.has_control(FILE_TYPE) && d.has_control(FILE_NAME);
        let t0 = Instant::now();
        let d = self.wait_dialog_for("Export Project", EXPORT_DIALOG_TIMEOUT, is_save)?;
        tracing::info!(ms = t0.elapsed().as_millis() as u64, "diálogo de Export Project abierto");
        let types = d.combo_items(FILE_TYPE);
        let Some(i) = types.iter().position(|t| t.to_ascii_lowercase().contains("*.vxml")) else {
            let _ = d.press(IDCANCEL);
            return Err(VijeoError::Unsupported {
                capability: vijeo_core::Capability::Reports,
                version: "6.3".into(),
                reason: format!(
                    "Export Project sólo ofrece {types:?}: activa HKLM\\SOFTWARE\\WOW6432Node\\Schneider Electric\\Vijeo-Designer63\\Vijeo-Frame\\Options\\SupportsXMLImportExport = 1"
                ),
            });
        };
        // El tipo cuenta: Vijeo escribe `.vxml` sólo si el filtro elegido es el primero (FUN_004ae510).
        if !d.select(FILE_TYPE, i) || !d.set_text(FILE_NAME, "Edit", &out.to_string_lossy()) || !d.press(IDOK) {
            let _ = d.press(IDCANCEL);
            return Err(VijeoError::Internal("no se pudo rellenar el diálogo Export Project".into()));
        }
        let mut last: Option<(Option<SystemTime>, Instant)> = None;
        self.wait_until("Export Project", self.t.build, |p| {
            let now = mtime(out);
            match last {
                Some((m, since)) if m == now => (now.is_some() && since.elapsed() >= FILE_SETTLE && p.ui.dialogs().is_empty()).then_some(()),
                _ => {
                    last = Some((now, Instant::now()));
                    None
                }
            }
        })
    }

    /// Rellena la página de target del asistente (New Project / New Target) y pulsa Finish.
    fn fill_target_page(&mut self, target: &NewTarget) -> VijeoResult<()> {
        let page = |d: &UiDialog| d.has_control(WIZ_TARGET_TYPE) && d.has_control(WIZ_MODEL);
        // Las páginas del asistente existen todas desde el principio: la de target es la que tiene el combo visible.
        let d = self.wait_dialog("asistente de target", |d| page(d) && !d.combo_items(WIZ_TARGET_TYPE).is_empty())?;
        let fail = |d: &UiDialog, why: String| {
            let _ = d.press(IDCANCEL);
            Err(VijeoError::invalid("model", why))
        };
        let types = d.combo_items(WIZ_TARGET_TYPE);
        let Some(ti) = find_family(&types, &target.family) else {
            return fail(&d, format!("el asistente de Vijeo no ofrece la familia «{}» (ofrece: {})", target.family, types.join(", ")));
        };
        if !d.select(WIZ_TARGET_TYPE, ti) {
            return fail(&d, "no se pudo elegir la familia en el asistente".into());
        }
        std::thread::sleep(Duration::from_millis(500));
        let models = d.combo_items(WIZ_MODEL);
        let Some(mi) = find_item(&models, &target.model) else {
            return fail(&d, format!("el asistente de Vijeo no ofrece el modelo «{}» en {} (ofrece: {})", target.model, target.family, models.join(", ")));
        };
        if !d.select(WIZ_MODEL, mi) {
            return fail(&d, "no se pudo elegir el modelo en el asistente".into());
        }
        std::thread::sleep(Duration::from_millis(500));
        if let Some(display) = &target.display {
            // La pantalla de los modulares es otro combo de la misma página.
            let Some((id, di)) = d
                .controls
                .iter()
                .filter(|c| c.class.eq_ignore_ascii_case("ComboBox") && ![WIZ_TARGET_TYPE, WIZ_MODEL].contains(&c.id))
                .find_map(|c| d.combo_items(c.id).iter().position(|x| x.eq_ignore_ascii_case(display)).map(|i| (c.id, i)))
            else {
                return fail(&d, format!("el asistente de Vijeo no ofrece la pantalla «{display}» para {}", target.model));
            };
            if !d.select(id, di) {
                return fail(&d, "no se pudo elegir la pantalla en el asistente".into());
            }
        }
        if !d.enter_text(WIZ_TARGET_NAME, &target.name) || !d.press(WIZ_FINISH) {
            return fail(&d, "no se pudo completar el asistente".into());
        }
        self.wait_until("asistente de target", self.t.call, |p| p.ui.dialogs().iter().all(|d| !page(d)).then_some(()))
    }

    /// New Target con el asistente. El nombre lo puede ajustar Vijeo: quien llama comprueba el resultado.
    pub fn add_target(&mut self, target: &NewTarget) -> VijeoResult<()> {
        self.command(CMD_NEW_TARGET)?;
        self.fill_target_page(target)?;
        self.save()
    }

    fn tree(&self) -> VijeoResult<RemoteTree> {
        RemoteTree::find(self.pid(), NAVIGATOR_TREE).ok_or_else(|| VijeoError::Internal("no se encontró el árbol del Navigator de Vijeo".into()))
    }

    /// Targets en el orden del Navigator. Recién abierto un proyecto, el árbol tarda un poco en llenarse.
    pub fn target_items(&self) -> VijeoResult<Vec<(TreeItem, String)>> {
        let tree = self.tree()?;
        let start = Instant::now();
        let root = loop {
            if let Some(r) = tree.root() {
                break r;
            }
            if start.elapsed() >= NAVIGATOR_FILL {
                return Err(VijeoError::Internal("el Navigator de Vijeo está vacío".into()));
            }
            std::thread::sleep(POLL);
        };
        Ok(tree.children(root).into_iter().filter_map(|i| tree.text(i).map(|t| (i, t))).collect())
    }

    fn target_item(&self, name: &str) -> VijeoResult<TreeItem> {
        self.target_items()?.into_iter().find(|(_, t)| t.eq_ignore_ascii_case(name)).map(|(i, _)| i).ok_or_else(|| VijeoError::TargetNotFound(name.to_owned()))
    }

    /// Textos del Navigator bajo el target, `depth` niveles, sangrados (para diagnosticar `open_panel`).
    pub fn navigator_outline(&self, target: &str, depth: usize) -> VijeoResult<Vec<String>> {
        let mut out: Vec<String> = RemoteTree::describe_all(self.pid()).into_iter().map(|(id, visible, roots)| format!("árbol {id} visible={visible}: {roots:?}")).collect();
        let tree = self.tree()?;
        let start = self.target_item(target).map_err(|e| VijeoError::Internal(format!("{e}; {}", out.join(" | "))))?;
        let mut stack = vec![(start, 0usize)];
        while let Some((item, level)) = stack.pop() {
            out.push(format!("{}{}", "  ".repeat(level), tree.text(item).unwrap_or_default()));
            if level < depth {
                tree.expand(item);
                stack.extend(tree.children(item).into_iter().rev().map(|c| (c, level + 1)));
            }
        }
        Ok(out)
    }

    /// Selecciona el nodo `node` (hijo directo del target) en el Navigator: los comandos del menú Variable
    /// actúan sobre el target seleccionado.
    fn select_target_node(&self, target: &str, node: &str) -> VijeoResult<()> {
        let tree = self.tree()?;
        let titem = self.target_item(target)?;
        tree.expand(titem);
        let item = tree
            .children(titem)
            .into_iter()
            .find(|&c| tree.text(c).is_some_and(|t| t.eq_ignore_ascii_case(node)))
            .ok_or_else(|| VijeoError::Internal(format!("no hay nodo «{node}» bajo «{target}» en el Navigator")))?;
        if !tree.focus(item) {
            return Err(VijeoError::Internal(format!("no se pudo seleccionar «{target} › {node}» en el Navigator")));
        }
        std::thread::sleep(Duration::from_millis(500));
        Ok(())
    }

    /// Import Variables (`0xC419`) de un CSV delimitado por comas en el target y guarda. Devuelve lo que
    /// Vijeo escribió en la ventana de salida (`14 variables were imported.`, `Import complete.`).
    ///
    /// Tras un Build All en la misma instancia Vijeo deja de atender el comando (y el Intro del Navigator):
    /// el diálogo no se abre y esto acaba en `timeout` (verificado; la causa no se conoce).
    pub fn import_variables(&mut self, target: &str, csv: &Path, overwrite: bool) -> VijeoResult<String> {
        self.select_target_node(target, "Variables")?;
        self.command(CMD_IMPORT_VARIABLES)?;
        let is_import = |d: &UiDialog| d.has_control(FILE_NAME) && d.has_control(IMPORT_DELIMITER);
        let built = self.built;
        let d = self.wait_dialog("Import Variables", is_import).map_err(|e| match e {
            VijeoError::Timeout { op, ms } if built => VijeoError::Timeout { op: format!("{op}{AFTER_BUILD_HINT}"), ms },
            e => e,
        })?;
        let types = d.combo_items(FILE_TYPE);
        let csv_type = types.iter().position(|t| t.to_ascii_lowercase().contains("*.csv"));
        let delimiter = d.combo_items(IMPORT_DELIMITER).iter().position(|t| t.contains("Comma"));
        let ok = match (csv_type, delimiter) {
            (Some(t), Some(c)) => {
                d.select(FILE_TYPE, t)
                    && d.select(IMPORT_DELIMITER, c)
                    && d.click(if overwrite { IMPORT_OVERWRITE } else { IMPORT_KEEP_EXISTING })
                    && d.set_text(FILE_NAME, "Edit", &csv.to_string_lossy())
            }
            _ => false,
        };
        if !ok || !d.press(IDOK) {
            let _ = d.press(IDCANCEL);
            return Err(VijeoError::Internal(format!("no se pudo rellenar el diálogo Import Variables (tipos: {types:?})")));
        }
        // La ayuda habla de un aviso final, pero 6.3 sólo escribe en la ventana de salida: `Importing variables
        // from <csv>...`, `<n> variables were imported.` e `Import complete.`, o `Import operation halted. …`
        // (y entonces no importa nada).
        let pid = self.pid();
        let file = csv.to_string_lossy().to_lowercase();
        let report = move || -> Option<Vec<String>> {
            let lines = feedback_lines(pid)?;
            let start = lines.iter().rposition(|l| l.to_lowercase().contains("importing variables from") && l.to_lowercase().contains(&file))?;
            let tail: Vec<String> = lines[start..].iter().map(|l| l.trim().to_owned()).collect();
            tail.iter().any(|l| l.contains("Import complete") || l.contains("halted")).then_some(tail)
        };
        let lines = self.wait_until("Import Variables", self.t.call, |_| report())?;
        self.wait_until("Import Variables", self.t.call, |p| p.ui.dialogs().is_empty().then_some(()))?;
        if let Some(why) = lines.iter().find(|l| l.contains("halted")) {
            return Err(VijeoError::invalid("csv", format!("Vijeo no importó nada: {why}")));
        }
        self.save()?;
        Ok(lines.join("\n"))
    }

    /// Diagnóstico: selecciona el nodo `node` (hijo directo del target) en el Navigator, manda el comando
    /// `cmd` y devuelve los controles de cada diálogo que se abra (id, clase, texto, y los elementos de los
    /// combos). Cierra los diálogos con Cancel.
    pub fn probe_command(&mut self, target: &str, node: &str, cmd: u16) -> VijeoResult<Vec<String>> {
        let mut out: Vec<String> = Vec::new();
        self.select_target_node(target, node)?;
        self.command(cmd)?;
        let start = Instant::now();
        while self.ui.dialogs().is_empty() && start.elapsed() < DIALOG_TIMEOUT {
            std::thread::sleep(POLL);
        }
        std::thread::sleep(Duration::from_millis(800));
        for d in self.ui.dialogs() {
            out.push(format!("diálogo «{}» message_box={} texto={:?}", d.info.title, d.info.message_box, d.info.text));
            for c in &d.controls {
                let items = if c.class.eq_ignore_ascii_case("ComboBox") { format!(" {:?}", d.combo_items(c.id)) } else { String::new() };
                out.push(format!("  {} {} {:?}{items}", c.id, c.class, c.text));
            }
            let _ = d.click(IDCANCEL);
        }
        // Que el diálogo cancelado acabe de cerrarse antes de lo siguiente.
        let _ = self.wait_until("cerrar el diálogo", self.t.call, |p| p.ui.dialogs().is_empty().then_some(()));
        Ok(out)
    }

    /// Crea un panel base en el target: New Panel (`0xC065`) con el nodo «Base Panels» seleccionado en el Navigator.
    /// Vijeo lo crea al momento, sin diálogo, como `<n>: Panel<n>` (n = el siguiente PanelID libre) y lo abre en el
    /// editor (verificado en vivo). Con `name`, se renombra editando su etiqueta; si Vijeo no acepta el nombre, el
    /// panel recién creado se borra y se devuelve el error con su texto. Devuelve (PanelID, nombre que queda).
    pub fn new_panel(&mut self, target: &str, name: Option<&str>) -> VijeoResult<(u32, String)> {
        let tree = self.tree()?;
        let titem = self.target_item(target)?;
        let bp = self.base_panels_item(&tree, titem).ok_or_else(|| VijeoError::Internal(format!("no se encontró el nodo de paneles base de «{target}» en el Navigator")))?;
        tree.expand(bp);
        let before: Vec<TreeItem> = tree.children(bp);
        if !tree.focus(bp) {
            return Err(VijeoError::Internal(format!("no se pudo seleccionar «{target} › Base Panels» en el Navigator")));
        }
        std::thread::sleep(Duration::from_millis(300));
        let built = self.built;
        self.sync_command("crear el panel", CMD_NEW_PANEL)?;
        let (item, label) = self
            .wait_until("crear el panel", self.t.call, |p| {
                let tree = p.tree().ok()?;
                tree.expand(bp);
                tree.children(bp).into_iter().find(|c| !before.contains(c)).and_then(|c| tree.text(c).map(|t| (c, t)))
            })
            .map_err(|e| match e {
                VijeoError::Timeout { op, ms } if built => VijeoError::Timeout { op: format!("{op}{AFTER_BUILD_HINT}"), ms },
                e => e,
            })?;
        let (id, created) = parse_panel_label(&label).ok_or_else(|| VijeoError::Internal(format!("etiqueta de panel no reconocida en el Navigator: «{label}»")))?;
        let mut got = created.clone();
        if let Some(want) = name.filter(|n| *n != created) {
            let tree = self.tree()?;
            let renamed = tree.rename(item, want);
            std::thread::sleep(Duration::from_millis(800));
            let refused = self.ui.dialogs().into_iter().find(|d| d.info.message_box);
            let why = match refused {
                Some(d) => {
                    let text = d.info.text.clone();
                    let _ = d.click(if d.button_ids().contains(&IDOK) { IDOK } else { IDCANCEL });
                    Some(format!("Vijeo no aceptó «{want}»: {text}"))
                }
                None if !renamed => Some(format!("no se pudo editar el nombre de «{created}» en el Navigator")),
                None => None,
            };
            got = tree.text(item).and_then(|l| parse_panel_label(&l)).map(|(_, n)| n).unwrap_or_default();
            if let Some(why) = why.or_else(|| (got != want).then(|| format!("Vijeo dejó el nombre «{got}» en vez de «{want}»"))) {
                let undone = self.delete_panel_item(item, &got);
                let tail = match undone {
                    Ok(()) => format!("; se borró el panel «{created}» que se había creado"),
                    Err(e) => format!("; el panel quedó creado como «{got}» y no se pudo borrar: {e}"),
                };
                self.save()?;
                return Err(VijeoError::invalid("name", format!("{why}{tail}")));
            }
        }
        self.save()?;
        Ok((id, got))
    }

    /// Nodo del Navigator de una pantalla: «Base Panels» o «Popup Windows» › carpetas › `<n>: <nombre>`.
    fn screen_item(&self, target: &str, popup: bool, path: &str) -> VijeoResult<TreeItem> {
        let tree = self.tree()?;
        let titem = self.target_item(target)?;
        let group = if popup { POPUP_WINDOWS_NODE } else { BASE_PANELS_NODE[0] };
        let mut item = self.group_item(&tree, titem, group).ok_or_else(|| VijeoError::Internal(format!("no se encontró «{group}» de «{target}» en el Navigator")))?;
        for seg in path.split('\\') {
            tree.expand(item);
            item = tree
                .children(item)
                .into_iter()
                .find(|&c| tree.text(c).is_some_and(|t| navigator_label_is(&t, seg)))
                .ok_or_else(|| VijeoError::Internal(format!("no se encontró «{seg}» de «{path}» en el Navigator")))?;
        }
        Ok(item)
    }

    /// Renombra un panel base o una ventana emergente editando su etiqueta en el Navigator (F2, como
    /// [`Self::new_panel`]); un aviso de Vijeo es un error con su texto. El número (`21: `) no cambia. Guarda.
    pub fn rename_screen(&mut self, target: &str, popup: bool, path: &str, new: &str) -> VijeoResult<String> {
        let item = self.screen_item(target, popup, path).map_err(|e| self.after_build(e))?;
        let tree = self.tree()?;
        let ok = tree.rename(item, new);
        std::thread::sleep(Duration::from_millis(800));
        if let Some(d) = self.ui.dialogs().into_iter().find(|d| d.info.message_box) {
            let text = d.info.text.clone();
            let _ = d.click(if d.button_ids().contains(&IDOK) { IDOK } else { IDCANCEL });
            return Err(VijeoError::invalid("new_name", format!("Vijeo no aceptó «{new}»: {text}")));
        }
        let got = tree.text(item).and_then(|l| parse_panel_label(&l)).map(|(_, n)| n).unwrap_or_default();
        if !ok || got != new {
            let hint = if self.built { AFTER_BUILD_HINT } else { "" };
            return Err(VijeoError::Internal(format!("Vijeo dejó el nombre «{got}» en vez de «{new}»{hint}")));
        }
        self.save()?;
        Ok(got)
    }

    /// Borra un panel base o una ventana emergente (con sus paneles): Supr en el Navigator y Yes en «Delete
    /// Panel» / «Delete Popup Window» (`You are about to delete [21 : RFID_1]`). Guarda.
    pub fn delete_screen(&mut self, target: &str, popup: bool, path: &str) -> VijeoResult<()> {
        let item = self.screen_item(target, popup, path).map_err(|e| self.after_build(e))?;
        let name = path.rsplit('\\').next().unwrap_or(path).to_owned();
        self.delete_panel_item(item, &name).map_err(|e| self.after_build(e))?;
        self.save()
    }

    /// Añade a un timeout la pista de que, tras un Build All, Vijeo deja de atender el Navigator.
    fn after_build(&self, e: VijeoError) -> VijeoError {
        match e {
            VijeoError::Timeout { op, ms } if self.built => VijeoError::Timeout { op: format!("{op}{AFTER_BUILD_HINT}"), ms },
            VijeoError::Internal(m) if self.built && !m.contains(AFTER_BUILD_HINT) => VijeoError::Internal(format!("{m}{AFTER_BUILD_HINT}")),
            e => e,
        }
    }

    /// Borra un panel del Navigator: Supr y Yes en la confirmación que nombra el panel.
    fn delete_panel_item(&mut self, item: TreeItem, name: &str) -> VijeoResult<()> {
        let tree = self.tree()?;
        if !tree.press_delete(item) {
            return Err(VijeoError::Internal(format!("no se pudo seleccionar «{name}» en el Navigator")));
        }
        let lower = name.to_lowercase();
        let confirm = move |d: &UiDialog| sorted_buttons(d) == [IDYES, IDNO] && d.info.text.to_lowercase().contains(&lower);
        let d = self.wait_dialog("borrar el panel", confirm)?;
        d.click(IDYES);
        // Un aviso tras el Yes (Vijeo no lo borra) es un error con su texto.
        let r = self.wait_until("borrar el panel", self.t.call, |p| {
            let warning = p.ui.dialogs().into_iter().find(|d| d.info.message_box && !d.info.text.contains("You are about to delete"));
            if let Some(d) = warning {
                let text = d.info.text.clone();
                let _ = d.click(if d.button_ids().contains(&IDOK) { IDOK } else { IDCANCEL });
                return Some(Err(text));
            }
            p.tree().ok().filter(|t| t.text(item).is_none()).map(|_| Ok(()))
        })?;
        r.map_err(|text| VijeoError::invalid("panel", format!("Vijeo no borró «{name}»: {text}")))
    }

    /// Cancela los diálogos abiertos (diagnóstico).
    pub fn cancel_dialogs(&mut self) {
        for d in self.ui.dialogs() {
            let _ = d.click(IDCANCEL);
        }
        let _ = self.wait_until("cerrar el diálogo", self.t.call, |p| p.ui.dialogs().is_empty().then_some(()));
    }

    /// Diagnóstico: selecciona el nodo `path` bajo el target (cada etiqueta empieza por el segmento, para pasar por
    /// alto ` [ 172.20.10.167 ]`), manda `cmd` y describe los diálogos (combos y listas incluidos); los cancela.
    pub fn probe_node_command(&mut self, target: &str, path: &[&str], cmd: u16) -> VijeoResult<Vec<String>> {
        let tree = self.tree()?;
        let mut item = self.target_item(target)?;
        for seg in path {
            tree.expand(item);
            let s = seg.to_lowercase();
            item = tree.children(item).into_iter().find(|&c| tree.text(c).is_some_and(|t| t.to_lowercase().starts_with(&s))).ok_or_else(|| VijeoError::Internal(format!("no hay «{seg}» en {path:?}")))?;
        }
        let mut out = vec![format!("seleccionado {:?} focus={}", tree.text(item), tree.focus(item))];
        std::thread::sleep(Duration::from_millis(500));
        self.command(cmd)?;
        let start = Instant::now();
        while self.ui.dialogs().is_empty() && start.elapsed() < Duration::from_secs(8) {
            std::thread::sleep(POLL);
        }
        std::thread::sleep(Duration::from_millis(800));
        for d in self.ui.dialogs() {
            out.push(format!("diálogo «{}» message_box={} texto={:?}", d.info.title, d.info.message_box, d.info.text));
            for c in &d.controls {
                let items = if c.class.eq_ignore_ascii_case("ComboBox") {
                    format!(" {:?}", d.combo_items(c.id))
                } else if c.class.eq_ignore_ascii_case("ListBox") {
                    format!(" {:?}", d.list_items(c.id))
                } else if c.class.eq_ignore_ascii_case("Edit") {
                    format!(" = {:?}", d.text_of(c.id, "Edit"))
                } else {
                    String::new()
                };
                out.push(format!("  {} {} {:?}{items}", c.id, c.class, c.text));
            }
            let _ = d.click(IDCANCEL);
        }
        let _ = self.wait_until("cerrar el diálogo", self.t.call, |p| p.ui.dialogs().is_empty().then_some(()));
        Ok(out)
    }

    /// Añade un equipo al target (verificado en vivo, ejemplo Modbus):
    /// - a un driver que ya tiene (`existing_driver`): «Insert» (`0xC066`) con el driver seleccionado abre
    ///   «Equipment Selection» (lista 4826);
    /// - si no, con un driver nuevo: «Insert» con «IO Manager» seleccionado abre «New Driver» (fabricante 4817,
    ///   drivers 2584, equipos 4826; los drivers que ya tiene el target no salen) y luego «Driver Configuration»,
    ///   que se acepta con sus valores por defecto. Vijeo crea `<Driver>01` con su primer equipo `<Equipo>01`.
    ///
    /// Luego renombra driver y equipo editando su etiqueta y, con `ip`/`unit_id`, los pone en «Equipment
    /// Configuration» (`0xC424`: IP 1000, Unit ID 1002, como Modbus TCP). Si algo falla después de crear, borra lo
    /// creado (el driver nuevo o el equipo) y devuelve el error. Devuelve (driver, equipo) como quedan.
    pub fn add_equipment(&mut self, target: &str, spec: &NewEquipment) -> VijeoResult<(String, String)> {
        let tree = self.tree()?;
        let titem = self.target_item(target)?;
        tree.expand(titem);
        let io = tree
            .children(titem)
            .into_iter()
            .find(|&c| tree.text(c).is_some_and(|t| t.eq_ignore_ascii_case(IO_MANAGER_NODE)))
            .ok_or_else(|| VijeoError::Internal(format!("no se encontró «{IO_MANAGER_NODE}» bajo «{target}» en el Navigator")))?;
        tree.expand(io);
        let built = self.built;
        let hint = move |e: VijeoError| match e {
            VijeoError::Timeout { op, ms } if built => VijeoError::Timeout { op: format!("{op}{AFTER_BUILD_HINT}"), ms },
            e => e,
        };
        // Vijeo puede abrir él mismo «Equipment Configuration» del equipo nuevo (verificado al añadir un equipo a
        // ModbusTCP): se rellena ahí. Un error de esa configuración se devuelve después de encontrar los nodos, para
        // poder borrar lo creado.
        let mut configured = false;
        let pending: Option<VijeoError>;
        let (drv, eq, created_driver) = match &spec.existing_driver {
            Some(name) => {
                let d = tree
                    .children(io)
                    .into_iter()
                    .find(|&c| tree.text(c).is_some_and(|t| node_name(&t).eq_ignore_ascii_case(name)))
                    .ok_or_else(|| VijeoError::Internal(format!("no se encontró el driver «{name}» en el Navigator")))?;
                tree.expand(d);
                let before = tree.children(d);
                if !tree.focus(d) {
                    return Err(VijeoError::Internal(format!("no se pudo seleccionar el driver «{name}» en el Navigator")));
                }
                std::thread::sleep(Duration::from_millis(300));
                self.command(CMD_INSERT)?;
                let dlg = self.wait_dialog("Equipment Selection", |d| d.has_control(EQUIPMENT_LIST) && !d.has_control(NEW_DRIVER_MANUFACTURER)).map_err(hint)?;
                choose_equipment(&dlg, spec)?;
                pending = self.auto_equipment_config("añadir el equipo", spec, &mut configured)?;
                let eq = self.wait_until("añadir el equipo", self.t.call, |p| {
                    let t = p.tree().ok()?;
                    t.expand(d);
                    t.children(d).into_iter().find(|c| !before.contains(c))
                })?;
                (d, eq, false)
            }
            None => {
                let before = tree.children(io);
                if !tree.focus(io) {
                    return Err(VijeoError::Internal(format!("no se pudo seleccionar «{IO_MANAGER_NODE}» en el Navigator")));
                }
                std::thread::sleep(Duration::from_millis(300));
                self.command(CMD_INSERT)?;
                let dlg = self.wait_dialog("New Driver", |d| d.has_control(NEW_DRIVER_MANUFACTURER) && d.has_control(NEW_DRIVER_LIST)).map_err(hint)?;
                let cancel = |d: &UiDialog, e: VijeoError| {
                    let _ = d.click(IDCANCEL);
                    e
                };
                let makers = dlg.combo_items(NEW_DRIVER_MANUFACTURER);
                let m = pick(&makers, &spec.manufacturer).map_err(|why| cancel(&dlg, VijeoError::invalid("manufacturer", why)))?;
                if !dlg.select(NEW_DRIVER_MANUFACTURER, m) {
                    return Err(cancel(&dlg, VijeoError::Internal("no se pudo elegir el fabricante en New Driver".into())));
                }
                std::thread::sleep(Duration::from_millis(700));
                let drivers = dlg.list_items(NEW_DRIVER_LIST);
                let k = pick(&drivers, &spec.driver).map_err(|why| {
                    cancel(
                        &dlg,
                        VijeoError::invalid(
                            "driver",
                            format!("{why} (de «{}»). Los drivers que ya tiene el target no salen en New Driver: para añadir un equipo a uno de ellos, indica su nombre en el proyecto", makers[m]),
                        ),
                    )
                })?;
                if !dlg.list_select(NEW_DRIVER_LIST, k) {
                    return Err(cancel(&dlg, VijeoError::Internal("no se pudo elegir el driver en New Driver".into())));
                }
                std::thread::sleep(Duration::from_millis(700));
                choose_equipment(&dlg, spec)?;
                pending = self.auto_equipment_config("crear el driver", spec, &mut configured)?;
                let (d, eq) = self.wait_until("crear el driver", self.t.call, |p| {
                    let t = p.tree().ok()?;
                    t.expand(io);
                    let d = t.children(io).into_iter().find(|c| !before.contains(c))?;
                    t.expand(d);
                    t.children(d).first().map(|&e| (d, e))
                })?;
                (d, eq, true)
            }
        };
        let r = match pending {
            Some(e) => Err(e),
            None => self.configure_new_equipment(drv, eq, created_driver, spec, configured),
        };
        if let Err(e) = r {
            let (item, what) = if created_driver { (drv, "el driver") } else { (eq, "el equipo") };
            let tail = match self.delete_node(item) {
                Ok(()) => format!("; se borró {what} que se había creado"),
                Err(d) => format!("; {what} quedó creado y no se pudo borrar: {d}"),
            };
            self.save()?;
            return Err(match e {
                VijeoError::InvalidArgument { field, reason } => VijeoError::InvalidArgument { field, reason: format!("{reason}{tail}") },
                e => VijeoError::Internal(format!("{e}{tail}")),
            });
        }
        let tree = self.tree()?;
        let names = (tree.text(drv).map(|t| node_name(&t)).unwrap_or_default(), tree.text(eq).map(|t| node_name(&t)).unwrap_or_default());
        self.save()?;
        Ok(names)
    }

    /// Termina los diálogos tras New Driver / Equipment Selection. Si Vijeo abre «Equipment Configuration», pone
    /// ahí IP y Unit ID (si se piden) y la acepta; un campo que falta no corta: se acepta con lo que trae y el error
    /// vuelve como `Ok(Some(error))` para que quien llama borre lo creado.
    fn auto_equipment_config(&mut self, op: &str, spec: &NewEquipment, configured: &mut bool) -> VijeoResult<Option<VijeoError>> {
        let Some(dlg) = self.finish_device_dialogs(op)? else { return Ok(None) };
        let problem = fill_equipment_config(&dlg, spec).err();
        std::thread::sleep(Duration::from_millis(300));
        dlg.click(IDOK);
        *configured = true;
        self.wait_config_closed(op)?;
        if let Some(again) = self.finish_device_dialogs(op)? {
            let _ = again.click(IDCANCEL);
            return Err(VijeoError::Internal(format!("{op}: «Equipment Configuration» volvió a abrirse")));
        }
        Ok(problem)
    }

    /// Espera a que se cierre «Equipment Configuration» tras OK (el clic es asíncrono). Si Vijeo avisa de un valor
    /// no válido, se cierra el aviso y la configuración (Cancel) y se devuelve el texto como error.
    fn wait_config_closed(&mut self, op: &str) -> VijeoResult<()> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            let ds = self.ui.dialogs();
            if let Some(m) = ds.iter().find(|d| d.info.message_box && !d.info.title.contains("Configuration")) {
                let text = m.info.text.clone();
                let _ = m.click(if m.button_ids().contains(&IDOK) { IDOK } else { IDCANCEL });
                std::thread::sleep(Duration::from_millis(500));
                for d in self.ui.dialogs().into_iter().filter(|d| d.info.title.contains("Equipment Configuration")) {
                    let _ = d.click(IDCANCEL);
                }
                return Err(VijeoError::invalid("equipment", format!("{op}: Vijeo no aceptó la configuración del equipo: {text}")));
            }
            if !ds.iter().any(|d| d.info.title.contains("Equipment Configuration")) {
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
        Err(VijeoError::Internal(format!("{op}: «Equipment Configuration» no se cerró con OK")))
    }

    /// Nombres, IP y Unit ID del driver/equipo recién creados (IP y Unit ID, si no se pusieron ya en la
    /// configuración que abrió Vijeo al crearlo).
    fn configure_new_equipment(&mut self, drv: TreeItem, eq: TreeItem, created_driver: bool, spec: &NewEquipment, configured: bool) -> VijeoResult<()> {
        if created_driver {
            if let Some(n) = &spec.driver_name {
                self.rename_node(drv, n, "driver_name")?;
            }
        }
        if let Some(n) = &spec.name {
            self.rename_node(eq, n, "name")?;
        }
        if configured || (spec.ip.is_none() && spec.unit_id.is_none()) {
            return Ok(());
        }
        let tree = self.tree()?;
        if !tree.focus(eq) {
            return Err(VijeoError::Internal("no se pudo seleccionar el equipo en el Navigator".into()));
        }
        std::thread::sleep(Duration::from_millis(300));
        self.command(CMD_EQUIPMENT_CONFIG)?;
        let dlg = self.wait_dialog("Equipment Configuration", |d| d.info.title.contains("Equipment Configuration") || d.has_control(EQUIP_IP))?;
        if let Err(e) = fill_equipment_config(&dlg, spec) {
            let _ = dlg.click(IDCANCEL);
            return Err(e);
        }
        std::thread::sleep(Duration::from_millis(300));
        dlg.click(IDOK);
        self.wait_config_closed("configurar el equipo")?;
        match self.finish_device_dialogs("configurar el equipo")? {
            None => Ok(()),
            Some(again) => {
                let _ = again.click(IDCANCEL);
                Err(VijeoError::Internal("«Equipment Configuration» volvió a abrirse".into()))
            }
        }
    }

    /// Renombra un nodo del Navigator (driver o equipo) editando su etiqueta; un aviso de Vijeo es un error.
    fn rename_node(&mut self, item: TreeItem, want: &str, field: &str) -> VijeoResult<()> {
        let tree = self.tree()?;
        let ok = tree.rename(item, want);
        std::thread::sleep(Duration::from_millis(800));
        if let Some(d) = self.ui.dialogs().into_iter().find(|d| d.info.message_box) {
            let text = d.info.text.clone();
            let _ = d.click(if d.button_ids().contains(&IDOK) { IDOK } else { IDCANCEL });
            return Err(VijeoError::invalid(field, format!("Vijeo no aceptó «{want}»: {text}")));
        }
        let got = tree.text(item).map(|t| node_name(&t)).unwrap_or_default();
        if !ok || got != want {
            return Err(VijeoError::invalid(field, format!("Vijeo dejó el nombre «{got}» en vez de «{want}»")));
        }
        Ok(())
    }

    /// Termina los diálogos que siguen a New Driver / Equipment Selection / Equipment Configuration: «Driver
    /// Configuration» se acepta tal cual; un aviso es un error con su texto; los demás conocidos se responden.
    /// Acaba cuando lleva un rato sin diálogos (`None`) o al aparecer «Equipment Configuration», que se devuelve.
    fn finish_device_dialogs(&mut self, op: &str) -> VijeoResult<Option<UiDialog>> {
        let start = Instant::now();
        let mut quiet = Instant::now();
        while start.elapsed() < self.t.call {
            let ds = self.ui.dialogs();
            if ds.is_empty() {
                if quiet.elapsed() >= Duration::from_millis(1500) {
                    return Ok(None);
                }
            } else {
                quiet = Instant::now();
                for d in ds {
                    if d.info.title.contains("Equipment Configuration") {
                        return Ok(Some(d));
                    } else if d.info.title.contains("Driver Configuration") {
                        d.click(IDOK);
                    } else if d.info.message_box && !matches!(classify(&d), Answer::Click(..) | Answer::Fill(..)) {
                        let text = d.info.text.clone();
                        let _ = d.click(if d.button_ids().contains(&IDOK) { IDOK } else { IDCANCEL });
                        return Err(VijeoError::invalid("equipment", format!("{op}: Vijeo avisó «{text}»")));
                    } else {
                        self.handle_dialogs(&|_| false)?;
                    }
                }
            }
            std::thread::sleep(POLL);
        }
        Err(VijeoError::Timeout { op: op.to_owned(), ms: self.t.call.as_millis() as u64 })
    }

    /// Borra un nodo del Navigator (driver o equipo): Supr y Yes en la confirmación.
    fn delete_node(&mut self, item: TreeItem) -> VijeoResult<()> {
        let tree = self.tree()?;
        if !tree.press_delete(item) {
            return Err(VijeoError::Internal("no se pudo seleccionar el nodo en el Navigator".into()));
        }
        let d = self.wait_dialog("borrar", |d| sorted_buttons(d) == [IDYES, IDNO])?;
        d.click(IDYES);
        self.wait_until("borrar", self.t.call, |p| p.tree().ok().filter(|t| t.text(item).is_none()).map(|_| ()))
    }

    /// Nodo del Navigator que agrupa los paneles base del target.
    fn base_panels_item(&self, tree: &RemoteTree, target: TreeItem) -> Option<TreeItem> {
        self.group_item(tree, target, BASE_PANELS_NODE[0])
    }

    /// Nodo `group` del Navigator (`Base Panels`, `Popup Windows`) en los tres primeros niveles bajo el target.
    fn group_item(&self, tree: &RemoteTree, target: TreeItem, group: &str) -> Option<TreeItem> {
        let mut level = vec![target];
        for _ in 0..3 {
            let mut next = Vec::new();
            for item in level {
                tree.expand(item);
                for c in tree.children(item) {
                    if tree.text(c).is_some_and(|t| t.eq_ignore_ascii_case(group)) {
                        return Some(c);
                    }
                    next.push(c);
                }
            }
            level = next;
        }
        None
    }

    /// Abre el panel base `path` (`Carpeta\Panel`) del target con Intro en el Navigator, como el doble clic.
    /// Termina cuando el título del marco muestra el panel activo (`… - [<target> - <panel> - <idioma>]`).
    pub fn open_panel(&mut self, target: &str, path: &str) -> VijeoResult<()> {
        let tree = self.tree()?;
        let titem = self.target_item(target)?;
        let mut item = self.base_panels_item(&tree, titem).ok_or_else(|| VijeoError::Internal(format!("no se encontró el nodo de paneles base de «{target}» en el Navigator")))?;
        for seg in path.split('\\') {
            tree.expand(item);
            item = tree
                .children(item)
                .into_iter()
                .find(|&c| tree.text(c).is_some_and(|t| navigator_label_is(&t, seg)))
                .ok_or_else(|| VijeoError::Internal(format!("no se encontró «{seg}» de «{path}» en el Navigator")))?;
        }
        let panel = path.rsplit('\\').next().unwrap_or(path);
        let active = format!("[{target} - {panel}").to_lowercase();
        let shown = |p: &Self| {
            p.ui.title().is_some_and(|t| {
                let t = t.to_lowercase();
                t.contains(&format!("{active} - ")) || t.contains(&format!("{active}]"))
            })
        };
        if shown(self) {
            return Ok(());
        }
        if !tree.press_enter(item) {
            return Err(VijeoError::Internal(format!("no se pudo seleccionar «{path}» en el Navigator")));
        }
        let r = self.wait_until("abrir el panel", OPEN_PANEL_TIMEOUT, |p| shown(p).then_some(()));
        r.map_err(|e| match e {
            VijeoError::Timeout { .. } => {
                let hint = if self.built { AFTER_BUILD_HINT } else { "" };
                VijeoError::Internal(format!("Vijeo no abrió «{path}» (título de la ventana: {:?}){hint}", self.ui.title().unwrap_or_default()))
            }
            e => e,
        })
    }

    /// Comando de menú esperando a que Vijeo lo termine. Si no termina en [`EDIT_COMMAND_WAIT`] (un diálogo
    /// modal, o un pegado grande), se responde a los diálogos conocidos y se espera a que vuelva a atender.
    fn sync_command(&mut self, op: &str, id: u16) -> VijeoResult<()> {
        if self.ui.command_sync(id, EDIT_COMMAND_WAIT) {
            return self.handle_dialogs(&|_| false);
        }
        self.wait_until(op, self.t.call, |p| (p.ui.dialogs().is_empty() && p.ui.responsive()).then_some(()))
    }

    /// Da el foco al editor del panel abierto: Copy/Cut/Paste van a lo que tiene el foco.
    fn focus_document(&mut self) -> VijeoResult<()> {
        if self.ui.focus_active_document() {
            std::thread::sleep(Duration::from_millis(200));
            Ok(())
        } else {
            Err(VijeoError::Internal("no hay ningún panel abierto en el editor de Vijeo".into()))
        }
    }

    /// Abre el panel y copia (o corta, con `cut`) todos sus objetos: Select All + Copy/Cut con el foco en el
    /// editor. Devuelve el contenido del formato de objetos del portapapeles, o `None` si el panel no tiene
    /// objetos. Deja el portapapeles con eso: quien llama guarda y devuelve el del usuario.
    pub fn copy_objects(&mut self, target: &str, path: &str, cut: bool) -> VijeoResult<Option<Vec<u8>>> {
        self.open_panel(target, path)?;
        self.focus_document()?;
        let fmt = clipboard::format(objects::CLIPBOARD_FORMAT);
        clipboard::clear().map_err(VijeoError::Internal)?;
        self.sync_command("seleccionar los objetos", CMD_SELECT_ALL)?;
        let (op, cmd) = if cut { ("cortar los objetos", CMD_CUT) } else { ("copiar los objetos", CMD_COPY) };
        self.sync_command(op, cmd)?;
        if clipboard::has_format(clipboard::format(objects::PANEL_CLIPBOARD_FORMAT)) {
            return Err(VijeoError::Internal("Vijeo copió el panel entero en vez de sus objetos (el foco estaba en el Navigator)".into()));
        }
        if !clipboard::has_format(fmt) {
            return Ok(None);
        }
        let bytes = clipboard::get_ole(fmt).map_err(VijeoError::Internal)?;
        bytes.map(Some).ok_or_else(|| VijeoError::Internal(format!("Vijeo ofreció {} pero no lo entregó", objects::CLIPBOARD_FORMAT)))
    }

    /// Pega `cfb` (contenido del formato de objetos) en el panel abierto en el editor.
    pub fn paste_objects(&mut self, cfb: &[u8]) -> VijeoResult<()> {
        self.focus_document()?;
        clipboard::set(clipboard::format(objects::CLIPBOARD_FORMAT), cfb).map_err(VijeoError::Internal)?;
        self.sync_command("pegar los objetos", CMD_PASTE)
    }

    /// Renombra el target editando su etiqueta en el Navigator. Devuelve el nombre que queda.
    pub fn rename_target(&mut self, old: &str, new: &str) -> VijeoResult<String> {
        let item = self.target_item(old)?;
        let tree = self.tree()?;
        if !tree.rename(item, new) {
            return Err(VijeoError::Internal(format!("no se pudo editar el nombre de «{old}» en el Navigator")));
        }
        // Un nombre no válido abre un aviso: se cierra y se informa con su texto.
        std::thread::sleep(Duration::from_millis(800));
        self.handle_dialogs(&|_| false)?;
        if let Some(d) = self.ui.dialogs().into_iter().find(|d| d.info.message_box) {
            let _ = d.click(if d.button_ids().contains(&IDOK) { IDOK } else { IDCANCEL });
            return Err(VijeoError::invalid("new_name", format!("Vijeo no aceptó «{new}»: {}", d.info.text)));
        }
        let got = tree.text(item).unwrap_or_default();
        self.save()?;
        Ok(got)
    }

    /// Borra el target: Supr en el Navigator y Yes en «Delete Target».
    pub fn delete_target(&mut self, name: &str) -> VijeoResult<()> {
        let item = self.target_item(name)?;
        if !self.tree()?.press_delete(item) {
            return Err(VijeoError::Internal(format!("no se pudo seleccionar «{name}» en el Navigator")));
        }
        let lower = name.to_lowercase();
        let confirm = move |d: &UiDialog| sorted_buttons(d) == [IDYES, IDNO] && d.info.text.to_lowercase().contains(&lower);
        let d = self.wait_dialog("Delete Target", confirm)?;
        d.click(IDYES);
        self.wait_until("borrar el target", self.t.call, |p| p.target_items().ok().filter(|ts| !ts.iter().any(|(_, t)| t.eq_ignore_ascii_case(name))).map(|_| ()))?;
        self.save()
    }

    /// Cierra el proyecto sin guardar y sale. Si algo no sale como se espera, se termina el proceso.
    fn shutdown(&mut self) {
        let name = self.temp.clone();
        if self.ui.is_alive() && self.ui.command(CMD_CLOSE_PROJECT) {
            let start = Instant::now();
            while self.ui.is_alive() && start.elapsed() < CLOSE_TIMEOUT {
                // «Do you wish to save the changes you made to <proyecto>?» (Yes/No/Cancel): No. Con historial de
                // revisiones es el diálogo del comentario, que no es un MessageBox.
                let ask = |d: &UiDialog| (d.info.message_box || d.has_control(REVISION_COMMENT)) && sorted_buttons(d) == [IDCANCEL, IDYES, IDNO] && d.info.text.contains(&name);
                if let Some(d) = self.ui.dialogs().into_iter().find(ask) {
                    d.click(IDNO);
                }
                if self.ui.title().is_some_and(|s| !s.contains(&name)) && self.ui.dialogs().is_empty() {
                    break;
                }
                std::thread::sleep(POLL);
            }
        }
        let closed_project = self.ui.title().is_some_and(|s| !s.contains(&name));
        if !(closed_project && self.ui.dialogs().is_empty() && self.ui.close(CLOSE_TIMEOUT)) {
            self.ui.kill();
        }
    }

    /// Cierra Vijeo y borra la carpeta temporal. Devuelve un aviso si no se pudo borrar.
    pub fn close(mut self) -> Option<String> {
        self.shutdown();
        self.remove_folder()
    }

    fn remove_folder(&mut self) -> Option<String> {
        let r = vdz::remove_temp_project(&self.workspace, &self.temp).err().map(|e| format!("no se pudo borrar la carpeta temporal Vijeo-Manager\\{}: {e}", self.temp));
        // Ya borrada: que `Drop` no lo repita.
        self.temp.clear();
        r
    }
}

impl Drop for UiProject {
    fn drop(&mut self) {
        if !self.temp.is_empty() {
            self.shutdown();
            if let Some(w) = self.remove_folder() {
                tracing::warn!("{w}");
            }
        }
    }
}

/// Equipo que se añade con [`UiProject::add_equipment`].
#[derive(Debug, Clone, Default)]
pub struct NewEquipment {
    /// Driver del target al que se añade (nombre en el Navigator); `None`: se crea uno con New Driver.
    pub existing_driver: Option<String>,
    /// Fabricante y driver como los muestra New Driver (o una parte única del texto).
    pub manufacturer: String,
    pub driver: String,
    /// Tipo de equipo de la lista (`Modbus Equipment`); por defecto, el primero.
    pub equipment_type: Option<String>,
    pub driver_name: Option<String>,
    pub name: Option<String>,
    pub ip: Option<[u8; 4]>,
    pub unit_id: Option<u8>,
}

/// Pone IP y Unit ID (los que pida `spec`) en «Equipment Configuration». No pulsa OK.
fn fill_equipment_config(dlg: &UiDialog, spec: &NewEquipment) -> VijeoResult<()> {
    let missing = |field: &str, what: &str| VijeoError::invalid(field, format!("la configuración de este equipo no tiene {what} (sólo los equipos de red como Modbus TCP)"));
    if let Some(ip) = spec.ip {
        if !dlg.set_ip(EQUIP_IP, ip) {
            return Err(missing("ip_address", "dirección IP"));
        }
    }
    if let Some(u) = spec.unit_id {
        if !dlg.has_control(EQUIP_UNIT_ID) || !dlg.enter_text(EQUIP_UNIT_ID, &u.to_string()) {
            return Err(missing("unit_id", "Unit ID"));
        }
    }
    Ok(())
}

/// Elige el tipo de equipo en la lista 4826 (New Driver o Equipment Selection) y pulsa OK.
fn choose_equipment(dlg: &UiDialog, spec: &NewEquipment) -> VijeoResult<()> {
    let items = dlg.list_items(EQUIPMENT_LIST);
    let i = match &spec.equipment_type {
        Some(w) => pick(&items, w).map_err(|why| {
            let _ = dlg.click(IDCANCEL);
            VijeoError::invalid("equipment_type", why)
        })?,
        None if !items.is_empty() => 0,
        None => {
            let _ = dlg.click(IDCANCEL);
            return Err(VijeoError::Internal("la lista de tipos de equipo está vacía".into()));
        }
    };
    if !dlg.list_select(EQUIPMENT_LIST, i) {
        let _ = dlg.click(IDCANCEL);
        return Err(VijeoError::Internal("no se pudo elegir el tipo de equipo".into()));
    }
    std::thread::sleep(Duration::from_millis(300));
    dlg.click(IDOK);
    Ok(())
}

/// Elemento de una lista de Vijeo: el que es `want` sin distinguir mayúsculas o, si no, el único que lo contiene.
fn pick(items: &[String], want: &str) -> Result<usize, String> {
    let w = want.trim().to_lowercase();
    if let Some(i) = items.iter().position(|x| x.trim().to_lowercase() == w) {
        return Ok(i);
    }
    let hits: Vec<usize> = items.iter().enumerate().filter(|(_, x)| !w.is_empty() && x.to_lowercase().contains(&w)).map(|(i, _)| i).collect();
    match hits.as_slice() {
        [i] => Ok(*i),
        [] => Err(format!("«{want}» no está entre {items:?}")),
        _ => Err(format!("«{want}» es ambiguo: {:?}", hits.iter().map(|&i| &items[i]).collect::<Vec<_>>())),
    }
}

/// Nombre de un driver o equipo en el Navigator, sin la dirección que Vijeo pone detrás (`M221 [ 172.20.10.167 ]`).
fn node_name(label: &str) -> String {
    label.split(" [").next().unwrap_or(label).trim().to_owned()
}

/// PanelID y nombre de la etiqueta de un panel base en el Navigator (`5: Panel5` → `(5, "Panel5")`).
fn parse_panel_label(label: &str) -> Option<(u32, String)> {
    let (id, name) = label.split_once(": ")?;
    Some((id.trim().parse().ok()?, name.trim().to_owned())).filter(|(_, n)| !n.is_empty())
}

/// Etiqueta de un nodo del Navigator que es `name`: los paneles base llevan delante su número (`1: Start`).
fn navigator_label_is(label: &str, name: &str) -> bool {
    let bare = match label.split_once(": ") {
        Some((id, rest)) if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) => rest,
        _ => label,
    };
    bare.trim().eq_ignore_ascii_case(name.trim())
}

/// Elemento de un combo del asistente: el que es `want` o, si no hay, el único que es `want (…)` (el
/// asistente puede añadir la resolución al nombre comercial: `HMIGTO1300 (320x240)`).
fn find_item(items: &[String], want: &str) -> Option<usize> {
    if let Some(i) = items.iter().position(|x| x.trim().eq_ignore_ascii_case(want.trim())) {
        return Some(i);
    }
    let prefix = format!("{} (", want.trim().to_ascii_lowercase());
    let hits: Vec<usize> = items.iter().enumerate().filter(|(_, x)| x.trim().to_ascii_lowercase().starts_with(&prefix)).map(|(i, _)| i).collect();
    match hits.as_slice() {
        [i] => Some(*i),
        _ => None,
    }
}

/// Familia en el combo del asistente: como [`find_item`] o, si no, el único elemento que empieza por el
/// nombre del catálogo (`Tablet` es «Tablets and Smartphones» en el asistente, verificado con --probe-models).
fn find_family(items: &[String], want: &str) -> Option<usize> {
    find_item(items, want).or_else(|| {
        let w = want.trim().to_ascii_lowercase();
        let hits: Vec<usize> = items.iter().enumerate().filter(|(_, x)| !w.is_empty() && x.trim().to_ascii_lowercase().starts_with(&w)).map(|(i, _)| i).collect();
        match hits.as_slice() {
            [i] => Some(*i),
            _ => None,
        }
    })
}

/// Target para los asistentes, con los nombres que muestra Vijeo (los comerciales del catálogo).
#[derive(Debug, Clone)]
pub struct NewTarget {
    pub name: String,
    /// Familia (`HMIGTO Series`).
    pub family: String,
    /// Modelo (`HMIGTO1300 (320x240)`).
    pub model: String,
    /// Pantalla de los modulares (`HMIDT351 (800x480)`).
    pub display: Option<String>,
}

pub struct UiBuild {
    /// Líneas tal cual (con los marcadores de tipo).
    pub lines: Vec<String>,
    pub notes: Vec<String>,
}

/// Compila una copia de `vdz` con el nombre temporal `temp` y la cierra sin guardar.
pub fn build_copy(frame_dir: &Path, workspace: &Path, vdz: &Path, work: &Path, temp: &str, t: Timeouts) -> VijeoResult<UiBuild> {
    let mut p = UiProject::open(frame_dir, workspace, vdz, work, temp, t)?;
    let lines = p.build();
    let mut notes = p.notes().to_vec();
    notes.extend(p.close());
    Ok(UiBuild { lines: lines?, notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vijeo_ui::dialog::DialogInfo;
    use vijeo_ui::ui::UiControl;

    fn dialog(text: &str, message_box: bool, controls: &[(i32, &str)]) -> UiDialog {
        UiDialog::for_test(
            DialogInfo { title: "Vijeo".into(), text: text.into(), buttons: controls.iter().filter(|c| c.1 == "Button").map(|c| c.0.to_string()).collect(), message_box },
            controls.iter().map(|&(id, class)| UiControl { id, class: class.into(), text: String::new() }).collect(),
        )
    }

    #[test]
    fn navigator_labels() {
        assert!(navigator_label_is("1: Start", "start"));
        assert!(navigator_label_is("61: iEM_Energy", "iEM_Energy"));
        assert!(navigator_label_is("Carpeta", "carpeta"));
        assert!(!navigator_label_is("1: Start", "1"));
        assert!(!navigator_label_is("A: Start", "Start"));
    }

    #[test]
    fn device_lists() {
        let drivers: Vec<String> = ["Jbus (RTU)", "Modbus (RTU)", "Modbus Slave", "Modbus TCP/IP", "XWAY TCP/IP"].iter().map(|s| s.to_string()).collect();
        assert_eq!(pick(&drivers, "modbus (rtu)"), Ok(1));
        assert_eq!(pick(&drivers, "Modbus TCP"), Ok(3));
        assert_eq!(pick(&drivers, "xway"), Ok(4));
        assert!(pick(&drivers, "modbus").unwrap_err().contains("ambiguo"));
        assert!(pick(&drivers, "Profibus").unwrap_err().contains("no está"));
        assert_eq!(node_name("M221_Controller [ 172.20.10.167 ]"), "M221_Controller");
        assert_eq!(node_name("ModbusRTU01 [ COM1 ]"), "ModbusRTU01");
        assert_eq!(node_name("ModbusTCP"), "ModbusTCP");
    }

    #[test]
    fn panel_labels() {
        assert_eq!(parse_panel_label("5: Panel5"), Some((5, "Panel5".to_owned())));
        assert_eq!(parse_panel_label("61: iEM_Energy"), Some((61, "iEM_Energy".to_owned())));
        assert_eq!(parse_panel_label("12: Mot ores"), Some((12, "Mot ores".to_owned())));
        assert_eq!(parse_panel_label("Base Panels"), None);
        assert_eq!(parse_panel_label("A: Start"), None);
        assert_eq!(parse_panel_label("7: "), None);
    }

    #[test]
    fn finds_families_by_prefix_only_if_unique() {
        let items: Vec<String> = ["iPC Series", "HMIGTU Series", "HMIGTUX Series", "Tablets and Smartphones"].map(String::from).to_vec();
        assert_eq!(find_family(&items, "Tablet"), Some(3));
        assert_eq!(find_family(&items, "HMIGTU Series"), Some(1), "exacto antes que prefijo");
        assert_eq!(find_family(&items, "HMIGTU"), None, "prefijo ambiguo");
        assert_eq!(find_family(&items, "HMISCU Series"), None);
    }

    #[test]
    fn known_dialogs() {
        let conversion = dialog("Opening the project will update it…", false, &[(11553, "Static"), (CONVERSION_CHECKBOX, "Button"), (6, "Button"), (7, "Button"), (9, "Button")]);
        assert!(matches!(classify(&conversion), Answer::Click(IDYES, _)));
        let target = dialog("The project was created with a Target Type that is not supported in this version", true, &[(6, "Button"), (7, "Button"), (0xFFFF, "Static")]);
        assert!(matches!(classify(&target), Answer::Click(IDNO, _)));
        assert_eq!(classify(&dialog("", false, &[(100, "msctls_progress32")])), Answer::Wait);
        assert_eq!(classify(&dialog("Progress", false, &[(IDCANCEL, "Button"), (100, "msctls_progress32")])), Answer::Wait, "Building All");
        let wizard = dialog("Project Name", true, &[(0xFFFF, "Static"), (WIZ_NEXT, "Button"), (WIZ_FINISH, "Button"), (IDCANCEL, "Button")]);
        assert_eq!(classify(&wizard), Answer::Wait);
        let many = dialog("You already have 3 instance(s) of Vijeo-Frame running.  You may experience slower performance", true, &[(1, "Button"), (0xFFFF, "Static")]);
        assert!(matches!(classify(&many), Answer::Click(IDOK, _)));
        let many_cancel = dialog("You already have 4 instance(s) of Vijeo-Frame running.", true, &[(IDCANCEL, "Button"), (0xFFFF, "Static")]);
        assert!(matches!(classify(&many_cancel), Answer::Click(IDCANCEL, _)));
        // Save Project en un proyecto con historial de revisiones (PROYC): comentario y Yes.
        let revision = dialog(
            "Revision History Comment: Do you wish to save the changes you made to MCP_TMP_1_2?",
            false,
            &[(6, "Button"), (2, "Button"), (7, "Button"), (REVISION_COMMENT, "Edit"), (REVISION_LABEL, "Static"), (11218, "Static")],
        );
        assert!(matches!(classify(&revision), Answer::Fill(REVISION_COMMENT, REVISION_TEXT, IDYES, _)));
        // Un Yes/No cualquiera no se contesta.
        assert_eq!(classify(&dialog("Delete everything?", true, &[(6, "Button"), (7, "Button"), (0xFFFF, "Static")])), Answer::Unknown);
    }
}
