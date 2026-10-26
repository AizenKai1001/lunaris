//! Traversal: vaulting, hurdling, mantling and climbing over obstacles with captured clips and **motion warping**, the way Epic's Game Animation Sample does it. A game script asks (`Traversal.try_for`, a time the
//! check keeps trying) and the engine
//!
//! 1. **senses**: a sweep forward (as far as the speed says: 0.75 m standing, 3.5 m at 5 m/s) for a `Traversable` box; its nearest top edge is the *front ledge* (a point, the outward normal), the opposite edge the *back
//!    ledge*; the height of the ledge above the feet, the depth of the box, and the floor behind it (`back floor`: the highest solid top below the ledge there, within the height of the obstacle). Room above the ledge for the
//!    character is checked too. The kind of move follows Epic's rules (the numbers of `TryTraversalAction`, in metres): **vault** = a back ledge, no floor behind, height 0.5 to 1.25, depth under 0.59; **hurdle** = a back
//!    ledge and floor, height 0.5 to 1.25, depth under 0.59, the drop behind more than 0.5; **mantle** = deeper than 0.59, height 0.5 to 2.75 (up to 1.5: the 1 m mantle, above it the 2.5 m climb); else nothing.
//! 2. **chooses** the move among the captured clips of that kind for the gait (stand, walk, run) and, with the motion matching database of the character, by how well the clip's pose at the moment it would be entered
//!    matches the character's own pose (this picks the left or right foot version and the 1.0 or V2 style); the moment is where the clip's distance to the ledge is the character's (the clip's *branch-in* window);
//!    if the character is further than the clip begins, the move waits (`try_for`) until it has run that far.
//! 3. **warps**: the clip's root path is laid onto the real ledge (the clips were captured against a ledge at x = 0, z = 0, `ledge height` high: the animated `attach` bone is that ledge relative to the root) and the
//!    differences are removed inside the clip's warp windows (*front ledge*: the root reaches the ledge exactly as authored, turning to face it; *back ledge*: it passes over the real back edge; *back floor*: it lands
//!    at the real height of the floor), by offsets that grow smoothly over each window. The character's `Transform` is written from the warped path (physics is off while it plays), the `Animator` plays the clip, and
//!    at the clip's blend-out moment (or, for a climb, when the player moves) the move ends: the character is given the velocity the clip had and motion matching takes over (a switch inertialisation blends).
//!
//! 4. **catches** (an air catch): a character that is *falling* (the script sets `Traversal.catch_try`) beside a `Traversable` block whose top is within reach of its hands catches the ledge: a move of the file with
//!    `"catch": true` (a clip of a fall that ends with the hands on the ledge, then the pull up: the Game Animation Sample's `Traversal_Catch_*`) is chosen by how high the ledge is above the feet (`rel` of the move)
//!    and by the gait, and is entered `catch_lead` seconds before the clip's catch moment: the clip's path is laid on the real ledge and the difference from where the character is is removed inside the clip's front ledge
//!    window, which is how the fall ends in the grab; from the catch on it is the mantle of any other (hanging on a tall ledge, the shimmy, the push off the wall).
//!
//! `Traversal` goes on the entity of the character's model (it has the `Animator` and `MotionMatcher`, and a `Parent`: the character, with the `CharacterBody`). See `docs/GAMEPLAY.md`, "Traversal".
use crate::components::{Animator, CharacterBody, Collider, Ik, LocalTransform, Parent, Transform};
use crate::detmath;
use crate::inertia::Inertia;
use crate::motionmatch::{self, Dynamics, MotionMatcher, POSE_START};
use lunaris_ecs::{component, Entity, Sym, Uid, World};
use lunaris_json::{parse, Json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// What a move may be: the numbers of Epic's check, in metres.
const MIN_HEIGHT: f32 = 0.5;
const MAX_JUMP_HEIGHT: f32 = 1.25;
const MAX_MANTLE_HEIGHT: f32 = 2.75;
const THIN: f32 = 0.59;
const MIN_DROP: f32 = 0.5;
/// How far under the top of a thin wall the floor beyond it may be for it to be a hurdle that is caught from the air (further down it is a vault: nothing to land on that the clip's landing fits).
const CATCH_MAX_DROP: f32 = 2.5;
const CLIMB_ABOVE: f32 = 1.5;
/// The kinds of move (`Traversal.action`).
pub const VAULT: u32 = 1;
pub const HURDLE: u32 = 2;
pub const MANTLE: u32 = 3;
/// The push off a wall (Space while climbing): a move of its own, never chosen by the check.
pub const WALLJUMP: u32 = 4;

#[derive(Clone, Debug)]
pub struct Window {
    /// 0 front ledge, 1 back ledge, 2 back floor.
    pub target: u8,
    pub start: f32,
    pub end: f32,
    pub rotation: bool,
    pub ignore_z: bool,
}

/// One captured move.
#[derive(Clone, Debug)]
pub struct Move {
    pub name: String,
    pub action: u32,
    pub climb: bool,
    /// The height of the ledge in the clip.
    pub height: f32,
    /// Where the clip has its ledge (x, y, z): the animated `attach` bone says it at the end of the front ledge window.
    pub ledge: [f32; 3],
    /// 0 standing, 1 walking, 2 running.
    pub gait: u8,
    pub length: f32,
    pub rate: f32,
    /// The root of the character in the clip, every `1 / rate` seconds: x, y, z, yaw.
    pub root: Vec<[f32; 4]>,
    pub windows: Vec<Window>,
    /// The first part of the clip a move may be entered from.
    pub branch_end: f32,
    pub blend_at: f32,
    /// A mantle or a climb does not end before the root is this far past the ledge: the character stands on the top, not at its edge (0: no such rule).
    pub exit_over: f32,
    pub wait_input: bool,
    /// The moment the hands are on the ledge (a mantle or climb may wait here until forward is held): the end of the first warp window.
    pub hang_at: f32,
    /// The earliest moment a character that holds forward may leave a mantle or climb: when the root is over the top (mantle), or just after the last window.
    pub early_out: f32,
    /// An air catch: the clip is a fall that ends with the hands on the ledge (at `hang_at`, the end of its front window), not a run up to it. Never chosen by the check on the ground.
    pub catch: bool,
    /// For a catch: how high the ledge is above the root at the catch moment (the height class of the clip) and how far in front of the ledge the root is.
    pub rel: f32,
    pub catch_dist: f32,
}

impl Move {
    pub fn root_at(&self, t: f32) -> [f32; 4] {
        let x = (t * self.rate).clamp(0.0, (self.root.len() - 1) as f32);
        let k = (x.floor() as usize).min(self.root.len() - 1);
        let k1 = (k + 1).min(self.root.len() - 1);
        let u = x - k as f32;
        let (a, b) = (self.root[k], self.root[k1]);
        [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u, a[2] + (b[2] - a[2]) * u, a[3] + (b[3] - a[3]) * u]
    }
}

/// The hanging pose and the shimmy clips: the character hangs by its hands from a ledge (`hands_up` above its root, `hands_fwd` in front of it) and shifts along the ledge at `speed` m/s while the clip plays in place.
#[derive(Clone, Debug)]
pub struct Shimmy {
    pub left: String,
    pub right: String,
    pub speed: f32,
    pub hands_up: f32,
    pub hands_fwd: f32,
    pub feet_up: f32,
    /// How far under the top of the ledge the wrist is in the hanging pose (the fingers go over the edge, the palm is on the wall's face).
    pub grip: f32,
}

#[derive(Clone, Debug)]
pub struct TraversalData {
    pub moves: Vec<Move>,
    pub shimmy: Option<Shimmy>,
    /// The wall jump: the number (from 1) of the move of that kind, 0 when the file has none.
    pub walljump: usize,
}

impl TraversalData {
    pub fn from_json(text: &str) -> Result<TraversalData, String> {
        let j = parse(text).map_err(|e| e.message)?;
        let list = j.get("montages").and_then(|m| m.as_object()).ok_or("the file has no `montages`")?;
        let f = |v: &Json, k: &str| v.get(k).and_then(|x| x.as_f64()).map(|x| x as f32);
        let mut moves = Vec::new();
        for (name, m) in list {
            let kind = m.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            let root: Vec<[f32; 4]> = m
                .get("root")
                .and_then(|r| r.as_array())
                .ok_or("a move has no `root`")?
                .iter()
                .map(|p| {
                    let a = p.as_array().unwrap_or(&[]);
                    let g = |i: usize| a.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
                    [g(0), g(1), g(2), g(3)]
                })
                .collect();
            if root.len() < 2 {
                return Err(format!("`{name}` has too short a root path"));
            }
            let mut windows = Vec::new();
            for w in m.get("windows").and_then(|w| w.as_array()).unwrap_or(&[]) {
                let target = match w.get("target").and_then(|t| t.as_str()) {
                    Some("front") => 0,
                    Some("back") => 1,
                    Some("floor") => 2,
                    _ => continue,
                };
                windows.push(Window { target, start: f(w, "start").unwrap_or(0.0), end: f(w, "end").unwrap_or(0.0), rotation: w.get("rotation").and_then(|x| x.as_bool()).unwrap_or(false), ignore_z: w.get("ignore_z").and_then(|x| x.as_bool()).unwrap_or(false) });
            }
            let blend = m.get("blend_out");
            moves.push(Move {
                name: name.clone(),
                action: m.get("action").and_then(|a| a.as_u64()).unwrap_or(0) as u32,
                climb: kind == "Climb",
                height: f(m, "height").unwrap_or(1.0),
                ledge: {
                    let a = m.get("ledge").and_then(|l| l.as_array()).unwrap_or(&[]);
                    let g = |i: usize| a.get(i).and_then(|x| x.as_f64());
                    [g(0).unwrap_or(0.0) as f32, g(1).or(f(m, "height").map(|h| h as f64)).unwrap_or(1.0) as f32, g(2).unwrap_or(0.0) as f32]
                },
                gait: match m.get("gait").and_then(|g| g.as_str()) {
                    Some("run") => 2,
                    Some("walk") => 1,
                    _ => 0,
                },
                length: f(m, "length").unwrap_or(1.0),
                rate: f(m, "rate").unwrap_or(30.0),
                root,
                windows,
                branch_end: f(m, "branch_end").unwrap_or(0.0),
                blend_at: blend.and_then(|b| f(b, "at")).unwrap_or(f32::MAX),
                exit_over: 0.0,
                wait_input: blend.and_then(|b| b.get("wait_input")).and_then(|x| x.as_bool()).unwrap_or(false),
                hang_at: 0.0,
                early_out: 0.0,
                catch: m.get("catch").and_then(|x| x.as_bool()).unwrap_or(false),
                rel: f(m, "rel").unwrap_or(0.0),
                catch_dist: f(m, "catch_dist").unwrap_or(0.0),
            });
        }
        for m in moves.iter_mut() {
            if m.action == MANTLE {
                m.exit_over = 0.2;
            }
            m.hang_at = m.windows.iter().filter(|w| w.target == 0).map(|w| w.end).fold(0.0f32, f32::max);
            let last = m.windows.iter().map(|w| w.end).fold(0.0f32, f32::max);
            m.early_out = if m.action == MANTLE {
                // the root is over the ledge (0.1 m under it), and a moment more
                let mut t = m.hang_at;
                let n = ((m.length * m.rate) as usize).min(m.root.len() - 1);
                for k in 0..=n {
                    let tk = k as f32 / m.rate;
                    if tk >= m.hang_at && m.root[k][1] >= m.ledge[1] - 0.1 {
                        t = tk;
                        break;
                    }
                    t = tk;
                }
                (t + 0.15).max(m.blend_at.min(m.length) * 0.5)
            } else {
                (last + 0.05).max(m.blend_at.min(m.length) * 0.5)
            };
        }
        if moves.is_empty() {
            return Err("the file has no moves".into());
        }
        for m in moves.iter_mut() {
            if m.action == WALLJUMP {
                // the clip's own start is where the move begins: its path is taken from there
                m.ledge = [m.root[0][0], m.root[0][1], m.root[0][2]];
            }
        }
        let walljump = moves.iter().position(|m| m.action == WALLJUMP).map_or(0, |i| i + 1);
        let shimmy = j.get("shimmy").map(|sh| {
            let st = |k: &str| sh.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            Shimmy { left: st("left"), right: st("right"), speed: f(sh, "speed").unwrap_or(0.45), hands_up: f(sh, "hands_up").unwrap_or(2.2), hands_fwd: f(sh, "hands_fwd").unwrap_or(0.3), feet_up: f(sh, "feet_up").unwrap_or(1.0), grip: f(sh, "grip").unwrap_or(0.1) }
        });
        Ok(TraversalData { moves, shimmy, walljump })
    }
}

fn store() -> &'static Mutex<HashMap<PathBuf, Result<Arc<TraversalData>, String>>> {
    static S: OnceLock<Mutex<HashMap<PathBuf, Result<Arc<TraversalData>, String>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The traversal file `name` of the workspace (read once, then shared).
pub fn load(root: &Path, name: &str) -> Result<Arc<TraversalData>, String> {
    if name.contains("..") || name.contains(':') || name.starts_with('/') || name.starts_with('\\') {
        return Err(format!("`{name}` leaves the workspace"));
    }
    let path = root.join(name);
    let mut s = store().lock().map_err(|_| "the store is poisoned".to_string())?;
    if let Some(r) = s.get(&path) {
        return r.clone();
    }
    let r = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display())).and_then(|t| TraversalData::from_json(&t)).map(Arc::new);
    s.insert(path, r.clone());
    r
}

