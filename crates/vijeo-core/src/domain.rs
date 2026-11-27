use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Identificador de un proyecto abierto en la sesión del MCP (no lo asigna Vijeo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ProjectId(pub u32);

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectInfo {
    pub id: ProjectId,
    pub name: String,
    /// Ruta del `.vdz` del que se abrió o donde se guardó por última vez.
    pub path: Option<PathBuf>,
    /// Hay cambios hechos con el MCP que no están en el `.vdz` (lo editado a mano en la ventana de Vijeo no se
    /// detecta, pero project_save también lo guarda).
    pub dirty: bool,
    pub targets: Vec<TargetInfo>,
    /// Qué hizo Vijeo con la copia al abrirla (actualizar la versión, convertir un modelo no soportado…).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectSummary {
    pub id: ProjectId,
    pub name: String,
    pub path: Option<PathBuf>,
    /// PID del proceso `Vijeo-Frame.exe` que tiene abierto el proyecto.
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TargetInfo {
    pub name: String,
    /// Modelo guardado y, en los modulares, la pantalla (`Model_HMIG3U:HMIDT351 (800x480)`; sin pantalla si
    /// su resolución la comparten varias).
    pub hmi_type: String,
    /// Modelo del catálogo que corresponde a `hmi_type`, si se reconoce.
    pub model: Option<ModelRef>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ModelRef {
    pub internal_name: String,
    /// Nombre comercial.
    pub name: String,
    pub display: Option<String>,
}

/// Familia de HMI (`Targets.xml`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FamilyInfo {
    pub internal_name: String,
    pub name: String,
    pub default_model: Option<String>,
    pub model_count: usize,
}

/// Modelo de HMI del catálogo (`Models_*.xml`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ModelInfo {
    /// Código interno que usa COM (`Model_HMIS5T`).
    pub internal_name: String,
    /// Nombre comercial (`HMIS5T(HMIS65/S85)`).
    pub name: String,
    /// Valor canónico para crear el target con la pantalla por defecto.
    pub hmi_type: String,
    pub family: String,
    pub family_name: Option<String>,
    /// Pantallas posibles en los modelos modulares (HMIGTU…).
    pub display_units: Vec<String>,
    pub default_display: Option<String>,
    pub screen_width: Option<u32>,
    pub screen_height: Option<u32>,
    pub target_os: Option<String>,
    pub download_types: Vec<String>,
    /// Tiene bloque `<SoMachine>` (se puede usar en proyectos de Machine Expert).
    pub somachine: bool,
    /// Tiene una segunda Ethernet (ETH2) aparte de la de descarga (`DualEthernetSupported`: HMIG2U, HMIG3U, HMIG3X).
    pub dual_ethernet: bool,
    /// Máscara de licencia que exige el modelo.
    pub required_key: u64,
    /// Se puede crear con el asistente de Vijeo: lo que dio `--probe-models` (`probe`) o, si no se probó,
    /// la regla (familia activa y licencia estándar).
    pub creatable: bool,
    /// Resultado de `--probe-models` para este modelo, si se probó.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe: Option<ProbeOutcome>,
}

/// Resultado de crear en vivo un target de un modelo (`vijeo-mcp --probe-models`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeOutcome {
    /// Vijeo creó el target con ese modelo y esa pantalla.
    Created,
    /// El asistente no lo ofrece, Vijeo crea otro modelo o lo rechaza con un aviso.
    Rejected,
    /// La prueba falló por otra causa (tiempo, caída de Vijeo): no dice nada del modelo.
    Failed,
}

/// Prueba en vivo de un modelo.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelProbe {
    pub internal_name: String,
    /// Lo que se pidió al asistente (pantalla por defecto en los modulares).
    pub hmi_type: String,
    pub outcome: ProbeOutcome,
    /// Motivo del rechazo o del fallo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ModelProbeReport {
    /// Fichero de caché donde quedó el resultado (lo lee el MCP al arrancar).
    pub cache: PathBuf,
    pub probed: Vec<ModelProbe>,
    /// Modelos que no se probaron, con el motivo (familia desactivada: el asistente no la ofrece).
    pub skipped: Vec<String>,
    pub duration_ms: u64,
}

/// Cómo pide el usuario un modelo de HMI.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelSelector {
    /// Código interno (`Model_HMIS5T`), nombre comercial (`HMIGTO5310`) o `hmi_type` completo
    /// (`Model_HMIG3U:HMIDT351 (800x480)`). Sin distinguir mayúsculas.
    pub model: String,
    /// Pantalla para modelos modulares (HMIGTU/GTUX): nombre exacto o su prefijo (`HMIDT351`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

/// Resultado de resolver un [`ModelSelector`] contra el catálogo.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ResolvedModel {
    pub model: ModelInfo,
    /// Valor exacto a pasar a `NewTarget`.
    pub hmi_type: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ModelFilter {
    /// Familia (nombre interno o comercial, sin distinguir mayúsculas).
    #[serde(default)]
    pub family: Option<String>,
    /// Sólo modelos que se pueden crear.
    #[serde(default)]
    pub creatable: Option<bool>,
    /// Texto a buscar en código, nombre comercial o pantallas.
    #[serde(default)]
    pub text: Option<String>,
}

/// Driver de comunicación instalado (`HMI IO\<id>_PDL.dll`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriverInfo {
    /// Nombre del fichero sin `_PDL.dll` (`Schneider_Modbus_TCP`).
    pub id: String,
    pub file: String,
    /// Fabricante, deducido del prefijo del fichero.
    pub manufacturer: String,
    /// Protocolo, deducido del resto del nombre (`Modbus TCP`).
    pub protocol: String,
    /// `ethernet` / `serial` / `usb` / `fieldbus`, si el nombre lo indica.
    pub transport: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DriverFilter {
    /// Fabricante (subcadena, sin distinguir mayúsculas).
    #[serde(default)]
    pub manufacturer: Option<String>,
    /// ethernet / serial / usb / fieldbus.
    #[serde(default)]
    pub transport: Option<String>,
    /// Texto a buscar en id, fabricante o protocolo.
    #[serde(default)]
    pub text: Option<String>,
}

