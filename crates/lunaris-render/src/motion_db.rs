//! Builds a motion matching database (`lunaris_sim::motionmatch::MotionDb`) from the clips of a model: for every clip, every 1/30 s, the feature vector of the schema (the module docs of
//! `lunaris-sim/src/motionmatch.rs`): the path the clip's root travels in the next 0.7 s (its root motion, in the frame of the root at that moment) and the pose of the feet and hips. This
//! is what Unreal's Pose Search "indexing" of a database does. `lunaris mmbuild MODEL.glb OUT.mmdb` runs it.
use crate::math::*;
use crate::rig::{PoseLayer, Rig};
use lunaris_sim::motionmatch::{schema_groups, DbClip, MotionDb, DIM, OFFSETS, POSE_START};

/// What to build.
pub struct BuildOptions {
    /// Only the clips whose names start with this (empty: all).
    pub prefix: String,
    /// Frames a second.
    pub rate: f32,
    pub foot_l: String,
    pub foot_r: String,
    pub thigh_l: String,
    pub thigh_r: String,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions { prefix: String::new(), rate: 30.0, foot_l: "foot_l".into(), foot_r: "foot_r".into(), thigh_l: "thigh_l".into(), thigh_r: "thigh_r".into() }
    }
}

/// The tags (kinds of clip) and the words in a clip's name that put it in them.
pub const TAGS: [(&str, &[&str]); 29] = [
    ("idle", &["Stand_Idle"]),
    ("turn", &["Stand_Turn"]),
    ("walk_loop", &["Walk_Loop"]),
    ("run_loop", &["Run_Loop"]),
    ("walk_start", &["Walk_Start", "Walk_Reface_Start"]),
    ("run_start", &["Run_Start", "Run_Reface_Start"]),
    ("walk_stop", &["Walk_Stop"]),
    ("run_stop", &["Run_Stop"]),
    ("walk_pivot", &["Walk_Pivot"]),
    ("run_pivot", &["Run_Pivot", "Run_Box", "Run_Turn", "Run_Shuffle"]),
    ("crouch", &["Crouch"]),
    ("jump_start", &["Jump_F_Start", "Jump_B_Start", "Jump_LL_Start", "Jump_RL_Start"]),
    ("fall_start", &["Jump_F_Off"]),
    ("jump_fall", &["Jump_Loop_Fall"]),
    ("land_light", &["Land_Stand_Light", "Land_Walk_Light", "Land_Run_Light", "Land_Sprint_Light"]),
    ("land_heavy", &["Land_Stand_Heavy", "Land_Walk_Heavy", "Land_Run_Heavy", "Land_Sprint_Heavy"]),
    // a roll out of a hard fall that carries the run on, and a stumble out of a very hard one: their own kinds, so that a script says which of the two (or neither) a landing may be
    ("land_roll", &["Land_Roll"]),
    ("land_stumble", &["Land_Stumble"]),
    ("sprint_loop", &["Sprint_Loop"]),
    ("sprint_start", &["Sprint_Start", "Sprint_Reface_Start"]),
    ("sprint_stop", &["Sprint_Stop"]),
    ("sprint_pivot", &["Sprint_Turn", "Sprint_Diamond"]),
    // the kinds of a crouch (a crouching character picks among the clips of its own, by what it does: they have the `crouch` kind as well)
    ("crouch_idle", &["Crouch_Idle_Loop"]),
    ("crouch_turn", &["Crouch_Idle_Turn"]),
    ("crouch_loop", &["Crouch_Loop"]),
    ("crouch_start", &["Crouch_Start"]),
    ("crouch_stop", &["Crouch_Stop"]),
    ("crouch_pivot", &["Crouch_Pivot", "Crouch_Turn_", "Crouch_Box", "Crouch_Shuffle"]),
    ("other", &["Transition_"]),
];

fn tags_of(name: &str) -> u32 {
    let mut m = 0u32;
    for (i, (_, words)) in TAGS.iter().enumerate() {
        if words.iter().any(|w| name.contains(w)) {
            m |= 1 << i;
        }
    }
    if m == 0 {
        m = 1 << (TAGS.len() - 1);
    }
    m
}