component! {
    /// Something a character can vault, hurdle, mantle or climb: a box (a `Collider` of shape 1 on the same entity). `margin` is how far in from the ends of an edge the hands may be put.
    pub struct Traversable {
        pub margin: f32 = 0.2,
    }
}

component! {
    /// Traversal for a character (see the module docs and `docs/GAMEPLAY.md`, "Traversal"): on the entity of the model (with its `Animator`, a `Parent` that is the character with the `CharacterBody`, and a
    /// `MotionMatcher` for the pose and speed). `data` is the traversal file. A script sets `try_for` (seconds, 0.5 when the player asks): the check runs every tick while it counts down; it ends with the move
    /// started (`active`), or `failed` for the tick the answer is that there is nothing to traverse (the script may jump instead). `move_input` tells a climb that the player wants to move on. The answers of
    /// the check: `action` (0 none, 1 vault, 2 hurdle, 3 mantle), `height`, `depth`, `drop` (the ledge above the floor behind it), `front`, `front_n`, `back`, `floor`. While `active`: `pos` and `yaw` are where the character
    /// is and faces, `time` the clip's time; `exit` is 1 for the tick it ends, with `exit_vel` (the velocity of the clip, x and z) and `exit_yaw`.
    pub struct Traversal {
        pub data: Sym = Sym(0),
        pub try_for: f32 = 0.0,
        pub move_input: bool = false,
        pub failed: bool = false,
        pub active: bool = false,
        pub exit: bool = false,
        pub action: u32 = 0,
        pub height: f32 = 0.0,
        pub depth: f32 = 0.0,
        pub drop: f32 = 0.0,
        pub front: [f32; 3] = [0.0, 0.0, 0.0],
        pub front_n: [f32; 3] = [0.0, 0.0, 0.0],
        pub back: [f32; 3] = [0.0, 0.0, 0.0],
        pub floor: [f32; 3] = [0.0, 0.0, 0.0],
        pub has_floor: bool = false,
        pub clip: Sym = Sym(0),
        pub time: f32 = 0.0,
        pub pos: [f32; 3] = [0.0, 0.0, 0.0],
        pub yaw: f32 = 0.0,
        pub exit_vel: [f32; 2] = [0.0, 0.0],
        pub exit_yaw: f32 = 0.0,
        pub moves: u32 = 0,
        pub mv: u32 = 0,
        pub origin: [f32; 3] = [0.0, 0.0, 0.0],
        pub yaw_l: f32 = 0.0,
        pub base: [f32; 3] = [0.0, 0.0, 0.0],
        pub yaw_base: f32 = 0.0,
        pub delta: [f32; 9] = [0.0; 9],
        // the velocity a catch was entered with (the way into the grab is a cubic that starts at it)
        pub catch_v: [f32; 3] = [0.0, 0.0, 0.0],
        pub gravity: f32 = 1.0,
        pub t_start: f32 = 0.0,
        pub ground_y: f32 = 0.0,
        pub land_y: f32 = 0.0,
        // How far from where a move starts the character may go along the way in: the room before the next wall (set when it starts).
        pub u_max: f32 = 100.0,
        // How far ahead (metres) a body may be sensed (the reach grows from `reach_min` at a stand to `reach_max` at a run of 5 m/s): Space far from the obstacle does not climb it.
        pub reach_min: f32 = 0.75,
        pub reach_max: f32 = 3.5,
        // How squarely the character must face what it climbs: the cosine of the widest angle between the way it faces and the way straight at the face of the obstacle (0.65: about 50 degrees; 0 asks nothing: any obstacle
        // in reach along the way it faces counts). With it the air catch looks along the way the character faces (not where the keys point) and does not grab a ledge when the keys point away from it. A game whose
        // character always faces where the camera looks sets it so that a wall run beside a wall, or running past a ledge looking along it, does not climb it.
        pub face_min: f32 = 0.0,
        // The seconds the move is entered over (inertialization span while it starts / leaves the move into motion matching): 0 keeps the animator's own.
        pub blend_in: f32 = 0.0,
        pub blend_out: f32 = 0.0,
        // The player holds forward (or whatever the game makes of it) while on a ledge: a mantle/climb that has its hands on the ledge only goes on while this is on.
        pub hold: bool = false,
        // How high (m) a ledge must be for the character to be able to hang on it: lower ones (a mantle of a metre, a climb of up to this) always play out in full, whatever is held, with no hanging, no letting go, no shimmy and
        // no push off the wall (`hang_ok` is what the engine made of the ledge that is being climbed).
        pub hang_min: f32 = 1.8,
        pub hang_ok: bool = false,
        // Let go of a mantle/climb: the move ends where it is and the character drops back from the wall (`dropped` is on for the tick it leaves).
        pub cancel: bool = false,
        // On for as long as the character hangs on the ledge waiting for `hold`.
        pub hanging: bool = false,
        pub dropped: bool = false,
        pub span_base: f32 = 0.0,
        pub restore: u32 = 0,
        // inputs of the game: the sideways keys (A -1, D 1: while hanging they shimmy along the ledge, otherwise they do nothing) and Space (pushes off the wall while climbing)
        pub side: f32 = 0.0,
        pub jump: bool = false,
        // the shimmy: how far along the ledge (to the character's left) it has gone and the limits, whether this ledge allows it (high enough for the hanging pose, wide enough), the pose being shown (0 the climb clip,
        // 1 the hanging pose) and how far the capsule has gone over to it (0..1), how far the model is lowered below the capsule (the hanging pose may put the character's origin under the ground), the climb clip's time
        // it waits at, the hanging pose's offsets (in front of the root: the hands, up: the hands)
        pub shim: f32 = 0.0,
        pub shim_lo: f32 = 0.0,
        pub shim_hi: f32 = 0.0,
        pub shim_ok: bool = false,
        pub pose: u32 = 0,
        pub pose_k: f32 = 0.0,
        pub vis: f32 = 0.0,
        pub hang_t: f32 = 0.0,
        pub hang_off: f32 = 0.0,
        pub hang_y: f32 = 0.0,
        // above: the root has got over the height of the ledge (from there the move finishes at once, `fin_speed` times as fast), when; seconds since the move began
        pub above: bool = false,
        pub t_above: f32 = 0.0,
        pub age: f32 = 0.0,
        pub fin_speed: f32 = 1.7,
        // the wall jump: `pushed` is on with `exit` after one (with the vertical speed it leaves with: `exit_vy`), `push_scale` stretches its path away from the wall, `push_blend` is the inertialization span of the push
        pub pushed: bool = false,
        pub exit_vy: f32 = 0.0,
        pub push_scale: f32 = 1.0,
        pub push_blend: f32 = 0.12,
        // the wall jump's clip plays this many times as fast (the Mixamo clip is a slow, heavy jump)
        pub push_speed: f32 = 1.7,
        // the inertialization span of the switch between the climb clip and the hanging pose
        pub pose_blend: f32 = 0.3,
        // goals of inverse kinematics (the game's `Ik` entities, kind 2, pins) that put the hands on the ledge and the feet against the wall while hanging: the places and weights are written here; `ik_w` (hands)
        // and `ik_wf` (feet) are how far they are in
        pub ik_hand_l: Uid = Uid(0),
        pub ik_hand_r: Uid = Uid(0),
        pub ik_foot_l: Uid = Uid(0),
        pub ik_foot_r: Uid = Uid(0),
        pub ik_w: f32 = 0.0,
        pub ik_wf: f32 = 0.0,
        // how far under the top of the ledge the wrists are in the hanging pose
        pub grip: f32 = 0.1,
        // the push off the wall ends when the character is off the wall (the move's `blend_out.at`) and leaves with at least this speed away from it and this speed up, into the clip the game's `push_tags` choose
        // (motion matching's tags of a fall), switched to at once (`push_exit_blend`, an inertialization span of 0.05 s)
        pub push_vel: f32 = 4.5,
        pub push_up: f32 = 2.4,
        pub push_tags: u32 = 0,
        // letting go of the ledge (`cancel`, `dropped`): the matcher's tags of the fall are set, as for the push off the wall, so that its first choice after the move is a fall and not a clip of walking (the tags it had were those of the ground)
        pub drop_tags: u32 = 0,
        pub push_exit_blend: f32 = 0.05,
        // The air catch: a script sets `catch_try` while the character falls; `caught` is on while the move that plays is a catch. A ledge is caught if it is within `catch_reach` metres in front of the feet, its top
        // is between a hand's reach under the feet and `catch_top` above them, and a catch clip whose height class (`rel`) is within `catch_tol` of the ledge's height above the feet is in the file. The clip is entered
        // `catch_lead` seconds before its catch moment. A ledge that is `catch_hang` or more above the feet is hung on (like a climb) until the player holds forward.
        pub catch_try: bool = false,
        pub caught: bool = false,
        pub catch_reach: f32 = 0.9,
        pub catch_top: f32 = 2.2,
        pub catch_tol: f32 = 0.5,
        pub catch_lead: f32 = 0.18,
        pub catch_hang: f32 = 1.1,
    }
}

