//! The three script tools: what an agent does to write gameplay as text instead of JSON.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn text(source: &str) -> Json {
    Json::obj([("source", Json::str(source))])
}

const GRAVITY: &str = "/// Fall.\nsystem fall each Transform, Velocity {\n    Velocity.lin[1] -= 9.8 * dt;\n    if Transform.pos[1] < 0 {\n        Transform.pos[1] = 0;\n    }\n}\n";

#[test]
fn compile_script_checks_without_touching_the_world_and_reports_where() {
    let mut e = engine();
    let rev = e.call("revision", &Json::Null).unwrap();
    let ok = e.call("compile_script", &text(GRAVITY)).unwrap();
    assert_eq!(ok.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(ok.get("systems").unwrap().index(0).unwrap().get("name").unwrap().as_str(), Some("fall"));
    assert!(e.call("get_system", &j(r#"{"name":"fall"}"#)).is_err(), "compiling defines nothing");
    let bad = e.call("compile_script", &text("system s each Transform {\n    Transform.pos[0] = ;\n}")).unwrap();
    assert_eq!(bad.get("ok").and_then(|v| v.as_bool()), Some(false));
    let first = bad.get("errors").unwrap().index(0).unwrap();
    assert_eq!(first.get("line").unwrap().as_u64(), Some(2));
    assert!(first.get("message").unwrap().as_str().unwrap().contains("expected a value"));
    // reading changes nothing: the revision is the same
    assert_eq!(e.call("revision", &Json::Null).unwrap().get("revision"), rev.get("revision"));
}

#[test]
fn define_script_defines_every_system_as_one_undo_step_and_runs_them() {
    let mut e = engine();
    let src = format!("{GRAVITY}\nsystem drift each Transform {{\n    Transform.pos[0] += 1.0 * dt;\n}}\n");
    let r = e.call("define_script", &text(&src)).unwrap();
    assert_eq!(r.get("systems").unwrap().as_array().unwrap().len(), 2);
    let names: Vec<String> = e.call("systems", &Json::Null).unwrap().get("graph").unwrap().as_array().unwrap().iter().map(|s| s.get("name").unwrap().as_str().unwrap().to_string()).collect();
    assert!(names.contains(&"fall".to_string()) && names.contains(&"drift".to_string()), "{names:?}");
    // the JSON of the system is exactly what the script says
    let def = e.call("get_system", &j(r#"{"name":"fall"}"#)).unwrap();
    let def = def.get("def").unwrap();
    assert_eq!(def.get("doc").unwrap().as_str(), Some("Fall."));
    // one undo takes both away
    e.call("undo", &Json::Null).unwrap();
    assert!(e.call("get_system", &j(r#"{"name":"fall"}"#)).is_err() && e.call("get_system", &j(r#"{"name":"drift"}"#)).is_err());
}

#[test]
fn a_script_the_world_refuses_is_all_or_nothing_and_the_error_has_its_line() {
    let mut e = engine();
    let src = "system fine each Transform {\n    Transform.pos[0] = 1;\n}\nsystem wrong each Transform {\n    Transform.pos[0] = 1;\n    Transform.nope[0] = 2;\n}\n";
    let err = e.call("define_script", &text(src)).unwrap_err();
    assert!(err.message.starts_with("6:"), "{}", err.message);
    assert!(err.message.contains("nope"), "{}", err.message);
    assert!(e.call("get_system", &j(r#"{"name":"fine"}"#)).is_err(), "the first system was rolled back");
    // a script that does not even read
    let err = e.call("define_script", &text("system x each Transform { ) }")).unwrap_err();
    assert_eq!(err.code, "bad_script");
    assert!(err.hint.as_deref().unwrap_or("").contains("1:"), "{err:?}");
    // the redo stack is not polluted by the rollback
    assert!(e.call("redo", &Json::Null).is_err());
}

#[test]
fn get_script_gives_back_text_that_defines_the_same_system() {
    let mut e = engine();
    e.call("define_system", &j(r#"{"name":"spin","each":["Transform"],"do":[{"set":"Transform.rot[1]","to":{"add":[{"get":"Transform.rot[1]"},{"mul":[0.5,{"dt":true}]}]}}]}"#)).unwrap();
    let s = e.call("get_script", &j(r#"{"name":"spin"}"#)).unwrap();
    let source = s.get("source").unwrap().as_str().unwrap().to_string();
    assert!(source.contains("system spin each Transform"), "{source}");
    let before = e.call("get_system", &j(r#"{"name":"spin"}"#)).unwrap().get("def").unwrap().clone();
    e.call("remove_system", &j(r#"{"name":"spin"}"#)).unwrap();
    e.call("define_script", &text(&source)).unwrap();
    let after = e.call("get_system", &j(r#"{"name":"spin"}"#)).unwrap().get("def").unwrap().clone();
    assert_eq!(before.to_string_compact().unwrap().replace(",\"enabled\":true", ""), after.to_string_compact().unwrap().replace(",\"enabled\":true", ""));
    assert!(e.call("get_script", &j(r#"{"name":"nope"}"#)).unwrap_err().code == "no_such_system");
}

/// Every graph system of the scene `scene` (a path in the workspace `root`) as Lunaris Script text and back: how many systems it has, and what is said of each that comes back different, is refused or has no text form.
fn script_trip(root: &std::path::Path, scene: &str) -> (usize, Vec<String>) {
    let mut e = crate::Engine::new(1, root);
    e.call("load_scene", &Json::obj([("path", Json::str(scene))])).unwrap();
    let names: Vec<String> = e.call("systems", &Json::Null).unwrap().get("graph").and_then(|s| s.as_array()).unwrap_or(&[]).iter().filter_map(|s| s.get("name").and_then(|n| n.as_str()).map(str::to_string)).collect();
    // (the unary operators a scene may hold as `{"abs": [x]}` are read the same as `{"abs": x}`; `enabled: true` is what a system is anyway)
    fn unwrapped(j: &Json) -> Json {
        const UNARY: [&str; 18] = ["abs", "floor", "ceil", "sqrt", "neg", "not", "sin", "cos", "tan", "round", "sign", "fract", "exp", "ln", "log", "asin", "acos", "atan"];
        match j {
            Json::Obj(pairs) if pairs.len() == 1 && UNARY.contains(&pairs[0].0.as_str()) => match &pairs[0].1 {
                Json::Arr(a) if a.len() == 1 => Json::Obj(vec![(pairs[0].0.clone(), unwrapped(&a[0]))]),
                v => Json::Obj(vec![(pairs[0].0.clone(), unwrapped(v))]),
            },
            Json::Obj(pairs) => Json::Obj(pairs.iter().map(|(k, v)| (k.clone(), unwrapped(v))).collect()),
            Json::Arr(a) => Json::Arr(a.iter().map(unwrapped).collect()),
            other => other.clone(),
        }
    }
    // (the order of the keys of an object means nothing: the script text writes `doc` and `order` where it likes)
    fn sorted(j: &Json) -> Json {
        match j {
            Json::Obj(pairs) => {
                let mut v: Vec<(String, Json)> = pairs.iter().map(|(k, x)| (k.clone(), sorted(x))).collect();
                v.sort_by(|a, b| a.0.cmp(&b.0));
                Json::Obj(v)
            }
            Json::Arr(a) => Json::Arr(a.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    let plain = |d: &Json| sorted(&unwrapped(d)).to_string_compact().unwrap().replace("\"enabled\":true,", "").replace(",\"enabled\":true", "");
    let mut bad = Vec::new();
    for name in &names {
        let Ok(got) = e.call("get_script", &Json::obj([("name", Json::str(name.clone()))])) else {
            bad.push(format!("{name}: has no script form"));
            continue;
        };
        let source = got.get("source").unwrap().as_str().unwrap().to_string();
        let before = e.call("get_system", &Json::obj([("name", Json::str(name.clone()))])).unwrap().get("def").unwrap().clone();
        e.call("remove_system", &Json::obj([("name", Json::str(name.clone()))])).unwrap();
        match e.call("define_script", &text(&source)) {
            Err(err) => bad.push(format!("{name}: the text is refused: {err}")),
            Ok(_) => {
                let after = e.call("get_system", &Json::obj([("name", Json::str(name.clone()))])).unwrap().get("def").unwrap().clone();
                let (x, y) = (plain(&before), plain(&after));
                if x != y {
                    // where the two texts first differ, with what stands round it
                    let at = x.bytes().zip(y.bytes()).position(|(p, q)| p != q).unwrap_or(x.len().min(y.len()));
                    let around = |s: &str| s.chars().skip(at.saturating_sub(70)).take(160).collect::<String>();
                    bad.push(format!("{name}: comes back different, first at byte {at}:\n      before: ...{}\n      after:  ...{}", around(&x), around(&y)));
                }
            }
        }
    }
    (names.len(), bad)
}

/// Every graph system of a real scene as Lunaris Script text and back: `LUNARIS_BIG_ROOT=/workspace LUNARIS_BIG_SCENE=scenes/x.lunaris.json cargo test --release -p lunaris-api real_scene_scripts -- --ignored --nocapture`.
#[test]
#[ignore]
fn every_system_of_a_real_scene_survives_the_trip_through_script_text_real_scene_scripts() {
    let (Ok(root), Ok(scene)) = (std::env::var("LUNARIS_BIG_ROOT"), std::env::var("LUNARIS_BIG_SCENE")) else { return };
    let (n, bad) = script_trip(std::path::Path::new(&root), &scene);
    println!("{n} graph systems through script text; {} do not survive", bad.len());
    for b in &bad {
        println!("  {b}");
    }
    assert!(bad.is_empty(), "{} systems change in the script text and back", bad.len());
}

/// The same for every example the repository ships (a hundred and more systems in 70 scenes: the statements and operators that only some of them use).
#[test]
fn every_system_of_every_example_survives_the_trip_through_script_text() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut scenes = Vec::new();
    crate::scene_roundtrip_tests::scenes_under(&repo.join("examples"), &mut scenes);
    let (mut systems, mut bad) = (0, Vec::new());
    for path in &scenes {
        let rel = path.strip_prefix(&repo).unwrap().to_string_lossy().replace('\\', "/");
        let (n, scene_bad) = script_trip(&repo, &rel);
        systems += n;
        bad.extend(scene_bad.into_iter().map(|b| format!("{rel}: {b}")));
    }
    assert!(systems > 100, "only {systems} graph systems found in the examples");
    assert!(bad.is_empty(), "{} of {systems} systems change in the script text and back:\n{}", bad.len(), bad.join("\n"));
}

/// A real scene saved and loaded again is the same world: `LUNARIS_BIG_ROOT=/workspace LUNARIS_BIG_SCENE=scenes/x.lunaris.json cargo test --release -p lunaris-api real_scene_saves -- --ignored --nocapture`.
#[test]
#[ignore]
fn a_real_scene_saved_and_loaded_again_is_the_same_world_real_scene_saves() {
    let (Ok(root), Ok(scene)) = (std::env::var("LUNARIS_BIG_ROOT"), std::env::var("LUNARIS_BIG_SCENE")) else { return };
    let root = std::path::PathBuf::from(root);
    let mut a = crate::Engine::new(1, root.clone());
    a.call("load_scene", &Json::obj([("path", Json::str(scene))])).unwrap();
    let h1 = a.call("hash", &Json::Null).unwrap().get("hash").unwrap().as_str().unwrap().to_string();
    let t = std::time::Instant::now();
    let probe = format!("_probe_resave_{}.lunaris.json", std::process::id());
    a.call("save_scene", &Json::obj([("path", Json::str(probe.clone()))])).unwrap();
    println!("saved in {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let mut b = crate::Engine::new(1, root.clone());
    let t = std::time::Instant::now();
    b.call("load_scene", &Json::obj([("path", Json::str(probe.clone()))])).unwrap();
    println!("loaded in {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let h2 = b.call("hash", &Json::Null).unwrap().get("hash").unwrap().as_str().unwrap().to_string();
    let (n1, n2) = (a.sim.world.len(), b.sim.world.len());
    println!("{n1} entities, hash {h1}; after the round trip {n2} entities, hash {h2}");
    if h1 != h2 {
        // what differs: entity by entity, component by component (the first few), then resources and systems
        let mut shown = 0;
        let uids: Vec<u64> = a.call("query", &Json::obj([("select", Json::arr([])), ("limit", Json::from(100_000))])).unwrap().get("entities").unwrap().as_array().unwrap().iter().filter_map(|e| e.get("uid").and_then(|u| u.as_u64())).collect();
        let mut differing = 0;
        for uid in uids {
            let q = Json::obj([("uid", Json::from(uid as i64))]);
            let (x, y) = (a.call("get", &q), b.call("get", &q));
            let (Ok(x), Ok(y)) = (x, y) else {
                differing += 1;
                if shown < 8 {
                    println!("  entity {uid}: missing after the round trip");
                    shown += 1;
                }
                continue;
            };
            let (cx, cy) = (x.get("components").cloned().unwrap_or(Json::Null), y.get("components").cloned().unwrap_or(Json::Null));
            if cx.to_string_compact().unwrap() != cy.to_string_compact().unwrap() {
                differing += 1;
                if shown < 8 {
                    if let (Json::Obj(px), Json::Obj(py)) = (&cx, &cy) {
                        for (k, v) in px {
                            let other = py.iter().find(|(n, _)| n == k).map(|(_, w)| w);
                            if other.map(|w| w.to_string_compact().unwrap()) != Some(v.to_string_compact().unwrap()) {
                                println!("  entity {uid} {k}: {} -> {}", v.to_string_compact().unwrap().chars().take(160).collect::<String>(), other.map(|w| w.to_string_compact().unwrap().chars().take(160).collect::<String>()).unwrap_or("(gone)".to_string()));
                            }
                        }
                        for (k, _) in py {
                            if !px.iter().any(|(n, _)| n == k) {
                                println!("  entity {uid} {k}: (new)");
                            }
                        }
                    }
                    shown += 1;
                }
            }
        }
        println!("{differing} entities differ in their components");
        let (ra, rb) = (a.call("resources", &Json::Null).unwrap().to_string_compact().unwrap(), b.call("resources", &Json::Null).unwrap().to_string_compact().unwrap());
        println!("resources the same: {}; systems the same: {}; prefabs the same: {}", ra == rb, a.call("systems", &Json::Null).unwrap().to_string_compact().unwrap() == b.call("systems", &Json::Null).unwrap().to_string_compact().unwrap(), a.call("prefabs", &Json::Null).unwrap().to_string_compact().unwrap() == b.call("prefabs", &Json::Null).unwrap().to_string_compact().unwrap());
    }
    assert_eq!((n1, &h1), (n2, &h2), "the saved scene is not the same world");
    // a second save is the same text as the first (nothing is lost or reordered on the way)
    let first = std::fs::read_to_string(root.join(&probe)).unwrap();
    b.call("save_scene", &Json::obj([("path", Json::str(probe.clone()))])).unwrap();
    let second = std::fs::read_to_string(root.join(&probe)).unwrap();
    assert!(first == second, "saving twice gave different text ({} and {} bytes)", first.len(), second.len());
}
