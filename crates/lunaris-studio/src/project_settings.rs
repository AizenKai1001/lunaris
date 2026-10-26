//! Project Settings: what a game made in this workspace is, apart from its scenes: its name, the scene it starts in, its quality, the size of its window and the seed of its
//! simulation. Kept in `project.lunaris.json` at the root of the workspace (plain text, next to the scenes, so it goes with the project into version control), edited in the
//! Project Settings window (Edit menu) and written into the `game.json` that Build Windows Player makes.
use lunaris_json::Json;
use std::path::Path;

/// The file in the workspace root.
pub const FILE: &str = "project.lunaris.json";
/// The render qualities a built game may ask for.
pub const QUALITIES: [&str; 5] = ["classic", "low", "medium", "high", "ultra"];
/// The window sizes a player accepts.
pub const SIZE_RANGE: std::ops::RangeInclusive<u32> = 64..=4096;

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSettings {
    /// The game's name (its program and its window): empty means the name of the scene.
    pub name: String,
    /// The scene the game starts in, as a path in the workspace: empty means the scene that is open when the game is built.
    pub scene: String,
    pub quality: String,
    pub size: (u32, u32),
    /// The seed of the game's simulation.
    pub seed: u32,
}

impl Default for ProjectSettings {
    fn default() -> ProjectSettings {
        ProjectSettings { name: String::new(), scene: String::new(), quality: "high".to_string(), size: (1280, 720), seed: 1 }
    }
}

impl ProjectSettings {
    /// The settings of the project in `root`; a missing or unreadable file gives the defaults (a new project has none).
    pub fn load(root: &Path) -> ProjectSettings {
        std::fs::read_to_string(root.join(FILE)).ok().and_then(|t| lunaris_json::parse(&t).ok()).map(|j| ProjectSettings::from_json(&j)).unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let text = self.to_json().to_string_pretty().map_err(|e| e.to_string())?;
        std::fs::write(root.join(FILE), text).map_err(|e| format!("cannot write {FILE}: {e}"))
    }

    pub fn to_json(&self) -> Json {
        Json::obj([
            ("lunaris_project", Json::from(1)),
            ("name", Json::str(self.name.clone())),
            ("scene", Json::str(self.scene.clone())),
            ("quality", Json::str(self.quality.clone())),
            ("size", Json::arr([Json::from(self.size.0 as i64), Json::from(self.size.1 as i64)])),
            ("seed", Json::from(self.seed as i64)),
        ])
    }

    /// Settings from a document: what is missing or out of range keeps its default (the window size is kept inside what a player accepts, an unknown quality is "high").
    pub fn from_json(j: &Json) -> ProjectSettings {
        let d = ProjectSettings::default();
        let text = |k: &str| j.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let size = j.get("size").and_then(|s| s.as_array()).and_then(|a| Some((a.first()?.as_u64()?, a.get(1)?.as_u64()?)));
        ProjectSettings {
            name: text("name").unwrap_or(d.name),
            scene: text("scene").unwrap_or(d.scene),
            quality: text("quality").filter(|q| QUALITIES.contains(&q.as_str())).unwrap_or(d.quality),
            size: size.map(|(w, h)| ((w.min(u32::MAX as u64) as u32).clamp(*SIZE_RANGE.start(), *SIZE_RANGE.end()), (h.min(u32::MAX as u64) as u32).clamp(*SIZE_RANGE.start(), *SIZE_RANGE.end()))).unwrap_or(d.size),
            seed: j.get("seed").and_then(|v| v.as_u64()).map(|s| s.min(u32::MAX as u64) as u32).unwrap_or(d.seed),
        }
    }

    /// The `game.json` a built game carries (the player reads `name`, `scene`, `quality`, `size` and `seed` from it): `name` is the one the game was built under and
    /// `scene` the path the scene has inside the game.
    pub fn game_config(&self, name: &str, scene: &str) -> Json {
        Json::obj([
            ("name", Json::str(name)),
            ("scene", Json::str(scene)),
            ("quality", Json::str(self.quality.clone())),
            ("size", Json::arr([Json::from(self.size.0 as i64), Json::from(self.size.1 as i64)])),
            ("seed", Json::from(self.seed as i64)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("lunaris-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn settings_round_trip_through_the_file_and_a_new_project_has_the_defaults() {
        let root = scratch("round");
        assert_eq!(ProjectSettings::load(&root), ProjectSettings::default(), "no file, the defaults");
        let s = ProjectSettings { name: "Coin Chase".into(), scene: "scenes/level1.lunaris.json".into(), quality: "low".into(), size: (1920, 1080), seed: 42 };
        s.save(&root).unwrap();
        assert!(root.join(FILE).is_file());
        assert_eq!(ProjectSettings::load(&root), s);
        // a file that is not JSON is not an error: the defaults come back
        std::fs::write(root.join(FILE), "{not json").unwrap();
        assert_eq!(ProjectSettings::load(&root), ProjectSettings::default());
    }

    #[test]
    fn out_of_range_and_unknown_values_fall_back_without_losing_the_rest() {
        let j = lunaris_json::parse(r#"{"name":"Game","quality":"ultra-deluxe","size":[10,99999],"seed":-5,"future_field":true}"#).unwrap();
        let s = ProjectSettings::from_json(&j);
        assert_eq!(s.name, "Game");
        assert_eq!(s.quality, "high", "an unknown quality is the default");
        assert_eq!(s.size, (64, 4096), "the size is kept to what a player accepts");
        assert_eq!(s.seed, 1, "a negative seed is not a seed");
        assert_eq!(s.scene, "");
        // a seed above 32 bits is cut to what the editor can show
        let j = lunaris_json::parse(r#"{"seed":9999999999}"#).unwrap();
        assert_eq!(ProjectSettings::from_json(&j).seed, u32::MAX);
    }

    #[test]
    fn the_game_config_has_what_the_player_reads() {
        let s = ProjectSettings { quality: "medium".into(), size: (800, 600), seed: 7, ..ProjectSettings::default() };
        let c = s.game_config("My Game", "scenes/main.lunaris.json");
        assert_eq!(c.get("name").and_then(|v| v.as_str()), Some("My Game"));
        assert_eq!(c.get("scene").and_then(|v| v.as_str()), Some("scenes/main.lunaris.json"));
        assert_eq!(c.get("quality").and_then(|v| v.as_str()), Some("medium"));
        assert_eq!(c.get("seed").and_then(|v| v.as_u64()), Some(7));
        let size: Vec<u64> = c.get("size").unwrap().as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
        assert_eq!(size, [800, 600]);
    }
}
