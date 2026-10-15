//! The editor driven the way a person drives it, without a window: egui is given pointer and key events frame by frame, and the tests look
//! at what the engine's world is afterwards. The scene is drawn by the CPU reference renderer (no graphics card is needed).
use crate::app::App;
use crate::state::{Mode, Options};
use crate::viewport::Axis;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use lunaris_json::Json;

struct Harness {
    ctx: egui::Context,
    app: App,
    time: f64,
}

fn j(s: &str) -> Json {
    lunaris_json::parse(s).unwrap()
}

impl Harness {
    /// An editor on an empty scratch workspace with a small scene: a floor, a red box at the middle and a ball beside it.
    fn new() -> Harness {
        let dir = std::env::temp_dir().join(format!("lunaris-studio-test-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')', ' '], ""));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = egui::Context::default();
        let opts = Options { root: dir, quality: "reference".to_string(), mute: true, ..Options::default() };
        let app = App::new(&ctx, opts);
        let mut h = Harness { ctx, app, time: 0.0 };
        for c in [
            r#"{"components":{"Name":{"value":"Floor"},"Transform":{"pos":[0,-0.5,0],"scale":[20,1,20]},"MeshRenderer":{"mesh":"cube","color":[0.3,0.6,0.3,1]}}}"#,
            r#"{"components":{"Name":{"value":"Box"},"Transform":{"pos":[0,1,0]},"MeshRenderer":{"mesh":"cube","color":[0.9,0.2,0.2,1]}}}"#,
            r#"{"components":{"Name":{"value":"Ball"},"Transform":{"pos":[3,1,0]},"MeshRenderer":{"mesh":"sphere","color":[0.2,0.4,0.9,1]}}}"#,
            r#"{"components":{"Name":{"value":"Sun"},"Transform":{"rot":[-0.5,0.3,0.1,0.8]},"Light":{"kind":0}}}"#,
        ] {
            h.app.studio.call("spawn", j(c)).unwrap();
        }
        h.app.studio.dirty = false;
        h.app.studio.rows_dirty = true;
        h.frames(3);
        h
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.time += 1.0 / 60.0;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 900.0))), time: Some(self.time), events, focused: true, ..RawInput::default() };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| app.draw(ui));
        // there is no graphics card behind this window: the picture it would have drawn is dropped
        out.textures_delta.clear();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    fn uid(&self, name: &str) -> u64 {
        self.app.studio.all.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no entity named {name}")).uid
    }

    fn pos_of(&mut self, name: &str) -> [f64; 3] {
        let uid = self.uid(name);
        let g = self.app.studio.call("get", j(&format!(r#"{{"uid":{uid},"components":["Transform"]}}"#))).unwrap();
        let p = g.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect::<Vec<_>>();
        [p[0], p[1], p[2]]
    }

    fn move_to(&mut self, p: Pos2) {
        self.frame(vec![Event::PointerMoved(p)]);
    }

    fn press(&mut self, p: Pos2) {
        self.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
    }

    fn release(&mut self, p: Pos2) {
        self.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    }

    fn click(&mut self, p: Pos2) {
        self.move_to(p);
        self.press(p);
        self.release(p);
        self.frames(2);
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.move_to(from);
        self.press(from);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            self.move_to(Pos2::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t));
        }
        self.release(to);
        self.frames(2);
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.frame(vec![Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers }]);
        self.frame(vec![Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers }]);
        self.frames(2);
    }

    fn screen_of(&self, name: &str, p: [f32; 3]) -> Pos2 {
        let _ = name;
        self.app.viewport.project(p).unwrap_or_else(|| panic!("{name} is not on the screen"))
    }
}

