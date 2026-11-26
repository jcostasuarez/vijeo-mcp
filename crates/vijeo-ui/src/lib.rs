//! Control de Vijeo-Designer desde fuera, como lo haría un usuario y sin la API COM.
//!
//! - [`ui::UiInstance`]: un `Vijeo-Frame.exe` lanzado como ejecutable, manejado con comandos de menú,
//!   diálogos ([`dialog`]), el árbol del Navigator ([`tree`]) y la Feedback Zone ([`feedback`]).
//! - [`process`]: localizar, vigilar y terminar procesos de Vijeo.
//! - [`discover_installations`]: instalaciones de Vijeo-Designer según el registro.

pub mod dialog;
pub mod feedback;
mod install;
pub mod process;
pub mod text;
pub mod tree;
pub mod ui;

pub use install::discover_installations;
