//! The interface beyond flat rectangles, through the tool API: `check` knows about styles, fonts and pins, and `files` lists fonts.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn problems(e: &mut crate::Engine) -> Vec<String> {
    let c = e.call("check", &Json::Null).unwrap();
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| p.get("message").unwrap().as_str().unwrap().to_string()).collect()
}

#[test]
fn check_finds_missing_pictures_and_fonts_and_styles_without_a_ui_and_files_lists_fonts() {
    let mut e = engine();
    let dir = e.root.join("ui_fonts");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../lunaris-render/testdata/pixel.ttf"), dir.join("pixel.ttf")).unwrap();
    std::fs::write(dir.join("broken.ttf"), b"not a font at all").unwrap();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Ui":{},"UiStyle":{"image":"missing_icon.png","radius":0.1}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Text":{"value":"x"},"TextFont":{"file":"ui_fonts/pixel.ttf"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Text":{"value":"x"},"TextFont":{"file":"ui_fonts/broken.ttf"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":4,"components":{"UiStyle":{"radius":0.1},"UiAnchor":{"entity":99}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":5,"components":{"Ui":{},"UiStyle":{"image":"circle"},"TextFont":{"file":"ui_fonts/pixel.ttf"}}}"#)).unwrap();
    let p = problems(&mut e);
    assert!(p.iter().any(|m| m.contains("missing_icon.png") && m.contains("not a built-in texture")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("broken.ttf") && m.contains("cannot be used")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("UiStyle but no Ui")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("UiAnchor but no Ui or Text")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("UiAnchor.entity 99")), "{p:?}");
    // the good ones are not mentioned
    assert!(p.iter().all(|m| !m.contains("pixel.ttf") && !m.contains("`circle`")), "{p:?}");
    // fonts are a kind of file
    let f = e.call("files", &j(r#"{"path":"ui_fonts","kind":"font"}"#)).unwrap();
    let names: Vec<String> = f.get("files").unwrap().as_array().unwrap().iter().map(|x| x.get("path").unwrap().as_str().unwrap().to_string()).collect();
    assert!(names.iter().any(|n| n.ends_with("pixel.ttf")) && names.len() == 2, "{names:?}");
}

#[test]
fn profile_runs_the_ticks_and_says_where_the_time_goes() {
    let mut e = engine();
    for i in 0..300 {
        e.call("spawn", &j(&format!(r#"{{"components":{{"Transform":{{"pos":[{i},5,0]}},"Velocity":{{}},"Collider":{{"shape":0}},"RigidBody":{{"mass":1}}}}}}"#))).unwrap();
    }
    e.call("define_system", &j(r#"{"name":"spin","each":["Transform"],"do":[{"set":"Transform.pos[0]","to":{"add":[{"get":"Transform.pos[0]"},0.01]}}]}"#)).unwrap();
    let tick = e.sim.tick;
    let p = e.call("profile", &j(r#"{"ticks":30}"#)).unwrap();
    assert_eq!(e.sim.tick, tick + 30, "the ticks really ran");
    assert_eq!(p.get("ticks").unwrap().as_u64(), Some(30));
    assert!(p.get("avg_tick_ms").unwrap().as_f64().unwrap() > 0.0 && p.get("max_tick_ms").unwrap().as_f64().unwrap() >= p.get("avg_tick_ms").unwrap().as_f64().unwrap());
    let stages: Vec<String> = p.get("stages").unwrap().as_array().unwrap().iter().map(|s| s.get("stage").unwrap().as_str().unwrap().to_string()).collect();
    for want in ["system physics", "graph system spin", "graph systems (all)", "navigation", "characters", "gameplay blocks"] {
        assert!(stages.iter().any(|s| s == want), "{want} is timed: {stages:?}");
    }
    // sorted by time, and profiling leaves nothing behind: no more timing, and the same state as a run without it
    let times: Vec<f64> = p.get("stages").unwrap().as_array().unwrap().iter().map(|s| s.get("avg_ms").unwrap().as_f64().unwrap()).collect();
    assert!(times.windows(2).all(|w| w[0] >= w[1]), "{times:?}");
    assert!(e.sim.profile.is_none());
    // the shape of the tail: how many steps took longer than each limit, never more for a higher one
    let over: Vec<u64> = ["10", "15", "20", "30", "50", "100"].iter().map(|k| p.get("steps_over_ms").and_then(|o| o.get(k)).and_then(|v| v.as_u64()).unwrap_or(u64::MAX)).collect();
    assert!(over.windows(2).all(|w| w[0] >= w[1]) && over[0] <= 30, "{over:?}");
    let mut plain = engine();
    for i in 0..300 {
        plain.call("spawn", &j(&format!(r#"{{"components":{{"Transform":{{"pos":[{i},5,0]}},"Velocity":{{}},"Collider":{{"shape":0}},"RigidBody":{{"mass":1}}}}}}"#))).unwrap();
    }
    plain.call("define_system", &j(r#"{"name":"spin","each":["Transform"],"do":[{"set":"Transform.pos[0]","to":{"add":[{"get":"Transform.pos[0]"},0.01]}}]}"#)).unwrap();
    plain.call("step", &j(r#"{"ticks":30}"#)).unwrap();
    assert_eq!(e.sim.state_hash(), plain.sim.state_hash(), "timing the ticks does not change them");
    assert!(e.call("profile", &j(r#"{"ticks":0}"#)).is_err() && e.call("profile", &j(r#"{"ticks":99999}"#)).is_err());
    let with_picture = e.call("profile", &j(r#"{"ticks":3,"render":true}"#)).unwrap();
    assert!(with_picture.get("render_ms").is_some() || with_picture.get("render_error").is_some());
}

#[test]
fn model_info_writes_root_motion_text_and_check_reads_animation_text() {
    let mut e = engine();
    std::fs::create_dir_all(e.root.join("models")).unwrap();
    std::fs::copy(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/models/swayer.glb"), e.root.join("models/swayer.glb")).unwrap();
    let info = e.call("model_info", &j(r#"{"path":"models/swayer.glb","root":"top"}"#)).unwrap();
    let rig = info.get("rig").unwrap();
    assert_eq!(rig.get("root_motion_joint").unwrap().as_str(), Some("top"));
    let text = rig.get("root_motion").unwrap().as_str().unwrap().to_string();
    assert!(text.contains("sway length=2.0000:") && text.contains("nod length=1.2000:"), "{text}");
    let clips = lunaris_sim::gameplay::parse_root_motion(&text).unwrap();
    assert_eq!(clips.len(), 2);
    // the top of the column sways to one side: the travel is not zero, and it starts at the origin
    assert_eq!(clips[0].keys[0].1, [0.0, 0.0, 0.0]);
    assert!(clips[0].keys.iter().any(|k| k.1[0].abs() > 0.1), "{:?}", clips[0].keys);
    // the default root is the first joint of the skeleton (it does not move in this model)
    let base = e.call("model_info", &j(r#"{"path":"models/swayer.glb"}"#)).unwrap();
    assert!(base.get("rig").unwrap().get("root_motion_joint").unwrap().as_str().is_some());
    // check reads the texts of AnimEvents and RootMotion
    e.call("spawn", &j(&format!(r#"{{"uid":1,"components":{{"Animator":{{"clip":"sway"}},"RootMotion":{{"curves":{}}},"AnimEvents":{{"events":"sway length=2: 5 late"}}}}}}"#, Json::Str(text).to_string_lossy()))).unwrap();
    let p = problems(&mut e);
    assert!(p.iter().any(|m| m.contains("AnimEvents.events") && m.contains("must be from 0 to the clip's length")), "{p:?}");
    assert!(p.iter().all(|m| !m.contains("RootMotion.curves")), "the text model_info wrote is valid: {p:?}");
}

#[test]
fn check_reads_dialogue_scripts_and_the_entities_that_show_them() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Dialogue":{"script":"== a\n-> nowhere","text_ui":9}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Text":{"value":""}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Dialogue":{"script":"== a\nHi","text_ui":2}}}"#)).unwrap();
    let p = problems(&mut e);
    assert!(p.iter().any(|m| m.contains("Dialogue.script") && m.contains("does not exist")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("Dialogue.text_ui is 9")), "{p:?}");
    assert!(p.iter().all(|m| !m.contains("entity 3") && !m.contains("Dialogue.text_ui is 2")), "the good talk is not mentioned: {p:?}");
}

#[test]
fn a_moving_thing_tagged_nav_obstacle_is_planned_around_and_an_untagged_one_is_ignored() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[20,1,20]}}}"#)).unwrap();
    // a crate in the middle: a dynamic body, which navigation does not see
    e.call("spawn", &j(r#"{"uid":2,"components":{"Transform":{"pos":[0,1.0,0]},"Collider":{"shape":1,"size":[2,2,6]},"RigidBody":{"mass":50},"Velocity":{}}}"#)).unwrap();
    let length = |e: &mut crate::Engine| e.call("path", &j(r#"{"from":[-6,0,0],"to":[6,0,0]}"#)).unwrap().get("length").unwrap().as_f64().unwrap();
    let through = length(&mut e);
    assert!((through - 12.0).abs() < 0.7, "straight through the crate: {through}");
    e.call("add_component", &j(r#"{"uid":2,"component":"NavObstacle","value":{}}"#)).unwrap();
    let around = length(&mut e);
    assert!(around > 13.5, "round the crate (6 long, so a detour of several metres): {around}");
    e.call("set", &j(r#"{"uid":2,"component":"NavObstacle","value":{"on":false}}"#)).unwrap();
    assert!((length(&mut e) - through).abs() < 0.01, "switched off, it is ignored again");
}

#[test]
fn autotile_picks_the_tile_that_fits_the_neighbours() {
    let mut e = engine();
    // a 5 x 5 map: a 3 x 3 block of ground (1) in the middle of empty cells (-1), and a lone ground cell in the corner
    let data = "-1 -1 -1 -1 -1\n-1 1 1 1 -1\n-1 1 1 1 -1\n-1 1 1 1 -1\n-1 -1 -1 -1 1";
    e.call("spawn", &j(&format!(r#"{{"uid":1,"components":{{"Tilemap":{{"size":[5,5],"data":{}}}}}}}"#, Json::str(data).to_string_lossy()))).unwrap();
    let numbers = |e: &mut crate::Engine| -> Vec<Vec<i64>> {
        let text = e.call("get", &j(r#"{"uid":1,"components":["Tilemap"]}"#)).unwrap().get("components").unwrap().get("Tilemap").unwrap().get("data").unwrap().as_str().unwrap().to_string();
        text.lines().map(|l| l.split_whitespace().map(|n| n.parse().unwrap()).collect()).collect()
    };
    // 16 tiles, from base 100, the edge counting as empty so that the block's own sides show: N 1 + E 2 + S 4 + W 8
    let r = e.call("autotile", &j(r#"{"uid":1,"base":100,"edge":"empty"}"#)).unwrap();
    assert_eq!(r.get("tiles_set").unwrap().as_u64(), Some(10));
    let m = numbers(&mut e);
    assert_eq!(m[2][2], 115, "the middle: all four neighbours");
    assert_eq!(m[1][1], 100 + 2 + 4, "top left corner of the block: ground to the east and the south");
    assert_eq!(m[1][2], 100 + 2 + 4 + 8, "the top edge: east, south and west");
    assert_eq!(m[3][3], 100 + 1 + 8, "bottom right corner: north and west");
    assert_eq!((m[0][0], m[4][4]), (-1, 100), "empty cells stay, a lone cell is tile 0 of the set");
    // the blob set: 47 tiles, the middle is the last one, an island the first, and a corner that touches ground only diagonally is not counted
    e.call("set", &j(&format!(r#"{{"uid":1,"component":"Tilemap","value":{{"data":{}}}}}"#, Json::str(data).to_string_lossy()))).unwrap();
    e.call("autotile", &j(r#"{"uid":1,"mode":"blob","base":0,"edge":"empty"}"#)).unwrap();
    let b = numbers(&mut e);
    assert_eq!(b[2][2], 46);
    assert_eq!(b[4][4], 0);
    assert_ne!(b[1][1], b[1][2], "a corner and an edge are different tiles");
    // the border counts as ground by default, so a block against the border has no edge there
    e.call("set", &j(&format!(r#"{{"uid":1,"component":"Tilemap","value":{{"data":{}}}}}"#, Json::str("1 1 -1\n1 1 -1\n-1 -1 -1").to_string_lossy()))).unwrap();
    e.call("set", &j(r#"{"uid":1,"component":"Tilemap","value":{"size":[3,3]}}"#)).unwrap();
    e.call("autotile", &j(r#"{"uid":1,"base":0}"#)).unwrap();
    assert_eq!(numbers(&mut e)[0][0], 1 + 8 + 2 + 4, "all four sides: the border is ground too");
    // errors
    assert_eq!(e.call("autotile", &j(r#"{"uid":99}"#)).unwrap_err().code, "no_such_entity");
    assert_eq!(e.call("autotile", &j(r#"{"uid":1,"mode":"x"}"#)).unwrap_err().code, "bad_mode");
}

#[test]
fn debug_overlays_draw_collider_shapes_velocities_and_ranges_over_the_picture() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{"pos":[0,2,8]},"Camera":{"clear":[0,0,0,1]}}}"#)).unwrap();
    // a dynamic box moving right, a static sphere, a trigger, a point light and a sensor: none of them has a mesh, so without overlays the picture is empty
    e.call("spawn", &j(r#"{"uid":2,"components":{"Transform":{"pos":[0,1,0],"scale":[2,2,2]},"Collider":{"shape":1,"size":[1,1,1]},"RigidBody":{"mass":1},"Velocity":{"lin":[3,0,0]}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Transform":{"pos":[-3,1,0]},"Collider":{"shape":0,"size":[2,2,2]}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":4,"components":{"Transform":{"pos":[3,1,-2]},"Collider":{"shape":1,"size":[1,1,1],"trigger":true}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":5,"components":{"Transform":{"pos":[0,3,0]},"Light":{"kind":1,"range":2.5}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":6,"components":{"Transform":{"pos":[0,0.2,3]},"Sensor":{"radius":2,"fov":90}}}"#)).unwrap();
    let count = |e: &mut crate::Engine, debug: &str, color: [u8; 3]| -> usize {
        let path = format!("dbg/{}.png", debug.replace(['"', '[', ']', ','], ""));
        e.call("render", &j(&format!(r#"{{"width":240,"height":160,"quality":"reference","image":false,"path":"{path}"{}}}"#, if debug.is_empty() { String::new() } else { format!(r#","debug":{debug}"#) }))).unwrap();
        let bytes = std::fs::read(e.root.join(&path)).unwrap();
        let mut dec = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut buf = vec![0u8; dec.output_buffer_size().unwrap()];
        let info = dec.next_frame(&mut buf).unwrap();
        let ch = info.color_type.samples();
        buf.chunks(ch).filter(|p| p[0] == color[0] && p[1] == color[1] && p[2] == color[2]).count()
    };
    assert_eq!(count(&mut e, "", [60, 220, 90]), 0, "nothing without the overlay");
    assert!(count(&mut e, r#"["colliders"]"#, [60, 220, 90]) > 40, "the dynamic box, in green");
    assert!(count(&mut e, r#"["colliders"]"#, [160, 160, 160]) > 40, "the static sphere, in gray");
    assert!(count(&mut e, r#"["colliders"]"#, [240, 210, 40]) > 10, "the trigger, in yellow");
    assert!(count(&mut e, r#"["velocity"]"#, [250, 70, 70]) > 10, "the velocity arrow");
    assert!(count(&mut e, r#"["lights"]"#, [255, 160, 40]) > 20, "the reach of the light");
    assert!(count(&mut e, r#"["sensors"]"#, [80, 220, 220]) > 20, "the sensor");
    assert!(count(&mut e, r#""all""#, [60, 220, 90]) > 40, "all kinds at once");
    assert_eq!(count(&mut e, r#"["velocity"]"#, [60, 220, 90]), 0, "only what was asked for");
    let err = e.call("render", &j(r#"{"debug":["bones"],"image":false}"#)).unwrap_err();
    assert_eq!(err.code, "bad_arguments");
}


#[test]
fn check_reads_flipbook_tables() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Sprite":{"sheet":[4,2]},"Animator":{"clip":"run"},"SpriteAnim":{"clips":"idle: 0-3\nrun: 4-7 fps=12 step@1"}}}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("SpriteAnim") && !m.contains("flip")), "{:?}", problems(&mut e));
    assert!(problems(&mut e).iter().all(|m| !m.contains("no MeshRenderer")), "a 2D flipbook is animated without a mesh: {:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":1,"component":"SpriteAnim","value":{"clips":"idle: 0-3\nrun: 4-9"}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("`run`") && m.contains("past the end of the Sprite's sheet")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":1,"component":"Animator","value":{"clip":"walk"}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("`walk` is not a clip of SpriteAnim.clips")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":1,"component":"SpriteAnim","value":{"clips":"idle 0-3"}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("SpriteAnim.clips: line 1")), "{:?}", problems(&mut e));
    e.call("spawn", &j(r#"{"uid":2,"components":{"SpriteAnim":{"clips":"a: 0"}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("needs an Animator and a Sprite")), "{:?}", problems(&mut e));
}

#[test]
fn check_warns_about_controller_inputs_that_no_pad_sends() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"InputMap":{"bindings":"jump = space, gamepad.a\nmove_x = d, gamepad.lx, gamepad2.ly:-1\nfire = gamepad.rt, gamepad.dpad_left, gamepad3.connected"}}}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("no controller sends")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":1,"component":"InputMap","value":{"bindings":"jump = gamepad.aa, gamepad5.a\nfire = gamepad.dpad_upp, gamepad"}}"#)).unwrap();
    let p = problems(&mut e);
    for bad in ["input.gamepad.aa", "input.gamepad5.a", "input.gamepad.dpad_upp", "`input.gamepad`"] {
        assert!(p.iter().any(|m| m.contains(bad) && m.contains("no controller sends")), "{bad}: {p:?}");
    }
}

#[test]
fn check_reads_scene_switches() {
    let mut e = engine();
    std::fs::create_dir_all(e.root.join("levels")).unwrap();
    std::fs::write(e.root.join("levels/two.lunaris.json"), r#"{"lunaris":1,"entities":[]}"#).unwrap();
    e.call("spawn", &j(r#"{"uid":1,"components":{"SceneSwitch":{"path":"levels/two.lunaris.json"}}}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("SceneSwitch")), "{:?}", problems(&mut e));
    e.call("spawn", &j(r#"{"uid":2,"components":{"SceneSwitch":{"path":"levels/three.lunaris.json"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"SceneSwitch":{}}}"#)).unwrap();
    let p = problems(&mut e);
    assert!(p.iter().any(|m| m.contains("levels/three.lunaris.json") && m.contains("not a file")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("SceneSwitch.path is empty")), "{p:?}");
}

#[test]
fn check_reads_steering() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{},"Velocity":{},"Steer":{"seek":2}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("Steer.seek names entity 2")), "{:?}", problems(&mut e));
    e.call("spawn", &j(r#"{"uid":2,"components":{"Transform":{},"Steer":{"plane":3}}}"#)).unwrap();
    let p = problems(&mut e);
    assert!(p.iter().any(|m| m.contains("Steer needs a Velocity")) && p.iter().any(|m| m.contains("Steer.plane 3")), "{p:?}");
    assert!(!p.iter().any(|m| m.contains("Steer.seek names")), "{p:?}");
}

#[test]
fn check_reads_particle_modules() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Particles":{"count":500},"ParticleModules":{"modules":"color 0:1,1,1,1 1:0,0,0,0\nwind 1,0,0\ncollide plane 0,1,0 d=0"}}}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("ParticleModules")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":1,"component":"ParticleModules","value":{"modules":"wind 1,0"}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("ParticleModules.modules: line 1") && m.contains("three numbers")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":1,"component":"ParticleModules","value":{"modules":"mesh cube"}}"#)).unwrap();
    e.call("set", &j(r#"{"uid":1,"component":"Particles","value":{"count":30000}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("draws at most 20000 solids")), "{:?}", problems(&mut e));
    e.call("spawn", &j(r#"{"uid":2,"components":{"ParticleModules":{"modules":"noise"}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("needs a Particles cloud")), "{:?}", problems(&mut e));
}

#[test]
fn check_reads_particle_systems() {
    let mut e = engine();
    let effect = |text: &str| Json::obj([("uid", Json::from(1i64)), ("components", Json::obj([("ParticleSystem", Json::obj([("effect", Json::str(text))]))]))]);
    // a good effect: no complaint
    e.call("spawn", &effect(r#"{"emitters":[{"spawn":{"rate":10},"init":{"life":1}}]}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("ParticleSystem")), "{:?}", problems(&mut e));
    // a mistake says where it is
    e.call("set", &j(r#"{"uid":1,"component":"ParticleSystem","value":{"effect":"{\"emitters\":[{\"update\":{\"noise\":{\"scale\":0}}}]}"}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("ParticleSystem.effect: emitters[0].update.noise.scale")), "{:?}", problems(&mut e));
    // an odd key is a warning with the nearest one; a picture that is not there is named
    e.call("set", &j(r#"{"uid":1,"component":"ParticleSystem","value":{"effect":"{\"emitters\":[{\"spwan\":{},\"spawn\":{\"rate\":3},\"render\":{\"texture\":\"fx/none.png\"}}]}"}}"#)).unwrap();
    let p = problems(&mut e);
    assert!(p.iter().any(|m| m.contains("did you mean `spawn`")) && p.iter().any(|m| m.contains("`fx/none.png`") && m.contains("not in the workspace")), "{p:?}");
    // a file that is not there, an empty field
    e.call("set", &j(r#"{"uid":1,"component":"ParticleSystem","value":{"effect":"fx/missing.fx.json"}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("the effect file `fx/missing.fx.json` is not in the workspace")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":1,"component":"ParticleSystem","value":{"effect":""}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("ParticleSystem.effect is empty")), "{:?}", problems(&mut e));
    // an effect file of the workspace is read, and an emitter count far past the budget is warned of
    std::fs::create_dir_all(e.root.join("fx")).unwrap();
    std::fs::write(e.root.join("fx/big.fx.json"), r#"{"budget":100,"emitters":[{"max":50000,"spawn":{"rate":20000},"init":{"life":5}}]}"#).unwrap();
    e.call("set", &j(r#"{"uid":1,"component":"ParticleSystem","value":{"effect":"fx/big.fx.json"}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("can hold") && m.contains("the budget is 100")), "{:?}", problems(&mut e));
}

#[test]
fn check_warns_about_2d_bodies_at_different_z() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Camera":{"ortho":true},"Transform":{}}}"#)).unwrap();
    let body = |z: f32| format!(r#"{{"components":{{"Transform":{{"pos":[0,0,{z}]}},"Velocity":{{}},"Collider":{{"shape":0}},"RigidBody":{{"mass":1}}}}}}"#);
    e.call("spawn", &j(&body(0.0))).unwrap();
    e.call("spawn", &j(&body(0.0))).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("different z")), "{:?}", problems(&mut e));
    e.call("spawn", &j(r#"{"components":{"Transform":{"pos":[0,0,0.5]},"Collider":{"shape":1}}}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("different z")), "a static box in another plane is the level's business: {:?}", problems(&mut e));
    e.call("spawn", &j(&body(0.1))).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("different z")), "{:?}", problems(&mut e));
}

#[test]
fn check_warns_when_a_2d_follow_camera_would_tilt_the_picture() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Tilemap":{"size":[2,2],"data":"0 0 0 0"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Transform":{},"Camera":{"ortho":true},"CameraRig":{"target":1}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("tilts the 2D picture")), "the default rig looks at the target a metre above it: {:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":3,"component":"CameraRig","value":{"look_at":false}}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("tilts the 2D picture")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":3,"component":"CameraRig","value":{"look_at":true,"look_offset":[0,0,0]}}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("tilts the 2D picture")), "looking straight at the target is fine: {:?}", problems(&mut e));
}

#[test]
fn set_resource_takes_many_values_in_one_undoable_step_and_a_bad_one_changes_nothing() {
    let mut e = engine();
    let r = e.call("set_resource", &j(r#"{"values":{"cpu.percent":12.5,"net.rx":2500,"flag":true}}"#)).unwrap();
    assert_eq!(r.get("applied").unwrap().as_u64(), Some(3));
    assert_eq!(e.sim.world.resource("cpu.percent"), Some(12.5));
    assert_eq!(e.sim.world.resource("flag"), Some(1.0));
    // one undo takes all three back
    e.call("undo", &Json::Null).unwrap();
    assert_eq!(e.sim.world.resource("cpu.percent"), None);
    assert_eq!(e.sim.world.resource("net.rx"), None);
    // a value that is not a number: an error, and nothing was set
    assert!(e.call("set_resource", &j(r#"{"values":{"a":1,"b":"high"}}"#)).is_err());
    assert_eq!(e.sim.world.resource("a"), None);
    assert!(e.call("set_resource", &j(r#"{"values":{}}"#)).is_err());
    // the old form still works
    e.call("set_resource", &j(r#"{"name":"score","value":3}"#)).unwrap();
    assert_eq!(e.sim.world.resource("score"), Some(3.0));
}

