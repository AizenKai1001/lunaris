//! How the spider looks: a mesh and a colour for each of its 50 bodies (their sizes are the model's: twice the half sizes), and the drawn-on details that are not part of the physics (eight eyes, the fangs, the
//! palps, a stripe on the abdomen and the spinnerets). A game that has a spider model of its own ignores this and follows `Creature::pose` with it.
use crate::model::{Model, LEGB, N_LEGS};

/// The mesh (a built-in shape of the engine), the colour and the glow of body `i`.
pub struct Look {
    pub mesh: &'static str,
    pub colour: [f32; 4],
    pub emissive: f32,
}

pub fn part_look(i: usize) -> Look {
    if i == 0 {
        return Look { mesh: "sphere", colour: [0.50, 0.28, 0.14, 1.0], emissive: 0.0 };
    }
    if i == 1 {
        return Look { mesh: "sphere", colour: [0.38, 0.20, 0.10, 1.0], emissive: 0.0 };
    }
    let leg = (i - 2) / LEGB;
    let seg = (i - 2) % LEGB;
    let shade = if leg < N_LEGS / 2 { 1.0 } else { 0.9 };
    let t = seg as f32 / 5.0;
    Look { mesh: "cylinder", colour: [(0.55 + 0.40 * t) * shade, (0.30 + 0.48 * t) * shade, (0.14 + 0.38 * t) * shade, 1.0], emissive: 0.0 }
}

/// The size (metres) a body is drawn at: the full extents of its box.
pub fn part_size(model: &Model, i: usize) -> [f32; 3] {
    [model.half[i][0] * 2.0, model.half[i][1] * 2.0, model.half[i][2] * 2.0]
}

/// A detail drawn on a body: where it sits in the body's own frame (metres), how it is turned, how big it is and what it looks like.
pub struct Decor {
    pub parent: usize,
    pub mesh: &'static str,
    pub offset: [f32; 3],
    pub rot: [f32; 4],
    pub size: [f32; 3],
    pub colour: [f32; 4],
    pub emissive: f32,
}

/// The rotation that takes +Y to the unit vector `d`.
pub fn q_y_to(d: [f32; 3]) -> [f32; 4] {
    let c = [d[2], 0.0, -d[0]]; // (0,1,0) x d
    let w = 1.0 + d[1];
    if w < 1.0e-6 {
        return [1.0, 0.0, 0.0, 0.0];
    }
    let n = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2] + w * w).sqrt();
    [c[0] / n, c[1] / n, c[2] / n, w / n]
}

/// Where a detail is in the world: the body's position and rotation, its offset (in the body's own frame) and its own turn.
pub fn decor_world(body_pos: crate::vecmath::V3, body_rot: [f32; 4], offset: [f32; 3], local_rot: [f32; 4]) -> ([f32; 3], [f32; 4]) {
    use crate::vecmath::{from_quat, mulv, qmul, v3};
    let r = from_quat(&body_rot);
    let o = mulv(&r, v3(offset[0], offset[1], offset[2]));
    ([body_pos.x + o.x, body_pos.y + o.y, body_pos.z + o.z], qmul(&body_rot, &local_rot))
}

pub fn decorations() -> Vec<Decor> {
    let mut v = Vec::new();
    // the prosoma is 0.22 x 0.10 x 0.17 and the abdomen 0.30 x 0.22 x 0.24; offsets below are fractions of them (0.5 is the tip)
    let (px, py, pz) = (0.22f32, 0.10f32, 0.17f32);
    let id = [0.0, 0.0, 0.0, 1.0];
    // the eyes: two large principal eyes in front, two rows of smaller ones on the sides and the top
    for (x, y, z, r) in [(0.43f32, 0.22f32, 0.13f32, 0.028f32), (0.43, 0.22, -0.13, 0.028), (0.40, 0.30, 0.34, 0.019), (0.40, 0.30, -0.34, 0.019), (0.22, 0.46, 0.18, 0.02), (0.22, 0.46, -0.18, 0.02), (0.10, 0.44, 0.32, 0.016), (0.10, 0.44, -0.32, 0.016)] {
        v.push(Decor { parent: 0, mesh: "sphere", offset: [x * px, y * py, z * pz], rot: id, size: [r * 2.0; 3], colour: [1.0, 0.85, 0.6, 1.0], emissive: 0.9 });
    }
    // the chelicerae (fangs) below the front, and the pedipalps (the short feelers) beside them
    for z in [0.16f32, -0.16] {
        v.push(Decor { parent: 0, mesh: "cylinder", offset: [0.5 * px, -0.32 * py, z * 0.7 * pz], rot: q_y_to([0.3, -0.95, 0.0]), size: [0.028, 0.06, 0.028], colour: [0.3, 0.12, 0.05, 1.0], emissive: 0.0 });
        v.push(Decor { parent: 0, mesh: "cylinder", offset: [0.5 * px, -0.1 * py, z * 2.2 * pz], rot: q_y_to([0.9, -0.3, 0.0]), size: [0.026, 0.09, 0.026], colour: [0.85, 0.55, 0.28, 1.0], emissive: 0.0 });
    }
    // the abdomen: a light stripe along the back and the spinnerets at the tail
    let (ax, ay, az) = (0.30f32, 0.22f32, 0.24f32);
    v.push(Decor { parent: 1, mesh: "sphere", offset: [0.0, 0.42 * ay, 0.0], rot: id, size: [0.9 * ax, 0.2 * ay, 0.28 * az], colour: [0.85, 0.7, 0.45, 1.0], emissive: 0.2 });
    for z in [0.05f32, -0.05] {
        v.push(Decor { parent: 1, mesh: "cylinder", offset: [-0.5 * ax, -0.05 * ay, z * 2.0 * az], rot: q_y_to([-0.9, 0.2, 0.0]), size: [0.025, 0.05, 0.025], colour: [0.3, 0.14, 0.06, 1.0], emissive: 0.0 });
    }
    v
}
