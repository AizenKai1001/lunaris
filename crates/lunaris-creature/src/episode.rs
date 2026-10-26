//! The trainer's episode, in Rust: the same task that `simulate_spider` of `creature_core.h` gives a creature (targets that appear relative to the head and the heading and are reached within 0.30 m, levels of
//! them, shoves, and the score). The game does not need it; the tests do: they run the golden episodes of the trainer here and compare bit for bit.
use crate::creature::{fmaxf, fminf, support, unit, Creature, DT};
use crate::ground::{Ground, WithSolids};
use crate::solid::{SolidBox, Solids};
use crate::mathx::{dcosf, dsinf};
use crate::model::{Model, NB};
use crate::vecmath::*;
use crate::vm::Brain;

const PI: f32 = 3.14159265358979;
/// Floats a tick in a trace: 7 for every body (position, rotation), then target x, target z, the ground under the target, targets reached, distance, the ground under the head, contacts, force.
pub const TRACE_PER_TICK: usize = NB * 7 + 8;

/// How far the target appears, how far off the heading (radians, either side), how fast it moves, whether the creature is shoved now and then, how many targets solve the level.
#[derive(Clone, Copy, Debug)]
pub struct Level {
    pub d: f32,
    pub dj: f32,
    pub bmin: f32,
    pub bmax: f32,
    pub speed: f32,
    pub push: bool,
    pub need: i32,
    pub far: bool,
    /// 0: open ground; 1: a slab (a ramp, or a wall when it is upright) whose face the targets lie on.
    pub env: i32,
    /// The slab's tilt from the ground, radians.
    pub tilt: f32,
    /// How far up the slab's face the targets are: from `along0`, up to `along_span` further.
    pub along0: f32,
    pub along_span: f32,
    /// Env 2 (a block of furniture: a crate, a table, a cabinet): its height, and the food lies on its top.
    pub block_h: f32,
    /// The block's height is `block_h` plus up to this much more, a different one each layout.
    pub block_span: f32,
    /// Env 2: the block's top front edge is cut at 45 degrees, a different size each layout, from `bevel_lo` up to `bevel_hi` (creature metres; under 0.02 it stays sharp).
    pub bevel_lo: f32,
    pub bevel_hi: f32,
}

pub fn level_of(l: i32) -> Level {
    let mut r = Level { d: 0.0, dj: 0.0, bmin: 0.0, bmax: 0.0, speed: 0.0, push: false, need: 2, far: l >= 4 && l <= 8, env: 0, tilt: 0.0, along0: 0.8, along_span: 0.5, block_h: 0.0, block_span: 0.0, bevel_lo: 0.0, bevel_hi: 0.0 };
    match l.clamp(0, 24) {
        0 => {
            r.d = 1.0;
            r.dj = 0.0;
            r.bmin = 0.0;
            r.bmax = 0.05;
            r.need = 1;
        }
        1 => {
            r.d = 1.4;
            r.dj = 0.1;
            r.bmin = 0.10;
            r.bmax = 0.40;
        }
        2 => {
            r.d = 1.7;
            r.dj = 0.2;
            r.bmin = 0.30;
            r.bmax = 0.80;
        }
        3 => {
            r.d = 2.0;
            r.dj = 0.3;
            r.bmin = 0.60;
            r.bmax = 1.40;
        }
        4 => {
            r.d = 2.4;
            r.dj = 0.3;
            r.bmin = 0.0;
            r.bmax = PI;
        }
        5 => {
            r.d = 2.2;
            r.dj = 0.3;
            r.bmin = 0.0;
            r.bmax = PI;
            r.speed = 0.10;
        }
        6 | 7 => {
            r.d = 2.2;
            r.dj = 0.3;
            r.bmin = 0.0;
            r.bmax = PI;
            r.speed = 0.20;
        }
        8 => {
            r.d = 2.2;
            r.dj = 0.3;
            r.bmin = 0.0;
            r.bmax = PI;
            r.speed = 0.20;
            r.push = true;
        }
        9 => {
            r.d = 6.5;
            r.dj = 1.5;
            r.bmin = 0.0;
            r.bmax = PI;
            r.need = 1;
        }
        // climbing: ramps of 15, 25, 30, 35, 40, 45, 55 and 70 degrees, then a wall; the target is on the face
        10..=17 => {
            r.env = 1;
            r.tilt = [0.262, 0.436, 0.524, 0.611, 0.70, 0.785, 0.96, 1.222][(l - 10) as usize];
            r.need = 1;
        }
        18 => {
            r.env = 1;
            r.tilt = 1.5707964;
            r.need = 1;
        }
        // furniture: a block 1 m deep and 1.4 m wide stands in front of the spider, 0.1 to 0.3, 0.2 to 0.8, 0.4 to 2, 1 to 4 and 2 to 6.5 m high, a different one each time (for a spider an eighth of the size it was trained at: a crate, a sofa, a stool, a chest, a table), and the food is on top of it
        20..=24 => {
            r.env = 2;
            r.need = 1;
            r.block_h = [0.1, 0.2, 0.4, 1.0, 2.0][(l - 20) as usize];
            r.block_span = [0.2, 0.6, 1.6, 3.0, 4.5][(l - 20) as usize];
            r.bevel_lo = [0.15, 0.10, 0.05, 0.0, 0.0][(l - 20) as usize];
            r.bevel_hi = [0.35, 0.30, 0.25, 0.20, 0.10][(l - 20) as usize];
        }
        // a tall wall: the target is anywhere from 0.8 to 2.6 m up it (a ladder of heights in one level: some of the spiders always get there, and the ones that climb higher are the ones that count)
        19 => {
            r.env = 1;
            r.tilt = 1.5707964;
            r.need = 1;
            r.along0 = 0.8;
            r.along_span = 1.8;
        }
        _ => {}
    }
    r
}

