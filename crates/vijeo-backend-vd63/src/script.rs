//! Compilación de scripts de acción de botones con el compilador de Vijeo, sin Vijeo abierto.
//!
//! La acción de script (`CNativeAction` 0x5A) guarda dos cadenas: la fuente y el Java compilado (`end:{…}`). Al pegar,
//! Vijeo vuelve a enlazar los nombres de la fuente pero no genera el Java, y Build All no compila la fuente: con el
//! Java vacío el botón no haría nada en el HMI (docs `objetos/switch.md` §3.2). El Java lo genera `ScriptEngine.dll`
//! (servidor COM de 32 bits, `PDSLValidate::Validate`), que pregunta por cada nombre a un `IObjectMethodValidate`. El
//! de Vijeo necesita el proyecto abierto, así que `script_compile.ps1` (C# compilado al vuelo en el PowerShell de 32
//! bits) responde con lo que sale del XML del proyecto: variables con su tipo, carpetas, el objeto `Sys` y `Math`.
//! Con el script de los ejemplos (DateAndTime) el resultado es idéntico byte a byte al que guardó Vijeo.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use vijeo_core::{VijeoError, VijeoResult};

const SCRIPT: &str = include_str!("script_compile.ps1");
const TIMEOUT: Duration = Duration::from_secs(120);

/// Tipo de una variable para el compilador: `BOOL`, `INTEGER`, `REAL`, `STRING` o `BaseTag` (instancia de estructura).
pub type Symbol = (String, String);

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Message {
    pub code: u32,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Compiled {
    /// Java compilado; `None` si hubo errores.
    pub compiled: Option<String>,
    #[serde(default)]
    pub errors: Vec<Message>,
    #[serde(default)]
    pub warnings: Vec<Message>,
}

#[derive(Serialize)]
struct Input<'a> {
    symbols: &'a [Symbol],
    scripts: &'a [String],
}

/// La fuente como la guarda Vijeo: saltos de línea `\r\n`.
pub fn normalize(src: &str) -> String {
    src.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// Tipo del compilador para un tipo de variable del XML (`None`: no se puede usar en un script desde el MCP).
pub fn symbol_type(data_type: &str) -> Option<&'static str> {
    Some(match data_type.to_ascii_uppercase().as_str() {
        "BOOL" => "BOOL",
        "INT" | "UINT" | "DINT" | "UDINT" | "INTEGER" => "INTEGER",
        "REAL" | "FLOAT" => "REAL",
        "STRING" => "STRING",
        _ => return None,
    })
}

/// PowerShell de 32 bits: ScriptEngine.dll es un servidor COM en proceso de 32 bits.
fn powershell32() -> std::path::PathBuf {
    let windir = std::env::var_os("SystemRoot").or_else(|| std::env::var_os("windir")).unwrap_or_else(|| "C:\\Windows".into());
    Path::new(&windir).join("SysWOW64").join("WindowsPowerShell").join("v1.0").join("powershell.exe")
}

