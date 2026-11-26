//! Interpretación de la ventana de salida de Vijeo (Feedback Zone).
//!
//! Ejemplo de una compilación con errores:
//! ```text
//! -------------------------------- Build All --------------------------------
//! Target - HMISTO531_532
//! Warning 2133: [Target] [HMISTO531_532] [Allow Ethernet Download & Upload at Run Time] The security risk…
//! HMISTO531_532 - I/O
//! Error 1331: [Driver] [SoMachineNetwork01] requires Equipment Address.
//! Build Failed - 3 Error(s), 2 Warning(s)
//! ```
//!
//! Cada línea lleva delante un marcador de tipo (verificado): `\x01\x02` información, `\x01\x0D`
//! warning, `\x01\x0B` error. Así la severidad no depende del idioma de Vijeo.

use vijeo_core::{BuildMessage, FeedbackLog, MessageSeverity};

/// Palabra inicial de un error en los idiomas de Vijeo, por si una línea llega sin marcador.
const ERROR_WORDS: &[&str] = &["error", "erreur", "fehler", "errore", "erro"];

/// Quita el marcador de tipo: (severidad si la indica, texto).
fn split_marker(line: &str) -> (Option<MessageSeverity>, &str) {
    match line.as_bytes() {
        [0x01, 0x0B, ..] => (Some(MessageSeverity::Error), &line[2..]),
        [0x01, 0x0D, ..] => (Some(MessageSeverity::Warning), &line[2..]),
        [0x01, k, ..] if *k < 0x20 => (None, &line[2..]),
        _ => (None, line),
    }
}

/// `Error 1331: texto` → (severidad según la palabra, código, texto).
fn diagnostic(line: &str) -> Option<(MessageSeverity, u32, String)> {
    let (word, rest) = line.trim_start().split_once(' ')?;
    let (code, text) = rest.split_once(':')?;
    let code: u32 = code.trim().parse().ok()?;
    if word.is_empty() || word.chars().any(|c| c.is_ascii_digit() || c.is_ascii_punctuation()) {
        return None;
    }
    let severity = if ERROR_WORDS.iter().any(|w| word.eq_ignore_ascii_case(w)) { MessageSeverity::Error } else { MessageSeverity::Warning };
    Some((severity, code, text.trim().to_owned()))
}

/// Separadores (`---- Build All ----`) y líneas vacías.
fn is_noise(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with("---")
}

/// Cabecera de sección: `Target - X`, `X - HMI`, `X - I/O`.
fn is_section(line: &str) -> bool {
    let t = line.trim();
    t.contains(" - ") && !t.contains(':') && !t.contains("Error(s)") && t.len() < 120
}

/// Resumen sin errores ni warnings (`Main.ENU` de 6.3).
const BUILD_COMPLETE: &str = "Build Complete";

/// `N palabra(s)`: empieza por el número y no tiene más cifras.
fn counted(part: &str) -> Option<i32> {
    let p = part.trim();
    let digits = p.find(|c: char| !c.is_ascii_digit())?;
    let (n, rest) = p.split_at(digits);
    (!n.is_empty() && rest.starts_with(' ') && rest.trim().chars().any(char::is_alphabetic) && !rest.contains(|c: char| c.is_ascii_digit())).then(|| n.parse().ok())?
}

/// Recuento de una línea de resumen tras el último ` - `. Formatos de `Main.ENU`:
/// `… - %1 %2(s), %3 %4(s)` (errores, warnings) y `… - %1 %2(s)` (sólo warnings: no hubo errores).
/// Se mira la forma y no las palabras, que dependen del idioma; descarta `SRAM - Total: 512 KB, …`.
fn counts(line: &str) -> Option<(i32, i32)> {
    let (_, tail) = line.rsplit_once(" - ")?;
    let nums: Option<Vec<i32>> = tail.split(',').map(counted).collect();
    match nums?.as_slice() {
        [e, w] => Some((*e, *w)),
        [w] => Some((0, *w)),
        _ => None,
    }
}

