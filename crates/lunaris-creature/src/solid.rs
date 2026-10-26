//! Solid things that are not the heightfield: walls, ramps, ceilings, platforms, as up to `MAX_BOXES` oriented boxes. A creature touches them with the corners of its bodies, as it touches the ground, and its
//! adhesive feet stick to them. `nearest` gives the signed distance (negative inside) and the unit normal (out of the solid) of the closest box, for the contact solver and for the senses.
use crate::vecmath::*;

pub const MAX_BOXES: usize = 4;

/// An oriented box: centre, rotation (local to world, a matrix) and half sizes.
#[derive(Clone, Copy, Debug, Default)]
pub struct SolidBox {
    pub c: V3,
    pub r: M3,
    pub half: V3,
}

/// The solids of an episode or of a world.
#[derive(Clone, Copy, Debug, Default)]
pub struct Solids {
    pub n: usize,
    pub b: [SolidBox; MAX_BOXES],
}

fn mulv_t(r: &M3, v: V3) -> V3 {
    // the transpose of a rotation matrix (row-major, `m[3 * row + col]`) applied to v
    v3(r.m[0] * v.x + r.m[3] * v.y + r.m[6] * v.z, r.m[1] * v.x + r.m[4] * v.y + r.m[7] * v.z, r.m[2] * v.x + r.m[5] * v.y + r.m[8] * v.z)
}

impl SolidBox {
    /// Signed distance from `p` to the box and the unit normal there (pointing out of the box).
    pub fn sdf(&self, p: V3) -> (f32, V3) {
        let q = mulv_t(&self.r, p - self.c);
        let d = v3(q.x.abs() - self.half.x, q.y.abs() - self.half.y, q.z.abs() - self.half.z);
        let ox = if d.x > 0.0 { d.x } else { 0.0 };
        let oy = if d.y > 0.0 { d.y } else { 0.0 };
        let oz = if d.z > 0.0 { d.z } else { 0.0 };
        let outside = (ox * ox + oy * oy + oz * oz).sqrt();
        let sx = if q.x < 0.0 { -1.0 } else { 1.0 };
        let sy = if q.y < 0.0 { -1.0 } else { 1.0 };
        let sz = if q.z < 0.0 { -1.0 } else { 1.0 };
        if outside > 1.0e-9 {
            let nl = v3(ox * sx / outside, oy * sy / outside, oz * sz / outside);
            (outside, mulv(&self.r, nl))
        } else {
            // inside: the face that is nearest (the largest, least negative d)
            let mut nl = v3(sx, 0.0, 0.0);
            let mut best = d.x;
            if d.y > best {
                best = d.y;
                nl = v3(0.0, sy, 0.0);
            }
            if d.z > best {
                best = d.z;
                nl = v3(0.0, 0.0, sz);
            }
            (best, mulv(&self.r, nl))
        }
    }
}

impl Solids {
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
    /// The nearest solid to `p`: its signed distance and outward normal; `None` when there is none.
    pub fn nearest(&self, p: V3) -> Option<(f32, V3)> {
        let mut best: Option<(f32, V3)> = None;
        for i in 0..self.n {
            let (d, n) = self.b[i].sdf(p);
            match best {
                Some((bd, _)) if bd <= d => {}
                _ => best = Some((d, n)),
            }
        }
        best
    }
    /// A box from its centre, a rotation of `angle` radians about the axis `axis` (a unit vector) and its half sizes.
    pub fn add(&mut self, c: V3, axis: V3, angle: f32, half: V3) {
        if self.n >= MAX_BOXES {
            return;
        }
        let q = [axis.x * crate::mathx::dsinf(angle * 0.5), axis.y * crate::mathx::dsinf(angle * 0.5), axis.z * crate::mathx::dsinf(angle * 0.5), crate::mathx::dcosf(angle * 0.5)];
        self.b[self.n] = SolidBox { c, r: from_quat(&q), half };
        self.n += 1;
    }
}
