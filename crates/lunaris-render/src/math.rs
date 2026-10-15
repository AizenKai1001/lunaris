//! Small linear algebra for rendering. Right-handed, +Y up, the camera looks down -Z, clip space follows OpenGL (z in [-w, w]).
//! Matrices are column-major: element (row r, column c) is `m[c * 4 + r]`. Only exact IEEE operations plus `lunaris_sim::detmath`
//! are used, so results are bit-identical on every platform.
use lunaris_sim::detmath;

pub type V3 = [f32; 3];

#[inline]
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
pub fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}
#[inline]
pub fn normalize(a: V3) -> V3 {
    let l = length(a);
    if l > 0.0 {
        scale(a, 1.0 / l)
    } else {
        a
    }
}

/// Rotate `v` by the unit quaternion `q = (x, y, z, w)`.
pub fn rotate(q: [f32; 4], v: V3) -> V3 {
    let qv = [q[0], q[1], q[2]];
    let t = scale(cross(qv, v), 2.0);
    add(add(v, scale(t, q[3])), cross(qv, t))
}

/// Normalise a quaternion; a zero quaternion becomes the identity.
pub fn quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n > 0.0 {
        let i = 1.0 / n;
        [q[0] * i, q[1] * i, q[2] * i, q[3] * i]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// The rotation that does `b` first and then `a` (quaternions as x, y, z, w).
pub fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// Rotation about a unit axis by `radians`.
pub fn quat_axis_angle(axis: V3, radians: f32) -> [f32; 4] {
    let h = radians as f64 * 0.5;
    let (s, c) = (detmath::sin(h) as f32, detmath::cos(h) as f32);
    let a = normalize(axis);
    [a[0] * s, a[1] * s, a[2] * s, c]
}

/// Rotation that turns the local -Z axis towards `target` as seen from `eye`, keeping `up` upwards.
pub fn quat_look_at(eye: V3, target: V3, up: V3) -> [f32; 4] {
    let f = normalize(sub(target, eye)); // forward (-Z in local space)
    let mut r = normalize(cross(f, up));
    if length(r) == 0.0 {
        r = [1.0, 0.0, 0.0];
    }
    let u = cross(r, f);
    // basis columns: right, up, back (= -forward)
    let (m00, m10, m20) = (r[0], r[1], r[2]);
    let (m01, m11, m21) = (u[0], u[1], u[2]);
    let (m02, m12, m22) = (-f[0], -f[1], -f[2]);
    let tr = m00 + m11 + m22;
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    };
    quat_normalize(q)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M4(pub [f32; 16]);

impl M4 {
    pub fn identity() -> M4 {
        let mut m = [0.0; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        M4(m)
    }

    /// `self * o` (apply `o` first).
    pub fn mul(&self, o: &M4) -> M4 {
        let mut r = [0.0f32; 16];
        for c in 0..4 {
            for row in 0..4 {
                let mut s = 0.0;
                for k in 0..4 {
                    s += self.0[k * 4 + row] * o.0[c * 4 + k];
                }
                r[c * 4 + row] = s;
            }
        }
        M4(r)
    }

    #[inline]
    pub fn mul_v4(&self, v: [f32; 4]) -> [f32; 4] {
        let m = &self.0;
        [
            m[0] * v[0] + m[4] * v[1] + m[8] * v[2] + m[12] * v[3],
            m[1] * v[0] + m[5] * v[1] + m[9] * v[2] + m[13] * v[3],
            m[2] * v[0] + m[6] * v[1] + m[10] * v[2] + m[14] * v[3],
            m[3] * v[0] + m[7] * v[1] + m[11] * v[2] + m[15] * v[3],
        ]
    }

    pub fn transform_point(&self, p: V3) -> V3 {
        let r = self.mul_v4([p[0], p[1], p[2], 1.0]);
        [r[0], r[1], r[2]]
    }

    /// Translation * rotation * scale.
    pub fn from_trs(t: V3, q: [f32; 4], s: V3) -> M4 {
        let q = quat_normalize(q);
        let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
        let (xx, yy, zz) = (x * x, y * y, z * z);
        let (xy, xz, yz) = (x * y, x * z, y * z);
        let (wx, wy, wz) = (w * x, w * y, w * z);
        let r00 = 1.0 - 2.0 * (yy + zz);
        let r01 = 2.0 * (xy - wz);
        let r02 = 2.0 * (xz + wy);
        let r10 = 2.0 * (xy + wz);
        let r11 = 1.0 - 2.0 * (xx + zz);
        let r12 = 2.0 * (yz - wx);
        let r20 = 2.0 * (xz - wy);
        let r21 = 2.0 * (yz + wx);
        let r22 = 1.0 - 2.0 * (xx + yy);
        let mut m = [0.0; 16];
        m[0] = r00 * s[0];
        m[1] = r10 * s[0];
        m[2] = r20 * s[0];
        m[4] = r01 * s[1];
        m[5] = r11 * s[1];
        m[6] = r21 * s[1];
        m[8] = r02 * s[2];
        m[9] = r12 * s[2];
        m[10] = r22 * s[2];
        m[12] = t[0];
        m[13] = t[1];
        m[14] = t[2];
        m[15] = 1.0;
        M4(m)
    }

    /// View matrix of a camera placed at `pos` with orientation `q` (the inverse of its rigid transform).
    pub fn view(pos: V3, q: [f32; 4]) -> M4 {
        let r = M4::from_trs([0.0; 3], q, [1.0; 3]);
        let mut m = [0.0; 16];
        for row in 0..3 {
            for col in 0..3 {
                m[col * 4 + row] = r.0[row * 4 + col]; // transpose of the rotation
            }
        }
        let t = [-pos[0], -pos[1], -pos[2]];
        for row in 0..3 {
            m[12 + row] = m[row] * t[0] + m[4 + row] * t[1] + m[8 + row] * t[2];
        }
        m[15] = 1.0;
        M4(m)
    }

    /// Perspective projection; `fov_y` in degrees.
    pub fn perspective(fov_y_deg: f32, aspect: f32, near: f32, far: f32) -> M4 {
        let half = (fov_y_deg as f64).to_radians_det() * 0.5;
        let f = (1.0 / detmath::tan(half)) as f32;
        let mut m = [0.0; 16];
        m[0] = f / aspect;
        m[5] = f;
        m[10] = (far + near) / (near - far);
        m[11] = -1.0;
        m[14] = 2.0 * far * near / (near - far);
        M4(m)
    }

    /// Orthographic projection showing `height` world units from bottom to top.
    pub fn ortho(height: f32, aspect: f32, near: f32, far: f32) -> M4 {
        let hh = height * 0.5;
        let hw = hh * aspect;
        let mut m = [0.0; 16];
        m[0] = 1.0 / hw;
        m[5] = 1.0 / hh;
        m[10] = -2.0 / (far - near);
        m[14] = -(far + near) / (far - near);
        m[15] = 1.0;
        M4(m)
    }

    /// Direction transform (no translation).
    #[inline]
    pub fn transform_dir(&self, v: V3) -> V3 {
        let m = &self.0;
        [m[0] * v[0] + m[4] * v[1] + m[8] * v[2], m[1] * v[0] + m[5] * v[1] + m[9] * v[2], m[2] * v[0] + m[6] * v[1] + m[10] * v[2]]
    }

    /// Inverse of a matrix made of rotation, scale and translation (any affine matrix with a non-singular 3x3 part).
    /// A singular matrix returns the identity.
    pub fn inverse_affine(&self) -> M4 {
        let m = &self.0;
        let a = |r: usize, c: usize| m[c * 4 + r];
        let cof = |r: usize, c: usize| {
            let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
            let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
            a(r1, c1) * a(r2, c2) - a(r1, c2) * a(r2, c1)
        };
        let det = a(0, 0) * cof(0, 0) + a(0, 1) * cof(0, 1) + a(0, 2) * cof(0, 2);
        if det == 0.0 || !det.is_finite() {
            return M4::identity();
        }
        let inv_det = 1.0 / det;
        let mut r = [0.0f32; 16];
        for row in 0..3 {
            for col in 0..3 {
                r[col * 4 + row] = cof(col, row) * inv_det; // adjugate = transposed cofactors
            }
        }
        let t = [m[12], m[13], m[14]];
        for row in 0..3 {
            r[12 + row] = -(r[row] * t[0] + r[4 + row] * t[1] + r[8 + row] * t[2]);
        }
        r[15] = 1.0;
        M4(r)
    }

    /// Matrix that transforms normals: the cofactor matrix of the upper 3x3 (the inverse transpose up to scale), sign-corrected
    /// so mirrored transforms do not flip normals.
    pub fn normal_matrix(&self) -> [[f32; 3]; 3] {
        let m = &self.0;
        let a = |r: usize, c: usize| m[c * 4 + r];
        let cof = |r: usize, c: usize| {
            let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
            let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
            a(r1, c1) * a(r2, c2) - a(r1, c2) * a(r2, c1)
        };
        let det = a(0, 0) * cof(0, 0) + a(0, 1) * cof(0, 1) + a(0, 2) * cof(0, 2);
        let s = if det < 0.0 { -1.0 } else { 1.0 };
        let mut out = [[0.0; 3]; 3];
        for (r, row) in out.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = cof(r, c) * s;
            }
        }
        out
    }
}