/// How high above the surface the prosoma stands when the spider is standing: where a target for the body is, above the surface it lies on.
pub const STANCE: f32 = 0.27;

/// The slab of a climbing level: where its face begins, which way it climbs, its normal and the way across.
struct Slab {
    o: V3,
    dir: V3,
    n: V3,
    b: V3,
}

/// Makes the slab of a climbing level (a box 3.2 m up the slope, 3 m across, 0.2 m thick) in front of the spider, a little turned and a little nearer or further each time; returns how it lies.
fn build_slab(lv: &Level, rng: &mut u64, solids: &mut Solids) -> Slab {
    let u1 = unit(rng);
    let u2 = unit(rng);
    let psi = (u1 - 0.5) * 0.6;
    let d0 = 1.0 + 0.3 * u2;
    let (cp, sp) = (dcosf(psi), dsinf(psi));
    let a = v3(cp, 0.0, -sp);
    let bb = v3(sp, 0.0, cp);
    let (ca, sa) = (dcosf(lv.tilt), dsinf(lv.tilt));
    let dir = v3(a.x * ca, sa, a.z * ca);
    let n = v3(-a.x * sa, ca, -a.z * sa);
    let o = v3(a.x * d0, 0.0, a.z * d0);
    let (len, wid, thick) = (1.6f32, 1.5f32, 0.1f32);
    let c = o + dir * len - n * thick;
    solids.b[0] = SolidBox { c, r: M3 { m: [dir.x, n.x, bb.x, dir.y, n.y, bb.y, dir.z, n.z, bb.z] }, half: v3(len, thick, wid) };
    solids.n = 1;
    Slab { o, dir, n, b: bb }
}

/// The block of a furniture level: the middle of its top face and the way across (b) and back (a).
struct Block {
    top: V3,
    a: V3,
    b: V3,
}

