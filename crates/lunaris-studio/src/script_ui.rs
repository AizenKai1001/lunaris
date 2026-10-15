//! The Script tab: a code editor in the look of Visual Studio Code, for Lunaris Script (`crates/lunaris-script`).
//!
//! Tabs of open files, line numbers, syntax colours, a minimap, squiggles under mistakes, a Problems and an Output panel, a find box, a
//! command palette (Ctrl+Shift+P) and quick open (Ctrl+P), completion while typing, bracket pairs, auto-indent, Ctrl+/ comments.
//! A script is saved in `scripts/<name>.lun` of the project and applied to the world with `define_system` (Ctrl+S); what the engine finds
//! wrong with it is shown at the place in the text it belongs to.
use crate::code_edit::{self as ce, Item, Kind};
use crate::state::{LogKind, Studio};
use crate::vsc_theme::{self, VsTheme};
use eframe::egui::{
    self, text::{CCursor, CCursorRange, LayoutJob, TextFormat}, Align, Align2, Color32, CornerRadius, Event, FontId, Id, Key, Modifiers, Pos2, Rect, Sense, Shape, Stroke, TextEdit, Ui, UiBuilder, Vec2,
};
use lunaris_json::Json;
use lunaris_script as script;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// The colours of the editor, taken from a Visual Studio Code theme (`vsc_theme.rs`): the same keys VS Code reads.
#[derive(Clone)]
struct Pal {
    bg: Color32,
    sidebar: Color32,
    activity: Color32,
    activity_fg: Color32,
    activity_dim: Color32,
    tab_bar: Color32,
    tab_inactive: Color32,
    tab_sep: Color32,
    tab_top: Color32,
    tab_active_fg: Color32,
    tab_inactive_fg: Color32,
    edge: Color32,
    status: Color32,
    status_fg: Color32,
    status_hover: Color32,
    text: Color32,
    code: Color32,
    dim: Color32,
    bright: Color32,
    line_no: Color32,
    line_no_active: Color32,
    hover: Color32,
    list_select: Color32,
    row_select: Color32,
    selection: Color32,
    error: Color32,
    widget_bg: Color32,
    widget_border: Color32,
    input_bg: Color32,
    focus: Color32,
    cursor: Color32,
    line_hl: Option<Color32>,
    line_hl_border: Option<Color32>,
    bracket_bg: Color32,
    bracket_border: Color32,
    find_current: Color32,
    find_other: Color32,
    badge_bg: Color32,
    badge_fg: Color32,
    toolbar_hover: Color32,
    slider: Color32,
    slider_hover: Color32,
    scroll: (Color32, Color32, Color32),
    indent: Color32,
    indent_active: Color32,
    suggest_bg: Color32,
    suggest_sel: Color32,
    classes: [(Color32, bool); 13],
    kinds: [Color32; 7],
}

fn class_index(c: script::Class) -> usize {
    use script::Class::*;
    match c {
        Keyword => 0,
        Control => 1,
        Type => 2,
        Property => 3,
        Variable => 4,
        Function => 5,
        Number => 6,
        String => 7,
        Comment => 8,
        Resource => 9,
        Operator => 10,
        Punct => 11,
        Error => 12,
    }
}

impl Pal {
    fn from_theme(t: &VsTheme) -> Pal {
        let rgb = Color32::from_rgb;
        let c = |keys: &[&str], d: Color32| t.first(keys).unwrap_or(d);
        let bg = c(&["editor.background"], rgb(30, 30, 30));
        let code = c(&["editor.foreground", "foreground"], rgb(212, 212, 212));
        let text = c(&["foreground"], rgb(204, 204, 204));
        let dim = c(&["descriptionForeground", "tab.inactiveForeground", "icon.foreground"], rgb(133, 133, 133));
        let tok = |scopes: &[&str], d: Color32| t.token_of(scopes).map(|(col, st)| (col, st.italic)).unwrap_or((d, false));
        let err = c(&["editorError.foreground"], rgb(241, 76, 76));
        let classes = [
            tok(&["storage.type", "keyword"], code),
            tok(&["keyword.control", "keyword"], code),
            tok(&["entity.name.type", "support.class", "entity.name.class", "entity.name"], code),
            tok(&["variable.other.property", "support.variable.property", "meta.object-literal.key", "variable"], code),
            tok(&["variable", "variable.other.readwrite"], code),
            tok(&["entity.name.function", "support.function"], code),
            tok(&["constant.numeric"], code),
            tok(&["string"], code),
            tok(&["comment"], dim),
            tok(&["variable.other.constant", "constant"], code),
            tok(&["keyword.operator"], code),
            tok(&["punctuation"], code),
            (err, false),
        ];
        let sym = |k: &str, d: Color32| c(&[k], d);
        let kinds = [
            sym("symbolIcon.keywordForeground", rgb(204, 204, 204)),
            sym("symbolIcon.functionForeground", rgb(177, 128, 215)),
            sym("symbolIcon.classForeground", rgb(238, 157, 40)),
            sym("symbolIcon.fieldForeground", rgb(117, 190, 255)),
            sym("symbolIcon.variableForeground", rgb(117, 190, 255)),
            sym("symbolIcon.constantForeground", rgb(204, 204, 204)),
            sym("symbolIcon.enumeratorForeground", rgb(238, 157, 40)),
        ];
        let sidebar = c(&["sideBar.background"], rgb(37, 37, 38));
        let list_hover = c(&["list.hoverBackground"], rgb(42, 45, 46));
        Pal {
            bg,
            sidebar,
            activity: c(&["activityBar.background"], rgb(51, 51, 51)),
            activity_fg: c(&["activityBar.foreground"], Color32::WHITE),
            activity_dim: c(&["activityBar.inactiveForeground"], rgb(133, 133, 133)),
            tab_bar: c(&["editorGroupHeader.tabsBackground"], rgb(37, 37, 38)),
            tab_inactive: c(&["tab.inactiveBackground"], rgb(45, 45, 45)),
            tab_sep: c(&["tab.border"], rgb(37, 37, 38)),
            tab_top: c(&["tab.activeBorderTop"], Color32::TRANSPARENT),
            tab_active_fg: c(&["tab.activeForeground"], Color32::WHITE),
            tab_inactive_fg: c(&["tab.inactiveForeground"], dim),
            edge: c(&["sideBar.border", "panel.border", "editorGroup.border", "widget.border"], rgb(43, 43, 43)),
            status: c(&["statusBar.background"], rgb(0, 122, 204)),
            status_fg: c(&["statusBar.foreground"], Color32::WHITE),
            status_hover: c(&["statusBarItem.hoverBackground"], Color32::from_white_alpha(30)),
            text,
            code,
            dim,
            bright: c(&["list.activeSelectionForeground", "tab.activeForeground"], Color32::WHITE),
            line_no: c(&["editorLineNumber.foreground"], rgb(133, 133, 133)),
            line_no_active: c(&["editorLineNumber.activeForeground"], rgb(198, 198, 198)),
            hover: list_hover,
            list_select: c(&["list.activeSelectionBackground"], rgb(4, 57, 94)),
            row_select: c(&["list.inactiveSelectionBackground"], rgb(55, 55, 61)),
            selection: c(&["editor.selectionBackground"], rgb(38, 79, 120)),
            error: err,
            widget_bg: c(&["editorWidget.background"], rgb(37, 37, 38)),
            widget_border: c(&["editorWidget.border", "widget.border", "contrastBorder"], rgb(69, 69, 69)),
            input_bg: c(&["input.background"], rgb(60, 60, 60)),
            focus: c(&["focusBorder"], rgb(0, 127, 212)),
            cursor: c(&["editorCursor.foreground"], rgb(174, 175, 173)),
            line_hl: t.color("editor.lineHighlightBackground"),
            line_hl_border: t.color("editor.lineHighlightBorder"),
            bracket_bg: c(&["editorBracketMatch.background"], Color32::from_rgba_unmultiplied(0, 100, 0, 26)),
            bracket_border: c(&["editorBracketMatch.border"], rgb(136, 136, 136)),
            find_current: c(&["editor.findMatchBackground"], rgb(81, 92, 106)),
            find_other: c(&["editor.findMatchHighlightBackground"], Color32::from_rgba_unmultiplied(234, 92, 0, 85)),
            badge_bg: c(&["badge.background"], rgb(77, 77, 77)),
            badge_fg: c(&["badge.foreground"], Color32::WHITE),
            toolbar_hover: c(&["toolbar.hoverBackground"], Color32::from_white_alpha(25)),
            slider: c(&["minimapSlider.background"], Color32::from_rgba_unmultiplied(121, 121, 121, 25)),
            slider_hover: c(&["minimapSlider.hoverBackground"], Color32::from_rgba_unmultiplied(100, 100, 100, 35)),
            scroll: (c(&["scrollbarSlider.background"], Color32::from_rgba_unmultiplied(121, 121, 121, 102)), c(&["scrollbarSlider.hoverBackground"], Color32::from_rgba_unmultiplied(100, 100, 100, 178)), c(&["scrollbarSlider.activeBackground"], Color32::from_rgba_unmultiplied(191, 191, 191, 102))),
            indent: c(&["editorIndentGuide.background1", "editorIndentGuide.background"], Color32::from_white_alpha(20)),
            indent_active: c(&["editorIndentGuide.activeBackground1", "editorIndentGuide.activeBackground"], Color32::from_white_alpha(70)),
            suggest_bg: c(&["editorSuggestWidget.background", "editorWidget.background"], rgb(37, 37, 38)),
            suggest_sel: c(&["editorSuggestWidget.selectedBackground", "list.activeSelectionBackground"], rgb(4, 57, 94)),
            classes,
            kinds,
        }
    }

    fn class_color(&self, c: script::Class) -> Color32 {
        self.classes[class_index(c)].0
    }

    fn class_italic(&self, c: script::Class) -> bool {
        self.classes[class_index(c)].1
    }

    fn kind_color(&self, k: Kind) -> Color32 {
        self.kinds[match k {
            Kind::Keyword => 0,
            Kind::Function => 1,
            Kind::Component => 2,
            Kind::Field => 3,
            Kind::Variable => 4,
            Kind::Resource => 5,
            Kind::Value => 6,
        }]
    }
}

static PALETTE: RwLock<Option<Arc<Pal>>> = RwLock::new(None);

/// The colours in use (VS Code's own default theme until another is chosen).
fn pal() -> Arc<Pal> {
    if let Some(p) = PALETTE.read().ok().and_then(|g| g.clone()) {
        return p;
    }
    let p = Arc::new(Pal::from_theme(&vsc_theme::load(vsc_theme::DEFAULT_THEME)));
    if let Ok(mut g) = PALETTE.write() {
        *g = Some(p.clone());
    }
    p
}

fn use_theme(name: &str) {
    let p = Arc::new(Pal::from_theme(&vsc_theme::load(name)));
    if let Ok(mut g) = PALETTE.write() {
        *g = Some(p);
    }
}

/// VS Code's editor line height: 1.35 times the font size on Windows, rounded.
const LINE_H: f32 = 19.0;

fn hash_text(s: &str) -> u64 {
    s.bytes().fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211))
}

