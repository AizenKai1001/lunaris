//! Lunaris Script text with damage: the text of every graph system of every example (`get_script`) has characters taken out, repeated, swapped, replaced and joined with junk words, brackets and numbers
//! at the ends of their ranges, and is given to `compile_script` and `define_script`. A mistake has to come back as an error with a line and a column (docs/SCRIPT.md); a panic, a hang or a runaway
//! allocation in the lexer, the parser or the compiler is a bug.
use crate::scene_roundtrip_tests::scenes_under;
use crate::Engine;
use lunaris_json::Json;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const JUNK: [&str; 24] = ["", " ", "\n", "{", "}", "[", "]", "(", ")", ";", ",", "=", "==", "+", "-", "*", "/", "9999999999999999999999", "-0", "1e999", "nan", "\"", "//", "system each"];

/// The text with a few characters (not bytes: the text stays valid UTF-8) damaged.
fn damage(text: &str, rng: &mut Rng) -> String {
    let mut c: Vec<char> = text.chars().collect();
    if c.is_empty() {
        return String::new();
    }
    for _ in 0..1 + rng.below(4) {
        if c.is_empty() {
            break;
        }
        let i = rng.below(c.len());
        match rng.below(8) {
            0 => {
                c.remove(i);
            }
            1 => c.insert(i, c[i]),
            2 => {
                let j = rng.below(c.len());
                c.swap(i, j);
            }
            3 => c[i] = (rng.below(95) as u8 + 32) as char,
            4 => {
                let junk: Vec<char> = JUNK[rng.below(JUNK.len())].chars().collect();
                c.splice(i..i, junk);
            }
            5 => c.truncate(i),
            6 => {
                // a stretch repeated: deep nesting and long lines
                let len = (1 + rng.below(40)).min(c.len() - i);
                let piece: Vec<char> = c[i..i + len].to_vec();
                for _ in 0..1 + rng.below(30) {
                    c.splice(i..i, piece.clone());
                }
            }
            _ => c[i] = '\u{0}',
        }
    }
    c.into_iter().collect()
}

fn damage_the_scripts(rounds: usize) -> (usize, usize, Vec<String>) {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut scenes = Vec::new();
    scenes_under(&repo.join("examples"), &mut scenes);
    let (mut systems, mut runs, mut bad) = (0usize, 0usize, Vec::<String>::new());
    for path in &scenes {
        let rel = path.strip_prefix(&repo).unwrap().to_string_lossy().replace('\\', "/");
        let mut e = Engine::new(1, &repo);
        if e.call("load_scene", &Json::obj([("path", Json::str(rel.clone()))])).is_err() {
            continue;
        }
        // (a second engine with the same scene for `define_script`: a definition that is accepted changes the world, and it is this one that changes)
        let mut scratch = Engine::new(1, &repo);
        let _ = scratch.call("load_scene", &Json::obj([("path", Json::str(rel.clone()))]));
        let names: Vec<String> = e.call("systems", &Json::Null).unwrap().get("graph").and_then(|s| s.as_array()).unwrap_or(&[]).iter().filter_map(|s| s.get("name").and_then(|n| n.as_str()).map(str::to_string)).collect();
        for name in names {
            let Ok(got) = e.call("get_script", &Json::obj([("name", Json::str(name.clone()))])) else { continue };
            let source = got.get("source").and_then(|s| s.as_str()).unwrap_or("").to_string();
            systems += 1;
            let mut rng = Rng(0x5C21 ^ (rel.len() as u64) << 20 ^ source.len() as u64);
            for round in 0..rounds {
                let text = damage(&source, &mut rng);
                runs += 1;
                let r = catch_unwind(AssertUnwindSafe(|| {
                    let _ = e.call("compile_script", &Json::obj([("source", Json::str(text.clone()))]));
                    let _ = scratch.call("define_script", &Json::obj([("source", Json::str(text.clone()))]));
                }));
                if let Err(p) = r {
                    let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "a panic".to_string());
                    bad.push(format!("{rel} {name} round {round}: {}\n      text: {:?}", msg.lines().next().unwrap_or("").chars().take(140).collect::<String>(), text.chars().take(160).collect::<String>()));
                }
            }
        }
    }
    (systems, runs, bad)
}

/// A short run for every build: 6 damaged copies of every system of every example.
#[test]
fn damaged_script_text_gives_errors_and_not_panics_a_short_run() {
    let (systems, runs, bad) = damage_the_scripts(6);
    assert!(systems > 100 && runs > 600, "{systems} systems, {runs} damaged texts");
    assert!(bad.is_empty(), "{} panics in {runs} damaged texts:\n{}", bad.len(), bad.iter().take(8).cloned().collect::<Vec<_>>().join("\n"));
}

/// Many more: `LUNARIS_FUZZ_ROUNDS` (default 200) damaged copies of every system. `cargo test --release -p lunaris-api damaged_script_text -- --ignored --nocapture`
#[test]
#[ignore]
fn damaged_script_text_gives_errors_and_not_panics() {
    let rounds: usize = std::env::var("LUNARIS_FUZZ_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let (systems, runs, bad) = damage_the_scripts(rounds);
    println!("{systems} systems, {runs} damaged texts, {} panics", bad.len());
    for b in bad.iter().take(30) {
        println!("  {b}");
    }
    assert!(bad.is_empty(), "{} panics", bad.len());
}
