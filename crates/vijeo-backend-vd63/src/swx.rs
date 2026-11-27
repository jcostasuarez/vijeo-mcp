//! Lectura de la configuración de un proyecto directamente de su `.vdz`, sin Vijeo, y escritura de las IP
//! de descarga y de la dirección de los equipos ([`edit_target`]).
//!
//! El `.vdz` es un ZIP con `<Proyecto>.SwxCF`, un fichero OLE compuesto. Lo que se lee:
//! - `Targets\Target N\PrimaryTarget\Document`: propiedades del target, serializadas con `CArchive`
//!   de MFC (cadenas `FF FE FF <len>` + UTF-16). Las de posición conocida se etiquetan.
//! - `Targets\Target N\Component M` (el que tiene `DriverList`, casi siempre el 2): el I/O Manager. `DriverList` y `DriverK\ProtocolList` son
//!   registros de cadenas `u32 bytes + UTF-16 terminada en 0`; `DriverK\DriverConfig` y
//!   `EquipmentJ\ProtocolConfig` son `u32 versión + u32 bytes + UTF-16`.
//! - `Services\NameServer`: empieza por el nombre del primer target.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;

use vijeo_core::{
    ConfigChange, EquipmentChanges, IoDriverConfig, IoEquipmentConfig, TargetConfig, TargetConfigChanges, TargetDownload, VijeoError, VijeoResult,
};

type Cfb = cfb::CompoundFile<Cursor<Vec<u8>>>;

