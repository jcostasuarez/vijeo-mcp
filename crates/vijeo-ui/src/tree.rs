//! Árbol del Navigator (`SysTreeView32`) de una instancia normal de Vijeo-Frame.
//!
//! Los mensajes del TreeView que llevan estructuras no se traducen entre procesos: el `TVITEMW` se
//! escribe en memoria de Vijeo (`VirtualAllocEx`) con el formato de 32 bits, porque `Vijeo-Frame.exe`
//! 6.3 es un proceso de 32 bits. Sólo se usa con instancias lanzadas por el MCP.

use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::Memory::{VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_DELETE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{
    GetDlgCtrlID, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, SendMessageTimeoutW, SMTO_ABORTIFHUNG, SMTO_BLOCK, WM_KEYDOWN, WM_KEYUP, WM_SETTEXT,
};

use crate::dialog::{class_of, descendants_of, top_windows_of};

const TVM_EXPAND: u32 = 0x1102;
const TVE_EXPAND: usize = 2;
const TVM_GETNEXTITEM: u32 = 0x110A;
const TVM_SELECTITEM: u32 = 0x110B;
const TVM_ENDEDITLABELNOW: u32 = 0x1116;
const TVM_GETITEMW: u32 = 0x113E;
const TVM_EDITLABELW: u32 = 0x1141;
const TVGN_ROOT: usize = 0;
const TVGN_NEXT: usize = 1;
const TVGN_CHILD: usize = 4;
const TVGN_CARET: usize = 9;
const TVIF_TEXT: u32 = 1;
/// `TVITEMW` de 32 bits: 10 campos de 4 bytes.
const TVITEM32_LEN: usize = 40;
const TEXT_CHARS: usize = 512;

fn send(h: HWND, msg: u32, w: usize, l: isize) -> Option<isize> {
    let mut result = 0usize;
    let ok = unsafe { SendMessageTimeoutW(h, msg, WPARAM(w), LPARAM(l), SMTO_ABORTIFHUNG | SMTO_BLOCK, 5000, Some(&mut result)) };
    (ok.0 != 0).then_some(result as isize)
}

/// Memoria reservada en otro proceso; se libera al soltarla.
struct RemoteBuf {
    process: HANDLE,
    addr: *mut c_void,
}

impl RemoteBuf {
    fn new(pid: u32, len: usize) -> Option<Self> {
        let process = unsafe { OpenProcess(PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_QUERY_INFORMATION, false, pid) }.ok()?;
        let addr = unsafe { VirtualAllocEx(process, None, len, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
        if addr.is_null() {
            let _ = unsafe { CloseHandle(process) };
            return None;
        }
        Some(Self { process, addr })
    }

    fn write(&self, offset: usize, bytes: &[u8]) -> bool {
        unsafe { WriteProcessMemory(self.process, self.addr.add(offset), bytes.as_ptr().cast(), bytes.len(), None) }.is_ok()
    }

    fn read(&self, offset: usize, len: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        unsafe { ReadProcessMemory(self.process, self.addr.add(offset), buf.as_mut_ptr().cast(), len, None) }.ok()?;
        Some(buf)
    }

    fn addr32(&self, offset: usize) -> u32 {
        self.addr as usize as u32 + offset as u32
    }
}

impl Drop for RemoteBuf {
    fn drop(&mut self) {
        unsafe {
            let _ = VirtualFreeEx(self.process, self.addr, 0, MEM_RELEASE);
            let _ = CloseHandle(self.process);
        }
    }
}

/// Elemento del árbol (`HTREEITEM` del otro proceso).
pub type TreeItem = isize;

/// Un `SysTreeView32` de la instancia `pid`, buscado por su id de control (el Navigator de proyecto es 2001).
pub struct RemoteTree {
    hwnd: isize,
    pid: u32,
}

impl RemoteTree {
    pub fn find(pid: u32, ctrl_id: i32) -> Option<Self> {
        top_windows_of(pid)
            .into_iter()
            .flat_map(descendants_of)
            .find(|&h| class_of(h).eq_ignore_ascii_case("SysTreeView32") && unsafe { GetDlgCtrlID(h) } == ctrl_id)
            .map(|h| Self { hwnd: h.0 as isize, pid })
    }

    /// Todos los `SysTreeView32` de la instancia: (id de control, visible, textos de sus raíces). Diagnóstico.
    pub fn describe_all(pid: u32) -> Vec<(i32, bool, Vec<String>)> {
        top_windows_of(pid)
            .into_iter()
            .flat_map(descendants_of)
            .filter(|&h| class_of(h).eq_ignore_ascii_case("SysTreeView32"))
            .map(|h| {
                let t = Self { hwnd: h.0 as isize, pid };
                let mut roots = Vec::new();
                let mut cur = t.root();
                while let Some(r) = cur {
                    roots.push(t.text(r).unwrap_or_default());
                    cur = t.next(TVGN_NEXT, r);
                    if roots.len() > 50 {
                        break;
                    }
                }
                (unsafe { GetDlgCtrlID(h) }, unsafe { IsWindowVisible(h) }.as_bool(), roots)
            })
            .collect()
    }

    fn h(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    fn next(&self, code: usize, item: TreeItem) -> Option<TreeItem> {
        send(self.h(), TVM_GETNEXTITEM, code, item).filter(|&i| i != 0)
    }

    pub fn root(&self) -> Option<TreeItem> {
        self.next(TVGN_ROOT, 0)
    }

    pub fn children(&self, item: TreeItem) -> Vec<TreeItem> {
        let mut out = Vec::new();
        let mut cur = self.next(TVGN_CHILD, item);
        while let Some(c) = cur {
            out.push(c);
            if out.len() > 10_000 {
                break;
            }
            cur = self.next(TVGN_NEXT, c);
        }
        out
    }

    pub fn text(&self, item: TreeItem) -> Option<String> {
        let buf = RemoteBuf::new(self.pid, TVITEM32_LEN + TEXT_CHARS * 2)?;
        let mut tv = [0u8; TVITEM32_LEN];
        tv[0..4].copy_from_slice(&TVIF_TEXT.to_le_bytes());
        tv[4..8].copy_from_slice(&(item as u32).to_le_bytes());
        tv[16..20].copy_from_slice(&buf.addr32(TVITEM32_LEN).to_le_bytes());
        tv[20..24].copy_from_slice(&(TEXT_CHARS as u32).to_le_bytes());
        if !buf.write(0, &tv) || send(self.h(), TVM_GETITEMW, 0, buf.addr as isize)? == 0 {
            return None;
        }
        let bytes = buf.read(TVITEM32_LEN, TEXT_CHARS * 2)?;
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
        Some(String::from_utf16_lossy(&units))
    }

    pub fn select(&self, item: TreeItem) -> bool {
        send(self.h(), TVM_SELECTITEM, TVGN_CARET, item).is_some_and(|r| r != 0)
    }

    /// Renombra como al editar la etiqueta: `TVM_EDITLABEL` → texto → `TVM_ENDEDITLABELNOW`. El Navigator
    /// recibe `TVN_ENDLABELEDIT` y aplica el nombre (o lo rechaza y deja el anterior). `TVM_EDITLABEL` sólo abre
    /// la edición si el árbol tiene el foco (con un panel recién abierto lo tiene el editor y devolvía 0).
    pub fn rename(&self, item: TreeItem, name: &str) -> bool {
        if !self.focus(item) {
            return false;
        }
        let Some(edit) = send(self.h(), TVM_EDITLABELW, 0, item).filter(|&e| e != 0) else { return false };
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        send(HWND(edit as *mut _), WM_SETTEXT, 0, wide.as_ptr() as isize).is_some() && send(self.h(), TVM_ENDEDITLABELNOW, 0, 0).is_some()
    }

    /// Despliega `item` (el Navigator rellena algunos nodos al desplegarlos).
    pub fn expand(&self, item: TreeItem) -> bool {
        send(self.h(), TVM_EXPAND, TVE_EXPAND, item).is_some()
    }

    /// Selecciona `item`, da el foco al árbol y pulsa Supr, como el usuario (el Navigator abre su
    /// confirmación; `WM_COMMAND` Delete al marco no llega porque el foco está en el editor de paneles).
    pub fn press_delete(&self, item: TreeItem) -> bool {
        self.press_key(item, VK_DELETE.0, 0x53)
    }

    /// Selecciona `item`, da el foco al árbol y pulsa Intro (abre el panel, como el doble clic).
    pub fn press_enter(&self, item: TreeItem) -> bool {
        self.press_key(item, VK_RETURN.0, 0x1C)
    }

    /// Selecciona `item` y da el foco al árbol, como al hacer clic en él: los comandos de menú que dependen de
    /// lo seleccionado en el Navigator (Import Variables) sólo están activos con el foco ahí.
    pub fn focus(&self, item: TreeItem) -> bool {
        if !self.select(item) {
            return false;
        }
        let theirs = unsafe { GetWindowThreadProcessId(self.h(), None) };
        let mine = unsafe { GetCurrentThreadId() };
        if theirs == 0 || !unsafe { AttachThreadInput(mine, theirs, true) }.as_bool() {
            return false;
        }
        let _ = unsafe { SetFocus(Some(self.h())) };
        let _ = unsafe { AttachThreadInput(mine, theirs, false) };
        true
    }

    /// `WM_KEYDOWN`/`WM_KEYUP` de la tecla `vk` (código de exploración `scan`) con el foco en el árbol.
    fn press_key(&self, item: TreeItem, vk: u16, scan: u32) -> bool {
        if !self.focus(item) {
            return false;
        }
        let w = WPARAM(vk as usize);
        let extended = if vk == VK_DELETE.0 { 0x0100_0000 } else { 0 };
        let down = 1 | (scan << 16) | extended;
        let up = down | 0xC000_0000;
        unsafe { PostMessageW(Some(self.h()), WM_KEYDOWN, w, LPARAM(down as isize)).is_ok() && PostMessageW(Some(self.h()), WM_KEYUP, w, LPARAM(up as isize)).is_ok() }
    }

    /// Elemento seleccionado.
    pub fn caret(&self) -> Option<TreeItem> {
        self.next(TVGN_CARET, 0)
    }
}
