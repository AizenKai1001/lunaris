//! Building a game: a folder with a program that opens the scene (`<Name>.exe`, a copy of the Lunaris player), the scene itself, and every file the scene
//! mentions (pictures, models, sounds, WebAssembly modules), at the same relative paths, so the game runs on a machine that has nothing else of
//! Lunaris installed. The player reads `game.json` next to itself.
use crate::project_settings::ProjectSettings;
use lunaris_json::Json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Built {
    pub folder: PathBuf,
    pub exe: PathBuf,
    pub files: usize,
    pub bytes: u64,
    pub missing: Vec<String>,
}

/// Every string in a document, with its path.
fn strings(v: &Json, out: &mut BTreeSet<String>) {
    match v {
        Json::Str(s) => {
            if s.len() < 260 && !s.is_empty() {
                out.insert(s.clone());
            }
            // a `SoundFile` names several files, one on each line or separated by `|`
            if s.contains('\n') || s.contains('|') {
                for part in s.split(|c| c == '\n' || c == '|') {
                    let part = part.trim();
                    if part.len() < 260 && !part.is_empty() {
                        out.insert(part.to_string());
                    }
                }
            }
        }
        Json::Arr(a) => a.iter().for_each(|x| strings(x, out)),
        Json::Obj(o) => o.iter().for_each(|(_, x)| strings(x, out)),
        _ => {}
    }
}

/// The files of the workspace a scene document refers to: any string that is the relative path of a file that exists.
pub fn referenced_files(scene: &Json, root: &Path) -> Vec<String> {
    let mut all = BTreeSet::new();
    strings(scene, &mut all);
    let mut files = Vec::new();
    for s in all {
        if !(s.contains('.') || s.contains('/')) {
            continue;
        }
        let Ok(rel) = lunaris_json::safe_relative(&s) else { continue };
        let rel = rel.replace('\\', "/");
        if rel.starts_with("target/") || rel.starts_with(".git/") {
            continue;
        }
        if root.join(&rel).is_file() {
            files.push(rel);
        }
    }
    files
}

