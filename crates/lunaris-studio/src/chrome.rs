//! The furniture around the panels, as Unity has it: a strip of tabs over every panel, the toolbar rows, section headers.
use crate::theme;
use eframe::egui::{self, Align, Color32, CornerRadius, FontId, Frame, Layout, Margin, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};

/// The strip with the tab titles of a panel (dark, the active tab as light as the panel under it). `right` fills the right end.
/// Returns the tab that was clicked, if any.
pub fn tab_strip<T: PartialEq + Copy>(ui: &mut Ui, tabs: &[(T, &str)], active: T, right: impl FnOnce(&mut Ui)) -> Option<T> {
    let mut clicked = None;
    let strip = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), 22.0));
    ui.painter().rect_filled(strip, CornerRadius::ZERO, theme::TAB_STRIP);
    ui.painter().line_segment([strip.left_bottom(), strip.right_bottom()], Stroke::new(1.0, theme::LINE));
    ui.scope_builder(UiBuilder::new().max_rect(strip).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (t, label) in tabs {
            let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(12.5), theme::TEXT);
            let w = galley.size().x + 22.0;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 22.0), Sense::click());
            let is = *t == active;
            if is {
                ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::PANEL);
                ui.painter().line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0, theme::LINE));
                ui.painter().line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0, theme::LINE));
                ui.painter().line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.5, theme::BLUE));
                ui.painter().line_segment([rect.left_bottom() + Vec2::new(1.0, 0.0), rect.right_bottom() + Vec2::new(-1.0, 0.0)], Stroke::new(1.0, theme::PANEL));
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::BUTTON_ACTIVE.gamma_multiply(0.6));
            }
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(12.5), if is { theme::TEXT_BRIGHT } else { theme::TEXT_DIM });
            if resp.clicked() {
                clicked = Some(*t);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(4.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            right(ui);
        });
    });
    ui.advance_cursor_after_rect(strip);
    clicked
}

/// A panel's content area: the panel colour with a margin.
pub fn body() -> Frame {
    Frame::new().fill(theme::PANEL).inner_margin(Margin::same(4))
}

/// The outer frame of a docked panel: no margin (the tab strip touches the edges), a dark line around.
pub fn dock(fill: Color32) -> Frame {
    Frame::new().fill(fill).inner_margin(Margin::same(0)).stroke(Stroke::new(1.0, theme::LINE))
}

/// A foldout bar (the title of a component in the inspector): an arrow, an icon, a title. Returns whether it is open and the bar's response
/// (clicking the bar opens and closes it; the response is for a context menu).
pub fn foldout(ui: &mut Ui, id: egui::Id, title: &str, icon: crate::icons::Icon, default_open: bool) -> (bool, egui::Response) {
    let open_id = id.with("open");
    let mut open = ui.data_mut(|d| d.get_persisted::<bool>(open_id)).unwrap_or(default_open);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
    let fill = if resp.hovered() { Color32::from_rgb(70, 70, 70) } else { theme::HEADER };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
    ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, theme::LINE));
    // the arrow
    let c = egui::pos2(rect.min.x + 10.0, rect.center().y);
    let pts = if open { vec![c + Vec2::new(-4.0, -2.5), c + Vec2::new(4.0, -2.5), c + Vec2::new(0.0, 3.0)] } else { vec![c + Vec2::new(-2.5, -4.0), c + Vec2::new(-2.5, 4.0), c + Vec2::new(3.0, 0.0)] };
    ui.painter().add(egui::Shape::convex_polygon(pts, theme::TEXT_DIM, Stroke::NONE));
    crate::icons::draw(ui.painter(), Rect::from_center_size(egui::pos2(rect.min.x + 30.0, rect.center().y), Vec2::splat(16.0)), icon, icon.tint());
    ui.painter().text(egui::pos2(rect.min.x + 46.0, rect.center().y), egui::Align2::LEFT_CENTER, title, FontId::proportional(13.0), theme::TEXT_BRIGHT);
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_persisted(open_id, open));
    }
    (open, resp)
}

/// A thin vertical rule in a toolbar.
pub fn rule(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(7.0, 22.0), Sense::hover());
    ui.painter().line_segment([rect.center_top() + Vec2::new(0.0, 3.0), rect.center_bottom() - Vec2::new(0.0, 3.0)], Stroke::new(1.0, theme::LINE));
}

/// A drop-down style button that opens a menu (Unity's "Shaded v", "Layers v").
pub fn drop_down(ui: &mut Ui, label: &str, add_contents: impl FnOnce(&mut Ui)) {
    let r = ui.add(egui::Button::new(egui::RichText::new(format!("{label}  v")).size(12.0)).min_size(Vec2::new(0.0, 20.0)));
    let id = ui.make_persistent_id(("dropdown", label));
    egui::Popup::menu(&r).id(id).show(add_contents);
}

pub fn dim(text: &str) -> egui::RichText {
    egui::RichText::new(text).small().color(theme::TEXT_DIM)
}

pub fn selected_stroke() -> Stroke {
    Stroke::new(1.0, theme::BLUE)
}

pub fn outline_rect(painter: &egui::Painter, r: Rect, color: Color32) {
    painter.rect_stroke(r, CornerRadius::same(1), Stroke::new(1.5, color), StrokeKind::Outside);
}
