//! Mesh colliders and the picture: the triangles physics collides with are the triangles the renderer draws, and a ray from physics hits
//! exactly what a ray at the model hits. (The simulation crate has its own small model reader, which cannot depend on the renderer's
//! importer; these tests are what keeps the two from drifting apart.)
use crate::query::{raycast_with, RayTargets};
use crate::texture::TextureCache;
use lunaris_json::parse;
use lunaris_sim::{Command, Sim};
use std::path::{Path, PathBuf};

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn models_in(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            models_in(&p, out);
        } else if p.extension().is_some_and(|e| e == "glb" || e == "gltf") {
            out.push(p);
        }
    }
}

#[test]
fn physics_and_the_renderer_read_the_same_triangles_from_every_model_of_the_examples() {
    let mut files = Vec::new();
    models_in(&examples(), &mut files);
    assert!(files.len() >= 3, "the example models are missing: {files:?}");
    for path in files {
        let bytes = std::fs::read(&path).unwrap();
        let drawn = crate::gltf::import(&bytes, path.parent());
        let seen = lunaris_sim::meshcol_gltf::parse_model(&bytes, path.parent());
        let (drawn, seen) = match (drawn, seen) {
            (Ok(d), Ok(s)) => (d, s),
            (Err(a), Err(b)) => {
                assert_eq!(a.contains("no triangles"), b.contains("no triangles"), "{}: {a} / {b}", path.display());
                continue;
            }
            (a, b) => panic!("{}: the renderer says {:?}, physics says {:?}", path.display(), a.err(), b.err()),
        };
        let mesh = &drawn.mesh;
        assert_eq!(mesh.idx.len() / 3, seen.tris.len(), "{}: the number of triangles", path.display());
        if mesh.rig.is_some() {
            // (a model with a skeleton is drawn in the rest pose of its skeleton; physics keeps the stored positions)
            assert!(seen.skinned, "{}", path.display());
            continue;
        }
        for (k, t) in mesh.idx.chunks_exact(3).enumerate() {
            let want = [mesh.pos[t[0] as usize], mesh.pos[t[1] as usize], mesh.pos[t[2] as usize]];
            let s = seen.tris[k];
            let got = [seen.verts[s[0] as usize], seen.verts[s[1] as usize], seen.verts[s[2] as usize]];
            assert_eq!(got, want, "{}: triangle {k} is not the one that is drawn", path.display());
        }
    }
}

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let lunaris_json::Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