/// Acción del informe de Vijeo (global o de panel).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ActionInfo {
    pub target: String,
    /// Número de la acción en el informe.
    pub index: Option<u32>,
    /// Disparador tal como lo escribe el informe (`Conditional - On Data Change, Var`).
    pub trigger: String,
    pub description: Option<String>,
    /// Condición de enclavamiento.
    pub interlock: Option<String>,
    /// Tipo de cuerpo (`Script`, …).
    pub kind: Option<String>,
    /// Código del script u operaciones, en texto plano.
    pub body: Option<String>,
}

/// Panel según el informe de paneles.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PanelReport {
    pub target: String,
    /// Grupo del informe: `Base Panels`, `Master Panels`…
    pub group: String,
    pub name: String,
    /// Ruta dentro del grupo (`Carpeta\Panel`).
    pub path: String,
    /// Ruta de `panel_list` (`BP:\...`), sólo en paneles base.
    pub panel_ref: Option<String>,
    pub actions: Vec<ActionInfo>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PanelRef {
    /// Ruta tal como la devuelve Vijeo: `BP:\Start`, `BP:\Carpeta\Panel`.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BuildResult {
    pub errors: i32,
    pub warnings: i32,
    pub duration_ms: u64,
    /// Errores y warnings con su texto, leídos de la ventana de salida de Vijeo.
    pub messages: Vec<BuildMessage>,
    /// Ventana de salida completa de esta compilación (sin líneas vacías ni separadores).
    pub output: Vec<String>,
    /// Por qué faltan los mensajes, si no se pudieron leer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_note: Option<String>,
    /// Qué hizo Vijeo con la copia al abrirla (actualizar la versión, convertir un modelo no soportado…).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageSeverity {
    Error,
    Warning,
}

/// Un error o warning de la ventana de salida (`Error 1331: [Driver] [X] requires Equipment Address.`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct BuildMessage {
    pub severity: MessageSeverity,
    /// Número de mensaje de Vijeo (`1331`).
    pub code: Option<u32>,
    /// Texto tras el código, con las rutas `[Driver] [X]` que indican el objeto afectado.
    pub text: String,
    /// Sección de la salida en la que aparece (`Target - HMI1`, `HMI1 - I/O`…).
    pub context: Option<String>,
}

/// Contenido actual de la ventana de salida de Vijeo.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FeedbackLog {
    pub messages: Vec<BuildMessage>,
    pub output: Vec<String>,
}

/// Configuración de un proyecto leída de su `.vdz` (drivers, equipos y propiedades de cada target).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectConfig {
    /// Fichero del que se ha leído: el `.vdz`, o el `.SwxCF` de la copia de trabajo de un proyecto abierto
    /// (con los cambios que aún no están en su `.vdz`).
    pub source: PathBuf,
    /// El proyecto abierto tiene cambios sin guardar que este resultado no refleja.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unsaved_changes: bool,
    pub targets: Vec<TargetConfig>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TargetConfig {
    pub name: String,
    /// Carpeta interna del target en el `.SwxCF` (`Target 1`).
    pub storage: String,
    /// Familia (`Target_HMIGTO`).
    pub family: Option<String>,
    pub description: Option<String>,
    /// Modelo guardado en el fichero (`Model_HMIGTO4310`).
    pub model: Option<String>,
    /// `model` con la pantalla, como en `target_list` (sólo con el proyecto abierto).
    pub hmi_type: Option<String>,
    /// Resolución de pantalla guardada en el fichero (`1024x768`): distingue el display dentro del
    /// modelo (HMIDT642 / HMIDT651).
    pub resolution: Option<String>,
    pub download: TargetDownload,
    /// Imagen de arranque (`StartupOptions/SplashFile`).
    pub splash_file: Option<String>,
    /// Resto de valores de texto de las propiedades del target, sin etiquetar, en el orden en que se guardan.
    pub other_values: Vec<String>,
    pub drivers: Vec<IoDriverConfig>,
}

/// Propiedades de descarga del target (dirección IP del HMI, puerto serie…).
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct TargetDownload {
    pub ip_address: Option<String>,
    pub subnet_mask: Option<String>,
    pub default_gateway: Option<String>,
    /// Ruta de descarga a memoria USB/CF (`E:\`).
    pub path: Option<String>,
    pub serial_port: Option<String>,
    pub baud_rate: Option<u32>,
}

/// Driver de comunicación configurado en el I/O Manager de un target.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct IoDriverConfig {
    /// Nombre del driver en el proyecto (`ModbusTCPIP01`).
    pub name: String,
    pub manufacturer: Option<String>,
    /// Identificador del driver (`Schneider_Modbus_PDL_TCP`).
    pub driver_id: Option<String>,
    pub storage: String,
    /// Parámetros del driver (`BaudRate`, `TimeoutPeriod`…) tal como los guarda Vijeo.
    pub settings: std::collections::BTreeMap<String, String>,
    pub equipment: Vec<IoEquipmentConfig>,
}

/// Equipo (PLC, variador…) de un driver.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct IoEquipmentConfig {
    pub name: String,
    /// Protocolo (`Schneider_Modbus_PCL_TCP`).
    pub protocol_id: Option<String>,
    pub storage: String,
    /// Configuración del equipo tal como la guarda el driver (`[4]T:0:192.168.1.10:502:255:...`).
    pub address: Option<String>,
    /// Primera dirección IPv4 de `address`.
    pub ip_address: Option<String>,
    /// Número que sigue a la IP en `address`.
    pub port: Option<u16>,
    /// Unit ID (sólo Modbus TCP: el campo que sigue al puerto).
    pub unit_id: Option<u8>,
    pub scan_groups: Vec<String>,
}

