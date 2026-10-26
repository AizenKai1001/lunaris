//! Software occlusion culling: the solid volumes of the scene (`Mesh::occluders`: the bodies of the buildings of a city) drawn into a small depth picture on the CPU, and the objects whose boxes lie wholly behind them left out of the
//! picture. Worked out for each picture afresh from the objects that are in it (no waiting for the card, nothing a frame late: what a turn of the head shows is tested against what is in front of it now).
//!
//! The depth picture is 256 columns wide. A prism is drawn as the faces that look at the camera (clipped at the near plane), each pixel keeping the distance, along the view direction, of the nearest face and which prism it is. An
//! object is hidden when every pixel of its box's screen rectangle, a pixel wider on every side (the picture is coarse, a pixel's centre being inside a face does not mean the whole pixel is), has a face nearer than the box's
//! nearest corner by a margin. An object inside a prism (the rooms of a building) is not hidden by it; prisms nearer to the camera than `MIN_DISTANCE` are not used (up close one looks through the windows and doors).
use lunaris_render::math::{M4, V3};

/// Prisms nearer than this (metres from the camera to the closest point of the prism's box) do not hide anything.
const MIN_DISTANCE: f32 = 18.0;
/// Where the view is cut off in front: a face is clipped there, a box that reaches it is never hidden.
const NEAR_CLIP: f32 = 0.3;

#[derive(Clone, Copy)]
pub struct Prism {
    /// The four bottom corners, then the four top corners above them, in the same order round the footprint.
    pub c: [V3; 8],
}

pub struct OcclusionBuffer {
    w: usize,
    h: usize,
    /// Distance along the view direction of the nearest face at each pixel's centre, infinity where there is none.
    depth: Vec<f32>,
    /// Which prism (an index into `prisms`) that face belongs to.
    id: Vec<u32>,
    prisms: Vec<Prism>,
    view: [f32; 16],
    proj: [f32; 16],
    eye: V3,
    pub drawn: u32,
}

#[derive(Clone, Copy)]
struct P {
    x: f32,
    y: f32,
    z: f32,
}

impl OcclusionBuffer {
    pub fn new() -> OcclusionBuffer {
        OcclusionBuffer { w: 0, h: 0, depth: Vec::new(), id: Vec::new(), prisms: Vec::new(), view: [0.0; 16], proj: [0.0; 16], eye: [0.0; 3], drawn: 0 }
    }

    /// Start a picture: `width` columns, `aspect` (width over height) says how many rows. A perspective view only: false for an orthographic one (nothing is culled).
    pub fn begin(&mut self, view: &M4, proj: &M4, eye: V3, aspect: f32) -> bool {
        if proj.0[15] != 0.0 || !aspect.is_finite() || aspect <= 0.0 {
            return false;
        }
        self.w = 256;
        self.h = ((256.0 / aspect).round() as usize).clamp(48, 512);
        self.depth.clear();
        self.depth.resize(self.w * self.h, f32::INFINITY);
        self.id.clear();
        self.id.resize(self.w * self.h, u32::MAX);
        self.prisms.clear();
        self.view = view.0;
        self.proj = proj.0;
        self.eye = eye;
        self.drawn = 0;
        true
    }

