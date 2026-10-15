//! Collision layers (`CollisionLayer`): a body passes through what it does not collide with, whatever the shapes are; both masks must agree;
//! triggers, contacts, characters, agents and sensors follow the same rule; and a world without the component is untouched.
use super::*;
use crate::layers::{Filter, Layers, ALL};
use crate::meshcol_gltf::tests::glb_of;
use crate::png::tests::make_png;
use components::{CharacterBody, NavAgent, Sensor, Transform, Velocity};
use lunaris_json::{parse, Json};
use std::path::PathBuf;

type V3 = [f32; 3];

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn pos(s: &Sim, uid: u64) -> V3 {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn lin(s: &Sim, uid: u64) -> V3 {
    s.world.get::<Velocity>(s.world.entity(uid).unwrap()).unwrap().lin
}

/// The layer part of a scene entity: `layer(1, 4)` is layer 1 colliding with layer 2 only (mask bit 2).
fn layer(layer: u32, mask: u32) -> String {
    format!(r#","CollisionLayer":{{"layer":{layer},"mask":{mask}}}"#)
}

fn dir_for(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lunaris-layers-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ------------------------------------------------------------------------------------------------ the rule

#[test]
fn a_ball_passes_through_a_wall_on_another_layer_and_is_stopped_by_one_on_its_own() {
    // no gravity, a ball rolling at 6 m/s at a wall one metre thick, 4 m away
    let run = |wall: &str, ball: &str| {
        let mut s = Sim::new(1);
        s.threads = 1;
        spawn(&mut s, 9, r#"{"PhysicsSettings":{"gravity":[0,0,0]}}"#);
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[4,0,0]}},"Collider":{{"shape":1,"size":[1,6,6]}}{wall}}}"#));
        spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Velocity":{{"lin":[6,0,0]}},"Collider":{{"shape":0,"size":[1,1,1]}},"RigidBody":{{"mass":1,"restitution":0.5,"friction":0}}{ball}}}"#));
        let mut contacts = 0usize;
        for _ in 0..120 {
            s.step();
            contacts += s.contacts.len();
        }
        (pos(&s, 2)[0], lin(&s, 2)[0], contacts)
    };
    // the old world: it hits the wall and comes back
    let (x, vx, _) = run("", "");
    assert!(x < 3.6 && vx <= 0.0, "without layers it bounces off the wall: x {x}, vx {vx}");
    // both on layer 1 and 2 that want each other: the same
    let (x, vx, c) = run(&layer(1, 1 << 2), &layer(2, 1 << 1));
    assert!(x < 3.6 && vx <= 0.0 && c > 0, "layers that want each other still collide: x {x}, vx {vx}");
    // the ball does not collide with the wall's layer, and the wall does not with the ball's: straight through, never a contact
    let (x, vx, c) = run(&layer(1, 1 << 1), &layer(2, 1 << 2));
    assert!(x > 8.0 && (vx - 6.0).abs() < 1e-3, "a wall on another layer is no wall: x {x}, vx {vx}");
    assert_eq!(c, 0, "and there is not a single contact to report");
}