const CODE_FONT: f32 = 14.0;
const TAB_H: f32 = 35.0;
const CRUMB_H: f32 = 22.0;
const STATUS_H: f32 = 22.0;
const PROBLEMS_H: f32 = 170.0;
const ACTIVITY_W: f32 = 48.0;
const SIDEBAR_W: f32 = 240.0;
const MINIMAP_W: f32 = 96.0;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Side {
    #[default]
    Explorer,
    Search,
    Hidden,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Bottom {
    #[default]
    Problems,
    Output,
}

struct Doc {
    uid: u64,
    title: String,
    path: Option<PathBuf>,
    /// Text that came from a system of the scene (not from a file yet).
    scene_system: Option<String>,
    text: String,
    saved: String,
    diags: Vec<script::Diagnostic>,
    engine: Vec<script::Diagnostic>,
    checked: u64,
    engine_for: u64,
    applied: u64,
    applied_names: Vec<String>,
    /// Anchor and head of the selection as of the last frame (character indices).
    cursor: (usize, usize),
}

impl Doc {
    fn dirty(&self) -> bool {
        self.text != self.saved
    }

    fn problems(&self) -> impl Iterator<Item = &script::Diagnostic> {
        self.diags.iter().chain(self.engine.iter())
    }
}

struct Completion {
    items: Vec<Item>,
    selected: usize,
    word: ce::Word,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cmd {
    Apply,
    Save,
    NewScript,
    CloseTab,
    Find,
    GotoLine,
    ToggleMinimap,
    ToggleProblems,
    ToggleSidebar,
    Format,
    Comment,
    Reload,
    Complete,
    ColorTheme,
    OpenBlueprint,
}

const COMMANDS: &[(Cmd, &str, &str)] = &[
    (Cmd::Apply, "Script: Apply to the World", "Ctrl+S"),
    (Cmd::Save, "File: Save Script", ""),
    (Cmd::NewScript, "File: New Script", ""),
    (Cmd::CloseTab, "View: Close Editor", "Ctrl+W"),
    (Cmd::Find, "Edit: Find", "Ctrl+F"),
    (Cmd::GotoLine, "Go to Line...", "Ctrl+G"),
    (Cmd::Comment, "Edit: Toggle Line Comment", "Ctrl+/"),
    (Cmd::Format, "Format Document", "Shift+Alt+F"),
    (Cmd::Complete, "Trigger Suggest", "Ctrl+Space"),
    (Cmd::Reload, "Script: Reload from the Scene", ""),
    (Cmd::ColorTheme, "Preferences: Color Theme", "Ctrl+K Ctrl+T"),
    (Cmd::OpenBlueprint, "Script: Open as Blueprint (nodes)", ""),
    (Cmd::ToggleMinimap, "View: Toggle Minimap", ""),
    (Cmd::ToggleProblems, "View: Toggle Problems", "Ctrl+Shift+M"),
    (Cmd::ToggleSidebar, "View: Toggle Side Bar", "Ctrl+B"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteMode {
    Commands,
    Files,
    Themes,
}

struct Palette {
    mode: PaletteMode,
    query: String,
    selected: usize,
    fresh: bool,
}

struct Find {
    query: String,
    case: bool,
    current: usize,
    fresh: bool,
}

/// What the tab remembers apart from the open files.
#[derive(Default)]
struct St {
    active: usize,
    side: Side,
    bottom: Bottom,
    problems_open: bool,
    output: Vec<String>,
    files: Vec<(String, PathBuf)>,
    files_at: Option<Instant>,
    systems: Vec<(String, bool)>,
    systems_at: Option<Instant>,
    palette: Option<Palette>,
    find: Option<Find>,
    comp: Option<Completion>,
    minimap_off: bool,
    search_text: String,
    hl: Option<(u64, Arc<Vec<script::HlSpan>>)>,
    mini: Option<(u64, Arc<Vec<MiniSeg>>)>,
    /// Put the cursor here (line, column, both 0-based) and show it.
    reveal: Option<(usize, usize)>,
    next_uid: u64,
    untitled: u32,
    want_focus: bool,
    scroll_y: f32,
    view_h: f32,
    content_h: f32,
    mini_scroll: Option<f32>,
    request_apply: bool,
    sys_open: bool,
    files_open: bool,
    /// Scroll to this line without moving the cursor.
    scroll_line: Option<usize>,
    /// Select this range of characters (and focus the text).
    select_range: Option<(usize, usize)>,
    themes: Vec<vsc_theme::ThemeInfo>,
    theme_saved: String,
    theme_now: String,
    root: PathBuf,
    prefs_loaded: bool,
}

#[derive(Clone, Copy)]
struct MiniSeg {
    line: u32,
    col: u32,
    len: u32,
    color: Color32,
}

#[derive(Default)]
pub struct ScriptEditor {
    docs: Vec<Doc>,
    st: St,
}

// ---------------------------------------------------------------------------------------------------------------- documents

fn scripts_dir(studio: &Studio) -> PathBuf {
    studio.opts.root.join("scripts")
}

fn new_doc(st: &mut St, title: String, path: Option<PathBuf>, scene_system: Option<String>, text: String, saved: String) -> Doc {
    st.next_uid += 1;
    Doc { uid: st.next_uid, title, path, scene_system, text, saved, diags: vec![], engine: vec![], checked: u64::MAX, engine_for: 0, applied: 0, applied_names: vec![], cursor: (0, 0) }
}

impl ScriptEditor {
    /// Ask the tab to apply the active script (the window's Ctrl+S while this tab is up).
    pub fn request_apply(&mut self) {
        self.st.request_apply = true;
    }

    pub fn is_dirty(&self) -> bool {
        self.docs.iter().any(|d| d.dirty())
    }

    pub fn open_path(&mut self, studio: &mut Studio, path: &Path) {
        let full = if path.is_absolute() { path.to_path_buf() } else { studio.opts.root.join(path) };
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref() == Some(full.as_path())) {
            self.st.active = i;
            return;
        }
        match std::fs::read_to_string(&full) {
            Ok(text) => {
                let title = full.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "script.lun".to_string());
                let d = new_doc(&mut self.st, title, Some(full), None, text.clone(), text);
                self.docs.push(d);
                self.st.active = self.docs.len() - 1;
                self.st.want_focus = true;
            }
            Err(e) => studio.toast(format!("cannot open {}: {e}", full.display()), true),
        }
    }

    /// Open a graph system of the scene as a script: its file when it has one that still says the same, otherwise its definition as text.
    pub fn open_system(&mut self, studio: &mut Studio, name: &str) {
        if let Some(i) = self.docs.iter().position(|d| d.scene_system.as_deref() == Some(name) || d.applied_names.iter().any(|n| n == name)) {
            self.st.active = i;
            return;
        }
        let def = match studio.engine.call("get_system", &Json::obj([("name", Json::str(name))])) {
            Ok(d) => d.get("def").cloned().unwrap_or(d),
            Err(e) => {
                studio.toast(format!("cannot read system {name}: {}", e.message), true);
                return;
            }
        };
        // a script file of that name that compiles to the very same system keeps its comments and layout
        let file = scripts_dir(studio).join(format!("{name}.lun"));
        if let Ok(text) = std::fs::read_to_string(&file) {
            if let Ok(list) = script::compile(&text) {
                if list.iter().any(|c| c.name == name && same_system(&c.def, &def)) {
                    self.open_path(studio, &file);
                    return;
                }
            }
        }
        match script::decompile(&def) {
            Ok(text) => {
                let mut d = new_doc(&mut self.st, format!("{name}.lun"), None, Some(name.to_string()), text.clone(), text.clone());
                d.applied = hash_text(&text);
                d.applied_names = vec![name.to_string()];
                self.docs.push(d);
                self.st.active = self.docs.len() - 1;
                self.st.want_focus = true;
            }
            Err(e) => studio.toast(format!("{name} has no script form: {e}"), true),
        }
    }

    fn new_script(&mut self, studio: &Studio) {
        self.st.untitled += 1;
        let name = format!("new_system_{}", self.st.untitled);
        let text = format!("/// Describe what {name} does.\nsystem {name} each Transform {{\n    \n}}\n");
        let d = new_doc(&mut self.st, format!("{name}.lun"), None, None, text, String::new());
        let _ = studio;
        self.docs.push(d);
        self.st.active = self.docs.len() - 1;
        self.st.want_focus = true;
    }

    fn close(&mut self, i: usize) {
        if i < self.docs.len() {
            self.docs.remove(i);
            if self.st.active >= self.docs.len() {
                self.st.active = self.docs.len().saturating_sub(1);
            } else if self.st.active > i {
                self.st.active -= 1;
            }
        }
    }
}

/// Do two definitions say the same thing (the engine adds `enabled`, the writer chooses the order of the keys)?
fn same_system(a: &Json, b: &Json) -> bool {
    let v = pal();
    fn norm(j: &Json) -> Json {
        let v = pal();
        match j {
            Json::Obj(o) => {
                let mut v: Vec<(String, Json)> = o.iter().filter(|(k, v)| !(k == "enabled" && *v == Json::Bool(true))).map(|(k, v)| (k.clone(), norm(v))).collect();
                v.sort_by(|x, y| x.0.cmp(&y.0));
                Json::Obj(v)
            }
            Json::Arr(a) => Json::Arr(a.iter().map(norm).collect()),
            Json::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => Json::Int(*f as i64),
            other => other.clone(),
        }
    }
    norm(a) == norm(b)
}

// ---------------------------------------------------------------------------------------------------------------- applying

fn out(st: &mut St, line: impl Into<String>) {
    st.output.push(line.into());
    if st.output.len() > 300 {
        st.output.remove(0);
    }
}

/// Compile the text of a document and define its systems in the world. Mistakes go to the document as diagnostics.
fn apply(st: &mut St, doc: &mut Doc, studio: &mut Studio) -> bool {
    doc.engine.clear();
    let list = match script::compile(&doc.text) {
        Ok(l) => l,
        Err(d) => {
            out(st, format!("{}: not applied, {} problem{}", doc.title, d.len(), if d.len() == 1 { "" } else { "s" }));
            doc.diags = d;
            st.problems_open = true;
            st.bottom = Bottom::Problems;
            return false;
        }
    };
    doc.diags.clear();
    let mut ok = true;
    let mut names: Vec<String> = Vec::new();
    for c in &list {
        let t0 = Instant::now();
        match studio.engine.call("define_system", &c.def) {
            Ok(_) => {
                studio.log(LogKind::Call, "define_system".to_string(), c.def.to_string_compact().unwrap_or_default().chars().take(400).collect(), t0.elapsed().as_secs_f32() * 1000.0);
                studio.dirty = true;
                names.push(c.name.clone());
                out(st, format!("{}: system `{}` applied", doc.title, c.name));
            }
            Err(e) => {
                ok = false;
                let range = e.path.as_deref().and_then(|p| c.locate(p)).unwrap_or_else(|| c.locate("name").unwrap_or(c.range.clone()));
                doc.engine.push(script::diagnostic_at(&doc.text, range, e.message.clone(), e.hint.clone()));
                studio.log(LogKind::Error, format!("define_system: {}", e.message), String::new(), t0.elapsed().as_secs_f32() * 1000.0);
                out(st, format!("{}: system `{}` refused: {}", doc.title, c.name, e.message));
            }
        }
    }
    doc.engine_for = hash_text(&doc.text);
    if !ok {
        st.problems_open = true;
        st.bottom = Bottom::Problems;
        return false;
    }
    // a system that was in this script before and is not any more: remove it, or it would go on running next to its replacement
    for old in doc.applied_names.clone() {
        if !names.contains(&old) && studio.engine.call("remove_system", &Json::obj([("name", Json::str(old.clone()))])).is_ok() {
            out(st, format!("{}: system `{old}` removed (it is not in the script any more)", doc.title));
            studio.dirty = true;
        }
    }
    doc.applied_names = names.clone();
    doc.applied = hash_text(&doc.text);
    doc.scene_system = None;
    studio.toast(format!("Applied {}", names.join(", ")), false);
    true
}

/// Write the document to its file (it gets `scripts/<first system>.lun` when it has none yet).
fn save_file(st: &mut St, doc: &mut Doc, studio: &mut Studio) {
    let path = match &doc.path {
        Some(p) => p.clone(),
        None => {
            let name = script::compile(&doc.text).ok().and_then(|l| l.first().map(|c| c.name.clone())).unwrap_or_else(|| doc.title.trim_end_matches(".lun").to_string());
            scripts_dir(studio).join(format!("{name}.lun"))
        }
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::write(&path, &doc.text) {
        Ok(()) => {
            doc.title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            doc.path = Some(path.clone());
            doc.saved = doc.text.clone();
            st.files_at = None;
            out(st, format!("saved {}", path.display()));
        }
        Err(e) => studio.toast(format!("cannot save {}: {e}", path.display()), true),
    }
}

// ---------------------------------------------------------------------------------------------------------------- the tab

impl ScriptEditor {
    pub fn show(&mut self, ui: &mut Ui, studio: &mut Studio) {
        let v = pal();
        let ScriptEditor { docs, st } = self;
        if !st.prefs_loaded {
            st.prefs_loaded = true;
            st.root = studio.opts.root.clone();
            let name = read_pref(&st.root).unwrap_or_else(|| vsc_theme::DEFAULT_THEME.to_string());
            use_theme(&name);
            st.theme_saved = name.clone();
            st.theme_now = name;
        }
        refresh_lists(st, studio);
        if docs.is_empty() {
            // the tab opens with something in it: the first system of the scene, or a new script
            if let Some((name, _)) = st.systems.first().cloned() {
                let mut me = ScriptEditor { docs: std::mem::take(docs), st: std::mem::take(st) };
                me.open_system(studio, &name);
                if me.docs.is_empty() {
                    me.new_script(studio);
                }
                *docs = me.docs;
                *st = me.st;
            } else {
                let mut me = ScriptEditor { docs: std::mem::take(docs), st: std::mem::take(st) };
                me.new_script(studio);
                *docs = me.docs;
                *st = me.st;
            }
        }
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, CornerRadius::ZERO, v.bg);
        let ctx = ui.ctx().clone();

        // global keys of the tab
        let mut cmds: Vec<Cmd> = Vec::new();
        let mut new_palette: Option<PaletteMode> = None;
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::P) || i.consume_key(Modifiers::NONE, Key::F1) {
                new_palette = Some(PaletteMode::Commands);
            }
            if i.consume_key(Modifiers::COMMAND, Key::P) {
                new_palette = Some(PaletteMode::Files);
            }
            if i.consume_key(Modifiers::COMMAND, Key::S) || i.consume_key(Modifiers::NONE, Key::F5) {
                cmds.push(Cmd::Apply);
            }
            if i.consume_key(Modifiers::COMMAND, Key::F) {
                cmds.push(Cmd::Find);
            }
            if i.consume_key(Modifiers::COMMAND, Key::G) {
                cmds.push(Cmd::GotoLine);
            }
            if i.consume_key(Modifiers::COMMAND, Key::W) {
                cmds.push(Cmd::CloseTab);
            }
            if i.consume_key(Modifiers::COMMAND, Key::B) {
                cmds.push(Cmd::ToggleSidebar);
            }
            if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::M) {
                cmds.push(Cmd::ToggleProblems);
            }
            if i.consume_key(Modifiers::COMMAND, Key::Space) {
                cmds.push(Cmd::Complete);
            }
            if i.consume_key(Modifiers::SHIFT | Modifiers::ALT, Key::F) {
                cmds.push(Cmd::Format);
            }
        });
        if let Some(m) = new_palette {
            st.palette = Some(Palette { mode: m, query: String::new(), selected: 0, fresh: true });
        }
        if std::mem::take(&mut st.request_apply) {
            cmds.push(Cmd::Apply);
        }

        // layout
        let activity = Rect::from_min_size(full.min, Vec2::new(ACTIVITY_W, full.height()));
        let side_w = if st.side == Side::Hidden { 0.0 } else { SIDEBAR_W };
        let sidebar = Rect::from_min_size(activity.right_top(), Vec2::new(side_w, full.height()));
        let main = Rect::from_min_max(Pos2::new(sidebar.max.x, full.min.y), full.max);
        let status = Rect::from_min_max(Pos2::new(main.min.x, main.max.y - STATUS_H), main.max);
        let bottom_h = if st.problems_open { PROBLEMS_H.min(main.height() * 0.5) } else { 0.0 };
        let bottom = Rect::from_min_max(Pos2::new(main.min.x, status.min.y - bottom_h), Pos2::new(main.max.x, status.min.y));
        let tabs = Rect::from_min_size(main.min, Vec2::new(main.width(), TAB_H));
        let crumbs = Rect::from_min_size(tabs.left_bottom(), Vec2::new(main.width(), CRUMB_H));
        let editor = Rect::from_min_max(crumbs.left_bottom(), Pos2::new(main.max.x, bottom.min.y));

        let mut acts: Vec<Act> = Vec::new();
        region(ui, activity, |ui| activity_bar(ui, activity, st, &mut cmds));
        if st.side != Side::Hidden {
            region(ui, sidebar, |ui| match st.side {
                Side::Explorer => explorer(ui, sidebar, st, docs, &mut acts),
                Side::Search => search_panel(ui, sidebar, st, docs, studio, &mut acts),
                Side::Hidden => {}
            });
        }
        region(ui, tabs, |ui| tab_bar(ui, tabs, st, docs, &mut acts, &mut cmds));
        if let Some(doc) = docs.get(st.active) {
            region(ui, crumbs, |ui| breadcrumbs(ui, crumbs, doc, studio));
        }
        if bottom_h > 0.0 {
            region(ui, bottom, |ui| problems_panel(ui, bottom, st, docs, &mut acts));
        }
        region(ui, status, |ui| status_bar(ui, status, st, docs, &mut cmds));
        if st.active < docs.len() {
            let i = st.active;
            let doc = &mut docs[i];
            region(ui, editor, |ui| editor_body(ui, editor, st, doc, studio, &mut cmds));
        }
        // lines between the parts
        let p = ui.painter();
        p.line_segment([activity.right_top(), activity.right_bottom()], Stroke::new(1.0, v.edge));
        if side_w > 0.0 {
            p.line_segment([sidebar.right_top(), sidebar.right_bottom()], Stroke::new(1.0, v.edge));
        }
        if bottom_h > 0.0 {
            p.line_segment([bottom.left_top(), bottom.right_top()], Stroke::new(1.0, v.edge));
        }
        if st.palette.is_some() {
            palette_widget(ui, full, st, docs, &mut acts, &mut cmds);
        }

        // what was asked
        for a in acts {
            let mut me = ScriptEditor { docs: std::mem::take(docs), st: std::mem::take(st) };
            match a {
                Act::OpenFile(p) => me.open_path(studio, &p),
                Act::OpenSystem(n) => me.open_system(studio, &n),
                Act::Activate(i) => {
                    me.st.active = i.min(me.docs.len().saturating_sub(1));
                    me.st.want_focus = true;
                }
                Act::Close(i) => me.close(i),
                Act::Reveal(doc_i, line, col) => {
                    me.st.active = doc_i.min(me.docs.len().saturating_sub(1));
                    me.st.reveal = Some((line, col));
                }
                Act::NewScript => me.new_script(studio),
            }
            *docs = me.docs;
            *st = me.st;
        }
        for c in cmds {
            run_command(c, st, docs, studio);
        }
        if docs.is_empty() {
            let mut me = ScriptEditor { docs: std::mem::take(docs), st: std::mem::take(st) };
            me.new_script(studio);
            *docs = me.docs;
            *st = me.st;
        }
    }
}

