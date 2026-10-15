//! `export_game`: the world as a start scene plus every file it uses, in a folder that plays by itself (`lunaris play game.lunaris.json`), with the player
//! program next to it and a launcher. Files are found by what the scene says: every string in it that names a file of the workspace. Nothing is deleted or
//! changed in the workspace, and a folder that already has files in it is only written to when asked.
use crate::engine::{hex, Args, Engine};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;
use std::collections::BTreeSet;
use std::path::Path;

/// File name endings that mean "a file the game loads". A string that ends in one and names no file is reported as missing.
const ASSET_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "glb", "gltf", "bin", "wav", "ogg", "mp3", "ttf", "otf", "wasm"];
const MAX_FILES: usize = 20_000;
const MAX_BYTES: u64 = 4 << 30;

fn collect_strings(j: &Json, out: &mut BTreeSet<String>) {
    match j {
        Json::Str(s) => {
            if s.len() <= 260 && !s.contains('\n') {
                out.insert(s.clone());
            }
        }
        Json::Arr(a) => a.iter().for_each(|v| collect_strings(v, out)),
        Json::Obj(pairs) => pairs.iter().for_each(|(_, v)| collect_strings(v, out)),
        _ => {}
    }
}

fn has_asset_extension(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && ASSET_EXTENSIONS.contains(&ext))
}

/// The files a `.gltf` model reads besides itself (its buffers and pictures), relative to the workspace.
fn gltf_dependencies(root: &Path, rel: &str) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join(rel)) else { return Vec::new() };
    let Ok(doc) = lunaris_json::parse(&text) else { return Vec::new() };
    let dir = rel.rsplit_once('/').map(|(d, _)| format!("{d}/")).unwrap_or_default();
    let mut out = Vec::new();
    for list in ["buffers", "images"] {
        for item in doc.get(list).and_then(|l| l.as_array()).unwrap_or(&[]) {
            if let Some(uri) = item.get("uri").and_then(|u| u.as_str()) {
                if !uri.starts_with("data:") {
                    out.push(format!("{dir}{uri}"));
                }
            }
        }
    }
    out
}

fn total_size(files: &[(String, u64)]) -> u64 {
    files.iter().map(|f| f.1).sum()
}

impl Engine {
    pub(crate) fn export_tool(&mut self, a: &Args) -> Result<Json> {
        let name = a.opt_str("name")?.unwrap_or("game").to_string();
        if name.is_empty() || name.len() > 40 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(EcsError::new("bad_name", format!("`{name}` is not a usable name")).at("name").hint("letters, digits, `_` and `-`, up to 40 characters"));
        }
        let out_rel = a.opt_str("out")?.unwrap_or("dist/game").to_string();
        let out = self.resolve(&out_rel)?;
        let out_norm = lunaris_json::safe_relative(&out_rel).map_err(|w| EcsError::new("bad_path", w.to_string()).at("out"))?.trim_end_matches('/').to_string();
        if out == self.root || out_norm.is_empty() || out_norm == "." {
            return Err(EcsError::new("bad_path", "`out` must be a folder inside the workspace, not the workspace itself").at("out").hint("for example dist/mygame"));
        }
        let flag = |key: &str, default: bool| -> Result<bool> {
            match a.0.get(key) {
                None | Some(Json::Null) => Ok(default),
                Some(Json::Bool(b)) => Ok(*b),
                Some(_) => Err(EcsError::new("type_mismatch", format!("`{key}` must be true or false")).at(key)),
            }
        };
        let (overwrite, want_player, force) = (flag("overwrite", false)?, flag("player", true)?, flag("force", false)?);
        if out.is_dir() && !overwrite && std::fs::read_dir(&out).map(|mut d| d.next().is_some()).unwrap_or(false) {
            return Err(EcsError::new("folder_not_empty", format!("`{out_rel}` already has files in it")).at("out").hint("choose another folder, or pass overwrite: true to write into it (files with the same names are replaced, nothing else is touched)"));
        }
        // a game with errors in it is not worth shipping: say what is wrong first
        let checked = self.check_tool(&Args(&Json::Null))?;
        let errors: Vec<String> = checked
            .get("problems")
            .and_then(|p| p.as_array())
            .unwrap_or(&[])
            .iter()
            .filter(|p| p.get("severity").and_then(|s| s.as_str()) == Some("error"))
            .filter_map(|p| p.get("message").and_then(|m| m.as_str()).map(str::to_string))
            .collect();
        if !errors.is_empty() && !force {
            return Err(EcsError::new("game_has_errors", format!("`check` reports {} error(s), for example: {}", errors.len(), errors[0])).hint("fix them (call `check`), or pass force: true to export anyway"));
        }

