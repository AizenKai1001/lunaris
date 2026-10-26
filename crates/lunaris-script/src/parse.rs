//! The parser of Lunaris Script: text in, graph-system JSON (docs/GRAPH.md) out, with a map from JSON paths back to source text so that an
//! error the engine reports at `do[2].to.add[1]` can be underlined in the editor.
use crate::lex::{self, diag, is_reserved, Diag, TokKind, Token};
use lunaris_json::Json;
use std::collections::HashMap;
use std::ops::Range;

/// Operations whose argument is one value rather than a list: `{"neg": x}`.
pub const SINGLE: [&str; 17] = ["neg", "abs", "sqrt", "floor", "ceil", "round", "sign", "trunc", "fract", "sin", "cos", "tan", "atan", "exp", "ln", "not", "rand"];
/// Operations that take any number of arguments (one or more).
pub const NARY: [&str; 6] = ["add", "mul", "min", "max", "and", "or"];
pub const BINARY: [&str; 12] = ["sub", "div", "mod", "lt", "le", "gt", "ge", "eq", "ne", "atan2", "pow", "hypot"];
pub const TERNARY: [&str; 5] = ["select", "clamp", "lerp", "smoothstep", "noise"];

pub fn is_call_name(n: &str) -> bool {
    SINGLE.contains(&n) || NARY.contains(&n) || BINARY.contains(&n) || TERNARY.contains(&n)
}

pub fn call_names() -> Vec<&'static str> {
    SINGLE.iter().chain(NARY.iter()).chain(BINARY.iter()).chain(TERNARY.iter()).copied().collect()
}

pub fn is_ident(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && (b[0].is_ascii_alphabetic() || b[0] == b'_') && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

/// Where the parts of one system are in the text, keyed by their path in the JSON (`do[0].to`).
#[derive(Clone, Debug, Default)]
pub struct SpanMap {
    items: Vec<(String, Range<usize>)>,
}

impl SpanMap {
    fn add(&mut self, path: impl Into<String>, span: Range<usize>) {
        self.items.push((path.into(), span));
    }

    /// The text a JSON path (as the engine reports it) belongs to. A path that is not known exactly is shortened until one is.
    pub fn locate(&self, path: &str) -> Option<Range<usize>> {
        let mut p = path.to_string();
        loop {
            if let Some((_, r)) = self.items.iter().find(|(k, _)| *k == p) {
                return Some(r.clone());
            }
            if p.is_empty() {
                return None;
            }
            // drop the last `[n]` or `.name`
            let cut = if p.ends_with(']') { p.rfind('[') } else { p.rfind('.') };
            p.truncate(cut.unwrap_or(0));
        }
    }

    pub fn paths(&self) -> impl Iterator<Item = (&str, &Range<usize>)> {
        self.items.iter().map(|(k, v)| (k.as_str(), v))
    }
}

/// An expression with the places of its parts, before it knows its own path.
struct Ex {
    json: Json,
    span: Range<usize>,
    op: Option<String>,
    array: bool,
    kids: Vec<Ex>,
}

impl Ex {
    fn leaf(json: Json, span: Range<usize>) -> Ex {
        Ex { json, span, op: None, array: false, kids: vec![] }
    }

    fn op(op: &str, array: bool, kids: Vec<Ex>, span: Range<usize>) -> Ex {
        let args: Vec<Json> = kids.iter().map(|k| k.json.clone()).collect();
        let arg = if array { Json::Arr(args) } else { args.into_iter().next().unwrap_or(Json::Null) };
        Ex { json: Json::obj([(op.to_string(), arg)]), span, op: Some(op.to_string()), array, kids }
    }

    fn record(&self, path: &str, out: &mut SpanMap) {
        out.add(path, self.span.clone());
        if let Some(op) = &self.op {
            let at = format!("{path}.{op}");
            out.add(at.clone(), self.span.clone());
            for (i, k) in self.kids.iter().enumerate() {
                k.record(&if self.array { format!("{at}[{i}]") } else { at.clone() }, out);
            }
        }
    }
}

type R<T> = Result<T, ()>;

/// Reading stops after this many problems.
const MAX_PROBLEMS: usize = 100;
/// How deeply expressions and blocks may nest: text typed in an editor can be anything, and the reader calls itself for each level.
const MAX_DEPTH: usize = 64;

/// One system, as read from the text.
#[derive(Clone, Debug)]
pub struct Parsed {
    pub name: String,
    pub def: Json,
    pub spans: SpanMap,
    /// The whole `system ... { ... }` text.
    pub range: Range<usize>,
}

struct Parser<'a> {
    src: &'a str,
    toks: Vec<Token>,
    pos: usize,
    docs: HashMap<usize, String>,
    diags: Vec<Diag>,
    spans: SpanMap,
    depth: usize,
}

