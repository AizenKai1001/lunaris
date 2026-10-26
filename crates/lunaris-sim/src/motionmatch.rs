//! Motion matching: the animation of a character is not a state machine of clips but a search. A **database** holds poses of many captured clips, each described by a vector of
//! features: where the character will be in the next 0.7 seconds (positions, velocities and facing, in its own frame), and what its feet and hips are doing now. Several times a
//! second the character's *wish* (the path it is about to walk, from its velocity, what the player asks and how fast it can speed up and turn) and its current pose are turned into
//! the same kind of vector, the database frame with the lowest weighted cost is found, and the `Animator` is told to play that clip from that moment (it crossfades). Nothing says
//! "run" or "stop": a clip that starts a run, one that stops, one that pivots and the loops all simply compete.
//!
//! This is the algorithm of Unreal's Pose Search plugin and of the talks it comes from (Simon Clavet's "Motion Matching and The Road to Next-Gen Animation", Ubisoft, GDC 2016):
//!
//! * **The schema** is the one of the Game Animation Sample's `PSS_Default`: a trajectory channel with samples at -0.05 s (position), 0 s (velocity, facing), +0.35 s (position, facing)
//!   and +0.7 s (position, velocity, facing), weights 0.3, 1, 1, 1 and a pose channel group: `foot_l` relative to `foot_r`, the velocities of both feet (weight 0.3, one normalisation
//!   group) and the heading of the hips (weight 0.1). Everything is in the character's own frame (so a clip matches wherever and however the character stands), in the 2D ground plane for
//!   the trajectory. The database is built from a model's clips by `lunaris mmbuild` (`crates/lunaris-render/src/motion_db.rs`).
//! * **Cost**: the sum over the groups of `weight * mean((query - frame)^2) / scale^2`, divided by the sum of the weights; `scale` is the deviation of the group over the whole
//!   database ("normalise with common schema"), so every group counts by its weight and not by its units or its number of values.
//! * **Continuing pose**: the pose that is playing now is compared with the query too, with `continuing_bias` added (negative keeps the current clip longer): it is kept unless some
//!   frame is cheaper than it. Searches run every `search_interval` seconds, and at once when the clip has ended.
//! * **Tags** are the chooser: the bits of `tags` say which kinds of clips (idle, walk loop, run start, pivot ...) may be picked now; a game script sets them from what is going on.
//!
//! `MotionMatcher` goes on the entity with the `MeshRenderer` and the `Animator` of the character's model (the entity's own `Transform` is the character's place and facing); a graph
//! system sets `vel` (the velocity the player asks for), `face` (the direction it should face) and `tags`. See `docs/GAMEPLAY.md`, "Motion matching".
use crate::components::{Animator, Transform};
use crate::detmath;
use lunaris_ecs::{component, Entity, Sym, World};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// The number of values of a feature vector: the trajectory (16) and the pose (11).
pub const DIM: usize = 27;
/// Where the pose part of the vector starts.
pub const POSE_START: usize = 16;
/// The seconds of the four trajectory samples.
pub const OFFSETS: [f32; 4] = [-0.05, 0.0, 0.35, 0.7];

const MAGIC: &[u8; 8] = b"LMMDB003";

/// A weighted group of values of the feature vector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Group {
    pub start: u32,
    pub len: u32,
    pub weight: f32,
    /// The deviation of the group over the database.
    pub scale: f32,
}

