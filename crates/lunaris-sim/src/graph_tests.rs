use super::*;
use crate::graph::compile;
use lunaris_ecs::{parse_component_def, World};
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

const GRAVITY: &str = r#"{
  "name": "gravity", "each": ["Transform", "Velocity"],
  "do": [
    { "set": "Velocity.lin[1]", "to": { "sub": [ { "get": "Velocity.lin[1]" }, { "mul": [9.8, { "dt": true }] } ] } },
    { "if": { "lt": [ { "get": "Transform.pos[1]" }, 0 ] },
      "then": [ { "set": "Transform.pos[1]", "to": 0 },
                { "set": "Velocity.lin[1]", "to": { "mul": [ { "neg": { "get": "Velocity.lin[1]" } }, 0.8 ] } } ] } ] }"#;

fn scripts_hash(s: &Sim) -> u64 {
    let mut h = lunaris_ecs::StateHasher::default();
    s.scripts.hash_into(&mut h);
    h.finish()
}

fn ball_sim(uid: u64) -> Sim {
    let mut s = Sim::new(1);
    s.apply(&Command::Spawn { uid: Some(uid), components: vec![("Transform".into(), j(r#"{"pos":[0,5,0]}"#)), ("Velocity".into(), j("{}"))] }).unwrap();
    s.apply(&Command::DefineSystem { def: j(GRAVITY) }).unwrap();
    s
}

#[test]
fn a_graph_system_matches_a_hand_written_reference_exactly() {
    let mut s = ball_sim(1);
    s.step_n(600);
    let e = s.world.entity(1).unwrap();
    let t = *s.world.get::<components::Transform>(e).unwrap();
    let v = *s.world.get::<components::Velocity>(e).unwrap();

    // the same arithmetic written out by hand: native movement in f32, then the graph in f64 stored back as f32
    let dt = 1.0f32 / 60.0;
    let (mut py, mut vy) = (5.0f32, 0.0f32);
    for _ in 0..600 {
        py += vy * dt; // movement system (linear part; angular velocity is zero)
        vy = (vy as f64 - 9.8f64 * dt as f64) as f32;
        if (py as f64) < 0.0 {
            py = 0.0;
            vy = (-(vy as f64) * 0.8) as f32;
        }
    }
    assert_eq!(t.pos[1].to_bits(), py.to_bits(), "{} vs {}", t.pos[1], py);
    assert_eq!(v.lin[1].to_bits(), vy.to_bits());
    assert!(t.pos[1] >= 0.0 && t.pos[1] < 5.0);
}

#[test]
fn graph_systems_are_deterministic_and_independent_of_storage_order() {
    let build = |reverse: bool| {
        let mut s = Sim::new(9);
        s.apply(&Command::Define { def: j(r#"{"name":"Age","fields":[{"name":"t","type":"f32"}]}"#) }).unwrap();
        s.apply(&Command::Define { def: j(r#"{"name":"Spawner","fields":[{"name":"every","type":"u32","default":10}]}"#) }).unwrap();
        s.apply(&Command::DefineSystem { def: j(r#"{
          "name": "age_and_die", "order": 10, "each": ["Age"],
          "do": [ { "set": "Age.t", "to": { "add": [ { "get": "Age.t" }, { "dt": true } ] } },
                  { "if": { "gt": [ { "get": "Age.t" }, { "add": [ 0.5, { "mul": [ { "rand": 1 }, 0.5 ] } ] } ] }, "then": [ { "despawn": true } ] } ] }"#) }).unwrap();
        s.apply(&Command::DefineSystem { def: j(r#"{
          "name": "spawner", "order": 20, "each": ["Spawner", "Transform"],
          "do": [ { "if": { "eq": [ { "mod": [ { "tick": true }, { "get": "Spawner.every" } ] }, 0 ] },
                   "then": [ { "spawn": { "Age": { "t": 0 } } } ] } ] }"#) }).unwrap();
        let mut uids: Vec<u64> = (1..=8).collect();
        if reverse {
            uids.reverse();
        }
        for uid in uids {
            s.apply(&Command::Spawn { uid: Some(uid), components: vec![("Transform".into(), j("{}")), ("Spawner".into(), j(&format!(r#"{{"every": {}}}"#, 5 + uid % 4)))] }).unwrap();
        }
        s.step_n(300);
        s
    };
    let (a, b, c) = (build(false), build(false), build(true));
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(a.state_hash(), c.state_hash(), "spawn order of the spawners must not matter");
    let age = a.world.component_id("Age").unwrap();
    let mut alive = 0;
    for e in a.world.entities_by_uid() {
        if a.world.has(e, age) {
            alive += 1;
        }
    }
    assert!(alive > 0 && alive < 200, "entities should be created and destroyed continuously, {alive} alive");
    assert!(a.world.next_uid() > 100, "many entities were spawned over time");
    assert!(a.scripts.get("spawner").unwrap().last_error.is_none(), "{:?}", a.scripts.get("spawner").unwrap().last_error);
}

#[test]
fn bad_definitions_explain_themselves() {
    let mut w = World::new();
    crate::register_standard_components(&mut w);
    let cases = [
        (r#"{"name":"a","each":["Transform"],"do":[{"set":"Transform.pos","to":1}]}"#, "array of 3"),
        (r#"{"name":"a","each":["Transform"],"do":[{"set":"Velocity.lin[0]","to":1}]}"#, "not in this system's `each`"),
        (r#"{"name":"a","each":["Transform"],"do":[{"set":"Transform.pos[7]","to":1}]}"#, "out of range"),
        (r#"{"name":"a","each":["Transform"],"do":[{"set":"Transform.posx[0]","to":1}]}"#, "no field `posx`"),
        (r#"{"name":"a","each":["Transform"],"do":[{"set":"Transform.pos[0]","to":{"addd":[1,2]}}]}"#, "unknown operation `addd`"),
        (r#"{"name":"a","each":["Transform"],"do":[{"set":"Transform.pos[0]","to":{"sub":[1]}}]}"#, "exactly 2"),
        (r#"{"name":"a","each":["Transform"],"do":[{"set":"Transform.pos[0]","to":{"var":"nope"}}]}"#, "no variable named `nope`"),
        (r#"{"name":"a","each":["Nope"],"do":[]}"#, "no component named"),
        (r#"{"name":"a","each":[],"do":[]}"#, "`each` must list"),
        (r#"{"name":"a b","each":["Transform"],"do":[]}"#, "`name` must be"),
        (r#"{"name":"a","each":["Transform"],"do":[{"explode":true}]}"#, "unknown statement"),
        (r#"{"name":"a","each":["Transform"],"do":[],"extra":1}"#, "unknown key"),
        (r#"{"name":"a","each":["Transform"]}"#, "`do`"),
    ];
    for (src, want) in cases {
        let e = compile(&mut w, &j(src)).expect_err(src);
        let text = format!("{} {}", e.message, e.hint.clone().unwrap_or_default());
        assert!(text.contains(want), "`{src}` gave `{text}` (wanted `{want}`)");
        assert!(e.path.is_some() || e.code == "unknown_component", "{src}: {e:?}");
    }
}

#[test]
fn runtime_errors_are_recorded_and_do_not_stop_the_simulation() {
    let mut s = Sim::new(1);
    s.apply(&Command::Spawn { uid: Some(1), components: vec![("Transform".into(), j("{}")), ("Velocity".into(), j("{}"))] }).unwrap();
    s.apply(&Command::Spawn { uid: Some(2), components: vec![("Transform".into(), j("{}")), ("Velocity".into(), j(r#"{"lin":[1,0,0]}"#))] }).unwrap();
    // divides by the x velocity: entity 1 has 0 and errors, entity 2 works
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"inv","each":["Transform","Velocity"],"do":[{"set":"Transform.pos[2]","to":{"div":[1,{"get":"Velocity.lin[0]"}]}}]}"#) }).unwrap();
    s.step_n(3);
    let sys = s.scripts.get("inv").unwrap();
    assert!(sys.last_error.as_ref().unwrap().contains("division by zero"), "{:?}", sys.last_error);
    assert!(sys.last_error.as_ref().unwrap().contains("entity 1"));
    let e2 = s.world.entity(2).unwrap();
    assert_eq!(s.world.get::<components::Transform>(e2).unwrap().pos[2], 1.0, "other entities still ran");
    // writing a boolean to a number field, and a number to a boolean field, are errors too
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"inv","each":["Transform"],"do":[{"set":"Transform.pos[0]","to":true}]}"#) }).unwrap();
    s.step();
    assert!(s.scripts.get("inv").unwrap().last_error.as_ref().unwrap().contains("expected a number"));
    // the simulation kept going
    assert_eq!(s.tick, 4);
}

#[test]
fn definition_commands_are_undoable_and_saved_with_the_scene() {
    let mut s = ball_sim(1);
    // replace, disable, remove: each inverse restores the previous state
    let before = scripts_hash(&s);
    for cmd in [
        Command::DefineSystem { def: j(r#"{"name":"gravity","each":["Transform"],"do":[]}"#) },
        Command::SetSystemEnabled { name: "gravity".into(), enabled: false },
        Command::RemoveSystem { name: "gravity".into() },
    ] {
        let applied = s.apply(&cmd).unwrap();
        for inv in applied.inverse.unwrap() {
            s.apply(&inv).unwrap();
        }
        assert_eq!(scripts_hash(&s), before, "{cmd:?} was not undone");
    }
    assert_eq!(s.apply(&Command::RemoveSystem { name: "nope".into() }).unwrap_err().code, "no_such_system");
    // scenes carry the systems and behave identically after a round trip through text
    s.step_n(120);
    let text = s.scene_json().to_string_pretty().unwrap();
    assert!(text.contains("\"systems\""));
    let mut loaded = Sim::new(1);
    loaded.load_scene(&parse(&text).unwrap()).unwrap();
    assert!(loaded.scripts.get("gravity").is_some());
    let mut fresh = ball_sim(1);
    s.step_n(120);
    fresh.step_n(240);
    // the loaded copy resumes from the saved positions: run it the same 120 ticks and compare with the original
    loaded.tick = 120;
    loaded.step_n(120);
    let y = |sim: &Sim| sim.world.get::<components::Transform>(sim.world.entity(1).unwrap()).unwrap().pos[1];
    assert_eq!(y(&loaded).to_bits(), y(&s).to_bits());
    assert_eq!(y(&fresh).to_bits(), y(&s).to_bits());
}

#[test]
fn recordings_include_systems_defined_during_the_recording() {
    let mut s = Sim::new(3);
    s.apply(&Command::Spawn { uid: Some(1), components: vec![("Transform".into(), j(r#"{"pos":[0,3,0]}"#)), ("Velocity".into(), j("{}"))] }).unwrap();
    s.step_n(10);
    s.start_recording().unwrap();
    s.step_n(20);
    s.apply(&Command::DefineSystem { def: j(GRAVITY) }).unwrap();
    s.step_n(100);
    s.apply(&Command::SetSystemEnabled { name: "gravity".into(), enabled: false }).unwrap();
    s.step_n(10);
    let rec = s.stop_recording().unwrap();
    let rec = Recording::from_json(&parse(&rec.to_json().to_string_compact().unwrap()).unwrap()).unwrap();
    let replayed = Sim::replay(&Sim::new(0), &rec).unwrap();
    assert_eq!(replayed.state_hash(), s.state_hash());
}

#[test]
fn structural_statements_work_add_remove_and_let() {
    let mut s = Sim::new(1);
    let (n, d, f) = parse_component_def(&j(r#"{"name":"Burning","fields":[{"name":"left","type":"f32","default":2}]}"#)).map(|x| (x.0, x.1, x.2)).unwrap();
    s.world.register_dynamic(&n, &d, &f).unwrap();
    s.apply(&Command::Spawn { uid: Some(1), components: vec![("Transform".into(), j("{}"))] }).unwrap();
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"ignite","each":["Transform"],"without":["Burning"],"do":[{"add":"Burning","values":{"left":1.5}}]}"#) }).unwrap();
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"burn","each":["Burning"],"do":[
        {"let":"left","be":{"sub":[{"get":"Burning.left"},{"dt":true}]}},
        {"set":"Burning.left","to":{"var":"left"}},
        {"if":{"le":[{"var":"left"},0]},"then":[{"remove":"Burning"}]}]}"#) }).unwrap();
    s.step();
    let burning = s.world.component_id("Burning").unwrap();
    assert!(s.world.has(s.world.entity(1).unwrap(), burning), "ignite added the component");
    s.step_n(60);
    // after ~1.5 s of burning it is removed, then ignite adds it again: it flips on and off
    let mut seen_off = false;
    let mut seen_on = false;
    for _ in 0..400 {
        s.step();
        if s.world.has(s.world.entity(1).unwrap(), burning) {
            seen_on = true;
        } else {
            seen_off = true;
        }
    }
    assert!(seen_on && seen_off);
    assert!(s.scripts.systems.iter().all(|x| x.last_error.is_none()), "{:?}", s.scripts.systems.iter().map(|x| x.last_error.clone()).collect::<Vec<_>>());
}

#[test]
fn graph_systems_scale_to_many_entities() {
    let mut s = Sim::new(1);
    for i in 0..100_000u64 {
        s.world.spawn_with2(components::Transform { pos: [i as f32, 0.0, 0.0], ..Default::default() }, components::Velocity { lin: [1.0, 0.0, 0.0], ang: [0.0; 3] }).unwrap();
    }
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"drift","each":["Transform","Velocity"],"do":[
        {"set":"Velocity.lin[1]","to":{"add":[{"get":"Velocity.lin[1]"},{"mul":[0.1,{"dt":true}]}]}},
        {"if":{"gt":[{"get":"Transform.pos[1]"},100]},"then":[{"set":"Transform.pos[1]","to":0}]}]}"#) }).unwrap();
    let t = std::time::Instant::now();
    s.step_n(20);
    let per_tick = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
    assert!(per_tick < 100.0, "100k entities took {per_tick:.1} ms per tick");
    assert!(s.scripts.get("drift").unwrap().last_error.is_none());
}


#[test]
fn results_do_not_depend_on_the_thread_count() {
    // enough entities for the parallel path (>= 4096 for native queries, >= 2048 for graphs)
    let build = |threads: usize| {
        let mut s = Sim::new(5);
        s.threads = threads;
        s.apply(&Command::Define { def: j(r#"{"name":"Age","fields":[{"name":"t","type":"f32"}]}"#) }).unwrap();
        for i in 0..6000u64 {
            let comps = vec![
                ("Transform".to_string(), j(&format!(r#"{{"pos":[{},0,0]}}"#, i % 97))),
                ("Velocity".to_string(), j(r#"{"lin":[1,0.5,0],"ang":[0,0,0.3]}"#)),
                ("Wander".to_string(), j(r#"{"strength":2}"#)),
                ("Age".to_string(), j("{}")),
            ];
            s.apply(&Command::Spawn { uid: Some(i + 1), components: comps }).unwrap();
        }
        s.apply(&Command::DefineSystem { def: j(r#"{"name":"age_and_split","each":["Age","Transform"],"do":[
            {"set":"Age.t","to":{"add":[{"get":"Age.t"},{"dt":true}]}},
            {"if":{"gt":[{"get":"Age.t"},{"add":[0.4,{"mul":[{"rand":3},0.3]}]}]},"then":[
                {"if":{"lt":[{"rand":4},0.5]},"then":[{"despawn":true}],"else":[{"set":"Age.t","to":0},{"spawn":{"Age":{"t":0}}}]}]}]}"#) }).unwrap();
        s.step_n(60);
        s
    };
    let base = build(1);
    assert!(base.world.len() != 6000, "the graph system should have created and destroyed entities");
    for t in [2usize, 3, 8] {
        let other = build(t);
        assert_eq!(base.state_hash(), other.state_hash(), "threads = {t} changed the result");
    }
}

#[test]
fn resources_are_read_written_and_independent_of_the_thread_count() {
    // 6000 entities each add to a shared total; a second system reads it. Results must not depend on threads or storage order.
    let build = |threads: usize, reverse: bool| {
        let mut s = Sim::new(3);
        s.threads = threads;
        let mut ids: Vec<u64> = (1..=6000).collect();
        if reverse {
            ids.reverse();
        }
        for i in ids {
            s.apply(&Command::Spawn { uid: Some(i), components: vec![("Transform".to_string(), j(&format!(r#"{{"pos":[{},0,0]}}"#, i % 13)))] }).unwrap();
        }
        s.apply(&Command::DefineSystem { def: j(r#"{"name":"count_x","each":["Transform"],"do":[
            {"res_add":"total_x","by":{"get":"Transform.pos[0]"}},
            {"res_add":"seen","by":1}]}"#) }).unwrap();
        s.apply(&Command::DefineSystem { def: j(r#"{"name":"mirror","order":200,"each":["Transform"],"without":["Velocity"],"do":[
            {"if":{"eq":[{"uid":true},1]},"then":[{"res_set":"last_total","to":{"res":"total_x"}}]}]}"#) }).unwrap();
        s.step_n(3);
        s
    };
    let a = build(1, false);
    assert_eq!(a.world.resource("seen"), Some(18000.0));
    let per_tick: f64 = (1..=6000u64).map(|i| (i % 13) as f64).sum();
    assert_eq!(a.world.resource("total_x"), Some(per_tick * 3.0));
    // "mirror" runs after "count_x" in the same tick, so it sees this tick's total
    assert_eq!(a.world.resource("last_total"), Some(per_tick * 3.0));
    for (threads, reverse) in [(2, false), (8, false), (4, true)] {
        let b = build(threads, reverse);
        assert_eq!(a.world.resources(), b.world.resources(), "threads {threads}, reversed {reverse}");
    }
}

#[test]
fn resources_are_part_of_the_state_the_scene_the_undo_stack_and_replays() {
    let mut s = Sim::new(1);
    let h0 = s.state_hash();
    s.apply(&Command::SetResource { name: "score".into(), value: Some(5.0) }).unwrap();
    assert_ne!(h0, s.state_hash(), "a resource changes the state hash");
    // a snapshot is a clone, so it carries them
    let snap = s.clone();
    s.apply(&Command::SetResource { name: "score".into(), value: Some(9.0) }).unwrap();
    assert_eq!(snap.world.resource("score"), Some(5.0));
    // saved in scenes, except live input
    s.apply(&Command::SetResource { name: "input.w".into(), value: Some(1.0) }).unwrap();
    let scene = s.scene_json();
    assert_eq!(scene.get("resources").and_then(|r| r.get("score")).and_then(|v| v.as_f64()), Some(9.0));
    assert!(scene.get("resources").and_then(|r| r.get("input.w")).is_none());
    let mut t = Sim::new(1);
    t.load_scene(&scene).unwrap();
    assert_eq!(t.world.resource("score"), Some(9.0));
    // inverse commands restore the previous value, and removal is a value too
    let applied = s.apply(&Command::SetResource { name: "score".into(), value: None }).unwrap();
    assert_eq!(s.world.resource("score"), None);
    for c in applied.inverse.unwrap() {
        s.apply(&c).unwrap();
    }
    assert_eq!(s.world.resource("score"), Some(9.0));
    // invalid names and values are refused with a hint
    assert_eq!(s.apply(&Command::SetResource { name: "bad name".into(), value: Some(1.0) }).unwrap_err().code, "bad_resource_name");
    assert_eq!(s.apply(&Command::SetResource { name: "x".into(), value: Some(f64::NAN) }).unwrap_err().code, "bad_resource_value");
    // JSON round trip of the command
    let c = Command::SetResource { name: "a.b".into(), value: Some(2.5) };
    assert_eq!(Command::from_json(&c.to_json()).unwrap(), c);
    assert_eq!(Command::from_json(&Command::SetResource { name: "a".into(), value: None }.to_json()).unwrap(), Command::SetResource { name: "a".into(), value: None });
}

#[test]
fn a_recording_replays_the_same_input_and_the_same_game() {
    // a graph system moves a body while the "input.right" resource is held; the recording must reproduce the run
    let mut s = Sim::new(2);
    s.apply(&Command::Spawn { uid: Some(1), components: vec![("Transform".to_string(), j("{}"))] }).unwrap();
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"walk","each":["Transform"],"do":[
        {"set":"Transform.pos[0]","to":{"add":[{"get":"Transform.pos[0]"},{"mul":[{"res":"input.right"},3,{"dt":true}]}]}},
        {"if":{"gt":[{"res":"input.right"},0.5]},"then":[{"res_add":"steps","by":1}]}]}"#) }).unwrap();
    s.start_recording().unwrap();
    s.step_n(10);
    s.apply(&Command::SetResource { name: "input.right".into(), value: Some(1.0) }).unwrap();
    s.step_n(30);
    s.apply(&Command::SetResource { name: "input.right".into(), value: Some(0.0) }).unwrap();
    s.step_n(5);
    let rec = s.stop_recording().unwrap();
    assert_eq!(s.world.resource("steps"), Some(30.0));
    let again = Sim::replay(&s, &rec).unwrap();
    assert_eq!(again.state_hash(), rec.final_hash);
    assert_eq!(again.world.resource("steps"), Some(30.0));
}

#[test]
fn bad_resource_uses_in_graphs_are_explained() {
    let mut s = Sim::new(1);
    let e = s.apply(&Command::DefineSystem { def: j(r#"{"name":"g","each":["Transform"],"do":[{"set":"Transform.pos[0]","to":{"res":"not valid"}}]}"#) }).unwrap_err();
    assert_eq!(e.code, "bad_graph");
    assert_eq!(e.path.as_deref(), Some("do[0].to.res"));
    let e = s.apply(&Command::DefineSystem { def: j(r#"{"name":"g","each":["Transform"],"do":[{"res_add":"score"}]}"#) }).unwrap_err();
    assert!(e.message.contains("by"), "{}", e.message);
    let e = s.apply(&Command::DefineSystem { def: j(r#"{"name":"g","each":["Transform"],"do":[{"nope":1}]}"#) }).unwrap_err();
    assert!(e.hint.unwrap().contains("res_add"));
}

// ---------------------------------------------------------------------------------------------------------------- contact systems
fn contact_world(spawn_order_reversed: bool) -> Sim {
    let mut s = Sim::new(4);
    s.apply(&Command::Define { def: j(r#"{"name":"Player","fields":[{"name":"hits","type":"i32"}]}"#) }).unwrap();
    s.apply(&Command::Define { def: j(r#"{"name":"Orb","fields":[{"name":"value","type":"f32","default":1}]}"#) }).unwrap();
    let mut items: Vec<(u64, String)> = vec![(1, r#"{"Transform":{"pos":[0,0,0]},"Player":{},"Collider":{"shape":0,"size":[2,2,2]},"Velocity":{},"RigidBody":{"gravity_scale":0}}"#.to_string())];
    // five orbs inside the player's reach, two far away
    for i in 0..5u64 {
        items.push((10 + i, format!(r#"{{"Transform":{{"pos":[{},0,0]}},"Orb":{{"value":{}}},"Collider":{{"shape":0,"size":[0.5,0.5,0.5],"trigger":true}}}}"#, 0.2 + i as f64 * 0.1, i + 1)));
    }
    for i in 0..2u64 {
        items.push((20 + i, format!(r#"{{"Transform":{{"pos":[{},0,0]}},"Orb":{{"value":100}},"Collider":{{"shape":0,"size":[0.5,0.5,0.5],"trigger":true}}}}"#, 30 + i * 3)));
    }
    if spawn_order_reversed {
        items.reverse();
    }
    for (uid, comps) in items {
        let Json::Obj(components) = j(&comps) else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    }
    s
}

const COLLECT: &str = r#"{"name":"collect","on_contact":{"a":["Player"],"b":["Orb"]},"do":[
    {"res_add":"score","by":{"get":"b.Orb.value"}},
    {"set":"a.Player.hits","to":{"add":[{"get":"a.Player.hits"},1]}},
    {"despawn":"b"}]}"#;

#[test]
fn a_contact_system_reacts_to_each_touching_pair_and_can_change_both_entities() {
    let mut s = contact_world(false);
    s.apply(&Command::DefineSystem { def: j(COLLECT) }).unwrap();
    s.step();
    assert_eq!(s.scripts.systems[0].last_error, None);
    assert_eq!(s.world.resource("score"), Some(1.0 + 2.0 + 3.0 + 4.0 + 5.0), "five touching orbs, scored by their value; the far ones are untouched");
    let hits = s.world.get_json(s.world.entity(1).unwrap(), s.world.component_id("Player").unwrap()).unwrap();
    assert_eq!(hits.get("hits").unwrap().as_i64(), Some(5));
    assert_eq!(s.world.len(), 3, "the player and the two far orbs remain; the five collected orbs were despawned");
    // next tick nothing touches any more: no new score
    s.step();
    assert_eq!(s.world.resource("score"), Some(15.0));
}

#[test]
fn contact_systems_do_not_depend_on_spawn_order_or_threads_and_replay() {
    let build = |reversed: bool, threads: usize| {
        let mut s = contact_world(reversed);
        s.threads = threads;
        s.apply(&Command::DefineSystem { def: j(COLLECT) }).unwrap();
        s.start_recording().unwrap();
        s.step_n(3);
        s
    };
    let a = build(false, 1);
    for (r, t) in [(true, 1), (false, 4), (true, 8)] {
        let b = build(r, t);
        assert_eq!(a.world.resources(), b.world.resources());
        assert_eq!(a.world.len(), b.world.len());
        assert_eq!(a.state_hash(), b.state_hash(), "reversed = {r}, threads = {t}");
    }
    let mut a = a;
    let rec = a.stop_recording().unwrap();
    let again = Sim::replay(&a, &rec).unwrap();
    assert_eq!(again.state_hash(), rec.final_hash);
}

#[test]
fn contact_roles_filters_and_properties() {
    // both entities can match both roles: the system then runs once for each orientation
    let mut s = Sim::new(1);
    s.apply(&Command::Define { def: j(r#"{"name":"Ball","fields":[{"name":"n","type":"i32"}]}"#) }).unwrap();
    for (uid, x) in [(1u64, 0.0), (2, 0.8)] {
        let Json::Obj(components) = j(&format!(r#"{{"Transform":{{"pos":[{x},0,0]}},"Ball":{{}},"Collider":{{"shape":0,"size":[1,1,1]}},"Velocity":{{}},"RigidBody":{{"gravity_scale":0}}}}"#)) else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    }
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"bump","on_contact":{"a":["Ball"],"b":["Ball"]},"do":[
        {"set":"a.Ball.n","to":{"add":[{"get":"a.Ball.n"},1]}},
        {"res_add":"normal_x","by":{"contact":"nx"}},
        {"res_add":"seen_uid_b","by":{"uid":"b"}},
        {"res_add":"depth","by":{"contact":"depth"}}]}"#) }).unwrap();
    s.step();
    assert_eq!(s.scripts.systems[0].last_error, None);
    let n = |uid: u64| s.world.get_json(s.world.entity(uid).unwrap(), s.world.component_id("Ball").unwrap()).unwrap().get("n").unwrap().as_i64().unwrap();
    assert_eq!((n(1), n(2)), (1, 1), "each ball was `a` once");
    assert_eq!(s.world.resource("normal_x"), Some(0.0), "the normal points a to b in one orientation and b to a in the other, so they cancel");
    assert_eq!(s.world.resource("seen_uid_b"), Some(3.0));
    assert!(s.world.resource("depth").unwrap() > 0.0);
    // trigger filters
    let mut t = contact_world(false);
    t.apply(&Command::DefineSystem { def: j(r#"{"name":"solid_only","on_contact":{"a":["Player"],"b":["Orb"],"triggers":"solid"},"do":[{"res_add":"n","by":1}]}"#) }).unwrap();
    t.apply(&Command::DefineSystem { def: j(r#"{"name":"trigger_only","on_contact":{"a":["Player"],"b":["Orb"],"triggers":"only"},"do":[{"res_add":"m","by":1}]}"#) }).unwrap();
    t.step();
    assert_eq!(t.world.resource("n"), None, "orbs are triggers, so a `solid` system never runs");
    assert_eq!(t.world.resource("m"), Some(5.0));
}

#[test]
fn contact_systems_explain_their_mistakes() {
    let mut s = contact_world(false);
    let err = |s: &mut Sim, def: &str| s.apply(&Command::DefineSystem { def: j(def) }).unwrap_err();
    let e = err(&mut s, r#"{"name":"x","on_contact":{"a":["Player"],"b":["Orb"]},"do":[{"set":"Player.hits","to":1}]}"#);
    assert!(e.message.contains("a.") && e.hint.unwrap().contains("a.Transform"), "paths need a role");
    let e = err(&mut s, r#"{"name":"x","on_contact":{"a":["Player"],"b":["Orb"]},"do":[{"set":"b.Player.hits","to":1}]}"#);
    assert!(e.message.contains("not in this system's `on_contact` list"));
    let e = err(&mut s, r#"{"name":"x","on_contact":{"a":["Player"],"b":["Orb"]},"do":[{"despawn":true}]}"#);
    assert!(e.message.contains("which entity"));
    let e = err(&mut s, r#"{"name":"x","each":["Orb"],"on_contact":{"a":["Player"],"b":["Orb"]},"do":[]}"#);
    assert!(e.message.contains("not both"));
    let e = err(&mut s, r#"{"name":"x","each":["Orb"],"do":[{"res_add":"n","by":{"contact":"depth"}}]}"#);
    assert!(e.message.contains("only be used in an on_contact"));
    let e = err(&mut s, r#"{"name":"x","each":["Orb"],"do":[{"remove":"Orb","on":"a"}]}"#);
    assert!(e.message.contains("only used in on_contact"));
    let e = err(&mut s, r#"{"name":"x","on_contact":{"a":["Player"],"b":["Orb"]},"do":[{"remove":"Orb"}]}"#);
    assert!(e.message.contains("which entity"));
    let e = err(&mut s, r#"{"name":"x","on_contact":{"a":["Player"],"b":["Orb"],"triggers":"sometimes"},"do":[]}"#);
    assert_eq!(e.path.as_deref(), Some("on_contact.triggers"));
    let e = err(&mut s, r#"{"name":"x","on_contact":{"a":["Player"]},"do":[]}"#);
    assert_eq!(e.path.as_deref(), Some("on_contact.b"));
    // add and remove on a chosen entity work
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"mark","on_contact":{"a":["Player"],"b":["Orb"]},"do":[{"add":"Wander","on":"b","values":{"strength":3}},{"remove":"Velocity","on":"a"}]}"#) }).unwrap();
    s.step();
    assert_eq!(s.scripts.systems[0].last_error, None, "{:?}", s.scripts.systems[0].last_error);
    assert!(s.world.get_json(s.world.entity(10).unwrap(), s.world.component_id("Wander").unwrap()).is_some(), "orb 10 got a Wander");
}

#[test]
fn queuing_the_same_change_twice_from_different_contacts_is_harmless() {
    // two players touch the same orb in the same tick: both ask for it to be despawned and for score; it is despawned once
    let mut s = Sim::new(1);
    s.apply(&Command::Define { def: j(r#"{"name":"Player","fields":[{"name":"n","type":"i32"}]}"#) }).unwrap();
    s.apply(&Command::Define { def: j(r#"{"name":"Orb","fields":[{"name":"n","type":"i32"}]}"#) }).unwrap();
    for (uid, comps) in [
        (1u64, r#"{"Transform":{"pos":[-0.4,0,0]},"Player":{},"Collider":{"size":[1,1,1]},"Velocity":{},"RigidBody":{"gravity_scale":0}}"#),
        (2, r#"{"Transform":{"pos":[0.4,0,0]},"Player":{},"Collider":{"size":[1,1,1]},"Velocity":{},"RigidBody":{"gravity_scale":0}}"#),
        (3, r#"{"Transform":{"pos":[0,0,0]},"Orb":{},"Collider":{"size":[0.5,0.5,0.5],"trigger":true}}"#),
    ] {
        let Json::Obj(components) = j(comps) else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    }
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"take","on_contact":{"a":["Player"],"b":["Orb"],"triggers":"only"},"do":[{"res_add":"taken","by":1},{"despawn":"b"},{"remove":"Orb","on":"b"}]}"#) }).unwrap();
    s.step();
    assert_eq!(s.world.resource("taken"), Some(2.0), "both players reacted");
    assert!(s.world.entity(3).is_none());
    assert_eq!(s.scripts.systems[0].last_error, None, "the second despawn of the same orb is skipped, not an error");
}

#[test]
fn emitted_events_are_ordered_outside_the_state_and_come_back_on_replay() {
    let build = |reversed: bool, threads: usize| {
        let mut s = Sim::new(2);
        s.threads = threads;
        let mut ids: Vec<u64> = (1..=3000).collect();
        if reversed {
            ids.reverse();
        }
        for i in ids {
            s.apply(&Command::Spawn { uid: Some(i), components: vec![("Transform".to_string(), j(&format!(r#"{{"pos":[{},0,0]}}"#, i % 7)))] }).unwrap();
        }
        s.apply(&Command::DefineSystem { def: j(r#"{"name":"pings","each":["Transform"],"do":[
            {"if":{"gt":[{"get":"Transform.pos[0]"},5.5]},"then":[{"emit":"far"}]},
            {"if":{"eq":[{"uid":true},7]},"then":[{"emit":"seven"}]}]}"#) }).unwrap();
        s.start_recording().unwrap();
        s.step_n(2);
        s
    };
    let a = build(false, 1);
    // uid 6, 13, ... have x = 6 (> 5.5): about 428 of them per tick, plus "seven"; ordered by uid within a tick
    let tick0: Vec<&Event> = a.events.iter().filter(|e| e.tick == 0).collect();
    assert_eq!(tick0.len(), (1..=3000u64).filter(|i| i % 7 == 6).count() + 1);
    let uids: Vec<u64> = tick0.iter().map(|e| e.uid).collect();
    let mut sorted = uids.clone();
    sorted.sort_unstable();
    assert_eq!(uids, sorted, "events of one tick come out in uid order, whatever the storage order or thread count");
    assert_eq!(tick0.iter().filter(|e| e.name == "seven").count(), 1);
    for (r, t) in [(true, 1), (false, 8), (true, 3)] {
        assert_eq!(build(r, t).events, a.events, "reversed {r}, threads {t}");
    }
    // events are not state: the hash of two worlds is the same whether or not anybody emitted
    let mut quiet = build(false, 1);
    let before = quiet.state_hash();
    quiet.events.clear();
    assert_eq!(before, quiet.state_hash());
    // a replay produces the same events again
    let mut a = a;
    let rec = a.stop_recording().unwrap();
    let again = Sim::replay(&a, &rec).unwrap();
    assert_eq!(again.events, a.events);
    // bad names are refused with a hint
    let e = a.apply(&Command::DefineSystem { def: j(r#"{"name":"bad","each":["Transform"],"do":[{"emit":"two words"}]}"#) }).unwrap_err();
    assert_eq!(e.code, "bad_graph");
    assert!(e.hint.unwrap().contains("coin"));
}

#[test]
fn an_event_remembers_where_it_was_emitted_even_when_the_emitter_is_gone_in_the_same_tick() {
    let mut s = Sim::new(3);
    s.apply(&Command::Spawn { uid: Some(1), components: vec![("Transform".to_string(), j(r#"{"pos":[4,1.5,-2]}"#)), ("Name".to_string(), j(r#"{"value":"a"}"#))] }).unwrap();
    s.apply(&Command::Spawn { uid: Some(2), components: vec![("Name".to_string(), j(r#"{"value":"b"}"#))] }).unwrap();
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"pick","each":["Name"],"do":[{"emit":"coin"},{"despawn":true}]}"#) }).unwrap();
    s.step();
    assert!(s.world.entity(1).is_none(), "the orb is gone");
    assert_eq!(s.events.len(), 2);
    assert_eq!((s.events[0].uid, s.events[0].pos), (1, Some([4.0, 1.5, -2.0])));
    assert_eq!((s.events[1].uid, s.events[1].pos), (2, None), "an entity without a Transform has no place");
}

#[test]
fn contact_systems_can_emit_too() {
    let mut s = contact_world(false);
    s.apply(&Command::DefineSystem { def: j(r#"{"name":"ding","on_contact":{"a":["Player"],"b":["Orb"]},"do":[{"emit":"coin"}]}"#) }).unwrap();
    s.step();
    let coins = s.events.iter().filter(|e| e.name == "coin").count();
    assert_eq!(coins, 5, "one per touching orb");
    assert!(s.events.iter().all(|e| e.uid == 1), "the issuing entity is the first of the pair");
}

/// Run `{"res_set":"out","to":expr}` once per entity, with and without the batch vm, and return the value or the error text.
fn math_result(expr: &str) -> Result<f64, String> {
    let run = |interpret: bool| {
        let mut s = Sim::new(1);
        s.apply(&Command::Spawn { uid: Some(1), components: vec![("Transform".into(), j("{}"))] }).unwrap();
        s.scripts.interpret_only = interpret;
        s.apply(&Command::DefineSystem { def: j(&format!(r#"{{"name":"m","each":["Transform"],"do":[{{"res_set":"out","to":{expr}}}]}}"#)) }).unwrap();
        s.step();
        let sys = s.scripts.get("m").unwrap();
        assert!(interpret || sys.uses_batch(), "{expr} should run on the batch vm");
        match &sys.last_error {
            Some(e) => Err(e.clone()),
            None => Ok(s.world.resource("out").unwrap()),
        }
    };
    let (vm, reference) = (run(false), run(true));
    assert_eq!(vm, reference, "{expr}: the vm and the interpreter must agree");
    vm
}

#[test]
fn the_math_library_gives_the_right_numbers_and_the_same_numbers_in_the_vm() {
    let close = |expr: &str, want: f64| {
        let got = math_result(expr).unwrap_or_else(|e| panic!("{expr} failed: {e}"));
        assert!((got - want).abs() <= 1e-12 * want.abs().max(1.0), "{expr} = {got}, expected {want}");
    };
    close(r#"{"sign":-3.5}"#, -1.0);
    close(r#"{"sign":0}"#, 0.0);
    close(r#"{"sign":9}"#, 1.0);
    close(r#"{"fract":2.75}"#, 0.75);
    close(r#"{"fract":-0.25}"#, 0.75);
    close(r#"{"trunc":-2.7}"#, -2.0);
    close(r#"{"trunc":2.7}"#, 2.0);
    close(r#"{"tan":0.7}"#, 0.7f64.tan());
    close(r#"{"atan":1}"#, std::f64::consts::FRAC_PI_4);
    close(r#"{"atan2":[1,1]}"#, std::f64::consts::FRAC_PI_4);
    close(r#"{"atan2":[1,-1]}"#, 3.0 * std::f64::consts::FRAC_PI_4);
    close(r#"{"atan2":[-1,-1]}"#, -3.0 * std::f64::consts::FRAC_PI_4);
    close(r#"{"atan2":[0,0]}"#, 0.0);
    close(r#"{"exp":1}"#, std::f64::consts::E);
    close(r#"{"ln":2.718281828459045}"#, 1.0);
    close(r#"{"ln":8}"#, 8f64.ln());
    close(r#"{"pow":[2,10]}"#, 1024.0);
    close(r#"{"pow":[2,-2]}"#, 0.25);
    close(r#"{"pow":[-2,3]}"#, -8.0);
    close(r#"{"pow":[-2,2]}"#, 4.0);
    close(r#"{"pow":[2,0.5]}"#, std::f64::consts::SQRT_2);
    close(r#"{"pow":[10,2.5]}"#, 10f64.powf(2.5));
    close(r#"{"pow":[0,3]}"#, 0.0);
    close(r#"{"pow":[7,0]}"#, 1.0);
    close(r#"{"hypot":[3,4]}"#, 5.0);
    close(r#"{"smoothstep":[0,1,0.5]}"#, 0.5);
    close(r#"{"smoothstep":[0,1,-3]}"#, 0.0);
    close(r#"{"smoothstep":[0,1,3]}"#, 1.0);
    close(r#"{"smoothstep":[10,20,12.5]}"#, 0.15625);
    // noise: the value at a point is the one `noise2` gives; a bad point is 0
    close(&format!(r#"{{"noise":[1.5,2.5,0]}}"#), crate::noise::noise2(1.5, 2.5, 0.0));
    close(r#"{"noise":[1e300,0,0]}"#, 0.0);
    // refusals say why, and the batch vm refuses in the same words
    for (expr, why) in [
        (r#"{"ln":0}"#, "ln of a number that is not positive"),
        (r#"{"ln":-1}"#, "ln of a number that is not positive"),
        (r#"{"pow":[0,-1]}"#, "zero to a negative power"),
        (r#"{"pow":[-8,0.5]}"#, "negative number to a fractional power"),
        (r#"{"pow":[10,400]}"#, "result is too large"),
        (r#"{"exp":1000}"#, "exp: the result is too large"),
        (r#"{"smoothstep":[1,1,0]}"#, "lo and hi are equal"),
    ] {
        let e = math_result(expr).unwrap_err();
        assert!(e.contains(why), "{expr}: {e}");
    }
    // they compose with the older operations
    close(r#"{"add":[{"mul":[2,{"atan2":[1,1]}]},{"pow":[2,3]}]}"#, std::f64::consts::FRAC_PI_2 + 8.0);
}