fn binop(t: &Token) -> Option<(u8, &'static str, bool)> {
    if t.kind != TokKind::Punct {
        return None;
    }
    Some(match t.text.as_str() {
        "||" => (1, "or", true),
        "&&" => (2, "and", true),
        "==" => (3, "eq", false),
        "!=" => (3, "ne", false),
        "<" => (4, "lt", false),
        "<=" => (4, "le", false),
        ">" => (4, "gt", false),
        ">=" => (4, "ge", false),
        "+" => (5, "add", true),
        "-" => (5, "sub", false),
        "*" => (6, "mul", true),
        "/" => (6, "div", false),
        "%" => (6, "mod", false),
        _ => return None,
    })
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    fn peek_at(&self, n: usize) -> Option<&Token> {
        self.toks.get(self.pos + n)
    }

    fn is_punct(&self, s: &str) -> bool {
        self.peek().is_some_and(|t| t.kind == TokKind::Punct && t.text == s)
    }

    fn is_word(&self, s: &str) -> bool {
        self.peek().is_some_and(|t| t.kind == TokKind::Ident && t.text == s)
    }

    fn eof_pos(&self) -> usize {
        self.src.len()
    }

    /// The range to blame when something is missing here: the token that is in the way, or the end of the text.
    fn here(&self) -> Range<usize> {
        match self.peek() {
            Some(t) => t.range(),
            None => {
                let e = self.eof_pos();
                e.saturating_sub(1)..e
            }
        }
    }

    fn prev_end(&self) -> usize {
        if self.pos == 0 {
            0
        } else {
            self.toks[self.pos - 1].end
        }
    }

    fn push_diag(&mut self, d: Diag) {
        self.diags.push(d);
        if self.diags.len() >= MAX_PROBLEMS {
            // enough: stop reading, so that no loop can run on and on over broken text
            self.pos = self.toks.len();
        }
    }

    fn fail<T>(&mut self, range: Range<usize>, message: impl Into<String>) -> R<T> {
        self.push_diag(diag(range.start, range.end, message));
        Err(())
    }

    fn fail_hint<T>(&mut self, range: Range<usize>, message: impl Into<String>, hint: impl Into<String>) -> R<T> {
        let mut d = diag(range.start, range.end, message);
        d.hint = Some(hint.into());
        self.push_diag(d);
        Err(())
    }

    fn describe(&self) -> String {
        match self.peek() {
            Some(t) => match t.kind {
                TokKind::Str => "text".to_string(),
                TokKind::Int | TokKind::Float => format!("the number {}", t.text),
                TokKind::Res => format!("${}", t.text),
                _ => format!("`{}`", &self.src[t.start..t.end]),
            },
            None => "the end of the file".to_string(),
        }
    }

    fn expect_punct(&mut self, s: &str, what: &str) -> R<Range<usize>> {
        if self.is_punct(s) {
            let r = self.toks[self.pos].range();
            self.pos += 1;
            Ok(r)
        } else {
            let at = self.here();
            let found = self.describe();
            self.fail(at, format!("expected `{s}` {what}, found {found}"))
        }
    }

    fn expect_ident(&mut self, what: &str) -> R<(String, Range<usize>)> {
        match self.peek() {
            Some(t) if t.kind == TokKind::Ident => {
                let r = (t.text.clone(), t.range());
                self.pos += 1;
                Ok(r)
            }
            _ => {
                let at = self.here();
                let found = self.describe();
                self.fail(at, format!("expected {what}, found {found}"))
            }
        }
    }

    fn expect_word(&mut self, w: &str, what: &str) -> R<Range<usize>> {
        if self.is_word(w) {
            let r = self.toks[self.pos].range();
            self.pos += 1;
            Ok(r)
        } else {
            let at = self.here();
            let found = self.describe();
            self.fail(at, format!("expected `{w}` {what}, found {found}"))
        }
    }

    // ----------------------------------------------------------------------------------------------------- expressions

    fn expr(&mut self) -> R<Ex> {
        self.binary(1)
    }

    fn binary(&mut self, min: u8) -> R<Ex> {
        let mut left = self.unary()?;
        loop {
            let Some((prec, op, nary)) = self.peek().and_then(binop) else { break };
            if prec < min {
                break;
            }
            let opt = self.toks[self.pos].text.clone();
            if nary {
                let start = left.span.start;
                let mut operands = vec![left];
                while self.peek().is_some_and(|t| t.kind == TokKind::Punct && t.text == opt) {
                    self.pos += 1;
                    operands.push(self.binary(prec + 1)?);
                }
                let end = operands.last().map(|o| o.span.end).unwrap_or(start);
                left = Ex::op(op, true, operands, start..end);
            } else {
                self.pos += 1;
                let right = self.binary(prec + 1)?;
                let span = left.span.start..right.span.end;
                left = Ex::op(op, true, vec![left, right], span);
            }
        }
        Ok(left)
    }

    fn unary(&mut self) -> R<Ex> {
        if self.depth >= MAX_DEPTH {
            let at = self.here();
            return self.fail(at, "this is nested too deeply");
        }
        self.depth += 1;
        let r = self.unary_inner();
        self.depth -= 1;
        r
    }

    fn unary_inner(&mut self) -> R<Ex> {
        if self.is_punct("-") {
            let start = self.toks[self.pos].start;
            self.pos += 1;
            if let Some(t) = self.peek() {
                if matches!(t.kind, TokKind::Int | TokKind::Float) {
                    let (text, span) = (t.text.clone(), start..t.end);
                    self.pos += 1;
                    return Ok(Ex::leaf(number(&format!("-{text}")), span));
                }
            }
            let inner = self.unary()?;
            let span = start..inner.span.end;
            return Ok(Ex::op("neg", false, vec![inner], span));
        }
        if self.is_punct("!") {
            let start = self.toks[self.pos].start;
            self.pos += 1;
            let inner = self.unary()?;
            let span = start..inner.span.end;
            return Ok(Ex::op("not", false, vec![inner], span));
        }
        self.primary()
    }

    fn primary(&mut self) -> R<Ex> {
        let Some(t) = self.peek().cloned() else {
            let at = self.here();
            return self.fail(at, "expected a value, found the end of the file");
        };
        match t.kind {
            TokKind::Int | TokKind::Float => {
                self.pos += 1;
                Ok(Ex::leaf(number(&t.text), t.range()))
            }
            TokKind::Str => {
                self.pos += 1;
                Ok(Ex::leaf(Json::str(t.text.clone()), t.range()))
            }
            TokKind::Res => {
                self.pos += 1;
                Ok(Ex::op_named("res", Json::str(t.text.clone()), t.range()))
            }
            TokKind::Punct if t.text == "(" => {
                self.pos += 1;
                let mut inner = self.expr()?;
                self.expect_punct(")", "to close the parenthesis")?;
                // the parentheses are part of the picture of the value
                inner.span = t.start..self.prev_end();
                Ok(inner)
            }
            TokKind::Ident => self.ident_expr(t),
            _ => {
                let found = self.describe();
                self.fail(t.range(), format!("expected a value, found {found}"))
            }
        }
    }

    fn ident_expr(&mut self, t: Token) -> R<Ex> {
        let name = t.text.clone();
        match name.as_str() {
            "true" | "false" => {
                self.pos += 1;
                return Ok(Ex::leaf(Json::Bool(name == "true"), t.range()));
            }
            "json" if self.peek_at(1).is_some_and(|n| n.kind == TokKind::Punct && n.text == "(") => {
                let (j, r) = self.raw_json()?;
                return Ok(Ex::leaf(j, r));
            }
            _ => {}
        }
        // a call: name(args)
        if self.peek_at(1).is_some_and(|n| n.kind == TokKind::Punct && n.text == "(") {
            return self.call(t);
        }
        // a path: Comp.field[i], a.Comp.field, or a lone name
        self.pos += 1;
        let mut text = name.clone();
        let mut segs = 1usize;
        let mut end = t.end;
        let mut indexed = false;
        loop {
            if self.is_punct(".") && self.peek_at(1).is_some_and(|n| n.kind == TokKind::Ident) {
                self.pos += 1;
                let seg = &self.toks[self.pos];
                text.push('.');
                text.push_str(&seg.text);
                end = seg.end;
                segs += 1;
                self.pos += 1;
            } else if self.is_punct("[") {
                self.pos += 1;
                let Some(n) = self.peek().filter(|n| n.kind == TokKind::Int).cloned() else {
                    let at = self.here();
                    return self.fail_hint(at, "an index must be a whole number", "write it like pos[1]");
                };
                self.pos += 1;
                self.expect_punct("]", "to close the index")?;
                text.push_str(&format!("[{}]", n.text));
                end = self.prev_end();
                indexed = true;
            } else {
                break;
            }
        }
        let span = t.start..end;
        if segs == 1 && !indexed {
            return Ok(match name.as_str() {
                "dt" => Ex::op_named("dt", Json::Bool(true), span),
                "tick" => Ex::op_named("tick", Json::Bool(true), span),
                "uid" => Ex::op_named("uid", Json::Bool(true), span),
                _ if is_reserved(&name) => return self.fail(span, format!("`{name}` is a keyword here, not a value")),
                _ => Ex::op_named("var", Json::str(name), span),
            });
        }
        if segs == 1 {
            return self.fail_hint(span, format!("`{name}` is a local variable: only component fields have an index"), "fields look like Transform.pos[1]");
        }
        if name == "contact" && segs == 2 && !indexed {
            return Ok(Ex::op_named("contact", Json::str(text.trim_start_matches("contact.")), span));
        }
        Ok(Ex::op_named("get", Json::str(text), span))
    }

    fn call(&mut self, t: Token) -> R<Ex> {
        let name = t.text.clone();
        self.pos += 2; // name (
        let mut args: Vec<Ex> = Vec::new();
        if name == "uid" {
            // uid(a) / uid(b): the uid of one of the two entities of a contact
            let (role, rr) = self.expect_ident("`a` or `b`")?;
            if role != "a" && role != "b" {
                return self.fail(rr, "`uid` takes `a` or `b` (the two entities of a contact)");
            }
            self.expect_punct(")", "to close the call")?;
            return Ok(Ex::op_named("uid", Json::str(role), t.start..self.prev_end()));
        }
        if !self.is_punct(")") {
            loop {
                args.push(self.expr()?);
                if self.is_punct(",") {
                    self.pos += 1;
                    continue;
                }
                break;
            }
        }
        self.expect_punct(")", "to close the call")?;
        let span = t.start..self.prev_end();
        if !is_call_name(&name) {
            let mut all = call_names();
            all.sort_unstable();
            return self.fail_hint(t.range(), format!("`{name}` is not a function"), format!("functions: {}", all.join(" ")));
        }
        let (min, max, array) = if SINGLE.contains(&name.as_str()) {
            (1, 1, false)
        } else if NARY.contains(&name.as_str()) {
            (1, usize::MAX, true)
        } else if BINARY.contains(&name.as_str()) {
            (2, 2, true)
        } else {
            (3, 3, true)
        };
        if args.len() < min || args.len() > max {
            let want = if min == max { format!("exactly {min}") } else { "at least one".to_string() };
            return self.fail(span, format!("`{name}` takes {want} argument{}, got {}", if min == 1 && max == 1 { "" } else { "s" }, args.len()));
        }
        Ok(Ex::op(&name, array, args, span))
    }

    /// `json(...)`: any JSON text, taken as it is (an escape hatch for what the language has no words for).
    fn raw_json(&mut self) -> R<(Json, Range<usize>)> {
        let open = self.toks[self.pos + 1].end;
        // find the matching parenthesis in the source text, skipping strings
        let b = self.src.as_bytes();
        let (mut depth, mut i, mut in_str) = (1i32, open, false);
        while i < b.len() {
            let c = b[i];
            if in_str {
                if c == b'\\' {
                    i += 1;
                } else if c == b'"' {
                    in_str = false;
                }
            } else if c == b'"' {
                in_str = true;
            } else if c == b'(' {
                depth += 1;
            } else if c == b')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            i += 1;
        }
        let start = self.toks[self.pos].start;
        if depth != 0 {
            return self.fail(start..self.eof_pos(), "this json( is never closed");
        }
        let body = &self.src[open..i];
        let parsed = lunaris_json::parse(body);
        // skip the tokens that were inside
        while self.pos < self.toks.len() && self.toks[self.pos].start < i + 1 {
            self.pos += 1;
        }
        match parsed {
            Ok(j) => Ok((j, start..i + 1)),
            Err(e) => self.fail(start..i + 1, format!("not valid JSON: {e}")),
        }
    }

    // ----------------------------------------------------------------------------------------------------- statements

    /// `{ statement* }` at a JSON path such as `do` or `do[1].then`.
    fn block(&mut self, path: &str) -> R<Vec<Json>> {
        if self.depth >= MAX_DEPTH {
            let at = self.here();
            return self.fail(at, "this is nested too deeply");
        }
        self.depth += 1;
        let r = self.block_inner(path);
        self.depth -= 1;
        r
    }

    fn block_inner(&mut self, path: &str) -> R<Vec<Json>> {
        self.expect_punct("{", "to start the block")?;
        let mut out = Vec::new();
        loop {
            if self.is_punct("}") {
                self.pos += 1;
                return Ok(out);
            }
            if self.peek().is_none() {
                let at = self.here();
                return self.fail_hint(at, "this block is never closed", "add a } at the end");
            }
            let at = format!("{path}[{}]", out.len());
            let before = self.pos;
            match self.statement(&at) {
                Ok(j) => out.push(j),
                Err(()) => {
                    self.sync();
                    // `sync` stops at a stray `)` or `]`: step over it, or this loop would try the same token for ever
                    if self.pos == before {
                        self.pos += 1;
                    }
                    // keep the numbering of the statements that did parse
                    out.push(Json::Null);
                }
            }
        }
    }

    /// After an error: skip to just after the next `;` at this nesting, or stop before the `}` that ends the block.
    fn sync(&mut self) {
        let mut depth = 0i32;
        while let Some(t) = self.peek() {
            if t.kind == TokKind::Punct {
                match t.text.as_str() {
                    "{" | "(" | "[" => depth += 1,
                    "}" | ")" | "]" => {
                        if depth == 0 {
                            return;
                        }
                        depth -= 1;
                    }
                    ";" if depth == 0 => {
                        self.pos += 1;
                        return;
                    }
                    _ => {}
                }
            }
            self.pos += 1;
        }
    }

    fn end_statement(&mut self) -> R<()> {
        if self.is_punct(";") {
            self.pos += 1;
            return Ok(());
        }
        if self.is_punct("}") {
            return Ok(());
        }
        // blame the end of the previous statement: that is where the semicolon belongs
        let e = self.prev_end();
        let found = self.describe();
        self.fail_hint(e..e + 1, format!("expected `;` after the statement, found {found}"), "statements end with a semicolon")
    }

    fn statement(&mut self, at: &str) -> R<Json> {
        let Some(t) = self.peek().cloned() else {
            let r = self.here();
            return self.fail(r, "expected a statement");
        };
        let start = t.start;
        match (t.kind, t.text.as_str()) {
            (TokKind::Ident, "let") => {
                self.pos += 1;
                let (name, nr) = self.expect_ident("a name for the variable")?;
                if is_reserved(&name) {
                    return self.fail(nr, format!("`{name}` is a keyword: pick another name"));
                }
                self.expect_punct("=", "after the variable name")?;
                let e = self.expr()?;
                self.end_statement()?;
                e.record(&format!("{at}.be"), &mut self.spans);
                self.spans.add(format!("{at}.let"), nr);
                self.spans.add(at, start..self.prev_end().max(start));
                Ok(Json::obj([("let", Json::str(name)), ("be", e.json)]))
            }
            (TokKind::Ident, "if") => self.if_statement(at),
            (TokKind::Ident, "despawn") => {
                self.pos += 1;
                let mut arg = Json::Bool(true);
                if let Some(r) = self.peek().filter(|n| n.kind == TokKind::Ident && (n.text == "a" || n.text == "b")).cloned() {
                    self.pos += 1;
                    arg = Json::str(r.text);
                }
                self.end_statement()?;
                self.spans.add(at, start..self.prev_end());
                Ok(Json::obj([("despawn", arg)]))
            }
            (TokKind::Ident, "emit") => {
                self.pos += 1;
                let name = match self.peek().cloned() {
                    Some(n) if n.kind == TokKind::Str || n.kind == TokKind::Ident => {
                        self.pos += 1;
                        n.text
                    }
                    _ => {
                        let r = self.here();
                        return self.fail_hint(r, "emit needs the name of the event", "emit \"thud\";");
                    }
                };
                self.end_statement()?;
                self.spans.add(at, start..self.prev_end());
                Ok(Json::obj([("emit", Json::str(name))]))
            }
            (TokKind::Ident, "spawn") => {
                self.pos += 1;
                let comps = self.component_list(&format!("{at}.spawn"))?;
                self.end_statement()?;
                self.spans.add(at, start..self.prev_end());
                Ok(Json::obj([("spawn", comps)]))
            }
            (TokKind::Ident, "spawn_prefab") => self.spawn_prefab(at),
            (TokKind::Ident, "add") => {
                self.pos += 1;
                let (comp, cr) = self.expect_ident("the component to add")?;
                let mut pairs = vec![("add".to_string(), Json::str(comp))];
                self.spans.add(format!("{at}.add"), cr);
                if self.is_punct("{") {
                    let v = self.field_list(&format!("{at}.values"))?;
                    pairs.push(("values".to_string(), v));
                }
                if self.is_word("on") {
                    self.pos += 1;
                    let (role, rr) = self.expect_ident("`a` or `b`")?;
                    self.spans.add(format!("{at}.on"), rr);
                    pairs.push(("on".to_string(), Json::str(role)));
                }
                self.end_statement()?;
                self.spans.add(at, start..self.prev_end());
                Ok(Json::Obj(pairs))
            }
            (TokKind::Ident, "remove") => {
                self.pos += 1;
                let (comp, cr) = self.expect_ident("the component to remove")?;
                let mut pairs = vec![("remove".to_string(), Json::str(comp))];
                self.spans.add(format!("{at}.remove"), cr);
                if self.is_word("on") {
                    self.pos += 1;
                    let (role, rr) = self.expect_ident("`a` or `b`")?;
                    self.spans.add(format!("{at}.on"), rr);
                    pairs.push(("on".to_string(), Json::str(role)));
                }
                self.end_statement()?;
                self.spans.add(at, start..self.prev_end());
                Ok(Json::Obj(pairs))
            }
            (TokKind::Ident, "json") if self.peek_at(1).is_some_and(|n| n.kind == TokKind::Punct && n.text == "(") => {
                let (j, r) = self.raw_json()?;
                self.end_statement()?;
                self.spans.add(at, r);
                Ok(j)
            }
            (TokKind::Res, _) => {
                self.pos += 1;
                let name = t.text.clone();
                let op = self.assign_op()?;
                let e = self.expr()?;
                self.end_statement()?;
                self.spans.add(at, start..self.prev_end());
                self.spans.add(format!("{at}.res_set"), t.range());
                self.spans.add(format!("{at}.res_add"), t.range());
                let (key, by, value) = match op.as_str() {
                    "=" => ("res_set", "to", e),
                    "+=" => ("res_add", "by", e),
                    "-=" => {
                        let span = e.span.clone();
                        ("res_add", "by", Ex::op("neg", false, vec![e], span))
                    }
                    _ => return self.fail_hint(t.range(), format!("`{op}` cannot change a resource"), "use =, += or -="),
                };
                value.record(&format!("{at}.{by}"), &mut self.spans);
                Ok(Json::obj([(key.to_string(), Json::str(name)), (by.to_string(), value.json)]))
            }
            (TokKind::Ident, _) => self.assignment(at),
            _ => {
                let found = self.describe();
                self.fail_hint(t.range(), format!("expected a statement, found {found}"), "statements: let, if, despawn, emit, spawn, spawn_prefab, add, remove, Comp.field = value, $resource = value")
            }
        }
    }

    fn assign_op(&mut self) -> R<String> {
        match self.peek() {
            Some(t) if t.kind == TokKind::Punct && matches!(t.text.as_str(), "=" | "+=" | "-=" | "*=" | "/=") => {
                let s = t.text.clone();
                self.pos += 1;
                Ok(s)
            }
            _ => {
                let at = self.here();
                let found = self.describe();
                self.fail(at, format!("expected `=` after the name, found {found}"))
            }
        }
    }

    /// `Comp.field[i] = value;` and its `+=` family.
    fn assignment(&mut self, at: &str) -> R<Json> {
        let t = self.peek().cloned().unwrap_or_else(|| unreachable_token());
        // the place being written, as text
        let mut text = t.text.clone();
        let mut segs = 1;
        self.pos += 1;
        let mut end = t.end;
        loop {
            if self.is_punct(".") && self.peek_at(1).is_some_and(|n| n.kind == TokKind::Ident) {
                self.pos += 1;
                text.push('.');
                text.push_str(&self.toks[self.pos].text);
                end = self.toks[self.pos].end;
                self.pos += 1;
                segs += 1;
            } else if self.is_punct("[") {
                self.pos += 1;
                let Some(n) = self.peek().filter(|n| n.kind == TokKind::Int).cloned() else {
                    let r = self.here();
                    return self.fail_hint(r, "an index must be a whole number", "write it like pos[1]");
                };
                self.pos += 1;
                self.expect_punct("]", "to close the index")?;
                text.push_str(&format!("[{}]", n.text));
                end = self.prev_end();
            } else {
                break;
            }
        }
        let place = t.start..end;
        if segs == 1 {
            let found = self.describe();
            return self.fail_hint(place, format!("`{text}` is not something that can be assigned (next: {found})"), "assign a component field (Transform.pos[1] = 5;), a resource ($score = 0;), or declare a variable with let");
        }
        let op = self.assign_op()?;
        let e = self.expr()?;
        self.end_statement()?;
        self.spans.add(at, t.start..self.prev_end());
        self.spans.add(format!("{at}.set"), place.clone());
        let value = match op.as_str() {
            "=" => e,
            _ => {
                let get = Ex::op_named("get", Json::str(text.clone()), place.clone());
                let (name, array) = match op.as_str() {
                    "+=" => ("add", true),
                    "-=" => ("sub", true),
                    "*=" => ("mul", true),
                    _ => ("div", true),
                };
                let span = place.start..e.span.end;
                Ex::op(name, array, vec![get, e], span)
            }
        };
        value.record(&format!("{at}.to"), &mut self.spans);
        Ok(Json::obj([("set".to_string(), Json::str(text)), ("to".to_string(), value.json)]))
    }

    fn if_statement(&mut self, at: &str) -> R<Json> {
        let start = self.toks[self.pos].start;
        self.pos += 1;
        let cond = self.expr()?;
        cond.record(&format!("{at}.if"), &mut self.spans);
        if !self.is_punct("{") {
            let r = self.here();
            let found = self.describe();
            return self.fail_hint(r, format!("expected `{{` to start the if block, found {found}"), "if condition { ... }");
        }
        let then = self.block(&format!("{at}.then"))?;
        let mut pairs = vec![("if".to_string(), cond.json), ("then".to_string(), Json::Arr(then))];
        if self.is_word("else") {
            self.pos += 1;
            let els = if self.is_word("if") {
                let inner = self.if_statement(&format!("{at}.else[0]"))?;
                vec![inner]
            } else {
                self.block(&format!("{at}.else"))?
            };
            pairs.push(("else".to_string(), Json::Arr(els)));
        }
        self.spans.add(at, start..self.prev_end());
        Ok(Json::Obj(pairs))
    }

    /// `Comp { field: value, ... }, Other { ... }` inside braces.
    fn component_list(&mut self, path: &str) -> R<Json> {
        self.expect_punct("{", "to start the list of components")?;
        let mut comps: Vec<(String, Json)> = Vec::new();
        loop {
            if self.is_punct("}") {
                self.pos += 1;
                break;
            }
            let (name, nr) = self.expect_ident("a component name")?;
            self.spans.add(format!("{path}.{name}"), nr);
            let fields = if self.is_punct("{") { self.field_list(&format!("{path}.{name}"))? } else { Json::obj::<String>([]) };
            comps.push((name, fields));
            if self.is_punct(",") {
                self.pos += 1;
            } else if !self.is_punct("}") {
                let r = self.here();
                let found = self.describe();
                return self.fail(r, format!("expected `,` or `}}` after the component, found {found}"));
            }
        }
        Ok(Json::Obj(comps))
    }

    /// `{ field: value, ... }`
    fn field_list(&mut self, path: &str) -> R<Json> {
        self.expect_punct("{", "to start the fields")?;
        let mut fields: Vec<(String, Json)> = Vec::new();
        loop {
            if self.is_punct("}") {
                self.pos += 1;
                break;
            }
            let (name, nr) = self.expect_ident("a field name")?;
            self.expect_punct(":", "after the field name")?;
            let (json, span) = if self.is_punct("[") {
                // an array of values, `normal: [0, 1, 0]` (the `with` of spawn_prefab sets array fields this way)
                let open = self.expect_punct("[", "to start the array")?;
                let mut items = Vec::new();
                loop {
                    if self.is_punct("]") {
                        break;
                    }
                    let e = self.expr()?;
                    e.record(&format!("{path}.{name}[{}]", items.len()), &mut self.spans);
                    items.push(e.json);
                    if self.is_punct(",") {
                        self.pos += 1;
                    } else if !self.is_punct("]") {
                        let r = self.here();
                        let found = self.describe();
                        return self.fail(r, format!("expected `,` or `]` after the value, found {found}"));
                    }
                }
                let close = self.expect_punct("]", "to close the array")?;
                self.spans.add(format!("{path}.{name}"), nr);
                (Json::Arr(items), open.start..close.end)
            } else {
                let e = self.expr()?;
                self.spans.add(format!("{path}.{name}"), nr);
                e.record(&format!("{path}.{name}"), &mut self.spans);
                (e.json, e.span.clone())
            };
            if fields.iter().any(|(k, _)| *k == name) {
                return self.fail(span, format!("`{name}` is given twice"));
            }
            fields.push((name, json));
            if self.is_punct(",") {
                self.pos += 1;
            } else if !self.is_punct("}") {
                let r = self.here();
                let found = self.describe();
                return self.fail(r, format!("expected `,` or `}}` after the value, found {found}"));
            }
        }
        Ok(Json::Obj(fields))
    }

    fn tuple(&mut self, path: &str) -> R<Json> {
        self.expect_punct("(", "to start the three numbers")?;
        let mut items = Vec::new();
        loop {
            let e = self.expr()?;
            e.record(&format!("{path}[{}]", items.len()), &mut self.spans);
            items.push(e.json);
            if self.is_punct(",") {
                self.pos += 1;
                continue;
            }
            break;
        }
        self.expect_punct(")", "to close the list")?;
        Ok(Json::Arr(items))
    }

    fn spawn_prefab(&mut self, at: &str) -> R<Json> {
        let start = self.toks[self.pos].start;
        self.pos += 1;
        let Some(n) = self.peek().filter(|n| n.kind == TokKind::Str || n.kind == TokKind::Ident).cloned() else {
            let r = self.here();
            return self.fail_hint(r, "spawn_prefab needs the name of the prefab", "spawn_prefab \"Bullet\" at (0, 1, 0);");
        };
        self.pos += 1;
        self.spans.add(format!("{at}.spawn_prefab"), n.range());
        let mut pairs = vec![("spawn_prefab".to_string(), Json::str(n.text))];
        loop {
            if self.is_word("at") {
                self.pos += 1;
                let t = self.tuple(&format!("{at}.at"))?;
                pairs.push(("at".to_string(), t));
            } else if self.is_word("velocity") {
                self.pos += 1;
                let t = self.tuple(&format!("{at}.velocity"))?;
                pairs.push(("velocity".to_string(), t));
            } else if self.is_word("with") {
                self.pos += 1;
                let c = self.component_list(&format!("{at}.with"))?;
                pairs.push(("with".to_string(), c));
            } else {
                break;
            }
        }
        self.end_statement()?;
        self.spans.add(at, start..self.prev_end());
        Ok(Json::Obj(pairs))
    }

    // ----------------------------------------------------------------------------------------------------- systems

    fn name_list(&mut self) -> R<Vec<Json>> {
        let mut out = Vec::new();
        loop {
            let (n, _) = self.expect_ident("a component name")?;
            out.push(Json::str(n));
            if self.is_punct(",") {
                self.pos += 1;
                continue;
            }
            return Ok(out);
        }
    }

    fn system(&mut self) -> R<Parsed> {
        let head = self.toks[self.pos].clone();
        let doc = self.docs.get(&self.pos).cloned();
        self.pos += 1;
        let (name, nr) = self.expect_ident("the name of the system")?;
        let mut pairs: Vec<(String, Json)> = vec![("name".to_string(), Json::str(name.clone()))];
        if let Some(d) = doc {
            pairs.push(("doc".to_string(), Json::str(d)));
        }
        let (mut each, mut without, mut contact, mut order, mut disabled): (Option<Json>, Option<Json>, Option<Json>, Option<Json>, bool) = (None, None, None, None, false);
        loop {
            let Some(t) = self.peek().cloned() else { break };
            if t.kind != TokKind::Ident {
                break;
            }
            match t.text.as_str() {
                "each" => {
                    self.pos += 1;
                    each = Some(Json::Arr(self.name_list()?));
                }
                "without" => {
                    self.pos += 1;
                    without = Some(Json::Arr(self.name_list()?));
                }
                "order" => {
                    self.pos += 1;
                    match self.peek().filter(|n| n.kind == TokKind::Int).cloned() {
                        Some(n) => {
                            self.pos += 1;
                            order = Some(number(&n.text));
                        }
                        None => {
                            let r = self.here();
                            return self.fail_hint(r, "order is a whole number", "order 100");
                        }
                    }
                }
                "disabled" => {
                    self.pos += 1;
                    disabled = true;
                }
                "on_contact" => {
                    self.pos += 1;
                    let mut oc: Vec<(String, Json)> = Vec::new();
                    for role in ["a", "b"] {
                        self.expect_word(role, "(the on_contact list is: a(Components) b(Components))")?;
                        self.expect_punct("(", "after the role")?;
                        let names = if self.is_punct(")") { vec![] } else { self.name_list()? };
                        self.expect_punct(")", "to close the list")?;
                        oc.push((role.to_string(), Json::Arr(names)));
                    }
                    if self.is_word("triggers") {
                        self.pos += 1;
                        let (m, mr) = self.expect_ident("all, only or solid")?;
                        if !["all", "only", "solid"].contains(&m.as_str()) {
                            return self.fail_hint(mr, format!("`{m}` is not a way to treat triggers"), "triggers all | only | solid");
                        }
                        oc.push(("triggers".to_string(), Json::str(m)));
                    }
                    contact = Some(Json::Obj(oc));
                }
                _ => break,
            }
        }
        if let Some(c) = contact {
            if each.is_some() {
                return self.fail(nr, "a system is either `each` (one entity at a time) or `on_contact` (touching pairs), not both");
            }
            pairs.push(("on_contact".to_string(), c));
        } else if let Some(e) = each {
            pairs.push(("each".to_string(), e));
        } else {
            pairs.push(("each".to_string(), Json::Arr(vec![])));
        }
        if let Some(w) = without {
            pairs.push(("without".to_string(), w));
        }
        if let Some(o) = order {
            pairs.push(("order".to_string(), o));
        }
        if disabled {
            pairs.push(("enabled".to_string(), Json::Bool(false)));
        }
        let before = self.spans.items.len();
        let stmts = self.block("do")?;
        pairs.push(("do".to_string(), Json::Arr(stmts)));
        let range = head.start..self.prev_end();
        self.spans.add("", range.clone());
        self.spans.add("name", nr);
        let _ = before;
        let spans = std::mem::take(&mut self.spans);
        Ok(Parsed { name, def: Json::Obj(pairs), spans, range })
    }
}

