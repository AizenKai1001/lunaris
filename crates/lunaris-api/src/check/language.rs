//! `check` rules for switching the language from inside a game (`LanguageList`, docs/GAMEPLAY.md "Languages"): a list with nothing to switch, a language that has
//! no file, a start language that is not in the list.
use super::{file_exists, Problems};
use lunaris_ecs::{Result, World};
use lunaris_render::texture::language_name_ok;
use lunaris_sim::components::{LanguageList, Localization};
use std::path::Path;

/// The workspace file of a language: a plain name is `lang/<name>.json`, a name with slashes is the path of the file without `.json`.
fn language_file(name: &str) -> String {
    if name.contains('/') {
        format!("{name}.json")
    } else {
        format!("lang/{name}.json")
    }
}

pub(super) fn run(world: &mut World, root: &Path, problems: &mut Problems) -> Result<()> {
    let mut lists: Vec<(u64, u32)> = Vec::new();
    world.for_each_uid::<(&LanguageList,), _>(|_, uid, (l,)| lists.push((uid, l.languages.0)))?;
    lists.sort_by_key(|l| l.0);
    for (n, (uid, sym)) in lists.iter().enumerate() {
        let names = lunaris_sim::language::parse(world.strings().get(*sym));
        let Some(e) = world.entity(*uid) else { continue };
        let start = world.get::<Localization>(e).map(|l| world.strings().get(l.language.0).to_string());
        if start.is_none() {
            problems.add("warning", Some(*uid), "has a LanguageList but no Localization on the same entity, so there is no language to switch".into(), "put `Localization` and `LanguageList` on one entity");
            continue;
        }
        if n > 0 {
            problems.add("warning", Some(*uid), format!("a second LanguageList: only the one with the lowest uid ({}) is used", lists[0].0), "keep one entity with a Localization and a LanguageList");
            continue;
        }
        if names.is_empty() {
            problems.add("warning", Some(*uid), "LanguageList.languages names no language (a comma separated list such as \"en, de\"), so nothing is switched".into(), "list the languages in the order of your menu: \"en, de, fr\"");
            continue;
        }
        let mut seen: Vec<&str> = Vec::new();
        for name in &names {
            if seen.contains(&name.as_str()) {
                problems.add("warning", Some(*uid), format!("LanguageList names `{name}` twice: the second place is never used by anything but a menu that counts it"), "list each language once");
            }
            seen.push(name);
            if !language_name_ok(name) {
                problems.add("error", Some(*uid), format!("LanguageList: `{name}` is not a language name (letters, digits, `-` and `_`, or a path of the workspace without `.json`)"), "as in Localization.language: `en`, or `mygame/lang/en`");
            } else if !file_exists(root, &language_file(name)) {
                problems.add("warning", Some(*uid), format!("LanguageList: the language `{name}` has no file `{}`, so its texts show as their keys", language_file(name)), "create the file ({\"hello\": \"Hello\"}) or take the language off the list");
            }
        }
        if let Some(start) = start.filter(|s| !s.is_empty() && !names.contains(s)) {
            problems.add("warning", Some(*uid), format!("Localization.language `{start}` is not in the LanguageList, so the game starts in `{}`", names[0]), "start with one of the listed languages (the list's place of it is `language.index` at the start)");
        }
    }
    Ok(())
}
