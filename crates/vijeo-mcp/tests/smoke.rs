//! Humo MCP: lanza el binario y habla JSON-RPC por stdio.
//! `smoke_*` no lanzan Vijeo; `live_*` (feature `live-vijeo`) sí, sobre ficheros temporales.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Client {
    fn start() -> Self {
        // Sin la caché de --probe-models del usuario: el catálogo sigue la regla de creabilidad.
        let no_cache = std::env::temp_dir().join("vijeo-mcp-smoke-no-cache");
        let mut child = Command::new(env!("CARGO_BIN_EXE_vijeo-mcp"))
            .arg("--cache-dir")
            .arg(&no_cache)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut c = Client { child, stdin, stdout, next: 1 };
        let init = c.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "smoke", "version": "0"}}),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "vijeo-mcp", "{init}");
        c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        c
    }

    fn send(&mut self, v: Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert!(self.stdout.read_line(&mut line).unwrap() > 0, "el servidor cerró stdout");
            let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("no es JSON ({e}): {line}"));
            if v["id"] == id {
                return v;
            }
        }
    }

    /// Devuelve (is_error, structuredContent o texto).
    fn call(&mut self, tool: &str, args: Value) -> (bool, Value) {
        let r = self.request("tools/call", json!({"name": tool, "arguments": args}));
        let res = &r["result"];
        assert!(!res.is_null(), "{r}");
        let is_error = res["isError"].as_bool().unwrap_or(false);
        let body = if res["structuredContent"].is_null() { res["content"][0]["text"].clone() } else { res["structuredContent"].clone() };
        (is_error, body)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // Cerrar stdin termina el servidor, que cierra sus instancias de Vijeo (Close + Exit).
        let (_, w) = std::io::pipe().unwrap();
        drop(std::mem::replace(&mut self.stdin, ChildStdin::from(std::os::windows::io::OwnedHandle::from(w))));
        let t = std::time::Instant::now();
        while t.elapsed() < std::time::Duration::from_secs(60) {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        eprintln!("el servidor no terminó tras cerrar stdin; se mata");
        let _ = self.child.kill();
    }
}

