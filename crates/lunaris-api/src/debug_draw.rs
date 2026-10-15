//! Debug overlays for the `render` tool: lines drawn over the finished picture that show what the simulation sees and the picture does not: the shapes of the
//! colliders, where things are heading, where agents are going, how far lights and sensors reach. They are drawn in front of everything (no hiding behind
//! walls), one pixel wide, in colours that say what they are. Only the picture changes.
use lunaris_ecs::{EcsError, Result, World};
use lunaris_render::math::{add, rotate, scale, M4, V3};
use lunaris_render::Frame;
use lunaris_sim::components::{CharacterBody, Collider, Light, NavAgent, RigidBody, Sensor, Transform, Velocity};

pub const KINDS: &[&str] = &["colliders", "velocity", "targets", "lights", "sensors", "wheels"];

pub struct Line {
    pub a: V3,
    pub b: V3,
    pub color: [u8; 3],
}

/// Which overlays a `debug` argument asks for: a list of names, or the word `all`.
pub fn parse_kinds(v: &lunaris_json::Json) -> Result<Vec<&'static str>> {
    let bad = |what: String| EcsError::new("bad_arguments", what).hint(format!("debug is a list of {} (or \"all\")", KINDS.join(", "))).at("debug");
    let names: Vec<String> = match v {
        lunaris_json::Json::Str(s) => vec![s.clone()],
        lunaris_json::Json::Arr(a) => a.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| bad("debug entries must be strings".into()))).collect::<Result<_>>()?,
        lunaris_json::Json::Bool(true) => vec!["all".to_string()],
        lunaris_json::Json::Null | lunaris_json::Json::Bool(false) => return Ok(Vec::new()),
        _ => return Err(bad("debug must be a list of names".into())),
    };
    let mut out = Vec::new();
    for n in names {
        if n == "all" {
            return Ok(KINDS.to_vec());
        }
        match KINDS.iter().find(|k| **k == n) {
            Some(k) => out.push(*k),
            None => return Err(bad(format!("unknown overlay `{n}`"))),
        }
    }
    Ok(out)
}

fn circle(c: V3, axis_u: V3, axis_v: V3, r: f32, color: [u8; 3], out: &mut Vec<Line>) {
    const N: usize = 24;
    let pt = |k: usize| {
        let a = k as f32 / N as f32 * std::f32::consts::TAU;
        add(c, add(scale(axis_u, r * a.cos()), scale(axis_v, r * a.sin())))
    };
    for k in 0..N {
        out.push(Line { a: pt(k), b: pt(k + 1), color });
    }
}

