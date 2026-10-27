//! Details panel, several entities selected: the rows whose value is not the same on all of them are marked (Unreal's "Multiple Values"), and stop being marked when they are made the same.
use crate::app::App;
use crate::state::Options;
use eframe::egui::{self, Event, RawInput, Rect, Pos2, Vec2};
use lunaris_json::{parse, Json};

fn editor() -> (egui::Context, App) {
    let dir = std::env::temp_dir().join(format!("lunaris-multi-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')', ' '], ""));
    std::fs::create_dir_all(&dir).unwrap();
    let _ = std::fs::remove_file(dir.join(".lunaris").join("editor.json"));
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, Options { root: dir, quality: "reference".to_string(), mute: true, ..Options::default() });
    for c in [
        r#"{"components":{"Name":{"value":"Box"},"Transform":{"pos":[0,1,0]},"MeshRenderer":{"mesh":"cube","color":[0.9,0.2,0.2,1]}}}"#,
        r#"{"components":{"Name":{"value":"Ball"},"Transform":{"pos":[3,1,0]},"MeshRenderer":{"mesh":"sphere","color":[0.2,0.4,0.9,1]}}}"#,
    ] {
        app.studio.call("spawn", parse(c).unwrap()).unwrap();
    }
    app.studio.dirty = false;
    app.studio.rows_dirty = true;
    (ctx, app)
}

fn frames(ctx: &egui::Context, app: &mut App, n: usize) {
    for i in 0..n {
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 900.0))), time: Some(i as f64 / 60.0), events: Vec::<Event>::new(), focused: true, ..RawInput::default() };
        let mut out = ctx.run_ui(raw, |ui| app.draw(ui));
        out.textures_delta.clear();
    }
}

fn uid(app: &App, name: &str) -> u64 {
    app.studio.all.iter().find(|r| r.name == name).unwrap().uid
}

#[test]
fn rows_that_the_selected_entities_do_not_share_are_marked_until_they_are_made_the_same() {
    let (ctx, mut app) = editor();
    frames(&ctx, &mut app, 3);
    let (b, ball) = (uid(&app, "Box"), uid(&app, "Ball"));
    app.studio.select_many(vec![b, ball], Some(ball));
    frames(&ctx, &mut app, 4);
    let mixed = app.inspector.mixed.clone();
    for key in ["MeshRenderer.mesh", "MeshRenderer.color", "Transform.pos"] {
        assert!(mixed.contains(key), "{key} differs and should be marked: {mixed:?}");
    }
    for key in ["MeshRenderer.roughness", "MeshRenderer.metallic", "Transform.scale"] {
        assert!(!mixed.contains(key), "{key} is the same on both and should not be marked: {mixed:?}");
    }
    // a value typed goes to every selected entity: the mesh is the same afterwards, and the row is not marked any more
    let g = "test-mesh".to_string();
    app.studio.set_selected(ball, "MeshRenderer", Json::obj([("mesh", Json::str("cube"))]), Some(&g));
    frames(&ctx, &mut app, 4);
    assert!(!app.inspector.mixed.contains("MeshRenderer.mesh"), "{:?}", app.inspector.mixed);
    assert!(app.inspector.mixed.contains("MeshRenderer.color"), "the colour still differs");
    // one entity selected: nothing is marked
    app.studio.select(Some(b));
    frames(&ctx, &mut app, 3);
    assert!(app.inspector.mixed.is_empty(), "{:?}", app.inspector.mixed);
}
