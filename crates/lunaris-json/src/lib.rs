//! Strict RFC 8259 JSON for Lunaris: a value type that keeps object key order, a parser with line/column errors,
//! and a writer (compact and pretty). No dependencies.
//!
//! Numbers are kept as `Int(i64)` when the text has no fraction or exponent and fits, otherwise `Float(f64)`.
//! Non-finite floats cannot be written (JSON has no NaN/Infinity); the writer returns an error instead of
//! emitting something invalid.

use std::fmt;
use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Json>),
    /// Insertion-ordered key/value pairs (duplicate keys are rejected by the parser).
    Obj(Vec<(String, Json)>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct JsonError {
    pub message: String,
    pub line: usize,
    pub column: usize,
    pub offset: usize,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at line {}, column {}", self.message, self.line, self.column)
    }
}

impl std::error::Error for JsonError {}

const MAX_DEPTH: usize = 128;

// ---------------------------------------------------------------------------------------------------------------
// accessors and builders
impl Json {
    pub fn obj<K: Into<String>>(pairs: impl IntoIterator<Item = (K, Json)>) -> Json {
        Json::Obj(pairs.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    pub fn arr(items: impl IntoIterator<Item = Json>) -> Json {
        Json::Arr(items.into_iter().collect())
    }

    pub fn str(s: impl Into<String>) -> Json {
        Json::Str(s.into())
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(p) => p.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn index(&self, i: usize) -> Option<&Json> {
        match self {
            Json::Arr(a) => a.get(i),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Int(i) => Some(*i),
            Json::Float(f) if f.fract() == 0.0 && *f >= -9.007_199_254_740_992e15 && *f <= 9.007_199_254_740_992e15 => Some(*f as i64),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Int(i) if *i >= 0 => Some(*i as u64),
            Json::Float(f) if f.fract() == 0.0 && *f >= 0.0 && *f <= 9.007_199_254_740_992e15 => Some(*f as u64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Int(i) => Some(*i as f64),
            Json::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Obj(p) => Some(p),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Json::Null => "null",
            Json::Bool(_) => "boolean",
            Json::Int(_) | Json::Float(_) => "number",
            Json::Str(_) => "string",
            Json::Arr(_) => "array",
            Json::Obj(_) => "object",
        }
    }

    /// Set (or replace) a key on an object. No-op on other types.
    pub fn set(&mut self, key: &str, value: Json) {
        if let Json::Obj(p) = self {
            if let Some(slot) = p.iter_mut().find(|(k, _)| k == key) {
                slot.1 = value;
            } else {
                p.push((key.to_string(), value));
            }
        }
    }
}

impl From<bool> for Json {
    fn from(v: bool) -> Self {
        Json::Bool(v)
    }
}
impl From<i64> for Json {
    fn from(v: i64) -> Self {
        Json::Int(v)
    }
}
impl From<i32> for Json {
    fn from(v: i32) -> Self {
        Json::Int(v as i64)
    }
}
impl From<u32> for Json {
    fn from(v: u32) -> Self {
        Json::Int(v as i64)
    }
}
impl From<usize> for Json {
    fn from(v: usize) -> Self {
        Json::Int(v as i64)
    }
}
impl From<f64> for Json {
    fn from(v: f64) -> Self {
        Json::Float(v)
    }
}
impl From<f32> for Json {
    fn from(v: f32) -> Self {
        Json::Float(v as f64)
    }
}
impl From<&str> for Json {
    fn from(v: &str) -> Self {
        Json::Str(v.to_string())
    }
}
impl From<String> for Json {
    fn from(v: String) -> Self {
        Json::Str(v)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// parser
pub fn parse(text: &str) -> Result<Json, JsonError> {
    let mut p = Parser { s: text.as_bytes(), text, pos: 0 };
    p.skip_ws();
    let v = p.value(0)?;
    p.skip_ws();
    if p.pos != p.s.len() {
        return Err(p.err("unexpected data after the JSON value"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    text: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, msg: &str) -> JsonError {
        self.err_at(msg, self.pos)
    }

    fn err_at(&self, msg: &str, offset: usize) -> JsonError {
        let upto = &self.text.as_bytes()[..offset.min(self.s.len())];
        let line = upto.iter().filter(|&&b| b == b'\n').count() + 1;
        let col_start = upto.iter().rposition(|&b| b == b'\n').map(|i| i + 1).unwrap_or(0);
        let column = String::from_utf8_lossy(&upto[col_start..]).chars().count() + 1;
        JsonError { message: msg.to_string(), line, column, offset }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.s.len() && matches!(self.s[self.pos], b' ' | b'\t' | b'\n' | b'\r') {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn expect_lit(&mut self, lit: &str, v: Json) -> Result<Json, JsonError> {
        if self.s[self.pos..].starts_with(lit.as_bytes()) {
            self.pos += lit.len();
            Ok(v)
        } else {
            Err(self.err(&format!("expected `{lit}`")))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting is too deep"));
        }
        match self.peek() {
            None => Err(self.err("unexpected end of input, expected a value")),
            Some(b'n') => self.expect_lit("null", Json::Null),
            Some(b't') => self.expect_lit("true", Json::Bool(true)),
            Some(b'f') => self.expect_lit("false", Json::Bool(false)),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            Some(c) => Err(self.err(&format!("unexpected character `{}`", c as char))),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.skip_ws();
                    if self.peek() == Some(b']') {
                        return Err(self.err("trailing comma in array"));
                    }
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(self.err("expected `,` or `]` in array")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.pos += 1;
        let mut pairs: Vec<(String, Json)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a string key"));
            }
            let key_pos = self.pos;
            let key = self.string()?;
            if pairs.iter().any(|(k, _)| *k == key) {
                return Err(self.err_at(&format!("duplicate key \"{key}\""), key_pos));
            }
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected `:` after object key"));
            }
            self.pos += 1;
            self.skip_ws();
            let v = self.value(depth + 1)?;
            pairs.push((key, v));
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.skip_ws();
                    if self.peek() == Some(b'}') {
                        return Err(self.err("trailing comma in object"));
                    }
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(pairs));
                }
                _ => return Err(self.err("expected `,` or `}` in object")),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        if self.pos + 4 > self.s.len() {
            return Err(self.err("incomplete \\u escape"));
        }
        let mut v = 0u32;
        for i in 0..4 {
            let c = self.s[self.pos + i];
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return Err(self.err_at("invalid hex digit in \\u escape", self.pos + i)),
            };
            v = v * 16 + d as u32;
        }
        self.pos += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.pos += 1; // opening quote
        // fast path: no escapes at all (nearly every key and value), one exact allocation
        {
            let start = self.pos;
            let mut i = start;
            while i < self.s.len() {
                let b = self.s[i];
                if b == b'"' {
                    self.pos = i + 1;
                    return Ok(self.text[start..i].to_string());
                }
                if b == b'\\' || b < 0x20 {
                    break;
                }
                i += 1;
            }
        }
        let mut out = String::new();
        loop {
            let start = self.pos;
            // fast path: copy a run of plain bytes
            while self.pos < self.s.len() {
                let b = self.s[self.pos];
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            out.push_str(&self.text[start..self.pos]);
            match self.peek() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let c = self.peek().ok_or_else(|| self.err("unterminated escape"))?;
                    self.pos += 1;
                    match c {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                if self.s[self.pos..].starts_with(b"\\u") {
                                    self.pos += 2;
                                    let lo = self.hex4()?;
                                    if !(0xDC00..0xE000).contains(&lo) {
                                        return Err(self.err("invalid low surrogate"));
                                    }
                                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                                } else {
                                    return Err(self.err("high surrogate without a low surrogate"));
                                }
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return Err(self.err("lone low surrogate"));
                            } else {
                                hi
                            };
                            out.push(char::from_u32(cp).ok_or_else(|| self.err("invalid code point"))?);
                        }
                        _ => return Err(self.err_at("invalid escape sequence", self.pos - 1)),
                    }
                }
                Some(_) => return Err(self.err("control character in string (must be escaped)")),
            }
        }
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        let negative = self.peek() == Some(b'-');
        if negative {
            self.pos += 1;
        }
        // While scanning, collect the digits as an integer mantissa and the decimal exponent, for the exact fast path below.
        let mut mantissa: u64 = 0;
        let mut too_many_digits = false;
        let mut push_digit = |d: u8, mantissa: &mut u64| {
            if *mantissa <= (u64::MAX - 9) / 10 {
                *mantissa = *mantissa * 10 + (d - b'0') as u64;
            } else {
                too_many_digits = true;
            }
        };
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(self.err("leading zeros are not allowed"));
                }
            }
            Some(d @ b'1'..=b'9') => {
                push_digit(d, &mut mantissa);
                self.pos += 1;
                while let Some(d @ b'0'..=b'9') = self.peek() {
                    push_digit(d, &mut mantissa);
                    self.pos += 1;
                }
            }
            _ => return Err(self.err("expected a digit")),
        }
        let mut is_int = true;
        let mut fraction_digits: i32 = 0;
        if self.peek() == Some(b'.') {
            is_int = false;
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("expected a digit after the decimal point"));
            }
            while let Some(d @ b'0'..=b'9') = self.peek() {
                push_digit(d, &mut mantissa);
                fraction_digits += 1;
                self.pos += 1;
            }
        }
        let mut exponent: i32 = 0;
        let mut exponent_ok = true;
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_int = false;
            self.pos += 1;
            let mut exp_negative = false;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                exp_negative = self.peek() == Some(b'-');
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("expected a digit in the exponent"));
            }
            while let Some(d @ b'0'..=b'9') = self.peek() {
                if exponent < 10_000 {
                    exponent = exponent * 10 + (d - b'0') as i32;
                } else {
                    exponent_ok = false;
                }
                self.pos += 1;
            }
            if exp_negative {
                exponent = -exponent;
            }
        }
        if !too_many_digits && exponent_ok {
            if is_int {
                if mantissa <= i64::MAX as u64 {
                    let m = mantissa as i64;
                    return Ok(Json::Int(if negative { -m } else { m }));
                }
            } else if mantissa <= (1u64 << 53) {
                // Both the mantissa and the power of ten are exact in an f64, so one multiplication or division rounds
                // correctly: the same result as the slow general conversion, without its cost.
                const POW10: [f64; 23] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22];
                let e10 = exponent - fraction_digits;
                if (-22..=22).contains(&e10) {
                    let m = mantissa as f64;
                    let f = if e10 >= 0 { m * POW10[e10 as usize] } else { m / POW10[(-e10) as usize] };
                    return Ok(Json::Float(if negative { -f } else { f }));
                }
            }
        }
        let txt = &self.text[start..self.pos];
        if is_int {
            if let Ok(i) = txt.parse::<i64>() {
                return Ok(Json::Int(i));
            }
        }
        match txt.parse::<f64>() {
            Ok(f) if f.is_finite() => Ok(Json::Float(f)),
            _ => Err(self.err_at("number is out of range", start)),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// writer
#[derive(Debug, PartialEq)]
pub struct WriteError(pub String);

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WriteError {}

impl Json {
    pub fn to_string_compact(&self) -> Result<String, WriteError> {
        let mut out = String::new();
        self.write(&mut out, None, 0)?;
        Ok(out)
    }

    pub fn to_string_pretty(&self) -> Result<String, WriteError> {
        let mut out = String::new();
        self.write(&mut out, Some(2), 0)?;
        out.push('\n');
        Ok(out)
    }

    /// Compact output that substitutes `null` for non-finite floats (for logs and diagnostics only).
    pub fn to_string_lossy(&self) -> String {
        let mut out = String::new();
        self.write_lossy(&mut out);
        out
    }

    fn write_lossy(&self, out: &mut String) {
        match self {
            Json::Float(f) if !f.is_finite() => out.push_str("null"),
            Json::Arr(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_lossy(out);
                }
                out.push(']');
            }
            Json::Obj(p) => {
                out.push('{');
                for (i, (k, v)) in p.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(out, k);
                    out.push(':');
                    v.write_lossy(out);
                }
                out.push('}');
            }
            other => {
                let _ = other.write(out, None, 0);
            }
        }
    }

    fn write(&self, out: &mut String, indent: Option<usize>, level: usize) -> Result<(), WriteError> {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(i) => {
                let _ = write!(out, "{i}");
            }
            Json::Float(f) => {
                if !f.is_finite() {
                    return Err(WriteError(format!("cannot write non-finite number {f}")));
                }
                // Rust's shortest round-trip formatting; an integral float prints as "1", which re-reads as Int(1);
                // readers use as_f64, so the value is the same.
                let _ = write!(out, "{f}");
            }
            Json::Str(s) => write_string(out, s),
            Json::Arr(a) => {
                if a.is_empty() {
                    out.push_str("[]");
                } else {
                    out.push('[');
                    for (i, v) in a.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        newline(out, indent, level + 1);
                        v.write(out, indent, level + 1)?;
                    }
                    newline(out, indent, level);
                    out.push(']');
                }
            }
            Json::Obj(p) => {
                if p.is_empty() {
                    out.push_str("{}");
                } else {
                    out.push('{');
                    for (i, (k, v)) in p.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        newline(out, indent, level + 1);
                        write_string(out, k);
                        out.push(':');
                        if indent.is_some() {
                            out.push(' ');
                        }
                        v.write(out, indent, level + 1)?;
                    }
                    newline(out, indent, level);
                    out.push('}');
                }
            }
        }
        Ok(())
    }
}

