//! Switching the language from a graph system: `LanguageList` beside the `Localization`, the resources `language.index` and `language.count`.
use super::*;
use components::Localization;
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn language(s: &Sim, uid: u64) -> String {
    let l = s.world.get::<Localization>(s.world.entity(uid).unwrap()).unwrap();
    s.world.strings().get(l.language.0).to_string()
}

fn with_list(initial: &str, list: &str) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, &format!(r#"{{"Localization":{{"language":"{initial}"}},"LanguageList":{{"languages":"{list}"}}}}"#));
    s
}

#[test]
fn a_scene_without_a_list_is_not_touched() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Localization":{"language":"de"}}"#);
    let before = s.state_hash();
    s.step_n(5);
    assert_eq!(language(&s, 1), "de");
    assert_eq!((s.world.resource("language.index"), s.world.resource("language.count")), (None, None));
    // (a list that names nothing does not touch it either)
    let mut s = with_list("de", " , ,");
    s.step();
    assert_eq!((language(&s, 1), s.world.resource("language.index"), s.world.resource("language.count")), ("de".to_string(), None, None));
    let _ = before;
}

#[test]
fn until_a_game_sets_the_index_the_language_the_scene_starts_with_counts() {
    let mut s = with_list("de", "en, de, fr");
    s.step();
    assert_eq!((s.world.resource("language.index"), s.world.resource("language.count")), (Some(1.0), Some(3.0)), "de is the second of the list");
    assert_eq!(language(&s, 1), "de");
    // a language that is not in the list: the first one is on
    let mut s = with_list("xx", "en, de, fr");
    s.step();
    assert_eq!((s.world.resource("language.index"), language(&s, 1).as_str()), (Some(0.0), "en"));
}

#[test]
fn setting_the_index_switches_the_language_at_the_end_of_the_same_tick() {
    let mut s = with_list("en", "en, de, fr");
    s.step();
    s.world.set_resource("language.index", Some(2.0)).unwrap();
    s.step();
    assert_eq!(language(&s, 1), "fr");
    s.world.set_resource("language.index", Some(1.0)).unwrap();
    s.step();
    assert_eq!(language(&s, 1), "de");
    // out of range goes to the nearest end, a fraction to the nearest whole number, and the resource says what is really on
    for (asked, want, name) in [(99.0, 2.0, "fr"), (-3.0, 0.0, "en"), (1.4, 1.0, "de"), (1.6, 2.0, "fr")] {
        s.world.set_resource("language.index", Some(asked)).unwrap();
        s.step();
        assert_eq!((s.world.resource("language.index"), language(&s, 1).as_str()), (Some(want), name), "asked for {asked}");
    }
}

#[test]
fn a_graph_system_cycles_through_the_languages() {
    let mut s = with_list("en", "en, de, fr");
    s.step(); // the engine sets language.count
    s.apply(&Command::DefineSystem {
        def: parse(r#"{"name":"next_language","each":["LanguageList"],"do":[{"if":{"gt":[{"res":"input.space"},0.5]},"then":[{"res_set":"language.index","to":{"mod":[{"add":[{"res":"language.index"},1]},{"res":"language.count"}]}},{"res_set":"input.space","to":0}]}]}"#).unwrap(),
    })
    .unwrap();
    let mut seen = vec![language(&s, 1)];
    for _ in 0..4 {
        s.world.set_resource("input.space", Some(1.0)).unwrap();
        s.step();
        seen.push(language(&s, 1));
    }
    assert_eq!(seen, ["en", "de", "fr", "en", "de"], "every press moves one on and the end wraps round");
}

#[test]
fn the_lowest_numbered_list_with_a_localization_wins_and_names_may_be_paths() {
    let mut s = Sim::new(1);
    spawn(&mut s, 7, r#"{"Localization":{"language":"b"},"LanguageList":{"languages":"a, b"}}"#);
    spawn(&mut s, 3, r#"{"Localization":{"language":"q"},"LanguageList":{"languages":"game/lang/en, game/lang/de"}}"#);
    s.world.set_resource("language.index", Some(1.0)).unwrap();
    s.step();
    assert_eq!((language(&s, 3).as_str(), language(&s, 7).as_str()), ("game/lang/de", "b"), "only entity 3 is looked at");
    assert_eq!(crate::language::parse("en, de\n fr,,\r\n"), ["en", "de", "fr"]);
    assert_eq!(crate::language::parse(&"x,".repeat(100)).len(), crate::language::MAX_LANGUAGES);
}

#[test]
fn the_choice_survives_a_saved_game_and_a_snapshot() {
    let mut s = with_list("en", "en, de, fr");
    s.step();
    s.world.set_resource("language.index", Some(2.0)).unwrap();
    s.step();
    let saved = s.save_json();
    let hash = s.state_hash();
    let mut t = Sim::new(1);
    t.load_save(&saved).unwrap();
    assert_eq!(t.state_hash(), hash);
    t.step();
    assert_eq!((language(&t, 1).as_str(), t.world.resource("language.index")), ("fr", Some(2.0)));
}
