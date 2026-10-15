//! Switching the language from inside a game. `Localization.language` is a name, and a graph system works with numbers only, so a game that wants a language menu
//! lists its languages in a `LanguageList` beside the `Localization` and keeps which one is on in the resource `language.index`. Every tick, after the graph systems,
//! the engine makes the `Localization` say the language that the index points at (so the picture of this very tick is in the new language), and keeps the resource
//! `language.count` up to date. A scene without a `LanguageList` is untouched. See `docs/GAMEPLAY.md`, "Languages".
use crate::components::{LanguageList, Localization};
use lunaris_ecs::{Entity, World};

/// The most languages one list holds.
pub const MAX_LANGUAGES: usize = 64;

/// The names in a `LanguageList.languages` text: separated by commas or new lines, trimmed, the empty ones dropped.
pub fn parse(text: &str) -> Vec<String> {
    text.split([',', '\n', '\r']).map(str::trim).filter(|s| !s.is_empty()).take(MAX_LANGUAGES).map(str::to_string).collect()
}

/// Apply `language.index`: for the lowest-numbered entity that has both a `LanguageList` and a `Localization`.
pub fn update(world: &mut World) {
    let mut found: Option<(u64, Entity, u32, u32)> = None;
    let _ = world.for_each_uid::<(&LanguageList, &Localization), _>(|e, uid, (list, loc)| {
        if found.is_none_or(|f| uid < f.0) {
            found = Some((uid, e, list.languages.0, loc.language.0));
        }
    });
    let Some((_, entity, list_sym, language_sym)) = found else { return };
    let names = parse(world.strings().get(list_sym));
    if names.is_empty() {
        return;
    }
    let count = names.len();
    if world.resource("language.count") != Some(count as f64) {
        let _ = world.set_resource("language.count", Some(count as f64));
    }
    // the resource decides; until a game has set it, the language the `Localization` starts with does (its place in the list, or the first)
    let current = world.strings().get(language_sym).to_string();
    let asked = world.resource("language.index").filter(|v| v.is_finite());
    let index = match asked {
        Some(v) => (v.round().max(0.0) as usize).min(count - 1),
        None => names.iter().position(|n| *n == current).unwrap_or(0),
    };
    if asked != Some(index as f64) {
        let _ = world.set_resource("language.index", Some(index as f64));
    }
    if names[index] != current {
        let sym = world.strings_mut().intern(&names[index]);
        if let Some(l) = world.get_mut::<Localization>(entity) {
            l.language.0 = sym;
        }
    }
}
