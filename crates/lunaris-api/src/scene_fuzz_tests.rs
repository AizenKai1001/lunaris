//! Real scenes with damaged numbers: a few numbers of an example scene are replaced by zero, a negative, a huge or a tiny value, NaN or infinity (what a hand-edited file, a bad generator or an agent will
//! eventually write), and the scene is loaded, stepped and checked. An error that explains itself is the right answer; a panic is a bug. The seeds are the repository's own examples, so every component and every
//! field a real scene uses is hit, not only those of a hand-made corpus (`fuzz_tests.rs`).
use crate::scene_roundtrip_tests::scenes_under;
use crate::Engine;
use lunaris_json::{parse, Json};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

/// Where the numbers are in a scene: the path (keys and indices) to each, with the key of the field that holds it.
fn numbers(j: &Json, path: &mut Vec<usize>, key: &str, out: &mut Vec<(Vec<usize>, String)>) {
    match j {
        Json::Int(_) | Json::Float(_) => out.push((path.clone(), key.to_string())),
        Json::Obj(pairs) => {
            for (i, (k, v)) in pairs.iter().enumerate() {
                path.push(i);
                numbers(v, path, k, out);
                path.pop();
            }
        }
        Json::Arr(a) => {
            for (i, v) in a.iter().enumerate() {
                path.push(i);
                numbers(v, path, key, out);
                path.pop();
            }
        }
        _ => {}
    }
}

/// The paths to every node below the root (not only the numbers).
fn all_nodes(j: &Json, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    match j {
        Json::Obj(pairs) => {
            for (i, (_, v)) in pairs.iter().enumerate() {
                path.push(i);
                out.push(path.clone());
                all_nodes(v, path, out);
                path.pop();
            }
        }
        Json::Arr(a) => {
            for (i, v) in a.iter().enumerate() {
                path.push(i);
                out.push(path.clone());
                all_nodes(v, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

/// Replace the node at `path` by something of another type, or take it out of its object or array.
fn restructure(doc: &mut Json, path: &[usize], rng: &mut Rng) -> String {
    let place = describe(doc, path);
    let (last, parent_path) = path.split_last().unwrap();
    let junk = [Json::Null, Json::Bool(true), Json::Str(String::new()), Json::Str("x".to_string()), Json::Int(-1), Json::Float(1e30), Json::Arr(Vec::new()), Json::Obj(Vec::new()), Json::Arr(vec![Json::Null, Json::Int(1)])];
    let remove = rng.below(3) == 0;
    let parent = at_mut(doc, parent_path);
    if remove {
        match parent {
            Json::Obj(p) => {
                p.remove(*last);
            }
            Json::Arr(a) => {
                a.remove(*last);
            }
            _ => {}
        }
        format!("{place} taken out")
    } else {
        let j = junk[rng.below(junk.len())].clone();
        let what = j.to_string_compact().unwrap_or_default();
        *at_mut(parent, &[*last]) = j;
        format!("{place} = {what}")
    }
}

fn at_mut<'a>(j: &'a mut Json, path: &[usize]) -> &'a mut Json {
    let mut cur = j;
    for &i in path {
        cur = match cur {
            Json::Obj(p) => &mut p[i].1,
            Json::Arr(a) => &mut a[i],
            _ => unreachable!("a path into a number"),
        };
    }
    cur
}

/// Where a path leads, in words: `entities[12].components.Collider.size[0]`.
fn describe(j: &Json, path: &[usize]) -> String {
    let (mut cur, mut out) = (j, String::new());
    for &i in path {
        match cur {
            Json::Obj(p) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(&p[i].0);
                cur = &p[i].1;
            }
            Json::Arr(a) => {
                out.push_str(&format!("[{i}]"));
                cur = &a[i];
            }
            _ => break,
        }
    }
    out
}

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
        (self.next() % n as u64) as usize
    }
}

const DAMAGE: [f64; 14] = [0.0, -1.0, 1.0, 0.5, 1e-9, -1e-9, 1e6, -1e6, 1e12, 1e30, -1e30, f64::NAN, f64::INFINITY, f64::NEG_INFINITY];