/// Build the game into the folder of `exe_path` (its file name, without `.exe`, is the game's name). `scene_json` is the scene as the
/// engine saved it; `scene_rel` is the path it will have in the game; `player` is the program to copy.
pub fn build(root: &Path, scene_json: &str, scene_rel: &str, player: &Path, exe_path: &Path, settings: &ProjectSettings) -> Result<Built, String> {
    let folder = exe_path.parent().ok_or("the target has no folder")?.to_path_buf();
    let name = exe_path.file_stem().and_then(|s| s.to_str()).filter(|s| !s.is_empty()).ok_or("the game needs a name")?.to_string();
    let scene = lunaris_json::parse(scene_json).map_err(|e| format!("the scene is not valid JSON: {e}"))?;
    let scene_rel = lunaris_json::safe_relative(scene_rel).map_err(|e| format!("the scene path is not usable: {e}"))?.replace('\\', "/");
    if !player.is_file() {
        return Err(format!("the player program {} does not exist (build it with: cargo build --release -p lunaris)", player.display()));
    }
    std::fs::create_dir_all(&folder).map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut missing = Vec::new();
    // the program and its settings
    std::fs::copy(player, exe_path).map_err(|e| format!("cannot copy the player to {}: {e}", exe_path.display()))?;
    files += 1;
    bytes += std::fs::metadata(exe_path).map(|m| m.len()).unwrap_or(0);
    let config = settings.game_config(&name, &scene_rel);
    std::fs::write(folder.join("game.json"), config.to_string_pretty().map_err(|e| e.to_string())?).map_err(|e| format!("cannot write game.json: {e}"))?;
    files += 1;
    // the scene
    let scene_path = folder.join(&scene_rel);
    if let Some(d) = scene_path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("cannot create {}: {e}", d.display()))?;
    }
    std::fs::write(&scene_path, scene_json).map_err(|e| format!("cannot write the scene: {e}"))?;
    files += 1;
    bytes += scene_json.len() as u64;
    // what the scene mentions
    for rel in referenced_files(&scene, root) {
        if rel == scene_rel {
            continue;
        }
        let (src, dst) = (root.join(&rel), folder.join(&rel));
        if let Some(d) = dst.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        match std::fs::copy(&src, &dst) {
            Ok(n) => {
                files += 1;
                bytes += n;
            }
            Err(_) => missing.push(rel),
        }
    }
    Ok(Built { folder, exe: exe_path.to_path_buf(), files, bytes, missing })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lunaris-package-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_built_game_has_the_program_the_scene_and_exactly_the_files_the_scene_uses() {
        let root = scratch("root");
        std::fs::create_dir_all(root.join("sounds")).unwrap();
        std::fs::create_dir_all(root.join("models")).unwrap();
        std::fs::write(root.join("sounds/boom.wav"), b"RIFF").unwrap();
        std::fs::write(root.join("sounds/step_a.ogg"), b"OggS").unwrap();
        std::fs::write(root.join("sounds/step_b.ogg"), b"OggS").unwrap();
        std::fs::write(root.join("models/ship.glb"), b"glTF").unwrap();
        std::fs::write(root.join("notes.txt"), b"not used").unwrap();
        let player = root.join("player.exe");
        std::fs::write(&player, b"MZ-player").unwrap();
        let scene = r#"{"lunaris":1,"entities":[{"uid":1,"components":{"MeshRenderer":{"mesh":"models/ship.glb","texture":"checker"},"AudioSource":{"file":"sounds/boom.wav"},"Name":{"value":"notes.txt"}}},{"uid":2,"components":{"SoundFile":{"file":"sounds/gone.wav"}}},{"uid":3,"components":{"SoundFile":{"name":"step","file":"sounds/step_a.ogg\nsounds/step_b.ogg"}}}]}"#;
        let out = scratch("out").join("My Game");
        let built = build(&root, scene, "scenes/main.lunaris.json", &player, &out.join("My Game.exe"), &ProjectSettings::default()).unwrap();
        assert_eq!(std::fs::read(out.join("My Game.exe")).unwrap(), b"MZ-player");
        assert!(out.join("scenes/main.lunaris.json").is_file() && out.join("sounds/boom.wav").is_file() && out.join("models/ship.glb").is_file());
        assert!(!out.join("sounds/gone.wav").exists(), "a file that does not exist is not copied (and the scene still builds)");
        assert!(out.join("sounds/step_a.ogg").is_file() && out.join("sounds/step_b.ogg").is_file(), "every file of a SoundFile that names several is copied");
        let cfg = lunaris_json::parse(&std::fs::read_to_string(out.join("game.json")).unwrap()).unwrap();
        assert_eq!(cfg.get("scene").and_then(|s| s.as_str()), Some("scenes/main.lunaris.json"));
        assert_eq!(cfg.get("name").and_then(|s| s.as_str()), Some("My Game"));
        // the project's settings are what the player is told
        let custom = ProjectSettings { quality: "low".into(), size: (800, 600), seed: 9, ..ProjectSettings::default() };
        let out2 = scratch("out2").join("Custom");
        build(&root, scene, "scenes/main.lunaris.json", &player, &out2.join("Custom.exe"), &custom).unwrap();
        let cfg2 = lunaris_json::parse(&std::fs::read_to_string(out2.join("game.json")).unwrap()).unwrap();
        assert_eq!((cfg2.get("quality").and_then(|s| s.as_str()), cfg2.get("seed").and_then(|s| s.as_u64())), (Some("low"), Some(9)));
        assert_eq!(cfg2.get("size").unwrap().as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect::<Vec<_>>(), [800, 600]);
        // the program, game.json, the scene, the sound, the two footstep sounds of the multi-file SoundFile, the model and notes.txt (a string that happens to name a file is a reference to it)
        assert_eq!(built.files, 8);
        assert!(built.bytes > 0 && built.missing.is_empty());
        // paths that leave the folder are not followed
        let evil = r#"{"entities":[{"uid":1,"components":{"X":{"f":"../outside.txt","g":"C:/Windows/win.ini"}}}]}"#;
        let files = referenced_files(&lunaris_json::parse(evil).unwrap(), &root);
        assert!(files.is_empty(), "{files:?}");
        // a missing player is explained
        assert!(build(&root, scene, "s.json", &root.join("nope.exe"), &out.join("x.exe"), &ProjectSettings::default()).unwrap_err().contains("cargo build"));
        assert!(build(&root, "{bad", "s.json", &player, &out.join("x.exe"), &ProjectSettings::default()).unwrap_err().contains("not valid JSON"));
    }
}
