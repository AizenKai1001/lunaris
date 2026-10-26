//! The endless city (`docs/REALWORLD.md`): a workspace with `city/city.json` (`{"seed": N}`, written by `citygen city`) gets the procedural city installed as the program's provider, so that its `ProcGrid`s and `gen:` prefabs work.
use std::path::Path;
use std::sync::Arc;

/// Install the city of the workspace `root` when it has one. `server` says that this program is a game server: it gets the city's collision shells (`Role::Server`: no pictures, no furniture) and not what a player sees; `city.json` may
/// say `"role": "visual"` or `"server"` to decide it itself.
pub fn install_if_present(root: &Path, server: bool, scene: Option<&str>) {
    use lunaris_citygen::runtime::Role;
    let Ok(text) = std::fs::read_to_string(root.join("city").join("city.json")) else { return };
    // a scene that has no `ProcGrid` has no use for a city (the workspace of a game may hold a city that only some of its scenes use): it is not made ready, which takes seconds
    if let Some(scene) = scene {
        let path = if Path::new(scene).is_absolute() { Path::new(scene).to_path_buf() } else { root.join(scene) };
        if std::fs::read_to_string(&path).is_ok_and(|t| !t.contains("\"ProcGrid\"")) {
            return;
        }
    }
    let doc = lunaris_json::parse(&text).ok();
    let seed = doc.as_ref().and_then(|d| d.get("seed").and_then(|s| s.as_i64())).unwrap_or(1) as u64;
    // `"game": true` says that this is the city of a multiplayer game: a server gets its collision, a player the pictures without the entities a server owns (`Role::Client`); without it the city is a single-player one
    let game = doc.as_ref().and_then(|d| d.get("game")).is_some_and(|g| matches!(g, lunaris_json::Json::Bool(true)));
    let role = match doc.as_ref().and_then(|d| d.get("role")).and_then(|r| r.as_str()) {
        Some("server") => Role::Server,
        Some("client") => Role::Client,
        Some("visual") => Role::Visual,
        _ if server => Role::Server,
        _ if game => Role::Client,
        _ => Role::Visual,
    };
    let t = std::time::Instant::now();
    // `"plaza": [i0, i1, j0, j1]`: blocks given over to a plaza (the middle of a game's map)
    let plaza = doc.as_ref().and_then(|d| d.get("plaza")).and_then(|p| match p {
        lunaris_json::Json::Arr(a) if a.len() == 4 => {
            let n: Vec<i32> = a.iter().filter_map(|v| v.as_i64()).map(|v| v as i32).collect();
            (n.len() == 4).then(|| (n[0], n[1], n[2], n[3]))
        }
        _ => None,
    });
    // `"server": {...}`: what the game makes of the doors and containers that a server owns (`lunaris_citygen::serverside`)
    let parts = doc.as_ref().and_then(|d| d.get("server")).map(lunaris_citygen::serverside::Parts::from_json).unwrap_or_default();
    // `"dome": R`: the city is round and ends at R metres (a wall and a dome stand there)
    let dome = doc.as_ref().and_then(|d| d.get("dome")).and_then(|v| v.as_f64()).map(|v| v as f32);
    // `"polar": true`: the streets are rings and radii (the round city of a game under a dome, `lunaris-citygen`'s `polar.rs`)
    let polar = doc.as_ref().and_then(|d| d.get("polar")).is_some_and(|g| matches!(g, lunaris_json::Json::Bool(true)));
    // `"ruins": [[band, sector], ...]`: blocks of the round city given over to rubble (a destroyed district)
    let ruins = doc.as_ref().map(lunaris_citygen::plan::ruins_from_json).unwrap_or_default();
    match lunaris_citygen::runtime::City::open_as(root, seed, role) {
        Ok(city) => {
            Arc::new(city.with_plaza(plaza).with_dome(dome).with_polar(polar).with_ruins(&ruins).with_parts(parts)).install();
            eprintln!("lunaris: the endless city (seed {seed}, {}) is ready in {:.1} s", match role { Role::Server => "collision shells for a server", Role::Client => "as a player of the game sees it", Role::Visual => "as a player sees it" }, t.elapsed().as_secs_f32());
        }
        Err(e) => eprintln!("lunaris: cannot open the city of {}: {e}", root.display()),
    }
}
