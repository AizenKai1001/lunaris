//! The terrain tools: ground made from a heightmap picture, as an agent meets it (check, terrain_height, reload_terrain, raycast, step).
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

/// A 33 x 33, 16-bit greyscale picture: a ramp that rises from 0 at the west edge to full height at the east edge.
fn write_ramp(dir: &std::path::Path, name: &str) {
    let mut bytes = Vec::new();
    for _z in 0..33 {
        for x in 0..33u32 {
            bytes.extend_from_slice(&((x * 65535 / 32) as u16).to_be_bytes());
        }
    }
    let mut data = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut data, 33, 33);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Sixteen);
        enc.write_header().unwrap().write_image_data(&bytes).unwrap();
    }
    std::fs::write(dir.join(name), data).unwrap();
}

fn problems(e: &mut crate::Engine) -> Vec<String> {
    let c = e.call("check", &Json::Null).unwrap();
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| p.get("message").unwrap().as_str().unwrap().to_string()).collect()
}

#[test]
fn a_terrain_is_checked_measured_walked_on_and_read_again_when_its_picture_changes() {
    let mut e = engine();
    let dir = e.root.clone();
    let _ = std::fs::remove_file(dir.join("ramp.png"));
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{"pos":[0,0,0]},"Terrain":{"heightmap":"ramp.png","size":[64,64],"height":10}}}"#)).unwrap();
    // the picture is not there yet: `check` says so, and there is no ground
    let before = problems(&mut e);
    assert!(before.iter().any(|m| m.contains("ramp.png") && m.contains("could not be used")), "{before:?}");
    let none = e.call("terrain_height", &j(r#"{"x":0,"z":0}"#)).unwrap_err();
    assert_eq!(none.code, "no_terrain");
    // the picture appears; the engine does not look again by itself for a while, `reload_terrain` makes it
    write_ramp(&dir, "ramp.png");
    let r = e.call("reload_terrain", &Json::Null).unwrap();
    assert_eq!(r.get("loaded").unwrap().as_array().unwrap().len(), 1, "{r:?}");
    assert!(problems(&mut e).iter().all(|m| !m.contains("ramp.png")), "{:?}", problems(&mut e));
    // height and slope: 5 m in the middle, 0 at the west edge, 10 m at the east edge; a slope of 10 m in 64 m leans the normal west
    let h = |e: &mut crate::Engine, x: f64, z: f64| e.call("terrain_height", &j(&format!(r#"{{"x":{x},"z":{z}}}"#))).unwrap();
    let mid = h(&mut e, 0.0, 0.0);
    assert!((mid.get("height").unwrap().as_f64().unwrap() - 5.0).abs() < 0.05, "{mid:?}");
    assert!(mid.get("normal").unwrap().index(0).unwrap().as_f64().unwrap() < -0.1, "{mid:?}");
    assert_eq!(mid.get("uid").unwrap().as_u64(), Some(1));
    assert!(h(&mut e, -31.9, 0.0).get("height").unwrap().as_f64().unwrap() < 0.1);
    assert!(h(&mut e, 31.9, 5.0).get("height").unwrap().as_f64().unwrap() > 9.9);
    assert!(e.call("terrain_height", &j(r#"{"x":40,"z":0}"#)).is_err(), "outside the ground");
    assert!(e.call("terrain_height", &j(r#"{"x":"a","z":0}"#)).is_err());
    // a ray finds the same ground
    let ray = e.call("raycast", &j(r#"{"origin":[0,50,0],"direction":[0,-1,0]}"#)).unwrap();
    let hit = ray.get("point").unwrap();
    assert!((hit.index(1).unwrap().as_f64().unwrap() - 5.0).abs() < 0.05, "{ray:?}");
    // a ball dropped onto it stops on it
    e.call("spawn", &j(r#"{"uid":2,"components":{"Transform":{"pos":[0,20,0]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1,"restitution":0.1}}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":300}"#)).unwrap();
    let ball = e.call("get", &j(r#"{"uid":2,"components":["Transform"]}"#)).unwrap();
    let y = ball.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().index(1).unwrap().as_f64().unwrap();
    assert!(y > 4.9 && y < 8.0, "the ball rests on the ground (5 m at the middle, a little more up the slope): {y}");
    // the picture is replaced: `reload_terrain` reads the new one (flat and black)
    let flat = vec![0u8; 33 * 33 * 2];
    let mut data = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut data, 33, 33);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Sixteen);
        enc.write_header().unwrap().write_image_data(&flat).unwrap();
    }
    std::fs::write(dir.join("ramp.png"), data).unwrap();
    e.call("reload_terrain", &Json::Null).unwrap();
    assert!(h(&mut e, 0.0, 0.0).get("height").unwrap().as_f64().unwrap().abs() < 0.01);
    // `reset` keeps the workspace folder, so a terrain still finds its picture
    e.call("reset", &Json::Null).unwrap();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{"pos":[0,3,0]},"Terrain":{"heightmap":"ramp.png","size":[64,64],"height":10}}}"#)).unwrap();
    assert!((h(&mut e, 0.0, 0.0).get("height").unwrap().as_f64().unwrap() - 3.0).abs() < 0.01);
}

#[test]
fn a_terrain_without_a_picture_name_or_a_place_is_reported() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Terrain":{"size":[10,10],"height":1}}}"#)).unwrap();
    let p = problems(&mut e);
    assert!(p.iter().any(|m| m.contains("no Transform")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("heightmap is empty")), "{p:?}");
}

fn ramp_world(e: &mut crate::Engine, name: &str) {
    let dir = e.root.clone();
    write_ramp(&dir, name);
    e.call("spawn", &j(&format!(r#"{{"uid":1,"components":{{"Transform":{{"pos":[0,0,0]}},"Terrain":{{"heightmap":"{name}","size":[64,64],"height":10}}}}}}"#))).unwrap();
}

fn position(e: &mut crate::Engine, uid: u64) -> [f64; 3] {
    let r = e.call("get", &j(&format!(r#"{{"uid":{uid},"components":["Transform"]}}"#))).unwrap();
    let p = r.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap();
    [p.index(0).unwrap().as_f64().unwrap(), p.index(1).unwrap().as_f64().unwrap(), p.index(2).unwrap().as_f64().unwrap()]
}

#[test]
fn scatter_puts_copies_on_the_ground_apart_from_each_other_the_same_way_every_time_and_undoes() {
    let go = || {
        let mut e = engine();
        ramp_world(&mut e, "scatter_ramp.png");
        let r = e.call("scatter", &j(r#"{"count":40,"seed":5,"min_distance":3,"scale":[0.8,1.2],"components":{"Name":{"value":"Tree"},"MeshRenderer":{"mesh":"cube"}}}"#)).unwrap();
        (e, r)
    };
    let (mut e, r) = go();
    assert_eq!(r.get("placed").unwrap().as_u64(), Some(40), "{r:?}");
    let (first, last) = (r.get("first_uid").unwrap().as_u64().unwrap(), r.get("last_uid").unwrap().as_u64().unwrap());
    assert_eq!(last - first, 39);
    let spots: Vec<[f64; 3]> = (first..=last).map(|u| position(&mut e, u)).collect();
    for (i, p) in spots.iter().enumerate() {
        let h = e.call("terrain_height", &j(&format!(r#"{{"x":{},"z":{}}}"#, p[0], p[2]))).unwrap().get("height").unwrap().as_f64().unwrap();
        assert!((h - p[1]).abs() < 0.01, "copy {i} stands on the ground: {p:?} over {h}");
        for q in &spots[..i] {
            assert!(((p[0] - q[0]).powi(2) + (p[2] - q[2]).powi(2)).sqrt() >= 2.99, "{p:?} and {q:?} are too close");
        }
    }
    // the same seed on the same ground gives the same forest
    let (mut again, _) = go();
    let spots2: Vec<[f64; 3]> = (first..=last).map(|u| position(&mut again, u)).collect();
    assert_eq!(spots, spots2);
    // one undo takes them all away
    e.call("undo", &Json::Null).unwrap();
    assert!(e.call("get", &j(&format!(r#"{{"uid":{first}}}"#))).is_err());
}

#[test]
fn scatter_keeps_to_the_slope_and_height_asked_for_and_says_when_nothing_fits() {
    let mut e = engine();
    ramp_world(&mut e, "scatter_ramp2.png");
    // the ramp leans about 9 degrees: nothing fits between 30 and 90
    let none = e.call("scatter", &j(r#"{"count":5,"slope":[30,90],"components":{"Name":{"value":"x"}}}"#)).unwrap_err();
    assert_eq!(none.code, "no_room");
    // only the high east side
    let r = e.call("scatter", &j(r#"{"count":30,"height":[8,10],"area":[-40,-40,40,40],"components":{"Name":{"value":"x"}}}"#)).unwrap();
    for u in r.get("first_uid").unwrap().as_u64().unwrap()..=r.get("last_uid").unwrap().as_u64().unwrap() {
        let p = position(&mut e, u);
        assert!(p[1] >= 7.99 && p[0] > 10.0, "{p:?}");
    }
    // more than fit: a note, not a failure
    let crowded = e.call("scatter", &j(r#"{"count":500,"min_distance":20,"components":{"Name":{"value":"y"}}}"#)).unwrap();
    assert!(crowded.get("placed").unwrap().as_u64().unwrap() < 20 && crowded.get("note").is_some(), "{crowded:?}");
    // arguments are checked
    for bad in [r#"{"count":5}"#, r#"{"count":5,"components":{},"prefab":"p"}"#, r#"{"count":0,"components":{}}"#, r#"{"count":5,"components":{},"slope":[10,5]}"#, r#"{"count":5,"prefab":"nope"}"#, r#"{"count":5,"components":{},"area":[1,2]}"#, r#"{"count":5,"components":{},"terrain":99}"#] {
        assert!(e.call("scatter", &j(bad)).is_err(), "{bad}");
    }
}

#[test]
fn scatter_places_prefabs_with_their_children_and_can_tilt_them_to_the_slope() {
    let mut e = engine();
    ramp_world(&mut e, "scatter_ramp3.png");
    e.call("define_prefab", &j(r#"{"name":"Pine","components":{"Name":{"value":"Pine"},"Transform":{},"MeshRenderer":{"mesh":"cylinder"}},"children":[{"name":"Top","components":{"LocalTransform":{"pos":[0,2,0]},"MeshRenderer":{"mesh":"sphere"}}}]}"#)).unwrap();
    let r = e.call("scatter", &j(r#"{"count":6,"prefab":"Pine","align":true,"yaw":false,"seed":2}"#)).unwrap();
    assert_eq!(r.get("placed").unwrap().as_u64(), Some(6));
    assert_eq!(r.get("entities").unwrap().as_u64(), Some(12), "each pine is a trunk and a top: {r:?}");
    let first = r.get("first_uid").unwrap().as_u64().unwrap();
    let t = e.call("get", &j(&format!(r#"{{"uid":{first},"components":["Transform"]}}"#))).unwrap();
    let rot = t.get("components").unwrap().get("Transform").unwrap().get("rot").unwrap();
    // tilted away from the vertical: leaning towards -x (the ground rises towards +x), so rotation about z and none about x
    let (rx, rz, rw) = (rot.index(0).unwrap().as_f64().unwrap(), rot.index(2).unwrap().as_f64().unwrap(), rot.index(3).unwrap().as_f64().unwrap());
    assert!(rx.abs() < 1e-3 && rz.abs() > 0.01 && rw > 0.99, "{rot:?}");
}

#[test]
fn the_minimap_shows_the_relief_of_a_terrain_and_things_standing_on_it() {
    let mut e = engine();
    ramp_world(&mut e, "minimap_ramp.png");
    e.call("spawn", &j(r#"{"components":{"Name":{"value":"Tree"},"Transform":{"pos":[-24,1,0]}}}"#)).unwrap();
    // from above: low ground to the west (a), high ground to the east (j), no letter for the terrain entity itself, the tree stays visible
    let top = e.call("minimap", &j(r#"{"center":[0,0,0],"radius":32,"cells":9}"#)).unwrap();
    let rows: Vec<String> = top.get("rows").unwrap().as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_string()).collect();
    assert!(rows.iter().all(|r| r.chars().count() == 9), "{rows:?}");
    assert_eq!(rows[8].chars().next(), Some('a'), "{rows:?}");
    assert_eq!(rows[8].chars().last(), Some('j'), "{rows:?}");
    assert!(rows[4].contains('T') && !rows.iter().any(|r| r.contains('H')), "{rows:?}");
    assert!(top.get("relief").unwrap().get("high").unwrap().as_f64().unwrap() > 9.0, "{top:?}");
    // from the side: ground (#) under the line of sight, low on the west and high on the east
    let side = e.call("minimap", &j(r#"{"center":[0,5,0],"radius":12,"cells":9,"view":"side"}"#)).unwrap();
    let rows: Vec<String> = side.get("rows").unwrap().as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_string()).collect();
    assert!(rows[8].contains('#') && rows[0].chars().all(|c| c != '#'), "{rows:?}");
}

#[test]
fn check_warns_about_joints_that_do_nothing() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{},"Joint":{"kind":1,"length":2}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Transform":{},"Velocity":{},"Collider":{"shape":0},"RigidBody":{"mass":1},"Joint":{"target":77,"length":2}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Transform":{},"Velocity":{},"Collider":{"shape":0},"RigidBody":{"mass":1},"Joint":{"target":2,"length":2}}}"#)).unwrap();
    let p = problems(&mut e);
    assert!(p.iter().any(|m| m.contains("not a dynamic body")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("target 77")), "{p:?}");
    assert_eq!(p.iter().filter(|m| m.contains("Joint")).count(), 2, "the third joint is fine: {p:?}");
}

#[test]
fn check_reports_broken_gameplay_text_and_paths() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"InputMap":{"bindings":"jump ="}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Transform":{},"StateMachine":{"machine":"state a\nstate b\na -> b when Nope.x > 1"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Transform":{},"Tween":{"path":"Transform.pos[9]"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":4,"components":{"Transform":{},"Sensor":{"filter":"Nothing"},"CameraRig":{"target":99}}}"#)).unwrap();
    let p = problems(&mut e);
    for word in ["InputMap.bindings", "no component `Nope`", "out of range", "Sensor.filter", "CameraRig's target"] {
        assert!(p.iter().any(|m| m.contains(word)), "{word}: {p:?}");
    }
    // a good one is quiet
    let mut ok = engine();
    ok.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{},"Velocity":{},"StateMachine":{"machine":"state a\nstate b\na -> b when Velocity.lin[0] > 1 and $x == 0\nenter b: set Velocity.lin[0] = 0"}}}"#)).unwrap();
    assert!(problems(&mut ok).iter().all(|m| !m.contains("StateMachine")), "{:?}", problems(&mut ok));
}
