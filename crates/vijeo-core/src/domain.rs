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
