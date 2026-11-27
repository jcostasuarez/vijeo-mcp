//! Portapapeles de Windows para pasar objetos a Vijeo y leerlos (inventario §13.1).
//!
//! Vijeo pone sus formatos con el portapapeles OLE y renderizado diferido: `GetClipboardData` devuelve 0
//! y hay que pedirlos con `OleGetClipboard` + `IDataObject::GetData`, que necesita un hilo STA con OLE
//! iniciado. Para dárselos basta `SetClipboardData` con el formato registrado (un HGLOBAL).
//!
//! El portapapeles es del usuario: [`TextBackup`] guarda su texto y lo devuelve al terminar (el resto de
//! formatos de otras aplicaciones no se puede conservar).

use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{OleGetClipboard, OleInitialize, OleUninitialize, ReleaseStgMedium, CF_UNICODETEXT};

/// Id del formato registrado `name` (el mismo en todos los procesos de la sesión).
pub fn format(name: &str) -> u32 {
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) }
}

/// Cambia cada vez que alguien escribe en el portapapeles.
pub fn sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

pub fn has_format(fmt: u32) -> bool {
    unsafe { IsClipboardFormatAvailable(fmt) }.is_ok()
}

/// Abre el portapapeles; otro proceso puede tenerlo abierto un momento, así que se reintenta.
struct Open;

impl Open {
    fn new() -> Result<Self, String> {
        let start = Instant::now();
        loop {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(Open);
            }
            if start.elapsed() > Duration::from_secs(3) {
                return Err("no se pudo abrir el portapapeles (lo tiene abierto otro programa)".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

fn global_from(bytes: &[u8]) -> Result<HGLOBAL, String> {
    let h = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) }.map_err(|e| format!("GlobalAlloc: {e}"))?;
    let p = unsafe { GlobalLock(h) };
    if p.is_null() {
        return Err("GlobalLock devolvió nulo".into());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(h);
    }
    Ok(h)
}

fn global_bytes(h: HGLOBAL) -> Option<Vec<u8>> {
    let n = unsafe { GlobalSize(h) };
    let p = unsafe { GlobalLock(h) };
    if p.is_null() {
        return None;
    }
    let v = unsafe { std::slice::from_raw_parts(p.cast::<u8>(), n) }.to_vec();
    unsafe {
        let _ = GlobalUnlock(h);
    }
    Some(v)
}

/// Vacía el portapapeles y deja `bytes` con el formato `fmt`.
pub fn set(fmt: u32, bytes: &[u8]) -> Result<(), String> {
    let h = global_from(bytes)?;
    let _open = Open::new()?;
    unsafe { EmptyClipboard() }.map_err(|e| format!("EmptyClipboard: {e}"))?;
    unsafe { SetClipboardData(fmt, Some(HANDLE(h.0))) }.map_err(|e| format!("SetClipboardData: {e}"))?;
    Ok(())
}

/// Vacía el portapapeles.
pub fn clear() -> Result<(), String> {
    let _open = Open::new()?;
    unsafe { EmptyClipboard() }.map_err(|e| format!("EmptyClipboard: {e}"))
}

/// Lee el formato `fmt` con el portapapeles OLE (en un hilo STA propio). `None` si no está.
pub fn get_ole(fmt: u32) -> Result<Option<Vec<u8>>, String> {
    std::thread::spawn(move || -> Result<Option<Vec<u8>>, String> {
        unsafe { OleInitialize(None) }.map_err(|e| format!("OleInitialize: {e}"))?;
        let r = (|| {
            let data = unsafe { OleGetClipboard() }.map_err(|e| format!("OleGetClipboard: {e}"))?;
            let fe = FORMATETC { cfFormat: fmt as u16, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
            let Ok(mut medium) = (unsafe { data.GetData(&fe) }) else { return Ok(None) };
            let bytes = if medium.tymed == TYMED_HGLOBAL.0 as u32 { global_bytes(unsafe { medium.u.hGlobal }) } else { None };
            unsafe { ReleaseStgMedium(&mut medium) };
            Ok(bytes)
        })();
        unsafe { OleUninitialize() };
        r
    })
    .join()
    .map_err(|_| "el hilo del portapapeles OLE terminó con un pánico".to_string())?
}

/// Texto Unicode del portapapeles.
pub fn get_text() -> Option<String> {
    let _open = Open::new().ok()?;
    let h = unsafe { GetClipboardData(CF_UNICODETEXT.0 as u32) }.ok()?;
    let bytes = global_bytes(HGLOBAL(h.0))?;
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
    Some(String::from_utf16_lossy(&units))
}

pub fn set_text(text: &str) -> Result<(), String> {
    let bytes: Vec<u8> = text.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()).collect();
    set(CF_UNICODETEXT.0 as u32, &bytes)
}

/// Texto que tenía el portapapeles; [`TextBackup::restore`] (o al soltarlo) lo vuelve a poner.
pub struct TextBackup {
    text: Option<String>,
    done: bool,
}

impl TextBackup {
    pub fn take() -> Self {
        Self { text: get_text(), done: false }
    }

    pub fn restore(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        let r = match &self.text {
            Some(t) => set_text(t),
            None => clear(),
        };
        if let Err(e) = r {
            tracing::warn!("no se pudo devolver el portapapeles del usuario: {e}");
        }
    }
}

impl Drop for TextBackup {
    fn drop(&mut self) {
        self.restore();
    }
}
