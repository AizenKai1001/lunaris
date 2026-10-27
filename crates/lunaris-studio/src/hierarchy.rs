//! The hierarchy panel: every entity by name, children under the entity they are attached to; click selects, double-click renames,
//! the context menu duplicates, deletes, attaches and creates.
use crate::icons::{self, Icon};
use crate::state::{Kind, Studio, Template};
use crate::theme;
use eframe::egui::{self, Color32, CornerRadius, FontId, Sense, Stroke, Vec2};
use lunaris_json::Json;

pub struct HierarchyUi {
    renaming: Option<(u64, String)>,
    focus_rename: bool,
    dragging: Option<u64>,
    pub want_focus: bool,
    /// Where the eye of each row that has one was drawn last frame (for tests).
    eyes: Vec<(u64, egui::Pos2)>,
}

impl Default for HierarchyUi {
    fn default() -> HierarchyUi {
        HierarchyUi { renaming: None, focus_rename: false, dragging: None, want_focus: false, eyes: Vec::new() }
    }
}

/// An eye: an almond with a pupil when open, a lowered lid when closed (Unreal's visibility column).
fn eye(p: &egui::Painter, c: egui::Pos2, open: bool, color: Color32) {
    let curve = |sign: f32| -> Vec<egui::Pos2> { (-6..=6).map(|k| egui::pos2(c.x + k as f32, c.y + sign * (1.0 - (k as f32 / 6.0).powi(2)) * 3.4)).collect() };
    if open {
        let mut ring = curve(-1.0);
        ring.extend(curve(1.0).into_iter().rev());
        p.add(egui::Shape::closed_line(ring, Stroke::new(1.2, color)));
        p.circle_filled(c, 1.9, color);
    } else {
        p.add(egui::Shape::line(curve(1.0).into_iter().map(|q| egui::pos2(q.x, q.y - 1.5)).collect(), Stroke::new(1.2, color)));
        for k in [-4.0f32, 0.0, 4.0] {
            p.line_segment([egui::pos2(c.x + k, c.y + 1.0), egui::pos2(c.x + k * 1.1, c.y + 3.6)], Stroke::new(1.0, color));
        }
    }
}

pub(crate) fn kind_color(k: Kind) -> Color32 {
    match k {
        Kind::Empty => Color32::from_rgb(120, 126, 140),
        Kind::Camera => Color32::from_rgb(235, 220, 140),
        Kind::Light => Color32::from_rgb(255, 200, 90),
        Kind::Mesh => Color32::from_rgb(120, 170, 255),
        Kind::Sprite => Color32::from_rgb(130, 220, 170),
        Kind::Text | Kind::Ui => Color32::from_rgb(200, 150, 240),
        Kind::Emitter => Color32::from_rgb(255, 150, 120),
        Kind::Audio => Color32::from_rgb(120, 220, 230),
        Kind::Collider => Color32::from_rgb(170, 190, 130),
        Kind::Tilemap => Color32::from_rgb(150, 200, 120),
        Kind::Environment => Color32::from_rgb(140, 190, 240),
    }
}

/// The menu of things to create (used by the panel's `+` button, the context menu and the Entity menu).
pub fn create_menu(ui: &mut egui::Ui, studio: &mut Studio, at: [f32; 3]) {
    let mut groups: Vec<&str> = Vec::new();
    for t in Template::ALL {
        if t.group().is_empty() {
            if ui.button(t.label()).clicked() {
                studio.spawn_template(t, at);
                ui.close();
            }
        } else if !groups.contains(&t.group()) {
            groups.push(t.group());
        }
    }
    for g in groups {
        ui.menu_button(g, |ui| {
            for t in Template::ALL.iter().filter(|t| t.group() == g) {
                if ui.button(t.label()).clicked() {
                    studio.spawn_template(*t, at);
                    ui.close();
                }
            }
        });
    }
    let prefabs = {
        studio.components_available();
        studio.prefabs.clone()
    };
    if !prefabs.is_empty() {
        ui.separator();
        ui.menu_button("Prefab", |ui| {
            for p in prefabs {
                if ui.button(&p).clicked() {
                    studio.spawn_prefab(&p, at);
                    ui.close();
                }
            }
        });
    }
}

