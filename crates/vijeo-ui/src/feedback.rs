//! Lectura de la ventana de salida de Vijeo-Frame («Feedback Zone»).
//!
//! Es un `ListBox` estándar con `LBS_HASSTRINGS` (id 102) dentro de la barra de id 711, así que
//! `LB_GETTEXT` funciona entre procesos sin inyectar nada. Se busca por ids y no por el título, que
//! depende del idioma de Vijeo. Vijeo la vacía al empezar cada compilación.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetDlgCtrlID, GetParent, SendMessageTimeoutW, GA_ROOT, SMTO_ABORTIFHUNG, SMTO_BLOCK};

use crate::dialog::{class_of, descendants_of, top_windows_of};

const FEEDBACK_BAR_ID: i32 = 711;
const LISTBOX_ID: i32 = 102;
const LB_GETTEXT: u32 = 0x0189;
const LB_GETTEXTLEN: u32 = 0x018A;
const LB_GETCOUNT: u32 = 0x018B;
/// Tiempo máximo por mensaje: si Vijeo no responde se deja de leer.
const MSG_TIMEOUT_MS: u32 = 2000;

fn send(hwnd: HWND, msg: u32, w: usize, l: isize) -> Option<isize> {
    let mut result = 0usize;
    let ok = unsafe { SendMessageTimeoutW(hwnd, msg, WPARAM(w), LPARAM(l), SMTO_ABORTIFHUNG | SMTO_BLOCK, MSG_TIMEOUT_MS, Some(&mut result)) };
    (ok.0 != 0).then_some(result as isize)
}

/// ¿Tiene `hwnd` un antecesor (dentro de su ventana raíz) con id `id`?
fn has_ancestor_id(hwnd: HWND, id: i32) -> bool {
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let mut cur = unsafe { GetParent(hwnd) }.unwrap_or_default();
    while !cur.is_invalid() && cur != root {
        if unsafe { GetDlgCtrlID(cur) } == id {
            return true;
        }
        cur = unsafe { GetParent(cur) }.unwrap_or_default();
    }
    false
}

fn feedback_listbox(pid: u32) -> Option<HWND> {
    top_windows_of(pid).into_iter().flat_map(descendants_of).find(|&h| {
        class_of(h).eq_ignore_ascii_case("ListBox") && unsafe { GetDlgCtrlID(h) } == LISTBOX_ID && has_ancestor_id(h, FEEDBACK_BAR_ID)
    })
}

/// Líneas de la Feedback Zone del proceso `pid`. `None` si no se encuentra la ventana o no responde.
pub fn feedback_lines(pid: u32) -> Option<Vec<String>> {
    let lb = feedback_listbox(pid)?;
    let count = send(lb, LB_GETCOUNT, 0, 0)?;
    let mut lines = Vec::with_capacity(count.max(0) as usize);
    for i in 0..count.max(0) as usize {
        let len = send(lb, LB_GETTEXTLEN, i, 0)?;
        if len < 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = send(lb, LB_GETTEXT, i, buf.as_mut_ptr() as isize)?;
        lines.push(String::from_utf16_lossy(&buf[..n.clamp(0, len) as usize]));
    }
    Some(lines)
}