fn newline(out: &mut String, indent: Option<usize>, level: usize) {
    if let Some(n) = indent {
        out.push('\n');
        for _ in 0..n * level {
            out.push(' ');
        }
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    // copy runs of text that needs no escaping in one go; only `"`, `\\` and control characters are special (all ASCII, so the
    // run boundaries are always character boundaries)
    let mut run = 0;
    for (i, b) in s.bytes().enumerate() {
        if b != b'"' && b != b'\\' && b >= 0x20 {
            continue;
        }
        out.push_str(&s[run..i]);
        run = i + 1;
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            8 => out.push_str("\\b"),
            0xc => out.push_str("\\f"),
            c => {
                let _ = write!(out, "\\u{:04x}", c);
            }
        }
    }
    out.push_str(&s[run..]);
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scalars_and_containers() {
        let v = parse(r#" {"a": [1, 2.5, -3e2, true, null, "x"], "b": {"c": 0}} "#).unwrap();
        assert_eq!(v.get("a").unwrap().index(0), Some(&Json::Int(1)));
        assert_eq!(v.get("a").unwrap().index(1), Some(&Json::Float(2.5)));
        assert_eq!(v.get("a").unwrap().index(2), Some(&Json::Float(-300.0)));
        assert_eq!(v.get("b").unwrap().get("c"), Some(&Json::Int(0)));
    }

    #[test]
    fn keeps_key_order() {
        let v = parse(r#"{"z":1,"a":2,"m":3}"#).unwrap();
        let keys: Vec<_> = v.as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["z", "a", "m"]);
        assert_eq!(v.to_string_compact().unwrap(), r#"{"z":1,"a":2,"m":3}"#);
    }

    #[test]
    fn round_trips_strings_with_escapes_and_unicode() {
        let src = r#""line\nbreak \"quoted\" \\ tab\t \u00e9 \ud83d\ude00 \u0001""#;
        let v = parse(src).unwrap();
        assert_eq!(v.as_str().unwrap(), "line\nbreak \"quoted\" \\ tab\t é 😀 \u{1}");
        let again = parse(&v.to_string_compact().unwrap()).unwrap();
        assert_eq!(v, again);
        // exact text, with plain runs before, between and after the escapes, and multi-byte characters inside the runs
        let tricky = Json::str("é\"b\\c\n\u{1}ü😀 plain \u{c}\u{8}\t\r end");
        let text = tricky.to_string_compact().unwrap();
        assert_eq!(text, r#""é\"b\\c\n\u0001ü😀 plain \f\b\t\r end""#);
        assert_eq!(parse(&text).unwrap(), tricky);
        assert_eq!(Json::str("").to_string_compact().unwrap(), r#""""#);
        assert_eq!(parse(r#""é😀 plain no escapes""#).unwrap().as_str().unwrap(), "é😀 plain no escapes");
    }

    #[test]
    fn the_fast_number_path_gives_exactly_the_same_values_as_the_standard_conversion() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut texts: Vec<String> = ["0", "-0", "0.0", "-0.0", "0.1", "0.3", "1e22", "1e23", "9007199254740992", "9007199254740993", "9007199254740993.0", "123456789012345678", "123456789012345678901234567890", "9223372036854775807", "9223372036854775808", "-9223372036854775808", "1.7976931348623157e308", "5e-324", "2.2250738585072014e-308", "1e-22", "1e-23", "0.000001", "100000000000000000000000", "4.35", "0.1e1", "12345.678e-3", "1E5", "1e+5"].iter().map(|s| s.to_string()).collect();
        for _ in 0..20_000 {
            let digits = 1 + (next() % 18) as usize;
            let int_len = 1 + (next() % digits as u64) as usize;
            let mut t = String::new();
            if next() % 4 == 0 {
                t.push('-');
            }
            for i in 0..digits {
                if i == int_len {
                    t.push('.');
                }
                let d = (next() % 10) as u8;
                // no leading zero in the integer part
                t.push((b'0' + if i == 0 && d == 0 { 1 } else { d }) as char);
            }
            if int_len == digits && next() % 3 == 0 {
                t.push_str(".5");
            }
            if next() % 3 == 0 {
                t.push_str(&format!("e{}", (next() % 60) as i32 - 30));
            }
            texts.push(t);
        }
        for t in texts {
            let want_int = t.parse::<i64>().ok().filter(|_| !t.contains(['.', 'e', 'E']));
            let want_float = t.parse::<f64>().ok().filter(|f| f.is_finite());
            match parse(&t) {
                Ok(Json::Int(i)) => assert_eq!(Some(i), want_int, "{t}"),
                Ok(Json::Float(f)) => assert_eq!(Some(f.to_bits()), want_float.map(f64::to_bits), "{t}"),
                Ok(other) => panic!("{t} parsed as {other:?}"),
                Err(_) => assert!(want_float.is_none() && want_int.is_none(), "{t} should parse"),
            }
        }
    }

    #[test]
    fn rejects_invalid_documents_with_positions() {
        let cases = [
            ("[1,]", "trailing comma"),
            ("{\"a\":1,}", "trailing comma"),
            ("01", "leading zeros"),
            ("1.", "decimal point"),
            ("\"abc", "unterminated"),
            ("{\"a\":1}x", "unexpected data"),
            ("[1 2]", "expected `,`"),
            ("{\"a\":1,\"a\":2}", "duplicate key"),
            ("\"\\ud800\"", "surrogate"),
            ("\"a\tb\"", "control character"),
            ("tru", "expected `true`"),
            ("", "end of input"),
            ("1e999", "out of range"),
        ];
        for (src, want) in cases {
            let e = parse(src).expect_err(src);
            assert!(e.message.contains(want), "{src}: got `{}`, wanted `{want}`", e.message);
            assert!(e.line >= 1 && e.column >= 1);
        }
    }

    #[test]
    fn error_position_is_line_and_column() {
        let e = parse("{\n  \"a\": 1,\n  \"b\": ?\n}").unwrap_err();
        assert_eq!((e.line, e.column), (3, 8));
    }

    #[test]
    fn numbers_keep_int_vs_float() {
        assert_eq!(parse("9007199254740993").unwrap(), Json::Int(9007199254740993));
        assert_eq!(parse("-0").unwrap(), Json::Int(0));
        assert_eq!(parse("1.0").unwrap(), Json::Float(1.0));
        assert_eq!(parse("1E2").unwrap(), Json::Float(100.0));
        assert!(matches!(parse("18446744073709551615").unwrap(), Json::Float(_)));
    }

    #[test]
    fn depth_limit() {
        let deep = "[".repeat(200) + &"]".repeat(200);
        assert!(parse(&deep).unwrap_err().message.contains("too deep"));
        let ok = "[".repeat(100) + &"]".repeat(100);
        assert!(parse(&ok).is_ok());
    }

    #[test]
    fn writer_rejects_non_finite() {
        assert!(Json::Float(f64::NAN).to_string_compact().is_err());
        assert!(Json::Arr(vec![Json::Float(f64::INFINITY)]).to_string_compact().is_err());
        assert_eq!(Json::Float(f64::NAN).to_string_lossy(), "null");
    }

    #[test]
    fn float_text_round_trips_exactly() {
        for f in [0.1f64, 1.0 / 3.0, 1e-300, 123456789.123456789, f64::MAX, f64::MIN_POSITIVE, -2.5e-10] {
            let t = Json::Float(f).to_string_compact().unwrap();
            assert_eq!(parse(&t).unwrap().as_f64().unwrap(), f, "{t}");
        }
    }

    #[test]
    fn pretty_output_parses_back() {
        let v = Json::obj([("a", Json::arr([Json::from(1), Json::from(2.5)])), ("b", Json::obj([("c", Json::Null)]))]);
        let p = v.to_string_pretty().unwrap();
        assert!(p.contains("\n  \"a\": ["));
        assert_eq!(parse(&p).unwrap(), v);
    }
}

/// Is `rel` a plain relative path that stays inside the folder it is joined to, **on every operating system**? Returns it with forward
/// slashes. Not empty, no NUL, not absolute (`/x`, `\x`, `C:\x`, `C:x`, `\\server\x`), no `..` component, and no component with a `:` in it
/// (a drive letter, or a Windows alternate data stream). Checked by text rather than by `std::path`, because `C:/x` is a relative path on
/// Linux and a drive on Windows, and a sandbox must say the same thing everywhere.
pub fn safe_relative(rel: &str) -> std::result::Result<String, &'static str> {
    if rel.is_empty() {
        return Err("the path is empty");
    }
    if rel.contains('\0') {
        return Err("the path contains a NUL");
    }
    let norm = rel.replace('\\', "/");
    if norm.starts_with('/') {
        return Err("the path is absolute");
    }
    for part in norm.split('/') {
        if part == ".." {
            return Err("the path goes up with `..`");
        }
        if part.contains(':') {
            return Err("the path has a drive letter or `:` in it");
        }
    }
    Ok(norm)
}

#[cfg(test)]
mod path_tests {
    use super::safe_relative;

    #[test]
    fn only_plain_relative_paths_pass_and_the_answer_is_the_same_everywhere() {
        for ok in ["a.json", "scenes/a.json", "scenes\\a.json", "./a.json", "a//b.json", "models/crate.glb"] {
            assert!(safe_relative(ok).is_ok(), "{ok}");
        }
        assert_eq!(safe_relative("scenes\\a.json").unwrap(), "scenes/a.json");
        for bad in ["", "../x", "a/../../b", "a\\..\\b", "/etc/passwd", "\\\\server\\share\\x", "\\x", "C:/Windows/x", "C:x", "d:\\x", "a/b:c", "x.txt:stream", "a\0b"] {
            assert!(safe_relative(bad).is_err(), "{bad:?}");
        }
    }
}
