//! The Unreal Editor look: a flat, near-black neutral theme (panels #242424, toolbars #111111 to #171717, fields #0f0f0f, bands #2f2f2f, rows striped #1a1a1a and #151515, a slate-blue
//! selection #40576f and a saturated blue for what is active), small type, tab strips over every panel. The values were read off the screenshots of Epic's own documentation
//! (the editor layout page of Unreal Engine 5): the editor is not made of Unreal's code or artwork.
use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle};
use std::sync::Arc;

/// The window and the panels' background.
pub const PANEL: Color32 = Color32::from_rgb(36, 36, 36);
/// The toolbar and the menu bar.
pub const TOOLBAR: Color32 = Color32::from_rgb(22, 22, 22);
/// The strip with the tabs of a panel.
pub const TAB_STRIP: Color32 = Color32::from_rgb(26, 26, 26);
/// Text fields and everything that is "inside" something.
pub const FIELD: Color32 = Color32::from_rgb(15, 15, 15);
/// The lines between panels and around fields.
pub const LINE: Color32 = Color32::from_rgb(12, 12, 12);
pub const BUTTON: Color32 = Color32::from_rgb(47, 47, 47);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(64, 64, 64);
pub const BUTTON_ACTIVE: Color32 = Color32::from_rgb(30, 30, 30);
/// A selected row.
pub const SELECT: Color32 = Color32::from_rgb(64, 87, 111);
/// The blue of the Play button while playing, and of what is on or active.
pub const PLAY_BLUE: Color32 = Color32::from_rgb(0, 112, 224);
pub const BLUE: Color32 = Color32::from_rgb(26, 140, 240);
pub const HOVER_ROW: Color32 = Color32::from_rgb(46, 46, 46);
/// The band of a category in the inspector, the header of a list and the status line under it.
pub const HEADER: Color32 = Color32::from_rgb(47, 47, 47);
pub const ROW_ALT: Color32 = Color32::from_rgb(41, 41, 41);
/// The rows of a list (the outliner, the content browser's tree): two shades in turn, on a darker ground than the panel.
pub const LIST_A: Color32 = Color32::from_rgb(26, 26, 26);
pub const LIST_B: Color32 = Color32::from_rgb(21, 21, 21);
pub const TEXT: Color32 = Color32::from_rgb(192, 192, 192);
pub const TEXT_DIM: Color32 = Color32::from_rgb(128, 128, 128);
pub const TEXT_BRIGHT: Color32 = Color32::from_rgb(232, 232, 232);
pub const OK: Color32 = Color32::from_rgb(110, 200, 130);
pub const WARN: Color32 = Color32::from_rgb(235, 190, 90);
pub const ERR: Color32 = Color32::from_rgb(230, 90, 80);
/// The outline of the selected object in the scene view.
pub const OUTLINE: Color32 = Color32::from_rgb(255, 150, 30);
pub const AXIS_X: Color32 = Color32::from_rgb(219, 64, 52);
pub const AXIS_Y: Color32 = Color32::from_rgb(141, 200, 46);
pub const AXIS_Z: Color32 = Color32::from_rgb(59, 128, 222);

// names the older panels used
pub const ACCENT: Color32 = BLUE;
pub const ACCENT_SOFT: Color32 = SELECT;
pub const BG: Color32 = Color32::from_rgb(32, 32, 32);
pub const PANEL_DARK: Color32 = TOOLBAR;
pub const BORDER: Color32 = LINE;

/// Segoe UI (the system's own font) when there is one: it is the type Unity's editor uses on Windows, and it reads better at small sizes
/// than the font egui brings.
fn fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, file) in [("segoe", "segoeui.ttf"), ("segoe-bold", "segoeuib.ttf")] {
        let path = std::path::Path::new("C:\\Windows\\Fonts").join(file);
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(name.to_string(), Arc::new(FontData::from_owned(bytes)));
            if name == "segoe" {
                fonts.families.entry(FontFamily::Proportional).or_default().insert(0, name.to_string());
            }
        }
    }
    // the bold face for the titles of nodes (the family always exists: without the font it is the usual one)
    let mut bold: Vec<String> = Vec::new();
    if fonts.font_data.contains_key("segoe-bold") {
        bold.push("segoe-bold".to_string());
    }
    bold.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Name("segoe-bold".into()), bold);
    // Consolas for code, as the code editors of Windows use it
    if let Ok(bytes) = std::fs::read(std::path::Path::new("C:\\Windows\\Fonts").join("consola.ttf")) {
        fonts.font_data.insert("consolas".to_string(), Arc::new(FontData::from_owned(bytes)));
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "consolas".to_string());
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    fonts(ctx);
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = Color32::from_rgb(34, 34, 34);
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = ROW_ALT;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = SELECT;
    v.selection.stroke = Stroke::new(1.0, BLUE);
    v.hyperlink_color = BLUE;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(3);
    v.menu_corner_radius = CornerRadius::same(2);
    v.popup_shadow = egui::epaint::Shadow { offset: [2, 4], blur: 10, spread: 0, color: Color32::from_black_alpha(110) };
    let r = CornerRadius::same(2);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.bg_fill = BUTTON;
    v.widgets.inactive.weak_bg_fill = BUTTON;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(18, 18, 18));
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_fill = BUTTON_HOVER;
    v.widgets.hovered.weak_bg_fill = BUTTON_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(18, 18, 18));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_BRIGHT);
    v.widgets.active.bg_fill = BUTTON_ACTIVE;
    v.widgets.active.weak_bg_fill = BUTTON_ACTIVE;
    v.widgets.active.bg_stroke = Stroke::new(1.0, BLUE);
    v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_BRIGHT);
    v.widgets.open.bg_fill = BUTTON_ACTIVE;
    v.widgets.open.weak_bg_fill = BUTTON_ACTIVE;
    ctx.set_visuals(v);
    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(5.0, 3.0);
    style.spacing.button_padding = egui::vec2(7.0, 2.0);
    style.spacing.interact_size.y = 20.0;
    style.spacing.menu_margin = egui::Margin::same(4);
    style.spacing.indent = 14.0;
    style.spacing.scroll = egui::style::ScrollStyle { bar_width: 9.0, ..egui::style::ScrollStyle::solid() };
    style.text_styles.insert(TextStyle::Body, FontId::new(13.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Button, FontId::new(13.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Heading, FontId::new(15.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace));
    ctx.set_global_style(style);
}

/// A 64x64 window icon: a crescent moon on a dark rounded square.
pub fn icon() -> egui::IconData {
    let n = 64usize;
    let mut rgba = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (dx, dy) = ((fx - 32.0).abs() - 24.0, (fy - 32.0).abs() - 24.0);
            let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - 8.0;
            if outside > 0.0 {
                continue;
            }
            let moon = ((fx - 30.0).powi(2) + (fy - 32.0).powi(2)).sqrt() < 18.0;
            let bite = ((fx - 40.0).powi(2) + (fy - 27.0).powi(2)).sqrt() < 16.0;
            let c = if moon && !bite { [224u8, 230, 255] } else { [38u8, 44, 70] };
            let i = (y * n + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    egui::IconData { rgba, width: n as u32, height: n as u32 }
}
