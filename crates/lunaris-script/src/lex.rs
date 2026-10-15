//! The tokenizer of Lunaris Script, and the syntax highlighter built on it. It never fails: a character it does not know becomes an
//! error token and a diagnostic, so an editor can colour text that is half typed.
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokKind {
    Ident,
    Int,
    Float,
    Str,
    /// `$name.with.dots` or `$"any name"`: a world resource. `text` is the name.
    Res,
    Punct,
    Comment,
    /// `/// text` before a `system`: becomes its `doc`.
    DocComment,
    Error,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokKind,
    pub start: usize,
    pub end: usize,
    /// Identifier, punctuation, decoded string, resource name, number text or comment text (after the slashes).
    pub text: String,
}

impl Token {
    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

/// A problem found while reading text, in byte offsets.
#[derive(Clone, Debug)]
pub struct Diag {
    pub start: usize,
    pub end: usize,
    pub message: String,
    pub hint: Option<String>,
}

pub fn diag(start: usize, end: usize, message: impl Into<String>) -> Diag {
    Diag { start, end: end.max(start), message: message.into(), hint: None }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

const PUNCT2: [&str; 9] = ["<=", ">=", "==", "!=", "&&", "||", "+=", "-=", "*="];
const PUNCT2B: [&str; 1] = ["/="];
const PUNCT1: &str = "+-*/%<>=!(){}[],;:.";

pub fn lex(src: &str) -> (Vec<Token>, Vec<Diag>) {
    let b = src.as_bytes();
    let mut toks = Vec::new();
    let mut diags = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' {
            i += 1;
            continue;
        }
        let start = i;
        // comments
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            let mut j = i + 2;
            while j < b.len() && b[j] != b'\n' {
                j += 1;
            }
            let doc = b.get(i + 2) == Some(&b'/') && b.get(i + 3) != Some(&b'/');
            let body_start = if doc { i + 3 } else { i + 2 };
            let text = src[body_start.min(j)..j].trim_end_matches('\r');
            let text = text.strip_prefix(' ').unwrap_or(text).to_string();
            toks.push(Token { kind: if doc { TokKind::DocComment } else { TokKind::Comment }, start, end: j, text });
            i = j;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let mut j = i + 2;
            let mut closed = false;
            while j < b.len() {
                if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                    j += 2;
                    closed = true;
                    break;
                }
                j += 1;
            }
            if !closed {
                diags.push(diag(start, b.len(), "this comment is never closed").hint_with("end it with */"));
            }
            toks.push(Token { kind: TokKind::Comment, start, end: j, text: String::new() });
            i = j;
            continue;
        }
        if is_ident_start(c) {
            let mut j = i + 1;
            while j < b.len() && is_ident_char(b[j]) {
                j += 1;
            }
            toks.push(Token { kind: TokKind::Ident, start, end: j, text: src[i..j].to_string() });
            i = j;
            continue;
        }
        if c.is_ascii_digit() {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let mut float = false;
            if b.get(j) == Some(&b'.') && b.get(j + 1).is_some_and(|d| d.is_ascii_digit()) {
                float = true;
                j += 1;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
            }
            if matches!(b.get(j), Some(b'e') | Some(b'E')) {
                let mut k = j + 1;
                if matches!(b.get(k), Some(b'+') | Some(b'-')) {
                    k += 1;
                }
                if b.get(k).is_some_and(|d| d.is_ascii_digit()) {
                    while k < b.len() && b[k].is_ascii_digit() {
                        k += 1;
                    }
                    float = true;
                    j = k;
                }
            }
            toks.push(Token { kind: if float { TokKind::Float } else { TokKind::Int }, start, end: j, text: src[i..j].to_string() });
            i = j;
            continue;
        }
        if c == b'"' {
            let (text, end, ok) = read_string(src, i);
            if !ok {
                diags.push(diag(start, end, "this text is never closed").hint_with("end it with a closing \""));
            }
            toks.push(Token { kind: TokKind::Str, start, end, text });
            i = end;
            continue;
        }
        if c == b'$' {
            if b.get(i + 1) == Some(&b'"') {
                let (text, end, ok) = read_string(src, i + 1);
                if !ok {
                    diags.push(diag(start, end, "this resource name is never closed"));
                }
                toks.push(Token { kind: TokKind::Res, start, end, text });
                i = end;
                continue;
            }
            let mut j = i + 1;
            if b.get(j).is_some_and(|d| is_ident_start(*d)) {
                loop {
                    while j < b.len() && is_ident_char(b[j]) {
                        j += 1;
                    }
                    // a name goes on after a dot with a letter, a digit or `_`: `$game.has.1` (the engine's own resources have such names)
                    if b.get(j) == Some(&b'.') && b.get(j + 1).is_some_and(|d| is_ident_char(*d)) {
                        j += 1;
                        continue;
                    }
                    break;
                }
                toks.push(Token { kind: TokKind::Res, start, end: j, text: src[i + 1..j].to_string() });
                i = j;
                continue;
            }
            diags.push(diag(start, i + 1, "a resource needs a name after the $").hint_with("for example $score or $input.space"));
            toks.push(Token { kind: TokKind::Error, start, end: i + 1, text: "$".into() });
            i += 1;
            continue;
        }
        // punctuation: two characters first
        if i + 1 < b.len() && b[i].is_ascii() && b[i + 1].is_ascii() {
            let two = &src[i..i + 2];
            if PUNCT2.contains(&two) || PUNCT2B.contains(&two) {
                toks.push(Token { kind: TokKind::Punct, start, end: i + 2, text: two.to_string() });
                i += 2;
                continue;
            }
        }
        if PUNCT1.as_bytes().contains(&c) {
            toks.push(Token { kind: TokKind::Punct, start, end: i + 1, text: (c as char).to_string() });
            i += 1;
            continue;
        }
        // anything else, one whole character
        let ch = src[i..].chars().next().unwrap_or('?');
        let end = i + ch.len_utf8();
        diags.push(diag(start, end, format!("unexpected character `{ch}`")));
        toks.push(Token { kind: TokKind::Error, start, end, text: ch.to_string() });
        i = end;
    }
    (toks, diags)
}

