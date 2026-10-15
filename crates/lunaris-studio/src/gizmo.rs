//! The handles on the selection in the scene view: arrows to move, rings to rotate, boxes on lines to scale.
//! Geometry only: where the handles are on the screen, which one is under the pointer, and how a drag along one changes the object.
use crate::mathx::*;
use crate::theme;
use eframe::egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// Drag to move the view, nothing is selected or changed.
    Hand,
    Move,
    Rotate,
    Scale,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    pub fn dir(self) -> V3 {
        match self {
            Axis::X => [1.0, 0.0, 0.0],
            Axis::Y => [0.0, 1.0, 0.0],
            Axis::Z => [0.0, 0.0, 1.0],
        }
    }

    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }

    pub fn color(self) -> Color32 {
        match self {
            Axis::X => theme::AXIS_X,
            Axis::Y => theme::AXIS_Y,
            Axis::Z => theme::AXIS_Z,
        }
    }

    /// The two directions that span the plane a rotation about this axis turns in.
    pub fn plane(self) -> (V3, V3) {
        match self {
            Axis::X => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            Axis::Y => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            Axis::Z => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        }
    }
}

/// Where the handles are: the selection's position, how long they are in the world, and (for scaling) how the object is turned.
#[derive(Clone, Copy, Debug)]
pub struct Handles {
    pub origin: V3,
    pub len: f32,
    pub rot: Q,
}

impl Handles {
    /// The direction of an axis handle for this tool: scaling follows the object, the others stay with the world.
    pub fn axis_dir(&self, tool: Tool, a: Axis) -> V3 {
        if tool == Tool::Scale {
            q_rotate(self.rot, a.dir())
        } else {
            a.dir()
        }
    }

    fn ring(&self, a: Axis, n: usize) -> Vec<V3> {
        let (u, v) = a.plane();
        let r = self.len * 0.85;
        (0..=n)
            .map(|i| {
                let t = i as f32 / n as f32 * std::f32::consts::TAU;
                add(self.origin, add(mul(u, t.cos() * r), mul(v, t.sin() * r)))
            })
            .collect()
    }
}

const RING_POINTS: usize = 48;

/// Which handle is under `pointer` (in the picture's own pixels), nearest first.
pub fn hit(tool: Tool, h: &Handles, m: &Mat, w: f32, hh: f32, pointer: [f32; 2], two_d: bool) -> Option<Axis> {
    let mut best: Option<(f32, Axis)> = None;
    for a in Axis::ALL {
        if two_d && a == Axis::Z && tool != Tool::Rotate {
            continue;
        }
        if two_d && tool == Tool::Rotate && a != Axis::Z {
            continue;
        }
        let d = match tool {
            Tool::Hand => continue,
            Tool::Move | Tool::Scale => {
                let tip = add(h.origin, mul(h.axis_dir(tool, a), h.len));
                match segment_to_screen(m, h.origin, tip, w, hh) {
                    Some((p, q)) => dist_to_segment(pointer, p, q),
                    None => continue,
                }
            }
            Tool::Rotate => {
                let pts = h.ring(a, RING_POINTS);
                let mut d = f32::MAX;
                for k in 0..RING_POINTS {
                    if let Some((p, q)) = segment_to_screen(m, pts[k], pts[k + 1], w, hh) {
                        d = d.min(dist_to_segment(pointer, p, q));
                    }
                }
                d
            }
        };
        if d < 8.0 && best.is_none_or(|b| d < b.0) {
            best = Some((d, a));
        }
    }
    best.map(|b| b.1)
}

