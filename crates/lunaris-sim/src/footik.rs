//! Feet on the ground: `FootIk` makes a walking, running or stair-climbing model stand on what it walks over. A clip plays the same whatever is under it; here the engine knows where the clip puts each foot (a table made
//! by `lunaris footpaths` for the clips of the model, and for clips made elsewhere by the tool that made them), asks the ground what is under that place (a ray down: boxes, stairs, terrain, meshes) and
//!
//! 1. lowers or raises the **model** (its `LocalTransform` y) so that its origin follows the ground under it, smoothed: the capsule of a `CharacterBody` jumps up a stair at a time, the picture of the character rises
//!    smoothly with the slope the stairs make. The height is the average of the ground along the way it goes (the line the stairs make), kept within reach of the planted feet (a leg never has to stretch or fold more
//!    than a hand's breadth);
//! 2. writes two goals of inverse kinematics (`Ik` kind 2, pins) for each foot: one that only keeps the foot out of the ground (the lowest of its heel, ankle, ball and toe is on or above it: the toes do not go
//!    through the riser of the next step) and one that, while the clip has the foot planted (and a moment before, and a moment after), puts it on the ground **where it fits**: a foot that would land across the edge of a
//!    step is moved along the way the character goes, onto the tread, and held there (a lock, like Unreal's foot placement `plant`) until the clip lifts it, so a planted foot rests on one step, whether the step is
//!    higher or lower than the clip expects, and does not slide.
//!
//! It also looks ahead along the way the character goes (rays down at three places in front of it, one behind) and says whether the character is on, or about to walk onto, **stairs** (`ahead`: 1 up, -1 down, 0 none:
//! two steps in a row in the same direction, not a single kerb, and not a slope: the ground it sees is flat treads): what a game needs to pick a stair clip and to hold the speed to a stair's pace.
//!
//! **The gait** (hybrid: the clip for the body, a planner for the legs). While a game sets `drive` (on stairs) the clip no longer chooses where the feet go: a planner does. It walks by the distance the character covers
//! (a phase `theta` that goes on by a step's length over two steps: the feet land on treads whatever the speed, and stand still when the character does), puts each foot on the tread a step ahead of the other (found
//! with the rays, fitted on the tread), carries it there in an arc that clears the riser, and plays the clip at that phase, so that its arms and body swing with the feet that are really in front. The legs themselves
//! are bent by the goals (the clip's knees). Going down it does the same: no clip of descending stairs is needed.
//!
//! The picture only: the simulation never sees the bent legs. It is off while the character is not on the ground, while a `Traversal` plays and when a game sets `weight` to 0 (a slide). Everything is f32 with `+ - * /`,
//! `sqrt` and the engine's own `sin` and `cos`: the same on every machine.
use crate::components::{Animator, CharacterBody, Ik, LocalTransform, Parent, Transform};
use crate::detmath;
use crate::inertia::{Inertia, MAX as INERTIA_MAX};
use crate::layers::Layers;
use crate::meshcol::MeshStore;
use crate::terrain::Terrains;
use crate::traversal::{set_ik, Traversal};
use crate::vehicle_ray::RayWorld;
use lunaris_ecs::{component, Entity, Sym, Uid, World};
use lunaris_json::{parse, Json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

component! {
    /// Feet on the ground (see `docs/GAMEPLAY.md`, "Feet on the ground"), on the entity of the model (with its `Animator`, a `Parent` that is the character with the `CharacterBody`). `data` is the table of where the clips
    /// put the feet (`lunaris footpaths`); `goal_l` and `goal_r` are the uids of two entities with an `Ik` (the foot joint of the model, `kind` 2, `axis` up) that hold a planted foot on the ground, `floor_l` and `floor_r` two that keep
    /// a foot out of it. `ankle` is how high the foot joint is above the sole, `smooth` the time (s) the model takes to follow the ground, `follow` (0..1) how much it does, `root_lift` a lift of the model's origin above the
    /// mean ground under the feet (for a table whose clips were made with the origin at another height), `max_drop` how far the ground may be from the capsule's before the model stops following. The last fields are the
    /// engine's: `gs` the height of the model's origin, `off` how far it is from the capsule's, `live` whether it runs, and the corrections `lift_l` and `lift_r` that the goals make to the feet now. `look` is how far ahead (m, plus
    /// a fifth of the speed) the first of the three rays that look for stairs is (the others half a metre and a metre further), `ahead` what they found (1 stairs up, -1 stairs down, 0 none), `rise` the height of the ground at the first and `behind` the height of the ground half a metre behind (both above the capsule's feet): a game that waits for the first riser to be near looks at `rise`, one that wants to know it is on a flight at `behind`. `heel` and `toe` are how far the sole reaches behind and in front of the foot joint (m), `slack_up` and `slack_down` how far the model's origin may be above or below what the planted feet need (the reach of the legs), `release` how long (s) a foot takes to go back to the clip after it was held, `lead` how long before the clip plants a foot (s) the hold begins to take it; `drive` is what a game sets to have the **gait** walk the legs (below; `gait` says it does, `theta` is its phase, `pitch` the distance between its steps, `pace` the speed it walks at (smoothed), `swing` the share of a cycle a foot is in the air, `arc` how high over the higher end of a step the foot is carried, `gait_lift` how far above the middle of the feet (less half a step's height: see the gait) the origin is kept in the gait, `sw_*` how far each foot is in its step (-1 on its tread), `from_*`, `to_*` and `tg_*` where a step began, where it ends and where the foot is now); `stride_down` how much of its stride a walking clip keeps going down a flight (a stride of a metre does not fit two treads: the feet are put nearer the body, the way Unreal's stride warping does), `stride` the factor now; `stance_y`, `line_y`, `line_s` (the line smoothed), `gs_v` (the speed of the origin), `bias`, `gw_*`, `wk` (how far the goals have come on since the character landed), `pole_l` and `pole_r` (the uids of two entities, one for each knee, that the engine keeps ahead of the character at the height of its hips: the knees bend towards them, so that a knee never bends backwards), `smx`, `gt_*`, `gv_*` and `gt_ok_*` (the smoothing of the goals after the gait comes on or goes off), `alh_*` (how long the pin along a foot's sole is kept after the foot was last fitted to a tread, so that it is let go of when the goal has come back to the clip's foot), `fy_*` (the height of the floor under each foot, as it is asked for: it rises over a twentieth of a second, so that a foot that crosses the edge of a step is not thrown up the height of it in one picture), `locked_*`, `rel_*` (how long the clip has not had the foot planted) and `lk_*` are the engine's own (the origin the planted feet ask for, the average ground along the way, how far the origin the planted feet ask for is from that average, on average, how strongly each foot is held and where).
    pub struct FootIk {
        pub data: Sym = Sym(0),
        pub goal_l: Uid = Uid(0),
        pub goal_r: Uid = Uid(0),
        pub floor_l: Uid = Uid(0),
        pub floor_r: Uid = Uid(0),
        pub weight: f32 = 1.0,
        pub ankle: f32 = 0.09,
        pub smooth: f32 = 0.1,
        pub follow: f32 = 1.0,
        pub root_lift: f32 = 0.0,
        pub max_drop: f32 = 0.5,
        pub gs: f32 = 0.0,
        pub off: f32 = 0.0,
        pub live: bool = false,
        pub lift_l: f32 = 0.0,
        pub lift_r: f32 = 0.0,
        pub look: f32 = 0.35,
        pub ahead: f32 = 0.0,
        pub rise: f32 = 0.0,
        pub behind: f32 = 0.0,
        pub heel: f32 = 0.075,
        pub toe: f32 = 0.21,
        pub slack_up: f32 = 0.06,
        pub slack_down: f32 = 0.25,
        pub release: f32 = 0.2,
        pub lead: f32 = 0.15,
        pub stance_y: f32 = 0.0,
        pub line_y: f32 = 0.0,
        pub line_s: f32 = 0.0,
        pub gs_v: f32 = 0.0,
        pub bias: f32 = 0.0,
        pub stride_down: f32 = 1.0,
        pub stride: f32 = 1.0,
        pub gw_l: f32 = 0.0,
        pub gw_r: f32 = 0.0,
        pub wk: f32 = 0.0,
        // how long the goals have had ground under the character (s): they come on only after `TOUCH_GRACE`
        pub td: f32 = 0.0,
        pub pole_l: Uid = Uid(0),
        pub pole_r: Uid = Uid(0),
        pub smx: f32 = 0.0,
        pub gt_l: [f32; 3] = [0.0, 0.0, 0.0],
        pub gt_r: [f32; 3] = [0.0, 0.0, 0.0],
        pub gv_l: [f32; 3] = [0.0, 0.0, 0.0],
        pub gv_r: [f32; 3] = [0.0, 0.0, 0.0],
        pub gt_ok_l: bool = false,
        pub gt_ok_r: bool = false,
        pub alh_l: f32 = 0.0,
        pub alh_r: f32 = 0.0,
        // where a goal was the last time its foot had ground under it, and how much of its weight is left while it fades out after the ground has gone (the foot over the edge of its platform)
        pub hold_l: [f32; 3] = [0.0, 0.0, 0.0],
        pub hold_r: [f32; 3] = [0.0, 0.0, 0.0],
        pub edge_l: f32 = 0.0,
        pub edge_r: f32 = 0.0,
        pub fy_l: f32 = 0.0,
        pub fy_r: f32 = 0.0,
        pub fy_ok_l: bool = false,
        pub fy_ok_r: bool = false,
        pub locked_l: bool = false,
        pub locked_r: bool = false,
        pub rel_l: f32 = 0.0,
        pub rel_r: f32 = 0.0,
        pub lk_l: [f32; 2] = [0.0, 0.0],
        pub lk_r: [f32; 2] = [0.0, 0.0],
        pub drive: bool = false,
        pub gait: bool = false,
        pub theta: f32 = 0.0,
        pub pitch: f32 = 0.25,
        pub swing: f32 = 0.42,
        pub arc: f32 = 0.04,
        pub gait_lift: f32 = 0.0,
        pub prev: [f32; 2] = [0.0, 0.0],
        pub pace: f32 = 0.0,
        pub sw_l: f32 = -1.0,
        pub sw_r: f32 = -1.0,
        pub from_l: [f32; 3] = [0.0, 0.0, 0.0],
        pub from_r: [f32; 3] = [0.0, 0.0, 0.0],
        pub to_l: [f32; 3] = [0.0, 0.0, 0.0],
        pub to_r: [f32; 3] = [0.0, 0.0, 0.0],
        pub tg_l: [f32; 3] = [0.0, 0.0, 0.0],
        pub tg_r: [f32; 3] = [0.0, 0.0, 0.0],
    }
}

/// Where the feet are in one clip, every 1/rate seconds: model-space positions (x left, y up, z forward, metres) and whether each is planted (0..1).
#[derive(Clone, Debug, Default)]
pub struct FootClip {
    /// When (s) the clip's left foot leaves the ground: the gait plays the clip from there, so that the clip's left foot is in the air when the gait's is. `cycle` is how long a cycle of the clip is (two steps).
    pub t_l0: f32,
    pub cycle: f32,
    /// How much higher (m) a treadmill clip's origin sits above the line of the ground under it than the planted feet ask (a stair clip made with the origin at another height than the stairs' line).
    pub lift: f32,
    /// The clip is a treadmill (its body keeps its height while the feet go down under it: a stair clip): the model's height is then worked out from the planted feet, which is exact, instead of following the mean ground.
    pub stance: bool,
    pub dur: f32,
    pub l: Vec<[f32; 3]>,
    pub r: Vec<[f32; 3]>,
    pub cl: Vec<f32>,
    pub cr: Vec<f32>,
    /// How far below the foot joint the lowest of the heel and the toe is, every frame (empty: the ankle's height); a foot whose toe or heel is raised has its joint higher over the tread.
    pub dl: Vec<f32>,
    pub dr: Vec<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct FootData {
    pub rate: f32,
    pub clips: HashMap<String, FootClip>,
}

fn points(j: Option<&Json>) -> Vec<[f32; 3]> {
    j.and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .map(|p| {
                    let v = p.as_array().unwrap_or(&[]);
                    let g = |i: usize| v.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
                    [g(0), g(1), g(2)]
                })
                .collect()
        })
        .unwrap_or_default()
}

