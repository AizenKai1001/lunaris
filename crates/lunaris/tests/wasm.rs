//! Sandboxed WebAssembly systems: the Rust sample module (examples/wasm/swarm.wasm, 900 bytes, committed) must agree with the graph
//! system that does the same thing bit for bit, and the sandbox must hold: no imports but `lunaris.rand`, a fuel budget that ends
//! endless loops, a memory cap, paths inside the workspace, and no state that survives from one tick to the next. Needs no permission
//! switch and no toolchain (the module is a file; `tools/build_wasm.py` rebuilds it).
use lunaris_api::Engine;
use lunaris_json::{parse, Json};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// 6,000 entities on a ring with different velocities, a target at the origin, the seek constant (like the native plugin test).
fn scene() -> Json {
    let mut entities = Vec::new();
    for i in 0..6000u64 {
        let a = i as f64 * 0.37;
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

fn engine() -> Engine {
    let mut e = Engine::new(7, repo_root());
    e.call("load_scene", &Json::obj([("scene", scene())])).expect("load the scene");
    e
}

fn run(e: &mut Engine, ticks: u64) {
    e.call("step", &Json::obj([("ticks", Json::from(ticks as i64))])).expect("step");
}

fn swarm_def(path: &str, extra: &str) -> Json {
    parse(&format!(
        r#"{{"name":"seek_wasm","wasm":"{path}"{extra},"each":["Transform","Velocity"],
            "fields":["Transform.pos[0]","Transform.pos[2]","Velocity.lin[0]","Velocity.lin[2]"],
            "writes":["Velocity.lin[0]","Velocity.lin[2]"],"res":["target.x","target.z","seek.k"]}}"#
    ))
    .unwrap()
}

#[test]
fn the_wasm_module_matches_the_graph_system_bit_for_bit_on_any_thread_count() {
    let graph_def = parse(&std::fs::read_to_string(repo_root().join("examples/plugins/swarm_graph.json")).unwrap()).unwrap();
    let mut reference = engine();
    reference.sim.threads = 1;
    reference.call("define_system", &graph_def).expect("graph system");
    run(&mut reference, 90);
    let want = reference.sim.world.state_hash();
    let left = reference.sim.world.len();
    assert!(left <= 5800 && left > 3000, "{left}");

    for threads in [1usize, 4, 8] {
        let mut e = engine();
        e.sim.threads = threads;
        e.call("define_system", &swarm_def("examples/wasm/swarm.wasm", "")).expect("wasm system");
        let described = e.call("systems", &Json::Obj(Vec::new())).unwrap().to_string_lossy();
        assert!(described.contains("\"kind\":\"wasm\""), "{described}");
        run(&mut e, 90);
        assert_eq!(e.sim.world.len(), left, "{threads} threads");
        assert_eq!(e.sim.world.state_hash(), want, "the sandboxed module and the graph system must agree exactly ({threads} threads)");
        assert!(e.sim.scripts.get("seek_wasm").unwrap().last_error.is_none(), "{:?}", e.sim.scripts.get("seek_wasm").unwrap().last_error);
    }
}

#[test]
fn a_recording_with_a_wasm_system_replays_to_the_same_state() {
    let mut e = engine();
    e.call("define_system", &swarm_def("examples/wasm/swarm.wasm", "")).unwrap();
    e.call("record_start", &Json::Obj(Vec::new())).unwrap();
    run(&mut e, 40);
    let live = e.sim.world.state_hash();
    e.call("record_stop", &Json::Obj(Vec::new())).unwrap();
    let r = e.call("replay", &Json::Obj(Vec::new())).expect("replay");
    assert_eq!(r.get("matches"), Some(&Json::Bool(true)), "{}", r.to_string_lossy());
    assert_eq!(e.sim.world.state_hash(), live);
}

/// Write a module in the text format into the workspace and return its path (the host accepts `.wat` as well as binary modules).
fn wat(name: &str, text: &str) -> String {
    let dir = repo_root().join("target").join("wasm-test");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), text).unwrap();
    format!("target/wasm-test/{name}")
}

const GOOD_PRELUDE: &str = r#"(memory (export "memory") 1) (func (export "lunaris_wasm_abi") (result i32) i32.const 1)"#;

fn define(e: &mut Engine, path: &str, extra: &str) -> Result<Json, lunaris_ecs::EcsError> {
    e.call("define_system", &swarm_def(path, extra))
}