/// The lines of the asked-for overlays.
pub fn lines(world: &mut World, kinds: &[&str], meshes: &lunaris_sim::meshcol::MeshStore) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let want = |k: &str| kinds.contains(&k);
    if want("colliders") {
        // convex hulls: every edge (dynamic ones green, the rest gray), at most 500 of them
        let mut dynamic_hull: std::collections::BTreeSet<u64> = Default::default();
        let _ = world.for_each_uid::<(&RigidBody, &Velocity), _>(|_, uid, (rb, _)| {
            if rb.mass > 0.0 {
                dynamic_hull.insert(uid);
            }
        });
        for h in meshes.hull_instances(world).into_iter().take(500) {
            let axes = h.axes();
            let color = if h.trigger {
                [240, 210, 40]
            } else if dynamic_hull.contains(&h.uid) {
                [60, 220, 90]
            } else {
                [160, 160, 160]
            };
            let w = |i: u16| {
                let v = h.hull.verts[i as usize];
                add(h.pos, add(add(scale(axes[0], v[0]), scale(axes[1], v[1])), scale(axes[2], v[2])))
            };
            for e in &h.hull.edges {
                out.push(Line { a: w(e.a), b: w(e.b), color });
            }
        }
        // level geometry (a MeshCollider): every triangle's edges when the model is small, otherwise the box round it
        for inst in meshes.instances(world) {
            let color = [120, 150, 190];
            let pm = &inst.placed;
            if pm.triangle_count() <= 3000 {
                for i in 0..pm.triangle_count() as u32 {
                    let [a, b, c] = pm.tri(i);
                    out.extend([Line { a, b, color }, Line { a: b, b: c, color }, Line { a: c, b: a, color }]);
                }
            } else {
                let (lo, hi) = (pm.bounds.lo, pm.bounds.hi);
                let corner = |k: usize| [if k & 1 == 0 { lo[0] } else { hi[0] }, if k & 2 == 0 { lo[1] } else { hi[1] }, if k & 4 == 0 { lo[2] } else { hi[2] }];
                for (i, j) in [(0, 1), (1, 3), (3, 2), (2, 0), (4, 5), (5, 7), (7, 6), (6, 4), (0, 4), (1, 5), (2, 6), (3, 7)] {
                    out.push(Line { a: corner(i), b: corner(j), color });
                }
            }
        }
        let mut dynamic: std::collections::BTreeSet<u64> = Default::default();
        let _ = world.for_each_uid::<(&RigidBody, &Velocity), _>(|_, uid, (rb, _)| {
            if rb.mass > 0.0 {
                dynamic.insert(uid);
            }
        });
        let mut characters: std::collections::BTreeSet<u64> = Default::default();
        let _ = world.for_each_uid::<(&CharacterBody,), _>(|_, uid, _| {
            characters.insert(uid);
        });
        let mut bodies: Vec<(u64, Transform, Collider)> = Vec::new();
        let _ = world.for_each_uid::<(&Transform, &Collider), _>(|_, uid, (t, c)| bodies.push((uid, *t, *c)));
        bodies.sort_by_key(|b| b.0);
        for (uid, t, c) in bodies.into_iter().take(2000) {
            let color = if c.trigger {
                [240, 210, 40]
            } else if characters.contains(&uid) {
                [80, 160, 255]
            } else if dynamic.contains(&uid) {
                [60, 220, 90]
            } else {
                [160, 160, 160]
            };
            let s = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
            let rot = if t.rot.iter().all(|v| v.is_finite()) { t.rot } else { [0.0, 0.0, 0.0, 1.0] };
            let axes = [rotate(rot, [1.0, 0.0, 0.0]), rotate(rot, [0.0, 1.0, 0.0]), rotate(rot, [0.0, 0.0, 1.0])];
            match c.shape {
                0 => {
                    let r = 0.5 * c.size[0].abs() * s[0].max(s[1]).max(s[2]);
                    circle(t.pos, axes[0], axes[1], r, color, &mut out);
                    circle(t.pos, axes[0], axes[2], r, color, &mut out);
                    circle(t.pos, axes[1], axes[2], r, color, &mut out);
                }
                2 => {
                    let r = 0.5 * c.size[0].abs() * s[0].max(s[2]);
                    let half = (0.5 * c.size[1].abs() * s[1] - r).max(0.0);
                    for sign in [-1.0f32, 1.0] {
                        let end = add(t.pos, scale(axes[1], half * sign));
                        circle(end, axes[0], axes[2], r, color, &mut out);
                        circle(end, axes[0], axes[1], r, color, &mut out);
                    }
                    for (ua, sa) in [(axes[0], 1.0f32), (axes[0], -1.0), (axes[2], 1.0), (axes[2], -1.0)] {
                        let side = scale(ua, r * sa);
                        out.push(Line { a: add(add(t.pos, scale(axes[1], half)), side), b: add(add(t.pos, scale(axes[1], -half)), side), color });
                    }
                }
                _ => {
                    let h = [0.5 * c.size[0].abs() * s[0], 0.5 * c.size[1].abs() * s[1], 0.5 * c.size[2].abs() * s[2]];
                    let corner = |sx: f32, sy: f32, sz: f32| add(t.pos, add(add(scale(axes[0], h[0] * sx), scale(axes[1], h[1] * sy)), scale(axes[2], h[2] * sz)));
                    for (i, j) in [(0, 1), (1, 3), (3, 2), (2, 0), (4, 5), (5, 7), (7, 6), (6, 4), (0, 4), (1, 5), (2, 6), (3, 7)] {
                        let p = |k: usize| corner(if k & 1 == 0 { -1.0 } else { 1.0 }, if k & 2 == 0 { -1.0 } else { 1.0 }, if k & 4 == 0 { -1.0 } else { 1.0 });
                        out.push(Line { a: p(i), b: p(j), color });
                    }
                }
            }
        }
    }
    if want("velocity") {
        let _ = world.for_each::<(&Transform, &Velocity), _>(|_, (t, v)| {
            if v.lin.iter().all(|x| x.is_finite()) && v.lin.iter().any(|x| x.abs() > 1e-3) {
                out.push(Line { a: t.pos, b: add(t.pos, scale(v.lin, 0.5)), color: [250, 70, 70] });
            }
        });
    }
    if want("targets") {
        let _ = world.for_each::<(&Transform, &NavAgent), _>(|_, (t, n)| {
            if n.target.iter().all(|x| x.is_finite()) && (n.active || n.status == 1) {
                out.push(Line { a: t.pos, b: n.target, color: [230, 80, 230] });
                for axis in [[0.3, 0.0, 0.0], [0.0, 0.3, 0.0], [0.0, 0.0, 0.3]] {
                    out.push(Line { a: add(n.target, scale(axis, -1.0)), b: add(n.target, axis), color: [230, 80, 230] });
                }
            }
        });
    }
    if want("lights") {
        let _ = world.for_each::<(&Transform, &Light), _>(|_, (t, l)| {
            if l.kind != 0 && l.range.is_finite() && l.range > 0.0 {
                for (u, v) in [([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0])] {
                    circle(t.pos, u, v, l.range.min(500.0), [255, 160, 40], &mut out);
                }
            }
        });
    }
    if want("wheels") {
        // the wheels of raycast vehicles: the ray down the strut (dark), the strut to the wheel's centre (yellow) and the wheel itself (green on the ground, red in the air)
        for w in lunaris_sim::vehicle::wheel_views(world).into_iter().take(512) {
            let color = if w.grounded { [60, 220, 90] } else { [230, 70, 60] };
            out.push(Line { a: w.mount, b: w.ray_end, color: [110, 110, 130] });
            out.push(Line { a: w.mount, b: w.centre, color: [240, 210, 40] });
            circle(w.centre, w.forward, w.up, w.radius, color, &mut out);
            // (a stroke across the wheel along its axle, so that a turning wheel can be told from a sliding one)
            out.push(Line { a: add(w.centre, scale(w.axle, -0.5 * w.radius)), b: add(w.centre, scale(w.axle, 0.5 * w.radius)), color });
        }
    }
    if want("sensors") {
        let _ = world.for_each::<(&Transform, &Sensor), _>(|_, (t, s)| {
            if s.radius.is_finite() && s.radius > 0.0 {
                circle(t.pos, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], s.radius.min(500.0), [80, 220, 220], &mut out);
                // the edges of the cone of sight (the sensor looks down its local -z)
                if s.fov.is_finite() && s.fov < 359.0 {
                    let half = (s.fov.clamp(1.0, 358.0) as f64 / 2.0).to_radians();
                    let (sn, cs) = (half.sin() as f32, half.cos() as f32);
                    for sign in [-1.0f32, 1.0] {
                        let local = [sign * sn * s.radius, 0.0, -cs * s.radius];
                        out.push(Line { a: t.pos, b: add(t.pos, rotate(t.rot, local)), color: [80, 220, 220] });
                    }
                }
            }
        });
    }
    out
}

