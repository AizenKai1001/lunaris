//! Mesh colliders as an agent meets them: `check` says what is wrong with one, `raycast` and `path` see the level, `step` lets things land on
//! it, and a snapshot brings the same future back.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

/// A .glb with the given vertex positions and, when asked, the given triangle indices (otherwise every three vertices are a triangle).
fn glb(positions: &[[f32; 3]], indices: Option<&[u32]>) -> Vec<u8> {
    let mut bin: Vec<u8> = positions.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    let pos_len = bin.len();
    let mut attrs = String::from(r#""attributes":{"POSITION":0}"#);
    let mut accessors = format!(r#"{{"bufferView":0,"componentType":5126,"count":{},"type":"VEC3"}}"#, positions.len());
    let mut views = format!(r#"{{"buffer":0,"byteOffset":0,"byteLength":{pos_len}}}"#);
    if let Some(idx) = indices {
        bin.extend(idx.iter().flat_map(|i| i.to_le_bytes()));
        attrs.push_str(r#","indices":1"#);
        accessors.push_str(&format!(r#",{{"bufferView":1,"componentType":5125,"count":{},"type":"SCALAR"}}"#, idx.len()));
        views.push_str(&format!(r#",{{"buffer":0,"byteOffset":{pos_len},"byteLength":{}}}"#, idx.len() * 4));
    }
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],"meshes":[{{"primitives":[{{{attrs}}}]}}],"buffers":[{{"byteLength":{}}}],"bufferViews":[{views}],"accessors":[{accessors}]}}"#,
        bin.len()
    );
    let mut json_bytes = json.into_bytes();
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let mut out = Vec::new();
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend(((12 + 8 + json_bytes.len() + 8 + bin.len()) as u32).to_le_bytes());
    out.extend((json_bytes.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(&json_bytes);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(b"BIN\0");
    out.extend(&bin);
    out
}

/// A floor (facing up) of 40 x 40 m at height 0 and a ramp up at 20 degrees: 4 m along x from x = 5, then a platform 1.46 m high.
fn level() -> Vec<u8> {
    let rise = 4.0 * 20.0f32.to_radians().tan();
    let mut v: Vec<[f32; 3]> = Vec::new();
    let mut quad = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]| v.extend([a, b, c, a, c, d]);
    quad([-20.0, 0.0, -20.0], [-20.0, 0.0, 20.0], [20.0, 0.0, 20.0], [20.0, 0.0, -20.0]);
    quad([5.0, 0.0, -3.0], [5.0, 0.0, 3.0], [9.0, rise, 3.0], [9.0, rise, -3.0]);
    quad([9.0, rise, -3.0], [9.0, rise, 3.0], [14.0, rise, 3.0], [14.0, rise, -3.0]);
    glb(&v, None)
}

fn problems(e: &mut crate::Engine) -> Vec<String> {
    let c = e.call("check", &Json::Null).unwrap();
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| format!("{}: {}", p.get("severity").unwrap().as_str().unwrap(), p.get("message").unwrap().as_str().unwrap())).collect()
}

fn y_of(e: &mut crate::Engine, uid: u64) -> f64 {
    let r = e.call("get", &j(&format!(r#"{{"uid":{uid},"components":["Transform"]}}"#))).unwrap();
    r.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().index(1).unwrap().as_f64().unwrap()
}

#[test]
fn check_says_what_is_wrong_with_a_mesh_collider() {
    let mut e = engine();
    let dir = e.root.clone();
    std::fs::write(dir.join("mesh_ok.glb"), level()).unwrap();
    // a triangle without any area
    std::fs::write(dir.join("mesh_flat.glb"), glb(&[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]], None)).unwrap();
    // more triangles than the limit: three vertices, three million and three indices that name them again and again
    let idx: Vec<u32> = (0..3 * (lunaris_sim::meshcol::MAX_TRIANGLES as u32 + 1)).map(|i| i % 3).collect();
    std::fs::write(dir.join("mesh_huge.glb"), glb(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], Some(&idx))).unwrap();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{},"MeshCollider":{"mesh":"mesh_ok.glb"}}}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("MeshCollider") && !m.contains("mesh_ok")), "a good level is clean: {:?}", problems(&mut e));
    for (uid, comps) in [
        (2, r#"{"Transform":{},"MeshCollider":{}}"#),
        (3, r#"{"Transform":{},"MeshCollider":{"mesh":"mesh_missing.glb"}}"#),
        (4, r#"{"Transform":{},"MeshCollider":{"mesh":"mesh_flat.glb"}}"#),
        (5, r#"{"Transform":{},"MeshCollider":{"mesh":"mesh_huge.glb"}}"#),
        (6, r#"{"Transform":{},"MeshCollider":{"mesh":"mesh_ok.glb"},"RigidBody":{"mass":2}}"#),
        (7, r#"{"MeshCollider":{"mesh":"mesh_ok.glb"}}"#),
        (8, r#"{"Transform":{},"MeshCollider":{"mesh":"cube"}}"#),
    ] {
        e.call("spawn", &j(&format!(r#"{{"uid":{uid},"components":{comps}}}"#))).unwrap();
    }
    let p = problems(&mut e);
    let has = |needles: &[&str]| p.iter().any(|m| m.starts_with("warning") && needles.iter().all(|n| m.contains(n)));
    assert!(has(&["MeshCollider.mesh is empty"]), "no model: {p:?}");
    assert!(has(&["mesh_missing.glb", "could not be used", "cannot read"]), "missing file: {p:?}");
    assert!(has(&["mesh_flat.glb", "could not be used", "no usable triangles"]), "no triangles: {p:?}");
    assert!(has(&["mesh_huge.glb", "could not be used", "at most 1000000"]), "huge, with the limit: {p:?}");
    assert!(has(&["RigidBody", "static level geometry"]), "a body: {p:?}");
    assert!(has(&["no Transform"]), "no place: {p:?}");
    assert!(has(&["`cube`", "not a model file"]), "a built-in shape is not a file: {p:?}");
    // the huge one was refused (nothing of it is kept), the good one is used
    assert!(e.sim.meshes.by_name.contains_key("mesh_ok.glb") && !e.sim.meshes.by_name.contains_key("mesh_huge.glb"));
}

#[test]
fn a_level_is_hit_by_rays_walked_over_by_paths_and_landed_on() {
    let mut e = engine();
    std::fs::write(e.root.join("mesh_level_a.glb"), level()).unwrap();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Name":{"value":"Level"},"Transform":{},"MeshCollider":{"mesh":"mesh_level_a.glb","friction":0.7,"restitution":0}}}"#)).unwrap();
    // a ray down onto the floor and onto the platform, and one that misses everything
    let hit = e.call("raycast", &j(r#"{"origin":[0,10,0],"direction":[0,-1,0]}"#)).unwrap();
    assert_eq!(hit.get("uid").unwrap().as_u64(), Some(1), "{hit:?}");
    assert!((hit.get("distance").unwrap().as_f64().unwrap() - 10.0).abs() < 1e-3 && hit.get("name").unwrap().as_str() == Some("Level"), "{hit:?}");
    let rise = 4.0 * 20.0f64.to_radians().tan();
    let top = e.call("raycast", &j(r#"{"origin":[12,10,0],"direction":[0,-1,0]}"#)).unwrap();
    assert!((top.get("point").unwrap().index(1).unwrap().as_f64().unwrap() - rise).abs() < 1e-3, "{top:?}");
    assert!(e.call("raycast", &j(r#"{"origin":[100,10,0],"direction":[0,-1,0]}"#)).unwrap().get("hit").unwrap().as_bool() == Some(false));
    // `include: visuals` does not see a collider
    assert_eq!(e.call("raycast", &j(r#"{"origin":[0,10,0],"direction":[0,-1,0],"include":"visuals"}"#)).unwrap().get("hit").unwrap().as_bool(), Some(false));
    // the way up the ramp to the platform
    let p = e.call("path", &j(&format!(r#"{{"from":[0,0,0],"to":[12,{rise},0]}}"#))).unwrap();
    assert!(p.get("exact").unwrap().as_bool() == Some(true) && p.get("length").unwrap().as_f64().unwrap() < 12.6, "{p:?}");
    // a ball dropped on the platform and one on the floor come to rest on them
    for (uid, x) in [(2, 12.0), (3, -5.0)] {
        e.call("spawn", &j(&format!(r#"{{"uid":{uid},"components":{{"Transform":{{"pos":[{x},4,0]}},"Velocity":{{}},"AngularVelocity":{{}},"Collider":{{"shape":0,"size":[0.5,0.5,0.5]}},"RigidBody":{{"mass":1,"restitution":0}}}}}}"#))).unwrap();
    }
    e.call("step", &j(r#"{"ticks":240}"#)).unwrap();
    assert!((y_of(&mut e, 2) - (rise + 0.25)).abs() < 0.02, "on the platform: {}", y_of(&mut e, 2));
    assert!((y_of(&mut e, 3) - 0.25).abs() < 0.02, "on the floor: {}", y_of(&mut e, 3));
    // the contacts tool names the level
    let c = e.call("contacts", &Json::Null).unwrap();
    assert!(c.get("contacts").unwrap().as_array().unwrap().iter().any(|k| k.get("a").and_then(|v| v.as_u64()) == Some(1) && k.get("a_name").and_then(|v| v.as_str()) == Some("Level")), "{c:?}");
}

/// The 12 triangles of a cube (half size `h`), as a plain list of vertices.
fn cube_glb(h: f32, centre: [f32; 3]) -> Vec<u8> {
    let c = |x: f32, y: f32, z: f32| [centre[0] + x * h, centre[1] + y * h, centre[2] + z * h];
    let corner = |i: usize| c(if i & 1 == 0 { -1.0 } else { 1.0 }, if i & 2 == 0 { -1.0 } else { 1.0 }, if i & 4 == 0 { -1.0 } else { 1.0 });
    let quads = [[1, 3, 7, 5], [0, 4, 6, 2], [2, 6, 7, 3], [0, 1, 5, 4], [4, 5, 7, 6], [0, 2, 3, 1]];
    let mut v = Vec::new();
    for q in quads {
        v.extend([corner(q[0]), corner(q[1]), corner(q[2]), corner(q[0]), corner(q[2]), corner(q[3])]);
    }
    glb(&v, None)
}

#[test]
fn check_says_what_is_wrong_with_a_convex_collider() {
    let mut e = engine();
    let dir = e.root.clone();
    std::fs::write(dir.join("hull_cube.glb"), cube_glb(0.5, [0.0; 3])).unwrap();
    // a flat model (a square): no volume
    std::fs::write(dir.join("hull_flat.glb"), glb(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]], None)).unwrap();
    // a ball of 300 corners: more than a hull keeps
    let n = 300usize;
    let golden = std::f64::consts::PI * (3.0 - 5.0f64.sqrt());
    let pts: Vec<[f32; 3]> = (0..n)
        .map(|i| {
            let y = 1.0 - 2.0 * (i as f64 + 0.5) / n as f64;
            let r = (1.0 - y * y).sqrt();
            [(r * (golden * i as f64).cos()) as f32, y as f32, (r * (golden * i as f64).sin()) as f32]
        })
        .collect();
    let fan: Vec<u32> = (1..n as u32 - 1).flat_map(|i| [0, i, i + 1]).collect();
    std::fs::write(dir.join("hull_ball.glb"), glb(&pts, Some(&fan))).unwrap();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{},"ConvexCollider":{"mesh":"hull_cube.glb"},"RigidBody":{"mass":1},"Velocity":{}}}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("ConvexCollider") && !m.contains("hull_cube")), "a good hull is clean: {:?}", problems(&mut e));
    for (uid, comps) in [
        (2, r#"{"Transform":{},"ConvexCollider":{}}"#),
        (3, r#"{"Transform":{},"ConvexCollider":{"mesh":"hull_missing.glb"}}"#),
        (4, r#"{"Transform":{},"ConvexCollider":{"mesh":"hull_flat.glb"}}"#),
        (5, r#"{"Transform":{},"ConvexCollider":{"mesh":"hull_ball.glb"}}"#),
        (6, r#"{"Transform":{},"ConvexCollider":{"mesh":"hull_cube.glb"},"Collider":{"shape":0}}"#),
        (7, r#"{"ConvexCollider":{"mesh":"hull_cube.glb"}}"#),
        (8, r#"{"Transform":{"scale":[1,0,1]},"ConvexCollider":{"mesh":"hull_cube.glb"}}"#),
    ] {
        e.call("spawn", &j(&format!(r#"{{"uid":{uid},"components":{comps}}}"#))).unwrap();
    }
    let p = problems(&mut e);
    let has = |needles: &[&str]| p.iter().any(|m| m.starts_with("warning") && needles.iter().all(|n| m.contains(n)));
    assert!(has(&["ConvexCollider.mesh is empty"]), "no model: {p:?}");
    assert!(has(&["hull_missing.glb", "could not be used", "cannot read"]), "missing file: {p:?}");
    assert!(has(&["hull_flat.glb", "no convex solid", "plane"]), "flat model: {p:?}");
    assert!(has(&["hull_ball.glb", "300 different corners", "at most 64"]), "simplified, with the limit: {p:?}");
    assert!(has(&["both a Collider and a ConvexCollider"]), "two shapes: {p:?}");
    assert!(has(&["ConvexCollider but no Transform"]), "no place: {p:?}");
    assert!(p.iter().filter(|m| m.contains("no convex solid")).count() >= 2, "a cube squashed flat by the scale has no volume either: {p:?}");
}

#[test]
fn a_hull_falls_onto_a_level_is_hit_by_rays_and_stops_sensors_seeing_through_it() {
    let mut e = engine();
    std::fs::write(e.root.join("hull_level.glb"), level()).unwrap();
    std::fs::write(e.root.join("hull_crate.glb"), cube_glb(0.5, [0.0; 3])).unwrap();
    std::fs::write(e.root.join("hull_offcentre.glb"), cube_glb(0.5, [3.0, 0.0, 0.0])).unwrap();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{},"MeshCollider":{"mesh":"hull_level.glb"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Name":{"value":"Crate"},"Transform":{"pos":[-6,3,0],"rot":[0,0.3826834,0,0.9238795]},"Velocity":{},"AngularVelocity":{},"ConvexCollider":{"mesh":"hull_crate.glb"},"RigidBody":{"mass":1,"restitution":0}}}"#)).unwrap();
    // a crate whose model is 3 m east of its origin: the entity's origin stays put while it falls
    e.call("spawn", &j(r#"{"uid":3,"components":{"Transform":{"pos":[-12,3,5]},"Velocity":{},"AngularVelocity":{},"ConvexCollider":{"mesh":"hull_offcentre.glb"},"RigidBody":{"mass":1,"restitution":0}}}"#)).unwrap();
    // a ray at the crate (before it moves): the nearest face of the cube turned 45 degrees about y
    let hit = e.call("raycast", &j(r#"{"origin":[-6,3,10],"direction":[0,0,-1]}"#)).unwrap();
    assert_eq!(hit.get("uid").unwrap().as_u64(), Some(2), "{hit:?}");
    assert_eq!(hit.get("name").unwrap().as_str(), Some("Crate"));
    assert!((hit.get("distance").unwrap().as_f64().unwrap() - (10.0 - std::f64::consts::FRAC_1_SQRT_2)).abs() < 1e-3, "{hit:?}");
    // the crate whose model is off to the side: a ray at where its cube is hits it, one at its origin misses
    assert_eq!(e.call("raycast", &j(r#"{"origin":[-9,3,10],"direction":[0,0,-1]}"#)).unwrap().get("uid").unwrap().as_u64(), Some(3));
    assert_eq!(e.call("raycast", &j(r#"{"origin":[-12,3,10],"direction":[0,0,-1]}"#)).unwrap().get("hit").unwrap().as_bool(), Some(false));
    e.call("step", &j(r#"{"ticks":300}"#)).unwrap();
    let y = |e: &mut crate::Engine, uid: u64| y_of(e, uid);
    assert!((y(&mut e, 2) - 0.5).abs() < 0.02, "the crate lies on the floor: {}", y(&mut e, 2));
    assert!((y(&mut e, 3) - 0.5).abs() < 0.02, "so does the other: {}", y(&mut e, 3));
    let p = e.call("get", &j(r#"{"uid":3,"components":["Transform"]}"#)).unwrap();
    let pos = p.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap();
    assert!((pos.index(0).unwrap().as_f64().unwrap() + 12.0).abs() < 0.05 && (pos.index(2).unwrap().as_f64().unwrap() - 5.0).abs() < 0.05, "it fell straight down: {p:?}");
}

#[test]
fn a_snapshot_brings_the_same_future_back_on_a_level() {
    let mut e = engine();
    std::fs::write(e.root.join("mesh_level_b.glb"), level()).unwrap();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{},"MeshCollider":{"mesh":"mesh_level_b.glb"}}}"#)).unwrap();
    for i in 0..6 {
        e.call("spawn", &j(&format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},{},0.3]}},"Velocity":{{"lin":[1,0,0]}},"AngularVelocity":{{}},"Collider":{{"shape":{},"size":[0.5,0.5,0.5]}},"RigidBody":{{"mass":1}}}}}}"#, 10 + i, 3 + i, 2 + i, i % 2))).unwrap();
    }
    e.call("step", &j(r#"{"ticks":50}"#)).unwrap();
    e.call("snapshot", &j(r#"{"name":"s"}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":200}"#)).unwrap();
    let a = e.call("hash", &Json::Null).unwrap();
    e.call("restore", &j(r#"{"name":"s"}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":200}"#)).unwrap();
    let b = e.call("hash", &Json::Null).unwrap();
    assert_eq!(a.get("hash"), b.get("hash"));
    assert!(a.get("hash").is_some());
}