#[test]
fn both_masks_must_agree_so_one_side_leaving_the_other_out_is_enough() {
    let run = |wall: &str, ball: &str, wall_uid: u64, ball_uid: u64| {
        let mut s = Sim::new(1);
        s.threads = 1;
        spawn(&mut s, 9, r#"{"PhysicsSettings":{"gravity":[0,0,0]}}"#);
        spawn(&mut s, wall_uid, &format!(r#"{{"Transform":{{"pos":[4,0,0]}},"Collider":{{"shape":1,"size":[1,6,6]}}{wall}}}"#));
        spawn(&mut s, ball_uid, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Velocity":{{"lin":[6,0,0]}},"Collider":{{"shape":0,"size":[1,1,1]}},"RigidBody":{{"mass":1,"restitution":0.5,"friction":0}}{ball}}}"#));
        s.step_n(120);
        pos(&s, ball_uid)[0]
    };
    for (wall_uid, ball_uid) in [(1, 2), (2, 1)] {
        // the ball wants the wall (its mask has layer 1) but the wall's mask leaves layer 2 out
        let x = run(&layer(1, 1 << 1), &layer(2, ALL), wall_uid, ball_uid);
        assert!(x > 8.0, "the wall does not want the ball: it passes ({x}), uids {wall_uid}/{ball_uid}");
        // the wall wants the ball but the ball's mask leaves layer 1 out
        let x = run(&layer(1, ALL), &layer(2, 1 << 2), wall_uid, ball_uid);
        assert!(x > 8.0, "the ball does not want the wall: it passes ({x}), uids {wall_uid}/{ball_uid}");
        // both want each other
        let x = run(&layer(1, 1 << 2), &layer(2, 1 << 1), wall_uid, ball_uid);
        assert!(x < 3.6, "both agree: it hits ({x}), uids {wall_uid}/{ball_uid}");
    }
}

#[test]
fn an_entity_without_the_component_is_layer_0_with_a_full_mask() {
    // a ball on layer 0 that wants layer 0 only, against a wall without the component: they collide
    let mut s = Sim::new(1);
    s.threads = 1;
    spawn(&mut s, 9, r#"{"PhysicsSettings":{"gravity":[0,0,0]}}"#);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[4,0,0]},"Collider":{"shape":1,"size":[1,6,6]}}"#);
    spawn(&mut s, 2, &format!(r#"{{"Transform":{{}},"Velocity":{{"lin":[6,0,0]}},"Collider":{{"shape":0}},"RigidBody":{{"restitution":0.5,"friction":0}}{}}}"#, layer(0, 1)));
    s.step_n(120);
    assert!(pos(&s, 2)[0] < 3.6, "the wall is layer 0: {:?}", pos(&s, 2));
    // the same ball on layer 5 that wants layer 5 only: it goes through
    let mut t = Sim::new(1);
    t.threads = 1;
    spawn(&mut t, 9, r#"{"PhysicsSettings":{"gravity":[0,0,0]}}"#);
    spawn(&mut t, 1, r#"{"Transform":{"pos":[4,0,0]},"Collider":{"shape":1,"size":[1,6,6]}}"#);
    spawn(&mut t, 2, &format!(r#"{{"Transform":{{}},"Velocity":{{"lin":[6,0,0]}},"Collider":{{"shape":0}},"RigidBody":{{"restitution":0.5,"friction":0}}{}}}"#, layer(5, 1 << 5)));
    t.step_n(120);
    assert!(pos(&t, 2)[0] > 8.0, "{:?}", pos(&t, 2));
}

#[test]
fn a_layer_above_31_counts_as_31_and_the_default_state_is_not_touched() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{}}{}}}"#, layer(99, 5)));
    spawn(&mut s, 2, r#"{"Transform":{}}"#);
    let layers = Layers::gather(&mut s.world);
    assert_eq!(layers.of(1), Filter::new(31, 5));
    assert_eq!(layers.of(1).layer, 31);
    assert_eq!(layers.of(2), Filter::DEFAULT, "no component, the default filter");
    assert!(Layers::gather(&mut Sim::new(1).world).is_empty(), "a world without the component has nothing to look up");
}

// ------------------------------------------------------------------------------------------------ every shape, against every kind of ground

const FLOOR: f32 = 0.0;

fn cube_tris(centre: V3, half: V3) -> Vec<[V3; 3]> {
    let c = |x: f32, y: f32, z: f32| [centre[0] + x * half[0], centre[1] + y * half[1], centre[2] + z * half[2]];
    let quad = |a: V3, b: V3, cc: V3, d: V3| vec![[a, b, cc], [a, cc, d]];
    let mut t = Vec::new();
    t.extend(quad(c(-1.0, 1.0, -1.0), c(-1.0, 1.0, 1.0), c(1.0, 1.0, 1.0), c(1.0, 1.0, -1.0)));
    t.extend(quad(c(-1.0, -1.0, -1.0), c(1.0, -1.0, -1.0), c(1.0, -1.0, 1.0), c(-1.0, -1.0, 1.0)));
    t.extend(quad(c(-1.0, -1.0, -1.0), c(-1.0, 1.0, -1.0), c(1.0, 1.0, -1.0), c(1.0, -1.0, -1.0)));
    t.extend(quad(c(-1.0, -1.0, 1.0), c(1.0, -1.0, 1.0), c(1.0, 1.0, 1.0), c(-1.0, 1.0, 1.0)));
    t.extend(quad(c(-1.0, -1.0, -1.0), c(-1.0, -1.0, 1.0), c(-1.0, 1.0, 1.0), c(-1.0, 1.0, -1.0)));
    t.extend(quad(c(1.0, -1.0, -1.0), c(1.0, 1.0, -1.0), c(1.0, 1.0, 1.0), c(1.0, -1.0, 1.0)));
    t
}

/// A folder with the models and the picture the grounds below use.
fn workspace(name: &str) -> PathBuf {
    let d = dir_for(name);
    // a thin sheet of a floor, 200 by 200 m, at the height of the others
    std::fs::write(d.join("floor.glb"), glb_of(&[[[-100.0, FLOOR, -100.0], [-100.0, FLOOR, 100.0], [100.0, FLOOR, 100.0]], [[-100.0, FLOOR, -100.0], [100.0, FLOOR, 100.0], [100.0, FLOOR, -100.0]]])).unwrap();
    // a unit cube, to be a hull (a static slab when stretched, a crate when not)
    std::fs::write(d.join("cube.glb"), glb_of(&cube_tris([0.0; 3], [0.5; 3]))).unwrap();
    // flat ground, 33 by 33 samples of black
    std::fs::write(d.join("flat.png"), make_png(33, 33, 16, 0, 4, &vec![0u16; 33 * 33])).unwrap();
    d
}

fn grounds() -> Vec<(&'static str, String)> {
    vec![
        ("a box", r#""Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[200,1,200]}"#.to_string()),
        ("a mesh of triangles", r#""Transform":{},"MeshCollider":{"mesh":"floor.glb"}"#.to_string()),
        ("a terrain", r#""Transform":{},"Terrain":{"heightmap":"flat.png","size":[200,200],"height":10}"#.to_string()),
        ("a static hull", r#""Transform":{"pos":[0,-0.5,0],"scale":[200,1,200]},"ConvexCollider":{"mesh":"cube.glb"}"#.to_string()),
    ]
}

/// Bodies that fall from 20 m (fast enough for the continuous collision of a mesh level and of a spinning ball to matter).
fn bodies() -> Vec<(&'static str, String)> {
    let body = r#""Transform":{"pos":[0,20,0]},"Velocity":{}"#;
    vec![
        ("a ball", format!(r#"{body},"Collider":{{"shape":0,"size":[0.5,0.5,0.5]}},"RigidBody":{{"mass":1,"restitution":0}}"#)),
        ("a spinning ball", format!(r#"{body},"AngularVelocity":{{"w":[0,0,3]}},"Collider":{{"shape":0,"size":[0.5,0.5,0.5]}},"RigidBody":{{"mass":1,"restitution":0}}"#)),
        ("a box", format!(r#"{body},"Collider":{{"shape":1,"size":[0.6,0.6,0.6]}},"RigidBody":{{"mass":1,"restitution":0}}"#)),
        ("a turning box", format!(r#"{body},"AngularVelocity":{{"w":[0.5,0.3,0.1]}},"Collider":{{"shape":1,"size":[0.6,0.6,0.6]}},"RigidBody":{{"mass":1,"restitution":0}}"#)),
        ("a capsule", format!(r#"{body},"AngularVelocity":{{}},"Collider":{{"shape":2,"size":[0.5,1.4,0.5]}},"RigidBody":{{"mass":1,"restitution":0}}"#)),
        ("a hull", format!(r#"{body},"AngularVelocity":{{"w":[0.2,0,0.4]}},"ConvexCollider":{{"mesh":"cube.glb"}},"RigidBody":{{"mass":1,"restitution":0}}"#)),
    ]
}

#[test]
fn every_kind_of_body_falls_through_every_kind_of_ground_on_another_layer_and_lands_on_one_it_shares() {
    let d = workspace("matrix");
    for (ground_name, ground) in grounds() {
        for (body_name, body) in bodies() {
            let run = |ground_layer: &str, body_layer: &str| {
                let mut s = Sim::new(1);
                s.threads = 1;
                s.asset_root = Some(d.clone());
                spawn(&mut s, 1, &format!("{{{ground}{ground_layer}}}"));
                spawn(&mut s, 2, &format!("{{{body}{body_layer}}}"));
                let mut touched = 0usize;
                for _ in 0..360 {
                    s.step();
                    touched += s.contacts.len();
                }
                (pos(&s, 2)[1], touched)
            };
            // the ground (layer 1) and the body (layer 2) want each other: it lands and rests
            let (y, touched) = run(&layer(1, 1 << 2), &layer(2, 1 << 1));
            assert!(y > 0.05 && y < 1.5 && touched > 0, "{body_name} on {ground_name}, same layers: rests at {y} ({touched} contacts)");
            // neither wants the other: it falls through and meets nothing
            let (y, touched) = run(&layer(1, 1 << 1), &layer(2, 1 << 2));
            assert!(y < -50.0, "{body_name} on {ground_name}, other layers: it fell through to {y}");
            assert_eq!(touched, 0, "{body_name} on {ground_name}: no contact is reported through a layer");
            // only the body wants the ground
            let (y, touched) = run(&layer(1, 1 << 1), &layer(2, 1 << 1));
            assert!(y < -50.0 && touched == 0, "{body_name} on {ground_name}, the ground does not want it: {y}");
            // and only the ground wants the body
            let (y, touched) = run(&layer(1, 1 << 2), &layer(2, 1 << 2));
            assert!(y < -50.0 && touched == 0, "{body_name} on {ground_name}, the body does not want the ground: {y}");
        }
    }
}

#[test]
fn bodies_on_different_layers_pass_through_each_other_in_a_pile() {
    // two stacked boxes: the second drops through the first when their layers do not meet, and lands on it when they do
    let run = |a: &str, b: &str| {
        let mut s = Sim::new(1);
        s.threads = 1;
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,-0.5,0]}},"Collider":{{"shape":1,"size":[50,1,50]}}{}}}"#, layer(1, ALL)));
        spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,0.5,0]}},"Velocity":{{}},"AngularVelocity":{{}},"Collider":{{"shape":1,"size":[1,1,1]}},"RigidBody":{{"mass":1}}{a}}}"#));
        spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":[0,3,0]}},"Velocity":{{}},"AngularVelocity":{{}},"Collider":{{"shape":1,"size":[1,1,1]}},"RigidBody":{{"mass":1}}{b}}}"#));
        s.step_n(240);
        (pos(&s, 2)[1], pos(&s, 3)[1])
    };
    let (low, high) = run(&layer(2, 1 << 1 | 1 << 3), &layer(3, 1 << 1 | 1 << 2));
    assert!((low - 0.5).abs() < 0.05 && (high - 1.5).abs() < 0.1, "they stack: {low} {high}");
    let (low, high) = run(&layer(2, 1 << 1), &layer(3, 1 << 1));
    assert!((low - 0.5).abs() < 0.05 && (high - 0.5).abs() < 0.05, "both on the floor, in the same place, through each other: {low} {high}");
}

// ------------------------------------------------------------------------------------------------ triggers and events

#[test]
fn a_trigger_only_fires_for_what_it_collides_with() {
    let mut s = Sim::new(1);
    s.threads = 1;
    spawn(&mut s, 9, r#"{"PhysicsSettings":{"gravity":[0,0,0]}}"#);
    // a trigger on layer 3 that wants layers 4 and 5
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{}},"Collider":{{"shape":1,"size":[4,4,4],"trigger":true}}{}}}"#, layer(3, 1 << 4 | 1 << 5)));
    // four bodies inside it: layer 4 (wants layer 3), layer 5 (does not want layer 3), layer 6 (wants it, but the trigger does not), none
    spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Velocity":{{}},"Collider":{{"shape":0}},"RigidBody":{{}}{}}}"#, layer(4, 1 << 3)));
    spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":[1,0,0]}},"Velocity":{{}},"Collider":{{"shape":0}},"RigidBody":{{}}{}}}"#, layer(5, 1 << 5)));
    spawn(&mut s, 4, &format!(r#"{{"Transform":{{"pos":[-1,0,0]}},"Velocity":{{}},"Collider":{{"shape":0}},"RigidBody":{{}}{}}}"#, layer(6, 1 << 3)));
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,1,0]},"Velocity":{},"Collider":{"shape":0},"RigidBody":{}}"#);
    s.step();
    let who: Vec<(u64, u64, bool)> = s.contacts.iter().map(|c| (c.a, c.b, c.trigger)).collect();
    assert!(who.contains(&(1, 2, true)), "layer 4 and the trigger want each other: {who:?}");
    assert!(!who.iter().any(|c| c.1 == 3 || c.0 == 3), "layer 5 does not want the trigger's layer: {who:?}");
    assert!(!who.iter().any(|c| c.1 == 4 || c.0 == 4), "the trigger does not want layer 6: {who:?}");
    assert!(!who.iter().any(|c| c.1 == 5 || c.0 == 5), "the trigger leaves layer 0 out: {who:?}");
    assert_eq!(who.len(), 1, "{who:?}");
}

// ------------------------------------------------------------------------------------------------ the character controller

fn walker(wall: &str, who: &str) -> Sim {
    let mut s = Sim::new(1);
    s.threads = 1;
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,-0.5,0]}},"Collider":{{"shape":1,"size":[100,1,100]}}{}}}"#, layer(1, ALL)));
    spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,0.2,0]}},"CharacterBody":{{}}{who}}}"#));
    spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":[5,1,0]}},"Collider":{{"shape":1,"size":[1,2,20]}}{wall}}}"#));
    s.step_n(20);
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<CharacterBody>(e).unwrap().walk = [3.0, 0.0];
    s.step_n(180);
    s
}

#[test]
fn a_character_walks_through_what_it_does_not_collide_with() {
    // the hero (layer 2) and a wall (layer 3) that want each other: it is stopped at 4.5 - 0.35
    let s = walker(&layer(3, 1 << 2), &layer(2, 1 << 1 | 1 << 3));
    assert!((pos(&s, 2)[0] - 4.15).abs() < 0.03, "stopped by the wall: {:?}", pos(&s, 2));
    // the hero does not want the wall's layer: it walks through
    let s = walker(&layer(3, 1 << 2), &layer(2, 1 << 1));
    assert!(pos(&s, 2)[0] > 8.0, "through the wall: {:?}", pos(&s, 2));
    // the wall does not want the hero: the same
    let s = walker(&layer(3, 1 << 3), &layer(2, 1 << 1 | 1 << 3));
    assert!(pos(&s, 2)[0] > 8.0, "the wall's mask: {:?}", pos(&s, 2));
    // and the ground still holds it up (layer 1 is in its mask and it is in the ground's)
    assert!(pos(&s, 2)[1].abs() < 0.05 && s.world.get::<CharacterBody>(s.world.entity(2).unwrap()).unwrap().grounded, "{:?}", pos(&s, 2));
    // a hero with no layer and a wall with none: the old world
    let s = walker("", "");
    assert!((pos(&s, 2)[0] - 4.15).abs() < 0.03, "{:?}", pos(&s, 2));
}

#[test]
fn a_character_falls_through_a_floor_it_does_not_collide_with() {
    let mut s = Sim::new(1);
    s.threads = 1;
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,-0.5,0]}},"Collider":{{"shape":1,"size":[100,1,100]}}{}}}"#, layer(1, ALL)));
    spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,0.5,0]}},"CharacterBody":{{}}{}}}"#, layer(2, 1 << 2)));
    s.step_n(120);
    assert!(pos(&s, 2)[1] < -5.0, "{:?}", pos(&s, 2));
}

