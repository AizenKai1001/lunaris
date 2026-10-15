//! # Lunaris Script
//!
//! A small text language for graph systems (`docs/GRAPH.md`). It is a *view* of the same JSON that `define_system` takes: the text, the
//! node graph and the JSON are three ways of looking at one thing, and they convert into each other.
//!
//! ```text
//! /// Fall, and bounce with some energy loss.
//! system gravity each Transform, Velocity without Frozen order 100 {
//!     Velocity.lin[1] = Velocity.lin[1] - 9.8 * dt;
//!     if Transform.pos[1] < 0 {
//!         Transform.pos[1] = 0;
//!         Velocity.lin[1] = -Velocity.lin[1] * 0.8;
//!     }
//! }
//! ```
//!
//! * [`compile`] turns text into one JSON document per system, with every problem reported by line and column.
//! * [`decompile`] turns a system's JSON into text; what the language has no words for is kept as `json(...)`.
//! * [`highlight`] colours text, even when it is half typed.
//! * [`Compiled::locate`] maps an error the engine reports at a JSON path (`do[2].to.add[1]`) back to a place in the text.
//!
//! The crate depends only on `lunaris-json`; the engine checks the meaning (unknown components, wrong types) when the system is defined.
mod lex;
mod parse;
mod print;

pub use lex::{highlight, is_reserved, Class, HlSpan, LineIndex, CONTROL_WORDS, KEYWORDS};
pub use parse::{call_names, is_call_name, is_ident, SpanMap, BINARY, NARY, SINGLE, TERNARY};
pub use print::{float_text, print_system};

use lunaris_json::Json;
use std::ops::Range;

/// A problem in a script: where it is (byte range and 1-based line/column) and what is wrong.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub message: String,
    pub hint: Option<String>,
}

impl Diagnostic {
    /// `line:col message`, the way a compiler prints it.
    pub fn to_text(&self) -> String {
        match &self.hint {
            Some(h) => format!("{}:{}: {} ({h})", self.line, self.col, self.message),
            None => format!("{}:{}: {}", self.line, self.col, self.message),
        }
    }
}

/// One system read from a script.
#[derive(Clone, Debug)]
pub struct Compiled {
    pub name: String,
    /// The document `define_system` takes.
    pub def: Json,
    pub spans: SpanMap,
    /// The whole system in the text.
    pub range: Range<usize>,
}

impl Compiled {
    /// The text an engine error at `path` (for instance `do[1].to.sub[0]`) belongs to.
    pub fn locate(&self, path: &str) -> Option<Range<usize>> {
        self.spans.locate(path)
    }
}

/// Make a [`Diagnostic`] for a byte range of `src`.
pub fn diagnostic_at(src: &str, range: Range<usize>, message: impl Into<String>, hint: Option<String>) -> Diagnostic {
    let idx = LineIndex::new(src);
    let (start, end) = (range.start.min(src.len()), range.end.min(src.len()).max(range.start.min(src.len())));
    let (line, col) = idx.line_col(src, start);
    let (end_line, end_col) = idx.line_col(src, end);
    Diagnostic { start, end, line, col, end_line, end_col, message: message.into(), hint }
}

/// Read every system of a script. Either all of them, or every problem found (never both).
pub fn compile(src: &str) -> Result<Vec<Compiled>, Vec<Diagnostic>> {
    let (systems, diags) = parse::parse(src);
    if !diags.is_empty() {
        let idx = LineIndex::new(src);
        return Err(diags
            .into_iter()
            .map(|d| {
                let (line, col) = idx.line_col(src, d.start);
                let (end_line, end_col) = idx.line_col(src, d.end);
                Diagnostic { start: d.start, end: d.end, line, col, end_line, end_col, message: d.message, hint: d.hint }
            })
            .collect());
    }
    if systems.is_empty() {
        return Err(vec![diagnostic_at(src, 0..src.len().min(1), "this file defines no system", Some("start with: system name each Transform { ... }".to_string()))]);
    }
    Ok(systems.into_iter().map(|p| Compiled { name: p.name, def: p.def, spans: p.spans, range: p.range }).collect())
}

/// The problems of a script (none when it compiles).
pub fn check(src: &str) -> Vec<Diagnostic> {
    compile(src).err().unwrap_or_default()
}

/// The script text of one system's JSON.
pub fn decompile(def: &Json) -> Result<String, String> {
    print_system(def)
}

/// The text of several systems, one after another with a blank line between.
pub fn decompile_all(defs: &[Json]) -> Result<String, String> {
    let mut parts = Vec::new();
    for d in defs {
        parts.push(print_system(d)?);
    }
    Ok(parts.join("\n"))
}

#[cfg(test)]
mod tests;