#[cfg(test)]
impl ScriptEditor {
    pub fn test_text(&self) -> String {
        self.docs.get(self.st.active).map(|d| d.text.clone()).unwrap_or_default()
    }

    pub fn test_problems(&self) -> Vec<String> {
        self.docs.get(self.st.active).map(|d| d.problems().map(|p| p.to_text()).collect()).unwrap_or_default()
    }

    pub fn test_completions(&self) -> Vec<String> {
        self.st.comp.as_ref().map(|c| c.items.iter().map(|i| i.label.clone()).collect()).unwrap_or_default()
    }

    pub fn test_docs(&self) -> Vec<String> {
        self.docs.iter().map(|d| d.title.clone()).collect()
    }

    pub fn test_output(&self) -> Vec<String> {
        self.st.output.clone()
    }
}

enum Act {
    OpenFile(PathBuf),
    OpenSystem(String),
    Activate(usize),
    Close(usize),
    Reveal(usize, usize, usize),
    NewScript,
}

fn run_command(c: Cmd, st: &mut St, docs: &mut Vec<Doc>, studio: &mut Studio) {
    let active = st.active.min(docs.len().saturating_sub(1));
    match c {
        Cmd::Apply => {
            if let Some(doc) = docs.get_mut(active) {
                // the text is kept in its file also when it does not compile, so that nothing typed is lost
                apply(st, doc, studio);
                save_file(st, doc, studio);
            }
        }
        Cmd::Save => {
            if let Some(doc) = docs.get_mut(active) {
                save_file(st, doc, studio);
            }
        }
        Cmd::NewScript => {
            let mut me = ScriptEditor { docs: std::mem::take(docs), st: std::mem::take(st) };
            me.new_script(studio);
            *docs = me.docs;
            *st = me.st;
        }
        Cmd::CloseTab => {
            let mut me = ScriptEditor { docs: std::mem::take(docs), st: std::mem::take(st) };
            me.close(active);
            *docs = me.docs;
            *st = me.st;
        }
        Cmd::Find => {
            let seed = docs.get(active).map(|d| {
                let (a, b) = (d.cursor.0.min(d.cursor.1), d.cursor.0.max(d.cursor.1));
                d.text.chars().skip(a).take(b - a).collect::<String>()
            });
            let q = seed.filter(|s| !s.is_empty() && !s.contains('\n')).or_else(|| st.find.as_ref().map(|f| f.query.clone())).unwrap_or_default();
            st.find = Some(Find { query: q, case: false, current: 0, fresh: true });
        }
        Cmd::GotoLine => st.palette = Some(Palette { mode: PaletteMode::Commands, query: ":".to_string(), selected: 0, fresh: true }),
        Cmd::ToggleMinimap => st.minimap_off = !st.minimap_off,
        Cmd::ToggleProblems => st.problems_open = !st.problems_open,
        Cmd::ToggleSidebar => st.side = if st.side == Side::Hidden { Side::Explorer } else { Side::Hidden },
        Cmd::Format => {
            if let Some(doc) = docs.get_mut(active) {
                match script::compile(&doc.text) {
                    Ok(list) => {
                        let defs: Vec<Json> = list.iter().map(|c| c.def.clone()).collect();
                        match script::decompile_all(&defs) {
                            Ok(t) => {
                                // keep the doc comments of the originals: they are part of the definition
                                doc.text = t;
                            }
                            Err(e) => studio.toast(format!("cannot format: {e}"), true),
                        }
                    }
                    Err(_) => studio.toast("fix the problems before formatting", true),
                }
            }
        }
        Cmd::Comment => st.want_focus = true,
        Cmd::Reload => {
            if let Some(doc) = docs.get_mut(active) {
                let name = doc.scene_system.clone().or_else(|| doc.applied_names.first().cloned());
                if let Some(n) = name {
                    if let Ok(d) = studio.engine.call("get_system", &Json::obj([("name", Json::str(n.clone()))])) {
                        let def = d.get("def").cloned().unwrap_or(d);
                        if let Ok(t) = script::decompile(&def) {
                            doc.text = t;
                            out(st, format!("{n}: reloaded from the scene"));
                        }
                    }
                }
            }
        }
        Cmd::Complete => st.want_focus = true,
        Cmd::OpenBlueprint => {
            // the system the cursor is in (or the first one of the script), once it is in the world
            if let Some(doc) = docs.get(active) {
                let at = ce::byte_of(&doc.text, doc.cursor.1);
                let name = script::compile(&doc.text).ok().and_then(|l| l.iter().find(|c| c.range.start <= at && at <= c.range.end).or(l.first()).map(|c| c.name.clone()));
                match name {
                    Some(n) if studio.engine.call("get_system", &Json::obj([("name", Json::str(n.clone()))])).is_ok() => studio.open_blueprint = Some(n),
                    Some(_) => studio.toast("Apply the script first (Ctrl+S): the Blueprint shows what is in the world", true),
                    None => studio.toast("The script has problems: fix them first", true),
                }
            }
        }
        Cmd::ColorTheme => {
            st.themes = vsc_theme::discover();
            st.palette = Some(Palette { mode: PaletteMode::Themes, query: String::new(), selected: st.themes.iter().position(|t| t.label == st.theme_saved).unwrap_or(0), fresh: true });
        }
    }
}