trait ToRadiansDet {
    fn to_radians_det(self) -> f64;
}
impl ToRadiansDet for f64 {
    fn to_radians_det(self) -> f64 {
        self * (detmath::PI / 180.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: V3, b: V3) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5)
    }

    #[test]
    fn quaternion_rotation_and_axis_angle() {
        let q = quat_axis_angle([0.0, 0.0, 1.0], (detmath::PI / 2.0) as f32);
        assert!(close(rotate(q, [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
        assert!(close(rotate([0.0, 0.0, 0.0, 1.0], [1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]));
    }

    #[test]
    fn trs_matches_manual_transform() {
        let q = quat_axis_angle([0.0, 1.0, 0.0], (detmath::PI / 2.0) as f32);
        let m = M4::from_trs([1.0, 2.0, 3.0], q, [2.0, 2.0, 2.0]);
        // local +X (scaled by 2) rotated 90 degrees about Y points to -Z
        assert!(close(m.transform_point([1.0, 0.0, 0.0]), [1.0, 2.0, 1.0]));
    }

    #[test]
    fn view_matrix_inverts_the_camera_transform() {
        let q = quat_axis_angle([0.0, 1.0, 0.0], 0.7);
        let pos = [3.0, 1.0, -2.0];
        let cam = M4::from_trs(pos, q, [1.0; 3]);
        let v = M4::view(pos, q);
        let p = [0.5, -1.5, 4.0];
        assert!(close(v.transform_point(cam.transform_point(p)), p));
    }

    #[test]
    fn perspective_maps_near_and_far_planes_to_minus_one_and_one() {
        let p = M4::perspective(60.0, 1.5, 0.5, 100.0);
        let near = p.mul_v4([0.0, 0.0, -0.5, 1.0]);
        let far = p.mul_v4([0.0, 0.0, -100.0, 1.0]);
        assert!((near[2] / near[3] + 1.0).abs() < 1e-5);
        assert!((far[2] / far[3] - 1.0).abs() < 1e-4);
        // the top edge of the field of view at distance 10 sits at y = 10 * tan(30 deg)
        let top = p.mul_v4([0.0, 10.0 * 0.577_350_3, -10.0, 1.0]);
        assert!((top[1] / top[3] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn affine_inverse_undoes_trs() {
        let q = quat_axis_angle([1.0, 2.0, 3.0], 0.9);
        let m = M4::from_trs([1.0, -2.0, 3.0], q, [2.0, 0.5, 3.0]);
        let inv = m.inverse_affine();
        let p = [0.3, -4.0, 2.5];
        assert!(close(inv.transform_point(m.transform_point(p)), p));
        assert!(close(m.transform_point(inv.transform_point(p)), p));
        assert_eq!(M4([0.0; 16]).inverse_affine(), M4::identity());
    }

    #[test]
    fn look_at_points_the_camera_at_the_target() {
        let eye = [0.0, 0.0, 5.0];
        let q = quat_look_at(eye, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert!(close(rotate(q, [0.0, 0.0, -1.0]), [0.0, 0.0, -1.0]));
        let q = quat_look_at([5.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert!(close(rotate(q, [0.0, 0.0, -1.0]), [-1.0, 0.0, 0.0]));
        let q = quat_look_at([1.0, 4.0, 2.0], [-3.0, 0.5, 1.0], [0.0, 1.0, 0.0]);
        let want = normalize(sub([-3.0, 0.5, 1.0], [1.0, 4.0, 2.0]));
        assert!(close(rotate(q, [0.0, 0.0, -1.0]), want));
    }

    #[test]
    fn normal_matrix_handles_non_uniform_scale() {
        let m = M4::from_trs([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0, 4.0, 1.0]);
        let n = m.normal_matrix();
        // a surface normal pointing up-right stays perpendicular to a surface stretched in Y
        let v = [n[0][0] * 1.0 + n[0][1] * 1.0, n[1][0] * 1.0 + n[1][1] * 1.0];
        assert!(v[0] > v[1], "{v:?}");
    }
}