// ------------------------------------------------------------------------------------------------ navigation

#[test]
fn an_agent_plans_straight_through_a_wall_that_is_not_in_its_layers() {
    let make = |wall: &str, agent: &str| {
        let mut s = Sim::new(1);
        s.threads = 1;
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,-0.5,0],"scale":[100,1,100]}},"Collider":{{"shape":1}}{}}}"#, layer(1, ALL)));
        spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[5,1.5,0],"scale":[1,3,10]}},"Collider":{{"shape":1}}{wall}}}"#));
        spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"NavAgent":{{"speed":4}},"CharacterBody":{{}}{agent}}}"#));
        s
    };
    let plan = |s: &mut Sim| {
        let e = s.world.entity(3).unwrap();
        let n = s.world.get_mut::<NavAgent>(e).unwrap();
        n.target = [10.0, 0.0, 0.0];
        n.active = true;
        s.step_n(1);
        s.nav.paths.get(&3).map(|p| p.points.clone()).unwrap_or_default()
    };
    // the old world: round the end of the wall
    let mut s = make("", "");
    let around = plan(&mut s);
    let len = |p: &[V3]| p.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][2] - w[0][2]).powi(2)).sqrt()).sum::<f32>();
    assert!(len(&around) > 14.0, "round the wall: {around:?}");
    // the wall is on a layer the agent does not collide with: straight to the target
    let mut s = make(&layer(3, 1 << 3), &layer(2, 1 << 1));
    let through = plan(&mut s);
    assert!((len(&through) - 10.0).abs() < 0.2, "straight through: {through:?}");
    // a wall that wants the agent's layer but the agent does not want: the same
    let mut s = make(&layer(3, ALL), &layer(2, 1 << 1));
    assert!((len(&plan(&mut s)) - 10.0).abs() < 0.2);
    // both want each other: round again
    let mut s = make(&layer(3, 1 << 2), &layer(2, 1 << 1 | 1 << 3));
    assert!(len(&plan(&mut s)) > 14.0);
    // and the `path` function for a point (no agent) counts every layer
    let mut s = make(&layer(3, 1 << 3), &layer(2, 1 << 1));
    let r = crate::nav::find_path(&mut s.world, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0], crate::nav::NavParams::default()).unwrap();
    assert!(r.length > 14.0, "a plain point sees every wall: {}", r.length);
}

