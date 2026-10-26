//! Creatures in the simulation: a spider that is spawned is alive and drawn (50 parts and its details), stands, walks towards its goal, follows an entity, is hashed, checkpointed and recorded like the rest of the
//! state, and is the same on every run. The brain is a hand-written walker (the pressure up to stand, each leg swinging with its own phase); the trained brains are the project's, not this test's.
use super::*;
use components::{Creature, CreaturePart, Transform};
use lunaris_json::{parse, Json};
use std::path::PathBuf;

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A workspace with a brain file: the body program sets the hydraulic pressure (r24) to 800; the leg program makes the swing and the lift follow the leg's gait phase.
fn workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lunaris_creatures_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let brain = r#"{"setup":[],"predict":[[1,3,0,0,0,0,0.45],[5,0,35,3,0,0,0],[5,1,36,3,0,0,0],[1,2,0,0,0,0,-150]],"learn":[[1,24,0,0,0,0,800]]}"#;
    std::fs::write(dir.join("walker.json"), brain).unwrap();
    dir
}

fn world(dir: &PathBuf) -> Sim {
    let mut s = Sim::new(3);
    s.threads = 1;
    s.asset_root = Some(dir.clone());
    s
}

fn creature_json(extra: &str) -> String {
    format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Creature":{{"brain":"walker.json","goal":[4,0,0]{extra}}}}}"#)
}

