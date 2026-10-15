//! A deterministic mutation fuzzer for the tool API. An AI agent will eventually send every kind of wrong argument; the engine
//! must answer each with an error that explains itself, never panic, hang or corrupt the world. Each valid call from a corpus is
//! damaged in random ways (wrong types, missing keys, huge numbers, deep nesting, junk strings) and sent to a live engine.
use lunaris_json::{parse, Json};
use std::panic::{catch_unwind, AssertUnwindSafe};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn junk(rng: &mut Rng) -> Json {
    match rng.below(20) {
        0 => Json::Null,
        1 => Json::Bool(true),
        2 => Json::Int(0),
        3 => Json::Int(-1),
        4 => Json::Int(i64::MAX),
        5 => Json::Int(i64::MIN),
        6 => Json::Float(1e308),
        7 => Json::Float(-0.0),
        8 => Json::Float(0.1),
        9 => Json::Str(String::new()),
        10 => Json::Str("x".into()),
        11 => Json::Str("a".repeat(5000)),
        12 => Json::Str("\u{0}\u{7f}\u{1F600}é\n\"\\".into()),
        13 => Json::Str("../../etc/passwd".into()),
        14 => Json::Str("Transform".into()),
        15 => Json::Arr(Vec::new()),
        16 => Json::Obj(Vec::new()),
        17 => Json::Arr(vec![Json::Int(1), Json::Int(2), Json::Int(3)]),
        18 => Json::Obj(vec![("a".into(), Json::Int(1))]),
        _ => {
            let mut j = Json::Int(1);
            for _ in 0..(5 + rng.below(40)) {
                j = if rng.below(2) == 0 { Json::Arr(vec![j]) } else { Json::Obj(vec![("k".into(), j)]) };
            }
            j
        }
    }
}

fn nodes(j: &Json, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    out.push(path.clone());
    match j {
        Json::Arr(a) => a.iter().enumerate().for_each(|(i, c)| {
            path.push(i);
            nodes(c, path, out);
            path.pop();
        }),
        Json::Obj(p) => p.iter().enumerate().for_each(|(i, (_, c))| {
            path.push(i);
            nodes(c, path, out);
            path.pop();
        }),
        _ => {}
    }
}

fn at_mut<'a>(j: &'a mut Json, path: &[usize]) -> &'a mut Json {
    let mut cur = j;
    for &i in path {
        cur = match cur {
            Json::Arr(a) => &mut a[i],
            Json::Obj(p) => &mut p[i].1,
            _ => unreachable!("paths come from nodes()"),
        };
    }
    cur
}

fn mutate(j: &mut Json, rng: &mut Rng) {
    let mut all = Vec::new();
    nodes(j, &mut Vec::new(), &mut all);
    let path = all[rng.below(all.len())].clone();
    match rng.below(6) {
        0 | 1 => *at_mut(j, &path) = junk(rng),
        2 => {
            // delete a child of the chosen container (or damage a scalar)
            let target = at_mut(j, &path);
            match target {
                Json::Arr(a) if !a.is_empty() => {
                    let i = rng.below(a.len());
                    a.remove(i);
                }
                Json::Obj(p) if !p.is_empty() => {
                    let i = rng.below(p.len());
                    p.remove(i);
                }
                other => *other = junk(rng),
            }
        }
        3 => {
            // numbers: flip sign, scale, or make it enormous
            let pick = rng.below(3);
            let target = at_mut(j, &path);
            let next = match &*target {
                Json::Int(i) => [Json::Int(i.wrapping_neg()), Json::Int(i.wrapping_mul(1_000_003)), Json::Int(i64::MAX)][pick].clone(),
                Json::Float(f) => [Json::Float(-*f), Json::Float(*f * 1e30), Json::Float(f64::MAX)][pick].clone(),
                other => other.clone(),
            };
            *target = next;
        }
        4 => {
            // rename a key to something close to valid
            if let Json::Obj(p) = at_mut(j, &path) {
                if !p.is_empty() {
                    let i = rng.below(p.len());
                    p[i].0.push('x');
                }
            }
        }
        _ => {
            let target = at_mut(j, &path);
            let inner = std::mem::replace(target, Json::Null);
            *target = if rng.below(2) == 0 { Json::Arr(vec![inner]) } else { Json::Obj(vec![("value".into(), inner)]) };
        }
    }
}