    fn to_view(&self, p: V3) -> P {
        let m = &self.view;
        P { x: m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12], y: m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13], z: m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14] }
    }

    /// A point of the view space as a pixel position and its distance along the view direction (which is the clip space's w).
    fn screen(&self, v: P) -> (f32, f32, f32) {
        let m = &self.proj;
        let w = (-v.z).max(1.0e-6);
        let cx = m[0] * v.x + m[4] * v.y + m[8] * v.z + m[12];
        let cy = m[1] * v.x + m[5] * v.y + m[9] * v.z + m[13];
        ((cx / w * 0.5 + 0.5) * self.w as f32, (0.5 - cy / w * 0.5) * self.h as f32, w)
    }

    /// Draw a prism's faces that look at the camera. Prisms near the camera, or that are not wholly in front of the picture's sides, are skipped here or clipped, as described above.
    pub fn add(&mut self, prism: &Prism) {
        // nearer than MIN_DISTANCE (the box of the prism): not used
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        let mut mid = [0.0f32; 3];
        for c in &prism.c {
            for k in 0..3 {
                lo[k] = lo[k].min(c[k]);
                hi[k] = hi[k].max(c[k]);
                mid[k] += c[k] * 0.125;
            }
        }
        let mut d2 = 0.0;
        for k in 0..3 {
            let e = (lo[k] - self.eye[k]).max(0.0).max(self.eye[k] - hi[k]);
            d2 += e * e;
        }
        if d2 < MIN_DISTANCE * MIN_DISTANCE {
            return;
        }
        // a prism that is a few pixels across hides nothing worth the work
        let view: [P; 8] = std::array::from_fn(|k| self.to_view(prism.c[k]));
        if view.iter().all(|v| -v.z > NEAR_CLIP) {
            let (mut x0, mut x1, mut y0, mut y1) = (f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY);
            for v in &view {
                let (sx, sy, _) = self.screen(*v);
                x0 = x0.min(sx);
                x1 = x1.max(sx);
                y0 = y0.min(sy);
                y1 = y1.max(sy);
            }
            if (x1 - x0) * (y1 - y0) < 24.0 || x1 < 0.0 || y1 < 0.0 || x0 > self.w as f32 || y0 > self.h as f32 {
                return;
            }
        }
        let id = self.prisms.len() as u32;
        self.prisms.push(*prism);
        // the faces: the top, and the four sides; each as four corners in order round it
        let faces: [[usize; 4]; 5] = [[4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];
        for f in faces {
            let p: [V3; 4] = [prism.c[f[0]], prism.c[f[1]], prism.c[f[2]], prism.c[f[3]]];
            // the normal, turned to point out of the prism
            let e1 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
            let e2 = [p[3][0] - p[0][0], p[3][1] - p[0][1], p[3][2] - p[0][2]];
            let mut n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            let out = [p[0][0] - mid[0], p[0][1] - mid[1], p[0][2] - mid[2]];
            if n[0] * out[0] + n[1] * out[1] + n[2] * out[2] < 0.0 {
                n = [-n[0], -n[1], -n[2]];
            }
            let to_eye = [self.eye[0] - p[0][0], self.eye[1] - p[0][1], self.eye[2] - p[0][2]];
            if n[0] * to_eye[0] + n[1] * to_eye[1] + n[2] * to_eye[2] <= 0.0 {
                continue;
            }
            // into the view space, clipped at the near plane (Sutherland-Hodgman against z = -NEAR_CLIP)
            let v: [P; 4] = [view[f[0]], view[f[1]], view[f[2]], view[f[3]]];
            let mut poly = [P { x: 0.0, y: 0.0, z: 0.0 }; 6];
            let mut count = 0;
            for k in 0..4 {
                let (a, b) = (v[k], v[(k + 1) % 4]);
                let (da, db) = (-a.z - NEAR_CLIP, -b.z - NEAR_CLIP);
                if da >= 0.0 {
                    poly[count] = a;
                    count += 1;
                }
                if (da >= 0.0) != (db >= 0.0) {
                    let t = da / (da - db);
                    poly[count] = P { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, z: a.z + (b.z - a.z) * t };
                    count += 1;
                }
            }
            if count < 3 {
                continue;
            }
            let mut s = [(0.0f32, 0.0f32, 0.0f32); 6];
            for k in 0..count {
                s[k] = self.screen(poly[k]);
            }
            for k in 1..count - 1 {
                self.triangle(s[0], s[k], s[k + 1], id);
            }
        }
        self.drawn += 1;
    }

    /// One triangle of pixel positions and distances, into the picture where it is nearer than what is there. The distance is interpolated as its inverse (that is linear on the screen).
    fn triangle(&mut self, a: (f32, f32, f32), b: (f32, f32, f32), c: (f32, f32, f32), id: u32) {
        let (w, h) = (self.w as f32, self.h as f32);
        let min_x = a.0.min(b.0).min(c.0).floor().max(0.0);
        let max_x = a.0.max(b.0).max(c.0).ceil().min(w - 1.0);
        let min_y = a.1.min(b.1).min(c.1).floor().max(0.0);
        let max_y = a.1.max(b.1).max(c.1).ceil().min(h - 1.0);
        if min_x > max_x || min_y > max_y {
            return;
        }
        let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        if area.abs() < 1.0e-6 {
            return;
        }
        let inv = 1.0 / area;
        let (ia, ib, ic) = (1.0 / a.2, 1.0 / b.2, 1.0 / c.2);
        for y in min_y as usize..=max_y as usize {
            let py = y as f32 + 0.5;
            for x in min_x as usize..=max_x as usize {
                let px = x as f32 + 0.5;
                // barycentric coordinates by edge functions (the sign of the area folds out)
                let w0 = ((b.0 - px) * (c.1 - py) - (b.1 - py) * (c.0 - px)) * inv;
                let w1 = ((c.0 - px) * (a.1 - py) - (c.1 - py) * (a.0 - px)) * inv;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = 1.0 / (w0 * ia + w1 * ib + w2 * ic);
                let at = y * self.w + x;
                if z < self.depth[at] {
                    self.depth[at] = z;
                    self.id[at] = id;
                }
            }
        }
    }

    /// Whether an object whose box has these eight corners (in the world) is wholly hidden by what was drawn. False when in doubt.
    pub fn hidden(&self, corners: &[V3; 8]) -> bool {
        if self.prisms.is_empty() {
            return false;
        }
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY);
        let mut near = f32::INFINITY;
        for c in corners {
            let v = self.to_view(*c);
            let dist = -v.z;
            if dist < NEAR_CLIP + 0.05 {
                return false;
            }
            let (sx, sy, _) = self.screen(v);
            min_x = min_x.min(sx);
            max_x = max_x.max(sx);
            min_y = min_y.min(sy);
            max_y = max_y.max(sy);
            near = near.min(dist);
        }
        // a pixel of margin all round (the pixels are coarse), and only what is on the screen
        let x0 = (min_x.floor() - 1.0).max(0.0);
        let x1 = (max_x.ceil() + 1.0).min(self.w as f32 - 1.0);
        let y0 = (min_y.floor() - 1.0).max(0.0);
        let y1 = (max_y.ceil() + 1.0).min(self.h as f32 - 1.0);
        if x0 > x1 || y0 > y1 {
            return false;
        }
        let limit = near - (0.5 + 0.01 * near);
        let covered = |skip: u32| -> bool {
            for y in y0 as usize..=y1 as usize {
                let row = y * self.w;
                for x in x0 as usize..=x1 as usize {
                    let d = self.depth[row + x];
                    if !(d < limit) || self.id[row + x] == skip {
                        return false;
                    }
                }
            }
            true
        };
        if !covered(u32::MAX - 1) {
            return false;
        }
        // hidden by something: but a box that stands inside one of the prisms (a room of a building) is not hidden by that one
        let mut mid = [0.0f32; 3];
        for c in corners {
            for k in 0..3 {
                mid[k] += c[k] * 0.125;
            }
        }
        match self.prisms.iter().position(|p| contains(p, mid)) {
            Some(own) => covered(own as u32),
            None => true,
        }
    }
}

