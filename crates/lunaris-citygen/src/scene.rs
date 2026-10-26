//! Scene files for the engine: the sun, sky, haze and exposure of a city picture, a camera, and zones that stream the city's cells in and out.

use lunaris_json::Json;

/// A rotation of `yaw` degrees about up and `pitch` degrees about the side, as the engine's quaternion (x y z w).
pub fn quat(yaw: f32, pitch: f32) -> [f32; 4] {
    let (sy, cy) = ((yaw.to_radians() / 2.0).sin(), (yaw.to_radians() / 2.0).cos());
    let (sp, cp) = ((pitch.to_radians() / 2.0).sin(), (pitch.to_radians() / 2.0).cos());
    [cy * sp, sy * cp, -sy * sp, cy * cp]
}

fn v(a: &[f32]) -> Json {
    Json::arr(a.iter().map(|x| Json::Float(*x as f64)))
}

fn n(x: f32) -> Json {
    Json::Float(x as f64)
}

/// How the picture looks: the sun, the sky and its haze, how bright the eye is.
#[derive(Clone, Debug)]
pub struct Look {
    pub sun_yaw: f32,
    pub sun_pitch: f32,
    pub sun: f32,
    pub ambient: f32,
    pub exposure_target: f32,
    pub turbidity: f32,
    pub fog: f32,
    pub fog_color: [f32; 3],
    /// The colour of the ground beyond the end of everything that is drawn.
    pub ground: [f32; 3],
    pub shadow_distance: f32,
    pub clouds: f32,
}

impl Default for Look {
    fn default() -> Look {
        Look { sun_yaw: -40.0, sun_pitch: -38.0, sun: 3.0, ambient: 1.0, exposure_target: 0.2, turbidity: 3.0, fog: 0.0, fog_color: [0.72, 0.80, 0.90], ground: [0.28, 0.28, 0.27], shadow_distance: 400.0, clouds: 0.25 }
    }
}

/// The entities every city picture has: environment, sun, atmosphere, shadows, exposure, global illumination.
pub fn base_entities(look: &Look) -> Vec<Json> {
    vec![
        Json::obj([("Environment", Json::obj([
            ("sky", Json::Bool(true)),
            ("clouds", n(look.clouds)),
            ("shadows", Json::Bool(true)),
            ("shadow_distance", n(look.shadow_distance)),
            ("ssao", n(1.2)),
            ("bloom", n(0.12)),
            ("vignette", n(0.08)),
            ("tonemap", Json::from(1)),
            ("ambient", n(look.ambient)),
            ("fog", n(look.fog)),
            ("fog_color", v(&look.fog_color)),
            ("ground", v(&look.ground)),
        ]))]),
        Json::obj([("Light", Json::obj([("kind", Json::from(0)), ("intensity", n(look.sun)), ("color", v(&[1.0, 0.95, 0.88]))])), ("Transform", Json::obj([("rot", v(&quat(look.sun_yaw, look.sun_pitch)))]))]),
        Json::obj([("Atmosphere", Json::obj([("turbidity", n(look.turbidity)), ("intensity", n(1.0))]))]),
        Json::obj([("Shadows", Json::obj([("softness", n(0.02))]))]),
        Json::obj([("AutoExposure", Json::obj([("min", n(0.25)), ("max", n(2.5)), ("speed", n(3.0)), ("target", n(look.exposure_target))]))]),
        Json::obj([("GlobalIllumination", Json::obj([("intensity", n(1.0)), ("radius", n(10.0))]))]),
    ]
}

/// A camera at `pos` looking along yaw and pitch (degrees).
pub fn camera(pos: [f32; 3], yaw: f32, pitch: f32, far: f32) -> Json {
    Json::obj([("Camera", Json::obj([("fov_y", n(60.0)), ("active", Json::Bool(true)), ("far", n(far))])), ("Transform", Json::obj([("pos", v(&pos)), ("rot", v(&quat(yaw, pitch)))]))])
}

/// A model at the origin (its vertices are already in the world frame).
pub fn model(path: &str) -> Json {
    Json::obj([("Transform", Json::obj([("pos", v(&[0.0, 0.0, 0.0]))])), ("MeshRenderer", Json::obj([("mesh", Json::str(path)), ("color", v(&[1.0, 1.0, 1.0, 1.0]))]))])
}

/// The scene document: entities get uids in order.
pub fn document(entities: Vec<Json>) -> Json {
    let list: Vec<Json> = entities.into_iter().enumerate().map(|(i, c)| Json::obj([("uid", Json::from(i + 1)), ("components", c)])).collect();
    Json::obj([("lunaris", Json::from(1)), ("entities", Json::Arr(list))])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_numbers_its_entities_from_one() {
        let doc = document(base_entities(&Look::default()));
        let list = doc.get("entities").unwrap().as_array().unwrap();
        assert_eq!(list.len(), 6);
        assert_eq!(list[0].get("uid").unwrap().as_i64(), Some(1));
        assert!(list[0].get("components").unwrap().get("Environment").is_some());
    }

    #[test]
    fn quaternions_are_unit_length() {
        let q = quat(35.0, -20.0);
        let l: f32 = q.iter().map(|x| x * x).sum();
        assert!((l - 1.0).abs() < 1e-5);
    }
}