/// Último resumen de la salida.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub errors: i32,
    pub warnings: i32,
    /// Es el de la compilación (`Build …`) y no uno de validación. Sólo se reconoce en inglés.
    pub build: bool,
}

pub fn summary(lines: &[String]) -> Option<Summary> {
    lines.iter().rev().find_map(|l| {
        let t = split_marker(l).1.trim();
        let build = t.starts_with("Build ");
        if t == BUILD_COMPLETE {
            return Some(Summary { errors: 0, warnings: 0, build });
        }
        counts(t).map(|(errors, warnings)| Summary { errors, warnings, build })
    })
}

/// Deja sólo los mensajes de `target`: sección `Target - <target>` o `<target> - …` (sin distinguir mayúsculas).
pub fn only_target(messages: Vec<BuildMessage>, target: &str) -> Vec<BuildMessage> {
    let t = target.to_lowercase();
    messages
        .into_iter()
        .filter(|m| m.context.as_deref().map(str::to_lowercase).is_some_and(|c| c.starts_with(&format!("{t} - ")) || c.ends_with(&format!(" - {t}"))))
        .collect()
}

pub fn parse(lines: &[String]) -> FeedbackLog {
    let mut messages = Vec::new();
    let mut output = Vec::new();
    let mut context: Option<String> = None;
    for raw in lines {
        let (marked, line) = split_marker(raw);
        if is_noise(line) {
            continue;
        }
        let line = line.trim_end();
        output.push(line.to_owned());
        if let Some((by_word, code, text)) = diagnostic(line) {
            messages.push(BuildMessage { severity: marked.unwrap_or(by_word), code: Some(code), text, context: context.clone() });
        } else if is_section(line) {
            context = Some(line.trim().to_owned());
        }
    }
    FeedbackLog { messages, output }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_build_output() {
        let raw = [
            "-------------------------------- Build All --------------------------------",
            "--------------------------------- Validating ---------------------------------",
            "Validating Services",
            "Target - HMISTO531_532",
            "Warning 2133: [Target] [HMISTO531_532] [Allow Ethernet Download & Upload at Run Time] The security risk is elevated.",
            "HMISTO531_532 - HMI",
            "Warning 2131: Enable Download Security to reduce unauthorized access to the target machine.",
            "Error 2841: [Actions][Conditional][Actions][Backlight Color] Target does not support Backlight Color control.",
            "HMISTO531_532 - I/O",
            "Error 1331: [Driver] [SoMachineNetwork01] requires Equipment Address.",
            "SRAM - Total: 512 KB, Control: 1 KB, Used: 80 KB, Available: 432 KB",
            "Validation Failed - 3 Error(s), 2 Warning(s)",
            " ",
            "Build Failed - 3 Error(s), 2 Warning(s)",
        ];
        let log = parse(&raw.map(String::from));
        assert_eq!(log.messages.len(), 4);
        let e = &log.messages[3];
        assert_eq!((e.severity, e.code), (MessageSeverity::Error, Some(1331)));
        assert_eq!(e.text, "[Driver] [SoMachineNetwork01] requires Equipment Address.");
        assert_eq!(e.context.as_deref(), Some("HMISTO531_532 - I/O"));
        assert_eq!(log.messages[0].severity, MessageSeverity::Warning);
        assert_eq!(log.messages[0].context.as_deref(), Some("Target - HMISTO531_532"));
        assert_eq!(log.messages[2].context.as_deref(), Some("HMISTO531_532 - HMI"));
        assert!(log.output.iter().all(|l| !l.starts_with("---") && !l.trim().is_empty()));
        assert_eq!(log.output.last().map(String::as_str), Some("Build Failed - 3 Error(s), 2 Warning(s)"));
    }

    #[test]
    fn type_markers() {
        let raw = [
            "\u{1}\u{2}-------------------------------- Build All --------------------------------",
            "\u{1}\u{2}HMI1 - I/O",
            "\u{1}\u{b}Fehler 1331: [Driver] [X] requires Equipment Address.",
            "\u{1}\rAviso 2131: Enable Download Security.",
            "\u{1}\u{b}Validation Failed - 1 Error(s), 1 Warning(s)",
            "\u{1}\u{2}",
        ];
        let log = parse(&raw.map(String::from));
        assert_eq!(log.messages.iter().map(|m| m.severity).collect::<Vec<_>>(), [MessageSeverity::Error, MessageSeverity::Warning]);
        assert_eq!(log.messages[0].context.as_deref(), Some("HMI1 - I/O"));
        assert_eq!(log.output, ["HMI1 - I/O", "Fehler 1331: [Driver] [X] requires Equipment Address.", "Aviso 2131: Enable Download Security.", "Validation Failed - 1 Error(s), 1 Warning(s)"]);
    }

    #[test]
    fn summary_and_target_filter() {
        let raw = [
            "Target - HMI1",
            "Warning 2133: [Target] [HMI1] x",
            "HMI1 - I/O",
            "Error 1331: [Driver] [X] requires Equipment Address.",
            "HMI2 - HMI",
            "Error 2841: y",
            "SRAM - Total: 512 KB, Used: 78 KB, Available: 434 KB",
            "Validation Failed - 2 Error(s), 1 Warning(s)",
            "\u{1}\u{b}Build Failed - 12 Error(s), 3 Warning(s)",
            "Press F4 to cycle through errors and warnings",
        ]
        .map(String::from);
        assert_eq!(summary(&raw), Some(Summary { errors: 12, warnings: 3, build: true }));
        assert_eq!(summary(&raw[..7]), None);
        assert_eq!(summary(&raw[..8]), Some(Summary { errors: 2, warnings: 1, build: false }));
        assert_eq!(counts("Échec de la génération - 0 erreur(s), 4 avertissement(s)"), Some((0, 4)));
        // Sin errores sólo se cuentan los warnings; tras el resumen va el tamaño del runtime (SET03).
        let ok = [
            "\u{1}\u{d}Validation Complete - 7 Warning(s)",
            "\u{1}\u{2}Romizer successful",
            "\u{1}\u{d}Build Complete - 7 Warning(s)",
            "\u{1}\u{2}Press F4 to cycle through errors and warnings",
            "\u{1}\u{2}Target 'HMIs' - HMI: Project 4,758 KB, System 38,639 KB, Total Size 43,397 KB (44,438,528 bytes).",
        ]
        .map(String::from);
        assert_eq!(summary(&ok), Some(Summary { errors: 0, warnings: 7, build: true }));
        assert_eq!(summary(&["\u{1}\u{2}Build Complete".to_owned()]), Some(Summary { errors: 0, warnings: 0, build: true }));
        // Un nombre de target con cifras no es un recuento.
        assert_eq!(counts("Target - HMI1"), None);
        assert_eq!(counts("Target - 2 Pantallas 3"), None);
        let log = parse(&raw);
        let hmi1 = only_target(log.messages.clone(), "hmi1");
        assert_eq!(hmi1.iter().map(|m| m.code).collect::<Vec<_>>(), [Some(2133), Some(1331)]);
        assert_eq!(only_target(log.messages, "HMI2").len(), 1);
    }

    #[test]
    fn localized_and_non_diagnostics() {
        assert_eq!(diagnostic("Advertencia 2131: algo").map(|d| d.0), Some(MessageSeverity::Warning));
        assert_eq!(diagnostic("Erreur 1331: x").map(|d| d.0), Some(MessageSeverity::Error));
        assert!(diagnostic("SRAM - Total: 512 KB").is_none());
        assert!(diagnostic("Validation Failed - 3 Error(s), 2 Warning(s)").is_none());
    }
}