/// Draw `lines` onto the picture through the picture's own view-projection.
pub fn draw(frame: &mut Frame, lines: &[Line]) {
    let (w, h) = (frame.width as i64, frame.height as i64);
    if w == 0 || h == 0 {
        return;
    }
    let vp: M4 = frame.view_proj;
    const EPS: f32 = 1e-3;
    for l in lines {
        let (mut a, mut b) = (vp.mul_v4([l.a[0], l.a[1], l.a[2], 1.0]), vp.mul_v4([l.b[0], l.b[1], l.b[2], 1.0]));
        if !a.iter().chain(&b).all(|v| v.is_finite()) || (a[3] <= EPS && b[3] <= EPS) {
            continue;
        }
        // cut at the near side: the part of the line with a positive w
        if a[3] <= EPS || b[3] <= EPS {
            let (inside, outside) = if a[3] > EPS { (a, b) } else { (b, a) };
            let t = (inside[3] - EPS) / (inside[3] - outside[3]);
            let cut = [inside[0] + (outside[0] - inside[0]) * t, inside[1] + (outside[1] - inside[1]) * t, inside[2] + (outside[2] - inside[2]) * t, EPS];
            if a[3] <= EPS {
                a = cut;
            } else {
                b = cut;
            }
        }
        let px = |p: [f32; 4]| ((p[0] / p[3] * 0.5 + 0.5) as f64 * w as f64, (1.0 - (p[1] / p[3] * 0.5 + 0.5)) as f64 * h as f64);
        let ((x0, y0), (x1, y1)) = (px(a), px(b));
        if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) || (x0 - x1).abs().max((y0 - y1).abs()) > 20000.0 {
            continue;
        }
        let steps = ((x1 - x0).abs().max((y1 - y0).abs()).ceil() as i64).clamp(1, 8192);
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            let (x, y) = ((x0 + (x1 - x0) * t).floor() as i64, (y0 + (y1 - y0) * t).floor() as i64);
            if x >= 0 && y >= 0 && x < w && y < h {
                let o = ((y * w + x) * 4) as usize;
                frame.rgba[o..o + 3].copy_from_slice(&l.color);
            }
        }
    }
}