#[test]
fn smoke_list_and_catalog() {
    let mut c = Client::start();
    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"vijeo_installations"));

    let (err, inst) = c.call("vijeo_installations", json!({}));
    assert!(!err, "{inst}");
    if inst["installations"].as_array().unwrap().is_empty() {
        eprintln!("sin Vijeo: sólo se prueban las herramientas globales");
        return;
    }
    // Con 6.3 instalada: las capacidades no soportadas no se listan.
    for t in ["project_open", "target_add", "variable_lookup", "catalog_list_models"] {
        assert!(names.contains(&t), "falta {t}");
    }
    assert!(names.contains(&"variables_import"));
    assert!(!names.iter().any(|n| n.starts_with("panel_export") || n.contains("download") || (n.contains("import") && *n != "variables_import")));

    let (err, models) = c.call("catalog_list_models", json!({"creatable": true}));
    assert!(!err, "{models}");
    // 29 con bloque <SoMachine> + los que sólo ofrece el asistente de New Project.
    assert_eq!(models["count"], 56);

    let (err, drivers) = c.call("catalog_list_drivers", json!({}));
    assert!(!err, "{drivers}");
    assert_eq!(drivers["count"], 64);
    let (err, modbus) = c.call("catalog_list_drivers", json!({"manufacturer": "schneider", "transport": "ethernet", "text": "modbus"}));
    assert!(!err, "{modbus}");
    assert!(modbus["drivers"].as_array().unwrap().iter().any(|d| d["id"] == "Schneider_Modbus_TCP"), "{modbus}");

    // project_show_in_vijeo: los errores se detectan antes de lanzar Vijeo (ninguno de estos lo lanza).
    assert!(names.contains(&"project_show_in_vijeo"));
    for args in [json!({}), json!({"path": r"C:\no\existe.vdz", "project": 1}), json!({"path": r"C:\no-existe-mcp\x.vdz"}), json!({"path": r"C:\x.txt"})] {
        let (err, msg) = c.call("project_show_in_vijeo", args);
        assert!(err && msg.as_str().unwrap().contains("[invalid_argument]"), "{msg}");
    }
    let (err, msg) = c.call("project_show_in_vijeo", json!({"project": 99}));
    assert!(err && msg.as_str().unwrap().contains("[project_not_found]"), "{msg}");

    // Validación del nombre antes de tocar Vijeo.
    let (err, msg) = c.call("project_rename", json!({"project": 99, "name": "a.b"}));
    assert!(err && msg.as_str().unwrap().contains("[invalid_argument]"), "{msg}");
    assert!(names.contains(&"panel_create"));
    let (err, msg) = c.call("panel_create", json!({"project": 99, "target": "HMI1", "name": "a.b"}));
    assert!(err && msg.as_str().unwrap().contains("[invalid_argument]"), "{msg}");
    let (err, msg) = c.call("panel_create", json!({"project": 99, "target": "HMI1"}));
    assert!(err && msg.as_str().unwrap().contains("[project_not_found]"), "{msg}");
    assert!(names.contains(&"equipment_add"));
    for args in [
        json!({"project": 99, "target": "HMI1", "driver": "Modbus TCP", "name": "PLC 1"}),
        json!({"project": 99, "target": "HMI1", "driver": "Modbus TCP", "ip_address": "192.168.1"}),
        json!({"project": 99, "target": "HMI1", "driver": " "}),
    ] {
        let (err, msg) = c.call("equipment_add", args);
        assert!(err && msg.as_str().unwrap().contains("[invalid_argument]"), "{msg}");
    }
    let (err, msg) = c.call("equipment_add", json!({"project": 99, "target": "HMI1", "driver": "Modbus TCP"}));
    assert!(err && msg.as_str().unwrap().contains("[project_not_found]"), "{msg}");
    assert!(names.contains(&"panel_rename") && names.contains(&"panel_delete"));
    let (err, msg) = c.call("panel_rename", json!({"project": 99, "target": "HMI1", "panel": "Start", "new_name": "Inicio 2"}));
    assert!(err && msg.as_str().unwrap().contains("[invalid_argument]"), "{msg}");
    let (err, msg) = c.call("panel_rename", json!({"project": 99, "target": "HMI1", "kind": "popup", "panel": "Pop", "new_name": "Avisos"}));
    assert!(err && msg.as_str().unwrap().contains("[project_not_found]"), "{msg}");
    let (err, _) = c.call("panel_rename", json!({"project": 99, "target": "HMI1", "kind": "master", "panel": "Std", "new_name": "X"}));
    assert!(err, "kind desconocido");
    let (err, msg) = c.call("panel_delete", json!({"project": 99, "target": "HMI1", "panel": "Start"}));
    assert!(err && msg.as_str().unwrap().contains("[project_not_found]"), "{msg}");

    let (err, status) = c.call("vijeo_status", json!({}));
    assert!(!err, "{status}");
    assert_eq!(status["capabilities"]["build"]["state"], "supported");
    assert!(status["capabilities"].get("xml_import").is_none() && status["capabilities"].get("panel_object").is_none());

    // Errores de validación: no lanzan Vijeo.
    let (err, msg) = c.call("project_open", json!({"path": "C:\\x\\proyecto.SwxCF"}));
    assert!(err && msg.as_str().unwrap().contains("[invalid_argument]"), "{msg}");
    let (err, msg) = c.call(
        "project_open",
        json!({"path": "C:\\Users\\Public\\Documents\\Vijeo-Designer 6.3\\Vijeo-Manager\\x.vdz"}),
    );
    assert!(err && msg.as_str().unwrap().contains("[refused]"), "{msg}");
    let (err, msg) = c.call("project_info", json!({"project": 99}));
    assert!(err && msg.as_str().unwrap().contains("[project_not_found]"), "{msg}");
    let (err, msg) = c.call("variables_import", json!({"project": 99, "csv": "C:\\x\\v.csv"}));
    assert!(err && msg.as_str().unwrap().contains("[project_not_found]"), "{msg}");
}