/// The bit of the tag called `name`.
fn tag_bit(name: &str) -> u32 {
    TAGS.iter().position(|t| t.0 == name).map_or(0, |i| 1u32 << i)
}

/// The part of a clip that a search may pick from. A jump's start clip begins with the crouch before the leap and goes on far past the landing: only the frames of the take-off (the first ones with the
/// root above the ground) are for a character that has just left the ground; the clip of a fall off an edge only its first moments; a landing clip begins in the air and the frames that fit a character
/// that has just touched down are the ones at the moment the root is back on the ground. Every other clip: all of it.
fn search_window(rig: &Rig, clip: usize, tags: u32, length: f32, rate: f32) -> (f32, f32) {
    let step = 1.0 / rate;
    let frames = (length * rate).floor() as usize;
    let height = |t: f32| rig.root_motion(clip, t).0[1];
    if tags & tag_bit("jump_start") != 0 {
        if let Some(t) = (0..=frames).map(|k| k as f32 * step).find(|&t| height(t) > 0.03) {
            return ((t - step).max(0.0), (t + 0.15).min(length));
        }
    }
    if tags & tag_bit("fall_start") != 0 {
        return (0.0, 0.2f32.min(length));
    }
    if tags & (tag_bit("land_light") | tag_bit("land_heavy") | tag_bit("land_roll") | tag_bit("land_stumble")) != 0 {
        if let Some(t) = (0..=frames).map(|k| k as f32 * step).find(|&t| height(t) <= 0.03) {
            return ((t - 0.15).max(0.0), (t + 0.1).min(length));
        }
    }
    (0.0, length)
}

fn position(w: &M4) -> V3 {
    [w.0[12], w.0[13], w.0[14]]
}

fn joint_index(rig: &Rig, name: &str) -> Result<usize, String> {
    rig.joints.iter().position(|j| j.name == name).ok_or_else(|| format!("the skeleton has no joint `{name}`"))
}

/// The root of a clip on the ground, sampled every 1/rate: position (x, z) and facing (unit x, z), with the way beyond the ends worked out as Unreal does: a loop goes on by its own cycle, other
/// clips carry on with the velocity they end (or begin) with.
struct RootPath {
    rate: f32,
    length: f32,
    looping: bool,
    pos: Vec<[f32; 2]>,
    face: Vec<[f32; 2]>,
}

fn len2(a: [f32; 2]) -> f32 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

fn norm2(a: [f32; 2], fallback: [f32; 2]) -> [f32; 2] {
    let l = (a[0] * a[0] + a[1] * a[1]).sqrt();
    if l > 1e-6 {
        [a[0] / l, a[1] / l]
    } else {
        fallback
    }
}

impl RootPath {
    fn within(&self, t: f32) -> ([f32; 2], [f32; 2]) {
        let x = (t * self.rate).clamp(0.0, (self.pos.len() - 1) as f32);
        let k = (x.floor() as usize).min(self.pos.len() - 1);
        let k1 = (k + 1).min(self.pos.len() - 1);
        let u = x - k as f32;
        let p = [self.pos[k][0] + (self.pos[k1][0] - self.pos[k][0]) * u, self.pos[k][1] + (self.pos[k1][1] - self.pos[k][1]) * u];
        let f = norm2([self.face[k][0] + (self.face[k1][0] - self.face[k][0]) * u, self.face[k][1] + (self.face[k1][1] - self.face[k][1]) * u], self.face[k]);
        (p, f)
    }

