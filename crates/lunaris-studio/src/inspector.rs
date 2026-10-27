//! The inspector: every component of the selected entity as editable fields, built from what the engine says each component is
//! (`schema`), so a component added tomorrow, or defined by a game, shows up with no editor code. Every edit is a `set`.
use crate::icons::Icon;
use crate::mathx::{q_from_euler, q_to_euler};
use crate::state::{Entity, FieldDef, Studio};
use crate::theme;
use eframe::egui::{self, Color32, RichText};
use lunaris_json::Json;

pub struct InspectorUi {
    epoch: u64,
    /// The search box under the name: only the fields (and categories) whose names contain this are shown.
    pub search: String,
    /// Where the reset arrows were drawn last frame, by `Component.field` (for tests).
    pub resets: Vec<(String, egui::Pos2)>,
    pub files: Vec<(String, String)>,
    files_loaded: bool,
}

impl Default for InspectorUi {
    fn default() -> InspectorUi {
        InspectorUi { epoch: 0, search: String::new(), resets: Vec::new(), files: Vec::new(), files_loaded: false }
    }
}

/// Fields whose whole numbers are really choices.
fn choices(comp: &str, field: &str) -> Option<&'static [&'static str]> {
    match (comp, field) {
        ("Light", "kind") => Some(&["Directional (sun)", "Point", "Spot"]),
        ("Collider", "shape") => Some(&["Sphere", "Box"]),
        ("Environment", "tonemap") => Some(&["Filmic", "Reinhard", "Clamp", "Linear"]),
        _ => None,
    }
}

/// The icon of a component's bar.
fn component_icon(name: &str) -> Icon {
    match name {
        "Transform" | "LocalTransform" | "Parent" => Icon::Move,
        "MeshRenderer" => Icon::Cube,
        "Camera" => Icon::Camera,
        "Light" => Icon::Light,
        "Collider" | "RigidBody" | "PhysicsSettings" | "CharacterBody" => Icon::Collider,
        "Sprite" | "Animator" => Icon::Sprite,
        "Text" => Icon::Text,
        "Ui" => Icon::Ui,
        "Emitter" | "Particle" => Icon::Emitter,
        "AudioSource" | "SoundFile" | "Sound" | "AcousticMaterial" => Icon::Audio,
        "Tilemap" => Icon::Tilemap,
        "Environment" => Icon::Environment,
        _ => Icon::Gear,
    }
}

const BUILTIN_MESHES: [&str; 5] = ["cube", "sphere", "plane", "cylinder", "quad"];
const BUILTIN_TEXTURES: [&str; 9] = ["checker", "bricks", "grass", "wood", "stone", "tiles", "sand", "metal", "noise"];

fn num(v: Option<&Json>) -> f64 {
    v.and_then(|x| x.as_f64()).unwrap_or(0.0)
}

fn nums(v: Option<&Json>, n: usize) -> Vec<f64> {
    let mut out: Vec<f64> = v.and_then(|a| a.as_array()).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect()).unwrap_or_default();
    out.resize(n, 0.0);
    out
}

fn jarr(v: &[f64]) -> Json {
    Json::arr(v.iter().map(|x| Json::from(*x)))
}

fn is_color(field: &str, count: usize) -> bool {
    (count == 3 || count == 4) && (field == "color" || field.ends_with("_color") || field.starts_with("color") || matches!(field, "sky_top" | "sky_horizon" | "ground" | "clear" | "back_color" | "border_color" | "color0" | "color1"))
}

fn speed_for(v: f64) -> f64 {
    (v.abs() * 0.01).clamp(0.005, 1.0)
}

impl InspectorUi {
    fn group(&self, uid: u64, comp: &str, field: &str) -> String {
        format!("insp-{uid}-{comp}-{field}-{}", self.epoch)
    }