fn flags(j: Option<&Json>) -> Vec<f32> {
    j.and_then(|a| a.as_array()).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default()
}

impl FootData {
    pub fn from_json(text: &str) -> Result<FootData, String> {
        let j = parse(text).map_err(|e| e.message)?;
        let rate = j.get("rate").and_then(|r| r.as_f64()).unwrap_or(30.0) as f32;
        let clips = j.get("clips").and_then(|c| c.as_object()).ok_or("the file has no `clips`")?;
        let mut out = HashMap::new();
        for (name, c) in clips {
            let l = points(c.get("l"));
            let r = points(c.get("r"));
            if l.len() < 2 || l.len() != r.len() {
                continue;
            }
            let dl = flags(c.get("dl"));
            let dr = flags(c.get("dr"));
            let mut cl = flags(c.get("cl"));
            let mut cr = flags(c.get("cr"));
            cl.resize(l.len(), 0.0);
            cr.resize(l.len(), 0.0);
            let n = cl.len();
            let dur = c.get("dur").and_then(|d| d.as_f64()).unwrap_or(0.0) as f32;
            // (a loop of several steps, like the walk loop of the Game Animation Sample's 4 s, has the left foot leave the ground several times: the gait's cycle is the time between two)
            let lifts: Vec<usize> = (0..n.saturating_sub(1)).filter(|k| cl[(k + n - 2) % (n - 1).max(1)] >= 0.5 && cl[*k] < 0.5).collect();
            let at_s = |k: usize| k as f32 / (n - 1).max(1) as f32 * dur;
            // (the cycles of a loop of mocap are not all of one length, and a cycle of the average length that begins at a lift-off ends in the middle of a stride: the clip would jump by half a stride at every wrap of the gait's
            // phase. The cycle is taken between two real lift-offs, the pair whose distance is nearest the average)
            let mean = if lifts.is_empty() { dur } else { dur / lifts.len() as f32 };
            let (t_l0, cycle) = match lifts.windows(2).map(|w| (at_s(w[0]), at_s(w[1]) - at_s(w[0]))).min_by(|a, b| (a.1 - mean).abs().total_cmp(&(b.1 - mean).abs())) {
                Some(pair) => pair,
                None => (lifts.first().map_or(0.0, |k| at_s(*k)), mean),
            };
            out.insert(name.clone(), FootClip { dl, dr, t_l0, cycle, lift: c.get("lift").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32, stance: c.get("stance").and_then(|x| x.as_f64()).unwrap_or(0.0) > 0.5, dur: c.get("dur").and_then(|d| d.as_f64()).unwrap_or(0.0) as f32, l, r, cl, cr });
        }
        Ok(FootData { rate, clips: out })
    }
}