/// Lee el `.SwxCF` del `.vdz` en memoria.
pub fn open_vdz(vdz: &Path) -> VijeoResult<Cfb> {
    let file = std::fs::File::open(vdz).map_err(|e| VijeoError::io(vdz, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| VijeoError::io(vdz, format!("no es un .vdz válido: {e}")))?;
    let name = zip
        .file_names()
        .find(|n| !n.contains('/') && n.to_ascii_lowercase().ends_with(".swxcf"))
        .map(str::to_owned)
        .ok_or_else(|| VijeoError::io(vdz, "el .vdz no contiene ningún .SwxCF"))?;
    let mut buf = Vec::new();
    zip.by_name(&name).and_then(|mut f| f.read_to_end(&mut buf).map_err(Into::into)).map_err(|e| VijeoError::io(vdz, e))?;
    cfb::CompoundFile::open(Cursor::new(buf)).map_err(|e| VijeoError::io(vdz, format!("{name} no es un fichero OLE válido: {e}")))
}

/// Nombre del proyecto: el de la entrada `<nombre>.SwxCF` del `.vdz`.
pub fn project_name(vdz: &Path) -> VijeoResult<String> {
    let file = std::fs::File::open(vdz).map_err(|e| VijeoError::io(vdz, e))?;
    let zip = zip::ZipArchive::new(file).map_err(|e| VijeoError::io(vdz, format!("no es un .vdz válido: {e}")))?;
    let name = zip
        .file_names()
        .find(|n| !n.contains('/') && n.to_ascii_lowercase().ends_with(".swxcf"))
        .ok_or_else(|| VijeoError::io(vdz, "el .vdz no contiene ningún .SwxCF"))?;
    Ok(name[..name.len() - ".swxcf".len()].to_owned())
}

/// Lee un `.SwxCF` suelto (el de la carpeta de un proyecto abierto en `Vijeo-Manager`) en memoria.
pub fn open_swx(path: &Path) -> VijeoResult<Cfb> {
    let buf = std::fs::read(path).map_err(|e| VijeoError::io(path, e))?;
    cfb::CompoundFile::open(Cursor::new(buf)).map_err(|e| VijeoError::io(path, format!("no es un fichero OLE válido: {e}")))
}

pub fn stream(c: &mut Cfb, path: &str) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    c.open_stream(path).ok()?.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Reemplaza el contenido de un stream (con la longitud exacta de `bytes`).
pub fn write_stream(c: &mut Cfb, path: &str, bytes: &[u8]) -> VijeoResult<()> {
    use std::io::Write;
    let mut s = c.create_stream(path).map_err(|e| VijeoError::Internal(format!("no se pudo escribir {path}: {e}")))?;
    s.write_all(bytes).map_err(|e| VijeoError::Internal(format!("no se pudo escribir {path}: {e}")))
}

/// Todos los streams del fichero: (ruta, contenido).
pub fn all_streams(c: &mut Cfb) -> Vec<(String, Vec<u8>)> {
    let paths: Vec<String> = c.walk().filter(|e| e.is_stream()).map(|e| e.path().to_string_lossy().into_owned()).collect();
    paths.into_iter().filter_map(|p| stream(c, &p).map(|b| (p, b))).collect()
}

/// Subcarpetas de `path` cuyo nombre es `<prefijo><número>`, ordenadas por número.
fn numbered(c: &Cfb, path: &str, prefix: &str) -> Vec<String> {
    let Ok(entries) = c.read_storage(path) else { return Vec::new() };
    let mut v: Vec<(u32, String)> = entries
        .filter(|e| e.is_storage())
        .filter_map(|e| Some((e.name().strip_prefix(prefix)?.trim().parse().ok()?, e.name().to_owned())))
        .collect();
    v.sort();
    v.into_iter().map(|(_, n)| n).collect()
}

fn substorages(c: &Cfb, path: &str) -> Vec<String> {
    c.read_storage(path).map(|es| es.filter(|e| e.is_storage()).map(|e| e.name().to_owned()).collect()).unwrap_or_default()
}

fn utf16(b: &[u8]) -> Option<String> {
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16(&units).ok()
}

/// Cadenas `u32 bytes + UTF-16 terminada en 0` (no vacías) que aparecen en `b`, en orden.
fn len_prefixed_strings(b: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= b.len() {
        let len = u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        if len >= 4 && len % 2 == 0 && len <= 4096 && i + 4 + len <= b.len() {
            let body = &b[i + 4..i + 4 + len];
            if body[len - 2..] == [0, 0] {
                if let Some(s) = utf16(&body[..len - 2]) {
                    if !s.is_empty() && s.chars().all(|c| !c.is_control()) {
                        out.push(s);
                        i += 4 + len;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// `u32 versión + u32 bytes + UTF-16`.
fn versioned_text(b: &[u8]) -> Option<String> {
    let len = u32::from_le_bytes(b.get(4..8)?.try_into().ok()?) as usize;
    let s = utf16(b.get(8..8 + len)?)?;
    Some(s.trim_end_matches('\0').to_owned())
}

/// Cadenas `CString` de MFC (`FF FE FF` + longitud en caracteres + UTF-16) con la posición donde terminan.
fn mfc_strings(b: &[u8]) -> Vec<(String, usize)> {
    mfc_spans(b).into_iter().map(|(s, _, end)| (s, end)).collect()
}

/// Como [`mfc_strings`], con la posición donde empieza cada una (el `FF FE FF`).
fn mfc_spans(b: &[u8]) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= b.len() {
        if b[i..i + 3] != [0xFF, 0xFE, 0xFF] {
            i += 1;
            continue;
        }
        let (len, start) = match b[i + 3] {
            0xFF => match b.get(i + 4..i + 6).map(|x| u16::from_le_bytes([x[0], x[1]])) {
                Some(0xFFFF) => match b.get(i + 6..i + 10) {
                    Some(x) => (u32::from_le_bytes(x.try_into().unwrap()) as usize, i + 10),
                    None => break,
                },
                Some(n) => (n as usize, i + 6),
                None => break,
            },
            n => (n as usize, i + 4),
        };
        match b.get(start..start + 2 * len).and_then(utf16) {
            Some(s) => {
                out.push((s, i, start + 2 * len));
                i = start + 2 * len;
            }
            None => i += 3,
        }
    }
    out
}

/// `CString` de MFC: `FF FE FF`, la longitud en caracteres (`u8`, o `FF` + `u16`, o `FF FF FF` + `u32`) y UTF-16.
fn mfc_encode(s: &str) -> Vec<u8> {
    let units: Vec<u16> = s.encode_utf16().collect();
    let mut out = vec![0xFF, 0xFE, 0xFF];
    match units.len() {
        n if n < 0xFF => out.push(n as u8),
        n if n < 0xFFFF => {
            out.push(0xFF);
            out.extend((n as u16).to_le_bytes());
        }
        n => {
            out.extend([0xFF, 0xFF, 0xFF]);
            out.extend((n as u32).to_le_bytes());
        }
    }
    out.extend(units.iter().flat_map(|u| u.to_le_bytes()));
    out
}

fn is_ipv4(s: &str) -> bool {
    s.parse::<std::net::Ipv4Addr>().is_ok()
}

fn is_image(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    [".jpg", ".jpeg", ".bmp", ".png", ".gif"].iter().any(|e| l.ends_with(e))
}

/// Propiedades del target. Orden verificado en GTO/STO/GTU: familia, descripción, modelo, IP, máscara,
/// puerta de enlace, ruta de descarga, puerto serie (seguido del baud rate en un `u32`). La resolución
/// son dos `u16` (ancho, alto) 6 bytes después de la puerta de enlace (verificado en 20 proyectos:
/// 320x240, 800x600, 1024x768, 1280x800).
struct TargetProps {
    family: Option<String>,
    description: Option<String>,
    model: Option<String>,
    resolution: Option<String>,
    download: TargetDownload,
    splash_file: Option<String>,
    other_values: Vec<String>,
}

/// Las 8 primeras cadenas del `Document` son las propiedades de posición conocida ([`TargetProps`]). Los
/// iPC guardan la familia con su nombre comercial (`iPC Series`) en vez de `Target_*`: ahí se reconocen por
/// el código del modelo (`Model_IPC_RACK_RSO`, o `TARGET_HMI_P6` en el Harmony P6; verificado con --probe-models).
fn is_labelled<'a>(texts: impl IntoIterator<Item = &'a String>) -> bool {
    let t: Vec<&String> = texts.into_iter().take(8).collect();
    let model_code = |s: &str| ["model_", "target_"].iter().any(|p| s.to_ascii_lowercase().starts_with(p));
    t.len() == 8
        && (t[0].starts_with("Target_") || (!t[0].is_empty() && model_code(t[2])))
        && t[3..6].iter().all(|s| s.is_empty() || is_ipv4(s))
}

fn target_props(doc: &[u8]) -> TargetProps {
    let strings = mfc_strings(doc);
    let s = |i: usize| strings.get(i).map(|(v, _)| v.clone()).filter(|v| !v.is_empty());
    let labelled = is_labelled(strings.iter().map(|(s, _)| s));
    let mut props =
        TargetProps { family: None, description: None, model: None, resolution: None, download: TargetDownload::default(), splash_file: None, other_values: Vec::new() };
    let mut rest = 0;
    if labelled {
        let baud = doc.get(strings[7].1..strings[7].1 + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap())).filter(|b| (300..=1_000_000).contains(b));
        let at = strings[5].1 + 6;
        props.resolution = doc
            .get(at..at + 4)
            .map(|x| (u16::from_le_bytes([x[0], x[1]]), u16::from_le_bytes([x[2], x[3]])))
            .filter(|(w, h)| (1..=8192).contains(w) && (1..=8192).contains(h))
            .map(|(w, h)| format!("{w}x{h}"));
        props.family = s(0);
        props.description = s(1);
        props.model = s(2);
        props.download = TargetDownload { ip_address: s(3), subnet_mask: s(4), default_gateway: s(5), path: s(6), serial_port: s(7), baud_rate: baud };
        rest = 8;
    }
    for (v, _) in &strings[rest.min(strings.len())..] {
        if v.is_empty() {
            continue;
        }
        if props.splash_file.is_none() && is_image(v) {
            props.splash_file = Some(v.clone());
        } else {
            props.other_values.push(v.clone());
        }
    }
    props
}

/// `[4]T:0:172.20.10.167:502:255:0:...` → (IP, puerto, unit id si es Modbus TCP).
fn equipment_address(address: &str, protocol: Option<&str>) -> (Option<String>, Option<u16>, Option<u8>) {
    let body = address.split_once(']').map(|(_, r)| r).unwrap_or(address);
    let fields: Vec<&str> = body.split(':').map(str::trim).collect();
    let Some(ip_at) = fields.iter().position(|f| is_ipv4(f)) else { return (None, None, None) };
    let port = fields.get(ip_at + 1).and_then(|p| p.parse().ok());
    let unit = port.and(is_modbus_tcp(protocol).then(|| fields.get(ip_at + 2).and_then(|u| u.parse().ok())).flatten());
    (Some(fields[ip_at].to_owned()), port, unit)
}

fn is_modbus_tcp(protocol: Option<&str>) -> bool {
    protocol.is_some_and(|p| {
        let p = p.to_ascii_lowercase();
        p.contains("modbus") && p.contains("tcp")
    })
}

/// `address` con otra IP y, sólo en Modbus TCP, otro puerto o unit ID (los dos campos que siguen a la IP).
/// Los demás campos se dejan como están.
fn edit_equipment_address(address: &str, modbus_tcp: bool, ip: Option<&str>, port: Option<u16>, unit: Option<u8>) -> Result<String, String> {
    let (prefix, body) = match address.split_once(']') {
        Some((p, rest)) => (&address[..p.len() + 1], rest),
        None => ("", address),
    };
    let mut fields: Vec<String> = body.split(':').map(str::to_owned).collect();
    let ip_at = fields.iter().position(|f| is_ipv4(f.trim())).ok_or("la configuración del equipo no tiene ninguna IPv4")?;
    if (port.is_some() || unit.is_some()) && !modbus_tcp {
        return Err("port y unit_id sólo se pueden cambiar en equipos Modbus TCP".into());
    }
    let mut set = |at: usize, what: &str, value: String, ok: fn(&str) -> bool| match fields.get_mut(at) {
        Some(f) if ok(f.trim()) => {
            *f = value;
            Ok(())
        }
        _ => Err(format!("la configuración del equipo no tiene {what} detrás de la IP: {address}")),
    };
    if let Some(ip) = ip {
        set(ip_at, "IP", ip.to_owned(), |_| true)?;
    }
    if let Some(p) = port {
        set(ip_at + 1, "puerto", p.to_string(), |f| f.parse::<u16>().is_ok())?;
    }
    if let Some(u) = unit {
        set(ip_at + 2, "unit ID", u.to_string(), |f| f.parse::<u8>().is_ok())?;
    }
    Ok(format!("{prefix}{}", fields.join(":")))
}

/// `ProtocolConfig` con otra cadena: se conservan la versión y lo que sigue a la cadena (modo IEC, segunda
/// cadena y contador, inventario §12.8), y la cadena sigue terminada en 0 si lo estaba.
fn with_protocol_text(b: &[u8], text: &str) -> Option<Vec<u8>> {
    let len = u32_at(b, 4)? as usize;
    let old = b.get(8..8 + len)?;
    let nul = old.ends_with(&[0, 0]);
    let body: Vec<u8> = text.encode_utf16().chain(nul.then_some(0)).flat_map(u16::to_le_bytes).collect();
    let mut out = b[..4].to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out.extend_from_slice(&b[8 + len..]);
    Some(out)
}

/// IPv4 en su forma canónica; con `mask`, además una máscara de bits contiguos.
fn parse_ipv4(field: &str, value: &str, mask: bool) -> VijeoResult<String> {
    let ip: std::net::Ipv4Addr = value.trim().parse().map_err(|_| VijeoError::invalid(field, format!("«{value}» no es una dirección IPv4")))?;
    let bits = u32::from(ip);
    if mask && bits.leading_ones() + bits.trailing_zeros() != 32 {
        return Err(VijeoError::invalid(field, format!("«{value}» no es una máscara de subred")));
    }
    Ok(ip.to_string())
}

/// Posición de la red del target en el `Document`: las primeras tres IPv4 seguidas (IP, puerta de enlace,
/// máscara) detrás de las propiedades conocidas, sin etiquetar y fuera del XML (cadenas 21-23 en el ejemplo
/// Modbus y en SET03). Vijeo valida contra ella la puerta de enlace de descarga: fuera de su red da
/// `1126 [Subnet Mask] is invalid` y `[Default Gateway] is invalid` (verificado: en SET03, un GTU con
/// 192.168.1.125/255.255.0.0, la puerta 10.0.0.1 falla y 192.168.1.1 no). En el STO del ejemplo, de una sola
/// Ethernet, repite la IP de descarga.
fn second_network_at(spans: &[(String, usize, usize)]) -> Option<usize> {
    (8..spans.len().saturating_sub(2)).find(|&k| spans[k].0 != "0.0.0.0" && (k..k + 3).all(|j| is_ipv4(&spans[j].0)))
}

fn same_subnet(a: &str, b: &str, mask: &str) -> bool {
    let n = |s: &str| s.parse::<std::net::Ipv4Addr>().map(u32::from).ok();
    matches!((n(a), n(b), n(mask)), (Some(a), Some(b), Some(m)) if a & m == b & m)
}

/// Resultado de [`edit_target`].
pub struct TargetEdit {
    pub changes: Vec<ConfigChange>,
    /// El target como debería leerse del fichero una vez escrito.
    pub expected: TargetConfig,
}

/// Aplica `changes` al target `t` de `c` (en memoria): las IP de descarga en las cadenas 4.ª a 6.ª del
/// `PrimaryTarget\Document` (y en la red del target si la copia: [`second_network_at`]) y la dirección de cada equipo en
/// su `ProtocolConfig`. Valida todo antes de
/// escribir: si algo falla, `c` queda igual. Los valores que ya coinciden no cuentan como cambio.
///
/// `dual_ethernet`: el modelo tiene ETH2 (catálogo, `DualEthernetSupported`). Entonces la red del target es la ETH2
/// y nunca se toma por una copia de la descarga, aunque tenga la misma IP (el proyecto PROYC, un HMIG3U).
pub fn edit_target(c: &mut Cfb, t: &TargetConfig, changes: &TargetConfigChanges, dual_ethernet: bool) -> VijeoResult<TargetEdit> {
    let base = format!("/Targets/{}", t.storage);
    let mut out = Vec::new();
    let mut expected = t.clone();
    let mut writes: Vec<(String, Vec<u8>)> = Vec::new();

    if let Some(d) = &changes.download {
        let wanted = [(3, "ip_address", &d.ip_address, false), (4, "subnet_mask", &d.subnet_mask, true), (5, "default_gateway", &d.default_gateway, false)];
        let mut values = Vec::new();
        for (i, field, v, mask) in wanted {
            if let Some(v) = v {
                values.push((i, field, parse_ipv4(&format!("download.{field}"), v, mask)?));
            }
        }
        if !values.is_empty() {
            let path = format!("{base}/PrimaryTarget/Document");
            let doc = stream(c, &path).ok_or_else(|| VijeoError::Internal(format!("el target «{}» no tiene {path}", t.name)))?;
            let spans = mfc_spans(&doc);
            if !is_labelled(spans.iter().map(|(s, ..)| s)) {
                return Err(VijeoError::Refused(format!("no se reconoce el formato de las propiedades del target «{}»: no se cambia la IP de descarga", t.name)));
            }
            // (cadena, item, campo, valor)
            let mut repl: Vec<(usize, &str, &str, String)> = Vec::new();
            let mut fin = [spans[3].0.clone(), spans[4].0.clone(), spans[5].0.clone()];
            for (i, field, v) in values {
                fin[i - 3] = v.clone();
                repl.push((i, "download", field, v));
            }
            // La red del target (IP, puerta, máscara): si copia la de descarga se cambia con ella; si no (la ETH2
            // de un modelo con dos Ethernet), se deja, y la puerta tiene que ser de esa red.
            let net = second_network_at(&spans);
            let copy = net.filter(|&k| !dual_ethernet && spans[k].0 == spans[3].0 && spans[3].0 != "0.0.0.0");
            if let Some(k) = copy {
                for (j, field, v) in [(k, "ip_address", &fin[0]), (k + 1, "default_gateway", &fin[2]), (k + 2, "subnet_mask", &fin[1])] {
                    repl.push((j, "ethernet", field, v.clone()));
                }
            }
            let (net_ip, net_mask) = match net.filter(|_| copy.is_none()) {
                Some(k) => (spans[k].0.as_str(), spans[k + 2].0.as_str()),
                None => (fin[0].as_str(), fin[1].as_str()),
            };
            if fin[2] != "0.0.0.0" && !same_subnet(&fin[2], net_ip, net_mask) {
                let why = if copy.is_none() && net.is_some() {
                    format!(
                        "Vijeo valida la puerta de enlace de descarga contra la otra red del target ({net_ip}/{net_mask}, la ETH2 en los modelos con dos Ethernet, que esta herramienta no cambia) y fuera de ella la compilación da 1126 «Subnet Mask/Default Gateway is invalid»"
                    )
                } else {
                    format!("no está en la red {net_ip}/{net_mask}")
                };
                return Err(VijeoError::invalid("download.default_gateway", format!("{}: {why}; usa 0.0.0.0 (sin puerta) o una de esa red", fin[2])));
            }
            repl.retain(|(i, _, _, v)| spans[*i].0 != *v);
            repl.sort_by_key(|r| r.0);
            let mut doc_new = doc.clone();
            // De atrás adelante, para que las posiciones de las cadenas anteriores sigan valiendo.
            for (i, _, _, v) in repl.iter().rev() {
                doc_new.splice(spans[*i].1..spans[*i].2, mfc_encode(v));
            }
            for (i, item, field, v) in repl {
                out.push(ConfigChange { item: item.into(), field: field.into(), before: Some(spans[i].0.clone()).filter(|s| !s.is_empty()), after: v });
            }
            if doc_new != doc {
                let props = target_props(&doc_new);
                (expected.download, expected.other_values) = (props.download, props.other_values);
                writes.push((path, doc_new));
            }
        }
    }

    let io = io_path(c, &base);
    let mut seen = Vec::new();
    for (k, e) in changes.equipment.iter().enumerate() {
        let field = |f: &str| format!("equipment[{k}].{f}");
        if e.ip_address.is_none() && e.port.is_none() && e.unit_id.is_none() {
            return Err(VijeoError::invalid(field("name"), "no indica ningún cambio (ip_address, port o unit_id)"));
        }
        if e.port == Some(0) {
            return Err(VijeoError::invalid(field("port"), "el puerto no puede ser 0"));
        }
        let (di, ei) = find_equipment(t, e).map_err(|r| VijeoError::invalid(field("name"), r))?;
        if seen.contains(&(di, ei)) {
            return Err(VijeoError::invalid(field("name"), format!("el equipo «{}» ya aparece antes en equipment", e.name)));
        }
        seen.push((di, ei));
        let ip = e.ip_address.as_deref().map(|v| parse_ipv4(&field("ip_address"), v, false)).transpose()?;
        let (drv, eq) = (&t.drivers[di], &t.drivers[di].equipment[ei]);
        let path = format!("{io}/{}/{}/ProtocolConfig", drv.storage, eq.storage);
        let raw = stream(c, &path).ok_or_else(|| VijeoError::Refused(format!("el equipo «{}» no tiene ProtocolConfig", eq.name)))?;
        let address = versioned_text(&raw).filter(|s| !s.is_empty()).ok_or_else(|| VijeoError::Refused(format!("el equipo «{}» no tiene dirección guardada", eq.name)))?;
        let new = edit_equipment_address(&address, is_modbus_tcp(eq.protocol_id.as_deref()), ip.as_deref(), e.port, e.unit_id)
            .map_err(|r| VijeoError::invalid(format!("equipment[{k}]"), r))?;
        if new == address {
            continue;
        }
        let bytes = with_protocol_text(&raw, &new).ok_or_else(|| VijeoError::Refused(format!("no se reconoce el formato del ProtocolConfig de «{}»", eq.name)))?;
        writes.push((path, bytes));
        let x = &mut expected.drivers[di].equipment[ei];
        (x.ip_address, x.port, x.unit_id) = equipment_address(&new, x.protocol_id.as_deref());
        x.address = Some(new.clone());
        out.push(ConfigChange { item: format!("{}/{}", drv.name, eq.name), field: "address".into(), before: Some(address), after: new });
    }

    for (path, bytes) in writes {
        use std::io::Write;
        let mut s = c.create_stream(&path).map_err(|e| VijeoError::Internal(format!("no se pudo escribir {path}: {e}")))?;
        s.write_all(&bytes).map_err(|e| VijeoError::Internal(format!("no se pudo escribir {path}: {e}")))?;
    }
    Ok(TargetEdit { changes: out, expected })
}

/// (driver, equipo) del target que corresponde a `e`, por nombre (y driver, si se indica).
fn find_equipment(t: &TargetConfig, e: &EquipmentChanges) -> Result<(usize, usize), String> {
    let name = e.name.trim();
    let found: Vec<(usize, usize)> = t
        .drivers
        .iter()
        .enumerate()
        .filter(|(_, d)| e.driver.as_deref().is_none_or(|n| d.name.eq_ignore_ascii_case(n.trim())))
        .flat_map(|(di, d)| d.equipment.iter().enumerate().filter(|(_, q)| q.name.eq_ignore_ascii_case(name)).map(move |(ei, _)| (di, ei)))
        .collect();
    match found.as_slice() {
        [one] => Ok(*one),
        [] => {
            let all: Vec<String> = t.drivers.iter().flat_map(|d| d.equipment.iter().map(move |q| format!("{}/{}", d.name, q.name))).collect();
            Err(format!("no hay ningún equipo «{name}» en el target «{}» (equipos: {})", t.name, if all.is_empty() { "ninguno".into() } else { all.join(", ") }))
        }
        many => {
            let drivers: Vec<&str> = many.iter().map(|(di, _)| t.drivers[*di].name.as_str()).collect();
            Err(format!("hay {} equipos «{name}» (drivers {}): indica driver", many.len(), drivers.join(", ")))
        }
    }
}

/// Bytes del `.SwxCF` en memoria, con lo escrito.
pub fn into_bytes(mut c: Cfb) -> VijeoResult<Vec<u8>> {
    c.flush().map_err(|e| VijeoError::Internal(format!("no se pudo cerrar el .SwxCF: {e}")))?;
    Ok(c.into_inner().into_inner())
}

fn settings(text: &str) -> BTreeMap<String, String> {
    text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned())).filter(|(k, _)| !k.is_empty()).collect()
}

/// Registros de `DriverList` / `ProtocolList`: cada uno termina con el nombre de su carpeta
/// (`Driver2`, `Equipment1`) precedido de sus otros campos.
fn records(b: &[u8], storage_prefix: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for s in len_prefixed_strings(b) {
        let is_storage = s.strip_prefix(storage_prefix).is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        cur.push(s);
        if is_storage {
            out.push(std::mem::take(&mut cur));
        }
    }
    out
}

/// I/O Manager del target: el `Component N` que tiene `DriverList`. Suele ser el 2, pero no siempre
/// (`Compact_Hardwired_M221.vdz` lo tiene en el 4, con el editor en el 3).
fn io_path(c: &Cfb, base: &str) -> String {
    numbered(c, base, "Component ")
        .into_iter()
        .map(|comp| format!("{base}/{comp}"))
        .find(|p| c.is_stream(format!("{p}/DriverList")))
        .unwrap_or_else(|| format!("{base}/Component 2"))
}

fn drivers(c: &mut Cfb, base: &str) -> Vec<IoDriverConfig> {
    let io = io_path(c, base);
    let list = stream(c, &format!("{io}/DriverList")).map(|b| records(&b, "Driver")).unwrap_or_default();
    let mut out = Vec::new();
    for storage in numbered(c, &io, "Driver") {
        let dpath = format!("{io}/{storage}");
        // [nombre, fabricante, id del driver, carpeta]
        let rec = list.iter().find(|r| r.last() == Some(&storage));
        let field = |i: usize| rec.filter(|r| r.len() >= 4).map(|r| r[r.len() - 4 + i].clone());
        let protocols = stream(c, &format!("{dpath}/ProtocolList")).map(|b| records(&b, "Equipment")).unwrap_or_default();
        let mut equipment = Vec::new();
        for eq in numbered(c, &dpath, "Equipment") {
            let epath = format!("{dpath}/{eq}");
            // [nombre, protocolo, carpeta]
            let rec = protocols.iter().find(|r| r.last() == Some(&eq)).filter(|r| r.len() >= 3);
            let protocol_id = rec.map(|r| r[r.len() - 2].clone());
            let address = stream(c, &format!("{epath}/ProtocolConfig")).and_then(|b| versioned_text(&b)).filter(|s| !s.is_empty());
            let (ip_address, port, unit_id) = address.as_deref().map(|a| equipment_address(a, protocol_id.as_deref())).unwrap_or_default();
            equipment.push(IoEquipmentConfig {
                name: rec.map(|r| r[r.len() - 3].clone()).unwrap_or_else(|| eq.clone()),
                protocol_id,
                storage: eq,
                address,
                ip_address,
                port,
                unit_id,
                scan_groups: substorages(c, &epath),
            });
        }
        out.push(IoDriverConfig {
            name: field(0).unwrap_or_else(|| storage.clone()),
            manufacturer: field(1),
            driver_id: field(2),
            settings: stream(c, &format!("{dpath}/DriverConfig")).and_then(|b| versioned_text(&b)).map(|t| settings(&t)).unwrap_or_default(),
            storage,
            equipment,
        });
    }
    out
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Cadena `u32 bytes + UTF-16 terminada en 0` en `i`: (texto, fin).
fn lp_at(b: &[u8], i: usize) -> Option<(String, usize)> {
    let len = u32_at(b, i)? as usize;
    let end = i + 4 + len;
    let body = b.get(i + 4..end)?;
    if len < 2 || !len.is_multiple_of(2) || body[len - 2..] != [0, 0] {
        return None;
    }
    Some((utf16(&body[..len - 2])?, end))
}

/// `NameServer`: `u16 1, u16 nº de raíces`, los árboles de nodos y un `u32` con el siguiente id libre.
/// Nodo: `u16 1, u32 grupo, u16 tipo, u16 n, n entradas (`u32 id, u32 bytes, UTF-16 terminada en 0`),
/// `u16 nº de hijos`, hijos. Detrás puede quedar basura de un guardado anterior más largo (Vijeo no trunca
/// el stream). Devuelve los nombres de cada nodo en orden, o `None` si no encaja con ese formato.
fn parse_name_server(b: &[u8]) -> Option<Vec<Vec<String>>> {
    fn node(b: &[u8], mut i: usize, depth: usize, out: &mut Vec<Vec<String>>) -> Option<usize> {
        if depth > 64 || u16_at(b, i)? != 1 {
            return None;
        }
        let n = u16_at(b, i + 8)?;
        i += 10;
        let mut names = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let (name, end) = lp_at(b, i + 4)?;
            names.push(name);
            i = end;
        }
        out.push(names);
        let children = u16_at(b, i)?;
        i += 2;
        for _ in 0..children {
            i = node(b, i, depth + 1, out)?;
        }
        Some(i)
    }
    if u16_at(b, 0)? != 1 {
        return None;
    }
    let mut nodes = Vec::new();
    let mut end = 4;
    for _ in 0..u16_at(b, 2)? {
        end = node(b, end, 0, &mut nodes)?;
    }
    u32_at(b, end)?;
    Some(nodes)
}
/// Nombres de los targets: las entradas del primer nodo de `NameServer`, en orden de creación (el del
/// Navigator y el de las carpetas `Target N`; verificado con dos targets cuyo orden alfabético es el
/// contrario).
pub fn target_names(c: &mut Cfb) -> Option<Vec<String>> {
    parse_name_server(&stream(c, "/Services/NameServer")?)?.into_iter().next()
}

/// Configuración de cada target del fichero. `names` son los nombres de los targets en orden de
/// creación, si se conocen; si no, se leen de `NameServer` ([`target_names`]).
pub fn read_targets(c: &mut Cfb, names: Option<&[String]>) -> Vec<TargetConfig> {
    let storages = numbered(c, "/Targets", "Target");
    let stored = target_names(c);
    let names = names.or(stored.as_deref()).filter(|n| n.len() == storages.len());
    let first_name = stream(c, "/Services/NameServer").and_then(|b| len_prefixed_strings(b.get(..1024.min(b.len()))?).into_iter().next());
    let mut out = Vec::new();
    for (i, storage) in storages.iter().enumerate() {
        let base = format!("/Targets/{storage}");
        let name = match names {
            Some(n) => n[i].clone(),
            _ if storages.len() == 1 => first_name.clone().unwrap_or_else(|| storage.clone()),
            _ => storage.clone(),
        };
        let props = target_props(&stream(c, &format!("{base}/PrimaryTarget/Document")).unwrap_or_default());
        out.push(TargetConfig {
            name,
            storage: storage.clone(),
            family: props.family,
            description: props.description,
            model: props.model,
            hmi_type: None,
            resolution: props.resolution,
            download: props.download,
            splash_file: props.splash_file,
            other_values: props.other_values,
            drivers: drivers(c, &base),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lp(s: &str) -> Vec<u8> {
        let mut v: Vec<u8> = s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        let mut out = ((v.len()) as u32).to_le_bytes().to_vec();
        out.append(&mut v);
        out
    }

    fn mfc(s: &str) -> Vec<u8> {
        let mut out = vec![0xFF, 0xFE, 0xFF, s.encode_utf16().count() as u8];
        out.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
        out
    }

    #[test]
    fn driver_list_records() {
        // Cabecera real de DriverList (6 u32 + u16) seguida de un registro.
        let mut b = vec![6, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0x3c, 0, 0, 0, 0x3e, 0, 0, 0, 0x40, 0, 0, 0, 6, 0];
        for s in ["ModbusTCP", "Schneider Electric Industries SAS", "Schneider_Modbus_PDL_TCP", "Driver2"] {
            b.extend(lp(s));
        }
        b.extend([0, 0, 0, 0]);
        assert_eq!(records(&b, "Driver"), vec![vec!["ModbusTCP", "Schneider Electric Industries SAS", "Schneider_Modbus_PDL_TCP", "Driver2"]]);
    }

    #[test]
    fn protocol_list_records() {
        let mut b = vec![0x0a, 0, 0, 0, 2, 0, 0, 0, 0x12, 0];
        for s in ["PROYC_Modbus", "Schneider_Modbus_PCL_TCP", "Equipment2"] {
            b.extend(lp(s));
        }
        b.extend([0, 0, 0, 0]);
        for s in ["VFD_EEMBC01A", "Schneider_Modbus_PCL_TCP", "Equipment3"] {
            b.extend(lp(s));
        }
        let r = records(&b, "Equipment");
        assert_eq!(r.len(), 2);
        assert_eq!(r[1], vec!["VFD_EEMBC01A", "Schneider_Modbus_PCL_TCP", "Equipment3"]);
    }

    #[test]
    fn equipment_addresses() {
        let p = Some("Schneider_Modbus_PCL_TCP");
        assert_eq!(equipment_address("[4]T:0:172.20.10.167:502:255:0:0.0.0.0:120:1:6:3:1", p), (Some("172.20.10.167".into()), Some(502), Some(255)));
        assert_eq!(equipment_address("[4]T:0:10.20.30.82:502:254:1:10.20.30.1:120:1:6:2:1:0", p), (Some("10.20.30.82".into()), Some(502), Some(254)));
        assert_eq!(equipment_address("[4]T:0:10.0.0.1:502:7", Some("Other_TCP")), (Some("10.0.0.1".into()), Some(502), None));
        assert_eq!(equipment_address("[2]S:1:9600", p), (None, None, None));
    }

    #[test]
    fn driver_settings() {
        let s = settings("TXWait=3\r\nBaudRate=19200\r\nDLMName=Modbus.dlm\r\n");
        assert_eq!(s.get("BaudRate").map(String::as_str), Some("19200"));
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn ipc_documents_are_labelled_by_model() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        // Cadenas de un HMIRSO (Model_IPC_RACK_RSO) recién creado.
        assert!(is_labelled(&v(&["iPC Series", "", "Model_IPC_RACK_RSO", "0.0.0.0", "255.255.0.0", "0.0.0.0", "", "COM1"])));
        assert!(is_labelled(&v(&["iPC Series", "", "TARGET_HMI_P6", "0.0.0.0", "255.255.0.0", "0.0.0.0", "", "COM1"])), "Harmony P6");
        assert!(!is_labelled(&v(&["iPC Series", "Model_IPC_RACK_RSO", "0.0.0.0", "255.255.0.0", "0.0.0.0", "COM1", "", ""])), "sin descripción no encaja");
        assert!(!is_labelled(&v(&["Algo", "", "Otra cosa", "0.0.0.0", "", "", "", ""])));
    }

    #[test]
    fn target_document() {
        // Forma de PrimaryTarget\Document (Packaging_TempCtrl_HW_M221, HMIGTO4310).
        let mut d = vec![0x4d, 0, 0, 0];
        d.extend(mfc("Target_HMIGTO"));
        d.extend(mfc(""));
        d.extend([7, 0]);
        d.extend(mfc("Model_HMIGTO4310"));
        d.extend([2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0]);
        for s in ["192.168.100.10", "255.255.255.0", "0.0.0.0"] {
            d.extend(mfc(s));
        }
        // ProyectoB (HMIGTO6310): 800x600.
        d.extend([1, 0, 0, 0, 0, 0, 0x20, 3, 0x58, 2, 0x70, 0x17, 0, 0]);
        d.extend(mfc("G:\\"));
        d.extend([0; 12]);
        d.extend(mfc("COM1"));
        d.extend([0x80, 0x25, 0, 0, 8, 0, 0, 0]);
        d.extend(mfc("splash.jpg"));
        d.extend(mfc("pool.ntp.org"));
        let p = target_props(&d);
        assert_eq!(p.family.as_deref(), Some("Target_HMIGTO"));
        assert_eq!(p.description, None);
        assert_eq!(p.model.as_deref(), Some("Model_HMIGTO4310"));
        assert_eq!(p.resolution.as_deref(), Some("800x600"));
        assert_eq!(p.download.ip_address.as_deref(), Some("192.168.100.10"));
        assert_eq!(p.download.subnet_mask.as_deref(), Some("255.255.255.0"));
        assert_eq!(p.download.path.as_deref(), Some("G:\\"));
        assert_eq!(p.download.serial_port.as_deref(), Some("COM1"));
        assert_eq!(p.download.baud_rate, Some(9600));
        assert_eq!(p.splash_file.as_deref(), Some("splash.jpg"));
        assert_eq!(p.other_values, vec!["pool.ntp.org"]);
    }

    #[test]
    fn mfc_encoding_round_trips() {
        let long = "x".repeat(300);
        for s in ["", "10.0.0.1", "Subestaciónes", long.as_str()] {
            let mut b = mfc_encode(s);
            b.extend([1, 2, 3]);
            assert_eq!(mfc_spans(&b), vec![(s.to_owned(), 0, b.len() - 3)], "{}", s.len());
        }
    }

    #[test]
    fn edits_equipment_addresses() {
        let a = "[4]T:0:10.0.0.101:502:255:0:0.0.0.0:120:1:6:3:0:0:0";
        assert_eq!(edit_equipment_address(a, true, Some("10.0.0.7"), None, None).unwrap(), "[4]T:0:10.0.0.7:502:255:0:0.0.0.0:120:1:6:3:0:0:0");
        assert_eq!(edit_equipment_address(a, true, None, Some(5020), Some(1)).unwrap(), "[4]T:0:10.0.0.101:5020:1:0:0.0.0.0:120:1:6:3:0:0:0");
        assert!(edit_equipment_address(a, false, None, Some(502), None).is_err(), "puerto fuera de Modbus TCP");
        assert_eq!(edit_equipment_address("[2]X:10.0.0.1:7", false, Some("10.0.0.2"), None, None).unwrap(), "[2]X:10.0.0.2:7");
        assert!(edit_equipment_address("[2]S:1:9600", true, Some("10.0.0.2"), None, None).is_err(), "sin IP");
        assert!(edit_equipment_address("[4]T:0:10.0.0.1", true, None, Some(502), None).is_err(), "sin puerto detrás de la IP");
    }

    #[test]
    fn rewrites_protocol_config_keeping_the_tail() {
        // Forma real (SET03, Equipment2): versión 8, cadena con su 0, modo IEC 3, cadena vacía y contador.
        let mut b = vec![8, 0, 0, 0];
        b.extend(lp("[4]T:0:10.0.0.101:502:255"));
        let tail = [3, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        b.extend(tail);
        let n = with_protocol_text(&b, "[4]T:0:192.168.10.20:502:1").unwrap();
        assert_eq!(versioned_text(&n).as_deref(), Some("[4]T:0:192.168.10.20:502:1"));
        assert_eq!(&n[..4], &[8, 0, 0, 0]);
        assert!(n.ends_with(&tail));
        assert_eq!(n.len(), b.len() + 2, "un carácter más (+3 de la IP, −2 del unit ID)");
    }

    #[test]
    fn ipv4_and_masks() {
        assert_eq!(parse_ipv4("f", " 10.0.0.1 ", false).unwrap(), "10.0.0.1");
        assert!(parse_ipv4("f", "10.0.0", false).is_err());
        assert!(parse_ipv4("f", "10.0.0.256", false).is_err());
        assert!(parse_ipv4("f", "255.255.252.0", true).is_ok());
        assert!(parse_ipv4("f", "255.0.255.0", true).is_err());
    }

    /// `.SwxCF` mínimo con un target (propiedades de descarga y luego `tail`) y un driver Modbus TCP con dos equipos.
    fn sample_swx(tail: &[&str]) -> Cfb {
        use std::io::Write;
        let mut c = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        let mut doc = vec![0x53, 0, 0, 0];
        for s in ["Target_HMIGTU", "Subestaciónes"] {
            doc.extend(mfc(s));
        }
        doc.extend([3, 0]);
        doc.extend(mfc("Model_HMIG3U"));
        doc.extend([0x2d, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0]);
        for s in ["10.0.0.125", "255.255.255.0", "0.0.0.0"] {
            doc.extend(mfc(s));
        }
        doc.extend([1, 0, 0, 0, 0, 0, 0, 4, 0, 3, 0x70, 0x17, 0, 0]);
        for s in ["E:\\", "COM2"] {
            doc.extend(mfc(s));
        }
        doc.extend([0x80, 0x25, 0, 0]);
        for s in tail {
            doc.extend(mfc(s));
            doc.extend([0, 0]);
        }
        let io = "/Targets/Target 1/Component 2";
        c.create_storage_all(format!("{io}/Driver4/Equipment2/ScanGroup1")).unwrap();
        c.create_storage_all(format!("{io}/Driver4/Equipment3")).unwrap();
        c.create_storage_all("/Targets/Target 1/PrimaryTarget").unwrap();
        let mut put = |p: &str, b: &[u8]| c.create_stream(p).unwrap().write_all(b).unwrap();
        put("/Targets/Target 1/PrimaryTarget/Document", &doc);
        let mut dl = vec![0; 8];
        for s in ["MODBUS_TCP", "Schneider Electric Industries SAS", "Schneider_Modbus_PDL_TCP", "Driver4"] {
            dl.extend(lp(s));
        }
        put(&format!("{io}/DriverList"), &dl);
        let mut pl = vec![0; 4];
        for s in ["PLC_MT", "Schneider_Modbus_PCL_TCP", "Equipment2", "PM3", "Schneider_Modbus_PCL_TCP", "Equipment3"] {
            pl.extend(lp(s));
        }
        put(&format!("{io}/Driver4/ProtocolList"), &pl);
        for (eq, addr) in [("Equipment2", "[4]T:0:10.0.0.101:502:255:0:0.0.0.0:120:1:6:3:0:0:0"), ("Equipment3", "[4]T:0:10.0.0.151:502:2:0:0.0.0.0:120:1:4:3:1:0:0")] {
            let mut b = vec![8, 0, 0, 0];
            b.extend(lp(addr));
            b.extend([3, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            put(&format!("{io}/Driver4/{eq}/ProtocolConfig"), &b);
        }
        let bytes = into_bytes(c).unwrap();
        cfb::CompoundFile::open(Cursor::new(bytes)).unwrap()
    }

    #[test]
    fn edits_a_target_and_reads_back_what_was_expected() {
        // Como un GTU: detrás, la segunda Ethernet, con otra IP, que no se toca.
        let mut c = sample_swx(&["0.0.0.0", "192.168.1.125", "0.0.0.0", "255.255.0.0", "0.0.0.0"]);
        let before = read_targets(&mut c, None).remove(0);
        assert_eq!(before.download.ip_address.as_deref(), Some("10.0.0.125"));
        let changes = TargetConfigChanges {
            download: Some(vijeo_core::DownloadChanges { ip_address: Some("192.168.100.200".into()), subnet_mask: Some("255.255.255.0".into()), default_gateway: Some("192.168.100.1".into()) }),
            equipment: vec![
                EquipmentChanges { name: "plc_mt".into(), ip_address: Some("192.168.100.10".into()), unit_id: Some(1), ..Default::default() },
                EquipmentChanges { name: "PM3".into(), port: Some(502), ..Default::default() },
            ],
        };
        let edit = edit_target(&mut c, &before, &changes, true).unwrap();
        // La máscara ya era esa y PM3 ya usaba el 502: no cuentan.
        let fields: Vec<(&str, &str)> = edit.changes.iter().map(|x| (x.item.as_str(), x.field.as_str())).collect();
        assert_eq!(fields, [("download", "ip_address"), ("download", "default_gateway"), ("MODBUS_TCP/PLC_MT", "address")]);
        let mut c = cfb::CompoundFile::open(Cursor::new(into_bytes(c).unwrap())).unwrap();
        let after = read_targets(&mut c, None).remove(0);
        assert_eq!(serde_json::to_value(&after).unwrap(), serde_json::to_value(&edit.expected).unwrap());
        assert_eq!(after.download.ip_address.as_deref(), Some("192.168.100.200"));
        assert_eq!(after.download.default_gateway.as_deref(), Some("192.168.100.1"));
        assert_eq!(after.download.path.as_deref(), Some("E:\\"), "lo que sigue no se mueve");
        assert_eq!(after.download.baud_rate, Some(9600));
        assert_eq!(after.resolution, before.resolution);
        assert_eq!(after.other_values, before.other_values);
        let eq = &after.drivers[0].equipment[0];
        assert_eq!((eq.ip_address.as_deref(), eq.port, eq.unit_id), (Some("192.168.100.10"), Some(502), Some(1)));
        assert_eq!(after.drivers[0].equipment[1].address, before.drivers[0].equipment[1].address);

        // Errores: no se escribe nada.
        for bad in [
            EquipmentChanges { name: "NoExiste".into(), ip_address: Some("10.0.0.1".into()), ..Default::default() },
            EquipmentChanges { name: "PM3".into(), ..Default::default() },
            EquipmentChanges { name: "PM3".into(), ip_address: Some("10.0.0.300".into()), ..Default::default() },
        ] {
            let ch = TargetConfigChanges { download: None, equipment: vec![bad] };
            assert!(edit_target(&mut c, &after, &ch, true).is_err());
        }
        let twice = TargetConfigChanges { download: None, equipment: vec![EquipmentChanges { name: "PM3".into(), unit_id: Some(9), ..Default::default() }; 2] };
        assert!(edit_target(&mut c, &after, &twice, true).is_err());
        assert_eq!(serde_json::to_value(read_targets(&mut c, None)).unwrap(), serde_json::to_value([&after]).unwrap());

        // La puerta se valida contra la otra red (192.168.1.125/16), no contra la de descarga.
        let gw = |g: &str| TargetConfigChanges {
            download: Some(vijeo_core::DownloadChanges { default_gateway: Some(g.into()), ..Default::default() }),
            equipment: vec![],
        };
        let err = edit_target(&mut c, &after, &gw("10.0.0.1"), true).err().unwrap().to_string();
        assert!(err.contains("1126") && err.contains("192.168.1.125/255.255.0.0"), "{err}");
        let e = edit_target(&mut c, &after, &gw("192.168.100.254"), true).unwrap();
        assert!(e.changes.iter().all(|x| x.item == "download"), "la otra red no se toca");
    }

    #[test]
    fn keeps_eth2_of_dual_ethernet_targets_with_the_download_ip() {
        // Como PROYC (HMIG3U): la ETH2 tiene la misma IP que la descarga, con otra puerta y otra máscara.
        let tail = ["0.0.0.0", "10.0.0.125", "111.111.111.111", "255.255.0.0", "0.0.0.0", "pool.ntp.org"];
        let mut c = sample_swx(&tail);
        let before = read_targets(&mut c, None).remove(0);
        let dl = TargetConfigChanges {
            download: Some(vijeo_core::DownloadChanges { ip_address: Some("10.0.0.126".into()), ..Default::default() }),
            equipment: vec![],
        };
        let edit = edit_target(&mut c, &before, &dl, true).unwrap();
        let got: Vec<(&str, &str)> = edit.changes.iter().map(|x| (x.item.as_str(), x.field.as_str())).collect();
        assert_eq!(got, [("download", "ip_address")]);
        let mut c = cfb::CompoundFile::open(Cursor::new(into_bytes(c).unwrap())).unwrap();
        let after = read_targets(&mut c, None).remove(0);
        assert_eq!(after.other_values, tail, "la ETH2 queda igual");
        assert_eq!(after.download.ip_address.as_deref(), Some("10.0.0.126"));

        // La puerta se sigue validando contra la ETH2 (10.0.0.125/16).
        let gw = |g: &str| TargetConfigChanges {
            download: Some(vijeo_core::DownloadChanges { default_gateway: Some(g.into()), ..Default::default() }),
            equipment: vec![],
        };
        assert!(edit_target(&mut c, &after, &gw("172.16.0.1"), true).is_err());
        assert!(edit_target(&mut c, &after, &gw("10.0.200.1"), true).is_ok());

        // El mismo fichero en un modelo de una sola Ethernet sí se trata como copia.
        let e = edit_target(&mut sample_swx(&tail), &before, &dl, false).unwrap();
        assert!(e.changes.iter().any(|x| x.item == "ethernet" && x.field == "ip_address"));
    }

    #[test]
    fn updates_the_network_copy_of_single_ethernet_targets() {
        // Como el STO del ejemplo Modbus: la copia (IP, puerta, máscara) repite la de descarga.
        let mut c = sample_swx(&["0.0.0.0", "10.0.0.125", "0.0.0.0", "255.255.255.0", "0.0.0.0", "pool.ntp.org"]);
        let before = read_targets(&mut c, None).remove(0);
        let dl = |ip: &str, mask: Option<&str>| TargetConfigChanges {
            download: Some(vijeo_core::DownloadChanges { ip_address: Some(ip.into()), subnet_mask: mask.map(Into::into), default_gateway: Some("172.16.0.1".into()) }),
            equipment: vec![],
        };
        let edit = edit_target(&mut c, &before, &dl("172.16.0.20", Some("255.255.0.0")), false).unwrap();
        let got: Vec<(&str, &str, &str)> = edit.changes.iter().map(|x| (x.item.as_str(), x.field.as_str(), x.after.as_str())).collect();
        assert_eq!(
            got,
            [
                ("download", "ip_address", "172.16.0.20"),
                ("download", "subnet_mask", "255.255.0.0"),
                ("download", "default_gateway", "172.16.0.1"),
                ("ethernet", "ip_address", "172.16.0.20"),
                ("ethernet", "default_gateway", "172.16.0.1"),
                ("ethernet", "subnet_mask", "255.255.0.0"),
            ]
        );
        let mut c = cfb::CompoundFile::open(Cursor::new(into_bytes(c).unwrap())).unwrap();
        let after = read_targets(&mut c, None).remove(0);
        assert_eq!(after.other_values, ["0.0.0.0", "172.16.0.20", "172.16.0.1", "255.255.0.0", "0.0.0.0", "pool.ntp.org"]);
        assert_eq!(serde_json::to_value(&after).unwrap(), serde_json::to_value(&edit.expected).unwrap());

        // Con la máscara sin cambiar, la copia toma la de descarga actual.
        let edit = edit_target(&mut c, &after, &dl("172.16.0.21", None), false).unwrap();
        let got: Vec<(&str, &str)> = edit.changes.iter().map(|x| (x.item.as_str(), x.field.as_str())).collect();
        assert_eq!(got, [("download", "ip_address"), ("ethernet", "ip_address")]);
    }

    fn ns_node(group: u32, entries: &[(u32, &str)], children: &[Vec<u8>]) -> Vec<u8> {
        let mut b = 1u16.to_le_bytes().to_vec();
        b.extend(group.to_le_bytes());
        b.extend(3u16.to_le_bytes());
        b.extend((entries.len() as u16).to_le_bytes());
        for (id, name) in entries {
            b.extend(id.to_le_bytes());
            b.extend(lp(name));
        }
        b.extend((children.len() as u16).to_le_bytes());
        children.iter().for_each(|ch| b.extend(ch));
        b
    }

    #[test]
    fn parses_name_server() {
        // Dos raíces (la primera con los targets y dos hijos), el siguiente id libre y basura de un guardado anterior.
        let mut ns = vec![1, 0, 2, 0];
        ns.extend(ns_node(1, &[(1, "HMIs"), (2, "Otro")], &[ns_node(0x10, &[(5, "MODBUS")], &[]), ns_node(0x11, &[(6, "PLC")], &[])]));
        ns.extend(ns_node(2, &[], &[]));
        ns.extend(21u32.to_le_bytes());
        ns.extend([0xAB; 6]);
        let nodes = parse_name_server(&ns).unwrap();
        assert_eq!(nodes, [vec!["HMIs".to_owned(), "Otro".to_owned()], vec!["MODBUS".to_owned()], vec!["PLC".to_owned()], vec![]]);
        let mut bad = ns.clone();
        bad[0] = 2;
        assert!(parse_name_server(&bad).is_none());
        assert!(parse_name_server(&ns[..ns.len() - 10]).is_none(), "sin el siguiente id");
    }
}
