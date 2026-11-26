//! Instancia «normal» de `Vijeo-Frame.exe`: lanzada como ejecutable (`/Import <vdz>`), no por COM.
//!
//! Las instancias COM arrancan con `-Embedding` y Vijeo se pone en modo SoMachine (MSD), que añade a
//! cada target una red SoMachine sin dirección (falso `Error 1331`, inventario §12.9). Ésta queda en
//! modo normal y se maneja como lo haría un usuario: comandos de menú (`WM_COMMAND`) al marco y
//! `BM_CLICK` en los botones de sus diálogos. Sólo se actúa sobre las ventanas del proceso lanzado.

use std::ffi::OsStr;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use vijeo_core::{VijeoError, VijeoResult};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    GetDlgCtrlID, GetParent, IsWindowVisible, PostMessageW, EN_KILLFOCUS, SendMessageTimeoutW, BM_CLICK, CBN_CLOSEUP, CBN_SELCHANGE, CBN_SELENDOK, CB_GETCOUNT, CB_GETLBTEXT, CB_GETLBTEXTLEN,
    CB_SETCURSEL, SMTO_ABORTIFHUNG, SMTO_BLOCK, WM_CLOSE, WM_COMMAND, WM_SETTEXT,
};

use crate::dialog::{class_of, describe, descendants_of, text_of, top_windows_of, DialogInfo};

/// Un control de un diálogo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiControl {
    pub id: i32,
    pub class: String,
    pub text: String,
}

/// Un diálogo visible (`#32770`) de la instancia.
#[derive(Debug, Clone)]
pub struct UiDialog {
    /// `HWND` como entero: los `HWND` no son `Send`.
    hwnd: isize,
    pub info: DialogInfo,
    pub controls: Vec<UiControl>,
}

impl UiDialog {
    /// Diálogo sin ventana, para probar la lógica que decide qué responder (`click` no hace nada).
    #[doc(hidden)]
    pub fn for_test(info: DialogInfo, controls: Vec<UiControl>) -> Self {
        Self { hwnd: 0, info, controls }
    }

    /// Ids de sus botones (incluidas casillas).
    pub fn button_ids(&self) -> Vec<i32> {
        self.controls.iter().filter(|c| c.class.eq_ignore_ascii_case("Button")).map(|c| c.id).collect()
    }

    pub fn has_control(&self, id: i32) -> bool {
        self.controls.iter().any(|c| c.id == id)
    }

    /// Pulsa el botón `id` (`BM_CLICK` asíncrono: un `WM_COMMAND IDOK` no basta en los diálogos de Vijeo).
    pub fn click(&self, id: i32) -> bool {
        if self.hwnd == 0 {
            return false;
        }
        let Some(b) = descendants_of(hwnd(self.hwnd)).into_iter().find(|&c| unsafe { GetDlgCtrlID(c) } == id && class_of(c).eq_ignore_ascii_case("Button")) else {
            return false;
        };
        unsafe { PostMessageW(Some(b), BM_CLICK, WPARAM(0), LPARAM(0)) }.is_ok()
    }

    fn control(&self, id: i32, class: &str) -> Option<HWND> {
        if self.hwnd == 0 {
            return None;
        }
        descendants_of(hwnd(self.hwnd))
            .into_iter()
            .find(|&c| unsafe { GetDlgCtrlID(c) } == id && class_of(c).eq_ignore_ascii_case(class) && unsafe { IsWindowVisible(c) }.as_bool())
    }

    /// Escribe `text` en el control `id` de clase `class` (`WM_SETTEXT`; Windows copia la cadena al otro proceso).
    pub fn set_text(&self, id: i32, class: &str, text: &str) -> bool {
        let Some(c) = self.control(id, class) else { return false };
        let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
        send(c, WM_SETTEXT, 0, wide.as_ptr() as isize).is_some()
    }

    /// Como [`UiDialog::set_text`] y además avisa al diálogo de que el campo perdió el foco
    /// (`EN_KILLFOCUS`): los asistentes de Vijeo leen ahí el nombre del proyecto o del target.
    pub fn enter_text(&self, id: i32, text: &str) -> bool {
        let Some(c) = self.control(id, "Edit") else { return false };
        let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
        let parent = unsafe { GetParent(c) }.unwrap_or(hwnd(self.hwnd));
        send(c, WM_SETTEXT, 0, wide.as_ptr() as isize).is_some() && send(parent, WM_COMMAND, ((EN_KILLFOCUS as usize) << 16) | (id as u16 as usize), c.0 as isize).is_some()
    }

    /// Texto del control `id` de clase `class`.
    pub fn text_of(&self, id: i32, class: &str) -> Option<String> {
        self.control(id, class).map(text_of)
    }

    /// Elementos del combo `id` (`CB_GETLBTEXT`, que Windows también traduce entre procesos).
    pub fn combo_items(&self, id: i32) -> Vec<String> {
        let Some(c) = self.control(id, "ComboBox") else { return Vec::new() };
        let n = send(c, CB_GETCOUNT, 0, 0).unwrap_or(0).max(0) as usize;
        (0..n)
            .map(|i| {
                let len = send(c, CB_GETLBTEXTLEN, i, 0).unwrap_or(0).max(0) as usize;
                let mut buf = vec![0u16; len + 1];
                let got = send(c, CB_GETLBTEXT, i, buf.as_mut_ptr() as isize).unwrap_or(0).max(0) as usize;
                String::from_utf16_lossy(&buf[..got.min(len)])
            })
            .collect()
    }

