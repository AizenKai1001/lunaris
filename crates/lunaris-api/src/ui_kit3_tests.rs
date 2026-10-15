//! UI kit v3 through the tool API: the `check` rules for scroll views and their bars, the new components in `schema`, and the example scene `examples/ui_kit3.lunaris.json` played with the `input` tool.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn problems(e: &mut crate::Engine) -> Vec<String> {
    let c = e.call("check", &Json::Null).unwrap();
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| p.get("message").unwrap().as_str().unwrap().to_string()).collect()
}

fn about(e: &mut crate::Engine, words: &[&str]) -> Vec<String> {
    problems(e).into_iter().filter(|m| words.iter().any(|w| m.contains(w))).collect()
}

const SCROLL_WORDS: &[&str] = &["UiScroll", "UiScrollBar"];

#[test]
fn the_scroll_components_are_in_the_schema_with_their_fields() {
    let mut e = engine();
    let s = e.call("schema", &Json::Null).unwrap();
    let text = s.to_string_lossy();
    for needle in ["UiScroll", "UiScrollBar", "content_size", "thickness", "min_length"] {
        assert!(text.contains(needle), "the schema does not mention {needle}");
    }
}

#[test]
fn check_is_quiet_for_a_well_formed_scroll_view() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.4]},"UiScroll":{}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Ui":{"pos":[0.1,0.1],"size":[0.3,0.1],"button":true},"Parent":{"target":1}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Ui":{"button":true},"UiScrollBar":{"view":1,"track":4}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":4,"components":{"Ui":{"button":true}}}"#)).unwrap();
    assert!(about(&mut e, SCROLL_WORDS).is_empty(), "{:?}", about(&mut e, SCROLL_WORDS));
}