/// Makes the block of a furniture level (1 m deep, 1.4 m wide, as high as the level says) in front of the spider, a little turned and a little nearer or further each time.
fn build_block(lv: &Level, rng: &mut u64, solids: &mut Solids) -> Block {
    let u1 = unit(rng);
    let u2 = unit(rng);
    let u3 = unit(rng);
    let u4 = unit(rng);
    let psi = (u1 - 0.5) * 0.6;
    let d0 = 1.0 + 0.3 * u2;
    let (cp, sp) = (dcosf(psi), dsinf(psi));
    let a = v3(cp, 0.0, -sp);
    let bb = v3(sp, 0.0, cp);
    let (sx, sz) = (1.0f32, 1.4f32);
    let h = lv.block_h + lv.block_span * u3;
    let cb = fminf(lv.bevel_lo + (lv.bevel_hi - lv.bevel_lo) * u4, 0.5 * h);
    let c = v3(a.x * (d0 + sx * 0.5), h * 0.5, a.z * (d0 + sx * 0.5));
    if cb < 0.02 {
        solids.b[0] = SolidBox { c, r: M3 { m: [a.x, 0.0, bb.x, 0.0, 1.0, 0.0, a.z, 0.0, bb.z] }, half: v3(sx * 0.5, h * 0.5, sz * 0.5) };
        solids.n = 1;
    } else {
        // the top front edge is cut at 45 degrees: a lower box, an upper box set back by the cut, and a slab along the cut (its outer face is the cut)
        let ha = h - cb;
        let is2 = 0.70710678f32;
        let r0 = M3 { m: [a.x, 0.0, bb.x, 0.0, 1.0, 0.0, a.z, 0.0, bb.z] };
        solids.b[0] = SolidBox { c: v3(a.x * (d0 + sx * 0.5), ha * 0.5, a.z * (d0 + sx * 0.5)), r: r0, half: v3(sx * 0.5, ha * 0.5, sz * 0.5) };
        let ub = d0 + cb + (sx - cb) * 0.5;
        solids.b[1] = SolidBox { c: v3(a.x * ub, ha + cb * 0.5, a.z * ub), r: r0, half: v3((sx - cb) * 0.5, cb * 0.5, sz * 0.5) };
        let mu = d0 + cb * 0.5;
        let my = ha + cb * 0.5;
        let cu = mu + is2 * cb * 0.5;
        let cy = my - is2 * cb * 0.5;
        solids.b[2] = SolidBox { c: v3(a.x * cu, cy, a.z * cu), r: M3 { m: [a.x * is2, a.x * is2, bb.x, is2, -is2, 0.0, a.z * is2, a.z * is2, bb.z] }, half: v3(cb * is2, cb * 0.5, sz * 0.5) };
        solids.n = 3;
    }
    Block { top: v3(c.x, h, c.z), a, b: bb }
}

/// A place for the body on the block's top: up to a quarter metre along it and 0.3 m across, a stance above the surface.
fn block_target(k: &Block, rng: &mut u64) -> V3 {
    let u1 = unit(rng);
    let u2 = unit(rng);
    let along = (u1 * 2.0 - 1.0) * 0.25;
    let across = (u2 * 2.0 - 1.0) * 0.3;
    k.top + k.a * along + k.b * across + v3(0.0, STANCE, 0.0)
}

/// A place for the body on the slab's face: 0.8 to 1.3 m up the slope, up to half a metre to either side, a stance above the surface.
fn slab_target(s: &Slab, lv: &Level, rng: &mut u64) -> V3 {
    let u1 = unit(rng);
    let u2 = unit(rng);
    let along = lv.along0 + lv.along_span * u1;
    let across = (u2 * 2.0 - 1.0) * 0.5;
    s.o + s.dir * along + s.b * across + s.n * STANCE
}

