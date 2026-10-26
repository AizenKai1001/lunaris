//! A creature alive: its body (rigid-body physics: maximal coordinates, sequential impulses, warm started, small steps with relax rounds) and its brain (three programs of the Evolve-0 machine). A
//! line-for-line port of `simulate_spider` of `spidergpu/kernels/creature_core.h`, split at the places where the world comes in (`think`: the senses and the brain, `physics`: the body), so that a game can
//! drive it: give it a target (a place on the ground) and the ground, and it walks to it. Everything is f32 arithmetic in a fixed order and the deterministic math of `mathx`, so it moves exactly as it did
//! in the trainer (the GPU and its CPU twin agree with this crate to the last bit; `tests/parity.rs` shows it on golden files).
//!
//! The senses (tokens, readable by the programs with InGet; `NTOK` of them): 0..3 clocks (sine and cosine of 0.8 s and of 1.6 s periods), 4 bearing of the target (milliradians, positive to the left), 5 its
//! distance (mm), 6 how far the nose points up (thousandths), 7 how far the right side points up, 8 the body's up axis against the world's up, 9 height above the ground (mm), 10 forward speed (mm/s), 11
//! sideways speed (right positive), 12 turning rate (mrad/s, positive to the left), 13 vertical speed, 14 the ground's slope along the heading (uphill positive), 15 its slope to the right, 16 the tick;
//! 17..24 whether each foot touches the ground; 25..64 the 40 joint angles (mrad: for each of the five joints of a leg kind, the eight legs), 65..104 their speeds, 105..110 the thread (held, its length, its tension, where its anchor is as the body sees it: ahead, left, up), 111..114 four feelers from the head (how far the nearest surface is, in thousandths of 1.5 creature-metres, 1000 if there is none: ahead, up, ahead-and-up, ahead-and-down; they let a brain feel the edge of a block it is climbing). The BODY program gets tokens 0..24
//! in r32..r56 and leaves the hydraulic pressure in r24 and two signals in r25, r26. A LEG program, run for each of the eight legs every tick, gets in r32..r63: its number, its side (+1000 right,
//! -1000 left), its place along the body, the sine and cosine of its own gait phase, its five joint angles and speeds, its foot contact, the body tokens 4..15, the target's bearing as its side and as its place
//! see it, and the two signals; its five muscle torques (thousandths of the strongest) are r0..r4.
use crate::ground::Ground;
use crate::mathx::{datan2f, dcosf, dsinf};
use crate::model::{Model, LEGB, LEGJ, NB, NJ, N_LEGS};
use crate::vecmath::*;
use crate::nn::{nn_step, nn_tanh, NNB_H, NNB_IN, NNB_IN_V1, NNB_OUT, NN_H, NN_IN, NN_IN_V1, NN_LEG_P, NN_LEG_P_V1, NN_OUT, NNP_V1};
use crate::vm::{vm_run, Brain, Ins, MEMN, NREG, NTOK};

/// Contacts at most.
pub const MAXC: usize = 64;
pub const DT: f32 = 1.0 / 60.0;
const G: f32 = 9.8;
const WMAX: f32 = 14.0;
const PI: f32 = 3.14159265358979;
const BUDGET_BODY: i32 = 60;
const BUDGET_LEG: i32 = 96;
/// The adhesion of a foot, newtons for each corner of the tarsus that touches (a foot with four corners down holds 16 N at full grip).
const ADH: f32 = 8.0;
/// Silk: how far a thread can be shot (m), the shortest it can be reeled to, how fast it is reeled (m/s) and the most it can pull (N).
const SILK_RANGE: f32 = 4.0;
/// How far the head's four feelers reach (creature metres).
const RAY_RANGE: f32 = 1.5;
const SILK_MIN: f32 = 0.2;
const SILK_REEL: f32 = 0.8;
const SILK_TMAX: f32 = 150.0;
/// The thread's stiffness (N/m) and damping (N s/m).
const SILK_K: f32 = 2000.0;
const SILK_C: f32 = 100.0;

pub fn fminf(a: f32, b: f32) -> f32 {
    a.min(b)
}
pub fn fmaxf(a: f32, b: f32) -> f32 {
    a.max(b)
}

/// The cosines of the leg azimuths 40, 80, 118, 150 degrees (and their mirror images).
fn leg_cos(k: usize) -> f32 {
    const C: [f32; 4] = [0.766_044_43, 0.173_648_18, -0.469_471_56, -0.866_025_4];
    C[k & 3]
}