/// Draw the handles of `tool` over the picture. `at` maps a point of the picture to the window.
pub fn draw(painter: &Painter, at: &dyn Fn([f32; 2]) -> Pos2, tool: Tool, h: &Handles, m: &Mat, w: f32, hh: f32, hover: Option<Axis>, active: Option<Axis>, two_d: bool) {
    for a in Axis::ALL {
        if two_d && a == Axis::Z && tool != Tool::Rotate {
            continue;
        }
        if two_d && tool == Tool::Rotate && a != Axis::Z {
            continue;
        }
        let lit = hover == Some(a) || active == Some(a);
        let col = if lit { Color32::from_rgb(255, 235, 120) } else { a.color() };
        let width = if lit { 3.5 } else { 2.2 };
        match tool {
            Tool::Hand => {}
            Tool::Move | Tool::Scale => {
                let tip = add(h.origin, mul(h.axis_dir(tool, a), h.len));
                if let Some((p, q)) = segment_to_screen(m, h.origin, tip, w, hh) {
                    let (pa, pb) = (at(p), at(q));
                    painter.line_segment([pa, pb], Stroke::new(width, col));
                    let dir = (pb - pa).normalized();
                    if dir.length() > 0.0 {
                        let side = Vec2::new(-dir.y, dir.x);
                        if tool == Tool::Move {
                            painter.add(Shape::convex_polygon(vec![pb + dir * 12.0, pb - dir * 2.0 + side * 6.0, pb - dir * 2.0 - side * 6.0], col, Stroke::NONE));
                        } else {
                            let c = pb + dir * 4.0;
                            painter.rect_filled(Rect::from_center_size(c, Vec2::splat(9.0)), CornerRadius::ZERO, col);
                        }
                    }
                }
            }
            Tool::Rotate => {
                let pts = h.ring(a, RING_POINTS);
                let mut line: Vec<Pos2> = Vec::new();
                for k in 0..RING_POINTS {
                    if let Some((p, q)) = segment_to_screen(m, pts[k], pts[k + 1], w, hh) {
                        if line.is_empty() {
                            line.push(at(p));
                        }
                        line.push(at(q));
                    }
                }
                if line.len() > 1 {
                    painter.add(Shape::line(line, Stroke::new(if lit { 3.0 } else { 1.8 }, col)));
                }
            }
        }
    }
    if let Some(s) = to_screen(m, h.origin, w, hh) {
        painter.circle_filled(at(s), 3.0, Color32::WHITE);
    }
}

/// How far along the handle's line the pointer's ray reaches (move, scale), or the angle in the ring's plane (rotate), at the start of a drag.
pub fn grab_value(tool: Tool, h: &Handles, a: Axis, ray_o: V3, ray_d: V3) -> Option<f32> {
    match tool {
        Tool::Move | Tool::Scale => closest_on_line(h.origin, h.axis_dir(tool, a), ray_o, ray_d),
        Tool::Rotate => ring_angle(h, a, ray_o, ray_d),
        Tool::Hand => None,
    }
}

/// The angle of the point where the ray meets the plane of the ring, measured in the plane (radians).
pub fn ring_angle(h: &Handles, a: Axis, ray_o: V3, ray_d: V3) -> Option<f32> {
    let hit = ray_plane(ray_o, ray_d, h.origin, a.dir())?;
    let (u, v) = a.plane();
    let d = sub(hit, h.origin);
    Some(dot(d, v).atan2(dot(d, u)))
}

/// The change of a move: the new position when the pointer's ray is `(ray_o, ray_d)` and the drag began at `grab`.
pub fn moved(h: &Handles, a: Axis, grab: f32, ray_o: V3, ray_d: V3) -> Option<V3> {
    let t = closest_on_line(h.origin, a.dir(), ray_o, ray_d)?;
    Some(add(h.origin, mul(a.dir(), t - grab)))
}

/// The factor a scale handle drag gives (1 = unchanged).
pub fn scaled(h: &Handles, a: Axis, grab: f32, ray_o: V3, ray_d: V3) -> Option<f32> {
    let t = closest_on_line(h.origin, h.axis_dir(Tool::Scale, a), ray_o, ray_d)?;
    Some((1.0 + (t - grab) / h.len.max(1e-4)).max(0.01))
}

