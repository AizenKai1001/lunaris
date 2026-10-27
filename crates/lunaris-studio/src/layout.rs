//! The editor's layout (the panels' sizes, which panels are shown, the bottom tab) kept between runs in `.lunaris/editor.json`, the file that also holds the script editor's theme.

use crate::state::BottomTab;
use lunaris_json::Json;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Panel sizes in points, the smallest and the largest a panel may have (the drag edges clamp to the same).
pub(crate) const INSPECTOR: (f32, f32) = (240.0, 640.0);
pub(crate) const HIERARCHY: (f32, f32) = (160.0, 480.0);

/// The Systems tab needs room for the graph; the other tabs are lists.
pub(crate) fn bottom_range(tab: BottomTab) -> (f32, f32) {
    if tab == BottomTab::Systems {
        (340.0, 900.0)
    } else {
        (90.0, 600.0)
    }
}

/// A write at most this often: a drag changes the size on every frame.
const WRITE_EVERY: Duration = Duration::from_millis(1000);

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PanelLayout {
    pub inspector_width: f32,
    pub hierarchy_width: f32,
    pub bottom_height: f32,
    pub show_hierarchy: bool,
    pub show_inspector: bool,
    pub show_bottom: bool,
    pub tab: BottomTab,
}

impl Default for PanelLayout {
    fn default() -> PanelLayout {
        PanelLayout { inspector_width: 330.0, hierarchy_width: 250.0, bottom_height: 240.0, show_hierarchy: true, show_inspector: true, show_bottom: true, tab: BottomTab::Project }
    }
}

const TABS: [(BottomTab, &str); 7] = [
    (BottomTab::Project, "project"),
    (BottomTab::Console, "console"),
    (BottomTab::Systems, "systems"),
    (BottomTab::Profiler, "profiler"),
    (BottomTab::History, "history"),
    (BottomTab::Streaming, "streaming"),
    (BottomTab::Problems, "problems"),
];

pub(crate) fn tab_name(tab: BottomTab) -> &'static str {
    TABS.iter().find(|(t, _)| *t == tab).map(|(_, n)| *n).unwrap_or("project")
}

pub(crate) fn tab_from_name(name: &str) -> Option<BottomTab> {
    TABS.iter().find(|(_, n)| *n == name).map(|(t, _)| *t)
}

fn file(root: &Path) -> PathBuf {
    root.join(".lunaris").join("editor.json")
}

impl PanelLayout {
    /// What a hand-edited or older file may hold is kept inside the limits: a missing or odd value is the default.
    pub fn clamped(mut self) -> PanelLayout {
        self.inspector_width = self.inspector_width.clamp(INSPECTOR.0, INSPECTOR.1);
        self.hierarchy_width = self.hierarchy_width.clamp(HIERARCHY.0, HIERARCHY.1);
        let (lo, hi) = bottom_range(self.tab);
        self.bottom_height = self.bottom_height.clamp(lo, hi);
        self
    }

    pub fn load(root: &Path) -> PanelLayout {
        let mut out = PanelLayout::default();
        let Some(doc) = std::fs::read_to_string(file(root)).ok().and_then(|t| lunaris_json::parse(&t).ok()) else {
            return out;
        };
        let Some(l) = doc.get("layout") else {
            return out;
        };
        let num = |key: &str, into: &mut f32| {
            if let Some(v) = l.get(key).and_then(Json::as_f64).filter(|v| v.is_finite()) {
                *into = v as f32;
            }
        };
        num("inspector_width", &mut out.inspector_width);
        num("hierarchy_width", &mut out.hierarchy_width);
        num("bottom_height", &mut out.bottom_height);
        let flag = |key: &str, into: &mut bool| {
            if let Some(v) = l.get(key).and_then(Json::as_bool) {
                *into = v;
            }
        };
        flag("show_hierarchy", &mut out.show_hierarchy);
        flag("show_inspector", &mut out.show_inspector);
        flag("show_bottom", &mut out.show_bottom);
        if let Some(t) = l.get("tab").and_then(Json::as_str).and_then(tab_from_name) {
            out.tab = t;
        }
        out.clamped()
    }

