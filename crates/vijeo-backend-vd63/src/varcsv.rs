//! CSV de variables en el formato de Export/Import Variables de Vijeo 6.3 (ayuda §6.6.3).
//!
//! Línea 1: `'5.1.0, Vijeo-Designer 6.3.0 CSV output`; línea 2: las 46 columnas; después, primero las
//! carpetas (`Folder,<ruta con puntos>,,,,"",`) y luego una fila por variable con 47 campos (la última
//! columna acaba en coma). Qué columnas lleva cada tipo y cuáles van entre comillas sale de los CSV que
//! exporta Vijeo 6.3 (12 000 filas de proyectos reales): BOOL, enteros, REAL y STRING, internas y externas,
//! con y sin alarma. La columna 10 es `<idioma> ID` con el idioma de los mensajes de alarma (`1` = L1).

use std::collections::BTreeSet;

use vijeo_core::{VariableAlarmSpec, VariableSpec, VijeoError, VijeoResult};

/// Versión del formato que escribe Vijeo 6.3 en la primera línea.
const FORMAT_VERSION: &str = "5.1.0";

const COLUMNS: [&str; 46] = [
    "Type",
    "Name",
    "Data Type",
    "Data Source",
    "Dimension",
    "Description",
    "Initial Value",
    "NumofBytes",
    "Data Sharing",
    "Alarm",
    "{lang} ID",
    "Alarm Message",
    "Alarm Type",
    "Trigger Condition",
    "Deadband",
    "Target",
    "LoLo\\Lo\\Hi\\HiHi",
    "Minor",
    "Major",
    "Alarm Group",
    "Severity",
    "Vibration Pattern",
    "Vibration Time",
    "Sound File",
    "Play Mode",
    "Scan Group",
    "Device Address",
    "Bit Number",
    "Data Format",
    "Signed",
    "Data Length",
    "Offset Bit No",
    "Bit Width",
    "InputRange",
    "Min",
    "Max",
    "DataScaling",
    "RawMin",
    "RawMax",
    "ScaledMin",
    "ScaledMax",
    "IndirectEnabled",
    "IndirectAddress",
    "Retentive",
    "LoggingGroup",
    "LogUserOperationsOnVariable",
];

/// Palabras reservadas de Vijeo (ayuda, apéndice 3 «Reserved Words»): no pueden ser el nombre de una
/// variable ni de una carpeta, sin distinguir mayúsculas. Import Variables se para en la primera
/// (`Import operation halted. Invalid Variable name was found … at line N`) y no importa nada (verificado
/// con una carpeta `Int`).
const RESERVED: &[&str] = &[
    "abstract", "APPLICATION_BUILD_DATE", "APPLICATION_BUILD_TIME", "array", "assert", "bitfield16", "bitfield32", "blockdint", "blockfloat", "blockint",
    "blockinteger", "blockudint", "blockuint", "bool", "boolean", "bottom", "break", "BUILD_NUM", "byte", "case", "catch", "char", "class", "const",
    "continue", "default", "DEFAULT_GTWY", "dint", "discrete", "DNS_ADDRESS_1", "do", "double", "E", "else", "enum", "extends", "false", "final",
    "finally", "float", "for", "FREE_KBYTE_MAIN", "FREE_KBYTE_OPTIONAL", "FREE_KBYTE_SECONDARY", "goto", "HMI", "HMI_APPLICATION_CHECKSUM", "if",
    "implements", "import", "instanceof", "int", "integer", "interface", "IP_ADDRESS_1", "IP_ADDRESS_2", "left", "long", "MAC_ADDRESS_1",
    "MAC_ADDRESS_2", "Math", "native", "new", "null", "package", "PI", "private", "PROEVIEW", "PROJECT_NAME", "protected", "public", "read", "real",
    "RecipeControl", "return", "right", "short", "Sound", "static", "STATION_NUMBER", "strictfp", "string", "StringBuffer", "structure",
    "SUBNET_MASK", "super", "switch", "synchronized", "Sys", "TARGET_NAME", "TARGET_TYPE", "this", "throw", "throws", "top", "TOTAL_DRAM_USAGE",
    "transient", "true", "try", "udint", "uint", "VERSION_NUM", "void", "volatile", "WEBGATE", "while", "write",
];