/// The rotation (about the ring's world axis) a ring drag gives: from the grabbed angle to the current one.
pub fn turned(h: &Handles, a: Axis, grab: f32, ray_o: V3, ray_d: V3, snap_degrees: Option<f32>) -> Option<Q> {
    let now = ring_angle(h, a, ray_o, ray_d)?;
    let mut delta = now - grab;
    // the shortest way round
    while delta > std::f32::consts::PI {
        delta -= std::f32::consts::TAU;
    }
    while delta < -std::f32::consts::PI {
        delta += std::f32::consts::TAU;
    }
    if let Some(s) = snap_degrees {
        let step = s.to_radians();
        delta = (delta / step).round() * step;
    }
    Some(q_axis_angle(a.dir(), delta))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handles() -> Handles {
        Handles { origin: [0.0; 3], len: 2.0, rot: [0.0, 0.0, 0.0, 1.0] }
    }

    #[test]
    fn a_move_follows_the_ray_along_the_axis_and_a_scale_is_a_ratio() {
        let h = handles();
        // grabbed where the ray from above met the x axis at x = 0.5; the pointer then moves to x = 1.5
        let (o1, d1) = ([0.5, 5.0, 0.0], [0.0, -1.0, 0.0]);
        let g = grab_value(Tool::Move, &h, Axis::X, o1, d1).unwrap();
        let p = moved(&h, Axis::X, g, [1.5, 5.0, 0.0], d1).unwrap();
        assert!((p[0] - 1.0).abs() < 1e-5 && p[1] == 0.0 && p[2] == 0.0, "{p:?}");
        let gs = grab_value(Tool::Scale, &h, Axis::X, o1, d1).unwrap();
        let f = scaled(&h, Axis::X, gs, [2.5, 5.0, 0.0], d1).unwrap();
        assert!((f - 2.0).abs() < 1e-5, "one handle length further doubles it: {f}");
        assert!(scaled(&h, Axis::X, gs, [-9.0, 5.0, 0.0], d1).unwrap() >= 0.01, "never flipped or zero");
    }

    #[test]
    fn a_ring_drag_turns_by_the_angle_the_pointer_swept() {
        let h = handles();
        // looking down the y axis at the ring around y: the pointer goes from +x to +z (a quarter turn)
        let d = [0.0, -1.0, 0.0];
        let g = grab_value(Tool::Rotate, &h, Axis::Y, [1.0, 5.0, 0.0], d).unwrap();
        let q = turned(&h, Axis::Y, g, [0.0, 5.0, 1.0], d, None).unwrap();
        let v = q_rotate(q, [1.0, 0.0, 0.0]);
        // the sweep from the plane's first direction (z) to its second (x) is a quarter turn about +y
        assert!((v[0].abs() - 0.0).abs() < 1e-3 || (v[2].abs() - 0.0).abs() < 1e-3, "{v:?}");
        let snapped = turned(&h, Axis::Y, g, [1.0, 5.0, 0.3], d, Some(15.0)).unwrap();
        let ang = 2.0 * snapped[3].clamp(-1.0, 1.0).acos().to_degrees();
        assert!(((ang / 15.0).round() * 15.0 - ang).abs() < 1e-3, "snapped to 15 degrees: {ang}");
    }

    #[test]
    fn handles_are_found_under_the_pointer_and_nothing_else_is() {
        // an identity view maps x, y in -1..1 to the picture; a 200 x 200 picture: the x handle runs from the middle to the right
        let m = Mat::IDENTITY;
        let h = Handles { origin: [0.0; 3], len: 0.5, rot: [0.0, 0.0, 0.0, 1.0] };
        assert_eq!(hit(Tool::Move, &h, &m, 200.0, 200.0, [125.0, 100.0], false), Some(Axis::X));
        assert_eq!(hit(Tool::Move, &h, &m, 200.0, 200.0, [100.0, 75.0], false), Some(Axis::Y));
        assert_eq!(hit(Tool::Move, &h, &m, 200.0, 200.0, [30.0, 30.0], false), None);
        assert_eq!(hit(Tool::Hand, &h, &m, 200.0, 200.0, [125.0, 100.0], false), None);
        // the z handle points at the camera in this view: it is a dot, and 2D views have no z handle
        assert_eq!(hit(Tool::Move, &h, &m, 200.0, 200.0, [125.0, 100.0], true), Some(Axis::X));
    }
}
