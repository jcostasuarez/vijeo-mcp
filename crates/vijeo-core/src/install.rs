use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Serialize;

/// Versión de producto (`FileVersion` de `Vijeo-Frame.exe`, p.ej. `6.3.0.103`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub build: u16,
}

impl Version {
    pub const fn new(major: u16, minor: u16, patch: u16, build: u16) -> Self {
        Self { major, minor, patch, build }
    }

    /// Acepta de 1 a 4 componentes separados por `.` (`"6.3"`, `"6.3.0.103"`).
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = [0u16; 4];
        let mut n = 0;
        for p in s.trim().split('.') {
            if n == 4 {
                return None;
            }
            parts[n] = p.trim().parse().ok()?;
            n += 1;
        }
        (n > 0).then(|| Self::new(parts[0], parts[1], parts[2], parts[3]))
    }

    /// `major.minor`, que es lo que distingue a un backend.
    pub fn short(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}.{}", self.major, self.minor, self.patch, self.build)
    }
}

impl Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl JsonSchema for Version {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Version".into()
    }
    fn json_schema(g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(g)
    }
}

/// Una instalación de Vijeo-Designer encontrada en el equipo.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Installation {
    /// `FileVersion` de `Vijeo-Frame.exe` (o la del registro si no se pudo leer).
    pub product_version: Version,
    /// Carpeta `...\Vijeo-Frame`.
    pub frame_dir: PathBuf,
    /// Clave de registro de la instalación (`...\Vijeo-Designer63\06.30`).
    pub registry_key: String,
}

impl Installation {
    pub fn frame_exe(&self) -> PathBuf {
        self.frame_dir.join("Vijeo-Frame.exe")
    }

    /// Carpeta raíz de la instalación (padre de `Vijeo-Frame`).
    pub fn root_dir(&self) -> PathBuf {
        self.frame_dir.parent().map(PathBuf::from).unwrap_or_else(|| self.frame_dir.clone())
    }
}

/// Grado en que un backend soporta una instalación.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    No,
    /// Versión no verificada pero cercana: se arranca con las capacidades no verificadas desactivadas.
    Compatible,
    Exact,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_versions() {
        assert_eq!(Version::parse("6.3.0.103"), Some(Version::new(6, 3, 0, 103)));
        assert_eq!(Version::parse("6.3"), Some(Version::new(6, 3, 0, 0)));
        assert_eq!(Version::parse(" 6.2.10 "), Some(Version::new(6, 2, 10, 0)));
        assert_eq!(Version::parse(""), None);
        assert_eq!(Version::parse("6.x"), None);
        assert_eq!(Version::parse("1.2.3.4.5"), None);
        assert!(Version::new(6, 3, 0, 103) > Version::new(6, 2, 9, 999));
        assert_eq!(Version::new(6, 3, 0, 103).short(), "6.3");
    }
}