/// One clip of the database.
#[derive(Clone, Debug, PartialEq)]
pub struct DbClip {
    pub name: String,
    pub length: f32,
    pub looping: bool,
    /// Which kinds of clip it is: the bits of the database's `tag_names`.
    pub tags: u32,
    /// Its frames are `first .. first + count` of the database.
    pub first: u32,
    pub count: u32,
    /// The part of the clip (seconds) that a search may pick a frame from: the whole clip for most; for a jump the frames at take-off, for a landing the frames at touch-down (their root motion says where
    /// they are), so that the clip is picked at the moment its motion is the character's.
    pub from: f32,
    pub to: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MotionDb {
    pub rate: f32,
    pub tag_names: Vec<String>,
    pub groups: Vec<Group>,
    pub clips: Vec<DbClip>,
    /// The clip time of every frame.
    pub times: Vec<f32>,
    /// `DIM` values per frame.
    pub feat: Vec<f32>,
    names: HashMap<String, usize>,
}

fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn put_f32(b: &mut Vec<u8>, v: f32) {
    b.extend_from_slice(&v.to_le_bytes());
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.b.get(self.at..self.at + 4).ok_or("the file ends too soon")?;
        self.at += 4;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        let s = self.b.get(self.at..self.at + n).ok_or("the file ends too soon")?;
        self.at += n;
        String::from_utf8(s.to_vec()).map_err(|_| "a name is not UTF-8".to_string())
    }
}

impl MotionDb {
    pub fn new(rate: f32, tag_names: Vec<String>, groups: Vec<Group>, clips: Vec<DbClip>, times: Vec<f32>, feat: Vec<f32>) -> MotionDb {
        let names = clips.iter().enumerate().map(|(i, c)| (c.name.clone(), i)).collect();
        MotionDb { rate, tag_names, groups, clips, times, feat, names }
    }

    pub fn frames(&self) -> usize {
        self.times.len()
    }

    pub fn clip_index(&self, name: &str) -> Option<usize> {
        self.names.get(name).copied()
    }

