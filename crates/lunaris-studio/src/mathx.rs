//! The small amount of vector, quaternion and projection arithmetic the editor needs.
pub type V3 = [f32; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn mul(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
pub fn norm(a: V3) -> V3 {
    let l = len(a);
    if l > 1e-12 {
        mul(a, 1.0 / l)
    } else {
        [0.0; 3]
    }
}

pub type Q = [f32; 4];

pub fn q_mul(a: Q, b: Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
pub fn q_conj(q: Q) -> Q {
    [-q[0], -q[1], -q[2], q[3]]
}
pub fn q_norm(q: Q) -> Q {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n > 1e-12 {
        [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}
pub fn q_rotate(q: Q, v: V3) -> V3 {
    let u = [q[0], q[1], q[2]];
    let t = mul(cross(u, v), 2.0);
    add(add(v, mul(t, q[3])), cross(u, t))
}
pub fn q_axis_angle(axis: V3, radians: f32) -> Q {
    let a = norm(axis);
    let (s, c) = (radians * 0.5).sin_cos();
    [a[0] * s, a[1] * s, a[2] * s, c]
}

/// A rotation from Euler angles in degrees in the order the other editors use: roll (Z) first, then pitch (X), then yaw (Y).
pub fn q_from_euler(deg: V3) -> Q {
    let r = |d: f32| d.to_radians();
    let qy = q_axis_angle([0.0, 1.0, 0.0], r(deg[1]));
    let qx = q_axis_angle([1.0, 0.0, 0.0], r(deg[0]));
    let qz = q_axis_angle([0.0, 0.0, 1.0], r(deg[2]));
    q_norm(q_mul(qy, q_mul(qx, qz)))
}

/// The Euler angles (degrees) of a rotation, for `q_from_euler`'s order (R = Ry * Rx * Rz). Pitch is kept in -90..90.
pub fn q_to_euler(q: Q) -> V3 {
    let q = q_norm(q);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    // -R[1][2] = sin(pitch)
    let sp = (2.0 * (w * x - y * z)).clamp(-1.0, 1.0);
    let pitch = sp.asin();
    let (yaw, roll);
    if sp.abs() < 0.999_999 {
        yaw = (2.0 * (x * z + w * y)).atan2(1.0 - 2.0 * (x * x + y * y));
        roll = (2.0 * (x * y + w * z)).atan2(1.0 - 2.0 * (x * x + z * z));
    } else {
        // straight up or down: gimbal lock, all of the turn goes into yaw
        yaw = (-2.0 * (x * z - w * y)).atan2(1.0 - 2.0 * (y * y + z * z));
        roll = 0.0;
    }
    [pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees()]
}

/// A 4x4 matrix as the renderer hands it over (`lunaris_render::math::M4`, column-major).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat(pub [f32; 16]);

impl Mat {
    pub const IDENTITY: Mat = Mat([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);

    /// Clip-space coordinates of a world point (x, y, z, w).
    pub fn clip(&self, p: V3) -> [f32; 4] {
        let m = &self.0;
        [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
            m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15],
        ]
    }
}

/// A point of a segment in clip space, kept in front of the camera.
fn clip_near(a: [f32; 4], b: [f32; 4]) -> Option<([f32; 4], [f32; 4])> {
    const EPS: f32 = 1e-3;
    if a[3] <= EPS && b[3] <= EPS {
        return None;
    }
    let lerp = |p: [f32; 4], q: [f32; 4], t: f32| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, p[2] + (q[2] - p[2]) * t, p[3] + (q[3] - p[3]) * t];
    let (mut a, mut b) = (a, b);
    if a[3] <= EPS {
        a = lerp(a, b, (EPS - a[3]) / (b[3] - a[3]));
    } else if b[3] <= EPS {
        b = lerp(b, a, (EPS - b[3]) / (a[3] - b[3]));
    }
    Some((a, b))
}

/// Where a world point lands in a picture of `w` x `h` pixels (y down); `None` behind the camera.
pub fn to_screen(m: &Mat, p: V3, w: f32, h: f32) -> Option<[f32; 2]> {
    let c = m.clip(p);
    if c[3] <= 1e-3 {
        return None;
    }
    Some([(c[0] / c[3] * 0.5 + 0.5) * w, (0.5 - c[1] / c[3] * 0.5) * h])
}

/// A world segment in picture pixels, cut where it passes behind the camera.
pub fn segment_to_screen(m: &Mat, a: V3, b: V3, w: f32, h: f32) -> Option<([f32; 2], [f32; 2])> {
    let (ca, cb) = clip_near(m.clip(a), m.clip(b))?;
    let f = |c: [f32; 4]| [(c[0] / c[3] * 0.5 + 0.5) * w, (0.5 - c[1] / c[3] * 0.5) * h];
    Some((f(ca), f(cb)))
}

/// Distance in pixels from point `p` to the segment `a`-`b`.
pub fn dist_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (ab, ap) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
    let l2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if l2 > 1e-9 { ((ap[0] * ab[0] + ap[1] * ab[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
}

/// The parameter `t` of the point `origin + axis * t` of a line that is nearest to the ray `(ray_o, ray_d)`; `None` when they are parallel.
pub fn closest_on_line(origin: V3, axis: V3, ray_o: V3, ray_d: V3) -> Option<f32> {
    let w0 = sub(origin, ray_o);
    let (a, b, c) = (dot(axis, axis), dot(axis, ray_d), dot(ray_d, ray_d));
    let (d, e) = (dot(axis, w0), dot(ray_d, w0));
    let denom = a * c - b * b;
    if denom.abs() < 1e-6 {
        return None;
    }
    Some((b * e - c * d) / denom)
}

/// Where the ray meets the plane through `point` with normal `n`.
pub fn ray_plane(ray_o: V3, ray_d: V3, point: V3, n: V3) -> Option<V3> {
    let den = dot(ray_d, n);
    if den.abs() < 1e-6 {
        return None;
    }
    let t = dot(sub(point, ray_o), n) / den;
    (t >= 0.0).then(|| add(ray_o, mul(ray_d, t)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: V3, b: V3) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3 && (a[2] - b[2]).abs() < 1e-3
    }

    #[test]
    fn euler_angles_round_trip_and_rotate_like_the_other_editors() {
        for e in [[0.0, 0.0, 0.0], [30.0, 0.0, 0.0], [0.0, 45.0, 0.0], [0.0, 0.0, 60.0], [20.0, 40.0, 10.0], [-35.0, 120.0, -70.0], [10.0, -170.0, 80.0]] {
            let back = q_to_euler(q_from_euler(e));
            // the same rotation (the angles themselves may differ by whole turns)
            let (qa, qb) = (q_from_euler(e), q_from_euler(back));
            let d = (qa[0] * qb[0] + qa[1] * qb[1] + qa[2] * qb[2] + qa[3] * qb[3]).abs();
            assert!(d > 0.9999, "{e:?} -> {back:?}");
        }
        // yaw 90 turns +Z towards +X; pitch 90 about X turns +Y to +Z
        assert!(near(q_rotate(q_from_euler([0.0, 90.0, 0.0]), [0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]));
        assert!(near(q_rotate(q_from_euler([90.0, 0.0, 0.0]), [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
        assert!(near(q_rotate(q_from_euler([0.0, 0.0, 90.0]), [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
        // straight up is a gimbal lock and still comes back as the same rotation
        let up = q_from_euler([90.0, 30.0, 0.0]);
        let again = q_from_euler(q_to_euler(up));
        assert!((up[0] * again[0] + up[1] * again[1] + up[2] * again[2] + up[3] * again[3]).abs() > 0.999);
    }

    #[test]
    fn projection_helpers_clip_behind_the_camera_and_measure_pixels() {
        // a camera looking down -z at the origin from z = 5, as the renderer builds it
        let mut m = [0.0f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = -1.0;
        m[11] = -1.0; // w = -z
        m[14] = -1.0;
        m[15] = 5.0;
        // (w = -(z) + 5 ... for z = 0 the point is 5 in front: w = 5)
        let mat = Mat(m);
        let s = to_screen(&mat, [0.0, 0.0, 0.0], 200.0, 100.0).unwrap();
        assert!((s[0] - 100.0).abs() < 1e-3 && (s[1] - 50.0).abs() < 1e-3);
        assert!(to_screen(&mat, [0.0, 0.0, 9.0], 200.0, 100.0).is_none(), "behind the camera");
        let (a, b) = segment_to_screen(&mat, [0.0, 0.0, 0.0], [0.0, 0.0, 20.0], 200.0, 100.0).unwrap();
        assert!(a[0].is_finite() && b[0].is_finite(), "{a:?} {b:?}");
        assert!(segment_to_screen(&mat, [0.0, 0.0, 8.0], [1.0, 0.0, 9.0], 200.0, 100.0).is_none());
        assert!((dist_to_segment([5.0, 5.0], [0.0, 0.0], [10.0, 0.0]) - 5.0).abs() < 1e-5);
        assert!((dist_to_segment([-3.0, 4.0], [0.0, 0.0], [10.0, 0.0]) - 5.0).abs() < 1e-5, "beyond the end");
    }

    #[test]
    fn dragging_along_an_axis_finds_the_nearest_point_of_the_line() {
        // the x axis through the origin; a ray from (3, 5, 0) straight down hits it at x = 3
        let t = closest_on_line([0.0; 3], [1.0, 0.0, 0.0], [3.0, 5.0, 0.0], [0.0, -1.0, 0.0]).unwrap();
        assert!((t - 3.0).abs() < 1e-5);
        assert!(closest_on_line([0.0; 3], [1.0, 0.0, 0.0], [0.0, 5.0, 0.0], [1.0, 0.0, 0.0]).is_none(), "parallel");
        let p = ray_plane([0.0, 5.0, 0.0], [0.0, -1.0, 0.0], [0.0; 3], [0.0, 1.0, 0.0]).unwrap();
        assert!(near(p, [0.0; 3]));
        assert!(ray_plane([0.0, 5.0, 0.0], [0.0, 1.0, 0.0], [0.0; 3], [0.0, 1.0, 0.0]).is_none(), "the plane is behind");
    }
}