    fn at(&self, t: f32) -> ([f32; 2], [f32; 2]) {
        if t >= 0.0 && t <= self.length {
            return self.within(t);
        }
        let (p0, f0) = self.within(0.0);
        let (p1, f1) = self.within(self.length);
        if self.looping && self.length > 1e-3 {
            let cycles = (t / self.length).floor();
            let tr = t - cycles * self.length;
            let d = [p1[0] - p0[0], p1[1] - p0[1]];
            let (p, f) = self.within(tr);
            return ([p[0] + d[0] * cycles, p[1] + d[1] * cycles], f);
        }
        let h = 0.1f32.min(self.length * 0.5).max(1e-3);
        if t > self.length {
            let (pa, _) = self.within(self.length - h);
            let v = [(p1[0] - pa[0]) / h, (p1[1] - pa[1]) / h];
            ([p1[0] + v[0] * (t - self.length), p1[1] + v[1] * (t - self.length)], f1)
        } else {
            let (pb, _) = self.within(h);
            let v = [(pb[0] - p0[0]) / h, (pb[1] - p0[1]) / h];
            ([p0[0] + v[0] * t, p0[1] + v[1] * t], f0)
        }
    }
}

/// A ground vector in the frame of a character that faces `f`.
fn local(f: [f32; 2], v: [f32; 2]) -> [f32; 2] {
    [v[0] * f[1] - v[1] * f[0], v[0] * f[0] + v[1] * f[1]]
}

