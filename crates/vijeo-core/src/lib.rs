//! Núcleo independiente de versión del MCP de Vijeo-Designer: tipos de dominio, capacidades,
//! errores y los traits que implementa cada backend (uno por versión de Vijeo).
//!
//! La capa MCP sólo conoce este crate; todo lo específico de una versión vive en su backend.

mod backend;
mod capability;
mod domain;
mod error;
mod install;

pub use backend::*;
pub use capability::*;
pub use domain::*;
pub use error::*;
pub use install::*;