pub fn splitmix(s: &mut u64) -> u64 {
    *s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *s;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A number in [0, 1).
pub fn unit(s: &mut u64) -> f32 {
    (splitmix(s) >> 40) as f32 / 16_777_216.0
}

/// A creature: its state, its brain and everything its solver remembers from one step to the next.
#[derive(Clone)]
pub struct Creature {
    pub model: Model,
    brain: Brain,
    // the body's state
    x: [V3; NB],
    vl: [V3; NB],
    wl: [V3; NB],
    q: [[f32; 4]; NB],
    r: [M3; NB],
    iw: [M3; NB],
    // this creature's own body: inverse masses and inertias, strength, grip, joint stiffness
    im: [f32; NB],
    invi: [[f32; 3]; NB],
    ks: f32,
    kg: f32,
    kk: f32,
    // the brain's state
    mem: [f64; MEMN],
    rng: u64,
    sig: [f32; 2],
    tau: [f32; NB],
    /// The hydraulic pressure of the legs, 0 to 1 (it cannot change faster than `SLEW` a tick).
    pub pressure: f32,
    /// Whether each foot touched the ground in the last tick.
    pub foot: [bool; N_LEGS],
    // what the solver found the step before (warm start)
    jl: [V3; NB],
    jaw: [V3; NB],
    jacc_lo: [f32; NB],
    jacc_hi: [f32; NB],
    cimp: [[[f32; 3]; 16]; NB],
    /// How hard each foot grips (0 to 1): the adhesive pad of the tarsus holds on to what it touches. Set by the brain (r5 of a leg run).
    pub grip: [f32; N_LEGS],
    // the hidden states of a neural brain
    nn_hl: [[f32; NN_H]; N_LEGS],
    nn_hb: [f32; NNB_H],
    /// Silk: whether a thread is held, where it is anchored, how long it is (m) and how hard it pulls (N). The body program holds it with r28 > 500 and reels it with r29 (thousandths, positive in).
    pub anchored: bool,
    pub anchor: V3,
    pub silk_len: f32,
    pub silk_tension: f32,
    // per-step work arrays
    jrp: [V3; NB],
    jrc: [V3; NB],
    jbp: [V3; NB],
    ju: [V3; NB],
    jt1: [V3; NB],
    jt2: [V3; NB],
    jk: [[f32; 9]; NB],
    jkang: [[f32; 4]; NB],
    jbang: [[f32; 2]; NB],
    jam: [f32; NB],
    jth: [f32; NB],
    /// Contacts and force of the last step, for tests.
    pub last_contacts: usize,
    pub last_force: f32,
    /// The rounds of the solver in each small step.
    pub rounds: i32,
    tick: u32,
}

impl Creature {
    /// A creature standing in its rest pose on the ground below the origin of the world, with the body of the seed `body_seed` (the seed decides the strength, weight, grip and joint stiffness of this
    /// creature, and its machine's random numbers).
    pub fn new(model: &Model, brain: Brain, body_seed: u64, ground: &dyn Ground) -> Creature {
        let h_start = ground.height(0.0, 0.0);
        let mut x = [V3::default(); NB];
        let mut q = [[0.0f32; 4]; NB];
        for i in 0..NB {
            x[i] = ld3(&model.rest_pos[i]);
            x[i].y += h_start;
            q[i] = model.rest_q[i];
        }
        let mut im = [0.0f32; NB];
        let mut invi = [[0.0f32; 3]; NB];
        let (mut ks, mut kg, mut kk) = (1.0f32, 1.0f32, 1.0f32);
        {
            let mut z = body_seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0xD1B5_4A32_D192_ED03);
            let mut u = [0.0f32; 4];
            for k in 0..4 {
                u[k] = unit(&mut z) * 2.0 - 1.0;
            }
            let v = model.indiv;
            ks = 1.0 + 0.15 * v * u[0];
            let km = 1.0 + 0.10 * v * u[1];
            kg = 1.0 + 0.15 * v * u[2];
            kk = 1.0 + 0.20 * v * u[3];
            for i in 0..NB {
                im[i] = model.inv_mass[i] / km;
                for a in 0..3 {
                    invi[i][a] = model.inv_i[i][a] / km;
                }
            }
        }
        let mut c = Creature {
            model: model.clone(),
            brain,
            x,
            vl: [V3::default(); NB],
            wl: [V3::default(); NB],
            q,
            r: [M3::default(); NB],
            iw: [M3::default(); NB],
            im,
            invi,
            ks,
            kg,
            kk,
            mem: [0.0; MEMN],
            rng: body_seed | 1,
            sig: [0.0; 2],
            tau: [0.0; NB],
            pressure: 0.6,
            foot: [true; N_LEGS],
            jl: [V3::default(); NB],
            jaw: [V3::default(); NB],
            jacc_lo: [0.0; NB],
            jacc_hi: [0.0; NB],
            cimp: [[[0.0; 3]; 16]; NB],
            grip: [0.0; N_LEGS],
            nn_hl: [[0.0; NN_H]; N_LEGS],
            nn_hb: [0.0; NNB_H],
            anchored: false,
            anchor: V3::default(),
            silk_len: 0.0,
            silk_tension: 0.0,
            jrp: [V3::default(); NB],
            jrc: [V3::default(); NB],
            jbp: [V3::default(); NB],
            ju: [V3::default(); NB],
            jt1: [V3::default(); NB],
            jt2: [V3::default(); NB],
            jk: [[0.0; 9]; NB],
            jkang: [[0.0; 4]; NB],
            jbang: [[0.0; 2]; NB],
            jam: [0.0; NB],
            jth: [0.0; NB],
            last_contacts: 0,
            last_force: 0.0,
            rounds: 3,
            tick: 0,
        };
        c.update_frames();
        c
    }

    /// Puts the creature (just made, standing in its rest pose at the origin facing +x) at (x, z) on the ground, turned `yaw` radians about the vertical (0 faces +x; positive turns towards -z, as
    /// the right-handed rotation about +y does). Call it before the first step.
    pub fn place(&mut self, x: f32, z: f32, yaw: f32, ground: &dyn Ground) {
        let (s, c) = (dsinf(yaw), dcosf(yaw));
        let y0 = ground.height(x, z) - ground.height(0.0, 0.0);
        let qy = [0.0, dsinf(yaw * 0.5), 0.0, dcosf(yaw * 0.5)];
        for i in 0..NB {
            let p = self.x[i];
            // a rotation about +y by yaw: x' = x cos + z sin, z' = -x sin + z cos
            self.x[i] = v3(p.x * c + p.z * s + x, p.y + y0, -p.x * s + p.z * c + z);
            self.q[i] = qmul(&qy, &self.q[i]);
        }
        self.update_frames();
    }

    /// Feeds every number of the creature's state to `f`, in a fixed order (for the engine's state hash).
    pub fn fold_state(&self, f: &mut dyn FnMut(u64)) {
        for i in 0..NB {
            for v in [self.x[i].x, self.x[i].y, self.x[i].z, self.vl[i].x, self.vl[i].y, self.vl[i].z, self.wl[i].x, self.wl[i].y, self.wl[i].z, self.q[i][0], self.q[i][1], self.q[i][2], self.q[i][3]] {
                f(v.to_bits() as u64);
            }
        }
        for v in self.mem.iter() {
            f(v.to_bits());
        }
        f(self.rng);
        f(self.pressure.to_bits() as u64);
        f(self.tick as u64);
        for k in 0..N_LEGS {
            f(self.grip[k].to_bits() as u64);
        }
        for l in 0..N_LEGS {
            for v in self.nn_hl[l].iter() {
                f(v.to_bits() as u64);
            }
        }
        for v in self.nn_hb.iter() {
            f(v.to_bits() as u64);
        }
        f(self.anchored as u64);
        for v in [self.anchor.x, self.anchor.y, self.anchor.z, self.silk_len] {
            f(v.to_bits() as u64);
        }
        for k in 0..N_LEGS {
            f(self.foot[k] as u64);
        }
    }

    fn update_frames(&mut self) {
        for i in 0..NB {
            self.r[i] = from_quat(&self.q[i]);
            self.iw[i] = world_inv_i(&self.r[i], &self.invi[i]);
        }
    }

    /// The two ends of the thread when one is held: where it leaves the body and where it is anchored.
    pub fn thread_ends(&self) -> Option<(V3, V3)> {
        if self.anchored {
            Some((self.spinneret(), self.anchor))
        } else {
            None
        }
    }

    /// Where the thread leaves the body: the spinnerets, at the rear tip of the abdomen.
    fn spinneret(&self) -> V3 {
        let hs = ld3(&self.model.half[1]);
        let r = mulv(&self.r[1], v3(-hs.x, 0.0, 0.0));
        v3(self.x[1].x + r.x, self.x[1].y + r.y, self.x[1].z + r.z)
    }

    /// The thread: while it is held it is anchored where a line from the spinnerets towards the target first meets a surface within range, then it is reeled in or paid out; when it is not held it is cut.
    fn silk_update(&mut self, hold: bool, reel: f32, aim: V3, ground: &dyn Ground) {
        if !hold {
            self.anchored = false;
            self.silk_tension = 0.0;
            return;
        }
        let s = self.spinneret();
        if !self.anchored {
            let d = dot(aim, aim).sqrt();
            if d > 1.0e-4 {
                let dir = aim * (1.0 / d);
                if let Some(p) = cast(ground, s, dir, SILK_RANGE) {
                    let dd = p - s;
                    self.anchor = p;
                    self.anchored = true;
                    self.silk_len = fmaxf(dot(dd, dd).sqrt(), SILK_MIN);
                }
            }
        } else {
            self.silk_len = fminf(fmaxf(self.silk_len - reel * SILK_REEL * DT, SILK_MIN), SILK_RANGE + 1.0);
        }
    }

    /// Ticks lived so far.
    pub fn ticks(&self) -> u32 {
        self.tick
    }
    /// The position and rotation (x, y, z, w) of body `i` (0 the prosoma, 1 the abdomen, then the segments of the legs).
    pub fn pose(&self, i: usize) -> (V3, [f32; 4]) {
        (self.x[i], self.q[i])
    }
    /// How far apart the two anchors of the worst joint are right now (metres): the solver's accuracy (a joint that holds has less than a millimetre).
    pub fn joint_error(&self) -> f32 {
        let mut worst = 0.0f32;
        for b in 1..NB {
            let p = self.model.parent[b] as usize;
            let a = self.x[p] + mulv(&self.r[p], ld3(&self.model.rp[b]));
            let c = self.x[b] + mulv(&self.r[b], ld3(&self.model.rc[b]));
            let d = a - c;
            let e = dot(d, d).sqrt();
            if e > worst {
                worst = e;
            }
        }
        worst
    }

    /// The brain's command to the joint of body `i` in the last tick, -1 to 1 of the strongest torque (a leg's swing, lift and knees are bodies 2 + 6 * leg + 0..4).
    pub fn command(&self, i: usize) -> f32 {
        self.tau[i]
    }

    pub fn velocity(&self, i: usize) -> V3 {
        self.vl[i]
    }
    /// A sudden change of speed of the whole body (a blow, a gust).
    pub fn shove(&mut self, dv: V3) {
        for i in 0..NB {
            self.vl[i] = self.vl[i] + dv;
        }
    }
    /// The way the prosoma faces (its x axis in the world) and its up axis.
    pub fn facing(&self) -> V3 {
        mulv(&self.r[0], v3(1.0, 0.0, 0.0))
    }
    pub fn up(&self) -> V3 {
        mulv(&self.r[0], v3(0.0, 1.0, 0.0))
    }

    /// The senses and the brain, for one tick: reads the world (the `target` as a place on the ground, the ground itself), runs the programs, and leaves the torque commands and the hydraulic pressure for the
    /// physics of this tick.
    pub fn think(&mut self, target: [f32; 3], ground: &dyn Ground) {
        let tick = self.tick;
        let t = tick as f32 * DT;
        self.update_frames();
        let head = self.x[0];
        let mut tok = [0.0f64; NTOK];
        let tau_ = 2.0 * PI;
        let fwdw = mulv(&self.r[0], v3(1.0, 0.0, 0.0));
        let upw = mulv(&self.r[0], v3(0.0, 1.0, 0.0));
        let sidew = mulv(&self.r[0], v3(0.0, 0.0, 1.0)); // to the right
        // the target as the body sees it (so that it means the same on a wall as on the floor): ahead, to the left and up
        let tv = v3(target[0] - head.x, target[1] - head.y, target[2] - head.z);
        let tf = dot(tv, fwdw);
        let tl = -dot(tv, sidew);
        let tu = dot(tv, upw);
        let bearing = datan2f(tl, tf);
        let dist = dot(tv, tv).sqrt();
        let elevation = datan2f(tu, (tf * tf + tl * tl).sqrt());
        let v_fwd = dot(self.vl[0], fwdw);
        let v_side = dot(self.vl[0], sidew);
        let yaw = dot(self.wl[0], upw);
        let (clearance, support_n) = support(ground, head);
        let mut body_in = [0.0f32; 16];
        body_in[0] = dsinf(tau_ * t / 0.8);
        body_in[1] = dcosf(tau_ * t / 0.8);
        body_in[2] = dsinf(tau_ * t / 1.6);
        body_in[3] = dcosf(tau_ * t / 1.6);
        body_in[4] = bearing;
        body_in[5] = dist;
        body_in[6] = fwdw.y;
        body_in[7] = sidew.y;
        body_in[8] = upw.y;
        body_in[9] = clearance;
        body_in[10] = v_fwd;
        body_in[11] = v_side;
        body_in[12] = yaw;
        body_in[13] = self.vl[0].y;
        body_in[14] = elevation;
        body_in[15] = dot(support_n, fwdw);
        let mut w = 0usize;
        for v in body_in.iter() {
            tok[w] = ((*v * 1000.0f32) as i64) as f64;
            w += 1;
        }
        tok[w] = tick as f64;
        w += 1;
        for leg in 0..N_LEGS {
            tok[w] = if self.foot[leg] { 1000.0 } else { 0.0 };
            w += 1;
        }
        // the joint angles and speeds: for each kind of joint (swing, lift, femur-patella, patella-tibia, tibia-metatarsus), the eight legs
        let mut ang = [0.0f32; NJ];
        let mut rate = [0.0f32; NJ];
        for jn in 0..NJ {
            let child = 2 + LEGB * (jn % N_LEGS) + jn / N_LEGS;
            let par = self.model.parent[child] as usize;
            let axp = ld3(&self.model.axp[child]);
            ang[jn] = hinge_angle(&self.q[par], &self.q[child], &self.model.qrel[child], axp);
            let axw = mulv(&self.r[par], axp);
            rate[jn] = dot(self.wl[child] - self.wl[par], axw);
        }
        for jn in 0..NJ {
            tok[w] = ((ang[jn] * 1000.0f32) as i64) as f64;
            w += 1;
        }
        for jn in 0..NJ {
            tok[w] = ((rate[jn] * 1000.0f32) as i64) as f64;
            w += 1;
        }
        // the thread: whether it is held, how long, how hard it pulls, and where its anchor is as the body sees it (ahead, to the left, up)
        {
            let sp = self.spinneret();
            let d = v3(self.anchor.x - sp.x, self.anchor.y - sp.y, self.anchor.z - sp.z);
            let dl = dot(d, d).sqrt();
            let (af, al, au) = if self.anchored && dl > 1e-6 { (dot(d, fwdw) / dl, -dot(d, sidew) / dl, dot(d, upw) / dl) } else { (0.0, 0.0, 0.0) };
            let silk_in = [if self.anchored { 1.0f32 } else { 0.0 }, if self.anchored { self.silk_len } else { 0.0 }, self.silk_tension * 0.01, af, al, au];
            for v in silk_in.iter() {
                tok[w] = ((*v * 1000.0f32) as i64) as f64;
                w += 1;
            }
        }
        // four feelers from the head, along the body's ahead, up, ahead-and-up and ahead-and-down: how far the nearest surface is (all of RAY_RANGE: nothing), to feel an edge in front or a ceiling above
        {
            let hd = self.x[0];
            let isq = 0.70710678f32;
            let fdir = [fwdw, upw, (fwdw + upw) * isq, (fwdw - upw) * isq];
            for k in 0..4 {
                let mut fd = RAY_RANGE;
                if let Some(hit) = cast(ground, hd, fdir[k], RAY_RANGE) {
                    let dd = hit - hd;
                    fd = fminf(dot(dd, dd).sqrt(), RAY_RANGE);
                }
                tok[w] = ((fd / RAY_RANGE * 1000.0f32) as i64) as f64;
                w += 1;
            }
        }
        debug_assert_eq!(w, NTOK);
        // the body program: pressure and signals
        let mut reg = [0.0f64; NREG];
        for i in 0..25 {
            reg[32 + i] = tok[i];
        }
        if let Some(nnw) = self.brain.nn.as_deref() {
            // the neural brain: the body net reads the body's senses and the thread's and writes what the body program would: the pressure (r24), two signals (r25, r26), the thread's aim (r27, r30), hold (r28) and reel (r29)
            let mut xb = [0.0f32; NNB_IN];
            for i in 0..4 {
                xb[i] = (tok[i] / 1000.0) as f32;
            }
            xb[4] = (tok[4] / 1000.0) as f32;
            xb[5] = (tok[5] / 3000.0) as f32;
            xb[6] = (tok[6] / 1000.0) as f32;
            xb[7] = (tok[7] / 1000.0) as f32;
            xb[8] = (tok[8] / 1000.0) as f32;
            xb[9] = (tok[9] / 300.0) as f32;
            xb[10] = (tok[10] / 1000.0) as f32;
            xb[11] = (tok[11] / 1000.0) as f32;
            xb[12] = (tok[12] / 2000.0) as f32;
            xb[13] = (tok[13] / 1000.0) as f32;
            xb[14] = (tok[14] / 1000.0) as f32;
            xb[15] = (tok[15] / 1000.0) as f32;
            for i in 0..8 {
                xb[16 + i] = (tok[17 + i] / 1000.0) as f32;
            }
            xb[24] = (tok[105] / 1000.0) as f32;
            xb[25] = (tok[106] / 4000.0) as f32;
            xb[26] = (tok[107] / 1000.0) as f32;
            xb[27] = (tok[108] / 1000.0) as f32;
            xb[28] = (tok[109] / 1000.0) as f32;
            xb[29] = (tok[110] / 1000.0) as f32;
            xb[30] = (tok[111] / 1000.0) as f32;
            xb[31] = (tok[112] / 1000.0) as f32;
            xb[32] = (tok[113] / 1000.0) as f32;
            xb[33] = (tok[114] / 1000.0) as f32;
            let mut ob = [0.0f32; NNB_OUT];
            let mut hn = [0.0f32; NNB_H];
            if nnw.len() == NNP_V1 {
                // a net from before the feelers: it reads the senses it was trained on
                let xb1: [f32; NNB_IN_V1] = std::array::from_fn(|i| xb[i]);
                nn_step::<NNB_H, NNB_IN_V1, NNB_OUT>(&nnw[NN_LEG_P_V1..], &xb1, &self.nn_hb, &mut hn, &mut ob);
            } else {
                nn_step::<NNB_H, NNB_IN, NNB_OUT>(&nnw[NN_LEG_P..], &xb, &self.nn_hb, &mut hn, &mut ob);
            }
            self.nn_hb = hn;
            reg[24] = ((0.5f32 * (1.0f32 + nn_tanh(ob[0]))) * 1000.0f32) as f64;
            reg[28] = if ob[1] > 0.0 { 1000.0 } else { 0.0 };
            reg[29] = (nn_tanh(ob[2]) * 1000.0f32) as f64;
            reg[27] = (nn_tanh(ob[3]) * 3141.5927f32) as f64;
            reg[30] = (nn_tanh(ob[4]) * 1570.7964f32) as f64;
            reg[25] = (nn_tanh(ob[5]) * 1000.0f32) as f64;
            reg[26] = (nn_tanh(ob[6]) * 1000.0f32) as f64;
        } else {
            if tick == 0 {
                vm_run(&self.brain.setup, &mut reg, &mut self.mem, &tok, &mut self.rng, BUDGET_BODY);
                reg = [0.0; NREG];
                for i in 0..25 {
                    reg[32 + i] = tok[i];
                }
            }
            vm_run(&self.brain.body, &mut reg, &mut self.mem, &tok, &mut self.rng, BUDGET_BODY);
        }
        {
            let pv = reg[24];
            let want = fminf(fmaxf(if pv == pv && pv - pv == 0.0 { (pv / 1000.0) as f32 } else { 0.0 }, 0.0), 1.0);
            let slew = 0.5 * DT;
            self.pressure += fminf(fmaxf(want - self.pressure, -slew), slew);
            for i in 0..2 {
                self.sig[i] = reg[25 + i] as f32;
            }
            // the thread: hold it (r28 above 500) or cut it, and reel it in (r29 positive) or pay it out
            let hv = reg[28];
            let hold = hv == hv && hv - hv == 0.0 && hv > 500.0;
            let rv = reg[29];
            let reel = fminf(fmaxf(if rv == rv && rv - rv == 0.0 { (rv / 1000.0) as f32 } else { 0.0 }, -1.0), 1.0);
            // where it is shot is the brain's to choose: the direction as the prosoma sees it, an azimuth to the left (r27, milliradians) and an elevation up (r30)
            let azv = reg[27];
            let elv = reg[30];
            let az = fminf(fmaxf(if azv == azv && azv - azv == 0.0 { (azv / 1000.0) as f32 } else { 0.0 }, -PI), PI);
            let el = fminf(fmaxf(if elv == elv && elv - elv == 0.0 { (elv / 1000.0) as f32 } else { 0.0 }, -1.5707964), 1.5707964);
            let leftb = v3(-sidew.x, -sidew.y, -sidew.z);
            let (ce, se) = (dcosf(el), dsinf(el));
            let (ca, sa) = (dcosf(az), dsinf(az));
            let aim = fwdw * (ce * ca) + upw * se + leftb * (ce * sa);
            self.silk_update(hold, reel, aim, ground);
        }
        // the legs: one run each, with its own senses
        for leg in 0..N_LEGS {
            reg = [0.0; NREG];
            let side: f32 = if leg < 4 { 1000.0 } else { -1000.0 };
            let fbk = leg_cos(leg) * 1000.0;
            // the two tetrapods: legs 0, 2 (right) and 5, 7 (left) against 1, 3 and 4, 6
            let psi = if leg == 0 || leg == 2 || leg == 5 || leg == 7 { 0.0 } else { PI };
            let ph = tau_ * t / 0.8 + psi;
            reg[32] = leg as f64;
            reg[33] = side as f64;
            reg[34] = ((fbk) as i64) as f64;
            reg[35] = ((dsinf(ph) * 1000.0f32) as i64) as f64;
            reg[36] = ((dcosf(ph) * 1000.0f32) as i64) as f64;
            for kind in 0..LEGJ {
                reg[37 + kind] = tok[25 + kind * N_LEGS + leg];
                reg[42 + kind] = tok[25 + NJ + kind * N_LEGS + leg];
            }
            reg[47] = tok[17 + leg];
            for i in 0..12 {
                reg[48 + i] = tok[4 + i];
            }
            reg[60] = ((side as f64 * tok[4] / 1000.0) as i64) as f64;
            reg[61] = ((fbk as f64 * tok[4] / 1000.0) as i64) as f64;
            for i in 0..2 {
                reg[62 + i] = self.sig[i] as f64;
            }
            if let Some(nnw) = self.brain.nn.as_deref() {
                let mut xl = [0.0f32; NN_IN];
                xl[0] = (reg[33] / 1000.0) as f32;
                xl[1] = (reg[34] / 1000.0) as f32;
                xl[2] = (reg[35] / 1000.0) as f32;
                xl[3] = (reg[36] / 1000.0) as f32;
                xl[4] = (reg[47] / 1000.0) as f32;
                for k in 0..5 {
                    xl[5 + k] = (reg[37 + k] / 1000.0) as f32;
                }
                for k in 0..5 {
                    xl[10 + k] = (reg[42 + k] / 10000.0) as f32;
                }
                xl[15] = (reg[48] / 1000.0) as f32;
                xl[16] = (reg[49] / 3000.0) as f32;
                xl[17] = (reg[50] / 1000.0) as f32;
                xl[18] = (reg[51] / 1000.0) as f32;
                xl[19] = (reg[52] / 1000.0) as f32;
                xl[20] = (reg[53] / 300.0) as f32;
                xl[21] = (reg[54] / 1000.0) as f32;
                xl[22] = (reg[55] / 1000.0) as f32;
                xl[23] = (reg[56] / 2000.0) as f32;
                xl[24] = (reg[57] / 1000.0) as f32;
                xl[25] = (reg[58] / 1000.0) as f32;
                xl[26] = (reg[59] / 1000.0) as f32;
                xl[27] = (reg[62] / 1000.0) as f32;
                xl[28] = (reg[63] / 1000.0) as f32;
                xl[29] = (tok[105] / 1000.0) as f32;
                xl[30] = (tok[106] / 4000.0) as f32;
                xl[31] = (tok[111] / 1000.0) as f32;
                xl[32] = (tok[112] / 1000.0) as f32;
                xl[33] = (tok[113] / 1000.0) as f32;
                xl[34] = (tok[114] / 1000.0) as f32;
                let mut ol = [0.0f32; NN_OUT];
                let mut hn = [0.0f32; NN_H];
                if nnw.len() == NNP_V1 {
                    let xl1: [f32; NN_IN_V1] = std::array::from_fn(|i| xl[i]);
                    nn_step::<NN_H, NN_IN_V1, NN_OUT>(nnw, &xl1, &self.nn_hl[leg], &mut hn, &mut ol);
                } else {
                    nn_step::<NN_H, NN_IN, NN_OUT>(nnw, &xl, &self.nn_hl[leg], &mut hn, &mut ol);
                }
                self.nn_hl[leg] = hn;
                for k in 0..5 {
                    reg[k] = (nn_tanh(ol[k]) * 1000.0f32) as f64;
                }
                reg[5] = ((0.5f32 * (1.0f32 + nn_tanh(ol[5]))) * 1000.0f32) as f64;
            } else {
                vm_run(&self.brain.leg, &mut reg, &mut self.mem, &tok, &mut self.rng, BUDGET_LEG);
            }
            for kind in 0..LEGJ {
                let child = 2 + LEGB * leg + kind;
                let v = reg[kind];
                let tv = if v == v && v - v == 0.0 { (v / 1000.0) as f32 } else { 0.0 };
                self.tau[child] = fminf(fmaxf(tv, -1.0), 1.0);
            }
            let gv = reg[5];
            self.grip[leg] = fminf(fmaxf(if gv == gv && gv - gv == 0.0 { (gv / 1000.0) as f32 } else { 0.0 }, 0.0), 1.0);
        }
    }

    /// The physics of one tick: the body falls, is held up by the ground and by its joints, and moves under the muscle torques and the hydraulic pressure that `think` left.
    pub fn physics(&mut self, ground: &dyn Ground) {
        let m = &self.model;
        let beta = m.beta;
        let ws = m.ws;
        let subs = m.sub as i32;
        let h = DT / subs as f32;
        for k in 0..N_LEGS {
            self.foot[k] = false;
        }
        let mut cb = [0usize; MAXC];
        let mut cs = [0usize; MAXC];
        let mut cr = [V3::default(); MAXC];
        let mut cn = [V3::default(); MAXC];
        let mut ct1 = [V3::default(); MAXC];
        let mut ct2 = [V3::default(); MAXC];
        let mut ckn = [0.0f32; MAXC];
        let mut ckt1 = [0.0f32; MAXC];
        let mut ckt2 = [0.0f32; MAXC];
        let mut cbias = [0.0f32; MAXC];
        let mut cacn = [0.0f32; MAXC];
        let mut cact1 = [0.0f32; MAXC];
        let mut cact2 = [0.0f32; MAXC];
        let mut cadh = [0.0f32; MAXC];
        for _sub in 0..subs {
            self.update_frames();
            // forces: gravity, muscles and hydraulics, the elasticity and friction of the joints (as changes of velocity)
            for i in 0..NB {
                if self.im[i] > 0.0 {
                    self.vl[i].y -= G * h;
                }
            }
            // the thread: pulls the front of the abdomen towards the anchor like a stiff spring with a damper, never pushes, and no harder than silk can hold
            self.silk_tension = 0.0;
            if self.anchored {
                let hs1 = ld3(&self.model.half[1]);
                let rr = mulv(&self.r[1], v3(-hs1.x, 0.0, 0.0));
                let sp = self.x[1] + rr;
                let dvec = self.anchor - sp;
                let dist = dot(dvec, dvec).sqrt();
                if dist > self.silk_len + 1.0e-4 && dist > 1.0e-6 {
                    let u = dvec * (1.0 / dist);
                    let vp = self.vl[1] + cross(self.wl[1], rr);
                    let toward = dot(vp, u);
                    let f = fminf(fmaxf(SILK_K * (dist - self.silk_len) - SILK_C * toward, 0.0), SILK_TMAX);
                    let pp = u * (f * h);
                    self.vl[1] = self.vl[1] + pp * self.im[1];
                    self.wl[1] = self.wl[1] + mulv(&self.iw[1], cross(rr, pp));
                    self.silk_tension = f;
                }
            }
            for b in 1..NB {
                let p = self.model.parent[b] as usize;
                let u = mulv(&self.r[p], ld3(&self.model.axp[b]));
                let mut torque = if self.model.hk[b] > 0.0 { 0.0 } else { self.ks * (self.tau[b] * self.model.tmax[b]) };
                if self.model.hk[b] > 0.0 {
                    // the hydraulics: the pressure asks for a posture (rest at half pressure, extended above it, flexed below it) and the joint is held there like by a stiff spring that cannot push harder
                    // than the blood can
                    let dir = if self.model.hyd[b] > 0.0 { 1.0f32 } else { -1.0 };
                    let mut want = if self.pressure >= 0.5 { dir * self.model.hx[b] * (self.pressure - 0.5) * 2.0 } else { -dir * self.model.hf[b] * (0.5 - self.pressure) * 2.0 };
                    // the brain's command moves the posture the joint is held at: up to the whole extension range one way and the whole flexion range the other
                    let cmd = self.tau[b];
                    want += dir * (if cmd >= 0.0 { self.model.hx[b] } else { self.model.hf[b] }) * cmd;
                    let th = hinge_angle(&self.q[p], &self.q[b], &self.model.qrel[b], ld3(&self.model.axp[b]));
                    let cap = self.model.hyd[b].abs() * self.ks;
                    torque += fminf(fmaxf(self.kk * self.model.hk[b] * (want - th), -cap), cap);
                } else if self.model.spring[b] > 0.0 {
                    let th = hinge_angle(&self.q[p], &self.q[b], &self.model.qrel[b], ld3(&self.model.axp[b]));
                    torque -= self.kk * self.model.spring[b] * th;
                }
                if torque != 0.0 {
                    let tt = u * (torque * h);
                    self.wl[b] = self.wl[b] + mulv(&self.iw[b], tt);
                    self.wl[p] = self.wl[p] - mulv(&self.iw[p], tt);
                }
                // friction: implicit, so that light limbs stay stable
                let damp = self.model.damp[b];
                let meff = dot(u, mulv(&self.iw[b], u)) + dot(u, mulv(&self.iw[p], u));
                if damp > 0.0 {
                    let rel = dot(self.wl[b] - self.wl[p], u);
                    let l = -damp * h * rel / (1.0 + damp * h * meff);
                    let tt = u * l;
                    self.wl[b] = self.wl[b] + mulv(&self.iw[b], tt);
                    self.wl[p] = self.wl[p] - mulv(&self.iw[p], tt);
                }
                // a muscle cannot move a joint faster than WMAX
                if self.tau[b] != 0.0 || self.model.hk[b] != 0.0 {
                    let rel = dot(self.wl[b] - self.wl[p], u);
                    let cl = fminf(fmaxf(rel, -WMAX), WMAX);
                    if cl != rel && meff > 1e-12 {
                        let tt = u * ((cl - rel) / meff);
                        self.wl[b] = self.wl[b] + mulv(&self.iw[b], tt);
                        self.wl[p] = self.wl[p] - mulv(&self.iw[p], tt);
                    }
                }
            }
            // the joints for this step
            for b in 1..NB {
                let p = self.model.parent[b] as usize;
                self.jrp[b] = mulv(&self.r[p], ld3(&self.model.rp[b]));
                self.jrc[b] = mulv(&self.r[b], ld3(&self.model.rc[b]));
                self.jbp[b] = ((self.x[b] + self.jrc[b]) - (self.x[p] + self.jrp[b])) * (beta / h);
                // K = (im_p + im_c) I - [rp]x IWp [rp]x - [rc]x IWc [rc]x
                let imsum = self.im[p] + self.im[b];
                let mut k = [0.0f32; 9];
                k[0] = imsum;
                k[4] = imsum;
                k[8] = imsum;
                for side in 0..2 {
                    let rv = if side == 0 { self.jrp[b] } else { self.jrc[b] };
                    let inertia = if side == 0 { &self.iw[p] } else { &self.iw[b] };
                    let s = [0.0, -rv.z, rv.y, rv.z, 0.0, -rv.x, -rv.y, rv.x, 0.0f32];
                    let mut si = [0.0f32; 9];
                    for a in 0..3 {
                        for c in 0..3 {
                            si[a * 3 + c] = s[a * 3] * inertia.m[c] + s[a * 3 + 1] * inertia.m[3 + c] + s[a * 3 + 2] * inertia.m[6 + c];
                        }
                    }
                    for a in 0..3 {
                        for c in 0..3 {
                            k[a * 3 + c] -= si[a * 3] * s[c] + si[a * 3 + 1] * s[3 + c] + si[a * 3 + 2] * s[6 + c];
                        }
                    }
                }
                let c00 = k[4] * k[8] - k[5] * k[7];
                let c01 = k[5] * k[6] - k[3] * k[8];
                let c02 = k[3] * k[7] - k[4] * k[6];
                let det = k[0] * c00 + k[1] * c01 + k[2] * c02;
                let id = if det.abs() > 1e-20 { 1.0 / det } else { 0.0 };
                self.jk[b] = [
                    c00 * id,
                    (k[2] * k[7] - k[1] * k[8]) * id,
                    (k[1] * k[5] - k[2] * k[4]) * id,
                    c01 * id,
                    (k[0] * k[8] - k[2] * k[6]) * id,
                    (k[2] * k[3] - k[0] * k[5]) * id,
                    c02 * id,
                    (k[1] * k[6] - k[0] * k[7]) * id,
                    (k[0] * k[4] - k[1] * k[3]) * id,
                ];
                // the hinge: the axis as the parent has it, and the child's
                let u = mulv(&self.r[p], ld3(&self.model.axp[b]));
                let uc = mulv(&self.r[b], ld3(&self.model.axc[b]));
                self.ju[b] = u;
                let hv = if u.x.abs() < 0.9 { v3(1.0, 0.0, 0.0) } else { v3(0.0, 1.0, 0.0) };
                let mut t1 = cross(u, hv);
                let l1 = dot(t1, t1).sqrt();
                t1 = t1 * (1.0 / l1);
                let t2 = cross(u, t1);
                self.jt1[b] = t1;
                self.jt2[b] = t2;
                let m1 = mulv(&self.iw[p], t1) + mulv(&self.iw[b], t1);
                let m2 = mulv(&self.iw[p], t2) + mulv(&self.iw[b], t2);
                let k00 = dot(t1, m1);
                let k01 = dot(t1, m2);
                let k11 = dot(t2, m2);
                let dd = k00 * k11 - k01 * k01;
                let idd = if dd > 1e-12 { 1.0 / dd } else { 0.0 };
                self.jkang[b] = [k11 * idd, -k01 * idd, -k01 * idd, k00 * idd];
                let phi = cross(uc, u);
                self.jbang[b] = [(beta / h) * dot(phi, t1), (beta / h) * dot(phi, t2)];
                let am = dot(u, mulv(&self.iw[p], u)) + dot(u, mulv(&self.iw[b], u));
                self.jam[b] = if am > 1e-12 { 1.0 / am } else { 0.0 };
                self.jth[b] = hinge_angle(&self.q[p], &self.q[b], &self.model.qrel[b], ld3(&self.model.axp[b]));
            }
            // the contacts with the ground and with solids: the corners of every box that are at or under the surface
            let mut nc = 0usize;
            macro_rules! add_contact {
                ($b:expr, $slot:expr, $rv:expr, $n:expr, $depth:expr) => {{
                    let (b_, slot_, rv_, n_, depth_) = ($b, $slot, $rv, $n, $depth);
                    let ha = if n_.x.abs() < 0.9 { v3(1.0, 0.0, 0.0) } else { v3(0.0, 0.0, 1.0) };
                    let t1 = norm3(cross(n_, ha));
                    let t2 = cross(n_, t1);
                    cb[nc] = b_;
                    cs[nc] = slot_;
                    cr[nc] = rv_;
                    cn[nc] = n_;
                    ct1[nc] = t1;
                    ct2[nc] = t2;
                    let im_b = self.im[b_];
                    let rn = cross(rv_, n_);
                    let rx = cross(rv_, t1);
                    let rz = cross(rv_, t2);
                    ckn[nc] = 1.0 / (im_b + dot(rn, mulv(&self.iw[b_], rn)));
                    ckt1[nc] = 1.0 / (im_b + dot(rx, mulv(&self.iw[b_], rx)));
                    ckt2[nc] = 1.0 / (im_b + dot(rz, mulv(&self.iw[b_], rz)));
                    cbias[nc] = fmaxf(0.0, (beta / h) * (depth_ - 0.002));
                    cacn[nc] = self.cimp[b_][slot_][0];
                    cact1[nc] = self.cimp[b_][slot_][1];
                    cact2[nc] = self.cimp[b_][slot_][2];
                    // the adhesive pad of a foot (the tarsus of a leg): how hard it holds on, as an impulse that may pull
                    cadh[nc] = if b_ >= 2 && (b_ - 2) % LEGB == LEGB - 1 { ADH * self.grip[(b_ - 2) / LEGB] * self.kg * h } else { 0.0 };
                    nc += 1;
                }};
            }
            let mut b = 0usize;
            while b < NB && nc < MAXC {
                let hs = ld3(&self.model.half[b]);
                let diag = dot(hs, hs).sqrt();
                let hc = ground.height(self.x[b].x, self.x[b].z);
                if self.x[b].y - hc > diag * 1.5 + 0.02 {
                    b += 1;
                    continue;
                }
                let mut s = 0usize;
                while s < 8 && nc < MAXC {
                    let lc = v3(if s & 1 != 0 { hs.x } else { -hs.x }, if s & 2 != 0 { hs.y } else { -hs.y }, if s & 4 != 0 { hs.z } else { -hs.z });
                    let rv = mulv(&self.r[b], lc);
                    let px = self.x[b].x + rv.x;
                    let py = self.x[b].y + rv.y;
                    let pz = self.x[b].z + rv.z;
                    let gh = ground.height(px, pz);
                    if py - gh < 0.003 {
                        let n = ground.normal(px, pz);
                        add_contact!(b, s, rv, n, (gh - py) * n.y);
                    }
                    s += 1;
                }
                b += 1;
            }
            // the solids (walls, ramps, ceilings): the same, with the signed distance and normal of the nearest one
            let mut b = 0usize;
            while b < NB && nc < MAXC {
                let hs = ld3(&self.model.half[b]);
                let diag = dot(hs, hs).sqrt();
                match ground.solid(self.x[b]) {
                    None => {
                        b += 1;
                        continue;
                    }
                    Some((dc, _)) if dc > diag * 1.5 + 0.02 => {
                        b += 1;
                        continue;
                    }
                    _ => {}
                }
                let mut s = 0usize;
                while s < 8 && nc < MAXC {
                    let lc = v3(if s & 1 != 0 { hs.x } else { -hs.x }, if s & 2 != 0 { hs.y } else { -hs.y }, if s & 4 != 0 { hs.z } else { -hs.z });
                    let rv = mulv(&self.r[b], lc);
                    let pc = v3(self.x[b].x + rv.x, self.x[b].y + rv.y, self.x[b].z + rv.z);
                    if let Some((d, n)) = ground.solid(pc) {
                        if d < 0.003 {
                            add_contact!(b, s + 8, rv, n, -d);
                        }
                    }
                    s += 1;
                }
                b += 1;
            }
            for i in 0..NB {
                for s in 0..16 {
                    self.cimp[i][s] = [0.0; 3];
                }
            }
            // warm start: what held the body up the step before is applied at once, the rounds only correct it
            for b in 1..NB {
                let p = self.model.parent[b] as usize;
                self.jl[b] = self.jl[b] * ws;
                self.jaw[b] = self.jaw[b] * ws;
                self.jacc_lo[b] *= ws;
                self.jacc_hi[b] *= ws;
                let pp = self.jl[b];
                self.vl[b] = self.vl[b] + pp * self.im[b];
                self.wl[b] = self.wl[b] + mulv(&self.iw[b], cross(self.jrc[b], pp));
                self.vl[p] = self.vl[p] - pp * self.im[p];
                self.wl[p] = self.wl[p] - mulv(&self.iw[p], cross(self.jrp[b], pp));
                let ll = self.jaw[b] + self.ju[b] * (self.jacc_lo[b] - self.jacc_hi[b]);
                self.wl[b] = self.wl[b] + mulv(&self.iw[b], ll);
                self.wl[p] = self.wl[p] - mulv(&self.iw[p], ll);
            }
            for c in 0..nc {
                let b = cb[c];
                cact1[c] *= ws;
                cacn[c] *= ws;
                cact2[c] *= ws;
                let pp = cn[c] * cacn[c] + ct1[c] * cact1[c] + ct2[c] * cact2[c];
                self.vl[b] = self.vl[b] + pp * self.im[b];
                self.wl[b] = self.wl[b] + mulv(&self.iw[b], cross(cr[c], pp));
            }
            // the rounds: first with the correction of position errors, then (after the bodies moved) once more without it
            let mu = self.model.mu;
            for pass in 0..2 {
                let bf = if pass == 0 { 1.0f32 } else { 0.0 };
                let nr = if pass == 0 { self.rounds } else { self.model.relax as i32 };
                for _round in 0..nr {
                    for b in 1..NB {
                        let p = self.model.parent[b] as usize;
                        // the two anchors move together
                        let vrel = (self.vl[b] + cross(self.wl[b], self.jrc[b])) - (self.vl[p] + cross(self.wl[p], self.jrp[b]));
                        let rhs = v3(-(vrel.x + bf * self.jbp[b].x), -(vrel.y + bf * self.jbp[b].y), -(vrel.z + bf * self.jbp[b].z));
                        let jk = &self.jk[b];
                        let lam = v3(jk[0] * rhs.x + jk[1] * rhs.y + jk[2] * rhs.z, jk[3] * rhs.x + jk[4] * rhs.y + jk[5] * rhs.z, jk[6] * rhs.x + jk[7] * rhs.y + jk[8] * rhs.z);
                        self.jl[b] = self.jl[b] + lam;
                        self.vl[b] = self.vl[b] + lam * self.im[b];
                        self.wl[b] = self.wl[b] + mulv(&self.iw[b], cross(self.jrc[b], lam));
                        self.vl[p] = self.vl[p] - lam * self.im[p];
                        self.wl[p] = self.wl[p] - mulv(&self.iw[p], cross(self.jrp[b], lam));
                        // the hinge: no relative spin across the axis
                        {
                            let wrel = self.wl[b] - self.wl[p];
                            let r1 = bf * self.jbang[b][0] - dot(wrel, self.jt1[b]);
                            let r2 = bf * self.jbang[b][1] - dot(wrel, self.jt2[b]);
                            let ja = &self.jkang[b];
                            let l1 = ja[0] * r1 + ja[1] * r2;
                            let l2 = ja[2] * r1 + ja[3] * r2;
                            let ll = self.jt1[b] * l1 + self.jt2[b] * l2;
                            self.jaw[b] = self.jaw[b] + ll;
                            self.wl[b] = self.wl[b] + mulv(&self.iw[b], ll);
                            self.wl[p] = self.wl[p] - mulv(&self.iw[p], ll);
                        }
                        // the limits
                        if self.model.lim[b][0] < self.model.lim[b][1] {
                            let u = self.ju[b];
                            let mut s = dot(self.wl[b] - self.wl[p], u);
                            let c_lo = self.jth[b] - self.model.lim[b][0];
                            let smin = if c_lo >= 0.0 { -c_lo / h } else { -bf * beta * c_lo / h };
                            let mut nw = fmaxf(self.jacc_lo[b] + (smin - s) * self.jam[b], 0.0);
                            let mut l = nw - self.jacc_lo[b];
                            self.jacc_lo[b] = nw;
                            if l != 0.0 {
                                let tt = u * l;
                                self.wl[b] = self.wl[b] + mulv(&self.iw[b], tt);
                                self.wl[p] = self.wl[p] - mulv(&self.iw[p], tt);
                                s += l * (dot(u, mulv(&self.iw[b], u)) + dot(u, mulv(&self.iw[p], u)));
                            }
                            let c_hi = self.model.lim[b][1] - self.jth[b];
                            let smax = if c_hi >= 0.0 { c_hi / h } else { bf * beta * c_hi / h };
                            nw = fmaxf(self.jacc_hi[b] + (s - smax) * self.jam[b], 0.0);
                            l = nw - self.jacc_hi[b];
                            self.jacc_hi[b] = nw;
                            if l != 0.0 {
                                let tt = u * l;
                                self.wl[b] = self.wl[b] - mulv(&self.iw[b], tt);
                                self.wl[p] = self.wl[p] + mulv(&self.iw[p], tt);
                            }
                        }
                    }
                    for c in 0..nc {
                        let b = cb[c];
                        let mut vp = self.vl[b] + cross(self.wl[b], cr[c]);
                        // the normal
                        let vn = dot(vp, cn[c]);
                        let nw = fmaxf(cacn[c] + (bf * cbias[c] - vn) * ckn[c], -cadh[c]);
                        let l = nw - cacn[c];
                        cacn[c] = nw;
                        {
                            let pp = cn[c] * l;
                            self.vl[b] = self.vl[b] + pp * self.im[b];
                            self.wl[b] = self.wl[b] + mulv(&self.iw[b], cross(cr[c], pp));
                        }
                        // friction (Coulomb)
                        let lim = mu * self.kg * self.model.mub[b] * (cacn[c] + cadh[c]);
                        vp = self.vl[b] + cross(self.wl[b], cr[c]);
                        let n1 = fminf(fmaxf(cact1[c] - dot(vp, ct1[c]) * ckt1[c], -lim), lim);
                        let d1 = n1 - cact1[c];
                        cact1[c] = n1;
                        let n2 = fminf(fmaxf(cact2[c] - dot(vp, ct2[c]) * ckt2[c], -lim), lim);
                        let d2 = n2 - cact2[c];
                        cact2[c] = n2;
                        let pp = ct1[c] * d1 + ct2[c] * d2;
                        self.vl[b] = self.vl[b] + pp * self.im[b];
                        self.wl[b] = self.wl[b] + mulv(&self.iw[b], cross(cr[c], pp));
                    }
                }
                if pass == 0 {
                    // the bodies move
                    for i in 0..NB {
                        let ws_ = dot(self.wl[i], self.wl[i]).sqrt();
                        if ws_ > 60.0 {
                            self.wl[i] = self.wl[i] * (60.0 / ws_);
                        }
                        self.x[i] = self.x[i] + self.vl[i] * h;
                        let wq = [self.wl[i].x * 0.5 * h, self.wl[i].y * 0.5 * h, self.wl[i].z * 0.5 * h, 0.0f32];
                        let dq = qmul(&wq, &self.q[i]);
                        let q0 = self.q[i][0] + dq[0];
                        let q1 = self.q[i][1] + dq[1];
                        let q2 = self.q[i][2] + dq[2];
                        let q3 = self.q[i][3] + dq[3];
                        let nq = 1.0 / (q0 * q0 + q1 * q1 + q2 * q2 + q3 * q3).sqrt();
                        self.q[i] = [q0 * nq, q1 * nq, q2 * nq, q3 * nq];
                    }
                }
            }
            self.last_contacts = nc;
            self.last_force = 0.0;
            for c in 0..nc {
                self.last_force += cacn[c] / h;
            }
            for c in 0..nc {
                self.cimp[cb[c]][cs[c]] = [cacn[c], cact1[c], cact2[c]];
                // a foot (the tarsus of a leg) that pushes on the ground touches it
                if cb[c] >= 2 && (cb[c] - 2) % LEGB == LEGB - 1 && cacn[c].abs() > 1e-4 {
                    self.foot[(cb[c] - 2) / LEGB] = true;
                }
            }
        }
        self.tick += 1;
    }

    /// One tick: the senses and the brain, then the physics.
    pub fn step(&mut self, target: [f32; 3], ground: &dyn Ground) {
        self.think(target, ground);
        self.physics(ground);
    }

    /// One tick of standing still instead of thinking: the brain is not run (its memory keeps what it was), the hydraulic pressure goes to a firm stance, every joint is held at its neutral setpoint, every foot grips, the
    /// thread is let go. A resting, scanning or frozen spider is this: all eight legs down, nothing moving.
    pub fn stand(&mut self, ground: &dyn Ground) {
        self.update_frames();
        let slew = 0.5 * DT;
        self.pressure += fminf(fmaxf(0.75 - self.pressure, -slew), slew);
        for t in self.tau.iter_mut() {
            *t = 0.0;
        }
        self.grip = [1.0; N_LEGS];
        self.sig = [0.0; 2];
        self.silk_update(false, 0.0, v3(1.0, 0.0, 0.0), ground);
        self.physics(ground);
    }
}