    /// Elige el elemento `index` del combo `id` como lo haría el usuario. `CB_SETCURSEL` sólo cambia lo que
    /// se ve: el diálogo actualiza su estado (p.ej. el tipo de fichero de un `Save As`) con las
    /// notificaciones `CBN_SELENDOK` + `CBN_SELCHANGE` + `CBN_CLOSEUP` (verificado con Export Project).
    pub fn select(&self, id: i32, index: usize) -> bool {
        let Some(c) = self.control(id, "ComboBox") else { return false };
        let d = hwnd(self.hwnd);
        let notify = |code: u32| send(d, WM_COMMAND, ((code as usize) << 16) | (id as u16 as usize), c.0 as isize);
        send(c, CB_SETCURSEL, index, 0).is_some_and(|r| r == index as isize)
            && notify(CBN_SELENDOK).is_some()
            && notify(CBN_SELCHANGE).is_some()
            && notify(CBN_CLOSEUP).is_some()
    }

    /// Pulsa el botón `id` con `WM_COMMAND BN_CLICKED`. Los diálogos de fichero no atienden `BM_CLICK`
    /// si no son la ventana activa; los de Vijeo, al revés (ver [`UiDialog::click`]).
    pub fn press(&self, id: i32) -> bool {
        let Some(b) = self.control(id, "Button") else { return false };
        unsafe { PostMessageW(Some(hwnd(self.hwnd)), WM_COMMAND, WPARAM(id as u16 as usize), LPARAM(b.0 as isize)) }.is_ok()
    }
}

/// `SendMessageTimeout` (5 s, sin esperar si el otro hilo está colgado): `None` si no respondió.
fn send(h: HWND, msg: u32, w: usize, l: isize) -> Option<isize> {
    let mut result = 0usize;
    let ok = unsafe { SendMessageTimeoutW(h, msg, WPARAM(w), LPARAM(l), SMTO_ABORTIFHUNG | SMTO_BLOCK, 5000, Some(&mut result)) };
    (ok.0 != 0).then_some(result as isize)
}

fn hwnd(v: isize) -> HWND {
    HWND(v as *mut _)
}

pub struct UiInstance {
    child: Child,
}

impl UiInstance {
    /// Lanza `exe` con `args`, sin heredar la entrada/salida estándar (la del MCP es su canal stdio).
    pub fn launch<I, S>(exe: &Path, args: I) -> VijeoResult<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let child = Command::new(exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| VijeoError::Internal(format!("no se pudo lanzar {}: {e}", exe.display())))?;
        tracing::info!(pid = child.id(), "Vijeo-Frame lanzado como ejecutable");
        // Que muera con el MCP aunque éste no llegue a cerrarlo (Drop, shutdown).
        if !crate::process::tie_to_this_process(HANDLE(child.as_raw_handle())) {
            tracing::warn!(pid = child.id(), "no se pudo atar Vijeo-Frame al job del MCP: si el MCP muere de golpe, quedará abierto");
        }
        Ok(Self { child })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn visible_tops(&self) -> Vec<HWND> {
        top_windows_of(self.pid()).into_iter().filter(|&h| unsafe { IsWindowVisible(h) }.as_bool()).collect()
    }

    /// Marco principal: la primera ventana visible con título que no es un diálogo.
    fn frame(&self) -> Option<HWND> {
        self.visible_tops().into_iter().find(|&h| class_of(h) != "#32770" && !text_of(h).is_empty())
    }

    /// Título del marco principal (`<Proyecto> - Vijeo-Frame - [<target> - <panel>]`).
    pub fn title(&self) -> Option<String> {
        self.frame().map(text_of)
    }

    /// Diálogos visibles de la instancia.
    pub fn dialogs(&self) -> Vec<UiDialog> {
        self.visible_tops()
            .into_iter()
            .filter(|&h| class_of(h) == "#32770")
            .map(|h| UiDialog {
                hwnd: h.0 as isize,
                info: describe(h),
                controls: descendants_of(h).into_iter().map(|c| UiControl { id: unsafe { GetDlgCtrlID(c) }, class: class_of(c), text: text_of(c).replace('&', "") }).collect(),
            })
            .collect()
    }

    /// Envía el comando de menú `id` al marco principal. `false` si no hay marco.
    pub fn command(&self, id: u16) -> bool {
        self.frame().is_some_and(|f| unsafe { PostMessageW(Some(f), WM_COMMAND, WPARAM(id as usize), LPARAM(0)) }.is_ok())
    }

    /// Pide al marco que se cierre (`WM_CLOSE`) y espera a que el proceso termine.
    pub fn close(&mut self, timeout: Duration) -> bool {
        if let Some(f) = self.frame() {
            let _ = unsafe { PostMessageW(Some(f), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
        self.wait_exit(timeout)
    }

    pub fn wait_exit(&mut self, timeout: Duration) -> bool {
        let t = Instant::now();
        while self.is_alive() {
            if t.elapsed() >= timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        true
    }

    pub fn kill(&mut self) {
        if self.is_alive() {
            tracing::warn!(pid = self.pid(), "se termina la instancia normal de Vijeo");
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl Drop for UiInstance {
    fn drop(&mut self) {
        self.kill();
    }
}