const DATA_TYPES: [&str; 8] = ["BOOL", "INT", "UINT", "DINT", "UDINT", "Integer", "REAL", "STRING"];

/// Columnas (índice en [`COLUMNS`]).
mod col {
    pub const TYPE: usize = 0;
    pub const NAME: usize = 1;
    pub const DATA_TYPE: usize = 2;
    pub const SOURCE: usize = 3;
    pub const DIMENSION: usize = 4;
    pub const DESCRIPTION: usize = 5;
    pub const INITIAL: usize = 6;
    pub const BYTES: usize = 7;
    pub const SHARING: usize = 8;
    pub const ALARM: usize = 9;
    pub const LANG: usize = 10;
    pub const ALARM_MESSAGE: usize = 11;
    pub const ALARM_TYPE: usize = 12;
    pub const TRIGGER: usize = 13;
    pub const LIMITS: usize = 16;
    pub const ALARM_GROUP: usize = 19;
    pub const SEVERITY: usize = 20;
    pub const VIBRATION: usize = 21;
    pub const SOUND: usize = 23;
    pub const PLAY_MODE: usize = 24;
    pub const SCAN_GROUP: usize = 25;
    pub const ADDRESS: usize = 26;
    pub const BIT: usize = 27;
    pub const FORMAT: usize = 28;
    pub const SIGNED: usize = 29;
    pub const LENGTH: usize = 30;
    pub const INPUT_RANGE: usize = 33;
    pub const SCALING: usize = 36;
    pub const INDIRECT: usize = 41;
    pub const RETENTIVE: usize = 43;
    pub const LOG_USER_OPS: usize = 45;
}

/// Una fila del CSV ya validada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub data_type: &'static str,
    pub external: bool,
    pub scan_group: Option<String>,
    fields: Vec<String>,
}

/// CSV completo (texto con CRLF) y lo que se escribió.
pub struct Csv {
    pub text: String,
    pub folders: Vec<String>,
    pub rows: Vec<Row>,
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn bad(field: String, why: impl Into<String>) -> VijeoError {
    VijeoError::invalid(field, why)
}

fn is_integer(t: &str) -> bool {
    matches!(t, "INT" | "UINT" | "DINT" | "UDINT" | "Integer")
}

fn check_name(field: &str, name: &str) -> VijeoResult<()> {
    if name.is_empty() || name.len() > 255 {
        return Err(bad(field.into(), "el nombre no puede estar vacío"));
    }
    for seg in name.split('.') {
        let mut chars = seg.chars();
        let ok_start = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if !ok_start || !seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(bad(field.into(), format!("«{name}»: cada parte del nombre empieza por letra o `_` y sólo lleva letras ASCII, dígitos y `_` (las carpetas se separan con puntos)")));
        }
        if let Some(r) = RESERVED.iter().find(|r| r.eq_ignore_ascii_case(seg)) {
            return Err(bad(field.into(), format!("«{name}»: «{seg}» es una palabra reservada de Vijeo ({r}) y no puede ser nombre de variable ni de carpeta")));
        }
    }
    Ok(())
}

