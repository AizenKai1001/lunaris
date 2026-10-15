//! The offline mind demo from docs/MIND.md must keep working: the scripted model walks the body next to the Goal, and the
//! recorded session replays without the model.
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn scripted_mind_reaches_the_goal_and_replays_identically() {
    let root = repo_root();
    let dir = std::env::temp_dir().join(format!("lunaris-mind-demo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let transcript = dir.join("transcript.json");
    let out = Command::new(env!("CARGO_BIN_EXE_lunaris"))
        .current_dir(&root)
        .args(["mind", "examples/arena.lunaris.json", "--body", "1", "--goal", "reach the Goal", "--scripted", "examples/mind_walk.scripted.json", "--turns", "12", "--think-ticks", "30", "--transcript"])
        .arg(&transcript)
        .output()
        .expect("run lunaris mind");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "{text}\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("replay without the model: identical"), "{text}");

    let doc = std::fs::read_to_string(&transcript).unwrap();
    let doc = lunaris_json::parse(&doc).unwrap();
    let turns = doc.get("turns").and_then(|t| t.as_array()).unwrap();
    assert_eq!(turns.len(), 12);
    let last = turns.last().unwrap();
    let x = last.get("observation").and_then(|o| o.get("you")).and_then(|y| y.get("Transform")).and_then(|t| t.get("pos")).and_then(|p| p.index(0)).and_then(|x| x.as_f64()).unwrap();
    assert!((x - 8.5).abs() < 0.01, "the body should stop at x = 8.5, next to the Goal at 9, got {x}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_goal_given_as_data_stops_the_run_and_sets_the_exit_code() {
    let root = repo_root();
    let run = |turns: &str| {
        Command::new(env!("CARGO_BIN_EXE_lunaris"))
            .current_dir(&root)
            .args(["mind", "examples/arena.lunaris.json", "--body", "1", "--goal", "reach the Goal", "--scripted", "examples/mind_walk.scripted.json", "--turns", turns, "--think-ticks", "30", "--success", "examples/arena.success.json"])
            .output()
            .expect("run lunaris mind")
    };
    let reached = run("12");
    let text = String::from_utf8_lossy(&reached.stdout).to_string();
    assert!(reached.status.success(), "{text}");
    assert!(text.contains("success: the goal was reached after 6 turns"), "{text}");
    assert!(text.contains("replay without the model: identical"), "{text}");
    let missed = run("3");
    let text = String::from_utf8_lossy(&missed.stdout).to_string();
    assert!(!missed.status.success(), "a goal that was not reached must be a failing exit code:\n{text}");
    assert!(text.contains("NOT reached in 3 turns") && text.contains("NOT YET"), "{text}");
}
