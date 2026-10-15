use super::*;
use lunaris_json::{parse, Json};

component! {
    pub struct Pos { pub x: f32 = 0.0, pub y: f32 = 0.0, pub z: f32 = 0.0 }
}
component! {
    pub struct Vel { pub v: [f32; 3] = [0.0, 0.0, 0.0] }
}
component! {
    pub struct Mixed { pub flag: bool = true, pub big: u64 = 7, pub small: i32 = -3, pub name: Sym = Sym(0), pub target: Uid = Uid(0) }
}
component! {
    pub struct Marker { }
}

#[test]
fn spawn_insert_get_despawn() {
    let mut w = World::new();
    let a = w.spawn();
    let b = w.spawn();
    w.insert(a, Pos { x: 1.0, y: 2.0, z: 3.0 }).unwrap();
    w.insert(b, Pos { x: 4.0, ..Pos::default() }).unwrap();
    w.insert(a, Vel { v: [1.0, 0.0, 0.0] }).unwrap();
    assert_eq!(w.get::<Pos>(a).unwrap().y, 2.0);
    assert_eq!(w.get::<Vel>(a).unwrap().v[0], 1.0);
    assert!(w.get::<Vel>(b).is_none());
    assert_eq!(w.len(), 2);
    assert!(w.despawn(a));
    assert!(!w.despawn(a));
    assert!(!w.is_alive(a));
    assert_eq!(w.get::<Pos>(b).unwrap().x, 4.0);
    assert_eq!(w.len(), 1);
}

#[test]
fn moving_between_archetypes_keeps_other_entities_intact() {
    let mut w = World::new();
    let ents: Vec<Entity> = (0..50).map(|_| w.spawn()).collect();
    for (i, &e) in ents.iter().enumerate() {
        w.insert(e, Pos { x: i as f32, y: 0.0, z: 0.0 }).unwrap();
    }
    // add Vel to every third entity, remove Pos from every fifth
    for (i, &e) in ents.iter().enumerate() {
        if i % 3 == 0 {
            w.insert(e, Vel { v: [i as f32, 1.0, 2.0] }).unwrap();
        }
        if i % 5 == 0 {
            assert!(w.remove_typed::<Pos>(e).unwrap());
        }
    }
    for (i, &e) in ents.iter().enumerate() {
        match w.get::<Pos>(e) {
            Some(p) => assert_eq!(p.x, i as f32),
            None => assert_eq!(i % 5, 0),
        }
        match w.get::<Vel>(e) {
            Some(v) => assert_eq!(v.v[0], i as f32),
            None => assert_ne!(i % 3, 0),
        }
    }
}

#[test]
fn entity_slots_are_reused_with_new_generations() {
    let mut w = World::new();
    let a = w.spawn();
    w.despawn(a);
    let b = w.spawn();
    assert_eq!(a.index, b.index);
    assert_ne!(a.generation, b.generation);
    assert!(!w.is_alive(a));
    assert!(w.is_alive(b));
    assert!(w.get::<Pos>(a).is_none());
}

#[test]
fn uids_are_stable_and_unique() {
    let mut w = World::new();
    let a = w.spawn();
    let b = w.spawn_uid(100).unwrap();
    let c = w.spawn();
    assert_eq!(w.uid_of(a), Some(1));
    assert_eq!(w.uid_of(b), Some(100));
    assert_eq!(w.uid_of(c), Some(101));
    assert_eq!(w.entity(100), Some(b));
    assert!(w.spawn_uid(100).is_err());
    assert!(w.spawn_uid(0).is_err());
    w.despawn(b);
    assert_eq!(w.entity(100), None);
    assert!(w.spawn_uid(100).is_ok());
}

#[test]
fn queries_visit_exactly_the_matching_entities_and_can_mutate() {
    let mut w = World::new();
    for i in 0..1000 {
        let e = w.spawn();
        w.insert(e, Pos { x: i as f32, ..Pos::default() }).unwrap();
        if i % 2 == 0 {
            w.insert(e, Vel { v: [1.0, 2.0, 3.0] }).unwrap();
        }
    }
    let mut visited = 0;
    w.for_each::<(&mut Pos, &Vel), _>(|_, (p, v)| {
        p.x += v.v[0];
        p.y += v.v[1];
        visited += 1;
    })
    .unwrap();
    assert_eq!(visited, 500);
    let mut sum = 0.0f64;
    w.for_each::<(&Pos,), _>(|_, (p,)| sum += p.y as f64).unwrap();
    assert_eq!(sum, 1000.0);
}

#[test]
fn aliasing_queries_are_rejected() {
    let mut w = World::new();
    let e = w.spawn();
    w.insert(e, Pos::default()).unwrap();
    let err = w.for_each::<(&mut Pos, &Pos), _>(|_, _| {}).unwrap_err();
    assert_eq!(err.code, "aliasing_query");
}