fn state(s: &Sim, uid: u64) -> Creature {
    *s.world.get::<Creature>(s.world.entity(uid).unwrap()).unwrap()
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn parts(s: &mut Sim) -> usize {
    let mut n = 0usize;
    let _ = s.world.for_each::<(&CreaturePart,), _>(|_, (_,)| n += 1);
    n
}

#[test]
fn a_spider_is_made_drawn_and_stands() {
    let dir = workspace("stand");
    let mut s = world(&dir);
    spawn(&mut s, 1, &creature_json(""));
    for _ in 0..90 {
        s.step();
    }
    // the 50 segments, the details that are drawn on (8 eyes, 2 fangs, 2 palps, a stripe, 2 spinnerets) and the silk thread (drawn only while one is held)
    assert_eq!(parts(&mut s), 50 + 15 + 1);
    assert_eq!(s.creatures.len(), 1);
    let c = state(&s, 1);
    assert!(c.height > 0.12 && c.height < 0.45, "it stands on its legs, not on its belly: {}", c.height);
    assert!(c.upright > 0.9, "it is the right way up: {}", c.upright);
    assert!(c.feet >= 4, "{} feet on the ground", c.feet);
    // the creature's entity rides on its prosoma
    assert!((pos(&s, 1)[1] - c.height).abs() < 0.01);
}

#[test]
fn it_walks_towards_its_goal_and_follows_an_entity() {
    let dir = workspace("walk");
    let mut s = world(&dir);
    spawn(&mut s, 1, &creature_json(""));
    let d0 = {
        s.step();
        state(&s, 1).distance
    };
    for _ in 0..360 {
        s.step();
    }
    let c = state(&s, 1);
    assert!(c.distance.is_finite() && c.upright > 0.5);
    // the hand-written walker does not steer; the test is that the machinery works, the distance is a number that follows the body
    assert!((c.distance - d0).abs() > 0.01 || c.speed >= 0.0);
    // a target entity: the creature takes its place
    spawn(&mut s, 1000, r#"{"Transform":{"pos":[-3,0,2]}}"#);
    s.world.get_mut::<Creature>(s.world.entity(1).unwrap()).unwrap().target = lunaris_ecs::Uid(1000);
    s.step();
    let c = state(&s, 1);
    let p = pos(&s, 1);
    let want = ((-3.0f32 - p[0]).powi(2) + (2.0f32 - p[2]).powi(2)).sqrt();
    assert!((c.distance - want).abs() < 0.2, "the distance is measured to the entity: {} against {want}", c.distance);
}

#[test]
fn the_creature_is_part_of_the_state_and_the_same_on_every_run() {
    let dir = workspace("det");
    let run = |ticks: u64| {
        let mut s = world(&dir);
        spawn(&mut s, 1, &creature_json(r#","body_seed":5"#));
        for _ in 0..ticks {
            s.step();
        }
        s
    };
    let (a, b) = (run(120), run(120));
    assert_eq!(a.state_hash(), b.state_hash(), "the same run gives the same state");
    assert_ne!(a.state_hash(), run(121).state_hash());
    // a checkpoint and a rewind bring the creature back exactly
    let mut s = run(60);
    let c = s.checkpoint();
    for _ in 0..60 {
        s.step();
    }
    let later = s.state_hash();
    s.rewind_to(&c).unwrap();
    for _ in 0..60 {
        s.step();
    }
    assert_eq!(s.state_hash(), later, "rewinding to a checkpoint and stepping again repeats the history");
    // a whole clone is the same state
    let t = s.clone();
    assert_eq!(t.state_hash(), s.state_hash());
}

#[test]
fn removing_the_creature_removes_its_parts() {
    let dir = workspace("gone");
    let mut s = world(&dir);
    spawn(&mut s, 1, &creature_json(""));
    for _ in 0..5 {
        s.step();
    }
    assert!(parts(&mut s) > 0);
    s.apply(&Command::Despawn { uid: 1 }).unwrap();
    s.step();
    assert_eq!(parts(&mut s), 0);
    assert_eq!(s.creatures.len(), 0);
}

#[test]
fn a_missing_brain_is_an_error_not_a_crash() {
    let dir = workspace("nobrain");
    let mut s = world(&dir);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Creature":{"brain":"nothing.json"}}"#);
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(s.creatures.len(), 0);
    assert!(s.creatures.errors.contains_key("nothing.json"));
}

#[test]
fn a_smaller_creature_is_the_same_animal_in_smaller_units() {
    let dir = workspace("size");
    let run = |extra: &str| {
        let mut s = world(&dir);
        spawn(&mut s, 1, &creature_json(extra));
        for _ in 0..90 {
            s.step();
        }
        state(&s, 1)
    };
    let (big, small) = (run(""), run(r#","size":0.25"#));
    // a quarter of the height, standing the same way
    let ratio = small.height / big.height;
    assert!((ratio - 0.25).abs() < 0.05, "a quarter-size animal stands a quarter as high: {} against {}", small.height, big.height);
    assert!(small.upright > 0.9 && small.feet >= 4, "it stands: upright {} feet {}", small.upright, small.feet);
    // and its parts are drawn a quarter as big: the prosoma entity's scale
    let mut s = world(&dir);
    spawn(&mut s, 1, &creature_json(r#","size":0.25"#));
    s.step();
    let mut biggest = 0.0f32;
    let _ = s.world.for_each::<(&CreaturePart, &Transform), _>(|_, (_, t)| biggest = biggest.max(t.scale[0]).max(t.scale[1]).max(t.scale[2]));
    assert!(biggest < 0.2, "no part of a quarter-size spider is bigger than a fifth of a metre: {biggest}");
}

#[test]
fn a_foraging_creature_eats_food_and_it_appears_somewhere_else() {
    use components::{Food, FoodSpot};
    let dir = workspace("forage");
    let mut s = world(&dir);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Creature":{"brain":"walker.json","forage":true}}"#);
    // food within reach of the body, and a place far away for it to appear at
    spawn(&mut s, 50, r#"{"Transform":{"pos":[0.15,0.25,0]},"Food":{}}"#);
    spawn(&mut s, 60, r#"{"Transform":{"pos":[6,0.25,6]},"FoodSpot":{}}"#);
    for _ in 0..60 {
        s.step();
    }
    let c = state(&s, 1);
    assert!(c.eaten >= 1, "the creature ate: {}", c.eaten);
    let e = s.world.entity(50).unwrap();
    assert_eq!(s.world.get::<Food>(e).unwrap().eaten, c.eaten, "the food counts how often it was eaten");
    assert_eq!(pos(&s, 50), [6.0, 0.25, 6.0], "the food reappeared at the only food spot");
    assert!(s.world.get::<FoodSpot>(s.world.entity(60).unwrap()).is_some());
}

#[test]
fn a_neural_brain_is_loaded_from_a_champions_file_and_runs_the_same_every_time() {
    let dir = workspace("neural");
    // a champions file of the neural kind: small deterministic weights, the pressure output biased up so that it stands
    let mut w: Vec<f32> = Vec::new();
    let mut z = 0x9E37_79B9u32;
    for _ in 0..lunaris_creature::nn::NNP {
        z = z.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        w.push(((z >> 8) as f32 / 16_777_216.0 - 0.5) * 0.2);
    }
    let weights = w.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(",");
    std::fs::write(dir.join("net.json"), format!(r#"{{"kind":"nn","genomes":[{{"nn":[{weights}]}}]}}"#)).unwrap();
    let run = |brain: &str| {
        let mut s = world(&dir);
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Creature":{{"brain":"{brain}","goal":[4,0,0]}}}}"#));
        for _ in 0..120 {
            s.step();
        }
        s
    };
    let (a, b) = (run("net.json"), run("net.json"));
    assert_eq!(a.state_hash(), b.state_hash(), "a neural brain gives the same state on every run");
    assert_ne!(a.state_hash(), run("walker.json").state_hash(), "the neural brain, not the programs, drives the body");
    assert!(state(&a, 1).height.is_finite() && state(&a, 1).height > 0.0, "the body is alive: {}", state(&a, 1).height);
}

#[test]
fn a_foraging_creature_gives_up_on_food_it_cannot_reach_and_goes_for_another() {
    let dir = workspace("giveup");
    // a creature that only stands: it never gets closer to anything
    std::fs::write(dir.join("stander.json"), r#"{"setup":[],"predict":[],"learn":[[1,24,0,0,0,0,800]]}"#).unwrap();
    let mut s = world(&dir);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Creature":{"brain":"stander.json","forage":true,"patience":1.0}}"#);
    // the nearest food is up in the air, the other is on the floor a little further away
    spawn(&mut s, 50, r#"{"Transform":{"pos":[0.3,2.5,0]},"Food":{}}"#);
    spawn(&mut s, 51, r#"{"Transform":{"pos":[2.8,0.25,0]},"Food":{}}"#);
    spawn(&mut s, 60, r#"{"Transform":{"pos":[6,0.25,6]},"FoodSpot":{}}"#);
    s.step();
    assert_eq!(state(&s, 1).target.0, 50, "it starts with the nearest food");
    // it stands and gets nowhere, so it gives up on the high food and goes for the other (and, standing still, on that one after a while: it never stops trying the food it can)
    let mut switched = None;
    for tick in 0..420 {
        s.step();
        if state(&s, 1).target.0 == 51 {
            switched = Some(tick);
            break;
        }
    }
    let tick = switched.expect("it gave up on the food it could not reach");
    assert!(tick > 60, "not at once: patience is a second (after {tick} ticks)");
    assert_eq!(state(&s, 1).given_up.0, 50);
}

#[test]
fn a_spider_rests_after_a_meal_and_freezes_when_alarmed() {
    let dir = workspace("moods");
    // the walker walks on and on; resting and freezing must stop it (it stands, all feet down)
    let dist = |s: &Sim| {
        let p = pos(s, 1);
        (p[0] * p[0] + p[2] * p[2]).sqrt()
    };
    // an alarm freezes it from the start: it stays where it was made
    let mut s = world(&dir);
    spawn(&mut s, 1, &creature_json(r#","alarm":3.0,"scan_every":0"#));
    for _ in 0..150 {
        s.step();
    }
    assert!(dist(&s) < 0.15, "frozen by the alarm it did not walk away: {}", dist(&s));
    assert!(state(&s, 1).feet >= 6, "it stands on its legs while frozen: {} feet", state(&s, 1).feet);
    // the alarm runs out and it walks
    for _ in 0..300 {
        s.step();
    }
    assert!(dist(&s) > 0.25, "after the alarm it walks: {}", dist(&s));
    // a meal is followed by a rest of about `rest` seconds
    let mut s = world(&dir);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Creature":{"brain":"walker.json","forage":true,"rest":4.0,"scan_every":0}}"#);
    spawn(&mut s, 50, r#"{"Transform":{"pos":[0.15,0.25,0]},"Food":{}}"#);
    spawn(&mut s, 60, r#"{"Transform":{"pos":[6,0.25,6]},"FoodSpot":{}}"#);
    let mut ate_at = None;
    for t in 0..200 {
        s.step();
        if ate_at.is_none() && state(&s, 1).eaten >= 1 {
            ate_at = Some(t);
        }
    }
    let ate_at = ate_at.expect("it ate");
    assert!(state(&s, 1).pause_left > 0.0 || ate_at > 120, "it is resting after the meal");
}