/// Valor inicial tal como lo escribe Vijeo para el tipo (`Off`/`On`, número, texto).
fn initial_value(field: &str, t: &str, v: Option<&str>) -> VijeoResult<String> {
    let Some(v) = v.map(str::trim) else {
        return Ok(match t {
            "BOOL" => "Off",
            "STRING" => "",
            _ => "0",
        }
        .to_owned());
    };
    let range = |lo: i64, hi: i64| -> VijeoResult<String> {
        match v.parse::<i64>() {
            Ok(n) if (lo..=hi).contains(&n) => Ok(n.to_string()),
            _ => Err(bad(field.into(), format!("«{v}» no es un entero de {t} ({lo} a {hi})"))),
        }
    };
    match t {
        "BOOL" => match v.to_ascii_lowercase().as_str() {
            "on" | "1" | "true" => Ok("On".into()),
            "off" | "0" | "false" => Ok("Off".into()),
            _ => Err(bad(field.into(), format!("«{v}»: un BOOL empieza en On u Off"))),
        },
        "INT" => range(i16::MIN as i64, i16::MAX as i64),
        "UINT" => range(0, u16::MAX as i64),
        "DINT" | "Integer" => range(i32::MIN as i64, i32::MAX as i64),
        "UDINT" => range(0, u32::MAX as i64),
        "REAL" => match v.parse::<f32>() {
            // Vijeo sólo acepta el punto decimal.
            Ok(x) if x.is_finite() && !v.contains(',') => Ok(v.to_owned()),
            _ => Err(bad(field.into(), format!("«{v}» no es un REAL (con punto decimal)"))),
        },
        _ => Ok(v.to_owned()),
    }
}

fn alarm_fields(i: usize, f: &mut [String], t: &str, a: &VariableAlarmSpec) -> VijeoResult<()> {
    let field = |s: &str| format!("variables[{i}].alarm.{s}");
    if t != "BOOL" {
        return Err(bad(field("message"), "sólo se generan alarmas de BOOL (las de límite de los numéricos no están verificadas)"));
    }
    if a.message.trim().is_empty() || a.message.chars().count() > 160 {
        return Err(bad(field("message"), "el mensaje de alarma no puede estar vacío y tiene como mucho 160 caracteres"));
    }
    let trigger = match a.trigger.as_deref().map(|s| s.trim().to_ascii_lowercase()) {
        None => "when high",
        Some(s) if s == "when high" || s == "high" => "when high",
        Some(s) if s == "when low" || s == "low" => "when low",
        Some(s) => return Err(bad(field("trigger"), format!("«{s}»: usa when high o when low"))),
    };
    let severity = a.severity.unwrap_or(1);
    if !(1..=999).contains(&severity) {
        return Err(bad(field("severity"), "de 1 a 999"));
    }
    f[col::ALARM] = "Enable".into();
    f[col::ALARM_MESSAGE] = quote(a.message.trim());
    f[col::ALARM_TYPE] = quote("");
    f[col::TRIGGER] = quote(trigger);
    f[col::LIMITS] = quote("_\\_\\_\\_");
    f[col::ALARM_GROUP] = a.group.as_deref().map(str::trim).filter(|g| !g.is_empty()).unwrap_or("None").to_owned();
    f[col::SEVERITY] = severity.to_string();
    for c in [col::VIBRATION, col::SOUND, col::PLAY_MODE] {
        f[c] = quote("");
    }
    Ok(())
}

