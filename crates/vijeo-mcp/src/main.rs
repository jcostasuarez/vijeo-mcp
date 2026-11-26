//! Servidor MCP (stdio) para Vijeo-Designer. stdout es el canal MCP: el log va a stderr o a fichero.

mod config;
mod server;

use std::process::ExitCode;

use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;
use vijeo_core::{BackendRegistry, ModelFilter, ProbeOutcome};

use config::{Config, USAGE};
use server::{State, VijeoServer};

/// Registro de backends conocidos. Para una versión nueva: añadir su factoría aquí.
fn registry() -> BackendRegistry {
    BackendRegistry::new().register(Box::new(vijeo_backend_vd63::Vd63Factory))
}

fn init_logging(cfg: &Config) -> Result<(), String> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false);
    match &cfg.log_file {
        Some(p) => {
            let f = std::fs::OpenOptions::new().create(true).append(true).open(p).map_err(|e| format!("{}: {e}", p.display()))?;
            builder.with_writer(std::sync::Mutex::new(f)).init();
        }
        None => builder.with_writer(std::io::stderr).init(),
    }
    Ok(())
}

fn build_state(cfg: &Config) -> State {
    let registry = registry();
    let installations = vijeo_ui::discover_installations();
    let (backend, backend_error) = match registry.select(&installations, cfg.vijeo_version.as_deref()) {
        None => {
            let msg = if installations.is_empty() {
                "no se encontró ninguna instalación de Vijeo-Designer".to_owned()
            } else {
                let found: Vec<String> = installations.iter().map(|i| i.product_version.to_string()).collect();
                format!("ningún backend soporta las versiones instaladas ({})", found.join(", "))
            };
            (None, Some(msg))
        }
        Some(sel) => {
            tracing::info!(version = %sel.installation.product_version, backend = sel.factory.id(), support = ?sel.support, "instalación elegida");
            match sel.factory.create(&sel.installation, sel.support, &cfg.backend) {
                Ok(b) => (Some(b), None),
                Err(e) => (None, Some(e.to_string())),
            }
        }
    };
    if let Some(e) = &backend_error {
        tracing::error!("{e}");
    }
    State { registry, installations, backend, backend_error }
}

/// `--probe-models` (D5): prueba los modelos en Vijeo, guarda la caché y sale. Aquí stdout no es el canal
/// MCP: lleva el resultado en JSON; el progreso va a stderr.
async fn probe_models(cfg: &Config) -> ExitCode {
    let state = build_state(cfg);
    let Some(backend) = state.backend else {
        eprintln!("{}", state.backend_error.unwrap_or_default());
        return ExitCode::FAILURE;
    };
    let filter = ModelFilter { text: cfg.probe_only.clone(), ..Default::default() };
    let b = backend.clone();
    let r = tokio::task::spawn_blocking(move || {
        let catalog = b.catalog().ok_or_else(|| "el backend no tiene catálogo de modelos".to_owned())?;
        // Los de familias desactivadas no se prueban.
        let total = catalog.models(&ModelFilter { creatable: None, ..filter.clone() }).iter().filter(|m| m.family_name.is_some()).count();
        let mut n = 0;
        catalog
            .probe_models(&filter, &mut |p| {
                n += 1;
                let detail = p.detail.as_deref().map(|d| format!(": {d}")).unwrap_or_default();
                eprintln!("[{n}/{total}] {} → {:?} ({} s){detail}", p.internal_name, p.outcome, p.duration_ms / 1000);
            })
            .map_err(|e| e.to_string())
    })
    .await
    .unwrap_or_else(|e| Err(format!("la prueba se interrumpió: {e}")));
    let _ = tokio::task::spawn_blocking(move || backend.shutdown()).await;
    match r {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
            let count = |o| report.probed.iter().filter(|p| p.outcome == o).count();
            eprintln!(
                "{} creados, {} rechazados, {} fallidos, {} sin probar; caché: {}",
                count(ProbeOutcome::Created),
                count(ProbeOutcome::Rejected),
                count(ProbeOutcome::Failed),
                report.skipped.len(),
                report.cache.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("--probe-models: {e}");
            ExitCode::FAILURE
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cfg = match Config::parse(std::env::args().skip(1)) {
        Ok(Some(c)) => c,
        Ok(None) => {
            eprintln!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = init_logging(&cfg) {
        eprintln!("no se pudo abrir el log: {e}");
        return ExitCode::FAILURE;
    }
    if cfg.probe_models {
        return probe_models(&cfg).await;
    }

    let server = VijeoServer::new(build_state(&cfg));
    let backend = server.backend();
    let service = match server.serve(rmcp::transport::stdio()).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("no se pudo iniciar el servidor MCP: {e}");
            return ExitCode::FAILURE;
        }
    };
    tokio::select! {
        r = service.waiting() => {
            if let Err(e) = r {
                tracing::error!("el servidor MCP terminó con error: {e}");
            }
        }
        _ = tokio::signal::ctrl_c() => tracing::info!("Ctrl+C"),
    }
    // Close antes de Exit en todas las instancias, para no dejar procesos ni M5DTemPHolder.
    if let Some(b) = backend {
        let _ = tokio::task::spawn_blocking(move || b.shutdown()).await;
    }
    tracing::info!("fin");
    ExitCode::SUCCESS
}
