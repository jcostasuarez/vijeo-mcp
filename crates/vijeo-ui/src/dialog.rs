//! Detección de diálogos de un proceso (ventanas de primer nivel de clase `#32770`).
//!
//! Un `MessageBox` (también `AfxMessageBox`) se reconoce por su texto en un `Static` con id `0xFFFF`
//! y botones con ids estándar (`IDOK`…`IDCONTINUE`). Así no se confunde con diálogos de progreso.

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, EnumWindows, GetClassNameW, GetDlgCtrlID, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogInfo {
    pub title: String,
    pub text: String,
    pub buttons: Vec<String>,
    pub message_box: bool,
}

pub(crate) fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

pub(crate) fn text_of(hwnd: HWND) -> String {
    let mut buf = vec![0u16; 2048];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let v = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    v.push(hwnd);
    BOOL(1)
}

/// Ventanas de primer nivel del proceso `pid` (visibles o no).
pub(crate) fn top_windows_of(pid: u32) -> Vec<HWND> {
    let mut tops: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut tops as *mut Vec<HWND> as isize));
    }
    tops.retain(|&h| {
        let mut p = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut p)) };
        p == pid
    });
    tops
}

/// Todas las ventanas descendientes de `hwnd`.
pub(crate) fn descendants_of(hwnd: HWND) -> Vec<HWND> {
    let mut children: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumChildWindows(Some(hwnd), Some(collect), LPARAM(&mut children as *mut Vec<HWND> as isize));
    }
    children
}

/// Diálogos visibles (`#32770`) de primer nivel del proceso `pid`.
pub fn dialogs_of(pid: u32) -> Vec<DialogInfo> {
    top_windows_of(pid)
        .into_iter()
        .filter(|&h| unsafe { IsWindowVisible(h) }.as_bool() && class_of(h) == "#32770")
        .map(describe)
        .collect()
}

/// El primer `MessageBox` visible del proceso, si lo hay.
pub fn message_box_of(pid: u32) -> Option<DialogInfo> {
    dialogs_of(pid).into_iter().find(|d| d.message_box)
}

pub(crate) fn describe(hwnd: HWND) -> DialogInfo {
    let children = descendants_of(hwnd);
    let mut texts = Vec::new();
    let mut buttons = Vec::new();
    let mut has_msg_static = false;
    let mut has_std_button = false;
    let mut has_other = false;
    for c in children {
        let class = class_of(c);
        let id = unsafe { GetDlgCtrlID(c) };
        let t = text_of(c);
        if class.eq_ignore_ascii_case("Static") {
            if id == 0xFFFF {
                has_msg_static = true;
            }
            if !t.trim().is_empty() {
                texts.push(t.trim().to_owned());
            }
        } else if class.eq_ignore_ascii_case("Button") {
            if (1..=11).contains(&id) {
                has_std_button = true;
            }
            buttons.push(t.replace('&', ""));
        } else if class.to_ascii_lowercase().contains("progress") {
            has_other = true;
        }
    }
    DialogInfo {
        title: text_of(hwnd),
        text: texts.join(" "),
        buttons,
        message_box: has_msg_static && has_std_button && !has_other,
    }
}
