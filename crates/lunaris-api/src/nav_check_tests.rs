//! `check` says what a crowd of path finders needs on a server: `NavLimits`, and a cap on a search that cannot succeed. A few agents, or a world that has both, hear nothing.
use crate::tests::engine;
use crate::Engine;
use lunaris_json::{parse, Json};

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &parse(args).unwrap()).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn crowd(e: &mut Engine, n: u64) {
    for i in 0..n {
        call(e, "spawn", &format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{i},0,0]}},"CharacterBody":{{}},"NavAgent":{{}}}}}}"#, 100 + i));
    }
}

fn messages(e: &mut Engine) -> Vec<String> {
    let c = call(e, "check", "{}");
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| format!("{}: {}", p.get("severity").unwrap().as_str().unwrap(), p.get("message").unwrap().as_str().unwrap())).collect()
}

#[test]
fn a_crowd_of_path_finders_with_no_limits_is_told_so() {
    let mut e = engine();
    crowd(&mut e, 25);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("25 NavAgents and no NavLimits")), "{m:?}");
    // a few agents are nobody's problem
    let mut few = engine();
    crowd(&mut few, 5);
    let m = messages(&mut few);
    assert!(!m.iter().any(|x| x.contains("NavLimits")), "{m:?}");
}

#[test]
fn limits_with_no_cap_on_a_failed_search_are_told_so_and_complete_limits_say_nothing() {
    let mut e = engine();
    crowd(&mut e, 25);
    call(&mut e, "spawn", r#"{"uid":50,"components":{"NavLimits":{"searches_per_tick":2}}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("no NavLimits")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("max_searched is 0 with 25 NavAgents")), "{m:?}");
    call(&mut e, "set", r#"{"uid":50,"component":"NavLimits","value":{"max_searched":20000}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("NavLimits")), "{m:?}");
}
