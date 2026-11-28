//! Pantallas de un target leídas del `.vdz`, sin Vijeo: paneles base (con sus carpetas), paneles maestros y
//! ventanas emergentes, con el número que muestra el Navigator.
//!
//! Cada grupo es un almacén con `FolderList` (almacenes `FolderN` de sus carpetas), `WindowList` (almacenes `PanelN`
//! de sus paneles) y, en las carpetas, `WindowEditorProperties` con el nombre. Cada panel tiene `WindowObject`
//! (`u32 0x0B | u32 | nombre | u32 PanelID…`) y `GraphicalObject` (sus objetos). Las ventanas emergentes son
//! `PopupWindowList/UniqueNameK` (lista en `PopupWindows`), con `PageListProperties` (`u32 | u32 | u32 WindowID…`
//! y el nombre) y sus paneles como los de un grupo.

use std::collections::BTreeMap;

use crate::objects::{strings_in, PanelObjects};

pub const BASE: &str = "Base Panels";
pub const MASTER: &str = "Master Panels";
pub const POPUP: &str = "Popup Windows";

/// Panel del `.vdz`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenFile {
    /// [`BASE`], [`MASTER`] o [`POPUP`].
    pub group: &'static str,
    /// Carpetas que lo contienen (sin la ventana en los popups).
    pub folders: Vec<String>,
    pub name: String,
    /// PanelID (`21` de `21: RFID_1`; 10001… en los paneles de las ventanas emergentes, 1000001… en los maestros).
    pub id: u32,
    /// Ventana emergente que lo contiene y su WindowID (`1` de `1: ComErrorCodes`).
    pub window: Option<(String, u32)>,
    /// Almacén del panel (`…/BasePanelsList/Folder0/Panel3`, con `/`).
    pub dir: String,
    /// Objetos de primer nivel (los grupos cuentan como uno).
    pub objects: usize,
}

impl ScreenFile {
    /// Ruta dentro del grupo, como en el XML de Vijeo: `Carpeta\Panel`; en los popups, `Carpeta\Ventana\Panel`.
    pub fn path(&self) -> String {
        let mut parts = self.folders.clone();
        if let Some((w, _)) = &self.window {
            parts.push(w.clone());
        }
        parts.push(self.name.clone());
        parts.join("\\")
    }

    /// Sitio legible, como el de las referencias del XML: `Folder «F» › Panel «P»`, `Window «W» › Panel «P»`,
    /// `MasterPanel «M»`.
    pub fn label(&self) -> String {
        let mut parts: Vec<String> = self.folders.iter().map(|f| format!("Folder «{f}»")).collect();
        match (self.group, &self.window) {
            (_, Some((w, _))) => {
                parts.push(format!("Window «{w}»"));
                parts.push(format!("Panel «{}»", self.name));
            }
            (MASTER, None) => parts.push(format!("MasterPanel «{}»", self.name)),
            _ => parts.push(format!("Panel «{}»", self.name)),
        }
        parts.join(" › ")
    }
}