fn prefs_file(root: &Path) -> PathBuf {
    root.join(".lunaris").join("editor.json")
}

fn read_pref(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(prefs_file(root)).ok()?;
    lunaris_json::parse(&text).ok()?.get("script_theme")?.as_str().map(str::to_string)
}

fn write_pref(root: &Path, theme: &str) {
    let file = prefs_file(root);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut doc = std::fs::read_to_string(&file).ok().and_then(|t| lunaris_json::parse(&t).ok()).unwrap_or_else(|| Json::obj::<String>([]));
    doc.set("script_theme", Json::str(theme));
    let _ = std::fs::write(file, doc.to_string_pretty().unwrap_or_default());
}

fn region(ui: &mut Ui, rect: Rect, f: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        f(ui);
    });
}

fn refresh_lists(st: &mut St, studio: &mut Studio) {
    if st.files_at.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
        st.files_at = Some(Instant::now());
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(scripts_dir(studio)) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "lun") {
                    files.push((p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), p));
                }
            }
        }
        files.sort();
        st.files = files;
    }
    if st.systems_at.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
        st.systems_at = Some(Instant::now());
        if let Ok(s) = studio.engine.call("systems", &Json::Null) {
            st.systems = s.get("graph").and_then(|g| g.as_array()).map(|a| a.iter().filter_map(|x| x.get("name").and_then(|n| n.as_str()).map(|n| (n.to_string(), x.get("last_error").is_some_and(|e| e.as_str().is_some())))).collect()).unwrap_or_default();
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------- the activity bar and the side bar

fn activity_bar(ui: &mut Ui, rect: Rect, st: &mut St, cmds: &mut Vec<Cmd>) {
    let v = pal();
    ui.painter().rect_filled(rect, CornerRadius::ZERO, v.activity);
    for (k, (side, tip)) in [(Side::Explorer, "Explorer (Ctrl+B)"), (Side::Search, "Search")].into_iter().enumerate() {
        let r = Rect::from_min_size(rect.min + Vec2::new(0.0, k as f32 * 48.0), Vec2::new(ACTIVITY_W, 48.0));
        let resp = ui.interact(r, Id::new(("script-activity", k)), Sense::click()).on_hover_text(tip);
        let active = st.side == side;
        let col = if active || resp.hovered() { v.activity_fg } else { v.activity_dim };
        if active {
            ui.painter().line_segment([r.left_top() + Vec2::new(1.0, 0.0), r.left_bottom() + Vec2::new(1.0, 0.0)], Stroke::new(2.0, v.activity_fg));
        }
        let c = r.center();
        let p = ui.painter();
        let s = Stroke::new(1.6, col);
        if side == Side::Explorer {
            // two sheets of paper
            p.rect_stroke(Rect::from_min_size(c + Vec2::new(-7.0, -10.0), Vec2::new(11.0, 14.0)), CornerRadius::same(1), s, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_size(c + Vec2::new(-3.0, -5.0), Vec2::new(11.0, 14.0)), CornerRadius::same(1), s, egui::StrokeKind::Middle);
        } else {
            p.circle_stroke(c + Vec2::new(-2.0, -2.0), 6.5, s);
            p.line_segment([c + Vec2::new(3.0, 3.0), c + Vec2::new(9.0, 9.0)], s);
        }
        if resp.clicked() {
            if active {
                cmds.push(Cmd::ToggleSidebar);
            } else {
                st.side = side;
            }
        }
    }
}

fn section_header(ui: &mut Ui, title: &str, open: &mut bool, plus: bool) -> bool {
    let v = pal();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, CornerRadius::ZERO, v.hover);
    }
    let c = Pos2::new(rect.min.x + 12.0, rect.center().y);
    let pts = if *open { vec![c + Vec2::new(-3.5, -2.0), c + Vec2::new(3.5, -2.0), c + Vec2::new(0.0, 3.0)] } else { vec![c + Vec2::new(-2.0, -3.5), c + Vec2::new(-2.0, 3.5), c + Vec2::new(3.0, 0.0)] };
    p.add(Shape::convex_polygon(pts, v.text, Stroke::NONE));
    p.text(Pos2::new(rect.min.x + 22.0, rect.center().y), Align2::LEFT_CENTER, title, FontId::proportional(11.0), v.text);
    let mut plus_clicked = false;
    if plus {
        let pr = Rect::from_center_size(Pos2::new(rect.max.x - 16.0, rect.center().y), Vec2::splat(18.0));
        let pr_resp = ui.interact(pr, resp.id.with("plus"), Sense::click());
        let col = if pr_resp.hovered() { v.bright } else { v.dim };
        let pc = pr.center();
        ui.painter().line_segment([pc + Vec2::new(-4.0, 0.0), pc + Vec2::new(4.0, 0.0)], Stroke::new(1.4, col));
        ui.painter().line_segment([pc + Vec2::new(0.0, -4.0), pc + Vec2::new(0.0, 4.0)], Stroke::new(1.4, col));
        plus_clicked = pr_resp.clicked();
    }
    if resp.clicked() && !plus_clicked {
        *open = !*open;
    }
    plus_clicked
}

fn side_row(ui: &mut Ui, label: &str, selected: bool, dot: Option<Color32>, icon: Color32, detail: &str) -> egui::Response {
    let v = pal();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::ZERO, v.row_select);
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::ZERO, v.hover);
    }
    // a small "L" file icon
    let ic = Pos2::new(rect.min.x + 24.0, rect.center().y);
    p.rect_stroke(Rect::from_center_size(ic, Vec2::new(10.0, 13.0)), CornerRadius::same(1), Stroke::new(1.2, icon), egui::StrokeKind::Middle);
    p.text(ic, Align2::CENTER_CENTER, "L", FontId::proportional(8.5), icon);
    p.text(Pos2::new(rect.min.x + 38.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(13.0), if selected { v.bright } else { v.text });
    if !detail.is_empty() {
        p.text(Pos2::new(rect.max.x - 8.0, rect.center().y), Align2::RIGHT_CENTER, detail, FontId::proportional(11.0), v.dim);
    }
    if let Some(d) = dot {
        p.circle_filled(Pos2::new(rect.max.x - 14.0, rect.center().y), 3.5, d);
    }
    resp
}

fn explorer(ui: &mut Ui, rect: Rect, st: &mut St, docs: &[Doc], acts: &mut Vec<Act>) {
    let v = pal();
    ui.painter().rect_filled(rect, CornerRadius::ZERO, v.sidebar);
    ui.painter().text(rect.min + Vec2::new(20.0, 18.0), Align2::LEFT_CENTER, "EXPLORER", FontId::proportional(11.0), v.text);
    let body = Rect::from_min_max(rect.min + Vec2::new(0.0, 36.0), rect.max);
    ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        egui::ScrollArea::vertical().id_salt("script-explorer").auto_shrink([false, false]).show(ui, |ui| {
            let mut open = !st.files_open;
            let plus = section_header(ui, "SCRIPTS", &mut open, true);
            st.files_open = !open;
            if plus {
                acts.push(Act::NewScript);
            }
            if open {
                // the files, and the open documents that are not files yet
                let active_path = docs.get(st.active).and_then(|d| d.path.clone());
                for (name, path) in st.files.clone() {
                    let dirty = docs.iter().any(|d| d.path.as_ref() == Some(&path) && d.dirty());
                    if side_row(ui, &name, active_path.as_ref() == Some(&path), dirty.then_some(v.text), v.kind_color(Kind::Field), "").clicked() {
                        acts.push(Act::OpenFile(path));
                    }
                }
                if st.files.is_empty() {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(22.0);
                        ui.label(egui::RichText::new("No scripts yet. Press + to create one.").size(12.0).color(v.dim));
                    });
                }
            }
            let mut sys_open = !st.sys_open;
            section_header(ui, "SYSTEMS IN THE SCENE", &mut sys_open, false);
            st.sys_open = !sys_open;
            if sys_open {
                let active_sys = docs.get(st.active).and_then(|d| d.scene_system.clone().or_else(|| d.applied_names.first().cloned()));
                for (name, err) in st.systems.clone() {
                    let r = side_row(ui, &name, active_sys.as_deref() == Some(name.as_str()), err.then_some(v.error), v.kind_color(Kind::Component), "");
                    if r.clicked() {
                        acts.push(Act::OpenSystem(name));
                    }
                }
                if st.systems.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(22.0);
                        ui.label(egui::RichText::new("The scene has no graph systems.").size(12.0).color(v.dim));
                    });
                }
            }
        });
    });
}

fn search_panel(ui: &mut Ui, rect: Rect, st: &mut St, docs: &[Doc], studio: &Studio, acts: &mut Vec<Act>) {
    let v = pal();
    ui.painter().rect_filled(rect, CornerRadius::ZERO, v.sidebar);
    ui.painter().text(rect.min + Vec2::new(20.0, 18.0), Align2::LEFT_CENTER, "SEARCH", FontId::proportional(11.0), v.text);
    let body = Rect::from_min_max(rect.min + Vec2::new(10.0, 36.0), rect.max - Vec2::new(10.0, 4.0));
    ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
        let te = ui.add(TextEdit::singleline(&mut st.search_text).hint_text("Search").desired_width(f32::INFINITY).text_color(v.text));
        let _ = te;
        ui.add_space(6.0);
        let q = st.search_text.clone();
        if q.is_empty() {
            return;
        }
        // open documents and the files of the scripts folder
        let mut hits: Vec<(String, Option<usize>, Option<PathBuf>, usize, usize, String)> = Vec::new();
        let mut seen: Vec<PathBuf> = Vec::new();
        for (i, d) in docs.iter().enumerate() {
            if let Some(p) = &d.path {
                seen.push(p.clone());
            }
            for (l, line) in d.text.lines().enumerate() {
                if let Some(c) = line.to_lowercase().find(&q.to_lowercase()) {
                    hits.push((d.title.clone(), Some(i), d.path.clone(), l, ce::char_of(line, c), line.trim().to_string()));
                }
            }
        }
        let _ = studio;
        for (name, path) in &st.files {
            if seen.contains(path) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(path) {
                for (l, line) in text.lines().enumerate() {
                    if let Some(c) = line.to_lowercase().find(&q.to_lowercase()) {
                        hits.push((name.clone(), None, Some(path.clone()), l, ce::char_of(line, c), line.trim().to_string()));
                    }
                }
            }
        }
        ui.label(egui::RichText::new(format!("{} result{}", hits.len(), if hits.len() == 1 { "" } else { "s" })).size(11.0).color(v.dim));
        egui::ScrollArea::vertical().id_salt("script-search").auto_shrink([false, false]).show(ui, |ui| {
            for (title, di, path, line, col, text) in hits.into_iter().take(200) {
                let r = side_row(ui, &format!("{}  {}", text.chars().take(40).collect::<String>(), ""), false, None, v.dim, &format!("{title}:{}", line + 1));
                if r.clicked() {
                    match (di, path) {
                        (Some(i), _) => acts.push(Act::Reveal(i, line, col)),
                        (None, Some(p)) => {
                            acts.push(Act::OpenFile(p));
                            acts.push(Act::Reveal(usize::MAX, line, col));
                        }
                        _ => {}
                    }
                }
            }
        });
    });
}