type V3 = [f32; 3];

fn rot_y(v: [f32; 2], yaw: f32) -> [f32; 2] {
    let (s, c) = (detmath::sin(yaw as f64) as f32, detmath::cos(yaw as f64) as f32);
    [v[0] * c + v[1] * s, -v[0] * s + v[1] * c]
}

fn yaw_of(q: [f32; 4]) -> f32 {
    2.0 * detmath::atan2(q[1] as f64, q[3] as f64) as f32
}

fn wrap(a: f32) -> f32 {
    let pi = std::f32::consts::PI;
    let mut x = a;
    while x > pi {
        x -= 2.0 * pi;
    }
    while x < -pi {
        x += 2.0 * pi;
    }
    x
}

fn smooth(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// A box of the world (axis-aligned or turned about the vertical axis).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Obb {
    uid: u64,
    c: V3,
    h: V3,
    yaw: f32,
    traversable: bool,
    margin: f32,
}

impl Obb {
    fn to_local(&self, p: V3) -> V3 {
        let r = rot_y([p[0] - self.c[0], p[2] - self.c[2]], -self.yaw);
        [r[0], p[1] - self.c[1], r[1]]
    }

    fn to_world(&self, p: V3) -> V3 {
        let r = rot_y([p[0], p[2]], self.yaw);
        [r[0] + self.c[0], p[1] + self.c[1], r[1] + self.c[2]]
    }

    fn top(&self) -> f32 {
        self.c[1] + self.h[1]
    }

    fn bottom(&self) -> f32 {
        self.c[1] - self.h[1]
    }

    /// Is (x, z) over the box?
    fn covers(&self, x: f32, z: f32, pad: f32) -> bool {
        let l = self.to_local([x, self.c[1], z]);
        l[0].abs() <= self.h[0] + pad && l[2].abs() <= self.h[2] + pad
    }
}

fn boxes(world: &mut World) -> Vec<Obb> {
    let mut v: Vec<Obb> = Vec::new();
    let _ = world.for_each_uid::<(&Transform, &Collider), _>(|_, uid, (t, c)| {
        if c.shape == 1 && !c.trigger {
            let h = [c.size[0] * t.scale[0].abs() * 0.5, c.size[1] * t.scale[1].abs() * 0.5, c.size[2] * t.scale[2].abs() * 0.5];
            v.push(Obb { uid, c: t.pos, h, yaw: yaw_of(t.rot), traversable: false, margin: 0.2 });
        }
    });
    let mut marks: Vec<(u64, f32)> = Vec::new();
    let _ = world.for_each_uid::<(&Traversable,), _>(|_, uid, (m,)| marks.push((uid, m.margin)));
    for (uid, margin) in marks {
        if let Some(b) = v.iter_mut().find(|b| b.uid == uid) {
            b.traversable = true;
            b.margin = margin;
        }
    }
    v.sort_by_key(|b| b.uid);
    v
}

/// What the check of a place found.
#[derive(Clone, Copy, Debug, Default)]
pub struct Check {
    pub action: u32,
    pub height: f32,
    pub depth: f32,
    pub drop: f32,
    pub front: V3,
    pub normal: V3,
    pub back: V3,
    pub floor: V3,
    pub has_floor: bool,
    /// The height of the highest solid top under the ledge beyond its back face, whatever the character's own height is (the floor a hurdle that is caught from the air lands on).
    pub floor_any: Option<f32>,
    /// How far the feet are from the ledge, along the way in (positive: in front of it).
    pub distance: f32,
    /// The room beyond the ledge: how far a body can go along the way in from the ledge (a mantle) or from the back edge (a hurdle, a vault) before it meets a wall.
    pub ahead: f32,
    /// How far along the ledge, to the character's left, the hands may be put, from the front point: the ends of the edge (less the margin), the lower number is to the right.
    pub lat: [f32; 2],
}

/// How far along `dir` (a unit vector on the ground) from `from` the first box that stands in the way of a body of `radius` is, within `max`: the boxes (except `skip`) that reach into the heights `y_lo` to `y_hi`.
fn first_block(bx: &[Obb], from: [f32; 2], dir: [f32; 2], max: f32, radius: f32, y_lo: f32, y_hi: f32, skip: u64) -> Option<f32> {
    let mut best: Option<f32> = None;
    for b in bx {
        if b.uid == skip || b.top() < y_lo || b.bottom() > y_hi {
            continue;
        }
        let p = b.to_local([from[0], b.c[1], from[1]]);
        let d = rot_y(dir, -b.yaw);
        let (mut t0, mut t1) = (0.0f32, max);
        let mut hit = true;
        for (a, v, half) in [(p[0], d[0], b.h[0] + radius), (p[2], d[1], b.h[2] + radius)] {
            if v.abs() < 1e-6 {
                if a.abs() > half {
                    hit = false;
                }
            } else {
                let (u0, u1) = ((-half - a) / v, (half - a) / v);
                let (lo, hi) = if u0 < u1 { (u0, u1) } else { (u1, u0) };
                t0 = t0.max(lo);
                t1 = t1.min(hi);
                if t0 > t1 {
                    hit = false;
                }
            }
        }
        if hit && best.is_none_or(|t| t0 < t) {
            best = Some(t0);
        }
    }
    best
}

/// The check: from `feet`, facing `fwd` (a unit vector on the ground), looking `reach` ahead with a body of `radius`. `None`: no `Traversable` box in the way (or no room).
fn sense(bx: &[Obb], feet: V3, fwd: [f32; 2], reach: f32, radius: f32) -> Option<Check> {
    sense_h(bx, feet, fwd, reach, radius, MIN_HEIGHT - 0.05, f32::MAX)
}