/// Valida una variable y arma su fila.
pub fn row(i: usize, v: &VariableSpec) -> VijeoResult<Row> {
    let field = |s: &str| format!("variables[{i}].{s}");
    let name = v.name.trim();
    check_name(&field("name"), name)?;
    let t = DATA_TYPES
        .iter()
        .find(|t| t.eq_ignore_ascii_case(v.data_type.trim()))
        .copied()
        .ok_or_else(|| bad(field("data_type"), format!("«{}»: tipos admitidos {}", v.data_type, DATA_TYPES.join(", "))))?;
    let external = match v.source.as_deref().map(|s| s.trim().to_ascii_lowercase()) {
        None => v.address.is_some(),
        Some(s) if s == "external" => true,
        Some(s) if s == "internal" => false,
        Some(s) => return Err(bad(field("source"), format!("«{s}»: Internal o External"))),
    };
    let mut f = vec![String::new(); COLUMNS.len() + 1];
    f[col::TYPE] = "Variable".into();
    f[col::NAME] = name.to_owned();
    f[col::DATA_TYPE] = t.into();
    f[col::SOURCE] = if external { "External" } else { "Internal" }.into();
    f[col::DIMENSION] = "0".into();
    let desc = v.description.as_deref().unwrap_or("").trim();
    if desc.chars().count() > 256 {
        return Err(bad(field("description"), "como mucho 256 caracteres"));
    }
    f[col::DESCRIPTION] = quote(desc);
    f[col::SHARING] = match v.sharing.as_deref().map(|s| s.trim().to_ascii_lowercase().replace(['_', '-'], " ")) {
        None => "None".into(),
        Some(s) if s == "none" => "None".into(),
        Some(s) if s == "read only" || s == "readonly" => "Read Only".into(),
        Some(s) => return Err(bad(field("sharing"), format!("«{s}»: None o Read Only"))),
    };
    // Los STRING no llevan la columna Alarm.
    f[col::ALARM] = if t == "STRING" { String::new() } else { "Disable".into() };
    f[col::LANG] = "1".into();
    if t == "STRING" {
        let n = v.string_length.unwrap_or(20);
        if !(2..=100).contains(&n) || n % 2 != 0 {
            return Err(bad(field("string_length"), "número par de 2 a 100"));
        }
        f[col::BYTES] = n.to_string();
    } else if v.string_length.is_some() {
        return Err(bad(field("string_length"), "sólo en STRING"));
    }
    if let Some(a) = &v.alarm {
        alarm_fields(i, &mut f, t, a)?;
    }
    let numeric = is_integer(t) || t == "REAL";
    if numeric {
        f[col::INPUT_RANGE] = "Disable".into();
    }
    if external {
        if v.initial_value.is_some() || v.retentive {
            return Err(bad(field("initial_value"), "initial_value y retentive son sólo de variables Internal"));
        }
        let sg = v.scan_group.as_deref().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| bad(field("scan_group"), "una variable External necesita scan_group"))?;
        let addr = v.address.as_deref().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| bad(field("address"), "una variable External necesita address"))?;
        f[col::SCAN_GROUP] = sg.to_owned();
        f[col::ADDRESS] = quote(addr);
        if let Some(b) = v.bit.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            if t != "BOOL" {
                return Err(bad(field("bit"), "sólo en BOOL"));
            }
            f[col::BIT] = b.to_owned();
        }
        if is_integer(t) {
            let signed = v.signed.unwrap_or(matches!(t, "INT" | "DINT"));
            let bits = v.data_length.unwrap_or(if matches!(t, "DINT" | "UDINT") { 32 } else { 16 });
            if bits != 16 && bits != 32 {
                return Err(bad(field("data_length"), "16 o 32"));
            }
            f[col::FORMAT] = "BIN".into();
            f[col::SIGNED] = if signed { "2sComplement" } else { "Unsigned" }.into();
            f[col::LENGTH] = format!("{bits}Bits");
        } else if v.signed.is_some() || v.data_length.is_some() {
            return Err(bad(field("signed"), "signed y data_length son sólo de los tipos enteros"));
        }
        if numeric {
            f[col::SCALING] = "Disable".into();
        }
        f[col::INDIRECT] = "Disable".into();
    } else {
        if v.address.is_some() || v.scan_group.is_some() || v.bit.is_some() {
            return Err(bad(field("address"), "address, scan_group y bit son sólo de variables External"));
        }
        if v.signed.is_some() || v.data_length.is_some() {
            return Err(bad(field("signed"), "signed y data_length son sólo de variables External"));
        }
        f[col::INITIAL] = quote(&initial_value(&field("initial_value"), t, v.initial_value.as_deref())?);
        f[col::RETENTIVE] = if v.retentive { "Enable" } else { "Disable" }.into();
    }
    f[col::LOG_USER_OPS] = "Disable".into();
    Ok(Row { name: name.to_owned(), data_type: t, external, scan_group: external.then(|| f[col::SCAN_GROUP].clone()), fields: f })
}