fn unreachable_token() -> Token {
    Token { kind: TokKind::Error, start: 0, end: 0, text: String::new() }
}

impl Ex {
    fn op_named(op: &str, arg: Json, span: Range<usize>) -> Ex {
        Ex { json: Json::obj([(op.to_string(), arg)]), span, op: None, array: false, kids: vec![] }
    }
}

fn number(text: &str) -> Json {
    let t = text.trim_start_matches('-');
    let neg = text.starts_with('-');
    if !t.contains(['.', 'e', 'E']) {
        if let Ok(i) = t.parse::<i64>() {
            return Json::Int(if neg { -i } else { i });
        }
    }
    let f: f64 = t.parse().unwrap_or(0.0);
    Json::Float(if neg { -f } else { f })
}

/// Read a file of systems. Every system that could be read is returned along with every problem found; there are no systems when there are problems.
pub fn parse(src: &str) -> (Vec<Parsed>, Vec<Diag>) {
    let (all, mut diags) = lex::lex(src);
    // doc comments belong to the next real token; comments are otherwise dropped
    let mut toks = Vec::new();
    let mut docs: HashMap<usize, String> = HashMap::new();
    let mut pending: Vec<String> = Vec::new();
    for t in all {
        match t.kind {
            TokKind::Comment => {}
            TokKind::DocComment => pending.push(t.text.clone()),
            _ => {
                if !pending.is_empty() {
                    docs.insert(toks.len(), pending.join("\n"));
                    pending.clear();
                }
                toks.push(t);
            }
        }
    }
    let mut p = Parser { src, toks, pos: 0, docs, diags: Vec::new(), spans: SpanMap::default(), depth: 0 };
    let mut systems: Vec<Parsed> = Vec::new();
    while p.peek().is_some() {
        if p.is_word("system") {
            match p.system() {
                Ok(s) => {
                    if systems.iter().any(|o| o.name == s.name) {
                        let r = s.spans.locate("name").unwrap_or(s.range.clone());
                        p.push_diag(diag(r.start, r.end, format!("there is already a system named `{}` in this file", s.name)));
                    }
                    systems.push(s);
                }
                Err(()) => {
                    // skip to the next `system` at the start of a statement list
                    p.pos += 1;
                    while p.peek().is_some() && !p.is_word("system") {
                        p.pos += 1;
                    }
                }
            }
        } else {
            let r = p.here();
            let found = p.describe();
            p.push_diag(diag(r.start, r.end, format!("expected `system`, found {found}")));
            while p.peek().is_some() && !p.is_word("system") {
                p.pos += 1;
            }
        }
    }
    diags.extend(p.diags);
    diags.sort_by_key(|d| d.start);
    diags.dedup_by(|a, b| a.start == b.start && a.message == b.message);
    (systems, diags)
}
