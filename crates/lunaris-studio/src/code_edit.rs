//! The text-editing behaviour of the script editor, as plain functions on text: what Enter, Tab, `/` with Ctrl, a typed bracket or Backspace
//! do, which words to offer while typing, and where a search string is. Positions are character indices, as egui's cursors are.
//! They are separate from the window code so that they can be tested without one.
use lunaris_script as script;

pub const INDENT: &str = "    ";

#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub text: String,
    pub anchor: usize,
    pub head: usize,
}

fn chars(text: &str) -> Vec<char> {
    text.chars().collect()
}

fn string(c: &[char]) -> String {
    c.iter().collect()
}

fn line_start(c: &[char], mut i: usize) -> usize {
    while i > 0 && c[i - 1] != '\n' {
        i -= 1;
    }
    i
}

fn line_end(c: &[char], mut i: usize) -> usize {
    while i < c.len() && c[i] != '\n' {
        i += 1;
    }
    i
}

fn indent_len(c: &[char], ls: usize) -> usize {
    c[ls..].iter().take_while(|ch| **ch == ' ' || **ch == '\t').count()
}

/// Replace `[from, to)` with `with` and put the cursor after it.
fn splice(c: &[char], from: usize, to: usize, with: &str) -> Edit {
    let mut out: Vec<char> = c[..from].to_vec();
    out.extend(with.chars());
    let at = out.len();
    out.extend_from_slice(&c[to..]);
    Edit { text: string(&out), anchor: at, head: at }
}

