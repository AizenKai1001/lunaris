//! Switching the language from a graph system, through the tool API: what `check` says about a `LanguageList`, the picture in each language and the export.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn problems(e: &mut crate::Engine) -> Vec<String> {
    let c = e.call("check", &Json::Null).unwrap();
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| p.get("message").unwrap().as_str().unwrap().to_string()).collect()
}

fn about_the_list(e: &mut crate::Engine) -> Vec<String> {
    problems(e).into_iter().filter(|m| m.contains("LanguageList") || m.contains("Localization.language")).collect()
}

fn lang_files(e: &crate::Engine) {
    std::fs::create_dir_all(e.root.join("lang")).unwrap();
    std::fs::write(e.root.join("lang/en.json"), r#"{"play":"Play"}"#).unwrap();
    std::fs::write(e.root.join("lang/de.json"), r#"{"play":"Spielen"}"#).unwrap();
}

#[test]
fn check_is_quiet_about_a_list_whose_files_exist() {
    let mut e = engine();
    lang_files(&e);
    e.call("spawn", &j(r#"{"uid":1,"components":{"Localization":{"language":"de"},"LanguageList":{"languages":"en, de"}}}"#)).unwrap();
    assert!(about_the_list(&mut e).is_empty(), "{:?}", about_the_list(&mut e));
}

#[test]
fn check_says_what_is_wrong_with_a_list() {
    let mut e = engine();
    lang_files(&e);
    // no Localization beside it
    e.call("spawn", &j(r#"{"uid":1,"components":{"LanguageList":{"languages":"en, de"}}}"#)).unwrap();
    assert!(about_the_list(&mut e).iter().any(|m| m.contains("no Localization on the same entity")), "{:?}", about_the_list(&mut e));
    e.call("remove_component", &j(r#"{"uid":1,"component":"LanguageList"}"#)).unwrap();
    // nothing in the list, a name that is not a language name, a missing file, a repeated name, a start language that is not listed, a second list
    e.call("spawn", &j(r#"{"uid":2,"components":{"Localization":{"language":"xx"},"LanguageList":{"languages":" , "}}}"#)).unwrap();
    assert!(about_the_list(&mut e).iter().any(|m| m.contains("names no language")), "{:?}", about_the_list(&mut e));
    e.call("set", &j(r#"{"uid":2,"component":"LanguageList","value":{"languages":"en, de, fr, en, a.b"}}"#)).unwrap();
    let p = about_the_list(&mut e);
    assert!(p.iter().any(|m| m.contains("`fr` has no file `lang/fr.json`")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("`en` twice")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("`a.b` is not a language name")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("`xx` is not in the LanguageList") && m.contains("starts in `en`")), "{p:?}");
    assert!(p.iter().all(|m| !m.contains("`de` has no file") && !m.contains("`en` has no file")), "{p:?}");
    e.call("spawn", &j(r#"{"uid":3,"components":{"Localization":{"language":"en"},"LanguageList":{"languages":"en"}}}"#)).unwrap();
    assert!(about_the_list(&mut e).iter().any(|m| m.contains("a second LanguageList") && m.contains("(2)")), "{:?}", about_the_list(&mut e));
}

#[test]
fn the_picture_changes_language_when_a_graph_system_sets_the_index() {
    let mut e = engine();
    lang_files(&e);
    e.call("spawn", &j(r#"{"uid":1,"components":{"Camera":{},"Transform":{"pos":[0,0,3]}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Localization":{"language":"en"},"LanguageList":{"languages":"en, de"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Text":{"value":"{#play}","pos":[0.1,0.1],"size":0.3}}}"#)).unwrap();
    // pressing space (the `input` tool sets the key) advances to the next language, the way a settings screen would
    e.call("define_script", &j(r#"{"source":"system next_language each LanguageList { if $input.space > 0.5 { $language.index = ($language.index + 1) % $language.count; $input.space = 0; } }"}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":2}"#)).unwrap();
    let draw = |e: &mut crate::Engine| e.call("render", &j(r#"{"width":160,"height":90,"quality":"reference"}"#)).unwrap().get("hash").unwrap().as_str().unwrap().to_string();
    let english = draw(&mut e);
    e.call("input", &j(r#"{"values":{"space":true}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":1}"#)).unwrap();
    let german = draw(&mut e);
    assert_ne!(english, german, "the text is another word");
    assert_eq!(e.call("resources", &Json::Null).unwrap().get("resources").and_then(|r| r.get("language.index")).and_then(|v| v.as_f64()), Some(1.0));
    // (the key is released by the system itself: a second step does not advance again)
    e.call("step", &j(r#"{"ticks":3}"#)).unwrap();
    assert_eq!(draw(&mut e), german);
    e.call("input", &j(r#"{"values":{"space":true}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":1}"#)).unwrap();
    assert_eq!(draw(&mut e), english, "past the last language it wraps to the first");
}

#[test]
fn an_export_takes_every_language_of_the_list_even_from_other_folders() {
    let mut e = engine();
    let root = e.root.clone();
    let _ = std::fs::remove_dir_all(root.join("dist_languages"));
    for d in ["game/lang", "other/lang"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    std::fs::write(root.join("game/lang/en.json"), r#"{"hi":"Hello"}"#).unwrap();
    std::fs::write(root.join("other/lang/de.json"), r#"{"hi":"Hallo"}"#).unwrap();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Localization":{"language":"game/lang/en"},"LanguageList":{"languages":"game/lang/en, other/lang/de"}}}"#)).unwrap();
    e.call("export_game", &j(r#"{"out":"dist_languages","name":"langs","force":true}"#)).unwrap();
    for want in ["game/lang/en.json", "other/lang/de.json"] {
        assert!(root.join("dist_languages").join(want).is_file(), "{want} is in the folder");
    }
}
