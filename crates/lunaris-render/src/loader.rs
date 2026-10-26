//! Loading files on other threads, for worlds that stream: a picture of 8192 x 8192 takes most of a second to decode, a heightmap or a model a tenth of one, and a game that waits for them stops for as long.
//! A `Loader` has a few worker threads and takes requests (`request`): the workspace's cache asks a little before a streamed piece of the world comes into range (`StreamZone.prefetch`), and the
//! results are taken back by the thread that draws (`poll`, or `wait_for` the one that is wanted now). Nothing here is a drawing or a simulation step: what is loaded is the same bytes the
//! synchronous path would have read, so the pictures are the same, only the stall is gone.
use crate::gltf::Imported;
use crate::texture::Texture;
use crate::terrain_layers::Splat;
use lunaris_sim::terrain::Heightfield;
use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

/// What a request is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// A PNG or JPEG decoded into a `Texture`.
    Texture,
    /// A `.gltf` or `.glb`.
    Model,
    /// A greyscale PNG read as a `Heightfield` (a terrain's or a water body's).
    Heights,
    /// An RGBA PNG read as a `Splat`.
    Splat,
}

pub enum Payload {
    Texture(Arc<Texture>),
    Model(Imported),
    Heights(Heightfield),
    Splat(Splat),
}

pub struct Done {
    pub kind: Kind,
    pub name: String,
    pub result: Result<Payload, String>,
}

type Slot = Arc<OnceLock<Result<Arc<Texture>, String>>>;

/// The pictures that have been decoded, by name, however many threads ask for one: the first decodes and the others wait for it, so a picture that a terrain and the roofs of a model both name is read once and is
/// one `Arc` (one copy in memory, one upload to the card).
#[derive(Clone, Default)]
pub struct Decoded(Arc<Mutex<HashMap<String, Slot>>>);

impl Decoded {
    pub fn picture(&self, root: &Path, name: &str) -> Result<Arc<Texture>, String> {
        let slot = self.0.lock().unwrap_or_else(|e| e.into_inner()).entry(name.to_string()).or_default().clone();
        slot.get_or_init(|| Texture::from_png(&safe_path(root, name)?).map(Arc::new)).clone()
    }

    /// Let go of `name` (it is read again from its file the next time it is asked for).
    pub fn forget(&self, name: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(name);
    }

    /// Let go of the pictures nothing else holds (a picture a mesh or the cache has dropped): the table must not keep them alive. Returns how many went.
    pub fn prune(&self) -> usize {
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let before = m.len();
        m.retain(|_, slot| match slot.get() {
            Some(Ok(t)) => Arc::strong_count(t) > 1,
            Some(Err(_)) => false,
            None => true,
        });
        before - m.len()
    }

    pub fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `name` inside the workspace `root`: a relative path with no `..`, no drive letter.
pub fn safe_path(root: &Path, name: &str) -> Result<PathBuf, String> {
    let p = Path::new(name);
    if p.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err("path must be relative and stay inside the workspace".into());
    }
    Ok(root.join(p))
}

struct Job {
    kind: Kind,
    name: String,
}

pub struct Loader {
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Done>,
    pending: BTreeSet<(Kind, String)>,
}

impl Loader {
    /// `workers` threads (at least one) that read files of the workspace `root`; `decoded` is shared with the cache that owns the loader.
    pub fn new(root: PathBuf, decoded: Decoded, workers: usize) -> Loader {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (done_tx, done) = mpsc::channel::<Done>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        for k in 0..workers.max(1) {
            let (rx, tx, root, decoded) = (job_rx.clone(), done_tx.clone(), root.clone(), decoded.clone());
            let _ = std::thread::Builder::new().name(format!("lunaris-load-{k}")).spawn(move || {
                crate::bg::background_priority();
                loop {
                    let job = match rx.lock().unwrap_or_else(|e| e.into_inner()).recv() {
                        Ok(j) => j,
                        Err(_) => break,
                    };
                    let result = run(&root, &decoded, &job);
                    if tx.send(Done { kind: job.kind, name: job.name, result }).is_err() {
                        break;
                    }
                }
            });
        }
        Loader { jobs, done, pending: BTreeSet::new() }
    }

