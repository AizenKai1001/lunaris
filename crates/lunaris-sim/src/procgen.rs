//! Procedural content on demand: a **provider** the program installs makes files (models), prefab definitions and the zones of a grid cell from names alone, so that a world of any size is a handful of
//! entities and the rest is generated as the camera comes near (a city whose every building can be walked into, `docs/REALWORLD.md`).
//!
//! - `file(path)`: the bytes of a model that does not exist on disk (the loaders ask here first for paths the provider claims, e.g. `city/gen_shell_7_3_-2.glb`). Both the renderer's loader and the physics reader do,
//!   so what you see is what you stand on.
//! - `prefab(name)`: the JSON text of a prefab named `gen:...`, spawned by a `StreamZone` like any other (the entities it makes are plain entities in the world).
//! - `zones(grid, i, j)`: the `StreamZone`s of cell (i, j) of a grid named by a `ProcGrid` entity. The cells within `ProcGrid.radius` of the camera have their zones; the others have none.
//!
//! A provider must be a pure function of its arguments (the same name gives the same bytes and the same zones) and cheap to call from several threads at once.
use std::sync::{Arc, RwLock};

/// A `StreamZone` a provider asks for: where it is and what it loads.
#[derive(Clone, Debug, Default)]
pub struct ZoneDef {
    pub name: String,
    pub pos: [f32; 3],
    pub prefab: String,
    pub radius: f32,
    pub min_radius: f32,
    pub hysteresis: f32,
    pub prefetch: f32,
    pub vertical: f32,
    pub assets: String,
}

pub trait Provider: Send + Sync {
    /// Is `path` a file this provider makes?
    fn claims(&self, path: &str) -> bool;
    /// The bytes of the model file `path` (relative to the workspace), or None when this provider does not make it. Makes them (and waits) when they are not made yet.
    fn file(&self, path: &str) -> Option<Result<Vec<u8>, String>>;
    /// Whether the bytes are already made (so that a caller that must not wait can ask for them later).
    fn file_ready(&self, path: &str) -> bool;
    /// Start making `path` in the background (a no-op for a path that is not ours or is already made or being made).
    fn prefetch_file(&self, path: &str);
    /// The definition (JSON text) of the prefab `name`, or None when it is not ours.
    fn prefab(&self, name: &str) -> Option<String>;
    /// The zones of cell (i, j) of the grid `grid`.
    fn zones(&self, grid: &str, i: i32, j: i32) -> Vec<ZoneDef>;
}

static PROVIDER: RwLock<Option<Arc<dyn Provider>>> = RwLock::new(None);

/// Install the provider (the program does this once, before a world that uses `gen:` names or a `ProcGrid` runs).
pub fn set_provider(p: Option<Arc<dyn Provider>>) {
    *PROVIDER.write().expect("procgen provider lock") = p;
}

pub fn provider() -> Option<Arc<dyn Provider>> {
    PROVIDER.read().expect("procgen provider lock").clone()
}

/// The bytes of a procedural file, when a provider makes it.
pub fn file(path: &str) -> Option<Result<Vec<u8>, String>> {
    provider()?.file(path)
}

/// Tests that install a provider take this lock: the provider is one for the whole program.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl Provider for Fake {
        fn claims(&self, path: &str) -> bool {
            path.starts_with("gen/")
        }
        fn file(&self, path: &str) -> Option<Result<Vec<u8>, String>> {
            path.starts_with("gen/").then(|| Ok(path.as_bytes().to_vec()))
        }
        fn file_ready(&self, _: &str) -> bool {
            true
        }
        fn prefetch_file(&self, _: &str) {}
        fn prefab(&self, name: &str) -> Option<String> {
            name.starts_with("gen:").then(|| r#"{"components":{"Transform":{}}}"#.to_string())
        }
        fn zones(&self, _: &str, i: i32, j: i32) -> Vec<ZoneDef> {
            vec![ZoneDef { name: format!("cell {i} {j}"), pos: [i as f32 * 10.0, 0.0, j as f32 * 10.0], prefab: "gen:x".into(), radius: 8.0, ..Default::default() }]
        }
    }

    #[test]
    fn a_provider_claims_the_names_it_makes_and_leaves_the_rest() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_provider(Some(Arc::new(Fake)));
        assert_eq!(file("gen/a.glb").unwrap().unwrap(), b"gen/a.glb".to_vec());
        assert!(file("models/a.glb").is_none());
        assert!(provider().unwrap().prefab("gen:cell").is_some() && provider().unwrap().prefab("House").is_none());
        set_provider(None);
        assert!(file("gen/a.glb").is_none());
    }
}