#[test]
fn a_ray_at_a_mesh_collider_hits_what_a_ray_at_the_visible_model_hits() {
    // the same model, the same turned and stretched placement: once to look at (uid 1), once to collide with (uid 2)
    let q = [0.0, 0.3826834, 0.0, 0.9238795];
    let place = format!(r#""Transform":{{"pos":[1.5,0.5,-2],"rot":{q:?},"scale":[2.0,1.5,2.5]}}"#);
    let mut visual = Sim::new(1);
    spawn(&mut visual, 1, &format!(r#"{{{place},"MeshRenderer":{{"mesh":"models/crate.glb"}}}}"#));
    let mut solid = Sim::new(1);
    spawn(&mut solid, 2, &format!(r#"{{{place},"MeshCollider":{{"mesh":"models/crate.glb"}}}}"#));
    let mut assets = TextureCache::new(Some(examples()));
    let mut hits = 0;
    for ix in -8..=8 {
        for iy in -6..=6 {
            let origin = [ix as f32 * 0.5, iy as f32 * 0.5 + 0.1, 12.0];
            let target = [1.5, 0.5, -2.0];
            let dir = [target[0] - origin[0], target[1] - origin[1], target[2] - origin[2]];
            let a = raycast_with(&mut visual.world, &mut assets, origin, dir, 100.0, RayTargets::Visuals).unwrap();
            let b = raycast_with(&mut solid.world, &mut assets, origin, dir, 100.0, RayTargets::Colliders).unwrap();
            match (a, b) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    assert_eq!((a.distance, a.point, a.normal), (b.distance, b.point, b.normal), "ray from {origin:?}");
                    assert_eq!((a.uid, b.uid), (1, 2));
                    hits += 1;
                }
                (a, b) => panic!("ray from {origin:?}: the model gives {a:?}, the collider {b:?}"),
            }
        }
    }
    assert!(hits > 30, "the rays should hit the model often enough to mean something: {hits}");
    // each kind of ray sees only its own: the picture's rays do not see a collider, and the collider's do not see a picture
    let o = [1.5, 0.5, 12.0];
    assert!(raycast_with(&mut solid.world, &mut assets, o, [0.0, 0.0, -1.0], 100.0, RayTargets::Visuals).unwrap().is_none());
    assert!(raycast_with(&mut visual.world, &mut assets, o, [0.0, 0.0, -1.0], 100.0, RayTargets::Colliders).unwrap().is_none());
    assert!(raycast_with(&mut solid.world, &mut assets, o, [0.0, 0.0, -1.0], 100.0, RayTargets::Both).unwrap().is_some());
}

#[test]
fn a_ray_at_a_convex_collider_hits_the_hull_of_the_model_not_its_dents() {
    // crate.glb is a 1 m cube with a pyramid on top (tip at y = 1.2): the hull is the cube with a roof of four slopes up to that tip
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"ConvexCollider":{"mesh":"models/crate.glb"}}"#);
    let mut assets = TextureCache::new(Some(examples()));
    let down = |s: &mut Sim, assets: &mut TextureCache, x: f32| raycast_with(&mut s.world, assets, [x, 5.0, 0.0], [0.0, -1.0, 0.0], 100.0, RayTargets::Colliders).unwrap();
    // beside the tip, over the slope that runs from the tip (0, 1.2) to the cube's top edge (0.5, 0.5)
    let h = down(&mut s, &mut assets, 0.1).expect("the roof");
    assert!((h.point[1] - 1.06).abs() < 1e-3 && h.uid == 1 && h.normal[1] > 0.5 && h.normal[0] > 0.3, "{h:?}");
    // over the edge of the cube, out of the roof's reach: the cube's top face at 0.5 is inside the hull's slope, so the slope is hit lower down
    let side = raycast_with(&mut s.world, &mut assets, [-3.0, 0.0, 0.0], [1.0, 0.0, 0.0], 100.0, RayTargets::Colliders).unwrap().expect("the side");
    assert!((side.distance - 2.5).abs() < 1e-3 && side.normal[0] < -0.99, "{side:?}");
    assert!(down(&mut s, &mut assets, 0.8).is_none());
    // a visual ray does not see it, and a scale and a turn move it
    assert!(raycast_with(&mut s.world, &mut assets, [0.1, 5.0, 0.0], [0.0, -1.0, 0.0], 100.0, RayTargets::Visuals).unwrap().is_none());
    s.world.get_mut::<lunaris_sim::components::Transform>(s.world.entity(1).unwrap()).unwrap().scale = [2.0, 1.0, 1.0];
    let wide = raycast_with(&mut s.world, &mut assets, [-3.0, 0.0, 0.0], [1.0, 0.0, 0.0], 100.0, RayTargets::Colliders).unwrap().expect("wider");
    assert!((wide.distance - 2.0).abs() < 1e-3, "{wide:?}");
    // it agrees with the simulation's own copy
    let mut t = Sim::new(1);
    t.asset_root = Some(examples());
    spawn(&mut t, 1, r#"{"Transform":{"pos":[0,0,0]},"ConvexCollider":{"mesh":"models/crate.glb"}}"#);
    t.step();
    let mut shared = TextureCache::new(None);
    shared.share_collision_meshes(t.meshes.clone());
    let a = raycast_with(&mut t.world, &mut shared, [0.1, 5.0, 0.0], [0.0, -1.0, 0.0], 100.0, RayTargets::Colliders).unwrap().expect("shared");
    assert_eq!((a.distance, a.normal), (h.distance, h.normal));
}

#[test]
fn a_collider_that_is_not_solid_and_a_model_that_cannot_be_read_are_not_hit() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{},"MeshCollider":{"mesh":"models/crate.glb","solid":false}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[5,0,0]},"MeshCollider":{"mesh":"models/nothing.glb"}}"#);
    let mut assets = TextureCache::new(Some(examples()));
    assert!(raycast_with(&mut s.world, &mut assets, [0.0, 0.0, 5.0], [0.0, 0.0, -1.0], 100.0, RayTargets::Both).unwrap().is_none());
    assert!(raycast_with(&mut s.world, &mut assets, [5.0, 0.0, 5.0], [0.0, 0.0, -1.0], 100.0, RayTargets::Both).unwrap().is_none());
    assert!(assets.warnings.iter().any(|w| w.contains("nothing.glb")), "{:?}", assets.warnings);
    // the simulation's own copy is used when it is offered
    let mut t = Sim::new(1);
    t.asset_root = Some(examples());
    spawn(&mut t, 1, r#"{"Transform":{},"MeshCollider":{"mesh":"models/crate.glb"}}"#);
    t.step();
    let mut shared = TextureCache::new(None);
    shared.share_collision_meshes(t.meshes.clone());
    let h = raycast_with(&mut t.world, &mut shared, [0.0, 0.0, 5.0], [0.0, 0.0, -1.0], 100.0, RayTargets::Colliders).unwrap().expect("through the shared copy");
    assert_eq!(h.uid, 1);
    assert!(shared.warnings.is_empty());
}