    /// Ask for a file to be loaded. Returns false when the same request is already on its way.
    pub fn request(&mut self, kind: Kind, name: &str) -> bool {
        if !self.pending.insert((kind, name.to_string())) {
            return false;
        }
        if self.jobs.send(Job { kind, name: name.to_string() }).is_err() {
            self.pending.remove(&(kind, name.to_string()));
            return false;
        }
        true
    }

    pub fn is_pending(&self, kind: Kind, name: &str) -> bool {
        self.pending.contains(&(kind, name.to_string()))
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// What has been loaded since the last call (does not wait).
    pub fn poll(&mut self) -> Vec<Done> {
        let mut out = Vec::new();
        while let Ok(d) = self.done.try_recv() {
            self.pending.remove(&(d.kind, d.name.clone()));
            out.push(d);
        }
        out
    }

    /// Wait until the request `(kind, name)` has been answered; returns it with everything else that arrived meanwhile.
    pub fn wait_for(&mut self, kind: Kind, name: &str) -> Vec<Done> {
        let mut out = self.poll();
        while self.is_pending(kind, name) {
            match self.done.recv() {
                Ok(d) => {
                    self.pending.remove(&(d.kind, d.name.clone()));
                    out.push(d);
                }
                Err(_) => {
                    self.pending.remove(&(kind, name.to_string()));
                    break;
                }
            }
        }
        out
    }
}

fn run(root: &Path, decoded: &Decoded, job: &Job) -> Result<Payload, String> {
    match job.kind {
        Kind::Texture => decoded.picture(root, &job.name).map(Payload::Texture),
        Kind::Heights => {
            safe_path(root, &job.name)?;
            Heightfield::read(root, &job.name).map(Payload::Heights)
        }
        Kind::Splat => {
            safe_path(root, &job.name)?;
            Splat::read(root, &job.name).map(Payload::Splat)
        }
        Kind::Model => {
            let rel = Path::new(&job.name);
            if rel.is_absolute() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
                return Err("model paths are relative to the workspace and cannot use `..`".to_string());
            }
            let path = root.join(rel);
            let bytes = match lunaris_sim::procgen::file(&job.name) {
                Some(made) => made?,
                None => std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?,
            };
            let folder = rel.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            // the pictures the model names are decoded through the shared table: one that a terrain (or another model) names too is the same picture
            let mut resolver = |uri: &str| -> Option<Arc<Texture>> {
                let joined = folder.join(uri).to_string_lossy().replace('\\', "/");
                if !root.join(&joined).is_file() {
                    return None;
                }
                decoded.picture(root, &joined).ok()
            };
            crate::gltf::import_with(&bytes, path.parent(), Some(&mut resolver)).map(Payload::Model)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(path: &Path, w: u32, h: u32, depth: png::BitDepth, color: png::ColorType, data: &[u8]) {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(color);
            enc.set_depth(depth);
            enc.write_header().unwrap().write_image_data(data).unwrap();
        }
        std::fs::write(path, out).unwrap();
    }

    fn workspace(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lunaris-loader-{}-{name}", std::process::id()));
        std::fs::create_dir_all(dir.join("m")).unwrap();
        png(&dir.join("pic.png"), 8, 8, png::BitDepth::Eight, png::ColorType::Rgb, &[180u8; 8 * 8 * 3]);
        png(&dir.join("m").join("pic.png"), 8, 8, png::BitDepth::Eight, png::ColorType::Rgb, &[90u8; 8 * 8 * 3]);
        png(&dir.join("h.png"), 5, 5, png::BitDepth::Sixteen, png::ColorType::Grayscale, &[0x40u8; 5 * 5 * 2]);
        png(&dir.join("s.png"), 4, 4, png::BitDepth::Eight, png::ColorType::Rgba, &[255u8; 4 * 4 * 4]);
        dir
    }

    #[test]
    fn requests_come_back_loaded_once_and_a_wait_returns_the_one_that_is_wanted() {
        let dir = workspace("basic");
        let decoded = Decoded::default();
        let mut l = Loader::new(dir.clone(), decoded.clone(), 3);
        assert!(l.request(Kind::Texture, "pic.png"));
        assert!(!l.request(Kind::Texture, "pic.png"), "already on its way");
        assert!(l.request(Kind::Heights, "h.png") && l.request(Kind::Splat, "s.png") && l.request(Kind::Texture, "nothing.png"));
        let mut got = l.wait_for(Kind::Texture, "nothing.png");
        while l.pending() > 0 {
            got.extend(l.wait_for(Kind::Splat, "s.png"));
            got.extend(l.poll());
            std::thread::yield_now();
        }
        assert_eq!(got.len(), 4);
        let by = |k: Kind, n: &str| got.iter().find(|d| d.kind == k && d.name == n).unwrap_or_else(|| panic!("no answer for {n}"));
        assert!(matches!(&by(Kind::Texture, "pic.png").result, Ok(Payload::Texture(t)) if t.width == 8));
        assert!(matches!(&by(Kind::Heights, "h.png").result, Ok(Payload::Heights(h)) if h.cols == 5));
        assert!(matches!(&by(Kind::Splat, "s.png").result, Ok(Payload::Splat(s)) if s.cols == 4));
        assert!(by(Kind::Texture, "nothing.png").result.is_err(), "a missing file is an error, not a hang");
        assert_eq!(decoded.len(), 2, "the two pictures asked for (one of them missing) are in the table");
        let evil = l.request(Kind::Texture, "../outside.png");
        assert!(evil);
        let answers = l.wait_for(Kind::Texture, "../outside.png");
        assert!(answers.iter().any(|d| d.name == "../outside.png" && d.result.is_err()), "a name that leaves the workspace is refused");
    }

    #[test]
    fn a_picture_that_a_model_and_a_request_both_name_is_decoded_once() {
        let dir = workspace("shared");
        let f = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let mut bin = f(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        bin.extend(f(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
        bin.extend([0u8, 0, 1, 0, 2, 0, 0, 0]);
        std::fs::write(dir.join("m").join("tri.bin"), &bin).unwrap();
        let gltf = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"TEXCOORD_0":1}},"indices":2,"material":0}}]}}],
            "materials":[{{"pbrMetallicRoughness":{{"baseColorTexture":{{"index":0}}}}}}],"textures":[{{"source":0}}],"images":[{{"uri":"pic.png"}}],
            "buffers":[{{"byteLength":{},"uri":"tri.bin"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":24}},{{"buffer":0,"byteOffset":60,"byteLength":6}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}},{{"bufferView":2,"componentType":5123,"count":3,"type":"SCALAR"}}]}}"#,
            bin.len()
        );
        std::fs::write(dir.join("m").join("tri.gltf"), gltf).unwrap();
        let decoded = Decoded::default();
        let mut l = Loader::new(dir, decoded.clone(), 4);
        l.request(Kind::Model, "m/tri.gltf");
        l.request(Kind::Texture, "m/pic.png");
        let mut got = l.wait_for(Kind::Model, "m/tri.gltf");
        got.extend(l.wait_for(Kind::Texture, "m/pic.png"));
        let model = got.iter().find_map(|d| match &d.result {
            Ok(Payload::Model(i)) => Some(i.mesh.tex.clone().expect("textured")),
            _ => None,
        });
        let pic = got.iter().find_map(|d| match &d.result {
            Ok(Payload::Texture(t)) => Some(t.clone()),
            _ => None,
        });
        assert!(Arc::ptr_eq(&model.expect("the model loaded"), &pic.expect("the picture loaded")), "one decode, one copy");
        assert_eq!(decoded.len(), 1);
        decoded.forget("m/pic.png");
        assert!(decoded.is_empty());
    }
}
