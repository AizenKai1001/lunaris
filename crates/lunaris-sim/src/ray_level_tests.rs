//! A grid over the mesh levels (a streamed city has a few hundred shells) changes nothing about what a ray finds: the same nearest hit, bit for bit, as asking every level, and the same answer to `occluded`.
use super::*;
use crate::vehicle_ray::RayWorld;
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A closed box of the given half sizes with its base on the ground: 8 corners, 12 triangles.
fn box_mesh(hx: f32, h: f32, hz: f32) -> crate::meshcol::CollisionMesh {
    let v = vec![[-hx, 0.0, -hz], [hx, 0.0, -hz], [hx, 0.0, hz], [-hx, 0.0, hz], [-hx, h, -hz], [hx, h, -hz], [hx, h, hz], [-hx, h, hz]];
    let t: Vec<[u32; 3]> = vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
    crate::meshcol::CollisionMesh::new(v, t).unwrap()
}

#[test]
fn a_grid_over_the_mesh_levels_finds_what_asking_every_level_finds() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[1400,1,1400]},"Collider":{"shape":1}}"#);
    std::sync::Arc::make_mut(&mut s.meshes).by_name.insert("small".to_string(), std::sync::Arc::new(box_mesh(6.0, 9.0, 8.0)));
    std::sync::Arc::make_mut(&mut s.meshes).by_name.insert("tall".to_string(), std::sync::Arc::new(box_mesh(10.0, 40.0, 12.0)));
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32
    };
    for i in 0..160u64 {
        let (x, z) = (next() * 1200.0 - 600.0, next() * 1200.0 - 600.0);
        let mesh = if i % 4 == 0 { "tall" } else { "small" };
        spawn(&mut s, 100 + i, &format!(r#"{{"Transform":{{"pos":[{x},0,{z}],"rot":[0,{},0,{}]}},"MeshCollider":{{"mesh":"{mesh}"}}}}"#, (i as f32 * 0.37).sin() * 0.3, (1.0 - (i as f32 * 0.37).sin().powi(2) * 0.09).sqrt()));
    }
    let with = RayWorld::gather(&mut s.world, &s.heightfields, &s.meshes, None);
    assert_eq!(with.levels(), (160, true), "160 levels are enough for a grid");
    let without = RayWorld::gather(&mut s.world, &s.heightfields, &s.meshes, None).without_level_grid();
    assert_eq!(without.levels(), (160, false));
    let (mut hits, mut misses) = (0, 0);
    for k in 0..4000 {
        let o = [next() * 1300.0 - 650.0, next() * 60.0 + 0.5, next() * 1300.0 - 650.0];
        let mut d = match k % 6 {
            0 => [0.0, -1.0, 0.0],
            1 => [0.0, 1.0, 0.0],
            _ => [next() * 2.0 - 1.0, next() * 0.6 - 0.3, next() * 2.0 - 1.0],
        };
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1.0e-6);
        d = [d[0] / l, d[1] / l, d[2] / l];
        let max = [4.0, 20.0, 80.0, 300.0, 900.0][(k / 6) % 5];
        let a = with.cast(o, d, max, Default::default(), u64::MAX);
        let b = without.cast(o, d, max, Default::default(), u64::MAX);
        assert_eq!(a.map(|h| (h.t.to_bits(), h.uid)), b.map(|h| (h.t.to_bits(), h.uid)), "ray {k}: from {o:?} along {d:?} for {max}");
        assert_eq!(with.occluded(o, d, max, Default::default()), without.occluded(o, d, max, Default::default()), "occluded, ray {k}");
        if a.is_some_and(|h| h.uid >= 100) {
            hits += 1;
        } else {
            misses += 1;
        }
    }
    assert!(hits > 100 && misses > 100, "the rays hit levels {hits} times and not {misses} times");
}

#[test]
fn few_levels_get_no_grid() {
    let mut s = Sim::new(1);
    std::sync::Arc::make_mut(&mut s.meshes).by_name.insert("small".to_string(), std::sync::Arc::new(box_mesh(6.0, 9.0, 8.0)));
    for i in 0..10u64 {
        spawn(&mut s, 100 + i, &format!(r#"{{"Transform":{{"pos":[{},0,0]}},"MeshCollider":{{"mesh":"small"}}}}"#, i * 30));
    }
    assert_eq!(RayWorld::gather(&mut s.world, &s.heightfields, &s.meshes, None).levels(), (10, false));
}
