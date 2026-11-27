//! CNativeAction (HMIManagers.dll, `CNativeAction::Save/Load`): una acción de un botón
//! (Switch: listas «al tocar», «al soltar», «mientras se pulsa»).
//!
//! Gramática:
//! ```text
//! u32  version        7 (Load acepta 1..7; con 6+ lleva InternalID al final)
//! u32  reserved       Save escribe una variable local (0 en todo el corpus); Load lo lee y lo ignora
//! u32  kind           eNP_ACTION_TYPE
//! ...  cuerpo según kind (tabla `layout`): k × CExpression, s × BSTR
//! u32  internal_id    InternalID del XML (orden de la acción dentro del objeto, desde 1)
//! ```
//! El cuerpo lo decide `CNativeAction::Load` por el tipo (Save escribe las expresiones que existan,
//! que para un objeto bien formado son las mismas):
//! - 1 expresión (destino): bit 0..4, 100, 0x70..0x7C, 0x81..0x86, 0x88, 0x8A..0x8C, 0xEC
//! - 2 expresiones (destino, origen): palabra 10..15, 0x6E, 0x6F
//! - nada: 0x14, 0x3E, 0x50..0x57, 0xE6, 0xE7, 0xED, 0xEE, 0xFA, 0xFB
//! - 1 BSTR: 0x15, 0x1E, 0x22, 0x23, 0x28..0x2D, 0x2F..0x37, 0x3C, 0x3D, 0x46, 0x47, 0xC8..0xD1,
//!   0xDC..0xE3 (0xE0: además 1 expresión delante), 0xEB, 0xF2, 0xF3, 0xF4, 0xF6
//! - 1 expresión + BSTR: 0x1F, 0x2E, 0xF5, 0xF9
//! - 2 expresiones + BSTR: 0x20 (abrir popup: X, Y), 0xEF, 0xF0, 0xF1
//! - 3 expresiones + BSTR: 0x21, 0xF8, 0xFC
//! - 2 BSTR: 0x5A (script: texto y segunda cadena)
//! - 0xF7 (decisión): 1 expresión (condición) + 2 CActionList (u8 n + n CNativeAction): si se
//!   cumple / si no.

use super::bin::{Reader, Writer, R};
use super::expr::Expr;

pub const ACTION_VERSION: u32 = 7;

/// Tipos de acción con nombre (eNP_ACTION_TYPE; nombres del XML).
pub mod kind {
    pub const BIT_SET: u32 = 0;
    pub const BIT_RESET: u32 = 1;
    pub const BIT_MOMENTARY_ON: u32 = 2;
    pub const BIT_MOMENTARY_OFF: u32 = 3;
    pub const BIT_TOGGLE: u32 = 4;
    pub const WORD_SET: u32 = 10;
    pub const WORD_ADD: u32 = 11;
    pub const WORD_SUBTRACT: u32 = 12;
    pub const WORD_AND: u32 = 13;
    pub const WORD_OR: u32 = 14;
    pub const WORD_XOR: u32 = 15;
    pub const PREVIOUS_PANEL: u32 = 0x14;
    pub const CHANGE_PANEL: u32 = 0x15;
    pub const POPUP_OPEN_CENTERED: u32 = 0x1E;
    pub const POPUP_OPEN: u32 = 0x20;
    pub const POPUP_CLOSE: u32 = 0x22;
    pub const CONFIGURATION: u32 = 0x52;
    pub const SCRIPT: u32 = 0x5A;
    pub const STRING_OVERWRITE: u32 = 0x6E;
    pub const STRING_ADD: u32 = 0x6F;
    pub const DECISION: u32 = 0xF7;
}

/// Número de expresiones y de BSTR del cuerpo según el tipo (versión 7).
pub fn layout(k: u32) -> Option<(usize, usize)> {
    Some(match k {
        0..=4 | 100 | 0x70..=0x7C | 0x81..=0x86 | 0x88 | 0x8A..=0x8C | 0xEC => (1, 0),
        10..=15 | 0x6E | 0x6F => (2, 0),
        0x14 | 0x3E | 0x50..=0x57 | 0xE6 | 0xE7 | 0xED | 0xEE | 0xFA | 0xFB => (0, 0),
        0xE0 => (1, 1),
        0x15 | 0x1E | 0x22 | 0x23 | 0x28..=0x2D | 0x2F..=0x37 | 0x3C | 0x3D | 0x46 | 0x47 | 0xC8..=0xD1 | 0xDC..=0xE3
        | 0xEB | 0xF2 | 0xF3 | 0xF4 | 0xF6 => (0, 1),
        0x1F | 0x2E | 0xF5 | 0xF9 => (1, 1),
        0x20 | 0xEF | 0xF0 | 0xF1 => (2, 1),
        0x21 | 0xF8 | 0xFC => (3, 1),
        0x5A => (0, 2),
        _ => return None,
    })
}

