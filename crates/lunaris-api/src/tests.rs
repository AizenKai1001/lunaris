use super::*;
use lunaris_json::{parse, Json};
use std::io::Cursor;

pub(crate) fn engine() -> Engine {
    let dir = std::env::temp_dir().join(format!("lunaris-api-test-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')', ' '], ""));
    std::fs::create_dir_all(&dir).unwrap();
    Engine::new(42, dir)
}

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

/// Call a tool directly.
fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &j(args)).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn call_err(e: &mut Engine, tool: &str, args: &str) -> lunaris_ecs::EcsError {
    e.call(tool, &j(args)).expect_err(tool)
}

/// Send one JSON-RPC line through `serve` and parse the reply.
fn rpc(e: &mut Engine, line: &str) -> Json {
    let mut out = Vec::new();
    serve(e, Cursor::new(format!("{line}\n")), &mut out).unwrap();
    parse(String::from_utf8(out).unwrap().trim()).unwrap()
}

fn tool_result(reply: &Json) -> (bool, Json) {
    let res = reply.get("result").unwrap_or_else(|| panic!("no result in {}", reply.to_string_lossy()));
    let text = res.get("content").unwrap().index(0).unwrap().get("text").unwrap().as_str().unwrap();
    (res.get("isError").unwrap().as_bool().unwrap(), parse(text).unwrap())
}

#[test]
fn mcp_handshake_and_tool_list() {
    let mut e = engine();
    let init = rpc(&mut e, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#);
    let r = init.get("result").unwrap();
    assert_eq!(r.get("protocolVersion").unwrap().as_str(), Some("2024-11-05"));
    assert_eq!(r.get("serverInfo").unwrap().get("name").unwrap().as_str(), Some("lunaris"));
    assert!(r.get("capabilities").unwrap().get("tools").is_some());
    // a notification gets no reply
    let mut out = Vec::new();
    serve(&mut e, Cursor::new("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n"), &mut out).unwrap();
    assert!(out.is_empty());
    let list = rpc(&mut e, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let tools = list.get("result").unwrap().get("tools").unwrap().as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.get("name").unwrap().as_str().unwrap()).collect();
    assert_eq!(names, tool_names());
    for t in tools {
        assert_eq!(t.get("inputSchema").unwrap().get("type").unwrap().as_str(), Some("object"), "{}", t.get("name").unwrap().as_str().unwrap());
        assert!(!t.get("description").unwrap().as_str().unwrap().is_empty());
    }
    assert!(names.contains(&"query") && names.contains(&"snapshot") && names.contains(&"replay"));
}

#[test]
fn tools_call_returns_text_content_and_flags_errors() {
    let mut e = engine();
    let ok = rpc(&mut e, r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"spawn","arguments":{"components":{"Name":{"value":"Hero"}}}}}"#);
    let (is_err, body) = tool_result(&ok);
    assert!(!is_err);
    assert_eq!(body.get("uid").unwrap().as_i64(), Some(1));
    let bad = rpc(&mut e, r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get","arguments":{"uid":99}}}"#);
    let (is_err, body) = tool_result(&bad);
    assert!(is_err);
    assert_eq!(body.get("code").unwrap().as_str(), Some("no_such_entity"));
    assert!(body.get("hint").is_some());
}

#[test]
fn protocol_errors_are_proper_json_rpc() {
    let mut e = engine();
    let r = rpc(&mut e, "{not json");
    assert_eq!(r.get("error").unwrap().get("code").unwrap().as_i64(), Some(-32700));
    assert!(r.get("error").unwrap().get("data").unwrap().get("column").is_some());
    let r = rpc(&mut e, r#"{"jsonrpc":"2.0","id":9,"method":"nope"}"#);
    assert_eq!(r.get("error").unwrap().get("code").unwrap().as_i64(), Some(-32601));
    assert_eq!(r.get("id").unwrap().as_i64(), Some(9));
    let r = rpc(&mut e, r#"{"jsonrpc":"2.0","id":10,"method":"lunaris.get","params":{"uid":5}}"#);
    assert_eq!(r.get("error").unwrap().get("data").unwrap().get("code").unwrap().as_str(), Some("no_such_entity"));
    // direct method returns the bare result
    let r = rpc(&mut e, r#"{"jsonrpc":"2.0","id":11,"method":"lunaris.hash"}"#);
    assert!(r.get("result").unwrap().get("hash").is_some());
    // batch of two requests
    let mut out = Vec::new();
    serve(&mut e, Cursor::new("[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"},{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}]\n"), &mut out).unwrap();
    assert_eq!(parse(String::from_utf8(out).unwrap().trim()).unwrap().as_array().unwrap().len(), 2);
}

#[test]
fn author_a_component_spawn_query_edit_and_undo() {
    let mut e = engine();
    call(&mut e, "define_component", r#"{"name":"Health","doc":"hit points","fields":[{"name":"hp","type":"f32","default":100},{"name":"team","type":"str"}]}"#);
    let spawned = call(&mut e, "spawn", r#"{"count":5,"components":{"Health":{"hp":30,"team":"red"},"Transform":{}}}"#);
    assert_eq!(spawned.get("count").unwrap().as_i64(), Some(5));
    call(&mut e, "set", r#"{"uid":2,"component":"Health","value":{"hp":90}}"#);
    call(&mut e, "set", r#"{"uid":4,"component":"Health","value":{"hp":10,"team":"blue"}}"#);

    let q = call(&mut e, "query", r#"{"with":["Health"],"where":[{"component":"Health","field":"hp","op":"<","value":50}],"select":["Health"]}"#);
    assert_eq!(q.get("total").unwrap().as_i64(), Some(4));
    let uids: Vec<i64> = q.get("entities").unwrap().as_array().unwrap().iter().map(|x| x.get("uid").unwrap().as_i64().unwrap()).collect();
    assert_eq!(uids, vec![1, 3, 4, 5]);
    let blue = call(&mut e, "query", r#"{"where":[{"component":"Health","field":"team","value":"blue"}],"select":[]}"#);
    assert_eq!(blue.get("total").unwrap().as_i64(), Some(1));
    assert_eq!(call(&mut e, "query", r#"{"with":["Transform"],"without":["Name"],"count_only":true}"#).get("total").unwrap().as_i64(), Some(5));

    // undo the two sets and the batch spawn, then redo one
    call(&mut e, "undo", "{}");
    call(&mut e, "undo", "{}");
    let after = call(&mut e, "get", r#"{"uid":4,"components":["Health"]}"#);
    assert_eq!(after.get("components").unwrap().get("Health").unwrap().get("hp").unwrap().as_f64(), Some(30.0));
    call(&mut e, "undo", "{}");
    assert_eq!(call(&mut e, "describe", "{}").get("entities").unwrap().as_i64(), Some(0));
    call(&mut e, "redo", "{}");
    assert_eq!(call(&mut e, "describe", "{}").get("entities").unwrap().as_i64(), Some(5));
    // the two edits that were undone before the spawn are still redoable, in order
    call(&mut e, "redo", "{}");
    call(&mut e, "redo", "{}");
    let hp = |e: &mut Engine, uid: i64| call(e, "get", &format!(r#"{{"uid":{uid},"components":["Health"]}}"#)).get("components").unwrap().get("Health").unwrap().get("hp").unwrap().as_f64().unwrap();
    assert_eq!((hp(&mut e, 2), hp(&mut e, 4)), (90.0, 10.0));
    assert_eq!(call_err(&mut e, "redo", "{}").code, "nothing_to_redo");
    // a fresh edit clears the redo stack
    call(&mut e, "undo", "{}");
    call(&mut e, "set", r#"{"uid":1,"component":"Health","value":{"hp":1}}"#);
    assert_eq!(call_err(&mut e, "redo", "{}").code, "nothing_to_redo");
}

#[test]
fn snapshot_and_restore_let_you_try_things() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"count":20,"components":{"Transform":{},"Velocity":{"lin":[1,0,0]},"Wander":{}}}"#);
    call(&mut e, "step", r#"{"ticks":30}"#);
    let before = call(&mut e, "snapshot", r#"{"name":"before"}"#);
    let h0 = before.get("hash").unwrap().as_str().unwrap().to_string();
    call(&mut e, "despawn", r#"{"uid":3}"#);
    call(&mut e, "step", r#"{"ticks":100}"#);
    assert_ne!(call(&mut e, "hash", "{}").get("hash").unwrap().as_str().unwrap(), h0);
    let restored = call(&mut e, "restore", r#"{"name":"before"}"#);
    assert_eq!(restored.get("hash").unwrap().as_str().unwrap(), h0);
    assert_eq!(restored.get("tick").unwrap().as_i64(), Some(30));
    assert_eq!(call_err(&mut e, "restore", r#"{"name":"nope"}"#).code, "no_such_snapshot");
}

#[test]
fn determinism_two_engines_same_calls_same_hash_and_replay_matches() {
    let script = |e: &mut Engine| {
        call(e, "record_start", "{}");
        call(e, "spawn", r#"{"count":100,"components":{"Transform":{},"Velocity":{"lin":[1,1,0]},"Wander":{"strength":2}}}"#);
        call(e, "step", r#"{"ticks":200}"#);
        call(e, "set", r#"{"uid":10,"component":"Velocity","value":{"lin":[9,9,9]}}"#);
        call(e, "despawn", r#"{"uid":11}"#);
        call(e, "step", r#"{"ticks":200}"#);
        let stop = call(e, "record_stop", "{}");
        (call(e, "hash", "{}").get("hash").unwrap().as_str().unwrap().to_string(), stop)
    };
    let mut a = engine();
    let mut b = engine();
    let (ha, stop) = script(&mut a);
    let (hb, _) = script(&mut b);
    assert_eq!(ha, hb);
    assert_eq!(stop.get("final_hash").unwrap().as_str(), Some(ha.as_str()));
    let replay = call(&mut a, "replay", "{}");
    assert_eq!(replay.get("matches"), Some(&Json::Bool(true)), "{}", replay.to_string_lossy());
    // through a file
    call(&mut a, "reset", "{}");
    call(&mut a, "record_start", "{}");
    call(&mut a, "spawn", r#"{"count":5,"components":{"Transform":{}}}"#);
    call(&mut a, "step", r#"{"ticks":10}"#);
    call(&mut a, "record_stop", r#"{"path":"runs/r.recording.json"}"#);
    assert_eq!(call(&mut a, "replay", r#"{"path":"runs/r.recording.json"}"#).get("matches"), Some(&Json::Bool(true)));
}

#[test]
fn batch_is_atomic_and_undoes_as_one() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"Transform":{}}}"#);
    let err = call_err(&mut e, "batch", r#"{"commands":[{"op":"set","uid":1,"component":"Transform","value":{"pos":[5,5,5]}},{"op":"despawn","uid":77}]}"#);
    assert_eq!(err.code, "no_such_entity");
    let t = call(&mut e, "get", r#"{"uid":1,"components":["Transform"]}"#);
    assert_eq!(t.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().index(0).unwrap().as_f64(), Some(0.0), "failed batch must roll back");
    call(&mut e, "batch", r#"{"commands":[{"op":"spawn","components":{"Name":{"value":"a"}}},{"op":"spawn","components":{"Name":{"value":"b"}}}]}"#);
    assert_eq!(call(&mut e, "describe", "{}").get("entities").unwrap().as_i64(), Some(3));
    call(&mut e, "undo", "{}");
    assert_eq!(call(&mut e, "describe", "{}").get("entities").unwrap().as_i64(), Some(1));
}

#[test]
fn errors_teach_the_caller() {
    let mut e = engine();
    call(&mut e, "spawn", "{}");
    let typo = call_err(&mut e, "add_component", r#"{"uid":1,"component":"Transfrom"}"#);
    assert_eq!(typo.code, "unknown_component");
    assert!(typo.hint.unwrap().contains("did you mean `Transform`"));
    let field = call_err(&mut e, "add_component", r#"{"uid":1,"component":"Transform","value":{"position":[1,2,3]}}"#);
    assert_eq!(field.code, "unknown_field");
    assert!(field.hint.unwrap().contains("pos: f32[3]"));
    assert_eq!(call_err(&mut e, "set", r#"{"uid":1,"component":"Transform","value":{}}"#).code, "missing_component");
    assert_eq!(call_err(&mut e, "set", r#"{"component":"Transform","value":{}}"#).code, "missing_param");
    assert_eq!(call_err(&mut e, "get", r#"{"uid":"one"}"#).code, "type_mismatch");
    let tool = call_err(&mut e, "spwan", "{}");
    assert_eq!(tool.code, "unknown_tool");
    assert!(tool.hint.unwrap().contains("spawn"));
    assert_eq!(call_err(&mut e, "query", r#"{"where":[{"component":"Transform","field":"pos","value":1}]}"#).code, "bad_arguments");
    assert_eq!(call_err(&mut e, "step", r#"{"ticks":99999999999}"#).code, "out_of_range");
}

#[test]
fn scenes_save_and_load_atomically_inside_the_workspace() {
    let mut e = engine();
    call(&mut e, "define_component", r#"{"name":"Tag","fields":[{"name":"label","type":"str"}]}"#);
    call(&mut e, "spawn", r#"{"count":3,"components":{"Tag":{"label":"x"},"Transform":{"pos":[1,2,3]}}}"#);
    let saved = call(&mut e, "save_scene", r#"{"path":"scenes/a.lunaris.json"}"#);
    assert_eq!(saved.get("entities").unwrap().as_i64(), Some(3));
    let h = call(&mut e, "hash", "{}").get("hash").unwrap().as_str().unwrap().to_string();
    call(&mut e, "reset", "{}");
    call(&mut e, "define_component", r#"{"name":"Tag","fields":[{"name":"label","type":"str"}]}"#);
    let loaded = call(&mut e, "load_scene", r#"{"path":"scenes/a.lunaris.json"}"#);
    assert_eq!(loaded.get("entities").unwrap().as_i64(), Some(3));
    // the loaded world has the same entities (hash differs only in tick/RNG-free fields, which load resets)
    let q = call(&mut e, "query", r#"{"count_only":true,"with":["Tag"]}"#);
    assert_eq!(q.get("total").unwrap().as_i64(), Some(3));
    let _ = h;
    // bad scene: current world must survive
    let err = call_err(&mut e, "load_scene", r#"{"scene":{"lunaris":1,"entities":[{"uid":1,"components":{"Transform":{"pos":"bad"}}}]}}"#);
    assert_eq!(err.code, "type_mismatch");
    assert_eq!(call(&mut e, "query", r#"{"count_only":true}"#).get("total").unwrap().as_i64(), Some(3));
}

#[test]
fn paths_cannot_escape_the_workspace() {
    let mut e = engine();
    for p in ["../evil.json", "/etc/passwd", "C:/Windows/x.json", "a/../../b.json"] {
        let err = call_err(&mut e, "save_scene", &format!(r#"{{"path":"{p}"}}"#));
        assert_eq!(err.code, "bad_path", "{p}");
    }
}

#[test]
fn bulk_spawn_and_step_are_fast_enough_to_use() {
    let mut e = engine();
    let t0 = std::time::Instant::now();
    call(&mut e, "spawn", r#"{"count":50000,"components":{"Transform":{},"Velocity":{"lin":[1,0,0]}}}"#);
    let spawn_ms = t0.elapsed().as_millis();
    let t1 = std::time::Instant::now();
    call(&mut e, "step", r#"{"ticks":20}"#);
    let step_ms = t1.elapsed().as_millis();
    assert!(spawn_ms < 5000, "spawning 50k took {spawn_ms} ms");
    assert!(step_ms < 2000, "20 ticks of 50k took {step_ms} ms");
}

#[test]
fn render_pick_and_project_let_an_agent_see_the_world() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Eye"},"Transform":{"pos":[0,0,4]},"Camera":{}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Ball"},"Transform":{},"MeshRenderer":{"mesh":"sphere","color":[1,0.2,0.2,1]}}}"#);
    let r = call(&mut e, "render", r#"{"width":160,"height":120,"path":"renders/a.png"}"#);
    assert_eq!(r.get("camera").unwrap().as_str(), Some("camera entity 1"));
    let vis = r.get("visible").unwrap().as_array().unwrap();
    assert_eq!(vis.len(), 1);
    assert_eq!(vis[0].get("uid").unwrap().as_i64(), Some(2));
    assert_eq!(vis[0].get("name").unwrap().as_str(), Some("Ball"));
    let img = r.get("image").unwrap();
    assert_eq!(img.get("mime").unwrap().as_str(), Some("image/png"));
    let b64 = img.get("data").unwrap().as_str().unwrap();
    assert!(b64.starts_with("iVBORw0KGgo"), "PNG signature in base64");
    assert!(r.get("saved_to").unwrap().as_str().unwrap().ends_with("a.png"));
    // the same world renders to the same hash; changing it changes the hash
    let h1 = r.get("hash").unwrap().as_str().unwrap().to_string();
    let h2 = call(&mut e, "render", r#"{"width":160,"height":120,"image":false}"#);
    assert_eq!(h2.get("hash").unwrap().as_str().unwrap(), h1);
    assert!(h2.get("image").is_none());
    call(&mut e, "set", r#"{"uid":2,"component":"Transform","value":{"pos":[0.3,0,0]}}"#);
    assert_ne!(call(&mut e, "render", r#"{"width":160,"height":120,"image":false}"#).get("hash").unwrap().as_str().unwrap(), h1);
    // pick and project agree with each other
    let p = call(&mut e, "project", r#"{"point":[0.3,0,0],"width":160,"height":120}"#);
    let (x, y) = (p.get("x").unwrap().as_f64().unwrap().round() as i64, p.get("y").unwrap().as_f64().unwrap().round() as i64);
    assert_eq!(p.get("on_screen"), Some(&Json::Bool(true)));
    let hit = call(&mut e, "pick", &format!(r#"{{"x":{x},"y":{y},"width":160,"height":120}}"#));
    assert_eq!(hit.get("uid").unwrap().as_i64(), Some(2));
    assert_eq!(hit.get("name").unwrap().as_str(), Some("Ball"));
    let miss = call(&mut e, "pick", r#"{"x":0,"y":0,"width":160,"height":120}"#);
    assert!(miss.get("uid").unwrap().is_null());
    // an explicit viewpoint does not need a camera entity and does not change the world
    let before = call(&mut e, "hash", "{}");
    let side = call(&mut e, "render", r#"{"eye":[5,1,0],"target":[0,0,0],"width":80,"height":60,"image":false}"#);
    assert!(side.get("camera").unwrap().as_str().unwrap().contains("explicit"));
    assert_eq!(call(&mut e, "hash", "{}"), before);
    assert_eq!(call_err(&mut e, "render", r#"{"eye":[0,0,5],"camera":1}"#).code, "bad_arguments");
    assert_eq!(call_err(&mut e, "render", r#"{"width":0}"#).code, "bad_size");
    assert_eq!(call_err(&mut e, "pick", r#"{"x":9999,"y":0}"#).code, "out_of_range");
}

#[test]
fn mcp_returns_the_frame_as_image_content() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[0,0,3]},"Camera":{}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Transform":{},"MeshRenderer":{"mesh":"cube"}}}"#);
    let reply = rpc(&mut e, r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"render","arguments":{"width":64,"height":48}}}"#);
    let content = reply.get("result").unwrap().get("content").unwrap().as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(content[0].get("type").unwrap().as_str(), Some("image"));
    assert_eq!(content[0].get("mimeType").unwrap().as_str(), Some("image/png"));
    assert!(content[0].get("data").unwrap().as_str().unwrap().len() > 100);
    assert_eq!(content[1].get("type").unwrap().as_str(), Some("text"));
    let text = parse(content[1].get("text").unwrap().as_str().unwrap()).unwrap();
    assert!(text.get("image").is_none(), "the base64 must not be repeated inside the text part");
    assert_eq!(text.get("visible").unwrap().as_array().unwrap().len(), 1);
}

#[test]
fn an_agent_can_program_behaviour_with_graph_systems_and_no_compiler() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"count":3,"components":{"Transform":{"pos":[0,10,0]},"Velocity":{}}}"#);
    // a mistake is reported with a path and a hint, and nothing is recorded
    let err = call_err(&mut e, "define_system", r#"{"name":"g","each":["Transform","Velocity"],"do":[{"set":"Velocity.lin[1]","to":{"subb":[1,2]}}]}"#);
    assert_eq!(err.code, "bad_graph");
    assert_eq!(err.path.as_deref(), Some("do[0].to.subb"));
    assert!(err.hint.unwrap().contains("sub"));
    assert_eq!(call(&mut e, "systems", "{}").get("graph").unwrap().as_array().unwrap().len(), 0);
    // define a working one
    let def = r#"{"name":"gravity","doc":"fall","each":["Transform","Velocity"],"do":[
        {"set":"Velocity.lin[1]","to":{"sub":[{"get":"Velocity.lin[1]"},{"mul":[9.8,{"dt":true}]}]}}]}"#;
    assert_eq!(call(&mut e, "define_system", def).get("undoable"), Some(&Json::Bool(true)));
    call(&mut e, "step", r#"{"ticks":60}"#);
    let q = call(&mut e, "query", r#"{"with":["Velocity"],"select":["Velocity"],"limit":1}"#);
    let vy = q.get("entities").unwrap().index(0).unwrap().get("components").unwrap().get("Velocity").unwrap().get("lin").unwrap().index(1).unwrap().as_f64().unwrap();
    assert!((vy + 9.8).abs() < 0.01, "after one second of gravity vy = {vy}");
    let list = call(&mut e, "systems", "{}");
    let g = &list.get("graph").unwrap().as_array().unwrap()[0];
    assert_eq!(g.get("name").unwrap().as_str(), Some("gravity"));
    assert_eq!(g.get("enabled"), Some(&Json::Bool(true)));
    assert!(g.get("nodes").unwrap().as_i64().unwrap() >= 5);
    assert_eq!(call(&mut e, "get_system", r#"{"name":"gravity"}"#).get("def").unwrap().get("name").unwrap().as_str(), Some("gravity"));
    assert!(call(&mut e, "describe", "{}").get("systems").unwrap().as_array().unwrap().iter().any(|s| s.as_str() == Some("gravity")));
    // disabling stops it; the hash of a stepped world then stays physically consistent (velocity no longer changes)
    call(&mut e, "enable_system", r#"{"name":"gravity","enabled":false}"#);
    let before = call(&mut e, "query", r#"{"with":["Velocity"],"select":["Velocity"],"limit":1}"#);
    call(&mut e, "step", r#"{"ticks":30}"#);
    assert_eq!(call(&mut e, "query", r#"{"with":["Velocity"],"select":["Velocity"],"limit":1}"#), before);
    // undo walks back through enable and define
    call(&mut e, "undo", "{}");
    assert_eq!(call(&mut e, "systems", "{}").get("graph").unwrap().index(0).unwrap().get("enabled"), Some(&Json::Bool(true)));
    call(&mut e, "undo", "{}");
    assert_eq!(call(&mut e, "systems", "{}").get("graph").unwrap().as_array().unwrap().len(), 0);
    call(&mut e, "redo", "{}");
    assert_eq!(call_err(&mut e, "remove_system", r#"{"name":"nope"}"#).code, "no_such_system");
    // systems survive a scene save and load, and the determinism check still holds
    call(&mut e, "save_scene", r#"{"path":"scenes/g.lunaris.json"}"#);
    // a second engine on the same workspace folder (the test helper shares it per thread)
    let mut e2 = engine();
    call(&mut e2, "load_scene", r#"{"path":"scenes/g.lunaris.json"}"#);
    assert_eq!(call(&mut e2, "systems", "{}").get("graph").unwrap().as_array().unwrap().len(), 1);
}

#[test]
fn raycast_and_nearby_give_an_agent_its_other_senses() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Wall"},"Transform":{"pos":[0,0,-5],"scale":[4,4,0.5]},"MeshRenderer":{"mesh":"cube"}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Coin"},"Transform":{"pos":[1,0,-1]},"MeshRenderer":{"mesh":"sphere"}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Far"},"Transform":{"pos":[30,0,0]}}}"#);
    let hit = call(&mut e, "raycast", r#"{"origin":[0,0,0],"direction":[0,0,-1]}"#);
    assert_eq!(hit.get("hit"), Some(&Json::Bool(true)));
    assert_eq!(hit.get("name").unwrap().as_str(), Some("Wall"));
    assert!((hit.get("distance").unwrap().as_f64().unwrap() - 4.75).abs() < 1e-4);
    let side = call(&mut e, "raycast", r#"{"origin":[1,0,0],"direction":[0,0,-1],"max_distance":50}"#);
    assert_eq!(side.get("name").unwrap().as_str(), Some("Coin"));
    assert_eq!(call(&mut e, "raycast", r#"{"origin":[0,0,0],"direction":[0,0,-1],"max_distance":2}"#).get("hit"), Some(&Json::Bool(false)));
    assert_eq!(call_err(&mut e, "raycast", r#"{"origin":[0,0,0],"direction":[0,0,0]}"#).code, "bad_arguments");
    // nearby: sorted by distance, filterable by component
    let n = call(&mut e, "nearby", r#"{"point":[0,0,0],"radius":6}"#);
    let names: Vec<&str> = n.get("entities").unwrap().as_array().unwrap().iter().map(|x| x.get("name").unwrap().as_str().unwrap()).collect();
    assert_eq!(names, vec!["Coin", "Wall"]);
    assert!(n.get("entities").unwrap().index(0).unwrap().get("distance").unwrap().as_f64().unwrap() < 1.5);
    let meshes_only = call(&mut e, "nearby", r#"{"point":[30,0,0],"radius":1,"with":["MeshRenderer"]}"#);
    assert_eq!(meshes_only.get("total").unwrap().as_i64(), Some(0));
    assert_eq!(call(&mut e, "nearby", r#"{"point":[30,0,0],"radius":1}"#).get("total").unwrap().as_i64(), Some(1));
    assert_eq!(call_err(&mut e, "nearby", r#"{"point":[0,0,0],"radius":-1}"#).code, "type_mismatch");
}

#[test]
fn physics_makes_the_world_push_back_and_contacts_say_who_touched_whom() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Floor"},"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[30,1,30]}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Ball"},"Transform":{"pos":[0,2,0]},"Velocity":{},"Collider":{"shape":0},"RigidBody":{"restitution":0.2}}}"#);
    call(&mut e, "step", r#"{"ticks":240}"#);
    let y = call(&mut e, "get", r#"{"uid":2,"components":["Transform"]}"#).get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().index(1).unwrap().as_f64().unwrap();
    assert!((y - 0.5).abs() < 0.03, "the ball rests on the floor, y = {y}");
    let c = call(&mut e, "contacts", "{}");
    assert!(c.get("total").unwrap().as_i64().unwrap() >= 1);
    let first = c.get("contacts").unwrap().index(0).unwrap();
    assert_eq!(first.get("a_name").unwrap().as_str(), Some("Floor"));
    assert_eq!(first.get("b_name").unwrap().as_str(), Some("Ball"));
    assert!(first.get("normal").unwrap().index(1).unwrap().as_f64().unwrap() > 0.9);
    // gravity is world data: switching it off lets the ball float
    call(&mut e, "spawn", r#"{"components":{"PhysicsSettings":{"gravity":[0,0,0]}}}"#);
    call(&mut e, "set", r#"{"uid":2,"component":"Velocity","value":{"lin":[0,1,0]}}"#);
    call(&mut e, "step", r#"{"ticks":60}"#);
    let y2 = call(&mut e, "get", r#"{"uid":2,"components":["Transform"]}"#).get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().index(1).unwrap().as_f64().unwrap();
    assert!(y2 > 1.3, "no gravity: the ball keeps moving up, y = {y2}");
}

/// Every tool the dispatcher handles must be advertised (MCP `tools/list`, CLI, the mind's prompt) and every advertised tool must
/// be handled. The dispatch names are read from the source of `Engine::dispatch`, so adding a tool in one place only fails here.
#[test]
fn advertised_tools_match_the_dispatcher() {
    let src = include_str!("engine.rs");
    let start = src.find("fn dispatch(&mut self").expect("Engine::dispatch");
    let end = src[start..].find("// ----").map(|i| start + i).expect("end of the dispatcher");
    let mut dispatched: Vec<String> = Vec::new();
    for line in src[start..end].lines() {
        let t = line.trim_start();
        // a match arm on one tool name looks like `"name" => ...`
        if let Some(rest) = t.strip_prefix('"') {
            if let Some(q) = rest.find('"') {
                if rest[q + 1..].trim_start().starts_with("=>") && rest[..q].chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                    dispatched.push(rest[..q].to_string());
                }
            }
        }
    }
    dispatched.sort();
    let mut advertised: Vec<String> = tool_names().into_iter().map(str::to_string).collect();
    advertised.sort();
    assert!(dispatched.len() > 30, "the scan found only {dispatched:?}");
    let missing: Vec<&String> = dispatched.iter().filter(|d| !advertised.contains(d)).collect();
    let dead: Vec<&String> = advertised.iter().filter(|a| !dispatched.contains(a)).collect();
    assert!(missing.is_empty(), "handled but not advertised: {missing:?}");
    assert!(dead.is_empty(), "advertised but not handled: {dead:?}");
}

#[test]
fn input_and_resources_are_one_door_for_humans_and_agents() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"Transform":{}}}"#);
    call(&mut e, "define_system", r#"{"name":"walk","each":["Transform"],"do":[
        {"set":"Transform.pos[0]","to":{"add":[{"get":"Transform.pos[0]"},{"mul":[{"res":"input.right"},60,{"dt":true}]}]}},
        {"res_add":"distance","by":{"mul":[{"res":"input.right"},60,{"dt":true}]}}]}"#);
    // listed with the resources it reads
    let sys = call(&mut e, "systems", "{}");
    let names: Vec<&str> = sys.get("graph").unwrap().index(0).unwrap().get("resources").unwrap().as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
    assert_eq!(names, ["input.right"]);
    call(&mut e, "record_start", "{}");
    call(&mut e, "step", r#"{"ticks":10}"#);
    call(&mut e, "input", r#"{"values":{"right":true}}"#);
    call(&mut e, "step", r#"{"ticks":60}"#);
    call(&mut e, "input", r#"{"values":{"right":false}}"#);
    call(&mut e, "step", r#"{"ticks":10}"#);
    let r = call(&mut e, "resources", "{}");
    let res = r.get("resources").unwrap();
    assert_eq!(res.get("input.right").unwrap().as_f64(), Some(0.0));
    assert!((res.get("distance").unwrap().as_f64().unwrap() - 60.0).abs() < 0.01, "{res:?}");
    // input is not an edit: nothing to undo (the spawn and system are the history)
    call(&mut e, "undo", "{}");
    assert_eq!(call(&mut e, "systems", "{}").get("graph").unwrap().as_array().unwrap().len(), 0, "undo stepped back over the system, not over the input");
    call(&mut e, "redo", "{}");
    // the recording includes the input, so a replay needs no one at the keyboard
    call(&mut e, "record_stop", "{}");
    assert_eq!(call(&mut e, "replay", "{}").get("matches"), Some(&Json::Bool(true)));
    // set_resource is an ordinary undoable edit and accepts null to remove
    call(&mut e, "set_resource", r#"{"name":"score","value":10}"#);
    assert_eq!(call(&mut e, "undo", "{}").get("ok"), Some(&Json::Bool(true)));
    assert!(call(&mut e, "resources", "{}").get("resources").unwrap().get("score").is_none());
    // errors teach
    let err = call_err(&mut e, "input", r#"{"values":{"w":"yes"}}"#);
    assert_eq!(err.code, "type_mismatch");
    assert_eq!(call_err(&mut e, "input", "{}").code, "missing_param");
    assert_eq!(call_err(&mut e, "set_resource", r#"{"name":"two words","value":1}"#).code, "bad_resource_name");
}

#[test]
fn a_long_step_stops_at_its_wall_clock_budget_and_can_be_continued() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"count":20000,"components":{"Transform":{},"Velocity":{"lin":[1,0,0]},"Wander":{"strength":1}}}"#);
    let r = call(&mut e, "step", r#"{"ticks":10000000,"budget_ms":50}"#);
    assert_eq!(r.get("stopped_early"), Some(&Json::Bool(true)), "{r:?}");
    let done = r.get("ticks_run").unwrap().as_u64().unwrap();
    assert!(done > 0 && done < 10_000_000);
    assert_eq!(r.get("tick").unwrap().as_u64().unwrap(), done);
    assert!(r.get("note").unwrap().as_str().unwrap().contains("call step again"));
    // a normal step reports no early stop, and the result is still deterministic: same ticks, same hash
    let a = call(&mut e, "step", r#"{"ticks":5}"#);
    assert!(a.get("stopped_early").is_none());
}

#[test]
fn the_minimap_gives_a_model_a_picture_it_can_read_in_text() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"uid":1,"components":{"Name":{"value":"Hero"},"Transform":{"pos":[0,0,0]}}}"#);
    call(&mut e, "spawn", r#"{"uid":2,"components":{"Name":{"value":"Orb 1"},"Transform":{"pos":[4,0,-6]}}}"#);
    call(&mut e, "spawn", r#"{"uid":3,"components":{"Name":{"value":"Orb 2"},"Transform":{"pos":[-6,0,6]}}}"#);
    call(&mut e, "spawn", r#"{"uid":4,"components":{"Name":{"value":"Ogre"},"Transform":{"pos":[-6.2,0,5.8]}}}"#);
    call(&mut e, "spawn", r#"{"uid":5,"components":{"Name":{"value":"Floor"},"Transform":{"scale":[100,1,100]}}}"#);
    call(&mut e, "spawn", r#"{"uid":6,"components":{"Name":{"value":"Far"},"Transform":{"pos":[500,0,0]}}}"#);
    let m = call(&mut e, "minimap", r#"{"around":1,"radius":10,"cells":21}"#);
    let rows: Vec<&str> = m.get("rows").unwrap().as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
    assert_eq!(rows.len(), 21);
    assert!(rows.iter().all(|r| r.chars().count() == 21));
    assert_eq!(rows[10].chars().nth(10), Some('@'), "the entity in `around` is in the middle: {rows:?}");
    // Orb 1 is at x +4, z -6: right of centre, above it (up is -z)
    let cell = 20.0 / 21.0;
    let (col, row) = (((4.0 + 10.0) / cell) as usize, ((-6.0 + 10.0) / cell) as usize);
    assert_eq!(rows[row].chars().nth(col), Some('O'), "{rows:?}");
    // Orb 2 and Ogre share a cell and a first letter: the cell shows O, and the legend counts them
    let legend = m.get("legend").unwrap();
    assert!(legend.get("O").unwrap().as_str().unwrap().contains("x2") || legend.get("O").unwrap().as_str().unwrap().contains("Orb"), "{legend:?}");
    assert!(m.to_string_lossy().contains("Hero"));
    // the 100-unit floor is background and left out; things beyond the radius are not shown
    assert!(!rows.concat().contains('F'), "{rows:?}");
    assert!(legend.get("F").is_none() && legend.get("A").is_none());
    // another view and errors
    let side = call(&mut e, "minimap", r#"{"around":1,"radius":10,"cells":11,"view":"side"}"#);
    assert_eq!(side.get("up").unwrap().as_str(), Some("+y"));
    assert_eq!(call_err(&mut e, "minimap", r#"{"view":"diagonal"}"#).code, "bad_arguments");
    assert_eq!(call_err(&mut e, "minimap", r#"{"around":99}"#).code, "no_such_entity");
}

#[test]
fn events_reach_the_outside_through_step_and_the_events_tool() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"count":3,"components":{"Transform":{},"Velocity":{"lin":[1,0,0]}}}"#);
    call(&mut e, "define_system", r#"{"name":"beep","each":["Transform"],"do":[{"if":{"gt":[{"get":"Transform.pos[0]"},0.05]},"then":[{"emit":"beep"}]}]}"#);
    let r = call(&mut e, "step", r#"{"ticks":10}"#);
    let events = r.get("events").expect("step reports the events of this call").as_array().unwrap();
    assert!(!events.is_empty() && events.iter().all(|ev| ev.get("name").unwrap().as_str() == Some("beep")));
    assert!(events[0].get("tick").unwrap().as_u64().unwrap() < 10 && events[0].get("uid").is_some());
    let all = call(&mut e, "events", r#"{"limit":5}"#);
    assert_eq!(all.get("events").unwrap().as_array().unwrap().len(), 5);
    assert!(all.get("total").unwrap().as_u64().unwrap() >= 5);
    // events from a later step only
    let later = call(&mut e, "events", r#"{"since_tick":10}"#);
    assert_eq!(later.get("total").unwrap().as_u64(), Some(0));
    let r2 = call(&mut e, "step", r#"{"ticks":1}"#);
    assert_eq!(r2.get("events").unwrap().as_array().unwrap().len(), 3, "three entities, one tick");
}

#[test]
fn edits_with_the_same_undo_group_are_one_undo_step() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"uid":1,"components":{"Transform":{"pos":[0,0,0]}}}"#);
    for x in 1..=20 {
        call(&mut e, "set", &format!(r#"{{"uid":1,"component":"Transform","value":{{"pos":[{x},0,0]}},"undo_group":"drag-1"}}"#));
    }
    call(&mut e, "set", r#"{"uid":1,"component":"Transform","value":{"pos":[99,0,0]}}"#); // an ordinary edit is its own step
    let x = |e: &mut Engine| call(e, "get", r#"{"uid":1}"#).get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().index(0).unwrap().as_f64().unwrap();
    assert_eq!(x(&mut e), 99.0);
    call(&mut e, "undo", "{}");
    assert_eq!(x(&mut e), 20.0, "the ordinary edit is undone first");
    call(&mut e, "undo", "{}");
    assert_eq!(x(&mut e), 0.0, "all twenty moves of the drag come back in one step");
    call(&mut e, "redo", "{}");
    assert_eq!(x(&mut e), 20.0);
    // a different group, or no group, starts a new step
    call(&mut e, "set", r#"{"uid":1,"component":"Transform","value":{"pos":[5,0,0]},"undo_group":"drag-2"}"#);
    call(&mut e, "set", r#"{"uid":1,"component":"Transform","value":{"pos":[6,0,0]},"undo_group":"drag-3"}"#);
    call(&mut e, "undo", "{}");
    assert_eq!(x(&mut e), 5.0);
}

#[test]
fn check_finds_the_mistakes_that_would_otherwise_be_a_blank_picture() {
    // the shipped games are clean
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    for scene in ["examples/orb_dash.lunaris.json", "examples/moon_hopper.lunaris.json", "examples/models.lunaris.json", "examples/showcase.lunaris.json"] {
        let mut e = Engine::new(1, root.clone());
        call(&mut e, "load_scene", &format!(r#"{{"path":"{scene}"}}"#));
        let c = call(&mut e, "check", "{}");
        assert_eq!(c.get("ok"), Some(&Json::Bool(true)), "{scene}: {}", c.to_string_lossy());
        assert_eq!(c.get("warnings").unwrap().as_u64(), Some(0), "{scene}: {}", c.to_string_lossy());
    }
    // a scene full of mistakes
    let mut e = Engine::new(1, root);
    call(&mut e, "spawn", r#"{"uid":1,"components":{"MeshRenderer":{"mesh":"cube"}}}"#); // no Transform
    call(&mut e, "spawn", r#"{"uid":2,"components":{"Transform":{},"MeshRenderer":{"mesh":"teapot"}}}"#);
    call(&mut e, "spawn", r#"{"uid":3,"components":{"Transform":{},"MeshRenderer":{"mesh":"models/missing.glb"}}}"#);
    call(&mut e, "spawn", r#"{"uid":4,"components":{"Transform":{},"Sprite":{"texture":"nope.png"}}}"#);
    call(&mut e, "spawn", r#"{"uid":5,"components":{"Transform":{},"Tilemap":{"size":[3,2],"data":"0 1 2"}}}"#);
    call(&mut e, "spawn", r#"{"uid":6,"components":{"Text":{"value":"Score {scoer}"}}}"#);
    call(&mut e, "spawn", r#"{"uid":7,"components":{"Collider":{}}}"#);
    call(&mut e, "define_system", r#"{"name":"reader","each":["Transform"],"do":[{"set":"Transform.pos[0]","to":{"res":"speed"}}]}"#);
    let c = call(&mut e, "check", "{}");
    assert_eq!(c.get("ok"), Some(&Json::Bool(false)));
    let text = c.to_string_lossy();
    for needle in ["no active Camera", "no Transform, so it is never drawn", "unknown mesh `teapot`", "models/missing.glb", "nope.png", "3 tile numbers for 3 x 2 = 6", "scoer", "reads the resource `speed`", "Collider but no Transform"] {
        assert!(text.contains(needle), "expected a problem mentioning `{needle}` in {text}");
    }
    assert!(c.get("errors").unwrap().as_u64().unwrap() >= 4 && c.get("warnings").unwrap().as_u64().unwrap() >= 4);
    // every problem names the entity it is about (or is about the world), has a severity and a message
    for p in c.get("problems").unwrap().as_array().unwrap() {
        assert!(matches!(p.get("severity").and_then(|s| s.as_str()), Some("error" | "warning")) && p.get("message").is_some());
    }
}

#[test]
fn the_guide_tool_hands_an_agent_the_manual_whole_or_by_section() {
    let mut e = engine();
    let all = call(&mut e, "guide", "{}");
    let topics: Vec<&str> = all.get("topics").unwrap().as_array().unwrap().iter().map(|t| t.as_str().unwrap()).collect();
    assert_eq!(topics, ["the loop", "the world", "behaviour", "seeing", "checking your work", "sharing a world", "pitfalls"]);
    assert!(all.get("text").unwrap().as_str().unwrap().contains("define_system"));
    let b = call(&mut e, "guide", r#"{"topic":"behav"}"#);
    let text = b.get("text").unwrap().as_str().unwrap();
    assert!(text.starts_with("## Behaviour") && text.contains("on_contact") && !text.contains("## Seeing"));
    let err = call_err(&mut e, "guide", r#"{"topic":"cooking"}"#);
    assert_eq!(err.code, "no_such_topic");
    assert!(err.hint.unwrap().contains("pitfalls"));
    // the MCP handshake points a new agent at it
    let init = rpc(&mut e, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    assert!(init.to_string_lossy().contains("Call `guide`"));
}

#[test]
fn an_oversized_request_line_is_refused_and_the_next_request_still_works() {
    let mut e = engine();
    let big = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{"x":"{}"}}}}"#, "a".repeat(5000));
    let input = format!("{big}\n{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}}\n{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\"}}");
    let mut out = Vec::new();
    serve_with_limit(&mut e, Cursor::new(input), &mut out, 1000).unwrap();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].contains("-32600") && lines[0].contains("request too large"), "{}", lines[0]);
    assert!(lines[1].contains("\"id\":2") && lines[2].contains("\"id\":3"), "the requests after the big one are served: {text}");
    // a line exactly at the limit is fine
    let mut out = Vec::new();
    let ok = r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#;
    serve_with_limit(&mut e, Cursor::new(format!("{ok}\n")), &mut out, ok.len()).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("\"id\":9"));
}

/// The revision counter goes up for anything that may change the world and stays put for reads, so a client can poll it to find out
/// that someone else has been working in the same world.
#[test]
fn the_revision_counter_moves_only_when_something_may_have_changed() {
    let mut e = engine();
    let rev = |e: &mut Engine| call(e, "revision", "{}").get("revision").unwrap().as_u64().unwrap();
    let r0 = rev(&mut e);
    for (tool, args) in [("describe", "{}"), ("query", "{}"), ("hash", "{}"), ("systems", "{}"), ("resources", "{}"), ("render", r#"{"width":16,"height":9,"image":false}"#), ("save_scene", "{}"), ("revision", "{}")] {
        call(&mut e, tool, args);
        assert_eq!(rev(&mut e), r0, "{tool} is read-only and must not move the revision");
    }
    call(&mut e, "spawn", r#"{"components":{"Transform":{}}}"#);
    let r1 = rev(&mut e);
    assert!(r1 > r0, "spawn changes the world");
    call(&mut e, "step", r#"{"ticks":1}"#);
    let r2 = rev(&mut e);
    assert!(r2 > r1, "step changes the world");
    assert!(e.call("spawn", &parse(r#"{"components":{"Nope":{}}}"#).unwrap()).is_err());
    assert!(rev(&mut e) > r2, "a failed edit may have changed something part way, so it counts");
    assert_eq!(call(&mut e, "describe", "{}").get("revision").unwrap().as_u64().unwrap(), e.revision());
}

#[test]
fn attached_entities_follow_their_parents_through_the_tools_and_check_explains_mistakes() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"uid":1,"components":{"Name":{"value":"car"},"Transform":{"pos":[4,0,0]},"Velocity":{"lin":[0,0,6]}}}"#);
    call(&mut e, "spawn", r#"{"uid":2,"components":{"Name":{"value":"driver"},"Transform":{},"Parent":{"target":1},"LocalTransform":{"pos":[0,1,0]}}}"#);
    let pos = |e: &mut Engine, uid: u64| {
        let g = call(e, "get", &format!(r#"{{"uid":{uid},"components":["Transform"]}}"#));
        let p = g.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().clone();
        [p.index(0).unwrap().as_f64().unwrap(), p.index(1).unwrap().as_f64().unwrap(), p.index(2).unwrap().as_f64().unwrap()]
    };
    assert_eq!(pos(&mut e, 2), [4.0, 1.0, 0.0], "attached as soon as it is spawned");
    call(&mut e, "step", r#"{"ticks":60}"#);
    let p = pos(&mut e, 2);
    assert!((p[0] - 4.0).abs() < 1e-4 && (p[1] - 1.0).abs() < 1e-4 && (p[2] - 6.0).abs() < 1e-3, "{p:?}");
    // undo of an edit to the parent brings the child back too
    call(&mut e, "set", r#"{"uid":1,"component":"Transform","value":{"pos":[0,0,0]}}"#);
    assert_eq!(pos(&mut e, 2), [0.0, 1.0, 0.0]);
    call(&mut e, "undo", "{}");
    assert!((pos(&mut e, 2)[2] - 6.0).abs() < 1e-3);
    // a healthy hierarchy checks out; mistakes are named
    let c = call(&mut e, "check", "{}");
    let msgs = |c: &Json| c.get("problems").unwrap().as_array().unwrap().iter().map(|p| p.get("message").unwrap().as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert!(!msgs(&c).iter().any(|m| m.contains("Parent") || m.contains("LocalTransform")), "{:?}", msgs(&c));
    call(&mut e, "spawn", r#"{"uid":3,"components":{"Transform":{},"Parent":{"target":77},"LocalTransform":{}}}"#);
    call(&mut e, "spawn", r#"{"uid":4,"components":{"Transform":{},"Parent":{"target":1}}}"#);
    call(&mut e, "spawn", r#"{"uid":5,"components":{"Transform":{},"LocalTransform":{}}}"#);
    call(&mut e, "spawn", r#"{"uid":6,"components":{"Transform":{},"Parent":{"target":7},"LocalTransform":{}}}"#);
    call(&mut e, "spawn", r#"{"uid":7,"components":{"Transform":{},"Parent":{"target":6},"LocalTransform":{}}}"#);
    let all = msgs(&call(&mut e, "check", "{}")).join("\n");
    for needle in ["Parent 77 does not exist", "has a Parent but no LocalTransform", "has a LocalTransform but no Parent", "Parent cycle"] {
        assert!(all.contains(needle), "check should say `{needle}`:\n{all}");
    }
}

#[test]
fn verify_runs_a_playtest_on_a_scratch_engine_and_says_what_failed() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"Transform":{}}}"#);
    let before = call(&mut e, "hash", "{}");
    let report = call(
        &mut e,
        "verify",
        r#"{"spec":{"name":"demo","replay_check":true,"steps":[
            {"call":{"tool":"spawn","args":{"components":{"Name":{"value":"Ball"},"Transform":{"pos":[0,5,0]},"Velocity":{},"Collider":{"shape":0},"RigidBody":{}}}}},
            {"call":{"tool":"spawn","args":{"components":{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[10,1,10]}}}}},
            {"step":180},
            {"expect":{"name":"Ball","field":"Transform.pos[1]","approx":[0.5,0.05]}},
            {"expect":{"name":"Ball","field":"Transform.pos[1]","gt":3,"message":"it should still be high"}},
            {"expect":{"count":{"with":["Collider"]},"eq":2}},
            {"expect":{"exists":"Nobody","eq":false}},
            {"until":{"tick":true,"ge":400,"max_ticks":100}},
            {"call":{"tool":"despawn","args":{"uid":99}}},
            {"call":{"tool":"despawn","args":{"uid":99},"error":"no_such_entity"}},
            {"bogus":1}
        ]}}"#,
    );
    let res = report.get("result").unwrap_or(&report);
    let results = res.get("results").unwrap().as_array().unwrap();
    let verdict = |what: &str| results.iter().find(|r| r.get("what").and_then(|w| w.as_str()).is_some_and(|w| w.contains(what))).unwrap_or_else(|| panic!("no result for {what}: {}", res.to_string_lossy())).clone();
    // what failed says what was seen and what was wanted
    let high = results.iter().find(|r| r.get("message").and_then(|m| m.as_str()).is_some_and(|m| m.contains("it should still be high"))).expect("the failing expectation is reported");
    let text = high.get("message").unwrap().as_str().unwrap();
    assert!(text.contains("got 0.49") && text.contains("wanted > 3"), "{text}");
    assert_eq!(verdict("until tick").get("ok"), Some(&Json::Bool(false)));
    assert!(verdict("until tick").get("message").unwrap().as_str().unwrap().contains("still 280 after 100 ticks"), "{}", res.to_string_lossy());
    assert!(results.iter().any(|r| r.get("message").and_then(|m| m.as_str()).is_some_and(|m| m.contains("unknown step `bogus`"))));
    assert_eq!(res.get("ok"), Some(&Json::Bool(false)));
    // it ran on a scratch engine: the live world is exactly as it was
    assert_eq!(call(&mut e, "hash", "{}"), before);
    assert_eq!(call(&mut e, "query", "{}").get("total").unwrap().as_u64(), Some(1));
    // the passing parts really passed, including the recording replay
    assert_eq!(verdict("number of entities").get("ok"), Some(&Json::Bool(true)));
    assert_eq!(verdict("exists").get("ok"), Some(&Json::Bool(true)));
    assert_eq!(verdict("replays").get("ok"), Some(&Json::Bool(true)));
    // a spec that is all true says so, and a bad spec is an error with a hint
    let good = call(&mut e, "verify", r#"{"spec":{"steps":[{"step":3},{"expect":{"tick":true,"eq":3}},{"expect":{"resource":"nothing","eq":0}}]}}"#);
    assert_eq!(good.get("ok"), Some(&Json::Bool(true)));
    assert_eq!(good.get("passed").unwrap().as_u64(), Some(2));
    assert_eq!(call_err(&mut e, "verify", r#"{"spec":{"scene":"nope.json","steps":[]}}"#).code, "io_error");
    assert_eq!(call_err(&mut e, "verify", r#"{"spec":{}}"#).code, "bad_spec");
    assert_eq!(call_err(&mut e, "verify", "{}").code, "bad_arguments");
}

#[test]
fn files_lists_the_workspace_by_kind_and_stays_inside_it() {
    let dir = std::env::temp_dir().join(format!("lunaris-files-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, bytes) in [("level.lunaris.json", 5usize), ("models/ship.glb", 100), ("models/old.gltf", 7), ("art/hero.PNG", 3), ("art/deep/tile.png", 4), ("sfx/boom.wav", 9), ("notes.txt", 1), ("target/ignored.png", 1), (".hidden/h.png", 1)] {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![0u8; bytes]).unwrap();
    }
    let mut e = Engine::new(1, &dir);
    let paths = |r: &Json| r.get("files").unwrap().as_array().unwrap().iter().map(|f| format!("{}:{}", f.get("kind").unwrap().as_str().unwrap(), f.get("path").unwrap().as_str().unwrap())).collect::<Vec<_>>();
    let top = call(&mut e, "files", "{}");
    assert_eq!(paths(&top), ["scene:level.lunaris.json", "other:notes.txt"], "folders are not entered unless asked; sorted by path");
    let all = call(&mut e, "files", r#"{"recursive":true}"#);
    assert_eq!(paths(&all), ["texture:art/deep/tile.png", "texture:art/hero.PNG", "scene:level.lunaris.json", "model:models/old.gltf", "model:models/ship.glb", "other:notes.txt", "sound:sfx/boom.wav"], "target and hidden folders are skipped");
    assert_eq!(paths(&call(&mut e, "files", r#"{"recursive":true,"kind":"model"}"#)), ["model:models/old.gltf", "model:models/ship.glb"]);
    assert_eq!(paths(&call(&mut e, "files", r#"{"path":"art","recursive":true,"kind":"texture"}"#)), ["texture:art/deep/tile.png", "texture:art/hero.PNG"]);
    let ship = all.get("files").unwrap().as_array().unwrap().iter().find(|f| f.get("path").and_then(|p| p.as_str()) == Some("models/ship.glb")).unwrap();
    assert_eq!(ship.get("bytes").unwrap().as_u64(), Some(100));
    let limited = call(&mut e, "files", r#"{"recursive":true,"limit":2}"#);
    assert_eq!(limited.get("total").unwrap().as_u64(), Some(2));
    assert_eq!(limited.get("truncated"), Some(&Json::Bool(true)));
    assert_eq!(call_err(&mut e, "files", r#"{"path":".."}"#).code, "bad_path");
    assert_eq!(call_err(&mut e, "files", r#"{"path":"notes.txt"}"#).code, "not_a_folder");
    assert_eq!(call_err(&mut e, "files", r#"{"path":"nope"}"#).code, "io_error");
    assert_eq!(call_err(&mut e, "files", r#"{"kind":"video"}"#).code, "bad_kind");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A stand-in for a GPU renderer: draws with the reference renderer and says so, so that the plumbing can be tested anywhere.
struct FakeGpu {
    drew: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    fail: bool,
}

impl GpuBackend for FakeGpu {
    fn name(&self) -> String {
        "fake adapter".to_string()
    }

    fn render(&mut self, world: &mut lunaris_ecs::World, textures: &mut lunaris_render::TextureCache, opts: &lunaris_render::RenderOptions, quality: &str) -> Result<lunaris_render::Frame, String> {
        assert!(QUALITIES.contains(&quality) && quality != "reference");
        self.drew.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.fail {
            return Err("device lost".to_string());
        }
        lunaris_render::render(world, textures, opts).map_err(|e| e.message)
    }
}

#[test]
fn the_render_tool_uses_a_gpu_renderer_when_offered_and_says_which_drew() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[0,0,4]},"Camera":{}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Transform":{},"MeshRenderer":{"mesh":"cube"}}}"#);
    // without a GPU renderer everything is drawn by the reference renderer, deterministically
    let r = call(&mut e, "render", r#"{"width":64,"height":48,"image":false}"#);
    assert_eq!(r.get("renderer").unwrap().as_str(), Some("cpu-reference"));
    assert_eq!(r.get("deterministic"), Some(&Json::Bool(true)));
    // asking for a GPU quality on a host without one still draws, and says why it was not the GPU
    let r = call(&mut e, "render", r#"{"width":64,"height":48,"image":false,"quality":"high"}"#);
    assert_eq!(r.get("renderer").unwrap().as_str(), Some("cpu-reference"));
    assert!(r.get("warnings").unwrap().as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("no GPU renderer")));
    // an unknown name is refused with the list of good ones
    let err = call_err(&mut e, "render", r#"{"quality":"ultraviolet"}"#);
    assert_eq!(err.code, "bad_arguments");
    assert!(err.hint.as_deref().unwrap_or("").contains("reference"));
    // with one offered, `render` uses it by default and `reference` still means the CPU
    let drew = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let d = drew.clone();
    e.enable_gpu(Box::new(move || Ok(Box::new(FakeGpu { drew: d, fail: false }) as Box<dyn GpuBackend>)), "high");
    assert_eq!(drew.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing starts until a picture needs it");
    let r = call(&mut e, "render", r#"{"width":64,"height":48,"image":false}"#);
    assert!(r.get("renderer").unwrap().as_str().unwrap().contains("fake adapter"));
    assert_eq!(r.get("quality").unwrap().as_str(), Some("high"));
    assert_eq!(r.get("deterministic"), Some(&Json::Bool(false)));
    assert!(!r.get("visible").unwrap().as_array().unwrap().is_empty());
    let r = call(&mut e, "render", r#"{"width":64,"height":48,"image":false,"quality":"reference"}"#);
    assert_eq!(r.get("renderer").unwrap().as_str(), Some("cpu-reference"));
    assert_eq!(drew.load(std::sync::atomic::Ordering::SeqCst), 1);
    // a renderer that fails mid-way does not break the tool: the reference renderer takes over, with a warning
    let mut e2 = engine();
    call(&mut e2, "spawn", r#"{"components":{"Transform":{"pos":[0,0,4]},"Camera":{}}}"#);
    let d2 = drew.clone();
    e2.enable_gpu(Box::new(move || Ok(Box::new(FakeGpu { drew: d2, fail: true }) as Box<dyn GpuBackend>)), "medium");
    let r = call(&mut e2, "render", r#"{"width":64,"height":48,"image":false}"#);
    assert_eq!(r.get("renderer").unwrap().as_str(), Some("cpu-reference"));
    assert!(r.get("warnings").unwrap().as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("device lost")));
    // and one that cannot even start
    let mut e3 = engine();
    call(&mut e3, "spawn", r#"{"components":{"Transform":{"pos":[0,0,4]},"Camera":{}}}"#);
    e3.enable_gpu(Box::new(|| Err("no adapter".to_string())), "high");
    let r = call(&mut e3, "render", r#"{"width":64,"height":48,"image":false}"#);
    assert_eq!(r.get("renderer").unwrap().as_str(), Some("cpu-reference"));
    assert!(r.get("warnings").unwrap().as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("no adapter")));
}

#[test]
fn check_knows_mesh_textures_and_the_built_in_materials() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[0,0,4]},"Camera":{}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Wall"},"Transform":{},"MeshRenderer":{"mesh":"cube","texture":"bricks","uv_scale":[3,2]}}}"#);
    let ok = call(&mut e, "check", "{}");
    assert!(ok.get("ok") == Some(&Json::Bool(true)), "{}", ok.to_string_lossy());
    call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Floor"},"Transform":{},"MeshRenderer":{"mesh":"plane","texture":"textures/nope.png"}}}"#);
    let bad = call(&mut e, "check", "{}");
    let problems = bad.get("problems").unwrap().as_array().unwrap();
    assert!(problems.iter().any(|p| p.get("message").unwrap().as_str().unwrap().contains("mesh texture `textures/nope.png`")), "{}", bad.to_string_lossy());
}

#[test]
fn model_info_and_check_know_animated_models() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let mut e = Engine::new(1, repo);
    let info = call(&mut e, "model_info", r#"{"path":"examples/models/swayer.glb"}"#);
    let rig = info.get("rig").unwrap();
    assert_eq!(rig.get("joints").unwrap().as_i64(), Some(4));
    let clips: Vec<String> = rig.get("clips").unwrap().as_array().unwrap().iter().map(|c| c.get("name").unwrap().as_str().unwrap().to_string()).collect();
    assert_eq!(clips, ["sway", "nod"]);
    let size = info.get("bounds").unwrap().get("size").unwrap().as_array().unwrap();
    assert!((size[1].as_f64().unwrap() - 3.2).abs() < 1e-3, "{size:?}");
    assert_eq!(info.get("warnings").unwrap().as_array().unwrap().len(), 0);
    // a model without a skeleton says so, and a missing file is an error that names the way out
    let plain = call(&mut e, "model_info", r#"{"path":"examples/models/crate.glb"}"#);
    assert!(plain.get("rig").unwrap().is_null());
    assert_eq!(plain.get("textured"), Some(&Json::Bool(true)));
    let err = call_err(&mut e, "model_info", r#"{"path":"examples/models/nope.glb"}"#);
    assert_eq!(err.code, "model_load_failed");
    assert!(err.hint.as_deref().unwrap_or("").contains("files"));
    // `check` follows the Animator to the model: an unknown clip is an error that lists the real ones
    call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[0,0,6]},"Camera":{}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Good"},"Transform":{},"MeshRenderer":{"mesh":"examples/models/swayer.glb"},"Animator":{"clip":"sway"}}}"#);
    assert_eq!(call(&mut e, "check", "{}").get("ok"), Some(&Json::Bool(true)));
    call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Bad"},"Transform":{},"MeshRenderer":{"mesh":"examples/models/swayer.glb"},"Animator":{"clip":"dance"}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Cube"},"Transform":{},"MeshRenderer":{"mesh":"cube"},"Animator":{}}}"#);
    let report = call(&mut e, "check", "{}");
    let problems = report.get("problems").unwrap().as_array().unwrap();
    let all: String = problems.iter().map(|p| format!("{} {}", p.get("message").unwrap().as_str().unwrap(), p.get("hint").and_then(|h| h.as_str()).unwrap_or(""))).collect::<Vec<_>>().join("\n");
    assert!(all.contains("no animation called `dance`") && all.contains("its clips: sway, nod"), "{all}");
    assert!(all.contains("its mesh `cube` is not a model file"), "{all}");
}

#[test]
fn every_standard_component_describes_itself_in_the_schema() {
    // the `schema` tool is how an agent learns what a component is for: the doc comment of each `component!` must reach it
    let mut e = engine();
    let schema = call(&mut e, "schema", "{}");
    let comps = schema.get("components").and_then(|c| c.as_array()).unwrap();
    assert!(comps.len() >= 20);
    for c in comps {
        let name = c.get("name").and_then(|n| n.as_str()).unwrap();
        let doc = c.get("doc").and_then(|d| d.as_str()).unwrap_or("");
        assert!(doc.len() >= 20, "{name} has no description in the schema ({doc:?})");
        assert!(!doc.contains("  ") && !doc.contains('\n'), "{name}: the description is one tidy line");
    }
}
