//! Vectors, 3x3 matrices and quaternions (x, y, z, w) as the creature's physics uses them: plain f32 arithmetic in a fixed order, no fused multiply-add (Rust never contracts on its own).
use crate::mathx::datan2f;
use std::ops::{Add, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub fn v3(x: f32, y: f32, z: f32) -> V3 {
    V3 { x, y, z }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, b: V3) -> V3 {
        v3(self.x + b.x, self.y + b.y, self.z + b.z)
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, b: V3) -> V3 {
        v3(self.x - b.x, self.y - b.y, self.z - b.z)
    }
}
impl Mul<f32> for V3 {
    type Output = V3;
    fn mul(self, s: f32) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}

pub fn dot(a: V3, b: V3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}
pub fn cross(a: V3, b: V3) -> V3 {
    v3(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}
pub fn ld3(p: &[f32]) -> V3 {
    v3(p[0], p[1], p[2])
}
pub fn norm3(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 1e-12 {
        a * (1.0 / l)
    } else {
        v3(0.0, 1.0, 0.0)
    }
}

/// A 3x3 matrix, row major.
#[derive(Clone, Copy, Debug, Default)]
pub struct M3 {
    pub m: [f32; 9],
}

pub fn mulv(a: &M3, v: V3) -> V3 {
    v3(a.m[0] * v.x + a.m[1] * v.y + a.m[2] * v.z, a.m[3] * v.x + a.m[4] * v.y + a.m[5] * v.z, a.m[6] * v.x + a.m[7] * v.y + a.m[8] * v.z)
}

pub fn from_quat(q: &[f32; 4]) -> M3 {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let (xx, yy, zz, xy, xz, yz, wx, wy, wz) = (x * x, y * y, z * z, x * y, x * z, y * z, w * x, w * y, w * z);
    M3 {
        m: [
            1.0 - 2.0 * (yy + zz),
            2.0 * (xy - wz),
            2.0 * (xz + wy),
            2.0 * (xy + wz),
            1.0 - 2.0 * (xx + zz),
            2.0 * (yz - wx),
            2.0 * (xz - wy),
            2.0 * (yz + wx),
            1.0 - 2.0 * (xx + yy),
        ],
    }
}

/// The world inverse inertia: R diag(i) R^T.
pub fn world_inv_i(r: &M3, inv_i: &[f32; 3]) -> M3 {
    let mut o = M3::default();
    for a in 0..3 {
        for b in 0..3 {
            o.m[a * 3 + b] = r.m[a * 3] * inv_i[0] * r.m[b * 3] + r.m[a * 3 + 1] * inv_i[1] * r.m[b * 3 + 1] + r.m[a * 3 + 2] * inv_i[2] * r.m[b * 3 + 2];
        }
    }
    o
}

pub fn qmul(a: &[f32; 4], b: &[f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

pub fn qconj(a: &[f32; 4]) -> [f32; 4] {
    [-a[0], -a[1], -a[2], a[3]]
}

pub fn wrap_pi(mut a: f32) -> f32 {
    let pi = 3.141_592_653_589_79_f32;
    while a > pi {
        a -= 2.0 * pi;
    }
    while a < -pi {
        a += 2.0 * pi;
    }
    a
}

/// The angle of body c relative to its parent p about the hinge, since the start (positive: counter-clockwise about the axis in the parent's frame).
pub fn hinge_angle(qp: &[f32; 4], qc: &[f32; 4], qrel: &[f32; 4], axis_p: V3) -> f32 {
    let cp = qconj(qp);
    let rel = qmul(&cp, qc);
    let cr = qconj(qrel);
    let mut d = qmul(&rel, &cr);
    if d[3] < 0.0 {
        d = [-d[0], -d[1], -d[2], -d[3]];
    }
    wrap_pi(2.0 * datan2f(d[0] * axis_p.x + d[1] * axis_p.y + d[2] * axis_p.z, d[3]))
}
