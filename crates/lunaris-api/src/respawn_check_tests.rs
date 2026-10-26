//! `check` names the mistakes of a respawning player: no hit points, a corpse prefab that does not exist, no spawn point to use; a good setup says nothing.
use crate::tests::engine;
use crate::Engine;
use lunaris_json::{parse, Json};

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &parse(args).unwrap()).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn spawn(e: &mut Engine, uid: u64, components: &str) {
    call(e, "spawn", &format!(r#"{{"uid":{uid},"components":{components}}}"#));
}

fn messages(e: &mut Engine) -> Vec<String> {
    let c = call(e, "check", "{}");
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| format!("{}: {}", p.get("severity").unwrap().as_str().unwrap(), p.get("message").unwrap().as_str().unwrap())).collect()
}

#[test]
fn a_good_respawning_player_says_nothing() {
    let mut e = engine();
    call(&mut e, "define_prefab", r#"{"name":"corpse","components":{"Name":{"value":"Corpse"}}}"#);
    spawn(&mut e, 20, r#"{"Transform":{},"SpawnPoint":{}}"#);
    spawn(&mut e, 10, r#"{"Transform":{},"HitPoints":{},"Respawn":{"corpse":"corpse"}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("Respawn")), "{m:?}");
}

#[test]
fn the_mistakes_of_a_respawn_are_named() {
    let mut e = engine();
    spawn(&mut e, 10, r#"{"Transform":{},"Respawn":{"corpse":"nothing_like_it","delay":-1}}"#);
    spawn(&mut e, 11, r#"{"Transform":{},"HitPoints":{},"Respawn":{"group":5}}"#);
    // a point that belongs to somebody else is no use to them
    spawn(&mut e, 20, r#"{"Transform":{},"SpawnPoint":{"group":9}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("needs HitPoints")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Respawn.corpse `nothing_like_it`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("no active SpawnPoint")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("delay and avoid are negative")), "{m:?}");
}