impl HierarchyUi {
    /// The middle of the eye of the row of `uid`, as drawn last frame (for tests).
    #[cfg(test)]
    pub fn eye_center(&self, uid: u64) -> Option<egui::Pos2> {
        self.eyes.iter().find(|(u, _)| *u == uid).map(|(_, p)| *p)
    }

    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio, spawn_at: [f32; 3]) {
        ui.horizontal(|ui| {
            let r = crate::chrome::search_box(ui, &mut studio.filter, "Search", ui.available_width() - 108.0);
            if r.changed() {
                studio.rebuild_tree();
            }
            // the type filter (Unreal's Outliner filters by what an actor is): only the entities of one kind, as a flat list like a search
            let label = studio.kind_filter.map_or("All types", type_name).to_string();
            let mut picked: Option<Option<Kind>> = None;
            crate::chrome::drop_down(ui, &label, |ui| {
                if ui.selectable_label(studio.kind_filter.is_none(), "All types").clicked() {
                    picked = Some(None);
                    ui.close();
                }
                for k in FILTER_KINDS {
                    let n = studio.all.iter().filter(|r| r.kind == k).count();
                    if n > 0 && ui.selectable_label(studio.kind_filter == Some(k), format!("{} ({n})", type_name(k))).clicked() {
                        picked = Some(Some(k));
                        ui.close();
                    }
                }
            });
            if let Some(k) = picked {
                studio.kind_filter = k;
                studio.rebuild_tree();
            }
            ui.menu_button("+", |ui| create_menu(ui, studio, spawn_at));
        });
        ui.add_space(2.0);
        // the scene: the root of the list
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            icons::draw(ui.painter(), r, Icon::Scene, Icon::Scene.tint());
            let name = studio.scene.as_deref().and_then(|s| s.rsplit('/').next()).unwrap_or("Untitled").trim_end_matches(".lunaris.json").to_string();
            ui.label(egui::RichText::new(format!("{}{}", name, if studio.dirty { "*" } else { "" })).strong());
        });
        ui.painter().line_segment([ui.cursor().left_top(), ui.cursor().right_top()], Stroke::new(1.0, theme::LINE));
        ui.add_space(2.0);
        let row_h = 20.0;
        let rows = studio.rows.clone();
        let total = rows.len();
        let mut clicked: Option<u64> = None;
        let mut toggle: Option<u64> = None;
        let mut delete: Option<u64> = None;
        let mut duplicate: Option<u64> = None;
        let mut attach: Option<(u64, Option<u64>)> = None;
        let mut start_rename: Option<(u64, String)> = None;
        let mut eye_click: Option<(u64, bool)> = None;
        self.eyes.clear();
        let selected = studio.selection;
        let chosen: Vec<u64> = studio.selected.clone();
        let mut scroll_to: Option<usize> = None;
        if self.want_focus {
            scroll_to = rows.iter().position(|r| Some(r.uid) == selected);
            self.want_focus = false;
        }
        // the header of the list ("Item Label", "Type"), then the list on a ground darker than the panel, then the status line under it (Unreal's Outliner)
        let footer_h = 20.0;
        let (header, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
        ui.painter().rect_filled(header, CornerRadius::ZERO, theme::HEADER);
        eye(ui.painter(), egui::pos2(header.min.x + 14.0, header.center().y), true, theme::TEXT_DIM);
        ui.painter().text(egui::pos2(header.min.x + 46.0, header.center().y), egui::Align2::LEFT_CENTER, "Item Label", FontId::proportional(12.0), theme::TEXT);
        let show_type = header.width() > 210.0;
        if show_type {
            ui.painter().text(egui::pos2(header.max.x - 74.0, header.center().y), egui::Align2::LEFT_CENTER, "Type", FontId::proportional(12.0), theme::TEXT);
        }
        let list_h = (ui.available_height() - footer_h).max(40.0);
        ui.painter().rect_filled(egui::Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), list_h)), CornerRadius::ZERO, theme::LIST_B);
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("hierarchy-scroll").max_height(list_h);
        if let Some(i) = scroll_to {
            area = area.vertical_scroll_offset((i as f32 * row_h - 80.0).max(0.0));
        }
        area.show_rows(ui, row_h, total, |ui, range| {
            for idx in range {
                let r = &rows[idx];
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click_and_drag());
                let is_sel = chosen.contains(&r.uid);
                ui.painter().rect_filled(rect, CornerRadius::ZERO, if idx % 2 == 0 { theme::LIST_A } else { theme::LIST_B });
                if is_sel {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::SELECT);
                    if selected == Some(r.uid) && chosen.len() > 1 {
                        // the main one of several: a bar at the left
                        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, Vec2::new(3.0, row_h)), CornerRadius::ZERO, theme::BLUE);
                    }
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::HOVER_ROW);
                }
                // the eye column: whether the meshes of the row (and of what is under it) are shown; a click shows or hides them all
                let hidden = r.meshes > 0 && r.shown == 0;
                let eye_rect = egui::Rect::from_center_size(egui::pos2(rect.min.x + 14.0, rect.center().y), Vec2::new(22.0, row_h));
                if r.meshes > 0 {
                    let on_eye = resp.hover_pos().is_some_and(|p| eye_rect.contains(p));
                    let color = if hidden { theme::TEXT_DIM.gamma_multiply(0.7) } else if on_eye { theme::TEXT_BRIGHT } else { theme::TEXT };
                    eye(ui.painter(), eye_rect.center(), !hidden, color);
                    self.eyes.push((r.uid, eye_rect.center()));
                    if resp.clicked() && resp.interact_pointer_pos().is_some_and(|p| eye_rect.contains(p)) {
                        eye_click = Some((r.uid, hidden));
                    }
                }
                let x0 = rect.min.x + 28.0 + r.depth as f32 * 14.0;
                // the fold arrow
                if r.children > 0 && studio.filter.is_empty() && studio.kind_filter.is_none() {
                    let open = studio.expanded.contains(&r.uid);
                    let arrow = egui::Rect::from_min_size(egui::pos2(x0, rect.min.y), Vec2::new(14.0, row_h));
                    ui.painter().text(arrow.center(), egui::Align2::CENTER_CENTER, if open { "v" } else { ">" }, FontId::monospace(11.0), theme::TEXT_DIM);
                    if resp.clicked() && resp.interact_pointer_pos().is_some_and(|p| arrow.contains(p)) {
                        toggle = Some(r.uid);
                    }
                }
                let ix = x0 + 16.0;
                let icon = Icon::of_kind(r.kind);
                icons::draw(ui.painter(), egui::Rect::from_center_size(egui::pos2(ix + 8.0, rect.center().y), Vec2::splat(16.0)), icon, icon.tint());
                let tx = ix + 22.0;
                if matches!(&self.renaming, Some((u, _)) if *u == r.uid) {
                    let mut finished = false;
                    if let Some((_, text)) = &mut self.renaming {
                        let edit_rect = egui::Rect::from_min_max(egui::pos2(tx - 2.0, rect.min.y + 1.0), egui::pos2(rect.max.x - 4.0, rect.max.y - 1.0));
                        let te = ui.put(edit_rect, egui::TextEdit::singleline(text).font(FontId::proportional(13.0)));
                        if self.focus_rename {
                            te.request_focus();
                            self.focus_rename = false;
                        }
                        if te.lost_focus() {
                            finished = true;
                        }
                    }
                    if finished {
                        if let Some((uid, text)) = self.renaming.take() {
                            if !text.trim().is_empty() {
                                studio.rename(uid, text.trim());
                            }
                        }
                    }
                } else {
                    let name_room = if show_type { rect.max.x - 78.0 } else { rect.max.x - 4.0 };
                    ui.painter().with_clip_rect(egui::Rect::from_min_max(rect.min, egui::pos2(name_room, rect.max.y))).text(egui::pos2(tx, rect.center().y), egui::Align2::LEFT_CENTER, &r.name, FontId::proportional(13.0), if (r.pos.is_none() && r.kind == Kind::Empty) || hidden { theme::TEXT_DIM } else { theme::TEXT });
                    if show_type {
                        ui.painter().text(egui::pos2(rect.max.x - 74.0, rect.center().y), egui::Align2::LEFT_CENTER, type_name(r.kind), FontId::proportional(12.0), theme::TEXT_DIM);
                    }
                }
                if resp.clicked() && !matches!(&self.renaming, Some((u, _)) if *u == r.uid) && !resp.interact_pointer_pos().is_some_and(|p| r.meshes > 0 && eye_rect.contains(p)) {
                    clicked = Some(r.uid);
                }
                if resp.double_clicked() {
                    start_rename = Some((r.uid, r.name.clone()));
                }
                if resp.drag_started() {
                    self.dragging = Some(r.uid);
                }
                // dropping something on a row attaches it there
                if self.dragging.is_some() && self.dragging != Some(r.uid) && resp.hovered() && ui.input(|i| i.pointer.any_released()) {
                    attach = Some((self.dragging.unwrap_or(0), Some(r.uid)));
                }
                resp.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        start_rename = Some((r.uid, r.name.clone()));
                        ui.close();
                    }
                    if ui.button("Duplicate    Ctrl+D").clicked() {
                        duplicate = Some(r.uid);
                        ui.close();
                    }
                    if ui.button("Delete    Del").clicked() {
                        delete = Some(r.uid);
                        ui.close();
                    }
                    ui.separator();
                    if r.parent.is_some() && ui.button("Detach from parent").clicked() {
                        attach = Some((r.uid, None));
                        ui.close();
                    }
                    ui.menu_button("Create", |ui| create_menu(ui, studio, spawn_at));
                });
            }
        });
        // drop on empty space detaches
        if self.dragging.is_some() && ui.input(|i| i.pointer.any_released()) {
            if attach.is_none() {
                let over_rows = ui.min_rect().contains(ui.input(|i| i.pointer.interact_pos().unwrap_or_default()));
                let _ = over_rows;
            }
            self.dragging = None;
        }
        if let Some(u) = clicked {
            let (ctrl, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
            if ctrl {
                studio.toggle_select(u);
            } else if shift && studio.selection.is_some() {
                // a range in the order of the list
                let order: Vec<u64> = rows.iter().map(|r| r.uid).collect();
                let (a, b) = (studio.selection.and_then(|s| order.iter().position(|x| *x == s)), order.iter().position(|x| *x == u));
                match (a, b) {
                    (Some(a), Some(b)) => studio.select_many(order[a.min(b)..=a.max(b)].to_vec(), Some(u)),
                    _ => studio.select(Some(u)),
                }
            } else {
                studio.select(Some(u));
            }
        }
        if let Some((u, show)) = eye_click {
            studio.set_shown(u, show);
        }
        if let Some(u) = toggle {
            if !studio.expanded.remove(&u) {
                studio.expanded.insert(u);
            }
            studio.rebuild_tree();
        }
        if let Some(r) = start_rename {
            studio.select(Some(r.0));
            self.renaming = Some(r);
            self.focus_rename = true;
        }
        // (the menu of a row that is one of several selected acts on all of them, of a row that is not, on that row)
        if let Some(u) = duplicate {
            if studio.is_selected(u) {
                studio.duplicate_selected();
            } else {
                studio.duplicate(u);
            }
        }
        if let Some(u) = delete {
            if studio.is_selected(u) {
                studio.delete_selected();
            } else {
                studio.delete(u);
            }
        }
        if let Some((child, parent)) = attach {
            attach_to(studio, child, parent);
        }
        if let Some(d) = self.dragging {
            // a hint while dragging
            if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                let name = studio.all.iter().find(|r| r.uid == d).map(|r| r.name.clone()).unwrap_or_default();
                ui.painter().text(p + Vec2::new(12.0, 8.0), egui::Align2::LEFT_TOP, format!("attach {name} to..."), FontId::proportional(12.0), theme::ACCENT);
            }
        }
        // the status line: how many, and how many of them are selected
        let (status, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), footer_h), Sense::hover());
        ui.painter().rect_filled(status, CornerRadius::ZERO, theme::HEADER);
        let shown = if studio.kind_filter.is_some() || !studio.filter.is_empty() { format!("{} of {} entities", total, studio.total_entities) } else { format!("{} entities", studio.total_entities) };
        let text = if chosen.len() > 1 { format!("{shown} ({} selected)", chosen.len()) } else { shown };
        ui.painter().text(egui::pos2(status.min.x + 8.0, status.center().y), egui::Align2::LEFT_CENTER, text, FontId::proportional(12.0), theme::TEXT_DIM);
    }
}