/// Where a ray from `o` along the unit vector `d` first meets the ground or a solid within `max` metres (the last point of free air before it), or `None`.
pub fn cast(ground: &dyn Ground, o: V3, d: V3, max: f32) -> Option<V3> {
    let inside = |p: V3| -> bool {
        if p.y < ground.height(p.x, p.z) {
            return true;
        }
        match ground.solid(p) {
            Some((ds, _)) => ds < 0.0,
            None => false,
        }
    };
    let step = 0.05f32;
    let mut prev = 0.0f32;
    let mut s = step;
    while s <= max {
        let p = o + d * s;
        if inside(p) {
            // between prev (free) and s (inside): halve six times
            let (mut lo, mut hi) = (prev, s);
            for _ in 0..6 {
                let mid = 0.5 * (lo + hi);
                if inside(o + d * mid) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            return Some(o + d * lo);
        }
        prev = s;
        s += step;
    }
    None
}

/// How far a point is from the nearest surface (the heightfield below it or a solid) and which way that surface faces.
pub fn support(ground: &dyn Ground, p: V3) -> (f32, V3) {
    let mut d = p.y - ground.height(p.x, p.z);
    let mut n = ground.normal(p.x, p.z);
    if let Some((ds, ns)) = ground.solid(p) {
        if ds < d {
            d = ds;
            n = ns;
        }
    }
    (d, n)
}

/// Builds a program from instructions (for tests and for hand-written brains).
pub fn brain(setup: Vec<Ins>, leg: Vec<Ins>, body: Vec<Ins>) -> Brain {
    Brain { setup, leg, body, nn: None }
}