trait HintWith {
    fn hint_with(self, h: &str) -> Diag;
}

impl HintWith for Diag {
    fn hint_with(mut self, h: &str) -> Diag {
        self.hint = Some(h.to_string());
        self
    }
}

/// Read a string literal that starts at the quote `at`. Returns (decoded text, end offset, closed).
fn read_string(src: &str, at: usize) -> (String, usize, bool) {
    let mut out = String::new();
    let mut it = src[at + 1..].char_indices();
    while let Some((k, ch)) = it.next() {
        let pos = at + 1 + k;
        match ch {
            '"' => return (out, pos + 1, true),
            '\n' => return (out, pos, false),
            '\\' => match it.next() {
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                Some((_, 'r')) => out.push('\r'),
                Some((_, c)) => out.push(c),
                None => return (out, src.len(), false),
            },
            c => out.push(c),
        }
    }
    (out, src.len(), false)
}

// ---------------------------------------------------------------------------------------------------------------- highlighting

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// `system`, `let`, `despawn`, `true`, ...
    Keyword,
    /// `if`, `else`: they steer what runs.
    Control,
    /// A component name: `Transform`.
    Type,
    /// A field of a component, or a role: `pos`, `a`.
    Property,
    /// A local variable.
    Variable,
    /// `sin(...)`, `clamp(...)`.
    Function,
    Number,
    String,
    Comment,
    /// `$score`
    Resource,
    Operator,
    Punct,
    Error,
}

#[derive(Clone, Copy, Debug)]
pub struct HlSpan {
    pub start: usize,
    pub end: usize,
    pub class: Class,
}

pub const CONTROL_WORDS: [&str; 2] = ["if", "else"];
pub const KEYWORDS: [&str; 22] = [
    "system", "each", "without", "on_contact", "order", "disabled", "let", "despawn", "emit", "spawn", "spawn_prefab", "add", "remove", "at", "velocity", "with", "on", "triggers",
    "true", "false", "dt", "tick",
];
/// Words a variable or component cannot be named.
pub fn is_reserved(w: &str) -> bool {
    KEYWORDS.contains(&w) || CONTROL_WORDS.contains(&w) || w == "uid" || w == "contact" || w == "json"
}

