//! The Lunaris Editor: a native desktop application (egui) around the engine.
//!
//! It is one more client of the tool API: every click is a tool call (`spawn`, `set`, `snapshot`, ...), so undo, recording, the MCP
//! server and an AI agent working in the same project all see exactly what the editor does. The scene view is drawn by the engine's own
//! GPU renderer.
mod app;
mod blueprint;
mod blueprint_ui;
mod canvas_ui;
mod center;
mod chrome;
mod code_edit;
mod console;
mod dialog;
mod effects;
mod gizmo;
mod graph;
mod hierarchy;
mod history_ui;
mod icons;
mod input;
mod inspector;
mod mathx;
mod package;
mod panels;
mod paint;
mod place;
mod profiler;
mod project;
mod project_settings;
#[cfg(test)]
mod shot;
mod script_ui;
mod sequencer;
mod state;
mod theme;
mod thumbs;
mod ui_designer;
mod viewport;
mod vsc_defaults;
mod vsc_theme;

#[cfg(test)]
mod ui_tests;

pub use state::Options;

pub const USAGE: &str = "lunaris-editor [scene.lunaris.json] [--root DIR] [--seed N] [--size WxH] [--quality classic|low|medium|high|ultra|reference] [--select NAME|UID] [--play] [--mute] [--tab project|console|systems|script|blueprint|ui|canvas] [--canvas FILE.canvas] [--script FILE.lun] [--system NAME] [--build GAME.exe] [--view scene|2d|game] [--shot FILE [--shot-delay SECONDS]]";

/// Read the command line (without the program name).
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--root" => o.root = std::path::PathBuf::from(value("--root")?),
            "--seed" => o.seed = value("--seed")?.parse().map_err(|_| "--seed is a whole number".to_string())?,
            "--size" => {
                o.size_given = true;
                let v = value("--size")?;
                let (w, h) = v.split_once('x').ok_or("--size is WxH, for example 1600x900")?;
                o.size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--quality" => o.quality = value("--quality")?,
            "--select" => o.select = Some(value("--select")?),
            "--shot" => o.shot = Some(std::path::PathBuf::from(value("--shot")?)),
            "--shot-delay" => o.shot_delay = value("--shot-delay")?.parse().map_err(|_| "--shot-delay is a number of seconds".to_string())?,
            "--play" => o.play = true,
            "--mute" => o.mute = true,
            "--tab" => o.tab = Some(value("--tab")?),
            "--system" => o.system = Some(value("--system")?),
            "--script" => o.script = Some(value("--script")?),
            "--build" => o.build = Some(std::path::PathBuf::from(value("--build")?)),
            "--view" => o.view = Some(value("--view")?),
            "--canvas" => o.canvas = Some(value("--canvas")?),
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s => o.scene = Some(s.to_string()),
        }
    }
    // a scene given as a path inside the root is made relative to it
    if let Some(s) = o.scene.clone() {
        let p = std::path::Path::new(&s);
        if p.is_absolute() {
            if let Ok(rel) = p.strip_prefix(&o.root) {
                o.scene = Some(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(o)
}

/// Build the scene as a game without opening a window: `lunaris-editor scene.json --build Builds/Game/Game.exe`.
pub fn build_game(opts: &Options) -> Result<String, String> {
    let target = opts.build.clone().ok_or("no --build target")?;
    let mut studio = state::Studio::new(opts.clone());
    if opts.scene.is_none() {
        return Err("--build needs a scene".to_string());
    }
    let scene_rel = studio.scene.clone().ok_or_else(|| "the scene could not be opened (see above)".to_string())?;
    let doc = studio.call("save_scene", lunaris_json::Json::Null)?;
    let json = doc.to_string_pretty().map_err(|e| e.to_string())?;
    let player = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(if cfg!(windows) { "lunaris-player.exe" } else { "lunaris-player" }))).filter(|p| p.is_file()).ok_or("lunaris-player is not next to the editor program (cargo build --release builds both)")?;
    // the project's settings (`project.lunaris.json`); a project without them is built at the quality asked for on the command line
    let mut settings = project_settings::ProjectSettings::load(&opts.root);
    if !opts.root.join(project_settings::FILE).is_file() {
        settings.quality = opts.quality.clone();
    }
    let b = package::build(&opts.root, &json, &scene_rel, &player, &target, &settings)?;
    Ok(format!("built {} ({} files, {:.1} MB){}", b.exe.display(), b.files, b.bytes as f64 / 1e6, if b.missing.is_empty() { String::new() } else { format!("; could not copy: {}", b.missing.join(", ")) }))
}

/// Open the editor window and run until it is closed.
pub fn run(opts: Options) -> Result<(), String> {
    let (w, h) = opts.size;
    let mut viewport = eframe::egui::ViewportBuilder::default().with_title("Lunaris Editor").with_inner_size([w, h]).with_min_inner_size([1000.0, 600.0]).with_icon(theme::icon());
    if opts.size_given {
        // asked for a size: that size
    } else if opts.shot.is_none() {
        // like the other editors, the window opens maximised
        viewport = viewport.with_maximized(true);
    }
    if opts.shot.is_some() {
        // a window made only to take its picture does not need to be seen (or to take the focus from whoever is working)
        viewport = viewport.with_position([-4000.0, 40.0]).with_active(false);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Lunaris Editor", options, Box::new(move |cc| Ok(Box::new(app::App::new(&cc.egui_ctx, opts.clone()))))).map_err(|e| e.to_string())
}
