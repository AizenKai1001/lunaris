//! `check` says what a streamed world needs: a limit on the zones loaded in a tick, and a procedural grid that does not look at thousands of cells round every anchor. A small world, or one that has both, hears nothing.
use crate::tests::engine;
use crate::Engine;
use lunaris_json::{parse, Json};

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &parse(args).unwrap()).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn messages(e: &mut Engine) -> Vec<String> {
    let c = call(e, "check", "{}");
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| format!("{}: {}", p.get("severity").unwrap().as_str().unwrap(), p.get("message").unwrap().as_str().unwrap())).collect()
}

fn zones(e: &mut Engine, from: u64, n: u64) {
    call(e, "define_prefab", r#"{"name":"block","components":{"Transform":{}}}"#);
    for i in from..from + n {
        call(e, "spawn", &format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},0,0]}},"StreamZone":{{"prefab":"block","radius":50}}}}}}"#, 1000 + i, i * 100));
    }
}

#[test]
fn many_zones_or_a_grid_with_no_limit_on_the_loads_are_told_so_and_a_limit_quiets_it() {
    let mut e = engine();
    zones(&mut e, 0, 5);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("stream.loads_per_tick")), "a few zones are nobody's problem: {m:?}");
    zones(&mut e, 5, 30);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("StreamZones with no limit")), "{m:?}");
    call(&mut e, "set_resource", r#"{"name":"stream.loads_per_tick","value":3}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("stream.loads_per_tick")), "{m:?}");
    // a grid with no zones yet is told too, until the limit is there
    let mut g = engine();
    call(&mut g, "spawn", r#"{"uid":5,"components":{"ProcGrid":{"grid":"city","cell":100,"radius":300}}}"#);
    let m = messages(&mut g);
    assert!(m.iter().any(|x| x.contains("a ProcGrid with no limit")), "{m:?}");
}

#[test]
fn a_grid_that_looks_at_thousands_of_cells_round_an_anchor_is_told_so() {
    let mut e = engine();
    call(&mut e, "set_resource", r#"{"name":"stream.loads_per_tick","value":3}"#);
    call(&mut e, "spawn", r#"{"uid":5,"components":{"ProcGrid":{"grid":"far","cell":100,"radius":300}}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("cells round every anchor")), "{m:?}");
    call(&mut e, "spawn", r#"{"uid":6,"components":{"ProcGrid":{"grid":"huge","cell":10,"radius":3000}}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("cells round every anchor") && x.contains("radius 3000")), "{m:?}");
}