pub fn highlight(src: &str) -> Vec<HlSpan> {
    let (toks, _) = lex(src);
    let mut out = Vec::with_capacity(toks.len());
    let n = toks.len();
    // significant tokens only, to look at neighbours
    let sig: Vec<usize> = (0..n).filter(|&i| !matches!(toks[i].kind, TokKind::Comment | TokKind::DocComment)).collect();
    let mut pos_of = vec![usize::MAX; n];
    for (k, &i) in sig.iter().enumerate() {
        pos_of[i] = k;
    }
    for (i, t) in toks.iter().enumerate() {
        let class = match t.kind {
            TokKind::Comment | TokKind::DocComment => Class::Comment,
            TokKind::Int | TokKind::Float => Class::Number,
            TokKind::Str => Class::String,
            TokKind::Res => Class::Resource,
            TokKind::Error => Class::Error,
            TokKind::Punct => {
                if matches!(t.text.as_str(), "(" | ")" | "{" | "}" | "[" | "]" | "," | ";" | ":" | ".") {
                    Class::Punct
                } else {
                    Class::Operator
                }
            }
            TokKind::Ident => {
                let k = pos_of[i];
                let prev = if k > 0 { Some(&toks[sig[k - 1]]) } else { None };
                let next = sig.get(k + 1).map(|&j| &toks[j]);
                let after_dot = prev.is_some_and(|p| p.kind == TokKind::Punct && p.text == ".");
                let next_is = |s: &str| next.is_some_and(|t| t.kind == TokKind::Punct && t.text == s);
                let upper = t.text.as_bytes()[0].is_ascii_uppercase();
                if after_dot {
                    if upper {
                        Class::Type
                    } else {
                        Class::Property
                    }
                } else if CONTROL_WORDS.contains(&t.text.as_str()) {
                    Class::Control
                } else if KEYWORDS.contains(&t.text.as_str()) && !(next_is("(") && matches!(t.text.as_str(), "a" | "b")) {
                    Class::Keyword
                } else if next_is("(") && !upper {
                    Class::Function
                } else if upper {
                    Class::Type
                } else if next_is(".") {
                    Class::Property
                } else {
                    Class::Variable
                }
            }
        };
        out.push(HlSpan { start: t.start, end: t.end, class });
    }
    out
}

/// Byte offsets of line starts, for turning an offset into a line and column.
#[derive(Clone, Debug)]
pub struct LineIndex {
    starts: Vec<usize>,
    len: usize,
}

impl LineIndex {
    pub fn new(src: &str) -> LineIndex {
        let mut starts = vec![0];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        LineIndex { starts, len: src.len() }
    }

    /// 1-based line and column (a column counts characters).
    pub fn line_col(&self, src: &str, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.len);
        let line = match self.starts.binary_search(&offset) {
            Ok(l) => l,
            Err(l) => l - 1,
        };
        let mut s = self.starts[line];
        let mut e = offset;
        while !src.is_char_boundary(s) {
            s += 1;
        }
        while !src.is_char_boundary(e) {
            e -= 1;
        }
        (line + 1, src[s..e.max(s)].chars().count() + 1)
    }

    /// The byte offset of a 1-based line and column (clamped to the text).
    pub fn offset(&self, src: &str, line: usize, col: usize) -> usize {
        let l = line.saturating_sub(1).min(self.starts.len() - 1);
        let s = self.starts[l];
        let end = self.starts.get(l + 1).copied().unwrap_or(self.len);
        let text = src[s..end].trim_end_matches(['\n', '\r']);
        let mut off = s;
        for (k, ch) in text.chars().enumerate() {
            if k + 1 >= col {
                break;
            }
            off += ch.len_utf8();
        }
        if col > text.chars().count() + 1 {
            off = s + text.len();
        }
        off
    }

    pub fn line_count(&self) -> usize {
        self.starts.len()
    }
}
