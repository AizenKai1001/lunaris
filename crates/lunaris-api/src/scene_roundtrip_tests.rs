//! Saving a scene and loading it again must give the same world, for every example the repository ships: the real data that a hand-made scene in a test never has (a negative zero that lost its sign in the text, a field the writer
//! skipped, a system or a resource the save forgot).
use crate::Engine;
use lunaris_json::{parse, Json};
use std::path::{Path, PathBuf};

pub(crate) fn scenes_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            // (a probe folder some tools leave behind is not an example)
            if !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('_')) {
                scenes_under(&p, out);
            }
        } else if p.to_string_lossy().ends_with(".lunaris.json") {
            out.push(p);
        }
    }
}

fn hash_of(e: &mut Engine) -> String {
    e.call("hash", &Json::Null).unwrap().get("hash").unwrap().as_str().unwrap().to_string()
}

#[test]
fn every_example_scene_saved_and_loaded_again_is_the_same_world() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut scenes = Vec::new();
    scenes_under(&repo.join("examples"), &mut scenes);
    assert!(scenes.len() > 30, "only {} example scenes found under {}", scenes.len(), repo.display());
    let mut bad: Vec<String> = Vec::new();
    for path in &scenes {
        let rel = path.strip_prefix(&repo).unwrap().to_string_lossy().replace('\\', "/");
        let mut a = Engine::new(1, &repo);
        if let Err(e) = a.call("load_scene", &Json::obj([("path", Json::str(rel.clone()))])) {
            bad.push(format!("{rel}: does not load: {e}"));
            continue;
        }
        let first = hash_of(&mut a);
        let text = a.call("save_scene", &Json::Null).unwrap().to_string_pretty().unwrap();
        let mut b = Engine::new(1, &repo);
        let scene = match parse(&text) {
            Ok(s) => s,
            Err(e) => {
                bad.push(format!("{rel}: the saved text does not parse: {e}"));
                continue;
            }
        };
        if let Err(e) = b.call("load_scene", &Json::obj([("scene", scene)])) {
            bad.push(format!("{rel}: the saved text does not load: {e}"));
            continue;
        }
        let second = hash_of(&mut b);
        if first != second {
            bad.push(format!("{rel}: the state hash changed ({first} -> {second}), {} -> {} entities", a.sim.world.len(), b.sim.world.len()));
            continue;
        }
        // the second save is the same text: nothing is written differently once it has been read back
        let again = b.call("save_scene", &Json::Null).unwrap().to_string_pretty().unwrap();
        if again != text {
            let line = text.lines().zip(again.lines()).position(|(x, y)| x != y).map(|i| i + 1).unwrap_or(0);
            bad.push(format!("{rel}: a second save differs from the first (first difference at line {line})"));
        }
    }
    assert!(bad.is_empty(), "{} of {} example scenes do not survive save and load:\n{}", bad.len(), scenes.len(), bad.join("\n"));
}

/// What `check` says about every example scene (they are the repository's demonstrations: an error here is a mistake in an example or a rule that is wrong). Ignored: it prints the findings.
/// `cargo test --release -p lunaris-api every_example_scene_check -- --ignored --nocapture`
#[test]
#[ignore]
fn every_example_scene_check_findings() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut scenes = Vec::new();
    scenes_under(&repo.join("examples"), &mut scenes);
    let (mut errors, mut warnings, mut infos) = (0, 0, 0);
    for path in &scenes {
        let rel = path.strip_prefix(&repo).unwrap().to_string_lossy().replace('\\', "/");
        let mut e = Engine::new(1, &repo);
        if e.call("load_scene", &Json::obj([("path", Json::str(rel.clone()))])).is_err() {
            continue;
        }
        let out = e.call("check", &Json::Null).unwrap();
        let list = out.get("problems").and_then(|p| p.as_array()).map(|p| p.to_vec()).unwrap_or_default();
        for p in &list {
            let sev = p.get("severity").and_then(|s| s.as_str()).unwrap_or("?");
            match sev {
                "error" => errors += 1,
                "warning" => warnings += 1,
                _ => infos += 1,
            }
            let msg = p.get("message").and_then(|m| m.as_str()).unwrap_or("");
            println!("{rel}: [{sev}] {}{}", p.get("entity").map(|u| format!("#{} ", u.to_string_compact().unwrap_or_default())).unwrap_or_default(), msg.chars().take(200).collect::<String>());
        }
    }
    println!("{} scenes: {errors} errors, {warnings} warnings, {infos} notes", scenes.len());
}

/// How long each example takes to load, to check and to save (the slowest first): where scene loading is slow, which a small machine feels first. Ignored: it prints a table.
/// `cargo test --release -p lunaris-api every_example_scene_load_times -- --ignored --nocapture`
#[test]
#[ignore]
fn every_example_scene_load_times() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut scenes = Vec::new();
    scenes_under(&repo.join("examples"), &mut scenes);
    let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1e3;
    let mut rows: Vec<(f64, String)> = Vec::new();
    for path in &scenes {
        let rel = path.strip_prefix(&repo).unwrap().to_string_lossy().replace('\\', "/");
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let mut e = Engine::new(1, &repo);
        let t = std::time::Instant::now();
        if e.call("load_scene", &Json::obj([("path", Json::str(rel.clone()))])).is_err() {
            continue;
        }
        let load = ms(t);
        let n = e.sim.world.len();
        let t = std::time::Instant::now();
        let _ = e.call("check", &Json::Null);
        let check = ms(t);
        let t = std::time::Instant::now();
        let text = e.call("save_scene", &Json::Null).unwrap().to_string_pretty().unwrap();
        let save = ms(t);
        rows.push((load, format!("{rel}: {n} entities, {} KB file, load {load:.1} ms, check {check:.1} ms, save {save:.1} ms ({} KB)", bytes / 1024, text.len() / 1024)));
    }
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for (_, r) in rows.iter().take(25) {
        println!("{r}");
    }
}

/// What `check` costs on one scene, run again and again (so that a profiler can look at it): `LUNARIS_BIG_ROOT=/workspace LUNARIS_BIG_SCENE=scenes/x.lunaris.json cargo test --release -p lunaris-api check_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn check_cost_on_a_big_scene() {
    let (Ok(root), Ok(scene)) = (std::env::var("LUNARIS_BIG_ROOT"), std::env::var("LUNARIS_BIG_SCENE")) else { return };
    let mut e = Engine::new(1, Path::new(&root));
    e.call("load_scene", &Json::obj([("path", Json::str(scene))])).unwrap();
    let runs: usize = std::env::var("LUNARIS_RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
    let t = std::time::Instant::now();
    for _ in 0..runs {
        let _ = e.call("check", &Json::Null).unwrap();
    }
    println!("check: {:.1} ms each over {runs} runs ({} entities)", t.elapsed().as_secs_f64() * 1e3 / runs as f64, e.sim.world.len());
}