#[test]
fn the_hierarchy_lists_the_world_and_a_click_in_the_scene_selects_what_is_under_the_pointer() {
    let mut h = Harness::new();
    assert_eq!(h.app.studio.rows.len(), 4, "{:?}", h.app.studio.rows.iter().map(|r| &r.name).collect::<Vec<_>>());
    assert!(h.app.studio.selection.is_none());
    let at = h.screen_of("Box", [0.0, 1.0, 0.0]);
    h.click(at);
    assert_eq!(h.app.studio.selection, Some(h.uid("Box")), "the box is under the pointer");
    assert!(h.app.studio.entity.as_ref().unwrap().comp("Transform").is_some(), "the inspector has its components");
    // another object, then empty sky
    let ball = h.screen_of("Ball", [3.0, 1.0, 0.0]);
    h.click(ball);
    assert_eq!(h.app.studio.selection, Some(h.uid("Ball")));
    let r = h.app.viewport.last_rect;
    h.click(Pos2::new(r.center().x, r.min.y + 6.0));
    assert_eq!(h.app.studio.selection, None, "nothing is up there: the selection goes");
}

#[test]
fn dragging_a_gizmo_handle_moves_the_selection_along_that_axis_only_and_is_one_undo_step() {
    let mut h = Harness::new();
    h.click(h.screen_of("Box", [0.0, 1.0, 0.0]));
    assert_eq!(h.app.studio.selection, Some(h.uid("Box")));
    let before = h.pos_of("Box");
    let from = h.app.viewport.handle_point(&h.app.studio, Axis::X, 0.6).expect("the handle is on the screen");
    let tip = h.app.viewport.handle_point(&h.app.studio, Axis::X, 1.0).unwrap();
    let to = Pos2::new(from.x + (tip.x - from.x) * 2.0 + 20.0, from.y + (tip.y - from.y) * 2.0);
    h.drag(from, to);
    let after = h.pos_of("Box");
    assert!(after[0] > before[0] + 0.8, "moved along x: {before:?} -> {after:?}");
    assert!((after[1] - before[1]).abs() < 1e-4 && (after[2] - before[2]).abs() < 1e-4, "only along x: {after:?}");
    assert!(h.app.studio.dirty, "the scene is modified");
    // the whole drag was made of many `set` calls but is one step to undo
    h.key(Key::Z, Modifiers::COMMAND);
    let undone = h.pos_of("Box");
    assert!((undone[0] - before[0]).abs() < 1e-4, "one undo puts it back: {undone:?}");
}

#[test]
fn delete_and_undo_and_duplicate_work_from_the_keyboard() {
    let mut h = Harness::new();
    let ball = h.uid("Ball");
    h.app.studio.select(Some(ball));
    h.key(Key::Delete, Modifiers::NONE);
    assert!(h.app.studio.all.iter().all(|r| r.name != "Ball"), "the ball is gone");
    assert_eq!(h.app.studio.selection, None);
    h.key(Key::Z, Modifiers::COMMAND);
    assert!(h.app.studio.all.iter().any(|r| r.name == "Ball"), "undo brings it back");
    h.app.studio.select(Some(h.uid("Ball")));
    h.key(Key::D, Modifiers::COMMAND);
    assert!(h.app.studio.all.iter().any(|r| r.name == "Ball 2"), "a copy with its own name: {:?}", h.app.studio.all.iter().map(|r| &r.name).collect::<Vec<_>>());
    assert_eq!(h.app.studio.total_entities, 5);
}