fn store() -> &'static Mutex<HashMap<PathBuf, Result<Arc<FootData>, String>>> {
    static S: OnceLock<Mutex<HashMap<PathBuf, Result<Arc<FootData>, String>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The foot table file `name` of the workspace (read once, then shared).
pub fn load(root: &Path, name: &str) -> Result<Arc<FootData>, String> {
    if name.is_empty() || name.contains("..") || name.contains(':') || name.starts_with('/') || name.starts_with('\\') {
        return Err(format!("`{name}` is not a file of the workspace"));
    }
    let path = root.join(name);
    let mut s = store().lock().map_err(|_| "the store is poisoned".to_string())?;
    if let Some(r) = s.get(&path) {
        return r.clone();
    }
    let r = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display())).and_then(|t| FootData::from_json(&t)).map(Arc::new);
    s.insert(path, r.clone());
    r
}

/// The feet of a clip at time `t` (looping or clamped): the two positions and how planted each is.
pub fn sample(d: &FootData, c: &FootClip, t: f32, looping: bool) -> ([f32; 3], [f32; 3], f32, f32) {
    let n = c.l.len();
    let dur = if c.dur > 0.0 { c.dur } else { (n - 1) as f32 / d.rate.max(1.0) };
    let t = if looping && dur > 0.0 { t.rem_euclid(dur) } else { t.clamp(0.0, dur) };
    let x = if dur > 0.0 { t / dur * (n - 1) as f32 } else { 0.0 };
    let k = (x.floor() as usize).min(n - 1);
    let k1 = (k + 1).min(n - 1);
    let u = x - k as f32;
    let lerp3 = |a: [f32; 3], b: [f32; 3]| [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u, a[2] + (b[2] - a[2]) * u];
    (lerp3(c.l[k], c.l[k1]), lerp3(c.r[k], c.r[k1]), c.cl[k] + (c.cl[k1] - c.cl[k]) * u, c.cr[k] + (c.cr[k1] - c.cr[k]) * u)
}

/// How far below each foot joint the lowest of the heel and toe is in a clip at time `t`, if the table says (else `ankle`).
pub fn drops(c: &FootClip, t: f32, looping: bool, ankle: f32) -> (f32, f32) {
    let at = |v: &Vec<f32>| -> f32 {
        let n = v.len();
        if n < 2 || c.dur <= 0.0 {
            return ankle;
        }
        let t = if looping { t.rem_euclid(c.dur) } else { t.clamp(0.0, c.dur) };
        let x = t / c.dur * (n - 1) as f32;
        let k = (x.floor() as usize).min(n - 1);
        let k1 = (k + 1).min(n - 1);
        v[k] + (v[k1] - v[k]) * (x - k as f32)
    };
    (at(&c.dl), at(&c.dr))
}

fn yaw_of(q: [f32; 4]) -> f32 {
    2.0 * detmath::atan2(q[1] as f64, q[3] as f64) as f32
}

/// A critically damped spring: `current` goes to `target` with no overshoot and no jump of speed (`vel` is carried from tick to tick), in about `smooth_time` seconds. (The polynomial for the exponential is Unity's SmoothDamp's.)
fn smooth_damp(current: f32, target: f32, vel: &mut f32, smooth_time: f32, dt: f32) -> f32 {
    let omega = 2.0 / smooth_time.max(1.0e-3);
    let x = omega * dt;
    let k = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (*vel + omega * change) * dt;
    *vel = (*vel - omega * temp) * k;
    target + (change + temp) * k
}

/// How fast (m/s) the goal of a foot that the clip walks may move along the ground where the foot is pinned, and in height.
const FLAT_SLEW: f32 = 3.5;
const FLAT_SLEW_UP: f32 = 5.0;
/// How long (s) the pin along a foot's sole is kept after the foot was last fitted to a tread.
const ALONG_HOLD: f32 = 0.25;
/// Where the poles of the knees are: ahead of the character (m), at the height of its hips above the model's origin, and apart (m, either side of its middle).
const POLE_AHEAD: f32 = 1.0;
const POLE_HEIGHT: f32 = 0.9;
const POLE_APART: f32 = 0.09;
/// How far (metres) a locked foot may be held from the place the clip has it.
const LOCK_LEASH: f32 = 0.35;
/// How far (m) the ground under a foot may be from the character's own for the foot to follow it: a foot over the edge of the platform the character stands on, or a step too high to climb, is not.
const EDGE_DROP: f32 = 0.7;
/// How long (s) a goal whose foot has lost the ground under it (it went over the edge of the platform) takes to fade out, where it was: the foot goes back to the clip's pose smoothly and not in one picture.
const EDGE_FADE: f32 = 0.15;
/// The goals wait this long (s) after the character touches down before they come on: the state of the game that picks the landing clip (a roll, a stumble) runs a picture behind the touch-down, and a goal that came on
/// for that picture held the foot to the old clip's place (up to the lock's leash away) and let go of it again in the next: a foot that went back and forward a few tens of centimetres as the character landed.
const TOUCH_GRACE: f32 = 0.05;
/// Where the ground is read along the way the character goes to find the line it makes (metres from the character, ahead positive): spread over nearly two metres, at spacings that do not repeat the pitch of any stairs.
const LINE_AT: [f32; 11] = [-0.93, -0.71, -0.52, -0.33, -0.14, 0.04, 0.19, 0.37, 0.56, 0.75, 0.94];

/// Where the foot that has its joint at `a` fits: how far along `fwd` (m) to shift it so that the whole foot (from `heel` behind the joint to `toe` in front of it) stands on one tread. The ground is read at 2.5 cm steps
/// from the middle of the foot outwards until it jumps (a riser, an edge, nothing): a slope has no jump, so a foot on a slope is not shifted. Where the ground beyond the jump is higher (the riser of the next step going up,
/// the step behind going down) the foot must stop at the wall; where it is lower (the nose of a step) the foot may hang over it by 3 cm. A tread too short for the foot gets the foot in the middle of it.
fn fit_tread(ground: &dyn Fn(f32, f32) -> Option<f32>, a: [f32; 2], fwd: [f32; 2], heel: f32, toe: f32) -> f32 {
    const STEP: f32 = 0.025;
    const JUMP: f32 = 0.06;
    const OVER: f32 = 0.04;
    let at = |u: f32| ground(a[0] + fwd[0] * u, a[1] + fwd[1] * u);
    let mid = (toe - heel) * 0.5;
    let Some(h0) = at(mid) else { return 0.0 };
    // (the end of the tread in one direction, found to a few millimetres by halving the last step, and how far the foot may hang over it there)
    let edge = |sign: f32| -> (f32, f32) {
        let mut prev = h0;
        let mut last = mid;
        for k in 1..=12 {
            let u = mid + sign * STEP * k as f32;
            match at(u) {
                Some(h) if (h - prev).abs() <= JUMP => {
                    prev = h;
                    last = u;
                }
                beyond => {
                    let wall = beyond.is_some_and(|h| h > prev);
                    let (mut on, mut off) = (last, u);
                    for _ in 0..4 {
                        let m = (on + off) * 0.5;
                        match at(m) {
                            Some(h) if (h - prev).abs() <= JUMP => on = m,
                            _ => off = m,
                        }
                    }
                    return ((on + off) * 0.5, if wall { 0.0 } else { OVER });
                }
            }
        }
        (mid + sign * 0.3, OVER)
    };
    let (hi, over_toe) = edge(1.0);
    let (lo, over_heel) = edge(-1.0);
    let (s_min, s_max) = (lo + heel - over_heel, hi - toe + over_toe);
    let s = if s_min <= s_max { 0.0f32.clamp(s_min, s_max) } else { (s_min + s_max) * 0.5 };
    s.clamp(-0.2, 0.2)
}