/// The check with its own limits on the height of the top of a box above the feet: from `top_lo` (an obstacle) to `top_hi` (an air catch: what a hand reaches).
fn sense_h(bx: &[Obb], feet: V3, fwd: [f32; 2], reach: f32, radius: f32, top_lo: f32, top_hi: f32) -> Option<Check> {
    // the first box that the sweep (a segment, the box widened by the radius) enters
    let mut best: Option<(f32, usize)> = None;
    for (i, b) in bx.iter().enumerate() {
        if !b.traversable || b.top() < feet[1] + top_lo || b.top() > feet[1] + top_hi || b.bottom() > feet[1] + 1.7 {
            continue;
        }
        let p = b.to_local([feet[0], feet[1], feet[2]]);
        let d = rot_y(fwd, -b.yaw);
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        let mut hit = true;
        for (a, dir, half) in [(p[0], d[0] * reach, b.h[0] + radius), (p[2], d[1] * reach, b.h[2] + radius)] {
            if dir.abs() < 1e-6 {
                if a.abs() > half {
                    hit = false;
                }
            } else {
                let (u0, u1) = ((-half - a) / dir, (half - a) / dir);
                let (lo, hi) = if u0 < u1 { (u0, u1) } else { (u1, u0) };
                t0 = t0.max(lo);
                t1 = t1.min(hi);
                if t0 > t1 {
                    hit = false;
                }
            }
        }
        if hit && best.is_none_or(|(t, _)| t0 < t) {
            best = Some((t0, i));
        }
    }
    let (t_hit, bi) = best?;
    let b = bx[bi];
    let hit = [feet[0] + fwd[0] * reach * t_hit, feet[1], feet[2] + fwd[1] * reach * t_hit];
    let a = b.to_local(feet);
    // the face that the character is most in front of
    let faces: [([f32; 2], f32); 4] = [([1.0, 0.0], a[0] - b.h[0]), ([-1.0, 0.0], -a[0] - b.h[0]), ([0.0, 1.0], a[2] - b.h[2]), ([0.0, -1.0], -a[2] - b.h[2])];
    let (n, _) = faces.iter().copied().max_by(|x, y| x.1.total_cmp(&y.1))?;
    let hl = b.to_local(hit);
    let (along_axis, half_along, half_thick) = if n[0] != 0.0 { (2usize, b.h[2], b.h[0]) } else { (0usize, b.h[0], b.h[2]) };
    let room = (half_along - b.margin).max(0.0);
    let along = hl[along_axis].clamp(-room, room);
    let top = b.top();
    let mut front_l = [0.0, 0.0, 0.0];
    let mut back_l = [0.0, 0.0, 0.0];
    front_l[1] = top - b.c[1];
    back_l[1] = top - b.c[1];
    if n[0] != 0.0 {
        front_l[0] = n[0] * b.h[0];
        back_l[0] = -n[0] * b.h[0];
    } else {
        front_l[2] = n[1] * b.h[2];
        back_l[2] = -n[1] * b.h[2];
    }
    front_l[along_axis] = along;
    back_l[along_axis] = along;
    let front = b.to_world(front_l);
    let back = b.to_world(back_l);
    let nw = rot_y(n, b.yaw);
    let height = top - feet[1];
    let depth = 2.0 * half_thick;
    // room for the character above the front ledge
    for o in bx {
        if o.uid == b.uid || o.top() < top + 0.1 || o.bottom() > top + 1.6 {
            continue;
        }
        if o.covers(front[0] + nw[0] * radius, front[2] + nw[1] * radius, radius) {
            return None;
        }
    }
    // the floor behind: the highest solid top below the ledge, beyond the back face
    let beyond = [back[0] - nw[0] * (radius + 0.05), back[2] - nw[1] * (radius + 0.05)];
    let mut floor_y: Option<f32> = None;
    for o in bx {
        if o.uid == b.uid || o.top() > top - 0.05 {
            continue;
        }
        if o.covers(beyond[0], beyond[1], 0.0) && floor_y.is_none_or(|f| o.top() > f) {
            floor_y = Some(o.top());
        }
    }
    let has_floor = floor_y.is_some_and(|f| f >= top - height - 0.05);
    let floor = [beyond[0], floor_y.unwrap_or(top - height), beyond[1]];
    let drop = if has_floor { top - floor[1] } else { 0.0 };
    // the kind of move (Epic's rules)
    let action = if height >= MIN_HEIGHT && height <= MAX_JUMP_HEIGHT && depth < THIN && !has_floor {
        VAULT
    } else if height >= MIN_HEIGHT && height <= MAX_JUMP_HEIGHT && depth < THIN && has_floor && drop > MIN_DROP {
        HURDLE
    } else if height >= MIN_HEIGHT && height <= MAX_MANTLE_HEIGHT && depth >= THIN {
        MANTLE
    } else {
        0
    };
    let distance = (front[0] - feet[0]) * -nw[0] + (front[2] - feet[2]) * -nw[1];
    // room for the root to go on in: onto a block, as far as its top reaches and a wall on it allows; beyond a thin obstacle, as far as the landing is free (a clip would otherwise carry the character into it)
    let din = [-nw[0], -nw[1]];
    let mut action = action;
    let ahead = if depth >= THIN {
        let clear = first_block(bx, [front[0], front[2]], din, 8.0, radius, top + 0.1, top + 1.7, b.uid).unwrap_or(f32::MAX);
        let a = (depth - 0.05).min(clear - radius - 0.02);
        if a < 0.3 {
            action = 0;
        }
        a
    } else {
        let fy = if has_floor { floor[1] } else { top - height };
        let clear = first_block(bx, [back[0], back[2]], din, 6.0, radius, fy + 0.1, fy + 1.7, b.uid).unwrap_or(f32::MAX);
        let a = depth + (clear - radius - 0.02);
        if has_floor && a < depth + 0.3 {
            action = 0;
        }
        a
    };
    // the ends of the edge that the hands may reach, along the character's left (a character that faces the wall: left is the way's quarter turn)
    let left = [-nw[1], nw[0]];
    let lat = {
        let mut ends = [0.0f32; 2];
        for (k, sgn) in [-1.0f32, 1.0].into_iter().enumerate() {
            let mut e = front_l;
            e[along_axis] = sgn * room;
            let w = b.to_world(e);
            ends[k] = (w[0] - front[0]) * left[0] + (w[2] - front[2]) * left[1];
        }
        [ends[0].min(ends[1]), ends[0].max(ends[1])]
    };
    Some(Check { action, height, depth, drop, front, normal: [nw[0], 0.0, nw[1]], back, floor, has_floor, floor_any: floor_y, distance, ahead, lat })
}

/// The place and facing of the root at time `t` of a planned move: the path laid on the ledge with the warps, and the root kept out of the ground.
fn path(mv: &Move, tr: &Traversal, t: f32) -> (V3, f32) {
    let (mut p, yaw) = path_raw(mv, tr, t);
    // the root does not go further in than there is room for
    let din = [detmath::sin(tr.yaw_l as f64) as f32, detmath::cos(tr.yaw_l as f64) as f32];
    let u = (p[0] - tr.origin[0]) * din[0] + (p[2] - tr.origin[2]) * din[1];
    if u > tr.u_max {
        p[0] -= din[0] * (u - tr.u_max);
        p[2] -= din[1] * (u - tr.u_max);
    }
    // a shimmy has moved the climb along the ledge (to the character's left)
    p[0] += din[1] * tr.shim;
    p[2] -= din[0] * tr.shim;
    // the root never goes into the ground: a clip captured against a higher ledge than the real one would put it below (before the obstacle: the ground it started on; after the first window: also the floor behind)
    let first_end = mv.windows.first().map_or(0.0, |w| w.end);
    let low = if t < first_end { tr.ground_y } else { tr.ground_y.min(tr.land_y) };
    p[1] = p[1].max(low);
    (p, yaw)
}

/// The same without the floor under it (what the offsets of the windows are worked out from).
fn path_raw(mv: &Move, tr: &Traversal, t: f32) -> (V3, f32) {
    let c = mv.root_at(t);
    let mut p = laid(mv, tr, t);
    // the start offset, gone after the first window; then the offsets of the windows (smoothly over each)
    let mut yaw = tr.yaw_l + c[3];
    for (k, w) in mv.windows.iter().enumerate().take(3) {
        // (a move entered inside its first window works the start offset out over what is left of it)
        let s0 = if k == 0 { w.start.max(tr.t_start.min(w.end - 0.05)) } else { w.start };
        let ramp = smooth((t - s0) / (w.end - s0).max(0.02));
        if k == 0 {
            for i in 0..3 {
                p[i] += tr.base[i] * (1.0 - ramp);
            }
            if w.rotation {
                yaw += tr.yaw_base * (1.0 - ramp);
            } else {
                yaw += tr.yaw_base;
            }
        } else {
            for i in 0..3 {
                p[i] += tr.delta[k * 3 + i] * ramp;
            }
        }
    }
    if mv.windows.is_empty() {
        for i in 0..3 {
            p[i] += tr.base[i];
        }
        yaw += tr.yaw_base;
    }
    // the way into a catch: the clip's fall (from a great height, at 15 m/s) is not laid over the real one, which is slower: from where the character was when it grabbed, at the speed it had, the root goes in a cubic to
    // where the clip has it at the catch moment (the end of the front window), at the clip's own speed there; no jump of the speed at the start, none at the end
    if mv.catch {
        if let Some(w) = mv.windows.first() {
            let span = w.end - tr.t_start;
            if span > 0.02 && t < w.end {
                let u = ((t - tr.t_start) / span).clamp(0.0, 1.0);
                let p0 = {
                    let q = laid(mv, tr, tr.t_start);
                    [q[0] + tr.base[0], q[1] + tr.base[1], q[2] + tr.base[2]]
                };
                let (p1, e) = (laid(mv, tr, w.end), CATCH_SAMPLE);
                let (a, b) = (laid(mv, tr, w.end - e), laid(mv, tr, w.end + e));
                let (u2, u3) = (u * u, u * u * u);
                let (h00, h10, h01, h11) = (2.0 * u3 - 3.0 * u2 + 1.0, u3 - 2.0 * u2 + u, -2.0 * u3 + 3.0 * u2, u3 - u2);
                for i in 0..3 {
                    let v1 = (b[i] - a[i]) / (2.0 * e);
                    p[i] = h00 * p0[i] + h10 * span * tr.catch_v[i] + h01 * p1[i] + h11 * span * v1;
                }
            }
        }
    }
    (p, yaw)
}

