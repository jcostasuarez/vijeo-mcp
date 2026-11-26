//! Instalaciones de Vijeo-Designer según el registro:
//! `HKLM\SOFTWARE\(WOW6432Node\)Schneider Electric\Vijeo-Designer*\<NN.NN>`.

use std::path::{Path, PathBuf};

use vijeo_core::{Installation, Version};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO};
use windows::Win32::System::Registry::*;

const VENDOR_KEY: &str = r"SOFTWARE\Schneider Electric";

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// Abre en la vista de 32 bits (Vijeo es x86).
fn open(root: HKEY, path: &str) -> Option<Key> {
    let mut h = HKEY::default();
    let r = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ | KEY_WOW64_32KEY, &mut h) };
    (r == ERROR_SUCCESS).then_some(Key(h))
}

fn subkeys(k: &Key) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        let r = unsafe { RegEnumKeyExW(k.0, i, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) };
        if r != ERROR_SUCCESS {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    out
}

/// Valor `REG_SZ` de `k\sub` (`value = None` → valor por defecto).
fn get_sz(k: &Key, sub: &str, value: Option<&str>) -> Option<String> {
    let sub_h = HSTRING::from(sub);
    let val_h = value.map(HSTRING::from);
    let sub_p = if sub.is_empty() { PCWSTR::null() } else { PCWSTR(sub_h.as_ptr()) };
    let val_p = val_h.as_ref().map(|h| PCWSTR(h.as_ptr())).unwrap_or(PCWSTR::null());
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let mut size = 0u32;
    unsafe {
        if RegGetValueW(k.0, sub_p, val_p, flags, None, None, Some(&mut size)) != ERROR_SUCCESS || size == 0 {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        if RegGetValueW(k.0, sub_p, val_p, flags, None, Some(buf.as_mut_ptr().cast()), Some(&mut size)) != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

/// `FileVersion` fija del ejecutable.
pub(crate) fn file_version(exe: &Path) -> Option<Version> {
    let path = HSTRING::from(exe.as_os_str());
    unsafe {
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, None, size, data.as_mut_ptr().cast()).ok()?;
        let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(data.as_ptr().cast(), &HSTRING::from("\\"), &mut p, &mut len).as_bool() || p.is_null() {
            return None;
        }
        let fi = &*(p as *const VS_FIXEDFILEINFO);
        Some(Version::new(
            (fi.dwFileVersionMS >> 16) as u16,
            (fi.dwFileVersionMS & 0xFFFF) as u16,
            (fi.dwFileVersionLS >> 16) as u16,
            (fi.dwFileVersionLS & 0xFFFF) as u16,
        ))
    }
}

fn is_version_key(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5 && b[2] == b'.' && b.iter().enumerate().all(|(i, c)| i == 2 || c.is_ascii_digit())
}

/// Lista las instalaciones encontradas, de la versión más alta a la más baja.
pub fn discover_installations() -> Vec<Installation> {
    let Some(vendor) = open(HKEY_LOCAL_MACHINE, VENDOR_KEY) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for product in subkeys(&vendor).into_iter().filter(|k| k.starts_with("Vijeo-Designer")) {
        let Some(pk) = open(HKEY_LOCAL_MACHINE, &format!(r"{VENDOR_KEY}\{product}")) else { continue };
        for ver in subkeys(&pk).into_iter().filter(|k| is_version_key(k)) {
            let frame_dir = get_sz(&pk, &format!(r"{ver}\Path"), Some("Path"))
                .map(PathBuf::from)
                .or_else(|| get_sz(&pk, &format!(r"{ver}\Install"), Some("Path")).map(|p| PathBuf::from(p).join("Vijeo-Frame")));
            let Some(frame_dir) = frame_dir else { continue };
            let exe = frame_dir.join("Vijeo-Frame.exe");
            if !exe.is_file() {
                tracing::debug!("{} no existe; se ignora la clave {product}\\{ver}", exe.display());
                continue;
            }
            let product_version = file_version(&exe)
                .or_else(|| get_sz(&pk, &ver, Some("Version")).and_then(|v| Version::parse(&v)))
                .unwrap_or_default();
            out.push(Installation { product_version, frame_dir, registry_key: format!(r"HKLM\{VENDOR_KEY}\{product}\{ver}") });
        }
    }
    out.sort_by_key(|i| std::cmp::Reverse(i.product_version));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_keys() {
        assert!(is_version_key("06.30"));
        assert!(!is_version_key("Vijeo-Frame"));
        assert!(!is_version_key("6.30"));
    }

    /// Informativo: en la VM de desarrollo hay una 6.3 instalada.
    #[test]
    fn discover_does_not_panic() {
        for i in discover_installations() {
            eprintln!("{i:?}");
        }
    }
}