/// Valida todas las variables y arma el CSV. `vijeo_version` es la de la primera línea (`6.3.0`).
pub fn build(vars: &[VariableSpec], language: &str, vijeo_version: &str) -> VijeoResult<Csv> {
    if vars.is_empty() {
        return Err(VijeoError::invalid("variables", "no hay ninguna variable"));
    }
    if language.trim().is_empty() || language.contains([',', '"', '\r', '\n']) {
        return Err(VijeoError::invalid("language", "nombre de idioma vacío o con comas, comillas o saltos de línea"));
    }
    let rows: Vec<Row> = vars.iter().enumerate().map(|(i, v)| row(i, v)).collect::<VijeoResult<_>>()?;
    let mut seen = BTreeSet::new();
    let mut folders: Vec<String> = Vec::new();
    for r in &rows {
        if !seen.insert(r.name.to_ascii_lowercase()) {
            return Err(VijeoError::invalid("variables", format!("«{}» está repetida (Vijeo no distingue mayúsculas en los nombres)", r.name)));
        }
        let parts: Vec<&str> = r.name.split('.').collect();
        for k in 1..parts.len() {
            let folder = parts[..k].join(".");
            if !folders.iter().any(|f| f.eq_ignore_ascii_case(&folder)) {
                folders.push(folder);
            }
        }
    }
    if let Some(r) = rows.iter().find(|r| folders.iter().any(|f| f.eq_ignore_ascii_case(&r.name))) {
        return Err(VijeoError::invalid("variables", format!("«{}» es a la vez variable y carpeta", r.name)));
    }
    let header: Vec<String> = COLUMNS.iter().map(|c| c.replace("{lang}", language.trim())).collect();
    let mut text = format!("'{FORMAT_VERSION}, Vijeo-Designer {vijeo_version} CSV output\r\n{}\r\n", header.join(","));
    for f in &folders {
        text.push_str(&format!("Folder,{f},,,,\"\",\r\n"));
    }
    for r in &rows {
        text.push_str(&r.fields.join(","));
        text.push_str("\r\n");
    }
    Ok(Csv { text, folders, rows })
}

