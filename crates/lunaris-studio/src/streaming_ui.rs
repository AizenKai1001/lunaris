//! The Streaming overlay of the scene view: every `StreamZone` as a ring on the ground (its `radius`, and the inner ring of a ring zone), bright green while the engine has the zone loaded and grey-blue while it has not, so that the layout of a streamed world
//! and what is loaded of it can be seen (Unreal's World Partition debug view). The zones are read twice a second with a `query`, the nearest to the camera are drawn, and the Stats overlay says how many of them are loaded.
use crate::mathx::*;
use crate::state::Studio;
use crate::theme;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use lunaris_json::Json;
use std::time::{Duration, Instant};

/// How many zones one read asks for (a city has more; the rest are not drawn and the count says so).
pub const MAX_ZONES: usize = 4000;
/// How many of them are drawn: the ones nearest to the camera.
const MAX_DRAWN: usize = 250;
const RING_SEGMENTS: usize = 28;

#[derive(Clone, Debug, PartialEq)]
pub struct ZoneRing {
    pub uid: u64,
    /// The entity's `Name` (empty when it has none) and the prefab the zone loads.
    pub name: String,
    pub prefab: String,
    pub pos: V3,
    pub radius: f32,
    pub min_radius: f32,
    pub loaded: bool,
}

/// The zones in the answer of a `query` that selected `Transform` and `StreamZone`, and how many zones there are in all.
pub fn zones_of(answer: &Json) -> (Vec<ZoneRing>, usize) {
    let total = answer.get("total").and_then(|t| t.as_u64()).unwrap_or(0) as usize;
    let mut out = Vec::new();
    for e in answer.get("entities").and_then(|e| e.as_array()).unwrap_or(&[]) {
        let comps = e.get("components");
        let Some(z) = comps.and_then(|c| c.get("StreamZone")) else { continue };
        let Some(p) = comps.and_then(|c| c.get("Transform")).and_then(|t| t.get("pos")).and_then(|p| p.as_array()).filter(|a| a.len() >= 3) else { continue };
        let f = |key: &str, default: f32| z.get(key).and_then(|v| v.as_f64()).map(|v| v as f32).unwrap_or(default);
        out.push(ZoneRing {
            uid: e.get("uid").and_then(|u| u.as_u64()).unwrap_or(0),
            name: comps.and_then(|c| c.get("Name")).and_then(|n| n.get("value")).and_then(|v| v.as_str()).unwrap_or("").to_string(),
            prefab: z.get("prefab").and_then(|p| p.as_str()).unwrap_or("").to_string(),
            pos: [p[0].as_f64().unwrap_or(0.0) as f32, p[1].as_f64().unwrap_or(0.0) as f32, p[2].as_f64().unwrap_or(0.0) as f32],
            radius: f("radius", 100.0),
            min_radius: f("min_radius", 0.0),
            loaded: z.get("loaded").and_then(|v| v.as_bool()).unwrap_or(false),
        });
    }
    (out, total)
}

/// The zones as last read.
#[derive(Default)]
pub struct ZoneCache {
    loaded_at: Option<Instant>,
    pub zones: Vec<ZoneRing>,
    pub total: usize,
}

impl ZoneCache {
    /// Read the zones again when the last read is older than half a second.
    pub fn refresh(&mut self, studio: &mut Studio) {
        if self.loaded_at.is_some_and(|t| t.elapsed() < Duration::from_millis(500)) {
            return;
        }
        self.loaded_at = Some(Instant::now());
        let names = |n: &[&str]| Json::arr(n.iter().map(|s| Json::str(*s)));
        let q = Json::obj([("with", names(&["StreamZone"])), ("select", names(&["Name", "Transform", "StreamZone"])), ("limit", Json::from(MAX_ZONES as i64))]);
        if let Ok(a) = studio.engine.call("query", &q) {
            (self.zones, self.total) = zones_of(&a);
        }
    }

    /// Read again at the next frame (the overlay was just switched on).
    pub fn invalidate(&mut self) {
        self.loaded_at = None;
    }

    pub fn loaded_count(&self) -> usize {
        self.zones.iter().filter(|z| z.loaded).count()
    }

    /// "12 of 340 zones loaded" (and how many were not read, when there are more than one read asks for).
    pub fn summary(&self) -> String {
        let unread = self.total.saturating_sub(self.zones.len());
        if unread > 0 {
            format!("zones {} of {} loaded ({unread} not read)", self.loaded_count(), self.zones.len())
        } else {
            format!("zones {} of {} loaded", self.loaded_count(), self.zones.len())
        }
    }
}

/// The points of a circle of `radius` metres on the ground round `centre`, the first point repeated at the end.
pub fn circle(centre: V3, radius: f32, n: usize) -> Vec<V3> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            [centre[0] + t.cos() * radius, centre[1], centre[2] + t.sin() * radius]
        })
        .collect()
}