// ---------------------------------------------------------------------------------------------------------------- tabs, breadcrumbs

fn tab_bar(ui: &mut Ui, rect: Rect, st: &mut St, docs: &[Doc], acts: &mut Vec<Act>, cmds: &mut Vec<Cmd>) {
    let v = pal();
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::ZERO, v.tab_bar);
    let mut x = rect.min.x;
    for (i, d) in docs.iter().enumerate() {
        let galley = p.layout_no_wrap(d.title.clone(), FontId::proportional(13.0), v.text);
        let w = (galley.size().x + 70.0).max(120.0);
        let r = Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(w, TAB_H));
        if r.min.x > rect.max.x - 120.0 {
            break;
        }
        let active = i == st.active;
        let resp = ui.interact(r, Id::new(("script-tab", d.uid)), Sense::click());
        p.rect_filled(r, CornerRadius::ZERO, if active { v.bg } else { v.tab_inactive });
        if active {
            p.line_segment([r.left_top() + Vec2::new(0.0, 0.5), r.right_top() + Vec2::new(0.0, 0.5)], Stroke::new(1.0, v.tab_top));
        }
        p.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, v.tab_sep));
        // an icon, the name, and a dot or a cross at the right
        let ic = Pos2::new(r.min.x + 18.0, r.center().y);
        let icon_col = v.kind_color(Kind::Field);
        p.rect_stroke(Rect::from_center_size(ic, Vec2::new(10.0, 13.0)), CornerRadius::same(1), Stroke::new(1.2, icon_col), egui::StrokeKind::Middle);
        p.text(ic, Align2::CENTER_CENTER, "L", FontId::proportional(8.5), icon_col);
        let has_error = d.problems().next().is_some();
        let name_col = if has_error { v.error } else if active { v.tab_active_fg } else { v.tab_inactive_fg };
        p.text(Pos2::new(r.min.x + 32.0, r.center().y), Align2::LEFT_CENTER, &d.title, FontId::proportional(13.0), name_col);
        let xr = Rect::from_center_size(Pos2::new(r.max.x - 16.0, r.center().y), Vec2::splat(18.0));
        let close = ui.interact(xr, Id::new(("script-tab-close", d.uid)), Sense::click());
        if d.dirty() && !(resp.hovered() || close.hovered()) {
            p.circle_filled(xr.center(), 4.0, v.text);
        } else if active || resp.hovered() || close.hovered() {
            if close.hovered() {
                p.rect_filled(xr, CornerRadius::same(5), v.toolbar_hover);
            }
            let c = xr.center();
            let s = Stroke::new(1.3, v.text);
            p.line_segment([c + Vec2::new(-3.5, -3.5), c + Vec2::new(3.5, 3.5)], s);
            p.line_segment([c + Vec2::new(-3.5, 3.5), c + Vec2::new(3.5, -3.5)], s);
        }
        if close.clicked() {
            acts.push(Act::Close(i));
        } else if resp.clicked() {
            acts.push(Act::Activate(i));
        }
        x += w;
    }
    p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, v.edge));
    // the button at the right: Apply (a triangle, like Run)
    let btn = Rect::from_center_size(Pos2::new(rect.max.x - 22.0, rect.center().y), Vec2::splat(22.0));
    let resp = ui.interact(btn, Id::new("script-apply"), Sense::click()).on_hover_text("Apply to the World (Ctrl+S)");
    if resp.hovered() {
        p.rect_filled(btn, CornerRadius::same(5), v.toolbar_hover);
    }
    let c = btn.center();
    p.add(Shape::convex_polygon(vec![c + Vec2::new(-3.5, -5.5), c + Vec2::new(-3.5, 5.5), c + Vec2::new(5.5, 0.0)], Color32::TRANSPARENT, Stroke::new(1.3, v.text)));
    if resp.clicked() {
        cmds.push(Cmd::Apply);
    }
    let _ = st;
}

fn breadcrumbs(ui: &mut Ui, rect: Rect, doc: &Doc, studio: &Studio) {
    let v = pal();
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, v.bg);
    let mut parts: Vec<String> = Vec::new();
    parts.push(studio.opts.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "project".to_string()));
    parts.push("scripts".to_string());
    parts.push(doc.title.clone());
    // the system the cursor is in
    if let Ok(list) = script::compile(&doc.text) {
        let at = ce::byte_of(&doc.text, doc.cursor.1);
        if let Some(c) = list.iter().find(|c| c.range.start <= at && at <= c.range.end) {
            parts.push(c.name.clone());
        }
    } else if let Some(first) = doc.text.lines().find_map(|l| l.trim().strip_prefix("system ")) {
        parts.push(first.split_whitespace().next().unwrap_or("").to_string());
    }
    let mut x = rect.min.x + 14.0;
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            p.text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, ">", FontId::proportional(11.0), v.dim);
            x += 14.0;
        }
        let last = i + 1 == parts.len();
        let r = p.text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, part, FontId::proportional(12.0), if last { v.text } else { v.dim });
        x = r.max.x + 6.0;
    }
}

// ---------------------------------------------------------------------------------------------------------------- the editor

/// What the pieces of this frame's drawing found out.
struct Drawn {
    galley: Arc<egui::Galley>,
    galley_pos: Pos2,
    range: Option<CCursorRange>,
    changed: bool,
    scroll_y: f32,
    view_h: f32,
    content_h: f32,
}

fn highlight_for(st: &mut St, text: &str) -> Arc<Vec<script::HlSpan>> {
    let v = pal();
    let h = hash_text(text);
    if let Some((k, v)) = &st.hl {
        if *k == h {
            return v.clone();
        }
    }
    let v = Arc::new(script::highlight(text));
    st.hl = Some((h, v.clone()));
    v
}

fn job_for(text: &str, spans: &[script::HlSpan], font: &FontId) -> LayoutJob {
    let v = pal();
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let v = pal();
    let fmt = |c: Color32, italics: bool| TextFormat { font_id: font.clone(), color: c, italics, line_height: Some(LINE_H), ..Default::default() };
    let mut at = 0;
    for s in spans {
        if s.start < at || s.end > text.len() || !text.is_char_boundary(s.start) || !text.is_char_boundary(s.end) {
            continue;
        }
        if s.start > at {
            job.append(&text[at..s.start], 0.0, fmt(v.code, false));
        }
        job.append(&text[s.start..s.end], 0.0, fmt(v.class_color(s.class), v.class_italic(s.class)));
        at = s.end;
    }
    job.append(&text[at.min(text.len())..], 0.0, fmt(v.code, false));
    job
}

/// The cursor range of a text field as (anchor, head).
fn read_range(ctx: &egui::Context, id: Id) -> Option<(usize, usize)> {
    let state = TextEdit::load_state(ctx, id)?;
    let r = state.cursor.char_range()?;
    Some((r.secondary.index.0, r.primary.index.0))
}

fn write_range(ctx: &egui::Context, id: Id, anchor: usize, head: usize) {
    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(anchor), CCursor::new(head))));
    TextEdit::store_state(ctx, id, state);
}

/// The keys the editor handles before the text field sees them. Returns whether the text was changed by them.
fn handle_keys(st: &mut St, doc: &mut Doc, studio: &mut Studio, ctx: &egui::Context, id: Id) -> bool {
    if !ctx.memory(|m| m.has_focus(id)) {
        return false;
    }
    let (mut anchor, mut head) = read_range(ctx, id).unwrap_or(doc.cursor);
    let mut text = doc.text.clone();
    let mut changed = false;
    let mut accept: Option<Item> = None;
    let mut close_comp = false;
    ctx.input_mut(|i| {
        // the suggestion list takes the arrows, Enter, Tab and Escape while it is open
        if let Some(c) = &mut st.comp {
            let n = c.items.len();
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                c.selected = (c.selected + 1) % n.max(1);
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                c.selected = (c.selected + n.max(1) - 1) % n.max(1);
            }
            if i.consume_key(Modifiers::NONE, Key::Enter) || i.consume_key(Modifiers::NONE, Key::Tab) {
                accept = c.items.get(c.selected).cloned();
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                close_comp = true;
            }
        }
        if accept.is_some() || close_comp {
            return;
        }
        let mut apply_edit = |e: ce::Edit, text: &mut String, anchor: &mut usize, head: &mut usize, changed: &mut bool| {
            if e.text != *text {
                *changed = true;
            }
            *text = e.text;
            *anchor = e.anchor;
            *head = e.head;
        };
        if i.consume_key(Modifiers::NONE, Key::Enter) {
            let e = ce::enter(&text, anchor, head);
            apply_edit(e, &mut text, &mut anchor, &mut head, &mut changed);
        }
        if i.consume_key(Modifiers::SHIFT, Key::Tab) {
            let e = ce::tab(&text, anchor, head, true);
            apply_edit(e, &mut text, &mut anchor, &mut head, &mut changed);
        }
        if i.consume_key(Modifiers::NONE, Key::Tab) {
            let e = ce::tab(&text, anchor, head, false);
            apply_edit(e, &mut text, &mut anchor, &mut head, &mut changed);
        }
        if i.consume_key(Modifiers::COMMAND, Key::Slash) {
            let e = ce::toggle_comment(&text, anchor, head);
            apply_edit(e, &mut text, &mut anchor, &mut head, &mut changed);
        }
        if let Some(e) = ce::backspace(&text, anchor, head) {
            if i.consume_key(Modifiers::NONE, Key::Backspace) {
                apply_edit(e, &mut text, &mut anchor, &mut head, &mut changed);
            }
        }
        // typed brackets and quotes
        let mut k = 0;
        while k < i.events.len() {
            let mut handled = false;
            if let Event::Text(t) = &i.events[k] {
                let mut it = t.chars();
                if let (Some(ch), None) = (it.next(), it.next()) {
                    if let Some(e) = ce::typed(&text, anchor, head, ch) {
                        apply_edit(e, &mut text, &mut anchor, &mut head, &mut changed);
                        handled = true;
                    }
                }
            }
            if handled {
                i.events.remove(k);
            } else {
                k += 1;
            }
        }
    });
    if close_comp {
        st.comp = None;
        return false;
    }
    if let Some(item) = accept {
        if let Some(c) = st.comp.take() {
            let e = ce::accept(&doc.text, &c.word, head, &item);
            doc.text = e.text;
            write_range(ctx, id, e.anchor, e.head);
            return true;
        }
    }
    let _ = studio;
    if changed {
        doc.text = text;
        write_range(ctx, id, anchor, head);
    }
    changed
}

fn refresh_diags(doc: &mut Doc) {
    let h = hash_text(&doc.text);
    if doc.checked != h {
        doc.checked = h;
        doc.diags = script::check(&doc.text);
    }
    if doc.engine_for != h {
        doc.engine.clear();
    }
}

fn char_range_of(text: &str, d: &script::Diagnostic) -> (usize, usize) {
    let s = ce::char_of(text, d.start);
    let e = ce::char_of(text, d.end.max(d.start));
    (s, if e <= s { s + 1 } else { e })
}

fn squiggle(painter: &egui::Painter, x0: f32, x1: f32, y: f32, color: Color32) {
    let mut pts = Vec::new();
    let mut x = x0;
    let mut up = false;
    while x < x1 {
        pts.push(Pos2::new(x, y + if up { -1.6 } else { 0.4 }));
        x += 2.0;
        up = !up;
    }
    pts.push(Pos2::new(x1, y));
    painter.add(Shape::line(pts, Stroke::new(1.0, color)));
}

