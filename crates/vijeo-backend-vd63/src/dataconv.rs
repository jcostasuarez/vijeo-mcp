//! `DataConverter.exe` (ayuda §20.5.3.2-3): recetas `.rcp` y registros `.dat` a CSV/TXT y CSV a `.rcp`.
//!
//! Es un ejecutable de consola: devuelve 0 si todo fue bien y escribe en la salida estándar (con `-v`, el
//! progreso). Los nombres de salida son los de los ficheros de entrada; lo que escribió se sabe comparando
//! la carpeta de salida antes y después.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use vijeo_core::{DataConvertMode, DataConvertRequest, DataConvertResult, DataDateFormat, VijeoError, VijeoResult};

pub const EXE: &str = "DataConverter.exe";

/// Argumentos de DataConverter para `req` (las rutas, ya validadas).
pub fn arguments(req: &DataConvertRequest) -> VijeoResult<Vec<String>> {
    let mut a = vec!["-i".to_owned(), req.input.to_string_lossy().into_owned(), "-o".to_owned(), req.output.to_string_lossy().into_owned()];
    match req.mode {
        DataConvertMode::ToCsv => {}
        DataConvertMode::RcpToSimpleCsv => a.push("-csr".into()),
        DataConvertMode::RcpToRuntimeCsv => a.push("-crf".into()),
        DataConvertMode::CsvToRcp => a.push("-ccr".into()),
        DataConvertMode::RuntimeCsvToRcp => a.push("-cfr".into()),
    }
    if let Some(rp) = &req.recipe_dir {
        if !matches!(req.mode, DataConvertMode::CsvToRcp | DataConvertMode::RuntimeCsvToRcp) {
            return Err(VijeoError::invalid("recipe_dir", "sólo en csv_to_rcp y runtime_csv_to_rcp"));
        }
        a.push("-rp".into());
        a.push(rp.to_string_lossy().into_owned());
    } else if req.mode == DataConvertMode::CsvToRcp {
        return Err(VijeoError::invalid("recipe_dir", "csv_to_rcp valida el CSV contra las recetas .rcp descargadas del HMI: indica su carpeta"));
    }
    if req.subfolders {
        a.push("-s".into());
    }
    if req.keep_structure {
        if !req.subfolders {
            return Err(VijeoError::invalid("keep_structure", "sólo tiene sentido con subfolders"));
        }
        a.push("-b".into());
    }
    if req.overwrite {
        a.push("-cw".into());
    }
    if req.headers {
        a.push("-ch".into());
    }
    if let Some(d) = req.date_format {
        a.push(
            match d {
                DataDateFormat::MmDdYy => "-ct",
                DataDateFormat::YyMmDd => "-ct0",
                DataDateFormat::DdMmYy => "-ct1",
                DataDateFormat::YyyyMmDd => "-ct3",
                DataDateFormat::DdMmYyyy => "-ct4",
            }
            .into(),
        );
    }
    if let Some(d) = &req.delimiter {
        let c = if d.eq_ignore_ascii_case("tab") {
            "t".to_owned()
        } else {
            let mut chars = d.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c != '|' && !c.is_whitespace() && c != '"' => c.to_string(),
                _ => return Err(VijeoError::invalid("delimiter", "un solo carácter (no `|`, comillas ni espacio) o `tab`")),
            }
        };
        a.push(format!("-d{c}"));
    }
    a.push("-v".into());
    Ok(a)
}

/// Fecha de cada fichero bajo `dir`.
fn listing(dir: &Path) -> BTreeMap<PathBuf, Option<SystemTime>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(p, e.metadata().and_then(|m| m.modified()).ok());
            }
        }
    }
    out
}

