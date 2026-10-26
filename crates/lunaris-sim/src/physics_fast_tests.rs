//! The fast physics path against the reference: the same random world is run twice, once with each, and the state hash and the contacts of
//! every tick must be identical. The worlds mix every kind of collider the engine has (and the ones the fast path hands to the reference:
//! spinning bodies, joints), and change while they run (teleports, spawns, despawns, new colliders, layers) so that the grids kept between
//! ticks are checked against edits of every sort.
use super::*;
use lunaris_json::{parse, Json};

struct R(u64);

impl R {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn f(&mut self) -> f32 {
        (self.next() & 0xFF_FFFF) as f32 / (1u32 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    fn below(&mut self, n: u32) -> u32 {
        (self.next() % n.max(1) as u64) as u32
    }
    fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }
}

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

#[derive(Clone, Copy, Debug)]
struct Spec {
    n_static: u32,
    n_dyn: u32,
    size: f32,
    rotated: bool,
    capsules: bool,
    triggers: bool,
    layers: bool,
    spinners: bool,
    joints: bool,
    movers: bool,
    floor: bool,
    big: bool,
    huge: bool,
    static_rb: bool,
    edits: bool,
    scale: bool,
    fast: bool,
    /// The world has a `PhysicsSleep` (with low limits, so that bodies fall asleep within the run).
    sleep: bool,
}

fn spec_of(seed: u64) -> Spec {
    let mut r = R(seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xABCD);
    let mut b = |p: f32| r.chance(p);
    Spec {
        n_static: [0, 3, 12, 40, 90, 160][seed as usize % 6],
        n_dyn: [1, 5, 20, 60, 120][(seed / 6) as usize % 5],
        size: [4.0, 12.0, 40.0][(seed / 30) as usize % 3],
        rotated: b(0.4),
        capsules: b(0.4),
        triggers: b(0.35),
        layers: b(0.3),
        spinners: b(0.12),
        joints: b(0.12),
        movers: b(0.3),
        floor: b(0.7),
        big: b(0.3),
        huge: b(0.08),
        static_rb: b(0.3),
        edits: b(0.6),
        scale: b(0.3),
        fast: b(0.35),
        sleep: b(0.5),
    }
}

fn quat(r: &mut R) -> String {
    let a = r.range(0.0, std::f32::consts::TAU) * 0.5;
    if r.chance(0.5) {
        format!("[0,{},0,{}]", a.sin(), a.cos())
    } else {
        let (x, y, z) = (r.range(-1.0, 1.0), r.range(-1.0, 1.0), r.range(-1.0, 1.0));
        let n = (x * x + y * y + z * z).sqrt().max(1e-3);
        format!("[{},{},{},{}]", a.sin() * x / n, a.sin() * y / n, a.sin() * z / n, a.cos())
    }
}

/// One random entity's components.
fn random_entity(r: &mut R, spec: &Spec, kind: &str, uid_hint: u64) -> Vec<(String, Json)> {
    let off = if spec.huge { 100_000.0 } else { 0.0 };
    let half = spec.size;
    let pos = [r.range(-half, half) + off, r.range(0.2, 6.0), r.range(-half, half) + off];
    let turned = spec.rotated && r.chance(0.35);
    let mut tf = format!(r#"{{"pos":[{},{},{}]"#, pos[0], pos[1], pos[2]);
    if turned {
        tf.push_str(&format!(r#","rot":{}"#, quat(r)));
    }
    if spec.scale && r.chance(0.3) {
        tf.push_str(&format!(r#","scale":[{},{},{}]"#, r.range(0.5, 2.0), r.range(0.5, 2.0), r.range(0.5, 2.0)));
    }
    tf.push('}');
    let shape = if spec.capsules && r.chance(0.3) {
        2
    } else if r.chance(0.4) {
        0
    } else {
        1
    };
    let sz = match shape {
        0 => {
            let d = r.range(0.3, 2.0);
            format!("[{d},{d},{d}]")
        }
        2 => {
            let d = r.range(0.3, 1.0);
            format!("[{d},{},{d}]", r.range(1.0, 2.5))
        }
        _ => format!("[{},{},{}]", r.range(0.3, 3.0), r.range(0.3, 3.0), r.range(0.3, 3.0)),
    };
    let trigger = spec.triggers && kind != "dyn" && r.chance(0.15);
    let mut c: Vec<(String, Json)> = vec![("Transform".into(), j(&tf)), ("Collider".into(), j(&format!(r#"{{"shape":{shape},"size":{sz},"trigger":{trigger}}}"#)))];
    match kind {
        "dyn" => {
            // (some bodies are shots: faster than their size in a tick, which is what the continuous collision is for)
            let speed = if spec.fast && r.chance(0.4) { 90.0 } else { 3.0 };
            c.push(("Velocity".into(), j(&format!(r#"{{"lin":[{},{},{}]}}"#, r.range(-speed, speed), r.range(-1.0, 1.0), r.range(-speed, speed)))));
            c.push(("RigidBody".into(), j(&format!(r#"{{"mass":{},"restitution":{},"friction":{},"gravity_scale":{},"drag":{}}}"#, r.range(0.4, 4.0), r.range(0.0, 0.9), r.range(0.0, 1.0), if r.chance(0.3) { 0.0 } else { 1.0 }, if r.chance(0.3) { 0.2 } else { 0.0 }))));
            if spec.spinners && r.chance(0.5) {
                c.push(("AngularVelocity".into(), j(&format!(r#"{{"w":[{},{},{}]}}"#, r.range(-3.0, 3.0), r.range(-3.0, 3.0), r.range(-3.0, 3.0)))));
            }
        }
        _ => {
            let mut has_vel = false;
            if spec.movers && r.chance(0.15) {
                has_vel = true;
                c.push(("Velocity".into(), j(&format!(r#"{{"lin":[{},0,{}]}}"#, r.range(-1.0, 1.0), r.range(-1.0, 1.0)))));
            }
            if spec.static_rb && r.chance(0.15) {
                // scenery with a material, or a body whose mass is 0 / that has no velocity
                c.push(("RigidBody".into(), j(&format!(r#"{{"mass":{},"restitution":{},"friction":{}}}"#, if r.chance(0.5) { 0.0 } else { 2.0 }, r.range(0.0, 1.0), r.range(0.0, 1.0)))));
                if !has_vel && r.chance(0.5) {
                    c.push(("Velocity".into(), j("{}")));
                }
            }
        }
    }
    if spec.layers && r.chance(0.3) {
        c.push(("CollisionLayer".into(), j(&format!(r#"{{"layer":{},"mask":{}}}"#, r.below(4), [0xF, 0x3, 0x5, 0xE, 0x1][r.below(5) as usize]))));
    }
    let _ = uid_hint;
    c
}

fn build(seed: u64, spec: &Spec) -> Sim {
    let mut r = R(seed ^ 0x5555);
    let mut sim = Sim::new(seed);
    let mut uid = 1u64;
    fn spawn(sim: &mut Sim, uid: &mut u64, comps: Vec<(String, Json)>) {
        sim.apply(&Command::Spawn { uid: Some(*uid), components: comps }).unwrap();
        *uid += 1;
    }
    let off = if spec.huge { 100_000.0 } else { 0.0 };
    if spec.floor {
        spawn(&mut sim, &mut uid, vec![("Transform".into(), j(&format!(r#"{{"pos":[{off},-0.5,{off}]}}"#))), ("Collider".into(), j(&format!(r#"{{"shape":1,"size":[{},1,{}]}}"#, spec.size * 6.0, spec.size * 6.0)))]);
    }
    if spec.big {
        for k in 0..3 {
            let a = k as f32 * 2.1;
            spawn(&mut sim, &mut uid, vec![("Transform".into(), j(&format!(r#"{{"pos":[{},2,{}]}}"#, off + a.cos() * spec.size, off + a.sin() * spec.size))), ("Collider".into(), j(&format!(r#"{{"shape":1,"size":[{},4,1]}}"#, spec.size * 1.5)))]);
        }
    }
    for _ in 0..spec.n_static {
        let c = random_entity(&mut r, spec, "static", 0);
        spawn(&mut sim, &mut uid, c);
    }
    let mut dyn_uids = Vec::new();
    for _ in 0..spec.n_dyn {
        let c = random_entity(&mut r, spec, "dyn", 0);
        dyn_uids.push(uid);
        spawn(&mut sim, &mut uid, c);
    }
    if spec.joints && dyn_uids.len() >= 2 {
        for k in 0..(dyn_uids.len() / 4).min(6) {
            let a = dyn_uids[r.below(dyn_uids.len() as u32) as usize];
            let target = if r.chance(0.5) { dyn_uids[r.below(dyn_uids.len() as u32) as usize] } else { 0 };
            let kind = r.below(5);
            let _ = sim.apply(&Command::Add { uid: a, component: "Joint".into(), value: j(&format!(r#"{{"kind":{kind},"target":{target},"anchor":[0,8,0],"length":{},"stiffness":40,"damping":2}}"#, r.range(1.0, 4.0))) });
            let _ = k;
        }
    }
    if spec.sleep {
        spawn(&mut sim, &mut uid, vec![("PhysicsSleep".into(), j(r#"{"time":0.1,"linear":0.6,"angular":0.6}"#))]);
    }
    sim
}

/// A random edit, applied to both worlds.
fn edit(r: &mut R, spec: &Spec, a: &mut Sim, b: &mut Sim, next_uid: &mut u64) {
    let max_uid = *next_uid;
    let pick = |r: &mut R| 1 + r.below(max_uid as u32) as u64;
    let cmd = match r.below(8) {
        0 => Command::Set { uid: pick(r), component: "Transform".into(), value: j(&format!(r#"{{"pos":[{},{},{}]}}"#, r.range(-spec.size, spec.size), r.range(0.2, 5.0), r.range(-spec.size, spec.size))) },
        1 => Command::Despawn { uid: pick(r) },
        2 => {
            let kind = if r.chance(0.5) { "dyn" } else { "static" };
            let c = random_entity(r, spec, kind, 0);
            let u = *next_uid;
            *next_uid += 1;
            Command::Spawn { uid: Some(u), components: c }
        }
        3 => Command::Remove { uid: pick(r), component: "RigidBody".into() },
        4 => Command::Add { uid: pick(r), component: "RigidBody".into(), value: j(r#"{"mass":1.5}"#) },
        5 => Command::Set { uid: pick(r), component: "Collider".into(), value: j(&format!(r#"{{"size":[{},{},{}],"trigger":{}}}"#, r.range(0.3, 2.0), r.range(0.3, 2.0), r.range(0.3, 2.0), r.chance(0.2))) },
        6 => Command::Add { uid: pick(r), component: "CollisionLayer".into(), value: j(&format!(r#"{{"layer":{},"mask":{}}}"#, r.below(4), [0xF, 0x3, 0x5][r.below(3) as usize])) },
        _ => Command::Set { uid: pick(r), component: "Transform".into(), value: j(&format!(r#"{{"scale":[{},{},{}]}}"#, r.range(0.4, 2.5), r.range(0.4, 2.5), r.range(0.4, 2.5))) },
    };
    let ra = a.apply(&cmd);
    let rb = b.apply(&cmd);
    assert_eq!(ra.is_ok(), rb.is_ok(), "the same edit must behave the same in both worlds: {cmd:?}");
}

/// The contacts of two worlds are the same, bit for bit (a world that has gone to NaN, as a few of the random ones do, has contacts that are NaN in both and `==` says no).
fn contacts_equal(a: &[crate::physics::Contact], b: &[crate::physics::Contact]) -> bool {
    let same = |x: &[f32], y: &[f32]| x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits());
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.a == y.a && x.b == y.b && x.trigger == y.trigger && same(&x.normal, &y.normal) && x.depth.to_bits() == y.depth.to_bits())
}

fn run_pair(seed: u64, ticks: u32) {
    let spec = spec_of(seed);
    let mut a = build(seed, &spec);
    let mut b = build(seed, &spec);
    b.reference_physics = true;
    let mut r = R(seed ^ 0x7777);
    let mut next_uid = 1 + a.world.len() as u64 + 50;
    for t in 0..ticks {
        if spec.edits && t % 5 == 3 {
            for _ in 0..1 + r.below(3) {
                edit(&mut r, &spec, &mut a, &mut b, &mut next_uid);
            }
        }
        a.step();
        b.step();
        assert!(contacts_equal(&a.contacts, &b.contacts), "contacts differ: seed {seed} tick {t} spec {spec:?}: {:?} against {:?}", a.contacts, b.contacts);
        assert_eq!(a.state_hash(), b.state_hash(), "state differs: seed {seed} tick {t} spec {spec:?}");
    }
}

/// Finds what differs in a world where the two paths part (debugging aid): `cargo test -p lunaris-sim debug_seed -- --ignored --nocapture`.
#[test]
#[ignore]
fn debug_seed() {
    let seed: u64 = std::env::var("SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(648);
    let spec = spec_of(seed);
    let mut a = build(seed, &spec);
    let mut b = build(seed, &spec);
    b.reference_physics = true;
    let mut r = R(seed ^ 0x7777);
    let mut next_uid = 1 + a.world.len() as u64 + 50;
    for t in 0..150u32 {
        if spec.edits && t % 5 == 3 {
            for _ in 0..1 + r.below(3) {
                edit(&mut r, &spec, &mut a, &mut b, &mut next_uid);
            }
        }
        let (sa0, sb0) = (a.sleep.sleeper_count(), b.sleep.sleeper_count());
        if t >= 46 {
            let probe: u64 = std::env::var("PROBE").ok().and_then(|v| v.parse().ok()).unwrap_or(44);
            if let Some(e) = a.world.entity(probe) {
                let tf = a.world.get::<crate::components::Transform>(e).copied();
                let co = a.world.get::<crate::components::Collider>(e).copied();
                let rb = a.world.get::<crate::components::RigidBody>(e).copied();
                eprintln!("before tick {t}: uid {probe} asleep {} tf {:?} col {:?} rb mass {:?}", a.sleep.is_asleep(probe), tf.map(|x| (x.pos, x.scale)), co.map(|c| (c.shape, c.size, c.trigger)), rb.map(|r| r.mass));
                if let Some(tf) = tf {
                    for uid in 1..next_uid + 5 {
                        let Some(o) = a.world.entity(uid) else { continue };
                        let Some(to) = a.world.get::<crate::components::Transform>(o) else { continue };
                        let d = [(to.pos[0] - tf.pos[0]).abs(), (to.pos[1] - tf.pos[1]).abs(), (to.pos[2] - tf.pos[2]).abs()];
                        if uid != probe && d[0] < 4.0 && d[1] < 4.0 && d[2] < 4.0 {
                            let c = a.world.get::<crate::components::Collider>(o).copied();
                            let r = a.world.get::<crate::components::RigidBody>(o).copied();
                            let v = a.world.get::<crate::components::Velocity>(o).map(|v| v.lin);
                            eprintln!("   near: uid {uid} asleep {} pos {:?} col {:?} mass {:?} vel {:?}", a.sleep.is_asleep(uid), to.pos, c.map(|c| (c.shape, c.size, c.trigger)), r.map(|r| r.mass), v);
                        }
                    }
                }
            }
        }
        let f0 = crate::physics::FAST_TICKS.load(std::sync::atomic::Ordering::Relaxed);
        a.step();
        b.step();
        let ran_fast = crate::physics::FAST_TICKS.load(std::sync::atomic::Ordering::Relaxed) - f0;
        if !contacts_equal(&a.contacts, &b.contacts) {
            eprintln!("tick {t}: contacts differ (fast steps this tick {ran_fast}): {} against {}", a.contacts.len(), b.contacts.len());
            for (x, y) in a.contacts.iter().zip(&b.contacts) {
                if x != y {
                    eprintln!("   first difference: {x:?} against {y:?}");
                    break;
                }
            }
            for c in a.contacts.iter().filter(|c| !b.contacts.contains(c)) {
                eprintln!("   only in fast: {c:?}");
            }
            for c in b.contacts.iter().filter(|c| !a.contacts.contains(c)) {
                eprintln!("   only in reference: {c:?}");
            }
            return;
        }
        if a.state_hash() != b.state_hash() {
            eprintln!("tick {t}: states differ; sleepers before {sa0} {sb0}, after {} {}", a.sleep.sleeper_count(), b.sleep.sleeper_count());
            eprintln!("sleep equal {} memory equal {} contacts equal {}", a.sleep == b.sleep, a.contact_memory == b.contact_memory, a.contacts == b.contacts);
            for uid in 1..next_uid + 5 {
                let (ea, eb) = (a.world.entity(uid), b.world.entity(uid));
                if ea.is_some() != eb.is_some() {
                    eprintln!("uid {uid}: exists {} {}", ea.is_some(), eb.is_some());
                    continue;
                }
                let (Some(ea), Some(eb)) = (ea, eb) else { continue };
                let (ta, tb) = (a.world.get::<crate::components::Transform>(ea).copied(), b.world.get::<crate::components::Transform>(eb).copied());
                let (va, vb) = (a.world.get::<crate::components::Velocity>(ea).map(|v| v.lin), b.world.get::<crate::components::Velocity>(eb).map(|v| v.lin));
                if ta.map(|x| x.pos) != tb.map(|x| x.pos) || va != vb {
                    eprintln!("uid {uid}: asleep {} {} pos {:?} {:?} vel {:?} {:?}", a.sleep.is_asleep(uid), b.sleep.is_asleep(uid), ta.map(|x| x.pos), tb.map(|x| x.pos), va, vb);
                }
            }
            return;
        }
    }
    eprintln!("no difference");
}

#[test]
fn fast_physics_matches_the_reference_on_random_worlds() {
    use std::sync::atomic::Ordering::Relaxed;
    let (t0, c0, f0) = (crate::physics::FAST_TICKS.load(Relaxed), crate::physics::FAST_CONTACTS.load(Relaxed), crate::physics::CCD_FIXES.load(Relaxed));
    let (s0, w0) = (crate::physics::SLEPT.load(Relaxed), crate::physics::WOKE.load(Relaxed));
    for seed in 0..240 {
        run_pair(seed, 90);
    }
    // (the worlds with a `PhysicsSleep` must reach sleeping and waking, or the comparison above says nothing about them)
    let (slept, woke) = (crate::physics::SLEPT.load(Relaxed) - s0, crate::physics::WOKE.load(Relaxed) - w0);
    eprintln!("bodies put to sleep so far {slept}, woken {woke}");
    assert!(slept > 500 && woke > 200, "sleeping and waking must both happen in the random worlds ({slept} {woke})");
    // (other tests run at the same time, so these are lower bounds of this test's own count)
    let fixes = crate::physics::CCD_FIXES.load(Relaxed) - f0;
    assert!(fixes > 200, "the random worlds must reach the continuous collision (it moved bodies only {fixes} times)");
    eprintln!("fast steps so far {}, contacts found by the fast path so far {}", crate::physics::FAST_TICKS.load(Relaxed) - t0, crate::physics::FAST_CONTACTS.load(Relaxed) - c0);
}

/// A much bigger sweep (about two thousand more worlds, a few minutes): `cargo test -p lunaris-sim --release many_more_worlds -- --ignored`.
#[test]
#[ignore]
fn fast_physics_matches_the_reference_on_many_more_worlds() {
    // (`SWEEP_FROM`, `SWEEP_TO` and `SWEEP_TICKS` choose other worlds and longer runs)
    let var = |name: &str, default: u64| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
    for seed in var("SWEEP_FROM", 240)..var("SWEEP_TO", 2400) {
        run_pair(seed, var("SWEEP_TICKS", 150) as u32);
    }
}

#[test]
fn fast_physics_matches_the_reference_on_longer_runs() {
    for seed in [3, 77, 150, 311, 640, 901] {
        run_pair(seed, 400);
    }
}

/// Two worlds that start the same and are stepped by different numbers of threads must stay the same (the fast path may use threads).
#[test]
fn fast_physics_does_not_depend_on_the_thread_count() {
    use std::sync::atomic::Ordering::Relaxed;
    let before = crate::physics::PAR_TICKS.load(Relaxed);
    for seed in [5u64, 17, 29, 41, 53, 66, 78, 90] {
        let spec = spec_of(seed);
        let mut one = build(seed, &spec);
        let mut many = build(seed, &spec);
        one.threads = 1;
        many.threads = 7;
        for t in 0..120 {
            one.step();
            many.step();
            assert_eq!(one.state_hash(), many.state_hash(), "seed {seed} tick {t}");
        }
    }
    assert!(crate::physics::PAR_TICKS.load(Relaxed) - before > 200, "the threaded code must have run in this test");
}

/// A fork of a world keeps the same history (its own empty cache must not change anything).
#[test]
fn a_fork_of_a_world_stays_in_lockstep_with_fast_physics() {
    for seed in [8u64, 21, 33] {
        let spec = spec_of(seed);
        let mut a = build(seed, &spec);
        a.step_n(30);
        let mut b = a.clone();
        for t in 0..60 {
            a.step();
            b.step();
            assert_eq!(a.state_hash(), b.state_hash(), "seed {seed} tick {t}");
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------- boxes that touch to within rounding

/// `x` moved by `k` steps of the last bit (any sign).
fn nudge(x: f32, k: i32) -> f32 {
    let bits = x.to_bits() as i64;
    let moved = if x >= 0.0 { bits + k as i64 } else { bits - k as i64 };
    f32::from_bits(moved.clamp(0, u32::MAX as i64) as u32)
}

/// Two bodies that touch to within a few units of the last bit of their numbers, on a random axis, in worlds where the axis of the sweep of the
/// reference is x, y or z: whether the reference meets the pair depends on that axis, so the fast path has to find the same axis.
#[test]
fn boxes_that_touch_to_within_rounding_are_handled_like_the_reference() {
    use std::sync::atomic::Ordering::Relaxed;
    let before = crate::physics::AXIS_TICKS.load(Relaxed);
    let mut r = R(0xB0B0);
    let mut contacts_seen = 0;
    for trial in 0..6000u64 {
        let spread_axis = r.below(3) as usize;
        let touch_axis = r.below(3) as usize;
        let mk = |sim: &mut Sim, uid: u64, comps: Vec<(&str, String)>| {
            sim.apply(&Command::Spawn { uid: Some(uid), components: comps.into_iter().map(|(n, v)| (n.to_string(), j(&v))).collect() }).unwrap();
        };
        let (mut a, mut b) = (Sim::new(trial), Sim::new(trial));
        b.reference_physics = true;
        let h = [r.range(0.1, 1.7), r.range(0.1, 1.7), r.range(0.1, 1.7)];
        let rad = r.range(0.1, 1.2);
        let base = [r.range(-30.0, 30.0), r.range(0.5, 5.0), r.range(-30.0, 30.0)];
        let kind = r.below(3);
        let capsule_len = rad * 2.0 + r.range(0.2, 1.5);
        let dyn_half = match kind {
            0 => [rad, rad, rad],
            1 => [rad, rad * 0.7, rad * 1.3],
            _ => [rad, capsule_len * 0.5, rad],
        };
        // the dynamic body touches the box on `touch_axis`, give or take a few units of the last bit, and overlaps it on the other axes
        let mut pos = base;
        for k in 0..3 {
            if k == touch_axis {
                let sign = if r.chance(0.5) { 1.0 } else { -1.0 };
                pos[k] = nudge(base[k] + sign * (h[k] + dyn_half[k]), r.below(9) as i32 - 4);
            } else {
                pos[k] = base[k] + r.range(-0.5, 0.5) * (h[k] + dyn_half[k]) * 0.5;
            }
        }
        // scenery far away on `spread_axis` so that it is the axis the reference sweeps along
        let anchors: Vec<[f32; 3]> = (0..3).map(|n| {
            let mut p = [r.range(-5.0, 5.0), r.range(0.5, 2.0), r.range(-5.0, 5.0)];
            p[spread_axis] = (n as f32 - 1.0) * 400.0 + r.range(-5.0, 5.0);
            p
        }).collect();
        for sim in [&mut a, &mut b] {
            mk(sim, 1, vec![("Transform", format!(r#"{{"pos":[{},{},{}]}}"#, base[0], base[1], base[2])), ("Collider", format!(r#"{{"shape":1,"size":[{},{},{}]}}"#, h[0] * 2.0, h[1] * 2.0, h[2] * 2.0))]);
            for (n, p) in anchors.iter().enumerate() {
                mk(sim, 10 + n as u64, vec![("Transform", format!(r#"{{"pos":[{},{},{}]}}"#, p[0], p[1], p[2])), ("Collider", r#"{"shape":1,"size":[1,1,1]}"#.into())]);
            }
            let shape = match kind {
                0 => format!(r#"{{"shape":0,"size":[{},{},{}]}}"#, rad * 2.0, rad * 2.0, rad * 2.0),
                1 => format!(r#"{{"shape":1,"size":[{},{},{}]}}"#, dyn_half[0] * 2.0, dyn_half[1] * 2.0, dyn_half[2] * 2.0),
                _ => format!(r#"{{"shape":2,"size":[{},{},{}]}}"#, rad * 2.0, capsule_len, rad * 2.0),
            };
            mk(sim, 2, vec![("Transform", format!(r#"{{"pos":[{},{},{}]}}"#, pos[0], pos[1], pos[2])), ("Velocity", "{}".into()), ("Collider", shape), ("RigidBody", r#"{"mass":1,"gravity_scale":0}"#.into())]);
        }
        for t in 0..2 {
            a.step();
            b.step();
            contacts_seen += a.contacts.len();
            assert_eq!(a.contacts, b.contacts, "contacts differ: trial {trial} tick {t} touch axis {touch_axis} spread axis {spread_axis}");
            assert_eq!(a.state_hash(), b.state_hash(), "state differs: trial {trial} tick {t} touch axis {touch_axis} spread axis {spread_axis}");
        }
    }
    let worked_out = crate::physics::AXIS_TICKS.load(Relaxed) - before;
    eprintln!("boundary trials: sweep axis worked out on {worked_out} steps, {contacts_seen} contacts");
    assert!(worked_out > 1000, "the trials must reach the case where the sweep axis matters");
}
