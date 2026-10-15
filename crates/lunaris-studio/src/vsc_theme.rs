//! Visual Studio Code colour themes: the files that VS Code itself reads (`colors` and `tokenColors`, with `include` chains), so that the
//! Script tab can look exactly the way VS Code looks, in any theme VS Code has. The four dark themes VS Code ships are built in (MIT
//! licence, `assets/vscode/LICENSE.txt`); the other themes of an installed VS Code and of its extensions are found on disk.
use crate::vsc_defaults::DARK_DEFAULTS;
use eframe::egui::Color32;
use lunaris_json::Json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const EMBEDDED: &[(&str, &str)] = &[
    ("dark_vs.json", include_str!("../assets/vscode/dark_vs.json")),
    ("dark_plus.json", include_str!("../assets/vscode/dark_plus.json")),
    ("dark_modern.json", include_str!("../assets/vscode/dark_modern.json")),
    ("2026-dark.json", include_str!("../assets/vscode/2026-dark.json")),
];

/// The theme VS Code starts with.
pub const DEFAULT_THEME: &str = "Dark 2026";

/// The built-in themes: (name, file).
const BUILT_IN: &[(&str, &str)] = &[("Dark 2026", "2026-dark.json"), ("Dark Modern", "dark_modern.json"), ("Dark+", "dark_plus.json"), ("Visual Studio Dark", "dark_vs.json")];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FontStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Clone, Debug)]
struct Rule {
    scope: String,
    fg: Option<Color32>,
    style: FontStyle,
}

#[derive(Clone, Debug, Default)]
pub struct VsTheme {
    pub name: String,
    pub dark: bool,
    colors: HashMap<String, Color32>,
    rules: Vec<Rule>,
}

/// `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA`.
pub fn parse_color(s: &str) -> Option<Color32> {
    let h = s.trim().strip_prefix('#')?;
    let v = |i: usize, n: usize| u8::from_str_radix(h.get(i..i + n)?, 16).ok();
    match h.len() {
        3 => Some(Color32::from_rgb(v(0, 1)? * 17, v(1, 1)? * 17, v(2, 1)? * 17)),
        4 => Some(Color32::from_rgba_unmultiplied(v(0, 1)? * 17, v(1, 1)? * 17, v(2, 1)? * 17, v(3, 1)? * 17)),
        6 => Some(Color32::from_rgb(v(0, 2)?, v(2, 2)?, v(4, 2)?)),
        8 => Some(Color32::from_rgba_unmultiplied(v(0, 2)?, v(2, 2)?, v(4, 2)?, v(6, 2)?)),
        _ => None,
    }
}

