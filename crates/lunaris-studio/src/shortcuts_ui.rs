//! The Keyboard Shortcuts window (Help menu): every key and mouse gesture of the editor, by area, in one place. The table is data so that a test can check it against what the editor does.
use crate::theme;
use eframe::egui::{self, RichText};

/// (the area, [(the keys, what they do)]).
pub const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "The scene view",
        &[
            ("Left click", "Select what is under the pointer (Ctrl+click adds it to the selection or takes it out)"),
            ("Q  W  E  R", "Hand, Move, Rotate and Scale tools"),
            ("Ctrl while dragging a handle", "Snap to the step chosen in the toolbar, or to 0.5 m, 15 degrees and 0.1"),
            ("Right button drag", "Orbit (in Top, Front and the other flat views: pan)"),
            ("Right button + W A S D Q E", "Fly the camera (Shift is faster; the wheel sets the speed)"),
            ("Middle button drag, Shift + right drag", "Pan"),
            ("Alt + left drag", "Orbit"),
            ("Mouse wheel", "Zoom"),
            ("F", "Frame the selection"),
            ("1 to 9", "Return to the camera view saved in that slot"),
            ("Ctrl + 1 to 9", "Save the camera view in that slot"),
        ],
    ),
    (
        "Editing",
        &[
            ("Ctrl+Z", "Undo"),
            ("Ctrl+Y or Ctrl+Shift+Z", "Redo"),
            ("Ctrl+C, Ctrl+V", "Copy the selected entities (with what hangs on them) and paste them, here or in another editor"),
            ("Ctrl+D", "Duplicate the selected entities with what hangs on them"),
            ("Delete", "Delete the selected entities with what hangs on them"),
            ("Ctrl+G", "Group the selected entities under a new empty entity"),
            ("Ctrl+A", "Select everything listed in the Hierarchy"),
            ("F2", "Rename the selected entity"),
            ("Double click a Hierarchy row", "Rename it"),
            ("Drag a Hierarchy row onto another", "Attach it (it keeps its place); onto empty space: detach"),
        ],
    ),
    (
        "The game",
        &[
            ("Ctrl+P", "Play, or stop when it is playing"),
            ("Esc", "Stop playing, or put the paint brush down"),
        ],
    ),
    (
        "The scene file",
        &[("Ctrl+N", "New scene"), ("Ctrl+O", "Open a scene"), ("Ctrl+S", "Save")],
    ),
];

/// Draw the window; false when it was closed.
pub fn window(ctx: &egui::Context) -> bool {
    let mut open = true;
    egui::Window::new("Keyboard Shortcuts").open(&mut open).collapsible(false).resizable(true).default_width(560.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
            for (area, rows) in SHORTCUTS {
                ui.label(RichText::new(*area).strong().color(theme::TEXT_BRIGHT));
                egui::Grid::new(("shortcuts", *area)).num_columns(2).spacing([18.0, 4.0]).show(ui, |ui| {
                    for (keys, what) in *rows {
                        ui.label(RichText::new(*keys).monospace().color(theme::BLUE));
                        ui.label(*what);
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
            }
        });
    });
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_area_has_rows_no_keys_are_listed_twice_and_the_new_shortcuts_are_there() {
        let all: Vec<&str> = SHORTCUTS.iter().flat_map(|(_, rows)| rows.iter().map(|(k, _)| *k)).collect();
        assert!(SHORTCUTS.iter().all(|(_, rows)| !rows.is_empty()));
        let mut sorted = all.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len(), "a key listed twice: {all:?}");
        for key in ["Ctrl+C, Ctrl+V", "Ctrl+G", "Ctrl+A", "F2", "Ctrl + 1 to 9", "Ctrl+P"] {
            assert!(all.contains(&key), "{key}");
        }
    }
}