/// Compila `scripts` con las variables `symbols`. `frame_dir` es la carpeta `Vijeo-Frame` de la instalación (de ahí
/// sale `MathMethodDefine.xml`). Los errores de compilación no son un error de la llamada: van en cada resultado.
pub fn compile(frame_dir: &Path, symbols: &[Symbol], scripts: &[String]) -> VijeoResult<Vec<Compiled>> {
    if scripts.is_empty() {
        return Ok(Vec::new());
    }
    let dir = std::env::temp_dir().join("vijeo-mcp").join(format!("script-{}-{}", std::process::id(), unique()));
    std::fs::create_dir_all(&dir).map_err(|e| VijeoError::Internal(format!("no se pudo crear {}: {e}", dir.display())))?;
    let result = run(&dir, frame_dir, symbols, scripts);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn unique() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

fn run(dir: &Path, frame_dir: &Path, symbols: &[Symbol], scripts: &[String]) -> VijeoResult<Vec<Compiled>> {
    let io = |e: std::io::Error| VijeoError::Internal(format!("compilar scripts: {e}"));
    // PowerShell 5.1 lee como ANSI un .ps1 sin BOM.
    let ps1 = dir.join("script_compile.ps1");
    let mut f = std::fs::File::create(&ps1).map_err(io)?;
    f.write_all(b"\xEF\xBB\xBF").map_err(io)?;
    f.write_all(SCRIPT.as_bytes()).map_err(io)?;
    drop(f);
    let input = dir.join("in.json");
    let json = serde_json::to_vec(&Input { symbols, scripts }).map_err(|e| VijeoError::Internal(e.to_string()))?;
    std::fs::write(&input, json).map_err(io)?;

    let mut child = Command::new(powershell32())
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&ps1)
        .env("VIJEO_MCP_SCRIPT_IN", &input)
        .env("VIJEO_MCP_MATH_XML", frame_dir.join("MathMethodDefine.xml"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| VijeoError::Internal(format!("no se pudo lanzar el PowerShell de 32 bits para compilar scripts: {e}")))?;
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
        if let Some(s) = child.try_wait().map_err(io)? {
            break s;
        }
        if start.elapsed() >= TIMEOUT {
            let _ = child.kill();
            return Err(VijeoError::Timeout { op: "compilar scripts".into(), ms: TIMEOUT.as_millis() as u64 });
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let stdout = String::from_utf8_lossy(&out_t.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_t.join().unwrap_or_default()).into_owned();
    let json = stdout.trim().trim_start_matches('\u{feff}');
    if !status.success() || json.is_empty() {
        let why = stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("sin salida").to_owned();
        return Err(VijeoError::Internal(format!("el compilador de scripts de Vijeo (ScriptEngine.dll) falló: {why}")));
    }
    let out: Vec<Compiled> = serde_json::from_str(json).map_err(|e| VijeoError::Internal(format!("resultado del compilador no reconocido: {e}")))?;
    if out.len() != scripts.len() {
        return Err(VijeoError::Internal(format!("el compilador devolvió {} resultados para {} scripts", out.len(), scripts.len())));
    }
    Ok(out)
}

/// Texto de los errores de un script para el usuario: `línea L, columna C: mensaje (código)`.
pub fn describe(errors: &[Message]) -> String {
    errors.iter().map(|m| format!("línea {}, columna {}: {} ({})", m.line, m.column, m.message, m.code)).collect::<Vec<_>>().join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_maps_types() {
        assert_eq!(normalize("a\nb\r\nc"), "a\r\nb\r\nc");
        assert_eq!(symbol_type("UDINT"), Some("INTEGER"));
        assert_eq!(symbol_type("Float"), Some("REAL"));
        assert_eq!(symbol_type("TEWdgtVal"), None);
    }

    /// Con la instalación de Vijeo 6.3 (sin abrir Vijeo): el script de DateAndTime de los ejemplos sale idéntico al
    /// Java que guardó Vijeo en el portapapeles del corpus; un nombre que no existe es un error con su posición.
    #[test]
    fn compiles_like_vijeo() {
        let frame = Path::new(r"C:\Program Files (x86)\Schneider Electric\Vijeo-Designer 6.3\Vijeo-Frame");
        if !frame.join("ScriptEngine.dll").is_file() {
            eprintln!("sin Vijeo 6.3: se omite");
            return;
        }
        let vars: Vec<Symbol> = ["Hour", "Min", "Sec", "Year", "Month", "Day"]
            .iter()
            .map(|x| (format!("MyController.DateAndTime.iGetRTC{x}"), "INTEGER".to_owned()))
            .chain([("sName".to_owned(), "STRING".to_owned()), ("Pump.rSpeed".to_owned(), "REAL".to_owned())])
            .collect();
        let src = "//-------------------------------------\r\n//Script Created: Oct 16, 2013\r\n//\r\n// Description:\r\n//\r\n//-------------------------------------\r\nSys.setTime(MyController.DateAndTime.iGetRTCHour.getIntValue(),\r\n\t\tMyController.DateAndTime.iGetRTCMin.getIntValue(),\r\n\t\tMyController.DateAndTime.iGetRTCSec.getIntValue());\r\nSys.setDate(MyController.DateAndTime.iGetRTCYear.getIntValue(),\r\n\t\tMyController.DateAndTime.iGetRTCMonth.getIntValue(),\r\n\t\tMyController.DateAndTime.iGetRTCDay.getIntValue());\r\n";
        let want = "end:{//-------------------------------------\r\n//Script Created: Oct 16, 2013\r\n//\r\n// Description:\r\n//\r\n//-------------------------------------\r\nCgpSystem.setTime(TagDB.MyController.DateAndTime.iGetRTCHour.getIntValue(0),\r\nTagDB.MyController.DateAndTime.iGetRTCMin.getIntValue(0),\r\nTagDB.MyController.DateAndTime.iGetRTCSec.getIntValue(0));\r\nCgpSystem.setDate(TagDB.MyController.DateAndTime.iGetRTCYear.getIntValue(0),\r\nTagDB.MyController.DateAndTime.iGetRTCMonth.getIntValue(0),\r\nTagDB.MyController.DateAndTime.iGetRTCDay.getIntValue(0));\r\n\r\n}";
        let scripts = [
            src.to_owned(),
            "int n = 0;\nfor (int i = 0; i < 3; i++) { n = n + i; }\nsName.write(\"n\" + n);\nPump.rSpeed.write((float)Math.sqrt(2.0));".to_owned(),
            "NoExiste.write(1);".to_owned(),
        ];
        let r = compile(frame, &vars, &scripts).unwrap();
        assert_eq!(r[0].compiled.as_deref(), Some(want), "{:?}", r[0].errors);
        let c = r[1].compiled.as_deref().unwrap_or_else(|| panic!("{:?}", r[1].errors));
        assert!(c.starts_with("end:{int n =0;") && c.contains("CgpScriptWatchdog.checkExecution();") && c.contains("TagDB.sName.write("), "{c}");
        assert!(r[2].compiled.is_none());
        assert_eq!((r[2].errors[0].code, r[2].errors[0].line, r[2].errors[0].column), (8020, 1, 9), "{:?}", r[2].errors);
    }
}