#[test]
fn check_says_what_a_broken_scroll_view_does_not_do() {
    let mut e = engine();
    // no Ui, nothing inside, a layout on the viewport itself, a wheel that does nothing, a size that is not a size
    e.call("spawn", &j(r#"{"uid":1,"components":{"UiScroll":{}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Ui":{"size":[0.4,0.4]},"UiScroll":{"speed":0.0,"content_size":[-1,0]},"UiLayout":{}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Ui":{"size":[0.0,0.4]},"UiScroll":{}}}"#)).unwrap();
    // bars: no Ui, a thumb that is not a button, a view that is not a view, a track that does not exist or is not a button
    e.call("spawn", &j(r#"{"uid":10,"components":{"UiScrollBar":{"view":2}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":11,"components":{"Ui":{},"UiScrollBar":{"view":2,"track":12,"thickness":0}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":12,"components":{"Ui":{}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":13,"components":{"Ui":{"button":true},"UiScrollBar":{"view":99,"track":98}}}"#)).unwrap();
    let p = about(&mut e, SCROLL_WORDS);
    let has = |s: &str| p.iter().any(|m| m.contains(s));
    assert!(has("UiScroll but no Ui"), "{p:?}");
    assert!(has("nothing is inside this UiScroll"), "{p:?}");
    assert!(has("UiScroll and a UiLayout on one entity"), "{p:?}");
    assert!(has("UiScroll.speed 0"), "{p:?}");
    assert!(has("UiScroll.content_size"), "{p:?}");
    assert!(has("has no size"), "{p:?}");
    assert!(has("UiScrollBar but no Ui"), "{p:?}");
    assert!(has("thumb of a UiScrollBar is not a button"), "{p:?}");
    assert!(has("view 99 is not an entity with a UiScroll"), "{p:?}");
    assert!(has("track 98 does not exist"), "{p:?}");
    assert!(has("track of a UiScrollBar is not a button"), "{p:?}");
    assert!(has("UiScrollBar.thickness 0"), "{p:?}");
}

/// The example scene in an engine whose workspace has the scene and its font.
fn with_example() -> crate::Engine {
    let mut e = engine();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::create_dir_all(e.root.join("examples/fonts")).unwrap();
    std::fs::copy(repo.join("examples/fonts/pixel.ttf"), e.root.join("examples/fonts/pixel.ttf")).unwrap();
    std::fs::copy(repo.join("examples/ui_kit3.lunaris.json"), e.root.join("ui_kit3.lunaris.json")).unwrap();
    e.call("load_scene", &j(r#"{"path":"ui_kit3.lunaris.json"}"#)).unwrap();
    e
}

fn uid_named(e: &mut crate::Engine, name: &str) -> u64 {
    let q = e.call("query", &j(&format!(r#"{{"where":[{{"component":"Name","field":"value","value":"{name}"}}],"select":[]}}"#))).unwrap();
    q.get("entities").unwrap().as_array().unwrap()[0].get("uid").unwrap().as_u64().unwrap()
}

fn field(e: &mut crate::Engine, uid: u64, component: &str, f: &str) -> Json {
    let g = e.call("get", &j(&format!(r#"{{"uid":{uid},"components":["{component}"]}}"#))).unwrap();
    g.get("components").unwrap().get(component).unwrap().get(f).unwrap().clone()
}

#[test]
fn the_example_scene_checks_clean_and_its_list_scrolls_with_the_wheel_and_the_pointer_through_the_tools() {
    let mut e = with_example();
    let errors = e.call("check", &Json::Null).unwrap().get("errors").and_then(|v| v.as_i64()).unwrap_or(0);
    assert_eq!(errors, 0, "{:?}", problems(&mut e));
    let every_word: Vec<&str> = SCROLL_WORDS.iter().chain(SELECT_WORDS).chain(DRAG_WORDS).chain(TIP_WORDS).copied().collect();
    assert!(about(&mut e, &every_word).is_empty(), "{:?}", about(&mut e, &every_word));
    let view = uid_named(&mut e, "Items view");
    e.call("step", &j(r#"{"ticks":3}"#)).unwrap();
    let off = |e: &mut crate::Engine| field(e, view, "UiScroll", "offset").index(1).unwrap().as_f64().unwrap();
    assert!((off(&mut e) - 0.05).abs() < 1e-4, "the scene starts a little way down: {}", off(&mut e));
    // the pointer over the list, one notch down
    e.call("input", &j(r#"{"values":{"mouse_x":0.15,"mouse_y":0.35,"mouse_wheel":-1}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":1}"#)).unwrap();
    e.call("input", &j(r#"{"values":{"mouse_wheel":0}}"#)).unwrap();
    assert!((off(&mut e) - 0.11).abs() < 1e-4, "{}", off(&mut e));
    // the pointer away from it does not scroll
    e.call("input", &j(r#"{"values":{"mouse_x":0.8,"mouse_y":0.9,"mouse_wheel":-1}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":3}"#)).unwrap();
    e.call("input", &j(r#"{"values":{"mouse_wheel":0}}"#)).unwrap();
    assert!((off(&mut e) - 0.11).abs() < 1e-4, "{}", off(&mut e));
}

const SELECT_WORDS: &[&str] = &["UiDropdown", "UiOption", "UiTab", "UiTabs", "UiGroup"];
const DRAG_WORDS: &[&str] = &["UiDrag", "UiDrop"];
const TIP_WORDS: &[&str] = &["UiTooltip", "UiRadial"];

#[test]
fn check_is_quiet_for_a_tooltip_with_its_box_and_a_plain_dial() {
    let mut e = engine();
    spawn_all(
        &mut e,
        &[
            r#"{"uid":1,"components":{"Ui":{},"UiTooltip":{"text":"Sword\n+5","delay":0.3}}}"#,
            r#"{"uid":2,"components":{"Ui":{"visible":false},"UiTooltipBox":{}}}"#,
            r#"{"uid":3,"components":{"Ui":{"color":[1,0,0,1]},"UiRadial":{"value":0.4,"inner":0.5,"start":30}}}"#,
            r#"{"uid":4,"components":{"Ui":{"fill":1.0},"UiStyle":{"radius":0.02},"UiRadial":{"shape":1}}}"#,
        ],
    );
    assert!(about(&mut e, TIP_WORDS).is_empty(), "{:?}", about(&mut e, TIP_WORDS));
}

#[test]
fn check_says_what_a_broken_tooltip_and_dial_do_not_do() {
    let mut e = engine();
    spawn_all(
        &mut e,
        &[
            // a trigger without a Ui, one with no text and a delay that is no time, and no box anywhere
            r#"{"uid":1,"components":{"UiTooltip":{"text":"x"}}}"#,
            r#"{"uid":2,"components":{"Ui":{},"UiTooltip":{"delay":-1}}}"#,
            // dials: no Ui, a shape and a hole that mean nothing, a frame and a picture that are not drawn
            r#"{"uid":10,"components":{"UiRadial":{}}}"#,
            r#"{"uid":11,"components":{"Ui":{},"UiRadial":{"shape":2,"inner":3}}}"#,
            r#"{"uid":12,"components":{"Ui":{},"UiRadial":{"shape":1,"inner":0.5}}}"#,
            r#"{"uid":13,"components":{"Ui":{"border":0.01},"UiStyle":{"image":"circle"},"UiRadial":{}}}"#,
        ],
    );
    let p = about(&mut e, TIP_WORDS);
    let has = |s: &str| p.iter().any(|m| m.contains(s));
    for want in [
        "UiTooltip but no Ui",
        "the UiTooltip has no text",
        "UiTooltip.delay -1",
        "no UiTooltipBox, so no tooltip is ever shown",
        "UiRadial but no Ui",
        "UiRadial.shape 2",
        "UiRadial.inner 3",
        "UiRadial.inner only makes a hole in the circle",
        "replaces the rectangle's bar, border, shadow, gradient and picture",
    ] {
        assert!(has(want), "no problem says `{want}`: {p:?}");
    }
    // boxes: no Ui, a button, and a second one
    spawn_all(
        &mut e,
        &[
            r#"{"uid":20,"components":{"UiTooltipBox":{}}}"#,
            r#"{"uid":21,"components":{"Ui":{"button":true},"UiTooltipBox":{"padding":-1}}}"#,
            r#"{"uid":22,"components":{"Ui":{},"UiTooltipBox":{}}}"#,
        ],
    );
    let p = about(&mut e, TIP_WORDS);
    let has = |s: &str| p.iter().any(|m| m.contains(s));
    for want in ["UiTooltipBox but no Ui", "the UiTooltipBox is a button", "a second UiTooltipBox: only the one with the lowest uid (20)", "UiTooltipBox.padding or offset is not a number"] {
        assert!(has(want), "no problem says `{want}`: {p:?}");
    }
    assert!(!has("no UiTooltipBox, so no tooltip"), "there are boxes now");
}

#[test]
fn check_is_quiet_for_slots_and_things_that_fit_together() {
    let mut e = engine();
    spawn_all(
        &mut e,
        &[
            r#"{"uid":1,"components":{"Ui":{},"UiDrop":{"name":"bag_1"}}}"#,
            r#"{"uid":2,"components":{"Ui":{},"UiDrop":{"name":"bag_2","accept":3}}}"#,
            r#"{"uid":3,"components":{"Ui":{"button":true},"UiDrag":{"item":4,"slot":1}}}"#,
            r#"{"uid":4,"components":{"Ui":{"button":true},"UiDrag":{"item":5,"kind":1}}}"#,
        ],
    );
    assert!(about(&mut e, DRAG_WORDS).is_empty(), "{:?}", about(&mut e, DRAG_WORDS));
}

#[test]
fn check_says_what_a_broken_drag_and_drop_does_not_do() {
    let mut e = engine();
    spawn_all(
        &mut e,
        &[
            // a thing without a Ui, one that is not a button, one in a slot that is nothing, two in one slot, a kind that wraps
            r#"{"uid":1,"components":{"UiDrag":{}}}"#,
            r#"{"uid":2,"components":{"Ui":{},"UiDrag":{}}}"#,
            r#"{"uid":3,"components":{"Ui":{"button":true},"UiDrag":{"slot":99}}}"#,
            r#"{"uid":4,"components":{"Ui":{"button":true},"UiDrag":{"slot":10}}}"#,
            r#"{"uid":5,"components":{"Ui":{"button":true},"UiDrag":{"slot":10,"kind":40}}}"#,
            // slots: no Ui, takes nothing, a name that is no resource name, a repeated name
            r#"{"uid":10,"components":{"Ui":{},"UiDrop":{"name":"bag"}}}"#,
            r#"{"uid":11,"components":{"UiDrop":{"accept":0}}}"#,
            r#"{"uid":12,"components":{"Ui":{},"UiDrop":{"name":"two words"}}}"#,
            r#"{"uid":13,"components":{"Ui":{},"UiDrop":{"name":"bag"}}}"#,
        ],
    );
    let p = about(&mut e, DRAG_WORDS);
    let has = |s: &str| p.iter().any(|m| m.contains(s));
    for want in [
        "UiDrag but no Ui",
        "a UiDrag is not a button",
        "slot 99 is not an entity with a UiDrop",
        "another UiDrag sits in the same slot 10",
        "UiDrag.kind 40 is above 31, so it counts as 8",
        "UiDrop but no Ui",
        "UiDrop.accept is 0",
        "UiDrop.name `two words` does not make a resource name",
        "another UiDrop has the name `bag`",
    ] {
        assert!(has(want), "no problem says `{want}`: {p:?}");
    }
}

fn spawn_all(e: &mut crate::Engine, entities: &[&str]) {
    for s in entities {
        e.call("spawn", &j(s)).unwrap();
    }
}

#[test]
fn check_is_quiet_for_a_well_formed_dropdown_and_tabs() {
    let mut e = engine();
    spawn_all(
        &mut e,
        &[
            r#"{"uid":1,"components":{"Ui":{"button":true},"UiDropdown":{"list":2,"selected":1,"resource":"ui.quality"}}}"#,
            r#"{"uid":2,"components":{"Ui":{},"UiGroup":{"shown":false}}}"#,
            r#"{"uid":3,"components":{"Ui":{"button":true},"UiOption":{"dropdown":1,"index":0},"Parent":{"target":2}}}"#,
            r#"{"uid":4,"components":{"Ui":{"button":true},"UiOption":{"dropdown":1,"index":1},"Parent":{"target":2}}}"#,
            r#"{"uid":10,"components":{"UiTabs":{"selected":0,"resource":"ui.tab"}}}"#,
            r#"{"uid":11,"components":{"Ui":{"button":true},"UiTab":{"tabs":10,"index":0,"panel":13}}}"#,
            r#"{"uid":12,"components":{"Ui":{"button":true},"UiTab":{"tabs":10,"index":1,"panel":14}}}"#,
            r#"{"uid":13,"components":{"Ui":{},"UiGroup":{}}}"#,
            r#"{"uid":14,"components":{"Ui":{},"UiGroup":{"shown":false}}}"#,
        ],
    );
    assert!(about(&mut e, SELECT_WORDS).is_empty(), "{:?}", about(&mut e, SELECT_WORDS));
}

#[test]
fn check_says_what_a_broken_dropdown_and_broken_tabs_do_not_do() {
    let mut e = engine();
    spawn_all(
        &mut e,
        &[
            // no Ui, a resource that is no name, no options, no list
            r#"{"uid":1,"components":{"UiDropdown":{"resource":"bad name"}}}"#,
            // a header that is not a button, a list that does not exist, a selection that matches nothing, two options with one index
            r#"{"uid":2,"components":{"Ui":{},"UiDropdown":{"list":99,"selected":5}}}"#,
            r#"{"uid":3,"components":{"Ui":{},"UiOption":{"dropdown":2,"index":0}}}"#,
            r#"{"uid":4,"components":{"Ui":{"button":true},"UiOption":{"dropdown":2,"index":0}}}"#,
            // a list without a group
            r#"{"uid":5,"components":{"Ui":{"button":true},"UiDropdown":{"list":6}}}"#,
            r#"{"uid":6,"components":{"Ui":{}}}"#,
            r#"{"uid":7,"components":{"Ui":{"button":true},"UiOption":{"dropdown":5,"index":0}}}"#,
            // an option of nothing
            r#"{"uid":8,"components":{"Ui":{},"UiOption":{"dropdown":50}}}"#,
            // tabs: a selection that matches no tab, a name that is no resource, a tab that is not a button and shows nothing, one of nowhere, one without a Ui, a panel without a group, a repeated index
            r#"{"uid":10,"components":{"UiTabs":{"selected":3,"resource":"x y"}}}"#,
            r#"{"uid":11,"components":{"Ui":{},"UiTab":{"tabs":10,"index":0,"panel":0}}}"#,
            r#"{"uid":12,"components":{"Ui":{"button":true},"UiTab":{"tabs":77,"index":0,"panel":13}}}"#,
            r#"{"uid":13,"components":{"Ui":{}}}"#,
            r#"{"uid":14,"components":{"UiTab":{"tabs":10,"index":0,"panel":15}}}"#,
            r#"{"uid":15,"components":{"Name":{"value":"nothing"}}}"#,
            // a set of tabs nobody belongs to
            r#"{"uid":20,"components":{"UiTabs":{}}}"#,
        ],
    );
    let p = about(&mut e, SELECT_WORDS);
    let has = |s: &str| p.iter().any(|m| m.contains(s));
    for want in [
        "UiDropdown but no Ui",
        "UiDropdown's resource `bad name` is not a resource name",
        "no UiOption names this UiDropdown",
        "has no `list`",
        "header of a UiDropdown is not a button",
        "list 99 does not exist",
        "UiDropdown.selected 5 is the index of no option",
        "same `index`",
        "no UiGroup, so only the list's own rectangle",
        "dropdown 50 is not an entity with a UiDropdown",
        "this UiOption is not a button",
        "UiTabs.selected 3 is the index of no tab",
        "UiTabs's resource `x y` is not a resource name",
        "this UiTab is not a button",
        "panel 0 does not exist",
        "tabs 77 is not an entity with a UiTabs",
        "panel of this UiTab has no UiGroup",
        "UiTab but no Ui",
        "neither a UiGroup nor a Ui",
        "another UiTab of the same set has the index 0",
        "no UiTab names this UiTabs",
    ] {
        assert!(has(want), "no problem says `{want}`: {p:?}");
    }
}
