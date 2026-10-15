//! Changing scenes from inside the game (Unity's `SceneManager.LoadScene`, Unreal's `OpenLevel`): a `SceneSwitch` entity names a scene file (`path`); when a graph
//! system or a state machine sets its `go` to true, at the end of that tick the engine replaces the world with that scene. A main menu, a door to the next level, a
//! game-over screen that restarts: each is a button or a contact that sets `SceneSwitch.go` on an entity that carries the path.
//!
//! What goes and what stays: every entity goes except those with a `Persist` component (the player, the score board, the music: "do not destroy on load"); the graph
//! systems and prefabs of the old scene go too unless `keep_systems` (the new scene brings its own: put the shared systems in each scene file, or keep them); the
//! resources (the score, the time of day) stay when `keep_resources` is on (the default), and win over the new scene's own defaults of the same names. The tick
//! and the random numbers go on. A scene that cannot be read or parsed changes nothing and sets `scene.error` to 1. An entity of the new scene whose number is a persistent
//! entity's is **that entity**: the persistent one wins and the duplicate is skipped (the resource `scene.skipped` counts them), so the scene that holds the music can be
//! switched back to. A `SceneReset` beside the `SceneSwitch` names resources that take the new scene's value instead of the carried one. `scene.loaded` is set to the tick of the
//! last switch and the event `scene_loaded` is emitted. Replays and networked games switch at the same tick, because the switch is an outcome of the simulation itself (the
//! scene files must be the same on every machine).
use crate::components::{Persist, SceneReset, SceneSwitch};
use crate::sim::Sim;
use lunaris_ecs::Entity;
use lunaris_json::Json;
use std::collections::BTreeSet;

