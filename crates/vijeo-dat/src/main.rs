//! Uso: vjd-dat <fichero.dat | directorio>... -o <directorio_salida>
//! Escribe un `.xml` (UTF-8) por cada `.dat` descifrado.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn collect(input: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if input.is_dir() {
        for entry in fs::read_dir(input)? {
            let p = entry?.path();
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dat")) {
                out.push(p);
            }
        }
    } else {
        out.push(input.to_path_buf());
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(pos) = args.iter().position(|a| a == "-o") else {
        eprintln!("uso: vjd-dat <fichero.dat | directorio>... -o <directorio_salida>");
        return ExitCode::from(2);
    };
    let Some(out_dir) = args.get(pos + 1).map(PathBuf::from) else {
        eprintln!("falta el directorio de salida tras -o");
        return ExitCode::from(2);
    };
    let mut files = Vec::new();
    for a in &args[..pos] {
        if let Err(e) = collect(Path::new(a), &mut files) {
            eprintln!("{a}: {e}");
            return ExitCode::FAILURE;
        }
    }
    if let Err(e) = fs::create_dir_all(&out_dir) {
        eprintln!("{}: {e}", out_dir.display());
        return ExitCode::FAILURE;
    }
    let mut failed = 0;
    for f in &files {
        let result = fs::read(f)
            .map_err(|e| e.to_string())
            .and_then(|raw| vijeo_dat::decrypt_dat(&raw).map_err(|e| e.to_string()));
        match result {
            Ok(text) => {
                let name = f.file_stem().unwrap_or_default().to_string_lossy().into_owned() + ".xml";
                let dst = out_dir.join(name);
                match fs::write(&dst, text.as_bytes()) {
                    Ok(()) => println!("ok   {} -> {} ({} chars)", f.display(), dst.display(), text.chars().count()),
                    Err(e) => {
                        failed += 1;
                        eprintln!("fail {}: {e}", dst.display());
                    }
                }
            }
            Err(e) => {
                failed += 1;
                eprintln!("fail {}: {e}", f.display());
            }
        }
    }
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