/// The kinds the type filter offers (those that have no entity are left out of the menu).
const FILTER_KINDS: [Kind; 12] = [Kind::Mesh, Kind::Light, Kind::Camera, Kind::Sprite, Kind::Tilemap, Kind::Text, Kind::Ui, Kind::Emitter, Kind::Audio, Kind::Collider, Kind::Environment, Kind::Empty];

/// What the Type column says for a kind of entity.
fn type_name(k: Kind) -> &'static str {
    match k {
        Kind::Empty => "Empty",
        Kind::Camera => "Camera",
        Kind::Light => "Light",
        Kind::Mesh => "Mesh",
        Kind::Sprite => "Sprite",
        Kind::Text => "Text",
        Kind::Ui => "Widget",
        Kind::Emitter => "Emitter",
        Kind::Audio => "Audio",
        Kind::Collider => "Collider",
        Kind::Tilemap => "Tilemap",
        Kind::Environment => "Environment",
    }
}

/// Attach `child` to `parent` (`None` detaches) without moving it: `Parent` and a `LocalTransform` worked out from the two positions.
pub fn attach_to(studio: &mut Studio, child: u64, parent: Option<u64>) {
    use crate::mathx::{q_conj, q_mul, q_rotate, sub};
    if parent == Some(child) {
        studio.toast("An entity cannot be attached to itself", true);
        return;
    }
    let get = |studio: &mut Studio, uid: u64| studio.try_call("get", Json::obj([("uid", Json::from(uid as i64))]));
    let Some(me) = get(studio, child) else { return };
    let group = studio.next_group();
    let jv = |a: &[f32]| Json::arr(a.iter().map(|x| Json::from(*x)));
    let Some(t) = me.get("components").and_then(|c| c.get("Transform")).cloned() else {
        studio.toast("The entity needs a Transform to be attached", true);
        return;
    };
    let f3 = |v: Option<&Json>, d: f32| v.and_then(|a| a.as_array()).filter(|a| a.len() >= 3).map(|a| [a[0].as_f64().unwrap_or(d as f64) as f32, a[1].as_f64().unwrap_or(d as f64) as f32, a[2].as_f64().unwrap_or(d as f64) as f32]).unwrap_or([d; 3]);
    let f4 = |v: Option<&Json>| v.and_then(|a| a.as_array()).filter(|a| a.len() == 4).map(|a| [a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32, a[3].as_f64().unwrap_or(1.0) as f32]).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let (pos, rot, scale) = (f3(t.get("pos"), 0.0), f4(t.get("rot")), f3(t.get("scale"), 1.0));
    match parent {
        None => {
            let _ = studio.try_call("remove_component", Json::obj([("uid", Json::from(child as i64)), ("component", Json::str("Parent")), ("undo_group", Json::str(group.clone()))]));
            let _ = studio.try_call("remove_component", Json::obj([("uid", Json::from(child as i64)), ("component", Json::str("LocalTransform")), ("undo_group", Json::str(group))]));
        }
        Some(p) => {
            let Some(par) = get(studio, p) else { return };
            let Some(pt) = par.get("components").and_then(|c| c.get("Transform")) else {
                studio.toast("The new parent needs a Transform", true);
                return;
            };
            let (ppos, prot, pscale) = (f3(pt.get("pos"), 0.0), f4(pt.get("rot")), f3(pt.get("scale"), 1.0));
            let d = q_rotate(q_conj(prot), sub(pos, ppos));
            let local_pos = [d[0] / pscale[0].max(1e-6), d[1] / pscale[1].max(1e-6), d[2] / pscale[2].max(1e-6)];
            let local_rot = q_mul(q_conj(prot), rot);
            let local_scale = [scale[0] / pscale[0].max(1e-6), scale[1] / pscale[1].max(1e-6), scale[2] / pscale[2].max(1e-6)];
            let local = Json::obj([("pos", jv(&local_pos)), ("rot", jv(&local_rot)), ("scale", jv(&local_scale))]);
            studio.expanded.insert(p);
            let has_local = me.get("components").and_then(|c| c.get("LocalTransform")).is_some();
            let has_parent = me.get("components").and_then(|c| c.get("Parent")).is_some();
            let call = |studio: &mut Studio, tool: &str, comp: &str, value: Json, group: &str| {
                let _ = studio.try_call(tool, Json::obj([("uid", Json::from(child as i64)), ("component", Json::str(comp)), ("value", value), ("undo_group", Json::str(group))]));
            };
            let parent_val = Json::obj([("target", Json::from(p as i64))]);
            call(studio, if has_local { "set" } else { "add_component" }, "LocalTransform", local, &group);
            call(studio, if has_parent { "set" } else { "add_component" }, "Parent", parent_val, &group);
        }
    }
    studio.rows_dirty = true;
}