/// Cambios de `target_config_set`. Lo que no se indica se deja como está.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TargetConfigChanges {
    /// Dirección del HMI (propiedades de descarga del target).
    #[serde(default)]
    pub download: Option<DownloadChanges>,
    /// Equipos (PLC, medidores…) a cambiar.
    #[serde(default)]
    pub equipment: Vec<EquipmentChanges>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DownloadChanges {
    /// IPv4 del HMI (`192.168.1.10`).
    #[serde(default)]
    pub ip_address: Option<String>,
    /// Máscara de subred (`255.255.255.0`).
    #[serde(default)]
    pub subnet_mask: Option<String>,
    /// Puerta de enlace (`0.0.0.0` si no hay).
    #[serde(default)]
    pub default_gateway: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct EquipmentChanges {
    /// Nombre del equipo, como en target_config (sin distinguir mayúsculas).
    pub name: String,
    /// Driver del equipo, sólo si hay dos equipos con el mismo nombre.
    #[serde(default)]
    pub driver: Option<String>,
    /// IPv4 del equipo.
    #[serde(default)]
    pub ip_address: Option<String>,
    /// Puerto TCP (sólo Modbus TCP).
    #[serde(default)]
    pub port: Option<u16>,
    /// Unit ID (sólo Modbus TCP, 0-255).
    #[serde(default)]
    pub unit_id: Option<u8>,
}

/// Un valor cambiado por `target_config_set`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ConfigChange {
    /// `download` o `<driver>/<equipo>`.
    pub item: String,
    /// `ip_address`, `subnet_mask`, `default_gateway` o `address` (la cadena completa del equipo).
    pub field: String,
    pub before: Option<String>,
    pub after: String,
}

/// Resultado de `target_config_set`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ConfigEditResult {
    /// `.vdz` escrito (o que se escribiría con `dry_run`).
    pub path: PathBuf,
    /// Se escribió el fichero: `false` con `dry_run` o si no había nada que cambiar.
    pub written: bool,
    pub changes: Vec<ConfigChange>,
    /// Configuración del target tal como queda (releída del fichero escrito).
    pub target: TargetConfig,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Resultado de Export Project.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct XmlExportResult {
    /// `.vxml` escrito (XML público, `public.xsd`).
    pub path: PathBuf,
    /// Carpeta con las imágenes que referencia el XML (atributos `FileName`/`FilePath` con rutas absolutas).
    pub images_dir: Option<PathBuf>,
    pub images: usize,
    pub targets: usize,
    /// Paneles base de todos los targets.
    pub panels: usize,
}

/// Dónde se usa una variable (calculado del XML del proyecto).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CrossReference {
    pub target: String,
    pub variable: String,
    /// Sitios donde aparece: `Panel «Start» › Switch «Switch1» @Variable`, `Acción global 2 › Script`…
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct VariableInfo {
    pub target: String,
    pub name: String,
    pub data_type: String,
    /// `Internal` / `External`.
    pub source: String,
    pub address: Option<String>,
    pub description: Option<String>,
    /// Columnas adicionales del informe que no tienen campo propio.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub extra: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct VariableLookup {
    pub name: String,
    pub exists: bool,
    pub data_type: Option<String>,
}

/// Resultado de validar un XML contra el `public.xsd` de la instalación.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct XmlValidation {
    pub path: PathBuf,
    pub schema: PathBuf,
    /// Sin errores (los warnings no cuentan) y bien formado.
    pub valid: bool,
    pub errors: usize,
    pub warnings: usize,
    /// El XML no está bien formado: la validación se paró en ese punto.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fatal: Option<String>,
    /// Todos los errores y warnings agrupados por mensaje (el mismo fallo se repite en cada objeto).
    pub summary: Vec<XmlIssueGroup>,
    /// Los primeros, en el orden del fichero.
    pub issues: Vec<XmlIssue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct XmlIssue {
    pub severity: MessageSeverity,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct XmlIssueGroup {
    pub severity: MessageSeverity,
    pub message: String,
    pub count: usize,
    pub first_line: u32,
}

/// Un script revisado por `script_lint`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ScriptLint {
    pub target: String,
    /// Dónde está (`GraphicalPanels › Panel «Start» › Action 1 › Script`), o `script` si es un texto suelto.
    pub location: String,
    /// `ScriptType` del XML (`WindowScript`, `GlobalActionScript`…).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_type: Option<String>,
    pub lines: usize,
    /// Resultado del compilador de scripts de Vijeo: `true` compila, `false` no (sus errores van en `issues`), ausente
    /// si no se pudo comprobar (ver `compiler_note`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compiles: Option<bool>,
    /// Por qué no se comprobó con el compilador (usa objetos que el MCP todavía no le describe, o no se pudo lanzar).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compiler_note: Option<String>,
    pub issues: Vec<ScriptIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ScriptIssue {
    pub severity: MessageSeverity,
    /// Línea y columna dentro del script, desde 1.
    pub line: u32,
    pub column: u32,
    pub message: String,
}

/// Codificación del CSV de variables.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CsvEncoding {
    /// Página de códigos OEM del sistema, como el «ANSI CSV» que exporta Vijeo (437 o 850 en Windows en español).
    #[default]
    Oem,
    /// UTF-16 LE con BOM («Unicode CSV»).
    Unicode,
}

