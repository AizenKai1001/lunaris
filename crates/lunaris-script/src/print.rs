//! Graph-system JSON back into Lunaris Script. `parse(print(def))` gives `def` again for everything the language can say; what it cannot
//! say (an operation added later, say) is written as `json(...)`, which parses back to the same JSON.
use crate::lex::is_reserved;
use crate::parse::{is_ident, BINARY, NARY, SINGLE, TERNARY};
use lunaris_json::Json;

const IND: &str = "    ";

/// The text of one system. Plugin and WebAssembly systems have no script form.
pub fn print_system(def: &Json) -> Result<String, String> {
    if def.as_object().is_none() {
        return Err("a system definition is a JSON object".to_string());
    }
    for k in ["plugin", "wasm"] {
        if def.get(k).is_some() {
            return Err(format!("this is a {k} system (compiled code): it has no script form"));
        }
    }
    let name = def.get("name").and_then(|n| n.as_str()).filter(|n| is_ident(n)).ok_or("the system has no valid name")?;
    let mut out = String::new();
    if let Some(doc) = def.get("doc").and_then(|d| d.as_str()).filter(|d| !d.is_empty()) {
        for line in doc.split('\n') {
            out.push_str("/// ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str("system ");
    out.push_str(name);
    let list = |v: Option<&Json>| -> Result<Vec<String>, String> {
        v.and_then(|a| a.as_array())
            .map(|a| a.iter().map(|x| x.as_str().filter(|s| is_ident(s)).map(str::to_string).ok_or_else(|| "a component name is not a plain name".to_string())).collect())
            .unwrap_or_else(|| Ok(vec![]))
    };
    if let Some(oc) = def.get("on_contact") {
        let (a, b) = (list(oc.get("a"))?, list(oc.get("b"))?);
        out.push_str(&format!(" on_contact a({}) b({})", a.join(", "), b.join(", ")));
        if let Some(t) = oc.get("triggers").and_then(|t| t.as_str()) {
            if t != "all" {
                out.push_str(&format!(" triggers {t}"));
            }
        }
    } else {
        let each = list(def.get("each"))?;
        if !each.is_empty() {
            out.push_str(&format!(" each {}", each.join(", ")));
        }
    }
    let without = list(def.get("without"))?;
    if !without.is_empty() {
        out.push_str(&format!(" without {}", without.join(", ")));
    }
    if let Some(o) = def.get("order") {
        match o {
            Json::Int(i) if *i >= 0 => out.push_str(&format!(" order {i}")),
            _ => return Err("`order` must be a whole number".to_string()),
        }
    }
    if def.get("enabled").and_then(|e| e.as_bool()) == Some(false) {
        out.push_str(" disabled");
    }
    out.push_str(" {\n");
    let stmts = def.get("do").and_then(|d| d.as_array()).ok_or("the system has no `do` list")?;
    block(stmts, 1, &mut out);
    out.push_str("}\n");
    Ok(out)
}

fn pad(depth: usize) -> String {
    IND.repeat(depth)
}

fn block(stmts: &[Json], depth: usize, out: &mut String) {
    for s in stmts {
        statement(s, depth, out);
    }
}

fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Is this text something `a.b[1].c`-shaped that the language reads back as the same path?
fn is_path(s: &str) -> bool {
    let (mut segs, mut rest) = (0, s);
    if s.is_empty() {
        return false;
    }
    loop {
        let end = rest.find(['.', '[']).unwrap_or(rest.len());
        let seg = &rest[..end];
        if !is_ident(seg) {
            return false;
        }
        segs += 1;
        rest = &rest[end..];
        // any number of [n] after a segment
        while let Some(r) = rest.strip_prefix('[') {
            let Some(close) = r.find(']') else { return false };
            if r[..close].is_empty() || !r[..close].bytes().all(|c| c.is_ascii_digit()) {
                return false;
            }
            rest = &r[close + 1..];
        }
        match rest.strip_prefix('.') {
            Some(r) => rest = r,
            None => return rest.is_empty() && segs >= 2,
        }
    }
}

fn is_resource(s: &str) -> bool {
    !s.is_empty() && s.split('.').all(is_ident)
}

fn res_text(name: &str) -> String {
    if is_resource(name) {
        format!("${name}")
    } else {
        format!("$\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn raw(j: &Json) -> String {
    format!("json({})", j.to_string_compact().unwrap_or_else(|_| "null".to_string()))
}

fn keys_are(s: &Json, keys: &[&str]) -> bool {
    match s.as_object() {
        Some(p) => p.iter().all(|(k, _)| keys.contains(&k.as_str())) && p.first().is_some_and(|(k, _)| k == keys[0]),
        None => false,
    }
}

fn statement(s: &Json, depth: usize, out: &mut String) {
    let p = pad(depth);
    let first = s.as_object().and_then(|o| o.first()).map(|(k, _)| k.as_str()).unwrap_or("");
    let text = |k: &str| s.get(k).and_then(|v| v.as_str());
    let line: Option<String> = match first {
        "set" if keys_are(s, &["set", "to"]) && s.get("to").is_some() => text("set").filter(|t| is_path(t)).map(|t| format!("{t} = {};", expr(s.get("to").unwrap_or(&Json::Null), 0))),
        "let" if keys_are(s, &["let", "be"]) && s.get("be").is_some() => text("let").filter(|n| is_ident(n) && !is_reserved(n)).map(|n| format!("let {n} = {};", expr(s.get("be").unwrap_or(&Json::Null), 0))),
        "res_set" if keys_are(s, &["res_set", "to"]) && s.get("to").is_some() => text("res_set").map(|n| format!("{} = {};", res_text(n), expr(s.get("to").unwrap_or(&Json::Null), 0))),
        "res_add" if keys_are(s, &["res_add", "by"]) && s.get("by").is_some() => text("res_add").map(|n| format!("{} += {};", res_text(n), expr(s.get("by").unwrap_or(&Json::Null), 0))),
        "emit" if keys_are(s, &["emit"]) => text("emit").map(|n| format!("emit {};", quote(n))),
        "despawn" if keys_are(s, &["despawn"]) => match s.get("despawn") {
            Some(Json::Bool(true)) => Some("despawn;".to_string()),
            Some(Json::Str(r)) if r == "a" || r == "b" => Some(format!("despawn {r};")),
            _ => None,
        },
        "remove" if keys_are(s, &["remove", "on"]) => text("remove").filter(|c| is_ident(c)).and_then(|c| match s.get("on") {
            None => Some(format!("remove {c};")),
            Some(Json::Str(r)) if r == "a" || r == "b" => Some(format!("remove {c} on {r};")),
            _ => None,
        }),
        "add" if keys_are(s, &["add", "values", "on"]) => text("add").filter(|c| is_ident(c)).and_then(|c| {
            let mut t = format!("add {c}");
            if let Some(v) = s.get("values") {
                t.push(' ');
                t.push_str(&fields(v)?);
            }
            match s.get("on") {
                None => {}
                Some(Json::Str(r)) if r == "a" || r == "b" => t.push_str(&format!(" on {r}")),
                _ => return None,
            }
            t.push(';');
            Some(t)
        }),
        "spawn" if keys_are(s, &["spawn"]) => s.get("spawn").and_then(components).map(|c| format!("spawn {c};")),
        "spawn_prefab" if keys_are(s, &["spawn_prefab", "at", "velocity", "with"]) => text("spawn_prefab").and_then(|n| {
            let mut t = format!("spawn_prefab {}", quote(n));
            for k in ["at", "velocity"] {
                if let Some(v) = s.get(k) {
                    let a = v.as_array()?;
                    t.push_str(&format!(" {k} ({})", a.iter().map(|e| expr(e, 0)).collect::<Vec<_>>().join(", ")));
                }
            }
            if let Some(w) = s.get("with") {
                t.push_str(&format!(" with {}", components(w)?));
            }
            t.push(';');
            Some(t)
        }),
        "if" if keys_are(s, &["if", "then", "else"]) && s.get("then").and_then(|t| t.as_array()).is_some() => {
            print_if(s, depth, out, false);
            return;
        }
        _ => None,
    };
    match line {
        Some(l) => {
            out.push_str(&p);
            out.push_str(&l);
            out.push('\n');
        }
        None => {
            out.push_str(&p);
            out.push_str(&raw(s));
            out.push_str(";\n");
        }
    }
}

fn print_if(s: &Json, depth: usize, out: &mut String, continuing: bool) {
    let p = pad(depth);
    if !continuing {
        out.push_str(&p);
    }
    out.push_str(&format!("if {} {{\n", expr(s.get("if").unwrap_or(&Json::Null), 0)));
    block(s.get("then").and_then(|t| t.as_array()).unwrap_or(&[]), depth + 1, out);
    out.push_str(&p);
    out.push('}');
    match s.get("else").and_then(|e| e.as_array()) {
        Some(e) if e.len() == 1 && e[0].get("if").is_some() && keys_are(&e[0], &["if", "then", "else"]) && e[0].get("then").and_then(|t| t.as_array()).is_some() => {
            out.push_str(" else ");
            print_if(&e[0], depth, out, true);
        }
        Some(e) if !e.is_empty() => {
            out.push_str(" else {\n");
            block(e, depth + 1, out);
            out.push_str(&p);
            out.push_str("}\n");
        }
        _ => out.push('\n'),
    }
}

fn fields(v: &Json) -> Option<String> {
    let o = v.as_object()?;
    if o.is_empty() {
        return Some("{}".to_string());
    }
    let mut parts = Vec::new();
    for (k, e) in o {
        if !is_ident(k) {
            return None;
        }
        match e {
            Json::Arr(items) => parts.push(format!("{k}: [{}]", items.iter().map(|x| expr(x, 0)).collect::<Vec<_>>().join(", "))),
            _ => parts.push(format!("{k}: {}", expr(e, 0))),
        }
    }
    Some(format!("{{ {} }}", parts.join(", ")))
}

fn components(v: &Json) -> Option<String> {
    let o = v.as_object()?;
    let mut parts = Vec::new();
    for (c, f) in o {
        if !is_ident(c) {
            return None;
        }
        parts.push(format!("{c} {}", fields(f)?));
    }
    Some(format!("{{ {} }}", parts.join(", ")))
}

// ---------------------------------------------------------------------------------------------------------------- expressions

const P_OR: u8 = 1;
const P_AND: u8 = 2;
const P_EQ: u8 = 3;
const P_CMP: u8 = 4;
const P_ADD: u8 = 5;
const P_MUL: u8 = 6;
const P_UNARY: u8 = 7;
const P_ATOM: u8 = 8;

fn infix(op: &str) -> Option<(u8, &'static str)> {
    Some(match op {
        "or" => (P_OR, "||"),
        "and" => (P_AND, "&&"),
        "eq" => (P_EQ, "=="),
        "ne" => (P_EQ, "!="),
        "lt" => (P_CMP, "<"),
        "le" => (P_CMP, "<="),
        "gt" => (P_CMP, ">"),
        "ge" => (P_CMP, ">="),
        "add" => (P_ADD, "+"),
        "sub" => (P_ADD, "-"),
        "mul" => (P_MUL, "*"),
        "div" => (P_MUL, "/"),
        "mod" => (P_MUL, "%"),
        _ => return None,
    })
}

/// The text of an expression and how tightly it binds.
fn expr_p(e: &Json) -> (String, u8) {
    match e {
        Json::Int(i) => (i.to_string(), if *i < 0 { P_UNARY } else { P_ATOM }),
        Json::Float(f) => {
            let t = float_text(*f);
            let neg = t.starts_with('-');
            (t, if neg { P_UNARY } else { P_ATOM })
        }
        Json::Bool(b) => (b.to_string(), P_ATOM),
        Json::Str(s) => (quote(s), P_ATOM),
        Json::Obj(o) if o.len() == 1 => {
            let (op, arg) = (&o[0].0, &o[0].1);
            let args: Option<&[Json]> = arg.as_array();
            match op.as_str() {
                "get" => match arg.as_str() {
                    Some(p) if is_path(p) && !p.starts_with("contact.") => (p.to_string(), P_ATOM),
                    _ => (raw(e), P_ATOM),
                },
                "var" => match arg.as_str() {
                    Some(n) if is_ident(n) && !is_reserved(n) => (n.to_string(), P_ATOM),
                    _ => (raw(e), P_ATOM),
                },
                "res" => match arg.as_str() {
                    Some(n) => (res_text(n), P_ATOM),
                    None => (raw(e), P_ATOM),
                },
                "dt" | "tick" if *arg == Json::Bool(true) => (op.clone(), P_ATOM),
                "uid" => match arg {
                    Json::Bool(true) => ("uid".to_string(), P_ATOM),
                    Json::Str(r) if r == "a" || r == "b" => (format!("uid({r})"), P_ATOM),
                    _ => (raw(e), P_ATOM),
                },
                "contact" => match arg.as_str() {
                    Some(k) if ["depth", "nx", "ny", "nz", "trigger"].contains(&k) => (format!("contact.{k}"), P_ATOM),
                    _ => (raw(e), P_ATOM),
                },
                "neg" | "not" if !matches!(arg, Json::Arr(_)) => {
                    let (inner, p) = expr_p(arg);
                    let numeric = matches!(arg, Json::Int(_) | Json::Float(_));
                    if op == "neg" && numeric {
                        // `-5` would read back as the number minus five, not as neg(5)
                        return (format!("neg({inner})"), P_ATOM);
                    }
                    let sym = if op == "neg" { "-" } else { "!" };
                    (if p >= P_ATOM { format!("{sym}{inner}") } else { format!("{sym}({inner})") }, P_UNARY)
                }
                name if SINGLE.contains(&name) => match arg {
                    Json::Arr(a) if a.len() == 1 => (format!("{name}({})", expr_p(&a[0]).0), P_ATOM),
                    Json::Arr(_) => (raw(e), P_ATOM),
                    one => (format!("{name}({})", expr_p(one).0), P_ATOM),
                },
                name if NARY.contains(&name) || BINARY.contains(&name) || TERNARY.contains(&name) => {
                    let Some(a) = args else { return (raw(e), P_ATOM) };
                    let want_ok = if NARY.contains(&name) {
                        !a.is_empty()
                    } else if BINARY.contains(&name) {
                        a.len() == 2
                    } else {
                        a.len() == 3
                    };
                    if !want_ok {
                        return (raw(e), P_ATOM);
                    }
                    if let Some((prec, sym)) = infix(name) {
                        let nary = NARY.contains(&name);
                        if a.len() >= 2 {
                            let mut parts = Vec::new();
                            for (i, x) in a.iter().enumerate() {
                                let (t, p) = expr_p(x);
                                let same_op = x.as_object().is_some_and(|xo| xo.len() == 1 && xo[0].0 == name);
                                // the first operand may sit at the same level (left to right); a later one, or the same n-ary
                                // operation that would be merged into this one, needs parentheses
                                let paren = p < prec || (p == prec && (i > 0 || (nary && same_op)));
                                parts.push(if paren { format!("({t})") } else { t });
                            }
                            return (parts.join(&format!(" {sym} ")), prec);
                        }
                    }
                    (format!("{name}({})", a.iter().map(|x| expr_p(x).0).collect::<Vec<_>>().join(", ")), P_ATOM)
                }
                _ => (raw(e), P_ATOM),
            }
        }
        _ => (raw(e), P_ATOM),
    }
}

pub fn expr(e: &Json, _min: u8) -> String {
    expr_p(e).0
}

/// A number the way the language reads it back as a float: always with a point or an exponent.
pub fn float_text(f: f64) -> String {
    if !f.is_finite() {
        return "0.0".to_string();
    }
    let a = f.abs();
    if a != 0.0 && !(1e-5..1e16).contains(&a) {
        let t = format!("{f:e}");
        return t;
    }
    let t = format!("{f}");
    if t.contains(['.', 'e']) {
        t
    } else {
        format!("{t}.0")
    }
}
