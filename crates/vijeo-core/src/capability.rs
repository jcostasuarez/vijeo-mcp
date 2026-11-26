use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Funcionalidades que una versión de Vijeo puede ofrecer (o no) a través de su backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Crear, abrir, guardar y cerrar proyectos `.vdz`.
    ProjectLifecycle,
    /// Listar, añadir y borrar targets.
    TargetManage,
    /// Compilar proyecto o target.
    Build,
    /// Export Project (`.vxml`) y lectura de variables, paneles y acciones de ese XML.
    Reports,
    /// Referencias cruzadas de las variables, calculadas del XML del proyecto.
    CrossReferences,
    /// Existencia y tipo de una variable, del XML del proyecto.
    VariableLookup,
    /// Paneles de un target, del XML del proyecto.
    PanelList,
    /// Catálogo de familias y modelos HMI descifrado de la instalación.
    ModelCatalog,
    /// Drivers de comunicación instalados (`HMI IO\*_PDL.dll`).
    DriverCatalog,
    /// Drivers, equipos y propiedades del target leídos del `.vdz`.
    ProjectConfig,
    /// IP de descarga del target y dirección de sus equipos escritas en el `.vdz`.
    ConfigEdit,
    /// Validar un `.vxml` contra el `public.xsd` de la instalación, sin Vijeo.
    XmlValidate,
    /// Revisar los scripts del proyecto (o un texto suelto) contra sus variables y la API de scripts.
    ScriptLint,
    /// Escribir un CSV de variables para «Import Variables» de Vijeo.
    VariableCsv,
    /// Convertir recetas y registros de datos con `DataConverter.exe`.
    DataConvert,
    /// Abrir un panel en la ventana de Vijeo de un proyecto abierto (Navigator).
    PanelOpen,
    /// Importar variables de un CSV en un target de un proyecto abierto (Variables › Import Variables).
    VariableImport,
}

impl Capability {
    pub const ALL: [Capability; 17] = [
        Capability::ProjectLifecycle,
        Capability::TargetManage,
        Capability::Build,
        Capability::Reports,
        Capability::CrossReferences,
        Capability::VariableLookup,
        Capability::PanelList,
        Capability::ModelCatalog,
        Capability::DriverCatalog,
        Capability::ProjectConfig,
        Capability::ConfigEdit,
        Capability::XmlValidate,
        Capability::ScriptLint,
        Capability::VariableCsv,
        Capability::DataConvert,
        Capability::PanelOpen,
        Capability::VariableImport,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Capability::ProjectLifecycle => "project_lifecycle",
            Capability::TargetManage => "target_manage",
            Capability::Build => "build",
            Capability::Reports => "reports",
            Capability::CrossReferences => "cross_references",
            Capability::VariableLookup => "variable_lookup",
            Capability::PanelList => "panel_list",
            Capability::ModelCatalog => "model_catalog",
            Capability::DriverCatalog => "driver_catalog",
            Capability::ProjectConfig => "project_config",
            Capability::ConfigEdit => "config_edit",
            Capability::XmlValidate => "xml_validate",
            Capability::ScriptLint => "script_lint",
            Capability::VariableCsv => "variable_csv",
            Capability::DataConvert => "data_convert",
            Capability::PanelOpen => "panel_open",
            Capability::VariableImport => "variable_import",
        }
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CapabilityState {
    Supported,
    /// Existe pero requiere activar una opción de configuración.
    Gated { option: String },
    Unsupported { reason: String },
}

/// Estado de cada capacidad en un backend. Las que no aparecen se consideran no soportadas.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct CapabilitySet(BTreeMap<Capability, CapabilityState>);

impl CapabilitySet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, cap: Capability, state: CapabilityState) -> Self {
        self.0.insert(cap, state);
        self
    }

    pub fn supported(self, cap: Capability) -> Self {
        self.with(cap, CapabilityState::Supported)
    }

    pub fn unsupported(self, cap: Capability, reason: impl Into<String>) -> Self {
        self.with(cap, CapabilityState::Unsupported { reason: reason.into() })
    }

    pub fn state(&self, cap: Capability) -> CapabilityState {
        self.0
            .get(&cap)
            .cloned()
            .unwrap_or_else(|| CapabilityState::Unsupported { reason: "no implementada en este backend".into() })
    }

    pub fn is_supported(&self, cap: Capability) -> bool {
        matches!(self.0.get(&cap), Some(CapabilityState::Supported))
    }

    pub fn iter(&self) -> impl Iterator<Item = (Capability, &CapabilityState)> {
        self.0.iter().map(|(c, s)| (*c, s))
    }
}
