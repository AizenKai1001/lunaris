//! The Place panel (Unreal's "Place Actors"): the things the Entity menu can make, as rows. Click one to add it in front of the camera; drag one into the scene view to add it where it is
//! dropped (the view does the dropping: `Viewport::template_drop`).
use crate::chrome;
use crate::hierarchy::kind_color;
use crate::paint::{Brush, BrushItem};
use crate::state::{Kind, Studio, Template};
use crate::theme;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Rect, Sense, Vec2};

/// What is being dragged out of the Place panel (the egui drag-and-drop payload): something the Entity menu makes, or a prefab of the scene.
#[derive(Clone, Debug, PartialEq)]
pub enum PlaceDrag {
    Template(Template),
    Prefab(String),
}

impl PlaceDrag {
    /// The name shown on the chip that hangs at the pointer.
    pub fn label(&self) -> &str {
        match self {
            PlaceDrag::Template(t) => t.label(),
            PlaceDrag::Prefab(n) => n,
        }
    }
}

/// The kind of entity a template makes (for the colour and the letter of its row).
pub fn kind_of(t: Template) -> Kind {
    match t {
        Template::Empty => Kind::Empty,
        Template::Cube | Template::Sphere | Template::Cylinder | Template::Plane => Kind::Mesh,
        Template::Camera => Kind::Camera,
        Template::Sun | Template::PointLight | Template::SpotLight => Kind::Light,
        Template::Environment => Kind::Environment,
        Template::Sprite => Kind::Sprite,
        Template::Text => Kind::Text,
        Template::UiPanel | Template::UiButton => Kind::Ui,
        Template::AudioSource => Kind::Audio,
        Template::Emitter => Kind::Emitter,
    }
}

/// How far above the point it is put on, a new thing stands: a unit cube, sphere or cylinder is centred on its origin so it rests on the surface at half its height, a light hangs
/// in the air, a camera is at head height.
pub fn lift(t: Template) -> f32 {
    match t {
        Template::Cube | Template::Sphere | Template::Cylinder => 0.5,
        Template::PointLight | Template::SpotLight => 2.0,
        Template::Camera => 1.5,
        _ => 0.0,
    }
}

/// Can the thing be sprinkled by the Paint brush? Whatever stands somewhere with a position of its own (not the floor plane, the camera, the sun, the sky or the screen-space things).
pub fn paintable(t: Template) -> bool {
    matches!(t, Template::Empty | Template::Cube | Template::Sphere | Template::Cylinder | Template::PointLight | Template::SpotLight | Template::Sprite | Template::AudioSource | Template::Emitter)
}

/// The heading a template is listed under (the ones the Entity menu has no group for are "Basic").
pub fn group_of(t: Template) -> &'static str {
    if t.group().is_empty() {
        "Basic"
    } else {
        t.group()
    }
}

const GROUPS: [&str; 7] = ["Basic", "3D Object", "Light", "2D", "UI", "Audio", "Effects"];

#[derive(Default)]
pub struct PlaceUi {
    filter: String,
    /// The revision of the scene the prefab rows were read at.
    prefab_rev: Option<u64>,
    /// Where each row was drawn last frame (for tests).
    rows: Vec<(PlaceDrag, Rect)>,
    /// A row's menu asked for this to be painted.
    paint_req: Option<Template>,
    /// Where the "Paint with this" entry of an open row menu was drawn (for tests).
    #[cfg_attr(not(test), allow(dead_code))]
    menu_item: Option<egui::Pos2>,
}

impl PlaceUi {
    /// Type something in the search box (for tests).
    #[cfg(test)]
    pub fn set_filter(&mut self, text: &str) {
        self.filter = text.to_string();
    }

    /// The middle of the row of `t`, as drawn last frame (for tests).
    pub fn row_center(&self, t: Template) -> Option<egui::Pos2> {
        self.rows.iter().find(|(r, _)| *r == PlaceDrag::Template(t)).map(|(_, r)| r.center())
    }

    /// The middle of the "Paint with this" entry of the row menu that is open (for tests).
    #[cfg(test)]
    pub fn menu_item_center(&self) -> Option<egui::Pos2> {
        self.menu_item
    }

    /// The middle of the row of the prefab `name`, as drawn last frame (for tests).
    pub fn prefab_center(&self, name: &str) -> Option<egui::Pos2> {
        self.rows.iter().find(|(r, _)| matches!(r, PlaceDrag::Prefab(n) if n == name)).map(|(_, r)| r.center())
    }