#[test]
fn playing_runs_the_simulation_and_stopping_puts_the_scene_back_exactly() {
    let mut h = Harness::new();
    let box_uid = h.uid("Box");
    h.app.studio.call("add_component", j(&format!(r#"{{"uid":{box_uid},"component":"Velocity","value":{{"lin":[2,0,0]}}}}"#))).unwrap();
    h.app.studio.dirty = false;
    let before = h.app.studio.engine.sim.state_hash();
    h.app.play();
    assert_eq!(h.app.studio.mode, Mode::Play);
    h.frames(60);
    assert!(h.app.studio.engine.sim.tick >= 30, "the simulation ran: tick {}", h.app.studio.engine.sim.tick);
    let moved = h.pos_of("Box");
    assert!(moved[0] > 0.3, "the box moved while playing: {moved:?}");
    h.app.stop();
    assert_eq!(h.app.studio.mode, Mode::Edit);
    assert_eq!(h.app.studio.engine.sim.state_hash(), before, "the scene is exactly as it was");
    assert!(!h.app.studio.dirty, "playing does not make the scene modified");
    assert_eq!(h.app.studio.engine.sim.tick, 0);
    // an edit made while playing is thrown away with it
    h.app.play();
    h.app.studio.set(box_uid, "Transform", j(r#"{"pos":[7,7,7]}"#), None);
    h.app.stop();
    assert!(h.pos_of("Box")[0].abs() < 1e-4);
}

#[test]
fn the_inspector_and_the_tools_agree_and_attached_things_move_by_their_local_position() {
    let mut h = Harness::new();
    let (b, ball) = (h.uid("Box"), h.uid("Ball"));
    // an edit through the studio is the `set` tool
    h.app.studio.select(Some(b));
    h.app.studio.set(b, "Transform", j(r#"{"pos":[1,2,3]}"#), None);
    h.app.studio.refresh_entity();
    assert_eq!(h.app.studio.entity.as_ref().unwrap().vec3("Transform", "pos"), Some([1.0, 2.0, 3.0]));
    // the ball is attached to the box: it keeps its place, and moving it in the world changes its LocalTransform
    crate::hierarchy::attach_to(&mut h.app.studio, ball, Some(b));
    h.app.studio.refresh_rows();
    h.app.studio.engine.sim.step();
    h.app.studio.select(Some(ball));
    let world_before = h.pos_of("Ball");
    assert!((world_before[0] - 3.0).abs() < 1e-3, "attaching did not move it: {world_before:?}");
    let group = h.app.studio.next_group();
    h.app.studio.move_to(ball, [4.0, 2.0, 3.0], &group);
    h.app.studio.engine.sim.step();
    let after = h.pos_of("Ball");
    assert!((after[0] - 4.0).abs() < 1e-3 && (after[1] - 2.0).abs() < 1e-3, "{after:?}");
    let local = h.app.studio.call("get", j(&format!(r#"{{"uid":{ball},"components":["LocalTransform"]}}"#))).unwrap();
    let lp = local.get("components").unwrap().get("LocalTransform").unwrap().get("pos").unwrap().as_array().unwrap()[0].as_f64().unwrap();
    assert!((lp - 3.0).abs() < 1e-3, "x is 3 away from the box at x = 1: {lp}");
    // detaching keeps the place too
    crate::hierarchy::attach_to(&mut h.app.studio, ball, None);
    h.app.studio.engine.sim.step();
    assert!((h.pos_of("Ball")[0] - 4.0).abs() < 1e-3);
}

#[test]
fn a_new_scene_has_a_camera_and_a_light_and_saving_then_opening_gives_the_same_world() {
    let mut h = Harness::new();
    h.app.studio.new_scene();
    assert_eq!(h.app.studio.total_entities, 2, "a camera and a sun");
    assert!(!h.app.studio.dirty);
    let uid = h.app.studio.spawn_template(crate::state::Template::Cube, [1.0, 2.0, 3.0]).unwrap();
    assert!(h.app.studio.dirty);
    assert!(h.app.studio.all.iter().any(|r| r.uid == uid && r.name == "Cube"));
    let hash = h.app.studio.engine.sim.state_hash();
    assert!(h.app.studio.save_as("scenes/new.lunaris.json"));
    assert!(!h.app.studio.dirty);
    assert_eq!(h.app.studio.scene.as_deref(), Some("scenes/new.lunaris.json"));
    h.app.studio.new_scene();
    h.app.studio.open_scene("scenes/new.lunaris.json");
    assert_eq!(h.app.studio.engine.sim.state_hash(), hash, "the scene came back identical");
    assert!(h.app.studio.title().starts_with("scenes/new.lunaris.json - Lunaris Editor"));
}

#[test]
fn the_whole_window_draws_in_every_mode_without_trouble() {
    let mut h = Harness::new();
    h.app.studio.select(Some(h.uid("Sun")));
    for tab in [crate::state::BottomTab::Project, crate::state::BottomTab::Console, crate::state::BottomTab::Systems] {
        h.app.studio.tab = tab;
        h.frames(3);
    }
    for kind in [crate::viewport::ViewKind::Scene2D, crate::viewport::ViewKind::Game, crate::viewport::ViewKind::Scene] {
        h.app.viewport.kind = kind;
        h.frames(3);
    }
    h.app.play();
    h.app.studio.pause();
    h.app.studio.pause();
    h.frames(5);
    h.app.stop();
    // every component the engine has can be shown in the inspector
    let names = h.app.studio.components_available();
    let b = h.uid("Box");
    h.app.studio.select(Some(b));
    for n in names {
        let _ = h.app.studio.call("add_component", j(&format!(r#"{{"uid":{b},"component":"{n}"}}"#)));
    }
    h.frames(4);
    let console_errors = h.app.studio.console.iter().filter(|l| l.kind == crate::state::LogKind::Error).count();
    assert!(console_errors < 10, "{console_errors} errors");
    assert!(h.app.viewport.error.is_none(), "{:?}", h.app.viewport.error);
}

#[test]
fn the_scale_handle_stretches_one_axis_and_the_rotate_ring_turns_by_the_swept_angle() {
    use crate::viewport::Tool;
    let mut h = Harness::new();
    h.click(h.screen_of("Box", [0.0, 1.0, 0.0]));
    // scale: drag the x handle further out
    h.app.viewport.tool = Tool::Scale;
    h.frames(2);
    let from = h.app.viewport.handle_point(&h.app.studio, Axis::X, 0.8).unwrap();
    let tip = h.app.viewport.handle_point(&h.app.studio, Axis::X, 1.0).unwrap();
    h.drag(from, Pos2::new(from.x + (tip.x - from.x) * 4.0, from.y + (tip.y - from.y) * 4.0));
    let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{},"components":["Transform"]}}"#, h.uid("Box")))).unwrap();
    let sc: Vec<f64> = g.get("components").unwrap().get("Transform").unwrap().get("scale").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!(sc[0] > 1.5 && (sc[1] - 1.0).abs() < 1e-4 && (sc[2] - 1.0).abs() < 1e-4, "only x stretched: {sc:?}");
    h.key(Key::Z, Modifiers::COMMAND);
    let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{},"components":["Transform"]}}"#, h.uid("Box")))).unwrap();
    let sc: Vec<f64> = g.get("components").unwrap().get("Transform").unwrap().get("scale").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!((sc[0] - 1.0).abs() < 1e-4, "one undo: {sc:?}");
    // rotate: sweep the y ring by 40 degrees
    h.app.viewport.tool = Tool::Rotate;
    h.frames(2);
    let (a, b) = (h.app.viewport.ring_point(&h.app.studio, Axis::Y, 10.0).unwrap(), h.app.viewport.ring_point(&h.app.studio, Axis::Y, 50.0).unwrap());
    h.drag(a, b);
    let uid = h.uid("Box");
    h.app.studio.select(Some(uid));
    h.app.studio.refresh_entity();
    let q = h.app.studio.entity.as_ref().unwrap().quat("Transform", "rot").unwrap();
    let e = crate::mathx::q_to_euler(q);
    assert!((e[1] - 40.0).abs() < 4.0 && e[0].abs() < 1.0 && e[2].abs() < 1.0, "a quarter-ish turn about y: {e:?}");
}

// ------------------------------------------------------------------------------------------------------------- the Script tab

const DRIFT: &str = r#"{"name":"drift","each":["Transform"],"do":[{"set":"Transform.pos[1]","to":{"add":[{"get":"Transform.pos[1]"},0.5]}}]}"#;

fn script_tab(h: &mut Harness) {
    h.app.studio.center = crate::state::Center::Script;
    h.frames(4);
    // a click in the text puts the cursor there
    h.click(Pos2::new(900.0, 300.0));
}

/// Replace the whole text the way pasting over a selection does.
fn paste_over(h: &mut Harness, text: &str) {
    h.frame(vec![Event::Key { key: Key::A, physical_key: Some(Key::A), pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
    h.frame(vec![Event::Paste(text.to_string())]);
    h.frames(2);
}

#[test]
fn the_script_tab_shows_a_system_as_text_and_ctrl_s_applies_what_was_edited() {
    let mut h = Harness::new();
    h.app.studio.call("define_system", j(DRIFT)).unwrap();
    script_tab(&mut h);
    let text = h.app.script.test_text();
    assert!(text.contains("system drift each Transform") && text.contains("Transform.pos[1] = Transform.pos[1] + 0.5;"), "{text}");
    assert!(h.app.script.test_problems().is_empty());
    // change the number, apply
    paste_over(&mut h, &text.replace("0.5", "2.5"));
    assert!(h.app.script.test_text().contains("2.5"));
    h.key(Key::S, Modifiers::COMMAND);
    let def = h.app.studio.call("get_system", j(r#"{"name":"drift"}"#)).unwrap();
    assert!(def.to_string_compact().unwrap().contains("2.5"), "{def:?}");
    // the script is kept in a file of the project, and nothing is wrong with it
    assert!(h.app.studio.opts.root.join("scripts").join("drift.lun").exists());
    assert!(h.app.script.test_problems().is_empty(), "{:?}", h.app.script.test_problems());
    // the world really runs the new system: the box rises 2.5 per step
    let before = h.pos_of("Box");
    h.app.studio.call("step", j(r#"{"ticks":2}"#)).unwrap();
    assert!((h.pos_of("Box")[1] - before[1] - 5.0).abs() < 1e-3, "{:?} -> {:?}", before, h.pos_of("Box"));
}

#[test]
fn a_mistake_is_a_problem_with_a_place_and_is_not_sent_to_the_engine() {
    let mut h = Harness::new();
    script_tab(&mut h);
    paste_over(&mut h, "system broken each Transform {\n    Transform.pos[1] = ;\n}\n");
    let problems = h.app.script.test_problems();
    assert!(problems.len() == 1 && problems[0].starts_with("2:"), "{problems:?}");
    h.key(Key::S, Modifiers::COMMAND);
    assert!(h.app.studio.call("get_system", j(r#"{"name":"broken"}"#)).is_err());
    assert!(h.app.script.test_output().iter().any(|l| l.contains("not applied")), "{:?}", h.app.script.test_output());
}

#[test]
fn what_the_engine_refuses_is_shown_at_the_line_it_belongs_to() {
    let mut h = Harness::new();
    script_tab(&mut h);
    paste_over(&mut h, "system t each Transform {\n    Transform.pos[0] = 1;\n    Transform.nope[0] = 2;\n}\n");
    assert!(h.app.script.test_problems().is_empty(), "this compiles: it is the world that does not know `nope`");
    h.key(Key::S, Modifiers::COMMAND);
    let problems = h.app.script.test_problems();
    assert!(problems.len() == 1 && problems[0].starts_with("3:") && problems[0].contains("nope"), "{problems:?}");
    assert!(h.app.studio.call("get_system", j(r#"{"name":"t"}"#)).is_err(), "a refused system is not defined");
    // changing the text clears the engine's complaint: it was about the old text
    paste_over(&mut h, "system t each Transform {\n    Transform.pos[0] = 1;\n}\n");
    assert!(h.app.script.test_problems().is_empty());
}

#[test]
fn typing_pairs_brackets_indents_and_offers_the_fields_of_a_component() {
    let mut h = Harness::new();
    script_tab(&mut h);
    paste_over(&mut h, "system a each Transform ");
    h.frame(vec![Event::Text("{".to_string())]);
    h.frames(2);
    assert_eq!(h.app.script.test_text(), "system a each Transform {}");
    h.key(Key::Enter, Modifiers::NONE);
    assert_eq!(h.app.script.test_text(), "system a each Transform {\n    \n}");
    h.frame(vec![Event::Text("T".to_string())]);
    h.frame(vec![Event::Text("r".to_string())]);
    h.frames(2);
    assert!(h.app.script.test_completions().iter().any(|c| c == "Transform"), "{:?}", h.app.script.test_completions());
    for ch in "ansform.po".chars() {
        h.frame(vec![Event::Text(ch.to_string())]);
    }
    h.frames(2);
    assert_eq!(h.app.script.test_completions(), vec!["pos".to_string()]);
    // Enter takes the suggestion instead of starting a line
    h.key(Key::Enter, Modifiers::NONE);
    assert!(h.app.script.test_text().contains("    Transform.pos\n"), "{:?}", h.app.script.test_text());
    // Ctrl+/ comments the line
    h.key(Key::Slash, Modifiers::COMMAND);
    assert!(h.app.script.test_text().contains("    // Transform.pos"), "{:?}", h.app.script.test_text());
}

#[test]
fn files_open_in_tabs_and_the_palette_finds_them() {
    let mut h = Harness::new();
    let dir = h.app.studio.opts.root.join("scripts");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("one.lun"), "system one each Transform {\n}\n").unwrap();
    std::fs::write(dir.join("two.lun"), "system two each Transform {\n}\n").unwrap();
    h.app.studio.center = crate::state::Center::Script;
    h.frames(4);
    let me = &mut h.app.script;
    me.open_path(&mut h.app.studio, &dir.join("one.lun"));
    me.open_path(&mut h.app.studio, &dir.join("two.lun"));
    h.frames(3);
    let docs = h.app.script.test_docs();
    assert!(docs.contains(&"one.lun".to_string()) && docs.contains(&"two.lun".to_string()), "{docs:?}");
    assert!(h.app.script.test_text().contains("system two"));
    // the whole tab draws with the palette open
    h.key(Key::P, Modifiers::COMMAND);
    h.frames(3);
    h.key(Key::Escape, Modifiers::NONE);
    h.frames(2);
}

// ------------------------------------------------------------------------------------------------------------- the Blueprint tab

fn blueprint_tab(h: &mut Harness) {
    h.app.studio.center = crate::state::Center::Blueprint;
    h.frames(5);
}

fn right_click(h: &mut Harness, p: Pos2) {
    h.move_to(p);
    h.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE }]);
    h.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE }]);
    h.frames(2);
}

fn type_text(h: &mut Harness, text: &str) {
    h.frame(vec![Event::Text(text.to_string())]);
    h.frames(1);
}

#[test]
fn the_blueprint_tab_shows_a_system_as_nodes_and_an_edited_value_compiles_into_the_world() {
    use crate::blueprint::{Pin, PinKind};
    let mut h = Harness::new();
    h.app.studio.call("define_system", j(DRIFT)).unwrap();
    blueprint_tab(&mut h);
    let bp = h.app.blueprint.test_bp().expect("the first system is open").clone();
    let ops: Vec<&str> = bp.nodes.iter().map(|n| n.op.as_str()).collect();
    assert!(ops.contains(&"set") && ops.contains(&"add") && ops.contains(&"get"), "{ops:?}");
    assert!(!h.app.blueprint.test_dirty(), "just opened: nothing to compile");
    // click the 0.5 on the Add node, type a new number
    let add = bp.nodes.iter().find(|n| n.op == "add").unwrap().id;
    let at = h.app.blueprint.test_default_screen(add, 1).expect("the box is on the screen");
    h.click(at);
    type_text(&mut h, "2.5");
    h.key(Key::Enter, Modifiers::NONE);
    assert!(h.app.blueprint.test_dirty(), "the graph changed");
    h.key(Key::S, Modifiers::COMMAND);
    let def = h.app.studio.call("get_system", j(r#"{"name":"drift"}"#)).unwrap();
    assert!(def.to_string_compact().unwrap().contains("2.5"), "{def:?}");
    assert!(!h.app.blueprint.test_dirty());
    // the layout is kept next to the project
    assert!(h.app.studio.opts.root.join("blueprints").join("drift.bp.json").exists());
    let _ = Pin { node: 0, kind: PinKind::ExecIn };
    let before = h.pos_of("Box");
    h.app.studio.call("step", j(r#"{"ticks":2}"#)).unwrap();
    assert!((h.pos_of("Box")[1] - before[1] - 5.0).abs() < 1e-3);
}

#[test]
fn a_node_from_the_menu_wired_up_by_dragging_becomes_a_statement() {
    use crate::blueprint::{Pin, PinKind};
    let mut h = Harness::new();
    h.app.studio.call("define_system", j(DRIFT)).unwrap();
    blueprint_tab(&mut h);
    // right click on an empty place: the menu of actions; search, Enter
    right_click(&mut h, Pos2::new(700.0, 560.0));
    assert!(h.app.blueprint.test_menu_open());
    type_text(&mut h, "Emit");
    h.key(Key::Enter, Modifiers::NONE);
    assert!(!h.app.blueprint.test_menu_open());
    let bp = h.app.blueprint.test_bp().unwrap().clone();
    let emit = bp.nodes.iter().find(|n| n.op == "emit").expect("the node was made").id;
    let set = bp.nodes.iter().find(|n| n.op == "set").unwrap().id;
    // drag from the end of Set Field's execution pin to the emit node's input
    let from = h.app.blueprint.test_pin_screen(Pin { node: set, kind: PinKind::ExecOut(0) }).unwrap();
    let to = h.app.blueprint.test_pin_screen(Pin { node: emit, kind: PinKind::ExecIn }).unwrap();
    h.drag(from, to);
    let bp = h.app.blueprint.test_bp().unwrap();
    assert!(bp.wires_from(Pin { node: set, kind: PinKind::ExecOut(0) }).any(|w| w.to.node == emit), "wired");
    let json = bp.to_json().unwrap();
    assert_eq!(json.get("do").unwrap().as_array().unwrap().len(), 2);
    // compile: the world has the second statement
    h.key(Key::S, Modifiers::COMMAND);
    let def = h.app.studio.call("get_system", j(r#"{"name":"drift"}"#)).unwrap();
    assert!(def.to_string_compact().unwrap().contains("\"emit\""), "{def:?}");
}

#[test]
fn nodes_are_selected_deleted_and_brought_back_by_undo() {
    let mut h = Harness::new();
    h.app.studio.call("define_system", j(DRIFT)).unwrap();
    blueprint_tab(&mut h);
    let bp = h.app.blueprint.test_bp().unwrap().clone();
    let get = bp.nodes.iter().find(|n| n.op == "get").unwrap().id;
    let n0 = bp.nodes.len();
    h.click(h.app.blueprint.test_header_screen(get).unwrap());
    assert_eq!(h.app.blueprint.test_selected(), vec![get]);
    h.key(Key::Delete, Modifiers::NONE);
    assert_eq!(h.app.blueprint.test_bp().unwrap().nodes.len(), n0 - 1);
    h.key(Key::Z, Modifiers::COMMAND);
    assert_eq!(h.app.blueprint.test_bp().unwrap().nodes.len(), n0, "undo brings it back");
    // a node dragged by its header moves
    let before = h.app.blueprint.test_bp().unwrap().node(get).unwrap().pos;
    let from = h.app.blueprint.test_header_screen(get).unwrap();
    h.drag(from, from + Vec2::new(40.0, 30.0));
    let after = h.app.blueprint.test_bp().unwrap().node(get).unwrap().pos;
    assert!((after[0] - before[0] - 40.0).abs() < 2.0 && (after[1] - before[1] - 30.0).abs() < 2.0, "{before:?} -> {after:?}");
}

#[test]
fn what_the_world_refuses_is_marked_on_the_node_it_belongs_to() {
    let mut h = Harness::new();
    h.app.studio.call("define_system", j(DRIFT)).unwrap();
    blueprint_tab(&mut h);
    // make the Set Field write a field that does not exist, through the node's own property box
    let bp = h.app.blueprint.test_bp().unwrap().clone();
    let set = bp.nodes.iter().find(|n| n.op == "set").unwrap().id;
    let r = h.app.blueprint.test_header_screen(set).unwrap();
    // the property row is below the pins: click it and type
    h.click(r + Vec2::new(0.0, 74.0));
    h.frames(2);
    h.frame(vec![Event::Key { key: Key::A, physical_key: Some(Key::A), pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
    type_text(&mut h, "Transform.nope[0]");
    h.key(Key::Enter, Modifiers::NONE);
    h.key(Key::S, Modifiers::COMMAND);
    // not applied: the old system is still the one in the world
    let def = h.app.studio.call("get_system", j(r#"{"name":"drift"}"#)).unwrap();
    assert!(!def.to_string_compact().unwrap().contains("nope"), "{def:?}");
    assert!(h.app.blueprint.test_dirty());
}

// ------------------------------------------------------------------------------------------------------------- the UI tab

fn ui_tab(h: &mut Harness) {
    h.app.studio.center = crate::state::Center::Ui;
    h.frames(5);
}

fn spawn_widget(h: &mut Harness, name: &str, ui: &str) -> u64 {
    let r = h.app.studio.call("spawn", j(&format!(r#"{{"components":{{"Name":{{"value":"{name}"}},"Ui":{ui}}}}}"#))).unwrap();
    r.get("uid").unwrap().as_u64().unwrap()
}

#[test]
fn the_ui_tab_lists_the_widgets_and_a_drag_moves_one_as_a_single_undo_step() {
    let mut h = Harness::new();
    let bar = spawn_widget(&mut h, "Health", r#"{"pos":[0.1,0.1],"size":[0.4,0.05],"color":[0.2,0.8,0.3,1],"back_color":[0,0,0,0.8],"fill":0.5}"#);
    let play = spawn_widget(&mut h, "Play", r#"{"pos":[0.4,0.6],"size":[0.2,0.08],"button":true,"text":"Play"}"#);
    ui_tab(&mut h);
    let names: Vec<String> = h.app.designer.test_widgets().into_iter().map(|w| w.1).collect();
    assert!(names.contains(&"Health".to_string()) && names.contains(&"Play".to_string()), "{names:?}");
    let kinds = h.app.designer.test_kinds();
    assert!(kinds.contains(&"Progress Bar".to_string()) && kinds.contains(&"Button".to_string()), "{kinds:?}");
    // drag the button: it follows the pointer (snapped to the 8 pixel grid)
    let r = h.app.designer.test_widget_screen(play).unwrap();
    let from = r.center();
    let before = h.app.designer.test_widgets().into_iter().find(|w| w.0 == play).unwrap().2;
    h.drag(from, from + Vec2::new(60.0, -30.0));
    let after = h.app.designer.test_widgets().into_iter().find(|w| w.0 == play).unwrap().2;
    let frame_w = r.width() / 0.2;
    let frame_h = r.height() / 0.08;
    assert!((after[0] - before[0] - 60.0 / frame_w).abs() < 9.0 / 1920.0, "{before:?} -> {after:?}");
    assert!((after[1] - before[1] + 30.0 / frame_h).abs() < 9.0 / 1080.0, "{before:?} -> {after:?}");
    assert_eq!(h.app.studio.selection, Some(play), "clicking a widget selects it");
    // one undo puts it back
    h.key(Key::Z, Modifiers::COMMAND);
    let undone = h.app.designer.test_widgets().into_iter().find(|w| w.0 == play).unwrap().2;
    assert!((undone[0] - before[0]).abs() < 1e-4 && (undone[1] - before[1]).abs() < 1e-4, "{before:?} -> {undone:?}");
    let _ = bar;
}

#[test]
fn a_handle_resizes_and_delete_and_duplicate_work_on_the_selected_widget() {
    let mut h = Harness::new();
    let w = spawn_widget(&mut h, "Panel", r#"{"pos":[0.2,0.2],"size":[0.3,0.2],"color":[0.1,0.1,0.3,1]}"#);
    ui_tab(&mut h);
    h.click(h.app.designer.test_widget_screen(w).unwrap().center());
    assert_eq!(h.app.studio.selection, Some(w));
    h.frames(2);
    // the right handle is at the middle of the right edge
    let r = h.app.designer.test_widget_screen(w).unwrap();
    h.drag(r.right_center(), r.right_center() + Vec2::new(80.0, 0.0));
    let size = h.app.designer.test_widgets().into_iter().find(|x| x.0 == w).unwrap().3;
    assert!(size[0] > 0.3 + 0.05 && (size[1] - 0.2).abs() < 1e-4, "wider only: {size:?}");
    // add from the palette code path, then duplicate and delete by key
    h.app.designer.test_add(&mut h.app.studio, "text");
    h.frames(3);
    assert!(h.app.designer.test_kinds().contains(&"Text Block".to_string()));
    let n = h.app.designer.test_widgets().len();
    h.key(Key::D, Modifiers::COMMAND);
    h.frames(2);
    assert_eq!(h.app.designer.test_widgets().len(), n + 1, "duplicated");
    h.key(Key::Delete, Modifiers::NONE);
    h.frames(2);
    assert_eq!(h.app.designer.test_widgets().len(), n, "deleted the copy");
}
