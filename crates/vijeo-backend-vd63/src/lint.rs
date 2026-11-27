//! Revisión estática de scripts de Vijeo (Java, ayuda de scripts §18.3-18.4), sin compilar.
//!
//! Lo que comprueba es lo que la ayuda da como errores habituales o lo que el validador de Vijeo rechaza:
//! - sintaxis básica: comillas y comentarios sin cerrar, paréntesis, corchetes y llaves desparejados;
//! - llamadas `Var.metodo()` sobre variables que no existen en el target (o con otras mayúsculas: los scripts
//!   distinguen mayúsculas), con la variable más parecida como sugerencia;
//! - métodos que no son de las variables de Vijeo, `X`/`XW` fuera de los enteros, métodos de `Sys` desconocidos;
//! - una variable de Vijeo usada directamente en una expresión (sin `getIntValue()`…) o como nombre de una
//!   variable local;
//! - `float x = 0.5;` sin `(float)` y decimales escritos como `.5`.
//!
//! Los objetos del proyecto que no son variables (equipos, grupos de alarmas, ventanas emergentes…) se
//! reconocen por su nombre y no se revisan.

use std::collections::{BTreeMap, BTreeSet};

use vijeo_core::{MessageSeverity, ScriptIssue};

/// Métodos de las variables (ayuda §18.9.1.6).
pub const VARIABLE_METHODS: [&str; 20] = [
    "ack",
    "add",
    "enableScaling",
    "flushToStorage",
    "getIntValue",
    "getIntValues",
    "getFloatValue",
    "getFloatValues",
    "getMax",
    "getMin",
    "getSize",
    "getStringValue",
    "isDataQualityGood",
    "sendData",
    "setLabel",
    "setMinMaxValues",
    "toString",
    "write",
    "X",
    "XW",
];

/// Métodos de `Sys` (ayuda §18.9.1, System Methods).
const SYS_METHODS: [&str; 19] = [
    "beep",
    "clearAllDataLoggingInMemory",
    "clearBiometricSwitch",
    "createProcess",
    "dataLoggingFlushAll",
    "exitRuntime",
    "flushAllDataLogging",
    "getInfoInt",
    "getInfoString",
    "getLastFieldRequested",
    "getLastFieldUpdated",
    "isPlatform",
    "login",
    "logout",
    "removeProcess",
    "setDate",
    "setTargetIPAddress",
    "setTime",
    "validateUser",
];

const INTEGER_TYPES: [&str; 5] = ["INT", "UINT", "DINT", "UDINT", "Integer"];

/// Clases de Java y objetos globales del runtime que no son variables.
const GLOBALS: [&str; 18] = [
    "Math",
    "Sys",
    "String",
    "StringBuffer",
    "Integer",
    "Float",
    "Double",
    "Long",
    "Short",
    "Byte",
    "Boolean",
    "Character",
    "Object",
    "DiagBuffer",
    "Sound",
    "TextFile",
    "System",
    "Thread",
];

const PRIMITIVES: [&str; 8] = ["int", "long", "short", "byte", "float", "double", "boolean", "char"];

const KEYWORDS: [&str; 30] = [
    "if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "return", "new", "null", "true", "false", "this", "try", "catch",
    "finally", "throw", "instanceof", "final", "static", "void", "class", "public", "private", "import", "package", "integer",
];

/// Lo que se sabe del target para revisar sus scripts.
#[derive(Debug, Default, Clone)]
pub struct Context {
    /// Nombre completo (`Carpeta.Var`, `Estructura.Miembro`) → tipo.
    vars: BTreeMap<String, String>,
    /// En minúsculas → nombre real.
    lower: BTreeMap<String, String>,
    /// Carpetas de variables, en minúsculas.
    folders: BTreeSet<String>,
    /// Otros nombres del target (paneles, objetos, equipos, grupos de alarmas…), en minúsculas.
    objects: BTreeSet<String>,
}