#[test]
fn a_nav_obstacle_blocks_only_the_agents_that_collide_with_it() {
    // a crate that moves (so it is not part of the map) but says it is an obstacle, on layer 3, across the way of two agents: one on layer 2 that wants layer 3
    // and one on layer 5 that does not
    let make = |agent: &str| {
        let mut s = Sim::new(1);
        s.threads = 1;
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,-0.5,0],"scale":[100,1,100]}},"Collider":{{"shape":1}}{}}}"#, layer(1, ALL)));
        spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[5,1.5,0],"scale":[1,3,10]}},"Collider":{{"shape":1}},"RigidBody":{{"mass":50}},"Velocity":{{}},"NavObstacle":{{"on":true}}{}}}"#, layer(3, ALL)));
        spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"NavAgent":{{"speed":4}},"CharacterBody":{{}}{agent}}}"#));
        s
    };
    let plan = |s: &mut Sim| {
        let e = s.world.entity(3).unwrap();
        let n = s.world.get_mut::<NavAgent>(e).unwrap();
        n.target = [10.0, 0.0, 0.0];
        n.active = true;
        s.step_n(1);
        let pts = s.nav.paths.get(&3).map(|p| p.points.clone()).unwrap_or_default();
        pts.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][2] - w[0][2]).powi(2)).sqrt()).sum::<f32>()
    };
    let mut s = make(&layer(2, 1 << 1 | 1 << 3));
    assert!(plan(&mut s) > 14.0, "an agent that collides with the crate plans round it");
    let mut s = make(&layer(5, 1 << 1));
    assert!((plan(&mut s) - 10.0).abs() < 0.3, "one that does not goes straight through where it stands");
}