    fn load_files(&mut self, studio: &mut Studio) {
        if self.files_loaded {
            return;
        }
        self.files_loaded = true;
        if let Ok(f) = studio.engine.call("files", &Json::obj([("recursive", Json::Bool(true)), ("limit", Json::from(2000))])) {
            for e in f.get("files").and_then(|x| x.as_array()).unwrap_or(&[]) {
                if let (Some(p), Some(k)) = (e.get("path").and_then(|p| p.as_str()), e.get("kind").and_then(|k| k.as_str())) {
                    self.files.push((p.to_string(), k.to_string()));
                }
            }
        }
    }

    pub fn refresh_files(&mut self) {
        self.files.clear();
        self.files_loaded = false;
    }

    /// Note where the reset arrow of the row `key` (`Component.field`) was drawn (if it was); true when it was clicked.
    fn arrow_clicked(&mut self, key: &str, arrow: Option<egui::Response>) -> bool {
        match arrow {
            Some(r) => {
                self.resets.push((key.to_string(), r.rect.center()));
                r.clicked()
            }
            None => false,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio) {
        self.resets.clear();
        // a new undo step starts when the pointer is released or Enter is pressed
        if ui.input(|i| i.pointer.any_released() || i.key_pressed(egui::Key::Enter)) {
            self.epoch += 1;
        }
        let Some(ent) = studio.entity.clone() else {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("Nothing selected").color(theme::TEXT_DIM)));
            ui.vertical_centered(|ui| ui.label(RichText::new("Click something in the scene or the hierarchy.").small().color(theme::TEXT_DIM)));
            return;
        };
        self.load_files(studio);
        if studio.selected.len() > 1 {
            ui.label(RichText::new(format!("{} entities selected: this is the one selected last; a value you change goes to every selected entity that has the component, and Move, Duplicate and Delete act on all of them", studio.selected.len())).small().color(theme::TEXT_DIM));
        }
        // name and the actions
        ui.horizontal(|ui| {
            let mut name = ent.name();
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(ui.available_width() - 120.0));
            if r.changed() {
                let g = self.group(ent.uid, "Name", "value");
                studio.set(ent.uid, "Name", Json::obj([("value", Json::str(name))]), Some(&g));
            }
            ui.label(RichText::new(format!("#{}", ent.uid)).small().color(theme::TEXT_DIM));
            if ui.small_button("Copy").on_hover_text("Duplicate (Ctrl+D)").clicked() {
                studio.duplicate(ent.uid);
            }
            if ui.small_button("Del").on_hover_text("Delete (Del)").clicked() {
                studio.delete(ent.uid);
            }
        });
        // the search box (Unreal's Details panel has one): it narrows what is shown to the fields whose names contain what is typed
        crate::chrome::search_box(ui, &mut self.search, "Search", ui.available_width());
        let query = self.search.trim().to_lowercase();
        ui.add_space(2.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("inspector-scroll").show(ui, |ui| {
            let attached = ent.parent().is_some();
            let mut remove: Option<String> = None;
            let mut copy: Option<String> = None;
            let mut paste = false;
            for (name, value) in &ent.comps {
                if name == "Name" {
                    continue;
                }
                let def = studio.def(name);
                // (a component whose name matches shows everything; otherwise only the fields that match, and nothing when none does)
                let comp_matches = query.is_empty() || name.to_lowercase().contains(&query);
                let shown: Vec<&FieldDef> = def.fields.iter().filter(|f| comp_matches || f.name.to_lowercase().replace('_', " ").contains(&query)).collect();
                let transform_matches = name == "Transform" && ["position", "rotation", "scale"].iter().any(|k| k.contains(&query));
                if !query.is_empty() && shown.is_empty() && !transform_matches {
                    continue;
                }
                let (open, bar) = crate::chrome::foldout(ui, egui::Id::new(("comp", ent.uid, name)), name, component_icon(name), true);
                let can_paste = studio.component_clipboard.as_ref().is_some_and(|(n, _)| n == name);
                bar.context_menu(|ui| {
                    if ui.button("Copy values").on_hover_text("Copy the values of this component to paste them on this or other entities").clicked() {
                        copy = Some(name.clone());
                        ui.close();
                    }
                    if ui.add_enabled(can_paste, egui::Button::new("Paste values")).on_hover_text("Put the copied values on every selected entity").clicked() {
                        paste = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Remove component").clicked() {
                        remove = Some(name.clone());
                        ui.close();
                    }
                });
                if !def.doc.is_empty() {
                    let first: String = def.doc.split(". ").next().unwrap_or("").chars().take(300).collect();
                    bar.on_hover_text(first);
                }
                if open {
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                        egui::Grid::new(("grid", ent.uid, name)).num_columns(2).spacing([8.0, 4.0]).min_col_width(96.0).striped(true).show(ui, |ui| {
                            if name == "Transform" {
                                self.transform(ui, studio, &ent, value, attached, def.defaults.as_ref());
                            } else {
                                for f in shown {
                                    self.field(ui, studio, &ent, name, f, value.get(&f.name), def.defaults.as_ref().and_then(|d| d.get(&f.name)));
                                }
                            }
                        });
                    });
                }
            }
            if let Some(c) = remove {
                let _ = studio.try_call("remove_component", Json::obj([("uid", Json::from(ent.uid as i64)), ("component", Json::str(c))]));
            }
            if let Some(c) = copy {
                studio.copy_component(ent.uid, &c);
            }
            if paste {
                studio.paste_component();
            }
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                if let Some((n, _)) = studio.component_clipboard.clone() {
                    if ui.button(format!("Paste {n}")).on_hover_text("Add the copied component, with its values, to every selected entity").clicked() {
                        studio.paste_component();
                    }
                }
                ui.menu_button("Add Component", |ui| {
                    let names = studio.components_available();
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for n in names {
                            if ent.comp(&n).is_some() {
                                continue;
                            }
                            if ui.button(&n).clicked() {
                                let _ = studio.try_call("add_component", Json::obj([("uid", Json::from(ent.uid as i64)), ("component", Json::str(n))]));
                                ui.close();
                            }
                        }
                    });
                });
            });
            ui.add_space(24.0);
        });
    }

    fn transform(&mut self, ui: &mut egui::Ui, studio: &mut Studio, ent: &Entity, value: &Json, attached: bool, defaults: Option<&Json>) {
        let uid = ent.uid;
        let pos = nums(value.get("pos"), 3);
        let rot = nums(value.get("rot"), 4);
        let scale = {
            let mut s = nums(value.get("scale"), 3);
            if value.get("scale").is_none() {
                s = vec![1.0; 3];
            }
            s
        };
        if attached {
            ui.label("");
            ui.label(RichText::new("attached: edit LocalTransform").small().color(theme::TEXT_DIM));
            ui.end_row();
        }
        ui.add_enabled_ui(!attached, |ui| {
            ui.label("Position");
        });
        let mut p = pos.clone();
        let off_pos = !attached && differs(value.get("pos"), defaults.and_then(|d| d.get("pos")));
        let (changed, arrow) = ui.add_enabled_ui(!attached, |ui| vec3_row(ui, &mut p, 0.05, ["X", "Y", "Z"], off_pos)).inner;
        if self.arrow_clicked("Transform.pos", arrow) {
            let g = self.group(uid, "Transform", "pos");
            studio.set_selected(uid, "Transform", Json::obj([("pos", defaults.and_then(|d| d.get("pos")).cloned().unwrap_or_else(|| jarr(&[0.0, 0.0, 0.0])))]), Some(&g));
        }
        ui.end_row();
        if changed {
            let g = self.group(uid, "Transform", "pos");
            studio.set_selected(uid, "Transform", Json::obj([("pos", jarr(&p))]), Some(&g));
        }
        ui.add_enabled_ui(!attached, |ui| {
            ui.label("Rotation");
        });
        let q = [rot[0] as f32, rot[1] as f32, rot[2] as f32, rot[3] as f32];
        let e = q_to_euler(q);
        let mut ed = [e[0] as f64, e[1] as f64, e[2] as f64];
        let off_rot = !attached && differs(value.get("rot"), defaults.and_then(|d| d.get("rot")));
        let (changed, arrow) = ui.add_enabled_ui(!attached, |ui| vec3_row(ui, &mut ed, 0.5, ["X", "Y", "Z"], off_rot)).inner;
        if self.arrow_clicked("Transform.rot", arrow) {
            let g = self.group(uid, "Transform", "rot");
            studio.set_selected(uid, "Transform", Json::obj([("rot", defaults.and_then(|d| d.get("rot")).cloned().unwrap_or_else(|| jarr(&[0.0, 0.0, 0.0, 1.0])))]), Some(&g));
        }
        ui.end_row();
        if changed {
            let nq = q_from_euler([ed[0] as f32, ed[1] as f32, ed[2] as f32]);
            let g = self.group(uid, "Transform", "rot");
            studio.set_selected(uid, "Transform", Json::obj([("rot", jarr(&[nq[0] as f64, nq[1] as f64, nq[2] as f64, nq[3] as f64]))]), Some(&g));
        }
        ui.add_enabled_ui(!attached, |ui| {
            ui.label("Scale");
        });
        let mut s = scale;
        let off_scale = !attached && differs(value.get("scale"), defaults.and_then(|d| d.get("scale")));
        let (changed, arrow) = ui.add_enabled_ui(!attached, |ui| vec3_row(ui, &mut s, 0.02, ["X", "Y", "Z"], off_scale)).inner;
        if self.arrow_clicked("Transform.scale", arrow) {
            let g = self.group(uid, "Transform", "scale");
            studio.set_selected(uid, "Transform", Json::obj([("scale", defaults.and_then(|d| d.get("scale")).cloned().unwrap_or_else(|| jarr(&[1.0, 1.0, 1.0])))]), Some(&g));
        }
        ui.end_row();
        if changed {
            let g = self.group(uid, "Transform", "scale");
            studio.set_selected(uid, "Transform", Json::obj([("scale", jarr(&s))]), Some(&g));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn field(&mut self, ui: &mut egui::Ui, studio: &mut Studio, ent: &Entity, comp: &str, f: &FieldDef, v: Option<&Json>, default: Option<&Json>) {
        let uid = ent.uid;
        let label = f.name.replace('_', " ");
        let label = format!("{}{}", &label[..1].to_uppercase(), &label[1..]);
        ui.label(label);
        let set = |this: &Self, studio: &mut Studio, value: Json| {
            let g = this.group(uid, comp, &f.name);
            studio.set_selected(uid, comp, Json::obj([(f.name.as_str(), value)]), Some(&g));
        };
        // (the widget gets the width of the cell less the room of the arrow; the arrow follows it on the rows whose value is not the one a new component starts with: Unreal's "reset to default")
        let mut arrow: Option<egui::Response> = None;
        ui.horizontal(|ui| {
            let room = (ui.available_width() - 24.0).max(60.0);
            ui.scope(|ui| {
                ui.set_max_width(room);
            match f.ty.as_str() {
                "bool" => {
                    let mut b = v.and_then(|x| x.as_bool()).unwrap_or(false);
                    if ui.checkbox(&mut b, "").changed() {
                        set(self, studio, Json::Bool(b));
                    }
                }
                "str" => {
                    let mut s = v.and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let list: Option<(Vec<String>, &str)> = match (comp, f.name.as_str()) {
                        ("MeshRenderer", "mesh") => Some((BUILTIN_MESHES.iter().map(|s| s.to_string()).chain(self.files.iter().filter(|(_, k)| k == "model").map(|(p, _)| p.clone())).collect(), "mesh")),
                        ("MeshRenderer", "texture") | ("Sprite", "texture") | ("Ui", "texture") => Some((BUILTIN_TEXTURES.iter().map(|s| s.to_string()).chain(self.files.iter().filter(|(_, k)| k == "texture").map(|(p, _)| p.clone())).collect(), "texture")),
                        (_, "file") => Some((self.files.iter().filter(|(_, k)| k == "sound").map(|(p, _)| p.clone()).collect(), "sound file")),
                        (_, "voice") | (_, "name") if comp == "Sound" || comp == "SoundFile" || comp == "AudioSource" => Some((["coin", "hit", "jump", "win", "lose", "click", "shoot", "explosion"].iter().map(|s| s.to_string()).collect(), "built-in sound")),
                        _ => None,
                    };
                    ui.horizontal(|ui| {
                        let w = (ui.available_width() - if list.is_some() { 30.0 } else { 0.0 }).max(60.0);
                        let r = ui.add(egui::TextEdit::singleline(&mut s).desired_width(w));
                        if r.changed() {
                            set(self, studio, Json::str(s.clone()));
                        }
                        if let Some((options, what)) = list {
                            ui.menu_button("...", |ui| {
                                ui.label(RichText::new(format!("Choose a {what}")).small().color(theme::TEXT_DIM));
                                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                                    for o in options {
                                        if ui.button(&o).clicked() {
                                            set(self, studio, Json::str(o));
                                            ui.close();
                                        }
                                    }
                                });
                            });
                        }
                    });
                }
                "uid" => {
                    let mut n = v.and_then(|x| x.as_u64()).unwrap_or(0);
                    ui.horizontal(|ui| {
                        if ui.add(egui::DragValue::new(&mut n).speed(0.2)).changed() {
                            set(self, studio, Json::from(n as i64));
                        }
                        if let Some(sel) = studio.selection {
                            if sel != uid && ui.small_button("<- selection").clicked() {
                                set(self, studio, Json::from(sel as i64));
                            }
                        }
                    });
                }
                "f32" | "f64" => {
                    if f.count <= 1 {
                        let mut x = num(v);
                        let sp = speed_for(x);
                        if ui.add(egui::DragValue::new(&mut x).speed(sp).max_decimals(4)).changed() {
                            set(self, studio, Json::from(x));
                        }
                    } else {
                        let mut xs = nums(v, f.count);
                        ui.horizontal(|ui| {
                            let mut changed = false;
                            if is_color(&f.name, f.count) {
                                let to8 = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
                                let mut c = Color32::from_rgba_unmultiplied(to8(xs[0]), to8(xs[1]), to8(xs[2]), if f.count == 4 { to8(xs[3]) } else { 255 });
                                let before = c;
                                let alpha = if f.count == 4 { egui::color_picker::Alpha::OnlyBlend } else { egui::color_picker::Alpha::Opaque };
                                if egui::color_picker::color_edit_button_srgba(ui, &mut c, alpha).changed() && c != before {
                                    let [r, g, b, a] = c.to_srgba_unmultiplied();
                                    xs[0] = r as f64 / 255.0;
                                    xs[1] = g as f64 / 255.0;
                                    xs[2] = b as f64 / 255.0;
                                    if f.count == 4 {
                                        xs[3] = a as f64 / 255.0;
                                    }
                                    changed = true;
                                }
                            }
                            let w = ((ui.available_width() - 6.0 * f.count as f32) / f.count as f32).clamp(36.0, 90.0);
                            for x in xs.iter_mut() {
                                let sp = speed_for(*x);
                                if ui.add_sized([w, 20.0], egui::DragValue::new(x).speed(sp).max_decimals(3)).changed() {
                                    changed = true;
                                }
                            }
                            if changed {
                                set(self, studio, jarr(&xs));
                            }
                        });
                    }
                }
                _ => {
                    // whole numbers
                    if f.count <= 1 {
                        let mut x = num(v);
                        if let Some(labels) = choices(comp, &f.name) {
                            let mut i = x as usize;
                            egui::ComboBox::from_id_salt(("choice", uid, comp, &f.name)).selected_text(labels.get(i).copied().unwrap_or("?")).show_ui(ui, |ui| {
                                for (k, l) in labels.iter().enumerate() {
                                    if ui.selectable_value(&mut i, k, *l).clicked() {
                                        set(self, studio, Json::from(k as i64));
                                    }
                                }
                            });
                        } else if ui.add(egui::DragValue::new(&mut x).speed(0.2).max_decimals(0)).changed() {
                            set(self, studio, Json::from(x.round() as i64));
                        }
                    } else {
                        let mut xs = nums(v, f.count);
                        ui.horizontal(|ui| {
                            let mut changed = false;
                            for x in xs.iter_mut() {
                                if ui.add(egui::DragValue::new(x).speed(0.2).max_decimals(0)).changed() {
                                    changed = true;
                                }
                            }
                            if changed {
                                set(self, studio, Json::arr(xs.iter().map(|x| Json::from(x.round() as i64))));
                            }
                        });
                    }
                }
            }
            });
            if differs(v, default) {
                arrow = Some(reset_button(ui));
            }
        });
        if let Some(r) = arrow {
            self.resets.push((format!("{comp}.{}", f.name), r.rect.center()));
            if r.clicked() {
                if let Some(d) = default {
                    let g = self.group(uid, comp, &f.name);
                    studio.set_selected(uid, comp, Json::obj([(f.name.as_str(), d.clone())]), Some(&g));
                }
            }
        }
        ui.end_row();
    }
}