/// The resource names a `SceneReset` lists: separated by spaces or commas; a name ending in `*` stands for every resource that starts so.
pub fn reset_names(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

fn reset_matches(names: &[String], resource: &str) -> bool {
    names.iter().any(|n| match n.strip_suffix('*') {
        Some(prefix) => resource.starts_with(prefix),
        None => n == resource,
    })
}

impl Sim {
    pub(crate) fn scene_requests(&mut self) {
        let mut asked: Vec<(u64, Entity, SceneSwitch)> = Vec::new();
        let _ = self.world.for_each_uid::<(&SceneSwitch,), _>(|e, uid, (s,)| {
            if s.go {
                asked.push((uid, e, *s));
            }
        });
        let Some((_, e, req)) = asked.into_iter().min_by_key(|a| a.0) else { return };
        if let Some(s) = self.world.get_mut::<SceneSwitch>(e) {
            s.go = false;
        }
        let reset = self.world.get::<SceneReset>(e).map(|r| reset_names(self.world.strings().get(r.resources.0))).unwrap_or_default();
        match self.switch_scene(&req, &reset) {
            Ok(()) => {}
            Err(why) => {
                let _ = self.world.set_resource("scene.error", Some(1.0));
                self.scripts.events.push(("scene_error".to_string(), 0, None));
                let _ = why;
            }
        }
    }

    fn switch_scene(&mut self, req: &SceneSwitch, reset: &[String]) -> Result<(), String> {
        let root = self.asset_root.clone().ok_or("no workspace folder")?;
        let path = self.world.strings().get(req.path.0).to_string();
        let rel = lunaris_json::safe_relative(&path).map_err(|e| format!("bad path `{path}`: {e}"))?;
        let text = std::fs::read_to_string(root.join(&rel)).map_err(|e| format!("cannot read `{rel}`: {e}"))?;
        let mut scene = lunaris_json::parse(&text).map_err(|e| format!("`{rel}` is not valid JSON: {e}"))?;
        // the entities that stay, and whether the new scene would clash with them
        let mut keep: BTreeSet<u64> = BTreeSet::new();
        let _ = self.world.for_each_uid::<(&Persist,), _>(|_, uid, (p,)| {
            if p.keep {
                keep.insert(uid);
            }
        });
        // an entity of the new scene that has a persistent entity's number is that entity: the persistent one wins, the duplicate is left out
        let mut skipped = 0usize;
        if let Json::Obj(pairs) = &mut scene {
            for (key, value) in pairs.iter_mut() {
                if let (true, Json::Arr(list)) = (key == "entities", value) {
                    let before = list.len();
                    list.retain(|en| !en.get("uid").and_then(|u| u.as_u64()).is_some_and(|u| keep.contains(&u)));
                    skipped = before - list.len();
                }
            }
        }
        let declared: BTreeSet<String> = match scene.get("resources") {
            Some(Json::Obj(res)) => res.iter().map(|(k, _)| k.clone()).collect(),
            _ => BTreeSet::new(),
        };
        let old_resources = if req.keep_resources { self.world.resources().clone() } else { Default::default() };
        // out with the old
        let doomed: Vec<Entity> = self.world.entities_by_uid().into_iter().filter(|e| self.world.uid_of(*e).is_some_and(|u| !keep.contains(&u))).collect();
        for e in doomed {
            self.world.despawn(e);
        }
        if !req.keep_systems {
            self.scripts.systems.clear();
            self.scripts.prefabs.clear();
            self.scripts.bt.clear();
        }
        // in with the new, then the resources that were carried over
        let result = self.load_scene(&scene);
        for (k, v) in old_resources {
            if !reset_matches(reset, &k) {
                let _ = self.world.set_resource(&k, Some(v));
            }
        }
        // what a `SceneReset` names is what the new scene says: gone when it says nothing
        if !reset.is_empty() {
            let stale: Vec<String> = self.world.resources().keys().filter(|k| reset_matches(reset, k) && !declared.contains(*k)).cloned().collect();
            for k in stale {
                let _ = self.world.set_resource(&k, None);
            }
        }
        // (written only by a game that has met it, so a scene that never repeats a persistent number keeps its state as it was)
        if skipped > 0 || self.world.resource("scene.skipped").is_some() {
            let _ = self.world.set_resource("scene.skipped", Some(skipped as f64));
        }
        let _: Json = scene;
        result.map_err(|e| format!("loading `{rel}`: {e}"))?;
        self.heightfields = Default::default();
        self.terrain_errors.clear();
        self.load_terrains();
        let _ = self.world.set_resource("scene.loaded", Some(self.tick as f64));
        let _ = self.world.set_resource("scene.error", Some(0.0));
        self.scripts.events.push(("scene_loaded".to_string(), 0, None));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::command::Command;
    use crate::Sim;
    use lunaris_json::parse as json;

    fn spawn(s: &mut Sim, uid: u64, comps: &str) {
        s.apply(&Command::Spawn { uid: Some(uid), components: json(comps).unwrap().as_object().unwrap().to_vec() }).unwrap();
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lunaris_scenes_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn count(s: &mut Sim, name: &str) -> usize {
        let mut syms: Vec<u32> = Vec::new();
        let _ = s.world.for_each_uid::<(&crate::components::Name,), _>(|_, _, (m,)| syms.push(m.value.0));
        syms.iter().filter(|x| s.world.strings().get(**x) == name).count()
    }

    #[test]
    fn a_switch_replaces_the_world_but_keeps_what_persists_and_the_resources() {
        let dir = scratch("switch");
        // the next level: two rocks, a score of its own that must not win, and a system
        let mut next = Sim::new(1);
        spawn(&mut next, 5, r#"{"Name":{"value":"rock"},"Transform":{}}"#);
        spawn(&mut next, 6, r#"{"Name":{"value":"rock"},"Transform":{}}"#);
        next.apply(&Command::SetResource { name: "score".into(), value: Some(0.0) }).unwrap();
        next.apply(&Command::SetResource { name: "level".into(), value: Some(2.0) }).unwrap();
        std::fs::write(dir.join("level2.json"), next.scene_json().to_string_pretty().unwrap()).unwrap();

        let mut s = Sim::new(1);
        s.asset_root = Some(dir.clone());
        spawn(&mut s, 1, r#"{"Name":{"value":"hero"},"Transform":{"pos":[1,2,3]},"Persist":{}}"#);
        spawn(&mut s, 2, r#"{"Name":{"value":"tree"},"Transform":{}}"#);
        spawn(&mut s, 3, r#"{"Name":{"value":"door"},"SceneSwitch":{"path":"level2.json"}}"#);
        s.apply(&Command::SetResource { name: "score".into(), value: Some(7.0) }).unwrap();
        s.step_n(3);
        assert_eq!((count(&mut s, "hero"), count(&mut s, "tree"), count(&mut s, "rock")), (1, 1, 0));
        // nothing happens until `go`
        s.apply(&Command::Set { uid: 3, component: "SceneSwitch".into(), value: json(r#"{"go":true}"#).unwrap() }).unwrap();
        s.step();
        assert_eq!((count(&mut s, "hero"), count(&mut s, "tree"), count(&mut s, "rock"), count(&mut s, "door")), (1, 0, 2, 0), "the hero stays, the tree and the door go, the rocks come");
        let hero = s.world.get::<crate::components::Transform>(s.world.entity(1).unwrap()).unwrap().pos;
        assert_eq!(hero, [1.0, 2.0, 3.0]);
        assert_eq!(s.world.resource("score"), Some(7.0), "the score carries over and wins over the new scene's 0");
        assert_eq!(s.world.resource("level"), Some(2.0), "and the new scene's own resources are there");
        assert_eq!(s.world.resource("scene.loaded"), Some(s.tick as f64));
        // the world goes on stepping
        s.step_n(5);
        assert_eq!(count(&mut s, "rock"), 2);
        assert!(s.events.iter().any(|e| e.name == "scene_loaded"), "the event reaches the log on the next tick");
    }

    #[test]
    fn a_scene_that_cannot_be_loaded_changes_nothing() {
        let dir = scratch("bad");
        std::fs::write(dir.join("broken.json"), "{ nope").unwrap();
        for (path, persist) in [("missing.json", ""), ("broken.json", ""), ("../escape.json", "")] {
            let mut s = Sim::new(1);
            s.asset_root = Some(dir.clone());
            spawn(&mut s, 1, &format!(r#"{{"Name":{{"value":"hero"}},"Transform":{{}}{persist}}}"#));
            spawn(&mut s, 2, &format!(r#"{{"Name":{{"value":"door"}},"SceneSwitch":{{"path":"{path}","go":true}}}}"#));
            s.step();
            assert_eq!((count(&mut s, "hero"), count(&mut s, "door")), (1, 1), "{path}: the world is as it was");
            assert_eq!(s.world.resource("scene.error"), Some(1.0), "{path}");
            assert!(!s.world.get::<crate::components::SceneSwitch>(s.world.entity(2).unwrap()).unwrap().go, "the request is not repeated every tick");
        }
    }

    #[test]
    fn systems_go_with_the_old_scene_unless_kept() {
        let dir = scratch("systems");
        let mut next = Sim::new(1);
        spawn(&mut next, 9, r#"{"Name":{"value":"rock"},"Transform":{}}"#);
        std::fs::write(dir.join("next.json"), next.scene_json().to_string_pretty().unwrap()).unwrap();
        let sys = r#"{"name":"drift","each":["Transform"],"do":[{"set":"Transform.pos[0]","to":{"add":[{"get":"Transform.pos[0]"},1]}}]}"#;
        for keep in [false, true] {
            let mut s = Sim::new(1);
            s.asset_root = Some(dir.clone());
            s.apply(&Command::DefineSystem { def: json(sys).unwrap() }).unwrap();
            spawn(&mut s, 3, &format!(r#"{{"Name":{{"value":"door"}},"SceneSwitch":{{"path":"next.json","go":true,"keep_systems":{keep}}}}}"#));
            s.step();
            s.step_n(4);
            let x = s.world.get::<crate::components::Transform>(s.world.entity(9).unwrap()).unwrap().pos[0];
            assert_eq!(x > 0.5, keep, "keep_systems {keep}: the rock drifted to {x}");
        }
    }

    /// A boot scene holds the music (uid 1, persistent) and goes to the title; the title and the game both repeat the music, as a scene that is played on its own would.
    #[test]
    fn a_scene_that_repeats_a_persistent_entity_can_be_switched_to_and_the_persistent_one_wins() {
        let dir = scratch("persist_back");
        let scene_with = |entities: &[(u64, &str)]| {
            let mut s = Sim::new(1);
            for (uid, comps) in entities {
                spawn(&mut s, *uid, comps);
            }
            s.scene_json().to_string_pretty().unwrap()
        };
        let music = r#"{"Name":{"value":"music"},"Transform":{"pos":[9,9,9]},"Persist":{}}"#;
        std::fs::write(dir.join("title.json"), scene_with(&[(1, music), (2, r#"{"Name":{"value":"title text"},"Transform":{}}"#), (3, r#"{"Name":{"value":"play"},"SceneSwitch":{"path":"game.json"}}"#)])).unwrap();
        std::fs::write(dir.join("game.json"), scene_with(&[(1, music), (5, r#"{"Name":{"value":"level"},"Transform":{}}"#), (6, r#"{"Name":{"value":"menu door"},"SceneSwitch":{"path":"title.json"}}"#)])).unwrap();
        let mut s = Sim::new(1);
        s.asset_root = Some(dir.clone());
        spawn(&mut s, 1, r#"{"Name":{"value":"music"},"Transform":{"pos":[1,2,3]},"Persist":{}}"#);
        spawn(&mut s, 3, r#"{"Name":{"value":"boot door"},"SceneSwitch":{"path":"title.json","go":true}}"#);
        s.step();
        assert_eq!(s.world.resource("scene.error"), Some(0.0), "the title holds the music too, and that is not an error");
        assert_eq!(s.world.resource("scene.skipped"), Some(1.0), "one entity of the title was the persistent one");
        let at = |s: &Sim| s.world.get::<crate::components::Transform>(s.world.entity(1).unwrap()).unwrap().pos;
        assert_eq!(at(&s), [1.0, 2.0, 3.0], "the persistent entity keeps its own state, not the title's copy");
        assert_eq!((count(&mut s, "music"), count(&mut s, "title text")), (1, 1));
        // to the game, and back to the title (which holds the persistent music again), and on, as often as you like
        for (door, there) in [(3u64, "level"), (6, "title text"), (3, "level")] {
            s.apply(&Command::Set { uid: door, component: "SceneSwitch".into(), value: json(r#"{"go":true}"#).unwrap() }).unwrap();
            s.step();
            assert_eq!((s.world.resource("scene.error"), count(&mut s, "music"), count(&mut s, there)), (Some(0.0), 1, 1), "through door {door}");
        }
        assert_eq!(at(&s), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn a_scene_reset_names_the_resources_that_take_the_new_scenes_value() {
        let dir = scratch("reset");
        let mut next = Sim::new(1);
        spawn(&mut next, 5, r#"{"Name":{"value":"rock"},"Transform":{}}"#);
        next.apply(&Command::SetResource { name: "paused".into(), value: Some(0.0) }).unwrap();
        next.apply(&Command::SetResource { name: "score".into(), value: Some(0.0) }).unwrap();
        next.apply(&Command::SetResource { name: "fx.glow".into(), value: Some(3.0) }).unwrap();
        std::fs::write(dir.join("next.json"), next.scene_json().to_string_pretty().unwrap()).unwrap();
        for (keep_resources, reset, want) in [
            // (what is carried, what the reset names, then: paused, leave, time.scale, score, fx.glow, fx.old)
            (true, "", [Some(1.0), Some(1.0), Some(0.0), Some(7.0), Some(1.0), Some(1.0)]),
            (true, "paused leave time.scale", [Some(0.0), None, None, Some(7.0), Some(1.0), Some(1.0)]),
            (true, "paused, leave,  time.scale fx.*", [Some(0.0), None, None, Some(7.0), Some(3.0), None]),
            (false, "leave fx.old", [Some(0.0), None, Some(0.0), Some(0.0), Some(3.0), None]),
        ] {
            let mut s = Sim::new(1);
            s.asset_root = Some(dir.clone());
            let door = format!(r#"{{"Name":{{"value":"door"}},"SceneSwitch":{{"path":"next.json","go":true,"keep_resources":{keep_resources}}}{}}}"#, if reset.is_empty() { String::new() } else { format!(r#","SceneReset":{{"resources":"{reset}"}}"#) });
            spawn(&mut s, 3, &door);
            for (name, v) in [("paused", 1.0), ("leave", 1.0), ("time.scale", 0.0), ("score", 7.0), ("fx.glow", 1.0), ("fx.old", 1.0)] {
                s.apply(&Command::SetResource { name: name.into(), value: Some(v) }).unwrap();
            }
            s.step();
            let got: Vec<Option<f64>> = ["paused", "leave", "time.scale", "score", "fx.glow", "fx.old"].iter().map(|n| s.world.resource(n)).collect();
            // (a switch that does not keep the resources leaves the old ones that nothing named where they were: only the new scene's values win)
            assert_eq!(got, want.to_vec(), "keep_resources {keep_resources}, reset `{reset}`");
            assert_eq!(s.world.resource("scene.error"), Some(0.0));
            assert_eq!(s.world.resource("scene.skipped"), None, "a scene that repeats no persistent entity does not even get the resource");
        }
    }

    #[test]
    fn reset_names_split_on_spaces_and_commas_and_stars_are_prefixes() {
        assert_eq!(super::reset_names("a b,c,,  d.e*"), ["a", "b", "c", "d.e*"]);
        let names = super::reset_names("paused fx.*");
        assert!(super::reset_matches(&names, "paused") && super::reset_matches(&names, "fx.glow") && super::reset_matches(&names, "fx."));
        assert!(!super::reset_matches(&names, "paused2") && !super::reset_matches(&names, "fx") && !super::reset_matches(&names, "score"));
    }
}