impl Context {
    pub fn add_variable(&mut self, name: &str, data_type: &str) {
        let parts: Vec<&str> = name.split('.').collect();
        for k in 1..parts.len() {
            self.folders.insert(parts[..k].join(".").to_lowercase());
        }
        self.lower.insert(name.to_lowercase(), name.to_owned());
        self.vars.insert(name.to_owned(), data_type.to_owned());
    }

    pub fn add_object(&mut self, name: &str) {
        self.objects.insert(name.to_lowercase());
    }

    /// La variable con el nombre más parecido (distancia de edición pequeña), para sugerirla.
    fn closest(&self, name: &str) -> Option<&str> {
        let want = name.to_lowercase();
        let limit = (want.chars().count() / 3).clamp(1, 3);
        self.lower.iter().map(|(l, real)| (edit_distance(&want, l), real)).filter(|(d, _)| *d <= limit).min_by_key(|(d, _)| *d).map(|(_, r)| r.as_str())
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > 3 {
        return usize::MAX;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ident,
    Number,
    Str,
    Punct(char),
}

#[derive(Debug, Clone)]
struct Tok {
    kind: Kind,
    text: String,
    line: u32,
    col: u32,
}

impl Tok {
    fn is(&self, c: char) -> bool {
        self.kind == Kind::Punct(c)
    }
    fn ident(&self) -> Option<&str> {
        (self.kind == Kind::Ident).then_some(self.text.as_str())
    }
}

struct Out(Vec<ScriptIssue>);

impl Out {
    fn push(&mut self, severity: MessageSeverity, line: u32, col: u32, message: impl Into<String>) {
        let i = ScriptIssue { severity, line, column: col, message: message.into() };
        if !self.0.contains(&i) {
            self.0.push(i);
        }
    }
    fn error(&mut self, t: &Tok, m: impl Into<String>) {
        self.push(MessageSeverity::Error, t.line, t.col, m);
    }
    fn warning(&mut self, t: &Tok, m: impl Into<String>) {
        self.push(MessageSeverity::Warning, t.line, t.col, m);
    }
}

/// Tokens del script, sin comentarios. Los errores léxicos (comillas, comentario sin cerrar) van a `out`.
fn lex(src: &str, out: &mut Out) -> Vec<Tok> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let (mut i, mut line, mut col) = (0usize, 1u32, 1u32);
    let advance = |i: &mut usize, line: &mut u32, col: &mut u32, c: char| {
        *i += 1;
        if c == '\n' {
            *line += 1;
            *col = 1;
        } else {
            *col += 1;
        }
    };
    while i < chars.len() {
        let c = chars[i];
        let (l0, c0) = (line, col);
        let next = chars.get(i + 1).copied();
        if c.is_whitespace() {
            advance(&mut i, &mut line, &mut col, c);
        } else if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                let d = chars[i];
                advance(&mut i, &mut line, &mut col, d);
            }
        } else if c == '/' && next == Some('*') {
            advance(&mut i, &mut line, &mut col, c);
            advance(&mut i, &mut line, &mut col, '*');
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    advance(&mut i, &mut line, &mut col, '*');
                    advance(&mut i, &mut line, &mut col, '/');
                    closed = true;
                    break;
                }
                let d = chars[i];
                advance(&mut i, &mut line, &mut col, d);
            }
            if !closed {
                out.push(MessageSeverity::Error, l0, c0, "comentario /* sin cerrar");
            }
        } else if c == '"' || c == '\'' {
            advance(&mut i, &mut line, &mut col, c);
            let mut text = String::new();
            let mut closed = false;
            while i < chars.len() && chars[i] != '\n' {
                let d = chars[i];
                advance(&mut i, &mut line, &mut col, d);
                if d == '\\' {
                    if let Some(&e) = chars.get(i) {
                        text.push(e);
                        advance(&mut i, &mut line, &mut col, e);
                    }
                } else if d == c {
                    closed = true;
                    break;
                } else {
                    text.push(d);
                }
            }
            if !closed {
                out.push(MessageSeverity::Error, l0, c0, if c == '"' { "cadena sin cerrar (falta la comilla doble final en la misma línea)" } else { "carácter sin cerrar (falta la comilla simple)" });
            }
            toks.push(Tok { kind: Kind::Str, text, line: l0, col: c0 });
        } else if c.is_ascii_digit() || (c == '.' && next.is_some_and(|n| n.is_ascii_digit())) {
            let mut text = String::new();
            while i < chars.len() {
                let d = chars[i];
                let exp_sign = (d == '+' || d == '-') && text.ends_with(['e', 'E']) && !text.starts_with("0x");
                if d.is_ascii_alphanumeric() || d == '.' || d == '_' || exp_sign {
                    text.push(d);
                    advance(&mut i, &mut line, &mut col, d);
                } else {
                    break;
                }
            }
            toks.push(Tok { kind: Kind::Number, text, line: l0, col: c0 });
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            let mut text = String::new();
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                let d = chars[i];
                text.push(d);
                advance(&mut i, &mut line, &mut col, d);
            }
            toks.push(Tok { kind: Kind::Ident, text, line: l0, col: c0 });
        } else {
            toks.push(Tok { kind: Kind::Punct(c), text: c.to_string(), line: l0, col: c0 });
            advance(&mut i, &mut line, &mut col, c);
        }
    }
    toks
}