    pub fn frame(&self, i: usize) -> &[f32] {
        &self.feat[i * DIM..(i + 1) * DIM]
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = MAGIC.to_vec();
        put_f32(&mut b, self.rate);
        put_u32(&mut b, self.tag_names.len() as u32);
        for t in &self.tag_names {
            put_u32(&mut b, t.len() as u32);
            b.extend_from_slice(t.as_bytes());
        }
        put_u32(&mut b, self.groups.len() as u32);
        for g in &self.groups {
            put_u32(&mut b, g.start);
            put_u32(&mut b, g.len);
            put_f32(&mut b, g.weight);
            put_f32(&mut b, g.scale);
        }
        put_u32(&mut b, self.clips.len() as u32);
        for c in &self.clips {
            put_u32(&mut b, c.name.len() as u32);
            b.extend_from_slice(c.name.as_bytes());
            put_f32(&mut b, c.length);
            put_u32(&mut b, c.looping as u32);
            put_u32(&mut b, c.tags);
            put_u32(&mut b, c.first);
            put_u32(&mut b, c.count);
            put_f32(&mut b, c.from);
            put_f32(&mut b, c.to);
        }
        put_u32(&mut b, self.times.len() as u32);
        for t in &self.times {
            put_f32(&mut b, *t);
        }
        for v in &self.feat {
            put_f32(&mut b, *v);
        }
        b
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<MotionDb, String> {
        if !bytes.starts_with(MAGIC) {
            return Err("not a motion matching database (or one of another version)".into());
        }
        let mut r = Reader { b: bytes, at: MAGIC.len() };
        let rate = r.f32()?;
        let n_tags = r.u32()? as usize;
        if n_tags > 32 {
            return Err("a database has at most 32 tags".into());
        }
        let tag_names = (0..n_tags).map(|_| r.string()).collect::<Result<Vec<_>, _>>()?;
        let n_groups = r.u32()? as usize;
        if n_groups > DIM {
            return Err("too many groups".into());
        }
        let mut groups = Vec::new();
        for _ in 0..n_groups {
            let g = Group { start: r.u32()?, len: r.u32()?, weight: r.f32()?, scale: r.f32()? };
            if (g.start + g.len) as usize > DIM || !(g.weight.is_finite() && g.scale.is_finite()) {
                return Err("a group does not fit the feature vector".into());
            }
            groups.push(g);
        }
        let n_clips = r.u32()? as usize;
        if n_clips > 100_000 {
            return Err("too many clips".into());
        }
        let mut clips = Vec::new();
        for _ in 0..n_clips {
            clips.push(DbClip { name: r.string()?, length: r.f32()?, looping: r.u32()? != 0, tags: r.u32()?, first: r.u32()?, count: r.u32()?, from: r.f32()?, to: r.f32()? });
        }
        let n = r.u32()? as usize;
        if n > 5_000_000 || clips.iter().any(|c| (c.first + c.count) as usize > n) {
            return Err("the frames do not fit the clips".into());
        }
        let mut times = Vec::with_capacity(n);
        for _ in 0..n {
            times.push(r.f32()?);
        }
        let mut feat = Vec::with_capacity(n * DIM);
        for _ in 0..n * DIM {
            feat.push(r.f32()?);
        }
        Ok(MotionDb::new(rate, tag_names, groups, clips, times, feat))
    }

    /// The cost of a query against a frame's features; the pose groups only count when `pose` (a query without a known pose compares the trajectory alone).
    pub fn cost(&self, q: &[f32], f: &[f32], pose: bool) -> f32 {
        let (mut total, mut wsum) = (0.0f32, 0.0f32);
        for g in &self.groups {
            if !pose && g.start as usize >= POSE_START {
                continue;
            }
            let (s, e) = (g.start as usize, (g.start + g.len) as usize);
            let mut acc = 0.0f32;
            for d in s..e {
                let x = q[d] - f[d];
                acc += x * x;
            }
            let sc = if g.scale > 1e-6 { g.scale } else { 1.0 };
            total += g.weight * (acc / g.len.max(1) as f32) / (sc * sc);
            wsum += g.weight;
        }
        if wsum > 0.0 {
            total / wsum
        } else {
            0.0
        }
    }

    /// The features of clip `c` at `time` (looped when the clip loops, held at the ends otherwise), linear between the two frames round it.
    pub fn features_at(&self, c: usize, time: f32) -> [f32; DIM] {
        let clip = &self.clips[c];
        let mut out = [0.0f32; DIM];
        if clip.count == 0 {
            return out;
        }
        let t = if clip.looping && clip.length > 0.0 { time.rem_euclid(clip.length) } else { time.clamp(0.0, clip.length) };
        let x = (t * self.rate).max(0.0);
        let k = (x.floor() as usize).min(clip.count as usize - 1);
        let u = if k + 1 < clip.count as usize { (x - k as f32).clamp(0.0, 1.0) } else { 0.0 };
        let a = self.frame(clip.first as usize + k);
        let b = self.frame(clip.first as usize + (k + 1).min(clip.count as usize - 1));
        for d in 0..DIM {
            out[d] = a[d] + (b[d] - a[d]) * u;
        }
        out
    }
}

/// The groups of the schema (the Game Animation Sample's `PSS_Default`), with scale 1; the database builder fills in the deviations. The first eight are the trajectory samples, the others the pose.
pub fn schema_groups() -> Vec<Group> {
    let g = |start: u32, len: u32, weight: f32| Group { start, len, weight, scale: 1.0 };
    vec![
        g(0, 2, 0.3),  // position at -0.05 s
        g(2, 2, 1.0),  // velocity now
        g(4, 2, 1.0),  // facing now
        g(6, 2, 1.0),  // position at 0.35 s
        g(8, 2, 1.0),  // facing at 0.35 s
        g(10, 2, 1.0), // position at 0.7 s
        g(12, 2, 1.0), // velocity at 0.7 s
        g(14, 2, 1.0), // facing at 0.7 s
        g(16, 3, 1.0), // foot_l relative to foot_r
        g(19, 3, 0.3), // velocity of foot_l
        g(22, 3, 0.3), // velocity of foot_r
        g(25, 2, 0.1), // heading of the hips
    ]
}

type Store = Mutex<BTreeMap<PathBuf, Result<Arc<MotionDb>, String>>>;

fn store() -> &'static Store {
    static STORE: OnceLock<Store> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// The database in file `name` of the workspace (read once, then shared).
pub fn load(root: &Path, name: &str) -> Result<Arc<MotionDb>, String> {
    if name.contains("..") || name.contains(':') || name.starts_with('/') || name.starts_with('\\') {
        return Err(format!("`{name}` leaves the workspace"));
    }
    let path = root.join(name);
    let mut s = store().lock().map_err(|_| "the database store is poisoned".to_string())?;
    if let Some(r) = s.get(&path) {
        return r.clone();
    }
    let r = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display())).and_then(|b| MotionDb::from_bytes(&b)).map(Arc::new);
    s.insert(path, r.clone());
    r
}