// ------------------------------------------------------------------------------------------------ sensors

#[test]
fn a_sensors_sight_is_only_blocked_by_layers_in_its_mask() {
    let make = |wall: &str, eyes: &str| {
        let mut s = Sim::new(1);
        s.threads = 1;
        spawn(&mut s, 9, r#"{"PhysicsSettings":{"gravity":[0,0,0]}}"#);
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{}},"Sensor":{{"radius":30,"fov":360,"filter":"Velocity","los":true,"eye":0}}{eyes}}}"#));
        spawn(&mut s, 2, r#"{"Transform":{"pos":[12,0,0]},"Velocity":{}}"#);
        spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":[6,0,0]}},"Collider":{{"shape":1,"size":[1,6,6]}}{wall}}}"#));
        s.step();
        let e = s.world.entity(1).unwrap();
        s.world.get::<Sensor>(e).unwrap().seen.0
    };
    assert_eq!(make("", ""), 0, "a wall in the way, nothing says otherwise: blocked, as it always was");
    assert_eq!(make(&layer(4, ALL), ""), 0, "a sensor without a layer has a full mask: the wall blocks");
    assert_eq!(make(&layer(4, ALL), &layer(0, ALL)), 0, "a full mask: blocked");
    assert_eq!(make(&layer(4, ALL), &layer(0, 1)), 2, "the wall's layer 4 is not in the sensor's mask: it sees through");
    assert_eq!(make(&layer(4, ALL), &layer(0, 1 << 4)), 0, "layer 4 is in it: blocked");
    // (one-way: the wall's own mask is not asked)
    assert_eq!(make(&layer(4, 0), &layer(0, 1 << 4)), 0);
}

