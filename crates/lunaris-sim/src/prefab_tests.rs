use super::*;
use crate::prefab::{plan, spawn_commands};
use components::Transform;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

const TREE: &str = r#"{
  "components": {"Name": {"value": "Tree"}, "Transform": {"pos": [0, 0, 0]}},
  "children": [
    {"name": "Crown", "components": {"LocalTransform": {"pos": [0, 2, 0]}, "MeshRenderer": {"mesh": "sphere"}}},
    {"name": "Trunk", "components": {"LocalTransform": {"pos": [0, 1, 0], "scale": [0.2, 2, 0.2]}}}
  ]
}"#;

fn define(s: &mut Sim, name: &str, def: &str) {
    s.apply(&Command::DefinePrefab { name: name.into(), def: j(def) }).unwrap();
}

fn spawn_prefab(s: &mut Sim, name: &str, overrides: &[(&str, &str)]) -> Vec<u64> {
    let over: Vec<(String, Json)> = overrides.iter().map(|(c, v)| (c.to_string(), j(v))).collect();
    let p = plan(&s.scripts.prefabs, name, &over).unwrap();
    let base = s.world.next_uid();
    let cmds = spawn_commands(&p, base);
    s.apply_batch(&cmds).unwrap();
    (0..p.len() as u64).map(|i| base + i).collect()
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

#[test]
fn a_prefab_spawns_its_children_attached_and_in_place() {
    let mut s = Sim::new(1);
    define(&mut s, "Tree", TREE);
    let uids = spawn_prefab(&mut s, "Tree", &[("Transform", r#"{"pos": [10, 0, 5]}"#)]);
    assert_eq!(uids.len(), 3);
    // the children already stand next to their parent (no tick needed), and follow it afterwards
    assert_eq!(pos(&s, uids[0]), [10.0, 0.0, 5.0]);
    assert_eq!(pos(&s, uids[1]), [10.0, 2.0, 5.0], "crown");
    assert_eq!(pos(&s, uids[2]), [10.0, 1.0, 5.0], "trunk");
    s.step();
    assert_eq!(pos(&s, uids[1]), [10.0, 2.0, 5.0]);
    let root = s.world.entity(uids[0]).unwrap();
    s.world.get_mut::<Transform>(root).unwrap().pos = [0.0, 0.0, 0.0];
    s.step();
    assert_eq!(pos(&s, uids[1]), [0.0, 2.0, 0.0], "the crown went with the tree");
    // names came from the `name` shorthand
    let crown = s.world.entity(uids[1]).unwrap();
    assert_eq!(s.world.get_json(crown, s.world.component_id("Name").unwrap()).unwrap().get("value").and_then(|v| v.as_str()), Some("Crown"));
}

#[test]
fn prefabs_inherit_and_overrides_replace_single_fields() {
    let mut s = Sim::new(1);
    define(&mut s, "Tree", TREE);
    define(&mut s, "Pine", r#"{"prefab": "Tree", "components": {"Name": {"value": "Pine"}}, "children": [{"name": "Tip", "components": {"LocalTransform": {"pos": [0, 4, 0]}}}]}"#);
    let p = plan(&s.scripts.prefabs, "Pine", &[("Transform".to_string(), j(r#"{"pos": [1, 2, 3]}"#))]).unwrap();
    assert_eq!(p.len(), 4, "the inherited crown and trunk plus the new tip");
    let root: std::collections::BTreeMap<&str, &Json> = p[0].components.iter().map(|(n, v)| (n.as_str(), v)).collect();
    assert_eq!(root["Name"].get("value").and_then(|v| v.as_str()), Some("Pine"), "own component replaces the inherited one");
    assert_eq!(root["Transform"].get("pos").map(|v| v.to_string_lossy()), Some("[1,2,3]".to_string()), "override replaces the field");
    assert_eq!(p[3].parent, Some(0), "children attach to the root");
}

#[test]
fn bad_prefabs_are_refused_with_the_reason_and_leave_nothing_behind() {
    let mut s = Sim::new(1);
    let err = s.apply(&Command::DefinePrefab { name: "A".into(), def: j(r#"{"components": {"Nope": {}}}"#) }).unwrap_err();
    assert!(err.message.contains("Nope"), "{}", err.message);
    let err = s.apply(&Command::DefinePrefab { name: "A".into(), def: j(r#"{"children": [{"prefab": "Missing"}]}"#) }).unwrap_err();
    assert_eq!(err.code, "no_such_prefab", "{}", err.message);
    let err = s.apply(&Command::DefinePrefab { name: "A".into(), def: j(r#"{"children": [{"prefab": "A"}]}"#) }).unwrap_err();
    assert!(err.code == "no_such_prefab" || err.code == "prefab_too_deep", "{err:?}");
    // a prefab that contains itself, defined in two steps, is stopped by the depth limit when it would be spawned or defined
    define(&mut s, "A", r#"{"children": [{"name": "leaf"}]}"#);
    let err = s.apply(&Command::DefinePrefab { name: "A".into(), def: j(r#"{"children": [{"prefab": "A"}]}"#) }).unwrap_err();
    assert_eq!(err.code, "prefab_too_deep", "{err:?}");
    assert!(s.scripts.prefabs.contains_key("A") && s.scripts.prefabs["A"].to_string_lossy().contains("leaf"), "the old definition is untouched");
    assert_eq!(s.apply(&Command::RemovePrefab { name: "B".into() }).unwrap_err().code, "no_such_prefab");
    assert!(s.apply(&Command::DefinePrefab { name: "".into(), def: j("{}") }).is_err());
}

#[test]
fn defining_and_removing_prefabs_undo_and_are_part_of_the_state_hash() {
    let mut s = Sim::new(1);
    let h0 = s.state_hash();
    let applied = s.apply(&Command::DefinePrefab { name: "Tree".into(), def: j(TREE) }).unwrap();
    let h1 = s.state_hash();
    assert_ne!(h0, h1);
    s.apply(&applied.inverse.unwrap()[0]).unwrap();
    assert_eq!(s.state_hash(), h0, "undo restores the state exactly");
    let mut again = Sim::new(1);
    again.apply(&Command::DefinePrefab { name: "Tree".into(), def: j(TREE) }).unwrap();
    assert_eq!(again.state_hash(), h1);
}

#[test]
fn scenes_carry_prefabs_and_instances_and_a_saved_scene_has_plain_entities() {
    let scene = j(&format!(r#"{{"lunaris": 1, "prefabs": {{"Tree": {TREE}}}, "instances": [{{"prefab": "Tree", "at": [4, 0, 4]}}, {{"prefab": "Tree", "at": [-4, 0, 4], "overrides": {{"Name": {{"value": "Other"}}}}}}], "entities": []}}"#));
    let mut s = Sim::new(1);
    let created = s.load_scene(&scene).unwrap();
    assert_eq!(created.len(), 6);
    assert_eq!(s.world.len(), 6);
    assert_eq!(pos(&s, created[3]), [-4.0, 0.0, 4.0]);
    let saved = s.scene_json();
    assert!(saved.get("prefabs").and_then(|p| p.get("Tree")).is_some(), "prefabs are saved");
    assert!(saved.get("instances").is_none(), "the entities themselves are saved");
    let mut t = Sim::new(1);
    t.load_scene(&saved).unwrap();
    assert_eq!(t.state_hash(), s.state_hash(), "the saved scene loads to the same state");
}

#[test]
fn a_graph_system_spawns_prefabs_and_the_run_is_deterministic_and_replays() {
    let ball = r#"{"components": {"Name": {"value": "Ball"}, "Transform": {"pos": [0, 0, 0]}, "Velocity": {}, "Ball": {}}}"#;
    let spawner = r#"{"name": "spawner", "each": ["Spawner"], "do": [
        {"if": {"eq": [{"mod": [{"tick": true}, 10]}, 0]}, "then": [
            {"spawn_prefab": "Ball", "at": [{"mul": [{"rand": 1}, 10]}, 5, 0], "velocity": [0, -1, 0], "with": {"Ball": {"age": 0}}}]}]}"#;
    let drop = r#"{"name": "drop", "each": ["Ball", "Transform"], "do": [{"if": {"lt": [{"get": "Transform.pos[1]"}, 2]}, "then": [{"despawn": true}]}]}"#;
    let run = |threads: usize| {
        let mut s = Sim::new(9);
        s.threads = threads;
        s.apply(&Command::Define { def: j(r#"{"name": "Spawner", "fields": []}"#) }).unwrap();
        s.apply(&Command::Define { def: j(r#"{"name": "Ball", "fields": [{"name": "age", "type": "f32", "default": 0}]}"#) }).unwrap();
        define(&mut s, "Ball", ball);
        s.apply(&Command::Spawn { uid: Some(1), components: vec![("Spawner".into(), j("{}"))] }).unwrap();
        s.apply(&Command::DefineSystem { def: j(spawner) }).unwrap();
        s.apply(&Command::DefineSystem { def: j(drop) }).unwrap();
        s.start_recording().unwrap();
        s.step_n(200);
        (s.world.len(), s.state_hash(), s.scripts.get("spawner").and_then(|x| x.last_error.clone()), s)
    };
    let (n1, h1, err, mut s1) = run(1);
    assert!(err.is_none(), "{err:?}");
    assert!(n1 > 5, "balls were spawned and some despawned ({n1} entities)");
    let (n8, h8, _, _) = run(8);
    assert_eq!((n1, h1), (n8, h8), "thread count must not matter");
    // the spawned entities got random positions from the engine's generator
    let xs: Vec<f32> = (1..s1.world.next_uid()).filter_map(|u| s1.world.entity(u)).filter_map(|e| s1.world.get::<Transform>(e).map(|t| t.pos[0])).collect();
    assert!(xs.windows(2).any(|w| w[0] != w[1]), "different balls start at different x: {xs:?}");
    // recorded and replayed without the spawner re-deciding anything: the same state
    let rec = s1.stop_recording().unwrap();
    let mut template = Sim::new(9);
    template.scripts = s1.scripts.clone();
    let replayed = Sim::replay(&template, &rec).unwrap();
    assert_eq!(replayed.state_hash(), rec.final_hash);
}

#[test]
fn a_missing_prefab_in_a_graph_system_is_the_systems_error_not_a_crash() {
    let mut s = Sim::new(1);
    s.apply(&Command::Define { def: j(r#"{"name": "Spawner", "fields": []}"#) }).unwrap();
    s.apply(&Command::Spawn { uid: Some(1), components: vec![("Spawner".into(), j("{}"))] }).unwrap();
    s.apply(&Command::DefineSystem { def: j(r#"{"name": "bad", "each": ["Spawner"], "do": [{"spawn_prefab": "Nope"}]}"#) }).unwrap();
    s.step();
    let e = s.scripts.get("bad").unwrap().last_error.clone().unwrap();
    assert!(e.contains("Nope"), "{e}");
    assert_eq!(s.world.len(), 1);
    // and a bad statement is refused when the system is defined
    let err = s.apply(&Command::DefineSystem { def: j(r#"{"name": "bad2", "each": ["Spawner"], "do": [{"spawn_prefab": "X", "at": [1, 2]}]}"#) }).unwrap_err();
    assert!(err.message.contains("three values"), "{}", err.message);
}

#[test]
fn spawn_prefab_with_can_set_array_fields_from_expressions() {
    let mut s = Sim::new(1);
    s.apply(&Command::Define { def: j(r#"{"name": "Spawner", "fields": [{"name": "k", "type": "f32", "default": 0}]}"#) }).unwrap();
    define(&mut s, "Mark", r#"{"components": {"Name": {"value": "Mark"}, "Transform": {}, "Decal": {}}}"#);
    s.apply(&Command::Spawn { uid: Some(1), components: vec![("Spawner".into(), j(r#"{"k": 0.5}"#))] }).unwrap();
    // an array of numbers, one of them an expression, in a component of the prefab's root: `Decal.normal` and `Transform.rot` (a four-element array) and a colour
    let def = r#"{"name": "marker", "each": ["Spawner"], "do": [
        {"if": {"eq": [{"tick": true}, 2]}, "then": [
            {"spawn_prefab": "Mark", "at": [1, 2, 3], "with": {"Decal": {"normal": [0, {"get": "Spawner.k"}, 1], "size": [2, 3], "life": 7}, "Transform": {"rot": [0, 1, 0, 0]}}}]}]}"#;
    s.apply(&Command::DefineSystem { def: j(def) }).unwrap();
    s.step_n(5);
    assert!(s.scripts.get("marker").unwrap().last_error.is_none(), "{:?}", s.scripts.get("marker").unwrap().last_error);
    let e = s.world.entity(2).expect("the prefab was spawned");
    let d = *s.world.get::<crate::components::Decal>(e).unwrap();
    assert_eq!((d.normal, d.size, d.life), ([0.0, 0.5, 1.0], [2.0, 3.0], 7.0), "{d:?}");
    let t = *s.world.get::<Transform>(e).unwrap();
    assert_eq!((t.rot, t.pos), ([0.0, 1.0, 0.0, 0.0], [1.0, 2.0, 3.0]), "{t:?}");
    // a scalar `with` field is as before, and the second spawn is an equal one (the run is deterministic: same hash from the same inputs)
    let mut u = Sim::new(1);
    u.apply(&Command::Define { def: j(r#"{"name": "Spawner", "fields": [{"name": "k", "type": "f32", "default": 0}]}"#) }).unwrap();
    define(&mut u, "Mark", r#"{"components": {"Name": {"value": "Mark"}, "Transform": {}, "Decal": {}}}"#);
    u.apply(&Command::Spawn { uid: Some(1), components: vec![("Spawner".into(), j(r#"{"k": 0.5}"#))] }).unwrap();
    u.apply(&Command::DefineSystem { def: j(def) }).unwrap();
    u.step_n(5);
    assert_eq!(u.state_hash(), s.state_hash());
}

#[test]
fn an_array_field_that_is_too_long_or_holds_something_that_is_not_a_value_is_refused_when_the_system_is_defined() {
    let mut s = Sim::new(1);
    s.apply(&Command::Define { def: j(r#"{"name": "Spawner", "fields": []}"#) }).unwrap();
    define(&mut s, "Mark", r#"{"components": {"Name": {"value": "Mark"}, "Transform": {}, "Decal": {}}}"#);
    let long = format!("[{}]", vec!["1"; 65].join(","));
    let err = s.apply(&Command::DefineSystem { def: j(&format!(r#"{{"name": "a", "each": ["Spawner"], "do": [{{"spawn_prefab": "Mark", "with": {{"Decal": {{"normal": {long}}}}}}}]}}"#)) }).unwrap_err();
    assert!(err.message.contains("at most 64"), "{}", err.message);
    // an array inside an array is not a value, and a plain `spawn` still takes no array (it is run by the batch interpreter)
    let err = s.apply(&Command::DefineSystem { def: j(r#"{"name": "b", "each": ["Spawner"], "do": [{"spawn_prefab": "Mark", "with": {"Decal": {"normal": [[0, 1], 0]}}}]}"#) }).unwrap_err();
    assert!(err.message.contains("array is not a value"), "{}", err.message);
    let err = s.apply(&Command::DefineSystem { def: j(r#"{"name": "c", "each": ["Spawner"], "do": [{"spawn": {"Decal": {"normal": [0, 1, 0]}}}]}"#) }).unwrap_err();
    assert!(err.message.contains("array is not a value"), "{}", err.message);
}