/// Load `doc` into an engine, step it and check it: Ok(()) whatever the answers, Err(message) when something panicked.
fn run_damaged(repo: &Path, doc: &Json, ticks: usize) -> Result<(), String> {
    catch_unwind(AssertUnwindSafe(|| {
        let trace = |what: &str| {
            if std::env::var_os("LUNARIS_FUZZ_TRACE").is_some() {
                eprintln!("FUZZ   ... {what}");
            }
        };
        let mut e = Engine::new(1, repo);
        trace("load_scene");
        if e.call("load_scene", &Json::obj([("scene", doc.clone())])).is_err() {
            return;
        }
        trace("check");
        let _ = e.call("check", &Json::Null);
        trace("step");
        let _ = e.call("step", &parse(&format!(r#"{{"ticks":{ticks},"budget_ms":20000}}"#)).unwrap());
        trace("check again");
        let _ = e.call("check", &Json::Null);
        trace("save_scene");
        let _ = e.call("save_scene", &Json::Null);
    }))
    .map_err(|p| p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "a panic".to_string()))
}

/// How many entities a scene has, from its text.
fn size_of(doc: &Json) -> usize {
    doc.get("entities").and_then(|e| e.as_array()).map(|a| a.len()).unwrap_or(0)
}

/// `rounds` mutations of every example scene of up to 450 entities, a few numbers each, each loaded, stepped `ticks` ticks and checked. Prints each panic with the number that caused it.
fn damage_the_examples(rounds: usize, ticks: usize) {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut scenes = Vec::new();
    scenes_under(&repo.join("examples"), &mut scenes);
    let mut seen: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let (mut runs, mut skipped) = (0usize, 0usize);
    for (n, path) in scenes.iter().enumerate() {
        let rel = path.strip_prefix(&repo).unwrap().to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(path).unwrap();
        let doc = parse(&text).unwrap();
        if size_of(&doc) > 450 {
            skipped += 1;
            continue;
        }
        let mut spots: Vec<(Vec<usize>, String)> = Vec::new();
        // (the numbers of the entities, and of the graph systems, the prefabs and the resources: the interpreter meets NaN and 1e30 too)
        numbers(&doc, &mut vec![], "", &mut spots);
        if spots.is_empty() {
            continue;
        }
        let mut nodes: Vec<Vec<usize>> = Vec::new();
        all_nodes(&doc, &mut vec![], &mut nodes);
        let mut rng = Rng(0x5EED ^ (n as u64) << 20);
        for round in 0..rounds {
            let mut damaged = doc.clone();
            let mut what: Vec<String> = Vec::new();
            // (every third round the structure is damaged instead of a number: a value of another type, a field or a component that is missing)
            if round % 3 == 2 && std::env::var_os("LUNARIS_FUZZ_PICK").is_none() {
                for _ in 0..(1 + rng.below(3)) {
                    let path = nodes[rng.below(nodes.len())].clone();
                    // (a path made before an earlier damage may lead nowhere now)
                    let mut ok = true;
                    let mut cur = &damaged;
                    for &i in &path {
                        cur = match cur {
                            Json::Obj(p) if i < p.len() => &p[i].1,
                            Json::Arr(a) if i < a.len() => &a[i],
                            _ => {
                                ok = false;
                                break;
                            }
                        };
                    }
                    if ok {
                        what.push(restructure(&mut damaged, &path, &mut rng));
                    }
                }
                if let Ok(only) = std::env::var("LUNARIS_FUZZ_ONLY") {
                    if only != format!("{rel}:{round}") {
                        continue;
                    }
                }
                runs += 1;
                if std::env::var_os("LUNARIS_FUZZ_TRACE").is_some() {
                    eprintln!("FUZZ {rel} round {round}: {}", what.join(", "));
                }
                if let Err(msg) = run_damaged(&repo, &damaged, ticks) {
                    let first: String = msg.lines().next().unwrap_or("").chars().take(160).collect();
                    seen.entry(format!("{rel}: {first}")).or_default().push(format!("round {round}: {}", what.join(", ")));
                }
                continue;
            }
            let pick: Option<usize> = std::env::var("LUNARIS_FUZZ_PICK").ok().and_then(|v| v.parse().ok());
            for m in 0..(1 + rng.below(4)) {
                let (path, key) = &spots[rng.below(spots.len())];
                let v = DAMAGE[rng.below(DAMAGE.len())];
                // (`LUNARIS_FUZZ_PICK=k` applies only the k-th damage of the round: which one of them it was)
                if pick.is_some_and(|k| k != m) {
                    continue;
                }
                let place = describe(&damaged, path);
                *at_mut(&mut damaged, path) = Json::Float(v);
                what.push(format!("{place} ({key}) = {v}"));
            }
            if let Ok(only) = std::env::var("LUNARIS_FUZZ_ONLY") {
                if only != format!("{rel}:{round}") {
                    continue;
                }
            }
            runs += 1;
            // (an allocation that fails ends the process and cannot be caught: the last line says which damage did it)
            if std::env::var_os("LUNARIS_FUZZ_TRACE").is_some() {
                eprintln!("FUZZ {rel} round {round}: {}", what.join(", "));
            }
            if let Err(msg) = run_damaged(&repo, &damaged, ticks) {
                let first: String = msg.lines().next().unwrap_or("").chars().take(160).collect();
                seen.entry(format!("{rel}: {first}")).or_default().push(format!("round {round}: {}", what.join(", ")));
            }
        }
    }
    println!("{runs} damaged scenes run ({skipped} big ones skipped), {} different panics", seen.len());
    for (k, v) in &seen {
        println!("{k}\n    e.g. {}", v[0]);
    }
    assert!(runs > rounds * 50, "{runs} damaged scenes only");
    assert!(seen.is_empty(), "{} different panics", seen.len());
}

/// A short run for every build: 4 damaged copies of every example, two seconds. (A damage that makes the engine allocate for ever does not panic: it ends the whole test run, which is how it is noticed.)
#[test]
fn damaged_example_scenes_never_panic_a_short_run() {
    damage_the_examples(4, 10);
}

/// Many more: `LUNARIS_FUZZ_ROUNDS` (default 300) damaged copies of every example, `LUNARIS_FUZZ_TICKS` ticks each; 20,000 scenes take 90 s. Find which damage kills the process with `LUNARIS_FUZZ_TRACE=1` (the last line printed), `LUNARIS_FUZZ_ONLY=scene:round` and `LUNARIS_FUZZ_PICK=k` (only the k-th damage of that round); run the binary under `ulimit -v` so that a runaway allocation fails at once.
/// `cargo test --release -p lunaris-api damaged_example_scenes -- --ignored --nocapture`
#[test]
#[ignore]
fn damaged_example_scenes_never_panic() {
    let rounds: usize = std::env::var("LUNARIS_FUZZ_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    let ticks: usize = std::env::var("LUNARIS_FUZZ_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    damage_the_examples(rounds, ticks);
}