/// Theme files are JSON with comments and trailing commas (VS Code reads them as such): make them plain JSON.
pub fn strip_jsonc(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut in_str = false;
    while i < c.len() {
        let ch = c[i];
        if in_str {
            out.push(ch);
            if ch == '\\' && i + 1 < c.len() {
                out.push(c[i + 1]);
                i += 1;
            } else if ch == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match ch {
            '"' => {
                in_str = true;
                out.push(ch);
            }
            '/' if c.get(i + 1) == Some(&'/') => {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if c.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            ',' => {
                // a comma before a closing bracket is dropped
                let mut j = i + 1;
                while j < c.len() && c[j].is_whitespace() {
                    j += 1;
                }
                if !(j < c.len() && (c[j] == '}' || c[j] == ']')) {
                    out.push(ch);
                }
            }
            _ => out.push(ch),
        }
        i += 1;
    }
    out
}

fn defaults() -> &'static HashMap<&'static str, Color32> {
    static D: OnceLock<HashMap<&'static str, Color32>> = OnceLock::new();
    D.get_or_init(|| DARK_DEFAULTS.iter().filter_map(|(k, v)| parse_color(v).map(|c| (*k, c))).collect())
}

impl VsTheme {
    /// One of the four themes that are built in.
    pub fn built_in(name: &str) -> Option<VsTheme> {
        let file = BUILT_IN.iter().find(|(n, _)| *n == name).map(|(_, f)| *f)?;
        let mut t = VsTheme { name: name.to_string(), dark: true, ..VsTheme::default() };
        t.merge(EMBEDDED.iter().find(|(f, _)| *f == file).map(|(_, text)| *text)?, None, 0).ok()?;
        t.name = name.to_string();
        Some(t)
    }

    pub fn from_file(path: &Path, name: &str) -> Result<VsTheme, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut t = VsTheme { name: name.to_string(), dark: true, ..VsTheme::default() };
        t.merge(&text, path.parent(), 0)?;
        t.name = name.to_string();
        Ok(t)
    }

    /// Add what a theme file says, after what its `include` says.
    fn merge(&mut self, text: &str, dir: Option<&Path>, depth: usize) -> Result<(), String> {
        if depth > 6 {
            return Err("themes include each other too deeply".to_string());
        }
        let doc = lunaris_json::parse(&strip_jsonc(text)).map_err(|e| format!("not a theme file: {e}"))?;
        if let Some(inc) = doc.get("include").and_then(|i| i.as_str()) {
            let inc = inc.trim_start_matches("./");
            let from_disk = dir.map(|d| d.join(inc)).and_then(|p| std::fs::read_to_string(&p).ok().map(|t| (t, p)));
            match from_disk {
                Some((t, p)) => self.merge(&t, p.parent(), depth + 1)?,
                None => {
                    let t = EMBEDDED.iter().find(|(f, _)| *f == inc).map(|(_, t)| *t).ok_or_else(|| format!("the included theme {inc} was not found"))?;
                    self.merge(t, None, depth + 1)?;
                }
            }
        }
        if let Some(t) = doc.get("type").and_then(|t| t.as_str()) {
            self.dark = !matches!(t, "light" | "hc-light");
        }
        if let Some(colors) = doc.get("colors").and_then(|c| c.as_object()) {
            for (k, v) in colors {
                match v.as_str().and_then(parse_color) {
                    Some(c) => {
                        self.colors.insert(k.clone(), c);
                    }
                    None => {
                        // `null` or an unreadable value: the theme says nothing for this colour
                        if v.is_null() {
                            self.colors.remove(k);
                        }
                    }
                }
            }
        }
        if let Some(rules) = doc.get("tokenColors").and_then(|r| r.as_array()) {
            for r in rules {
                let Some(settings) = r.get("settings") else { continue };
                let fg = settings.get("foreground").and_then(|f| f.as_str()).and_then(parse_color);
                let fs = settings.get("fontStyle").and_then(|f| f.as_str()).unwrap_or("");
                let style = FontStyle { bold: fs.contains("bold"), italic: fs.contains("italic"), underline: fs.contains("underline") };
                if fg.is_none() && fs.is_empty() {
                    continue;
                }
                let scopes: Vec<String> = match r.get("scope") {
                    Some(Json::Str(s)) => s.split(',').map(|x| x.trim().to_string()).collect(),
                    Some(Json::Arr(a)) => a.iter().filter_map(|x| x.as_str()).flat_map(|x| x.split(',').map(|y| y.trim().to_string()).collect::<Vec<_>>()).collect(),
                    _ => vec![String::new()],
                };
                for scope in scopes {
                    // selectors with spaces (descendants, exclusions) are not used
                    if scope.contains(' ') {
                        continue;
                    }
                    self.rules.push(Rule { scope, fg, style });
                }
            }
        }
        Ok(())
    }

    /// The colour of a workbench key, from the theme or else VS Code's own default.
    pub fn color(&self, key: &str) -> Option<Color32> {
        self.colors.get(key).copied().or_else(|| defaults().get(key).copied())
    }

    /// The first key of the list that has a colour.
    pub fn first(&self, keys: &[&str]) -> Option<Color32> {
        keys.iter().find_map(|k| self.color(k))
    }

    /// The colour and style the theme gives a TextMate scope: the rule with the longest matching scope wins (the later one on a tie).
    pub fn token(&self, scope: &str) -> Option<(Color32, FontStyle)> {
        let mut best: Option<&Rule> = None;
        for r in &self.rules {
            let hit = r.scope.is_empty() || r.scope == scope || (scope.starts_with(&r.scope) && scope.as_bytes().get(r.scope.len()) == Some(&b'.'));
            if hit && r.fg.is_some() && best.is_none_or(|b| r.scope.len() >= b.scope.len()) {
                best = Some(r);
            }
        }
        best.and_then(|r| r.fg.map(|c| (c, r.style)))
    }

    /// Try the scopes in order.
    pub fn token_of(&self, scopes: &[&str]) -> Option<(Color32, FontStyle)> {
        scopes.iter().find_map(|s| self.token(s))
    }
}

/// A theme that can be chosen.
#[derive(Clone, Debug)]
pub struct ThemeInfo {
    pub label: String,
    /// `None` for a built-in one.
    pub path: Option<PathBuf>,
}

fn nls(dir: &Path) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Ok(t) = std::fs::read_to_string(dir.join("package.nls.json")) {
        if let Ok(j) = lunaris_json::parse(&strip_jsonc(&t)) {
            for (k, v) in j.as_object().unwrap_or(&[]) {
                if let Some(s) = v.as_str() {
                    m.insert(k.clone(), s.to_string());
                }
            }
        }
    }
    m
}

