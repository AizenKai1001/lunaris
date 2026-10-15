//! Keys and the pointer as the games read them (`input.<key>`, `input.mouse_x`, ...), the same names `lunaris play` uses.
use eframe::egui;
use std::collections::BTreeSet;

/// The name a game sees for a key: `a`, `7`, `space`, `arrowleft`, `shift`, ...
pub fn key_name(key: egui::Key) -> Option<String> {
    let dbg = format!("{key:?}").to_lowercase();
    // letters, digits (Num7 -> 7), F keys and named keys are already their debug names in lower case
    let name = if let Some(d) = dbg.strip_prefix("num") { d.to_string() } else { dbg };
    let ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then_some(name)
}

/// What is held now: keys, modifiers and the mouse buttons.
pub fn held(ctx: &egui::Context) -> BTreeSet<String> {
    ctx.input(|i| {
        let mut set: BTreeSet<String> = i.keys_down.iter().filter_map(|k| key_name(*k)).collect();
        if i.modifiers.shift {
            set.insert("shift".to_string());
        }
        if i.modifiers.ctrl {
            set.insert("control".to_string());
        }
        if i.modifiers.alt {
            set.insert("alt".to_string());
        }
        if i.pointer.button_down(egui::PointerButton::Primary) {
            set.insert("mouse_left".to_string());
        }
        if i.pointer.button_down(egui::PointerButton::Secondary) {
            set.insert("mouse_right".to_string());
        }
        set
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_get_the_names_games_read() {
        assert_eq!(key_name(egui::Key::W).as_deref(), Some("w"));
        assert_eq!(key_name(egui::Key::Space).as_deref(), Some("space"));
        assert_eq!(key_name(egui::Key::ArrowLeft).as_deref(), Some("arrowleft"));
        assert_eq!(key_name(egui::Key::Num7).as_deref(), Some("7"));
        assert_eq!(key_name(egui::Key::Enter).as_deref(), Some("enter"));
        assert_eq!(key_name(egui::Key::F5).as_deref(), Some("f5"));
        for k in [egui::Key::A, egui::Key::Escape, egui::Key::Tab, egui::Key::Backspace, egui::Key::Minus] {
            let n = key_name(k).unwrap();
            assert!(lunaris_ecs::valid_resource_name(&format!("input.{n}")), "{n}");
        }
    }
}