/// Run every `FootIk`: put the model and the goals where the ground is (after motion matching and the traversal have chosen the clip and its time).
pub fn update(world: &mut World, root: Option<&Path>, fields: &Terrains, meshes: &MeshStore, dt: f32) {
    let mut found: Vec<(u64, Entity, FootIk)> = Vec::new();
    let _ = world.for_each_uid::<(&FootIk,), _>(|e, uid, (f,)| found.push((uid, e, *f)));
    if found.is_empty() {
        return;
    }
    found.sort_by_key(|f| f.0);
    let Some(root) = root else { return };
    // only what is near the characters can be under a foot or in the way of a lookahead ray (3 m ahead at a run, 2.4 m down, 2.2 m up): the world is gathered for that box, not whole
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for (_, e, _) in &found {
        let parent = world.get::<Parent>(*e).map(|p| p.target.0).unwrap_or(0);
        if let Some(t) = world.entity(parent).and_then(|pe| world.get::<Transform>(pe).map(|t| t.pos)) {
            for a in 0..3 {
                lo[a] = lo[a].min(t[a] - 6.0);
                hi[a] = hi[a].max(t[a] + 6.0);
            }
        }
    }
    if lo[0] > hi[0] {
        return;
    }
    let ray = RayWorld::gather(world, fields, meshes, Some((lo, hi)));
    let layers = Layers::gather(world);
    for (uid, e, mut fk) in found {
        let name = world.strings().get(fk.data.0).to_string();
        let Ok(data) = load(root, &name) else { continue };
        let parent = world.get::<Parent>(e).map(|p| p.target.0).unwrap_or(0);
        let Some(pe) = world.entity(parent) else { continue };
        let (Some(a), Some(ht)) = (world.get::<Animator>(e).map(|x| *x), world.get::<Transform>(pe).map(|x| *x)) else { continue };
        let local = world.get::<LocalTransform>(e).map(|l| *l).unwrap_or_default();
        let grounded = world.get::<CharacterBody>(pe).map_or(true, |c| c.grounded);
        let traversing = world.get::<Traversal>(e).is_some_and(|t| t.active);
        let clip_name = world.strings().get(a.clip.0).to_string();
        let w = if grounded && !traversing && fk.weight > 0.0 { fk.weight.min(1.0) } else { 0.0 };
        // After a switch of clips the pose on the screen is the new clip offset by what separated it from the old pose (inertialization), and the offset dies away over `span`: a foot that the new clip has planted
        // may still be in the air on the screen. The hold on a foot takes as much of it as the offset has gone (the same quintic curve), or it would pull the foot to the ground in one picture.
        let settle = if fk.gait {
            1.0
        } else {
            world.get::<Inertia>(e).map_or(1.0, |ch| {
                let n = (ch.n as usize).min(INERTIA_MAX);
                if n > 1 && ch.len[n - 1] > 1.0e-4 {
                    let x = (ch.gap[n - 1] / ch.len[n - 1]).clamp(0.0, 1.0);
                    x * x * x * (x * (x * 6.0 - 15.0) + 10.0)
                } else {
                    1.0
                }
            })
        };
        let table = data.clips.get(&clip_name);
        let hero = ht.pos;
        let who = layers.of(uid);
        // the ground's height under a place (absolute), from above the character's head (the rear foot on a flight down is up to a metre above it) down to well below its feet
        let ground = |x: f32, z: f32| ray.cast([x, hero[1] + 2.0, z], [0.0, -1.0, 0.0], 4.6, who, uid).map(|h| hero[1] + 2.0 - h.t);
        // stairs: the ground at three places ahead along the way the character goes and one behind (heights above the feet); two steps in a row in one direction, or being on a flight already, is a stair
        let walk = world.get::<CharacterBody>(pe).map(|c| c.walk).unwrap_or([0.0; 2]);
        let speed = (walk[0] * walk[0] + walk[1] * walk[1]).sqrt();
        fk.ahead = 0.0;
        fk.rise = 0.0;
        fk.behind = 0.0;
        if grounded && !traversing && speed > 0.05 && fk.weight > 0.0 {
            let dir = [walk[0] / speed, walk[1] / speed];
            // (the height above the feet of the ground there, if it is a flat tread: stairs are made of those, a slope of terrain or a ramp is not)
            let at = |d: f32| {
                ray.cast([hero[0] + dir[0] * d, hero[1] + 2.2, hero[2] + dir[1] * d], [0.0, -1.0, 0.0], 4.6, who, uid).filter(|h| h.normal[1] > 0.985).map(|h| hero[1] + 2.2 - h.t - hero[1])
            };
            let l1 = fk.look + 0.2 * speed.min(6.0);
            if let (Some(back), Some(r1), Some(r2), Some(r3)) = (at(-0.5), at(l1), at(l1 + 0.5), at(l1 + 1.0)) {
                fk.rise = r1;
                fk.behind = back;
                let (s1, s2) = (r2 - r1, r3 - r2);
                let flight = |lo: f32, hi: f32, v: f32| v >= lo && v <= hi;
                if (flight(0.15, 0.6, s1) && flight(0.15, 0.6, s2) && r1 <= 1.2) || (back <= -0.15 && back >= -0.9 && flight(0.1, 1.2, r1)) {
                    fk.ahead = 1.0;
                } else if (flight(-0.6, -0.15, s1) && flight(-0.6, -0.15, s2) && r1 >= -1.2) || (back >= 0.15 && back <= 0.9 && flight(-1.2, -0.1, r1)) {
                    fk.ahead = -1.0;
                }
            }
        }
        let goals = [fk.goal_l, fk.goal_r, fk.floor_l, fk.floor_r];
        let off_goals = |world: &mut World| {
            for g in goals {
                set_ik(world, g, [0.0; 3], [0.0, 1.0, 0.0], [0.0; 3], false, 0.0);
            }
        };
        let Some(clip) = table.filter(|_| w > 0.001) else {
            // off: the goals are off and the model comes back to the capsule (at once under a traversal, which sets the model's height itself)
            if traversing {
                fk.off = 0.0;
            } else {
                fk.off = if fk.off.abs() < 0.003 { 0.0 } else { fk.off * (1.0 - (dt * 10.0).min(1.0)) };
            }
            let wrote = fk.live || fk.off != 0.0;
            fk.live = false;
            fk.gs = hero[1] + fk.off;
            fk.lift_l = 0.0;
            fk.lift_r = 0.0;
            (fk.gw_l, fk.gw_r, fk.wk, fk.locked_l, fk.locked_r, fk.bias, fk.gait) = (0.0, 0.0, 0.0, false, false, 0.0, false);
            fk.td = 0.0;
            if wrote && !traversing {
                if let Some(l) = world.get_mut::<LocalTransform>(e) {
                    l.pos[1] = fk.off;
                }
            }
            off_goals(world);
            if let Some(f) = world.get_mut::<FootIk>(e) {
                *f = fk;
            }
            continue;
        };
        // ---- the way the model faces on the ground: the sole lies along it, and a foot is moved along it to fit a tread
        let yaw = yaw_of(ht.rot) + yaw_of(local.rot);
        let (sn, cs) = (detmath::sin(yaw as f64) as f32, detmath::cos(yaw as f64) as f32);
        let place = |p: [f32; 3]| [hero[0] + p[0] * cs + p[2] * sn, hero[2] - p[0] * sn + p[2] * cs];
        let fwd = [sn, cs];
        let on = |p: [f32; 2], u: f32| [p[0] + fwd[0] * u, p[1] + fwd[1] * u];
        // (the sole counts 2 cm shorter at each end than the foot: a toe that touches a riser by a few millimetres must not take the whole foot up to the next step)
        let (heel_e, toe_e) = (fk.heel - 0.02, fk.toe - 0.02);
        let ball = fk.toe * 0.64;
        // the highest ground under the heel, the joint, the ball and the toe of a foot whose joint is at `p`
        let support = |p: [f32; 2]| -> Option<f32> {
            let mut h: Option<f32> = None;
            for u in [-heel_e, 0.0, ball, toe_e] {
                let q = on(p, u);
                if let Some(g) = ground(q[0], q[1]) {
                    h = Some(h.map_or(g, |m: f32| m.max(g)));
                }
            }
            h
        };
        // where a foot whose joint wants to be at `p` fits on the tread under it
        let fit = |p: [f32; 2]| -> [f32; 2] { on(p, fit_tread(&ground, p, fwd, heel_e + 0.01, toe_e + 0.01)) };

        // ---- the gait: on stairs (a game says so with `drive`) the legs are walked by a planner instead of the clip. The clip is played at the planner's phase (`theta`, which goes on by the distance the character
        // walks: two steps of the stairs make a cycle) so that its arms and body swing with the feet that are really in front; each foot is put on the next tread and carried there in an arc; the clip's legs are only the
        // shape the goals bend (the knees).
        let dur = clip.dur.max(1.0e-3);
        let cycle = if clip.cycle > 0.05 { clip.cycle } else { dur };
        let clip_time = |theta: f32| (clip.t_l0 + theta * cycle).rem_euclid(dur);
        let swing = fk.swing.clamp(0.2, 0.48);
        if fk.drive && !fk.gait {
            // (starting: both feet on the treads under them, at the phase where both are down)
            fk.gait = true;
            fk.smx = 1.0;
            fk.theta = 0.5 - (0.5 - swing) * 0.5;
            fk.prev = [hero[0], hero[2]];
            let (p0l, p0r, _, _) = sample(&data, clip, clip_time(fk.theta), true);
            for (side, p) in [(0usize, p0l), (1, p0r)] {
                let xz = fit(place(p));
                let h = support(xz).unwrap_or(hero[1]);
                let tg = [xz[0], h + fk.ankle, xz[1]];
                if side == 0 {
                    (fk.tg_l, fk.from_l, fk.to_l, fk.sw_l) = (tg, tg, tg, -1.0);
                } else {
                    (fk.tg_r, fk.from_r, fk.to_r, fk.sw_r) = (tg, tg, tg, -1.0);
                }
            }
        } else if !fk.drive && fk.gait {
            // (leaving: the goals stay on the feet, where the gait had them, their targets ease to the places the clip that plays now has the feet at over a third of a second (`smx`), and then they let go over `release`)
            fk.gait = false;
            fk.smx = 1.0;
            (fk.locked_l, fk.locked_r, fk.rel_l, fk.rel_r, fk.gw_l, fk.gw_r) = (false, false, 0.0, 0.0, 1.0, 1.0);
        }
        if fk.gait {
            let here = [hero[0], hero[2]];
            // (the distance walked a tick is uneven on stairs: the capsule loses and gains speed rolling over each edge; the phase goes on by the smoothed pace, so the clip's body does not tremble)
            let raw = ((here[0] - fk.prev[0]) * fwd[0] + (here[1] - fk.prev[1]) * fwd[1]).abs();
            fk.prev = here;
            fk.pace += (raw / dt.max(1.0e-4) - fk.pace) * (dt / 0.12).min(1.0);
            let ds = fk.pace * dt;
            let theta0 = fk.theta;
            let mut th = theta0;
            if fk.pace > 0.06 {
                th += ds / (2.0 * fk.pitch.clamp(0.12, 0.6));
            } else if th < swing || (th >= 0.5 && th < 0.5 + swing) {
                // (standing: a step that is in the air is finished, in a third of a second, and the feet stay down)
                let end = if th < swing { swing } else { 0.5 + swing };
                th = (th + dt / 0.35 * swing).min(end);
            }
            let wrapped = th >= 1.0;
            if wrapped {
                th -= 1.0;
            }
            // a step begins: the foot that is behind comes forward onto the tread a step ahead of the other
            let starts = [(wrapped, 0usize), (theta0 < 0.5 && th >= 0.5 && !wrapped, 1usize)];
            for (go, side) in starts {
                if !go {
                    continue;
                }
                let (cur, other) = if side == 0 { (fk.tg_l, fk.tg_r) } else { (fk.tg_r, fk.tg_l) };
                let along = (other[0] - cur[0]) * fwd[0] + (other[2] - cur[2]) * fwd[1];
                let guess = on([cur[0], cur[2]], along + fk.pitch.clamp(0.12, 0.6));
                let xz = fit(guess);
                let h = support(xz).unwrap_or(other[1] - fk.ankle);
                let to = [xz[0], h + fk.ankle, xz[1]];
                let ahead = (xz[0] - other[0]) * fwd[0] + (xz[1] - other[2]) * fwd[1];
                fk.pitch += (ahead.clamp(0.12, 0.6) - fk.pitch) * 0.5;
                if side == 0 {
                    (fk.from_l, fk.to_l) = (cur, to);
                } else {
                    (fk.from_r, fk.to_r) = (cur, to);
                }
            }
            fk.theta = th;
            let clearance = fk.arc;
            let step = |from: [f32; 3], to: [f32; 3], u: f32| -> [f32; 3] {
                let sm = |x: f32| x * x * (3.0 - 2.0 * x);
                let e = sm(((u - 0.12) / 0.88).clamp(0.0, 1.0));
                let peak = from[1].max(to[1]) + clearance;
                let y = if to[1] < from[1] - 0.02 {
                    // (a step down: the foot is lifted first, goes forward over the edge of the tread it leaves, and only then is lowered, by how far it has come: lowered by time it would be below the nose of the tread
                    // while still over it, and the goal that keeps the foot out of the ground would hold it there, to let it fall when it clears the edge)
                    let lift = from[1] + (peak - from[1]) * sm((u / 0.35).clamp(0.0, 1.0));
                    let drop = to[1] + (peak - to[1]) * (1.0 - sm(((e - 0.45) / 0.55).clamp(0.0, 1.0)));
                    lift.min(drop)
                } else if u < 0.45 {
                    from[1] + (peak - from[1]) * sm(u / 0.45)
                } else {
                    peak + (to[1] - peak) * sm(((u - 0.45) / 0.55).clamp(0.0, 1.0))
                };
                [from[0] + (to[0] - from[0]) * e, y, from[2] + (to[2] - from[2]) * e]
            };
            if th < swing {
                fk.sw_l = th / swing;
                fk.tg_l = step(fk.from_l, fk.to_l, fk.sw_l);
            } else {
                fk.sw_l = -1.0;
                fk.tg_l = fk.to_l;
            }
            if th >= 0.5 && th < 0.5 + swing {
                fk.sw_r = (th - 0.5) / swing;
                fk.tg_r = step(fk.from_r, fk.to_r, fk.sw_r);
            } else {
                fk.sw_r = -1.0;
                fk.tg_r = fk.to_r;
            }
            // the clip is played at the planner's phase
            if let Some(an) = world.get_mut::<Animator>(e) {
                an.time = clip_time(fk.theta);
                an.speed = 0.0;
            }
        }
        let t_clip = if fk.gait { clip_time(fk.theta) } else { a.time };
        let (mut pl, mut pr, cl, cr) = sample(&data, clip, t_clip, a.looping || fk.gait);
        // going down a flight a walking clip's stride (a metre) does not fit the steps: its feet are put nearer the body; a stair clip is made for steps and keeps its own
        let want_stride = if fk.ahead < -0.5 && !clip.stance && !fk.gait { fk.stride_down.clamp(0.2, 1.0) } else { 1.0 };
        fk.stride = if fk.live { fk.stride + (want_stride - fk.stride) * (dt * 5.0).min(1.0) } else { want_stride };
        pl[2] *= fk.stride;
        pr[2] *= fk.stride;
        // the clip's planted flags a little ahead: how soon a foot comes down (near 1: at once, near 0: not within `lead`)
        let rate = a.speed.clamp(0.25, 3.0);
        let lead_steps = ((fk.lead / 0.03).round() as i32).clamp(1, 12);
        let coming = |left: bool| -> f32 {
            for k in 1..=lead_steps {
                let (_, _, l2, r2) = sample(&data, clip, a.time + 0.03 * rate * k as f32, a.looping);
                if (if left { l2 } else { r2 }) >= 0.5 {
                    return 1.0 - k as f32 / (lead_steps + 1) as f32;
                }
            }
            0.0
        };
        // ---- the feet: where each is held, how strongly, and the ground under it
        struct Foot {
            p: [f32; 3],
            c: f32,
            at: [f32; 2],
            h: Option<f32>,
            gw: f32,
            /// The height the goal asks for (the gait's own), else the ground's plus the ankle's.
            goal_y: Option<f32>,
            /// The sole's height this foot asks the origin to be at, and how much it counts: a planted foot says it from the clip's pose, a foot of the gait from where it is put.
            reach_h: Option<f32>,
            reach_w: f32,
            /// The goal pins the foot along the way it points too (it was fitted to a tread, or the gait places it), not only its height.
            along: bool,
        }
        let mut feet: Vec<Foot> = Vec::with_capacity(2);
        if fk.gait {
            let (drop_l, drop_r) = drops(clip, t_clip, true, fk.ankle);
            for side in 0..2 {
                let (p, tg, sw, to) = if side == 0 { (pl, fk.tg_l, fk.sw_l, fk.to_l) } else { (pr, fk.tg_r, fk.sw_r, fk.to_r) };
                let drop = if side == 0 { drop_l } else { drop_r };
                // (the lowest of the heel and the toe is on the tread: the foot rolls over it as the clip's does; in the air, and as it lands, the joint is at the ankle's height)
                let on_tread = if sw < 0.0 { 1.0 } else { ((sw - 0.8) / 0.2).clamp(0.0, 1.0) };
                let goal_y = tg[1] + (drop - fk.ankle) * on_tread;
                let at = [tg[0], tg[2]];
                let h = support(at);
                // a foot on its tread limits the origin; one that is coming down counts as it comes
                let (reach_h, reach_w) = if sw < 0.0 { (Some(tg[1] - fk.ankle), 1.0) } else { (Some(to[1] - fk.ankle), ((sw - 0.55) / 0.45).clamp(0.0, 1.0)) };
                feet.push(Foot { p, c: 0.0, at, h, gw: 1.0, goal_y: Some(goal_y), reach_h, reach_w, along: true });
            }
        } else {
            for side in 0..2 {
                let (p, c) = if side == 0 { (pl, cl.clamp(0.0, 1.0)) } else { (pr, cr.clamp(0.0, 1.0)) };
                let (mut locked, mut lk, mut gw, mut rel) = if side == 0 { (fk.locked_l, fk.lk_l, fk.gw_l, fk.rel_l) } else { (fk.locked_r, fk.lk_r, fk.gw_r, fk.rel_r) };
                let animated = place(p);
                let want = if locked && rel <= 0.1 { 1.0 } else { c.max(coming(side == 0)) };
                gw = if want > gw { want } else { (gw - dt / fk.release.max(0.01)).max(want) };
                if !fk.live {
                    (locked, gw) = (false, want);
                }
                let fitted = |from: [f32; 2], strength: f32| -> [f32; 2] {
                    if strength <= 0.001 {
                        return from;
                    }
                    fit(from)
                };
                // (the lock is let go of only when the clip has not had the foot planted for a tenth of a second: a switch of the clip by motion matching can say 'swinging' for a tick while the foot is on its step, and the
                // new clip's foot is somewhere else: a lock taken again then would put the foot a stride away)
                rel = if c < 0.2 { rel + dt } else { 0.0 };
                // (a foot is locked only where it had to be moved to fit a tread: on level ground the clip's own foot is right, and a lock would pin it to a place while the clip's foot, which moves as the clip's speed
                // says and not as the character's, goes on: the leg would be pulled out straight)
                let fit_a = if locked { animated } else { fitted(animated, gw) };
                if c >= 0.5 {
                    if !locked && (fit_a[0] - animated[0]).hypot(fit_a[1] - animated[1]) > 0.015 {
                        lk = fit_a;
                        locked = true;
                    }
                } else if rel > 0.1 {
                    locked = false;
                }
                if locked {
                    // (on a leash: a lock never holds a foot further than this from where the clip has it)
                    let d = [lk[0] - animated[0], lk[1] - animated[1]];
                    let len = d[0].hypot(d[1]);
                    if len > LOCK_LEASH {
                        lk = [animated[0] + d[0] * (LOCK_LEASH / len), animated[1] + d[1] * (LOCK_LEASH / len)];
                    }
                }
                let at = if locked { lk } else { fit_a };
                // (the ground under a foot that is more than `EDGE_DROP` from the character's own is another surface: a foot over the edge of the platform the character stands on, a step up it is not climbing; it is
                // not followed: the goal that put the foot on a floor two metres lower would pull the leg off the pose)
                let h = if w > 0.001 { support(at).filter(|g| (g - hero[1]).abs() <= EDGE_DROP) } else { None };
                if side == 0 {
                    (fk.locked_l, fk.lk_l, fk.gw_l, fk.rel_l) = (locked, lk, gw, rel);
                } else {
                    (fk.locked_r, fk.lk_r, fk.gw_r, fk.rel_r) = (locked, lk, gw, rel);
                }
                // a planted foot says the origin from the clip's pose (the sole's height above it); one that is coming down is taken to land with its sole level with the origin
                let reach_h = h.map(|h| if c > 0.5 { h - (p[1] - fk.ankle) } else { h });
                let along = locked || (at[0] - animated[0]).hypot(at[1] - animated[1]) > 0.015;
                feet.push(Foot { p, c, at, h, gw, goal_y: None, reach_h, reach_w: gw, along });
            }
        }
        // ---- the model's origin: the line the ground makes under the character, kept within reach of the planted feet. The line is the average of the ground along the way it goes over a long window at places that do
        // not repeat the pitch of the steps (the steps of a flight make a ramp of it: with samples a tread apart they would all cross a riser together and the average would jump).
        let dir = if speed > 0.05 { [walk[0] / speed, walk[1] / speed] } else { fwd };
        let (mut sum, mut wsum) = (0.0f32, 0.0f32);
        // (the ground at a place that is more than `EDGE_DROP` from the character's own is another surface, as under a foot: the floor behind the edge of the platform the character stands on does not pull the line down)
        for u in LINE_AT {
            if let Some(g) = ground(hero[0] + dir[0] * u, hero[2] + dir[1] * u).filter(|g| (g - hero[1]).abs() <= EDGE_DROP) {
                sum += g;
                wsum += 1.0;
            }
        }
        let line_raw = if wsum > 0.0 { sum / wsum } else { hero[1] };
        fk.line_y = line_raw;
        // (the line is the average of a few places: a place going over a riser moves it a little, and the steps of a flight make that a ripple; it is smoothed before it is used)
        fk.line_s = if fk.live { fk.line_s + (line_raw - fk.line_s) * (dt / 0.1).min(1.0) } else { line_raw };
        let line = fk.line_s;
        // (in the gait the origin sits a little above the line of the stairs: a person's hip is higher over the foot that carries the weight than the average of the steps)
        let own = if fk.gait { fk.root_lift + fk.gait_lift } else { clip.lift + fk.root_lift };
        // What each foot that is planted, or coming down, lets the origin be: its leg reaches `slack_up` further than the clip has it and bends `slack_down` more. The limits of a foot that is coming down loosen by half a
        // metre where it is far from landing, so that they close in gradually while it comes down and open again gradually after it has left (the pelvis does not drop when the foot touches, it has dropped while the
        // leg reached down). A stair clip has its legs bent: they can reach further than a walking clip's, which are nearly straight when the foot is planted.
        let slack_up = if clip.stance && !fk.gait { fk.slack_up + 0.1 } else if fk.gait { fk.slack_up + 0.03 } else { fk.slack_up };
        let (mut lower, mut upper) = (-1.0e9f32, 1.0e9f32);
        let (mut num, mut den) = (0.0f32, 0.0f32);
        for f in &feet {
            let Some(o) = f.reach_h else { continue };
            if f.reach_w <= 0.02 {
                continue;
            }
            let loose = (1.0 - f.reach_w.clamp(0.0, 1.0)) * 0.5;
            upper = upper.min(o + slack_up + loose);
            lower = lower.max(o - fk.slack_down - loose);
            if f.c > 0.5 {
                num += f.c * o;
                den += f.c;
            }
        }
        let mut origin = line + own + fk.bias;
        if fk.gait {
            // The hip of a person on stairs is over the middle of the two feet, less half the height of a step: with the lower foot straight under it (0.88 over the sole) when both are down. It follows the foot that is in
            // the air as it goes from one step to the next, so that it rises (or falls) the height of a step in each step, smoothly: this is the origin the gait asks for.
            let sole = |y: f32| y - fk.ankle;
            let sm = |x: f32| x * x * (3.0 - 2.0 * x);
            let (stay, flying, half) = if fk.sw_l >= 0.0 {
                (sole(fk.tg_r[1]), sole(fk.from_l[1]) + (sole(fk.to_l[1]) - sole(fk.from_l[1])) * sm(fk.sw_l.clamp(0.0, 1.0)), (fk.to_l[1] - fk.from_l[1]).abs() * 0.5)
            } else if fk.sw_r >= 0.0 {
                (sole(fk.tg_l[1]), sole(fk.from_r[1]) + (sole(fk.to_r[1]) - sole(fk.from_r[1])) * sm(fk.sw_r.clamp(0.0, 1.0)), (fk.to_r[1] - fk.from_r[1]).abs() * 0.5)
            } else {
                (sole(fk.tg_l[1]), sole(fk.tg_r[1]), (fk.tg_l[1] - fk.tg_r[1]).abs())
            };
            origin = (stay + flying) * 0.5 - half * 0.5 + own;
        }
        if lower <= upper {
            origin = origin.clamp(lower, upper);
        } else {
            origin = (lower + upper) * 0.5;
        }
        // A treadmill clip (a stair clip) has its origin a fixed distance from the line the stairs make, the clip's own pose: it is learnt from the planted feet, slowly (a dead band: the heel and toe rolling of a planted
        // foot is not the origin being wrong). A walking clip stays on the line.
        if clip.stance && !fk.gait && den > 0.5 {
            let raw = num / den - line - own;
            let err = (raw.signum() * (raw.abs() - 0.04).max(0.0)).clamp(-0.3, 0.3);
            fk.bias += (err - fk.bias) * (dt / 0.4).min(1.0);
        } else if !clip.stance || fk.gait {
            fk.bias *= 1.0 - (dt * 4.0).min(1.0);
        }
        fk.stance_y = if den > 0.05 { num / den } else { origin };
        let target = hero[1] + (origin - hero[1]).clamp(-fk.max_drop, fk.max_drop) * fk.follow.clamp(0.0, 1.0);
        if !fk.live {
            fk.gs = target;
            fk.gs_v = 0.0;
        } else {
            // (a stair clip's origin rises steadily and its planted feet keep it exact: it needs a shorter smoothing than a walk over bumps)
            let tau = if clip.stance || fk.gait { fk.smooth * 0.7 } else { fk.smooth };
            let mut v = fk.gs_v;
            fk.gs = smooth_damp(fk.gs, target, &mut v, tau, dt);
            fk.gs_v = v;
        }
        fk.live = true;
        fk.off = fk.gs - hero[1];
        if let Some(l) = world.get_mut::<LocalTransform>(e) {
            l.pos[1] = fk.off;
        }
        // ---- the goals
        // (they come on over a tenth of a second after the character lands: a foot that the clip has in the ground at the moment of landing is brought out of it, not thrown out in one picture; they go off at once)
        fk.td += dt;
        fk.wk = if w > fk.wk {
            if fk.td > TOUCH_GRACE {
                (fk.wk + dt / 0.1).min(w)
            } else {
                fk.wk
            }
        } else {
            w
        };
        let wg = fk.wk;
        let up = [0.0, 1.0, 0.0];
        let along = [fwd[0], 0.0, fwd[1]];
        // the knees bend towards a point ahead of the character at the height of its hips (the pole of the goals): the way a knee bends is then not made by where the clip happens to have it, which a leg that the clip has
        // nearly straight, and a goal that is moved past it, turn the wrong way (the knee bends backwards)
        let lat = [cs, -sn];
        let pole_y = hero[1] + fk.off + POLE_HEIGHT;
        for (uid, side) in [(fk.pole_l, 1.0f32), (fk.pole_r, -1.0f32)] {
            if let Some(pe2) = world.entity(uid.0).filter(|_| uid.0 != 0) {
                if let Some(t) = world.get_mut::<Transform>(pe2) {
                    t.pos = [hero[0] + fwd[0] * POLE_AHEAD + lat[0] * POLE_APART * side, pole_y, hero[2] + fwd[1] * POLE_AHEAD + lat[1] * POLE_APART * side];
                }
            }
        }
        // (a goal that has just begun to be set by the gait, or let go of by it, comes to its place over a third of a second: the foot is on the screen where the clip had it)
        fk.smx = (fk.smx - dt / 0.3).max(0.0);
        let tau = 0.1 * fk.smx;
        for (k, f) in feet.iter().enumerate() {
            let (goal, floor) = if k == 0 { (fk.goal_l, fk.floor_l) } else { (fk.goal_r, fk.floor_r) };
            let Some(h) = f.h else {
                if k == 0 {
                    (fk.gt_ok_l, fk.fy_ok_l) = (false, false);
                } else {
                    (fk.gt_ok_r, fk.fy_ok_r) = (false, false);
                }
                // (the ground under the foot is gone: its goal does not go off at once, which would put the foot back on the clip's pose, as far as the lock's leash away, in one picture: it stays where it was and fades out.
                // It keeps the pin along the way the foot points as well: without it the foot is free along the ground at once and jumps to the clip's foot, up to the leash, 0.35 m, in one picture, as a character walks off
                // the edge of a platform with a foot that was held)
                let (edge, hold) = if k == 0 { (&mut fk.edge_l, fk.hold_l) } else { (&mut fk.edge_r, fk.hold_r) };
                *edge = (*edge - dt / EDGE_FADE).max(0.0);
                let faded = *edge;
                if faded > 0.001 {
                    set_ik(world, goal, hold, up, along, false, faded);
                } else {
                    set_ik(world, goal, [0.0; 3], up, [0.0; 3], false, 0.0);
                }
                set_ik(world, floor, [0.0; 3], up, [0.0; 3], false, 0.0);
                continue;
            };
            let floor_raw = h + fk.ankle - hero[1];
            let (fy, fy_ok) = if k == 0 { (&mut fk.fy_l, &mut fk.fy_ok_l) } else { (&mut fk.fy_r, &mut fk.fy_ok_r) };
            if !*fy_ok {
                (*fy, *fy_ok) = (floor_raw, true);
            } else if floor_raw > *fy {
                *fy += (floor_raw - *fy) * (dt / 0.05).min(1.0);
            } else {
                *fy = floor_raw;
            }
            let floor_y = hero[1] + *fy;
            // A planted foot is put on the ground (its sole on it). A foot in the air keeps the height over the ground that the clip gives it, over the ground it is above: on level ground the clip's own height (the
            // hold has nothing to correct, so that a switch of the clip cannot make the foot jump), over a step a step higher as the foot comes over it. The two are blended by how planted the clip says the foot is.
            let follow = fk.gs + f.p[1] + (h - line);
            // (the gait's foot is never asked for less than the floor under it: over the nose of the tread it leaves, the goal that keeps the foot out of the ground would hold it up while the goal pulls it down)
            let desired = f.goal_y.map_or(floor_y * f.c + follow * (1.0 - f.c), |g| g.max(floor_y));
            let lift = desired - (fk.gs + f.p[1]);
            if k == 0 {
                fk.lift_l = lift;
            } else {
                fk.lift_r = lift;
            }
            set_ik(world, floor, [f.at[0], floor_y, f.at[1]], up, [0.0; 3], true, wg);
            let (pole, gt, gv, ok) = if k == 0 { (fk.pole_l, &mut fk.gt_l, &mut fk.gv_l, &mut fk.gt_ok_l) } else { (fk.pole_r, &mut fk.gt_r, &mut fk.gv_r, &mut fk.gt_ok_r) };
            let raw = [f.at[0] - hero[0], desired - hero[1], f.at[1] - hero[2]];
            let alh = if k == 0 { &mut fk.alh_l } else { &mut fk.alh_r };
            *alh = if f.along { ALONG_HOLD } else { (*alh - dt).max(0.0) };
            let pin_along = f.along || fk.smx > 0.0 || *alh > 0.0;
            if !*ok || (tau < 1.0e-3 && fk.gait && !(raw[1] < gt[1])) {
                (*gt, *gv, *ok) = (raw, [0.0; 3], true);
            } else {
                for a in 0..3 {
                    // (a gait foot whose floor drops away, as the heel clears the nose of the tread it leaves, is lowered over a twentieth of a second, not in one picture; it is raised at once)
                    let tau_a = if a == 1 && fk.gait && raw[1] < gt[1] { tau.max(0.045) } else { tau };
                    let before = gt[a];
                    gt[a] = smooth_damp(gt[a], raw[a], &mut gv[a], tau_a, dt);
                    if !fk.gait {
                        // (the goals of the clips' own feet move no faster than a foot goes, in height, and along the ground where the foot is pinned: a goal that jumps, because a foot has been fitted to another tread, or
                        // planted over an edge, glides there instead)
                        let lim = if a == 1 { FLAT_SLEW_UP } else if pin_along { FLAT_SLEW } else { f32::MAX };
                        gt[a] = before + (gt[a] - before).clamp(-lim * dt, lim * dt);
                    }
                }
            }
            let at_now = [hero[0] + gt[0], hero[1] + gt[1], hero[2] + gt[2]];
            for g in [goal, floor] {
                if let Some(ge) = world.entity(g.0).filter(|_| g.0 != 0) {
                    if let Some(ik) = world.get_mut::<Ik>(ge) {
                        ik.pole = pole;
                    }
                }
            }
            // (the way the foot points and its place along the sole are pinned only where the foot was fitted to a tread, or by the gait: on level ground the clip's foot is where it should be, and the pin that holds
            // it there would pull it away from the pose on the screen after a switch of clips)
            let easing = fk.smx.clamp(0.0, 1.0);
            let weight = wg * f.gw.clamp(0.0, 1.0).max(easing) * settle.max(easing);
            set_ik(world, goal, at_now, up, if pin_along { along } else { [0.0; 3] }, false, weight);
            if k == 0 {
                (fk.hold_l, fk.edge_l) = (at_now, weight.clamp(0.0, 1.0));
            } else {
                (fk.hold_r, fk.edge_r) = (at_now, weight.clamp(0.0, 1.0));
            }
        }
        if let Some(f) = world.get_mut::<FootIk>(e) {
            *f = fk;
        }
    }
}
