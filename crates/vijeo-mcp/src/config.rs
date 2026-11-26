//! Línea de comandos del servidor.

use std::path::PathBuf;
use std::time::Duration;

use vijeo_core::BackendConfig;

pub const USAGE: &str = "\
vijeo-mcp: servidor MCP (stdio) para Vijeo-Designer

Opciones:
  --vijeo-version <x.y>     Fuerza la instalación de esa versión (p.ej. 6.3)
  --max-instances <n>       Proyectos abiertos a la vez, uno por proceso de Vijeo (3)
  --start-timeout <s>       Espera máxima al arrancar Vijeo (30)
  --call-timeout <s>        Tiempo límite de una operación: guardar, exportar, un asistente (60)
  --build-timeout <s>       Tiempo límite de compilación e informes (600)
  --dialog-grace <ms>       Tiempo que se tolera un MessageBox antes de abortar (1500)
  --protect <dir>           Carpeta donde nunca se abre ni se guarda (repetible)
  --cache-dir <dir>         Caché de --probe-models (%LOCALAPPDATA%\\vijeo-mcp)
  --log-file <ruta>         Log a fichero en vez de a stderr
  --probe-models            No arranca el servidor: crea y borra en Vijeo un target de cada modelo,
                            guarda qué modelos se pueden crear y sale (unos 15 s por modelo)
  --probe-only <texto>      Con --probe-models, sólo los modelos cuyo código, nombre o pantalla
                            contienen el texto (lo ya probado de los demás se conserva)
  -h, --help                Esta ayuda

Log: variable RUST_LOG (por defecto info). Como servidor nunca escribe en stdout; con --probe-models
escribe ahí el resultado (JSON) y el progreso en stderr.";

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub vijeo_version: Option<String>,
    pub backend: BackendConfig,
    pub log_file: Option<PathBuf>,
    pub probe_models: bool,
    pub probe_only: Option<String>,
}

impl Config {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>, String> {
        let mut cfg = Config::default();
        let mut it = args.into_iter();
        while let Some(a) = it.next() {
            let mut val = |name: &str| it.next().ok_or_else(|| format!("falta el valor de {name}"));
            let num = |name: &str, v: String| v.parse::<u64>().map_err(|_| format!("{name}: «{v}» no es un número"));
            match a.as_str() {
                "-h" | "--help" => return Ok(None),
                "--vijeo-version" => cfg.vijeo_version = Some(val(&a)?),
                "--max-instances" => cfg.backend.max_instances = num(&a, val(&a)?)?.max(1) as usize,
                "--start-timeout" => cfg.backend.start_timeout = Duration::from_secs(num(&a, val(&a)?)?),
                "--call-timeout" => cfg.backend.call_timeout = Duration::from_secs(num(&a, val(&a)?)?),
                "--build-timeout" => cfg.backend.build_timeout = Duration::from_secs(num(&a, val(&a)?)?),
                "--dialog-grace" => cfg.backend.dialog_grace = Duration::from_millis(num(&a, val(&a)?)?),
                "--protect" => cfg.backend.protected_dirs.push(PathBuf::from(val(&a)?)),
                "--log-file" => cfg.log_file = Some(PathBuf::from(val(&a)?)),
                "--cache-dir" => cfg.backend.cache_dir = Some(PathBuf::from(val(&a)?)),
                "--probe-models" => cfg.probe_models = true,
                "--probe-only" => cfg.probe_only = Some(val(&a)?),
                other => return Err(format!("opción desconocida: {other}")),
            }
        }
        if cfg.probe_only.is_some() && !cfg.probe_models {
            return Err("--probe-only sólo vale con --probe-models".into());
        }
        Ok(Some(cfg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Option<Config>, String> {
        Config::parse(s.split_whitespace().map(String::from))
    }

    #[test]
    fn parses_options() {
        let c = parse("--vijeo-version 6.3 --max-instances 2 --call-timeout 5 --protect C:\\x --protect D:\\y").unwrap().unwrap();
        assert_eq!(c.vijeo_version.as_deref(), Some("6.3"));
        assert_eq!(c.backend.max_instances, 2);
        assert_eq!(c.backend.call_timeout, Duration::from_secs(5));
        assert_eq!(c.backend.protected_dirs.len(), 2);
        assert!(parse("--help").unwrap().is_none());
        assert!(parse("--max-instances").is_err());
        assert!(parse("--max-instances x").is_err());
        assert!(parse("--nope").is_err());
        let c = parse("--probe-models --probe-only HMIG3U --cache-dir C:\\c").unwrap().unwrap();
        assert!(c.probe_models && c.probe_only.as_deref() == Some("HMIG3U"));
        assert_eq!(c.backend.cache_dir, Some(PathBuf::from("C:\\c")));
        assert!(parse("--probe-only X").is_err(), "sin --probe-models");
    }
}