component! {
    /// Motion matching for a character (the module docs and docs/GAMEPLAY.md, "Motion matching"). Put it on the entity with the model's `MeshRenderer` and `Animator`; the entity's `Transform` is the character's
    /// place and facing. `db` is the database file of the workspace (`lunaris mmbuild MODEL.glb OUT.mmdb` makes it from a model's clips). A graph system sets, every tick: `vel` (the velocity the player asks for,
    /// metres a second on the ground: x, z), `face` (the direction the character should face, x, z; 0 keeps the facing) and `tags` (which kinds of clip may be picked: the bits of the database's tag list).
    /// `accel` and `brake` (metres a second per second) and `turn` (how fast the facing turns, 1/s) say how the character will speed up, slow down and turn, which is what the predicted path is made of; `decel` (slowing down or turning round with a wish: 0 = `brake`, which is for no wish), `turn_rate` (a steady turn in radians a second instead of the smooth `turn`) and `react` (a change of the wish of more than this many m/s, or of `tags`, searches at once; 0 = only every `search_interval`) and `hysteresis` (the playing clip is left for another that is cheaper by this fraction of its cost, besides `continuing_bias`) refine that.
    /// Every `search_interval` seconds the best frame is looked for; the clip playing now is kept unless a frame costs less than it by more than `continuing_bias` (negative favours it; costs are of the
    /// order of 0.1 to 1); `blend` is the crossfade seconds; a jump inside the clip that is playing must be at least `jump_threshold` seconds long. The answers: `cost` (of the pose picked at the last search),
    /// `keep_cost` (of the pose that was playing), `clip` (its number in the database + 1), `switches` and `searches` counted, `speed` (the measured ground speed). `warp` is how much a loop's playing speed may
    /// differ from 1 to make its own speed the character's (stride warping; 0 = never), and `force` makes the next update search at once (a script sets it when it changes `tags`: a jump, a landing). The rest is the engine's.
    pub struct MotionMatcher {
        pub db: Sym = Sym(0),
        pub tags: u32 = 0xFFFF_FFFF,
        pub vel: [f32; 2] = [0.0, 0.0],
        pub face: [f32; 2] = [0.0, 0.0],
        pub accel: f32 = 16.0,
        pub brake: f32 = 22.0,
        pub turn: f32 = 8.0,
        pub decel: f32 = 0.0,
        pub turn_rate: f32 = 0.0,
        pub react: f32 = 0.5,
        pub hysteresis: f32 = 0.0,
        pub search_interval: f32 = 0.1,
        pub blend: f32 = 0.2,
        pub continuing_bias: f32 = -0.05,
        pub jump_threshold: f32 = 0.2,
        pub enabled: bool = true,
        pub warp: f32 = 0.2,
        pub force: bool = false,
        pub cost: f32 = 0.0,
        pub keep_cost: f32 = 0.0,
        pub clip: u32 = 0,
        pub switches: u32 = 0,
        pub searches: u32 = 0,
        pub speed: f32 = 0.0,
        pub clock: f32 = 0.0,
        pub prev_pos: [f32; 3] = [0.0, 0.0, 0.0],
        pub vel_now: [f32; 2] = [0.0, 0.0],
        pub started: bool = false,
        pub last_wish: [f32; 2] = [0.0, 0.0],
        pub last_tags: u32 = 0,
    }
}

fn len2(a: [f32; 2]) -> f32 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

fn norm2(a: [f32; 2], fallback: [f32; 2]) -> [f32; 2] {
    let l = len2(a);
    if l > 1e-6 {
        [a[0] / l, a[1] / l]
    } else {
        fallback
    }
}

/// A ground vector in the frame of a character that faces `f` (its local z): x along (f.z, -f.x), z along f.
fn local(f: [f32; 2], v: [f32; 2]) -> [f32; 2] {
    [v[0] * f[1] - v[1] * f[0], v[0] * f[0] + v[1] * f[1]]
}