/// Draw the rings of the zones nearest to `eye` over the view in `rect`.
pub fn draw(painter: &Painter, rect: Rect, view_proj: &Mat, eye: V3, zones: &[ZoneRing]) {
    let (w, h) = (rect.width(), rect.height());
    let at = |p: [f32; 2]| Pos2::new(rect.min.x + p[0], rect.min.y + p[1]);
    let mut order: Vec<(f32, &ZoneRing)> = zones.iter().map(|z| (len(sub(z.pos, eye)), z)).collect();
    if order.len() > MAX_DRAWN {
        order.select_nth_unstable_by(MAX_DRAWN, |a, b| a.0.total_cmp(&b.0));
        order.truncate(MAX_DRAWN);
    }
    // the loaded ones last, so that they are on top
    order.sort_by_key(|(_, z)| z.loaded);
    for (_, z) in order {
        let (color, width) = if z.loaded { (Color32::from_rgba_unmultiplied(90, 220, 120, 210), 2.0) } else { (Color32::from_rgba_unmultiplied(175, 195, 240, 185), 1.2) };
        for r in std::iter::once(z.radius).chain((z.min_radius > 0.0).then_some(z.min_radius)) {
            let pts = circle(z.pos, r.max(0.01), RING_SEGMENTS);
            for pair in pts.windows(2) {
                if let Some((p, q)) = segment_to_screen(view_proj, pair[0], pair[1], w, h) {
                    painter.line_segment([at(p), at(q)], Stroke::new(width, color));
                }
            }
        }
        if let Some(c) = to_screen(view_proj, z.pos, w, h) {
            painter.circle_filled(at(c), if z.loaded { 3.0 } else { 2.0 }, color);
        }
    }
}

/// The zones to list: those that pass the filter (a part of the name, the prefab or the uid) and, when asked, only the loaded ones; the loaded first, then the nearest to `from` on the ground.
pub fn listed<'a>(zones: &'a [ZoneRing], filter: &str, loaded_only: bool, from: V3) -> Vec<&'a ZoneRing> {
    let filter = filter.trim().to_lowercase();
    let ground = |z: &ZoneRing| ((z.pos[0] - from[0]).powi(2) + (z.pos[2] - from[2]).powi(2)).sqrt();
    let mut out: Vec<&ZoneRing> = zones.iter().filter(|z| (!loaded_only || z.loaded) && (filter.is_empty() || z.name.to_lowercase().contains(&filter) || z.prefab.to_lowercase().contains(&filter) || z.uid.to_string() == filter)).collect();
    out.sort_by(|a, b| b.loaded.cmp(&a.loaded).then(ground(a).total_cmp(&ground(b))).then(a.uid.cmp(&b.uid)));
    out
}

/// What a click in the list asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub uid: u64,
    pub pos: V3,
    pub radius: f32,
    /// A double click: put the scene view on it.
    pub focus: bool,
}

/// The Streaming tab: every `StreamZone` of the scene as a row (loaded or not, its name, the prefab it loads, its radius and how far it is from the editor camera), the loaded ones first.
#[derive(Default)]
pub struct StreamingUi {
    filter: String,
    loaded_only: bool,
}