/// Una variable del CSV de `variables_generate_csv`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct VariableSpec {
    /// Nombre completo; las carpetas van con puntos (`Carpeta.Sub.Variable`). Letras, dígitos y `_`.
    pub name: String,
    /// BOOL, INT, UINT, DINT, UDINT, Integer, REAL o STRING.
    pub data_type: String,
    /// Internal o External. Por defecto External si hay `address`, si no Internal.
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Valor inicial (sólo Internal): número, On/Off en BOOL o texto en STRING.
    #[serde(default)]
    pub initial_value: Option<String>,
    /// Bytes de un STRING: par, de 2 a 100 (por defecto 20).
    #[serde(default)]
    pub string_length: Option<u32>,
    /// Grupo de escaneo del equipo (si el equipo no tiene grupos, el nombre del equipo). Sólo External.
    #[serde(default)]
    pub scan_group: Option<String>,
    /// Dirección en el equipo (`%MW610`). Sólo External.
    #[serde(default)]
    pub address: Option<String>,
    /// Bit dentro de la palabra (`X3`), para BOOL sobre una palabra.
    #[serde(default)]
    pub bit: Option<String>,
    /// Con signo (2sComplement) o sin signo (Unsigned). Por defecto INT y DINT con signo; UINT, UDINT e Integer sin signo.
    #[serde(default)]
    pub signed: Option<bool>,
    /// 16 o 32 bits. Por defecto 16 en INT, UINT e Integer y 32 en DINT y UDINT.
    #[serde(default)]
    pub data_length: Option<u32>,
    /// Compartir la variable: None (por defecto) o Read Only.
    #[serde(default)]
    pub sharing: Option<String>,
    /// Conservar el valor al apagar (sólo Internal).
    #[serde(default)]
    pub retentive: bool,
    /// Alarma de la variable (sólo BOOL).
    #[serde(default)]
    pub alarm: Option<VariableAlarmSpec>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct VariableAlarmSpec {
    /// Mensaje (como mucho 160 caracteres).
    pub message: String,
    /// `when high` (por defecto) o `when low`.
    #[serde(default)]
    pub trigger: Option<String>,
    /// Grupo de alarmas (por defecto None).
    #[serde(default)]
    pub group: Option<String>,
    /// Prioridad de 1 (máxima) a 999. Por defecto 1.
    #[serde(default)]
    pub severity: Option<u32>,
}