/// Operación de una acción.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    BitSet(Expr),
    BitReset(Expr),
    BitToggle(Expr),
    BitMomentaryOn(Expr),
    BitMomentaryOff(Expr),
    /// `dest := value` (value: constante con `Expr::constant` o variable con `Expr::int_value`).
    WordSet { dest: Expr, value: Expr },
    WordAdd { dest: Expr, value: Expr },
    WordSubtract { dest: Expr, value: Expr },
    /// Cambiar de panel: número del panel base como texto («2» para P2).
    ChangePanel(String),
    PreviousPanel,
    /// Abrir popup en una posición (`PopupPanelPosition="AtPosition"`): expresiones X e Y y el PanelID del popup
    /// como texto («10001» para P10001).
    PopupOpen { x: Expr, y: Expr, target: String },
    /// Abrir popup centrado (`Centered`, tipo 0x1E): PanelID del popup como texto.
    PopupOpenCentered(String),
    /// Cerrar popup: PanelID como texto; «-1» (`P4294967295` en el XML) es el popup en el que está el botón.
    PopupClose(String),
    /// Script de acción (0x5A): el texto fuente (variables como `{F,2:2.<id>,Carpeta}.{T,2:2.<id>,Var}`) y el Java
    /// compilado (`end:{…}`).
    Script { source: String, compiled: String },
    /// Decisión: condición y dos listas anidadas (`<Decision>`, `<DecisionTrueList>`, `<DecisionFalseList>`).
    Decision { cond: Expr, if_true: Vec<Action>, if_false: Vec<Action> },
    /// Cualquier otro tipo: expresiones y cadenas en el orden del flujo.
    Other { kind: u32, exprs: Vec<Expr>, strings: Vec<String> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub version: u32,
    pub reserved: u32,
    pub op: Op,
    pub internal_id: u32,
}

impl Op {
    pub fn kind(&self) -> u32 {
        use kind::*;
        match self {
            Op::BitSet(_) => BIT_SET,
            Op::BitReset(_) => BIT_RESET,
            Op::BitToggle(_) => BIT_TOGGLE,
            Op::BitMomentaryOn(_) => BIT_MOMENTARY_ON,
            Op::BitMomentaryOff(_) => BIT_MOMENTARY_OFF,
            Op::WordSet { .. } => WORD_SET,
            Op::WordAdd { .. } => WORD_ADD,
            Op::WordSubtract { .. } => WORD_SUBTRACT,
            Op::ChangePanel(_) => CHANGE_PANEL,
            Op::PreviousPanel => PREVIOUS_PANEL,
            Op::PopupOpen { .. } => POPUP_OPEN,
            Op::PopupOpenCentered(_) => POPUP_OPEN_CENTERED,
            Op::PopupClose(_) => POPUP_CLOSE,
            Op::Script { .. } => SCRIPT,
            Op::Decision { .. } => DECISION,
            Op::Other { kind, .. } => *kind,
        }
    }

    /// Expresiones y cadenas del cuerpo, en orden de flujo.
    fn parts(&self) -> (Vec<&Expr>, Vec<&str>) {
        match self {
            Op::BitSet(e) | Op::BitReset(e) | Op::BitToggle(e) | Op::BitMomentaryOn(e) | Op::BitMomentaryOff(e) => {
                (vec![e], vec![])
            }
            Op::WordSet { dest, value } | Op::WordAdd { dest, value } | Op::WordSubtract { dest, value } => {
                (vec![dest, value], vec![])
            }
            Op::ChangePanel(s) | Op::PopupClose(s) | Op::PopupOpenCentered(s) => (vec![], vec![s.as_str()]),
            Op::Script { source, compiled } => (vec![], vec![source.as_str(), compiled.as_str()]),
            Op::PreviousPanel => (vec![], vec![]),
            Op::PopupOpen { x, y, target } => (vec![x, y], vec![target.as_str()]),
            Op::Other { exprs, strings, .. } => (exprs.iter().collect(), strings.iter().map(|s| s.as_str()).collect()),
            Op::Decision { cond, .. } => (vec![cond], vec![]),
        }
    }