fn editor_body(ui: &mut Ui, rect: Rect, st: &mut St, doc: &mut Doc, studio: &mut Studio, cmds: &mut Vec<Cmd>) {
    let v = pal();
    let ctx = ui.ctx().clone();
    refresh_diags(doc);
    let id = Id::new(("script-text", doc.uid));
    let lines = doc.text.lines().count().max(1) + usize::from(doc.text.ends_with('\n'));
    let digits = lines.to_string().len().max(3);
    let gutter_w = 26.0 + digits as f32 * 8.4 + 14.0;
    let mini_w = if st.minimap_off { 0.0 } else { MINIMAP_W };
    let gutter = Rect::from_min_size(rect.min, Vec2::new(gutter_w, rect.height()));
    let text_rect = Rect::from_min_max(Pos2::new(rect.min.x + gutter_w, rect.min.y), Pos2::new(rect.max.x - mini_w, rect.max.y));
    let mini_rect = Rect::from_min_max(Pos2::new(rect.max.x - mini_w, rect.min.y), rect.max);

    // go to a place (Problems, search, Ctrl+G): put the cursor there
    let mut reveal_line: Option<usize> = None;
    if let Some((l, c)) = st.reveal.take() {
        let at = ce::index_of(&doc.text, l, c);
        write_range(&ctx, id, at, at);
        ctx.memory_mut(|m| m.request_focus(id));
        reveal_line = Some(l);
    }
    if let Some((a, b)) = st.select_range.take() {
        write_range(&ctx, id, a, b);
        ctx.memory_mut(|m| m.request_focus(id));
        reveal_line = Some(ce::line_col(&doc.text, a).0);
    }
    if let Some(l) = st.scroll_line.take() {
        reveal_line = Some(l);
    }
    if st.want_focus {
        st.want_focus = false;
        ctx.memory_mut(|m| m.request_focus(id));
    }
    let force_complete = cmds.iter().any(|c| *c == Cmd::Complete);
    let keys_changed = handle_keys(st, doc, studio, &ctx, id);

    let spans = highlight_for(st, &doc.text);
    let font = FontId::monospace(CODE_FONT);
    let row_h = LINE_H;
    let diags: Vec<script::Diagnostic> = doc.problems().cloned().collect();
    let find_matches: Vec<(usize, usize)> = st.find.as_ref().map(|f| ce::find_all(&doc.text, &f.query, f.case)).unwrap_or_default();
    let find_current = st.find.as_ref().map(|f| f.current).unwrap_or(0);
    let cursor_line = ce::line_col(&doc.text, doc.cursor.1).0;
    let bracket = {
        let c: Vec<char> = doc.text.chars().collect();
        let h = doc.cursor.1;
        let near = |i: usize| c.get(i).is_some_and(|ch| "()[]{}".contains(*ch));
        if near(h) || (h > 0 && near(h - 1)) { ce::matching_bracket(&doc.text, h) } else { None }
    };
    let want_scroll = st.mini_scroll.take();

    // the text, in a scroll area
    let mut drawn: Option<Drawn> = None;
    let text_hash_before = hash_text(&doc.text);
    let spans_hash = text_hash_before;
    ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| {
        ui.set_clip_rect(text_rect.intersect(ui.clip_rect()));
        ui.visuals_mut().selection.bg_fill = v.selection;
        ui.visuals_mut().selection.stroke = Stroke::new(1.0, v.focus);
        ui.visuals_mut().text_cursor.stroke = Stroke::new(2.0, v.cursor);
        ui.visuals_mut().extreme_bg_color = v.bg;
        {
            let slider = vsc_slider(&v);
            let style = ui.style_mut();
            style.spacing.scroll = egui::style::ScrollStyle { bar_width: 14.0, bar_inner_margin: 0.0, bar_outer_margin: 0.0, handle_min_length: 20.0, dormant_handle_opacity: 1.0, active_handle_opacity: 1.0, interact_handle_opacity: 1.0, fade: egui::style::ScrollFadeStyle { strength: 0.0, size: 0.0 }, ..egui::style::ScrollStyle::solid() };
            style.visuals.widgets.inactive.bg_fill = slider.0;
            style.visuals.widgets.inactive.weak_bg_fill = slider.0;
            style.visuals.widgets.hovered.bg_fill = slider.1;
            style.visuals.widgets.hovered.weak_bg_fill = slider.1;
            style.visuals.widgets.active.bg_fill = slider.2;
            style.visuals.widgets.active.weak_bg_fill = slider.2;
        }
        let mut scroll = egui::ScrollArea::both().id_salt(("script-scroll", doc.uid)).auto_shrink([false, false]);
        if let Some(y) = want_scroll {
            scroll = scroll.vertical_scroll_offset(y);
        }
        let so = scroll.show(ui, |ui| {
            // the line of the cursor, behind the text
            let top = ui.cursor().min;
            let hl_rect = Rect::from_min_size(Pos2::new(top.x - 8.0, top.y + 4.0 + cursor_line as f32 * row_h), Vec2::new(text_rect.width().max(4000.0), row_h));
            if let Some(c) = v.line_hl {
                ui.painter().rect_filled(hl_rect, CornerRadius::ZERO, c);
            } else if let Some(b) = v.line_hl_border {
                ui.painter().rect_stroke(hl_rect, CornerRadius::ZERO, Stroke::new(2.0, b), egui::StrokeKind::Inside);
            }
            let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _w: f32| {
                let s = buf.as_str();
                let sp: Arc<Vec<script::HlSpan>> = if hash_text(s) == spans_hash { spans.clone() } else { Arc::new(script::highlight(s)) };
                ui.fonts_mut(|f| f.layout_job(job_for(s, &sp, &font)))
            };
            let o = TextEdit::multiline(&mut doc.text)
                .id(id)
                .font(font.clone())
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::symmetric(8, 4))
                .desired_width(f32::INFINITY)
                .min_size(Vec2::new(0.0, text_rect.height() - 8.0))
                .lock_focus(true)
                .layouter(&mut layouter)
                .show(ui);
            let galley = o.galley.clone();
            let gp = o.galley_pos;
            // matches of the find box
            let painter = ui.painter();
            for (k, (a, b)) in find_matches.iter().enumerate() {
                let (ra, rb) = (galley.pos_from_cursor(CCursor::new(*a)), galley.pos_from_cursor(CCursor::new(*b)));
                if (ra.min.y - rb.min.y).abs() < 1.0 {
                    let r = Rect::from_min_max(gp + ra.min.to_vec2(), gp + Pos2::new(rb.min.x, ra.max.y).to_vec2());
                    painter.rect_filled(r, CornerRadius::same(2), if k == find_current % find_matches.len().max(1) { v.find_current } else { v.find_other });
                }
            }
            // the bracket pair next to the cursor
            if let Some((a, b)) = bracket {
                for i in [a, b] {
                    let (r1, r2) = (galley.pos_from_cursor(CCursor::new(i)), galley.pos_from_cursor(CCursor::new(i + 1)));
                    let r = Rect::from_min_max(gp + r1.min.to_vec2(), gp + Pos2::new(r2.min.x.max(r1.min.x + 6.0), r1.max.y).to_vec2());
                    painter.rect_filled(r, CornerRadius::ZERO, v.bracket_bg);
                    painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, v.bracket_border), egui::StrokeKind::Inside);
                }
            }
            // indent guides: a thin line at every level of indentation
            {
                let lines_text: Vec<&str> = doc.text.split('\n').collect();
                let indent: Vec<Option<usize>> = lines_text.iter().map(|l| if l.trim().is_empty() { None } else { Some(l.len() - l.trim_start_matches(' ').len()) }).collect();
                let mut level: Vec<usize> = indent.iter().map(|i| i.unwrap_or(0)).collect();
                // a blank line keeps the guides of the code around it
                let mut next_known = 0usize;
                for n in (0..indent.len()).rev() {
                    if indent[n].is_some() {
                        next_known = indent[n].unwrap_or(0);
                    } else {
                        level[n] = next_known;
                    }
                }
                let mut prev_known = 0usize;
                for n in 0..indent.len() {
                    if indent[n].is_some() {
                        prev_known = indent[n].unwrap_or(0);
                    } else {
                        level[n] = level[n].min(prev_known);
                    }
                }
                let cw = ui.fonts_mut(|f| f.glyph_width(&font, ' '));
                for (n, _) in lines_text.iter().enumerate() {
                    if let Some(row) = galley.rows.get(n) {
                        for k in 0..(level[n] / 4) {
                            let x = gp.x + k as f32 * 4.0 * cw;
                            painter.line_segment([Pos2::new(x + 0.5, gp.y + row.pos.y), Pos2::new(x + 0.5, gp.y + row.pos.y + row.row.size.y)], Stroke::new(1.0, v.indent));
                        }
                    }
                }
            }
            // the squiggles
            for d in &diags {
                let (a, b) = char_range_of(&doc.text, d);
                let (ra, rb) = (galley.pos_from_cursor(CCursor::new(a)), galley.pos_from_cursor(CCursor::new(b)));
                let y = gp.y + ra.max.y - 1.0;
                let x0 = gp.x + ra.min.x;
                let x1 = if (ra.min.y - rb.min.y).abs() < 1.0 { gp.x + rb.min.x } else { gp.x + galley.rows.get(ce::line_col(&doc.text, a).0).map(|r| r.row.size.x).unwrap_or(40.0) + 4.0 };
                squiggle(painter, x0, x1.max(x0 + 6.0), y, v.error);
            }
            if let Some(l) = reveal_line {
                let y = gp.y + galley.rows.get(l).map(|r| r.pos.y).unwrap_or(0.0);
                ui.scroll_to_rect(Rect::from_min_size(Pos2::new(gp.x, y), Vec2::new(8.0, row_h)), Some(Align::Center));
            }
            // hovering a mistake shows what is wrong
            if let Some(ptr) = ui.input(|i| i.pointer.hover_pos()) {
                if text_rect.contains(ptr) {
                    let local = ptr - gp;
                    let cc = galley.cursor_from_pos(local);
                    let at = cc.index.0;
                    let mut tips: Vec<String> = Vec::new();
                    for d in &diags {
                        let (a, b) = char_range_of(&doc.text, d);
                        if at >= a && at <= b {
                            tips.push(d.to_text());
                        }
                    }
                    if !tips.is_empty() {
                        egui::Area::new(Id::new("script-hover")).fixed_pos(ptr + Vec2::new(12.0, 18.0)).order(egui::Order::Tooltip).interactable(false).show(ui.ctx(), |ui| {
                            egui::Frame::popup(ui.style()).fill(v.widget_bg).stroke(Stroke::new(1.0, v.widget_border)).show(ui, |ui| {
                                ui.set_max_width(520.0);
                                for t in &tips {
                                    ui.label(egui::RichText::new(t).size(12.5).color(v.text));
                                }
                            });
                        });
                    }
                }
            }
            // like VS Code, the text can be scrolled until its last line is at the top
            ui.add_space((text_rect.height() - LINE_H * 3.0).max(0.0));
            (galley, gp, o.cursor_range, o.response.changed())
        });
        let (galley, gp, range, changed) = so.inner;
        drawn = Some(Drawn { galley, galley_pos: gp, range, changed, scroll_y: so.state.offset.y, view_h: so.inner_rect.height(), content_h: so.content_size.y });
    });
    let Some(d) = drawn else { return };
    st.scroll_y = d.scroll_y;
    st.view_h = d.view_h;
    st.content_h = d.content_h;
    if let Some(r) = d.range {
        doc.cursor = (r.secondary.index.0, r.primary.index.0);
    }
    let text_changed = keys_changed || d.changed || hash_text(&doc.text) != text_hash_before;

    // the line numbers (fixed at the left, they follow the vertical scroll)
    {
        let p = ui.painter_at(gutter);
        p.rect_filled(gutter, CornerRadius::ZERO, v.bg);
        let (cur_line, _) = ce::line_col(&doc.text, doc.cursor.1);
        for (n, row) in d.galley.rows.iter().enumerate() {
            let y = d.galley_pos.y + row.pos.y + row.row.size.y / 2.0;
            if y < gutter.min.y - 20.0 || y > gutter.max.y + 20.0 {
                continue;
            }
            let has_err = diags.iter().any(|dg| dg.line == n + 1);
            p.text(Pos2::new(gutter.max.x - 14.0, y), Align2::RIGHT_CENTER, (n + 1).to_string(), FontId::monospace(CODE_FONT - 1.0), if n == cur_line { v.line_no_active } else { v.dim });
            if has_err {
                p.circle_filled(Pos2::new(gutter.min.x + 9.0, y), 3.5, v.error);
            }
        }
    }

    // the minimap
    if mini_w > 0.0 {
        minimap(ui, mini_rect, st, doc, &spans);
    }

    // completion: after typing, offer words; Ctrl+Space asks for them
    if text_changed || force_complete {
        let (_, head) = doc.cursor;
        let typed_something = text_changed && !doc.text.is_empty();
        st.comp = None;
        if typed_something || force_complete {
            if let Some(word) = ce::word_at(&doc.text, head) {
                let wanted = force_complete || !word.prefix.is_empty() || word.before_dot.is_some() || word.dollar;
                if wanted {
                    let comps = studio.components_available();
                    let mut fields_for: Vec<(String, String)> = Vec::new();
                    let mut field_comp = String::new();
                    if let Some(chain) = &word.before_dot {
                        let comp = chain.rsplit('.').next().unwrap_or(chain).to_string();
                        if comps.contains(&comp) {
                            let def = studio.def(&comp);
                            fields_for = def.fields.iter().map(|f| (f.name.clone(), if f.count > 1 { format!("{}[{}]", f.ty, f.count) } else { f.ty.clone() })).collect();
                            field_comp = comp;
                        }
                    }
                    let mut resources: Vec<String> = Vec::new();
                    if word.dollar {
                        if let Ok(r) = studio.engine.call("resources", &Json::Null) {
                            if let Some(o) = r.get("resources").and_then(|o| o.as_object()) {
                                resources = o.iter().map(|(k, _)| k.clone()).collect();
                            }
                        }
                        if let Ok(s) = studio.engine.call("systems", &Json::Null) {
                            for g in s.get("graph").and_then(|g| g.as_array()).unwrap_or(&[]) {
                                for n in g.get("resources").and_then(|r| r.as_array()).unwrap_or(&[]) {
                                    if let Some(n) = n.as_str() {
                                        if !resources.iter().any(|r| r == n) {
                                            resources.push(n.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let fields = |c: &str| -> Vec<(String, String)> { if c == field_comp { fields_for.clone() } else { vec![] } };
                    let items = ce::suggest(&doc.text, &word, &comps, &fields, &resources);
                    if !items.is_empty() {
                        st.comp = Some(Completion { items, selected: 0, word });
                    }
                }
            }
        }
    }
    if let Some(c) = &st.comp {
        // the list hangs under the word being typed
        let at = CCursor::new(c.word.start);
        let r = d.galley.pos_from_cursor(at);
        let pos = d.galley_pos + Vec2::new(r.min.x, r.max.y + 2.0);
        if text_rect.expand(40.0).contains(pos) {
            let mut clicked: Option<usize> = None;
            egui::Area::new(Id::new("script-complete")).fixed_pos(pos).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                egui::Frame::NONE.fill(v.suggest_bg).stroke(Stroke::new(1.0, v.widget_border)).inner_margin(egui::Margin::same(2)).show(ui, |ui| {
                    ui.set_width(320.0);
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    for (k, it) in c.items.iter().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
                        let sel = k == c.selected;
                        if sel {
                            ui.painter().rect_filled(r, CornerRadius::ZERO, v.suggest_sel);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::ZERO, v.hover);
                        }
                        let ic = Pos2::new(r.min.x + 12.0, r.center().y);
                        ui.painter().rect_filled(Rect::from_center_size(ic, Vec2::splat(11.0)), CornerRadius::same(2), v.kind_color(it.kind).gamma_multiply(0.85));
                        ui.painter().text(Pos2::new(r.min.x + 28.0, r.center().y), Align2::LEFT_CENTER, &it.label, FontId::proportional(13.0), v.text);
                        ui.painter().text(Pos2::new(r.max.x - 8.0, r.center().y), Align2::RIGHT_CENTER, it.detail.trim_start_matches('~'), FontId::proportional(11.5), v.dim);
                        if resp.clicked() {
                            clicked = Some(k);
                        }
                    }
                });
            });
            if let Some(k) = clicked {
                if let Some(c) = st.comp.take() {
                    if let Some(item) = c.items.get(k) {
                        let e = ce::accept(&doc.text, &c.word, doc.cursor.1, item);
                        doc.text = e.text;
                        write_range(&ctx, id, e.anchor, e.head);
                        st.want_focus = true;
                    }
                }
            }
        } else {
            st.comp = None;
        }
    }
    // a click elsewhere or moving away from the word closes the list
    if let Some(c) = &st.comp {
        if !text_changed && !force_complete {
            let head = doc.cursor.1;
            if head < c.word.start || head > c.word.start + 64 {
                st.comp = None;
            }
        }
    }

    // the find box
    if st.find.is_some() {
        find_widget(ui, rect, st, doc, &ctx, id, mini_w);
    }
}

/// The scrollbar slider colours of the theme: (resting, hovered, dragged).
fn vsc_slider(v: &Pal) -> (Color32, Color32, Color32) {
    let base = v.scroll;
    (base.0, base.1, base.2)
}

fn minimap(ui: &mut Ui, rect: Rect, st: &mut St, doc: &Doc, spans: &Arc<Vec<script::HlSpan>>) {
    let v = pal();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::ZERO, v.bg);
    p.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0, v.edge));
    let h = hash_text(&doc.text);
    let segs = match &st.mini {
        Some((k, v)) if *k == h => v.clone(),
        _ => {
            let idx = script::LineIndex::new(&doc.text);
            let mut list: Vec<MiniSeg> = Vec::new();
            for s in spans.iter() {
                if matches!(s.class, script::Class::Punct) && s.end - s.start == 1 {
                    continue;
                }
                let (l, c) = idx.line_col(&doc.text, s.start);
                let (l2, c2) = idx.line_col(&doc.text, s.end);
                if l == l2 {
                    list.push(MiniSeg { line: l as u32, col: c as u32, len: (c2 - c) as u32, color: v.class_color(s.class) });
                }
            }
            let a = Arc::new(list);
            st.mini = Some((h, a.clone()));
            a
        }
    };
    let line_step = 3.0;
    let lines = doc.text.lines().count().max(1) as f32;
    let total_h = lines * line_step;
    // the minimap follows the scroll when the text is longer than it
    let editor_lines_h = st.content_h.max(1.0);
    let ratio = (st.scroll_y / editor_lines_h).clamp(0.0, 1.0);
    let offset = if total_h > rect.height() { (total_h - rect.height()) * ratio } else { 0.0 };
    for s in segs.iter() {
        let y = rect.min.y + s.line as f32 * line_step - offset;
        if y < rect.min.y - 3.0 || y > rect.max.y {
            continue;
        }
        let x = rect.min.x + 6.0 + s.col as f32 * 1.1;
        if x > rect.max.x - 2.0 {
            continue;
        }
        p.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new((s.len as f32 * 1.1).min(rect.max.x - x - 2.0), 2.0)), CornerRadius::ZERO, s.color.gamma_multiply(0.75));
    }
    // the part that is shown in the editor
    let (vis_top, vis_h) = if editor_lines_h > 0.0 { (st.scroll_y / editor_lines_h * total_h - offset, st.view_h / editor_lines_h * total_h) } else { (0.0, 0.0) };
    let slider = Rect::from_min_size(Pos2::new(rect.min.x + 1.0, rect.min.y + vis_top), Vec2::new(rect.width() - 1.0, vis_h.max(10.0))).intersect(rect);
    let resp = ui.interact(rect, Id::new("script-minimap"), Sense::click_and_drag());
    p.rect_filled(slider, CornerRadius::ZERO, if resp.hovered() || resp.dragged() { v.slider_hover } else { v.slider });
    if resp.dragged() || resp.clicked() {
        if let Some(pt) = resp.interact_pointer_pos() {
            // the pointer's line becomes the middle of the view
            let line = ((pt.y - rect.min.y + offset) / line_step).max(0.0);
            let frac = (line / lines).clamp(0.0, 1.0);
            st.mini_scroll = Some((frac * editor_lines_h - st.view_h / 2.0).max(0.0));
        }
    }
}

