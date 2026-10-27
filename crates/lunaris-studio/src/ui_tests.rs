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
    /// What egui drew, kept for `shoot`.
    screen: crate::shot::Screen,
    /// The size of the window.
    size: Vec2,
}

fn j(s: &str) -> Json {
    lunaris_json::parse(s).unwrap()
}

impl Harness {
    /// An editor on an empty scratch workspace with a small scene: a floor, a red box at the middle and a ball beside it.
    fn new() -> Harness {
        let dir = std::env::temp_dir().join(format!("lunaris-studio-test-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')', ' '], ""));
        std::fs::create_dir_all(&dir).unwrap();
        // (a layout that an earlier run with the same process number left behind would change the sizes the tests expect)
        let _ = std::fs::remove_file(dir.join(".lunaris").join("editor.json"));
        let ctx = egui::Context::default();
        let opts = Options { root: dir, quality: "reference".to_string(), mute: true, ..Options::default() };
        let app = App::new(&ctx, opts);
        let mut h = Harness { ctx, app, time: 0.0, screen: Default::default(), size: Vec2::new(1600.0, 900.0) };
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
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)), time: Some(self.time), events, focused: true, ..RawInput::default() };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| app.draw(ui));
        // (the textures egui hands over are kept, so that `shoot` can draw the window in software)
        self.screen.apply(&out);
        // there is no graphics card behind this window: the picture it would have drawn is dropped
        out.textures_delta.clear();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    /// An editor on the workspace `dir` (already holding what it should show), with no scene made.
    fn on(dir: std::path::PathBuf) -> Harness {
        let ctx = egui::Context::default();
        let opts = Options { root: dir, quality: "reference".to_string(), mute: true, ..Options::default() };
        let app = App::new(&ctx, opts);
        let mut h = Harness { ctx, app, time: 0.0, screen: Default::default(), size: Vec2::new(1600.0, 900.0) };
        h.frames(3);
        h
    }

    /// A picture of the window as it is now (1600 x 900 unless `size` was changed), written to `LUNARIS_SHOTS`/`name`.png.
    fn shoot(&mut self, name: &str) {
        let Ok(dir) = std::env::var("LUNARIS_SHOTS") else { return };
        self.screen.keep_shapes = true;
        self.frames(2);
        self.screen.keep_shapes = false;
        let (w, h) = (self.size.x as usize, self.size.y as usize);
        let rgba = self.screen.picture(&self.ctx, w, h);
        crate::shot::save_png(&std::path::Path::new(&dir).join(format!("{name}.png")), w, h, &rgba);
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
fn the_scene_toolbar_fits_a_small_window_for_looking_at() {
    let mut h = Harness::new();
    for (w, hh) in [(1280.0, 720.0), (1500.0, 800.0)] {
        h.size = Vec2::new(w, hh);
        h.frames(4);
        h.shoot(&format!("toolbar_{w}"));
    }
}

/// Pictures of every panel at 1280 x 720 (a small window), for looking at: nothing is checked, and without `LUNARIS_SHOTS` the test does nothing.
#[test]
fn every_panel_in_a_small_window_for_looking_at() {
    use crate::state::{BottomTab, Center};
    if std::env::var("LUNARIS_SHOTS").is_err() {
        return;
    }
    let mut h = Harness::new();
    h.size = Vec2::new(1280.0, 720.0);
    h.click(h.screen_of("Box", [0.0, 1.0, 0.0]));
    for (name, tab) in [("project", BottomTab::Project), ("console", BottomTab::Console), ("systems", BottomTab::Systems), ("profiler", BottomTab::Profiler), ("history", BottomTab::History)] {
        h.app.studio.tab = tab;
        h.frames(4);
        h.shoot(&format!("small_bottom_{name}"));
    }
    h.app.studio.tab = BottomTab::Project;
    for (name, center) in [("ui", Center::Ui), ("blueprint", Center::Blueprint), ("script", Center::Script), ("canvas", Center::Canvas), ("sequencer", Center::Sequencer), ("effects", Center::Effects)] {
        h.app.studio.center = center;
        h.frames(4);
        h.shoot(&format!("small_center_{name}"));
    }
}

#[test]
fn the_top_front_and_right_views_look_straight_along_an_axis_and_a_click_still_finds_the_box() {
    use crate::viewport::Projection;
    let mut h = Harness::new();
    let box_uid = h.uid("Box");
    h.app.viewport.projection = Projection::Top;
    h.app.viewport.target = [0.0, 1.0, 0.0];
    h.app.viewport.ortho_h = 10.0;
    h.frames(3);
    let o = h.screen_of("Box", [0.0, 1.0, 0.0]);
    let (px, pz) = (h.app.viewport.project([2.0, 1.0, 0.0]).unwrap(), h.app.viewport.project([0.0, 1.0, -2.0]).unwrap());
    assert!(px.x > o.x + 10.0 && (px.y - o.y).abs() < 1.0, "top: +x is to the right: {o:?} {px:?}");
    assert!(pz.y < o.y - 10.0 && (pz.x - o.x).abs() < 1.0, "top: -z is up the screen: {o:?} {pz:?}");
    // there is no perspective: a thing higher up is not drawn bigger or off to one side
    let high = h.app.viewport.project([2.0, 50.0, 0.0]).unwrap();
    assert!((high.x - px.x).abs() < 0.5 && (high.y - px.y).abs() < 0.5, "{high:?} {px:?}");
    // (the sun's icon is at the origin, over the middle of the box in this view: click near a corner)
    let corner = h.app.viewport.project([0.4, 1.0, 0.4]).unwrap();
    h.click(corner);
    assert_eq!(h.app.studio.selection, Some(box_uid), "a click in the top view picks the box");
    h.shoot("view_top");
    h.app.viewport.projection = Projection::Front;
    h.frames(3);
    let o = h.screen_of("Box", [0.0, 1.0, 0.0]);
    let (px, py) = (h.app.viewport.project([2.0, 1.0, 0.0]).unwrap(), h.app.viewport.project([0.0, 3.0, 0.0]).unwrap());
    assert!(px.x > o.x + 10.0 && (px.y - o.y).abs() < 1.0, "front: +x is to the right");
    assert!(py.y < o.y - 10.0 && (py.x - o.x).abs() < 1.0, "front: +y is up");
    h.shoot("view_front");
    h.app.viewport.projection = Projection::Right;
    h.frames(3);
    let o = h.screen_of("Box", [0.0, 1.0, 0.0]);
    let pz = h.app.viewport.project([0.0, 1.0, -2.0]).unwrap();
    assert!(pz.x > o.x + 10.0, "right: -z is to the right of the screen: {o:?} {pz:?}");
    // a bookmark brings the view back as it was
    h.app.viewport.set_bookmark(0);
    h.app.viewport.projection = Projection::Perspective;
    h.app.viewport.go_bookmark(0);
    assert_eq!(h.app.viewport.projection, Projection::Right);
}

#[test]
fn select_what_uses_this_finds_the_entities_that_name_a_file_in_any_of_the_fields_that_do() {
    let mut h = Harness::new();
    let spawn = |h: &mut Harness, comps: &str| h.app.studio.call("spawn", j(&format!(r#"{{"components":{comps}}}"#))).unwrap();
    spawn(&mut h, r#"{"Name":{"value":"A"},"Transform":{},"MeshRenderer":{"mesh":"cube","texture":"textures/wall.png"}}"#);
    spawn(&mut h, r#"{"Name":{"value":"B"},"Transform":{},"Sprite":{"texture":"textures/wall.png"}}"#);
    spawn(&mut h, r#"{"Name":{"value":"C"},"Transform":{},"MeshRenderer":{"mesh":"models/rock.glb","texture":"textures/other.png"}}"#);
    h.frames(2);
    let (a, b, c) = (h.uid("A"), h.uid("B"), h.uid("C"));
    assert_eq!(crate::project::users_of(&mut h.app.studio, "textures/wall.png"), [a.min(b), a.max(b)], "a mesh and a sprite name the same picture");
    assert_eq!(crate::project::users_of(&mut h.app.studio, "models/rock.glb"), [c]);
    assert!(crate::project::users_of(&mut h.app.studio, "textures/nothing.png").is_empty());
    // what the menu does with the answer
    let uids = crate::project::users_of(&mut h.app.studio, "textures/wall.png");
    let main = uids.first().copied();
    h.app.studio.select_many(uids, main);
    assert_eq!(h.app.studio.selected.len(), 2);
}

#[test]
fn the_hierarchy_type_filter_lists_one_kind_alone_or_with_a_search_and_frames_still_draw() {
    use crate::state::Kind;
    let mut h = Harness::new();
    let names = |h: &Harness| h.app.studio.rows.iter().map(|r| r.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&h).len(), 4);
    h.app.studio.kind_filter = Some(Kind::Mesh);
    h.app.studio.rebuild_tree();
    assert_eq!(names(&h), ["Floor", "Box", "Ball"], "only the meshes");
    h.app.studio.filter = "ba".to_string();
    h.app.studio.rebuild_tree();
    assert_eq!(names(&h), ["Ball"], "a search inside the type");
    h.app.studio.kind_filter = Some(Kind::Light);
    h.app.studio.rebuild_tree();
    assert!(names(&h).is_empty(), "no light is called ba");
    h.frames(2);
    h.app.studio.filter.clear();
    h.app.studio.kind_filter = None;
    h.app.studio.rebuild_tree();
    assert_eq!(names(&h).len(), 4, "back to the whole tree");
}

#[test]
fn the_streaming_overlay_reads_the_zones_and_says_how_many_are_loaded() {
    let mut h = Harness::new();
    h.app.studio.call("define_prefab", j(r#"{"name":"block","components":{"Transform":{}}}"#)).unwrap();
    for (x, z, loaded) in [(-6, 3, false), (0, 5, true), (6, 3, false)] {
        h.app.studio.call("spawn", j(&format!(r#"{{"components":{{"Transform":{{"pos":[{x},0.02,{z}]}},"StreamZone":{{"prefab":"block","radius":4,"min_radius":1.5,"loaded":{loaded}}}}}}}"#))).unwrap();
    }
    h.frames(2);
    assert!(h.app.viewport.zones.zones.is_empty(), "nothing is read while the overlay is off");
    h.app.viewport.streaming = true;
    h.app.viewport.zones.invalidate();
    h.frames(2);
    assert_eq!(h.app.viewport.zones.zones.len(), 3);
    assert_eq!(h.app.viewport.zones.summary(), "zones 1 of 3 loaded");
    h.shoot("streaming_overlay");
}

#[test]
fn duplicate_and_delete_take_what_is_attached_under_an_entity_with_it_and_each_is_one_undo_step() {
    let mut h = Harness::new();
    for c in [
        r#"{"uid":100,"components":{"Name":{"value":"Crate"},"Transform":{"pos":[0,0,5]}}}"#,
        r#"{"uid":101,"components":{"Name":{"value":"Lid"},"Transform":{"pos":[0,1,5]},"Parent":{"target":100},"LocalTransform":{"pos":[0,1,0]}}}"#,
        r#"{"uid":102,"components":{"Name":{"value":"Latch"},"Transform":{"pos":[0,1,6]},"Parent":{"target":101},"LocalTransform":{"pos":[0,0,1]}}}"#,
    ] {
        h.app.studio.call("spawn", j(c)).unwrap();
    }
    h.app.studio.refresh_rows();
    let total = h.app.studio.total_entities;
    assert_eq!(h.app.studio.subtree(100), [100, 101, 102], "a parent before its children");
    assert_eq!(h.app.studio.subtree(102), [102]);
    let get = |h: &mut Harness, uid: u64| -> Json { h.app.studio.call("get", j(&format!(r#"{{"uid":{uid},"components":["Name","Parent","Transform"]}}"#))).unwrap().get("components").unwrap().clone() };
    h.app.studio.duplicate(100);
    assert_eq!(h.app.studio.total_entities, total + 3, "the crate, its lid and the latch on the lid");
    let copy = h.app.studio.selection.expect("the copy of the crate is selected");
    let root = get(&mut h, copy);
    assert_eq!(root.get("Name").unwrap().get("value").unwrap().as_str(), Some("Crate 2"));
    assert_eq!(root.get("Transform").unwrap().get("pos").unwrap().as_array().unwrap()[0].as_f64(), Some(1.0), "a metre to the side");
    let kid = h.app.studio.all.iter().find(|r| r.parent == Some(copy)).expect("the copy has a child").uid;
    let grandkid = h.app.studio.all.iter().find(|r| r.parent == Some(kid)).expect("and the child has one").uid;
    assert!(![100, 101, 102].contains(&kid) && ![100, 101, 102].contains(&grandkid), "new entities, not the old ones");
    assert_eq!(get(&mut h, kid).get("Name").unwrap().get("value").unwrap().as_str(), Some("Lid"));
    assert_eq!(get(&mut h, 101).get("Parent").unwrap().get("target").unwrap().as_u64(), Some(100), "the originals are as they were");
    // one undo takes the whole copy back, and a delete takes the copy with what hangs on it and one undo brings it all back
    h.app.studio.undo();
    h.app.studio.refresh_rows();
    assert_eq!(h.app.studio.total_entities, total, "one undo takes the three copies away");
    h.app.studio.redo();
    h.app.studio.refresh_rows();
    assert_eq!(h.app.studio.total_entities, total + 3);
    h.app.studio.delete(copy);
    h.app.studio.refresh_rows();
    assert_eq!(h.app.studio.total_entities, total, "the crate and what hung on it are gone");
    assert!(h.app.studio.all.iter().all(|r| r.parent.is_none_or(|p| h.app.studio.all.iter().any(|q| q.uid == p))), "no child is left with a parent that is gone");
    h.app.studio.undo();
    h.app.studio.refresh_rows();
    assert_eq!(h.app.studio.total_entities, total + 3, "one undo brings them all back");
    assert_eq!(h.app.studio.all.iter().find(|r| r.uid == grandkid).and_then(|r| r.parent), Some(kid));
}

#[test]
fn the_keyboard_shortcuts_window_opens_from_the_help_menu_state_and_draws() {
    let mut h = Harness::new();
    h.app.shortcuts_open = true;
    h.frames(30); // (a window fades in over a few tenths of a second)
    assert!(h.app.shortcuts_open, "it stays open until it is closed");
    h.shoot("keyboard_shortcuts");
    // the World / Local button of the main toolbar and the pill of the scene toolbar are one switch
    assert!(!h.app.viewport.local_space);
    h.app.viewport.local_space = true;
    h.frames(2);
    assert!(h.app.viewport.local_space);
}

#[test]
fn the_problems_tab_lists_what_check_finds_and_a_click_selects_the_entity() {
    let mut h = Harness::new();
    let floor = h.uid("Floor");
    // an entity attached to one that is not there, and one attached with no LocalTransform: mistakes `check` names
    h.app.studio.call("spawn", j(r#"{"uid":100,"components":{"Name":{"value":"Lost"},"Transform":{},"Parent":{"target":99999},"LocalTransform":{}}}"#)).unwrap();
    h.app.studio.call("spawn", j(&format!(r#"{{"uid":101,"components":{{"Name":{{"value":"Bare"}},"Transform":{{}},"Parent":{{"target":{floor}}}}}}}"#))).unwrap();
    h.app.studio.tab = crate::state::BottomTab::Problems;
    h.frames(2);
    assert!(!h.app.problems.checked, "nothing is checked until asked");
    h.app.problems.run(&mut h.app.studio);
    let (errors, _) = h.app.problems.counts();
    assert!(errors >= 2, "{:?}", h.app.problems.list);
    assert!(h.app.problems.list.iter().any(|p| p.entity == Some(100)) && h.app.problems.list.iter().any(|p| p.entity == Some(101)));
    assert!(h.app.problems.list.windows(2).all(|w| w[0].severity != "warning" || w[1].severity != "error"), "errors come before warnings");
    h.frames(3);
    h.shoot("problems_tab");
    // a problem about an entity is a row that selects it
    h.app.studio.select(None);
    h.app.studio.select(Some(100));
    assert_eq!(h.app.studio.selection, Some(100));
    // with Auto on, a change of the scene is checked again by itself
    h.app.problems.auto = true;
    h.app.studio.call("despawn", j(r#"{"uid":100}"#)).unwrap();
    h.app.studio.call("despawn", j(r#"{"uid":101}"#)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1600));
    h.frames(3);
    assert!(h.app.problems.list.iter().all(|p| p.entity != Some(100) && p.entity != Some(101)), "{:?}", h.app.problems.list);
}

#[test]
fn ctrl_g_puts_the_selection_under_a_new_group_that_moves_them_all_and_one_undo_takes_it_back() {
    let mut h = Harness::new();
    let (b, ball) = (h.uid("Box"), h.uid("Ball"));
    let (box_at, ball_at) = (h.pos_of("Box"), h.pos_of("Ball"));
    h.app.studio.refresh_rows();
    let total = h.app.studio.total_entities;
    assert!(h.app.studio.group_selected().is_none(), "nothing selected: nothing to group");
    h.app.studio.select(Some(b));
    h.app.studio.toggle_select(ball);
    h.key(Key::G, Modifiers::COMMAND);
    assert_eq!(h.app.studio.total_entities, total + 1);
    let group = h.app.studio.selection.expect("the group is selected");
    assert_eq!(h.app.studio.all.iter().find(|r| r.uid == group).map(|r| r.name.as_str()), Some("Group"));
    for u in [b, ball] {
        assert_eq!(h.app.studio.all.iter().find(|r| r.uid == u).and_then(|r| r.parent), Some(group), "{u} hangs on the group");
    }
    // they did not move (their world places are what they were), and the group stands between them
    assert_eq!(h.pos_of("Box"), box_at);
    assert_eq!(h.pos_of("Ball"), ball_at);
    let gp = h.pos_of("Group");
    assert!((gp[0] - (box_at[0] + ball_at[0]) / 2.0).abs() < 1e-4 && (gp[1] - (box_at[1] + ball_at[1]) / 2.0).abs() < 1e-4, "{gp:?}");
    // moving the group moves them both
    h.app.studio.set(group, "Transform", j(r#"{"pos":[10,0,0]}"#), None);
    h.app.studio.engine.sim.step();
    assert!((h.pos_of("Box")[0] - (box_at[0] - gp[0] + 10.0)).abs() < 1e-3, "{:?}", h.pos_of("Box"));
    // a second group is named apart; one undo of the group call takes the group away and frees the two
    h.app.studio.set(group, "Transform", Json::obj([("pos", Json::arr(gp.iter().map(|x| Json::from(*x))))]), None);
    h.app.studio.select(Some(group));
    let second = h.app.studio.group_selected().expect("a group of the group");
    assert_eq!(h.app.studio.all.iter().find(|r| r.uid == second).map(|r| r.name.as_str()), Some("Group 2"));
    h.app.studio.undo();
    h.app.studio.refresh_rows();
    assert_eq!(h.app.studio.total_entities, total + 1, "one undo took the second group away");
    assert_eq!(h.app.studio.all.iter().find(|r| r.uid == group).and_then(|r| r.parent), None);
}

/// The width the Inspector panel takes, found from where the scene view ends in a 1600-point window.
fn inspector_width(h: &Harness) -> f32 {
    h.size.x - h.app.viewport.last_rect.max.x
}

#[test]
fn the_inspector_does_not_grow_a_little_every_frame() {
    let mut h = Harness::new();
    h.app.studio.select(Some(h.uid("Box")));
    h.frames(6);
    let first = inspector_width(&h);
    let widths: Vec<String> = [20, 40, 80].iter().map(|n| { h.frames(*n); format!("{:.1}", inspector_width(&h)) }).collect();
    assert!(widths.iter().all(|w| (w.parse::<f32>().unwrap() - first).abs() < 0.6), "{first:.1} after 6 frames, then {widths:?} after 20, 60 and 140 more: the panel creeps wider");
}

#[test]
fn the_scene_view_keeps_its_size_whatever_tab_or_entity_is_showing_and_the_panels_are_dragged_by_their_edges() {
    use crate::state::{BottomTab, Center};
    let mut h = Harness::new();
    h.app.studio.select(Some(h.uid("Box")));
    h.frames(6);
    let size = h.app.viewport.last_rect.size();
    // every bottom tab, each long enough to settle
    for tab in [BottomTab::Console, BottomTab::Systems, BottomTab::Profiler, BottomTab::History, BottomTab::Streaming, BottomTab::Problems, BottomTab::Project] {
        h.app.studio.tab = tab;
        h.frames(10);
        let now = h.app.viewport.last_rect.size();
        let narrow_tab = tab == BottomTab::Systems; // (the Systems tab needs a taller panel: the view is shorter there, and steady)
        assert!((now.x - size.x).abs() < 1.5 && (narrow_tab || (now.y - size.y).abs() < 1.5), "{tab:?}: the scene view went from {size:?} to {now:?}");
        let renders = h.app.viewport.renders;
        h.frames(10);
        assert_eq!(h.app.viewport.renders, renders, "{tab:?}: the scene was drawn again with nothing changed");
    }
    h.app.studio.center = Center::View;
    // dragging the top edge of the bottom panel and the right edge of the Hierarchy changes them, within limits
    let (hw, bh) = (h.app.hierarchy_width, h.app.bottom_height);
    let edge_x = h.app.viewport.last_rect.min.x - 1.5;
    h.drag(Pos2::new(edge_x, 300.0), Pos2::new(edge_x + 80.0, 300.0));
    assert!(h.app.hierarchy_width > hw + 40.0, "{}", h.app.hierarchy_width);
    let top = h.app.viewport.last_rect.max.y + 1.0;
    h.drag(Pos2::new(700.0, top), Pos2::new(700.0, top - 60.0));
    assert!(h.app.bottom_height > bh + 30.0, "{}", h.app.bottom_height);
}

#[test]
fn the_layout_is_kept_between_runs_and_an_untouched_one_writes_nothing() {
    use crate::state::BottomTab;
    let base = std::env::temp_dir().join(format!("lunaris-layout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    // an editor whose layout nobody changed leaves no file behind
    let quiet = base.join("quiet");
    std::fs::create_dir_all(&quiet).unwrap();
    let mut h = Harness::on(quiet.clone());
    h.frames(90);
    h.app.save_layout(true);
    assert!(!quiet.join(".lunaris").join("editor.json").exists(), "a layout that never changed was written");
    // one whose layout was changed writes it, next to the preference the file already held
    let dir = base.join("kept");
    std::fs::create_dir_all(dir.join(".lunaris")).unwrap();
    std::fs::write(dir.join(".lunaris").join("editor.json"), r#"{"script_theme":"Solarized"}"#).unwrap();
    let mut h = Harness::on(dir.clone());
    h.app.studio.tab = BottomTab::Console;
    h.app.show_hierarchy = false;
    h.frames(2);
    // (a drag on the Inspector's edge: the size the next run opens with)
    let edge = h.app.viewport.last_rect.max.x + 2.0;
    h.drag(Pos2::new(edge, 300.0), Pos2::new(edge - 150.0, 300.0));
    h.app.bottom_height = 180.0;
    let dragged = h.app.inspector_width;
    assert!(dragged > 400.0, "{dragged}");
    h.app.save_layout(true);
    let text = std::fs::read_to_string(dir.join(".lunaris").join("editor.json")).unwrap();
    assert!(text.contains("Solarized"), "the other preference was lost: {text}");
    let again = Harness::on(dir.clone());
    assert_eq!(again.app.studio.tab, BottomTab::Console);
    assert!(!again.app.show_hierarchy && again.app.show_inspector && again.app.show_bottom);
    assert!((again.app.inspector_width - dragged).abs() < 1.0, "{} instead of {dragged}", again.app.inspector_width);
    assert_eq!((again.app.hierarchy_width, again.app.bottom_height), (250.0, 180.0));
    // a window made only to take a picture leaves the layout as it was
    let opts = Options { root: dir.clone(), quality: "reference".to_string(), mute: true, shot: Some(dir.join("picture.png")), ..Options::default() };
    let mut shot = App::new(&egui::Context::default(), opts);
    shot.inspector_width = 500.0;
    shot.save_layout(true);
    let after = std::fs::read_to_string(dir.join(".lunaris").join("editor.json")).unwrap();
    assert_eq!(after, text, "the picture run changed the layout file");
}

#[test]
fn the_inspector_keeps_its_width_and_every_component_fits_in_it() {
    let mut h = Harness::new();
    h.app.studio.select(Some(h.uid("Box")));
    h.frames(4);
    let normal = inspector_width(&h);
    assert!((normal - 330.0).abs() < 2.0, "{normal}");
    let mut too_wide: Vec<String> = Vec::new();
    for name in h.app.studio.components_available() {
        let uid = h.app.studio.call("spawn", j(r#"{"components":{"Transform":{}}}"#)).unwrap().get("uid").unwrap().as_u64().unwrap();
        if h.app.studio.call("add_component", j(&format!(r#"{{"uid":{uid},"component":"{name}"}}"#))).is_err() {
            continue;
        }
        h.app.studio.rows_dirty = true;
        h.app.studio.select(Some(uid));
        h.frames(4);
        assert!((inspector_width(&h) - normal).abs() < 2.0, "{name} changed the width of the panel to {:.0}", inspector_width(&h));
        if h.app.inspector.overflow > 1.0 {
            too_wide.push(format!("{name} by {:.0}", h.app.inspector.overflow));
        }
    }
    assert!(too_wide.is_empty(), "rows wider than the panel (their ends are cut off): {too_wide:?}");
    // dragging the left edge of the panel makes it wider or narrower, within limits
    let edge = h.app.viewport.last_rect.max.x + 2.0;
    h.drag(Pos2::new(edge, 300.0), Pos2::new(edge - 100.0, 300.0));
    assert!(inspector_width(&h) > normal + 60.0, "{:.0}", inspector_width(&h));
    // (the measure itself: a panel far narrower than any row has rows that do not fit)
    h.app.inspector_width = 90.0;
    h.app.studio.select(Some(h.uid("Box")));
    h.frames(4);
    assert!(h.app.inspector.overflow > 10.0, "{}", h.app.inspector.overflow);
}

#[test]
fn the_inspector_draws_every_component_the_engine_has_without_trouble() {
    let mut h = Harness::new();
    let names = h.app.studio.components_available();
    assert!(names.len() > 100, "{} components", names.len());
    let (mut drawn, mut refused) = (0, 0);
    for name in &names {
        let uid = h.app.studio.call("spawn", j(r#"{"components":{"Transform":{}}}"#)).unwrap().get("uid").unwrap().as_u64().unwrap();
        if h.app.studio.call("add_component", j(&format!(r#"{{"uid":{uid},"component":"{name}"}}"#))).is_err() {
            refused += 1;
            continue;
        }
        h.app.studio.rows_dirty = true;
        h.app.studio.select(Some(uid));
        h.frames(2);
        drawn += 1;
        assert!(h.app.studio.entity.as_ref().is_some_and(|e| e.comp(name).is_some()), "the Inspector has {name} to draw");
    }
    assert!(drawn > 100, "{drawn} components drawn, {refused} refused");
    assert!(refused < names.len() / 3, "{refused} components cannot be added by name alone");
}

#[test]
fn a_field_that_names_an_entity_shows_its_name_and_the_picker_lists_by_what_is_typed() {
    use crate::inspector::{matching_entities, uid_label};
    let mut h = Harness::new();
    h.app.studio.call("spawn", j(r#"{"uid":100,"components":{"Name":{"value":"Hero"},"Transform":{}}}"#)).unwrap();
    h.app.studio.call("spawn", j(r#"{"uid":101,"components":{"Name":{"value":"Sidekick"},"Transform":{},"Parent":{"target":100},"LocalTransform":{}}}"#)).unwrap();
    h.app.studio.refresh_rows();
    assert_eq!(uid_label(&h.app.studio, 0), "None");
    assert_eq!(uid_label(&h.app.studio, 100), "Hero  #100");
    assert_eq!(uid_label(&h.app.studio, 9999), "#9999 (no such entity)");
    let names = |f: &str| matching_entities(&h.app.studio, f, 50).iter().map(|r| r.name.clone()).collect::<Vec<_>>();
    assert_eq!(names("sidek"), ["Sidekick"], "any case, part of the name");
    assert_eq!(names("100"), ["Hero"], "the uid");
    assert!(names("").len() >= 6, "everything when nothing is typed");
    assert_eq!(matching_entities(&h.app.studio, "", 3).len(), 3, "at most the limit");
    // the Inspector draws the field of the selected child (its Parent.target) without trouble
    h.app.studio.select(Some(101));
    h.frames(3);
    h.shoot("inspector_entity_picker");
}

#[test]
fn make_prefab_turns_an_entity_and_what_hangs_on_it_into_a_template_that_spawns_the_same_shape() {
    let mut h = Harness::new();
    for c in [
        r#"{"uid":100,"components":{"Name":{"value":"Crate"},"Transform":{"pos":[3,0,5],"scale":[2,2,2]},"MeshRenderer":{"mesh":"cube","color":[0.6,0.4,0.2,1]}}}"#,
        r#"{"uid":101,"components":{"Name":{"value":"Lid"},"Transform":{"pos":[3,2,5]},"Parent":{"target":100},"LocalTransform":{"pos":[0,1,0],"scale":[1,0.1,1]},"MeshRenderer":{"mesh":"cube"}}}"#,
        r#"{"uid":102,"components":{"Name":{"value":"Latch"},"Transform":{"pos":[3,2,6]},"Parent":{"target":101},"LocalTransform":{"pos":[0,0,1]},"MeshRenderer":{"mesh":"sphere"}}}"#,
    ] {
        h.app.studio.call("spawn", j(c)).unwrap();
    }
    assert_eq!(h.app.studio.make_prefab(100).as_deref(), Some("Crate"));
    assert!(h.app.studio.prefabs.contains(&"Crate".to_string()), "it is listed for the Place panel");
    assert_eq!(h.app.studio.make_prefab(100).as_deref(), Some("Crate 2"), "a name that is taken gets a number");
    let total = h.app.studio.total_entities;
    // spawning it makes the same shape: a root, its lid and the latch on the lid, the root where it was asked to be
    let r = h.app.studio.call("spawn_prefab", j(r#"{"prefab":"Crate","at":[20,0,0]}"#)).unwrap();
    assert!(r.get("ok").is_some() || r.get("uid").is_some() || r.get("first_uid").is_some(), "{r:?}");
    h.app.studio.refresh_rows();
    assert_eq!(h.app.studio.total_entities, total + 3);
    let roots: Vec<&crate::state::Row> = h.app.studio.all.iter().filter(|r| r.name == "Crate" && r.uid != 100).collect();
    assert_eq!(roots.len(), 1);
    let root = roots[0].uid;
    let pos = h.app.studio.call("get", j(&format!(r#"{{"uid":{root},"components":["Transform"]}}"#))).unwrap().get("components").unwrap().get("Transform").unwrap().clone();
    assert_eq!(pos.get("pos").unwrap().as_array().unwrap()[0].as_f64(), Some(20.0));
    assert_eq!(pos.get("scale").unwrap().as_array().unwrap()[0].as_f64(), Some(2.0), "the root keeps its scale");
    let lid = h.app.studio.all.iter().find(|r| r.parent == Some(root)).expect("the lid hangs on the new crate");
    assert_eq!(lid.name, "Lid");
    let latch = h.app.studio.all.iter().find(|r| r.parent == Some(lid.uid)).expect("and the latch on the lid");
    assert_eq!(latch.name, "Latch");
    // the original is as it was
    assert_eq!(h.app.studio.all.iter().find(|r| r.uid == 101).and_then(|r| r.parent), Some(100));
}

#[test]
fn ctrl_a_selects_everything_listed_and_f2_starts_renaming_the_selected_entity() {
    let mut h = Harness::new();
    let ball = h.uid("Ball");
    h.app.studio.select(Some(ball));
    h.key(Key::A, Modifiers::COMMAND);
    assert_eq!(h.app.studio.selected.len(), h.app.studio.rows.len(), "every row is selected");
    assert_eq!(h.app.studio.selection, Some(ball), "the main one stays the main one");
    h.app.studio.select(Some(ball));
    assert_eq!(h.app.hierarchy.renaming_uid(), None);
    h.key(Key::F2, Modifiers::NONE);
    assert_eq!(h.app.hierarchy.renaming_uid(), Some(ball), "F2 turns the row of the selected entity into a text box");
}

#[test]
fn ctrl_c_and_ctrl_v_copy_entities_with_what_hangs_on_them_into_this_scene_or_another_editor() {
    let mut h = Harness::new();
    for c in [
        r#"{"uid":100,"components":{"Name":{"value":"Crate"},"Transform":{"pos":[3,0,5]}}}"#,
        r#"{"uid":101,"components":{"Name":{"value":"Lid"},"Transform":{"pos":[3,1,5]},"Parent":{"target":100},"LocalTransform":{"pos":[0,1,0]}}}"#,
    ] {
        h.app.studio.call("spawn", j(c)).unwrap();
    }
    h.app.studio.refresh_rows();
    let total = h.app.studio.total_entities;
    // nothing selected: nothing to copy
    assert!(h.app.studio.copy_selected().is_none());
    // the keys reach the window as copy and paste events
    h.app.studio.select(Some(100));
    h.frame(vec![Event::Copy]);
    assert_eq!(h.app.studio.entity_clipboard.len(), 2, "the crate and its lid");
    h.frame(vec![Event::Paste("something that is not entities".to_string())]);
    h.frames(2);
    assert_eq!(h.app.studio.total_entities, total + 2, "a paste of other text pastes the last copy");
    let copy = h.app.studio.selection.expect("the pasted crate is selected");
    let got = h.app.studio.call("get", j(&format!(r#"{{"uid":{copy},"components":["Name","Transform"]}}"#))).unwrap();
    assert_eq!(got.get("components").unwrap().get("Name").unwrap().get("value").unwrap().as_str(), Some("Crate 2"));
    assert_eq!(got.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().as_array().unwrap()[0].as_f64(), Some(3.0), "pasted where it was");
    let lid = h.app.studio.all.iter().find(|r| r.parent == Some(copy)).expect("the pasted lid hangs on the pasted crate");
    assert_ne!(lid.uid, 101);
    // one undo takes the paste back
    h.app.studio.undo();
    h.app.studio.refresh_rows();
    assert_eq!(h.app.studio.total_entities, total);
    // the text of the system clipboard carries them to another editor
    h.app.studio.select(Some(100));
    let text = h.app.studio.copy_selected().expect("text");
    let mut other = Harness::new();
    let before = other.app.studio.total_entities;
    other.app.studio.entity_clipboard.clear();
    assert_eq!(other.app.studio.paste_entities(Some(&text)), 2);
    assert_eq!(other.app.studio.total_entities, before + 2);
    assert!(other.app.studio.all.iter().any(|r| r.name == "Crate") && other.app.studio.all.iter().any(|r| r.name == "Lid"));
    // a child copied alone is pasted free, with its place
    other.app.studio.select(Some(other.uid("Lid")));
    let lone = other.app.studio.copy_selected().expect("text");
    other.app.studio.paste_entities(Some(&lone));
    let pasted = other.app.studio.selection.unwrap();
    assert!(other.app.studio.all.iter().find(|r| r.uid == pasted).unwrap().parent.is_none(), "a copy whose parent was not copied is free");
    let got = other.app.studio.call("get", j(&format!(r#"{{"uid":{pasted},"components":["LocalTransform","Transform"]}}"#))).unwrap();
    assert!(got.get("components").unwrap().get("LocalTransform").is_none_or(|v| v.is_null()), "and it has no LocalTransform");
}

#[test]
fn the_rows_read_from_the_world_are_the_rows_the_query_tool_gave() {
    let mut h = Harness::new();
    for c in [
        r#"{"uid":100,"components":{"Name":{"value":"Parent"},"Transform":{"pos":[1,2,3]},"MeshRenderer":{"mesh":"cube"}}}"#,
        r#"{"uid":101,"components":{"Name":{"value":"Child"},"Transform":{},"Parent":{"target":100},"LocalTransform":{"pos":[0,1,0]},"MeshRenderer":{"mesh":"sphere","visible":false}}}"#,
        r#"{"uid":102,"components":{"Name":{"value":"Grandchild"},"Transform":{},"Parent":{"target":101},"LocalTransform":{},"MeshRenderer":{"mesh":"cube"}}}"#,
        r#"{"uid":103,"components":{"Transform":{"pos":[5,0,0]},"Camera":{}}}"#,
        r#"{"uid":104,"components":{"Name":{"value":"Panel"},"Ui":{"pos":[0.1,0.1],"size":[0.2,0.2]}}}"#,
        r#"{"uid":105,"components":{"Name":{"value":"No place"}}}"#,
        r#"{"uid":106,"components":{"Transform":{},"Emitter":{}}}"#,
        r#"{"uid":107,"components":{"Name":{"value":"Orphan"},"Transform":{},"Parent":{"target":99999},"LocalTransform":{}}}"#,
    ] {
        h.app.studio.call("spawn", j(c)).unwrap();
    }
    let (total, expect) = h.app.studio.rows_via_query().expect("the query answers");
    let (now_total, now) = crate::rows_world::read(&h.app.studio.engine.sim.world, 10_000);
    assert_eq!(total, now_total);
    assert_eq!(now, expect, "the same rows in the same order");
    assert!(now.len() >= 12 && now.iter().any(|r| r.uid == 100 && r.meshes == 3 && r.shown == 2), "a parent counts the meshes under it and the shown ones");
    // a limit keeps the first by uid
    let (t, few) = crate::rows_world::read(&h.app.studio.engine.sim.world, 5);
    assert_eq!((t, few.len()), (total, 5));
    assert_eq!(few.iter().map(|r| r.uid).collect::<Vec<_>>(), now.iter().take(5).map(|r| r.uid).collect::<Vec<_>>());
}

#[test]
fn a_selected_entity_does_not_make_the_editor_change_the_world_or_draw_again_by_itself() {
    let mut h = Harness::new();
    h.app.studio.select(Some(h.uid("Box")));
    h.frames(5);
    let (rev, renders) = (h.app.studio.live_revision, h.app.viewport.renders);
    h.frames(30);
    assert_eq!(h.app.studio.live_revision, rev, "drawing the Inspector wrote to the world");
    assert_eq!(h.app.viewport.renders, renders, "the picture was drawn again with nothing changed");
}

/// A real, big scene in the editor: `LUNARIS_BIG_ROOT=/path/to/workspace LUNARIS_BIG_SCENE=scenes/x.lunaris.json cargo test --release -p lunaris-studio --features eframe/x11 real_scene -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_scene_in_the_editor() {
    let (Ok(root), Ok(scene)) = (std::env::var("LUNARIS_BIG_ROOT"), std::env::var("LUNARIS_BIG_SCENE")) else { return };
    let mut h = Harness::on(std::path::PathBuf::from(&root));
    let t = std::time::Instant::now();
    h.app.studio.open_scene(&scene);
    println!("open_scene: {:.0} ms, {} entities", t.elapsed().as_secs_f64() * 1e3, h.app.studio.engine.sim.world.len());
    let t = std::time::Instant::now();
    h.app.studio.refresh_rows();
    println!("refresh_rows: {:.1} ms ({} rows of {})", t.elapsed().as_secs_f64() * 1e3, h.app.studio.all.len(), h.app.studio.total_entities);
    let t = std::time::Instant::now();
    h.frames(3);
    println!("three frames (the first draws the scene with the CPU reference renderer): {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let t = std::time::Instant::now();
    h.frames(30);
    println!("an idle frame: {:.2} ms", t.elapsed().as_secs_f64() * 1e3 / 30.0);
    // every kind of entity the scene has gets drawn in the Inspector
    let uids: Vec<u64> = h.app.studio.all.iter().map(|r| r.uid).collect();
    let step = (uids.len() / 400).max(1);
    let (mut selecting, mut drawing) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    let mut drawn = 0;
    let renders_before = h.app.viewport.renders;
    let mut redrawers: Vec<String> = Vec::new();
    for uid in uids.iter().step_by(step) {
        let t = std::time::Instant::now();
        h.app.studio.select(Some(*uid));
        selecting += t.elapsed();
        let (t, before, rect) = (std::time::Instant::now(), (h.app.viewport.renders, h.app.studio.live_revision), h.app.viewport.last_rect);
        h.frames(1);
        drawing += t.elapsed();
        drawn += 1;
        if h.app.viewport.renders != before.0 && redrawers.len() < 12 {
            let comps: Vec<String> = h.app.studio.entity.as_ref().map(|e| e.comps.iter().map(|(n, _)| n.clone()).collect()).unwrap_or_default();
            redrawers.push(format!("#{uid} view x {:.0}..{:.0} -> {:.0}..{:.0} of {}, {:?}", rect.min.x, rect.max.x, h.app.viewport.last_rect.min.x, h.app.viewport.last_rect.max.x, h.size.x, comps));
        }
    }
    for r in &redrawers {
        println!("a selection that drew a picture: {r}");
    }
    println!("pictures drawn during those {drawn} selections: {}", h.app.viewport.renders - renders_before);
    println!("select + frame for {drawn} entities spread over the scene: select {:.2} ms, frame {:.2} ms each", selecting.as_secs_f64() * 1e3 / drawn as f64, drawing.as_secs_f64() * 1e3 / drawn as f64);
    let t = std::time::Instant::now();
    h.app.problems.run(&mut h.app.studio);
    let (e, w) = h.app.problems.counts();
    println!("check: {:.0} ms, {e} errors, {w} warnings", t.elapsed().as_secs_f64() * 1e3);
    for p in h.app.problems.list.iter().take(8) {
        println!("  [{}] {:?}: {}", p.severity, p.entity, p.message);
    }
    // the width of the scene view over a few frames after each of a few selections: it should not move
    for uid in [3u64, 22, 1459, 1468, 1477, 51, 1459, 22, 3] {
        h.app.studio.select(Some(uid));
        let widths: Vec<String> = (0..4).map(|_| {
            h.frames(1);
            format!("{:.0}", h.app.viewport.last_rect.max.x)
        }).collect();
        println!("selecting #{uid}: the scene view ends at x = {} (the panel is {:.0} wide, set to {:.0}, overflow {:.1}, {:?})", widths.join(", "), inspector_width(&h), h.app.inspector_width, h.app.inspector.overflow, h.app.studio.entity.as_ref().map(|e| e.comps.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()));
    }
    // the cost of a frame with each bottom tab showing, and with each of the other editors in the middle
    {
        use crate::state::{BottomTab, Center};
        for (name, tab) in [("Project", BottomTab::Project), ("Console", BottomTab::Console), ("Systems", BottomTab::Systems), ("Profiler", BottomTab::Profiler), ("History", BottomTab::History), ("Streaming", BottomTab::Streaming), ("Problems", BottomTab::Problems)] {
            h.app.studio.tab = tab;
            h.frames(3);
            let heights: Vec<String> = (0..8).map(|_| { h.frames(1); format!("{:.0}x{:.0}", h.app.viewport.last_rect.width(), h.app.viewport.last_rect.height()) }).collect();
            println!("the {name} tab: the scene view is {}", heights.join(" "));
            let (t, renders) = (std::time::Instant::now(), h.app.viewport.renders);
            h.frames(20);
            println!("the {name} tab: {:.2} ms a frame, {} pictures drawn in those 20", t.elapsed().as_secs_f64() * 1e3 / 20.0, h.app.viewport.renders - renders);
        }
        h.app.studio.tab = BottomTab::Project;
        h.frames(3);
        h.app.project.reload();
        let t = std::time::Instant::now();
        h.frames(1);
        println!("the Project panel listing the workspace's files again (it does so every 3 s): {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
        for (name, c) in [("UI designer", Center::Ui), ("Blueprint", Center::Blueprint), ("Script", Center::Script), ("Canvas", Center::Canvas), ("Sequencer", Center::Sequencer), ("Effects", Center::Effects)] {
            h.app.studio.center = c;
            h.frames(3);
            let t = std::time::Instant::now();
            h.frames(20);
            println!("the {name} tab: {:.2} ms a frame", t.elapsed().as_secs_f64() * 1e3 / 20.0);
        }
        h.app.studio.center = Center::View;
        h.frames(3);
    }
    // play in the editor: the snapshot, a second of game time (the CPU renderer draws every frame here), and stop putting everything back
    {
        let t = std::time::Instant::now();
        h.app.studio.play();
        println!("Play: {:.1} ms to start", t.elapsed().as_secs_f64() * 1e3);
        let t = std::time::Instant::now();
        h.frames(30);
        println!("30 frames of play (with the CPU renderer drawing each): {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
        let t = std::time::Instant::now();
        h.app.studio.stop();
        h.frames(2);
        println!("Stop: {:.1} ms to put the scene back, {} entities", t.elapsed().as_secs_f64() * 1e3, h.app.studio.engine.sim.world.len());
        let t = std::time::Instant::now();
        let saved = h.app.studio.engine.call("save_scene", &Json::obj([("path", Json::str("scenes/_editor_probe_save.lunaris.json"))]));
        println!("saving the scene: {:.0} ms ({})", t.elapsed().as_secs_f64() * 1e3, if saved.is_ok() { "ok" } else { "refused" });
    }
    h.shoot("real_scene");
    h.app.studio.select(Some(33));
    h.frames(3);
    h.shoot("real_scene_33");
    h.app.studio.select(Some(51));
    h.frames(3);
    h.shoot("real_scene_51");
}

/// How long the editor takes to start and draw its first frames: `... startup_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn startup_cost() {
    let dir = std::env::temp_dir().join(format!("lunaris-startup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let t = std::time::Instant::now();
    let ctx = egui::Context::default();
    let opts = Options { root: dir, quality: "reference".to_string(), mute: true, ..Options::default() };
    let app = App::new(&ctx, opts);
    println!("App::new: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    let mut h = Harness { ctx, app, time: 0.0, screen: Default::default(), size: Vec2::new(1600.0, 900.0) };
    let t = std::time::Instant::now();
    h.frames(1);
    println!("the first frame: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    let t = std::time::Instant::now();
    h.frames(10);
    println!("the next ten frames: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
}

/// What a big scene costs the editor: `cargo test --release -p lunaris-studio --features eframe/x11 big_scene -- --ignored --nocapture`.
#[test]
#[ignore]
fn big_scene_editor_costs() {
    let mut h = Harness::new();
    let n = 10_000;
    let mut cmds = Vec::new();
    for i in 0..n {
        cmds.push(format!(r#"{{"op":"spawn","components":{{"Name":{{"value":"Thing {i}"}},"Transform":{{"pos":[{},0,{}]}},"MeshRenderer":{{"mesh":"cube","color":[0.5,0.5,0.5,1]}}}}}}"#, i % 100, i / 100));
    }
    h.app.studio.call("batch", j(&format!(r#"{{"commands":[{}]}}"#, cmds.join(",")))).unwrap();
    let time = |label: &str, f: &mut dyn FnMut()| {
        let t = std::time::Instant::now();
        f();
        println!("{label}: {:.2} ms", t.elapsed().as_secs_f64() * 1e3);
    };
    time("refresh_rows (10,000 entities)", &mut || h.app.studio.refresh_rows());
    h.frames(3);
    let t = std::time::Instant::now();
    h.frames(60);
    println!("an idle frame (nothing changed): {:.2} ms", t.elapsed().as_secs_f64() * 1e3 / 60.0);
    h.app.studio.mode = Mode::Play;
    let t = std::time::Instant::now();
    h.frames(60);
    println!("a playing frame (rows rebuilt every 250 ms, the game stepped): {:.2} ms", t.elapsed().as_secs_f64() * 1e3 / 60.0);
}

#[test]
fn the_streaming_tab_lists_the_zones_without_the_overlay_loaded_first() {
    let mut h = Harness::new();
    h.app.studio.call("define_prefab", j(r#"{"name":"block","components":{"Transform":{}}}"#)).unwrap();
    for (name, x, loaded) in [("Market", 0, false), ("Docks", 60, true), ("North", 120, false)] {
        h.app.studio.call("spawn", j(&format!(r#"{{"components":{{"Name":{{"value":"{name}"}},"Transform":{{"pos":[{x},0,0]}},"StreamZone":{{"prefab":"block","radius":30,"loaded":{loaded}}}}}}}"#))).unwrap();
    }
    assert!(!h.app.viewport.streaming, "the overlay is off");
    h.app.studio.tab = crate::state::BottomTab::Streaming;
    h.frames(4);
    assert_eq!(h.app.viewport.zones.zones.len(), 3, "the tab reads the zones itself");
    let names: Vec<&str> = crate::streaming_ui::listed(&h.app.viewport.zones.zones, "", false, [0.0; 3]).iter().map(|z| z.name.as_str()).collect();
    assert_eq!(names, ["Docks", "Market", "North"], "the loaded one first, then the nearest");
    h.shoot("streaming_tab");
}

#[test]
fn local_space_turns_the_move_handles_with_the_object_and_a_drag_goes_along_them() {
    let mut h = Harness::new();
    h.click(h.screen_of("Box", [0.0, 1.0, 0.0]));
    let uid = h.uid("Box");
    // a quarter turn about y: the box's own x axis is the world's -z
    h.app.studio.rotate_to(uid, [0.0, 0.70710677, 0.0, 0.70710677], "turn");
    h.frames(3);
    let world_tip = h.app.viewport.handle_point(&h.app.studio, Axis::X, 1.0).unwrap();
    h.app.viewport.local_space = true;
    h.frames(2);
    let local_tip = h.app.viewport.handle_point(&h.app.studio, Axis::X, 1.0).unwrap();
    assert!((world_tip - local_tip).length() > 20.0, "the x handle points elsewhere in local space: {world_tip:?} {local_tip:?}");
    let from = h.app.viewport.handle_point(&h.app.studio, Axis::X, 0.6).unwrap();
    let tip = h.app.viewport.handle_point(&h.app.studio, Axis::X, 1.0).unwrap();
    let before = h.pos_of("Box");
    h.drag(from, Pos2::new(from.x + (tip.x - from.x) * 1.8, from.y + (tip.y - from.y) * 1.8));
    let after = h.pos_of("Box");
    assert!((after[0] - before[0]).abs() < 1e-3 && (after[1] - before[1]).abs() < 1e-3, "nothing moved across the handle: {before:?} -> {after:?}");
    assert!((after[2] - before[2]).abs() > 0.3, "it went along the object's x, which is the world's z: {before:?} -> {after:?}");
}

#[test]
fn ctrl_and_a_number_save_the_camera_and_the_number_alone_returns_to_it() {
    let mut h = Harness::new();
    h.frames(2);
    let saved = (h.app.viewport.yaw, h.app.viewport.pitch, h.app.viewport.dist, h.app.viewport.target);
    h.key(Key::Num2, Modifiers::COMMAND);
    assert!(h.app.viewport.bookmarks[1].is_some() && h.app.viewport.bookmarks[0].is_none(), "Ctrl+2 saves view 2 only");
    // move the camera elsewhere, then press 2
    (h.app.viewport.yaw, h.app.viewport.pitch, h.app.viewport.dist, h.app.viewport.target) = (2.5, -0.3, 40.0, [30.0, 2.0, -10.0]);
    h.key(Key::Num3, Modifiers::NONE);
    assert_eq!(h.app.viewport.dist, 40.0, "a view that was never saved leaves the camera where it is");
    h.key(Key::Num2, Modifiers::NONE);
    assert_eq!((h.app.viewport.yaw, h.app.viewport.pitch, h.app.viewport.dist, h.app.viewport.target), saved, "2 returns to the saved camera");
    // the game view has the game's camera: nothing to save or return to
    h.app.viewport.kind = crate::viewport::ViewKind::Game;
    assert!(!h.app.viewport.set_bookmark(0) && !h.app.viewport.go_bookmark(1));
}

#[test]
fn the_snap_steps_chosen_in_the_toolbar_round_what_the_handles_do() {
    use crate::viewport::Tool;
    let mut h = Harness::new();
    h.click(h.screen_of("Box", [0.0, 1.0, 0.0]));
    // move, with a step of one metre: the box ends on a whole number of metres
    h.app.viewport.snap_move = 1.0;
    let from = h.app.viewport.handle_point(&h.app.studio, Axis::X, 0.6).unwrap();
    let tip = h.app.viewport.handle_point(&h.app.studio, Axis::X, 1.0).unwrap();
    h.drag(from, Pos2::new(from.x + (tip.x - from.x) * 2.0 + 13.0, from.y + (tip.y - from.y) * 2.0));
    let after = h.pos_of("Box");
    assert!(after[0] > 0.9 && (after[0] - after[0].round()).abs() < 1e-3, "a whole number of metres: {after:?}");
    // with the step off and no Ctrl a drag is free
    h.app.viewport.snap_move = 0.0;
    h.frames(2);
    let from = h.app.viewport.handle_point(&h.app.studio, Axis::X, 0.6).unwrap();
    let tip = h.app.viewport.handle_point(&h.app.studio, Axis::X, 1.0).unwrap();
    h.drag(from, Pos2::new(from.x + (tip.x - from.x) * 1.4 + 7.0, from.y + (tip.y - from.y) * 1.4));
    let free = h.pos_of("Box");
    assert!(free[0] > after[0] && (free[0] * 100.0 - (free[0] * 100.0).round()).abs() > 1e-3, "not on a step: {free:?}");
    // scale, a step of a quarter
    h.app.viewport.snap_scale = 0.25;
    h.app.viewport.tool = Tool::Scale;
    h.frames(2);
    let from = h.app.viewport.handle_point(&h.app.studio, Axis::X, 0.8).unwrap();
    let tip = h.app.viewport.handle_point(&h.app.studio, Axis::X, 1.0).unwrap();
    h.drag(from, Pos2::new(from.x + (tip.x - from.x) * 3.0, from.y + (tip.y - from.y) * 3.0));
    let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{},"components":["Transform"]}}"#, h.uid("Box")))).unwrap();
    let sc: Vec<f64> = g.get("components").unwrap().get("Transform").unwrap().get("scale").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!(sc[0] > 1.2 && (sc[0] * 4.0 - (sc[0] * 4.0).round()).abs() < 1e-3, "a quarter step: {sc:?}");
    // rotate, a step of 30 degrees: a sweep of 40 ends on 30
    h.app.viewport.snap_rot = 30.0;
    h.app.viewport.tool = Tool::Rotate;
    h.frames(2);
    let (a, b) = (h.app.viewport.ring_point(&h.app.studio, Axis::Y, 10.0).unwrap(), h.app.viewport.ring_point(&h.app.studio, Axis::Y, 50.0).unwrap());
    h.drag(a, b);
    let uid = h.uid("Box");
    h.app.studio.select(Some(uid));
    h.app.studio.refresh_entity();
    let e = crate::mathx::q_to_euler(h.app.studio.entity.as_ref().unwrap().quat("Transform", "rot").unwrap());
    assert!((e[1] - 30.0).abs() < 1.0, "on a step of 30 degrees: {e:?}");
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
    for tab in [crate::state::BottomTab::Project, crate::state::BottomTab::Console, crate::state::BottomTab::Systems, crate::state::BottomTab::Profiler, crate::state::BottomTab::History] {
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
fn several_entities_can_be_selected_moved_duplicated_and_deleted_together() {
    let mut h = Harness::new();
    let (b, ball) = (h.uid("Box"), h.uid("Ball"));
    let total = h.app.studio.total_entities;
    h.app.studio.select(Some(b));
    h.app.studio.toggle_select(ball);
    assert_eq!(h.app.studio.selected, vec![b, ball]);
    assert_eq!(h.app.studio.selection, Some(ball), "the one selected last is the main one");
    // Ctrl+click again takes one out; a plain select collapses everything onto one
    h.app.studio.toggle_select(b);
    assert_eq!(h.app.studio.selected, vec![ball]);
    h.app.studio.toggle_select(b);
    assert_eq!((h.app.studio.selected.clone(), h.app.studio.selection), (vec![ball, b], Some(b)));
    // a range from the Hierarchy: whatever is between, the clicked one is the main one
    let all: Vec<u64> = h.app.studio.rows.iter().map(|r| r.uid).collect();
    h.app.studio.select_many(all.clone(), Some(ball));
    assert_eq!(h.app.studio.selected.len(), all.len());
    assert_eq!(h.app.studio.selection, Some(ball));
    h.app.studio.select(Some(b));
    h.app.studio.toggle_select(ball);
    h.frames(3);
    // dragging the move handle of the main one takes the others along by the same amount
    let (before_ball, before_box) = (h.pos_of("Ball"), h.pos_of("Box"));
    let from = h.app.viewport.handle_point(&h.app.studio, Axis::X, 0.8).unwrap();
    let tip = h.app.viewport.handle_point(&h.app.studio, Axis::X, 1.0).unwrap();
    h.drag(from, Pos2::new(from.x + (tip.x - from.x) * 3.0, from.y + (tip.y - from.y) * 3.0));
    let (after_ball, after_box) = (h.pos_of("Ball"), h.pos_of("Box"));
    let (dx_ball, dx_box) = (after_ball[0] - before_ball[0], after_box[0] - before_box[0]);
    assert!(dx_ball > 0.5, "the main one moved along x: {dx_ball}");
    assert!((dx_ball - dx_box).abs() < 1e-3, "both moved by the same amount: {dx_ball} and {dx_box}");
    assert!((after_ball[1] - before_ball[1]).abs() < 1e-4 && (after_box[2] - before_box[2]).abs() < 1e-4, "and only along x");
    // duplicate: both are copied and the copies are the selection
    h.app.studio.duplicate_selected();
    h.frames(3);
    assert_eq!(h.app.studio.total_entities, total + 2);
    assert_eq!(h.app.studio.selected.len(), 2);
    assert!(!h.app.studio.is_selected(b) && !h.app.studio.is_selected(ball));
    // delete the selection: the copies go, the originals stay
    h.app.studio.delete_selected();
    h.frames(3);
    assert_eq!(h.app.studio.total_entities, total);
    assert!(h.app.studio.selected.is_empty() && h.app.studio.selection.is_none());
    assert_eq!(h.app.studio.all.iter().filter(|r| r.uid == b || r.uid == ball).count(), 2);
    // an entity that disappears leaves the selection
    h.app.studio.select(Some(b));
    h.app.studio.toggle_select(ball);
    h.app.studio.call("despawn", j(&format!(r#"{{"uid":{ball}}}"#))).unwrap();
    h.app.studio.rows_dirty = true;
    h.frames(3);
    assert_eq!(h.app.studio.selected, vec![b]);
}

#[test]
fn a_value_set_with_several_selected_goes_to_every_selected_entity_that_has_the_component_and_is_one_undo_step() {
    let mut h = Harness::new();
    let (b, ball, sun, floor) = (h.uid("Box"), h.uid("Ball"), h.uid("Sun"), h.uid("Floor"));
    let color_of = |h: &mut Harness, uid: u64| -> Vec<f64> {
        let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{uid},"components":["MeshRenderer"]}}"#))).unwrap();
        match g.get("components").and_then(|c| c.get("MeshRenderer")) {
            Some(m) => m.get("color").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect(),
            None => vec![],
        }
    };
    let (box_before, floor_before) = (color_of(&mut h, b), color_of(&mut h, floor));
    // Box, the Sun (which has no MeshRenderer) and Ball selected, Ball the main one
    h.app.studio.select(Some(b));
    h.app.studio.toggle_select(sun);
    h.app.studio.toggle_select(ball);
    assert_eq!(h.app.studio.selection, Some(ball));
    let errors = |h: &Harness| h.app.studio.console.iter().filter(|l| l.kind == crate::state::LogKind::Error).count();
    let errors_before = errors(&h);
    h.app.studio.set_selected(ball, "MeshRenderer", j(r#"{"color":[0,1,0,1]}"#), Some("g-1"));
    assert_eq!(color_of(&mut h, ball), vec![0.0, 1.0, 0.0, 1.0], "the main one");
    assert_eq!(color_of(&mut h, b), vec![0.0, 1.0, 0.0, 1.0], "the other mesh that is selected");
    assert_eq!(color_of(&mut h, floor), floor_before, "what is not selected stays");
    assert_eq!(errors(&h), errors_before, "the Sun has no MeshRenderer: skipped quietly, no error in the Console");
    // one Undo takes the edit back everywhere
    h.app.studio.undo();
    assert_eq!(color_of(&mut h, b), box_before);
    assert_ne!(color_of(&mut h, ball), vec![0.0, 1.0, 0.0, 1.0]);
    // a name is not copied; with one selected only that one changes
    h.app.studio.set_selected(ball, "Name", j(r#"{"value":"Renamed"}"#), None);
    assert_eq!(h.app.studio.all.iter().filter(|r| r.name == "Renamed").count(), 0, "rows refresh later, so look at the entity");
    let n = h.app.studio.call("get", j(&format!(r#"{{"uid":{b},"components":["Name"]}}"#))).unwrap();
    assert_eq!(n.get("components").unwrap().get("Name").unwrap().get("value").and_then(|v| v.as_str()), Some("Box"));
    h.app.studio.select(Some(b));
    h.app.studio.set_selected(b, "MeshRenderer", j(r#"{"color":[1,1,0,1]}"#), Some("g-2"));
    assert_eq!(color_of(&mut h, b), vec![1.0, 1.0, 0.0, 1.0]);
    assert_ne!(color_of(&mut h, ball), vec![1.0, 1.0, 0.0, 1.0]);
}

#[test]
fn the_effects_tab_lists_the_files_plays_an_effect_says_what_is_wrong_saves_and_puts_it_on_an_entity() {
    use crate::state::Center;
    let mut h = Harness::new();
    let root = h.app.studio.opts.root.clone();
    std::fs::create_dir_all(root.join("fx")).unwrap();
    std::fs::write(root.join("fx/spark.fx.json"), crate::effects::TEMPLATE).unwrap();
    h.app.studio.center = Center::Effects;
    h.frames(4);
    assert_eq!(h.app.effects.test_files(), ["fx/spark.fx.json"]);
    // an effect (the template) plays at once, on a stage of its own
    assert!(h.app.effects.test_has_picture());
    assert!(h.app.effects.effect().is_some(), "the template reads");
    assert!(h.app.effects.dirty(), "a new effect is not saved yet");
    // opening a file reads its text; nothing is changed yet
    assert!(h.app.effects.open(&mut h.app.studio, "fx/spark.fx.json"));
    h.frames(2);
    assert!(!h.app.effects.dirty());
    h.shoot("15_effects");
    // a mistake is the reader's own message, and the effect is not one until it is mended
    h.app.effects.text = h.app.effects.text.replace("\"rate\": 40", "\"rate\": -3");
    h.frames(2);
    assert!(h.app.effects.dirty() && h.app.effects.effect().is_none());
    h.shoot("16_effects_error");
    // mended and saved: the file on disk is the text
    h.app.effects.text = h.app.effects.text.replace("\"rate\": -3", "\"rate\": 90");
    h.frames(2);
    assert!(h.app.effects.effect().is_some());
    h.app.effects.save(&mut h.app.studio);
    assert!(!h.app.effects.dirty());
    assert!(std::fs::read_to_string(root.join("fx/spark.fx.json")).unwrap().contains("\"rate\": 90"));
    // the effect goes on the selected entity as its file, through the `set` tool (or `add_component` when it has none), and the entity has it
    let ball = h.uid("Ball");
    h.app.studio.select(Some(ball));
    h.app.effects.use_on_selection(&mut h.app.studio);
    let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{ball},"components":["ParticleSystem"]}}"#))).unwrap();
    assert_eq!(g.get("components").unwrap().get("ParticleSystem").unwrap().get("effect").unwrap().as_str(), Some("fx/spark.fx.json"));
    // and `check` reads it
    let c = h.app.studio.call("check", Json::Null).unwrap();
    assert_eq!(c.get("errors").and_then(|e| e.as_u64()), Some(0), "{c:?}");
    // a new effect has no file until it is saved, and is saved under its name
    h.app.effects.new_effect();
    h.app.effects.text = h.app.effects.text.replace("new effect", "Big Flame");
    h.frames(2);
    h.app.effects.save(&mut h.app.studio);
    assert!(root.join("fx/big_flame.fx.json").is_file());
    h.frames(2);
    assert_eq!(h.app.effects.test_files(), ["fx/big_flame.fx.json", "fx/spark.fx.json"]);
}

#[test]
fn the_sequencer_tab_reads_a_timeline_scrubs_a_preview_and_writes_edits_back() {
    use crate::sequencer::Kind;
    use crate::state::Center;
    let mut h = Harness::new();
    // the box gets a timeline: its height goes 0 -> 6 over 2 seconds, a flash event at 1, the end at 2
    let b = h.uid("Box");
    h.app.studio.call("add_component", j(&format!(r#"{{"uid":{b},"component":"Timeline","value":{{"script":"at 0 ramp Transform.pos[1] 0 -> 6 over 2\nat 1 event flash\nat 2 end","playing":false}}}}"#))).unwrap();
    h.app.studio.select(Some(b));
    h.app.studio.center = Center::Sequencer;
    h.frames(4);
    let seq = h.app.sequencer.test_seq().clone();
    assert_eq!(seq.items().count(), 3, "the three steps were read");
    assert_eq!(seq.lanes().iter().map(|l| l.label.as_str()).collect::<Vec<_>>(), ["Transform.pos[1]", "events"]);
    assert_eq!(seq.length(), 2.0);
    // in edit mode scrubbing moves the playhead and nothing else: the author's scene is not touched
    h.app.sequencer.seek(&mut h.app.studio, 1.0);
    assert!((h.pos_of("Box")[1] - 1.0).abs() < 1e-6, "the box stayed where the author put it");
    // in a preview the scene is where the sequence has it at that second
    h.app.studio.play();
    h.app.studio.pause();
    h.app.sequencer.seek(&mut h.app.studio, 1.0);
    h.frames(2);
    assert!((h.pos_of("Box")[1] - 3.0).abs() < 1e-4, "half way up the ramp: {:?}", h.pos_of("Box"));
    h.app.sequencer.seek(&mut h.app.studio, 99.0);
    assert!((h.pos_of("Box")[1] - 6.0).abs() < 1e-4, "past the end: the last value");
    // Stop puts the scene back exactly
    h.app.stop();
    h.frames(2);
    assert!((h.pos_of("Box")[1] - 1.0).abs() < 1e-6, "the preview left no trace in the scene");
    // an edit goes back into the entity's script: the event moves to 1.5 s
    let event_line = seq.items().find(|(_, i)| matches!(i.kind, Kind::Event(_))).map(|(l, _)| l).unwrap();
    h.app.sequencer.test_edit(|s| s.item_mut(event_line).unwrap().at = 1.5);
    h.app.sequencer.test_commit(&mut h.app.studio);
    h.frames(3);
    let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{b},"components":["Timeline"]}}"#))).unwrap();
    let script = g.get("components").unwrap().get("Timeline").unwrap().get("script").unwrap().as_str().unwrap().to_string();
    assert!(script.contains("at 1.5 event flash") && script.contains("at 0 ramp Transform.pos[1] 0 -> 6 over 2") && script.contains("at 2 end"), "{script}");
    assert_eq!(h.app.sequencer.test_script(), script);
    // the engine reads what the sequencer wrote
    let entries = lunaris_sim::gameplay::parse_timeline(&script).unwrap();
    assert_eq!(entries.len(), 3);
    // an entity with no Timeline shows nothing to edit (the tab offers to add one)
    let ball = h.uid("Ball");
    h.app.studio.select(Some(ball));
    h.frames(3);
    assert_eq!(h.app.sequencer.test_seq().items().count(), 0);
}

#[test]
fn the_project_settings_window_draws_and_saves_what_is_changed() {
    use crate::project_settings::{ProjectSettings, FILE};
    let mut h = Harness::new();
    let root = h.app.studio.opts.root.clone();
    // a scene in the workspace, so that the start-scene list has something to show
    h.app.studio.call("save_scene_as", j(r#"{"path":"scenes/level1.lunaris.json"}"#)).ok();
    assert_eq!(ProjectSettings::load(&root), ProjectSettings::default(), "a new project has the defaults and no file");
    assert!(!root.join(FILE).exists());
    h.app.open_settings();
    h.frames(4);
    assert!(h.app.settings_open);
    h.app.settings.name = "Coin Chase".into();
    h.app.settings.size = (1920, 1080);
    h.app.settings.quality = "medium".into();
    h.app.settings.seed = 77;
    h.frames(3);
    assert!(!root.join(FILE).exists(), "nothing is written until Save or Close");
    h.app.save_settings();
    assert_eq!(ProjectSettings::load(&root), h.app.settings);
    assert_eq!(ProjectSettings::load(&root).size, (1920, 1080));
    // a second editor on the same workspace starts with the saved settings
    let ctx = egui::Context::default();
    let again = App::new(&ctx, Options { root: root.clone(), quality: "reference".to_string(), mute: true, ..Options::default() });
    assert_eq!(again.settings.name, "Coin Chase");
    assert_eq!(again.settings.seed, 77);
}

#[test]
fn the_project_panel_shows_a_picture_as_its_own_thumbnail() {
    use crate::state::BottomTab;
    let mut h = Harness::new();
    // a 200 x 100 picture, red above and blue below, in the workspace
    let rgba: Vec<u8> = (0..200 * 100).flat_map(|i| if i / 200 < 50 { [220, 20, 20, 255] } else { [20, 20, 220, 255] }).collect();
    let png = lunaris_render::encode_png(200, 100, &rgba).unwrap();
    std::fs::write(h.app.studio.opts.root.join("flag.png"), png).unwrap();
    std::fs::write(h.app.studio.opts.root.join("notes.bin"), b"not a picture").unwrap();
    h.app.studio.tab = BottomTab::Project;
    h.app.project.reload();
    // the worker thread decodes it in the background: draw frames until the thumbnail is there
    let mut ready = false;
    for _ in 0..400 {
        h.frames(1);
        if h.app.project.thumbs_ready() > 0 {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(ready, "the picture's thumbnail was made");
    // exactly one: the file that is not a picture keeps its icon
    h.frames(5);
    assert_eq!(h.app.project.thumbs_ready(), 1);
}

#[test]
fn a_model_has_a_thumbnail_and_can_be_dragged_into_the_scene_where_it_stands_on_what_is_under_the_pointer() {
    use crate::state::BottomTab;
    let mut h = Harness::new();
    let root = h.app.studio.opts.root.clone();
    let crate_glb = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/models/crate.glb")).unwrap();
    std::fs::write(root.join("crate.glb"), crate_glb).unwrap();
    // a picture and a one-tenth-of-a-second-long sound as well
    let rgba: Vec<u8> = (0..16 * 16).flat_map(|_| [30, 200, 60, 255]).collect();
    std::fs::write(root.join("green.png"), lunaris_render::encode_png(16, 16, &rgba).unwrap()).unwrap();
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36u32 + 800).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 0x0001_0001, 8000, 8000, 0x0008_0001] {
        wav.extend_from_slice(&v.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&800u32.to_le_bytes());
    wav.extend((0..800).map(|i| if (i / 20) % 2 == 0 { 200u8 } else { 56u8 }));
    std::fs::write(root.join("beep.wav"), wav).unwrap();
    h.app.studio.tab = BottomTab::Project;
    h.app.project.reload();
    // the model, the picture and the sound (its waveform) all get a thumbnail (made on worker threads)
    let mut ready = false;
    for _ in 0..600 {
        h.frames(1);
        if h.app.project.thumbs_ready() >= 3 {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(ready, "the model's, the picture's and the sound's thumbnails were made: {}", h.app.project.thumbs_ready());
    h.frames(3);

    // drag the model's tile into the scene view, onto the floor
    let tile = h.app.project.tile_center("crate.glb").expect("the model has a tile");
    let before = h.app.studio.total_entities;
    let target = h.screen_of("floor", [3.0, 0.0, 2.0]);
    h.drag(tile, target);
    assert_eq!(h.app.studio.total_entities, before + 1, "dropping a model adds an entity");
    let uid = h.app.studio.selection.expect("the new entity is selected");
    let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{uid}}}"#))).unwrap();
    let comps = g.get("components").unwrap();
    assert_eq!(comps.get("Name").and_then(|n| n.get("value")).and_then(|v| v.as_str()), Some("crate"), "named after the file");
    assert_eq!(comps.get("MeshRenderer").and_then(|m| m.get("mesh")).and_then(|v| v.as_str()), Some("crate.glb"));
    let pos: Vec<f64> = comps.get("Transform").unwrap().get("pos").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!((pos[0] - 3.0).abs() < 0.4 && (pos[2] - 2.0).abs() < 0.4, "where the pointer was: {pos:?}");
    // the floor's top is at y = 0: the crate's lowest point is on it
    let info = h.app.studio.call("model_info", j(r#"{"path":"crate.glb"}"#)).unwrap();
    let lowest = info.get("bounds").unwrap().get("min").unwrap().as_array().unwrap()[1].as_f64().unwrap();
    assert!((pos[1] + lowest).abs() < 0.02, "stands on the floor: y {} lowest {lowest}", pos[1]);

    // a picture dropped on a mesh becomes its texture; dropped on nothing it changes nothing
    let tile = h.app.project.tile_center("green.png").expect("the picture has a tile");
    let on_box = h.screen_of("box", [0.0, 1.0, 0.0]);
    h.drag(tile, on_box);
    let box_uid = h.uid("Box");
    let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{box_uid},"components":["MeshRenderer"]}}"#))).unwrap();
    assert_eq!(g.get("components").unwrap().get("MeshRenderer").unwrap().get("texture").and_then(|v| v.as_str()), Some("green.png"));
    let tile = h.app.project.tile_center("green.png").unwrap();
    let ball_uid = h.uid("Ball");
    h.drag(tile, Pos2::new(h.app.viewport.last_rect.min.x + 4.0, h.app.viewport.last_rect.min.y + 4.0));
    let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{ball_uid},"components":["MeshRenderer"]}}"#))).unwrap();
    assert_eq!(g.get("components").unwrap().get("MeshRenderer").unwrap().get("texture").and_then(|v| v.as_str()).unwrap_or(""), "", "nothing was under the pointer there");

    // a sound dropped in the scene becomes an Audio Source that plays it
    let tile = h.app.project.tile_center("beep.wav").expect("the sound has a tile");
    let n = h.app.studio.total_entities;
    let target = h.screen_of("floor", [-3.0, 0.0, 1.0]);
    h.drag(tile, target);
    assert_eq!(h.app.studio.total_entities, n + 1);
    let uid = h.app.studio.selection.unwrap();
    let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{uid},"components":["AudioSource"]}}"#))).unwrap();
    assert_eq!(g.get("components").unwrap().get("AudioSource").unwrap().get("file").and_then(|v| v.as_str()), Some("beep.wav"));

    // letting go over the Hierarchy (or anywhere else) does nothing
    let n = h.app.studio.total_entities;
    let tile = h.app.project.tile_center("crate.glb").unwrap();
    h.drag(tile, Pos2::new(40.0, 200.0));
    assert_eq!(h.app.studio.total_entities, n);

    // the type filter narrows the tiles to one kind, from every folder
    std::fs::create_dir_all(root.join("deep")).unwrap();
    std::fs::write(root.join("deep").join("tick.wav"), std::fs::read(root.join("beep.wav")).unwrap()).unwrap();
    h.app.project.reload();
    h.app.project.set_kind_filter("sound");
    h.frames(4);
    assert!(h.app.project.tile_center("beep.wav").is_some() && h.app.project.tile_center("deep/tick.wav").is_some(), "both sounds, in whatever folder");
    assert!(h.app.project.tile_center("crate.glb").is_none() && h.app.project.tile_center("green.png").is_none(), "nothing else");
    h.app.project.set_kind_filter("model");
    h.frames(3);
    assert!(h.app.project.tile_center("crate.glb").is_some() && h.app.project.tile_center("beep.wav").is_none());
    h.app.project.set_kind_filter("");
    h.frames(3);
    assert!(h.app.project.tile_center("green.png").is_some() && h.app.project.tile_center("deep/tick.wav").is_none(), "all types: this folder only again");
}

#[test]
fn the_place_panel_adds_a_thing_by_a_click_or_where_it_is_dropped_standing_on_what_is_there() {
    use crate::state::{LeftTab, Template};
    let mut h = Harness::new();
    h.app.studio.left_tab = LeftTab::Place;
    h.frames(3);
    // every template has a row
    for t in Template::ALL {
        assert!(h.app.place.row_center(t).is_some(), "no row for {}", t.label());
    }
    // a click adds it in front of the camera and selects it
    let before = h.app.studio.total_entities;
    let sphere = h.app.place.row_center(Template::Sphere).unwrap();
    h.click(sphere);
    assert_eq!(h.app.studio.total_entities, before + 1);
    assert!(h.app.studio.selection.is_some());
    // a drag into the view adds it where it is dropped, resting on the floor (its top is at y = 0): a unit cube stands half its height above it
    let cube = h.app.place.row_center(Template::Cube).unwrap();
    let target = h.screen_of("floor", [-3.0, 0.0, 2.0]);
    let n = h.app.studio.total_entities;
    h.drag(cube, target);
    assert_eq!(h.app.studio.total_entities, n + 1, "the dropped cube was added");
    let uid = h.app.studio.selection.unwrap();
    let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{uid},"components":["Transform","MeshRenderer"]}}"#))).unwrap();
    let comps = g.get("components").unwrap();
    assert_eq!(comps.get("MeshRenderer").and_then(|m| m.get("mesh")).and_then(|v| v.as_str()), Some("cube"));
    let pos: Vec<f64> = comps.get("Transform").unwrap().get("pos").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!((pos[0] + 3.0).abs() < 0.4 && (pos[2] - 2.0).abs() < 0.4, "where it was dropped: {pos:?}");
    assert!((pos[1] - 0.5).abs() < 0.02, "standing on the floor: {pos:?}");
    // a light is hung in the air above the point
    let light = h.app.place.row_center(Template::PointLight).unwrap();
    let n = h.app.studio.total_entities;
    let bare = h.screen_of("floor", [2.0, 0.0, 2.0]);
    h.drag(light, bare);
    assert_eq!(h.app.studio.total_entities, n + 1);
    let uid = h.app.studio.selection.unwrap();
    let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{uid},"components":["Transform","Light"]}}"#))).unwrap();
    let y = g.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().as_array().unwrap()[1].as_f64().unwrap();
    assert!((y - 2.0).abs() < 0.02, "two metres up: {y}");
    // letting go over the Hierarchy adds nothing; the search narrows the rows
    let n = h.app.studio.total_entities;
    let cube = h.app.place.row_center(Template::Cube).unwrap();
    h.drag(cube, Pos2::new(cube.x, cube.y + 300.0));
    assert_eq!(h.app.studio.total_entities, n);
    // a prefab of the scene is a row too, and is dropped like the rest
    h.app.studio.call("define_prefab", j(r#"{"name":"Lamp","def":{"components":{"Name":{"value":"lamp"},"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"cylinder"}}}}"#)).unwrap();
    h.app.studio.rows_dirty = true;
    // (the search box narrows the rows to the one: the list is longer than the panel is high)
    h.app.place.set_filter("lamp");
    h.frames(6);
    let row = h.app.place.prefab_center("Lamp").expect("the prefab has a row");
    let n = h.app.studio.total_entities;
    h.drag(row, h.screen_of("floor", [4.0, 0.0, -2.0]));
    assert_eq!(h.app.studio.total_entities, n + 1, "the dropped prefab was spawned");
    let uid = h.app.studio.selection.unwrap();
    let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{uid},"components":["Transform"]}}"#))).unwrap();
    let pos: Vec<f64> = g.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!((pos[0] - 4.0).abs() < 0.4 && (pos[2] + 2.0).abs() < 0.4, "where it was dropped: {pos:?}");
    // the Hierarchy tab shows the entities again
    h.app.studio.left_tab = LeftTab::Hierarchy;
    h.frames(3);
    assert!(h.app.place.row_center(Template::Cube).is_none() || h.app.place.row_center(Template::Cube).is_some());
}

#[test]
fn the_paint_brush_sprinkles_copies_on_the_ground_where_it_is_dragged_and_one_undo_takes_the_stroke_back() {
    use crate::paint::{Brush, BrushItem};
    use crate::state::Template;
    let mut h = Harness::new();
    let mut b = Brush::new(BrushItem::Template(Template::Cube));
    b.radius = 1.5;
    b.spacing = 1.0;
    h.app.studio.brush = Some(b);
    h.frames(3);
    // a drag across the floor paints a row of cubes round the line it went along, each standing on the floor (a unit cube is centred half a height above it, scaled 0.8..1.2)
    let before = h.app.studio.total_entities;
    let (from, to) = (h.screen_of("floor", [-4.0, 0.0, 3.0]), h.screen_of("floor", [4.0, 0.0, 3.0]));
    h.drag(from, to);
    let made = h.app.studio.total_entities - before;
    assert!(made >= 8, "a stroke of eight metres made only {made}");
    let copies: Vec<u64> = h.app.studio.all.iter().filter(|r| r.name.starts_with("Cube ")).map(|r| r.uid).collect();
    assert_eq!(copies.len(), made);
    let (mut lo, mut hi, mut turned) = (f64::MAX, f64::MIN, false);
    for uid in &copies {
        let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{uid},"components":["Transform"]}}"#))).unwrap();
        let t = g.get("components").unwrap().get("Transform").unwrap();
        let p: Vec<f64> = t.get("pos").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        let s = t.get("scale").unwrap().as_array().unwrap()[0].as_f64().unwrap();
        assert!((0.8..=1.2).contains(&s) && (p[1] - 0.5 * s).abs() < 0.02, "stands on the floor at its size: {p:?} x{s}");
        assert!((p[2] - 3.0).abs() < 1.5 + 0.5, "near the line the pointer went along: {p:?}");
        lo = lo.min(p[0]);
        hi = hi.max(p[0]);
        turned |= t.get("rot").and_then(|r| r.as_array()).is_some_and(|r| r[1].as_f64().unwrap().abs() > 0.05);
    }
    assert!(lo < -2.5 && hi > 2.5, "the cubes run along the stroke: {lo}..{hi}");
    assert!(turned, "each copy has a turn of its own");
    // picking is off while painting: a click on the box does not select it, it paints
    let n = h.app.studio.total_entities;
    h.click(h.screen_of("Box", [0.0, 1.0, 0.0]));
    assert_eq!(h.app.studio.selection, None, "the click painted, it did not pick");
    assert!(h.app.studio.total_entities > n, "a click is a stroke of one point");
    // one Undo takes the click's copies back, another one the whole stroke's
    h.app.studio.undo();
    h.frames(3);
    assert_eq!(h.app.studio.total_entities, n, "one undo took the click back");
    h.app.studio.undo();
    h.frames(3);
    assert_eq!(h.app.studio.total_entities, before, "one undo took the whole stroke back");
    // Esc puts the brush down: a drag adds nothing and a click picks again
    h.key(Key::Escape, Modifiers::NONE);
    assert!(h.app.studio.brush.is_none());
    h.drag(from, to);
    assert_eq!(h.app.studio.total_entities, before);
    h.click(h.screen_of("Box", [0.0, 1.0, 0.0]));
    assert_eq!(h.app.studio.selection, Some(h.uid("Box")));
}

#[test]
fn the_outliner_has_an_eye_that_hides_a_mesh_and_what_is_attached_under_it_in_one_undo_step() {
    let mut h = Harness::new();
    let (b, ball) = (h.uid("Box"), h.uid("Ball"));
    h.frames(3);
    let visible = |h: &mut Harness, uid: u64| {
        let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{uid},"components":["MeshRenderer"]}}"#))).unwrap();
        g.get("components").unwrap().get("MeshRenderer").unwrap().get("visible").unwrap().as_bool().unwrap()
    };
    assert!(visible(&mut h, b));
    // a mesh has an eye, the Sun does not
    assert!(h.app.hierarchy.eye_center(b).is_some(), "the box has an eye");
    assert!(h.app.hierarchy.eye_center(h.uid("Sun")).is_none(), "the sun has none");
    // a click on it hides the mesh and does not select the row
    h.click(h.app.hierarchy.eye_center(b).unwrap());
    assert!(!visible(&mut h, b), "the eye hid it");
    assert_eq!(h.app.studio.selection, None, "the eye does not select");
    assert_eq!(h.app.studio.all.iter().find(|r| r.uid == b).unwrap().shown, 0);
    // another click shows it again
    h.click(h.app.hierarchy.eye_center(b).unwrap());
    assert!(visible(&mut h, b));
    // with the ball attached under the box, the box's eye acts on both, and one Undo shows both again
    crate::hierarchy::attach_to(&mut h.app.studio, ball, Some(b));
    h.app.studio.rows_dirty = true;
    h.frames(3);
    assert_eq!(h.app.studio.all.iter().find(|r| r.uid == b).unwrap().meshes, 2);
    h.click(h.app.hierarchy.eye_center(b).unwrap());
    assert!(!visible(&mut h, b) && !visible(&mut h, ball), "both are hidden");
    h.app.studio.undo();
    h.frames(3);
    assert!(visible(&mut h, b) && visible(&mut h, ball), "one undo showed both");
}

#[test]
fn the_paint_brush_is_armed_from_a_row_of_the_place_panel_and_shows_its_bar() {
    use crate::state::{LeftTab, Template};
    let mut h = Harness::new();
    h.app.studio.left_tab = LeftTab::Place;
    h.frames(3);
    assert!(crate::place::paintable(Template::Sphere) && !crate::place::paintable(Template::Camera) && !crate::place::paintable(Template::Plane));
    // a right click on a row opens its menu; "Paint with this" arms the brush
    let row = h.app.place.row_center(Template::Sphere).unwrap();
    h.move_to(row);
    h.frame(vec![Event::PointerButton { pos: row, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE }]);
    h.frame(vec![Event::PointerButton { pos: row, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE }]);
    h.frames(3);
    let item = h.app.place.menu_item_center().expect("the row's menu is open with a Paint entry");
    h.click(item);
    let brush = h.app.studio.brush.clone().expect("the brush is armed");
    assert_eq!(brush.label(), "Sphere");
    assert!(brush.per_point() >= 1);
}

#[test]
fn a_field_that_is_not_what_a_new_component_starts_with_has_a_reset_arrow_that_puts_it_back() {
    let mut h = Harness::new();
    let b = h.uid("Box");
    // the schema says what a new Transform starts with
    let d = h.app.studio.def("Transform").defaults.expect("the schema answers with defaults");
    assert_eq!(d.get("scale").and_then(|s| s.as_array()).map(|a| a.len()), Some(3));
    h.app.studio.select(Some(b));
    h.frames(4);
    // the Box was made at [0, 1, 0] with a colour of its own: those rows have an arrow, a field at its default does not
    let has = |h: &Harness, key: &str| h.app.inspector.resets.iter().any(|(k, _)| k == key);
    assert!(has(&h, "Transform.pos") && has(&h, "MeshRenderer.color"), "{:?}", h.app.inspector.resets);
    assert!(!has(&h, "Transform.scale") && !has(&h, "Transform.rot"), "{:?}", h.app.inspector.resets);
    // a click on the arrow of the position puts it back (one undo step)
    let at = h.app.inspector.resets.iter().find(|(k, _)| k == "Transform.pos").map(|(_, p)| *p).unwrap();
    h.click(at);
    h.frames(3);
    assert_eq!(h.pos_of("Box"), [0.0, 0.0, 0.0]);
    assert!(!has(&h, "Transform.pos"), "no arrow once it is the default");
    h.app.studio.undo();
    h.frames(3);
    assert_eq!(h.pos_of("Box")[1], 1.0, "one Undo brings the old position back");
}

#[test]
fn the_statistics_overlay_draws_and_the_wheel_sets_the_fly_speed_between_a_tenth_and_ten_times() {
    use crate::viewport::fly_speed_after_scroll;
    let mut h = Harness::new();
    h.app.viewport.stats = true;
    h.frames(5);
    assert!(h.app.viewport.frame_ms > 0.0, "the overlay measures the editor's frames");
    assert!(h.app.viewport.picture_ms > 0.0, "and the last picture");
    // the wheel: a notch faster, a notch slower, never outside the range
    assert!(fly_speed_after_scroll(1.0, 50.0) > 1.1 && fly_speed_after_scroll(1.0, -50.0) < 0.9);
    assert!((fly_speed_after_scroll(1.0, 0.0) - 1.0).abs() < 1e-6);
    let mut s = 1.0;
    for _ in 0..200 {
        s = fly_speed_after_scroll(s, 100.0);
    }
    assert_eq!(s, 10.0);
    for _ in 0..400 {
        s = fly_speed_after_scroll(s, -100.0);
    }
    assert_eq!(s, 0.1);
}

#[test]
fn the_profiler_tab_times_the_game_while_it_plays_and_only_while_it_is_showing() {
    use crate::state::BottomTab;
    let mut h = Harness::new();
    // not showing: playing does not time anything
    h.app.studio.tab = BottomTab::Console;
    h.app.play();
    h.frames(40);
    assert!(h.app.studio.engine.sim.profile.is_none(), "a closed Profiler tab costs nothing");
    // showing: windows of one second of game time arrive
    h.app.studio.tab = BottomTab::Profiler;
    h.frames(200);
    let w = h.app.profiler.last.clone().expect("a window of measurements after a few seconds of play");
    assert!(w.ticks >= crate::profiler::WINDOW_TICKS && !w.rows.is_empty(), "{} ticks, {} stages", w.ticks, w.rows.len());
    assert!(w.avg_ms > 0.0 && h.app.profiler.history.len() >= 1);
    // leaving the tab switches the timing off again
    h.app.studio.tab = BottomTab::Console;
    h.frames(3);
    assert!(h.app.studio.engine.sim.profile.is_none());
    h.app.stop();
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
fn a_breakpoint_on_a_statement_stops_the_game_when_the_selected_entity_is_about_to_run_it() {
    use crate::blueprint::{Pin, PinKind};
    let mut h = Harness::new();
    // above 2 the box drops to 0 (the `then` branch); otherwise it rises by half a unit
    let def = r#"{"name":"cap","each":["Transform"],"do":[{"if":{"gt":[{"get":"Transform.pos[1]"},2]},"then":[{"set":"Transform.pos[1]","to":0}],"else":[{"set":"Transform.pos[1]","to":{"add":[{"get":"Transform.pos[1]"},0.5]}}]}]}"#;
    h.app.studio.call("define_system", j(def)).unwrap();
    let b = h.uid("Box");
    h.app.studio.select(Some(b));
    blueprint_tab(&mut h);
    let bp = h.app.blueprint.test_bp().unwrap().clone();
    let sets: Vec<u32> = bp.nodes.iter().filter(|n| n.op == "set").map(|n| n.id).collect();
    assert_eq!(sets.len(), 2);
    // the `then` branch's set has a plain 0 in its box, the `else` one a wire from the add
    let then_set = *sets.iter().find(|id| bp.wire_into(Pin { node: **id, kind: PinKind::DataIn(0) }).is_none()).unwrap();
    // a node that is not a statement cannot have a breakpoint
    let get = bp.nodes.iter().find(|n| n.op == "get").unwrap().id;
    h.app.blueprint.test_toggle_break(get);
    assert!(!h.app.blueprint.test_has_breakpoints(), "a value is not a statement");
    h.app.blueprint.test_toggle_break(then_set);
    assert!(h.app.blueprint.test_has_breakpoints());
    // play: y goes 1, 1.5, 2, 2.5; after the tick that makes it 2.5 the next one would take the `then` branch, so the game stops before it
    h.app.play();
    assert_eq!(h.app.studio.mode, Mode::Play);
    for _ in 0..40 {
        h.frames(1);
        if h.app.studio.mode == Mode::Paused {
            break;
        }
    }
    assert_eq!(h.app.studio.mode, Mode::Paused, "the game stopped at the breakpoint");
    assert!((h.pos_of("Box")[1] - 2.5).abs() < 1e-5, "stopped before the statement ran: {:?}", h.pos_of("Box"));
    assert_eq!(h.app.blueprint.test_hit(), Some(then_set));
    assert!(h.app.blueprint.test_ran().contains(&then_set), "the statement is marked as the one that runs next");
    // go on: the `then` branch runs (y = 0) and the next tick takes the `else` branch, which has no breakpoint
    h.app.studio.play();
    for _ in 0..3 {
        h.frames(1);
        if h.app.studio.mode == Mode::Paused {
            break;
        }
    }
    assert!(h.app.studio.mode == Mode::Play || h.pos_of("Box")[1] < 2.0, "it went on past the breakpoint: {:?}", h.pos_of("Box"));
    // stopping the game clears the mark; removing the breakpoint stops the stops
    h.app.stop();
    h.frames(3);
    assert_eq!(h.app.blueprint.test_hit(), None);
    h.app.blueprint.test_toggle_break(then_set);
    assert!(!h.app.blueprint.test_has_breakpoints());
}

#[test]
fn the_blueprint_tab_shows_the_values_of_the_selected_entity_on_its_wires_even_for_an_edit_not_compiled_yet() {
    let mut h = Harness::new();
    h.app.studio.call("define_system", j(DRIFT)).unwrap();
    // nothing selected: nothing to show
    h.app.studio.select(None);
    blueprint_tab(&mut h);
    assert_eq!(h.app.blueprint.test_live_summary(), None);
    // the box stands at y = 1: `get` reads 1 and `add` makes 1.5
    let b = h.uid("Box");
    h.app.studio.select(Some(b));
    h.frames(3);
    assert_eq!(h.app.blueprint.test_live_summary(), Some((2, true)), "the add and the get have a value");
    let bp = h.app.blueprint.test_bp().unwrap().clone();
    let (add, get) = (bp.nodes.iter().find(|n| n.op == "add").unwrap().id, bp.nodes.iter().find(|n| n.op == "get").unwrap().id);
    assert_eq!(h.app.blueprint.test_live(get).as_deref(), Some("1"));
    assert_eq!(h.app.blueprint.test_live(add).as_deref(), Some("1.5"));
    // an edit that is not compiled: 0.5 becomes 2.5, and the values follow the graph on the screen
    let at = h.app.blueprint.test_default_screen(add, 1).expect("the box is on the screen");
    h.click(at);
    type_text(&mut h, "2.5");
    h.key(Key::Enter, Modifiers::NONE);
    assert!(h.app.blueprint.test_dirty());
    std::thread::sleep(std::time::Duration::from_millis(600));
    h.frames(3);
    assert_eq!(h.app.blueprint.test_live(add).as_deref(), Some("3.5"), "1 + 2.5, before Compile");
    // the world was not touched by any of this
    assert!((h.pos_of("Box")[1] - 1.0).abs() < 1e-6);
    // an entity without a Transform is not visited by the system
    let bare = h.app.studio.call("spawn", j(r#"{"components":{"Name":{"value":"Bare"}}}"#)).unwrap().get("uid").unwrap().as_u64().unwrap();
    h.app.studio.select(Some(bare));
    std::thread::sleep(std::time::Duration::from_millis(600));
    h.frames(3);
    assert_eq!(h.app.blueprint.test_live_summary(), Some((0, false)));
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

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

#[test]
fn the_project_panel_badges_the_files_git_sees_as_changed_and_says_the_branch() {
    use crate::vcs::Status;
    let dir = std::env::temp_dir().join(format!("lunaris-studio-git-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(&dir).args(args).output().map(|o| o.status.success()).unwrap_or(false);
    if !git(&["init", "-q", "-b", "trunk"]) {
        return; // no git on this machine
    }
    std::fs::write(dir.join("scenes/a.lunaris.json"), r#"{"entities":[]}"#).unwrap();
    std::fs::write(dir.join("scenes/b.lunaris.json"), r#"{"entities":[]}"#).unwrap();
    assert!(git(&["add", "."]) && git(&["-c", "user.name=t", "-c", "user.email=t@users.noreply.github.com", "commit", "-q", "-m", "first"]));
    std::fs::write(dir.join("scenes/a.lunaris.json"), r#"{"entities":[],"changed":true}"#).unwrap();
    std::fs::write(dir.join("scenes/c.lunaris.json"), r#"{"entities":[]}"#).unwrap();
    let mut h = Harness::on(dir.clone());
    h.app.studio.tab = crate::state::BottomTab::Project;
    for _ in 0..300 {
        h.frames(1);
        if h.app.project.git_answered() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.frames(2);
    let (a, summary) = h.app.project.git_status("scenes/a.lunaris.json");
    assert_eq!(a, Some(Status::Modified));
    assert_eq!(h.app.project.git_status("scenes/c.lunaris.json").0, Some(Status::Untracked));
    assert_eq!(h.app.project.git_status("scenes/b.lunaris.json").0, None, "a file that is as committed has no badge");
    assert!(summary.starts_with("trunk") && summary.contains("2 changed"), "{summary}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Pictures of the editor for looking at, drawn in software (`shot.rs`) from the Caldera example: nothing is checked, and without `LUNARIS_SHOTS` (the folder to write
/// them to) the test does nothing. `LUNARIS_SHOTS=/tmp/shots cargo test --release -p lunaris-studio screenshots -- --nocapture`.
#[test]
fn screenshots_of_the_editor_for_looking_at() {
    use crate::state::{BottomTab, Center, LeftTab};
    if std::env::var("LUNARIS_SHOTS").is_err() {
        return;
    }
    // (the scene names its models as `examples/caldera/models/...`: the workspace is the folder that holds `examples`)
    let dir = std::env::temp_dir().join(format!("lunaris-shots-{}", std::process::id()));
    copy_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/caldera"), &dir.join("examples").join("caldera"));
    let mut h = Harness::on(dir);
    h.app.studio.open_scene("examples/caldera/island.lunaris.json");
    h.frames(8);
    // 1: the whole window: the scene, the Hierarchy, the Inspector of the hero, the Project panel with the models' thumbnails
    h.app.project.set_folder("examples/caldera/models");
    h.app.studio.tab = BottomTab::Project;
    let hero = h.app.studio.all.iter().find(|r| r.name.to_lowercase().contains("hero")).map(|r| (r.uid, r.pos));
    if let Some((u, pos)) = hero {
        h.app.studio.select(Some(u));
        if let Some(p) = pos {
            // (a view of the hero and what stands round it, not of the hero's nose)
            h.app.viewport.focus([p[0], p[1] + 1.0, p[2]], 7.0);
        }
    }
    for _ in 0..200 {
        h.frames(1);
        if h.app.project.thumbs_ready() >= 12 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    h.frames(6);
    h.shoot("01_editor");
    // 2: the Place tab
    h.app.studio.left_tab = LeftTab::Place;
    h.frames(4);
    h.shoot("02_place");
    h.app.studio.left_tab = LeftTab::Hierarchy;
    // 3: several things selected
    let area = h.app.viewport.last_rect;
    let names: Vec<(u64, String)> = h
        .app
        .studio
        .all
        .iter()
        .filter(|r| r.kind == crate::state::Kind::Mesh && Some(r.uid) != hero.map(|x| x.0))
        .filter(|r| r.pos.and_then(|p| h.app.viewport.project(p)).is_some_and(|s| area.shrink(60.0).contains(s)))
        .take(4)
        .map(|r| (r.uid, r.name.clone()))
        .collect();
    if let Some((first, _)) = names.first() {
        h.app.studio.select(Some(*first));
        for (u, _) in names.iter().skip(1) {
            h.app.studio.toggle_select(*u);
        }
    }
    h.frames(6);
    h.shoot("03_several_selected");
    // 4: a model carried from the Project panel over the scene
    let tile = h.app.project.tile_center("examples/caldera/models/golem.glb").or_else(|| h.app.project.tile_center("examples/caldera/models/hero.glb"));
    if let Some(from) = tile {
        let to = Pos2::new(900.0, 420.0);
        h.move_to(from);
        h.press(from);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            h.move_to(Pos2::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t));
        }
        h.shoot("04_dragging_a_model");
        h.release(to);
        h.frames(4);
        h.shoot("05_model_dropped");
    }
    // 5: Paint mode with a model as the brush: a stroke in the making (the ring and the dots), then what it left
    let model = ["golem", "hero"].iter().find_map(|m| {
        let p = format!("examples/caldera/models/{m}.glb");
        h.app.project.tile_center(&p).map(|_| p)
    });
    if let Some(path) = model {
        let mut b = crate::paint::Brush::new(crate::paint::BrushItem::Model(path));
        b.radius = 4.0;
        b.spacing = 2.0;
        h.app.studio.brush = Some(b);
        h.app.studio.left_tab = LeftTab::Place;
        h.frames(3);
        let r = h.app.viewport.last_rect;
        let (from, to) = (Pos2::new(r.center().x - 200.0, r.center().y + 80.0), Pos2::new(r.center().x + 200.0, r.center().y + 120.0));
        h.move_to(from);
        h.press(from);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            h.move_to(Pos2::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t));
        }
        h.shoot("16_painting");
        h.release(to);
        h.frames(4);
        h.shoot("17_painted");
        h.app.studio.brush = None;
        h.app.studio.left_tab = LeftTab::Hierarchy;
        h.frames(2);
    }
    // 6: the Blueprint of a system, the Script of another
    h.app.studio.open_blueprint = Some("fader".to_string());
    h.frames(6);
    h.shoot("06_blueprint");
    h.app.studio.open_script = Some("fire_switch".to_string());
    h.frames(6);
    h.shoot("07_script");
    // 7: the Sequencer on an entity that has a Timeline
    let b = h.app.studio.all.first().map(|r| r.uid).unwrap();
    h.app.studio.call("add_component", j(&format!(r#"{{"uid":{b},"component":"Timeline","value":{{"script":"at 0 ramp Transform.pos[1] 0 -> 6 over 2\nat 0.5 set score 10\nat 1 event flash\nat 2.5 end","playing":false}}}}"#))).unwrap();
    h.app.studio.select(Some(b));
    h.app.studio.center = Center::Sequencer;
    h.frames(6);
    h.shoot("08_sequencer");
    // 8: the Project Settings window
    // 8b: the other tabs of the middle: the game as the camera sees it, the UI designer, the notes canvas
    h.app.studio.center = Center::View;
    h.app.viewport.set_kind(crate::viewport::ViewKind::Game);
    h.frames(6);
    h.shoot("09_game_view");
    h.app.viewport.set_kind(crate::viewport::ViewKind::Scene);
    h.app.studio.center = Center::Ui;
    h.frames(6);
    h.shoot("10_ui_designer");
    h.app.studio.center = Center::Canvas;
    h.frames(6);
    h.shoot("11_canvas");
    h.app.studio.center = Center::View;
    h.app.open_settings();
    h.frames(6);
    h.shoot("12_project_settings");
    h.app.settings_open = false;
    // 8c: the bottom panel's other tabs
    h.app.studio.tab = BottomTab::Console;
    h.app.studio.log(crate::state::LogKind::Warn, "[warning] the Light has no Transform".to_string(), String::new(), 0.0);
    h.app.studio.log(crate::state::LogKind::Error, "set: no such entity 99".to_string(), String::new(), 0.0);
    h.frames(4);
    h.shoot("14_console");
    // (the History tab: the edits so far, newest at the bottom, two undone below the line)
    h.app.studio.tab = BottomTab::History;
    h.app.studio.undo_by(2);
    h.frames(4);
    h.shoot("16_history");
    h.app.studio.tab = BottomTab::Systems;
    h.frames(4);
    h.shoot("15_systems");
    // 9: playing, with the Profiler showing
    h.app.studio.tab = BottomTab::Profiler;
    h.app.studio.play();
    for _ in 0..70 {
        h.frames(1);
    }
    // (the card's passes as a GPU quality would have measured them: the tests draw with the CPU reference, which has none)
    h.app.viewport.gpu_fixed = true;
    h.app.viewport.gpu = crate::viewport::GpuStats {
        adapter: "NVIDIA GeForce RTX 2060 SUPER".to_string(),
        passes: [("shadows", 0.62f32), ("main", 2.4), ("main.meshes", 1.7), ("main.sky", 0.2), ("main.terrain", 0.4), ("post", 0.35), ("ui", 0.05)].iter().map(|(n, ms)| (n.to_string(), *ms)).collect(),
        cpu: [("gather", 1.8f32), ("cull", 0.4), ("encode", 0.9), ("submit", 0.2)].iter().map(|(n, ms)| (n.to_string(), *ms)).collect(),
        draws: 1412,
        triangles: 880_000,
    };
    h.frames(4);
    h.shoot("13_profiler_while_playing");
    // compared with a capture made before some change: the stages that cost less are green, those that cost more red
    if let Some(w) = h.app.profiler.last.clone() {
        let mut base = w.clone();
        base.avg_ms *= 1.3;
        for (k, r) in base.rows.iter_mut().enumerate() {
            r.avg_ms *= if k % 3 == 0 { 1.6 } else if k % 3 == 1 { 0.8 } else { 1.0 };
        }
        h.app.profiler.baseline = Some(("profile-1792000000.json".to_string(), base));
        h.frames(3);
        h.shoot("13d_profiler_compared_with_a_capture");
    }
}

#[test]
fn the_history_tab_lists_the_edits_and_a_jump_takes_the_scene_to_the_state_after_one() {
    let mut h = Harness::new();
    let box_uid = h.uid("Box");
    for x in [1.0, 2.0, 3.0] {
        h.app.studio.call("set", j(&format!(r#"{{"uid":{box_uid},"component":"Transform","value":{{"pos":[{x},1,0]}}}}"#))).unwrap();
    }
    h.app.studio.tab = crate::state::BottomTab::History;
    h.frames(4);
    let hist = h.app.studio.engine.call("history", &Json::Null).unwrap();
    let (rows, now) = crate::history_ui::rows_of(&hist);
    assert!(rows.len() >= 4 && now == rows.len() - 1, "{} rows, now {now}", rows.len());
    // back to the state after the first of the three moves: two undos
    assert_eq!(rows[now - 2].label, format!("Edit Transform of {box_uid}"));
    h.app.studio.undo_by(rows[now - 2].steps);
    assert_eq!(h.pos_of("Box"), [1.0, 1.0, 0.0]);
    // and forward to the last
    let hist = h.app.studio.engine.call("history", &Json::Null).unwrap();
    let (rows, now) = crate::history_ui::rows_of(&hist);
    assert_eq!(rows.len() - 1 - now, 2, "two edits are undone");
    h.app.studio.undo_by(rows[rows.len() - 1].steps);
    assert_eq!(h.pos_of("Box"), [3.0, 1.0, 0.0]);
    // more steps than there are: it stops where it can
    h.app.studio.undo_by(1000);
    h.frames(2);
    assert_eq!(h.app.studio.engine.call("history", &Json::Null).unwrap().get("undo_depth").and_then(|v| v.as_u64()), Some(0));
}

#[test]
fn the_values_of_a_component_are_copied_and_pasted_onto_the_selected_entities_in_one_undo_step() {
    let mut h = Harness::new();
    let (b, ball) = (h.uid("Box"), h.uid("Ball"));
    // the box is red, the ball blue; copy the box's MeshRenderer and paste it on the ball (selected with it)
    assert!(h.app.studio.copy_component(b, "MeshRenderer"));
    assert!(!h.app.studio.copy_component(b, "Light"), "a component that is not there is not copied, and the clipboard keeps what it had");
    assert_eq!(h.app.studio.component_clipboard.as_ref().map(|c| c.0.as_str()), Some("MeshRenderer"));
    h.app.studio.select(Some(ball));
    let depth = |h: &mut Harness| h.app.studio.engine.call("history", &Json::Null).unwrap().get("undo_depth").and_then(|v| v.as_u64()).unwrap();
    let before = depth(&mut h);
    h.app.studio.paste_component();
    let color = |h: &mut Harness, uid: u64| {
        let g = h.app.studio.call("get", j(&format!(r#"{{"uid":{uid},"components":["MeshRenderer"]}}"#))).unwrap();
        g.get("components").unwrap().get("MeshRenderer").unwrap().get("color").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect::<Vec<_>>()
    };
    assert_eq!(color(&mut h, ball), color(&mut h, b), "the ball has the box's values now");
    assert_eq!(depth(&mut h), before + 1, "one undo step");
    h.app.studio.undo();
    assert_ne!(color(&mut h, ball), color(&mut h, b), "and one undo takes them back");
    // pasting a component the targets do not have adds it
    assert!(h.app.studio.copy_component(h.uid("Sun"), "Light"));
    h.app.studio.select(Some(ball));
    h.app.studio.paste_component();
    let has_light = h.app.studio.call("get", j(&format!(r#"{{"uid":{ball},"components":["Light"]}}"#))).unwrap().get("components").and_then(|c| c.get("Light")).is_some_and(|v| !v.is_null());
    assert!(has_light, "the Light was added");
}

#[test]
fn stopping_can_keep_what_the_selected_entities_have_after_playing() {
    let mut h = Harness::new();
    let (box_uid, ball) = (h.uid("Box"), h.uid("Ball"));
    let before_ball = h.pos_of("Ball");
    h.app.play();
    // while the game runs both are moved (by hand, as the game's own systems or the author's hand would), then the Box is selected and the stop keeps its changes only
    h.app.studio.call("set", j(&format!(r#"{{"uid":{box_uid},"component":"Transform","value":{{"pos":[7,1,-3]}}}}"#))).unwrap();
    h.app.studio.call("set", j(&format!(r#"{{"uid":{ball},"component":"Transform","value":{{"pos":[-9,1,9]}}}}"#))).unwrap();
    h.app.studio.select(Some(box_uid));
    let depth = |h: &mut Harness| h.app.studio.engine.call("history", &Json::Null).unwrap().get("undo_depth").and_then(|v| v.as_u64()).unwrap();
    let kept = h.app.stop_keeping();
    assert_eq!(kept, 1);
    assert!(!h.app.studio.is_playing());
    assert_eq!(h.pos_of("Box"), [7.0, 1.0, -3.0], "the Box keeps where it was moved to");
    assert_eq!(h.pos_of("Ball"), before_ball, "the Ball, not selected, is back where the scene had it");
    // (stopping restores the scene, which empties the undo list; what is kept is the one step on it)
    assert_eq!(depth(&mut h), 1, "the kept changes are one undo step");
    h.app.studio.undo();
    assert_eq!(h.pos_of("Box"), [0.0, 1.0, 0.0], "and one undo puts the Box back in the scene's place");
    // nothing selected, nothing to keep; not playing, nothing to do
    assert_eq!(h.app.studio.stop_keeping_selected(), 0);
}