// ------------------------------------------------------------------------------------------------ what they cost

/// `cargo test -p lunaris-sim --lib what_layers_cost -- --nocapture` prints the numbers quoted in docs/PHYSICS.md.
#[test]
fn what_layers_cost() {
    let build = |layered: bool| {
        let mut s = Sim::new(1);
        s.threads = 1;
        let extra = |layer_no: u32| if layered { layer(layer_no, ALL) } else { String::new() };
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,-0.5,0]}},"Collider":{{"shape":1,"size":[400,1,400]}}{}}}"#, extra(1)));
        // 2,000 balls dropped in a grid, half of them on each of two layers
        for i in 0..2000u64 {
            let (x, z) = ((i % 50) as f32 * 1.2 - 30.0, (i / 50) as f32 * 1.2 - 24.0);
            spawn(&mut s, 10 + i, &format!(r#"{{"Transform":{{"pos":[{x},{},{z}]}},"Velocity":{{}},"Collider":{{"shape":0,"size":[1,1,1]}},"RigidBody":{{"mass":1}}{}}}"#, 0.6 + (i % 7) as f32 * 0.9, extra(2 + (i % 2) as u32)));
        }
        s
    };
    let time = |layered: bool| {
        let mut s = build(layered);
        s.step_n(20);
        let t0 = std::time::Instant::now();
        s.step_n(120);
        t0.elapsed().as_secs_f64() * 1000.0 / 120.0
    };
    let (plain, layered) = (time(false), time(true));
    eprintln!("2,000 balls piling up, one thread: {plain:.3} ms a tick without layers, {layered:.3} ms with a CollisionLayer on every body (ratio {:.2})", layered / plain);
    assert!(layered < plain * 1.5 + 0.2, "layers must not slow the physics down noticeably: {plain} against {layered}");
}