    fn from_parts(k: u32, mut exprs: Vec<Expr>, mut strings: Vec<String>) -> Op {
        use kind::*;
        match (k, exprs.len(), strings.len()) {
            (BIT_SET, 1, 0) => Op::BitSet(exprs.remove(0)),
            (BIT_RESET, 1, 0) => Op::BitReset(exprs.remove(0)),
            (BIT_TOGGLE, 1, 0) => Op::BitToggle(exprs.remove(0)),
            (BIT_MOMENTARY_ON, 1, 0) => Op::BitMomentaryOn(exprs.remove(0)),
            (BIT_MOMENTARY_OFF, 1, 0) => Op::BitMomentaryOff(exprs.remove(0)),
            (WORD_SET | WORD_ADD | WORD_SUBTRACT, 2, 0) => {
                let value = exprs.pop().unwrap();
                let dest = exprs.pop().unwrap();
                match k {
                    WORD_SET => Op::WordSet { dest, value },
                    WORD_ADD => Op::WordAdd { dest, value },
                    _ => Op::WordSubtract { dest, value },
                }
            }
            (CHANGE_PANEL, 0, 1) => Op::ChangePanel(strings.remove(0)),
            (PREVIOUS_PANEL, 0, 0) => Op::PreviousPanel,
            (POPUP_OPEN, 2, 1) => {
                let y = exprs.pop().unwrap();
                let x = exprs.pop().unwrap();
                Op::PopupOpen { x, y, target: strings.remove(0) }
            }
            (POPUP_CLOSE, 0, 1) => Op::PopupClose(strings.remove(0)),
            (POPUP_OPEN_CENTERED, 0, 1) => Op::PopupOpenCentered(strings.remove(0)),
            (SCRIPT, 0, 2) => {
                let compiled = strings.pop().unwrap();
                Op::Script { source: strings.pop().unwrap(), compiled }
            }
            _ => Op::Other { kind: k, exprs, strings },
        }
    }
}

impl Action {
    pub fn new(op: Op, internal_id: u32) -> Action {
        Action { version: ACTION_VERSION, reserved: 0, op, internal_id }
    }

    pub fn parse(r: &mut Reader) -> R<Action> {
        let at = r.p;
        let version = r.u32()?;
        if version != ACTION_VERSION {
            return Err(format!("CNativeAction versión {version} en {at:#x} (sólo se soporta la 7)"));
        }
        let reserved = r.u32()?;
        let k = r.u32()?;
        if k == kind::DECISION {
            let cond = Expr::parse(r)?;
            let if_true = parse_nested(r)?;
            let if_false = parse_nested(r)?;
            let internal_id = r.u32()?;
            return Ok(Action { version, reserved, op: Op::Decision { cond, if_true, if_false }, internal_id });
        }
        let (ne, ns) = layout(k).ok_or_else(|| format!("CNativeAction de tipo {k:#x} no soportado en {at:#x}"))?;
        let mut exprs = Vec::with_capacity(ne);
        for _ in 0..ne {
            exprs.push(Expr::parse(r)?);
        }
        let mut strings = Vec::with_capacity(ns);
        for _ in 0..ns {
            strings.push(r.bstr()?);
        }
        let internal_id = r.u32()?;
        Ok(Action { version, reserved, op: Op::from_parts(k, exprs, strings), internal_id })
    }

    pub fn write(&self, w: &mut Writer) {
        w.u32(self.version);
        w.u32(self.reserved);
        w.u32(self.op.kind());
        let (es, ss) = self.op.parts();
        for e in es {
            e.write(w);
        }
        for s in ss {
            w.bstr(s);
        }
        if let Op::Decision { if_true, if_false, .. } = &self.op {
            write_nested(w, if_true);
            write_nested(w, if_false);
        }
        w.u32(self.internal_id);
    }
}

/// CActionList::Save (listas de una decisión): u8 n + n acciones.
fn parse_nested(r: &mut Reader) -> R<Vec<Action>> {
    let n = r.u8()?;
    (0..n).map(|_| Action::parse(r)).collect()
}

fn write_nested(w: &mut Writer, l: &[Action]) {
    w.u8(l.len() as u8);
    for a in l {
        a.write(w);
    }
}

/// Lista de acciones (`CArray<CNativeAction>`): u32 n + n acciones.
pub fn parse_list(r: &mut Reader) -> R<Vec<Action>> {
    let at = r.p;
    let n = r.u32()?;
    if n > 256 {
        return Err(format!("lista de {n} acciones en {at:#x}"));
    }
    (0..n).map(|_| Action::parse(r)).collect()
}

pub fn write_list(w: &mut Writer, l: &[Action]) {
    w.u32(l.len() as u32);
    for a in l {
        a.write(w);
    }
}
