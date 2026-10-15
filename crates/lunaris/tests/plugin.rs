//! Native plugins: the Rust sample plugin (examples/plugins/swarm) is built for real, loaded by the engine and compared with the
//! graph system that does the same thing. They must agree bit for bit, on any thread count, and the safety rules must hold:
//! plugins are off unless the host allows them, and must live inside the workspace.
use lunaris_api::Engine;
use lunaris_json::{parse, Json};
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn library_name() -> &'static str {
    if cfg!(windows) {
        "swarm.dll"
    } else if cfg!(target_os = "macos") {
        "libswarm.dylib"
    } else {
        "libswarm.so"
    }
}

/// Build the sample plugin (once per test run; cargo caches it) and return its path relative to the repository root.
fn build_plugin() -> String {
    let root = repo_root();
    let target = root.join("target").join("plugin-swarm");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out = Command::new(cargo)
        .current_dir(&root)
        .args(["build", "--release", "--offline", "--manifest-path", "examples/plugins/swarm/Cargo.toml", "--target-dir"])
        .arg(&target)
        .output()
        .expect("run cargo");
    assert!(out.status.success(), "building the plugin failed:\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(target.join("release").join(library_name()).exists(), "the plugin library was not produced");
    format!("target/plugin-swarm/release/{}", library_name())
}

/// 6,000 entities on a ring with different velocities, a target at the origin, the seek constant and an unrelated bystander.
fn scene() -> Json {
    let mut entities = Vec::new();
    for i in 0..6000u64 {
        let a = i as f64 * 0.37;
        // every 30th entity starts almost on the target, so the despawn path is exercised from the first tick
        let r = if i % 30 == 0 { 0.05 } else { 5.0 + (i % 97) as f64 * 0.31 };
        entities.push(format!(
            r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},0,{}]}},"Velocity":{{"lin":[{},0,{}]}}}}}}"#,
            i + 1,
            r * a.cos(),
            r * a.sin(),
            (i % 7) as f64 * 0.1 - 0.3,
            (i % 5) as f64 * 0.1 - 0.2
        ));
    }
    parse(&format!(r#"{{"lunaris":1,"next_uid":7000,"resources":{{"target.x":0,"target.z":0,"seek.k":2.5}},"entities":[{}]}}"#, entities.join(","))).unwrap()
}

fn engine(allow: bool) -> Engine {
    let mut e = Engine::new(7, repo_root());
    e.allow_plugins(allow);
    e.call("load_scene", &Json::obj([("scene", scene())])).expect("load the scene");
    e
}

fn run(e: &mut Engine, ticks: u64) {
    e.call("step", &Json::obj([("ticks", Json::from(ticks as i64))])).expect("step");
}

fn world_hash(e: &Engine) -> u64 {
    e.sim.world.state_hash()
}

#[test]
fn the_native_plugin_matches_the_graph_system_bit_for_bit_on_any_thread_count() {
    let lib = build_plugin();
    let graph_def = parse(&std::fs::read_to_string(repo_root().join("examples/plugins/swarm_graph.json")).unwrap()).unwrap();
    let plugin_def = |parallel: bool| {
        parse(&format!(
            r#"{{"name":"seek_native","plugin":"{lib}","parallel":{parallel},"each":["Transform","Velocity"],
                "fields":["Transform.pos[0]","Transform.pos[2]","Velocity.lin[0]","Velocity.lin[2]"],
                "writes":["Velocity.lin[0]","Velocity.lin[2]"],"res":["target.x","target.z","seek.k"]}}"#
        ))
        .unwrap()
    };

    // the reference: the graph system, interpreted
    let mut reference = engine(false);
    reference.sim.threads = 1;
    reference.call("define_system", &graph_def).expect("graph system");
    run(&mut reference, 90);
    let want = world_hash(&reference);
    let left = reference.sim.world.len();
    assert!(left <= 5800 && left > 3000, "the 200 that started on the target were despawned and most others are still on their way: {left} left");

    for (threads, parallel) in [(1usize, false), (1, true), (4, true), (8, true)] {
        let mut e = engine(true);
        e.sim.threads = threads;
        e.call("define_system", &plugin_def(parallel)).expect("plugin system");
        let described = e.call("systems", &Json::Obj(Vec::new())).unwrap().to_string_lossy();
        assert!(described.contains("\"kind\":\"plugin\""), "{described}");
        run(&mut e, 90);
        assert_eq!(e.sim.world.len(), left, "{threads} threads, parallel {parallel}");
        assert_eq!(world_hash(&e), want, "the plugin and the graph system must agree exactly ({threads} threads, parallel {parallel})");
        assert!(e.sim.scripts.get("seek_native").unwrap().last_error.is_none(), "{:?}", e.sim.scripts.get("seek_native").unwrap().last_error);
    }
}