#[test]
fn zero_sized_marker_components() {
    let mut w = World::new();
    let a = w.spawn();
    let b = w.spawn();
    w.insert(a, Marker {}).unwrap();
    w.insert(a, Pos { x: 5.0, ..Pos::default() }).unwrap();
    w.insert(b, Pos::default()).unwrap();
    let mut n = 0;
    w.for_each::<(&Marker, &Pos), _>(|e, (_, p)| {
        assert_eq!(e, a);
        assert_eq!(p.x, 5.0);
        n += 1;
    })
    .unwrap();
    assert_eq!(n, 1);
    assert!(w.remove_typed::<Marker>(a).unwrap());
    assert_eq!(w.get::<Pos>(a).unwrap().x, 5.0);
}

#[test]
fn json_round_trip_for_every_field_type() {
    let mut w = World::new();
    let cid = w.register_native::<Mixed>().unwrap();
    let e = w.spawn();
    let target = w.spawn();
    let tuid = w.uid_of(target).unwrap() as i64;
    w.insert_json(e, cid, &parse(&format!(r#"{{"flag": false, "big": 12345678901, "small": -9, "name": "hello", "target": {tuid}}}"#)).unwrap()).unwrap();
    let back = w.get_json(e, cid).unwrap();
    assert_eq!(back.get("flag"), Some(&Json::Bool(false)));
    assert_eq!(back.get("big"), Some(&Json::Int(12345678901)));
    assert_eq!(back.get("small"), Some(&Json::Int(-9)));
    assert_eq!(back.get("name"), Some(&Json::Str("hello".into())));
    assert_eq!(back.get("target"), Some(&Json::Int(tuid)));
    // partial merge keeps the other fields
    w.set_json(e, cid, &parse(r#"{"small": 4}"#).unwrap()).unwrap();
    let again = w.get_json(e, cid).unwrap();
    assert_eq!(again.get("small"), Some(&Json::Int(4)));
    assert_eq!(again.get("name"), Some(&Json::Str("hello".into())));
}

#[test]
fn defaults_apply_when_a_component_is_added_from_partial_json() {
    let mut w = World::new();
    let cid = w.register_native::<Mixed>().unwrap();
    let e = w.spawn();
    w.insert_json(e, cid, &parse(r#"{"small": 1}"#).unwrap()).unwrap();
    let m = w.get::<Mixed>(e).unwrap();
    assert!(m.flag);
    assert_eq!(m.big, 7);
    assert_eq!(m.small, 1);
}

#[test]
fn validation_errors_name_the_problem_and_help_fix_it() {
    let mut w = World::new();
    let cid = w.register_native::<Pos>().unwrap();
    let e = w.spawn();
    let unknown = w.insert_json(e, cid, &parse(r#"{"xx": 1}"#).unwrap()).unwrap_err();
    assert_eq!(unknown.code, "unknown_field");
    assert!(unknown.hint.as_deref().unwrap().contains("x: f32"));
    let wrong = w.insert_json(e, cid, &parse(r#"{"x": "no"}"#).unwrap()).unwrap_err();
    assert_eq!(wrong.code, "type_mismatch");
    assert_eq!(wrong.path.as_deref(), Some("Pos.x"));
    assert!(!w.has(e, cid), "a failed insert must not add the component");
    let vcid = w.register_native::<Vel>().unwrap();
    let len = w.insert_json(e, vcid, &parse(r#"{"v": [1, 2]}"#).unwrap()).unwrap_err();
    assert_eq!(len.code, "wrong_length");
    let range = w.insert_json(e, cid, &parse(r#"{"x": 1e300}"#).unwrap()).unwrap_err();
    assert_eq!(range.code, "out_of_range");
    // partial failure leaves the old value alone
    w.insert_json(e, cid, &parse(r#"{"x": 1}"#).unwrap()).unwrap();
    assert!(w.set_json(e, cid, &parse(r#"{"y": 2, "z": "bad"}"#).unwrap()).is_err());
    assert_eq!(w.get::<Pos>(e).unwrap().y, 0.0);
}

#[test]
fn unknown_component_suggests_the_closest_name() {
    let mut w = World::new();
    w.register_native::<Pos>().unwrap();
    w.register_native::<Vel>().unwrap();
    let err = w.require_component("Posn").unwrap_err();
    assert_eq!(err.code, "unknown_component");
    assert!(err.hint.unwrap().contains("did you mean `Pos`"));
}

#[test]
fn components_can_be_defined_at_runtime() {
    let mut w = World::new();
    let def = parse(r#"{"name":"Health","doc":"hit points","fields":[{"name":"hp","type":"f32","default":100},{"name":"regen","type":"f32","default":0.5},{"name":"tags","type":"u32","count":3}]}"#).unwrap();
    let (name, doc, specs) = parse_component_def(&def).unwrap();
    let cid = w.register_dynamic(&name, &doc, &specs).unwrap();
    let e = w.spawn();
    w.insert_json(e, cid, &parse(r#"{"hp": 80}"#).unwrap()).unwrap();
    let v = w.get_json(e, cid).unwrap();
    assert_eq!(v.get("hp").unwrap().as_f64(), Some(80.0));
    assert_eq!(v.get("regen").unwrap().as_f64(), Some(0.5));
    assert_eq!(v.get("tags").unwrap().as_array().unwrap().len(), 3);
    // idempotent re-definition, conflicting re-definition is an error
    assert_eq!(w.register_dynamic(&name, &doc, &specs).unwrap(), cid);
    let other = parse_component_def(&parse(r#"{"name":"Health","fields":[{"name":"hp","type":"i32"}]}"#).unwrap()).unwrap();
    assert_eq!(w.register_dynamic(&other.0, &other.1, &other.2).unwrap_err().code, "component_conflict");
}

#[test]
fn bad_definitions_are_rejected() {
    let mut w = World::new();
    for (def, code) in [
        (r#"{"name":"1bad","fields":[]}"#, "bad_name"),
        (r#"{"name":"Ok","fields":[{"name":"a","type":"f32"},{"name":"a","type":"f32"}]}"#, "duplicate_field"),
        (r#"{"name":"Ok","fields":[{"name":"a","type":"float"}]}"#, "bad_type"),
        (r#"{"name":"Ok","fields":[{"name":"a","type":"f32","count":0}]}"#, "bad_count"),
    ] {
        let err = parse_component_def(&parse(def).unwrap()).and_then(|(n, d, s)| w.register_dynamic(&n, &d, &s)).unwrap_err();
        assert_eq!(err.code, code, "{def}");
    }
}

#[test]
fn snapshot_by_clone_is_independent() {
    let mut w = World::new();
    let e = w.spawn();
    w.insert(e, Pos { x: 1.0, ..Pos::default() }).unwrap();
    let snap = w.clone();
    let hash = w.state_hash();
    assert_eq!(snap.state_hash(), hash);
    w.get_mut::<Pos>(e).unwrap().x = 99.0;
    let e2 = w.spawn();
    w.insert(e2, Vel::default()).unwrap();
    assert_ne!(w.state_hash(), hash);
    assert_eq!(snap.get::<Pos>(e).unwrap().x, 1.0);
    assert_eq!(snap.len(), 1);
    assert_eq!(snap.state_hash(), hash);
}

#[test]
fn state_hash_ignores_storage_order_but_sees_every_value() {
    let build = |order: &[u64]| {
        let mut w = World::new();
        w.register_native::<Pos>().unwrap();
        for &uid in order {
            let e = w.spawn_uid(uid).unwrap();
            w.insert(e, Pos { x: uid as f32, ..Pos::default() }).unwrap();
        }
        w
    };
    let a = build(&[1, 2, 3, 4]);
    let b = build(&[4, 3, 2, 1]);
    assert_eq!(a.state_hash(), b.state_hash());
    let mut c = build(&[1, 2, 3, 4]);
    let e = c.entity(3).unwrap();
    c.get_mut::<Pos>(e).unwrap().z = f32::MIN_POSITIVE;
    assert_ne!(a.state_hash(), c.state_hash());
}

#[test]
fn scene_json_round_trips_with_dynamic_components_and_strings() {
    let mut w = World::new();
    let name = parse_component_def(&parse(r#"{"name":"Label","fields":[{"name":"text","type":"str"}]}"#).unwrap()).unwrap();
    let lid = w.register_dynamic(&name.0, &name.1, &name.2).unwrap();
    let pid = w.register_native::<Pos>().unwrap();
    for i in 0..20 {
        let e = w.spawn();
        w.insert_json(e, lid, &parse(&format!(r#"{{"text":"entity {i} \"quoted\" é"}}"#)).unwrap()).unwrap();
        if i % 2 == 0 {
            w.insert_json(e, pid, &parse(&format!(r#"{{"x": {i}.5}}"#)).unwrap()).unwrap();
        }
    }
    let doc = to_scene_json(&w);
    let text = doc.to_string_pretty().unwrap();
    let mut w2 = World::new();
    w2.register_native::<Pos>().unwrap();
    load_scene_json(&mut w2, &parse(&text).unwrap()).unwrap();
    assert_eq!(w2.len(), 20);
    assert_eq!(w2.state_hash(), w.state_hash());
    // and the text is stable: saving the loaded world gives the identical document
    assert_eq!(to_scene_json(&w2).to_string_pretty().unwrap(), text);
}

#[test]
fn scene_errors_point_at_the_entity_and_component() {
    let mut w = World::new();
    w.register_native::<Pos>().unwrap();
    let scene = parse(r#"{"lunaris":1,"entities":[{"uid":1,"components":{"Pos":{"x":1}}},{"uid":2,"components":{"Pos":{"x":"bad"}}}]}"#).unwrap();
    let err = load_scene_json(&mut w, &scene).unwrap_err();
    assert_eq!(err.code, "type_mismatch");
    assert!(err.path.unwrap().starts_with("entities[1].components."));
    let err = load_scene_json(&mut World::new(), &parse(r#"{"lunaris":1,"entities":[{"uid":1,"components":{"Nope":{}}}]}"#).unwrap()).unwrap_err();
    assert_eq!(err.code, "unknown_component");
    let err = load_scene_json(&mut World::new(), &parse(r#"{"entities":[]}"#).unwrap()).unwrap_err();
    assert_eq!(err.code, "bad_scene");
}

#[test]
fn scene_error_paths_are_exact_and_a_bad_entity_is_never_half_created() {
    let load = |text: &str| {
        let mut w = World::new();
        w.register_native::<Pos>().unwrap();
        w.register_native::<Vel>().unwrap();
        let r = load_scene_json(&mut w, &parse(text).unwrap());
        (r, w)
    };
    let (r, w) = load(r#"{"lunaris":1,"entities":[{"uid":1,"components":{"Pos":{"x":1}}},{"uid":2,"components":{"Pos":{"x":2},"Vel":{"v":[1,"no",3]}}}]}"#);
    let err = r.unwrap_err();
    assert_eq!((err.code, err.path.as_deref()), ("type_mismatch", Some("entities[1].components.Vel.v[1]")));
    assert_eq!(w.len(), 1, "entity 2 failed validation as a whole, so none of its components exist");
    let (r, _) = load(r#"{"lunaris":1,"entities":[{"uid":1,"components":{"Pos":{"nope":1}}}]}"#);
    let err = r.unwrap_err();
    assert_eq!((err.code, err.path.as_deref()), ("unknown_field", Some("entities[0].components.Pos.nope")));
    let (r, _) = load(r#"{"lunaris":1,"entities":[{"uid":1},{"uid":1}]}"#);
    let err = r.unwrap_err();
    assert_eq!((err.code, err.path.as_deref()), ("duplicate_uid", Some("entities[1]")));
    let (r, _) = load(r#"{"lunaris":1,"entities":[{"uid":3,"components":[1]}]}"#);
    assert_eq!(r.unwrap_err().path.as_deref(), Some("entities[0]"));
    let (r, _) = load(r#"{"lunaris":1,"entities":[{"uid":3,"components":{"Pos":{"x":1e60}}}]}"#);
    assert_eq!(r.unwrap_err().code, "out_of_range");
    // numbers, booleans and strings all round trip through the one conversion
    let mut w = World::new();
    w.register_native::<Mixed>().unwrap();
    load_scene_json(&mut w, &parse(r#"{"lunaris":1,"entities":[{"uid":1,"components":{"Mixed":{"flag":false,"big":9,"small":-4,"name":"hello","target":1}}}]}"#).unwrap()).unwrap();
    let e = w.entity(1).unwrap();
    let m = w.get::<Mixed>(e).unwrap();
    assert!(!m.flag && m.big == 9 && m.small == -4 && m.target == Uid(1));
    assert_eq!(w.strings().get(m.name.0), "hello");
}

#[test]
fn many_entities_survive_churn() {
    let mut w = World::new();
    let mut live = Vec::new();
    let mut seed = 12345u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for step in 0..20000u32 {
        match next() % 4 {
            0 | 1 => {
                let e = w.spawn();
                w.insert(e, Pos { x: step as f32, ..Pos::default() }).unwrap();
                live.push((e, step as f32));
            }
            2 if !live.is_empty() => {
                let i = (next() as usize) % live.len();
                let (e, _) = live.swap_remove(i);
                assert!(w.despawn(e));
            }
            3 if !live.is_empty() => {
                let i = (next() as usize) % live.len();
                let (e, x) = live[i];
                if w.has(e, w.component_id("Vel").unwrap_or(ComponentId(u32::MAX))) {
                    w.remove_typed::<Vel>(e).unwrap();
                } else {
                    w.insert(e, Vel { v: [x, 0.0, 0.0] }).unwrap();
                }
            }
            _ => {}
        }
    }
    assert_eq!(w.len(), live.len());
    for (e, x) in live {
        assert_eq!(w.get::<Pos>(e).unwrap().x, x);
        if let Some(v) = w.get::<Vel>(e) {
            assert_eq!(v.v[0], x);
        }
    }
}
