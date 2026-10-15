//! Collision layers as an agent meets them: `raycast` takes a mask, `path` plans as the entity it starts from, `check` names the mistakes, a contact
//! system only runs for pairs that collide, and a scene that never mentions layers hashes exactly as it did before they existed.
use crate::tests::engine;
use crate::Engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &j(args)).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn call_err(e: &mut Engine, tool: &str, args: &str) -> lunaris_ecs::EcsError {
    e.call(tool, &j(args)).expect_err(tool)
}

fn spawn(e: &mut Engine, uid: u64, components: &str) {
    call(e, "spawn", &format!(r#"{{"uid":{uid},"components":{components}}}"#));
}

fn hit_uid(r: &Json) -> Option<u64> {
    if r.get("hit").and_then(|h| h.as_bool()) == Some(true) {
        r.get("uid").and_then(|u| u.as_u64())
    } else {
        None
    }
}

fn messages(e: &mut Engine) -> Vec<String> {
    let c = call(e, "check", "{}");
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| format!("{}: {}", p.get("severity").unwrap().as_str().unwrap(), p.get("message").unwrap().as_str().unwrap())).collect()
}

// ------------------------------------------------------------------------------------------------ raycast

/// Three plates along +x as seen from the origin: uid 1 at x = 3 on layer 3, uid 2 at x = 6 on layer 4, uid 3 at x = 9 without a layer (layer 0).
fn plates() -> Engine {
    let mut e = engine();
    spawn(&mut e, 1, r#"{"Transform":{"pos":[3,0,0]},"Collider":{"shape":1,"size":[0.5,4,4]},"CollisionLayer":{"layer":3}}"#);
    spawn(&mut e, 2, r#"{"Transform":{"pos":[6,0,0]},"Collider":{"shape":1,"size":[0.5,4,4]},"CollisionLayer":{"layer":4}}"#);
    spawn(&mut e, 3, r#"{"Transform":{"pos":[9,0,0]},"Collider":{"shape":1,"size":[0.5,4,4]}}"#);
    e
}

#[test]
fn raycast_takes_a_mask_and_sees_through_the_layers_it_leaves_out() {
    let mut e = plates();
    let ray = |e: &mut Engine, extra: &str| hit_uid(&call(e, "raycast", &format!(r#"{{"origin":[0,0,0],"direction":[1,0,0]{extra}}}"#)));
    assert_eq!(ray(&mut e, ""), Some(1), "no mask: the nearest, as always");
    assert_eq!(ray(&mut e, r#","mask":4294967295"#), Some(1));
    assert_eq!(ray(&mut e, r#","mask":16"#), Some(2), "only layer 4: it looks through the plate on layer 3");
    assert_eq!(ray(&mut e, r#","mask":1"#), Some(3), "only layer 0: the plate without a layer");
    assert_eq!(ray(&mut e, r#","mask":2"#), None, "nothing is on layer 1");
    assert_eq!(ray(&mut e, r#","mask":0"#), None, "an empty mask sees nothing");
    assert_eq!(ray(&mut e, r#","mask":17"#), Some(2), "layers 0 and 4: the plate on 4 is nearer than the one on 0");
    // the list form is the same
    assert_eq!(ray(&mut e, r#","layers":[4]"#), Some(2));
    assert_eq!(ray(&mut e, r#","layers":[3,4]"#), Some(1));
    assert_eq!(ray(&mut e, r#","layers":[]"#), None);
    // what it says when asked wrongly
    assert_eq!(call_err(&mut e, "raycast", r#"{"origin":[0,0,0],"direction":[1,0,0],"mask":1,"layers":[1]}"#).code, "bad_arguments");
    assert_eq!(call_err(&mut e, "raycast", r#"{"origin":[0,0,0],"direction":[1,0,0],"layers":[40]}"#).code, "type_mismatch");
    assert_eq!(call_err(&mut e, "raycast", r#"{"origin":[0,0,0],"direction":[1,0,0],"mask":4294967296}"#).code, "type_mismatch");
    assert_eq!(call_err(&mut e, "raycast", r#"{"origin":[0,0,0],"direction":[1,0,0],"mask":-1}"#).code, "type_mismatch");
    assert_eq!(call_err(&mut e, "raycast", r#"{"origin":[0,0,0],"direction":[1,0,0],"layers":"all"}"#).code, "type_mismatch");
}

#[test]
fn raycast_masks_what_is_drawn_as_well() {
    let mut e = engine();
    spawn(&mut e, 1, r#"{"Transform":{"pos":[3,0,0]},"MeshRenderer":{"mesh":"cube"},"CollisionLayer":{"layer":7}}"#);
    spawn(&mut e, 2, r#"{"Transform":{"pos":[6,0,0]},"MeshRenderer":{"mesh":"cube"}}"#);
    let ray = |e: &mut Engine, extra: &str| hit_uid(&call(e, "raycast", &format!(r#"{{"origin":[0,0,0],"direction":[1,0,0],"include":"visuals"{extra}}}"#)));
    assert_eq!(ray(&mut e, ""), Some(1));
    assert_eq!(ray(&mut e, r#","layers":[0]"#), Some(2), "the cube on layer 7 is not asked for");
    assert_eq!(ray(&mut e, r#","layers":[7]"#), Some(1));
}

#[test]
fn raycast_without_a_mask_is_the_old_raycast_in_a_world_that_uses_layers() {
    let mut e = plates();
    let r = call(&mut e, "raycast", r#"{"origin":[0,0,0],"direction":[1,0,0]}"#);
    assert_eq!(hit_uid(&r), Some(1));
    assert!((r.get("distance").unwrap().as_f64().unwrap() - 2.75).abs() < 1e-4, "{r:?}");
}

// ------------------------------------------------------------------------------------------------ contact systems

#[test]
fn a_contact_system_runs_only_for_pairs_that_collide() {
    let mut e = engine();
    call(&mut e, "define_component", r#"{"name":"Player","fields":[]}"#);
    call(&mut e, "define_component", r#"{"name":"Orb","fields":[]}"#);
    call(&mut e, "spawn", r#"{"uid":9,"components":{"PhysicsSettings":{"gravity":[0,0,0]}}}"#);
    spawn(&mut e, 1, r#"{"Transform":{"pos":[0,0,0]},"Player":{},"Velocity":{},"Collider":{"shape":0},"RigidBody":{},"CollisionLayer":{"layer":1,"mask":4}}"#);
    // three orbs, all overlapping the player: one on a layer the player wants, one it does not want, one that does not want the player
    spawn(&mut e, 2, r#"{"Transform":{"pos":[0.3,0,0]},"Orb":{},"Collider":{"shape":0,"trigger":true},"CollisionLayer":{"layer":2,"mask":2}}"#);
    spawn(&mut e, 3, r#"{"Transform":{"pos":[0,0.3,0]},"Orb":{},"Collider":{"shape":0,"trigger":true},"CollisionLayer":{"layer":3,"mask":2}}"#);
    spawn(&mut e, 4, r#"{"Transform":{"pos":[0,0,0.3]},"Orb":{},"Collider":{"shape":0,"trigger":true},"CollisionLayer":{"layer":2,"mask":8}}"#);
    call(&mut e, "define_system", r#"{"name":"collect","on_contact":{"a":["Player"],"b":["Orb"]},"do":[{"res_add":"collected","by":1},{"despawn":"b"}]}"#);
    call(&mut e, "step", r#"{"ticks":3}"#);
    let res = call(&mut e, "resources", "{}");
    let n = res.get("resources").unwrap().get("collected").and_then(|v| v.as_f64()).unwrap_or(0.0);
    assert_eq!(n, 1.0, "only orb 2 (layer 2, wants layer 1... the player wants layer 2) is touched: {n}");
    let left = call(&mut e, "query", r#"{"with":["Orb"]}"#);
    let uids: Vec<u64> = left.get("entities").unwrap().as_array().unwrap().iter().map(|x| x.get("uid").unwrap().as_u64().unwrap()).collect();
    assert_eq!(uids, vec![3, 4], "the other two were never in contact");
    // `contacts` agrees: nothing left
    let c = call(&mut e, "contacts", "{}");
    assert_eq!(c.get("total").unwrap().as_u64(), Some(0), "{c:?}");
}

// ------------------------------------------------------------------------------------------------ path

#[test]
fn path_from_an_entity_plans_on_its_layer() {
    let mut e = engine();
    spawn(&mut e, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1},"CollisionLayer":{"layer":1}}"#);
    spawn(&mut e, 2, r#"{"Transform":{"pos":[5,1.5,0],"scale":[1,3,10]},"Collider":{"shape":1},"CollisionLayer":{"layer":3}}"#);
    spawn(&mut e, 3, r#"{"Transform":{"pos":[0,0,0]},"NavAgent":{},"CharacterBody":{},"CollisionLayer":{"layer":2,"mask":2}}"#);
    let length = |e: &mut Engine, from: &str| call(e, "path", &format!(r#"{{"from":{from},"to":[10,0,0]}}"#)).get("length").unwrap().as_f64().unwrap();
    assert!(length(&mut e, "[0,0,0]") > 14.0, "a bare point counts every layer: round the wall");
    assert!((length(&mut e, "3") - 10.0).abs() < 0.2, "the agent's layer does not meet the wall: straight on");
}

// ------------------------------------------------------------------------------------------------ check

#[test]
fn check_names_the_mistakes_of_collision_layers() {
    let mut e = engine();
    // a clean scene says nothing about layers: ground on layer 1, a ball on layer 2 that wants it and is wanted
    spawn(&mut e, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[20,1,20]},"CollisionLayer":{"layer":1,"mask":4}}"#);
    spawn(&mut e, 2, r#"{"Transform":{"pos":[0,3,0]},"Velocity":{},"Collider":{"shape":0},"RigidBody":{},"CollisionLayer":{"layer":2,"mask":2}}"#);
    let about_layers = |e: &mut Engine| messages(e).into_iter().filter(|m| m.contains("ollision") || m.contains("mask") || m.contains("layer")).collect::<Vec<_>>();
    assert!(about_layers(&mut e).is_empty(), "{:?}", about_layers(&mut e));

    // layer above 31
    spawn(&mut e, 3, r#"{"Transform":{"pos":[2,0,0]},"Collider":{"shape":1},"CollisionLayer":{"layer":40,"mask":4294967295}}"#);
    let m = about_layers(&mut e);
    assert!(m.iter().any(|x| x.contains("above 31") && x.contains("40")), "{m:?}");

    // a mask of 0
    spawn(&mut e, 4, r#"{"Transform":{"pos":[4,0,0]},"Collider":{"shape":1},"CollisionLayer":{"layer":5,"mask":0}}"#);
    let m = about_layers(&mut e);
    assert!(m.iter().any(|x| x.contains("mask is 0") && x.contains("collides with nothing")), "{m:?}");

    // a layer on an entity with nothing to collide
    spawn(&mut e, 5, r#"{"Transform":{"pos":[6,0,0]},"CollisionLayer":{"layer":1}}"#);
    let m = about_layers(&mut e);
    assert!(m.iter().any(|x| x.contains("nothing it applies to")), "{m:?}");

    // a pair where only one side wants the other: a coin trigger (layer 6, wants layer 2) and the player (layer 2, wants layers 1 only)
    spawn(&mut e, 6, r#"{"Transform":{"pos":[8,0,0]},"Collider":{"shape":0,"trigger":true},"CollisionLayer":{"layer":6,"mask":4}}"#);
    let m = about_layers(&mut e);
    assert!(m.iter().any(|x| x.contains("wants to collide with entity") && x.contains("both masks must agree")), "{m:?}");
}

#[test]
fn check_says_when_a_body_would_fall_through_the_whole_world() {
    let mut e = engine();
    spawn(&mut e, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[20,1,20]},"CollisionLayer":{"layer":1,"mask":2}}"#);
    spawn(&mut e, 2, r#"{"Transform":{"pos":[0,3,0]},"Velocity":{},"Collider":{"shape":0},"RigidBody":{},"CollisionLayer":{"layer":2,"mask":4}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("falls through everything")), "{m:?}");
    // only the ground changing its mind is not enough (both must agree)...
    call(&mut e, "set", r#"{"uid":1,"component":"CollisionLayer","value":{"mask":4}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("falls through everything")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("wants to collide with entity")), "and the one-sided pair is named: {m:?}");
    // ...the ball has to want the ground too
    call(&mut e, "set", r#"{"uid":2,"component":"CollisionLayer","value":{"mask":2}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("falls through") || x.contains("wants to collide")), "{m:?}");
}

#[test]
fn a_world_without_layers_gets_no_layer_messages() {
    let mut e = engine();
    spawn(&mut e, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[20,1,20]}}"#);
    spawn(&mut e, 2, r#"{"Transform":{"pos":[0,3,0]},"Velocity":{},"Collider":{"shape":0},"RigidBody":{}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("layer") || x.contains("mask")), "{m:?}");
}

// ------------------------------------------------------------------------------------------------ nothing changes for a scene that does not use them

/// `tools/golden_hashes.txt` is the state hash of every example after 600 ticks. The physics and the characters, agents and sensors that layers touch
/// run in these scenes with no `CollisionLayer` anywhere: they must give the very same bits as before layers existed.
#[test]
fn the_examples_that_use_physics_keep_their_golden_hashes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = std::fs::read_to_string(root.join("tools/golden_hashes.txt")).unwrap().replace("\r\n", "\n");
    // rigid bodies of every shape (pile, dominoes, ragdolls, ropes, mesh_level: meshes and hulls), characters and navigation (walker, maze, skirmish),
    // terrain (terrain) and sensors with sight lines (village)
    for scene in ["pile", "dominoes", "ragdolls", "ropes", "mesh_level", "walker", "maze", "skirmish", "terrain", "village"] {
        let rel = format!("examples/{scene}.lunaris.json");
        let want = golden.lines().find(|l| l.starts_with(&rel)).unwrap_or_else(|| panic!("no golden line for {rel}"));
        let want = want.split_whitespace().nth(2).unwrap().to_string();
        let mut e = Engine::new(1, &root);
        call(&mut e, "load_scene", &format!(r#"{{"path":"{rel}"}}"#));
        while e.sim.tick < 600 {
            let left = 600 - e.sim.tick;
            call(&mut e, "step", &format!(r#"{{"ticks":{left},"budget_ms":120000}}"#));
        }
        let got = call(&mut e, "hash", "{}").get("hash").unwrap().as_str().unwrap().to_string();
        assert_eq!(got, want, "{scene}: state hash after 600 ticks changed");
    }
}
