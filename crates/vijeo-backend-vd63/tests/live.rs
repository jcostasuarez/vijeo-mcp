//! Pruebas sobre la instalación real.
//! - `real_catalog_*`: sólo leen `TargetInfo` (no lanzan Vijeo); se saltan si no hay 6.3.
//! - `live_*` (feature `live-vijeo`): lanzan Vijeo sobre copias en el directorio temporal.

use vijeo_backend_vd63::catalog::Catalog;
use vijeo_core::{BackendFactory, ModelFilter, Support};

fn installation() -> Option<vijeo_core::Installation> {
    vijeo_ui::discover_installations().into_iter().find(|i| vijeo_backend_vd63::Vd63Factory.supports(i) == Support::Exact)
}

#[test]
fn real_catalog_matches_live_validation() {
    let Some(inst) = installation() else {
        eprintln!("sin Vijeo 6.3: se omite");
        return;
    };
    let cat = Catalog::load(&inst.frame_dir).unwrap();
    let creatable = cat.filter(&ModelFilter { creatable: Some(true), ..Default::default() });
    // docs/reference/vd-6.3/models.json: 29 creables por COM (validación en vivo), que son los que tienen
    // bloque <SoMachine>. Con el asistente no hace falta ese bloque.
    let somachine: Vec<_> = creatable.iter().filter(|m| m.somachine).map(|m| &m.internal_name).collect();
    assert_eq!(somachine.len(), 29, "{somachine:?}");
    assert!(cat.models.iter().any(|m| m.internal_name == "Model_HMIS5T" && m.creatable));
    assert!(cat.models.iter().any(|m| m.internal_name == "Model_HMIG5U" && m.creatable && !m.somachine));
    // Sólo estos tienen ETH2; target_config_set no toca su red aunque tenga la IP de descarga.
    let mut dual: Vec<_> = cat.models.iter().filter(|m| m.dual_ethernet).map(|m| m.internal_name.as_str()).collect();
    dual.sort();
    assert_eq!(dual, ["Model_HMIG2U", "Model_HMIG3U", "Model_HMIG3X"]);
}

const EXAMPLES: &str = r"C:\Program Files (x86)\Schneider Electric\EcoStruxure Machine Expert - Basic\Examples";

/// Lee drivers y propiedades de los ejemplos de Machine Expert Basic sin lanzar Vijeo.
#[test]
fn real_sample_config() {
    let modbus = std::path::Path::new(EXAMPLES).join("Compact_Modbus SL_M221_Book.vdz");
    if !modbus.is_file() {
        eprintln!("sin ejemplos de Machine Expert Basic: se omite");
        return;
    }
    let mut cfb = vijeo_backend_vd63::swx::open_vdz(&modbus).unwrap();
    let ts = vijeo_backend_vd63::swx::read_targets(&mut cfb, None);
    assert_eq!(ts.len(), 1);
    let t = &ts[0];
    assert_eq!(t.name, "HMISTO531_532");
    assert_eq!(t.model.as_deref(), Some("Model_HMISTO531_532"));
    assert_eq!(t.download.ip_address.as_deref(), Some("172.20.10.168"));
    assert_eq!(t.download.subnet_mask.as_deref(), Some("255.255.0.0"));
    assert_eq!(t.download.baud_rate, Some(9600));
    assert_eq!(t.drivers.len(), 1);
    let d = &t.drivers[0];
    assert_eq!((d.name.as_str(), d.driver_id.as_deref()), ("ModbusTCP", Some("Schneider_Modbus_PDL_TCP")));
    assert_eq!(d.settings.get("TimeoutPeriod").map(String::as_str), Some("3000"));
    assert_eq!(d.equipment.len(), 1);
    let e = &d.equipment[0];
    assert_eq!(e.name, "M221_Controller");
    assert_eq!((e.ip_address.as_deref(), e.port, e.unit_id), (Some("172.20.10.167"), Some(502), Some(255)));
    assert_eq!(e.scan_groups, ["ScanGroup1"]);

    // El target está en «Target 2» (se borró el 1) y la E/S en «Component 4». Trae de SoMachine una red
    // SoMachine con dirección: es del proyecto, no la que añade la validación por COM.
    let hw = std::path::Path::new(EXAMPLES).join("Compact_Hardwired_M221.vdz");
    let mut cfb = vijeo_backend_vd63::swx::open_vdz(&hw).unwrap();
    let ts = vijeo_backend_vd63::swx::read_targets(&mut cfb, None);
    assert_eq!((ts.len(), ts[0].storage.as_str()), (1, "Target 2"));
    let som = ts[0].drivers.iter().find(|d| d.driver_id.as_deref() == Some("Schneider_PDL_OMS_Gateway3")).expect("red SoMachine");
    assert_eq!(som.name, "SoMachineNetwork01");
    assert_eq!(som.equipment[0].ip_address.as_deref(), Some("127.0.0.1"));
    assert!(ts[0].model.as_deref().is_some_and(|m| m.starts_with("Model_")));
}

#[cfg(feature = "live-vijeo")]
mod live {
    use super::*;
    use std::path::PathBuf;
    use vijeo_core::{BackendConfig, ModelSelector};

    fn sample() -> Option<PathBuf> {
        let p = PathBuf::from(r"C:\Program Files (x86)\Schneider Electric\EcoStruxure Machine Expert - Basic\Examples\Compact_Hardwired_M221.vdz");
        p.is_file().then_some(p)
    }

    fn work_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join("vijeo-mcp-live").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn modbus() -> Option<PathBuf> {
        let p = std::path::Path::new(EXAMPLES).join("Compact_Modbus SL_M221_Book.vdz");
        p.is_file().then_some(p)
    }

    /// Rutas `BP:\…` de los paneles base de un target, leídas sin el XML (`vdz_panel_list` con `project`).
    fn base_refs(c: &dyn vijeo_core::ConfigOps, p: vijeo_core::ProjectId, target: &str) -> Vec<String> {
        c.project_screens(p, Some(target)).unwrap().into_iter().filter_map(|s| s.panel_ref).collect()
    }

    fn backend() -> std::sync::Arc<dyn vijeo_core::Backend> {
        let inst = installation().expect("Vijeo 6.3");
        // Sin la caché de --probe-models del usuario: los tests ven la regla de creabilidad.
        let cfg = BackendConfig { cache_dir: Some(std::env::temp_dir().join("vijeo-mcp-live").join("no-cache")), ..Default::default() };
        vijeo_backend_vd63::Vd63Factory.create(&inst, Support::Exact, &cfg).unwrap()
    }

    #[test]
    fn live_ipc_target_is_kept() {
        // El Document de un iPC no da el modelo: el target se crea igual y no se deshace (--probe-models).
        let backend = backend();
        let vdz = work_dir("ipc").join("McpIpc.vdz");
        let projects = backend.projects().unwrap();
        let info = projects.new_project("McpIpc", &vdz, false, "Rack", &sel("Model_IPC_RACK_RSO", None)).unwrap();
        eprintln!("{info:?}");
        let p = info.id;
        let t = backend.targets().unwrap().add(p, "Box", &sel("Model_IPC_XBT_MPC22", None)).unwrap();
        eprintln!("{t:?}");
        let t = backend.targets().unwrap().add(p, "P6", &sel("TARGET_HMI_P6", None)).unwrap();
        eprintln!("{t:?}");
        projects.save(p, None, false).unwrap();
        projects.close(p, false).unwrap();
        let cfg = backend.config().unwrap().file_config(&vdz, None).unwrap();
        let got: Vec<(&str, Option<&str>)> = cfg.targets.iter().map(|t| (t.name.as_str(), t.model.as_deref())).collect();
        // El P6 se elige en el asistente como «Harmony P6 Box PN» (WIZARD_MODEL_NAMES).
        assert_eq!(got, [("Rack", Some("Model_IPC_RACK_RSO")), ("Box", Some("Model_IPC_XBT_MPC22")), ("P6", Some("TARGET_HMI_P6"))]);
        eprintln!("{}", serde_json::to_string_pretty(&cfg).unwrap());
    }

    fn sel(model: &str, display: Option<&str>) -> ModelSelector {
        ModelSelector { model: model.into(), display: display.map(Into::into) }
    }