/// Build the database of the clips of `rig` that pass `opt`.
pub fn build(rig: &Rig, opt: &BuildOptions) -> Result<MotionDb, String> {
    let (jl, jr, tl, tr) = (joint_index(rig, &opt.foot_l)?, joint_index(rig, &opt.foot_r)?, joint_index(rig, &opt.thigh_l)?, joint_index(rig, &opt.thigh_r)?);
    let rate = opt.rate;
    let h = 1.0 / rate;
    let mut clips = Vec::new();
    let mut times: Vec<f32> = Vec::new();
    let mut feat: Vec<f32> = Vec::new();
    for (ci, clip) in rig.clips.iter().enumerate() {
        if !opt.prefix.is_empty() && !clip.name.starts_with(&opt.prefix) {
            continue;
        }
        let length = clip.duration;
        if length < 2.0 * h {
            continue;
        }
        let n = (length * rate).floor() as usize + 1;
        let looping = clip.name.contains("Loop");
        // the root's path
        let mut root = RootPath { rate, length, looping, pos: Vec::with_capacity(n), face: Vec::with_capacity(n) };
        for k in 0..n {
            let (p, q) = rig.root_motion(ci, k as f32 * h);
            root.pos.push([p[0], p[2]]);
            let f = norm2([2.0 * (q[0] * q[2] + q[3] * q[1]), 1.0 - 2.0 * (q[0] * q[0] + q[1] * q[1])], [0.0, 1.0]);
            root.face.push(f);
        }
        // the root's track of an exported clip often stops one frame short of the poses' (the armature's curve has a key less than its bones): the last frame then repeats the one before and the
        // character seems to stop for 1/30 s at the end of every run cycle. Where the path moved and then stands still for exactly the last step, that step goes on at the pace before it.
        if root.pos.len() >= 3 {
            let m = root.pos.len();
            let step = |a: [f32; 2], b: [f32; 2]| [a[0] - b[0], a[1] - b[1]];
            let (last, before) = (step(root.pos[m - 1], root.pos[m - 2]), step(root.pos[m - 2], root.pos[m - 3]));
            if len2(last) < 1e-5 && len2(before) > 1e-3 {
                root.pos[m - 1] = [root.pos[m - 2][0] + before[0], root.pos[m - 2][1] + before[1]];
            }
        }
        // the pose at a time of the clip (the feet and thighs, model space)
        let pose = |t: f32| -> [V3; 4] {
            let w = rig.joint_worlds(&[PoseLayer { clip: Some(ci), time: t.clamp(0.0, length), looping: false, weight: 1.0, root: None, inertia: None }], &[]);
            [position(&w[jl]), position(&w[jr]), position(&w[tl]), position(&w[tr])]
        };
        let first = times.len() as u32;
        for k in 0..n {
            let t = k as f32 * h;
            let (p0, f0) = root.at(t);
            let rel = |tt: f32| {
                let (p, _) = root.at(tt);
                local(f0, [p[0] - p0[0], p[1] - p0[1]])
            };
            let vel = |tt: f32| {
                let (a, _) = root.at(tt - h);
                let (b, _) = root.at(tt + h);
                local(f0, [(b[0] - a[0]) / (2.0 * h), (b[1] - a[1]) / (2.0 * h)])
            };
            let face = |tt: f32| {
                let (_, f) = root.at(tt);
                local(f0, f)
            };
            let mut f = [0.0f32; DIM];
            let put = |f: &mut [f32; DIM], at: usize, v: [f32; 2]| {
                f[at] = v[0];
                f[at + 1] = v[1];
            };
            put(&mut f, 0, rel(t + OFFSETS[0]));
            put(&mut f, 2, vel(t + OFFSETS[1]));
            put(&mut f, 4, face(t + OFFSETS[1]));
            put(&mut f, 6, rel(t + OFFSETS[2]));
            put(&mut f, 8, face(t + OFFSETS[2]));
            put(&mut f, 10, rel(t + OFFSETS[3]));
            put(&mut f, 12, vel(t + OFFSETS[3]));
            put(&mut f, 14, face(t + OFFSETS[3]));
            // the pose
            let (now, before, after) = (pose(t), pose(t - h), pose(t + h));
            let dt2 = (t + h).min(length) - (t - h).max(0.0);
            let d = sub(now[0], now[1]);
            f[POSE_START] = d[0];
            f[POSE_START + 1] = d[1];
            f[POSE_START + 2] = d[2];
            for j in 0..2 {
                for a in 0..3 {
                    f[POSE_START + 3 + j * 3 + a] = (after[j][a] - before[j][a]) / dt2.max(1e-4);
                }
            }
            // the hips face where the line from the left thigh to the right thigh turned a quarter round says
            let r = sub(now[3], now[2]);
            let rl = (r[0] * r[0] + r[2] * r[2]).sqrt().max(1e-6);
            f[POSE_START + 9] = r[2] / rl;
            f[POSE_START + 10] = -r[0] / rl;
            times.push(t);
            feat.extend_from_slice(&f);
        }
        let tags = tags_of(&clip.name);
        let (from, to) = search_window(rig, ci, tags, length, rate);
        clips.push(DbClip { name: clip.name.clone(), length, looping, tags, first, count: n as u32, from, to });
    }
    if clips.is_empty() {
        return Err("no clip passes the prefix".into());
    }
    // the deviation of every group over the whole database; the two feet's velocities share theirs
    let frames = times.len();
    let mut groups = schema_groups();
    let dev = |from: usize, to: usize| -> f32 {
        let mut acc = 0.0f64;
        for d in from..to {
            let mean: f64 = (0..frames).map(|i| feat[i * DIM + d] as f64).sum::<f64>() / frames as f64;
            let var: f64 = (0..frames).map(|i| (feat[i * DIM + d] as f64 - mean).powi(2)).sum::<f64>() / frames as f64;
            acc += var;
        }
        let s = (acc / (to - from).max(1) as f64).sqrt() as f32;
        if s > 1e-4 {
            s
        } else {
            1.0
        }
    };
    let feet = dev(19, 25);
    for g in groups.iter_mut() {
        g.scale = if g.start == 19 || g.start == 22 { feet } else { dev(g.start as usize, (g.start + g.len) as usize) };
    }
    Ok(MotionDb::new(rate, TAGS.iter().map(|t| t.0.to_string()).collect(), groups, clips, times, feat))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::{Clip, Interp, Joint, Path, Track};

    fn rig_running(speed: f32) -> Rig {
        let j = |name: &str, x: f32| Joint { name: name.into(), parent: None, base: M4::identity(), t: [x, 0.0, 0.0], r: [0.0, 0.0, 0.0, 1.0], s: [1.0; 3], inverse_bind: M4::identity() };
        // the left thigh and foot are at +x, the right ones at -x: a character that faces +z has its left on the +x side in this model
        let joints = vec![j("foot_l", 0.1), j("foot_r", -0.1), j("thigh_l", 0.1), j("thigh_r", -0.1)];
        let root = vec![Track { joint: 0, path: Path::Translation, interp: Interp::Linear, times: vec![0.0, 2.0], values: vec![0.0, 0.0, 0.0, 0.0, 0.0, 2.0 * speed] }];
        let clip = Clip { name: "M_Neutral_Run_Loop_F".into(), duration: 2.0, tracks: Vec::new(), root };
        Rig { joints, order: vec![0, 1, 2, 3], clips: vec![clip], bind_pos: Vec::new(), bind_nrm: Vec::new(), vj: Vec::new(), vw: Vec::new() }
    }

    #[test]
    fn the_path_of_a_clip_becomes_its_trajectory_features() {
        let db = build(&rig_running(2.0), &BuildOptions::default()).unwrap();
        assert_eq!(db.clips.len(), 1);
        let c = &db.clips[0];
        assert!(c.looping && c.tags & (1 << 3) != 0, "a run loop: {c:?}");
        assert_eq!(c.count, 61);
        let f = db.frame(15);
        // velocity now, straight ahead (local z) at 2 m/s
        assert!(f[2].abs() < 1e-3 && (f[3] - 2.0).abs() < 1e-3, "vel {:?}", &f[2..4]);
        // 0.05 s ago it was 0.1 m behind; in 0.35 s 0.7 m ahead, in 0.7 s 1.4 m, going 2 m/s
        assert!((f[1] + 0.1).abs() < 1e-3 && (f[7] - 0.7).abs() < 1e-3 && (f[11] - 1.4).abs() < 1e-3 && (f[13] - 2.0).abs() < 1e-3, "{f:?}");
        // it always faces where it faces
        for at in [4, 8, 14] {
            assert!(f[at].abs() < 1e-5 && (f[at + 1] - 1.0).abs() < 1e-5);
        }
        // across the end of the loop the path goes on by its own cycle: the last frame still sees 1.4 m ahead
        let last = db.frame(60);
        assert!((last[11] - 1.4).abs() < 1e-2, "{:?}", &last[10..12]);
        // the pose: the left foot is 0.2 m to the +x of the right; the hips face +z (the line from the left thigh to the right thigh, turned a quarter)
        assert!((f[POSE_START] - 0.2).abs() < 1e-5 && f[POSE_START + 2].abs() < 1e-5);
        assert!(f[POSE_START + 9].abs() < 1e-4 && (f[POSE_START + 10] - 1.0).abs() < 1e-4, "{:?}", &f[POSE_START + 9..]);
        // the deviations are positive and the feet share one
        assert!(db.groups.iter().all(|g| g.scale > 0.0));
        assert_eq!(db.groups[9].scale, db.groups[10].scale);
    }

    #[test]
    fn the_names_of_the_sample_clips_put_them_in_their_kinds() {
        let kind = |name: &str| TAGS.iter().enumerate().filter(|(_, t)| t.1.iter().any(|w| name.contains(w))).map(|(i, _)| t_name(i)).collect::<Vec<_>>();
        fn t_name(i: usize) -> &'static str {
            TAGS[i].0
        }
        assert_eq!(kind("M_Neutral_Jump_F_Land_Roll_Lfoot"), ["land_roll"]);
        assert_eq!(kind("M_Neutral_Jump_F_Land_Stumble_Rfoot"), ["land_stumble"]);
        assert_eq!(kind("M_Neutral_Jump_F_Land_Sprint_Heavy_Lfoot"), ["land_heavy"]);
        assert_eq!(kind("M_Neutral_Jump_F_Land_Sprint_Light_Rfoot"), ["land_light"]);
        assert_eq!(kind("M_Neutral_Sprint_Loop_FL"), ["sprint_loop"]);
        assert_eq!(kind("M_Neutral_Sprint_Start_F_Lfoot"), ["sprint_start"]);
        assert_eq!(kind("M_Neutral_Sprint_Reface_Start_F_L_090"), ["sprint_start"]);
        assert_eq!(kind("M_Neutral_Sprint_Stop_FR_Rfoot"), ["sprint_stop"]);
        assert_eq!(kind("M_Neutral_Sprint_Turn_L_180_Lfoot"), ["sprint_pivot"]);
        assert_eq!(kind("M_Neutral_Sprint_Diamond_FL_FR_Lfoot"), ["sprint_pivot"]);
        // the take-off of a sprinting jump is a jump start, not a sprint start
        assert_eq!(kind("M_Neutral_Jump_F_Start_Sprint_Lfoot"), ["jump_start"]);
        // what has no kind is `other` (the last), and a transition says so too
        assert_eq!(tags_of("M_Neutral_Slide_Loop"), 1 << (TAGS.len() - 1));
        assert_eq!(tags_of("M_Neutral_Transition_Run_to_Sprint_Lfoot"), 1 << (TAGS.len() - 1));
        // a crouch has its own kinds besides `crouch`
        assert_eq!(kind("M_Neutral_Crouch_Loop_BL"), ["crouch", "crouch_loop"]);
        assert_eq!(kind("M_Neutral_Crouch_Start_F_Lfoot"), ["crouch", "crouch_start"]);
        assert_eq!(kind("M_Neutral_Crouch_Stop_LL_Rfoot"), ["crouch", "crouch_stop"]);
        assert_eq!(kind("M_Neutral_Crouch_Pivot_F_B_Rfoot"), ["crouch", "crouch_pivot"]);
        assert_eq!(kind("M_Neutral_Crouch_Turn_L_090_Lfoot"), ["crouch", "crouch_pivot"]);
        assert_eq!(kind("M_Neutral_Crouch_Idle_Turn_135_L"), ["crouch", "crouch_turn"]);
        assert_eq!(kind("M_Neutral_Crouch_Idle_Loop"), ["crouch", "crouch_idle"]);
        // the bits the game's scripts name (idle 0 ... land_heavy 15) do not move when kinds are added after them
        assert_eq!(tag_bit("run_loop"), 1 << 3);
        assert_eq!(tag_bit("land_heavy"), 1 << 15);
        assert_eq!(tag_bit("land_roll"), 1 << 16);
        assert_eq!(tag_bit("sprint_pivot"), 1 << 21);
        assert_eq!(tag_bit("crouch_idle"), 1 << 22);
    }

    #[test]
    fn a_prefix_picks_the_clips_and_no_match_is_an_error() {
        let rig = rig_running(1.0);
        assert!(build(&rig, &BuildOptions { prefix: "Walk".into(), ..Default::default() }).is_err());
        assert!(build(&rig, &BuildOptions { prefix: "M_Neutral_".into(), ..Default::default() }).is_ok());
        let mut missing = rig_running(1.0);
        missing.joints[0].name = "other".into();
        assert!(build(&missing, &BuildOptions::default()).unwrap_err().contains("foot_l"));
    }

    #[test]
    fn a_root_track_that_stops_one_frame_short_does_not_stall_the_cycle() {
        // 61 keys, every 1/30 s, 2 m/s, but the last key repeats the one before (the armature's curve is a key shorter than its bones')
        let mut rig = rig_running(2.0);
        let (times, values): (Vec<f32>, Vec<f32>) = (0..61).map(|k| (k as f32 / 30.0, [0.0, 0.0, 2.0 * (k.min(59) as f32) / 30.0])).fold((Vec::new(), Vec::new()), |(mut t, mut v), (a, b)| {
            t.push(a);
            v.extend_from_slice(&b);
            (t, v)
        });
        rig.clips[0].root = vec![Track { joint: 0, path: Path::Translation, interp: Interp::Linear, times, values }];
        let db = build(&rig, &BuildOptions::default()).unwrap();
        // frame 39 looks 0.7 s ahead: just across the end of the cycle, where the stall would show as a slower velocity
        let f = db.frame(39);
        assert!((f[13] - 2.0).abs() < 1e-2, "velocity across the end of the cycle: {:?}", &f[12..14]);
        assert!((f[11] - 1.4).abs() < 1e-2, "position across the end of the cycle: {:?}", &f[10..12]);
    }
}