/// (line, column) of a character index, both 0-based; the column counts characters.
pub fn line_col(text: &str, at: usize) -> (usize, usize) {
    let (mut line, mut col) = (0, 0);
    for (i, ch) in text.chars().enumerate() {
        if i >= at {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// The character index of a 0-based line and column (clamped to the line).
pub fn index_of(text: &str, line: usize, col: usize) -> usize {
    let (mut l, mut i) = (0, 0);
    let c = chars(text);
    while l < line && i < c.len() {
        if c[i] == '\n' {
            l += 1;
        }
        i += 1;
    }
    let end = line_end(&c, i.min(c.len()));
    (i + col).min(end)
}

pub fn byte_of(text: &str, char_index: usize) -> usize {
    text.char_indices().nth(char_index).map(|(b, _)| b).unwrap_or(text.len())
}

pub fn char_of(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

// ---------------------------------------------------------------------------------------------------------------- keys

/// Enter: a new line with the indentation of this one, one level deeper after `{`, and between `{` and `}` the closing brace moves to a
/// line of its own.
pub fn enter(text: &str, anchor: usize, head: usize) -> Edit {
    let c = chars(text);
    let (a, b) = (anchor.min(head).min(c.len()), anchor.max(head).min(c.len()));
    let ls = line_start(&c, a);
    let n = indent_len(&c, ls).min(a - ls);
    let indent = string(&c[ls..ls + n]);
    let before = if a > 0 { Some(c[a - 1]) } else { None };
    let after = c.get(b).copied();
    if before == Some('{') {
        if after == Some('}') {
            let mut with = format!("\n{indent}{INDENT}");
            let cursor = a + with.chars().count();
            with.push_str(&format!("\n{indent}"));
            let mut e = splice(&c, a, b, &with);
            e.anchor = cursor;
            e.head = cursor;
            return e;
        }
        return splice(&c, a, b, &format!("\n{indent}{INDENT}"));
    }
    splice(&c, a, b, &format!("\n{indent}"))
}

/// Tab and Shift+Tab: indent or dedent the lines of a selection; with no selection Tab goes to the next multiple of four columns.
pub fn tab(text: &str, anchor: usize, head: usize, shift: bool) -> Edit {
    let c = chars(text);
    let (a, b) = (anchor.min(head).min(c.len()), anchor.max(head).min(c.len()));
    let multi = c[a..b].contains(&'\n');
    if !shift && !multi {
        let col = a - line_start(&c, a);
        let n = 4 - col % 4;
        return splice(&c, a, b, &" ".repeat(n));
    }
    // the lines touched: a selection that ends at the very start of a line does not touch that line
    let first = line_start(&c, a);
    let last_pos = if b > a && b > 0 && c[b - 1] == '\n' { b - 1 } else { b };
    let last = line_end(&c, last_pos);
    let mut out: Vec<char> = c[..first].to_vec();
    let (mut na, mut nb) = (anchor, head);
    let mut shifts: Vec<(usize, isize)> = Vec::new(); // (position in the old text, change from here on)
    let mut i = first;
    loop {
        let le = line_end(&c, i);
        if shift {
            let n = c[i..le].iter().take(4).take_while(|ch| **ch == ' ').count();
            let n = if n == 0 && c.get(i) == Some(&'\t') { 1 } else { n };
            out.extend_from_slice(&c[i + n..le]);
            if n > 0 {
                shifts.push((i, -(n as isize)));
            }
        } else if le > i {
            out.extend(INDENT.chars());
            out.extend_from_slice(&c[i..le]);
            shifts.push((i, INDENT.len() as isize));
        } else {
            out.extend_from_slice(&c[i..le]);
        }
        if le >= last {
            break;
        }
        out.push('\n');
        i = le + 1;
    }
    out.extend_from_slice(&c[last..]);
    let adjust = |p: usize| -> usize {
        let mut q = p as isize;
        for (at, d) in &shifts {
            if p > *at || (p == *at && *d > 0) {
                q += if *d < 0 { -((*d).abs().min((p - at) as isize)) } else { *d };
            }
        }
        q.max(0) as usize
    };
    na = adjust(na);
    nb = adjust(nb);
    Edit { text: string(&out), anchor: na, head: nb }
}

/// Ctrl+/: comment or uncomment the lines of the selection (all of them are commented unless every one already is).
pub fn toggle_comment(text: &str, anchor: usize, head: usize) -> Edit {
    let c = chars(text);
    let (a, b) = (anchor.min(head).min(c.len()), anchor.max(head).min(c.len()));
    let first = line_start(&c, a);
    let last_pos = if b > a && b > 0 && c[b - 1] == '\n' { b - 1 } else { b };
    let last = line_end(&c, last_pos);
    // the lines
    let mut lines: Vec<(usize, usize)> = Vec::new();
    let mut i = first;
    loop {
        let le = line_end(&c, i);
        lines.push((i, le));
        if le >= last {
            break;
        }
        i = le + 1;
    }
    let body = |&(s, e): &(usize, usize)| -> Option<usize> {
        let n = indent_len(&c, s).min(e - s);
        (s + n < e).then_some(s + n)
    };
    let all_commented = lines.iter().filter_map(body).all(|p| c.get(p) == Some(&'/') && c.get(p + 1) == Some(&'/'));
    let min_indent = lines.iter().filter_map(|l| body(l).map(|p| p - l.0)).min().unwrap_or(0);
    let mut out: Vec<char> = c[..first].to_vec();
    let mut shifts: Vec<(usize, isize)> = Vec::new();
    for (k, &(s, e)) in lines.iter().enumerate() {
        match body(&(s, e)) {
            Some(p) if all_commented => {
                let mut n = 2;
                if c.get(p + 2) == Some(&' ') {
                    n = 3;
                }
                out.extend_from_slice(&c[s..p]);
                out.extend_from_slice(&c[p + n..e]);
                shifts.push((p, -(n as isize)));
            }
            Some(_) => {
                out.extend_from_slice(&c[s..s + min_indent]);
                out.extend("// ".chars());
                out.extend_from_slice(&c[s + min_indent..e]);
                shifts.push((s + min_indent, 3));
            }
            None => out.extend_from_slice(&c[s..e]),
        }
        if k + 1 < lines.len() {
            out.push('\n');
        }
    }
    out.extend_from_slice(&c[last..]);
    let adjust = |p: usize| -> usize {
        let mut q = p as isize;
        for (at, d) in &shifts {
            if p > *at || (p == *at && *d > 0) {
                q += if *d < 0 { -((*d).abs().min((p - at) as isize)) } else { *d };
            }
        }
        q.max(0) as usize
    };
    Edit { text: string(&out), anchor: adjust(anchor), head: adjust(head) }
}

fn closer_of(open: char) -> Option<char> {
    Some(match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '"' => '"',
        _ => return None,
    })
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// A typed character that the editor handles itself: an opening bracket or quote adds its partner (or wraps the selection), a closing
/// one that is already there is stepped over. `None`: let the text field insert the character.
pub fn typed(text: &str, anchor: usize, head: usize, ch: char) -> Option<Edit> {
    let c = chars(text);
    let (a, b) = (anchor.min(head).min(c.len()), anchor.max(head).min(c.len()));
    if let Some(close) = closer_of(ch) {
        if a != b {
            let mut out: Vec<char> = c[..a].to_vec();
            out.push(ch);
            out.extend_from_slice(&c[a..b]);
            out.push(close);
            out.extend_from_slice(&c[b..]);
            return Some(Edit { text: string(&out), anchor: a + 1, head: b + 1 });
        }
        // typing the closing quote over the one that is there
        if ch == '"' && c.get(a) == Some(&'"') {
            return Some(Edit { text: text.to_string(), anchor: a + 1, head: a + 1 });
        }
        let next = c.get(a).copied();
        let prev = if a > 0 { Some(c[a - 1]) } else { None };
        let next_ok = next.is_none_or(|n| n.is_whitespace() || matches!(n, ')' | ']' | '}' | ',' | ';'));
        let quote_ok = ch != '"' || !prev.is_some_and(|p| is_word_char(p) || p == '"' || p == '\\');
        // inside a comment nothing is closed
        let ls = line_start(&c, a);
        let in_comment = string(&c[ls..a]).contains("//");
        if next_ok && quote_ok && !in_comment {
            let mut out: Vec<char> = c[..a].to_vec();
            out.push(ch);
            out.push(close);
            out.extend_from_slice(&c[a..]);
            return Some(Edit { text: string(&out), anchor: a + 1, head: a + 1 });
        }
        return None;
    }
    if matches!(ch, ')' | ']' | '}') && a == b && c.get(a) == Some(&ch) {
        return Some(Edit { text: text.to_string(), anchor: a + 1, head: a + 1 });
    }
    None
}

/// Backspace between an opening bracket and its closing partner removes both.
pub fn backspace(text: &str, anchor: usize, head: usize) -> Option<Edit> {
    if anchor != head {
        return None;
    }
    let c = chars(text);
    let a = head.min(c.len());
    if a == 0 || a >= c.len() {
        return None;
    }
    if closer_of(c[a - 1]) == Some(c[a]) {
        return Some(splice(&c, a - 1, a + 1, ""));
    }
    None
}

// ---------------------------------------------------------------------------------------------------------------- completion

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Keyword,
    Function,
    Component,
    Field,
    Variable,
    Resource,
    Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub insert: String,
    pub kind: Kind,
    pub detail: String,
}

/// What is being typed at the cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    /// Character index where the word starts.
    pub start: usize,
    pub prefix: String,
    /// The dotted chain before the word: `Transform` for `Transform.po`.
    pub before_dot: Option<String>,
    pub dollar: bool,
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// The word under the cursor, or `None` inside a comment or a string.
pub fn word_at(text: &str, at: usize) -> Option<Word> {
    let c = chars(text);
    let at = at.min(c.len());
    let ls = line_start(&c, at);
    let line = &c[ls..at];
    // inside a // comment, or an unclosed string on this line
    let mut in_str = false;
    let mut i = 0;
    while i < line.len() {
        if !in_str && line[i] == '/' && line.get(i + 1) == Some(&'/') {
            return None;
        }
        if line[i] == '"' && (i == 0 || line[i - 1] != '\\') {
            in_str = !in_str;
        }
        i += 1;
    }
    if in_str {
        return None;
    }
    // a resource: `$input.sp`, the dots are part of its name
    let mut t0 = at;
    while t0 > ls && (is_ident_char(c[t0 - 1]) || c[t0 - 1] == '.') {
        t0 -= 1;
    }
    if t0 > ls && c[t0 - 1] == '$' {
        return Some(Word { start: t0, prefix: string(&c[t0..at]), before_dot: None, dollar: true });
    }
    let mut s = at;
    while s > ls && is_ident_char(c[s - 1]) {
        s -= 1;
    }
    let prefix = string(&c[s..at]);
    let dollar = false;
    let mut before_dot = None;
    if s > ls && c[s - 1] == '.' {
        let mut t = s - 1;
        while t > ls && (is_ident_char(c[t - 1]) || c[t - 1] == '.' || c[t - 1] == ']' || c[t - 1] == '[') {
            t -= 1;
        }
        before_dot = Some(string(&c[t..s - 1]));
    }
    Some(Word { start: s, prefix, before_dot, dollar })
}

/// Words to offer for `word`. `fields` gives the fields of a component (name, type text), `components` and `resources` are the lists the
/// world has.
pub fn suggest(text: &str, word: &Word, components: &[String], fields: &dyn Fn(&str) -> Vec<(String, String)>, resources: &[String]) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let p = word.prefix.to_lowercase();
    let mut add = |label: &str, insert: &str, kind: Kind, detail: &str| {
        let l = label.to_lowercase();
        let score = if p.is_empty() || l.starts_with(&p) {
            0
        } else if l.contains(&p) {
            1
        } else {
            return;
        };
        if label == word.prefix {
            return;
        }
        out.push(Item { label: label.to_string(), insert: insert.to_string(), kind, detail: format!("{}{}", if score == 1 { "~" } else { "" }, detail) });
    };
    if word.dollar {
        for r in resources {
            add(r, r, Kind::Resource, "resource");
        }
        return finish(out);
    }
    if let Some(chain) = &word.before_dot {
        let comp = chain.rsplit('.').next().unwrap_or(chain);
        if comp == "contact" {
            for k in ["depth", "nx", "ny", "nz", "trigger"] {
                add(k, k, Kind::Field, "the contact");
            }
            return finish(out);
        }
        // `a.Transform` / `b.`: the roles of a contact system lead to components
        if chain == "a" || chain == "b" {
            for n in components {
                add(n, n, Kind::Component, "component");
            }
            return finish(out);
        }
        for (name, ty) in fields(comp) {
            add(&name, &name, Kind::Field, &ty);
        }
        return finish(out);
    }
    for k in ["let", "if", "else", "despawn", "emit", "spawn", "spawn_prefab", "add", "remove", "system", "each", "without", "on_contact", "order", "disabled", "at", "velocity", "with", "on", "triggers"] {
        add(k, k, Kind::Keyword, "keyword");
    }
    for k in ["true", "false", "dt", "tick", "uid"] {
        add(k, k, Kind::Value, "value");
    }
    for f in script::call_names() {
        let (shape, insert) = if script::SINGLE.contains(&f) {
            ("(x)", format!("{f}()"))
        } else if script::BINARY.contains(&f) {
            ("(a, b)", format!("{f}()"))
        } else if script::TERNARY.contains(&f) {
            ("(a, b, c)", format!("{f}()"))
        } else {
            ("(a, b, ...)", format!("{f}()"))
        };
        add(f, &insert, Kind::Function, shape);
    }
    for n in components {
        add(n, n, Kind::Component, "component");
    }
    // local variables declared before the cursor
    let mut seen: Vec<String> = Vec::new();
    let words: Vec<&str> = text.split(|ch: char| !is_ident_char(ch)).filter(|w| !w.is_empty()).collect();
    for w in words.windows(2) {
        if w[0] == "let" && !seen.iter().any(|s| s == w[1]) && !script::is_reserved(w[1]) {
            seen.push(w[1].to_string());
        }
    }
    for v in seen {
        add(&v, &v, Kind::Variable, "variable");
    }
    finish(out)
}

fn finish(mut out: Vec<Item>) -> Vec<Item> {
    // prefix matches first, then by kind, then by name
    out.sort_by(|a, b| {
        let (ta, tb) = (a.detail.starts_with('~'), b.detail.starts_with('~'));
        ta.cmp(&tb).then(a.label.len().cmp(&b.label.len())).then(a.label.cmp(&b.label))
    });
    out.truncate(14);
    out
}

/// Accept an item: replace the word being typed with its text. The cursor goes inside `()` when the item ends with it.
pub fn accept(text: &str, word: &Word, cursor: usize, item: &Item) -> Edit {
    let c = chars(text);
    let end = cursor.min(c.len());
    let mut e = splice(&c, word.start, end, &item.insert);
    if item.insert.ends_with("()") {
        e.anchor -= 1;
        e.head -= 1;
    }
    e
}

// ---------------------------------------------------------------------------------------------------------------- search

/// Where `needle` is in `text`, as character ranges.
pub fn find_all(text: &str, needle: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return vec![];
    }
    let hay: Vec<char> = if case_sensitive { chars(text) } else { text.to_lowercase().chars().collect() };
    let n: Vec<char> = if case_sensitive { chars(needle) } else { needle.to_lowercase().chars().collect() };
    if hay.len() != chars(text).len() {
        // lower-casing changed the length (rare letters): search the original
        return find_all(text, needle, true);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i + n.len() <= hay.len() {
        if hay[i..i + n.len()] == n[..] {
            out.push((i, i + n.len()));
            i += n.len();
        } else {
            i += 1;
        }
    }
    out
}

/// The matching bracket of the one at (or just before) the cursor, as character indices: (the bracket, its partner).
pub fn matching_bracket(text: &str, at: usize) -> Option<(usize, usize)> {
    let c = chars(text);
    let spans = script::highlight(text);
    // brackets inside comments and strings do not count
    let mut skip = vec![false; c.len()];
    for s in spans {
        if matches!(s.class, script::Class::Comment | script::Class::String) {
            for ci in char_of(text, s.start)..char_of(text, s.end) {
                if ci < skip.len() {
                    skip[ci] = true;
                }
            }
        }
    }
    let try_at = |i: usize| -> Option<(usize, usize)> {
        let ch = *c.get(i)?;
        if skip[i] {
            return None;
        }
        let (open, close, dir): (char, char, isize) = match ch {
            '(' => ('(', ')', 1),
            '[' => ('[', ']', 1),
            '{' => ('{', '}', 1),
            ')' => ('(', ')', -1),
            ']' => ('[', ']', -1),
            '}' => ('{', '}', -1),
            _ => return None,
        };
        let mut depth = 0;
        let mut j = i as isize;
        while j >= 0 && (j as usize) < c.len() {
            let u = j as usize;
            if !skip[u] {
                if c[u] == open {
                    depth += dir;
                } else if c[u] == close {
                    depth -= dir;
                }
                if depth == 0 {
                    return Some((i, u));
                }
            }
            j += dir;
        }
        None
    };
    try_at(at).or_else(|| if at > 0 { try_at(at - 1) } else { None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_keeps_the_indentation_and_opens_a_block() {
        let ed = enter("    a = 1;", 10, 10);
        assert_eq!(ed.text, "    a = 1;\n    ");
        assert_eq!((ed.anchor, ed.head), (15, 15));
        let ed = enter("if x {", 6, 6);
        assert_eq!(ed.text, "if x {\n    ");
        // between braces the closing one goes to its own line and the cursor to the middle
        let ed = enter("  if x {}", 8, 8);
        assert_eq!(ed.text, "  if x {\n      \n  }");
        assert_eq!(ed.head, 8 + 1 + 6);
        // a selection is replaced
        let ed = enter("ab cd", 1, 4);
        assert_eq!(ed.text, "a\nd");
    }

    #[test]
    fn tab_inserts_to_the_next_stop_and_indents_whole_lines() {
        assert_eq!(tab("ab", 2, 2, false).text, "ab  ");
        assert_eq!(tab("", 0, 0, false).text, "    ");
        let ed = tab("a\nb\nc", 0, 5, false);
        assert_eq!(ed.text, "    a\n    b\n    c");
        assert_eq!((ed.anchor, ed.head), (4, 17));
        let ed = tab("    a\n  b\nc", 0, 12, true);
        assert_eq!(ed.text, "a\nb\nc");
        // a selection that ends at the start of a line leaves that line alone
        let ed = tab("a\nb", 0, 2, false);
        assert_eq!(ed.text, "    a\nb");
        // dedent of one line with the cursor in it
        let ed = tab("      x", 7, 7, true);
        assert_eq!(ed.text, "  x");
        assert_eq!(ed.head, 3);
    }

    #[test]
    fn ctrl_slash_comments_and_uncomments() {
        let ed = toggle_comment("a = 1;\n  b = 2;", 0, 15);
        assert_eq!(ed.text, "// a = 1;\n//   b = 2;");
        let back = toggle_comment(&ed.text, 0, ed.text.chars().count());
        assert_eq!(back.text, "a = 1;\n  b = 2;");
        // a single line, cursor stays with its text
        let ed = toggle_comment("x = 1;", 3, 3);
        assert_eq!(ed.text, "// x = 1;");
        assert_eq!(ed.head, 6);
        // blank lines are left alone
        let ed = toggle_comment("a\n\nb", 0, 4);
        assert_eq!(ed.text, "// a\n\n// b");
    }

    #[test]
    fn brackets_close_and_are_stepped_over() {
        let ed = typed("f", 1, 1, '(').unwrap();
        assert_eq!((ed.text.as_str(), ed.head), ("f()", 2));
        // typing the closer over the one that is there
        let ed = typed("f()", 2, 2, ')').unwrap();
        assert_eq!((ed.text.as_str(), ed.head), ("f()", 3));
        // not before a word: `(` in front of text stays alone
        assert!(typed("ab", 0, 0, '(').is_none());
        // a selection is wrapped
        let ed = typed("abc", 1, 2, '[').unwrap();
        assert_eq!((ed.text.as_str(), ed.anchor, ed.head), ("a[b]c", 2, 3));
        // quotes: not after a letter, not in a comment
        assert!(typed("don", 3, 3, '"').is_none());
        assert!(typed("// say ", 7, 7, '(').is_none());
        let ed = typed("emit ", 5, 5, '"').unwrap();
        assert_eq!(ed.text, "emit \"\"");
        let ed = typed("emit \"\"", 6, 6, '"').unwrap();
        assert_eq!(ed.head, 7);
        // backspace between the pair removes both
        let ed = backspace("f()", 2, 2).unwrap();
        assert_eq!(ed.text, "f");
        assert!(backspace("f(x)", 3, 3).is_none());
    }

    #[test]
    fn words_under_the_cursor() {
        let w = word_at("Transform.po", 12).unwrap();
        assert_eq!((w.prefix.as_str(), w.before_dot.as_deref(), w.start), ("po", Some("Transform"), 10));
        let w = word_at("x = $input.sp", 13).unwrap();
        assert!(w.dollar && w.prefix == "input.sp" && w.start == 5, "{w:?}");
        let w = word_at("x = $sc", 7).unwrap();
        assert!(w.dollar && w.prefix == "sc");
        assert!(word_at("// com", 6).is_none());
        assert!(word_at("emit \"ab", 8).is_none());
        let w = word_at("a.Transform.pos[1] = cl", 23).unwrap();
        assert_eq!(w.prefix, "cl");
        assert!(w.before_dot.is_none());
    }

    #[test]
    fn suggestions_know_components_fields_functions_and_locals() {
        let comps = vec!["Transform".to_string(), "Velocity".to_string()];
        let fields = |c: &str| -> Vec<(String, String)> {
            if c == "Transform" {
                vec![("pos".into(), "f32[3]".into()), ("rot".into(), "f32[4]".into())]
            } else {
                vec![]
            }
        };
        let text = "let speed = 1;\nTransform.po";
        let w = word_at(text, text.chars().count()).unwrap();
        let items = suggest(text, &w, &comps, &fields, &[]);
        assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), vec!["pos"]);
        let text = "let speed = 1;\nx = cl";
        let w = word_at(text, text.chars().count()).unwrap();
        let items = suggest(text, &w, &comps, &fields, &[]);
        assert!(items.iter().any(|i| i.label == "clamp" && i.insert == "clamp()"), "{items:?}");
        let text = "let speed = 1;\nx = sp";
        let w = word_at(text, text.chars().count()).unwrap();
        let items = suggest(text, &w, &comps, &fields, &[]);
        assert!(items.iter().any(|i| i.label == "speed" && i.kind == Kind::Variable), "{items:?}");
        let w = word_at("$sc", 3).unwrap();
        let items = suggest("$sc", &w, &comps, &fields, &["score".into(), "time".into()]);
        assert_eq!(items.len(), 1);
        // accepting replaces the word, and a call puts the cursor between the parentheses
        let text = "x = cl";
        let w = word_at(text, 6).unwrap();
        let clamp = Item { label: "clamp".into(), insert: "clamp()".into(), kind: Kind::Function, detail: String::new() };
        let ed = accept(text, &w, 6, &clamp);
        assert_eq!((ed.text.as_str(), ed.head), ("x = clamp()", 10));
    }

    #[test]
    fn search_and_brackets() {
        assert_eq!(find_all("Abab AB", "ab", false), vec![(0, 2), (2, 4), (5, 7)]);
        assert_eq!(find_all("Abab AB", "ab", true), vec![(2, 4)]);
        assert!(find_all("x", "", false).is_empty());
        assert_eq!(matching_bracket("f(a[1], {b})", 1), Some((1, 11)));
        assert_eq!(matching_bracket("f(a[1], {b})", 12), Some((11, 1)));
        assert_eq!(matching_bracket("// ( x", 3), None);
        assert_eq!(line_col("ab\ncd", 4), (1, 1));
        assert_eq!(index_of("ab\ncd", 1, 1), 4);
        assert_eq!(index_of("ab\ncd", 0, 99), 2);
    }
}
