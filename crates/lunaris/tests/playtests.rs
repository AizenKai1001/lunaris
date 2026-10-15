//! The sample playtests in examples/tests must keep passing through the real command (`lunaris verify`), and a spec that fails must
//! make the command exit with an error and say which expectation failed.
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn every_sample_playtest_passes() {
    let root = repo_root();
    let mut specs: Vec<PathBuf> = std::fs::read_dir(root.join("examples").join("tests"))
        .expect("examples/tests exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".test.json"))
        .collect();
    specs.sort();
    assert!(specs.len() >= 3, "the sample playtests are missing: {specs:?}");
    for spec in specs {
        let out = Command::new(env!("CARGO_BIN_EXE_lunaris")).current_dir(&root).arg("verify").arg(&spec).output().expect("run lunaris verify");
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(out.status.success(), "{}:\n{text}\n{}", spec.display(), String::from_utf8_lossy(&out.stderr));
        assert!(text.contains(" 0 failed"), "{text}");
    }
}

#[test]
fn a_failing_playtest_exits_with_an_error_and_names_the_expectation() {
    let root = repo_root();
    let dir = std::env::temp_dir().join(format!("lunaris-playtest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let spec = dir.join("wrong.test.json");
    std::fs::write(&spec, r#"{"name":"wrong on purpose","steps":[{"step":5},{"expect":{"tick":true,"eq":99,"message":"the clock should say 99"}}]}"#).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lunaris")).current_dir(&root).arg("verify").arg(&spec).output().expect("run lunaris verify");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(!out.status.success(), "a failing spec must exit with an error:\n{text}");
    assert!(text.contains("FAIL") && text.contains("the clock should say 99") && text.contains("got 5"), "{text}");
    // a missing file is a usage error (exit code 2), not a pass
    let out = Command::new(env!("CARGO_BIN_EXE_lunaris")).current_dir(&root).args(["verify", "no/such/spec.json"]).output().expect("run lunaris verify");
    assert_eq!(out.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}