    /// Write the layout into the file, keeping every other key of it (the script editor's theme).
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let path = file(root);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut doc = std::fs::read_to_string(&path).ok().and_then(|t| lunaris_json::parse(&t).ok()).filter(|d| d.as_object().is_some()).unwrap_or_else(|| Json::obj::<String>([]));
        let whole = |v: f32| Json::Float((v as f64).round());
        doc.set(
            "layout",
            Json::obj([
                ("inspector_width", whole(self.inspector_width)),
                ("hierarchy_width", whole(self.hierarchy_width)),
                ("bottom_height", whole(self.bottom_height)),
                ("show_hierarchy", Json::Bool(self.show_hierarchy)),
                ("show_inspector", Json::Bool(self.show_inspector)),
                ("show_bottom", Json::Bool(self.show_bottom)),
                ("tab", Json::str(tab_name(self.tab))),
            ]),
        );
        let text = doc.to_string_pretty().map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::write(path, text)
    }
}

/// Remembers what was last on disk and writes again, rarely, when the layout in use differs from it. Nothing is written for a layout that was never changed, so opening a project to look at it leaves no file behind.
pub(crate) struct Saver {
    saved: PanelLayout,
    at: Option<Instant>,
}

impl Saver {
    pub fn new(loaded: PanelLayout) -> Saver {
        Saver { saved: loaded, at: None }
    }

    /// Called every frame with the layout in use. Returns true when it wrote.
    pub fn tick(&mut self, root: &Path, now: &PanelLayout, force: bool) -> bool {
        if *now == self.saved || (!force && self.at.is_some_and(|t| t.elapsed() < WRITE_EVERY)) {
            return false;
        }
        self.at = Some(Instant::now());
        // (a failed write waits for the next change: the folder may be read-only, which is no reason to try again every second)
        self.saved = now.clone();
        now.save(root).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lunaris-layout-unit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".lunaris")).unwrap();
        d
    }

    #[test]
    fn a_missing_or_damaged_file_gives_the_defaults() {
        let d = dir("none");
        assert_eq!(PanelLayout::load(&d), PanelLayout::default());
        std::fs::write(d.join(".lunaris").join("editor.json"), "not json at all {").unwrap();
        assert_eq!(PanelLayout::load(&d), PanelLayout::default());
        std::fs::write(d.join(".lunaris").join("editor.json"), r#"[1,2,3]"#).unwrap();
        assert_eq!(PanelLayout::load(&d), PanelLayout::default());
    }

    #[test]
    fn odd_values_are_kept_inside_the_limits_or_ignored() {
        let d = dir("odd");
        std::fs::write(d.join(".lunaris").join("editor.json"), r#"{"layout":{"inspector_width":99999,"hierarchy_width":"wide","bottom_height":-5,"tab":"nonsense","show_bottom":false}}"#).unwrap();
        let l = PanelLayout::load(&d);
        assert_eq!(l.inspector_width, INSPECTOR.1);
        assert_eq!(l.hierarchy_width, PanelLayout::default().hierarchy_width);
        assert_eq!(l.bottom_height, bottom_range(BottomTab::Project).0);
        assert_eq!(l.tab, BottomTab::Project);
        assert!(!l.show_bottom && l.show_hierarchy && l.show_inspector);
    }

    #[test]
    fn a_saved_layout_loads_back_and_the_file_keeps_its_other_keys() {
        let d = dir("round");
        std::fs::write(d.join(".lunaris").join("editor.json"), r#"{"script_theme":"Solarized"}"#).unwrap();
        let l = PanelLayout { inspector_width: 401.4, hierarchy_width: 199.0, bottom_height: 512.0, show_hierarchy: false, show_inspector: true, show_bottom: false, tab: BottomTab::Systems };
        l.save(&d).unwrap();
        let back = PanelLayout::load(&d);
        assert_eq!((back.inspector_width, back.hierarchy_width, back.bottom_height), (401.0, 199.0, 512.0));
        assert_eq!((back.show_hierarchy, back.show_inspector, back.show_bottom, back.tab), (false, true, false, BottomTab::Systems));
        let text = std::fs::read_to_string(d.join(".lunaris").join("editor.json")).unwrap();
        assert!(text.contains("\"script_theme\"") && text.contains("Solarized"), "{text}");
    }

    #[test]
    fn the_saver_writes_a_change_once_and_not_again_for_the_same_layout() {
        let d = dir("saver");
        let mut s = Saver::new(PanelLayout::default());
        let mut l = PanelLayout::default();
        assert!(!s.tick(&d, &l, false), "nothing changed, nothing written");
        l.inspector_width = 400.0;
        assert!(s.tick(&d, &l, false));
        assert!(!s.tick(&d, &l, true), "the same layout is not written twice");
        // a second change within the second waits (a drag changes the size on every frame), unless forced (closing the editor)
        l.inspector_width = 410.0;
        assert!(!s.tick(&d, &l, false));
        assert!(s.tick(&d, &l, true));
        assert_eq!(PanelLayout::load(&d).inspector_width, 410.0);
    }
}