    /// The panel. A click on a row adds the thing at `at` (the middle of the view), a drag carries it to the scene view.
    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio, at: [f32; 3]) {
        let ctx = ui.ctx().clone();
        chrome::search_box(ui, &mut self.filter, "Search Classes", ui.available_width());
        ui.add_space(4.0);
        brush_bar(ui, studio);
        let filter = self.filter.to_lowercase();
        self.rows.clear();
        self.menu_item = None;
        if self.prefab_rev != Some(studio.live_revision) {
            studio.refresh_prefabs();
            self.prefab_rev = Some(studio.live_revision);
        }
        egui::ScrollArea::vertical().id_salt("place-rows").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for group in GROUPS {
                let items: Vec<Template> = Template::ALL.iter().copied().filter(|t| group_of(*t) == group && (filter.is_empty() || t.label().to_lowercase().contains(&filter) || group.to_lowercase().contains(&filter))).collect();
                if items.is_empty() {
                    continue;
                }
                // (the name of the category, centred and in capitals, as Unreal's Place Actors panel has it)
                ui.add_space(6.0);
                ui.vertical_centered(|ui| ui.label(egui::RichText::new(group.to_uppercase()).size(11.0).strong().color(theme::TEXT)));
                ui.add_space(2.0);
                for t in items {
                    let kind = kind_of(t);
                    if self.row(ui, &ctx, PlaceDrag::Template(t), kind.letter(), kind_color(kind)) {
                        studio.spawn_template(t, [at[0], at[1] + lift(t), at[2]]);
                    }
                }
            }
            // the prefabs of the scene (made with `define_prefab`)
            let prefabs: Vec<String> = studio.prefabs.iter().filter(|p| filter.is_empty() || p.to_lowercase().contains(&filter) || "prefabs".contains(&filter)).cloned().collect();
            if !prefabs.is_empty() {
                ui.add_space(6.0);
                ui.vertical_centered(|ui| ui.label(egui::RichText::new("PREFABS").size(11.0).strong().color(theme::TEXT)));
                ui.add_space(2.0);
                for p in prefabs {
                    if self.row(ui, &ctx, PlaceDrag::Prefab(p.clone()), "P", Color32::from_rgb(150, 210, 150)) {
                        studio.spawn_prefab(&p, at);
                    }
                }
            }
        });
        if let Some(t) = self.paint_req.take() {
            studio.brush = Some(Brush::new(BrushItem::Template(t)));
        }
        if let Some(d) = egui::DragAndDrop::payload::<PlaceDrag>(&ctx) {
            chrome::drag_chip(&ctx, d.label());
        }
    }

    /// One row: a coloured badge with a letter and the name. A drag starts the drag-and-drop; the answer is whether the row was clicked.
    fn row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, item: PlaceDrag, letter: &str, color: Color32) -> bool {
        // a card: a dark plate with the letter of the kind on the left, the name after it (Unreal draws the actor's picture on the plate)
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click_and_drag());
        ui.painter().rect_filled(rect, CornerRadius::same(4), if resp.hovered() { Color32::from_rgb(58, 58, 58) } else { Color32::from_rgb(47, 47, 47) });
        let plate = Rect::from_min_size(rect.min, Vec2::new(34.0, 34.0));
        ui.painter().rect_filled(plate, CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 }, Color32::from_rgb(30, 30, 30));
        ui.painter().circle_filled(plate.center(), 11.0, color.gamma_multiply(0.30));
        ui.painter().text(plate.center(), Align2::CENTER_CENTER, letter, FontId::new(14.0, egui::FontFamily::Name("segoe-bold".into())), color);
        ui.painter().text(egui::pos2(rect.min.x + 46.0, rect.center().y), Align2::LEFT_CENTER, item.label(), FontId::proportional(13.5), theme::TEXT_BRIGHT);
        ui.add_space(2.0);
        resp.clone().on_hover_text(format!("{}: click to add it in front of the camera, or drag it into the scene", item.label()));
        resp.context_menu(|ui| {
            if let PlaceDrag::Template(t) = &item {
                if paintable(*t) {
                    let entry = ui.button("Paint with this");
                    self.menu_item = Some(entry.rect.center());
                    if entry.clicked() {
                        self.paint_req = Some(*t);
                        ui.close();
                    }
                }
            }
        });
        if resp.drag_started() {
            egui::DragAndDrop::set_payload(ctx, item.clone());
        }
        self.rows.push((item, rect));
        resp.clicked()
    }
}

/// While a brush is armed: what is painted, the two numbers of the brush and the button that puts it down (Unreal's Foliage mode has these in its own panel).
fn brush_bar(ui: &mut egui::Ui, studio: &mut Studio) {
    let Some(brush) = studio.brush.as_mut() else { return };
    let mut stop = false;
    egui::Frame::new().fill(Color32::from_rgb(30, 34, 40)).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Painting {}", brush.label())).strong().color(theme::TEXT_BRIGHT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                stop = ui.small_button("Stop").on_hover_text("Stop painting (Esc in the scene view)").clicked();
            });
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Radius").color(theme::TEXT_DIM));
            ui.add(egui::DragValue::new(&mut brush.radius).range(0.2..=50.0).speed(0.05).suffix(" m"));
            ui.label(egui::RichText::new("Spacing").color(theme::TEXT_DIM));
            ui.add(egui::DragValue::new(&mut brush.spacing).range(0.1..=20.0).speed(0.02).suffix(" m"));
        });
        ui.label(egui::RichText::new("Drag over the scene view; one Undo takes a stroke back.").small().color(theme::TEXT_DIM));
    });
    ui.add_space(4.0);
    if stop {
        studio.brush = None;
    }
}