/// Petición de `variables_generate_csv`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct VariableCsvRequest {
    /// Ruta absoluta del `.csv` que se escribe.
    pub path: PathBuf,
    /// Reemplazar el fichero si ya existe.
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub encoding: CsvEncoding,
    /// Idioma de los mensajes de alarma (columna `<idioma> ID`): el nombre del primer idioma del proyecto
    /// (`Español`). Con `project` se toma del proyecto; si no, `Idioma1`.
    #[serde(default)]
    pub language: Option<String>,
    /// Proyecto abierto contra el que se comprueban los nombres, los grupos de escaneo y el idioma.
    #[serde(default)]
    pub project: Option<ProjectId>,
    /// Target del proyecto (puede faltar si sólo tiene uno).
    #[serde(default)]
    pub target: Option<String>,
    pub variables: Vec<VariableSpec>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct VariableCsvResult {
    pub path: PathBuf,
    pub variables: usize,
    pub folders: usize,
    pub encoding: CsvEncoding,
    pub language: String,
    /// Lo que conviene revisar antes de importar (variables que ya existen, grupos de escaneo desconocidos…).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Petición de `variables_import`: un CSV ya escrito (`csv`) o las variables para generarlo (`variables`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct VariableImportRequest {
    /// Proyecto abierto.
    pub project: ProjectId,
    /// Target (puede faltar si el proyecto sólo tiene uno).
    #[serde(default)]
    pub target: Option<String>,
    /// Ruta absoluta de un CSV de variables (Export Variables de Vijeo o variables_generate_csv), delimitado por comas.
    #[serde(default)]
    pub csv: Option<PathBuf>,
    /// Variables a importar (como en variables_generate_csv): se genera el CSV y se importa. Excluye `csv`.
    #[serde(default)]
    pub variables: Option<Vec<VariableSpec>>,
    /// Con `variables`: idioma de los mensajes de alarma (por defecto, el primero del proyecto).
    #[serde(default)]
    pub language: Option<String>,
    /// Reemplazar las variables que ya existen («Overwrite existing variables»). Si no, se conservan las del proyecto.
    #[serde(default)]
    pub replace_existing: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct VariableImportResult {
    pub target: String,
    /// CSV importado (con `variables`, el generado en la carpeta de trabajo, que se borra al cerrar el proyecto).
    pub csv: PathBuf,
    /// Variables que Vijeo dice haber importado (`N variables were imported.`).
    pub imported: Option<usize>,
    /// Lo que escribió Vijeo en la ventana de salida.
    pub feedback: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Conversión de `DataConverter.exe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DataConvertMode {
    /// Recetas (`.rcp`) y registros de datos (`.dat`) a `.csv` (o `.txt` con otro delimitador).
    ToCsv,
    /// `.rcp` a CSV simplificado (`-csr`).
    RcpToSimpleCsv,
    /// `.rcp` al CSV común del runtime (`-crf`).
    RcpToRuntimeCsv,
    /// CSV/TXT simplificado a `.rcp`, validado contra las recetas de `recipe_dir` (`-ccr`).
    CsvToRcp,
    /// CSV común del runtime a `.rcp` (`-cfr`).
    RuntimeCsvToRcp,
}

/// Formato de fecha de las marcas de tiempo de los `.dat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DataDateFormat {
    /// MM/DD/YY (`-ct`).
    MmDdYy,
    /// YY/MM/DD (`-ct0`).
    YyMmDd,
    /// DD/MM/YY (`-ct1`).
    DdMmYy,
    /// YYYY/MM/DD (`-ct3`).
    YyyyMmDd,
    /// DD/MM/YYYY (`-ct4`).
    DdMmYyyy,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DataConvertRequest {
    /// Fichero o carpeta de entrada (ruta absoluta).
    pub input: PathBuf,
    /// Carpeta de salida (ruta absoluta; se crea si no existe). El nombre de cada fichero es el de su origen.
    pub output: PathBuf,
    pub mode: DataConvertMode,
    /// Carpeta con las recetas `.rcp` descargadas del HMI contra las que se valida el CSV (`-rp`); obligatoria
    /// en `csv_to_rcp` y distinta de `output`.
    #[serde(default)]
    pub recipe_dir: Option<PathBuf>,
    /// Incluir subcarpetas de `input` (`-s`).
    #[serde(default)]
    pub subfolders: bool,
    /// Con `subfolders`, repetir la estructura de carpetas en la salida (`-b`).
    #[serde(default)]
    pub keep_structure: bool,
    /// Sobrescribir los ficheros que ya existan en la salida (`-cw`).
    #[serde(default)]
    pub overwrite: bool,
    /// Añadir cabeceras (Time stamp, Data, Quality) a los `.dat` convertidos (`-ch`).
    #[serde(default)]
    pub headers: bool,
    /// Convertir las marcas de tiempo de los `.dat` a fecha.
    #[serde(default)]
    pub date_format: Option<DataDateFormat>,
    /// Delimitador de campos: un carácter (no `|`) o `tab`. Con uno distinto de la coma la salida es `.txt`.
    #[serde(default)]
    pub delimiter: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DataConvertResult {
    /// `DataConverter` terminó con 0 y no se quejó de ningún fichero (con un fichero no válido también
    /// devuelve 0 y deja un CSV vacío).
    pub ok: bool,
    pub exit_code: Option<i32>,
    /// Líneas de la salida que dicen que un fichero no se pudo convertir (`<fichero>:  Not a valid DAT file`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
    /// Ficheros de la carpeta de salida nuevos o modificados.
    pub output_files: Vec<PathBuf>,
    /// Lo que escribió DataConverter (con `-v`, el progreso).
    pub log: Vec<String>,
    pub duration_ms: u64,
}

// ---------------------------------------------------------------- objetos de los paneles

/// Objeto de un panel (o de una pieza de la Toolchest), leído de su serialización binaria.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PanelObject {
    /// Ruta para referirse a él en panel_edit: nombres de los grupos que lo contienen y el suyo, separados por `/`
    /// (`GroupObject03/CmdFwdYellow`). Un nombre suelto vale si no se repite en el panel.
    pub path: String,
    pub name: String,
    /// Tipo de objeto (clase COM de Vijeo): `Switch`, `Text`, `RoundRect` (rectángulo), `Group` (grupo o smart object)…
    pub kind: String,
    /// Posición y tamaño en píxeles, absolutos en el panel (el XML da los hijos de un grupo relativos al grupo).
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    /// Textos de los datos propios del objeto (etiquetas, nombre del idioma, fuentes…), sin las referencias a variables.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub strings: Vec<String>,
    /// Variables a las que se refiere (nombre completo con carpetas).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<String>,
    /// Smart object (pieza «inteligente» de la Toolchest: fecha, hora, login…): id de la pieza (`SOItem` del XML).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smart_object: Option<String>,
    /// Propiedades del smart object (las que se cambian con `set_properties` de panel_edit).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<SmartProperty>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<PanelObject>,
}

/// Propiedad de un smart object.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SmartProperty {
    /// Id de la propiedad (el `Property ID` del XML), el que se usa en `properties`.
    pub name: String,
    /// `variable`, `color`, `combo`, `bool`, `int`, `double`, `char`, `string`, `text`, `font`… Sólo se cambian
    /// variables, colores, combos, números, booleanos, caracteres y cadenas.
    pub kind: String,
    /// Nombre visible en el editor, si es distinto del id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Valor: la variable (vacío si no tiene), el color `RRGGBB`, el valor del combo…
    pub value: String,
    /// Combos: opciones `valor «etiqueta»` (se indica el valor o la etiqueta).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PanelObjectsResult {
    pub target: String,
    /// Ruta de panel_list (`BP:\...`).
    pub panel: String,
    /// Objetos contando los de dentro de los grupos.
    pub count: usize,
    pub objects: Vec<PanelObject>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Una operación de panel_edit. `object` es una ruta de panel_objects (`Grupo/Objeto`) o un nombre que no se repita.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PanelEditOp {
    /// Cambia el nombre del objeto. Vijeo no admite nombres repetidos en un panel.
    Rename { object: String, new_name: String },
    /// Desplaza el objeto (y sus hijos, si es un grupo) `dx`, `dy` píxeles.
    Move { object: String, dx: i32, dy: i32 },
    /// Lleva la esquina superior izquierda del objeto a `x`, `y` (con sus hijos).
    MoveTo { object: String, x: i32, y: i32 },
    /// Cambia el tamaño manteniendo la esquina superior izquierda. No vale para grupos.
    Resize { object: String, width: i32, height: i32 },
    /// Sustituye un texto completo del objeto (una etiqueta, el texto de un Text) por otro. `old` tiene que ser
    /// exactamente uno de los `strings` de panel_objects; los saltos de línea son `\n`.
    SetText { object: String, old: String, new: String },
    /// Sustituye `old` por `new` dentro de todas las cadenas del objeto (o de todo el panel sin `object`), incluidas
    /// las referencias a variables: así se cambia la variable de un objeto (`xCmdFwd` → `xCmdRev`). Vijeo vuelve a
    /// enlazar las variables por nombre al pegar.
    Replace {
        #[serde(default)]
        object: Option<String>,
        old: String,
        new: String,
    },
    /// Borra el objeto (con sus hijos, si es un grupo). No puede dejar un grupo vacío.
    Delete { object: String },
    /// Añade animaciones al objeto o sustituye las que ya tenga de la misma clase.
    Animate { object: String, animations: Vec<AnimationSpec> },
    /// Quita animaciones del objeto: las de `kinds` o, sin `kinds`, todas.
    RemoveAnimations {
        object: String,
        #[serde(default)]
        kinds: Vec<AnimationKind>,
    },
    /// Sustituye las acciones de un botón (Switch). Las listas que no se indican no cambian.
    SetActions {
        object: String,
        #[serde(default)]
        on_touch: Option<Vec<SwitchAction>>,
        #[serde(default)]
        on_release: Option<Vec<SwitchAction>>,
        #[serde(default)]
        while_pressed: Option<Vec<SwitchAction>>,
    },
    /// Cambia los colores de un objeto (`RRGGBB`; `none` = transparente). Según el tipo: Rectangle y Circle
    /// (`fill`, `line`), Line (`line`), Text (`text`, `fill` = fondo, `line` = borde), Switch y Lamp (`on`, `off`,
    /// `text`), DataDisplay (`fill` = placa, `text`).
    SetColors {
        object: String,
        #[serde(default)]
        fill: Option<String>,
        #[serde(default)]
        line: Option<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        on: Option<String>,
        #[serde(default)]
        off: Option<String>,
    },
    /// Cambia propiedades de un smart object ({id: valor}, ids y opciones en panel_objects): variables con su
    /// ruta (`Carpeta.Var`, vacía para quitarla; tienen que existir), colores `RRGGBB`, combos por valor o
    /// etiqueta, números, booleanos (`true`/`false`), caracteres y cadenas.
    SetProperties { object: String, properties: std::collections::BTreeMap<String, String> },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PanelEditRequest {
    pub project: ProjectId,
    pub target: String,
    /// Panel base: ruta de panel_list (`BP:\Carpeta\Panel`), ruta sin `BP:\` o sólo el nombre si no se repite.
    pub panel: String,
    /// Operaciones, en orden.
    pub operations: Vec<PanelEditOp>,
    /// Sólo comprobar: copia los objetos, aplica las operaciones y devuelve el resultado sin tocar el panel.
    #[serde(default)]
    pub dry_run: bool,
}

/// Sustitución dentro de las cadenas de un objeto insertado (etiquetas, textos, variables).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TextReplace {
    pub old: String,
    pub new: String,
}

/// Acción de un botón. Las variables van con su ruta completa (`Carpeta.Variable`) y tienen que existir en el
/// proyecto: Vijeo las enlaza por nombre al pegar.
#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum SwitchAction {
    /// Pone a 1 una variable booleana.
    Set { variable: String },
    /// Pone a 0 una variable booleana.
    Reset { variable: String },
    /// Invierte una variable booleana.
    Toggle { variable: String },
    /// 1 mientras se pulsa, 0 al soltar.
    MomentaryOn { variable: String },
    /// 0 mientras se pulsa, 1 al soltar.
    MomentaryOff { variable: String },
    /// Escribe `value` (un número o el nombre de otra variable) en una variable numérica.
    WordSet { variable: String, value: String },
    /// Suma `value` a una variable numérica.
    WordAdd { variable: String, value: String },
    /// Resta `value` a una variable numérica.
    WordSubtract { variable: String, value: String },
    /// Cambia al panel base `panel` (como en project_goto).
    ChangePanel { panel: String },
    /// Vuelve al panel anterior.
    PreviousPanel,
    /// Abre la ventana emergente `popup` (su nombre o su ruta en Popup Windows). Con `x` e `y` (las dos), en esa
    /// posición en píxeles; sin ellas, centrada.
    OpenPopup {
        popup: String,
        #[serde(default)]
        x: Option<i32>,
        #[serde(default)]
        y: Option<i32>,
    },
    /// Cierra la ventana emergente `popup` (su nombre o su ruta en Popup Windows).
    ClosePopup { popup: String },
    /// Ejecuta un script de acción en Java con la API de scripts de Vijeo: variables (`Carpeta.Var.getIntValue()`,
    /// `.getFloatValue()`, `.getStringValue()`, `.write(…)`, `.X(n)`…), `Sys.…` (`setTime`, `beep`, `logout`…), `Math`,
    /// variables locales, `if`, `for`… Se compila con el compilador de Vijeo al aplicarlo: un error se devuelve con su
    /// línea y columna y no se cambia nada. Otros objetos (sonido, recetas, alarmas, ventanas…) no se admiten todavía.
    Script { script: String },
}

/// Fuente de un texto o una etiqueta: las fuentes de mapa de bits de Vijeo (GP).
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FontSize {
    /// GPFontModern6x10.
    #[default]
    Small,
    /// GPFontModern8x13.
    Medium,
    /// GPFontModern13x23.
    Large,
}

/// Alineación horizontal de un texto o una etiqueta.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HAlign {
    Left,
    Center,
    Right,
}

/// Objeto nuevo que crea panel_insert desde cero. Los colores son `RRGGBB` en hexadecimal (`00FF00`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NewObject {
    /// Botón (Switch). Tamaño por defecto 58×18.
    Switch {
        /// Texto del botón (`\n` para partir en varias líneas).
        label: String,
        /// Acciones al tocar, en orden.
        #[serde(default)]
        on_touch: Vec<SwitchAction>,
        /// Acciones al soltar.
        #[serde(default)]
        on_release: Vec<SwitchAction>,
        /// Acciones que se repiten mientras está pulsado.
        #[serde(default)]
        while_pressed: Vec<SwitchAction>,
        /// Variable booleana que enciende la lámpara del botón (color `on_color`).
        #[serde(default)]
        lamp: Option<String>,
        /// Variable booleana que tiene que estar a 1 para que el botón responda (enclavamiento).
        #[serde(default)]
        interlock: Option<String>,
        /// Color del botón encendido (lámpara a 1); por defecto `00FF00`.
        #[serde(default)]
        on_color: Option<String>,
        /// Color del botón apagado; por defecto `C0C0C0`.
        #[serde(default)]
        off_color: Option<String>,
        /// Color del texto; por defecto `626469`.
        #[serde(default)]
        text_color: Option<String>,
        #[serde(default)]
        font: FontSize,
        #[serde(default)]
        bold: bool,
        /// Estilo de dibujo de Vijeo (fichero de `NativeStyles\Primitive`, p.ej. `Square03`, `Round01`).
        #[serde(default)]
        style: Option<String>,
        /// Pitido al tocar.
        #[serde(default)]
        buzzer: bool,
    },
    /// Lámpara (Lamp) de una variable booleana: `on_color` con 1, `off_color` con 0. Tamaño por defecto 40×20.
    Lamp {
        variable: String,
        /// Texto fijo sobre la lámpara; o bien `on_label`/`off_label`, uno por estado.
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        on_label: Option<String>,
        #[serde(default)]
        off_label: Option<String>,
        /// Por defecto `00FF00`.
        #[serde(default)]
        on_color: Option<String>,
        /// Por defecto `C0C0C0`.
        #[serde(default)]
        off_color: Option<String>,
        /// Color del texto; por defecto `000000`.
        #[serde(default)]
        text_color: Option<String>,
        #[serde(default)]
        font: FontSize,
        #[serde(default)]
        bold: bool,
        /// Forma: `blank_area` (rectángulo, por defecto), `square` o `round` (piloto redondo, sin texto).
        #[serde(default)]
        shape: LampShape,
    },
    /// Display numérico (NumericDisplay) de una variable entera o real, con entrada por teclado opcional.
    /// Tamaño por defecto 60×20.
    NumericDisplay {
        variable: String,
        /// Variable real (Float): formato fijo con `decimals` decimales. Si no, entero.
        #[serde(default)]
        float: bool,
        /// Cifras enteras (por defecto 5).
        #[serde(default)]
        digits: Option<u16>,
        /// Decimales (por defecto 0; 2 si `float`).
        #[serde(default)]
        decimals: Option<u16>,
        /// El operador puede escribir el valor (teclado emergente).
        #[serde(default)]
        input: bool,
        /// Con `input`: límites (un número o el nombre de una variable).
        #[serde(default)]
        min: Option<String>,
        #[serde(default)]
        max: Option<String>,
        /// Color de la placa; por defecto `B0B0B0`.
        #[serde(default)]
        plate_color: Option<String>,
        /// Color del texto; por defecto `000000`.
        #[serde(default)]
        text_color: Option<String>,
        #[serde(default)]
        font: FontSize,
        #[serde(default)]
        bold: bool,
        /// Por defecto `center`.
        #[serde(default)]
        align: Option<HAlign>,
    },
    /// Display de texto (StringDisplay) de una variable de tipo string. Tamaño por defecto 100×20.
    StringDisplay {
        variable: String,
        /// Caracteres que muestra (por defecto 10).
        #[serde(default)]
        length: Option<u16>,
        #[serde(default)]
        input: bool,
        /// Color de la placa; por defecto `B0B0B0`.
        #[serde(default)]
        plate_color: Option<String>,
        /// Color del texto; por defecto `000000`.
        #[serde(default)]
        text_color: Option<String>,
        #[serde(default)]
        font: FontSize,
        #[serde(default)]
        bold: bool,
        /// Por defecto `center`.
        #[serde(default)]
        align: Option<HAlign>,
    },
    /// Texto fijo (Text). Tamaño por defecto: el del texto con su fuente.
    Text {
        /// `\n` para partir en varias líneas.
        text: String,
        /// Por defecto `000000`.
        #[serde(default)]
        color: Option<String>,
        /// Fondo; sin él, transparente.
        #[serde(default)]
        background: Option<String>,
        /// Borde; sin él, sin borde.
        #[serde(default)]
        border: Option<String>,
        #[serde(default)]
        font: FontSize,
        #[serde(default)]
        bold: bool,
        /// Por defecto `left`.
        #[serde(default)]
        align: Option<HAlign>,
    },
    /// Rectángulo. Tamaño por defecto 60×40.
    Rectangle {
        /// Relleno; `none` = transparente. Por defecto `9FA0A4`.
        #[serde(default)]
        fill: Option<String>,
        /// Borde; `none` = sin borde. Por defecto `009530`.
        #[serde(default)]
        line: Option<String>,
        /// Ancho del borde en px (por defecto 2).
        #[serde(default)]
        line_width: Option<u16>,
        /// Radio de las esquinas en px (0 = esquinas rectas; por defecto 3).
        #[serde(default)]
        corner: Option<u16>,
    },
    /// Elipse (círculo si `width` = `height`). Tamaño por defecto 30×30.
    Ellipse {
        /// Relleno; `none` = transparente. Por defecto `FF8000`.
        #[serde(default)]
        fill: Option<String>,
        /// Borde; `none` = sin borde. Por defecto `FFFFFF`.
        #[serde(default)]
        line: Option<String>,
        #[serde(default)]
        line_width: Option<u16>,
    },
    /// Línea de (`x`, `y`) a (`end_x`, `end_y`), en coordenadas del panel (sin `width`/`height`).
    Line {
        end_x: i32,
        end_y: i32,
        /// Por defecto `009431`.
        #[serde(default)]
        color: Option<String>,
        /// Por defecto 2.
        #[serde(default)]
        width: Option<u16>,
        /// Flechas: `none` (por defecto), `start`, `end` o `both`.
        #[serde(default)]
        arrows: Option<Arrows>,
    },
}