    fn workspace_entries() -> std::collections::BTreeSet<std::ffi::OsString> {
        let ws = std::path::Path::new(r"C:\Users\Public\Documents\Vijeo-Designer 6.3\Vijeo-Manager");
        std::fs::read_dir(ws).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).filter(|n| !n.to_string_lossy().starts_with("M5DTemPHolder")).collect()
    }

    #[test]
    fn live_new_project_targets_build_save() {
        let backend = backend();
        let ws_before = workspace_entries();
        let dir = work_dir("new");
        let vdz = dir.join("McpNuevo.vdz");
        let projects = backend.projects().unwrap();
        let info = projects.new_project("McpNuevo", &vdz, false, "Pantalla1", &sel("HMIG3U", Some("HMIDT351"))).unwrap();
        assert!(vdz.is_file());
        assert_eq!(info.name, "McpNuevo");
        assert_eq!(info.targets.len(), 1);
        assert_eq!((info.targets[0].name.as_str(), info.targets[0].hmi_type.as_str()), ("Pantalla1", "Model_HMIG3U:HMIDT351 (800x480)"));
        assert!(!info.dirty);
        let p = info.id;

        // Orden y modelo de cada target, también tras borrar uno (nombres de NameServer ↔ carpetas `Target N`).
        let targets = backend.targets().unwrap();
        assert_eq!(targets.add(p, "pantalla1", &sel("Model_HMIS5T", None)).unwrap_err().code(), "invalid_argument", "duplicado");
        targets.add(p, "ZZ", &sel("Model_HMIS5T", None)).unwrap();
        let g5u = targets.add(p, "AA", &sel("HMIG5U", None)).unwrap();
        assert!(g5u.hmi_type.starts_with("Model_HMIG5U"), "modelo sin <SoMachine>: {g5u:?}");
        targets.delete(p, "zz").unwrap();
        targets.add(p, "MM", &sel("Model_HMIS5T", None)).unwrap();
        let list: Vec<(String, String)> = targets.list(p).unwrap().into_iter().map(|t| (t.name, t.hmi_type)).collect();
        eprintln!("{list:?}");
        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Pantalla1", "AA", "MM"]);
        assert!(list[1].1.starts_with("Model_HMIG5U") && list[2].1 == "Model_HMIS5T", "{list:?}");
        assert!(!base_refs(backend.config().unwrap(), p, "Pantalla1").is_empty());
        assert!(backend.reports().unwrap().actions(p, None).unwrap().is_empty());

        let b = backend.build().unwrap().build_target(p, "Pantalla1").unwrap();
        eprintln!("build: {b:?}");
        assert_eq!(b.messages.len() as i32, b.errors + b.warnings);

        assert_eq!(projects.close(p, false).unwrap_err().code(), "refused", "cambios sin guardar");
        let saved = projects.save(p, None, false).unwrap();
        assert!(!saved.dirty);
        projects.close(p, false).unwrap();

        // Reabrir: los targets persisten.
        let info = projects.open(&vdz).unwrap();
        assert_eq!(info.name, "McpNuevo");
        let names: Vec<&str> = info.targets.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Pantalla1", "AA", "MM"]);
        assert_eq!(info.targets[0].model.as_ref().unwrap().internal_name, "Model_HMIG3U");
        projects.close(info.id, true).unwrap();
        backend.shutdown().unwrap();
        assert_eq!(workspace_entries(), ws_before, "carpetas temporales en Vijeo-Manager");
    }

    #[test]
    fn live_sample_reads_and_export() {
        let Some(src) = sample() else { return };
        let backend = backend();
        let dir = work_dir("sample");
        let vdz = dir.join("sample.vdz");
        std::fs::copy(&src, &vdz).unwrap();
        let info = backend.projects().unwrap().open(&vdz).unwrap();
        let p = info.id;
        let target = info.targets[0].name.clone();
        let reports = backend.reports().unwrap();

        let vars = backend.config().unwrap().project_variables(p, None, false, false).unwrap();
        eprintln!("{} variables; externas: {:?}", vars.len(), vars.iter().filter(|v| v.source == "External").map(|v| &v.name).take(5).collect::<Vec<_>>());
        let ext = vars.iter().find(|v| v.name == "MyController.Alarms.xAlarmEStop_1").expect("variable externa");
        assert!(ext.address.as_deref().unwrap_or_default().starts_with("%M"));

        let l = backend.variables().unwrap().lookup(p, &target, "mycontroller.alarms.xalarmestop_1").unwrap();
        assert!(l.exists);
        assert_eq!(l.data_type.as_deref(), Some("BOOL"));
        assert!(!backend.variables().unwrap().lookup(p, &target, "NoExiste_XYZ").unwrap().exists);

        let panels = reports.panels(p, None).unwrap();
        let mut base: Vec<String> = panels.iter().filter_map(|x| x.panel_ref.clone()).collect();
        let mut listed: Vec<String> = base_refs(backend.config().unwrap(), p, &target);
        base.sort();
        listed.sort();
        assert!(!base.is_empty());
        assert_eq!(base, listed);
        let actions = reports.actions(p, Some(&target)).unwrap();
        eprintln!("{actions:#?}");
        assert!(!actions.is_empty());
        // Usos leídos de la copia de trabajo (sin el XML): las mismas variables usadas que en el XML que se exporta abajo.
        let usage = backend.config().unwrap().project_usage(p, None, false).unwrap();
        assert!(usage.references.iter().any(|r| !r.references.is_empty()));
        assert!(usage.broken.is_empty(), "{:?}", usage.broken);

        let out = reports.export_xml(p, &dir.join("export"), false).unwrap();
        eprintln!("{out:?}");
        let text = std::fs::read_to_string(&out.path).unwrap();
        let doc = vijeo_backend_vd63::vxml::parse(&text).unwrap();
        let xml_used: std::collections::BTreeSet<String> =
            vijeo_backend_vd63::vxml::targets(&doc).into_iter().flat_map(vijeo_backend_vd63::vxml::cross_references).filter(|r| !r.references.is_empty()).map(|r| r.variable).collect();
        let used: std::collections::BTreeSet<String> = usage.references.iter().filter(|r| !r.references.is_empty() && !r.variable.starts_with('_')).map(|r| r.variable.clone()).collect();
        assert_eq!(used, xml_used, "variables usadas: offline / XML");
        assert_eq!(vijeo_backend_vd63::vxml::project_name(&doc).as_deref(), Some(info.name.as_str()), "nombre del proyecto, no el temporal");
        assert_eq!(out.panels, listed.len());
        assert_eq!(reports.export_xml(p, &dir.join("export"), false).unwrap_err().code(), "invalid_argument", "ya existe");
        backend.projects().unwrap().close(p, true).unwrap();
        backend.shutdown().unwrap();
    }

    #[test]
    fn live_build_messages_and_config() {
        // Compact_Modbus SL_M221_Book compila con 2 errores y 2 warnings en modo normal (sin el falso 1331).
        let Some(src) = modbus() else { return };
        let backend = backend();
        let dir = work_dir("messages");
        let vdz = dir.join("modbus.vdz");
        std::fs::copy(&src, &vdz).unwrap();
        let info = backend.projects().unwrap().open(&vdz).unwrap();
        eprintln!("{info:#?}");
        let p = info.id;
        let b = backend.build().unwrap().build_project(p).unwrap();
        eprintln!("{b:#?}");
        assert_eq!((b.errors, b.warnings), (2, 2));
        assert!(b.messages.iter().all(|m| m.code != Some(1331)), "sin el falso 1331");
        let count = |s| b.messages.iter().filter(|m| m.severity == s).count() as i32;
        assert_eq!((count(vijeo_core::MessageSeverity::Error), count(vijeo_core::MessageSeverity::Warning)), (b.errors, b.warnings));
        // La salida se vacía en cada compilación: compilar dos veces no duplica mensajes.
        let again = backend.build().unwrap().build_target(p, &info.targets[0].name).unwrap();
        assert_eq!(again.messages.len() as i32, again.errors + again.warnings);
        assert_eq!(backend.build().unwrap().feedback(p).unwrap().messages.len(), b.messages.len());
        assert_eq!(backend.build().unwrap().build_target(p, "NoExiste").unwrap_err().code(), "target_not_found");
        assert!(!backend.projects().unwrap().info(p).unwrap().dirty, "compilar no deja cambios");

        let cfg = backend.config().unwrap().project_config(p, None).unwrap();
        assert_eq!(cfg.targets[0].name, info.targets[0].name);
        assert!(cfg.targets[0].hmi_type.is_some());
        assert_eq!(cfg.targets[0].drivers[0].equipment[0].name, "M221_Controller");
        backend.projects().unwrap().close(p, false).unwrap();
        backend.shutdown().unwrap();
    }

    #[test]
    fn live_build_file_without_com() {
        let Some(src) = modbus() else { return };
        let backend = backend();
        let dir = work_dir("build_file");
        let vdz = dir.join("modbus.vdz");
        std::fs::copy(&src, &vdz).unwrap();
        let before = std::fs::read(&vdz).unwrap();
        let ws_before = workspace_entries();

        let b = backend.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("{b:#?}");
        assert_eq!((b.errors, b.warnings), (2, 2));
        assert!(b.messages.iter().all(|m| m.code != Some(1331)), "sin el falso 1331");
        assert_eq!(b.messages.iter().filter(|m| m.code == Some(2841)).count(), 2);
        assert!(b.notes.iter().any(|n| n.contains("convirtió")), "el modelo HMISTO531 no existe en 6.3: {:?}", b.notes);

        let t = backend.build().unwrap().build_file(&vdz, Some("hmisto531_532")).unwrap();
        assert_eq!(t.messages.len() as i32, t.errors + t.warnings);
        assert_eq!(backend.build().unwrap().build_file(&vdz, Some("NoExiste")).unwrap_err().code(), "target_not_found");

        // El original no cambia y no quedan carpetas temporales en el workspace.
        assert_eq!(std::fs::read(&vdz).unwrap(), before);
        assert_eq!(workspace_entries(), ws_before);
        backend.shutdown().unwrap();
    }

    #[test]
    fn live_config_set_build_and_reopen() {
        // Cambia IP de descarga y del equipo del ejemplo Modbus, lo compila (mismos 2/2) y comprueba que Vijeo
        // lo abre, y que al guardarlo él mismo los valores siguen ahí. Con VIJEO_MCP_LIVE_EXTRA=<.vdz> repite la
        // edición del primer equipo con una copia de ese proyecto.
        use vijeo_core::{DownloadChanges, EquipmentChanges, TargetConfigChanges};
        let Some(src) = modbus() else { return };
        let backend = backend();
        let config = backend.config().unwrap();
        let dir = work_dir("config_set");
        let vdz = dir.join("modbus.vdz");
        std::fs::copy(&src, &vdz).unwrap();
        let original = std::fs::read(&vdz).unwrap();
        let edited = dir.join("modbus_ip.vdz");
        let changes = TargetConfigChanges {
            download: Some(DownloadChanges { ip_address: Some("192.168.50.20".into()), subnet_mask: Some("255.255.255.0".into()), default_gateway: Some("192.168.50.1".into()) }),
            equipment: vec![EquipmentChanges { name: "M221_Controller".into(), ip_address: Some("192.168.50.10".into()), port: Some(502), unit_id: Some(1), ..Default::default() }],
        };
        let dry = config.edit_file_config(&vdz, None, &changes, Some(&edited), false, true).unwrap();
        assert!(!dry.written && !edited.exists());
        // Las tres de descarga, su copia de la red (el STO tiene una sola Ethernet) y el equipo.
        assert_eq!(dry.changes.len(), 7, "{:#?}", dry.changes);
        assert_eq!(dry.changes.iter().filter(|c| c.item == "ethernet").count(), 3);
        let r = config.edit_file_config(&vdz, None, &changes, Some(&edited), false, false).unwrap();
        eprintln!("{r:#?}");
        assert!(r.written);
        assert_eq!(std::fs::read(&vdz).unwrap(), original, "el origen no cambia con save_as");
        let again = config.edit_file_config(&edited, None, &changes, None, false, false).unwrap();
        assert!(!again.written && again.changes.is_empty(), "idempotente");
        assert!(!dir.join("modbus_ip.vdz.mcp-tmp").exists());

        let b = backend.build().unwrap().build_file(&edited, None).unwrap();
        eprintln!("{b:#?}");
        assert_eq!((b.errors, b.warnings), (2, 2), "los mismos errores que el original");

        // Vijeo lo abre y, al guardarlo él (Save Project), conserva los valores.
        let projects = backend.projects().unwrap();
        let info = projects.open(&edited).unwrap();
        assert_eq!(config.edit_file_config(&edited, None, &changes, None, false, true).unwrap_err().code(), "refused", "abierto");
        let resaved = dir.join("modbus_resaved.vdz");
        projects.save(info.id, Some(&resaved), false).unwrap();
        projects.close(info.id, false).unwrap();
        let t = config.file_config(&resaved, None).unwrap().targets.remove(0);
        assert_eq!(t.download.ip_address.as_deref(), Some("192.168.50.20"));
        assert_eq!(t.download.default_gateway.as_deref(), Some("192.168.50.1"));
        let e = &t.drivers[0].equipment[0];
        assert_eq!((e.ip_address.as_deref(), e.port, e.unit_id), (Some("192.168.50.10"), Some(502), Some(1)));

        if let Some(extra) = std::env::var_os("VIJEO_MCP_LIVE_EXTRA").map(PathBuf::from) {
            let copy = dir.join("extra.vdz");
            std::fs::copy(&extra, &copy).unwrap();
            let before = config.file_config(&copy, None).unwrap().targets.remove(0);
            let b0 = backend.build().unwrap().build_file(&copy, None).unwrap();
            let eq = &before.drivers.iter().find(|d| !d.equipment.is_empty()).unwrap().equipment[0];
            let ch = TargetConfigChanges {
                download: Some(DownloadChanges { ip_address: Some("10.0.7.126".into()), subnet_mask: Some("255.255.0.0".into()), default_gateway: None }),
                equipment: vec![EquipmentChanges { name: eq.name.clone(), ip_address: Some("10.0.7.99".into()), ..Default::default() }],
            };
            // En un GTU con segunda Ethernet, Vijeo valida la puerta contra ella: se rechaza antes de escribir.
            if before.other_values.iter().filter(|v| v.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| !ip.is_unspecified())).count() > 0 {
                let gw = TargetConfigChanges { download: Some(DownloadChanges { default_gateway: Some("10.0.0.1".into()), ..Default::default() }), equipment: vec![] };
                let e = config.edit_file_config(&copy, Some(&before.name), &gw, None, false, true).unwrap_err();
                eprintln!("extra puerta: {e}");
            }
            let r = config.edit_file_config(&copy, Some(&before.name), &ch, None, false, false).unwrap();
            eprintln!("extra: {:#?}", r.changes);
            let cat = Catalog::load(&installation().unwrap().frame_dir).unwrap();
            if before.model.as_deref().and_then(|m| cat.dual_ethernet(m)) == Some(true) {
                assert!(r.changes.iter().all(|c| c.item != "ethernet"), "la ETH2 no se toca");
                let after = config.file_config(&copy, Some(&before.name)).unwrap().targets.remove(0);
                assert_eq!(after.other_values, before.other_values);
            }
            let b1 = backend.build().unwrap().build_file(&copy, None).unwrap();
            eprintln!("extra build antes {}/{}, después {}/{}", b0.errors, b0.warnings, b1.errors, b1.warnings);
            assert_eq!((b1.errors, b1.warnings), (b0.errors, b0.warnings));
        }
        backend.shutdown().unwrap();
    }

    #[test]
    fn live_config_set_dual_ethernet() {
        // Con VIJEO_MCP_LIVE_DUAL=<.vdz> de un modelo con ETH2 (HMIG3U…): cambiar la descarga no toca la ETH2,
        // aunque tenga la misma IP (como PROYC). No lanza Vijeo.
        use vijeo_core::{DownloadChanges, TargetConfigChanges};
        let Some(src) = std::env::var_os("VIJEO_MCP_LIVE_DUAL").map(PathBuf::from) else { return };
        let backend = backend();
        let config = backend.config().unwrap();
        let copy = work_dir("config_dual").join("dual.vdz");
        std::fs::copy(&src, &copy).unwrap();
        let before = config.file_config(&copy, None).unwrap().targets.remove(0);
        let cat = Catalog::load(&installation().unwrap().frame_dir).unwrap();
        assert_eq!(before.model.as_deref().and_then(|m| cat.dual_ethernet(m)), Some(true), "{:?} no tiene ETH2", before.model);
        let ip: std::net::Ipv4Addr = before.download.ip_address.as_deref().unwrap().parse().unwrap();
        let new_ip = std::net::Ipv4Addr::from(u32::from(ip) ^ 1).to_string();
        let ch = TargetConfigChanges { download: Some(DownloadChanges { ip_address: Some(new_ip.clone()), ..Default::default() }), equipment: vec![] };
        let r = config.edit_file_config(&copy, Some(&before.name), &ch, None, false, false).unwrap();
        eprintln!("{:#?}", r.changes);
        assert!(r.written);
        assert!(r.changes.iter().all(|c| c.item == "download"), "la ETH2 no se toca");
        let after = config.file_config(&copy, Some(&before.name)).unwrap().targets.remove(0);
        assert_eq!(after.download.ip_address.as_deref(), Some(new_ip.as_str()));
        assert_eq!(after.other_values, before.other_values);
    }

    #[test]
    fn live_save_keeps_project() {
        // Guardar no añade drivers (el SoMachineNetwork01 del modo COM) y conserva el nombre del proyecto.
        let Some(src) = modbus() else { return };
        let backend = backend();
        let dir = work_dir("save");
        let vdz = dir.join("modbus.vdz");
        std::fs::copy(&src, &vdz).unwrap();
        let info = backend.projects().unwrap().open(&vdz).unwrap();
        let p = info.id;
        backend.build().unwrap().build_project(p).unwrap();
        let saved = dir.join("saved.vdz");
        let s = backend.projects().unwrap().save(p, Some(&saved), false).unwrap();
        assert_eq!(s.path.as_deref(), Some(saved.as_path()));
        backend.projects().unwrap().close(p, false).unwrap();

        let ids = |f: &std::path::Path| -> Vec<Option<String>> {
            let mut c = vijeo_backend_vd63::swx::open_vdz(f).unwrap();
            vijeo_backend_vd63::swx::read_targets(&mut c, None).into_iter().flat_map(|t| t.drivers).map(|d| d.driver_id).collect()
        };
        assert_eq!(ids(&saved), ids(&src));
        assert_eq!(vijeo_backend_vd63::swx::project_name(&saved).unwrap(), vijeo_backend_vd63::swx::project_name(&src).unwrap());
        let again = backend.build().unwrap().build_file(&saved, None).unwrap();
        assert_eq!((again.errors, again.warnings), (2, 2), "{again:#?}");
        backend.shutdown().unwrap();
    }

    #[test]
    fn live_parallel_opens_are_serialized() {
        let Some(src) = sample() else { return };
        let backend = backend();
        let dir = work_dir("parallel");
        let handles: Vec<_> = (0..2)
            .map(|i| {
                let vdz = dir.join(format!("p{i}.vdz"));
                std::fs::copy(&src, &vdz).unwrap();
                let b = backend.clone();
                std::thread::spawn(move || b.projects().unwrap().open(&vdz).map(|info| info.id))
            })
            .collect();
        let ids: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        for id in &ids {
            if let Ok(p) = id {
                backend.projects().unwrap().close(*p, true).unwrap();
            }
        }
        backend.shutdown().unwrap();
        assert!(ids.iter().all(Result::is_ok), "{ids:?}");
    }

    #[test]
    fn live_rename_persists() {
        let backend = backend();
        let dir = work_dir("rename");
        let vdz = dir.join("Renombrar.vdz");
        let projects = backend.projects().unwrap();
        let targets = backend.targets().unwrap();
        let p = projects.new_project("Renombrar", &vdz, false, "Uno", &sel("Model_HMIS5T", None)).unwrap().id;
        targets.add(p, "Dos", &sel("Model_HMIS5T", None)).unwrap();

        assert_eq!(targets.rename(p, "uno", "dos").unwrap_err().code(), "invalid_argument", "duplicado");
        assert_eq!(targets.rename(p, "uno", "Uno").unwrap().name, "Uno", "mismo nombre: no hace nada");
        assert_eq!(targets.rename(p, "uno", "Principal").unwrap().name, "Principal");
        assert!(targets.rename(p, "Nada", "X").is_err());
        projects.save(p, None, false).unwrap();
        let info = projects.rename(p, "Renombrado_1").unwrap();
        assert_eq!(info.name, "Renombrado_1");
        assert!(info.dirty, "cambio sin guardar tras renombrar");
        assert_eq!(projects.close(p, false).unwrap_err().code(), "refused");
        assert!(!projects.save(p, None, false).unwrap().dirty);
        projects.close(p, false).unwrap();

        let info = projects.open(&vdz).unwrap();
        assert_eq!(info.name, "Renombrado_1");
        let names: Vec<&str> = info.targets.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Principal", "Dos"]);
        projects.close(info.id, true).unwrap();
        backend.shutdown().unwrap();
    }

    /// Smart objects (decimocuarta tanda): inserta `DateTime\Date_Display_01` y `Time_Display_01` de la Toolchest con
    /// sus propiedades en el panel Alarms del ejemplo Hardwired, lee las del ejemplo (panel DateAndTime), cambia
    /// propiedades con panel_edit (dry_run y de verdad), exporta el XML para ver cómo lo entiende Vijeo, guarda y
    /// compila la copia aparte: sin el error 4336 (el 4230 ya lo trae el ejemplo).
    #[test]
    fn live_smart_object_properties() {
        use vijeo_core::{PanelEditRequest, PanelInsertRequest, PanelObject};
        let Some(src) = sample() else { return };
        fn find<'a>(objs: &'a [PanelObject], name: &str) -> Option<&'a PanelObject> {
            objs.iter().find_map(|o| if o.name == name { Some(o) } else { find(&o.children, name) })
        }
        fn prop<'a>(o: &'a PanelObject, name: &str) -> &'a str {
            o.properties.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{} sin {name}: {:?}", o.name, o.properties)).value.as_str()
        }
        let b = backend();
        let clip_before = vijeo_ui::clipboard::get_text();
        let info = b.projects().unwrap().open(&src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());
        let panels = b.panels().unwrap();
        let d = "MyController.DateAndTime";

        // Las del ejemplo, como las exporta Vijeo (corpus/xml: VariableType 1, DateFormat 2, variables iGetRTC*).
        let objs = panels.objects(p, &target, "DateAndTime").unwrap();
        let ex = find(&objs.objects, "Date_Display_3D_01").unwrap();
        eprintln!("{}", serde_json::to_string_pretty(ex).unwrap());
        assert_eq!((ex.kind.as_str(), ex.smart_object.as_deref()), ("SmartObject", Some("ISHEN190804115521")));
        assert_eq!((prop(ex, "VariableType"), prop(ex, "DateFormat")), ("1", "2"));
        assert_eq!(prop(ex, "DayVariable"), format!("{d}.iGetRTCDay"));
        assert_eq!(prop(ex, "DateVariable"), "");
        assert!(ex.strings.is_empty());

        // Variables que no existen: se rechaza antes de tocar Vijeo.
        let bad: PanelInsertRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Alarms",
            "items": [{"toolchest": r"DateTime\Date_Display_01", "x": 10, "y": 40, "properties": {"DayVariable": "NoExiste.iDia"}}]
        }))
        .unwrap();
        assert!(panels.insert(&bad).unwrap_err().to_string().contains("NoExiste.iDia"));

        let ins: PanelInsertRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Alarms",
            "items": [
                {"toolchest": r"DateTime\Date_Display_01", "x": 10, "y": 40, "name": "McpFecha",
                 "properties": {"VariableType": "Separate Variables", "DateFormat": "dd/mm/yyyy", "DayVariable": format!("{d}.iGetRTCDay"),
                                "MonthVariable": format!("{d}.iGetRTCMonth"), "YearVariable": format!("{d}.iGetRTCYear"), "TextColor": "0000FF"}},
                {"toolchest": r"DateTime\Time_Display_01", "x": 130, "y": 40, "name": "McpHora",
                 "properties": {"VariableType": "1", "HourVariable": format!("{d}.iGetRTCHour"), "MinuteVariable": format!("{d}.iGetRTCMin"),
                                "SecondVariable": format!("{d}.iGetRTCSec"), "BgColor": "FFFF00"}}
            ]
        }))
        .unwrap();
        let r = panels.insert(&ins).unwrap();
        eprintln!("{:#?}\n{:?}\n{:?}", r.changes, r.warnings, r.notes);
        let f = find(&r.objects, "McpFecha").expect("McpFecha");
        assert_eq!((f.kind.as_str(), f.left, f.top), ("SmartObject", 10, 40));
        assert_eq!((prop(f, "VariableType"), prop(f, "DateFormat"), prop(f, "TextColor")), ("1", "0", "0000FF"));
        assert_eq!(prop(f, "YearVariable"), format!("{d}.iGetRTCYear"));
        let h = find(&r.objects, "McpHora").expect("McpHora");
        assert_eq!((prop(h, "SecondVariable"), prop(h, "BgColor")), (format!("{d}.iGetRTCSec").as_str(), "FFFF00"));

        // panel_edit: dry_run no cambia nada; luego de verdad.
        let ed = |dry_run: bool| -> PanelEditRequest {
            serde_json::from_value(serde_json::json!({
                "project": p.0, "target": target, "panel": "Alarms", "dry_run": dry_run,
                "operations": [{"op": "set_properties", "object": "McpFecha",
                                "properties": {"YearVariable": format!("{d}.iSetRTCYear"), "DateFormat": "4", "BgColor": "C0C0C0"}}]
            }))
            .unwrap()
        };
        let dry = panels.edit(&ed(true)).unwrap();
        assert!(!dry.applied);
        assert_eq!(prop(find(&dry.objects, "McpFecha").unwrap(), "YearVariable"), format!("{d}.iSetRTCYear"));
        assert_eq!(prop(find(&panels.objects(p, &target, "Alarms").unwrap().objects, "McpFecha").unwrap(), "YearVariable"), format!("{d}.iGetRTCYear"));
        let e = panels.edit(&ed(false)).unwrap();
        eprintln!("{:?} {:?}", e.changes, e.warnings);
        assert!(e.applied && e.warnings.is_empty(), "{:?}", e.warnings);
        let f = find(&e.objects, "McpFecha").unwrap();
        assert_eq!((prop(f, "YearVariable"), prop(f, "DateFormat"), prop(f, "BgColor")), (format!("{d}.iSetRTCYear").as_str(), "4", "C0C0C0"));
        let wrong: PanelEditRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Alarms",
            "operations": [{"op": "set_properties", "object": "McpFecha", "properties": {"MonthVariable": "MyController.NoExiste"}}]
        }))
        .unwrap();
        assert!(panels.edit(&wrong).unwrap_err().to_string().contains("MyController.NoExiste"));

        // Cómo lo entiende Vijeo: su propio XML.
        let dir = work_dir("smart");
        let x = b.reports().unwrap().export_xml(p, &dir.join("xml"), true).unwrap();
        let xml = std::fs::read_to_string(&x.path).unwrap();
        for name in ["McpFecha", "McpHora"] {
            let at = xml.find(&format!("Name=\"{name}\"")).unwrap_or_else(|| panic!("{name} no está en el XML"));
            let end = xml[at..].find("</SmartObject>").map(|e| at + e).unwrap();
            let so = &xml[at..end];
            eprintln!("---- {name}\n{so}");
            if name == "McpFecha" {
                assert!(so.contains("SOItem=\"DATEDISP220804184201\""), "{so}");
                assert!(so.contains(&format!("ID=\"YearVariable\" Type=\"Variable\" Expression=\"{d}.iSetRTCYear\"")), "{so}");
                assert!(so.contains(&format!("ID=\"DayVariable\" Type=\"Variable\" Expression=\"{d}.iGetRTCDay\"")), "{so}");
                assert!(so.contains("ID=\"DateFormat\" Type=\"Combo\" ComboIndex=\"4\""), "{so}");
                assert!(so.contains("ID=\"BgColor\" Type=\"Color\" PlaceholderColor=\"C0C0C0\""), "{so}");
            } else {
                assert!(so.contains(&format!("ID=\"SecondVariable\" Type=\"Variable\" Expression=\"{d}.iGetRTCSec\"")), "{so}");
            }
        }

        let vdz = dir.join("McpSmart.vdz");
        b.projects().unwrap().save(p, Some(&vdz), true).unwrap();
        b.projects().unwrap().close(p, false).unwrap();
        assert_eq!(vijeo_ui::clipboard::get_text(), clip_before, "el texto del portapapeles vuelve a estar");
        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("{:?}", built.messages);
        for m in built.messages.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error) {
            assert!(m.code == Some(4230), "error inesperado: {m:?}");
        }
        b.shutdown().unwrap();
    }

    /// Amplía el corpus de smart objects (no comprueba nada): con `VIJEO_MCP_DUMP=<carpeta>` copia el panel
    /// DateAndTime de los ejemplos (smart objects configurados, versión 9) y un panel con piezas de la Toolchest
    /// pegadas (versiones 4, 7 y 8 que Vijeo sube a la 9) y guarda los portapapeles como `.bin`.
    #[test]
    fn live_dump_smart_object_clips() {
        use vijeo_core::{PanelInsertRequest, PanelOps, ProjectOps};
        let Ok(out) = std::env::var("VIJEO_MCP_DUMP") else { return };
        let out = PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        let inst = installation().expect("Vijeo 6.3");
        let cfg = BackendConfig { cache_dir: Some(std::env::temp_dir().join("vijeo-mcp-live").join("no-cache")), ..Default::default() };
        let b = vijeo_backend_vd63::Vd63Backend::new(&inst, Support::Exact, &cfg).unwrap();
        let examples = [("", "Compact_Hardwired_M221.vdz"), ("MB_", "Compact_Modbus SL_M221_Book.vdz"), ("DR_", "MaterialWork_Drill_HW_M221.vdz")];
        for (prefix, file) in examples {
            let src = std::path::Path::new(EXAMPLES).join(file);
            if !src.is_file() {
                continue;
            }
            let info = ProjectOps::open(&b, &src).unwrap();
            let (p, target) = (info.id, info.targets[0].name.clone());
            let raw = b.copy_panel_raw(p, &target, "DateAndTime").unwrap().expect("DateAndTime con objetos");
            std::fs::write(out.join(format!("{prefix}DateAndTime.bin")), &raw).unwrap();
            if prefix.is_empty() {
                let parts = [
                    r"DateTime\Date_Display_01",
                    r"DateTime\Time_Display_01",
                    r"DateTime\Date_Display_3D_01",
                    r"TextDisplay\TextDisplay_01",
                    r"Security\FixedNameLogin",
                    r"Configuration\BrightnessH_01_01",
                ];
                let items: Vec<serde_json::Value> =
                    parts.iter().enumerate().map(|(i, id)| serde_json::json!({"toolchest": id, "x": 5 + 105 * (i as i32 % 3), "y": 40 + 60 * (i as i32 / 3)})).collect();
                let req: PanelInsertRequest = serde_json::from_value(serde_json::json!({"project": p.0, "target": target, "panel": "Alarms", "items": items})).unwrap();
                let r = PanelOps::insert(&b, &req).unwrap();
                eprintln!("{:?} {:?}", r.changes, r.warnings);
                let raw = b.copy_panel_raw(p, &target, "Alarms").unwrap().unwrap();
                std::fs::write(out.join("TC_smart.bin"), &raw).unwrap();
            }
            ProjectOps::close(&b, p, true).unwrap();
        }
    }

    /// Amplía el corpus de acciones (no comprueba nada): con `VIJEO_MCP_DUMP=<carpeta>` copia los paneles del
    /// ejemplo Modbus cuyos botones abren el popup P10001 (`PopPanel Open AtPosition`) y los guarda como `.bin`.
    #[test]
    fn live_dump_popup_clips() {
        use vijeo_core::ProjectOps;
        let Ok(out) = std::env::var("VIJEO_MCP_DUMP") else { return };
        let Some(src) = modbus() else { return };
        let out = PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        let inst = installation().expect("Vijeo 6.3");
        let cfg = BackendConfig { cache_dir: Some(std::env::temp_dir().join("vijeo-mcp-live").join("no-cache")), ..Default::default() };
        let b = vijeo_backend_vd63::Vd63Backend::new(&inst, Support::Exact, &cfg).unwrap();
        let info = ProjectOps::open(&b, &src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());
        for panel in ["ATV_Motor_67", "RFID_1"] {
            let raw = b.copy_panel_raw(p, &target, panel).unwrap().expect("panel con objetos");
            std::fs::write(out.join(format!("MB_{panel}.bin")), &raw).unwrap();
        }
        ProjectOps::close(&b, p, true).unwrap();
    }

    /// Diagnóstico: qué diálogo abre Import Variables (0xC419) con el nodo Variables seleccionado.
    #[test]
    fn live_probe_import_variables_dialog() {
        use vijeo_core::ProjectOps;
        let Some(src) = modbus() else { return };
        let inst = installation().expect("Vijeo 6.3");
        let cfg = BackendConfig { cache_dir: Some(std::env::temp_dir().join("vijeo-mcp-live").join("no-cache")), ..Default::default() };
        let b = vijeo_backend_vd63::Vd63Backend::new(&inst, Support::Exact, &cfg).unwrap();
        let info = ProjectOps::open(&b, &src).unwrap();
        let target = info.targets[0].name.clone();
        let probe = |when: &str| {
            let r = b.probe_command(info.id, &target, "Variables", 0xC419);
            if let Ok(l) = &r {
                l.iter().for_each(|x| eprintln!("  {x}"));
            }
            let opened = r.as_ref().is_ok_and(|l| l.iter().any(|x| x.contains("«Import Variables»")));
            eprintln!("{when}: diálogo abierto = {opened}");
            opened
        };
        assert!(probe("recién abierto"));
        // Tras un Build All Vijeo deja activa la Feedback Zone; hasta la tanda 25 el diálogo ya no se abría porque
        // MFC descartaba el comando por deshabilitado. Con WM_MOUSEACTIVATE en RemoteTree::focus se abre.
        vijeo_core::BuildOps::build_project(&b, info.id).unwrap();
        let after_build = probe("tras Build All");
        ProjectOps::close(&b, info.id, true).unwrap();
        assert!(after_build, "tras Build All el diálogo también tiene que abrirse");
    }

    /// Pantallas sin el XML: en una copia del ejemplo Modbus se crea con Vijeo una carpeta en Base Panels (New Folder
    /// 0xC07B) con un panel y una subcarpeta dentro. `vdz_panel_list` con `project` y luego del `.vdz` guardado da lo
    /// mismo que el XML exportado (grupo, ruta, nombre, id) y project_goto abre el panel de la carpeta con su
    /// `panel_ref`. El `.vdz` resultante es el `McpCarpetas.vdz` del corpus de `docs/`.
    #[test]
    fn live_offline_panel_list() {
        use vijeo_core::{ConfigOps, ProjectOps, TargetOps};
        let Some(src) = modbus() else { return };
        let inst = installation().expect("Vijeo 6.3");
        let cfg = BackendConfig { cache_dir: Some(std::env::temp_dir().join("vijeo-mcp-live").join("no-cache")), ..Default::default() };
        let b = vijeo_backend_vd63::Vd63Backend::new(&inst, Support::Exact, &cfg).unwrap();
        let dir = work_dir("probe_folders");
        let info = ProjectOps::open(&b, &src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());
        let base = ["Graphical Panels", "Base Panels"];
        eprintln!("{:?}", b.probe_node_command(p, &target, &base, 0xC07B));
        eprintln!("{:?}", b.probe_node_command(p, &target, &["Graphical Panels", "Base Panels", "Folder1"], 0xC065));
        eprintln!("{:?}", b.probe_node_command(p, &target, &["Graphical Panels", "Base Panels", "Folder1"], 0xC07B));
        let open = ConfigOps::project_screens(&b, p, None).unwrap();
        eprintln!("{open:#?}");
        let inside: Vec<_> = open.iter().filter(|s| s.path.contains('\\') && s.group == "Base Panels").collect();
        assert_eq!(inside.len(), 1, "{open:#?}");
        let panel_ref = inside[0].panel_ref.clone().unwrap();
        assert!(panel_ref.starts_with(r"BP:\Folder1\"), "{panel_ref}");

        let xml = vijeo_core::ReportOps::export_xml(&b, p, &dir.join("export"), true).unwrap();
        let text = std::fs::read_to_string(&xml.path).unwrap();
        let doc = vijeo_backend_vd63::vxml::parse(&text).unwrap();
        let mut expected: Vec<(String, String, String)> =
            vijeo_backend_vd63::vxml::targets(&doc).into_iter().flat_map(vijeo_backend_vd63::vxml::panel_ids).map(|x| (x.group, x.path, x.id)).collect();
        let key = |all: &[vijeo_core::ScreenInfo]| {
            let mut v: Vec<(String, String, String)> = all.iter().map(|s| (s.group.clone(), s.path.clone(), format!("P{}", s.number))).collect();
            v.sort();
            v
        };
        expected.sort();
        assert_eq!(key(&open), expected, "vdz_panel_list (project) / XML");
        assert_eq!(TargetOps::open_panel(&b, p, &target, &panel_ref).unwrap().path, panel_ref);

        let out = dir.join("McpCarpetas.vdz");
        ProjectOps::save(&b, p, Some(&out), true).unwrap();
        ProjectOps::close(&b, p, true).unwrap();
        let file = ConfigOps::file_screens(&b, &out, None).unwrap();
        assert_eq!(key(&file), expected, "vdz_panel_list (path) / XML");
    }


    /// Varios Import Variables seguidos en la misma instancia y, después de un Build All, las herramientas del Navigator.
    /// Antes fallaban: Vijeo activa la Feedback Zone al importar o compilar y MFC descartaba los comandos del Navigator
    /// por deshabilitados; ahora `RemoteTree::focus` manda `WM_MOUSEACTIVATE` como un clic real.
    #[test]
    fn live_navigator_after_import_and_build() {
        use vijeo_core::{BuildOps, FileOps, PanelKind, PanelOps, ProjectOps, ReportOps, TargetOps, VariableCsvRequest, VariableImportRequest, VariableOps};
        let Some(src) = modbus() else { return };
        let inst = installation().expect("Vijeo 6.3");
        let cfg = BackendConfig { cache_dir: Some(std::env::temp_dir().join("vijeo-mcp-live").join("no-cache")), ..Default::default() };
        let b = vijeo_backend_vd63::Vd63Backend::new(&inst, Support::Exact, &cfg).unwrap();
        let dir = work_dir("navigator-after-build");
        let info = ProjectOps::open(&b, &src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());
        let import = |n: usize| {
            let path = dir.join(format!("v{n}.csv"));
            let vars = serde_json::json!([{"name": format!("McpNav.V{n}"), "data_type": "INT", "initial_value": "1"}]);
            let req: VariableCsvRequest = serde_json::from_value(serde_json::json!({"path": path, "variables": vars, "language": "English"})).unwrap();
            FileOps::variables_csv(&b, &req).unwrap();
            let imp: VariableImportRequest = serde_json::from_value(serde_json::json!({"project": p.0, "csv": path})).unwrap();
            let r = VariableOps::import_csv(&b, &imp).unwrap();
            assert_eq!(r.imported, Some(1), "import {n}: {r:?}");
        };
        import(1);
        import(2);
        ReportOps::export_xml(&b, p, &dir.join("xml"), true).unwrap();
        import(3);

        let built = BuildOps::build_project(&b, p).unwrap();
        eprintln!("build: {} errores", built.errors);
        import(4);
        let created = TargetOps::create_panel(&b, p, &target, PanelKind::Base, Some("TrasBuild")).unwrap();
        assert_eq!(created.name, "TrasBuild");
        let opened = TargetOps::open_panel(&b, p, &target, r"BP:\Start").unwrap();
        eprintln!("abierto: {opened:?}");
        let renamed = TargetOps::rename_panel(&b, p, &target, PanelKind::Base, "TrasBuild", "TrasBuild2").unwrap();
        eprintln!("renombrado: {renamed:?}");
        let objs = PanelOps::objects(&b, p, &target, "Start").unwrap();
        assert!(!objs.objects.is_empty());
        let deleted = TargetOps::delete_panel(&b, p, &target, PanelKind::Base, "TrasBuild2", false).unwrap();
        eprintln!("borrado: {deleted:?}");
        let req: vijeo_core::EquipmentAddRequest =
            serde_json::from_value(serde_json::json!({"project": p.0, "target": target, "driver": "modbustcp", "name": "PLC2", "ip_address": "172.20.10.170", "unit_id": 3})).unwrap();
        let eq = TargetOps::add_equipment(&b, &req).unwrap();
        assert_eq!(eq.equipment, "PLC2");
        let vars = vijeo_core::ConfigOps::project_variables(&b, p, None, false, false).unwrap();
        assert_eq!(vars.iter().filter(|v| v.name.starts_with("McpNav.")).count(), 4);
        ProjectOps::close(&b, p, true).unwrap();
    }


    /// Un CSV de variables_generate_csv se importa en Vijeo (Import Variables) y las variables quedan como
    /// se pidieron; el proyecto sigue compilando igual. Sobre el ejemplo Modbus (equipo M221_Controller,
    /// grupo ScanGroup1).
    #[test]
    fn live_generated_csv_imports() {
        use vijeo_core::{BuildOps, FileOps, ProjectOps, ConfigOps, VariableCsvRequest, VariableImportRequest, VariableOps};
        let Some(src) = modbus() else { return };
        let inst = installation().expect("Vijeo 6.3");
        let cfg = BackendConfig { cache_dir: Some(std::env::temp_dir().join("vijeo-mcp-live").join("no-cache")), ..Default::default() };
        let b = vijeo_backend_vd63::Vd63Backend::new(&inst, Support::Exact, &cfg).unwrap();
        let dir = work_dir("csv-import");
        let info = ProjectOps::open(&b, &src).unwrap();
        let p = info.id;
        let target = info.targets[0].name.clone();
        // El ejemplo Modbus compila con 2 errores y 2 warnings (sin el 1331 del modo SoMachine). Se compila
        // después de importar (live_navigator_after_import_and_build prueba importar después de compilar).
        let before = (2, 2);

        let ext =|name: &str, t: &str, addr: &str| serde_json::json!({"name": name, "data_type": t, "address": addr, "scan_group": "ScanGroup1"});
        let mut vars = vec![
            ext("McpCsv.Marcha", "BOOL", "%M100"),
            ext("McpCsv.Nivel", "INT", "%MW201"),
            ext("McpCsv.Contador", "UINT", "%MW202"),
            ext("McpCsv.Total", "DINT", "%MW204"),
            ext("McpCsv.Energia", "UDINT", "%MW206"),
            ext("McpCsv.Temp", "REAL", "%MW208"),
            ext("McpCsv.Generico", "Integer", "%MW210"),
            serde_json::json!({"name": "McpCsv.Bit3", "data_type": "BOOL", "address": "%MW212", "bit": "X3", "scan_group": "ScanGroup1"}),
            serde_json::json!({"name": "McpCsv.Texto", "data_type": "STRING", "address": "%MW220", "string_length": 10, "scan_group": "ScanGroup1"}),
            serde_json::json!({"name": "McpCsv.Interno.Consigna", "data_type": "INT", "initial_value": "25", "retentive": true, "description": "Consigna «año» ñ ó"}),
            serde_json::json!({"name": "McpCsv.Interno.Factor", "data_type": "REAL", "initial_value": "1.5"}),
            serde_json::json!({"name": "McpCsv.Interno.Flag", "data_type": "BOOL", "initial_value": "On", "sharing": "Read Only"}),
            serde_json::json!({"name": "McpCsv.Interno.Mensaje", "data_type": "STRING", "initial_value": "hola, \"mundo\"", "string_length": 20}),
            serde_json::json!({"name": "McpCsv.Interno.Alarma", "data_type": "BOOL", "alarm": {"message": "Alarma de prueba: nivel bajo", "trigger": "when low", "severity": 5}}),
        ];
        vars[0]["alarm"] = serde_json::json!({"message": "Motor en marcha", "severity": 2});
        let csv = dir.join("vars.csv");
        let req: VariableCsvRequest = serde_json::from_value(serde_json::json!({"path": csv, "project": p.0, "variables": vars})).unwrap();
        let gen = FileOps::variables_csv(&b, &req).unwrap();
        eprintln!("CSV: {gen:?}");
        assert!(gen.warnings.is_empty(), "{:?}", gen.warnings);

        let imp: VariableImportRequest = serde_json::from_value(serde_json::json!({"project": p.0, "csv": csv})).unwrap();
        let r = VariableOps::import_csv(&b, &imp).unwrap();
        eprintln!("Vijeo: {r:?}");
        assert_eq!(r.imported, Some(vars.len()), "{r:?}");
        assert_eq!(r.target, target);
        // La lectura offline del proyecto abierto ya las ve, sin project_save (el .vdz no cambió).
        let fresh = ConfigOps::project_variables(&b, p, None, false, false).unwrap();
        assert_eq!(fresh.iter().filter(|v| v.name.starts_with("McpCsv.")).count(), vars.len());
        assert_eq!(ConfigOps::file_variables(&b, &src, None, false, false).unwrap().iter().filter(|v| v.name.starts_with("McpCsv.")).count(), 0);

        // Con las variables en la petición: una nueva y una que ya existe (no se toca sin replace_existing).
        let more = serde_json::json!([
            {"name": "McpCsv.Interno.Extra", "data_type": "DINT", "initial_value": "7"},
            {"name": "McpCsv.Interno.Factor", "data_type": "DINT", "initial_value": "9"},
        ]);
        let imp: VariableImportRequest = serde_json::from_value(serde_json::json!({"project": p.0, "variables": more})).unwrap();
        let r = VariableOps::import_csv(&b, &imp).unwrap();
        eprintln!("Vijeo (variables): {r:?}");
        assert!(r.warnings.iter().any(|w| w.contains("McpCsv.Interno.Factor")), "{:?}", r.warnings);
        assert!(r.imported.is_some(), "{r:?}");
        vars.push(serde_json::json!({"name": "McpCsv.Interno.Extra"}));
        let got: Vec<_> = ConfigOps::project_variables(&b, p, Some(&target), false, false).unwrap().into_iter().filter(|v| v.name.starts_with("McpCsv.")).collect();
        for v in &got {
            eprintln!("{} {} {} {:?} {:?} {:?}", v.name, v.data_type, v.source, v.address, v.description, v.extra);
        }
        assert_eq!(got.len(), vars.len(), "se importaron {} de {}", got.len(), vars.len());
        let get = |n: &str| got.iter().find(|v| v.name == n).unwrap_or_else(|| panic!("falta {n}"));
        assert_eq!((get("McpCsv.Nivel").data_type.as_str(), get("McpCsv.Nivel").address.as_deref()), ("INT", Some("%MW201")));
        assert_eq!(get("McpCsv.Temp").data_type, "REAL");
        assert_eq!(get("McpCsv.Interno.Consigna").source, "Internal");
        assert_eq!(get("McpCsv.Interno.Consigna").description.as_deref(), Some("Consigna «año» ñ ó"), "acentos en OEM");
        assert_eq!(get("McpCsv.Marcha").extra.get("Alarms").map(String::as_str), Some("1"));
        assert_eq!(get("McpCsv.Interno.Extra").data_type, "DINT");
        assert_eq!(get("McpCsv.Interno.Factor").data_type, "REAL", "sin replace_existing no se toca");

        let after = BuildOps::build_project(&b, p).unwrap();
        eprintln!("build: el original da {}/{}, con las variables importadas {}/{}", before.0, before.1, after.errors, after.warnings);
        for m in &after.messages {
            eprintln!("  {m:?}");
        }
        assert_eq!(after.errors, before.0, "{:?}", after.messages);
        ProjectOps::close(&b, p, true).unwrap();
        std::fs::copy(&csv, std::env::temp_dir().join("vijeo-mcp-live").join("ultimo-import.csv")).ok();
    }

    /// equipment_add sobre el ejemplo Modbus (driver ModbusTCP con M221_Controller): un PLC más en ModbusTCP con IP y
    /// Unit ID, un driver Modbus (RTU) nuevo con nombres, errores (IP en un equipo serie: se deshace; nombre no válido;
    /// driver que no existe; nombre repetido), la copia guardada tiene las direcciones y compila como el original. Y en
    /// un proyecto nuevo, un driver Modbus TCP desde cero.
    #[test]
    fn live_equipment_add() {
        use vijeo_core::{EquipmentAddRequest, EquipmentAddResult};
        let Some(src) = modbus() else { return };
        let b = backend();
        let base = b.build().unwrap().build_file(&src, None).unwrap();
        let errs = |m: &[vijeo_core::BuildMessage]| {
            let mut v: Vec<_> = m.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).map(|m| m.code).collect();
            v.sort();
            v
        };
        let tops = b.targets().unwrap();
        let add = |v: serde_json::Value| -> vijeo_core::VijeoResult<EquipmentAddResult> {
            let req: EquipmentAddRequest = serde_json::from_value(v).unwrap();
            let r = tops.add_equipment(&req);
            eprintln!("{r:#?}");
            r
        };
        let info = b.projects().unwrap().open(&src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());

        let r = add(serde_json::json!({"project": p.0, "target": target, "driver": "modbustcp", "name": "PLC2", "ip_address": "172.20.10.170", "unit_id": 3})).unwrap();
        assert!(!r.created_driver);
        assert_eq!((r.equipment.as_str(), r.driver.name.as_str()), ("PLC2", "ModbusTCP"));
        let e = r.driver.equipment.iter().find(|e| e.name == "PLC2").unwrap();
        assert_eq!((e.ip_address.as_deref(), e.unit_id, e.port), (Some("172.20.10.170"), Some(3), Some(502)), "{e:?}");
        assert!(r.driver.equipment.iter().any(|e| e.name == "M221_Controller"));

        let r = add(serde_json::json!({"project": p.0, "target": target, "driver": "Modbus (RTU)", "driver_name": "SerieRTU", "name": "Variador1"})).unwrap();
        assert!(r.created_driver);
        assert_eq!((r.driver.name.as_str(), r.equipment.as_str()), ("SerieRTU", "Variador1"));
        // `Schneider_ModBus_PDL_ModBus_SIO`.
        assert!(r.driver.driver_id.as_deref().is_some_and(|d| d.to_lowercase().contains("modbus") && d.contains("SIO")), "{:?}", r.driver.driver_id);

        let count = || -> usize { b.config().unwrap().project_config(p, Some(&target)).map(|c| c.targets[0].drivers.iter().map(|d| d.equipment.len()).sum()).unwrap_or(0) };
        let before = count();
        let e = add(serde_json::json!({"project": p.0, "target": target, "driver": "SerieRTU", "name": "Variador2", "ip_address": "10.0.0.1"})).unwrap_err();
        assert_eq!(e.code(), "invalid_argument", "{e}");
        for (v, field) in [
            (serde_json::json!({"project": p.0, "target": target, "driver": "ModbusTCP", "name": "PLC 3"}), "name"),
            (serde_json::json!({"project": p.0, "target": target, "driver": "zzz-no-existe"}), "driver"),
            (serde_json::json!({"project": p.0, "target": target, "driver": "ModbusTCP", "name": "plc2"}), "name"),
            (serde_json::json!({"project": p.0, "target": target, "driver": "ModbusTCP", "ip_address": "300.1.1.1"}), "ip_address"),
        ] {
            let e = add(v).unwrap_err();
            assert!(e.to_string().contains(field), "{field}: {e}");
        }
        assert_eq!(count(), before, "un error dejó equipos de más");

        let dir = work_dir("equipment_add");
        let vdz = dir.join("McpEquipos.vdz");
        b.projects().unwrap().save(p, Some(&vdz), true).unwrap();
        b.projects().unwrap().close(p, false).unwrap();
        let cfg = b.config().unwrap().file_config(&vdz, None).unwrap();
        eprintln!("{:#?}", cfg.targets[0].drivers);
        let plc2 = cfg.targets[0].drivers.iter().flat_map(|d| &d.equipment).find(|e| e.name == "PLC2").expect("PLC2 en el .vdz");
        assert_eq!((plc2.ip_address.as_deref(), plc2.unit_id), (Some("172.20.10.170"), Some(3)));
        assert!(cfg.targets[0].drivers.iter().any(|d| d.name == "SerieRTU" && d.equipment.iter().any(|e| e.name == "Variador1")));
        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("original: {:?}\ncopia: {:?}", base.messages, built.messages);
        assert_eq!(errs(&built.messages), errs(&base.messages));

        // Proyecto nuevo: Modbus TCP desde cero (New Driver lo ofrece porque el target no lo tiene).
        let nuevo = dir.join("McpNuevoEquipos.vdz");
        let model: vijeo_core::ModelSelector = serde_json::from_value(serde_json::json!({"model": "HMIGTO5310"})).unwrap();
        let p2 = b.projects().unwrap().new_project("McpNuevoEquipos", &nuevo, true, "HMI1", &model).unwrap().id;
        let r = add(serde_json::json!({"project": p2.0, "target": "HMI1", "driver": "Modbus TCP", "driver_name": "Red", "name": "PLC1", "ip_address": "192.168.1.10", "unit_id": 1}));
        let r = r.unwrap();
        assert!(r.created_driver);
        let e = &r.driver.equipment[0];
        assert_eq!((r.driver.name.as_str(), e.name.as_str(), e.ip_address.as_deref(), e.unit_id), ("Red", "PLC1", Some("192.168.1.10"), Some(1)), "{:?}", r.driver);
        b.projects().unwrap().close(p2, true).unwrap();
        b.shutdown().unwrap();
    }

    /// vdz_copy, vdz_rename y vdz_delete sin Vijeo sobre el ejemplo Modbus; luego Vijeo abre la copia renombrada con su
    /// nombre nuevo y la compila con los errores del original. El borrado manda un .vdz de prueba propio a la Papelera.
    #[test]
    fn live_vdz_files() {
        let Some(src) = modbus() else { return };
        let b = backend();
        let errs = |m: &[vijeo_core::BuildMessage]| {
            let mut v: Vec<_> = m.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).map(|m| m.code).collect();
            v.sort();
            v
        };
        let base = b.build().unwrap().build_file(&src, None).unwrap();
        let dir = work_dir("vdz_files");
        let files = b.files().unwrap();
        let (a, c) = (dir.join("McpCopia.vdz"), dir.join("McpRenombrado.vdz"));
        let _ = std::fs::remove_file(&c);
        let r = files.copy_vdz(&src, &a, None, true).unwrap();
        eprintln!("{r:?}");
        assert_eq!(r.name, "McpCopia");
        let r = files.rename_vdz(&a, Some(&c), Some("McpProyecto2"), false).unwrap();
        eprintln!("{r:?}");
        assert!(r.name == "McpProyecto2" && !a.exists() && c.exists());
        let info = b.projects().unwrap().open(&c).unwrap();
        eprintln!("abierto: {} {:?}", info.name, info.path);
        assert_eq!(info.name, "McpProyecto2");
        b.projects().unwrap().close(info.id, false).unwrap();
        let built = b.build().unwrap().build_file(&c, None).unwrap();
        assert_eq!(errs(&built.messages), errs(&base.messages));
        // Abierto en el MCP: ni renombrar ni borrar.
        let info = b.projects().unwrap().open(&c).unwrap();
        assert_eq!(files.delete_vdz(&c).unwrap_err().code(), "refused");
        assert_eq!(files.rename_vdz(&c, None, Some("Otro"), false).unwrap_err().code(), "refused");
        b.projects().unwrap().close(info.id, false).unwrap();
        let gone = dir.join("McpBorrar.vdz");
        files.copy_vdz(&src, &gone, None, true).unwrap();
        let r = files.delete_vdz(&gone).unwrap();
        eprintln!("{r:?}");
        assert!(!gone.exists());
        b.shutdown().unwrap();
    }

    /// Variables de cada target como las exporta Vijeo en el XML del proyecto abierto (la referencia de las lecturas sin
    /// Vijeo).
    fn xml_variables(b: &std::sync::Arc<dyn vijeo_core::Backend>, p: vijeo_core::ProjectId, dir: &std::path::Path) -> Vec<vijeo_core::VariableInfo> {
        let out = b.reports().unwrap().export_xml(p, &dir.join("xml"), true).unwrap();
        let text = vijeo_backend_vd63::reports::decode_text(&std::fs::read(&out.path).unwrap());
        let doc = vijeo_backend_vd63::vxml::parse(&text).unwrap();
        vijeo_backend_vd63::vxml::targets(&doc).into_iter().flat_map(vijeo_backend_vd63::vxml::variables).collect()
    }

    /// vdz_variable_edit sobre una copia del ejemplo Modbus, sin Vijeo: carpetas y variables nuevas (internas y externas
    /// de varios tipos), cambios (dirección, descripción, bytes) y un borrado. Luego Vijeo: la copia compila con los
    /// errores del original; abierta, su XML tiene las variables como se escribieron; y lo que guarda Vijeo se relee
    /// igual sin Vijeo.
    #[test]
    fn live_offline_variables() {
        use vijeo_core::{VariableInfo, VariablesEditRequest};
        let Some(src) = modbus() else { return };
        let b = backend();
        let errs = |m: &[vijeo_core::BuildMessage]| {
            let mut v: Vec<_> = m.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).map(|m| m.code).collect();
            v.sort();
            v
        };
        let base = b.build().unwrap().build_file(&src, None).unwrap();
        let dir = work_dir("offline_variables");
        let vdz = dir.join("McpVariables.vdz");
        let cfg = b.config().unwrap();
        let req: VariablesEditRequest = serde_json::from_value(serde_json::json!({
            "path": src, "save_as": vdz, "overwrite": true,
            "edits": [
                {"op": "add_folder", "path": "Mcp.Sub"},
                {"op": "add", "name": "Mcp.Contador", "data_type": "INT", "initial_value": -5, "description": "de prueba"},
                {"op": "add", "name": "Mcp.Sub.Marcha", "data_type": "BOOL", "source": "External", "address": "%M300"},
                {"op": "add", "name": "Mcp.Nivel", "data_type": "REAL", "initial_value": 1.5},
                {"op": "add", "name": "Mcp.Texto", "data_type": "STRING", "initial_value": "hola", "string_bytes": 16},
                {"op": "add", "name": "McpRaiz", "data_type": "DINT"},
                {"op": "add", "name": "Mcp.Largo", "data_type": "UDINT", "source": "External", "address": "%MD500"},
                {"op": "add", "name": "MyController.Energy.iNueva", "data_type": "UINT", "source": "External", "address": "%MW400"},
                {"op": "set", "name": "MyController.Energy.iTimeSec", "address": "%MW99", "description": "segundos"},
                {"op": "set", "name": "sRefStringForComErrCodes", "string_bytes": 30},
                {"op": "add", "name": "Mcp.Borrable", "data_type": "DINT"},
                {"op": "delete", "name": "Mcp.Borrable"}
            ]
        }))
        .unwrap();
        let t = std::time::Instant::now();
        let r = cfg.edit_file_variables(&req).unwrap();
        eprintln!("{:?} en {:?}\n{:#?}", r.changes, t.elapsed(), r.variables);
        assert!(r.written && r.variables.len() == 9, "{r:?}");
        // Borrar algo que usan las pantallas se niega sin force.
        let used: VariablesEditRequest =
            serde_json::from_value(serde_json::json!({"path": vdz, "dry_run": true, "edits": [{"op": "delete", "name": "MyController.Energy.reTotalEnergy"}]})).unwrap();
        let e = cfg.edit_file_variables(&used).unwrap_err();
        eprintln!("{e}");
        assert!(e.code() == "invalid_argument" && e.to_string().contains("Panel"), "{e}");

        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("original: {:?}\ncopia: {:?}", base.messages, built.messages);
        assert_eq!(errs(&built.messages), errs(&base.messages));

        let offline = cfg.file_variables(&vdz, None, true, false).unwrap();
        let info = b.projects().unwrap().open(&vdz).unwrap();
        let xml = xml_variables(&b, info.id, &dir);
        let by = |all: &[VariableInfo], n: &str| all.iter().find(|v| v.name == n).cloned();
        for n in ["Mcp.Contador", "Mcp.Sub.Marcha", "Mcp.Nivel", "Mcp.Texto", "McpRaiz", "Mcp.Largo", "MyController.Energy.iNueva", "MyController.Energy.iTimeSec", "sRefStringForComErrCodes"] {
            let (x, o) = (by(&xml, n), by(&offline, n));
            eprintln!("{n}\n  XML:     {x:?}\n  offline: {o:?}");
            let (x, o) = (x.unwrap_or_else(|| panic!("{n} no está en el XML")), o.unwrap());
            assert_eq!((&x.data_type, &x.source, &x.address), (&o.data_type, &o.source, &o.address), "{n}");
            assert_eq!(x.description.as_deref().map(str::trim), o.description.as_deref().map(str::trim), "{n}");
            for k in ["ScanGroup", "InitialValue", "NoOfBytes"] {
                assert_eq!(x.extra.get(k), o.extra.get(k), "{n} {k}");
            }
        }
        assert!(by(&xml, "Mcp.Borrable").is_none() && by(&xml, "xRTCSyncDone").is_some());
        assert_eq!(xml.len(), cfg.file_variables(&vdz, None, false, false).unwrap().len());

        // Lo que guarda Vijeo se relee igual sin Vijeo.
        let resaved = dir.join("McpVariablesVijeo.vdz");
        b.projects().unwrap().save(info.id, Some(&resaved), true).unwrap();
        b.projects().unwrap().close(info.id, false).unwrap();
        let again = cfg.file_variables(&resaved, None, true, false).unwrap();
        let json = |v: &[VariableInfo]| serde_json::to_value(v).unwrap();
        assert_eq!(json(&again), json(&offline));
        b.shutdown().unwrap();
    }

    /// Usos sin Vijeo: en una copia del ejemplo Modbus se borra con force una variable que usa una pantalla; los
    /// enlaces que quedan rotos son los que Vijeo marca al compilar.
    #[test]
    fn live_offline_broken_links() {
        use vijeo_core::VariablesEditRequest;
        let Some(src) = modbus() else { return };
        let b = backend();
        let cfg = b.config().unwrap();
        let dir = work_dir("offline_broken");
        let vdz = dir.join("McpRotas.vdz");
        std::fs::copy(&src, &vdz).unwrap();
        let before = cfg.file_usage(&vdz, None, false).unwrap();
        assert!(before.broken.is_empty(), "{:?}", before.broken);
        let gone = "MyController.Energy.reTotalEnergy";
        let at = before.references.iter().find(|r| r.variable == gone).unwrap().references.clone();
        eprintln!("{gone}: {at:?}");
        let req: VariablesEditRequest = serde_json::from_value(serde_json::json!({"path": vdz, "edits": [{"op": "delete", "name": gone, "force": true}]})).unwrap();
        cfg.edit_file_variables(&req).unwrap();
        let after = cfg.file_usage(&vdz, None, false).unwrap();
        eprintln!("rotos: {:#?}", after.broken);
        let locations: Vec<&str> = after.broken.iter().map(|l| l.location.as_str()).collect();
        assert!(at.iter().all(|l| locations.contains(&l.as_str())), "{at:?} / {locations:?}");
        assert!(after.broken.iter().all(|l| l.name == "reTotalEnergy"));

        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        let errors: Vec<&vijeo_core::BuildMessage> = built.messages.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).collect();
        eprintln!("{errors:#?}");
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|m| m.text.contains("reTotalEnergy") || m.text.contains("TotalEnergy")), "{errors:?}");
        b.shutdown().unwrap();
    }

    /// Renombrar variables sin Vijeo (`vdz_variable_edit` con `rename`) en una copia del ejemplo Modbus: una de pantalla,
    /// una de una acción global y su script, y la del enclavamiento (en la raíz). La copia compila con los errores del
    /// original y, en el XML que exporta Vijeo, cada nombre nuevo está en los mismos sitios que el viejo en el original;
    /// el script usa los nombres nuevos. El parámetro de una referencia indirecta se niega.
    #[test]
    fn live_offline_rename_variables() {
        use vijeo_core::VariablesEditRequest;
        let Some(src) = modbus() else { return };
        let b = backend();
        let cfg = b.config().unwrap();
        let dir = work_dir("offline_rename");
        let vdz = dir.join("McpRenombrar.vdz");
        std::fs::copy(&src, &vdz).unwrap();
        let errs = |m: &[vijeo_core::BuildMessage]| {
            let mut v: Vec<_> = m.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).map(|m| m.code).collect();
            v.sort();
            v
        };
        let base = b.build().unwrap().build_file(&src, None).unwrap();
        let pairs = [("MyController.Energy.reTotalEnergy", "reEnergiaTotal"), ("MyController.DateAndTime.iGetRTCSec", "iSegundosRTC"), ("xRTCSyncDone", "xSincronizado")];
        let edits: Vec<serde_json::Value> = pairs.iter().map(|(n, l)| serde_json::json!({"op": "rename", "name": n, "new_name": l})).collect();
        let req: VariablesEditRequest = serde_json::from_value(serde_json::json!({"path": vdz, "edits": edits})).unwrap();
        let r = cfg.edit_file_variables(&req).unwrap();
        eprintln!("{:#?}", r.changes);
        assert!(r.written && r.changes.len() == 3, "{r:?}");
        let indirect: VariablesEditRequest =
            serde_json::from_value(serde_json::json!({"path": vdz, "dry_run": true, "edits": [{"op": "rename", "name": "sRefStringForComErrCodes", "new_name": "sRef"}]})).unwrap();
        let e = cfg.edit_file_variables(&indirect).unwrap_err();
        assert!(e.to_string().contains("referencias indirectas"), "{e}");

        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("original: {:?}\nrenombrado: {:?}", base.messages, built.messages);
        assert_eq!(errs(&built.messages), errs(&base.messages));

        let info = b.projects().unwrap().open(&vdz).unwrap();
        let out = b.reports().unwrap().export_xml(info.id, &dir.join("export"), true).unwrap();
        b.projects().unwrap().close(info.id, false).unwrap();
        let text = std::fs::read_to_string(&out.path).unwrap();
        let doc = vijeo_backend_vd63::vxml::parse(&text).unwrap();
        let xref = |doc: &roxmltree::Document, n: &str| -> Vec<String> {
            vijeo_backend_vd63::vxml::targets(doc).into_iter().flat_map(vijeo_backend_vd63::vxml::cross_references).filter(|r| r.variable == n).flat_map(|r| r.references).collect()
        };
        let orig_text = vijeo_backend_vd63::reports::decode_text(&std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/vd-6.3/corpus/xml/Compact_Modbus SL_M221 Book.vxml")).unwrap());
        let orig = vijeo_backend_vd63::vxml::parse(&orig_text).unwrap();
        for (old, leaf) in pairs {
            let new = match old.rsplit_once('.') {
                Some((p, _)) => format!("{p}.{leaf}"),
                None => leaf.to_owned(),
            };
            let (was, now) = (xref(&orig, old), xref(&doc, &new));
            eprintln!("{old}: {was:?}\n{new}: {now:?}");
            assert!(!was.is_empty());
            assert_eq!(now, was, "{new}");
            assert!(xref(&doc, old).is_empty());
        }
        let scripts: Vec<String> = vijeo_backend_vd63::vxml::targets(&doc).into_iter().flat_map(vijeo_backend_vd63::vxml::scripts).map(|s| s.2).collect();
        assert!(scripts.iter().any(|s| s.contains("MyController.DateAndTime.iSegundosRTC")) && !scripts.iter().any(|s| s.contains("iGetRTCSec")), "{scripts:?}");
        b.shutdown().unwrap();
    }

    /// Renombrar carpetas de variables sin Vijeo (`rename_folder`) en una copia del ejemplo Modbus: `MyController.Energy`
    /// (pantalla Energy) y `MyController.DateAndTime` (acción global con script, smart objects). La copia compila con los
    /// errores del original y, en el XML de Vijeo, cada variable de dentro está con su ruta nueva en los mismos sitios que
    /// en el original. Se niega con `MyController`, que nombra la referencia indirecta de `wRefComErrorCode`.
    #[test]
    fn live_offline_rename_folders() {
        use vijeo_core::VariablesEditRequest;
        let Some(src) = modbus() else { return };
        let b = backend();
        let cfg = b.config().unwrap();
        let dir = work_dir("offline_rename_folders");
        let vdz = dir.join("McpCarpetasVar.vdz");
        std::fs::copy(&src, &vdz).unwrap();
        let errs = |m: &[vijeo_core::BuildMessage]| {
            let mut v: Vec<_> = m.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).map(|m| m.code).collect();
            v.sort();
            v
        };
        let base = b.build().unwrap().build_file(&src, None).unwrap();
        let pairs = [("MyController.Energy", "Energia"), ("MyController.DateAndTime", "FechaHora")];
        let edits: Vec<serde_json::Value> = pairs.iter().map(|(p, n)| serde_json::json!({"op": "rename_folder", "path": p, "new_name": n})).collect();
        let req: VariablesEditRequest = serde_json::from_value(serde_json::json!({"path": vdz, "edits": edits})).unwrap();
        let r = cfg.edit_file_variables(&req).unwrap();
        eprintln!("{:#?}", r.changes);
        assert!(r.written && r.changes.len() == 2, "{r:?}");
        let root: VariablesEditRequest =
            serde_json::from_value(serde_json::json!({"path": vdz, "dry_run": true, "edits": [{"op": "rename_folder", "path": "MyController", "new_name": "Ctl"}]})).unwrap();
        let e = cfg.edit_file_variables(&root).unwrap_err();
        assert!(e.to_string().contains("wRefComErrorCode"), "{e}");

        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        assert_eq!(errs(&built.messages), errs(&base.messages), "{:?}", built.messages);

        let info = b.projects().unwrap().open(&vdz).unwrap();
        let out = b.reports().unwrap().export_xml(info.id, &dir.join("export"), true).unwrap();
        b.projects().unwrap().close(info.id, false).unwrap();
        let text = std::fs::read_to_string(&out.path).unwrap();
        let doc = vijeo_backend_vd63::vxml::parse(&text).unwrap();
        let orig_text = vijeo_backend_vd63::reports::decode_text(&std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/vd-6.3/corpus/xml/Compact_Modbus SL_M221 Book.vxml")).unwrap());
        let orig = vijeo_backend_vd63::vxml::parse(&orig_text).unwrap();
        let refs = |doc: &roxmltree::Document| -> std::collections::BTreeMap<String, Vec<String>> {
            vijeo_backend_vd63::vxml::targets(doc).into_iter().flat_map(vijeo_backend_vd63::vxml::cross_references).map(|r| (r.variable, r.references)).collect()
        };
        let (was, now) = (refs(&orig), refs(&doc));
        let mut checked = 0;
        for (old, leaf) in pairs {
            let new = format!("MyController.{leaf}");
            for (n, w) in was.iter().filter(|(n, _)| n.starts_with(&format!("{old}."))) {
                let moved = format!("{new}{}", &n[old.len()..]);
                assert_eq!(now.get(&moved), Some(w), "{n} → {moved}");
                assert!(!now.contains_key(n), "{n}");
                checked += usize::from(!w.is_empty());
            }
        }
        eprintln!("{checked} variables con usos comprobadas");
        assert!(checked >= 10);
        let scripts: Vec<String> = vijeo_backend_vd63::vxml::targets(&doc).into_iter().flat_map(vijeo_backend_vd63::vxml::scripts).map(|s| s.2).collect();
        assert!(scripts.iter().any(|s| s.contains("MyController.FechaHora.iGetRTCHour")) && !scripts.iter().any(|s| s.contains("DateAndTime")), "{scripts:?}");
        b.shutdown().unwrap();
    }

    /// Mover variables de carpeta sin Vijeo (`move`) en una copia del ejemplo Modbus: entre dos carpetas
    /// (`reTotalEnergy`, pantalla Energy), de la raíz a una carpeta nueva (`xRTCSyncDone`, enclavamiento y bit de la acción
    /// global) y de una carpeta a la raíz (`iGetRTCSec`, disparador y script de la acción, smart object). La copia compila
    /// con los errores del original y, en el XML de Vijeo, cada variable está con su nombre nuevo en los mismos sitios.
    #[test]
    fn live_offline_move_variables() {
        use vijeo_core::VariablesEditRequest;
        let Some(src) = modbus() else { return };
        let b = backend();
        let cfg = b.config().unwrap();
        let dir = work_dir("offline_move");
        let vdz = dir.join("McpMover.vdz");
        std::fs::copy(&src, &vdz).unwrap();
        let errs = |m: &[vijeo_core::BuildMessage]| {
            let mut v: Vec<_> = m.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).map(|m| m.code).collect();
            v.sort();
            v
        };
        let base = b.build().unwrap().build_file(&src, None).unwrap();
        let moves = [
            ("MyController.Energy.reTotalEnergy", "MyController.DateAndTime", "MyController.DateAndTime.reTotalEnergy"),
            ("xRTCSyncDone", "Mcp.Sub", "Mcp.Sub.xRTCSyncDone"),
            ("MyController.DateAndTime.iGetRTCSec", "", "iGetRTCSec"),
        ];
        let edits: Vec<serde_json::Value> = moves.iter().map(|(n, to, _)| serde_json::json!({"op": "move", "name": n, "to": to})).collect();
        let req: VariablesEditRequest = serde_json::from_value(serde_json::json!({"path": vdz, "edits": edits})).unwrap();
        let r = cfg.edit_file_variables(&req).unwrap();
        eprintln!("{:#?}", r.changes);
        assert!(r.written && r.changes.len() == 3, "{r:?}");

        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        assert_eq!(errs(&built.messages), errs(&base.messages), "{:?}", built.messages);

        let info = b.projects().unwrap().open(&vdz).unwrap();
        let out = b.reports().unwrap().export_xml(info.id, &dir.join("export"), true).unwrap();
        b.projects().unwrap().close(info.id, false).unwrap();
        let text = std::fs::read_to_string(&out.path).unwrap();
        let doc = vijeo_backend_vd63::vxml::parse(&text).unwrap();
        let orig_text = vijeo_backend_vd63::reports::decode_text(&std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/vd-6.3/corpus/xml/Compact_Modbus SL_M221 Book.vxml")).unwrap());
        let orig = vijeo_backend_vd63::vxml::parse(&orig_text).unwrap();
        let xref = |doc: &roxmltree::Document, n: &str| -> Option<Vec<String>> {
            vijeo_backend_vd63::vxml::targets(doc).into_iter().flat_map(vijeo_backend_vd63::vxml::cross_references).find(|r| r.variable == n).map(|r| r.references)
        };
        for (old, _, new) in moves {
            let (was, now) = (xref(&orig, old).unwrap(), xref(&doc, new));
            eprintln!("{old}: {was:?}\n{new}: {now:?}");
            assert!(!was.is_empty());
            assert_eq!(now.as_ref(), Some(&was), "{new}");
            assert!(xref(&doc, old).is_none(), "{old} ya no existe");
        }
        let scripts: Vec<String> = vijeo_backend_vd63::vxml::targets(&doc).into_iter().flat_map(vijeo_backend_vd63::vxml::scripts).map(|s| s.2).collect();
        assert!(scripts.iter().any(|s| s.contains("iGetRTCSec.getIntValue()")) && !scripts.iter().any(|s| s.contains("DateAndTime.iGetRTCSec")), "{scripts:?}");
        b.shutdown().unwrap();
    }

    /// Variables sin Vijeo en un proyecto de dos targets (`McpDosTargets.vdz` de `docs/`, hecho con Vijeo: `Comun` en
    /// los dos con tipos distintos): se editan los dos targets de una copia, la copia compila sin errores, su XML tiene las
    /// variables de cada target como se escribieron (también leídas con el proyecto abierto) y lo que guarda Vijeo se
    /// relee igual.
    #[test]
    fn live_offline_variables_two_targets() {
        use vijeo_core::{VariableInfo, VariablesEditRequest};
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/vd-6.3/corpus/McpDosTargets.vdz");
        if !src.is_file() {
            return;
        }
        let b = backend();
        let cfg = b.config().unwrap();
        let dir = work_dir("offline_two_targets");
        let vdz = dir.join("McpDosTargetsEdit.vdz");
        std::fs::copy(&src, &vdz).unwrap();
        // Sin target se niega (dice cuáles hay).
        let no_target: VariablesEditRequest = serde_json::from_value(serde_json::json!({"path": vdz, "dry_run": true, "edits": [{"op": "add", "name": "X", "data_type": "INT"}]})).unwrap();
        let e = cfg.edit_file_variables(&no_target).unwrap_err();
        assert!(e.to_string().contains("Primero") && e.to_string().contains("Segundo"), "{e}");
        for (target, extra) in [("Segundo", "Dos"), ("Primero", "Uno")] {
            let req: VariablesEditRequest = serde_json::from_value(serde_json::json!({
                "path": vdz, "target": target,
                "edits": [
                    {"op": "add_folder", "path": format!("Mcp{extra}.Sub")},
                    {"op": "add", "name": format!("Mcp{extra}.Sub.Marcha"), "data_type": "BOOL"},
                    {"op": "add", "name": format!("Mcp{extra}.Nivel"), "data_type": "REAL", "initial_value": 1.5, "description": "de prueba"},
                    {"op": "add", "name": "McpComun", "data_type": "DINT", "initial_value": 7},
                    {"op": "set", "name": "Comun", "description": format!("comun de {target}")}
                ]
            }))
            .unwrap();
            let r = cfg.edit_file_variables(&req).unwrap();
            eprintln!("{target}: {:?}", r.changes);
            assert!(r.written && r.variables.iter().all(|v| v.target == target), "{r:?}");
        }
        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("compilación: {:?}", built.messages);
        assert_eq!(built.errors, 0, "{:?}", built.messages);

        let offline = cfg.file_variables(&vdz, None, true, false).unwrap();
        eprintln!("{offline:#?}");
        assert_eq!(offline.len(), 2 * (3 + 3));
        let info = b.projects().unwrap().open(&vdz).unwrap();
        let xml = xml_variables(&b, info.id, &dir);
        let key = |v: &VariableInfo| (v.target.clone(), v.name.clone());
        let row = |v: &VariableInfo| (key(v), v.data_type.clone(), v.source.clone(), v.description.as_deref().map(str::trim).map(str::to_owned), v.extra.get("InitialValue").cloned());
        let sorted = |all: &[VariableInfo]| {
            let mut v: Vec<_> = all.iter().map(row).collect();
            v.sort();
            v
        };
        assert_eq!(sorted(&xml), sorted(&offline));
        // Con el proyecto abierto: lo mismo, y sólo un target si se pide.
        let open = cfg.project_variables(info.id, None, true, false).unwrap();
        assert_eq!(sorted(&open), sorted(&offline));
        let second = cfg.project_variables(info.id, Some("segundo"), true, false).unwrap();
        assert!(second.len() == 6 && second.iter().all(|v| v.target == "Segundo"), "{second:?}");

        let resaved = dir.join("McpDosTargetsVijeo.vdz");
        b.projects().unwrap().save(info.id, Some(&resaved), true).unwrap();
        b.projects().unwrap().close(info.id, false).unwrap();
        let json = |v: &[VariableInfo]| serde_json::to_value(v).unwrap();
        assert_eq!(json(&cfg.file_variables(&resaved, None, true, false).unwrap()), json(&offline));
        b.shutdown().unwrap();
    }

    /// project_new/project_save dicen dónde quedó el .vdz, y project_show_in_vijeo se niega (sin lanzar Vijeo) con un
    /// proyecto que se llama como uno de la lista de Vijeo («Samples», que trae la instalación en Vijeo-Manager).
    #[test]
    fn live_show_in_vijeo_refuses_existing_name() {
        let Some(src) = sample() else { return };
        let b = backend();
        let projects = b.projects().unwrap();
        let info = projects.open(&src).unwrap();
        projects.rename(info.id, "Samples").unwrap();
        let vdz = work_dir("show_in_vijeo").join("McpSamples.vdz");
        let saved = projects.save(info.id, Some(&vdz), true).unwrap();
        eprintln!("{:?}", saved.notes);
        assert!(saved.notes.iter().any(|n| n.contains("McpSamples.vdz") && n.contains("project_show_in_vijeo")), "{:?}", saved.notes);
        let e = projects.show_in_vijeo(None, Some(info.id)).unwrap_err();
        eprintln!("{e}");
        assert!(e.code() == "refused" && e.to_string().contains("«Samples»"), "{e}");
        let e = projects.show_in_vijeo(Some(&vdz), None).unwrap_err();
        assert_eq!(e.code(), "refused", "{e}");
        projects.close(info.id, false).unwrap();
        b.shutdown().unwrap();
    }

    /// panel_create sobre el ejemplo Hardwired (paneles base 1-4, 11, 12, 21, 51-54, 61): sin nombre (Panel5), con
    /// nombre, nombre repetido (no se crea nada) y nombres raros (o se crean con ese nombre o no queda ningún panel de
    /// más). Luego se le ponen objetos a «Motores» (un botón que vuelve a Start) y la copia compila como el original.
    #[test]
    fn live_panel_create() {
        use vijeo_core::PanelInsertRequest;
        let Some(src) = sample() else { return };
        let b = backend();
        let base = b.build().unwrap().build_file(&src, None).unwrap();
        let errs = |m: &[vijeo_core::BuildMessage]| {
            let mut v: Vec<_> = m.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).map(|m| m.code).collect();
            v.sort();
            v
        };
        let info = b.projects().unwrap().open(&src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());
        let tops = b.targets().unwrap();
        let cfg = b.config().unwrap();
        let names = || -> Vec<String> { base_refs(cfg, p, &target) };
        let start = names();

        let c1 = tops.create_panel(p, &target, vijeo_core::PanelKind::Base, None).unwrap();
        eprintln!("{c1:?}");
        assert_eq!((c1.name.as_str(), c1.panel_id, c1.path.as_str()), ("Panel5", 5, "BP:\\Panel5"));
        let c2 = tops.create_panel(p, &target, vijeo_core::PanelKind::Base, Some("Motores")).unwrap();
        eprintln!("{c2:?}");
        assert_eq!((c2.name.as_str(), c2.panel_id, c2.path.as_str()), ("Motores", 6, "BP:\\Motores"));
        let dup = tops.create_panel(p, &target, vijeo_core::PanelKind::Base, Some("start")).unwrap_err();
        assert_eq!(dup.code(), "invalid_argument", "{dup}");
        let mut now = names();
        assert!(now.contains(&c1.path) && now.contains(&c2.path), "{now:?}");
        assert_eq!(now.len(), start.len() + 2, "{now:?}");
        // Los tres primeros los rechazaba Vijeo (error 2003) y se deshacían; ahora no pasan la comprobación previa.
        // «_Motor» también lo rechazaba Vijeo (2003, el panel creado se borraba): ahora tampoco pasa la comprobación previa.
        for weird in ["Mot ores", "1Motor", "Motor-2", "Válvula", "_Motor"] {
            let r = tops.create_panel(p, &target, vijeo_core::PanelKind::Base, Some(weird));
            eprintln!("«{weird}»: {r:?}");
            let after = names();
            match r {
                Ok(c) => {
                    assert_eq!(c.name, weird);
                    assert_eq!(after.len(), now.len() + 1, "{after:?}");
                }
                Err(e) => assert_eq!(after.len(), now.len(), "«{weird}» no se creó pero quedó un panel de más: {e} / {after:?}"),
            }
            now = after;
        }

        let ins: PanelInsertRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Motores",
            "items": [
                {"create": {"type": "text", "text": "Motores", "font": "large"}, "x": 20, "y": 10},
                {"create": {"type": "switch", "label": "Inicio", "on_touch": [{"op": "change_panel", "panel": "Start"}]}, "x": 20, "y": 60}
            ]
        }))
        .unwrap();
        let r = panels_insert(&b, &ins);
        assert_eq!(r.objects.len(), 2, "{:?}", r.objects);

        let dir = work_dir("panel_create");
        let x = b.reports().unwrap().export_xml(p, &dir.join("xml"), true).unwrap();
        let xml = std::fs::read_to_string(&x.path).unwrap();
        assert!(xml.contains("Name=\"Motores\"") && xml.contains("ID=\"P6\""), "Motores (P6) no está en el XML");
        let vdz = dir.join("McpPanelNuevo.vdz");
        b.projects().unwrap().save(p, Some(&vdz), true).unwrap();
        b.projects().unwrap().close(p, false).unwrap();
        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("original: {:?}\ncopia: {:?}", base.messages, built.messages);
        assert_eq!(errs(&built.messages), errs(&base.messages));
        // Los paneles que quedaron vacíos (Panel5 y, si Vijeo lo aceptó, _Motor) dan «Panel is empty» cada uno.
        let empty = built.messages.iter().filter(|m| m.text.contains("Panel is empty")).count() as i32;
        assert!(empty >= 1, "{:?}", built.messages);
        assert_eq!(built.warnings, base.warnings + empty, "{:?}", built.messages);
        b.shutdown().unwrap();
    }

    /// panel_create con kind popup sobre el ejemplo Modbus (que tiene la ventana `1: ComErrorCodes`): una con el nombre de
    /// Vijeo (`Window1`, WindowID 2) y otra llamada «Avisos»; nombre repetido o inválido no crea nada. Un botón nuevo la
    /// abre con open_popup; el XML tiene las ventanas con sus paneles y la copia compila con los errores del original.
    #[test]
    fn live_popup_create() {
        use vijeo_core::{PanelInsertRequest, PanelKind};
        let Some(src) = modbus() else { return };
        let b = backend();
        let errs = |m: &[vijeo_core::BuildMessage]| {
            let mut v: Vec<_> = m.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).map(|m| m.code).collect();
            v.sort();
            v
        };
        let base = b.build().unwrap().build_file(&src, None).unwrap();
        let info = b.projects().unwrap().open(&src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());
        let tops = b.targets().unwrap();
        let w1 = tops.create_panel(p, &target, PanelKind::Popup, None).unwrap();
        eprintln!("{w1:?}");
        assert_eq!((w1.name.as_str(), w1.panel_id, w1.path.as_str()), ("Window1", 2, "Window1"));
        let w2 = tops.create_panel(p, &target, PanelKind::Popup, Some("Avisos")).unwrap();
        eprintln!("{w2:?}");
        assert_eq!((w2.name.as_str(), w2.panel_id), ("Avisos", 3));
        for bad in ["comerrorcodes", "Mal nombre"] {
            let e = tops.create_panel(p, &target, PanelKind::Popup, Some(bad)).unwrap_err();
            eprintln!("{bad}: {e}");
            assert_eq!(e.code(), "invalid_argument", "{e}");
        }
        let ins: PanelInsertRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Alarms",
            "items": [{"create": {"type": "switch", "label": "Avisos", "on_touch": [{"op": "open_popup", "popup": "Avisos"}]}, "x": 10, "y": 10, "name": "McpAbreAvisos"}]
        }))
        .unwrap();
        panels_insert(&b, &ins);
        let dir = work_dir("popup_create");
        let x = b.reports().unwrap().export_xml(p, &dir.join("xml"), true).unwrap();
        let xml = std::fs::read_to_string(&x.path).unwrap();
        for want in [r#"<Window Name="Window1" WindowID="W2""#, r#"<Window Name="Avisos" WindowID="W3""#] {
            assert!(xml.contains(want), "falta {want}");
        }
        let ids = b.reports().unwrap().panels(p, Some(&target)).unwrap();
        let pops: Vec<&str> = ids.iter().filter(|r| r.group == "Popup Windows").map(|r| r.path.as_str()).collect();
        eprintln!("popups: {pops:?}");
        assert_eq!(pops, ["ComErrorCodes", "Window1", "Avisos"]);
        let vdz = dir.join("McpPopupNuevo.vdz");
        b.projects().unwrap().save(p, Some(&vdz), true).unwrap();
        b.projects().unwrap().close(p, false).unwrap();
        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("original: {:?}\ncopia: {:?}", base.messages, built.messages);
        assert_eq!(errs(&built.messages), errs(&base.messages));
        b.shutdown().unwrap();
    }

    /// panel_rename y panel_delete sobre el ejemplo Modbus (paneles base `21: RFID_1`… y la ventana emergente
    /// `1: ComErrorCodes` con su panel `10001: Panel1`): renombrar base y popup, errores sin tocar nada, borrar un
    /// panel libre, negarse con uno usado por un botón y borrarlo con force; el XML y la compilación de la copia.
    #[test]
    fn live_panel_rename_delete() {
        use vijeo_core::{PanelInsertRequest, PanelKind};
        let Some(src) = modbus() else { return };
        let b = backend();
        let base = b.build().unwrap().build_file(&src, None).unwrap();
        let errs = |m: &[vijeo_core::BuildMessage]| {
            let mut v: Vec<_> = m.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).map(|m| m.code).collect();
            v.sort();
            v
        };
        let info = b.projects().unwrap().open(&src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());
        let tops = b.targets().unwrap();
        let cfg = b.config().unwrap();
        let names = || -> Vec<String> { base_refs(cfg, p, &target) };
        let has = |n: &str| names().iter().any(|x| x == n);

        let r = tops.rename_panel(p, &target, PanelKind::Base, "RFID_1", "Rfid_Uno").unwrap();
        eprintln!("{r:?}");
        assert_eq!((r.old_path.as_str(), r.path.as_str(), r.id), (r"BP:\RFID_1", r"BP:\Rfid_Uno", 21));
        assert!(has(r"BP:\Rfid_Uno") && !has(r"BP:\RFID_1"));
        let r = tops.rename_panel(p, &target, PanelKind::Popup, "ComErrorCodes", "ErroresCom").unwrap();
        eprintln!("{r:?}");
        assert_eq!((r.path.as_str(), r.id), ("ErroresCom", 1));
        let before = names();
        for (kind, panel, new) in [
            (PanelKind::Base, "Rfid_Uno", "RFID_2"),
            (PanelKind::Base, "NoExiste", "Otro"),
            (PanelKind::Base, "Rfid_Uno", "Mal nombre"),
            (PanelKind::Base, "Rfid_Uno", "Rfid_Uno"),
            (PanelKind::Popup, "NoExiste", "Otro"),
        ] {
            let e = tops.rename_panel(p, &target, kind, panel, new).unwrap_err();
            eprintln!("{panel} → {new}: {e}");
            assert_eq!(e.code(), "invalid_argument", "{e}");
        }
        assert_eq!(names(), before);

        tops.create_panel(p, &target, vijeo_core::PanelKind::Base, Some("Libre")).unwrap();
        tops.create_panel(p, &target, vijeo_core::PanelKind::Base, Some("Usado")).unwrap();
        let ins: PanelInsertRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Rfid_Uno",
            "items": [{"create": {"type": "switch", "label": "Usado", "on_touch": [{"op": "change_panel", "panel": "Usado"}]}, "x": 10, "y": 10, "name": "McpIrUsado"}]
        }))
        .unwrap();
        panels_insert(&b, &ins);
        let d = tops.delete_panel(p, &target, PanelKind::Base, "Libre", false).unwrap();
        eprintln!("{d:?}");
        assert!(d.dangling_references.is_empty(), "{d:?}");
        assert!(!has(r"BP:\Libre"));
        let e = tops.delete_panel(p, &target, PanelKind::Base, "Usado", false).unwrap_err();
        eprintln!("{e}");
        assert!(e.code() == "invalid_argument" && e.to_string().contains("McpIrUsado"), "{e}");
        assert!(has(r"BP:\Usado"));
        let d = tops.delete_panel(p, &target, PanelKind::Base, "Usado", true).unwrap();
        eprintln!("{d:?}");
        assert!(d.dangling_references.len() == 1 && d.dangling_references[0].contains("McpIrUsado"), "{d:?}");
        assert!(!has(r"BP:\Usado"));
        let popup = tops.delete_panel(p, &target, PanelKind::Popup, "ErroresCom", false);
        eprintln!("popup sin force: {popup:?}");
        let popup = match popup {
            Ok(d) => d,
            Err(_) => tops.delete_panel(p, &target, PanelKind::Popup, "ErroresCom", true).unwrap(),
        };
        eprintln!("{popup:?}");
        assert!(tops.delete_panel(p, &target, PanelKind::Popup, "ErroresCom", true).is_err());

        let dir = work_dir("panel_rename_delete");
        let x = b.reports().unwrap().export_xml(p, &dir.join("xml"), true).unwrap();
        let xml = std::fs::read_to_string(&x.path).unwrap();
        assert!(xml.contains(r#"Name="Rfid_Uno" ID="P21""#), "Rfid_Uno (P21) no está en el XML");
        // `RFID_1` y `ComErrorCodes` también son nombres de otros objetos del ejemplo: se buscan como pantalla.
        for gone in [r#"Name="RFID_1" ID="P21""#, r#"Name="Libre""#, r#"Name="Usado""#, r#"Name="ErroresCom""#, r#"Name="ComErrorCodes" WindowID"#] {
            assert!(!xml.contains(gone), "{gone} sigue en el XML");
        }
        let vdz = dir.join("McpPantallas.vdz");
        b.projects().unwrap().save(p, Some(&vdz), true).unwrap();
        b.projects().unwrap().close(p, false).unwrap();
        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("original: {:?}\ncopia: {:?}", base.messages, built.messages);
        let (eb, ec) = (errs(&base.messages), errs(&built.messages));
        eprintln!("errores original {eb:?} copia {ec:?}");
        // Cada referencia sin destino es un error 2010 «[Switch] [McpIrUsado] [When Touch] invalid selection.».
        let dangling = d.dangling_references.len() + popup.dangling_references.len();
        let mut want = eb.clone();
        want.extend(std::iter::repeat_n(Some(2010), dangling));
        want.sort();
        assert_eq!(ec, want);
        assert_eq!(built.warnings, base.warnings, "{:?}", built.messages);
        b.shutdown().unwrap();
    }

    /// project_goto (Navigator + Intro) y script_lint sobre el ejemplo Hardwired, que tiene paneles en carpetas
    /// y scripts. `VIJEO_MCP_LIVE_LINT` = otro .vdz cuyos scripts se revisan también (sólo se lee).
    #[test]
    fn live_goto_and_lint() {
        use vijeo_core::{ProjectOps, ReportOps, TargetOps};
        let Some(src) = sample() else { return };
        let inst = installation().expect("Vijeo 6.3");
        let cfg = BackendConfig { cache_dir: Some(std::env::temp_dir().join("vijeo-mcp-live").join("no-cache")), ..Default::default() };
        let b = vijeo_backend_vd63::Vd63Backend::new(&inst, Support::Exact, &cfg).unwrap();
        let info = ProjectOps::open(&b, &src).unwrap();
        let p = info.id;
        let target = info.targets[0].name.clone();
        let outline = b.navigator_outline(p, &target, 3);
        if let Err(e) = &outline {
            eprintln!("Navigator: {e}");
        }
        for l in outline.unwrap() {
            eprintln!("{l}");
        }
        let panels = base_refs(&b, p, &target);
        eprintln!("{panels:?}");
        let first = TargetOps::open_panel(&b, p, &target, &panels[0]).unwrap();
        assert_eq!(first.path, panels[0]);
        // Otro panel, por su nombre: el primero puede estar ya abierto al abrir el proyecto.
        let other = TargetOps::open_panel(&b, p, &target, "cabinet").unwrap();
        assert_eq!(other.path, "BP:\\Cabinet");
        let back = TargetOps::open_panel(&b, p, &target, "BP:\\Start").unwrap();
        assert_eq!(back.path, "BP:\\Start");
        if let Some(nested) = panels.iter().find(|r| r.matches('\\').count() > 1) {
            let name = nested.rsplit('\\').next().unwrap().to_lowercase();
            let opened = TargetOps::open_panel(&b, p, &target, &name);
            eprintln!("por nombre ({name}): {opened:?}");
            if let Ok(o) = opened {
                assert_eq!(&o.path, nested);
            }
        }
        assert_eq!(TargetOps::open_panel(&b, p, &target, "NoExiste").unwrap_err().code(), "invalid_argument");

        let lints = ReportOps::lint_scripts(&b, p, None, None).unwrap();
        let issues: Vec<_> = lints.iter().filter(|l| !l.issues.is_empty()).collect();
        eprintln!("{} scripts, {} con avisos", lints.len(), issues.len());
        for l in &issues {
            eprintln!("{} [{:?}]: {:?}", l.location, l.script_type, l.issues);
        }
        // Con el compilador de Vijeo: los scripts del ejemplo compilan.
        assert!(!lints.is_empty());
        for l in &lints {
            assert_eq!(l.compiles, Some(true), "{} {:?} {:?}", l.location, l.compiler_note, l.issues);
        }
        let one = ReportOps::lint_scripts(&b, p, None, Some("int a = 0;\nNoExiste_Var.write(a);")).unwrap();
        assert_eq!(one.len(), 1);
        // La variable que no existe la da la revisión propia; el compilador tampoco lo compila, sin repetir el error.
        assert_eq!(one[0].issues.len(), 1, "{:?}", one[0].issues);
        assert_eq!(one[0].compiles, Some(false));
        // Un error de sintaxis que sólo ve el compilador (falta el punto y coma), con su línea.
        let syntax = ReportOps::lint_scripts(&b, p, None, Some("int a = 0\nint b = a + 1;")).unwrap();
        assert_eq!(syntax[0].compiles, Some(false));
        assert!(syntax[0].issues.iter().any(|i| i.message.contains("compilador de Vijeo") && i.line == 2), "{:?}", syntax[0].issues);
        ProjectOps::close(&b, p, true).unwrap();

        if let Some(extra) = std::env::var_os("VIJEO_MCP_LIVE_LINT").map(PathBuf::from) {
            let info = ProjectOps::open(&b, &extra).unwrap();
            let lints = ReportOps::lint_scripts(&b, info.id, None, None).unwrap();
            eprintln!("{}: {} scripts", extra.display(), lints.len());
            for l in lints.iter().filter(|l| !l.issues.is_empty()) {
                eprintln!("{} [{:?}]: {:?}", l.location, l.script_type, l.issues);
            }
            ProjectOps::close(&b, info.id, true).unwrap();
        }
        vijeo_core::Backend::shutdown(&b).unwrap();
    }

    /// panel_objects, panel_edit (dry_run y de verdad) y panel_insert (Toolchest y otro panel) sobre una copia del
    /// ejemplo; luego se guarda y se compila la copia aparte.
    #[test]
    fn live_panel_objects_edit_insert() {
        use vijeo_core::{PanelEditRequest, PanelInsertRequest, PanelObject};
        let Some(src) = sample() else { return };
        fn find<'a>(objs: &'a [PanelObject], name: &str) -> Option<&'a PanelObject> {
            objs.iter().find_map(|o| if o.name == name { Some(o) } else { find(&o.children, name) })
        }
        let b = backend();
        let clip_before = vijeo_ui::clipboard::get_text();
        let info = b.projects().unwrap().open(&src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());
        let panels = b.panels().unwrap();

        let objs = panels.objects(p, &target, "DateAndTime").unwrap();
        eprintln!("{}", serde_json::to_string_pretty(&objs).unwrap());
        assert_eq!(objs.objects.len(), 1);
        assert_eq!(objs.objects[0].children.len(), 11);
        let t7 = find(&objs.objects, "Text07").unwrap();
        assert_eq!(t7.path, "GroupObject01/Text07");
        assert!(t7.strings.iter().any(|s| s == "Date and Time"), "{:?}", t7.strings);
        let sw = find(&objs.objects, "SetRTCPLC").unwrap();
        assert!(sw.variables.iter().any(|v| v.ends_with("xSetRTC")), "{:?}", sw.variables);

        let req = |dry_run: bool| -> PanelEditRequest {
            serde_json::from_value(serde_json::json!({
                "project": p.0, "target": target, "panel": "DateAndTime", "dry_run": dry_run,
                "operations": [
                    {"op": "rename", "object": "Text07", "new_name": "Titulo"},
                    {"op": "set_text", "object": "Titulo", "old": "Date and Time", "new": "Fecha y hora"},
                    {"op": "set_text", "object": "SetRTCPLC", "old": "set\nRTC PLC", "new": "SET\nRELOJ"}
                ]
            }))
            .unwrap()
        };
        let dry = panels.edit(&req(true)).unwrap();
        assert!(!dry.applied);
        assert!(find(&dry.objects, "Titulo").is_some());
        assert!(find(&panels.objects(p, &target, "DateAndTime").unwrap().objects, "Text07").is_some(), "dry_run no cambia el panel");

        let done = panels.edit(&req(false)).unwrap();
        eprintln!("{:?} {:?}", done.changes, done.warnings);
        assert!(done.applied && done.warnings.is_empty(), "{:?}", done.warnings);
        let titulo = find(&done.objects, "Titulo").unwrap();
        assert!(titulo.strings.iter().any(|s| s == "Fecha y hora"));
        assert_eq!((titulo.left, titulo.top), (t7.left, t7.top));
        assert!(find(&done.objects, "SetRTCPLC").unwrap().strings.iter().any(|s| s == "SET\nRELOJ"));

        let ins: PanelInsertRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "DateAndTime",
            "items": [
                {"toolchest": "DateTime\\Date_Display_01", "x": 120, "y": 200, "name": "McpDate1"},
                {"from_panel": "Cabinet", "object": "Home", "x": 5, "y": 200, "name": "McpHome"}
            ]
        }))
        .unwrap();
        let r = panels.insert(&ins).unwrap();
        eprintln!("{:?} {:?}", r.changes, r.warnings);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let d = find(&r.objects, "McpDate1").unwrap();
        assert_eq!((d.kind.as_str(), d.left, d.top, d.width, d.height), ("SmartObject", 120, 200, 100, 20));
        let h = find(&r.objects, "McpHome").unwrap();
        assert_eq!((h.kind.as_str(), h.left, h.top), ("Switch", 5, 200));
        assert_eq!(vijeo_ui::clipboard::get_text(), clip_before, "el texto del portapapeles se devuelve");

        let vdz = work_dir("panels").join("McpPanels.vdz");
        b.projects().unwrap().save(p, Some(&vdz), true).unwrap();
        b.projects().unwrap().close(p, false).unwrap();
        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("{:?}", built.messages);
        // Sólo lo que ya tenía el ejemplo (4230) y las variables de la pieza sin configurar (4336 McpDate1).
        for m in built.messages.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error) {
            let ok = m.code == Some(4230) || (m.code == Some(4336) && m.text.contains("McpDate1"));
            assert!(ok, "error inesperado: {m:?}");
        }
        b.shutdown().unwrap();
    }

    /// panel_insert con `create` (un objeto de cada tipo, con animaciones) y panel_edit de animaciones, acciones y
    /// colores; después se exporta el XML para ver cómo lo entiende Vijeo, se guarda y se compila la copia.
    #[test]
    fn live_panel_create_objects() {
        use vijeo_core::{PanelEditRequest, PanelInsertRequest, PanelObject};
        let Some(src) = sample() else { return };
        fn find<'a>(objs: &'a [PanelObject], name: &str) -> Option<&'a PanelObject> {
            objs.iter().find_map(|o| if o.name == name { Some(o) } else { find(&o.children, name) })
        }
        let b = backend();
        let info = b.projects().unwrap().open(&src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());
        let panels = b.panels().unwrap();
        let a = "MyController.ATV12";
        let ins: PanelInsertRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Alarms",
            "items": [
                {"create": {"type": "switch", "label": "Marcha", "on_touch": [{"op": "set", "variable": format!("{a}.xCmdFwd")}],
                    "on_release": [{"op": "word_add", "variable": format!("{a}.iVeloSetpointManMode"), "value": "10"}],
                    "lamp": format!("{a}.xStatCmdFwd"), "on_color": "FF0000", "font": "medium", "bold": true},
                 "x": 10, "y": 40, "name": "McpMarcha", "visibility": format!("{a}.xStatManMode")},
                {"create": {"type": "switch", "label": "Armario", "on_touch": [{"op": "change_panel", "panel": "Cabinet"}]}, "x": 80, "y": 40},
                {"create": {"type": "lamp", "variable": format!("{a}.xStatDriveRun"), "on_label": "RUN", "off_label": "STOP"}, "x": 150, "y": 40, "name": "McpLamp"},
                {"create": {"type": "lamp", "variable": format!("{a}.xStatMCB"), "shape": "round"}, "x": 200, "y": 40, "width": 20, "height": 20},
                {"create": {"type": "numeric_display", "variable": format!("{a}.iVeloSetpointAutoMode"), "input": true, "min": "0", "max": "500"},
                 "x": 10, "y": 70, "name": "McpVelo"},
                {"create": {"type": "numeric_display", "variable": "MyController.Energy.reTotalEnergy", "float": true, "digits": 6, "decimals": 1},
                 "x": 80, "y": 70, "name": "McpEnergia"},
                {"create": {"type": "text", "text": "Prueba MCP\ncreado desde cero", "font": "medium", "color": "0000FF", "background": "FFFF00"},
                 "x": 10, "y": 100, "name": "McpTexto", "animations": [{"type": "blink", "variable": format!("{a}.xStatDrive")}]},
                {"create": {"type": "rectangle", "fill": "00FF00", "line": "000000", "corner": 0}, "x": 180, "y": 100, "width": 40, "height": 30,
                 "name": "McpRect", "animations": [{"type": "fill", "variable": format!("{a}.iVeloSetpointManMode"), "min": 0, "max": 500}]},
                {"create": {"type": "ellipse", "fill": "808080"}, "x": 230, "y": 100, "name": "McpLed",
                 "animations": [{"type": "color", "variable": format!("{a}.xStatDriveRun"), "on_color": "00FF00", "off_color": "FF0000"}]},
                {"create": {"type": "line", "end_x": 300, "end_y": 140, "color": "FF0000", "arrows": "end"}, "x": 230, "y": 140, "name": "McpLinea",
                 "animations": [{"type": "position", "variable": format!("{a}.iVeloSetpointManMode"), "min": 0, "max": 100, "dx": 0, "dy": 30}]}
            ]
        }))
        .unwrap();
        let r = panels.insert(&ins).unwrap();
        eprintln!("{:#?}\n{:?}\n{:?}", r.changes, r.warnings, r.notes);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let want = [
            ("McpMarcha", "Switch", 10, 40),
            ("Switch01", "Switch", 80, 40),
            ("McpLamp", "Lamp", 150, 40),
            ("Lamp01", "Lamp", 200, 40),
            ("McpVelo", "DataDisplay", 10, 70),
            ("McpEnergia", "DataDisplay", 80, 70),
            ("McpTexto", "Text", 10, 100),
            ("McpRect", "Rectangle", 180, 100),
            ("McpLed", "Circle", 230, 100),
            ("McpLinea", "Line", 230, 140),
        ];
        for (name, kind, x, y) in want {
            let o = find(&r.objects, name).unwrap_or_else(|| panic!("falta {name}"));
            assert_eq!((o.kind.as_str(), o.left, o.top), (kind, x, y), "{name}");
        }

        // panel_edit: animaciones, acciones y colores sobre lo creado.
        let ed: PanelEditRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Alarms",
            "operations": [
                {"op": "animate", "object": "McpRect", "animations": [{"type": "visibility", "variable": format!("{a}.xStatAutoMode"), "invert": true}]},
                {"op": "remove_animations", "object": "McpTexto"},
                {"op": "set_actions", "object": "Switch01", "on_touch": [{"op": "toggle", "variable": format!("{a}.xSelManMode")}]},
                {"op": "set_colors", "object": "McpLamp", "on": "0000FF", "text": "FFFFFF"},
                {"op": "set_colors", "object": "McpTexto", "text": "FF0000", "fill": "none"},
                {"op": "move", "object": "McpLinea", "dx": -20, "dy": 0}
            ]
        }))
        .unwrap();
        let e = panels.edit(&ed).unwrap();
        eprintln!("{:#?}\n{:?}", e.changes, e.warnings);
        assert!(e.applied && e.warnings.is_empty(), "{:?}", e.warnings);
        assert_eq!(find(&e.objects, "McpLinea").unwrap().left, 210);

        // Cómo lo entiende Vijeo: su propio XML.
        let dir = work_dir("create");
        let x = b.reports().unwrap().export_xml(p, &dir.join("xml"), true).unwrap();
        let xml = std::fs::read_to_string(&x.path).unwrap();
        let obj = |name: &str| -> String {
            let at = xml.find(&format!("Name=\"{name}\"")).unwrap_or_else(|| panic!("{name} no está en el XML"));
            let start = xml[..at].rfind('<').unwrap();
            let tag: String = xml[start + 1..].chars().take_while(|c| c.is_alphanumeric()).collect();
            let end = xml[start..].find(&format!("</{tag}>")).map(|e| start + e + tag.len() + 3).unwrap_or(start + 2000);
            xml[start..end.min(xml.len())].to_owned()
        };
        for n in ["McpMarcha", "Switch01", "McpLamp", "Lamp01", "McpVelo", "McpEnergia", "McpTexto", "McpRect", "McpLed", "McpLinea"] {
            eprintln!("---- {n}\n{}", obj(n));
        }
        let m = obj("McpMarcha");
        assert!(m.contains("xCmdFwd") && m.contains("xStatCmdFwd") && m.contains("iVeloSetpointManMode") && m.contains("xStatManMode"), "{m}");
        assert!(obj("Switch01").contains("xSelManMode"));
        assert!(obj("McpLed").contains("xStatDriveRun"));
        assert!(obj("McpRect").contains("xStatAutoMode"));
        assert!(!obj("McpTexto").contains("xStatDrive"));

        let vdz = dir.join("McpCrear.vdz");
        b.projects().unwrap().save(p, Some(&vdz), true).unwrap();
        b.projects().unwrap().close(p, false).unwrap();
        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("{:?}", built.messages);
        for m in built.messages.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error) {
            assert!(m.code == Some(4230), "error inesperado: {m:?}");
        }
        b.shutdown().unwrap();
    }

    /// Display de texto (`string_display`) sobre la variable STRING `sRefStringForComErrCodes` del ejemplo
    /// Modbus: uno de sólo lectura y otro con entrada. Se mira cómo lo exporta Vijeo y la copia guardada tiene que
    /// compilar con los mismos errores que el original (2 errores, 2 warnings).
    #[test]
    fn live_string_display() {
        use vijeo_core::{PanelInsertRequest, PanelObject};
        let Some(src) = modbus() else { return };
        fn find<'a>(objs: &'a [PanelObject], name: &str) -> Option<&'a PanelObject> {
            objs.iter().find_map(|o| if o.name == name { Some(o) } else { find(&o.children, name) })
        }
        let b = backend();
        let base = b.build().unwrap().build_file(&src, None).unwrap();
        let errs = |m: &[vijeo_core::BuildMessage]| {
            let mut v: Vec<_> = m.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).map(|m| m.code).collect();
            v.sort();
            v
        };
        let info = b.projects().unwrap().open(&src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());
        let v = "sRefStringForComErrCodes";
        let ins: PanelInsertRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Alarms",
            "items": [
                {"create": {"type": "string_display", "variable": v}, "x": 10, "y": 40, "name": "McpTxtRO"},
                {"create": {"type": "string_display", "variable": v, "length": 20, "input": true, "plate_color": "FFFF00",
                    "text_color": "0000FF", "font": "medium", "bold": true, "align": "left"},
                 "x": 10, "y": 70, "width": 160, "height": 24, "name": "McpTxtIn"}
            ]
        }))
        .unwrap();
        let r = panels_insert(&b, &ins);
        for (name, x, y, w, h) in [("McpTxtRO", 10, 40, 100, 20), ("McpTxtIn", 10, 70, 160, 24)] {
            let o = find(&r.objects, name).unwrap_or_else(|| panic!("falta {name}"));
            assert_eq!((o.kind.as_str(), o.left, o.top, o.width, o.height), ("DataDisplay", x, y, w, h), "{name}: {o:?}");
            assert!(o.variables.iter().any(|x| x.contains(v)), "{name}: {o:?}");
        }

        let dir = work_dir("string_display");
        let x = b.reports().unwrap().export_xml(p, &dir.join("xml"), true).unwrap();
        let xml = std::fs::read_to_string(&x.path).unwrap();
        let obj = |name: &str| -> String {
            let at = xml.find(&format!("Name=\"{name}\"")).unwrap_or_else(|| panic!("{name} no está en el XML"));
            let start = xml[..at].rfind('<').unwrap();
            let tag: String = xml[start + 1..].chars().take_while(|c| c.is_alphanumeric()).collect();
            let end = xml[start..].find(&format!("</{tag}>")).map(|e| start + e + tag.len() + 3).unwrap_or(start + 3000);
            xml[start..end.min(xml.len())].to_owned()
        };
        let (ro, inp) = (obj("McpTxtRO"), obj("McpTxtIn"));
        eprintln!("---- McpTxtRO\n{ro}\n---- McpTxtIn\n{inp}");
        assert!(ro.starts_with("<StringDisplay") && ro.contains(&format!("Variable=\"{v}\"")), "{ro}");
        assert!(ro.contains("DisplayLength=\"10\"") && ro.contains("HAlign=\"Center\"") && !ro.contains("<InputMode"), "{ro}");
        assert!(inp.starts_with("<StringDisplay") && inp.contains(&format!("Variable=\"{v}\"")), "{inp}");
        // El XML escribe el hex del COLORREF (0x00BBGGRR sin ceros a la izquierda): amarillo → FFFF, azul → FF0000.
        for want in ["DisplayLength=\"20\"", "PlateColor=\"FFFF\"", "TextColor=\"FF0000\"", "HAlign=\"Left\"", "FontStyle=\"Bold\"",
            "FontFace=\"GPFontModern8x13\"", "KeypadFormat=\"StringKeypad\""] {
            assert!(inp.contains(want), "falta {want}: {inp}");
        }

        let vdz = dir.join("McpTexto.vdz");
        b.projects().unwrap().save(p, Some(&vdz), true).unwrap();
        b.projects().unwrap().close(p, false).unwrap();
        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("original: {:?}\ncopia: {:?}", base.messages, built.messages);
        assert_eq!(errs(&built.messages), errs(&base.messages));
        assert_eq!(built.warnings, base.warnings, "{:?}", built.messages);
        b.shutdown().unwrap();
    }

    /// Acciones de popup en botones nuevos, sobre el ejemplo Modbus (ventana ComErrorCodes, panel Panel1 = P10001):
    /// abrir en una posición, abrir centrado y cerrar. Se mira lo que guarda Vijeo tras pegar (portapapeles
    /// recopiado), su XML y que la copia compila como el original.
    #[test]
    fn live_popup_actions() {
        use vijeo_backend_vd63::objects::Clip;
        use vijeo_backend_vd63::widgets::{action::Op, switch::Switch};
        use vijeo_core::{Backend, PanelInsertRequest, PanelOps};
        let Some(src) = modbus() else { return };
        let inst = installation().expect("Vijeo 6.3");
        let cfg = BackendConfig { cache_dir: Some(std::env::temp_dir().join("vijeo-mcp-live").join("no-cache")), ..Default::default() };
        let b = vijeo_backend_vd63::Vd63Backend::new(&inst, Support::Exact, &cfg).unwrap();
        let errs = |m: &[vijeo_core::BuildMessage]| {
            let mut v: Vec<_> = m.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).map(|m| m.code).collect();
            v.sort();
            v
        };
        let base = b.build().unwrap().build_file(&src, None).unwrap();
        let info = b.projects().unwrap().open(&src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());
        // El popup del ejemplo es la ventana ComErrorCodes con un panel, Panel1 (P10001): vale cualquiera de los dos
        // nombres o la ruta.
        let ins: PanelInsertRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Alarms",
            "items": [
                {"create": {"type": "switch", "label": "PopXY", "on_touch": [{"op": "open_popup", "popup": "Panel1", "x": 30, "y": 25}]}, "x": 10, "y": 40, "name": "McpPopXY"},
                {"create": {"type": "switch", "label": "PopCen", "on_touch": [{"op": "open_popup", "popup": "ComErrorCodes"}]}, "x": 80, "y": 40, "name": "McpPopCen"},
                {"create": {"type": "switch", "label": "Cerrar", "on_release": [{"op": "close_popup", "popup": r"ComErrorCodes\Panel1"}]}, "x": 150, "y": 40, "name": "McpPopClose"}
            ]
        }))
        .unwrap();
        let r = PanelOps::insert(&b, &ins).unwrap();
        eprintln!("{:#?}\n{:?}", r.changes, r.warnings);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let bad: PanelInsertRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Alarms",
            "items": [{"create": {"type": "switch", "label": "X", "on_touch": [{"op": "open_popup", "popup": "NoExiste"}]}, "x": 0, "y": 0}]
        }))
        .unwrap();
        let e = PanelOps::insert(&b, &bad).unwrap_err().to_string();
        assert!(e.contains("ComErrorCodes\\Panel1"), "{e}");

        // Lo que Vijeo guarda tras pegar.
        let raw = b.copy_panel_raw(p, &target, "Alarms").unwrap().unwrap();
        let clip = Clip::from_bytes(&raw).unwrap();
        let ops = |name: &str| -> Vec<Op> {
            let path = clip.resolve(name).unwrap_or_else(|e| panic!("falta {name}: {e}"));
            let s = Switch::parse(clip.node(&path).class_data()).unwrap();
            s.on_touch.iter().chain(&s.on_release).map(|a| a.op.clone()).collect()
        };
        match &ops("McpPopXY")[..] {
            [Op::PopupOpen { x, y, target }] => assert_eq!((x.raw.as_str(), y.raw.as_str(), target.as_str()), ("30", "25", "10001")),
            o => panic!("{o:?}"),
        }
        assert_eq!(ops("McpPopCen"), [Op::PopupOpenCentered("10001".into())]);
        assert_eq!(ops("McpPopClose"), [Op::PopupClose("10001".into())]);

        let dir = work_dir("popup_actions");
        let x = b.reports().unwrap().export_xml(p, &dir.join("xml"), true).unwrap();
        let xml = std::fs::read_to_string(&x.path).unwrap();
        let obj = |name: &str| -> String {
            let at = xml.find(&format!("Name=\"{name}\"")).unwrap_or_else(|| panic!("{name} no está en el XML"));
            let start = xml[..at].rfind('<').unwrap();
            let end = xml[start..].find("</Switch>").map(|e| start + e + 9).unwrap_or(start + 3000);
            xml[start..end.min(xml.len())].to_owned()
        };
        for n in ["McpPopXY", "McpPopCen", "McpPopClose"] {
            eprintln!("---- {n}\n{}", obj(n));
        }
        let vdz = dir.join("McpPopup.vdz");
        b.projects().unwrap().save(p, Some(&vdz), true).unwrap();
        b.projects().unwrap().close(p, false).unwrap();
        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("original: {:?}\ncopia: {:?}", base.messages, built.messages);
        assert!(obj("McpPopXY").contains(r#"PopupPanelAction="Open" PopupPanelID="P10001" PopupPanelPosition="AtPosition" PopupPanelPosX="30" PopupPanelPosY="25""#));
        assert!(obj("McpPopCen").contains(r#"PopupPanelAction="Open" PopupPanelID="P10001" PopupPanelPosition="Centered""#));
        assert!(obj("McpPopClose").contains(r#"<WhenRelease>"#) && obj("McpPopClose").contains(r#"PopupPanelAction="Close" PopupPanelID="P10001""#));
        assert_eq!(errs(&built.messages), errs(&base.messages));
        assert_eq!(built.warnings, base.warnings, "{:?}", built.messages);
        b.shutdown().unwrap();
    }

    /// Acción de script en botones nuevos (ejemplo Hardwired): el script de DateAndTime de los ejemplos, que tiene que
    /// quedar con el mismo Java que guardó Vijeo, y otro con variable local, `if`, `write` y `Math`. Un script que no
    /// compila se rechaza con su posición sin tocar el panel. Se mira lo que guarda Vijeo, su XML y que compila.
    #[test]
    fn live_script_actions() {
        use vijeo_backend_vd63::objects::Clip;
        use vijeo_backend_vd63::widgets::{action::Op, switch::Switch};
        use vijeo_core::{Backend, PanelInsertRequest, PanelOps};
        let Some(src) = sample() else { return };
        let inst = installation().expect("Vijeo 6.3");
        let cfg = BackendConfig { cache_dir: Some(std::env::temp_dir().join("vijeo-mcp-live").join("no-cache")), ..Default::default() };
        let b = vijeo_backend_vd63::Vd63Backend::new(&inst, Support::Exact, &cfg).unwrap();
        let errs = |m: &[vijeo_core::BuildMessage]| {
            let mut v: Vec<_> = m.iter().filter(|m| m.severity == vijeo_core::MessageSeverity::Error).map(|m| m.code).collect();
            v.sort();
            v
        };
        let base = b.build().unwrap().build_file(&src, None).unwrap();
        let info = b.projects().unwrap().open(&src).unwrap();
        let (p, target) = (info.id, info.targets[0].name.clone());
        let date_time = "//-------------------------------------\r\n//Script Created: Oct 16, 2013\r\n//\r\n// Description:\r\n//\r\n//-------------------------------------\r\nSys.setTime(MyController.DateAndTime.iGetRTCHour.getIntValue(),\r\n\t\tMyController.DateAndTime.iGetRTCMin.getIntValue(),\r\n\t\tMyController.DateAndTime.iGetRTCSec.getIntValue());\r\nSys.setDate(MyController.DateAndTime.iGetRTCYear.getIntValue(),\r\n\t\tMyController.DateAndTime.iGetRTCMonth.getIntValue(),\r\n\t\tMyController.DateAndTime.iGetRTCDay.getIntValue());\r\n";
        let java = "end:{//-------------------------------------\r\n//Script Created: Oct 16, 2013\r\n//\r\n// Description:\r\n//\r\n//-------------------------------------\r\nCgpSystem.setTime(TagDB.MyController.DateAndTime.iGetRTCHour.getIntValue(0),\r\nTagDB.MyController.DateAndTime.iGetRTCMin.getIntValue(0),\r\nTagDB.MyController.DateAndTime.iGetRTCSec.getIntValue(0));\r\nCgpSystem.setDate(TagDB.MyController.DateAndTime.iGetRTCYear.getIntValue(0),\r\nTagDB.MyController.DateAndTime.iGetRTCMonth.getIntValue(0),\r\nTagDB.MyController.DateAndTime.iGetRTCDay.getIntValue(0));\r\n\r\n}";
        let own = "int h = MyController.DateAndTime.iGetRTCHour.getIntValue();\nif (h > 12) { h = h - 12; }\nMyController.DateAndTime.iSetRTCHour.write(h);\nMyController.Energy.reTotalEnergy.write((float)Math.sqrt(2.0));";

        // Uno que no compila: error con su posición y el panel sin tocar.
        let bad: PanelInsertRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Alarms",
            "items": [{"create": {"type": "switch", "label": "Mal", "on_touch": [{"op": "script", "script": "MyController.NoExiste.write(1);"}]}, "x": 0, "y": 0, "name": "McpMal"}]
        }))
        .unwrap();
        let e = PanelOps::insert(&b, &bad).unwrap_err().to_string();
        assert!(e.contains("línea 1") && e.contains("NoExiste"), "{e}");

        let ins: PanelInsertRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Alarms",
            "items": [
                {"create": {"type": "switch", "label": "Sync", "on_touch": [{"op": "script", "script": date_time}]}, "x": 10, "y": 40, "name": "McpScript1"},
                {"create": {"type": "switch", "label": "Own", "on_release": [{"op": "script", "script": own}]}, "x": 80, "y": 40, "name": "McpScript2"}
            ]
        }))
        .unwrap();
        let r = PanelOps::insert(&b, &ins).unwrap();
        eprintln!("{:#?}\n{:?}", r.changes, r.warnings);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert!(!r.objects.iter().any(|o| o.name == "McpMal"));
        // panel_edit: un script nuevo en «al tocar» de McpScript2 (el de «al soltar» se queda).
        let ed: vijeo_core::PanelEditRequest = serde_json::from_value(serde_json::json!({
            "project": p.0, "target": target, "panel": "Alarms",
            "operations": [{"op": "set_actions", "object": "McpScript2", "on_touch": [{"op": "script", "script": "Sys.beep();"}]}]
        }))
        .unwrap();
        let e = PanelOps::edit(&b, &ed).unwrap();
        assert!(e.applied && e.warnings.is_empty(), "{:?}", e.warnings);

        // Lo que Vijeo guarda tras pegar: la fuente vuelve enlazada ({F,2:…}) y el Java tal cual.
        let raw = b.copy_panel_raw(p, &target, "Alarms").unwrap().unwrap();
        let clip = Clip::from_bytes(&raw).unwrap();
        let ops = |name: &str| -> Vec<Op> {
            let path = clip.resolve(name).unwrap_or_else(|e| panic!("falta {name}: {e}"));
            let s = Switch::parse(clip.node(&path).class_data()).unwrap();
            s.on_touch.iter().chain(&s.on_release).map(|a| a.op.clone()).collect()
        };
        match &ops("McpScript1")[..] {
            [Op::Script { source, compiled }] => {
                eprintln!("fuente guardada: {source:?}");
                assert_eq!(compiled, java);
                assert!(source.contains("Sys.setTime(") && source.contains("iGetRTCHour}.getIntValue()"), "{source}");
            }
            o => panic!("{o:?}"),
        }
        match &ops("McpScript2")[..] {
            [Op::Script { compiled: beep, .. }, Op::Script { compiled, .. }] => {
                assert_eq!(beep, "end:{CgpSystem.beep();\r\n}");
                eprintln!("{compiled}");
                assert!(compiled.starts_with("end:{int h =TagDB.MyController.DateAndTime.iGetRTCHour.getIntValue(0);"), "{compiled}");
                assert!(compiled.contains("TagDB.MyController.DateAndTime.iSetRTCHour.write(h);"), "{compiled}");
                assert!(compiled.contains("TagDB.MyController.Energy.reTotalEnergy.write((float)Math.sqrt(2.0));"), "{compiled}");
            }
            o => panic!("{o:?}"),
        }

        let dir = work_dir("script_actions");
        let x = b.reports().unwrap().export_xml(p, &dir.join("xml"), true).unwrap();
        let xml = std::fs::read_to_string(&x.path).unwrap();
        let obj = |name: &str| -> String {
            let at = xml.find(&format!("Name=\"{name}\"")).unwrap_or_else(|| panic!("{name} no está en el XML"));
            let start = xml[..at].rfind('<').unwrap();
            let end = xml[start..].find("</Switch>").map(|e| start + e + 9).unwrap_or(start + 4000);
            xml[start..end.min(xml.len())].to_owned()
        };
        let (s1, s2) = (obj("McpScript1"), obj("McpScript2"));
        eprintln!("---- McpScript1\n{s1}\n---- McpScript2\n{s2}");
        assert!(s1.contains("<Script ScriptType=\"NativeScript\"") && s1.contains("Sys.setTime(MyController.DateAndTime.iGetRTCHour.getIntValue()"), "{s1}");
        assert!(s2.contains("<WhenRelease>") && s2.contains("iSetRTCHour.write(h)"), "{s2}");

        let vdz = dir.join("McpScripts.vdz");
        b.projects().unwrap().save(p, Some(&vdz), true).unwrap();
        b.projects().unwrap().close(p, false).unwrap();
        let built = b.build().unwrap().build_file(&vdz, None).unwrap();
        eprintln!("original: {:?}\ncopia: {:?}", base.messages, built.messages);
        assert_eq!(errs(&built.messages), errs(&base.messages));
        assert_eq!(built.warnings, base.warnings, "{:?}", built.messages);
        b.shutdown().unwrap();
    }

    fn panels_insert(b: &std::sync::Arc<dyn vijeo_core::Backend>, ins: &vijeo_core::PanelInsertRequest) -> vijeo_core::PanelChangeResult {
        let r = b.panels().unwrap().insert(ins).unwrap();
        eprintln!("{:#?}\n{:?}\n{:?}", r.changes, r.warnings, r.notes);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        r
    }
}

