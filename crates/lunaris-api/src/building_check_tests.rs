//! `check` names the mistakes of the building rules and pieces: a rules text that does not read, a prefab that is not there, pieces and tools of kinds the rules do not have, sockets nothing plugs into,
//! pieces or tools with no rules at all; a good setup says nothing.
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

const GOOD: &str = "piece floor size=4,0.5,4 free=1 anchor=1\\n  socket e at=2,0.25,0 type=edge mates=edge\\npiece wall size=4,3,0.2\\n  socket base at=0,-1.5,0 type=wall_base mates=edge\\n";

#[test]
fn good_rules_pieces_tools_and_sockets_say_nothing() {
    let mut e = engine();
    spawn(&mut e, 1, &format!(r#"{{"BuildRules":{{"text":"{GOOD}"}}}}"#));
    spawn(&mut e, 2, r#"{"Transform":{},"BuildPiece":{"kind":"floor"}}"#);
    spawn(&mut e, 3, r#"{"Transform":{},"BuildTool":{"kind":"wall"}}"#);
    spawn(&mut e, 4, r#"{"Transform":{},"BuildSocket":{"kind":"edge"}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("Build")), "{m:?}");
}

#[test]
fn the_mistakes_of_building_are_named() {
    let mut e = engine();
    spawn(&mut e, 1, r#"{"BuildRules":{"text":"piece p size=1,1\n  socket s at=0,0,0\n"}}"#);
    spawn(&mut e, 2, r#"{"Transform":{},"BuildPiece":{"kind":"ghost_piece"}}"#);
    spawn(&mut e, 3, r#"{"Transform":{},"BuildTool":{"kind":"nonsense"}}"#);
    spawn(&mut e, 4, r#"{"Transform":{},"BuildSocket":{"kind":"nobody_mates_this"}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("BuildRules:") && x.contains("size=1,1")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("BuildRules:") && x.contains("no type")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("BuildPiece.kind `ghost_piece`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("BuildTool.kind `nonsense`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("BuildSocket.kind `nobody_mates_this`")), "{m:?}");
    // pieces and tools with no rules at all
    let mut e = engine();
    spawn(&mut e, 2, r#"{"Transform":{},"BuildTool":{"kind":"wall"}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("no BuildRules")), "{m:?}");
}