/// Is `cur` something other than `default` (numbers within a millionth, arrays element by element)? A field with no known default never differs.
fn differs(cur: Option<&Json>, default: Option<&Json>) -> bool {
    fn same(a: &Json, b: &Json) -> bool {
        match (a, b) {
            (Json::Arr(x), Json::Arr(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
            _ => match (a.as_f64(), b.as_f64()) {
                (Some(p), Some(q)) => (p - q).abs() <= 1e-6 * (1.0 + p.abs().max(q.abs())),
                _ => a == b,
            },
        }
    }
    match (cur, default) {
        (Some(c), Some(d)) => !same(c, d),
        _ => false,
    }
}

/// The arrow that sets a field back to its default: a ring open at the top right with an arrowhead, dim until the pointer is over it.
fn reset_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(16.0), egui::Sense::click());
    let color = if resp.hovered() { theme::TEXT_BRIGHT } else { theme::TEXT_DIM };
    let (c, r) = (rect.center(), 4.6f32);
    let at = |deg: f32| c + egui::vec2(r * deg.to_radians().cos(), r * deg.to_radians().sin());
    let pts: Vec<egui::Pos2> = (0..=14).map(|k| at(40.0 + k as f32 * 20.0)).collect();
    ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(1.4, color)));
    // the head, at the end of the ring, pointing the way it runs
    let (end, dir) = (at(320.0), egui::vec2(-(320f32.to_radians().sin()), 320f32.to_radians().cos()));
    let side = egui::vec2(-dir.y, dir.x);
    ui.painter().add(egui::Shape::convex_polygon(vec![end + dir * 3.4, end - dir * 0.6 + side * 3.0, end - dir * 0.6 - side * 3.0], color, egui::Stroke::NONE));
    resp.on_hover_text("Reset to default")
}