/// Bytes del fichero: UTF-16 LE con BOM, o la página OEM (con aviso de los caracteres que no caben).
pub fn encode(text: &str, unicode: bool) -> (Vec<u8>, bool) {
    if unicode {
        let mut b = vec![0xFF, 0xFE];
        b.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        (b, false)
    } else {
        vijeo_ui::text::to_oem(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, t: &str) -> VariableSpec {
        serde_json::from_value(serde_json::json!({ "name": name, "data_type": t })).unwrap()
    }

    fn line(v: VariableSpec) -> String {
        row(0, &v).unwrap().fields.join(",")
    }

    /// Filas exportadas por Vijeo 6.3 (Tags-*.CSV del usuario), con el idioma «Idioma1».
    #[test]
    fn rows_match_vijeo_export() {
        let mut v = spec("QE1", "BOOL");
        v.scan_group = Some("EquipoModbus01".into());
        v.address = Some("%MW610".into());
        v.bit = Some("X3".into());
        assert_eq!(line(v), r#"Variable,QE1,BOOL,External,0,"",,,None,Disable,1,,,,,,,,,,,,,,,EquipoModbus01,"%MW610",X3,,,,,,,,,,,,,,Disable,,,,Disable,"#);

        let mut v = spec("sde_qb2", "BOOL");
        v.scan_group = Some("EquipoModbus01".into());
        v.address = Some("%MW612".into());
        v.bit = Some("X1".into());
        v.alarm = Some(VariableAlarmSpec { message: "/SET PRINCIPAL: DISPARO/".into(), trigger: None, group: Some("GrupoDeAlarmas1".into()), severity: None });
        assert_eq!(
            line(v),
            r#"Variable,sde_qb2,BOOL,External,0,"",,,None,Enable,1,"/SET PRINCIPAL: DISPARO/","","when high",,,"_\_\_\_",,,GrupoDeAlarmas1,1,"",,"","",EquipoModbus01,"%MW612",X1,,,,,,,,,,,,,,Disable,,,,Disable,"#
        );

        let mut v = spec("SET1_C1E1_PAT", "int");
        v.scan_group = Some("PLC_MT".into());
        v.address = Some("%MW4001".into());
        assert_eq!(line(v), r#"Variable,SET1_C1E1_PAT,INT,External,0,"",,,None,Disable,1,,,,,,,,,,,,,,,PLC_MT,"%MW4001",,BIN,2sComplement,16Bits,,,Disable,,,Disable,,,,,Disable,,,,Disable,"#);

        let mut v = spec("ZONA_PM1.UDINT01", "UDINT");
        v.scan_group = Some("TPLCN".into());
        v.address = Some("%MW1800".into());
        assert_eq!(line(v), r#"Variable,ZONA_PM1.UDINT01,UDINT,External,0,"",,,None,Disable,1,,,,,,,,,,,,,,,TPLCN,"%MW1800",,BIN,Unsigned,32Bits,,,Disable,,,Disable,,,,,Disable,,,,Disable,"#);

        let mut v = spec("N2R_GW_SLP_Temperatura", "REAL");
        v.description = Some("Gateway SLP - Lectura de Sensor de Temperatura".into());
        v.scan_group = Some("PLC_MT".into());
        v.address = Some("%MW6200".into());
        assert_eq!(
            line(v),
            r#"Variable,N2R_GW_SLP_Temperatura,REAL,External,0,"Gateway SLP - Lectura de Sensor de Temperatura",,,None,Disable,1,,,,,,,,,,,,,,,PLC_MT,"%MW6200",,,,,,,Disable,,,Disable,,,,,Disable,,,,Disable,"#
        );

        let mut v = spec("ZONA_UPS1.TIPO_EQUIP", "STRING");
        v.string_length = Some(10);
        v.scan_group = Some("TPLCN".into());
        v.address = Some("%MW1198".into());
        assert_eq!(line(v), r#"Variable,ZONA_UPS1.TIPO_EQUIP,STRING,External,0,"",,10,None,,1,,,,,,,,,,,,,,,TPLCN,"%MW1198",,,,,,,,,,,,,,,Disable,,,,Disable,"#);

        let mut v = spec("Osc_0_6s", "BOOL");
        v.description = Some("oscilador de 1 seg. interno".into());
        assert_eq!(line(v), r#"Variable,Osc_0_6s,BOOL,Internal,0,"oscilador de 1 seg. interno","Off",,None,Disable,1,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,Disable,,Disable,"#);

        let mut v = spec("Anim_3_estados", "INT");
        v.description = Some("palabra de 3 estados".into());
        assert_eq!(line(v), r#"Variable,Anim_3_estados,INT,Internal,0,"palabra de 3 estados","0",,None,Disable,1,,,,,,,,,,,,,,,,,,,,,,,Disable,,,,,,,,,,Disable,,Disable,"#);

        let mut v = spec("G45", "Integer");
        v.initial_value = Some("45".into());
        v.retentive = true;
        assert_eq!(line(v), r#"Variable,G45,Integer,Internal,0,"","45",,None,Disable,1,,,,,,,,,,,,,,,,,,,,,,,Disable,,,,,,,,,,Enable,,Disable,"#);

        let mut v = spec("ZONA_Generales.Paros_Emergencia.Paro_Emerg_Planta", "BOOL");
        v.description = Some("Paro de emergencia Planta".into());
        v.sharing = Some("read only".into());
        v.alarm = Some(VariableAlarmSpec { message: "Paro de emergencia Planta".into(), trigger: Some("When High".into()), group: None, severity: Some(1) });
        assert_eq!(
            line(v),
            r#"Variable,ZONA_Generales.Paros_Emergencia.Paro_Emerg_Planta,BOOL,Internal,0,"Paro de emergencia Planta","Off",,Read Only,Enable,1,"Paro de emergencia Planta","","when high",,,"_\_\_\_",,,None,1,"",,"","",,,,,,,,,,,,,,,,,,,Disable,,Disable,"#
        );
    }

    #[test]
    fn every_row_has_47_fields() {
        for t in DATA_TYPES {
            let v = spec("X", t);
            assert_eq!(row(0, &v).unwrap().fields.len(), 47);
        }
    }

    #[test]
    fn builds_file_with_folders_first() {
        let mut a = spec("Planta.Linea1.Marcha", "BOOL");
        a.scan_group = Some("PLC".into());
        a.address = Some("%M1".into());
        let csv = build(&[spec("Contador", "INT"), a, spec("Planta.Total", "DINT")], "Español", "6.3.0").unwrap();
        let lines: Vec<&str> = csv.text.split("\r\n").collect();
        assert_eq!(lines[0], "'5.1.0, Vijeo-Designer 6.3.0 CSV output");
        assert!(lines[1].starts_with("Type,Name,Data Type,Data Source,Dimension,Description,Initial Value,NumofBytes,Data Sharing,Alarm,Español ID,Alarm Message,"));
        assert!(lines[1].ends_with(",IndirectEnabled,IndirectAddress,Retentive,LoggingGroup,LogUserOperationsOnVariable"));
        assert_eq!(&lines[2..4], ["Folder,Planta,,,,\"\",", "Folder,Planta.Linea1,,,,\"\","]);
        assert!(lines[4].starts_with("Variable,Contador,INT,Internal"));
        assert_eq!(csv.rows.len(), 3);
        assert_eq!(*lines.last().unwrap(), "");
    }

    #[test]
    fn rejects_what_vijeo_would_not_import() {
        let ext = |t: &str| {
            let mut v = spec("V", t);
            v.scan_group = Some("PLC".into());
            v.address = Some("%MW1".into());
            v
        };
        assert!(row(0, &spec("1abc", "BOOL")).is_err());
        assert!(row(0, &spec("año", "BOOL")).is_err());
        assert!(row(0, &spec("a..b", "BOOL")).is_err());
        assert!(row(0, &spec("Planta.Int.Consigna", "INT")).is_err(), "carpeta con palabra reservada");
        assert!(row(0, &spec("Top", "BOOL")).is_err());
        assert!(row(0, &spec("E", "REAL")).is_err());
        assert!(row(0, &spec("Interno.Topes", "INT")).is_ok());
        assert!(row(0, &spec("V", "WORD")).is_err());
        let mut v = spec("V", "INT");
        v.initial_value = Some("40000".into());
        assert!(row(0, &v).is_err());
        let mut v = spec("V", "REAL");
        v.initial_value = Some("0,5".into());
        assert!(row(0, &v).is_err());
        let mut v = spec("V", "STRING");
        v.string_length = Some(7);
        assert!(row(0, &v).is_err());
        let mut v = ext("INT");
        v.alarm = Some(VariableAlarmSpec { message: "x".into(), trigger: None, group: None, severity: None });
        assert!(row(0, &v).is_err(), "alarma en un INT");
        let mut v = ext("BOOL");
        v.scan_group = None;
        assert!(row(0, &v).is_err(), "External sin scan_group");
        let mut v = ext("BOOL");
        v.initial_value = Some("On".into());
        assert!(row(0, &v).is_err(), "valor inicial en External");
        let mut v = spec("V", "INT");
        v.source = Some("External".into());
        assert!(row(0, &v).is_err(), "External sin dirección");
        assert!(build(&[spec("A", "INT"), spec("a", "BOOL")], "L", "6.3.0").is_err(), "repetida");
        assert!(build(&[spec("A", "INT"), spec("A.B", "BOOL")], "L", "6.3.0").is_err(), "variable y carpeta");
        assert!(build(&[], "L", "6.3.0").is_err());
    }

    #[test]
    fn source_defaults_to_external_with_address() {
        let mut v = spec("V", "UINT");
        v.address = Some("%MW3".into());
        v.scan_group = Some("PLC".into());
        let r = row(0, &v).unwrap();
        assert!(r.external);
        assert_eq!(r.fields[col::SIGNED], "Unsigned");
        let mut v = spec("Q", "BOOL");
        v.description = Some("dice \"hola\"".into());
        assert_eq!(row(0, &v).unwrap().fields[col::DESCRIPTION], "\"dice \"\"hola\"\"\"");
    }

    #[test]
    fn unicode_has_bom() {
        let (b, lossy) = encode("Añ", true);
        assert!(!lossy);
        assert_eq!(b, [0xFF, 0xFE, b'A', 0, 0xF1, 0]);
    }
}
