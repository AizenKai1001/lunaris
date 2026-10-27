//! The repository's real model, picture, sound and effect files with their bytes damaged, fed to the loaders: a file from a modelling program, a download that was cut off, a disk that flipped a bit, or a file
//! made to hurt must give an error that says what is wrong, never a panic, a hang or a huge allocation. The seeds are the files of `examples/` (glTF and GLB models, PNG and JPEG pictures, WAV sounds, effect and
//! scene JSON), so the parsers meet the layouts real exporters write, not only the ones a test makes up. Damage: bits flipped, cut short, a run of 0x00 or 0xFF (the length and count fields of a format),
//! bytes inserted, two stretches swapped, a four-byte field replaced by 0, 1, 0xFFFF or 0xFFFFFFFF.
use crate::scene_roundtrip_tests::scenes_under;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

/// The file and mutation being worked on, for the watchdog that ends the run when a parser does not come back.
static CURRENT: Mutex<String> = Mutex::new(String::new());

fn damage(bytes: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut b = bytes.to_vec();
    if b.is_empty() {
        return b;
    }
    match rng.below(7) {
        0 | 1 => {
            for _ in 0..1 + rng.below(10) {
                let i = rng.below(b.len());
                b[i] ^= 1 << rng.below(8);
            }
        }
        2 => b.truncate(rng.below(b.len() + 1)),
        3 => {
            let (start, len) = (rng.below(b.len()), 1 + rng.below(16));
            let fill = [0x00u8, 0xFF][rng.below(2)];
            for x in b.iter_mut().skip(start).take(len) {
                *x = fill;
            }
        }
        4 => {
            let at = rng.below(b.len() + 1);
            let extra: Vec<u8> = (0..1 + rng.below(64)).map(|_| rng.next() as u8).collect();
            b.splice(at..at, extra);
        }
        5 => {
            if b.len() > 8 {
                let (x, y) = (rng.below(b.len() / 2), b.len() / 2 + rng.below(b.len() / 2));
                let len = 1 + rng.below(8.min(b.len() - y));
                for k in 0..len {
                    b.swap(x + k, y + k);
                }
            }
        }
        _ => {
            if b.len() > 4 {
                let at = rng.below(b.len() - 3);
                let v: u32 = [0, 1, 0xFFFF, 0xFFFF_FFFF, 0x7FFF_FFFF, 0x8000_0000][rng.below(6)];
                b[at..at + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
    b
}

/// Every parser that takes a file of this kind. Errors are the right answer; a panic is what the caller looks for.
fn parse_all(ext: &str, name: &str, dir: &Path, bytes: &[u8]) {
    match ext {
        "glb" | "gltf" => {
            let _ = lunaris_render::gltf::import(bytes, Some(dir));
            let _ = lunaris_sim::meshcol::CollisionMesh::from_bytes(bytes, Some(dir));
        }
        "png" => {
            let _ = lunaris_render::texture::Texture::from_image_bytes(bytes);
            let _ = lunaris_sim::png::decode_png(bytes);
            let _ = lunaris_sim::terrain::Heightfield::from_png(bytes);
        }
        "jpg" => {
            let _ = lunaris_render::texture::Texture::from_image_bytes(bytes);
        }
        "wav" => {
            let _ = lunaris_audio::codec::decode_any(bytes);
        }
        "json" => {
            let text = String::from_utf8_lossy(bytes);
            let doc = lunaris_json::parse(&text);
            if name.ends_with(".fx.json") {
                let _ = lunaris_sim::fx::parse(&text);
            }
            if let Ok(d) = doc {
                // (and back to text: the writer meets what the parser let through)
                let _ = d.to_string_compact();
            }
        }
        _ => {}
    }
}

fn collect(repo: &Path) -> Vec<(String, String, PathBuf)> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        v.sort();
        for p in v {
            if p.is_dir() {
                if !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('_')) {
                    walk(&p, out);
                }
            } else {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&repo.join("examples"), &mut files);
    for p in files {
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if matches!(ext.as_str(), "glb" | "gltf" | "png" | "jpg" | "wav" | "json") {
            let rel = p.strip_prefix(repo).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, ext, p));
        }
    }
    out
}

/// `per_file` damaged copies of up to `max_files` files of each kind (the same ones every run), each given to every parser of its kind.
fn damage_the_files(per_file: usize, max_files: usize, max_bytes: u64) -> (usize, Vec<String>) {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // a watchdog: a parser that does not come back ends the run with the name of the file
    let started = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let watch = started.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        let t = watch.load(std::sync::atomic::Ordering::SeqCst);
        if t == u64::MAX {
            return;
        }
        let stuck = CURRENT.lock().map(|c| c.clone()).unwrap_or_default();
        // (the same input for 120 s: a parser that never comes back)
        static SEEN: Mutex<(String, u32)> = Mutex::new((String::new(), 0));
        let mut s = SEEN.lock().unwrap();
        if s.0 == stuck {
            s.1 += 1;
        } else {
            *s = (stuck.clone(), 0);
        }
        if s.1 >= 24 {
            eprintln!("A PARSER DID NOT COME BACK FOR 120 s: {stuck}");
            std::process::exit(3);
        }
    });
    let mut per_kind: std::collections::BTreeMap<String, usize> = Default::default();
    let (mut runs, mut bad) = (0usize, Vec::<String>::new());
    for (rel, ext, path) in collect(&repo) {
        let n = per_kind.entry(ext.clone()).or_insert(0);
        // (every file of a few, every Nth of a kind with hundreds: the same ones every run)
        if *n >= max_files {
            continue;
        }
        *n += 1;
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.len() > max_bytes {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let dir = path.parent().unwrap().to_path_buf();
        let mut rng = Rng(0xF11E ^ (rel.len() as u64) << 16 ^ bytes.len() as u64);
        for round in 0..per_file {
            let d = damage(&bytes, &mut rng);
            if let Ok(mut c) = CURRENT.lock() {
                *c = format!("{rel} round {round} ({} bytes)", d.len());
            }
            runs += 1;
            if let Err(p) = catch_unwind(AssertUnwindSafe(|| parse_all(&ext, &rel, &dir, &d))) {
                let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "a panic".to_string());
                bad.push(format!("{rel} round {round}: {}", msg.lines().next().unwrap_or("").chars().take(160).collect::<String>()));
            }
        }
    }
    started.store(u64::MAX, std::sync::atomic::Ordering::SeqCst);
    let _ = scenes_under; // (the same walk of the examples as the scene tests)
    (runs, bad)
}