fn find_widget(ui: &mut Ui, rect: Rect, st: &mut St, doc: &mut Doc, ctx: &egui::Context, text_id: Id, mini_w: f32) {
    let v = pal();
    let Some(find) = &mut st.find else { return };
    let matches = ce::find_all(&doc.text, &find.query, find.case);
    let w = 360.0;
    let pos = Pos2::new(rect.max.x - mini_w - w - 22.0, rect.min.y + 4.0);
    let mut close = false;
    let mut step: i32 = 0;
    egui::Area::new(Id::new("script-find")).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::NONE.fill(v.sidebar).stroke(Stroke::new(1.0, v.widget_border)).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
            ui.set_width(w);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let te = ui.add(TextEdit::singleline(&mut find.query).hint_text("Find").desired_width(170.0).text_color(v.text).background_color(v.input_bg));
                if find.fresh {
                    te.request_focus();
                    find.fresh = false;
                }
                if te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    step = if ui.input(|i| i.modifiers.shift) { -1 } else { 1 };
                    te.request_focus();
                }
                let n = matches.len();
                let label = if find.query.is_empty() { "".to_string() } else if n == 0 { "No results".to_string() } else { format!("{} of {}", find.current % n + 1, n) };
                ui.label(egui::RichText::new(label).size(12.0).color(if n == 0 && !find.query.is_empty() { v.error } else { v.text }));
                if ui.small_button("Aa").on_hover_text("Match case").clicked() {
                    find.case = !find.case;
                }
                if ui.small_button("^").on_hover_text("Previous match (Shift+Enter)").clicked() {
                    step = -1;
                }
                if ui.small_button("v").on_hover_text("Next match (Enter)").clicked() {
                    step = 1;
                }
                if ui.small_button("x").on_hover_text("Close (Escape)").clicked() {
                    close = true;
                }
            });
        });
    });
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        close = true;
    }
    if step != 0 && !matches.is_empty() {
        let n = matches.len() as i32;
        let cur = (find.current as i32 + step).rem_euclid(n) as usize;
        find.current = cur;
        let (a, _) = matches[cur];
        // the editor scrolls to it on the next frame
        st.scroll_line = Some(ce::line_col(&doc.text, a).0);
    }
    if close {
        // the match that was current becomes the selection
        if !matches.is_empty() {
            st.select_range = Some(matches[find.current % matches.len()]);
        }
        st.find = None;
        st.want_focus = true;
    }
    let _ = ui;
}

// ---------------------------------------------------------------------------------------------------------------- problems, status bar