/// Three small drag boxes with X, Y, Z in their colours; true when one was changed.
fn vec3_row(ui: &mut egui::Ui, v: &mut [f64], speed: f64, labels: [&str; 3], reset: bool) -> (bool, Option<egui::Response>) {
    let mut changed = false;
    let mut arrow = None;
    ui.horizontal(|ui| {
        let w = ((ui.available_width() - 42.0) / 3.0).clamp(46.0, 100.0);
        for (i, l) in labels.iter().enumerate() {
            let r = ui.add_sized([w, 20.0], egui::DragValue::new(&mut v[i]).speed(speed).max_decimals(3));
            if r.changed() {
                changed = true;
            }
            // (no letter: the edge of the box is red, green or blue, as in Unreal's Details panel; the letter is in the tooltip)
            let edge = egui::Rect::from_min_max(r.rect.min, egui::pos2(r.rect.min.x + 3.0, r.rect.max.y));
            ui.painter().rect_filled(edge, egui::CornerRadius { nw: 2, sw: 2, ne: 0, se: 0 }, [theme::AXIS_X, theme::AXIS_Y, theme::AXIS_Z][i]);
            r.on_hover_text(*l);
        }
        if reset {
            arrow = Some(reset_button(ui));
        }
    });
    (changed, arrow)
}
