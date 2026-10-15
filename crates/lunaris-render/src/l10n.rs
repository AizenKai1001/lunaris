//! Texts in several languages: `{#key}` in a `Text` or `Ui` text is the entry of `lang/<language>.json`, chosen by a `Localization` entity.
#[cfg(test)]
mod tests {
    use crate::render::gather_scene;
    use crate::{RenderOptions, TextureCache};
    use lunaris_json::Json;

    fn world(texts: &[&str]) -> lunaris_sim::Sim {
        let mut s = lunaris_sim::Sim::new(1);
        for (i, t) in texts.iter().enumerate() {
            let Json::Obj(components) = lunaris_json::parse(&format!(r#"{{"Text":{{"value":"{t}"}}}}"#)).unwrap() else { panic!() };
            s.apply(&lunaris_sim::Command::Spawn { uid: Some(10 + i as u64), components }).unwrap();
        }
        s
    }

    #[test]
    fn keys_become_the_text_of_the_chosen_language_and_a_missing_key_shows_itself() {
        let dir = std::env::temp_dir().join(format!("lunaris-l10n-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("lang")).unwrap();
        std::fs::write(dir.join("lang").join("en.json"), r#"{"hello": "Hello", "menu": {"play": "Play"}}"#).unwrap();
        std::fs::write(dir.join("lang").join("de.json"), r#"{"hello": "Hallo", "menu": {"play": "Spielen"}}"#).unwrap();
        let mut s = world(&["{#hello}, score {score}", "{#menu.play}", "{#nothing.here}"]);
        s.world.set_resource("score", Some(3.0)).unwrap();
        let Json::Obj(components) = lunaris_json::parse(r#"{"Localization":{"language":"en"}}"#).unwrap() else { panic!() };
        s.apply(&lunaris_sim::Command::Spawn { uid: Some(1), components }).unwrap();
        let mut tex = TextureCache::new(Some(dir.clone()));
        let say = |s: &mut lunaris_sim::Sim, tex: &mut TextureCache| -> Vec<String> {
            let scene = gather_scene(&mut s.world, tex, &RenderOptions { width: 64, height: 48, ..RenderOptions::default() }).unwrap();
            scene.texts.iter().map(|t| t.text.clone()).collect()
        };
        assert_eq!(say(&mut s, &mut tex), vec!["Hello, score 3", "Play", "nothing.here"]);
        let e = s.world.entity(1).unwrap();
        let de = s.world.strings_mut().intern("de");
        s.world.get_mut::<lunaris_sim::components::Localization>(e).unwrap().language.0 = de;
        assert_eq!(say(&mut s, &mut tex), vec!["Hallo, score 3", "Spielen", "nothing.here"]);
        // a language without a file: the keys show, with a warning
        let xx = s.world.strings_mut().intern("xx");
        s.world.get_mut::<lunaris_sim::components::Localization>(e).unwrap().language.0 = xx;
        assert_eq!(say(&mut s, &mut tex), vec!["hello, score 3", "menu.play", "nothing.here"]);
        assert!(tex.warnings.iter().any(|w| w.contains("language `xx`")), "{:?}", tex.warnings);
    }

    #[test]
    fn a_translated_text_can_hold_placeholders_of_its_own() {
        let dir = std::env::temp_dir().join(format!("lunaris-l10n-holes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("lang")).unwrap();
        std::fs::write(dir.join("lang").join("en.json"), r#"{"stats": "Level {level}   Score {score}"}"#).unwrap();
        let mut s = world(&["{#stats}", "Coins {score}: {#stats}"]);
        s.world.set_resource("score", Some(30.0)).unwrap();
        s.world.set_resource("level", Some(2.0)).unwrap();
        let Json::Obj(components) = lunaris_json::parse(r#"{"Localization":{"language":"en"}}"#).unwrap() else { panic!() };
        s.apply(&lunaris_sim::Command::Spawn { uid: Some(1), components }).unwrap();
        let mut tex = TextureCache::new(Some(dir));
        let scene = gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 48, ..RenderOptions::default() }).unwrap();
        assert_eq!(scene.texts.iter().map(|t| t.text.clone()).collect::<Vec<_>>(), vec!["Level 2   Score 30", "Coins 30: Level 2   Score 30"]);
    }

    #[test]
    fn a_language_name_with_slashes_is_a_path_beside_the_game_and_cannot_leave_the_workspace() {
        use crate::texture::language_name_ok;
        for good in ["en", "de-AT", "village_en", "examples/depths/lang/en"] {
            assert!(language_name_ok(good), "{good}");
        }
        for bad in ["", "/etc/passwd", "../x", "a//b", "lang/", "a.b", "a b"] {
            assert!(!language_name_ok(bad), "{bad}");
        }
        let dir = std::env::temp_dir().join(format!("lunaris-l10n-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("game").join("lang")).unwrap();
        std::fs::write(dir.join("game").join("lang").join("en.json"), r#"{"play": "Play"}"#).unwrap();
        let mut s = world(&["{#play}"]);
        let Json::Obj(components) = lunaris_json::parse(r#"{"Localization":{"language":"game/lang/en"}}"#).unwrap() else { panic!() };
        s.apply(&lunaris_sim::Command::Spawn { uid: Some(1), components }).unwrap();
        let mut tex = TextureCache::new(Some(dir));
        let scene = gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 48, ..RenderOptions::default() }).unwrap();
        assert_eq!(scene.texts.iter().map(|t| t.text.clone()).collect::<Vec<_>>(), vec!["Play"]);
    }

    #[test]
    fn water_becomes_a_see_through_grid_with_wave_numbers_and_a_flat_one_without_waves() {
        let mut s = lunaris_sim::Sim::new(1);
        for (uid, comps) in [(1u64, r#"{"Transform":{"pos":[0,2,0]},"Water":{"size":[60,30],"wave_height":0.4,"wave_length":6,"phase":1.5,"color":[0,0.3,0.4,0.7]}}"#), (2, r#"{"Transform":{"pos":[0,0,0]},"Water":{"size":[10,10],"wave_height":0}}"#)] {
            let Json::Obj(components) = lunaris_json::parse(comps).unwrap() else { panic!() };
            s.apply(&lunaris_sim::Command::Spawn { uid: Some(uid), components }).unwrap();
        }
        let mut tex = TextureCache::new(None);
        let scene = gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 48, ..RenderOptions::default() }).unwrap();
        let (a, b) = (scene.meshes.iter().find(|m| m.uid == 1).unwrap(), scene.meshes.iter().find(|m| m.uid == 2).unwrap());
        assert_eq!(a.water, Some([0.4, 6.0, 1.5]));
        assert_eq!(b.water, None, "no waves: flat");
        assert!((a.alpha - 0.7).abs() < 1e-6 && a.roughness < 0.1 && !a.cast_shadow);
        let grid = a.mesh.get();
        assert!(grid.pos.len() > 100 && grid.idx.len() % 3 == 0 && grid.nrm.iter().all(|n| *n == [0.0, 1.0, 0.0]));
        // 60 m with 6 m waves: cells of at most a metre
        assert!(grid.pos.iter().all(|p| p[1] == 0.0 && p[0].abs() <= 0.5 && p[2].abs() <= 0.5));
        assert_eq!(a.model.transform_point([0.5, 0.0, 0.5]), [30.0, 2.0, 15.0]);
    }

    #[test]
    fn a_sway_reaches_the_scene_with_its_phase_and_offset_and_runs_with_the_simulation() {
        let mut s = lunaris_sim::Sim::new(1);
        let Json::Obj(components) = lunaris_json::parse(r#"{"Transform":{},"MeshRenderer":{"mesh":"cylinder"},"Sway":{"amount":0.5,"extent":4,"speed":1,"offset":2}}"#).unwrap() else { panic!() };
        s.apply(&lunaris_sim::Command::Spawn { uid: Some(1), components }).unwrap();
        s.step_n(30);
        let mut tex = TextureCache::new(None);
        let scene = gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 48, ..RenderOptions::default() }).unwrap();
        let sw = scene.meshes[0].sway.expect("a sway");
        assert!((sw[0] - 0.5).abs() < 1e-6 && (sw[1] - 4.0).abs() < 1e-6, "{sw:?}");
        // half a second at one cycle a second is a half turn, plus the offset
        assert!((sw[2] - (std::f32::consts::PI + 2.0)).abs() < 0.02, "{sw:?}");
        assert!(scene.meshes[0].water.is_none());
    }

    #[test]
    fn level_of_detail_swaps_the_model_by_distance_and_hides_far_things() {
        let mut s = lunaris_sim::Sim::new(1);
        for (uid, comps) in [(1u64, r#"{"Transform":{"pos":[0,0,0]},"Camera":{}}"#), (2, r#"{"Transform":{"pos":[0,0,-5]},"MeshRenderer":{"mesh":"sphere"},"Lod":{"mesh1":"cube","distance1":10,"distance2":1000,"cull":50}}"#)] {
            let Json::Obj(components) = lunaris_json::parse(comps).unwrap() else { panic!() };
            s.apply(&lunaris_sim::Command::Spawn { uid: Some(uid), components }).unwrap();
        }
        let mut tex = TextureCache::new(None);
        let sphere = crate::mesh::builtin("sphere").unwrap().pos.len();
        let cube = crate::mesh::builtin("cube").unwrap().pos.len();
        let mut look = |s: &mut lunaris_sim::Sim, z: f32| -> Option<usize> {
            s.world.get_mut::<lunaris_sim::components::Transform>(s.world.entity(2).unwrap()).unwrap().pos = [0.0, 0.0, -z];
            let scene = gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 48, ..RenderOptions::default() }).unwrap();
            scene.meshes.iter().find(|m| m.uid == 2).map(|m| m.mesh.get().pos.len())
        };
        assert_eq!(look(&mut s, 5.0), Some(sphere), "near: the model it has");
        assert_eq!(look(&mut s, 20.0), Some(cube), "from 10 m: the simpler one");
        assert_eq!(look(&mut s, 60.0), None, "beyond 50 m: not drawn");
    }
}