/// How the character's movement code speeds up, slows down and turns, which is what the predicted path is made of (the same numbers the movement code uses).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dynamics {
    /// Metres a second per second when the velocity grows towards the wish.
    pub accel: f32,
    /// When the wish is none (the character stops).
    pub brake: f32,
    /// When the wish is something else but opposes the velocity (slowing down, turning round); 0 = `brake`.
    pub decel: f32,
    /// The facing goes to the wanted one at this rate, a fraction a second (a smooth approach); used when `turn_rate` is 0.
    pub turn: f32,
    /// The facing goes to the wanted one at this many radians a second (a steady turn, like a character movement's rotation rate); 0 = use `turn`.
    pub turn_rate: f32,
}

impl Dynamics {
    pub fn new(accel: f32, brake: f32, turn: f32) -> Dynamics {
        Dynamics { accel, brake, decel: 0.0, turn, turn_rate: 0.0 }
    }
}

/// The feature values of the trajectory part of a query: what the character is doing and is about to do.
pub fn trajectory_query(face0: [f32; 2], vel0: [f32; 2], goal_vel: [f32; 2], goal_face: [f32; 2], d: &Dynamics) -> [f32; POSE_START] {
    let mut q = [0.0f32; POSE_START];
    // the past: where it was 0.05 s ago, from its velocity
    let past = local(face0, [-vel0[0] * 0.05, -vel0[1] * 0.05]);
    q[0] = past[0];
    q[1] = past[1];
    let v0 = local(face0, vel0);
    q[2] = v0[0];
    q[3] = v0[1];
    q[4] = 0.0;
    q[5] = 1.0;
    // the future: speed up (or slow down) towards the wish and turn towards the wanted facing, in steps of 1/30 s
    let h = 1.0 / 30.0;
    let (mut p, mut v, mut f) = ([0.0f32; 2], vel0, face0);
    let want_face = if len2(goal_face) > 1e-6 { norm2(goal_face, face0) } else { f };
    let mut t = 0.0f32;
    for (slot, off) in [(2usize, OFFSETS[2]), (3usize, OFFSETS[3])] {
        while t + h * 0.5 < off {
            let dv = [goal_vel[0] - v[0], goal_vel[1] - v[1]];
            let dl = len2(dv);
            let rate = if len2(goal_vel) < 0.1 {
                d.brake
            } else if dv[0] * v[0] + dv[1] * v[1] < 0.0 {
                if d.decel > 0.0 {
                    d.decel
                } else {
                    d.brake
                }
            } else {
                d.accel
            };
            let step = (rate * h).min(dl);
            if dl > 1e-6 {
                v = [v[0] + dv[0] / dl * step, v[1] + dv[1] / dl * step];
            }
            p = [p[0] + v[0] * h, p[1] + v[1] * h];
            if d.turn_rate > 0.0 {
                let ang = detmath::atan2((f[0] * want_face[1] - f[1] * want_face[0]) as f64, (f[0] * want_face[0] + f[1] * want_face[1]) as f64) as f32;
                let step = ang.clamp(-d.turn_rate * h, d.turn_rate * h);
                let (sn, cs) = (detmath::sin(step as f64) as f32, detmath::cos(step as f64) as f32);
                f = norm2([f[0] * cs - f[1] * sn, f[0] * sn + f[1] * cs], f);
            } else {
                let k = (d.turn * h).min(1.0);
                f = norm2([f[0] + (want_face[0] - f[0]) * k, f[1] + (want_face[1] - f[1]) * k], f);
            }
            t += h;
        }
        let lp = local(face0, p);
        let lf = local(face0, f);
        if slot == 2 {
            q[6] = lp[0];
            q[7] = lp[1];
            q[8] = lf[0];
            q[9] = lf[1];
        } else {
            let lv = local(face0, v);
            q[10] = lp[0];
            q[11] = lp[1];
            q[12] = lv[0];
            q[13] = lv[1];
            q[14] = lf[0];
            q[15] = lf[1];
        }
    }
    q
}