impl StreamingUi {
    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio, cache: &mut ZoneCache, from: V3) -> Option<Pick> {
        cache.refresh(studio);
        ui.horizontal(|ui| {
            crate::chrome::search_box(ui, &mut self.filter, "Filter zones", 200.0);
            ui.toggle_value(&mut self.loaded_only, "Loaded only");
            ui.label(RichText::new(cache.summary()).small().color(theme::TEXT_DIM));
        });
        let rows = listed(&cache.zones, &self.filter, self.loaded_only, from);
        if rows.is_empty() {
            ui.label(RichText::new(if cache.zones.is_empty() { "The scene has no StreamZone." } else { "No zone matches." }).color(theme::TEXT_DIM));
            return None;
        }
        let mut pick: Option<Pick> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("streaming-rows").show_rows(ui, 18.0, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for k in range {
                let z = rows[k];
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::click());
                let selected = studio.is_selected(z.uid);
                ui.painter().rect_filled(rect, CornerRadius::ZERO, if selected { theme::SELECT } else if k % 2 == 0 { theme::ROW_ALT } else { theme::PANEL });
                let mid = rect.center().y;
                ui.painter().circle_filled(egui::pos2(rect.min.x + 12.0, mid), 4.0, if z.loaded { theme::OK } else { theme::TEXT_DIM.gamma_multiply(0.6) });
                let label = if z.name.is_empty() { format!("#{}", z.uid) } else { format!("{}  #{}", z.name, z.uid) };
                let dist = ((z.pos[0] - from[0]).powi(2) + (z.pos[2] - from[2]).powi(2)).sqrt();
                ui.painter().with_clip_rect(egui::Rect::from_min_size(rect.min, Vec2::new((rect.width() * 0.34).max(60.0), 18.0))).text(egui::pos2(rect.min.x + 24.0, mid), Align2::LEFT_CENTER, label, FontId::proportional(12.0), theme::TEXT);
                let prefab_x = rect.min.x + (rect.width() * 0.34).max(60.0) + 8.0;
                ui.painter().with_clip_rect(egui::Rect::from_min_max(egui::pos2(prefab_x, rect.min.y), egui::pos2(rect.max.x - 230.0, rect.max.y))).text(egui::pos2(prefab_x, mid), Align2::LEFT_CENTER, &z.prefab, FontId::proportional(12.0), theme::TEXT_DIM);
                let ring = if z.min_radius > 0.0 { format!("{:.0} to {:.0} m", z.min_radius, z.radius) } else { format!("{:.0} m", z.radius) };
                ui.painter().text(egui::pos2(rect.max.x - 224.0, mid), Align2::LEFT_CENTER, ring, FontId::monospace(11.0), theme::TEXT);
                ui.painter().text(egui::pos2(rect.max.x - 110.0, mid), Align2::LEFT_CENTER, format!("{dist:.0} m away"), FontId::monospace(11.0), theme::TEXT_DIM);
                if resp.clicked() || resp.double_clicked() {
                    studio.select(Some(z.uid));
                    pick = Some(Pick { uid: z.uid, pos: z.pos, radius: z.radius, focus: resp.double_clicked() });
                }
            }
        });
        pick
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_json::parse;

    #[test]
    fn the_rings_come_from_the_query_answer_and_a_zone_without_a_position_is_left_out() {
        let a = parse(
            r#"{"total":3,"entities":[
                {"uid":1,"components":{"Transform":{"pos":[10,2,-5]},"StreamZone":{"radius":50,"loaded":true}}},
                {"uid":2,"components":{"Transform":{"pos":[0,0,0]},"StreamZone":{"radius":200,"min_radius":80}}},
                {"uid":3,"components":{"StreamZone":{"radius":9}}}]}"#,
        )
        .unwrap();
        let (zones, total) = zones_of(&a);
        assert_eq!(total, 3);
        assert_eq!(zones.len(), 2);
        assert_eq!(zones[0], ZoneRing { uid: 1, name: String::new(), prefab: String::new(), pos: [10.0, 2.0, -5.0], radius: 50.0, min_radius: 0.0, loaded: true });
        assert_eq!((zones[1].radius, zones[1].min_radius, zones[1].loaded), (200.0, 80.0, false));
        let cache = ZoneCache { zones, total, ..ZoneCache::default() };
        assert_eq!(cache.loaded_count(), 1);
        assert_eq!(cache.summary(), "zones 1 of 2 loaded (1 not read)");
    }

    #[test]
    fn the_list_has_the_loaded_zones_first_then_the_nearest_and_the_filter_looks_at_name_prefab_and_uid() {
        let z = |uid: u64, name: &str, prefab: &str, x: f32, loaded: bool| ZoneRing { uid, name: name.to_string(), prefab: prefab.to_string(), pos: [x, 0.0, 0.0], radius: 50.0, min_radius: 0.0, loaded };
        let zones = vec![z(1, "North", "gen:b0_1", 300.0, false), z(2, "Market", "gen:b0_2", 100.0, false), z(3, "Docks", "gen:b0_3", 900.0, true), z(4, "", "tower", 50.0, true)];
        let uids = |v: Vec<&ZoneRing>| v.iter().map(|z| z.uid).collect::<Vec<_>>();
        assert_eq!(uids(listed(&zones, "", false, [0.0; 3])), [4, 3, 2, 1], "loaded first, each group nearest first");
        assert_eq!(uids(listed(&zones, "", true, [0.0; 3])), [4, 3]);
        assert_eq!(uids(listed(&zones, "b0_", false, [0.0; 3])), [3, 2, 1], "the prefab");
        assert_eq!(uids(listed(&zones, "MARK", false, [0.0; 3])), [2], "the name, any case");
        assert_eq!(uids(listed(&zones, "4", false, [0.0; 3])), [4], "the uid");
        // from the other end of the street the order of the nearest turns round
        assert_eq!(uids(listed(&zones, "", false, [1000.0, 0.0, 0.0])), [3, 4, 1, 2]);
    }

    #[test]
    fn a_circle_closes_on_itself_at_the_radius() {
        let c = circle([1.0, 3.0, 2.0], 5.0, 16);
        assert_eq!(c.len(), 17);
        assert!(len(sub(c[0], c[16])) < 1e-4);
        for p in &c {
            assert_eq!(p[1], 3.0, "on the ground plane of the zone");
            assert!((((p[0] - 1.0).powi(2) + (p[2] - 2.0).powi(2)).sqrt() - 5.0).abs() < 1e-4);
        }
    }
}