/// A short run for every build: 4 damaged copies of up to 8 files of each kind, none over 1 MB.
#[test]
fn damaged_model_picture_sound_and_effect_files_give_errors_and_not_panics_a_short_run() {
    let (runs, bad) = damage_the_files(4, 8, 1 << 20);
    assert!(runs > 80, "{runs} damaged files only");
    assert!(bad.is_empty(), "{} panics in {runs} damaged files:\n{}", bad.len(), bad.iter().take(12).cloned().collect::<Vec<_>>().join("\n"));
}

/// Many more: `LUNARIS_FUZZ_ROUNDS` (default 400) damaged copies of up to `LUNARIS_FUZZ_FILES` (default 60) files of each kind, up to 8 MB each. Run the binary under `ulimit -v` so that a runaway allocation
/// fails at once; `LUNARIS_FUZZ_TRACE=1` prints each file as it starts (the last line is the one that killed the process).
/// `cargo test --release -p lunaris-api damaged_model_picture -- --ignored --nocapture`
#[test]
#[ignore]
fn damaged_model_picture_sound_and_effect_files_give_errors_and_not_panics() {
    let rounds: usize = std::env::var("LUNARIS_FUZZ_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(400);
    let files: usize = std::env::var("LUNARIS_FUZZ_FILES").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    let (runs, bad) = damage_the_files(rounds, files, 8 << 20);
    println!("{runs} damaged files, {} panics", bad.len());
    for b in bad.iter().take(40) {
        println!("  {b}");
    }
    assert!(bad.is_empty(), "{} panics", bad.len());
}