fn themes_of(dir: &Path, out: &mut Vec<ThemeInfo>) {
    let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else { return };
    let Ok(pkg) = lunaris_json::parse(&strip_jsonc(&text)) else { return };
    let strings = nls(dir);
    for t in pkg.get("contributes").and_then(|c| c.get("themes")).and_then(|t| t.as_array()).unwrap_or(&[]) {
        let ui = t.get("uiTheme").and_then(|u| u.as_str()).unwrap_or("vs-dark");
        if ui == "vs" || ui == "hc-light" {
            continue;
        }
        let (Some(label), Some(path)) = (t.get("label").and_then(|l| l.as_str()).or_else(|| t.get("id").and_then(|l| l.as_str())), t.get("path").and_then(|p| p.as_str())) else { continue };
        let label = match label.strip_prefix('%').and_then(|l| l.strip_suffix('%')) {
            Some(key) => strings.get(key).cloned().unwrap_or_else(|| label.to_string()),
            None => label.to_string(),
        };
        out.push(ThemeInfo { label, path: Some(dir.join(path.trim_start_matches("./"))) });
    }
}

/// Every dark theme that can be used: the built-in ones, the ones of an installed VS Code, and those of extensions installed in it.
pub fn discover() -> Vec<ThemeInfo> {
    let mut out: Vec<ThemeInfo> = BUILT_IN.iter().map(|(n, _)| ThemeInfo { label: n.to_string(), path: None }).collect();
    let mut dirs: Vec<PathBuf> = Vec::new();
    // an installed VS Code (newest first), then the extensions folders
    let mut installs: Vec<PathBuf> = Vec::new();
    for base in [std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Programs").join("Microsoft VS Code")), Some(PathBuf::from("C:\\Program Files\\Microsoft VS Code"))].into_iter().flatten() {
        if let Ok(rd) = std::fs::read_dir(&base) {
            for e in rd.flatten() {
                let ext = e.path().join("resources").join("app").join("extensions");
                if ext.is_dir() {
                    installs.push(ext);
                }
            }
        }
        let ext = base.join("resources").join("app").join("extensions");
        if ext.is_dir() {
            installs.push(ext);
        }
    }
    installs.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    if let Some(newest) = installs.last() {
        if let Ok(rd) = std::fs::read_dir(newest) {
            dirs.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        if let Ok(rd) = std::fs::read_dir(PathBuf::from(home).join(".vscode").join("extensions")) {
            dirs.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    dirs.sort();
    for d in dirs {
        let mut found = Vec::new();
        themes_of(&d, &mut found);
        for t in found {
            if !out.iter().any(|o| o.label == t.label) {
                out.push(t);
            }
        }
    }
    out
}

/// Load a theme by its name (built in, or found by `discover`); the default one when it cannot be read.
pub fn load(name: &str) -> VsTheme {
    if let Some(t) = VsTheme::built_in(name) {
        return t;
    }
    if let Some(info) = discover().into_iter().find(|t| t.label == name) {
        if let Some(p) = &info.path {
            if let Ok(t) = VsTheme::from_file(p, name) {
                return t;
            }
        }
    }
    VsTheme::built_in(DEFAULT_THEME).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_come_in_every_form_vs_code_allows() {
        assert_eq!(parse_color("#1E1E1E"), Some(Color32::from_rgb(30, 30, 30)));
        assert_eq!(parse_color("#fff"), Some(Color32::from_rgb(255, 255, 255)));
        assert_eq!(parse_color("#888"), Some(Color32::from_rgb(136, 136, 136)));
        assert_eq!(parse_color("#FFFFFF14"), Some(Color32::from_rgba_unmultiplied(255, 255, 255, 0x14)));
        assert_eq!(parse_color("#0000"), Some(Color32::from_rgba_unmultiplied(0, 0, 0, 0)));
        assert_eq!(parse_color("red"), None);
        assert_eq!(parse_color("#12"), None);
    }

    #[test]
    fn comments_and_trailing_commas_are_removed_but_not_inside_text() {
        let src = "{ // line\n \"a\": \"x // not a comment, ]\", /* block */ \"b\": [1, 2,], }";
        let j = lunaris_json::parse(&strip_jsonc(src)).unwrap();
        assert_eq!(j.get("a").unwrap().as_str(), Some("x // not a comment, ]"));
        assert_eq!(j.get("b").unwrap().as_array().unwrap().len(), 2);
    }

    #[test]
    fn dark_2026_is_dark_modern_and_dark_plus_and_dark_vs_underneath() {
        let t = VsTheme::built_in("Dark 2026").unwrap();
        // from the 2026 file itself
        assert_eq!(t.color("editor.background"), parse_color("#121314"));
        // not in the 2026 file: from Dark Modern
        assert!(t.color("activityBar.activeBorder").is_some());
        // a colour nobody sets comes from VS Code's own defaults
        assert_eq!(t.color("editorCursor.foreground"), parse_color("#BBBEBF"));
        assert_eq!(t.color("editorBracketMatch.background"), parse_color("#3994BC55"));
        let modern = VsTheme::built_in("Dark Modern").unwrap();
        assert_eq!(modern.color("editor.background"), parse_color("#1F1F1F"));
        let vs = VsTheme::built_in("Visual Studio Dark").unwrap();
        assert_eq!(vs.color("editor.background"), parse_color("#1E1E1E"));
        assert!(t.dark);
    }

    #[test]
    fn the_longest_matching_scope_gives_a_token_its_colour() {
        let plus = VsTheme::built_in("Dark+").unwrap();
        // Dark+: keywords are blue, control keywords purple, functions yellow, numbers light green
        let kw = plus.token("keyword").unwrap().0;
        let ctl = plus.token("keyword.control").unwrap().0;
        assert_eq!(ctl, parse_color("#C586C0").unwrap());
        assert_ne!(kw, ctl);
        assert_eq!(plus.token("entity.name.function").unwrap().0, parse_color("#DCDCAA").unwrap());
        assert_eq!(plus.token("constant.numeric").unwrap().0, parse_color("#B5CEA8").unwrap());
        // a scope that is not in the theme falls back to its parent's rule
        assert_eq!(plus.token("keyword.control.flow").unwrap().0, ctl);
        assert_eq!(plus.token("comment.line.double-slash").unwrap().0, plus.token("comment").unwrap().0);
        assert!(plus.token("nothing.at.all").is_none());
    }

    #[test]
    fn a_theme_can_be_found_by_name_and_unknown_names_give_the_default() {
        assert_eq!(load("Dark Modern").name, "Dark Modern");
        assert_eq!(load("No Such Theme At All").name, DEFAULT_THEME);
        assert!(discover().iter().any(|t| t.label == "Dark 2026"));
    }
}