/// Lanza DataConverter con `args` y espera hasta `timeout`. El proceso queda atado al MCP.
pub fn run(exe: &Path, req: &DataConvertRequest, timeout: Duration) -> VijeoResult<DataConvertResult> {
    let args = arguments(req)?;
    let before = listing(&req.output);
    let t0 = Instant::now();
    let mut child = Command::new(exe)
        .args(&args)
        .current_dir(exe.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| VijeoError::Internal(format!("no se pudo lanzar {}: {e}", exe.display())))?;
    vijeo_ui::process::tie_child(&child);
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
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| VijeoError::Internal(format!("DataConverter: {e}")))? {
            break s;
        }
        if t0.elapsed() >= timeout {
            let _ = child.kill();
            return Err(VijeoError::Timeout { op: "DataConverter".into(), ms: timeout.as_millis() as u64 });
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let mut log: Vec<String> = Vec::new();
    for bytes in [out_t.join().unwrap_or_default(), err_t.join().unwrap_or_default()] {
        log.extend(crate::reports::decode_text(&bytes).lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).map(str::to_owned));
    }
    let after = listing(&req.output);
    let output_files = after.into_iter().filter(|(p, m)| before.get(p) != Some(m)).map(|(p, _)| p).collect();
    let errors: Vec<String> = log.iter().filter(|l| is_error_line(l)).cloned().collect();
    Ok(DataConvertResult { ok: status.success() && errors.is_empty(), exit_code: status.code(), errors, output_files, log, duration_ms: t0.elapsed().as_millis() as u64 })
}

/// Línea de la salida de DataConverter que avisa de un fallo. Con una entrada no válida termina con 0 igual
/// (`<fichero>:  Not a valid DAT file`, verificado); el resto de textos son de su propia ayuda.
fn is_error_line(l: &str) -> bool {
    let l = l.to_ascii_lowercase();
    ["not a valid", "not found", "error", "failed", "invalid", "cannot", "unable"].iter().any(|k| l.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(mode: DataConvertMode) -> DataConvertRequest {
        serde_json::from_value(serde_json::json!({ "input": r"C:\in", "output": r"C:\out dir", "mode": mode })).unwrap()
    }

    #[test]
    fn builds_arguments() {
        let mut r = req(DataConvertMode::ToCsv);
        r.subfolders = true;
        r.keep_structure = true;
        r.overwrite = true;
        r.headers = true;
        r.date_format = Some(DataDateFormat::DdMmYyyy);
        r.delimiter = Some("tab".into());
        assert_eq!(arguments(&r).unwrap(), ["-i", r"C:\in", "-o", r"C:\out dir", "-s", "-b", "-cw", "-ch", "-ct4", "-dt", "-v"]);
        let mut r = req(DataConvertMode::CsvToRcp);
        assert!(arguments(&r).is_err(), "csv_to_rcp sin recipe_dir");
        r.recipe_dir = Some(r"C:\rcp".into());
        assert_eq!(arguments(&r).unwrap(), ["-i", r"C:\in", "-o", r"C:\out dir", "-ccr", "-rp", r"C:\rcp", "-v"]);
        let mut r = req(DataConvertMode::ToCsv);
        r.delimiter = Some("|".into());
        assert!(arguments(&r).is_err());
        r.delimiter = Some(";".into());
        assert!(arguments(&r).unwrap().contains(&"-d;".to_owned()));
        let mut r = req(DataConvertMode::RcpToSimpleCsv);
        r.recipe_dir = Some(r"C:\rcp".into());
        assert!(arguments(&r).is_err(), "recipe_dir fuera de las conversiones a .rcp");
        let mut r = req(DataConvertMode::ToCsv);
        r.keep_structure = true;
        assert!(arguments(&r).is_err(), "keep_structure sin subfolders");
    }

    #[test]
    fn recognizes_error_lines() {
        assert!(is_error_line(r"C:\x\roto.dat:  Not a valid DAT file"));
        assert!(is_error_line("Input file or folder not found"));
        assert!(!is_error_line("Finished processing files."));
        assert!(!is_error_line("DataConverter, Copyright c 2003 - 2022, Schneider-Electric, All Rights Reserved"));
    }
}