/// Whether the point is inside the prism (its footprint is a convex four-cornered shape, its height from the bottom corners to the top ones), with half a metre to spare.
fn contains(p: &Prism, q: V3) -> bool {
    let (y0, y1) = (p.c[0][1], p.c[4][1]);
    if q[1] < y0.min(y1) - 0.5 || q[1] > y0.max(y1) + 0.5 {
        return false;
    }
    let mut sign = 0.0f32;
    for k in 0..4 {
        let (a, b) = (p.c[k], p.c[(k + 1) % 4]);
        let cross = (b[0] - a[0]) * (q[2] - a[2]) - (b[2] - a[2]) * (q[0] - a[0]);
        let len = ((b[0] - a[0]).powi(2) + (b[2] - a[2]).powi(2)).sqrt().max(1.0e-6);
        // half a metre outside an edge still counts as inside
        let d = cross / len;
        if sign == 0.0 {
            sign = if d >= 0.0 { 1.0 } else { -1.0 };
        }
        if d * sign < -0.5 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_render::math::M4;

    /// A camera at the origin looking down -z (the view is the identity), 90 degrees across.
    fn camera() -> (M4, M4) {
        let mut proj = M4::identity();
        proj.0[0] = 1.0;
        proj.0[5] = 1.0;
        proj.0[10] = -1.0;
        proj.0[11] = -1.0;
        proj.0[14] = -0.2;
        proj.0[15] = 0.0;
        (M4::identity(), proj)
    }

    fn prism(x0: f32, x1: f32, z0: f32, z1: f32, y0: f32, y1: f32) -> Prism {
        Prism { c: [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1], [x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]] }
    }

    fn cube(cx: f32, cy: f32, cz: f32, h: f32) -> [V3; 8] {
        let mut c = [[0.0; 3]; 8];
        for (i, v) in c.iter_mut().enumerate() {
            *v = [cx + if i & 1 == 0 { -h } else { h }, cy + if i & 2 == 0 { -h } else { h }, cz + if i & 4 == 0 { -h } else { h }];
        }
        c
    }

    #[test]
    fn a_wall_hides_a_box_behind_it_and_not_one_beside_it_or_in_front() {
        let (view, proj) = camera();
        let mut ob = OcclusionBuffer::new();
        assert!(ob.begin(&view, &proj, [0.0; 3], 16.0 / 9.0));
        // a building 40 m away, 30 m wide and 40 m tall
        ob.add(&prism(-15.0, 15.0, -60.0, -40.0, -2.0, 38.0));
        assert!(ob.drawn == 1);
        assert!(ob.hidden(&cube(0.0, 10.0, -120.0, 4.0)), "behind the middle of it");
        assert!(!ob.hidden(&cube(60.0, 10.0, -120.0, 4.0)), "far to the side, in the open");
        assert!(!ob.hidden(&cube(0.0, 10.0, -20.0, 3.0)), "in front of it");
        assert!(!ob.hidden(&cube(0.0, 200.0, -120.0, 4.0)), "above the line of its roof");
        // half behind the edge: not wholly hidden
        assert!(!ob.hidden(&cube(45.0, 10.0, -120.0, 8.0)));
    }

    #[test]
    fn a_room_inside_the_building_is_not_hidden_by_its_walls_and_a_near_building_hides_nothing() {
        let (view, proj) = camera();
        let mut ob = OcclusionBuffer::new();
        assert!(ob.begin(&view, &proj, [0.0; 3], 16.0 / 9.0));
        ob.add(&prism(-15.0, 15.0, -60.0, -40.0, -2.0, 38.0));
        // another one in front of it, so that the room has something else in front of it too
        assert!(!ob.hidden(&cube(0.0, 10.0, -50.0, 3.0)), "inside the building: its own walls do not hide it");
        let mut near = OcclusionBuffer::new();
        assert!(near.begin(&view, &proj, [0.0; 3], 16.0 / 9.0));
        near.add(&prism(-15.0, 15.0, -14.0, -4.0, -2.0, 38.0));
        assert_eq!(near.drawn, 0, "closer than the minimum distance");
        assert!(!near.hidden(&cube(0.0, 10.0, -120.0, 4.0)));
    }

    #[test]
    fn two_buildings_side_by_side_hide_what_is_behind_the_gap_between_them_together() {
        let (view, proj) = camera();
        let mut ob = OcclusionBuffer::new();
        assert!(ob.begin(&view, &proj, [0.0; 3], 16.0 / 9.0));
        ob.add(&prism(-30.0, 0.5, -60.0, -40.0, -2.0, 60.0));
        ob.add(&prism(-0.5, 30.0, -60.0, -40.0, -2.0, 60.0));
        assert!(ob.hidden(&cube(0.0, 10.0, -150.0, 3.0)), "the two together cover the gap");
    }
}