/// The clip's root laid on the ledge, without the offsets of the windows.
fn laid(mv: &Move, tr: &Traversal, t: f32) -> V3 {
    let c = mv.root_at(t);
    let pk = if mv.action == WALLJUMP { tr.push_scale } else { 1.0 };
    let l = rot_y([(c[0] - mv.ledge[0]) * pk, (c[2] - mv.ledge[2]) * pk], tr.yaw_l);
    [tr.origin[0] + l[0], tr.origin[1] + (c[1] - mv.ledge[1]), tr.origin[2] + l[1]]
}

/// The step the clip's own speed at the catch moment is measured over (the clips are sampled at 30 a second).
const CATCH_SAMPLE: f32 = 1.0 / 30.0;

/// Where the hanging pose puts the character: the hands on the ledge (the capsule's place, and how far the model has to be lowered below it where the ground does not let the capsule go).
fn hang_root(tr: &Traversal) -> (V3, f32) {
    let f = [detmath::sin(tr.yaw_l as f64) as f32, detmath::cos(tr.yaw_l as f64) as f32];
    let x = tr.origin[0] - f[0] * tr.hang_off + f[1] * tr.shim;
    let z = tr.origin[2] - f[1] * tr.hang_off - f[0] * tr.shim;
    let y_pose = tr.origin[1] - tr.hang_y;
    let y = y_pose.max(tr.ground_y);
    ([x, y, z], y_pose - y)
}

/// Write one pin goal of inverse kinematics: its place, the directions it holds the joint along, and how much it counts.
pub(crate) fn set_ik(world: &mut World, uid: Uid, pos: V3, axis: V3, axis2: V3, one_sided: bool, weight: f32) {
    if uid.0 == 0 {
        return;
    }
    let Some(e) = world.entity(uid.0) else { return };
    if let Some(t) = world.get_mut::<Transform>(e) {
        t.pos = pos;
    }
    if let Some(g) = world.get_mut::<Ik>(e) {
        g.kind = 2;
        g.axis = axis;
        g.axis2 = axis2;
        g.one_sided = one_sided;
        g.weight = weight.clamp(0.0, 1.0);
    }
}

/// The path with the offsets of the windows up to (not including) `k` in place, at the end of window `k`: where the root is when the window's own offset begins to be worked out.
fn plan_offsets(mv: &Move, tr: &mut Traversal, chk: &Check) {
    tr.delta = [0.0; 9];
    for (k, w) in mv.windows.iter().enumerate().take(3) {
        if k == 0 {
            continue;
        }
        let (p, _) = path_raw(mv, tr, w.end);
        let mut d = [0.0f32; 3];
        match w.target {
            1 => {
                // over the real back edge
                d[0] = chk.back[0] - p[0];
                d[2] = chk.back[2] - p[2];
            }
            2 => {
                // down to the real floor
                if !w.ignore_z {
                    let c = mv.root_at(w.end);
                    d[1] = (chk.floor[1] + c[1]) - p[1];
                }
            }
            _ => {}
        }
        for i in 0..3 {
            tr.delta[k * 3 + i] = d[i];
        }
    }
}

fn facing(t: &Transform) -> [f32; 2] {
    let y = yaw_of(t.rot);
    [detmath::sin(y as f64) as f32, detmath::cos(y as f64) as f32]
}

/// The height of the highest solid top that is at or below `p` and has `p` over it: the ground under a character in the air (nothing: far below).
fn ground_below(bx: &[Obb], p: V3) -> f32 {
    bx.iter().filter(|o| o.top() <= p[1] + 0.05 && o.covers(p[0], p[2], 0.0)).map(|o| o.top()).fold(-1.0e6f32, f32::max)
}

/// The air catch: is there a ledge for the falling character to grab, and which clip? When there is, the move is started (the caller writes `tr` back) and `true` is the answer.
fn try_catch(world: &mut World, e: Entity, tr: &mut Traversal, data: &TraversalData, bx: &[Obb], feet: V3, own: &Transform, body: &CharacterBody, mm: Option<&MotionMatcher>) -> bool {
    let vel = mm.map(|m| m.vel_now).unwrap_or([0.0, 0.0]);
    let sp = (vel[0] * vel[0] + vel[1] * vel[1]).sqrt();
    // the way it looks for a ledge: where the keys point (the matcher's wish), else where it is going, else where it faces (the velocity alone lags the keys: a character let go of a ledge drifts back for a moment)
    let wish = mm.map(|m| m.vel).unwrap_or([0.0, 0.0]);
    let wl = (wish[0] * wish[0] + wish[1] * wish[1]).sqrt();
    let f = if tr.face_min > 0.0 {
        facing(own)
    } else if wl > 0.5 {
        [wish[0] / wl, wish[1] / wl]
    } else if sp > 0.5 {
        [vel[0] / sp, vel[1] / sp]
    } else {
        facing(own)
    };
    let radius = body.radius.max(0.3);
    // (the reach is measured from the feet to the face: the sweep is of the body's width)
    let Some(chk) = sense_h(bx, feet, f, tr.catch_reach + radius, radius, -0.15, tr.catch_top) else { return false };
    // (the way the character faces, with the keys not pointing away from it: a ledge that is not faced, or that the keys leave, is not grabbed)
    if tr.face_min > 0.0 && (-(f[0] * chk.normal[0] + f[1] * chk.normal[2]) < tr.face_min || (wl > 0.5 && (wish[0] * f[0] + wish[1] * f[1]) / wl < -0.2)) {
        return false;
    }
    // what it is: a platform (a mantle: caught, then climbed), or a thin wall (a hurdle when there is a floor under its top to land on beyond it, a vault when there is none) with room to go on in
    let thin = chk.depth < THIN;
    let floor_below = chk.floor_any.filter(|f| chk.front[1] - f > MIN_DROP && chk.front[1] - f < CATCH_MAX_DROP);
    let obst = if !thin {
        MANTLE
    } else if chk.floor_any.is_some_and(|f| chk.front[1] - f <= MIN_DROP) {
        // (a low wall with the floor right under its top: nothing to catch, the character just walks over it)
        return false;
    } else if floor_below.is_some() {
        HURDLE
    } else {
        VAULT
    };
    if chk.ahead < if thin { chk.depth + 0.3 } else { 0.3 } || chk.distance < -0.05 || chk.distance > tr.catch_reach + radius {
        return false;
    }
    let rel_now = chk.front[1] - feet[1];
    let class: u8 = if sp < 0.5 {
        0
    } else if sp < 3.0 {
        1
    } else {
        2
    };
    let mut best: Option<(f32, usize)> = None;
    for (i, m) in data.moves.iter().enumerate() {
        if !m.catch || m.action != obst {
            continue;
        }
        let err = (rel_now - m.rel).abs();
        if err > tr.catch_tol {
            continue;
        }
        let score = err + 0.25 * (m.gait as f32 - class as f32).abs();
        if best.is_none_or(|b| score < b.0) {
            best = Some((score, i));
        }
    }
    let Some((_, bi)) = best else { return false };
    let mv = &data.moves[bi];
    tr.mv = bi as u32 + 1;
    tr.action = obst;
    tr.height = rel_now;
    tr.depth = chk.depth;
    tr.drop = chk.drop;
    tr.front = chk.front;
    tr.front_n = chk.normal;
    tr.back = chk.back;
    tr.floor = chk.floor;
    tr.has_floor = chk.has_floor;
    tr.u_max = chk.ahead.max(0.0);
    tr.dropped = false;
    tr.hanging = false;
    tr.above = false;
    tr.t_above = 0.0;
    tr.age = 0.0;
    tr.pose = 0;
    tr.pose_k = 0.0;
    tr.shim = 0.0;
    tr.hang_t = 0.0;
    tr.pushed = false;
    tr.exit_vy = 0.0;
    tr.vis = 0.0;
    tr.failed = false;
    tr.caught = true;
    // a ledge well above the feet is hung on until the player holds forward (and can be shimmied along or let go of), a low one is climbed out at once
    tr.shim_ok = false;
    tr.hang_ok = obst == MANTLE && rel_now >= tr.catch_hang;
    if let (Some(sh), true) = (data.shimmy.as_ref(), tr.hang_ok) {
        let y_pose = chk.front[1] - (sh.hands_up + sh.grip);
        let (lo, hi) = (chk.lat[0] + 0.3, chk.lat[1] - 0.3);
        tr.hang_off = sh.hands_fwd + 0.03;
        tr.hang_y = sh.hands_up + sh.grip;
        tr.grip = sh.grip;
        tr.shim_ok = y_pose + sh.feet_up >= ground_below(bx, feet) + 0.12 && hi - lo > 0.2;
        (tr.shim_lo, tr.shim_hi) = (lo.min(0.0), hi.max(0.0));
    }
    let yaw_l = detmath::atan2(-chk.normal[0] as f64, -chk.normal[2] as f64) as f32;
    tr.origin = chk.front;
    tr.yaw_l = yaw_l;
    tr.gravity = if body.gravity_scale > 0.0 { body.gravity_scale } else { 1.0 };
    let t_star = (mv.hang_at - tr.catch_lead).max(0.0);
    tr.base = [0.0; 3];
    tr.yaw_base = 0.0;
    tr.delta = [0.0; 9];
    tr.t_start = t_star;
    tr.ground_y = ground_below(bx, feet);
    tr.land_y = floor_below.filter(|_| obst == HURDLE).unwrap_or(tr.ground_y);
    let (p0, y0) = path_raw(mv, tr, t_star);
    tr.base = [feet[0] - p0[0], feet[1] - p0[1], feet[2] - p0[2]];
    tr.catch_v = [body.walk[0], body.fall, body.walk[1]];
    tr.yaw_base = wrap(detmath::atan2(f[0] as f64, f[1] as f64) as f32 - y0);
    // (a hurdle lands on the floor under its top, which is where the clip's floor window goes)
    let mut chk = chk;
    if let (HURDLE, Some(f)) = (obst, floor_below) {
        chk.floor = [chk.floor[0], f, chk.floor[2]];
        chk.has_floor = true;
        tr.floor = chk.floor;
        tr.has_floor = true;
    }
    plan_offsets(mv, tr, &chk);
    let sym = Sym(world.strings_mut().intern(&mv.name));
    tr.clip = sym;
    tr.active = true;
    tr.try_for = 0.0;
    tr.catch_try = false;
    tr.time = t_star;
    if let Some(an) = world.get_mut::<Animator>(e) {
        an.clip = sym;
        an.time = t_star;
        an.speed = 1.0;
        an.playing = true;
        an.looping = false;
        an.fade = 0.0;
    }
    if let Some(m) = world.get_mut::<MotionMatcher>(e) {
        m.enabled = false;
    }
    if tr.blend_in > 0.0 {
        if let Some(i) = world.get_mut::<Inertia>(e) {
            if tr.restore == 0 {
                tr.span_base = i.span;
            }
            i.span = tr.blend_in;
            tr.restore = 3;
        }
    }
    true
}

