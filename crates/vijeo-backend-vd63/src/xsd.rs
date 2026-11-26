//! Validación de un XML contra el `public.xsd` de la instalación, sin Vijeo.
//!
//! Se valida con `System.Xml` de .NET (XSD 1.0) desde Windows PowerShell, que está en todo Windows: no hay
//! en Rust un validador XSD completo. El script (`xsd_validate.ps1`) va incrustado y se lanza con
//! `-EncodedCommand`; las rutas van en variables de entorno, así que nunca forman parte del código.
//! `public.xsd` 6.3 no es XSD 1.0 válido en dos sitios: el script los corrige en memoria y lo dice (`relaxed`).

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;
use vijeo_core::{MessageSeverity, VijeoError, VijeoResult, XmlIssue, XmlIssueGroup, XmlValidation};

const SCRIPT: &str = include_str!("xsd_validate.ps1");
const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Deserialize)]
struct RawIssue {
    severity: String,
    #[serde(default)]
    line: u32,
    #[serde(default)]
    column: u32,
    message: String,
}

#[derive(Deserialize)]
struct RawGroup {
    severity: String,
    message: String,
    count: usize,
    #[serde(default)]
    first_line: u32,
}

#[derive(Deserialize)]
struct Raw {
    errors: usize,
    warnings: usize,
    fatal: Option<String>,
    #[serde(default)]
    relaxed: Vec<String>,
    #[serde(default)]
    issues: Vec<RawIssue>,
    #[serde(default)]
    summary: Vec<RawGroup>,
}

fn severity(s: &str) -> MessageSeverity {
    if s == "warning" {
        MessageSeverity::Warning
    } else {
        MessageSeverity::Error
    }
}

/// Base64 estándar (para `-EncodedCommand`).
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `-EncodedCommand` espera el script en UTF-16 LE y base64.
fn encoded_command(script: &str) -> String {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&utf16)
}

/// Valida `xml` contra `xsd`. Los errores de validación no son un error de la llamada: van en el resultado.
pub fn validate(xml: &Path, xsd: &Path, max_issues: usize) -> VijeoResult<XmlValidation> {
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded_command(SCRIPT)])
        .env("VJ_XSD", xsd)
        .env("VJ_XML", xml)
        .env("VJ_MAX", max_issues.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| VijeoError::Internal(format!("no se pudo lanzar powershell.exe para validar el XML: {e}")))?;
    // La salida se lee en otros hilos: con mucho texto, el proceso se bloquearía al llenar la tubería.
    let mut out = child.stdout.take().expect("stdout");
    let mut err = child.stderr.take().expect("stderr");
    let out_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        b
    });
    let err_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| VijeoError::Internal(format!("powershell.exe: {e}")))? {
            break s;
        }
        if start.elapsed() >= TIMEOUT {
            let _ = child.kill();
            return Err(VijeoError::Timeout { op: "validar el XML".into(), ms: TIMEOUT.as_millis() as u64 });
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let stdout = String::from_utf8_lossy(&out_t.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_t.join().unwrap_or_default()).into_owned();
    let json = stdout.trim().trim_start_matches('\u{feff}');
    if !status.success() || json.is_empty() {
        let why = stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("sin salida").to_owned();
        return Err(VijeoError::Internal(format!("la validación con System.Xml falló: {why}")));
    }
    let raw: Raw = serde_json::from_str(json).map_err(|e| VijeoError::Internal(format!("resultado de la validación no reconocido: {e}")))?;
    Ok(to_validation(raw, xml, xsd))
}

fn to_validation(raw: Raw, xml: &Path, xsd: &Path) -> XmlValidation {
    let mut notes: Vec<String> = Vec::new();
    if !raw.relaxed.is_empty() {
        notes.push(format!("public.xsd no es XSD 1.0 válido; para validar se corrigió en memoria: {}", raw.relaxed.join("; ")));
    }
    let mut summary: Vec<XmlIssueGroup> = raw
        .summary
        .into_iter()
        .map(|g| XmlIssueGroup { severity: severity(&g.severity), message: g.message, count: g.count, first_line: g.first_line })
        .collect();
    summary.sort_by(|a, b| (a.severity != MessageSeverity::Error, std::cmp::Reverse(a.count)).cmp(&(b.severity != MessageSeverity::Error, std::cmp::Reverse(b.count))));
    XmlValidation {
        path: xml.to_path_buf(),
        schema: xsd.to_path_buf(),
        valid: raw.errors == 0 && raw.fatal.is_none(),
        errors: raw.errors,
        warnings: raw.warnings,
        fatal: raw.fatal,
        summary,
        issues: raw.issues.into_iter().map(|i| XmlIssue { severity: severity(&i.severity), line: i.line, column: i.column, message: i.message }).collect(),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn script_fits_in_a_command_line() {
        assert!(encoded_command(SCRIPT).len() < 30_000);
    }

    #[test]
    fn validates_with_dotnet() {
        let dir = std::env::temp_dir().join(format!("vijeo-xsd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let xsd = dir.join("s.xsd");
        std::fs::write(
            &xsd,
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="A"><xs:complexType><xs:sequence><xs:element name="B" maxOccurs="unbounded"><xs:complexType>
    <xs:attribute name="N" type="xs:int" use="required"/></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
        )
        .unwrap();
        let ok = dir.join("ok.xml");
        std::fs::write(&ok, r#"<A><B N="1"/><B N="2"/></A>"#).unwrap();
        let bad = dir.join("bad.xml");
        std::fs::write(&bad, r#"<A><B N="x"/><B/><B N="y"/><C/></A>"#).unwrap();
        let broken = dir.join("broken.xml");
        std::fs::write(&broken, "<A><B N=\"1\"></A>").unwrap();

        let r = validate(&ok, &xsd, 10).unwrap();
        assert!(r.valid && r.errors == 0, "{r:?}");
        let r = validate(&bad, &xsd, 2).unwrap();
        assert!(!r.valid);
        assert_eq!(r.errors, 4, "{r:?}");
        assert_eq!(r.issues.len(), 2);
        assert_eq!(r.issues[0].line, 1);
        // Los dos N no enteros comparten mensaje salvo el valor; el atributo que falta y el elemento C van aparte.
        assert!(r.summary.iter().map(|g| g.count).sum::<usize>() == 4);
        let r = validate(&broken, &xsd, 10).unwrap();
        assert!(!r.valid && r.fatal.is_some(), "{r:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