fn problems_panel(ui: &mut Ui, rect: Rect, st: &mut St, docs: &[Doc], acts: &mut Vec<Act>) {
    let v = pal();
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::ZERO, v.bg);
    let n_problems: usize = docs.iter().map(|d| d.problems().count()).sum();
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 34.0));
    let mut x = rect.min.x + 18.0;
    for (tab, label, badge) in [(Bottom::Problems, "PROBLEMS", n_problems), (Bottom::Output, "OUTPUT", 0)] {
        let g = p.layout_no_wrap(label.to_string(), FontId::proportional(11.5), v.text);
        let w = g.size().x + if badge > 0 { 28.0 } else { 0.0 };
        let r = Rect::from_min_size(Pos2::new(x, head.min.y), Vec2::new(w, head.height()));
        let resp = ui.interact(r, Id::new(("script-bottom-tab", label)), Sense::click());
        let active = st.bottom == tab;
        p.text(Pos2::new(r.min.x, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(11.5), if active { v.bright } else { v.dim });
        if badge > 0 {
            let br = Rect::from_center_size(Pos2::new(r.max.x - 8.0, r.center().y), Vec2::new(18.0, 15.0));
            p.rect_filled(br, CornerRadius::same(8), v.badge_bg);
            p.text(br.center(), Align2::CENTER_CENTER, badge.to_string(), FontId::proportional(10.5), v.badge_fg);
        }
        if active {
            p.line_segment([Pos2::new(r.min.x, head.max.y - 1.0), Pos2::new(r.max.x, head.max.y - 1.0)], Stroke::new(1.0, v.bright));
        }
        if resp.clicked() {
            st.bottom = tab;
        }
        x += w + 26.0;
    }
    let close = Rect::from_center_size(Pos2::new(rect.max.x - 18.0, head.center().y), Vec2::splat(18.0));
    let cr = ui.interact(close, Id::new("script-bottom-close"), Sense::click());
    let c = close.center();
    let s = Stroke::new(1.3, if cr.hovered() { v.bright } else { v.dim });
    p.line_segment([c + Vec2::new(-3.5, -3.5), c + Vec2::new(3.5, 3.5)], s);
    p.line_segment([c + Vec2::new(-3.5, 3.5), c + Vec2::new(3.5, -3.5)], s);
    if cr.clicked() {
        st.problems_open = false;
    }
    let body = Rect::from_min_max(rect.min + Vec2::new(0.0, 34.0), rect.max);
    ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        egui::ScrollArea::vertical().id_salt("script-problems").auto_shrink([false, false]).show(ui, |ui| match st.bottom {
            Bottom::Problems => {
                if n_problems == 0 {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.label(egui::RichText::new("No problems have been detected in the open scripts.").size(12.5).color(v.dim));
                    });
                }
                for (di, d) in docs.iter().enumerate() {
                    for pr in d.problems() {
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::ZERO, v.hover);
                        }
                        let ic = Pos2::new(r.min.x + 28.0, r.center().y);
                        ui.painter().circle_filled(ic, 6.0, v.error);
                        ui.painter().text(ic, Align2::CENTER_CENTER, "x", FontId::proportional(9.0), Color32::WHITE);
                        let msg = ui.painter().text(Pos2::new(r.min.x + 44.0, r.center().y), Align2::LEFT_CENTER, &pr.message, FontId::proportional(13.0), v.text);
                        ui.painter().text(Pos2::new(msg.max.x + 10.0, r.center().y), Align2::LEFT_CENTER, format!("{} [Ln {}, Col {}]", d.title, pr.line, pr.col), FontId::proportional(12.0), v.dim);
                        if resp.clicked() {
                            acts.push(Act::Reveal(di, pr.line.saturating_sub(1), pr.col.saturating_sub(1)));
                        }
                    }
                }
            }
            Bottom::Output => {
                for l in &st.output {
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.label(egui::RichText::new(l).font(FontId::monospace(12.5)).color(v.text));
                    });
                }
            }
        });
    });
}

fn status_bar(ui: &mut Ui, rect: Rect, st: &mut St, docs: &[Doc], cmds: &mut Vec<Cmd>) {
    let v = pal();
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::ZERO, v.status);
    let white = v.status_fg;
    let n_problems: usize = docs.iter().map(|d| d.problems().count()).sum();
    // problems counter (click to open the panel)
    let r = Rect::from_min_size(rect.min, Vec2::new(70.0, rect.height()));
    let resp = ui.interact(r, Id::new("script-status-problems"), Sense::click());
    if resp.hovered() {
        p.rect_filled(r, CornerRadius::ZERO, v.status_hover);
    }
    p.circle_stroke(Pos2::new(r.min.x + 16.0, r.center().y), 5.0, Stroke::new(1.2, white));
    p.text(Pos2::new(r.min.x + 27.0, r.center().y), Align2::LEFT_CENTER, n_problems.to_string(), FontId::proportional(12.0), white);
    p.add(Shape::convex_polygon(vec![Pos2::new(r.min.x + 50.0, r.center().y + 4.0), Pos2::new(r.min.x + 56.0, r.center().y - 5.0), Pos2::new(r.min.x + 62.0, r.center().y + 4.0)], Color32::TRANSPARENT, Stroke::new(1.1, white)));
    if resp.clicked() {
        cmds.push(Cmd::ToggleProblems);
    }
    if let Some(d) = docs.get(st.active) {
        let state = if d.problems().next().is_some() {
            "Has problems"
        } else if d.applied == hash_text(&d.text) {
            "Applied"
        } else {
            "Not applied (Ctrl+S)"
        };
        p.text(Pos2::new(r.max.x + 10.0, rect.center().y), Align2::LEFT_CENTER, state, FontId::proportional(12.0), white);
        let (l, c) = ce::line_col(&d.text, d.cursor.1);
        let sel = d.cursor.0.abs_diff(d.cursor.1);
        let pos_label = if sel > 0 { format!("Ln {}, Col {} ({} selected)", l + 1, c + 1, sel) } else { format!("Ln {}, Col {}", l + 1, c + 1) };
        let mut x = rect.max.x - 12.0;
        for label in ["Lunaris Script", "LF", "UTF-8", "Spaces: 4", pos_label.as_str()] {
            let rr = p.text(Pos2::new(x, rect.center().y), Align2::RIGHT_CENTER, label, FontId::proportional(12.0), white);
            x = rr.min.x - 18.0;
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------- the palette

fn palette_widget(ui: &mut Ui, full: Rect, st: &mut St, docs: &[Doc], acts: &mut Vec<Act>, cmds: &mut Vec<Cmd>) {
    let v = pal();
    let ctx = ui.ctx().clone();
    let Some(pal) = &mut st.palette else { return };
    let w = 560.0f32.min(full.width() - 40.0);
    let pos = Pos2::new(full.center().x - w / 2.0, full.min.y + 6.0);
    // what the query matches
    let q = pal.query.to_lowercase();
    let mut items: Vec<(String, String, Choice)> = Vec::new();
    match pal.mode {
        PaletteMode::Commands => {
            if let Some(num) = q.strip_prefix(':') {
                let n: usize = num.trim().parse().unwrap_or(0);
                items.push((if n > 0 { format!("Go to line {n}") } else { "Type a line number to go to".to_string() }, String::new(), if n > 0 { Choice::Goto(n - 1) } else { Choice::None }));
            } else {
                let q = q.trim_start_matches('>').trim().to_string();
                for (c, label, key) in COMMANDS {
                    if q.is_empty() || label.to_lowercase().contains(&q) {
                        items.push((label.to_string(), key.to_string(), Choice::Command(*c)));
                    }
                }
            }
        }
        PaletteMode::Themes => {
            for t in &st.themes {
                if q.is_empty() || t.label.to_lowercase().contains(&q) {
                    items.push((t.label.clone(), if t.path.is_none() { "built in".to_string() } else { "VS Code".to_string() }, Choice::Theme(t.label.clone())));
                }
            }
        }
        PaletteMode::Files => {
            for (name, path) in &st.files {
                if q.is_empty() || name.to_lowercase().contains(&q) {
                    items.push((name.clone(), "file".to_string(), Choice::File(path.clone())));
                }
            }
            for (name, _) in &st.systems {
                if q.is_empty() || name.to_lowercase().contains(&q) {
                    items.push((format!("{name}"), "system in the scene".to_string(), Choice::System(name.clone())));
                }
            }
            for (i, d) in docs.iter().enumerate() {
                if d.path.is_none() && (q.is_empty() || d.title.to_lowercase().contains(&q)) {
                    items.push((d.title.clone(), "open".to_string(), Choice::Doc(i)));
                }
            }
        }
    }
    if pal.selected >= items.len() {
        pal.selected = items.len().saturating_sub(1);
    }
    let mut chosen: Option<Choice> = None;
    let mut close = false;
    ctx.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            pal.selected = (pal.selected + 1) % items.len().max(1);
        }
        if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            pal.selected = (pal.selected + items.len().max(1) - 1) % items.len().max(1);
        }
        if i.consume_key(Modifiers::NONE, Key::Escape) {
            close = true;
        }
    });
    egui::Area::new(Id::new("script-palette")).fixed_pos(pos).order(egui::Order::Foreground).show(&ctx, |ui| {
        egui::Frame::NONE.fill(v.widget_bg).stroke(Stroke::new(1.0, v.widget_border)).shadow(egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(120) }).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
            ui.set_width(w);
            let hint = match pal.mode {
                PaletteMode::Files => "Search scripts by name",
                PaletteMode::Themes => "Select Color Theme (Up/Down Keys to Preview)",
                PaletteMode::Commands => "Type a command",
            };
            let te = ui.add(TextEdit::singleline(&mut pal.query).hint_text(hint).desired_width(f32::INFINITY).text_color(v.text).background_color(v.input_bg));
            if pal.fresh {
                te.request_focus();
                pal.fresh = false;
                // the end of a prefilled ':'
                if let Some(mut s) = TextEdit::load_state(ui.ctx(), te.id) {
                    s.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(pal.query.chars().count()))));
                    TextEdit::store_state(ui.ctx(), te.id, s);
                }
            }
            if te.changed() {
                pal.selected = 0;
            }
            if te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                chosen = items.get(pal.selected).map(|x| x.2.clone());
                if chosen.is_none() {
                    close = true;
                }
            }
            ui.add_space(3.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            for (k, (label, detail, choice)) in items.iter().enumerate().take(12) {
                let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
                if k == pal.selected {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, v.list_select);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, v.hover);
                }
                ui.painter().text(Pos2::new(r.min.x + 10.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(13.0), v.text);
                ui.painter().text(Pos2::new(r.max.x - 10.0, r.center().y), Align2::RIGHT_CENTER, detail, FontId::proportional(11.5), v.dim);
                if resp.clicked() {
                    chosen = Some(choice.clone());
                }
            }
            if items.is_empty() {
                ui.label(egui::RichText::new("No matching results").size(12.5).color(v.dim));
            }
        });
    });
    // a click outside closes it
    if ctx.input(|i| i.pointer.any_pressed()) {
        if let Some(pt) = ctx.input(|i| i.pointer.interact_pos()) {
            if !Rect::from_min_size(pos, Vec2::new(w, 320.0)).contains(pt) {
                close = true;
            }
        }
    }
    // a theme is shown while its name is highlighted, like VS Code does
    if st.palette.as_ref().is_some_and(|p| p.mode == PaletteMode::Themes) {
        let sel = st.palette.as_ref().map(|p| p.selected).unwrap_or(0);
        if let Some((label, _, _)) = items.get(sel) {
            if *label != st.theme_now {
                use_theme(label);
                st.theme_now = label.clone();
            }
        }
    }
    if let Some(c) = chosen {
        st.palette = None;
        match c {
            Choice::Theme(l) => {
                use_theme(&l);
                st.theme_saved = l.clone();
                st.theme_now = l.clone();
                write_pref(&st.root, &l);
            }
            Choice::Command(cmd) => cmds.push(cmd),
            Choice::Goto(line) => acts.push(Act::Reveal(st.active, line, 0)),
            Choice::File(p) => acts.push(Act::OpenFile(p)),
            Choice::System(n) => acts.push(Act::OpenSystem(n)),
            Choice::Doc(i) => acts.push(Act::Activate(i)),
            Choice::None => {}
        }
    } else if close {
        if st.theme_now != st.theme_saved {
            use_theme(&st.theme_saved.clone());
            st.theme_now = st.theme_saved.clone();
        }
        st.palette = None;
        st.want_focus = true;
    }
}

#[derive(Clone)]
enum Choice {
    Theme(String),
    Command(Cmd),
    Goto(usize),
    File(PathBuf),
    System(String),
    Doc(usize),
    None,
}