/// Where clip `c` and time `t` stand in the database's frames for the pose of the clip playing now (`None` when the clip is not in the database).
fn playing(db: &MotionDb, name: &str, time: f32) -> Option<(usize, f32)> {
    let c = db.clip_index(name)?;
    let clip = &db.clips[c];
    let t = if clip.looping && clip.length > 0.0 { time.rem_euclid(clip.length) } else { time };
    Some((c, t))
}

/// Run the search of every `MotionMatcher` that is due, and tell its `Animator` what to play.
pub fn update(world: &mut World, root: Option<&Path>, dt: f32) {
    let mut found: Vec<(u64, Entity, MotionMatcher, Transform, Animator)> = Vec::new();
    let _ = world.for_each_uid::<(&MotionMatcher, &Transform, &Animator), _>(|e, uid, (m, t, a)| found.push((uid, e, *m, *t, *a)));
    if found.is_empty() {
        return;
    }
    found.sort_by_key(|f| f.0);
    for (_, e, mut m, tr, mut a) in found {
        if !m.enabled || m.db.0 == 0 {
            continue;
        }
        let Some(root) = root else { continue };
        let name = world.strings().get(m.db.0).to_string();
        let Ok(db) = load(root, &name) else {
            m.enabled = false;
            m.cost = -1.0;
            if let Some(mm) = world.get_mut::<MotionMatcher>(e) {
                *mm = m;
            }
            continue;
        };
        // what the character is doing: where it faces (the entity's rotation about y) and how it moves
        let q = tr.rot;
        let facing = norm2([2.0 * (q[0] * q[2] + q[3] * q[1]), 1.0 - 2.0 * (q[0] * q[0] + q[1] * q[1])], [0.0, 1.0]);
        let pos = [tr.pos[0], tr.pos[2]];
        let vel0 = if m.started && dt > 0.0 { [(pos[0] - m.prev_pos[0]) / dt, (pos[1] - m.prev_pos[2]) / dt] } else { [0.0, 0.0] };
        m.prev_pos = [pos[0], 0.0, pos[1]];
        m.started = true;
        // a smoothed velocity: a character that is moved in steps would otherwise make the path jitter
        m.vel_now = [m.vel_now[0] + (vel0[0] - m.vel_now[0]) * 0.5, m.vel_now[1] + (vel0[1] - m.vel_now[1]) * 0.5];
        m.speed = len2(m.vel_now);
        m.clock += dt;
        let cur_name = world.strings().get(a.clip.0).to_string();
        let cur = playing(&db, &cur_name, a.time);
        let cur_end = cur.map(|(c, t)| !db.clips[c].looping && t >= db.clips[c].length - 1.0 / db.rate.max(1.0)).unwrap_or(true);
        // a change of what the player wants (the wish moved by more than `react` m/s) or of the kinds of clip allowed is a reason to look at once: the character does not go on in the old clip while its velocity
        // has already changed
        let wish_jump = len2([m.vel[0] - m.last_wish[0], m.vel[1] - m.last_wish[1]]);
        let event = m.react > 0.0 && (wish_jump > m.react || m.tags != m.last_tags);
        m.last_wish = m.vel;
        m.last_tags = m.tags;
        if m.clock >= m.search_interval || cur.is_none() || cur_end || m.force || event {
            // a forced search is a fresh choice: the clip that is playing gives the pose to match but is not preferred over the others
            let forced = m.force;
            m.force = false;
            m.clock = 0.0;
            m.searches += 1;
            let traj = trajectory_query(facing, m.vel_now, m.vel, m.face, &Dynamics { accel: m.accel, brake: m.brake, decel: m.decel, turn: m.turn, turn_rate: m.turn_rate });
            let mut query = [0.0f32; DIM];
            query[..POSE_START].copy_from_slice(&traj);
            // the pose that is playing: its pose features are the query's (so the pose groups judge how well a frame continues it) and it is a candidate itself
            let mut keep: Option<(usize, f32, f32)> = None;
            if let (Some((c, t)), false) = (cur, cur_end) {
                let f = db.features_at(c, t);
                query[POSE_START..].copy_from_slice(&f[POSE_START..]);
                let cost = db.cost(&query, &f, true) + m.continuing_bias;
                keep = Some((c, t, cost));
                m.keep_cost = cost;
            } else {
                m.keep_cost = -1.0;
            }
            let have_pose = keep.is_some();
            let guard = (1.0 / db.rate.max(1.0)) * 4.0;
            let mut best: Option<(usize, f32)> = None; // (frame, cost)
            for (ci, clip) in db.clips.iter().enumerate() {
                if clip.tags & m.tags == 0 {
                    continue;
                }
                for k in 0..clip.count as usize {
                    let fi = clip.first as usize + k;
                    let ft = db.times[fi];
                    if !clip.looping && ft > clip.length - 0.15 {
                        continue;
                    }
                    if ft < clip.from - 1.0e-4 || ft > clip.to + 1.0e-4 {
                        continue;
                    }
                    if let (Some((c, t, _)), false) = (keep, forced) {
                        // no jump inside the clip that is playing, unless it is a long one
                        if c == ci && (ft - t).abs() < m.jump_threshold.max(guard) && !clip.looping {
                            continue;
                        }
                        if c == ci && clip.looping {
                            let l = clip.length.max(1e-3);
                            let d = ((ft - t).rem_euclid(l)).min((t - ft).rem_euclid(l));
                            if d < m.jump_threshold.max(guard) {
                                continue;
                            }
                        }
                    }
                    let cost = db.cost(&query, db.frame(fi), have_pose);
                    if best.map_or(true, |b| cost < b.1) {
                        best = Some((fi, cost));
                    }
                }
            }
            if let Some((fi, cost)) = best {
                let take = match keep {
                    // (a clip that plays is left only for one that costs less by `hysteresis` of its own cost as well: when nothing matches well, the choice does not flip between near equals)
                    Some((_, _, kc)) if !forced => cost < kc - m.hysteresis * kc.abs(),
                    _ => true,
                };
                if take {
                    // the clip of that frame
                    let ci = db.clips.iter().position(|c| fi >= c.first as usize && fi < (c.first + c.count) as usize).unwrap_or(0);
                    let clip = &db.clips[ci];
                    let sym = Sym(world.strings_mut().intern(&clip.name));
                    let changed = a.clip.0 != sym.0;
                    a.clip = sym;
                    a.time = db.times[fi];
                    a.looping = clip.looping;
                    a.speed = 1.0;
                    a.playing = true;
                    a.fade = if changed { m.blend } else { 0.0 };
                    m.cost = cost;
                    m.clip = ci as u32 + 1;
                    m.switches += 1;
                } else if let Some((c, _, kc)) = keep {
                    // the clip that is playing wins: `clip` and `cost` say so (an Animator that already plays a clip of the database never switches)
                    m.cost = kc;
                    m.clip = c as u32 + 1;
                }
            } else if let Some((c, _, kc)) = keep {
                m.cost = kc;
                m.clip = c as u32 + 1;
            }
        }
        // stride warping: a loop is played a little faster or slower so that its own speed is the character's (feet that do not slide at speeds between the captured ones)
        let playing_now = world.strings().get(a.clip.0).to_string();
        let target = match playing(&db, &playing_now, a.time) {
            // (only a clip that fits what the character does is warped: when the choice is poor the clip plays as captured, instead of also speeding up and slowing down with it)
            Some((c, t)) if m.warp > 0.0 && db.clips[c].looping && m.keep_cost >= -0.5 && m.keep_cost < 0.6 => {
                let f = db.features_at(c, t);
                let clip_speed = len2([f[2], f[3]]);
                if clip_speed > 0.5 && m.speed > 0.3 {
                    (m.speed / clip_speed).clamp(1.0 - m.warp, 1.0 + m.warp)
                } else {
                    1.0
                }
            }
            _ => 1.0,
        };
        a.speed += (target - a.speed) * (dt * 8.0).min(1.0);
        if let Some(mm) = world.get_mut::<MotionMatcher>(e) {
            *mm = m;
        }
        if let Some(an) = world.get_mut::<Animator>(e) {
            *an = a;
        }
    }
}
