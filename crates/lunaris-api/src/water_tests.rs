//! Water bodies as an agent meets them: `water_depth`, `InWater` and floating through the tools, and what `check` says about water that cannot be drawn or floats nothing.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn problems(e: &mut crate::Engine) -> Vec<String> {
    let c = e.call("check", &Json::Null).unwrap();
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| p.get("message").unwrap().as_str().unwrap().to_string()).collect()
}

fn write_grey16(path: &std::path::Path, f: &dyn Fn(usize, usize) -> u16) {
    let mut bytes = Vec::new();
    for z in 0..33 {
        for x in 0..33 {
            bytes.extend_from_slice(&f(x, z).to_be_bytes());
        }
    }
    let mut data = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut data, 33, 33);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Sixteen);
        enc.write_header().unwrap().write_image_data(&bytes).unwrap();
    }
    std::fs::write(path, data).unwrap();
}

fn radius(x: usize, z: usize) -> f32 {
    ((x as f32 - 16.0).powi(2) + (z as f32 - 16.0).powi(2)).sqrt()
}

/// A bowl of 64 m (0 at the middle to 6 m at the rim, 10 m of relief) with a lake at 3 m within 16 m of the middle: uid 1 is the terrain, uid 2 the water.
fn lake(e: &mut crate::Engine, tag: &str) {
    let dir = e.root.clone();
    write_grey16(&dir.join(format!("{tag}_bowl.png")), &|x, z| (0.6 * (radius(x, z) / 16.0).min(1.0) * 65535.0) as u16);
    write_grey16(&dir.join(format!("{tag}_lake.png")), &|x, z| if radius(x, z) <= 8.0 { (0.3 * 65535.0) as u16 } else { 0 });
    e.call("spawn", &j(&format!(r#"{{"uid":1,"components":{{"Transform":{{"pos":[0,0,0]}},"Terrain":{{"heightmap":"{tag}_bowl.png","size":[64,64],"height":10}}}}}}"#))).unwrap();
    e.call("spawn", &j(&format!(r#"{{"uid":2,"components":{{"Transform":{{"pos":[0,0,0]}},"WaterBody":{{"heightmap":"{tag}_lake.png","size":[64,64],"relief":10}}}}}}"#))).unwrap();
}

fn depth(e: &mut crate::Engine, args: &str) -> Json {
    e.call("water_depth", &j(args)).unwrap()
}

fn f(v: &Json, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64())
}

#[test]
fn water_depth_measures_the_surface_the_ground_and_the_water_between_them() {
    let mut e = engine();
    assert_eq!(e.call("water_depth", &j(r#"{"x":0,"z":0}"#)).unwrap_err().code, "no_water");
    lake(&mut e, "wd1");
    // the middle of the lake: the surface is 3 m up, the ground 0, so 3 m of water; with a height, how far above it the surface is
    let mid = depth(&mut e, r#"{"x":0,"z":0}"#);
    assert_eq!(mid.get("wet").unwrap().as_bool(), Some(true), "{mid:?}");
    assert!((f(&mid, "surface").unwrap() - 3.0).abs() < 0.01 && f(&mid, "ground").unwrap().abs() < 0.05 && (f(&mid, "depth").unwrap() - 3.0).abs() < 0.05, "{mid:?}");
    assert_eq!(mid.get("water").unwrap().as_u64(), Some(2));
    assert!(mid.get("above").is_none());
    let above = depth(&mut e, r#"{"x":0,"z":0,"y":1.25}"#);
    assert!((f(&above, "above").unwrap() - 1.75).abs() < 0.01, "{above:?}");
    // half way out, the ground is higher and the water shallower
    let side = depth(&mut e, r#"{"x":8,"z":0}"#);
    assert!(f(&side, "depth").unwrap() < f(&mid, "depth").unwrap() && f(&side, "depth").unwrap() > 0.5, "{side:?}");
    // the shore cell: the sheet runs on a little, but the ground is higher than it, so it is dry; beyond it there is no water at all
    let shore = depth(&mut e, r#"{"x":17.5,"z":0}"#);
    assert_eq!(shore.get("wet").unwrap().as_bool(), Some(false), "{shore:?}");
    assert_eq!(f(&shore, "depth"), Some(0.0), "{shore:?}");
    let bank = depth(&mut e, r#"{"x":26,"z":2}"#);
    assert_eq!((bank.get("wet").unwrap().as_bool(), f(&bank, "depth")), (Some(false), Some(0.0)), "{bank:?}");
    assert!(bank.get("surface").unwrap().is_null() && bank.get("water").unwrap().is_null(), "{bank:?}");
    assert!(f(&bank, "ground").unwrap() > 4.0);
    // outside the whole area, and bad arguments
    assert_eq!(depth(&mut e, r#"{"x":500,"z":0}"#).get("wet").unwrap().as_bool(), Some(false));
    for bad in [r#"{"x":"a","z":0}"#, r#"{"x":0}"#, r#"{"x":0,"z":0,"y":"high"}"#] {
        assert!(e.call("water_depth", &j(bad)).is_err(), "{bad}");
    }
    // a water picture that is missing is named, and nothing is wet
    e.call("spawn", &j(r#"{"uid":3,"components":{"Transform":{},"WaterBody":{"heightmap":"gone.png"}}}"#)).unwrap();
    let r = depth(&mut e, r#"{"x":0,"z":0}"#);
    assert_eq!(r.get("unread").unwrap().index(0).unwrap().as_str(), Some("gone.png"), "{r:?}");
    assert_eq!(r.get("wet").unwrap().as_bool(), Some(true), "the other lake is still there");
}

#[test]
fn a_hero_in_the_lake_gets_in_water_and_a_floating_ball_rides_the_surface() {
    let mut e = engine();
    lake(&mut e, "wd2");
    e.call("spawn", &j(r#"{"uid":10,"components":{"Transform":{"pos":[0,0.5,0]},"CharacterBody":{}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":11,"components":{"Transform":{"pos":[4,6,4]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1,"restitution":0.1},"Buoyant":{"density":500}}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":900}"#)).unwrap();
    // the hero fell to the bottom of the bowl and stands under 3 m of water
    let hero = e.call("get", &j(r#"{"uid":10,"components":["InWater","Transform"]}"#)).unwrap();
    let w = hero.get("components").unwrap().get("InWater").unwrap_or_else(|| panic!("{hero:?}"));
    assert!((f(w, "depth").unwrap() - 3.0).abs() < 0.3 && (f(w, "surface").unwrap() - 3.0).abs() < 0.01, "{w:?}");
    assert_eq!(w.get("body").unwrap().as_u64(), Some(2));
    // `query` finds the swimmers: anything with the component is in the water
    let q = e.call("query", &j(r#"{"with":["InWater"],"select":[]}"#)).unwrap();
    assert_eq!(q.get("total").unwrap().as_u64().unwrap(), 1, "{q:?}");
    // the ball floats with its middle at the surface
    let ball = e.call("get", &j(r#"{"uid":11,"components":["Transform","Buoyant"]}"#)).unwrap();
    let y = ball.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().index(1).unwrap().as_f64().unwrap();
    assert!((y - 3.0).abs() < 0.08, "{y}");
    assert!((f(ball.get("components").unwrap().get("Buoyant").unwrap(), "submerged").unwrap() - 0.5).abs() < 0.1);
    // the hero walks out onto the bank: the component goes
    e.call("set", &j(r#"{"uid":10,"component":"CharacterBody","value":{"walk":[4,0]}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":360}"#)).unwrap();
    let q = e.call("query", &j(r#"{"with":["InWater"],"select":[]}"#)).unwrap();
    assert_eq!(q.get("total").unwrap().as_u64().unwrap(), 0, "{q:?}");
}

#[test]
fn check_finds_water_that_cannot_be_drawn_and_floaters_that_cannot_float() {
    let mut good = engine();
    lake(&mut good, "wd3");
    good.call("spawn", &j(r#"{"uid":11,"components":{"Transform":{"pos":[0,6,0]},"Velocity":{},"Collider":{"shape":0},"RigidBody":{"mass":1},"Buoyant":{"density":500}}}"#)).unwrap();
    let p = problems(&mut good);
    assert!(p.iter().all(|m| !m.contains("WaterBody") && !m.contains("water") && !m.contains("Buoyant")), "{p:?}");
    // a picture with no water, one that is missing, an empty name, no place, nonsense numbers
    let mut e = engine();
    write_grey16(&e.root.join("wd4_dry.png"), &|_, _| 0);
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{},"WaterBody":{"heightmap":"wd4_dry.png","relief":0}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Transform":{},"WaterBody":{"heightmap":"wd4_gone.png","size":[0,5]}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Transform":{},"WaterBody":{}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":4,"components":{"WaterBody":{"heightmap":"wd4_dry.png"}}}"#)).unwrap();
    let p = problems(&mut e);
    for word in ["has no water in it", "wd4_gone.png", "heightmap is empty", "no Transform", "WaterBody.relief 0", "WaterBody.size"] {
        assert!(p.iter().any(|m| m.contains(word)), "{word}: {p:?}");
    }
    // floaters
    let mut f = engine();
    f.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{},"Buoyant":{"density":-4}}}"#)).unwrap();
    let p = problems(&mut f);
    assert!(p.iter().any(|m| m.contains("does not float")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("density -4")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("there is none in the world")), "{p:?}");
}