/// Run the traversal of every `Traversal`: the check while it is asked for, the move while it plays.
pub fn update(world: &mut World, root: Option<&Path>, dt: f32) {
    let mut found: Vec<(u64, Entity, Traversal)> = Vec::new();
    let _ = world.for_each_uid::<(&Traversal,), _>(|e, uid, (t,)| found.push((uid, e, *t)));
    if found.is_empty() {
        return;
    }
    found.sort_by_key(|f| f.0);
    let Some(root) = root else { return };
    let bx = boxes(world);
    for (_, e, mut tr) in found {
        tr.exit = false;
        tr.failed = false;
        let name = world.strings().get(tr.data.0).to_string();
        let Ok(data) = load(root, &name) else {
            continue;
        };
        let parent = world.get::<Parent>(e).map(|p| p.target.0).unwrap_or(0);
        let Some(pe) = world.entity(parent) else { continue };
        let (Some(a), Some(own), Some(ptr)) = (world.get::<Animator>(e).map(|x| *x), world.get::<Transform>(e).map(|x| *x), world.get::<Transform>(pe).map(|x| *x)) else { continue };
        let body = world.get::<CharacterBody>(pe).map(|b| *b);
        let mm = world.get::<MotionMatcher>(e).map(|m| *m);
        // the inertialization of a move's entry and exit is a little longer than the usual switch; the usual one comes back after it
        if tr.restore > 0 && !tr.active {
            tr.restore -= 1;
            if tr.restore == 0 {
                if let Some(i) = world.get_mut::<Inertia>(e) {
                    i.span = tr.span_base;
                }
            }
        }
        // the model lowered below the capsule for a hanging pose comes back up after the move (a hanging pose may put the character's origin under the ground)
        if !tr.active && tr.vis != 0.0 {
            tr.vis = if tr.vis.abs() < 0.004 { 0.0 } else { tr.vis * (1.0 - (dt * 9.0).min(1.0)) };
            if let Some(l) = world.get_mut::<LocalTransform>(e) {
                l.pos[1] = tr.vis;
            }
        }
        if tr.active {
            let mv_i = (tr.mv as usize).saturating_sub(1).min(data.moves.len() - 1);
            let mv = &data.moves[mv_i];
            tr.age += dt;
            let wall_jump = mv.action == WALLJUMP;
            let climb = mv.action == MANTLE;
            // only a tall ledge can be hung on: the pause, the letting go, the shimmy and the push off the wall are for it alone, a low or middle one is climbed out in full
            let hang = climb && tr.hang_ok;
            // the time in the move's clip: while the hanging pose of a shimmy is shown another clip plays and the climb waits at `hang_t`
            let t = if tr.pose == 1 { tr.hang_t } else { a.time };
            let name_now = world.strings().get(a.clip.0).to_string();
            let over = mv.exit_over <= 0.0 || mv.root_at(t.min(mv.length))[2] - mv.ledge[2] >= mv.exit_over;
            // above the thing that is climbed: from here the move finishes at once, quickly, whatever the player does
            if climb && !tr.above && t >= mv.hang_at - 0.001 && mv.root_at(t.min(mv.length))[1] >= mv.ledge[1] - 0.03 {
                tr.above = true;
                tr.t_above = t;
            }
            // Space pushes off the wall: the wall jump takes over from where the character is
            if hang && tr.jump && !tr.above && tr.age >= 0.12 && data.walljump > 0 {
                let wj = &data.moves[data.walljump - 1];
                let sym = Sym(world.strings_mut().intern(&wj.name));
                tr.mv = data.walljump as u32;
                // the push is entered where the clip's `branch_end` says (the clip's first moments have the hands on the wall; here they are on the ledge already), laid where the character is
                let t_s = wj.branch_end.clamp(0.0, wj.length - 0.1);
                tr.origin = ptr.pos;
                tr.base = [0.0; 3];
                tr.yaw_base = 0.0;
                tr.delta = [0.0; 9];
                tr.t_start = t_s;
                tr.u_max = 100.0;
                let (p0, _) = path_raw(wj, &tr, t_s);
                tr.base = [ptr.pos[0] - p0[0], ptr.pos[1] - p0[1], ptr.pos[2] - p0[2]];
                tr.land_y = tr.ground_y;
                tr.pose = 0;
                tr.pose_k = 0.0;
                tr.hanging = false;
                tr.age = 0.0;
                tr.jump = false;
                tr.clip = sym;
                if let Some(an) = world.get_mut::<Animator>(e) {
                    an.clip = sym;
                    an.time = t_s;
                    an.speed = tr.push_speed.max(0.1);
                    an.playing = true;
                    an.looping = false;
                    an.fade = 0.0;
                }
                if let Some(i) = world.get_mut::<Inertia>(e) {
                    if tr.restore == 0 {
                        tr.span_base = i.span;
                    }
                    i.span = tr.push_blend;
                    tr.restore = 3;
                }
                // (a script has switched motion matching on again this tick: it must not pick a clip over the push)
                if let Some(m) = world.get_mut::<MotionMatcher>(e) {
                    m.enabled = false;
                }
                if let Some(w) = world.get_mut::<Traversal>(e) {
                    *w = tr;
                }
                continue;
            }
            // with the hands on the ledge the climb waits for the player to hold forward; sideways it shimmies along the ledge in the hanging pose
            let pause = hang && t >= mv.hang_at - 0.001 && !tr.above && !tr.hold;
            tr.hanging = pause;
            let side = if tr.side.abs() > 0.3 { -tr.side.signum() } else { 0.0 };
            let mut set_clip: Option<(Sym, f32, bool)> = None;
            let mut set_loop: Option<bool> = None;
            let mut shim_speed = 1.0f32;
            if let Some(sh) = data.shimmy.as_ref().filter(|_| tr.shim_ok) {
                // (only where the character stopped on the ledge: not in the middle of the pull-up, if forward was let go of there)
                let at_ledge = tr.pose == 1 || t <= mv.hang_at + 0.15;
                if pause && side != 0.0 && at_ledge {
                    let want = Sym(world.strings_mut().intern(if side > 0.0 { &sh.left } else { &sh.right }));
                    if tr.pose == 0 {
                        tr.pose = 1;
                        tr.hang_t = t;
                        set_clip = Some((want, 0.0, true));
                    } else if a.clip.0 != want.0 {
                        set_clip = Some((want, 0.0, true));
                    } else {
                        set_loop = Some(true);
                    }
                    let at_end = if side > 0.0 { tr.shim >= tr.shim_hi } else { tr.shim <= tr.shim_lo };
                    shim_speed = if at_end { 0.0 } else { 1.0 };
                    tr.shim = (tr.shim + side * sh.speed * dt).clamp(tr.shim_lo, tr.shim_hi);
                } else if tr.pose == 1 {
                    if pause {
                        // let go of the key: the cycle that was begun is finished, then the hanging pose is held
                        set_loop = Some(false);
                    } else {
                        // forward again (or the climb goes on by itself): back to the climb clip where it was left
                        tr.pose = 0;
                        set_clip = Some((Sym(world.strings_mut().intern(&mv.name)), tr.hang_t, false));
                    }
                }
            } else if tr.pose == 1 {
                tr.pose = 0;
            }
            // the speed of the clip: held (hanging), faster to the end (above), else as made; it eases between them, a hold that is exact would look like a frozen picture
            let target = if wall_jump { tr.push_speed.max(0.1) } else if pause { 0.0 } else if tr.above { tr.fin_speed.max(1.0) } else { 1.0 };
            if set_clip.is_some() && tr.pose_blend > 0.0 {
                if let Some(i) = world.get_mut::<Inertia>(e) {
                    if tr.restore == 0 {
                        tr.span_base = i.span;
                    }
                    i.span = tr.pose_blend;
                    tr.restore = 3;
                }
            }
            if let Some(an) = world.get_mut::<Animator>(e) {
                if let Some((c, tm, lp)) = set_clip {
                    an.clip = c;
                    an.time = tm;
                    an.looping = lp;
                    an.playing = true;
                    an.fade = 0.0;
                    an.speed = 1.0;
                }
                if let Some(lp) = set_loop {
                    an.looping = lp;
                }
                if tr.pose == 1 {
                    an.speed = shim_speed;
                } else if wall_jump {
                    an.speed = target;
                } else {
                    an.speed += (target - an.speed) * (dt * 20.0).min(1.0);
                    if target == 0.0 && an.speed < 0.03 {
                        an.speed = 0.0;
                    }
                }
            }
            tr.pose_k = if tr.pose == 1 { (tr.pose_k + dt / 0.3).min(1.0) } else { (tr.pose_k - dt / 0.3).max(0.0) };
            // (a catch of any height can be let go of while the hands are on the ledge: from the grab to half a second after it)
            let cancelled = (hang || (tr.caught && t < mv.hang_at + 0.5)) && tr.cancel && t < mv.length;
            let ends = if wall_jump {
                t >= mv.blend_at
            } else if climb {
                t >= mv.length || (tr.above && over && t >= tr.t_above + 0.1)
            } else {
                t >= mv.length || (t >= mv.blend_at && over && (!mv.wait_input || tr.move_input)) || (tr.move_input && over && t >= mv.early_out)
            };
            let finished = (tr.pose == 0 && set_clip.is_none() && name_now != mv.name) || ends || cancelled;
            let (path_p, yaw) = path(mv, &tr, t.min(mv.length));
            let mut p = path_p;
            let k = smooth(tr.pose_k);
            let mut vis = 0.0;
            if k > 0.0 {
                // the hanging pose: the capsule goes where the pose has the hands on the ledge, the model is lowered by what the ground does not let the capsule go
                let (ph, v) = hang_root(&tr);
                for i in 0..3 {
                    p[i] += (ph[i] - p[i]) * k;
                }
                vis = v * k;
            }
            tr.vis = vis;
            tr.pos = p;
            tr.yaw = yaw;
            tr.time = t;
            if let Some(m) = world.get_mut::<Transform>(pe) {
                m.pos = p;
            }
            let q = [0.0, detmath::sin(yaw as f64 * 0.5) as f32, 0.0, detmath::cos(yaw as f64 * 0.5) as f32];
            if let Some(l) = world.get_mut::<LocalTransform>(e) {
                l.rot = q;
                l.pos[1] = vis;
            }
            if let Some(cb) = world.get_mut::<CharacterBody>(pe) {
                cb.walk = [0.0, 0.0];
                cb.jump = 0.0;
                cb.fall = 0.0;
                cb.gravity_scale = 0.0;
            }
            if let Some(m) = world.get_mut::<MotionMatcher>(e) {
                m.enabled = false;
            }
            // hands on the ledge and feet against the wall while hanging (and a moment before and after): the goals ease in and out
            let hands_on = climb && !tr.above && (pause || tr.pose == 1 || (t >= mv.hang_at - 0.2 && t < mv.hang_at + 0.06));
            let ease = (dt * 12.0).min(1.0);
            tr.ik_w += ((hands_on as u8 as f32) - tr.ik_w) * ease;
            tr.ik_wf += ((hands_on as u8 as f32) - tr.ik_wf) * ease;
            if tr.ik_w < 0.003 {
                tr.ik_w = 0.0;
                tr.ik_wf = 0.0;
            }
            {
                let f = [detmath::sin(tr.yaw_l as f64) as f32, detmath::cos(tr.yaw_l as f64) as f32];
                let left = [f[1], -f[0]];
                // (the wrist is on top of the ledge in the climb clip's pose, at the wall's face and under the top in the hanging pose: the fingers curl over the edge)
                let kk = smooth(tr.pose_k);
                let (over, up_by) = (0.06 + (-0.03 - 0.06) * kk, 0.035 + (-tr.grip - 0.035) * kk);
                let hand = [tr.origin[0] + left[0] * tr.shim + f[0] * over, tr.origin[1] + up_by, tr.origin[2] + left[1] * tr.shim + f[1] * over];
                let foot = [tr.origin[0] + left[0] * tr.shim - f[0] * 0.075, tr.origin[1] - 1.0, tr.origin[2] + left[1] * tr.shim - f[1] * 0.075];
                let (up, into) = ([0.0, 1.0, 0.0], [f[0], 0.0, f[1]]);
                set_ik(world, tr.ik_hand_l, hand, up, into, false, tr.ik_w);
                set_ik(world, tr.ik_hand_r, hand, up, into, false, tr.ik_w);
                // feet: braced against the wall in the shimmy's pose (on the wall's plane), only kept out of the wall in the climb clip's
                let (axis, one) = if tr.pose == 1 { (into, false) } else { ([-into[0], 0.0, -into[2]], true) };
                set_ik(world, tr.ik_foot_l, foot, axis, [0.0; 3], one, tr.ik_wf);
                set_ik(world, tr.ik_foot_r, foot, axis, [0.0; 3], one, tr.ik_wf);
            }
            if finished {
                tr.pushed = false;
                tr.exit_vy = 0.0;
                if cancelled {
                    // let go: out of the wall (the body is 0.4 m from the ledge's face at least), and down; a slow drift back, so that the body keeps facing the wall
                    let din = [detmath::sin(tr.yaw_l as f64) as f32, detmath::cos(tr.yaw_l as f64) as f32];
                    let u = (p[0] - tr.origin[0]) * din[0] + (p[2] - tr.origin[2]) * din[1];
                    let back = (u + 0.4).max(0.0);
                    tr.exit_vel = [-din[0] * 0.6, -din[1] * 0.6];
                    tr.dropped = true;
                    if let Some(m) = world.get_mut::<Transform>(pe) {
                        m.pos = [p[0] - din[0] * back, p[1], p[2] - din[1] * back];
                    }
                } else {
                    let (p0, _) = path(mv, &tr, (t - 0.1).max(0.0).min(mv.length));
                    // (a tenth of a second of the clip is a tenth over its playback speed of ours)
                    let k = world.get::<Animator>(e).map(|an| an.speed).unwrap_or(1.0).max(0.05) / 0.1;
                    tr.exit_vel = [(p[0] - p0[0]) * k, (p[2] - p0[2]) * k];
                    tr.dropped = false;
                    if wall_jump {
                        // the push off the wall: off the wall now; it leaves with momentum, away from it and up (at least `push_vel` and `push_up`, more if the clip's own path was faster)
                        tr.pushed = true;
                        let din = [detmath::sin(tr.yaw_l as f64) as f32, detmath::cos(tr.yaw_l as f64) as f32];
                        let v = (tr.exit_vel[0] * tr.exit_vel[0] + tr.exit_vel[1] * tr.exit_vel[1]).sqrt().max(tr.push_vel);
                        tr.exit_vel = [-din[0] * v, -din[1] * v];
                        tr.exit_vy = ((p[1] - p0[1]) * k).max(tr.push_up);
                    }
                }
                tr.exit_yaw = yaw;
                tr.exit = true;
                tr.active = false;
                tr.caught = false;
                tr.hanging = false;
                tr.pose = 0;
                tr.pose_k = 0.0;
                tr.above = false;
                tr.ik_w = 0.0;
                tr.ik_wf = 0.0;
                for g in [tr.ik_hand_l, tr.ik_hand_r, tr.ik_foot_l, tr.ik_foot_r] {
                    set_ik(world, g, [0.0; 3], [0.0, 1.0, 0.0], [0.0; 3], false, 0.0);
                }
                if let Some(an) = world.get_mut::<Animator>(e) {
                    an.speed = 1.0;
                }
                if let Some(cb) = world.get_mut::<CharacterBody>(pe) {
                    cb.gravity_scale = if tr.gravity > 0.0 { tr.gravity } else { 1.0 };
                    if tr.pushed {
                        cb.fall = tr.exit_vy;
                    }
                }
                if let Some(m) = world.get_mut::<MotionMatcher>(e) {
                    // motion matching takes over with a fresh choice, knowing the speed the move left with (and not the jump in place since its last look)
                    m.enabled = true;
                    m.force = true;
                    m.started = false;
                    if tr.pushed && tr.push_tags != 0 {
                        m.tags = tr.push_tags;
                    }
                    if tr.dropped && tr.drop_tags != 0 {
                        m.tags = tr.drop_tags;
                    }
                    m.vel_now = tr.exit_vel;
                    m.speed = (tr.exit_vel[0] * tr.exit_vel[0] + tr.exit_vel[1] * tr.exit_vel[1]).sqrt();
                }
                let span = if tr.pushed { tr.push_exit_blend } else { tr.blend_out };
                if span > 0.0 {
                    if let Some(i) = world.get_mut::<Inertia>(e) {
                        if tr.restore == 0 {
                            tr.span_base = i.span;
                        }
                        i.span = span;
                        tr.restore = 3;
                    }
                }
            }
            if let Some(w) = world.get_mut::<Traversal>(e) {
                *w = tr;
            }
            continue;
        }
        if tr.catch_try {
            let falling = body.is_some_and(|b| !b.grounded && b.fall < 1.5);
            if let (true, Some(b)) = (falling, body) {
                if try_catch(world, e, &mut tr, &data, &bx, ptr.pos, &own, &b, mm.as_ref()) {
                    if let Some(w) = world.get_mut::<Traversal>(e) {
                        *w = tr;
                    }
                    continue;
                }
            }
        }
        if !(tr.try_for > 0.0) {
            // nothing asked: the one-tick answers (`failed`, `exit`) are cleared
            if let Some(w) = world.get_mut::<Traversal>(e) {
                w.failed = false;
                w.exit = false;
            }
            continue;
        }
        tr.try_for = (tr.try_for - dt).max(0.0);
        let f = facing(&own);
        let vel = mm.map(|m| m.vel_now).unwrap_or([0.0, 0.0]);
        let speed = (vel[0] * f[0] + vel[1] * f[1]).max(0.0);
        let reach = tr.reach_min + (tr.reach_max - tr.reach_min) * (speed / 5.0).clamp(0.0, 1.0);
        let radius = body.map(|b| b.radius).unwrap_or(0.35).max(0.3);
        let Some(chk) = sense(&bx, ptr.pos, f, reach, radius) else {
            tr.action = 0;
            tr.failed = true;
            tr.try_for = 0.0;
            if let Some(w) = world.get_mut::<Traversal>(e) {
                *w = tr;
            }
            continue;
        };
        // (an obstacle that is not faced: not a thing to climb)
        if tr.face_min > 0.0 && -(f[0] * chk.normal[0] + f[1] * chk.normal[2]) < tr.face_min {
            tr.action = 0;
            tr.failed = true;
            tr.try_for = 0.0;
            if let Some(w) = world.get_mut::<Traversal>(e) {
                *w = tr;
            }
            continue;
        }
        tr.action = chk.action;
        tr.height = chk.height;
        tr.depth = chk.depth;
        tr.drop = chk.drop;
        tr.front = chk.front;
        tr.front_n = chk.normal;
        tr.back = chk.back;
        tr.floor = chk.floor;
        tr.has_floor = chk.has_floor;
        if chk.action == 0 {
            tr.failed = true;
            tr.try_for = 0.0;
            if let Some(w) = world.get_mut::<Traversal>(e) {
                *w = tr;
            }
            continue;
        }
        // choose among the moves of this kind: the gait, the height class, then the pose
        let gait = if speed < 0.5 {
            0u8
        } else if speed < 3.0 {
            1
        } else {
            2
        };
        let climb = chk.action == MANTLE && chk.height > CLIMB_ABOVE;
        let kind_ok = |m: &Move| !m.catch && m.action == chk.action && (chk.action != MANTLE || m.climb == climb);
        let mut pool: Vec<usize> = data.moves.iter().enumerate().filter(|(_, m)| kind_ok(m) && m.gait == gait).map(|(i, _)| i).collect();
        let mut g = gait;
        while pool.is_empty() && g > 0 {
            g -= 1;
            pool = data.moves.iter().enumerate().filter(|(_, m)| kind_ok(m) && m.gait == g).map(|(i, _)| i).collect();
        }
        if pool.is_empty() {
            pool = data.moves.iter().enumerate().filter(|(_, m)| kind_ok(m)).map(|(i, _)| i).collect();
        }
        if pool.is_empty() {
            tr.failed = true;
            tr.try_for = 0.0;
            if let Some(w) = world.get_mut::<Traversal>(e) {
                *w = tr;
            }
            continue;
        }
        let yaw_l = detmath::atan2(-chk.normal[0] as f64, -chk.normal[2] as f64) as f32;
        let db = mm.and_then(|m| motionmatch::load(root, world.strings().get(m.db.0)).ok());
        // the character's own pose and path, to be matched
        let query: Option<[f32; motionmatch::DIM]> = match (&db, mm) {
            (Some(db), Some(m)) => {
                let cur_name = world.strings().get(a.clip.0).to_string();
                let traj = motionmatch::trajectory_query(f, vel, vel, f, &Dynamics::new(m.accel, m.brake, m.turn));
                let mut q = [0.0f32; motionmatch::DIM];
                q[..POSE_START].copy_from_slice(&traj);
                if let Some(c) = db.clip_index(&cur_name) {
                    let ft = db.features_at(c, a.time);
                    q[POSE_START..].copy_from_slice(&ft[POSE_START..]);
                }
                Some(q)
            }
            _ => None,
        };
        let mut best: Option<(f32, usize, f32, f32)> = None; // (score, move, start time, distance still to run before it can start)
        for &i in &pool {
            let mv = &data.moves[i];
            // the moment of the clip where it is as far from the ledge as the character is
            let s = |t: f32| mv.ledge[2] - mv.root_at(t)[2];
            let (s_far, s_near) = (s(0.0), s(mv.branch_end.max(0.05)));
            let mut t_star = 0.0f32;
            let mut err = f32::MAX;
            let steps = ((mv.branch_end.max(0.05)) * mv.rate) as usize + 1;
            for k in 0..=steps {
                let t = (k as f32 / mv.rate).min(mv.branch_end.max(0.05));
                let e2 = (s(t) - chk.distance).abs();
                if e2 < err {
                    err = e2;
                    t_star = t;
                }
            }
            let wait = (chk.distance - s_far).max(0.0);
            let near = (s_near - chk.distance).max(0.0);
            let mut score = 2.0 * err + near;
            if let (Some(db), Some(q)) = (&db, &query) {
                if let Some(c) = db.clip_index(&mv.name) {
                    let fr = db.features_at(c, t_star);
                    score += db.cost(q, &fr, true);
                }
            }
            if best.is_none_or(|b| score < b.0) {
                best = Some((score, i, t_star, wait));
            }
        }
        let (_, bi, t_star, wait) = best.expect("a pool is not empty");
        if wait > 0.3 {
            // still too far from the ledge for the clip to begin: keep running (the check goes on)
            if tr.try_for <= 0.0 {
                tr.failed = true;
            }
            if let Some(w) = world.get_mut::<Traversal>(e) {
                *w = tr;
            }
            continue;
        }
        let mv = &data.moves[bi];
        tr.mv = bi as u32 + 1;
        tr.caught = false;
        tr.u_max = chk.ahead.max(0.0);
        tr.dropped = false;
        tr.hanging = false;
        tr.above = false;
        tr.t_above = 0.0;
        tr.age = 0.0;
        tr.pose = 0;
        tr.pose_k = 0.0;
        tr.shim = 0.0;
        tr.hang_t = 0.0;
        tr.pushed = false;
        tr.exit_vy = 0.0;
        tr.vis = 0.0;
        // a shimmy along this ledge: the hanging pose needs the feet clear of the ground and the ledge wide enough for the hands
        tr.shim_ok = false;
        tr.hang_ok = mv.action == MANTLE && chk.height >= tr.hang_min;
        if let (Some(sh), true) = (data.shimmy.as_ref(), mv.action == MANTLE && tr.hang_ok) {
            let y_pose = chk.front[1] - (sh.hands_up + sh.grip);
            let (lo, hi) = (chk.lat[0] + 0.3, chk.lat[1] - 0.3);
            // the wrist at the wall's face (a little outside it) and under the top of the ledge by `grip`: the hand's fingers go over the edge
            tr.hang_off = sh.hands_fwd + 0.03;
            tr.hang_y = sh.hands_up + sh.grip;
            tr.grip = sh.grip;
            tr.shim_ok = y_pose + sh.feet_up >= ptr.pos[1] + 0.12 && hi - lo > 0.2;
            (tr.shim_lo, tr.shim_hi) = (lo.min(0.0), hi.max(0.0));
        }
        tr.origin = chk.front;
        tr.yaw_l = yaw_l;
        tr.gravity = body.map(|b| b.gravity_scale).filter(|g| *g > 0.0).unwrap_or(1.0);
        // the path laid on the real ledge, the character's own place and facing as the starting offsets
        tr.base = [0.0; 3];
        tr.yaw_base = 0.0;
        tr.delta = [0.0; 9];
        tr.t_start = t_star;
        tr.ground_y = ptr.pos[1];
        tr.land_y = if chk.has_floor { chk.floor[1] } else { ptr.pos[1] };
        let (p0, y0) = path_raw(mv, &tr, t_star);
        tr.base = [ptr.pos[0] - p0[0], ptr.pos[1] - p0[1], ptr.pos[2] - p0[2]];
        tr.yaw_base = wrap(detmath::atan2(f[0] as f64, f[1] as f64) as f32 - y0);
        plan_offsets(mv, &mut tr, &chk);
        let sym = Sym(world.strings_mut().intern(&mv.name));
        tr.clip = sym;
        tr.active = true;
        tr.try_for = 0.0;
        tr.time = t_star;
        if let Some(an) = world.get_mut::<Animator>(e) {
            an.clip = sym;
            an.time = t_star;
            an.speed = 1.0;
            an.playing = true;
            an.looping = false;
            an.fade = 0.0;
        }
        if let Some(m) = world.get_mut::<MotionMatcher>(e) {
            m.enabled = false;
        }
        if tr.blend_in > 0.0 {
            if let Some(i) = world.get_mut::<Inertia>(e) {
                if tr.restore == 0 {
                    tr.span_base = i.span;
                }
                i.span = tr.blend_in;
                tr.restore = 3;
            }
        }
        if let Some(w) = world.get_mut::<Traversal>(e) {
            *w = tr;
        }
    }
}

#[cfg(test)]
pub(crate) fn boxes_for_tests(world: &mut World) -> Vec<Obb> {
    boxes(world)
}

#[cfg(test)]
pub(crate) fn sense_for_tests(bx: &[Obb], feet: V3, fwd: [f32; 2], reach: f32, radius: f32) -> Option<Check> {
    sense(bx, feet, fwd, reach, radius)
}