/// Lee todas las piezas de la Toolchest de la instalación (sin lanzar Vijeo): las que no tienen la
/// cabecera antigua (versión 2) tienen que leerse y reconstruirse byte a byte.
#[test]
fn real_toolchest_parts() {
    let Some(inst) = installation() else {
        eprintln!("sin Vijeo 6.3: se omite");
        return;
    };
    use vijeo_backend_vd63::objects::{Clip, LEGACY, PANEL};
    use vijeo_backend_vd63::toolchest;
    let t = std::time::Instant::now();
    let libs = toolchest::libraries(&inst.frame_dir);
    assert!(libs.len() > 100, "{} bibliotecas", libs.len());
    let (mut ok, mut legacy, mut panels, mut failed, mut clips) = (0, 0, 0, Vec::new(), Vec::new());
    for lib in &libs {
        for (p, r) in toolchest::read_all(lib) {
            match r {
                Ok(c) => {
                    assert!(!c.objects.is_empty());
                    ok += 1;
                    clips.push((format!("{}/{p}", lib.name), c));
                }
                Err(e) if e == LEGACY => legacy += 1,
                Err(e) if e == PANEL => panels += 1,
                Err(e) => failed.push(format!("{}/{p}: {e}", lib.name)),
            }
        }
    }
    eprintln!("piezas: {ok} leídas, {legacy} con la cabecera antigua, {panels} paneles enteros, {} ms", t.elapsed().as_millis());
    for (id, c) in &clips {
        let back = Clip::from_bytes(&c.to_cfb().unwrap()).unwrap();
        assert_eq!(back.stream(), c.stream(), "{id}");
    }
    assert!(failed.is_empty(), "{failed:?}");
    assert!(ok >= 800, "{ok}");
    // Smart objects (grupos v4, v7 y v8): la cola y la animación SmartObject se leen y se reescriben igual.
    use vijeo_backend_vd63::objects::Node;
    use vijeo_backend_vd63::widgets::smartobj;
    fn smart<'a>(n: &'a Node, out: &mut Vec<&'a Node>) {
        if n.kind() == "SmartObject" {
            out.push(n);
        }
        n.children().iter().for_each(|(_, c)| smart(c, out));
    }
    let (mut so, mut so_bad) = (0, Vec::new());
    for (id, c) in &clips {
        let mut nodes = Vec::new();
        c.objects.iter().for_each(|n| smart(n, &mut nodes));
        for n in nodes {
            so += 1;
            let Node::Group { tail, .. } = n else { unreachable!() };
            match smartobj::read(n).unwrap() {
                Ok(t) if t.write() == *tail && smartobj::item_id(n).is_some() => {}
                Ok(_) => so_bad.push(format!("{id} {}: distinta o sin animación tipada", n.name())),
                Err(e) => so_bad.push(format!("{id} {}: {e}", n.name())),
            }
        }
    }
    eprintln!("smart objects: {so}, con problemas: {}", so_bad.len());
    assert!(so_bad.is_empty(), "{so_bad:#?}");
    assert!(so >= 400, "{so}");
    let (d, c) = toolchest::find(&libs, r"DateTime\Date_Display_01").unwrap();
    assert_eq!((d.kinds[0].as_str(), d.width, d.height), ("SmartObject", 100, 20));
    assert_eq!(c.objects[0].children().len(), 2);
}