#[test]
fn plugins_are_off_unless_the_host_allows_them_and_bad_plugins_are_explained() {
    let lib = build_plugin();
    let def = |plugin: &str, export: &str, fields: &str, writes: &str| {
        parse(&format!(r#"{{"name":"p","plugin":"{plugin}","export":"{export}","each":["Transform","Velocity"],"fields":{fields},"writes":{writes},"res":["target.x","target.z","seek.k"]}}"#)).unwrap()
    };
    let good = |p: &str| def(p, "lunaris_update", r#"["Transform.pos[0]","Transform.pos[2]","Velocity.lin[0]","Velocity.lin[2]"]"#, r#"["Velocity.lin[0]"]"#);

    // switched off by default: a client of the tools cannot turn native code on
    let mut off = engine(false);
    let err = off.call("define_system", &good(&lib)).expect_err("plugins are off");
    assert!(err.message.contains("switched off") && err.hint.as_deref().unwrap_or("").contains("--allow-plugins"), "{err}");

    let mut on = engine(true);
    for (what, d, needle) in [
        ("outside the workspace", good("../outside.dll"), "relative path inside the workspace"),
        ("an absolute path", good(&Path::new(&repo_root()).join("target").display().to_string().replace('\\', "/")), "relative path inside the workspace"),
        ("a missing file", good("plugins/none.dll"), "cannot read the plugin"),
        ("a wrong export name", def(&lib, "no_such_function", r#"["Transform.pos[0]","Transform.pos[2]","Velocity.lin[0]","Velocity.lin[2]"]"#, "[]"), "does not export a function named `no_such_function`"),
        ("a write that is not a field", def(&lib, "lunaris_update", r#"["Transform.pos[0]"]"#, r#"["Velocity.lin[0]"]"#), "written but not listed in `fields`"),
        ("an unknown field", def(&lib, "lunaris_update", r#"["Transform.nope"]"#, "[]"), "has no field"),
    ] {
        let err = on.call("define_system", &d).expect_err(what);
        assert!(err.message.contains(needle) || err.hint.as_deref().unwrap_or("").contains(needle), "{what}: {err}");
    }
    // a file that is not a plugin at all
    std::fs::create_dir_all(repo_root().join("target").join("plugin-test")).unwrap();
    std::fs::write(repo_root().join("target").join("plugin-test").join("junk.dll"), b"this is not a library").unwrap();
    let err = on.call("define_system", &good("target/plugin-test/junk.dll")).expect_err("junk");
    assert!(err.message.contains("could not load") || err.message.contains("cannot"), "{err}");

    // a plugin that refuses (wrong field count: it returns error code 1) is recorded as the system's error, the world is untouched
    let mut e = engine(true);
    e.call("define_system", &def(&lib, "lunaris_update", r#"["Transform.pos[0]"]"#, "[]")).expect("defines fine");
    run(&mut e, 1);
    let msg = e.sim.scripts.get("p").unwrap().last_error.clone().expect("the plugin's refusal is recorded");
    assert!(msg.contains("returned error code 1"), "{msg}");
}