        let scene = self.sim.scene_json();
        let mut candidates = BTreeSet::new();
        collect_strings(&scene, &mut candidates);
        let mut wanted: BTreeSet<String> = BTreeSet::new();
        let mut missing: BTreeSet<String> = BTreeSet::new();
        for s in &candidates {
            let Ok(norm) = lunaris_json::safe_relative(s) else { continue };
            if self.root.join(&norm).is_file() {
                wanted.insert(norm);
            } else if has_asset_extension(&norm) && !norm.contains(' ') {
                missing.insert(norm);
            }
        }
        // the language files of a game with a `Localization`
        let mut languages = 0usize;
        self.sim.world.for_each_uid::<(&lunaris_sim::components::Localization,), _>(|_, _, _| languages += 1)?;
        if languages > 0 {
            if let Ok(rd) = std::fs::read_dir(self.root.join("lang")) {
                for e in rd.filter_map(|e| e.ok()) {
                    let n = e.file_name().to_string_lossy().to_string();
                    if n.ends_with(".json") && e.path().is_file() {
                        wanted.insert(format!("lang/{n}"));
                    }
                }
            }
        }
        // what a .gltf model reads besides itself
        let models: Vec<String> = wanted.iter().filter(|f| f.to_ascii_lowercase().ends_with(".gltf")).cloned().collect();
        for m in models {
            for dep in gltf_dependencies(&self.root, &m) {
                match lunaris_json::safe_relative(&dep) {
                    Ok(norm) if self.root.join(&norm).is_file() => {
                        wanted.insert(norm);
                    }
                    Ok(norm) => {
                        missing.insert(norm);
                    }
                    Err(_) => {}
                }
            }
        }
        // never copy the output folder into itself, and keep the player's own names free
        let scene_file = format!("{name}.lunaris.json");
        let reserved = [scene_file.as_str(), "export.json", "play.bat", "play.sh", "README.txt", "lunaris", "lunaris.exe"];
        wanted.retain(|f| !(f == &out_norm || f.starts_with(&format!("{out_norm}/"))));
        if let Some(clash) = wanted.iter().find(|f| reserved.contains(&f.as_str())) {
            return Err(EcsError::new("name_clash", format!("the game uses a file called `{clash}`, which the export writes itself")).hint("rename the file in the workspace, or choose another `name`"));
        }
        let mut files: Vec<(String, u64)> = Vec::new();
        for f in &wanted {
            let size = std::fs::metadata(self.root.join(f)).map(|m| m.len()).unwrap_or(0);
            files.push((f.clone(), size));
        }
        if files.len() > MAX_FILES || total_size(&files) > MAX_BYTES {
            return Err(EcsError::new("too_big", format!("{} files, {} bytes: more than an export takes ({MAX_FILES} files, {MAX_BYTES} bytes)", files.len(), total_size(&files))));
        }

        // write it
        let io = |what: &str, p: &Path, e: std::io::Error| EcsError::new("io_error", format!("cannot {what} {}: {e}", p.display()));
        std::fs::create_dir_all(&out).map_err(|e| io("create", &out, e))?;
        for (f, _) in &files {
            let to = out.join(f);
            if let Some(dir) = to.parent() {
                std::fs::create_dir_all(dir).map_err(|e| io("create", dir, e))?;
            }
            std::fs::copy(self.root.join(f), &to).map_err(|e| io("copy to", &to, e))?;
        }
        let scene_text = scene.to_string_pretty().map_err(|e| EcsError::new("write_error", e.to_string()))?;
        let scene_path = out.join(&scene_file);
        std::fs::write(&scene_path, &scene_text).map_err(|e| io("write", &scene_path, e))?;
        // the player program, when this process is `lunaris` itself
        let mut player: Option<String> = None;
        if want_player {
            if let Ok(exe) = std::env::current_exe() {
                let file = exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let stem = file.trim_end_matches(".exe").to_ascii_lowercase();
                if stem == "lunaris" && exe.is_file() {
                    std::fs::copy(&exe, out.join(&file)).map_err(|e| io("copy the player to", &out.join(&file), e))?;
                    player = Some(file);
                }
            }
        }
        let bat = format!("@echo off\r\ncd /d \"%~dp0\"\r\nlunaris.exe play {scene_file} %*\r\n");
        let sh = format!("#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\n./lunaris play {scene_file} \"$@\"\n");
        let readme = format!(
            "{name}\n\nPlay: run play.bat (Windows). It starts `lunaris play {scene_file}`: the scene and the {} files it uses are in this folder, with the same relative paths they had in the workspace.\n{}\
             Options of the player (after play.bat): --size 1920x1080, --quality classic|low|medium|high|ultra, --mute, --volume 0.5.\n\nThe files here keep their own licences: check them before you share this folder.\n",
            files.len(),
            if player.is_some() { "" } else { "The player program (lunaris.exe) was not copied here: put it next to play.bat.\n" }
        );
        for (file, text) in [("play.bat", bat), ("play.sh", sh), ("README.txt", readme)] {
            std::fs::write(out.join(file), text).map_err(|e| io("write", &out.join(file), e))?;
        }
        let hash = hex(self.sim.state_hash());
        let manifest = Json::obj([
            ("scene", Json::str(scene_file.clone())),
            ("state_hash", hash.clone()),
            ("tick", Json::from(self.sim.tick as i64)),
            ("files", Json::arr(files.iter().map(|(p, n)| Json::obj([("path", Json::str(p.clone())), ("bytes", Json::from(*n as i64))])))),
            ("missing", Json::arr(missing.iter().map(|m| Json::str(m.clone())))),
        ]);
        let manifest_text = manifest.to_string_pretty().map_err(|e| EcsError::new("write_error", e.to_string()))?;
        std::fs::write(out.join("export.json"), manifest_text).map_err(|e| io("write", &out.join("export.json"), e))?;
        let mut result = Json::obj([
            ("folder", Json::str(out_norm)),
            ("scene", Json::str(scene_file)),
            ("files", Json::from(files.len())),
            ("bytes", Json::from(total_size(&files) as i64 + scene_text.len() as i64)),
            ("state_hash", hash),
            ("player", player.map_or(Json::Null, Json::str)),
            ("missing", Json::arr(missing.iter().map(|m| Json::str(m.clone())))),
            ("errors_ignored", Json::from(errors.len())),
        ]);
        if !missing.is_empty() {
            result.set("note", Json::str("the scene names files that are not in the workspace (see `missing`); the exported game draws placeholders for them, as the workspace does"));
        }
        Ok(result)
    }
}