/// Herramientas que no abren Vijeo: CSV de variables, validación XSD (System.Xml) y DataConverter.
#[test]
fn smoke_file_tools() {
    let mut c = Client::start();
    let (_, inst) = c.call("vijeo_installations", json!({}));
    if inst["installations"].as_array().unwrap().is_empty() {
        eprintln!("sin Vijeo: se omite");
        return;
    }
    let dir = std::env::temp_dir().join("vijeo-mcp-smoke-files");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let csv = dir.join("vars.csv");
    let vars = json!([
        {"name": "Planta.Marcha", "data_type": "BOOL", "address": "%M1", "scan_group": "PLC", "alarm": {"message": "Motor en marcha"}},
        {"name": "Contador", "data_type": "int", "initial_value": "5", "description": "Año"}
    ]);
    let (err, r) = c.call("variables_generate_csv", json!({"path": csv, "language": "Español", "variables": vars}));
    assert!(!err && r["variables"] == 2 && r["folders"] == 1, "{r}");
    let bytes = std::fs::read(&csv).unwrap();
    assert!(bytes.starts_with(b"'5.1.0, Vijeo-Designer 6.3.0 CSV output\r\nType,Name,"), "{:?}", &bytes[..60]);
    assert!(bytes.windows(14).any(|w| w == b"Folder,Planta,"));
    let (err, msg) = c.call("variables_generate_csv", json!({"path": csv, "variables": vars}));
    assert!(err && msg.as_str().unwrap().contains("ya existe"), "{msg}");
    let (err, msg) = c.call("variables_generate_csv", json!({"path": dir.join("b.csv"), "variables": [{"name": "X", "data_type": "WORD"}]}));
    assert!(err && msg.as_str().unwrap().contains("[invalid_argument]"), "{msg}");

    let xml = dir.join("p.vxml");
    std::fs::write(&xml, "<?xml version=\"1.0\"?>\n<Project Name=\"P\"><Desconocido/></Project>").unwrap();
    let (err, v) = c.call("xml_validate", json!({"path": xml}));
    assert!(!err && v["valid"] == false && v["errors"].as_u64().unwrap() > 0 && v["fatal"].is_null(), "{v}");
    assert!(!v["summary"].as_array().unwrap().is_empty() && v["issues"][0]["line"].as_u64() > Some(0), "{v}");
    std::fs::write(&xml, "<Project><Target></Project>").unwrap();
    let (err, v) = c.call("xml_validate", json!({"path": xml}));
    assert!(!err && v["valid"] == false && v["fatal"].is_string(), "{v}");

    let input = dir.join("in");
    std::fs::create_dir_all(&input).unwrap();
    let (err, d) = c.call("data_convert", json!({"input": input.join("nada.dat"), "output": dir.join("out"), "mode": "to_csv"}));
    assert!(err, "entrada que no existe: {d}");
    std::fs::write(input.join("roto.dat"), b"no es un registro").unwrap();
    let (err, d) = c.call("data_convert", json!({"input": input.join("roto.dat"), "output": dir.join("out"), "mode": "to_csv", "headers": true}));
    // DataConverter termina con 0 y deja un CSV vacío: `ok` lo da por fallido por su salida.
    assert!(!err && d["ok"] == false && d["exit_code"] == 0, "{d}");
    assert!(d["errors"][0].as_str().unwrap().contains("Not a valid DAT file"), "{d}");
    let (err, msg) = c.call("data_convert", json!({"input": input, "output": dir.join("out"), "mode": "csv_to_rcp"}));
    assert!(err && msg.as_str().unwrap().contains("recipe_dir"), "{msg}");
}

#[cfg(feature = "live-vijeo")]
#[test]
fn live_project_roundtrip_over_mcp() {
    let dir = std::env::temp_dir().join("vijeo-mcp-smoke");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let vdz = dir.join("Smoke.vdz");
    let mut c = Client::start();

    let (err, info) = c.call(
        "project_new",
        json!({"name": "Smoke", "save_as": vdz, "target": {"name": "HMI1", "model": "HMIGTO5310"}}),
    );
    assert!(!err, "{info}");
    let p = info["id"].as_u64().unwrap();
    assert_eq!(info["targets"][0]["model"]["internal_name"], "Model_HMIGTO5310");

    let (err, panels) = c.call("panel_list", json!({"project": p, "target": "HMI1"}));
    assert!(!err, "{panels}");
    let (err, t) = c.call("target_rename", json!({"project": p, "target": "hmi1", "new_name": "Pantalla"}));
    assert!(!err && t["name"] == "Pantalla", "{t}");
    let (err, info) = c.call("project_rename", json!({"project": p, "name": "SmokeRenombrado"}));
    assert!(!err && info["name"] == "SmokeRenombrado" && info["dirty"] == true, "{info}");
    // Se lee del XML de Export Project: no hace falta que el proyecto compile (un target recién creado no compila).
    let (err, rep) = c.call("panel_report", json!({"project": p, "target": "Pantalla"}));
    assert!(!err && rep["panels"][0]["panel_ref"] == "BP:\\Panel1", "{rep}");
    let (err, b) = c.call("project_build", json!({"project": p}));
    assert!(!err, "{b}");
    let (err, v) = c.call("variable_lookup", json!({"project": p, "target": "Pantalla", "name": "NoExiste"}));
    assert!(!err && v["exists"] == false, "{v}");
    let (err, st) = c.call("vijeo_status", json!({}));
    assert!(!err && st["projects"].as_array().unwrap().len() == 1, "{st}");
    let (err, closed) = c.call("project_close", json!({"project": p, "discard": true}));
    assert!(!err, "{closed}");
}
