use super::*;
use lunaris_ecs::to_scene_json;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn populate(sim: &mut Sim, n: u64, order_reversed: bool) {
    let uids: Vec<u64> = if order_reversed { (1..=n).rev().collect() } else { (1..=n).collect() };
    for uid in uids {
        let x = uid as f64 * 0.5;
        sim.apply(&Command::Spawn {
            uid: Some(uid),
            components: vec![
                ("Transform".into(), j(&format!(r#"{{"pos":[{x},0,0]}}"#))),
                ("Velocity".into(), j(r#"{"lin":[1,0.5,0],"ang":[0,0,0.7]}"#)),
                ("Wander".into(), j(r#"{"strength":3}"#)),
            ],
        })
        .unwrap();
    }
}

fn entities_json(sim: &Sim) -> Json {
    to_scene_json(&sim.world).get("entities").unwrap().clone()
}

#[test]
fn same_seed_same_history_same_hash() {
    let run = |seed| {
        let mut s = Sim::new(seed);
        populate(&mut s, 200, false);
        s.step_n(500);
        s.state_hash()
    };
    assert_eq!(run(7), run(7));
    assert_ne!(run(7), run(8));
}

#[test]
fn results_do_not_depend_on_storage_order() {
    let run = |reversed| {
        let mut s = Sim::new(3);
        populate(&mut s, 150, reversed);
        s.step_n(200);
        s.state_hash()
    };
    assert_eq!(run(false), run(true), "per-entity randomness must be keyed by uid, not by row");
}

#[test]
fn clone_is_a_fork_that_stays_in_lockstep() {
    let mut a = Sim::new(11);
    populate(&mut a, 100, false);
    a.step_n(100);
    let mut b = a.clone();
    a.step_n(150);
    b.step_n(150);
    assert_eq!(a.state_hash(), b.state_hash());
    // a command on one fork changes only that fork
    b.apply(&Command::Despawn { uid: 5 }).unwrap();
    a.step_n(10);
    b.step_n(10);
    assert_ne!(a.state_hash(), b.state_hash());
    assert_eq!(a.world.len(), 100);
    assert_eq!(b.world.len(), 99);
}

#[test]
fn movement_integrates_position_and_rotation() {
    let mut s = Sim::new(1);
    s.apply(&Command::Spawn { uid: None, components: vec![("Transform".into(), j("{}")), ("Velocity".into(), j(r#"{"lin":[1,0,0],"ang":[0,0,3.14159265]}"#))] }).unwrap();
    s.step_n(60);
    let e = s.world.entity(1).unwrap();
    let t = *s.world.get::<components::Transform>(e).unwrap();
    assert!((t.pos[0] - 1.0).abs() < 1e-3, "{:?}", t.pos);
    // half a turn about z: quaternion ~ (0, 0, 1, 0), unit length
    assert!((t.rot[2].abs() - 1.0).abs() < 2e-2, "{:?}", t.rot);
    let len = (t.rot.iter().map(|x| x * x).sum::<f32>()).sqrt();
    assert!((len - 1.0).abs() < 1e-5);
}

#[test]
fn recording_replays_to_the_same_state_even_after_json_round_trip() {
    let mut s = Sim::new(21);
    populate(&mut s, 60, false);
    s.step_n(30);
    // churn so storage order differs from uid order before the recording starts
    for uid in [3u64, 17, 40] {
        s.apply(&Command::Despawn { uid }).unwrap();
    }
    s.start_recording().unwrap();
    s.step_n(20);
    s.apply(&Command::Spawn { uid: None, components: vec![("Transform".into(), j(r#"{"pos":[9,9,9]}"#))] }).unwrap();
    s.apply(&Command::Define { def: j(r#"{"name":"Mood","fields":[{"name":"level","type":"f32","default":0.5}]}"#) }).unwrap();
    s.apply(&Command::Add { uid: 1, component: "Mood".into(), value: j(r#"{"level":0.9}"#) }).unwrap();
    s.step_n(25);
    s.apply(&Command::Set { uid: 2, component: "Velocity".into(), value: j(r#"{"lin":[5,5,5]}"#) }).unwrap();
    s.apply(&Command::Remove { uid: 1, component: "Wander".into() }).unwrap();
    s.step_n(15);
    let rec = s.stop_recording().unwrap();
    let text = rec.to_json().to_string_compact().unwrap();
    let rec2 = Recording::from_json(&parse(&text).unwrap()).unwrap();
    assert_eq!(rec2.final_hash, s.state_hash());
    let replayed = Sim::replay(&Sim::new(0), &rec2).unwrap();
    assert_eq!(replayed.tick, s.tick);
    assert_eq!(replayed.state_hash(), s.state_hash());
}

#[test]
fn every_command_has_a_working_inverse() {
    let mut s = Sim::new(5);
    populate(&mut s, 10, false);
    let cmds = vec![
        Command::Spawn { uid: None, components: vec![("Transform".into(), j(r#"{"pos":[1,2,3]}"#)), ("Name".into(), j(r#"{"value":"new"}"#))] },
        Command::Add { uid: 2, component: "Name".into(), value: j(r#"{"value":"two"}"#) },
        Command::Add { uid: 2, component: "Name".into(), value: j(r#"{"value":"again"}"#) },
        Command::Set { uid: 3, component: "Transform".into(), value: j(r#"{"scale":[2,2,2]}"#) },
        Command::Remove { uid: 4, component: "Wander".into() },
        Command::Despawn { uid: 5 },
    ];
    for c in cmds {
        let before = entities_json(&s);
        let applied = s.apply(&c).unwrap();
        assert_ne!(entities_json(&s), before, "{c:?} changed nothing");
        for inv in applied.inverse.unwrap() {
            s.apply(&inv).unwrap();
        }
        assert_eq!(entities_json(&s), before, "undoing {c:?} did not restore the entities");
    }
}

#[test]
fn batches_are_atomic() {
    let mut s = Sim::new(5);
    populate(&mut s, 5, false);
    let before = entities_json(&s);
    let batch = vec![
        Command::Set { uid: 1, component: "Transform".into(), value: j(r#"{"pos":[100,0,0]}"#) },
        Command::Despawn { uid: 2 },
        Command::Set { uid: 999, component: "Transform".into(), value: j("{}") }, // fails
    ];
    let err = apply_batch(&mut s.world, &mut s.scripts, &batch).unwrap_err();
    assert_eq!(err.code, "no_such_entity");
    assert!(err.message.starts_with("command 2 failed"));
    assert_eq!(entities_json(&s), before);
    let (done, inverse) = apply_batch(&mut s.world, &mut s.scripts, &batch[..2]).unwrap();
    assert_eq!(done.len(), 2);
    for c in inverse.unwrap() {
        apply(&mut s.world, &mut s.scripts, &c).unwrap();
    }
    assert_eq!(entities_json(&s), before);
}

#[test]
fn failed_spawn_leaves_no_entity_behind() {
    let mut s = Sim::new(5);
    let err = s.apply(&Command::Spawn { uid: None, components: vec![("Transform".into(), j(r#"{"pos":[1,2]}"#))] }).unwrap_err();
    assert_eq!(err.code, "wrong_length");
    assert_eq!(s.world.len(), 0);
    let err = s.apply(&Command::Spawn { uid: None, components: vec![("Transfrm".into(), j("{}"))] }).unwrap_err();
    assert_eq!(err.code, "unknown_component");
    assert!(err.hint.unwrap().contains("did you mean `Transform`"));
    assert_eq!(s.world.len(), 0);
}

#[test]
fn commands_round_trip_through_json() {
    let cmds = vec![
        Command::Spawn { uid: Some(4), components: vec![("Transform".into(), j(r#"{"pos":[1,2,3]}"#))] },
        Command::Spawn { uid: None, components: vec![] },
        Command::Despawn { uid: 4 },
        Command::Add { uid: 1, component: "Name".into(), value: j(r#"{"value":"x"}"#) },
        Command::Set { uid: 1, component: "Name".into(), value: j(r#"{"value":"y"}"#) },
        Command::Remove { uid: 1, component: "Name".into() },
        Command::Define { def: j(r#"{"name":"A","fields":[]}"#) },
    ];
    for c in cmds {
        let text = c.to_json().to_string_compact().unwrap();
        assert_eq!(Command::from_json(&parse(&text).unwrap()).unwrap(), c, "{text}");
    }
    assert_eq!(Command::from_json(&j(r#"{"op":"explode"}"#)).unwrap_err().code, "bad_command");
    assert_eq!(Command::from_json(&j(r#"{"op":"set","uid":1}"#)).unwrap_err().code, "bad_command");
}

#[test]
fn recording_twice_or_stopping_without_start_is_an_error() {
    let mut s = Sim::new(1);
    assert_eq!(s.stop_recording().unwrap_err().code, "not_recording");
    s.start_recording().unwrap();
    assert_eq!(s.start_recording().unwrap_err().code, "already_recording");
}

#[test]
fn changing_an_animators_clip_fades_between_the_clips_and_cutting_is_a_choice() {
    use components::Animator;
    let mut s = Sim::new(1);
    let spawn_anim = |s: &mut Sim, uid: u64, fade: &str| {
        let Json::Obj(components) = lunaris_json::parse(&format!(r#"{{"Animator":{{"clip":"idle","fade":{fade}}}}}"#)).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    };
    spawn_anim(&mut s, 1, "0.5");
    spawn_anim(&mut s, 2, "0");
    s.step_n(10);
    let anim = |s: &Sim, uid: u64| *s.world.get::<Animator>(s.world.entity(uid).unwrap()).unwrap();
    assert_eq!(anim(&s, 1).blend, 1.0, "no fade at the start, and none without a change");
    for uid in [1, 2] {
        s.apply(&Command::Set { uid, component: "Animator".into(), value: lunaris_json::parse(r#"{"clip":"run","time":0}"#).unwrap() }).unwrap();
    }
    s.step();
    let a = anim(&s, 1);
    assert!(a.blend > 0.0 && a.blend < 0.1, "the fade has begun: {}", a.blend);
    assert!((a.prev_time - 11.0 / 60.0).abs() < 1e-3, "the old clip kept its own time ({}), not the reset one", a.prev_time);
    assert_eq!(anim(&s, 2).blend, 1.0, "fade 0 cuts");
    s.step_n(14);
    assert!((anim(&s, 1).blend - 15.0 / 30.0).abs() < 0.02, "half way after 15 of 30 ticks: {}", anim(&s, 1).blend);
    s.step_n(20);
    assert_eq!(anim(&s, 1).blend, 1.0, "the fade is over");
    // the state hash includes the fade, so recordings replay it exactly
    assert_ne!(Sim::new(1).state_hash(), s.state_hash());
}

#[test]
fn a_checkpoint_rewinds_exactly_and_the_future_repeats() {
    // physics and wandering use the random generator, the input resource changes the course: a rewind must bring back all of it
    let mut s = Sim::new(9);
    populate(&mut s, 40, false);
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"steer","each":["Velocity","Wander"],"do":[{"set":"Velocity.lin[1]","to":{"mul":[2,{"res":"input.d"}]}}]}"#) }).unwrap();
    s.step_n(20);
    let c = s.checkpoint();
    assert_eq!(c.tick(), 20);
    let h20 = s.state_hash();
    let press = Command::SetResource { name: "input.d".into(), value: Some(1.0) };
    s.apply(&press).unwrap();
    s.step_n(30);
    let h50 = s.state_hash();
    assert_ne!(h20, h50);
    // back to tick 20: the hash is the one it was, resources and random generator included
    s.rewind_to(&c).unwrap();
    assert_eq!(s.tick, 20);
    assert_eq!(s.state_hash(), h20);
    // the same inputs again give the same future...
    s.apply(&press).unwrap();
    s.step_n(30);
    assert_eq!(s.state_hash(), h50);
    // ...and different inputs give a different one (the rewind did not freeze anything)
    s.rewind_to(&c).unwrap();
    s.step_n(30);
    assert_ne!(s.state_hash(), h50);
    // the checkpoint can be used again and again
    s.rewind_to(&c).unwrap();
    assert_eq!(s.state_hash(), h20);
}

#[test]
fn a_rewind_forgets_the_events_of_the_ticks_it_takes_back_and_refuses_changed_systems() {
    let mut s = Sim::new(2);
    populate(&mut s, 5, false);
    s.step_n(10);
    let c = s.checkpoint();
    let ev = |tick| Event { tick, name: "boom".into(), uid: 1, pos: None };
    s.events.extend([ev(3), ev(10), ev(12)]);
    s.step_n(5);
    s.rewind_to(&c).unwrap();
    assert_eq!(s.events.iter().map(|e| e.tick).collect::<Vec<_>>(), vec![3], "events before the checkpoint stay, later ones are emitted again by the replay");
    // a system added after the checkpoint is something a checkpoint cannot undo: it says so and changes nothing
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"late","each":["Velocity"],"do":[{"set":"Velocity.lin[2]","to":{"mul":[1,{"res":"input.d"}]}}]}"#) }).unwrap();
    let before = s.state_hash();
    let err = s.rewind_to(&c).unwrap_err();
    assert_eq!(err.code, "rewind_unsupported");
    assert_eq!(s.state_hash(), before);
}