fn check_brackets(toks: &[Tok], out: &mut Out) {
    let mut stack: Vec<&Tok> = Vec::new();
    for t in toks {
        let Kind::Punct(c) = t.kind else { continue };
        match c {
            '(' | '[' | '{' => stack.push(t),
            ')' | ']' | '}' => {
                let open = match c {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                match stack.pop() {
                    Some(o) if o.is(open) => {}
                    Some(o) => {
                        out.error(t, format!("«{c}» no cierra el «{}» de la línea {}", o.text, o.line));
                        return;
                    }
                    None => {
                        out.error(t, format!("«{c}» sin su «{open}»"));
                        return;
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(o) = stack.last() {
        out.error(o, format!("«{}» sin cerrar", o.text));
    }
}

/// Un nombre con puntos (`Carpeta.Var[3].metodo`): sus partes, sin índices, y el token siguiente.
struct Chain<'a> {
    parts: Vec<&'a Tok>,
    next: Option<&'a Tok>,
    end: usize,
}

/// Salta un `[ … ]` que empieza en `i` y devuelve el índice siguiente.
fn skip_index(toks: &[Tok], mut i: usize) -> usize {
    let mut depth = 0;
    while i < toks.len() {
        if toks[i].is('[') {
            depth += 1;
        } else if toks[i].is(']') {
            depth -= 1;
            if depth == 0 {
                return i + 1;
            }
        }
        i += 1;
    }
    i
}

fn chain_at(toks: &[Tok], start: usize) -> Chain<'_> {
    let mut parts = vec![&toks[start]];
    let mut i = start + 1;
    loop {
        if toks.get(i).is_some_and(|t| t.is('[')) {
            i = skip_index(toks, i);
        }
        match (toks.get(i), toks.get(i + 1)) {
            (Some(d), Some(n)) if d.is('.') && n.kind == Kind::Ident => {
                parts.push(n);
                i += 2;
            }
            _ => break,
        }
    }
    Chain { parts, next: toks.get(i), end: i }
}

fn join(parts: &[&Tok]) -> String {
    parts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join(".")
}

/// Variables locales declaradas (`int a, b = 0;`, `String s;`, `for (int i = 0; …)`).
fn locals(toks: &[Tok], ctx: &Context, out: &mut Out) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let is_type = |t: &Tok| t.ident().is_some_and(|s| PRIMITIVES.contains(&s) || matches!(s, "String" | "StringBuffer" | "Object" | "integer"));
    let declare = |t: &Tok, found: &mut BTreeSet<String>, out: &mut Out| {
        if ctx.vars.contains_key(&t.text) {
            out.error(t, format!("una variable local no puede llamarse como la variable de Vijeo «{}»", t.text));
        }
        found.insert(t.text.clone());
    };
    let mut i = 0;
    while i + 1 < toks.len() {
        // `int[] a`, `char f[]`.
        let mut j = i + 1;
        if is_type(&toks[i]) {
            while toks.get(j).is_some_and(|t| t.is('[')) && toks.get(j + 1).is_some_and(|t| t.is(']')) {
                j += 2;
            }
        }
        let named = toks.get(j).is_some_and(|t| t.kind == Kind::Ident && !KEYWORDS.contains(&t.text.as_str()));
        let after_ok = toks.get(j + 1).is_some_and(|t| t.is('=') || t.is(';') || t.is(',') || t.is('[') || t.is(')'));
        if is_type(&toks[i]) && named && after_ok {
            declare(&toks[j], &mut found, out);
            // Más nombres separados por comas hasta el `;`, fuera de paréntesis.
            let mut depth = 0i32;
            let mut k = j + 1;
            while k < toks.len() && !(depth == 0 && toks[k].is(';')) {
                let t = &toks[k];
                if t.is('(') || t.is('[') || t.is('{') {
                    depth += 1;
                } else if t.is(')') || t.is(']') || t.is('}') {
                    depth -= 1;
                    if depth < 0 {
                        break;
                    }
                } else if depth == 0 && t.is(',') && toks.get(k + 1).is_some_and(|n| n.kind == Kind::Ident) {
                    declare(&toks[k + 1], &mut found, out);
                }
                k += 1;
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    found
}

/// `float x = 0.5;`: en los scripts los decimales son double y hace falta `(float)`.
fn check_float_literals(toks: &[Tok], out: &mut Out) {
    for w in toks.windows(4) {
        if w[0].ident() == Some("float") && w[1].kind == Kind::Ident && w[2].is('=') && w[3].kind == Kind::Number {
            let n = w[3].text.to_ascii_lowercase();
            if (n.contains('.') || n.contains('e')) && !n.ends_with('f') {
                out.error(&w[3], format!("{} es double: escribe (float){} o {}f", w[3].text, w[3].text, w[3].text));
            }
        }
    }
    for t in toks {
        if t.kind == Kind::Number && t.text.starts_with('.') {
            out.warning(t, format!("escribe 0{} (Vijeo no acepta decimales que empiezan por punto)", t.text));
        }
    }
}

/// Clases y objetos del runtime que el compilador del MCP (`script.rs`) todavía no conoce: si un script los usa, sus
/// errores no son fiables («Undeclared…» y errores en cascada, incluso «Syntax error» con `new TextFile(…)`).
const NOT_COMPILABLE: [&str; 13] = ["Integer", "Float", "Double", "Long", "Short", "Byte", "Boolean", "Character", "Object", "DiagBuffer", "Sound", "TextFile", "System"];

/// Resultado del compilador de Vijeo para un script, para `script_lint`.
pub struct CompilerCheck {
    /// `Some(true)` compila; `Some(false)` no compila; `None` no se pudo comprobar.
    pub compiles: Option<bool>,
    /// Por qué no se comprobó, si es el caso.
    pub note: Option<String>,
    /// Errores y warnings del compilador que la revisión propia no da ya en esa línea.
    pub issues: Vec<ScriptIssue>,
}

/// Junta el resultado del compilador (`compiled`) con la revisión propia (`own`) de un script.
///
/// - Si el script usa nombres que el compilador del MCP no conoce (objetos del proyecto que no son variables, campos de
///   `Sys`, clases como `Integer` o `TextFile`), el resultado no es fiable: no se da ningún error suyo.
/// - Si no, de cada línea se da el primer error del compilador (los siguientes suelen ser cascada: «Foo», «Foo.write»,
///   «write»…), salvo que la revisión propia ya tenga un error en esa línea, y todos sus warnings.
pub fn merge_compiler(own: &[ScriptIssue], compiled: &crate::script::Compiled, ctx: &Context) -> CompilerCheck {
    let unknown: BTreeSet<String> = compiled.errors.iter().filter(|m| m.code == 8020 || m.code == 8016).filter_map(|m| quoted(&m.message)).filter(|n| !compilable(n, ctx)).collect();
    if !unknown.is_empty() {
        let names: Vec<&str> = unknown.iter().map(String::as_str).collect();
        return CompilerCheck {
            compiles: None,
            note: Some(format!(
                "no se compiló: usa {}, que el compilador del MCP todavía no conoce (objetos del proyecto que no son variables, campos de Sys, clases como Integer o TextFile); revisado sólo con las reglas propias",
                names.join(", ")
            )),
            issues: Vec::new(),
        };
    }
    let own_error_lines: BTreeSet<u32> = own.iter().filter(|i| i.severity == MessageSeverity::Error).map(|i| i.line).collect();
    let mut issues = Vec::new();
    let mut lines = BTreeSet::new();
    let mut errors: Vec<&crate::script::Message> = compiled.errors.iter().collect();
    errors.sort_by_key(|m| (m.line, m.column));
    for m in errors {
        if own_error_lines.contains(&m.line) || !lines.insert(m.line) {
            continue;
        }
        issues.push(ScriptIssue { severity: MessageSeverity::Error, line: m.line, column: m.column, message: format!("{} (compilador de Vijeo, {})", m.message, m.code) });
    }
    for m in &compiled.warnings {
        let i = ScriptIssue { severity: MessageSeverity::Warning, line: m.line, column: m.column, message: format!("{} (compilador de Vijeo, {})", m.message, m.code) };
        if !issues.contains(&i) {
            issues.push(i);
        }
    }
    let compiles = compiled.compiled.is_some() && compiled.errors.is_empty();
    CompilerCheck { compiles: Some(compiles), note: None, issues }
}

/// Lo que va entre comillas en un mensaje del compilador (`Undeclared method "write"`).
fn quoted(msg: &str) -> Option<String> {
    let a = msg.find('"')?;
    let b = msg[a + 1..].find('"')?;
    Some(msg[a + 1..a + 1 + b].to_owned())
}

/// Si un nombre que el compilador no encontró es un error de verdad: no lo es si es (o empieza por) un objeto del
/// proyecto que no es variable, una clase que el compilador del MCP no conoce o un campo de `Sys` (`Sys.INFO_…`).
fn compilable(name: &str, ctx: &Context) -> bool {
    let root = name.split('.').next().unwrap_or(name);
    if NOT_COMPILABLE.contains(&root) {
        return false;
    }
    if root == "Sys" {
        let rest = &name[root.len()..];
        return rest.is_empty() || SYS_METHODS.contains(&rest.trim_start_matches('.'));
    }
    let lower = root.to_lowercase();
    let is_var = ctx.lower.contains_key(&lower) || ctx.folders.contains(&lower);
    is_var || !ctx.objects.contains(&lower)
}

/// Revisa un script. Las posiciones son del propio texto (línea y columna desde 1).
pub fn lint(src: &str, ctx: &Context) -> Vec<ScriptIssue> {
    let mut out = Out(Vec::new());
    let toks = lex(src, &mut out);
    check_brackets(&toks, &mut out);
    check_float_literals(&toks, &mut out);
    let locals = locals(&toks, ctx, &mut out);
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        let starts_chain = t.kind == Kind::Ident && !(i > 0 && toks[i - 1].is('.')) && !KEYWORDS.contains(&t.text.as_str()) && !PRIMITIVES.contains(&t.text.as_str());
        if !starts_chain {
            i += 1;
            continue;
        }
        let c = chain_at(&toks, i);
        i = c.end.max(i + 1);
        let first = c.parts[0].text.as_str();
        // Declaración (`String s`) o variable local: no es una variable de Vijeo.
        if locals.contains(first) || c.next.is_some_and(|n| n.kind == Kind::Ident) {
            continue;
        }
        let call = c.next.is_some_and(|n| n.is('('));
        if call && c.parts.len() == 1 {
            continue; // función suelta o constructor
        }
        let (receiver, method) = if call { (&c.parts[..c.parts.len() - 1], Some(*c.parts.last().unwrap())) } else { (&c.parts[..], None) };
        let name = join(receiver);
        if GLOBALS.contains(&first) {
            if first == "Sys" && receiver.len() == 1 {
                if let Some(m) = method.filter(|m| !SYS_METHODS.contains(&m.text.as_str())) {
                    out.warning(m, format!("Sys.{} no es un método de Sys de la ayuda de Vijeo", m.text));
                }
            }
            continue;
        }
        if let Some(ty) = ctx.vars.get(&name) {
            match method {
                Some(m) => check_method(m, &name, ty, &mut out),
                None => out.error(c.parts[0], format!("la variable de Vijeo «{name}» no se puede usar directamente en una expresión: usa {name}.getIntValue(), getFloatValue() o getStringValue()")),
            }
            continue;
        }
        if let Some(real) = ctx.lower.get(&name.to_lowercase()) {
            out.error(c.parts[0], format!("«{name}»: los scripts distinguen mayúsculas, la variable se llama «{real}»"));
            continue;
        }
        // Un prefijo que es variable: `Var.metodo.algo` o miembro que no existe de una estructura.
        if let Some(k) = (1..receiver.len()).rev().find(|&k| ctx.vars.contains_key(&join(&receiver[..k]))) {
            let var = join(&receiver[..k]);
            out.error(receiver[k], format!("«{}» no es un miembro de la variable «{var}»", receiver[k].text));
            continue;
        }
        let looks_like_variable = method.is_some_and(|m| VARIABLE_METHODS.iter().any(|v| v.eq_ignore_ascii_case(&m.text)));
        let in_folder = receiver.len() > 1 && ctx.folders.contains(&join(&receiver[..receiver.len() - 1]).to_lowercase());
        let known_object = ctx.objects.contains(&name.to_lowercase()) || ctx.objects.contains(&first.to_lowercase());
        if known_object && !in_folder {
            continue;
        }
        if looks_like_variable || in_folder {
            let hint = ctx.closest(&name).map(|s| format!(" ¿Quisiste decir «{s}»?")).unwrap_or_default();
            out.error(c.parts[0], format!("«{name}» no es una variable del target ni una variable local del script.{hint}"));
        }
    }
    out.0.sort_by_key(|i| (i.line, i.column));
    out.0
}

fn check_method(m: &Tok, var: &str, ty: &str, out: &mut Out) {
    let name = m.text.as_str();
    if !VARIABLE_METHODS.contains(&name) {
        match VARIABLE_METHODS.iter().find(|v| v.eq_ignore_ascii_case(name)) {
            Some(real) => out.error(m, format!("«{name}»: los scripts distinguen mayúsculas, el método es «{real}»")),
            None => out.error(m, format!("«{name}» no es un método de las variables de Vijeo ({})", VARIABLE_METHODS.join(", "))),
        }
        return;
    }
    let block = ty.to_ascii_lowercase().starts_with("block");
    if matches!(name, "X" | "XW") && !INTEGER_TYPES.contains(&ty) && !block {
        out.warning(m, format!("{name}() es para bits de variables enteras y «{var}» es {ty}"));
    }
    if matches!(name, "getIntValues" | "getFloatValues" | "getSize" | "sendData") && !block && !ty.is_empty() {
        out.warning(m, format!("{name}() es de las variables de bloque y «{var}» es {ty}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context {
        let mut c = Context::default();
        for (n, t) in [("HMI_Local_SET", "INT"), ("Ctl.Sec", "INT"), ("Ctl.Run", "BOOL"), ("Temp", "REAL"), ("Auth", "Authorization"), ("Auth.UserName", "STRING")] {
            c.add_variable(n, t);
        }
        c.add_object("ModbusEquipment01");
        c.add_object("PopupWindow1");
        c
    }

    fn msgs(src: &str) -> Vec<String> {
        lint(src, &ctx()).into_iter().map(|i| format!("{}:{} {}", i.line, i.column, i.message)).collect()
    }

    #[test]
    fn clean_script_has_no_issues() {
        let src = r#"//-------------------------------------
// Description: "comillas" en comentario
int x = 0, y;
float f = (float)0.5;
String s = "Ctl.Bad.write(1)";
x = Ctl.Sec.getIntValue();
if (x <= 10) { HMI_Local_SET.write(x + 1); }
for (int i = 0; i < 3; i++) { y = i; }
Temp.write(Math.sin(90));
ModbusEquipment01.openConfig();
PopupWindow1.open();
Sys.setTime(Ctl.Sec.getIntValue());
Auth.UserName.getStringValue();
HMI_Local_SET.XW(3, true);
"#;
        assert_eq!(msgs(src), Vec::<String>::new());
    }

    #[test]
    fn finds_unknown_and_miscased_variables() {
        let m = msgs("HMI_Local_SEt.write(1);\nCtl.Secs.getIntValue();\nCtl.Run.getintvalue();\nMotor.write(1);");
        assert_eq!(m.len(), 4, "{m:?}");
        assert!(m[0].starts_with("1:1 ") && m[0].contains("se llama «HMI_Local_SET»"), "{}", m[0]);
        assert!(m[1].starts_with("2:1 ") && m[1].contains("¿Quisiste decir «Ctl.Sec»?"), "{}", m[1]);
        assert!(m[2].starts_with("3:9 ") && m[2].contains("el método es «getIntValue»"), "{}", m[2]);
        assert!(m[3].contains("«Motor» no es una variable"), "{}", m[3]);
    }

    #[test]
    fn finds_direct_use_and_bad_members() {
        let m = msgs("int a = 0;\nif (HMI_Local_SET == 1) { a = 1; }\nAuth.Password.getStringValue();\nTemp.X(1);");
        assert_eq!(m.len(), 3, "{m:?}");
        assert!(m[0].starts_with("2:5 ") && m[0].contains("no se puede usar directamente"));
        assert!(m[1].starts_with("3:6 ") && m[1].contains("no es un miembro de la variable «Auth»"));
        assert!(m[2].starts_with("4:6 ") && m[2].contains("bits de variables enteras"));
    }

    #[test]
    fn finds_syntax_problems() {
        assert!(msgs("if (a { b(); }")[0].contains("«(» sin cerrar"));
        assert!(msgs("x = (a]; ")[0].contains("no cierra"));
        assert!(msgs("String s = \"abc;\nint b;")[0].contains("cadena sin cerrar"));
        assert!(msgs("/* hola")[0].contains("comentario /* sin cerrar"));
        assert!(msgs("int a = 0; }")[0].contains("sin su «{»"));
        let m = msgs("float f = 0.5;\ndouble d = .25;");
        assert!(m[0].contains("(float)0.5"), "{m:?}");
        assert!(m[1].contains("escribe 0.25"), "{m:?}");
        assert!(msgs("Sys.reboot();")[0].contains("no es un método de Sys"));
        assert!(msgs("int Temp = 0;")[0].contains("no puede llamarse como la variable"));
    }

    #[test]
    fn array_indices_are_skipped() {
        let mut c = ctx();
        c.add_variable("Arr", "INT");
        assert!(lint("int k = 2;\nArr[k].write(Arr[0].getIntValue());", &c).is_empty());
    }
}