/// What an episode is asked: where the targets appear (`layout_seed`), the creature's own body (`body_seed`) and the level.
#[derive(Clone, Copy, Debug)]
pub struct Cfg {
    pub layout_seed: u64,
    pub body_seed: u64,
    pub level: i32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EpisodeOut {
    pub score: f32,
    pub reached: f32,
    pub last_dist: f32,
    pub stand: f32,
    pub fall: f32,
    pub first_tick: f32,
    pub broken: f32,
    pub air: f32,
    pub cv: f32,
    pub vmean: f32,
    pub bob: f32,
}

/// Runs an episode of `ticks` ticks; returns the score and, if `trace` is given, the trace (`ticks` x `TRACE_PER_TICK` floats).
pub fn run_episode(model: &Model, brain: Brain, cfg: &Cfg, ground: &dyn Ground, ticks: usize, rounds: i32, trace: Option<&mut Vec<f32>>) -> EpisodeOut {
    run_episode_with(model, brain, cfg, ground, ticks, rounds, trace, None)
}

/// `run_episode`, calling `hook` with the creature and the tick before each tick's thinking (to look inside).
pub fn run_episode_with(model: &Model, brain: Brain, cfg: &Cfg, ground: &dyn Ground, ticks: usize, rounds: i32, mut trace: Option<&mut Vec<f32>>, mut hook: Option<&mut dyn FnMut(&Creature, usize)>) -> EpisodeOut {
    let mut c = Creature::new(model, brain, cfg.body_seed, ground);
    c.rounds = rounds;
    let mut push_rng = cfg.body_seed ^ 0xA5A5_A5A5_DEAD_BEEF;
    let mut layout_rng = cfg.layout_seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(12345);
    let lv = level_of(cfg.level);
    // a climbing level has a slab in front of the spider, and the ground and the slab are the world it senses and touches
    let mut solids = Solids::default();
    let slab = if lv.env == 1 { Some(build_slab(&lv, &mut layout_rng, &mut solids)) } else { None };
    let block = if lv.env == 2 { Some(build_block(&lv, &mut layout_rng, &mut solids)) } else { None };
    let wrapped = WithSolids { base: ground, solids };
    let ground: &dyn Ground = &wrapped;
    let (mut tx, mut ty, mut tz, mut tvx, mut tvz, mut d_spawn, mut t_spawn) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 1.0f32, 0.0f32);
    let (mut reached, mut first_tick) = (0i32, -1i32);
    let mut speed_bonus = 0.0f32;
    let (mut stand, mut fall, mut last_dist) = (0.0f32, 0.0f32, 1.0f32);
    let mut broken = false;
    let mut air = 0.0f32;
    let (mut sm_speed, mut sum_sm, mut sum_sm2, mut sum_vy2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut n_sm = 0i32;
    // the next target: open ground (a place at a distance and a bearing from the head and the heading, perhaps moving) or a place on the slab
    let mut spawn_target = |head: V3, fh: V3, tsec: f32, tx: &mut f32, ty: &mut f32, tz: &mut f32, tvx: &mut f32, tvz: &mut f32, d_spawn: &mut f32, t_spawn: &mut f32, rng: &mut u64| {
        if let Some(k) = &block {
            let p = block_target(k, rng);
            *tx = p.x;
            *ty = p.y;
            *tz = p.z;
            *tvx = 0.0;
            *tvz = 0.0;
            let dd = p - head;
            *d_spawn = dot(dd, dd).sqrt();
            *t_spawn = tsec;
            return;
        }
        if let Some(s) = &slab {
            let p = slab_target(s, &lv, rng);
            *tx = p.x;
            *ty = p.y;
            *tz = p.z;
            *tvx = 0.0;
            *tvz = 0.0;
            let dd = p - head;
            *d_spawn = dot(dd, dd).sqrt();
            *t_spawn = tsec;
            return;
        }
        let u1 = unit(rng);
        let u2 = unit(rng);
        let u3 = unit(rng);
        let u4 = unit(rng);
        let u5 = unit(rng);
        let u6 = unit(rng);
        let mut d = lv.d + lv.dj * (2.0 * u1 - 1.0);
        if lv.far && u5 < 0.25 {
            d = 4.0 + 2.0 * u6;
        }
        let b = (lv.bmin + (lv.bmax - lv.bmin) * u2) * (if u3 < 0.5 { 1.0 } else { -1.0 });
        let left = v3(fh.z, 0.0, -fh.x);
        *tx = head.x + d * (dcosf(b) * fh.x + dsinf(b) * left.x);
        *tz = head.z + d * (dcosf(b) * fh.z + dsinf(b) * left.z);
        *ty = ground.height(*tx, *tz) + STANCE;
        let hd = 2.0 * PI * u4;
        *tvx = lv.speed * dcosf(hd);
        *tvz = lv.speed * dsinf(hd);
        *d_spawn = d;
        *t_spawn = tsec;
    };
    spawn_target(c.pose(0).0, v3(1.0, 0.0, 0.0), 0.0, &mut tx, &mut ty, &mut tz, &mut tvx, &mut tvz, &mut d_spawn, &mut t_spawn, &mut layout_rng);
    for tick in 0..=ticks {
        let t = tick as f32 * DT;
        let (x0, q0) = c.pose(0);
        let gh_head = ground.height(x0.x, x0.z);
        let (clearance, sn) = support(ground, x0);
        let r0 = from_quat(&q0);
        {
            let upv = mulv(&r0, v3(0.0, 1.0, 0.0));
            let standing = fminf(fmaxf((clearance - 0.06) / 0.12, 0.0), 1.0) * (if dot(upv, sn) > 0.5 { 1.0 } else { 0.0 });
            stand += standing;
            if dot(upv, sn) < 0.3 {
                fall += 1.0;
            }
            let nfeet = c.foot.iter().filter(|f| **f).count();
            // a spider hanging on its thread is held up (what is not walking is a hop)
            let held = c.anchored && c.silk_tension > 10.0;
            if nfeet <= 2 && !held {
                air += 1.0;
            }
            let vh = c.velocity(0);
            {
                // along the surface it stands on (on the floor: horizontal), and across it (on the floor: vertical, the bobbing)
                let vnorm = dot(vh, sn);
                let vt2 = dot(vh, vh) - vnorm * vnorm;
                sm_speed += 0.15 * ((if vt2 > 0.0 { vt2 } else { 0.0 }).sqrt() - sm_speed);
                sum_sm += sm_speed;
                sum_sm2 += sm_speed * sm_speed;
                sum_vy2 += vnorm * vnorm;
                n_sm += 1;
            }
            if !(clearance == clearance) || (clearance > 0.7 && !c.anchored) || dot(vh, vh) > 36.0 {
                broken = true;
            }
        }
        {
            let mut dx = tx - x0.x;
            let mut dy = ty - x0.y;
            let mut dz = tz - x0.z;
            last_dist = (dx * dx + dy * dy + dz * dz).sqrt();
            if last_dist < 0.30 && !broken {
                reached += 1;
                if first_tick < 0 {
                    first_tick = tick as i32;
                }
                speed_bonus += 1.5 * fminf(fmaxf((d_spawn / fmaxf(t - t_spawn, 0.25)) / 1.2, 0.0), 1.0);
                let fwdw = mulv(&r0, v3(1.0, 0.0, 0.0));
                let mut nf = (fwdw.x * fwdw.x + fwdw.z * fwdw.z).sqrt();
                if nf < 1e-6 {
                    nf = 1e-6;
                }
                spawn_target(x0, v3(fwdw.x / nf, 0.0, fwdw.z / nf), t, &mut tx, &mut ty, &mut tz, &mut tvx, &mut tvz, &mut d_spawn, &mut t_spawn, &mut layout_rng);
                dx = tx - x0.x;
                dy = ty - x0.y;
                dz = tz - x0.z;
                last_dist = (dx * dx + dy * dy + dz * dz).sqrt();
            }
        }
        if let Some(tr) = trace.as_deref_mut() {
            if tick < ticks {
                for i in 0..NB {
                    let (p, q) = c.pose(i);
                    tr.extend_from_slice(&[p.x, p.y, p.z, q[0], q[1], q[2], q[3]]);
                }
                tr.extend_from_slice(&[tx, tz, ty, reached as f32, last_dist, gh_head, c.last_contacts as f32, c.last_force]);
            }
        }
        if tick == ticks || broken {
            break;
        }
        tx += tvx * DT;
        tz += tvz * DT;
        if lv.speed > 0.0 {
            ty = ground.height(tx, tz) + STANCE;
        }
        if let Some(h) = hook.as_deref_mut() {
            h(&c, tick);
        }
        c.think([tx, ty, tz], ground);
        if lv.push && tick > 60 && unit(&mut push_rng) < 1.0 / 150.0 {
            let a = 2.0 * PI * unit(&mut push_rng);
            let m = 0.25 + 0.35 * unit(&mut push_rng);
            c.shove(v3(m * dcosf(a), 0.0, m * dsinf(a)));
        }
        c.physics(ground);
    }
    let prog = fminf(fmaxf(1.0 - last_dist / d_spawn, 0.0), 1.0);
    let nt = if ticks > 0 { ticks as f32 } else { 1.0 };
    let nsm = if n_sm > 0 { n_sm as f32 } else { 1.0 };
    let vmean = sum_sm / nsm;
    let var = sum_sm2 / nsm - vmean * vmean;
    let cv = (if var > 0.0 { var } else { 0.0 }).sqrt() / (vmean + 0.05);
    let rms_vy = (sum_vy2 / nsm).sqrt();
    let smooth = fminf(fmaxf(1.25 - 0.9 * cv, 0.35), 1.0);
    let bob_pen = 0.8 * fmaxf(rms_vy - 0.10, 0.0);
    let supp = fminf(fmaxf(1.3 - 2.0 * air / nt, 0.2), 1.0);
    let mut score = ((reached as f32) + speed_bonus + 0.8 * prog) * smooth * supp + 0.25 * stand / nt - 0.15 * fall / nt - 0.6 * air / nt - bob_pen;
    if broken {
        score = 0.0;
    }
    EpisodeOut { score, reached: reached as f32, last_dist, stand: stand / nt, fall: fall / nt, first_tick: first_tick as f32, broken: if broken { 1.0 } else { 0.0 }, air: air / nt, cv, vmean, bob: rms_vy }
}
