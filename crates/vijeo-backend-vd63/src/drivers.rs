//! Drivers de comunicación instalados: `Vijeo-Frame\HMI IO\<id>_PDL.dll`.
//!
//! Los nombres comerciales viven en tablas de strings de `<id>_PDL_Lang.<idioma>` con IDs distintos
//! en cada driver, así que fabricante, protocolo y medio se deducen del nombre del fichero.

use std::path::Path;

use vijeo_core::{DriverFilter, DriverInfo, VijeoError, VijeoResult};

/// Prefijo del fichero → fabricante. Se prueban en orden: los más largos primero.
const MANUFACTURERS: &[(&str, &str)] = &[
    ("GE_Fanuc_Automation_", "GE Fanuc Automation"),
    ("Rockwell_AB_", "Rockwell Automation (Allen-Bradley)"),
    ("Schneider_", "Schneider Electric"),
    ("Magelis_", "Schneider Electric"),
    ("Siemens_", "Siemens"),
    ("Siemens", "Siemens"),
    ("Mitsubishi_", "Mitsubishi Electric"),
    ("Omron_", "Omron"),
    ("Toyoda_", "Toyoda Machine Works"),
    ("Panasonic_", "Panasonic"),
    ("ABB_", "ABB"),
    ("Emerson_", "Emerson Process Management"),
    ("Generic_", "Generic"),
    ("FieldBus_", "Fieldbus"),
];

/// Medio físico según los trozos del nombre (en minúsculas).
fn transport(tokens: &[String]) -> Option<&'static str> {
    let any = |set: &[&str]| tokens.iter().any(|t| set.contains(&t.as_str()));
    if tokens.iter().any(|t| t.ends_with("usb")) {
        Some("usb")
    } else if any(&["tcp", "udp", "ethernet", "isoontcp", "tiaeontcp", "xway", "srtp", "ipnative", "ip", "505ethernet"]) {
        Some("ethernet")
    } else if any(&["sio", "df1", "dh485", "rk512", "mpi", "mpid", "unitelway", "jbus", "snpx", "comli"]) {
        Some("serial")
    } else if any(&["fipio", "fipway", "modbusplus", "devicenet", "profibus"]) {
        Some("fieldbus")
    } else {
        None
    }
}

/// Describe un driver a partir de su id (`Schneider_Modbus_TCP`).
pub fn describe(id: &str, file: &str) -> DriverInfo {
    let (manufacturer, rest) = MANUFACTURERS
        .iter()
        .find_map(|(prefix, m)| {
            let head = id.get(..prefix.len())?;
            head.eq_ignore_ascii_case(prefix).then(|| (m.to_string(), &id[prefix.len()..]))
        })
        .unwrap_or_else(|| match id.split_once('_') {
            Some((m, r)) => (m.to_owned(), r),
            None => (id.to_owned(), ""),
        });
    let tokens: Vec<String> = rest.split('_').map(str::to_ascii_lowercase).collect();
    DriverInfo {
        id: id.to_owned(),
        file: file.to_owned(),
        manufacturer,
        protocol: rest.replace('_', " ").trim().to_owned(),
        transport: transport(&tokens).map(str::to_owned),
    }
}

/// Lista los `*_PDL.dll` de `frame_dir\HMI IO`, ordenados por id.
pub fn load(frame_dir: &Path) -> VijeoResult<Vec<DriverInfo>> {
    let dir = frame_dir.join("HMI IO");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| VijeoError::io(&dir, e))? {
        let file = entry.map_err(|e| VijeoError::io(&dir, e))?.file_name().to_string_lossy().into_owned();
        let lower = file.to_ascii_lowercase();
        if let Some(stem) = lower.strip_suffix("_pdl.dll") {
            out.push(describe(&file[..stem.len()], &file));
        }
    }
    out.sort_by_key(|d| d.id.to_ascii_lowercase());
    Ok(out)
}

pub fn filter(drivers: &[DriverInfo], f: &DriverFilter) -> Vec<DriverInfo> {
    let norm = |s: &str| s.trim().to_ascii_lowercase();
    let man = f.manufacturer.as_deref().map(norm);
    let tr = f.transport.as_deref().map(norm);
    let txt = f.text.as_deref().map(norm);
    drivers
        .iter()
        .filter(|d| man.as_ref().is_none_or(|m| norm(&d.manufacturer).contains(m.as_str())))
        .filter(|d| tr.as_ref().is_none_or(|t| d.transport.as_deref() == Some(t.as_str())))
        .filter(|d| {
            txt.as_ref().is_none_or(|t| [&d.id, &d.manufacturer, &d.protocol].iter().any(|s| norm(s).contains(t.as_str())))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_names() {
        let d = describe("Schneider_Modbus_TCP", "Schneider_Modbus_TCP_PDL.dll");
        assert_eq!((d.manufacturer.as_str(), d.protocol.as_str(), d.transport.as_deref()), ("Schneider Electric", "Modbus TCP", Some("ethernet")));
        let d = describe("GE_Fanuc_Automation_SNPX_Master", "x");
        assert_eq!((d.manufacturer.as_str(), d.protocol.as_str(), d.transport.as_deref()), ("GE Fanuc Automation", "SNPX Master", Some("serial")));
        let d = describe("Siemens505Ethernet", "x");
        assert_eq!((d.manufacturer.as_str(), d.protocol.as_str(), d.transport.as_deref()), ("Siemens", "505Ethernet", Some("ethernet")));
        assert_eq!(describe("Generic_BarcodeScannerUSB", "x").transport.as_deref(), Some("usb"));
        assert_eq!(describe("Schneider_ModBusPlus_GMU", "x").transport.as_deref(), Some("fieldbus"));
        assert_eq!(describe("Schneider_OMS", "x").transport, None);
        let d = describe("Acme_Thing", "x");
        assert_eq!((d.manufacturer.as_str(), d.protocol.as_str()), ("Acme", "Thing"));
    }

    #[test]
    fn filters() {
        let all = vec![describe("Schneider_Modbus_TCP", "a"), describe("Schneider_ModBus_SIO", "b"), describe("Siemens_TCP", "c")];
        let f = |m: Option<&str>, t: Option<&str>, x: Option<&str>| {
            filter(&all, &DriverFilter { manufacturer: m.map(Into::into), transport: t.map(Into::into), text: x.map(Into::into) }).len()
        };
        assert_eq!(f(Some("schneider"), None, None), 2);
        assert_eq!(f(None, Some("Ethernet"), None), 2);
        assert_eq!(f(None, None, Some("modbus")), 2);
        assert_eq!(f(Some("siemens"), Some("serial"), None), 0);
    }
}
