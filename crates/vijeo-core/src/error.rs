use serde::Serialize;

use crate::Capability;

/// Errores del MCP. Cada variante lleva el contexto necesario para que el modelo entienda qué falló.
#[derive(Debug, Clone, thiserror::Error)]
pub enum VijeoError {
    #[error("Vijeo-Designer no está instalado o no se encontró una instalación utilizable: {0}")]
    NotInstalled(String),
    #[error("la operación {op} superó el tiempo límite de {ms} ms; el proyecto puede haber quedado a medias: revísalo con project_info o ciérralo con discard=true")]
    Timeout { op: String, ms: u64 },
    #[error("Vijeo abrió un diálogo «{title}» que el MCP no sabe responder: {text}")]
    ModalDialog { title: String, text: String },
    #[error("argumento no válido «{field}»: {reason}")]
    InvalidArgument { field: String, reason: String },
    #[error("la capacidad {capability} no está disponible en Vijeo-Designer {version}: {reason}")]
    Unsupported { capability: Capability, version: String, reason: String },
    #[error("el proceso de Vijeo-Designer terminó inesperadamente durante {op}")]
    ServerCrashed { op: String },
    #[error("no hay ningún proyecto abierto con id {0}")]
    ProjectNotFound(u32),
    #[error("no existe el target «{0}» en el proyecto")]
    TargetNotFound(String),
    #[error("límite alcanzado: {0}")]
    Limit(String),
    #[error("operación rechazada: {0}")]
    Refused(String),
    #[error("error de E/S en {path}: {message}")]
    Io { path: String, message: String },
    #[error("error interno: {0}")]
    Internal(String),
}

impl VijeoError {
    /// Código estable que ve el modelo en el resultado de la herramienta.
    pub fn code(&self) -> &'static str {
        match self {
            VijeoError::NotInstalled(_) => "not_installed",
            VijeoError::Timeout { .. } => "timeout",
            VijeoError::ModalDialog { .. } => "modal_dialog",
            VijeoError::InvalidArgument { .. } => "invalid_argument",
            VijeoError::Unsupported { .. } => "unsupported",
            VijeoError::ServerCrashed { .. } => "server_crashed",
            VijeoError::ProjectNotFound(_) => "project_not_found",
            VijeoError::TargetNotFound(_) => "target_not_found",
            VijeoError::Limit(_) => "limit",
            VijeoError::Refused(_) => "refused",
            VijeoError::Io { .. } => "io",
            VijeoError::Internal(_) => "internal",
        }
    }

    pub fn invalid(field: impl Into<String>, reason: impl Into<String>) -> Self {
        VijeoError::InvalidArgument { field: field.into(), reason: reason.into() }
    }

    pub fn io(path: impl AsRef<std::path::Path>, err: impl std::fmt::Display) -> Self {
        VijeoError::Io { path: path.as_ref().display().to_string(), message: err.to_string() }
    }

    pub fn to_report(&self) -> ErrorReport {
        ErrorReport { code: self.code(), message: self.to_string() }
    }
}

/// Forma serializada de un error para el resultado de una herramienta (`is_error = true`).
#[derive(Debug, Clone, Serialize)]
pub struct ErrorReport {
    pub code: &'static str,
    pub message: String,
}

pub type VijeoResult<T> = Result<T, VijeoError>;