// ------------------------------------------------------------------------------------------------ a world that does not use them

#[test]
fn a_world_with_no_layers_never_filters_and_hashes_as_before() {
    // the same scene stepped with and without a `CollisionLayer` that changes nothing (layer 0, full mask on everything) hashes differently only by
    // that component, and the movement is the same bit for bit
    let build = |with: bool| {
        let mut s = Sim::new(1);
        s.threads = 1;
        let extra = if with { layer(0, ALL) } else { String::new() };
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,-0.5,0]}},"Collider":{{"shape":1,"size":[60,1,60]}}{extra}}}"#));
        for i in 0..12u64 {
            let x = (i % 4) as f32 * 0.9 - 1.4;
            let z = (i / 4) as f32 * 0.9 - 0.9;
            spawn(&mut s, 10 + i, &format!(r#"{{"Transform":{{"pos":[{x},{},{z}]}},"Velocity":{{}},"AngularVelocity":{{}},"Collider":{{"shape":{},"size":[0.8,0.8,0.8]}},"RigidBody":{{"mass":1}}{extra}}}"#, 1.0 + i as f32 * 0.9, i % 3));
        }
        s.step_n(300);
        s
    };
    let (a, b) = (build(false), build(true));
    for i in 0..12u64 {
        assert_eq!(pos(&a, 10 + i), pos(&b, 10 + i), "body {i}");
    }
    // and the plain world has no `CollisionLayer` in it at all
    let mut plain = build(false);
    assert!(Layers::gather(&mut plain.world).is_empty());
}
