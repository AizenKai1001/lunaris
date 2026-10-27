//! The Streaming overlay of the scene view: every `StreamZone` as a ring on the ground (its `radius`, and the inner ring of a ring zone), bright green while the engine has the zone loaded and grey-blue while it has not, so that the layout of a streamed world
//! and what is loaded of it can be seen (Unreal's World Partition debug view). The zones are read twice a second with a `query`, the nearest to the camera are drawn, and the Stats overlay says how many of them are loaded.
use crate::mathx::*;
use crate::state::Studio;
use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke};
use lunaris_json::Json;
use std::time::{Duration, Instant};

/// How many zones one read asks for (a city has more; the rest are not drawn and the count says so).
pub const MAX_ZONES: usize = 4000;
/// How many of them are drawn: the ones nearest to the camera.
const MAX_DRAWN: usize = 250;
const RING_SEGMENTS: usize = 28;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneRing {
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
        let q = Json::obj([("with", names(&["StreamZone"])), ("select", names(&["Transform", "StreamZone"])), ("limit", Json::from(MAX_ZONES as i64))]);
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
        assert_eq!(zones[0], ZoneRing { pos: [10.0, 2.0, -5.0], radius: 50.0, min_radius: 0.0, loaded: true });
        assert_eq!((zones[1].radius, zones[1].min_radius, zones[1].loaded), (200.0, 80.0, false));
        let cache = ZoneCache { zones, total, ..ZoneCache::default() };
        assert_eq!(cache.loaded_count(), 1);
        assert_eq!(cache.summary(), "zones 1 of 2 loaded (1 not read)");
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
