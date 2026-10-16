//! Swimming, wading, rolling, ladders and ledges for characters (`CharacterMotion` and `Climbable`; docs/GAMEPLAY.md, "Character motion").
//! `character.rs` calls `pre` for every character that has a `CharacterMotion`, before it moves it: `pre` reads this tick's input, advances the
//! clocks and the state machines (wading and swimming, a roll, a climb, a mantle) and answers with a `Plan`: the horizontal speed and the jump the
//! character really gets, whether gravity works on it this tick, the vertical speed it starts with, and (while it is being pulled up a ledge) the
//! place it is carried to. The walk itself, the collisions and the grounding stay the controller's. Everything is f32 with only `+ - * / sqrt`, in a
//! fixed order, so results are identical on every machine.
use crate::character::{blocked, lower_to_contact, too_steep, Grid};
use crate::components::{CharacterBody, CharacterMotion, Climbable};
use lunaris_ecs::Uid;

type V3 = [f32; 3];

/// A ladder of the world: its uid, foot, turn and shape.
pub(crate) struct Ladder {
    pub uid: u64,
    pub pos: V3,
    pub rot: [f32; 4],
    pub c: Climbable,
}

/// What `pre` needs to know about the world around the character.
pub(crate) struct Env<'a, 'g> {
    pub grid: &'a Grid<'g>,
    pub r: f32,
    pub height: f32,
    pub step_h: f32,
    pub min_up: f32,
    pub dt: f32,
    /// The height of the water's surface at the character's x, z, when there is water.
    pub surface: Option<f32>,
    pub ladders: &'a [Ladder],
}

/// What the controller does with the character this tick.
pub(crate) struct Plan {
    /// The horizontal speed (x, z) it gets.
    pub walk: [f32; 2],
    /// The jump it gets (an upward speed, honoured on the ground only).
    pub jump: f32,
    /// Does gravity pull it this tick? (Not while swimming or on a ladder.)
    pub gravity: bool,
    /// The vertical speed it starts the tick with.
    pub fall: f32,
    /// Set while it is pulled up a ledge: the place it is at after this tick; nothing else moves it.
    pub carried: Option<V3>,
}

fn finite(v: f32, default: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        default
    }
}

/// `v` turned by the unit quaternion `q` (x, y, z, w).
pub(crate) fn turn(q: [f32; 4], v: V3) -> V3 {
    let u = [q[0], q[1], q[2]];
    let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = cross(u, v);
    let t = [2.0 * t[0], 2.0 * t[1], 2.0 * t[2]];
    let c = cross(u, t);
    [v[0] + q[3] * t[0] + c[0], v[1] + q[3] * t[1] + c[1], v[2] + q[3] * t[2] + c[2]]
}

fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Where a character at `pos` that is pushing along `d` (a unit vector in x, z) lands when it is pulled up a ledge: the capsule is raised by `h` (from `lo` to `hi`,
/// in steps) until it clears the ledge's top, moved over it, and lowered onto it. `wall` asks for a wall in the way first (a jump is only a ledge grab when there is
/// something to grab: `gap` is how near the wall must be).
fn find_ledge(env: &Env, pos: V3, d: [f32; 2], lo: f32, hi: f32, gap: Option<f32>) -> Option<V3> {
    let (r, height) = (env.r, env.height);
    let mut near: Vec<usize> = Vec::new();
    // how far the capsule can go at its own height before something is in the way: the ledge's edge is there (and a wall is needed unless the caller knows better)
    let mut hit = None;
    let mut s = 0.0f32;
    while s <= 1.5 {
        if blocked([pos[0] + d[0] * s, pos[1], pos[2] + d[1] * s], r, height, env.grid, &mut near) {
            hit = Some(s);
            break;
        }
        s += 0.05;
    }
    if let Some(gap) = gap {
        // a wall must be right there
        if hit.is_none_or(|s| s > gap) {
            return None;
        }
    }
    // the capsule lands a radius and a bit beyond the edge, so that it stands on the ledge and not on its rim
    let reach = hit.unwrap_or(0.0) + r * 2.0 + 0.1;
    let mut h = lo.max(0.0);
    while h <= hi + 1.0e-4 {
        let up = [pos[0], pos[1] + h, pos[2]];
        if blocked(up, r, height, env.grid, &mut near) {
            return None;
        }
        let cand = [up[0] + d[0] * reach, up[1], up[2] + d[1] * reach];
        if !blocked(cand, r, height, env.grid, &mut near) {
            let (p, touched) = lower_to_contact(cand, h + 0.3, r, height, env.grid, &mut near);
            if !touched {
                return None;
            }
            let rise = p[1] - pos[1];
            if rise >= lo - 0.05 && rise <= hi + 0.05 && !too_steep(p, r, height, env.grid, env.min_up) {
                return Some(p);
            }
            return None;
        }
        h += 0.1;
    }
    None
}

/// The ladder `uid` and where the character is in its space: (sideways, in front of it, up from its foot), and the unit vector of its front in x, z.
fn in_ladder(l: &Ladder, pos: V3) -> (f32, f32, f32, [f32; 2]) {
    let f = turn(l.rot, [0.0, 0.0, 1.0]);
    let len = (f[0] * f[0] + f[2] * f[2]).sqrt();
    let f = if len > 1.0e-6 { [f[0] / len, f[2] / len] } else { [0.0, 1.0] };
    let d = [pos[0] - l.pos[0], pos[1] - l.pos[1], pos[2] - l.pos[2]];
    let side = [f[1], -f[0]];
    (d[0] * side[0] + d[2] * side[1], d[0] * f[0] + d[2] * f[1], d[1], f)
}

fn start_mantle(m: &mut CharacterMotion, from: V3, to: V3) {
    m.mantling = true;
    m.mantle_left = finite(m.mantle_time, 0.55).clamp(0.05, 5.0);
    m.mantle_from = from;
    m.mantle_to = to;
    m.climbing = false;
    m.rolling = false;
}

/// Advance the character's motion state for this tick and decide what the controller does with it.
pub(crate) fn pre(m: &mut CharacterMotion, c: &CharacterBody, pos: V3, grounded: bool, fall: f32, env: &Env) -> Plan {
    let dt = env.dt;
    let mut walk = [finite(c.walk[0], 0.0), finite(c.walk[1], 0.0)];
    let wlen = (walk[0] * walk[0] + walk[1] * walk[1]).sqrt();
    let vertical = finite(m.vertical, 0.0).clamp(-1.0, 1.0);
    let want_roll = m.roll;
    m.roll = false;
    m.roll_wait = (m.roll_wait - dt).max(0.0);
    m.climb_lock = (m.climb_lock - dt).max(0.0);
    let mut plan = Plan { walk, jump: c.jump, gravity: true, fall, carried: None };

    // ---- being pulled up a ledge: nothing else moves the character
    if m.mantling {
        m.mantle_left -= dt;
        let total = finite(m.mantle_time, 0.55).clamp(0.05, 5.0);
        let u = (1.0 - m.mantle_left / total).clamp(0.0, 1.0);
        let (a, b) = (ease(u / 0.6), ease((u - 0.35) / 0.65));
        let (f, t) = (m.mantle_from, m.mantle_to);
        let at = [f[0] + (t[0] - f[0]) * b, f[1] + (t[1] - f[1]) * a, f[2] + (t[2] - f[2]) * b];
        if m.mantle_left <= 0.0 {
            m.mantling = false;
            m.mantle_left = 0.0;
            m.swimming = false;
            m.wading = false;
        }
        plan.carried = Some(if m.mantling { at } else { t });
        plan.walk = [0.0, 0.0];
        plan.jump = 0.0;
        plan.gravity = false;
        plan.fall = 0.0;
        return plan;
    }

    // ---- the water: how deep, and wading or swimming (with a little hysteresis so that the edge does not flicker)
    let depth = env.surface.map_or(0.0, |s| (s - pos[1]).max(0.0));
    m.depth = depth;
    let swim_depth = finite(m.swim_depth, 1.1).max(0.2);
    m.swimming = depth > 0.0 && if m.swimming { depth > swim_depth - 0.15 } else { depth >= swim_depth };
    m.wading = depth > 0.0 && !m.swimming;
    m.under = env.surface.is_some_and(|s| pos[1] + env.height * 0.9 < s);

    // ---- a roll that is going on
    if m.rolling {
        m.roll_left -= dt;
        if m.roll_left <= 0.0 || m.swimming {
            m.rolling = false;
            m.roll_left = 0.0;
            m.roll_wait = finite(m.roll_cooldown, 0.25).max(0.0);
        } else {
            let t01 = (m.roll_left / finite(m.roll_time, 0.6).clamp(0.05, 5.0)).clamp(0.0, 1.0);
            let speed = finite(m.roll_speed, 7.0) * (0.25 + 0.75 * t01 * t01);
            plan.walk = [m.roll_dir[0] * speed, m.roll_dir[1] * speed];
            plan.jump = 0.0;
            return plan;
        }
    }

    // ---- a roll that starts: on the ground, along the walk (or `roll_dir` when there is no walk)
    if want_roll && grounded && m.roll_wait <= 0.0 && !m.swimming && !m.climbing {
        let dir = if wlen > 0.05 {
            Some([walk[0] / wlen, walk[1] / wlen])
        } else {
            let (x, z) = (finite(m.roll_dir[0], 0.0), finite(m.roll_dir[1], 0.0));
            let l = (x * x + z * z).sqrt();
            (l > 0.05).then(|| [x / l, z / l])
        };
        if let Some(d) = dir {
            m.rolling = true;
            m.roll_left = finite(m.roll_time, 0.6).clamp(0.05, 5.0);
            m.roll_dir = d;
            let speed = finite(m.roll_speed, 7.0);
            plan.walk = [d[0] * speed, d[1] * speed];
            plan.jump = 0.0;
            return plan;
        }
    }

    // ---- a ladder: let go, climb, or take hold
    if m.climbing {
        let ladder = env.ladders.iter().find(|l| l.uid == m.ladder.0);
        let Some(l) = ladder else {
            m.climbing = false;
            return plan;
        };
        let (lx, lz, ly, f) = in_ladder(l, pos);
        let (half, top) = (l.c.width.abs() * 0.5, l.c.height.abs());
        if c.jump > 0.0 {
            // let go with a little hop back
            m.climbing = false;
            m.climb_lock = 0.4;
            plan.fall = c.jump * 0.5;
            plan.jump = 0.0;
            return plan;
        }
        if lx.abs() > half + 0.4 || lz > l.c.reach.abs() + 0.8 || ly < -0.3 || ly > top + 0.6 {
            m.climbing = false;
            m.climb_lock = 0.25;
            return plan;
        }
        if vertical < -0.05 && (grounded || ly <= 0.02) {
            // down to the ground
            m.climbing = false;
            m.climb_lock = 0.3;
            return plan;
        }
        if vertical > 0.05 && ly >= top - 0.05 {
            // at the top: step onto whatever is behind the ladder
            let back = [-f[0], -f[1]];
            if let Some(to) = find_ledge(env, pos, back, -0.4, 1.2, None) {
                start_mantle(m, pos, to);
                m.climb_lock = 0.3;
                plan.carried = Some(pos);
                plan.walk = [0.0, 0.0];
                plan.jump = 0.0;
                plan.gravity = false;
                plan.fall = 0.0;
                return plan;
            }
        }
        let speed = vertical * finite(m.climb_speed, 2.0) * l.c.speed.abs();
        // hold on to the ladder: closer to its plane, at the climber's distance from it
        let want = env.r + 0.08;
        let gap = [(l.pos[0] + f[0] * want) - pos[0], (l.pos[2] + f[1] * want) - pos[2]];
        let (side_x, side_z) = (f[1], -f[0]);
        let on_plane = gap[0] * f[0] + gap[1] * f[1];
        let along = lx.clamp(-half, half) - lx;
        let towards = [f[0] * on_plane + side_x * along, f[1] * on_plane + side_z * along];
        plan.walk = [towards[0] / dt.max(1.0e-4), towards[1] / dt.max(1.0e-4)];
        plan.walk = {
            let n = (plan.walk[0] * plan.walk[0] + plan.walk[1] * plan.walk[1]).sqrt();
            if n > 6.0 {
                [plan.walk[0] * 6.0 / n, plan.walk[1] * 6.0 / n]
            } else {
                plan.walk
            }
        };
        plan.jump = 0.0;
        plan.gravity = false;
        plan.fall = speed;
        return plan;
    }
    if m.climb_lock <= 0.0 && finite(m.climb_speed, 2.0) > 0.0 && !m.swimming {
        let mut grab: Option<u64> = None;
        for l in env.ladders {
            let (lx, lz, ly, f) = in_ladder(l, pos);
            let (half, top, reach) = (l.c.width.abs() * 0.5, l.c.height.abs(), l.c.reach.abs());
            if lx.abs() > half + env.r || lz < -0.2 || lz > reach + env.r || ly < -0.15 || ly > top + 0.3 {
                continue;
            }
            let into = wlen > 0.1 && (walk[0] * -f[0] + walk[1] * -f[1]) > 0.5 * wlen;
            let up = vertical > 0.2 && ly < top - 0.3;
            let down = vertical < -0.2 && ly > top - 0.8;
            if into || up || down {
                grab = Some(l.uid);
                break;
            }
        }
        if let Some(uid) = grab {
            m.climbing = true;
            m.ladder = Uid(uid);
            plan.fall = 0.0;
            plan.gravity = false;
            plan.jump = 0.0;
            plan.walk = [0.0, 0.0];
            return plan;
        }
    }

    // ---- a ledge: a jump against it pulls the character up, and so does a swimmer pushing against a bank
    let reach_up = finite(m.mantle_height, 1.6);
    if reach_up > env.step_h && wlen > 0.1 && (m.swimming && wlen > 0.5 || c.jump > 0.0 && grounded) {
        let d = [walk[0] / wlen, walk[1] / wlen];
        let hi = if m.swimming { finite(m.float_depth, 1.35) + reach_up * 0.5 } else { reach_up };
        if let Some(to) = find_ledge(env, pos, d, env.step_h + 0.02, hi, Some(if m.swimming { 0.6 } else { 0.4 })) {
            start_mantle(m, pos, to);
            plan.carried = Some(pos);
            plan.walk = [0.0, 0.0];
            plan.jump = 0.0;
            plan.gravity = false;
            plan.fall = 0.0;
            return plan;
        }
    }

    // ---- wading slows the walk; swimming has no gravity, a float level, and its own speeds
    if m.swimming {
        let top = finite(m.swim_speed, 2.4).max(0.0);
        if wlen > top && wlen > 0.0 {
            walk = [walk[0] * top / wlen, walk[1] * top / wlen];
        }
        let dive = finite(m.dive_speed, 2.0).max(0.0);
        let level = env.surface.unwrap_or(pos[1]) - finite(m.float_depth, 1.35);
        let want = if vertical.abs() > 0.05 {
            let v = vertical * dive;
            // rising eases into the float level and never goes above it
            if v > 0.0 {
                v.min((level - pos[1]) * 3.0).max(-dive)
            } else {
                v
            }
        } else {
            ((level - pos[1]) * 3.0).clamp(-dive, dive)
        };
        let k = (8.0 * dt).min(1.0);
        plan.fall = fall + (want - fall) * k;
        plan.gravity = false;
        plan.jump = 0.0;
        plan.walk = walk;
    } else if m.wading {
        let slow = 1.0 - (1.0 - finite(m.wade_speed, 0.7).clamp(0.0, 1.0)) * (depth / swim_depth).min(1.0);
        plan.walk = [walk[0] * slow, walk[1] * slow];
    }
    plan
}