#[test]
fn the_sandbox_refuses_what_a_module_must_not_do() {
    let mut e = engine();
    // outside the workspace, absolute, missing
    for (what, path, needle) in [("outside", "../x.wasm", "relative path inside the workspace"), ("absolute", "C:/Windows/notepad.exe", "relative path inside the workspace"), ("missing", "wasm/none.wasm", "cannot read the module")] {
        let err = define(&mut e, path, "").expect_err(what);
        assert!(err.message.contains(needle), "{what}: {err}");
    }
    // not WebAssembly at all
    let junk = wat("junk.wasm", "this is not a module");
    assert!(define(&mut e, &junk, "").expect_err("junk").message.contains("not a valid WebAssembly module"));
    // an import other than lunaris.rand: the module could otherwise talk to the outside world
    let bad_import = wat("import.wat", &format!(r#"(module (import "env" "printf" (func (param i32))) {GOOD_PRELUDE} (func (export "lunaris_update") (param i32) (result i32) i32.const 0))"#));
    let err = define(&mut e, &bad_import, "").expect_err("imports are refused");
    assert!(err.message.contains("may import only `lunaris.rand`"), "{err}");
    // a missing export
    let no_update = wat("noupdate.wat", &format!(r#"(module {GOOD_PRELUDE})"#));
    assert!(define(&mut e, &no_update, "").expect_err("no update").message.contains("must export the update function"));
    let no_memory = wat("nomem.wat", r#"(module (func (export "lunaris_wasm_abi") (result i32) i32.const 1) (func (export "lunaris_update") (param i32) (result i32) i32.const 0))"#);
    assert!(define(&mut e, &no_memory, "").expect_err("no memory").message.contains("`memory`"));
    // a module asking for more memory than the cap fails to start
    let big = wat("big.wat", &format!(r#"(module (memory (export "memory") 2000) (func (export "lunaris_wasm_abi") (result i32) i32.const 1) (func (export "lunaris_update") (param i32) (result i32) i32.const 0))"#));
    let _ = define(&mut e, &big, ""); // defining may or may not fail; running must not take the memory
    run(&mut e, 1);
    assert!(e.sim.world.len() == 6000, "the world is untouched");
    // a wrong interface version is explained when the system first runs
    let v2 = wat("v2.wat", r#"(module (memory (export "memory") 1) (func (export "lunaris_wasm_abi") (result i32) i32.const 2) (func (export "lunaris_update") (param i32) (result i32) i32.const 0))"#);
    let mut e2 = engine();
    define(&mut e2, &v2, "").expect("defines");
    run(&mut e2, 1);
    let msg = e2.sim.scripts.get("seek_wasm").unwrap().last_error.clone().expect("recorded");
    assert!(msg.contains("interface version 2"), "{msg}");
}

#[test]
fn an_endless_loop_runs_out_of_fuel_instead_of_hanging_and_a_trap_is_an_error_not_a_crash() {
    let spin = wat("spin.wat", &format!(r#"(module {GOOD_PRELUDE} (func (export "lunaris_update") (param i32) (result i32) (loop $l (br $l)) i32.const 0))"#));
    let mut e = engine();
    define(&mut e, &spin, r#","fuel":200000"#).expect("defines");
    let t = std::time::Instant::now();
    run(&mut e, 1);
    assert!(t.elapsed().as_secs() < 10, "the loop must end quickly");
    let msg = e.sim.scripts.get("seek_wasm").unwrap().last_error.clone().expect("the fuel error is recorded");
    assert!(msg.contains("used more than 200000 instructions"), "{msg}");
    assert_eq!(e.sim.world.len(), 6000, "a failed system changes nothing");
}

#[test]
fn a_trap_is_an_error_not_a_crash() {
    let trap = wat("trap.wat", &format!(r#"(module {GOOD_PRELUDE} (func (export "lunaris_update") (param i32) (result i32) unreachable))"#));
    let mut e = engine();
    define(&mut e, &trap, "").unwrap();
    run(&mut e, 1);
    assert!(e.sim.scripts.get("seek_wasm").unwrap().last_error.clone().expect("trap").contains("failed in lunaris_update"));
}

#[test]
fn a_nonzero_result_is_the_systems_error_and_absurd_fuel_is_refused() {
    let fail = wat("fail.wat", &format!(r#"(module {GOOD_PRELUDE} (func (export "lunaris_update") (param i32) (result i32) i32.const 7))"#));
    let mut e = engine();
    define(&mut e, &fail, "").unwrap();
    run(&mut e, 1);
    assert!(e.sim.scripts.get("seek_wasm").unwrap().last_error.clone().expect("code").contains("error code 7"));
    // an out-of-range fuel is refused when the system is defined
    assert!(define(&mut e, &fail, r#","fuel":5"#).expect_err("fuel").message.contains("`fuel`"));
}

#[test]
fn a_hand_written_module_in_the_text_format_works_and_state_does_not_survive_a_tick() {
    // the module counts calls in a global and writes the count into Velocity.lin[0]: if state survived between ticks the value would
    // keep growing; every tick starts a fresh instance, so each entity batch sees 1 on the first batch of a tick
    let counter = wat(
        "counter.wat",
        r#"(module
             (memory (export "memory") 1)
             (global $calls (mut i32) (i32.const 0))
             (func (export "lunaris_wasm_abi") (result i32) i32.const 1)
             (func (export "lunaris_update") (param $b i32) (result i32)
               (local $table i32) (local $count i32) (local $i i32) (local $vx i32)
               (global.set $calls (i32.add (global.get $calls) (i32.const 1)))
               (local.set $count (i32.load offset=8 (local.get $b)))
               (local.set $table (i32.load offset=52 (local.get $b)))
               (local.set $vx (i32.load (local.get $table)))
               (local.set $i (i32.const 0))
               (block $done (loop $each
                 (br_if $done (i32.ge_u (local.get $i) (local.get $count)))
                 (f64.store (i32.add (local.get $vx) (i32.shl (local.get $i) (i32.const 3))) (f64.convert_i32_u (global.get $calls)))
                 (local.set $i (i32.add (local.get $i) (i32.const 1)))
                 (br $each)))
               i32.const 0))"#,
    );
    let mut e = Engine::new(1, repo_root());
    e.call("load_scene", &Json::obj([("scene", parse(r#"{"lunaris":1,"entities":[{"uid":1,"components":{"Velocity":{}}}]}"#).unwrap())])).unwrap();
    e.call(
        "define_system",
        &parse(&format!(r#"{{"name":"count","wasm":"{counter}","each":["Velocity"],"fields":["Velocity.lin[0]"],"writes":["Velocity.lin[0]"]}}"#)).unwrap(),
    )
    .expect("a .wat file is accepted");
    for _ in 0..3 {
        run(&mut e, 1);
        let v = e.call("get", &parse(r#"{"uid":1,"components":["Velocity"]}"#).unwrap()).unwrap();
        let x = v.get("components").unwrap().get("Velocity").unwrap().get("lin").unwrap().index(0).unwrap().as_f64().unwrap();
        assert_eq!(x, 1.0, "the global starts at 0 in every tick, so the first (only) call counts 1");
    }
}

/// Not a pass/fail test: how long one tick of 200,000 entities takes as a graph system (batch vm), a sandboxed module, and with no
/// system (`cargo test --release -p lunaris --test wasm how_fast -- --nocapture`).
#[test]
fn how_fast_is_the_sandbox() {
    let mut entities = Vec::new();
    for i in 0..200_000u64 {
        let a = i as f64 * 0.37;
        let r = 5.0 + (i % 97) as f64 * 0.31;
        entities.push(format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},0,{}]}},"Velocity":{{"lin":[0.1,0,0.2]}}}}}}"#, i + 1, r * a.cos(), r * a.sin()));
    }
    let scene = parse(&format!(r#"{{"lunaris":1,"next_uid":300000,"resources":{{"target.x":0,"target.z":0,"seek.k":0.01}},"entities":[{}]}}"#, entities.join(","))).unwrap();
    let graph_def = parse(&std::fs::read_to_string(repo_root().join("examples/plugins/swarm_graph.json")).unwrap()).unwrap();
    let time = |def: Option<&Json>| {
        let mut e = Engine::new(7, repo_root());
        e.sim.threads = 1;
        e.call("load_scene", &Json::obj([("scene", scene.clone())])).unwrap();
        if let Some(d) = def {
            e.call("define_system", d).unwrap();
        }
        run(&mut e, 2); // warm up
        let t = std::time::Instant::now();
        run(&mut e, 10);
        t.elapsed().as_secs_f64() * 1000.0 / 10.0
    };
    let base = time(None);
    let graph = time(Some(&graph_def)) - base;
    let wasm = time(Some(&swarm_def("examples/wasm/swarm.wasm", ""))) - base;
    println!("200,000 entities, one thread: a tick without the system {base:.2} ms; the graph system (batch vm) adds {graph:.2} ms; the sandboxed WebAssembly module adds {wasm:.2} ms ({:.1}x the vm)", wasm / graph);
}