/// Forma de una lámpara (estilo de dibujo de Vijeo).
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LampShape {
    /// Rectángulo con texto («BlankArea»).
    #[default]
    BlankArea,
    /// Cuadrado con relieve y texto («Square70»).
    Square,
    /// Piloto redondo, sin texto («Round04»).
    Round,
}

/// Flechas de una línea.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Arrows {
    None,
    Start,
    End,
    Both,
}

/// Qué color cambia una animación de color.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorPart {
    /// Relleno (en un Text, el color del texto).
    #[default]
    Fill,
    /// Borde o línea.
    Line,
    /// Fondo (el del patrón; en un Text, el del cuadro).
    Background,
}

/// Animación de un objeto (las de la pestaña Animación de Vijeo). Las variables van con su ruta completa
/// (`Carpeta.Variable`). Un objeto tiene como mucho una animación de cada clase: `visibility` y `blink`
/// comparten la de visibilidad.
#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnimationSpec {
    /// Visible mientras la variable booleana valga 1 (o 0 con `invert`).
    Visibility {
        variable: String,
        #[serde(default)]
        invert: bool,
    },
    /// Parpadea mientras la variable booleana valga 1.
    Blink {
        variable: String,
        #[serde(default)]
        fast: bool,
    },
    /// Cambia un color según una variable booleana: `on_color` con 1, `off_color` con 0 (`RRGGBB`; `none` =
    /// transparente).
    Color {
        variable: String,
        on_color: String,
        off_color: String,
        #[serde(default)]
        part: ColorPart,
    },
    /// Rellena el objeto según el valor de una variable: `min` vacío, `max` lleno.
    Fill {
        variable: String,
        min: f64,
        max: f64,
        /// `up` (desde abajo, por defecto) o `right` (desde la izquierda).
        #[serde(default)]
        direction: FillDirection,
    },
    /// Desplaza el objeto según el valor de una variable: con `min` está en su sitio y con `max`, `dx`, `dy`
    /// píxeles más allá.
    Position {
        variable: String,
        min: f64,
        max: f64,
        #[serde(default)]
        dx: i32,
        #[serde(default)]
        dy: i32,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FillDirection {
    #[default]
    Up,
    Right,
}

/// Clase de animación (para quitarlas con panel_edit).
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnimationKind {
    Visibility,
    Color,
    Fill,
    Position,
    Size,
    Rotate,
    Value,
}

/// Objeto que inserta panel_insert: una pieza de la Toolchest, un objeto de otro panel base del mismo target o
/// un objeto nuevo (`create`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PanelInsertItem {
    /// Objeto nuevo, creado desde cero.
    #[serde(default)]
    pub create: Option<NewObject>,
    /// Con `create`: tamaño (por defecto, el propio de cada tipo).
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,
    /// Variable booleana que muestra el objeto (oculto con 0). En los botones va en el propio botón; en el resto,
    /// como animación de visibilidad.
    #[serde(default)]
    pub visibility: Option<String>,
    /// Animaciones del objeto insertado (sustituyen a las que traiga de la misma clase).
    #[serde(default)]
    pub animations: Vec<AnimationSpec>,
    /// Pieza de la Toolchest: `id` de toolchest_list (`Biblioteca\PartNNN`) o `Biblioteca\<nombre del objeto>`.
    #[serde(default)]
    pub toolchest: Option<String>,
    /// O bien un panel base del mismo target (como en project_goto) del que se copia `object`.
    #[serde(default)]
    pub from_panel: Option<String>,
    /// Con `from_panel`: ruta del objeto en ese panel (panel_objects).
    #[serde(default)]
    pub object: Option<String>,
    /// Esquina superior izquierda en el panel de destino.
    pub x: i32,
    pub y: i32,
    /// Nombre del objeto insertado (si ya existe en el panel, Vijeo le pone otro).
    #[serde(default)]
    pub name: Option<String>,
    /// Sustituciones en sus cadenas (como `replace` de panel_edit), p.ej. para cambiar la variable de un Switch.
    #[serde(default)]
    pub replace: Vec<TextReplace>,
    /// Con `toolchest`, si la pieza es un smart object: sus propiedades {id: valor}, como `set_properties` de
    /// panel_edit (sin ellas llega con las variables vacías y project_build da el error 4336).
    #[serde(default)]
    pub properties: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PanelInsertRequest {
    pub project: ProjectId,
    pub target: String,
    /// Panel base de destino (como en project_goto).
    pub panel: String,
    pub items: Vec<PanelInsertItem>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PanelChangeResult {
    pub target: String,
    pub panel: String,
    /// Se aplicó (false con `dry_run`).
    pub applied: bool,
    /// Lo que se hizo, en orden.
    pub changes: Vec<String>,
    /// Objetos del panel después (con `dry_run`, como quedarían).
    pub objects: Vec<PanelObject>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Pieza de la Toolchest de la instalación.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ToolchestPart {
    /// Para panel_insert: `Biblioteca\PartNNN`.
    pub id: String,
    /// Carpeta de la biblioteca dentro de `Toolchest` (`Graph\BarGraph\Vertical`).
    pub library: String,
    /// Nombre de los objetos de nivel superior de la pieza.
    pub name: String,
    /// Tipos de objeto que contiene (sin repetir).
    pub kinds: Vec<String>,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ToolchestLibrary {
    pub library: String,
    pub parts: usize,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct ToolchestFilter {
    /// Sólo las bibliotecas cuya carpeta contiene este texto (sin distinguir mayúsculas). Sin `library` ni `text`
    /// se listan las bibliotecas, no las piezas.
    #[serde(default)]
    pub library: Option<String>,
    /// Sólo las piezas cuyo nombre, tipo o biblioteca contiene este texto.
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ToolchestResult {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<ToolchestLibrary>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<ToolchestPart>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}
