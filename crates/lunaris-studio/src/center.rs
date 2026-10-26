//! The tabs over the middle of the window: the scene and the game, and the editors that open next to them.
use crate::chrome;
use eframe::egui::Ui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CenterTab {
    Scene,
    Game,
    Ui,
    Blueprint,
    Script,
    Canvas,
    Sequencer,
    Effects,
}

/// The strip with the middle tabs. Returns the tab that was clicked.
pub fn strip(ui: &mut Ui, active: CenterTab, right: impl FnOnce(&mut Ui)) -> Option<CenterTab> {
    chrome::tab_strip(ui, &[(CenterTab::Scene, "Scene"), (CenterTab::Game, "Game"), (CenterTab::Ui, "UI"), (CenterTab::Blueprint, "Blueprint"), (CenterTab::Script, "Script"), (CenterTab::Canvas, "Canvas"), (CenterTab::Sequencer, "Sequencer"), (CenterTab::Effects, "Effects")], active, right)
}