/// Calls that would legitimately take long or allocate a lot are skipped after mutation (the limits themselves are tested elsewhere).
fn too_big(tool: &str, args: &Json) -> bool {
    let num = |k: &str| args.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
    match tool {
        "render" | "pick" | "project" => num("width") * num("height") * num("supersample").max(1.0).powi(2) > 400_000.0,
        "spawn" => num("count") > 2000.0,
        "step" => num("ticks") > 100.0,
        "reset" => num("threads") > 8.0,
        "verify" => ticks_in_spec(args) > 300.0,
        _ => false,
    }
}

/// The total number of ticks a (damaged) playtest spec would run: every number under `step` and `max_ticks`.
fn ticks_in_spec(j: &Json) -> f64 {
    match j {
        Json::Obj(p) => p.iter().map(|(k, v)| if k == "step" || k == "max_ticks" { v.as_f64().unwrap_or(0.0).abs() + ticks_in_spec(v) } else { ticks_in_spec(v) }).sum(),
        Json::Arr(a) => a.iter().map(ticks_in_spec).sum(),
        _ => 0.0,
    }
}

const CORPUS: &[(&str, &str)] = &[
    ("describe", "{}"),
    ("schema", r#"{"component":"Transform"}"#),
    ("define_component", r#"{"name":"Health","doc":"hp","fields":[{"name":"hp","type":"f32","default":100},{"name":"team","type":"str"},{"name":"slots","type":"i32","count":3}]}"#),
    ("spawn", r#"{"count":3,"components":{"Name":{"value":"x"},"Transform":{"pos":[1,2,3]},"Velocity":{"lin":[1,0,0]},"MeshRenderer":{"mesh":"cube"}}}"#),
    ("get", r#"{"uid":1,"components":["Transform"]}"#),
    ("set", r#"{"uid":1,"component":"Transform","value":{"pos":[0,1,0]}}"#),
    ("add_component", r#"{"uid":1,"component":"Wander","value":{"strength":2}}"#),
    ("remove_component", r#"{"uid":1,"component":"Wander"}"#),
    ("despawn", r#"{"uid":2}"#),
    ("query", r#"{"with":["Transform"],"select":["Transform"],"where":[{"component":"Transform","field":"pos","index":1,"op":">","value":0}],"limit":10,"offset":1}"#),
    ("batch", r#"{"commands":[{"op":"spawn","components":{"Transform":{}}},{"op":"set","uid":1,"component":"Transform","value":{"pos":[5,5,5]}}]}"#),
    ("step", r#"{"ticks":3}"#),
    ("hash", "{}"),
    ("snapshot", r#"{"name":"a"}"#),
    ("restore", r#"{"name":"a"}"#),
    ("snapshots", "{}"),
    ("delete_snapshot", r#"{"name":"a"}"#),
    ("undo", "{}"),
    ("redo", "{}"),
    ("record_start", "{}"),
    ("record_stop", r#"{"path":"fuzz/rec.json"}"#),
    ("replay", "{}"),
    ("save_scene", r#"{"path":"fuzz/scene.json"}"#),
    ("load_scene", r#"{"path":"fuzz/scene.json"}"#),
    ("contacts", r#"{"limit":5}"#),
    ("raycast", r#"{"origin":[0,5,0],"direction":[0,-1,0],"max_distance":50}"#),
    ("acoustics", r#"{"listener":[0,1,0],"source":[3,1,2],"rays":32,"source_rays":16}"#),
    ("record_audio", r#"{"ticks":4,"acoustics":true}"#),
    ("nearby", r#"{"point":[0,0,0],"radius":10,"limit":5,"with":["Transform"]}"#),
    ("render", r#"{"width":64,"height":48,"eye":[0,3,8],"target":[0,0,0],"fov":50,"supersample":1}"#),
    ("pick", r#"{"width":64,"height":48,"x":10,"y":10}"#),
    ("project", r#"{"width":64,"height":48,"point":[0,0,0]}"#),
    ("minimap", r#"{"around":1,"radius":12,"cells":15,"view":"top","with":["Transform"]}"#),
    ("check", "{}"),
    ("guide", r#"{"topic":"loop"}"#),
    ("events", r#"{"since_tick":0,"limit":10}"#),
    ("define_system", r#"{"name":"g","doc":"d","order":50,"each":["Transform","Velocity"],"without":["Wander"],"do":[
        {"let":"s","be":{"clamp":[{"get":"Velocity.lin[0]"},-5,5]}},
        {"if":{"and":[{"gt":[{"var":"s"},1]},{"not":{"lt":[{"res":"x"},0]}}]},"then":[{"set":"Transform.pos[0]","to":{"add":[{"get":"Transform.pos[0]"},{"mul":[{"var":"s"},{"dt":true}]}]}}],"else":[{"res_add":"n","by":1}]},
        {"if":{"lt":[{"rand":3},0.001]},"then":[{"despawn":true}]}]}"#),
    ("terrain_height", r#"{"x":1,"z":2}"#),
    ("terrain_height", r#"{"x":"a","z":2}"#),
    ("terrain_layer", r#"{"x":1,"z":2}"#),
    ("terrain_layer", r#"{"x":"a","z":2,"terrain":"b"}"#),
    ("terrain_layer", "{}"),
    ("water_depth", r#"{"x":1,"z":2}"#),
    ("water_depth", r#"{"x":1,"z":2,"y":"a"}"#),
    ("water_depth", "{}"),
    ("scatter", r#"{"count":3,"components":{},"layer":"grass","layer_min":2}"#),
    ("scatter", r#"{"count":3,"components":{},"layer":{"a":1}}"#),
    ("reload_terrain", "{}"),
    ("saves", "{}"),
    ("type_text", r#"{"text":"hi","enter":true}"#),
    ("type_text", "{}"),
    ("autotile", r#"{"uid":1}"#),
    ("autotile", r#"{"uid":1,"mode":"blob","solid":[1,2],"edge":"empty"}"#),
    ("autotile", r#"{"uid":1,"mode":"x"}"#),
    ("autotile", r#"{"uid":"a","solid":3}"#),
    ("generate_map", "{}"),
    ("generate_map", r#"{"uid":1,"mode":"caves","seed":4}"#),
    ("generate_map", r#"{"mode":"maze","width":9,"height":9,"braid":50}"#),
    ("generate_map", r#"{"mode":"x"}"#),
    ("generate_map", r#"{"width":3}"#),
    ("generate_map", r#"{"width":"a","height":-1,"seed":"s"}"#),
    ("generate_map", r#"{"min_room":9,"max_room":4}"#),
    ("generate_map", r#"{"uid":99}"#),
    ("tilemap_colliders", r#"{"uid":1}"#),
    ("tilemap_colliders", r#"{"uid":1,"solid":[1,[2,5]],"depth":0.5,"clear":false}"#),
    ("tilemap_colliders", r#"{"uid":1,"solid":[[1]],"components":3}"#),
    ("tilemap_colliders", r#"{"uid":"a","depth":-1}"#),
    ("import_tiled", r#"{"path":"nothing.tmj"}"#),
    ("import_tiled", r#"{"path":"../x","tile":0}"#),
    ("import_tiled", r#"{"path":3,"pos":[1,2]}"#),
    ("import_tiled", "{}"),
    ("profile", r#"{"ticks":3}"#),
    ("profile", "{}"),
    ("profile", r#"{"ticks":0}"#),
    ("profile", r#"{"ticks":"x"}"#),
    ("export_game", r#"{"out":"fuzz/dist"}"#),
    ("export_game", r#"{"out":"../x"}"#),
    ("export_game", r#"{"out":"."}"#),
    ("export_game", r#"{"name":"a b"}"#),
    ("export_game", r#"{"overwrite":"yes"}"#),
    ("save_game", "{}"),
    ("save_game", r#"{"slot":"../x"}"#),
    ("load_game", r#"{"slot":"nope"}"#),
    ("load_game", r#"{"slot":3}"#),
    ("scatter", r#"{"count":5}"#),
    ("scatter", r#"{"count":5,"prefab":"nope"}"#),
    ("scatter", r#"{"count":5,"components":{},"slope":[10,5]}"#),
    ("scatter", r#"{"count":"x","components":{}}"#),
    ("scatter", r#"{"count":3,"components":{"Name":{"value":"t"}},"area":[1,2,3]}"#),
    ("add_component", r#"{"uid":2,"component":"Terrain","value":{"heightmap":"nowhere.png","size":[10,10],"height":5}}"#),
    ("systems", "{}"),
    ("get_system", r#"{"name":"g"}"#),
    ("compile_script", r#"{"source":"system s each Transform { Transform.pos[0] = Transform.pos[0] + 1; }"}"#),
    ("compile_script", r#"{"source":"system s each { ) ] x"}"#),
    ("define_script", r#"{"source":"system s2 each Transform { Transform.pos[0] += dt; }"}"#),
    ("define_script", r#"{"source":"system s3 each Transform { Transform.nope[0] = 1; }"}"#),
    ("get_script", r#"{"name":"s2"}"#),
    ("remove_system", r#"{"name":"s2"}"#),
    ("enable_system", r#"{"name":"g","enabled":false}"#),
    ("remove_system", r#"{"name":"g"}"#),
    ("set_resource", r#"{"name":"score","value":3}"#),
    ("resources", "{}"),
    ("input", r#"{"values":{"w":true,"move_x":0.5}}"#),
    ("revision", "{}"),
    ("files", r#"{"path":".","recursive":true,"limit":20,"kind":"model"}"#),
    ("add_component", r#"{"uid":2,"component":"Parent","value":{"target":1}}"#),
    ("add_component", r#"{"uid":2,"component":"LocalTransform","value":{"pos":[1,0,0],"scale":[2,2,2]}}"#),
    ("set", r#"{"uid":1,"component":"Parent","value":{"target":2}}"#),
    ("define_prefab", r#"{"name":"P","components":{"Name":{"value":"p"},"Transform":{"pos":[1,0,0]}},"children":[{"name":"c","components":{"LocalTransform":{"pos":[0,1,0]}},"children":[{"prefab":"P"}]}]}"#),
    ("define_prefab", r#"{"name":"Q","prefab":"P","components":{"Name":{"value":"q"}}}"#),
    ("spawn_prefab", r#"{"prefab":"P","at":[1,2,3],"overrides":{"Name":{"value":"q"}},"count":2}"#),
    ("prefabs", r#"{"name":"P"}"#),
    ("remove_prefab", r#"{"name":"Q"}"#),
    ("path", r#"{"from":[0,0.5,0],"to":[5,0.5,5],"radius":0.4,"step_height":0.3}"#),
    ("path", r#"{"from":2,"to":3,"max_drop":2}"#),
    ("add_component", r#"{"uid":2,"component":"NavAgent","value":{"target":[3,0,3],"active":true,"speed":2}}"#),
    ("set", r#"{"uid":2,"component":"NavAgent","value":{"target":[-3,0,3],"active":true}}"#),
    ("add_component", r#"{"uid":3,"component":"Ui","value":{"pos":[0.1,0.1],"size":[0.3,0.2],"button":true,"text":"x {score}","fill":0.5,"border":0.01}}"#),
    ("input", r#"{"values":{"mouse_x":0.2,"mouse_y":0.15,"mouse_left":true}}"#),
    ("define_system", r#"{"name":"sp","each":["Collider"],"do":[{"if":{"lt":[{"rand":5},0.001]},"then":[{"spawn_prefab":"P","at":[{"get":"Transform.pos[0]"},1,0],"velocity":[0,1,0],"with":{"Name":{"value":"z"}}}]}]}"#),
    ("verify", r#"{"spec":{"scene":"fuzz/scene.json","replay_check":true,"steps":[{"input":{"d":true}},{"step":3},{"expect":{"tick":true,"eq":3}},{"expect":{"name":"n","field":"Transform.pos[1]","approx":[0.5,0.1]}},{"until":{"resource":"x","ge":1,"max_ticks":5}},{"call":{"tool":"hash","args":{}}},{"call":{"tool":"despawn","args":{"uid":99},"error":"no_such_entity"}}]}}"#),
];

#[test]
fn damaged_tool_calls_never_panic_and_the_engine_keeps_working() {
    // LUNARIS_FUZZ_SEED / LUNARIS_FUZZ_ROUNDS run a longer or different campaign (the defaults keep the normal test fast)
    let seed = std::env::var("LUNARIS_FUZZ_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0xF00D);
    let rounds: usize = std::env::var("LUNARIS_FUZZ_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(6000);
    let mut rng = Rng(seed);
    let mut e = crate::tests::engine();
    // a populated world, so calls have something to hit
    e.call("define_component", &parse(CORPUS[2].1).unwrap()).unwrap();
    e.call("spawn", &parse(r#"{"count":5,"components":{"Name":{"value":"n"},"Transform":{"pos":[0,0.5,0]},"Velocity":{},"MeshRenderer":{"mesh":"sphere"},"Collider":{},"RigidBody":{}}}"#).unwrap()).unwrap();
    e.call("save_scene", &parse(CORPUS[22].1).unwrap()).unwrap();
    let mut panics: Vec<String> = Vec::new();
    let mut errors = 0usize;
    let mut oks = 0usize;
    // a call that never comes back is as bad as a panic: a watchdog names it and ends the run
    let current: std::sync::Arc<std::sync::Mutex<(u64, String, std::time::Instant)>> = std::sync::Arc::new(std::sync::Mutex::new((0, String::new(), std::time::Instant::now())));
    {
        let current = std::sync::Arc::clone(&current);
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let c = current.lock().unwrap_or_else(|p| p.into_inner());
            if !c.1.is_empty() && c.2.elapsed() > std::time::Duration::from_secs(30) {
                eprintln!("HANG: this call has not returned for 30 seconds: {}", c.1);
                std::process::exit(99);
            }
        });
    }
    for round in 0..rounds {
        let (tool, args) = CORPUS[rng.below(CORPUS.len())];
        let mut a = parse(args).unwrap();
        for _ in 0..(1 + rng.below(3)) {
            mutate(&mut a, &mut rng);
        }
        // sometimes send the call unmutated, so the state keeps changing in valid ways too
        if rng.below(5) == 0 {
            a = parse(args).unwrap();
        }
        if too_big(tool, &a) {
            continue;
        }
        let shown = a.to_string_compact().unwrap_or_default();
        *current.lock().unwrap_or_else(|p| p.into_inner()) = (round as u64, format!("{tool} {}", shown.chars().take(400).collect::<String>()), std::time::Instant::now());
        let result = catch_unwind(AssertUnwindSafe(|| e.call(tool, &a)));
        current.lock().unwrap_or_else(|p| p.into_inner()).1.clear();
        match result {
            Ok(Ok(_)) => oks += 1,
            Ok(Err(err)) => {
                errors += 1;
                // every refusal has a stable code and a message a person can read
                assert!(!err.code.is_empty() && !err.message.is_empty(), "round {round}: {tool} {shown} gave an empty error");
            }
            Err(_) => panics.push(format!("{tool} {}", shown.chars().take(300).collect::<String>())),
        }
        if round % 500 == 499 {
            // the engine still answers and its state is still self-consistent after all that abuse
            let h1 = e.call("hash", &parse("{}").unwrap()).expect("hash works");
            let h2 = e.call("hash", &parse("{}").unwrap()).expect("hash works");
            assert_eq!(h1, h2);
            e.call("describe", &parse("{}").unwrap()).expect("describe works");
        }
    }
    assert!(panics.is_empty(), "{} calls panicked, for example:\n{}", panics.len(), panics.iter().take(5).cloned().collect::<Vec<_>>().join("\n"));
    println!("fuzz: {oks} calls succeeded, {errors} were refused with an explanation");
    assert!(oks > rounds / 12 && errors > rounds / 12, "the fuzzer should exercise both outcomes: {oks} ok, {errors} errors");
}

/// The newest components with absurd values (huge, negative, zero, nonsense text, entities that do not exist, themselves): stepping and drawing a world full of
/// them must neither panic nor hang, whatever `check` has to say about it.
#[test]
fn the_newest_components_survive_absurd_values_in_a_real_world() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let cases: &[(&str, &str)] = &[
        ("Particles", r#"{"count":4000000000,"lifetime":-5,"size":[1e30,-1e30],"speed_range":[1e30,-1e30],"gravity":[1e30,0,0],"drag":1e30,"turbulence":1e30,"turbulence_scale":-1e30,"sheet":[0,0],"cone":1e30,"extent":[1e30,-1e30,0],"seed":4294967295,"time":1e30,"spread_time":-1,"stretch":1e30,"fade_in":-1}"#),
        ("Particles", r#"{"count":0,"lifetime":0,"looping":false,"time":-1e30,"direction":[0,0,0],"sheet":[4294967295,4294967295],"texture":"../../x","shape":99}"#),
        ("UiStyle", r#"{"radius":1e30,"color2":[1e30,-1e30,1e30,1e30],"image":"../../x.png","image_uv":[1e30,-1e30,0,0],"tint":[1e30,1e30,1e30,1e30],"fit":4294967295,"shadow_color":[1,1,1,1],"shadow_offset":[1e30,-1e30]}"#),
        ("Ui", r#"{"pos":[1e30,-1e30],"size":[1e30,0],"text":"x","text_size":1e30,"border":1e30,"fill":-1e30}"#),
        ("TextFont", r#"{"file":"../../etc/passwd","wrap":-1e30,"line_spacing":1e30}"#),
        ("Text", r#"{"value":"hello\nworld","size":1e30,"pos":[1e30,1e30]}"#),
        ("UiAnchor", r#"{"entity":999999,"world":[1e30,-1e30,1e30],"offset":[1e30,1e30]}"#),
        ("MusicPlayer", r#"{"playlist":"../../x.wav\nvoice:\n\n#","crossfade":1e30,"volume":-1e30,"skip":4294967295}"#),
        ("AnimEvents", r#"{"events":"walk length=0: 1 x\n:::\nwalk length=1e30: 1e30 y, , ,"}"#),
        ("RootMotion", r#"{"curves":"walk length=1e-30: 0 0 0 0, 1e-30 1e30 1e30 1e30","scale":1e30}"#),
        ("Dialogue", r#"{"script":"== a\n* [x] -> a\n-> a","active":true,"text_ui":1,"choice_1":1,"speed":1e30,"choose":4294967295}"#),
        ("Dialogue", r#"{"script":"== a\nif $x >= 1 -> a\nset Nope.field = 1\n","active":true}"#),
        ("CameraView", r#"{"camera":1,"width":4294967295,"height":0,"every":4294967295}"#),
        ("Parallax", r#"{"factor":[1e30,-1e30],"origin":[1e30,1e30]}"#),
        ("AudioMixer", r#"{"config":"bus a parent=a\nroute x -> y\n"}"#),
        ("Ik", r#"{"skin":1,"joint":"nope","kind":1,"weight":1e30,"max_angle":1e30,"axis":[0,0,0]}"#),
    ];
    let mut e = crate::tests::engine();
    e.call("spawn", &parse(r#"{"uid":1,"components":{"Transform":{"pos":[0,1,5]},"Camera":{},"CameraView":{"camera":1,"width":24,"height":24}}}"#).unwrap()).unwrap();
    e.call("spawn", &parse(r#"{"uid":2,"components":{"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"cube"},"Sprite":{}}}"#).unwrap()).unwrap();
    for (n, (comp, value)) in cases.iter().enumerate() {
        let uid = 10 + n as u64;
        let spawned = e.call("spawn", &parse(&format!(r#"{{"uid":{uid},"components":{{"Transform":{{}},"{comp}":{value}}}}}"#)).unwrap());
        assert!(spawned.is_ok(), "{comp}: {spawned:?}");
    }
    let r = catch_unwind(AssertUnwindSafe(|| {
        for _ in 0..3 {
            let _ = e.call("step", &parse(r#"{"ticks":4}"#).unwrap());
            let _ = e.call("render", &parse(r#"{"width":48,"height":32,"image":false,"quality":"reference"}"#).unwrap());
            let _ = e.call("check", &parse("{}").unwrap());
            let _ = e.call("record_audio", &parse(r#"{"ticks":4,"acoustics":false}"#).unwrap());
        }
    }));
    assert!(r.is_ok(), "a call panicked on absurd values");
    // and the engine is still itself
    let h1 = e.call("hash", &parse("{}").unwrap()).unwrap();
    assert_eq!(h1, e.call("hash", &parse("{}").unwrap()).unwrap());
}