fn first_string(streams: &BTreeMap<String, Vec<u8>>, path: &str) -> Option<String> {
    streams.get(path).and_then(|b| strings_in(b, 0).into_iter().next())
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// Nombre y PanelID de un panel (`WindowObject`).
fn window_object(b: &[u8]) -> Option<(String, u32)> {
    let n = u32_at(b, 8)? as usize;
    let units: Vec<u16> = b.get(12..12 + n)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let name = String::from_utf16(&units).ok()?.trim_end_matches('\0').to_owned();
    Some((name, u32_at(b, 12 + n)?))
}

/// Paneles de un grupo o carpeta `dir` y, recursivamente, de sus carpetas.
fn walk(streams: &BTreeMap<String, Vec<u8>>, dir: &str, group: &'static str, folders: &[String], window: Option<&(String, u32)>, out: &mut Vec<ScreenFile>) {
    let list = |name: &str| streams.get(&format!("{dir}/{name}")).map(|b| strings_in(b, 0)).unwrap_or_default();
    for panel in list("WindowList") {
        let pdir = format!("{dir}/{panel}");
        let Some((name, id)) = streams.get(&format!("{pdir}/WindowObject")).and_then(|b| window_object(b)) else { continue };
        let objects = streams.get(&format!("{pdir}/GraphicalObject")).and_then(|b| PanelObjects::parse(b).ok()).map(|p| p.objects.len()).unwrap_or(0);
        out.push(ScreenFile { group, folders: folders.to_vec(), name, id, window: window.cloned(), dir: pdir, objects });
    }
    for folder in list("FolderList") {
        let fdir = format!("{dir}/{folder}");
        let name = first_string(streams, &format!("{fdir}/WindowEditorProperties")).unwrap_or_else(|| folder.clone());
        let mut inner = folders.to_vec();
        inner.push(name);
        walk(streams, &fdir, group, &inner, window, out);
    }
}

/// Pantallas de los streams de un target (rutas con `/`). Cada grupo sale en el orden del `.vdz`.
pub fn screens(streams: &BTreeMap<String, Vec<u8>>) -> Vec<ScreenFile> {
    let mut out = Vec::new();
    // Componente del target que tiene las pantallas (`/Targets/Target 1/Component 1`).
    let Some(comp) = streams.keys().find_map(|p| p.split_once("/WindowList/BasePanelsList/").map(|(c, _)| c.to_owned())) else { return out };
    walk(streams, &format!("{comp}/WindowList/BasePanelsList"), BASE, &[], None, &mut out);
    walk(streams, &format!("{comp}/WindowList/MasterPanelsNodeStorage"), MASTER, &[], None, &mut out);
    let popups = format!("{comp}/PopupWindowList");
    for w in first_list(streams, &format!("{popups}/PopupWindows")) {
        let wdir = format!("{popups}/{w}");
        let Some(props) = streams.get(&format!("{wdir}/PageListProperties")) else { continue };
        let name = strings_in(props, 0).into_iter().next().unwrap_or_else(|| w.clone());
        let window = (name, u32_at(props, 8).unwrap_or_default());
        walk(streams, &wdir, POPUP, &[], Some(&window), &mut out);
    }
    out
}

fn first_list(streams: &BTreeMap<String, Vec<u8>>, path: &str) -> Vec<String> {
    streams.get(path).map(|b| strings_in(b, 0)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLES: &str = r"C:\Program Files (x86)\Schneider Electric\EcoStruxure Machine Expert - Basic\Examples";
    const CORPUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), r"\..\..\docs\reference\vd-6.3\corpus\xml");

    fn streams_of(p: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
        let mut c = crate::swx::open_vdz(p).unwrap();
        crate::swx::all_streams(&mut c).into_iter().map(|(p, b)| (p.replace('\\', "/"), b)).collect()
    }

    /// Las pantallas de los cuatro ejemplos son las del XML que exporta Vijeo: grupo, ruta, nombre e id.
    #[test]
    fn matches_the_exported_xml() {
        for (name, xml) in [
            ("Compact_Modbus SL_M221_Book", "Compact_Modbus SL_M221 Book"),
            ("Compact_Hardwired_M221", "Compact_Hardwired_M221"),
            ("MaterialWork_Drill_HW_M221", "Material Working_Drill machine_H"),
            ("Packaging_TempCtrl_HW_M221", "Packaging_TempCtrl_HW_M221"),
        ] {
            let vdz = std::path::Path::new(EXAMPLES).join(format!("{name}.vdz"));
            let Ok(text) = std::fs::read(std::path::Path::new(CORPUS).join(format!("{xml}.vxml"))) else { return };
            if !vdz.is_file() {
                return;
            }
            let text = crate::reports::decode_text(&text);
            let doc = crate::vxml::parse(&text).unwrap();
            let mut expected: Vec<(String, String, String, String, String)> = crate::vxml::targets(&doc)
                .into_iter()
                .flat_map(crate::vxml::panel_ids)
                .map(|p| (p.group, p.path, p.name, p.id, p.window))
                .collect();
            let mut got: Vec<(String, String, String, String, String)> = screens(&streams_of(&vdz))
                .into_iter()
                .map(|s| (s.group.to_owned(), s.path(), s.name.clone(), format!("P{}", s.id), s.window.map(|w| w.0).unwrap_or_default()))
                .collect();
            expected.sort();
            got.sort();
            assert_eq!(got, expected, "{name}");
            assert!(got.len() > 5);
        }
    }

    /// Carpetas (y una carpeta dentro de otra) hechas con Vijeo en una copia del ejemplo Modbus (`live_probe_panel_folders`).
    #[test]
    fn reads_panel_folders() {
        let p = std::path::Path::new(CORPUS).join("../McpCarpetas.vdz");
        if !p.is_file() {
            return;
        }
        let all = screens(&streams_of(&p));
        let inside: Vec<&ScreenFile> = all.iter().filter(|s| !s.folders.is_empty()).collect();
        assert_eq!(inside.len(), 1, "{all:#?}");
        assert_eq!((inside[0].path(), inside[0].label()), ("Folder1\\Panel5".to_owned(), "Folder «Folder1» › Panel «Panel5»".to_owned()));
        assert_eq!(all.iter().filter(|s| s.group == BASE).count(), 16);
    }
}
