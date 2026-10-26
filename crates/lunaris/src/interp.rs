//! Drawing between simulation ticks. The simulation runs in fixed steps (60 a second) while the picture is drawn once per refresh of the display (144 or 280 a second), so without this every state of the world
//! is shown for two or three or five pictures in a row: motion is stepped at 60 Hz, and on a 144 Hz display unevenly (two pictures, then three), which looks like stutter and stiffness. Here the picture is
//! drawn at a moment *between* the last two ticks: the transforms of every entity (and the clock of every animation, and the offsets an `Inertia` chain is decaying) are blended from what they were a tick ago
//! to what they are, by how far the next tick is away. Only for the picture: the simulation never sees it (the originals are put back after the picture is drawn), so it stays deterministic and the saved,
//! replayed and networked state is the same. The price is that the picture is a tick (a sixtieth of a second) behind the simulation, as in every engine that does this.
use lunaris_ecs::World;
use lunaris_sim::components::{Animator, Transform};
use lunaris_sim::inertia::Inertia;
use std::collections::HashMap;

/// A world with more entities than this is not interpolated (the copy of every transform each tick would cost more than it gives).
const MAX_ENTITIES: usize = 20_000;
/// A thing that moved further than this (metres) in one tick was moved by the game (a teleport, a respawn) and is not blended.
const TELEPORT: f32 = 6.0;

#[derive(Clone, Copy)]
struct AnimSnap {
    clip: u32,
    time: f32,
    n: u32,
    gap: f32,
}

#[derive(Default)]
pub struct Interp {
    transforms: HashMap<u64, Transform>,
    anims: HashMap<u64, AnimSnap>,
    valid: bool,
    /// How many entities the world had (a big change is a different scene: nothing to blend).
    entities: usize,
}

/// What `apply` changed, to be put back by `restore`.
#[derive(Default)]
pub struct Undo {
    transforms: Vec<(u64, Transform)>,
    times: Vec<(u64, f32)>,
    gaps: Vec<(u64, f32)>,
}

fn nlerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let flip = if a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] < 0.0 { -1.0 } else { 1.0 };
    let q = [a[0] + (b[0] * flip - a[0]) * t, a[1] + (b[1] * flip - a[1]) * t, a[2] + (b[2] * flip - a[2]) * t, a[3] + (b[3] * flip - a[3]) * t];
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l > 1.0e-6 {
        [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
    } else {
        b
    }
}

impl Interp {
    /// Remember how things are now, just before a tick is simulated.
    pub fn capture(&mut self, world: &mut World) {
        self.transforms.clear();
        self.anims.clear();
        self.entities = world.len();
        let mut count = 0usize;
        let mut too_many = false;
        let _ = world.for_each_uid::<(&Transform,), _>(|_, uid, (t,)| {
            count += 1;
            if count > MAX_ENTITIES {
                too_many = true;
            } else {
                self.transforms.insert(uid, *t);
            }
        });
        if too_many {
            self.transforms.clear();
            self.valid = false;
            return;
        }
        let _ = world.for_each_uid::<(&Animator,), _>(|_, uid, (a,)| {
            self.anims.insert(uid, AnimSnap { clip: a.clip.0, time: a.time, n: 0, gap: 0.0 });
        });
        let _ = world.for_each_uid::<(&Inertia,), _>(|_, uid, (i,)| {
            if let Some(s) = self.anims.get_mut(&uid) {
                let n = (i.n as usize).min(i.gap.len());
                s.n = n as u32;
                s.gap = if n > 0 { i.gap[n - 1] } else { 0.0 };
            }
        });
        self.valid = true;
    }

    /// Put the world at `alpha` (0: as at the capture, 1: as it is) of the way between the captured moment and now, for drawing; `dt` is the length of a tick.
    pub fn apply(&self, world: &mut World, alpha: f32, dt: f32) -> Undo {
        let mut undo = Undo::default();
        if !self.valid || !(alpha > 0.0 && alpha < 1.0) || !alpha.is_finite() || world.len().abs_diff(self.entities) > self.entities / 4 + 8 {
            return undo;
        }
        let a = alpha;
        let _ = world.for_each_uid::<(&mut Transform,), _>(|_, uid, (t,)| {
            let Some(p) = self.transforms.get(&uid) else { return };
            if p.pos == t.pos && p.rot == t.rot && p.scale == t.scale {
                return;
            }
            let d = ((p.pos[0] - t.pos[0]).powi(2) + (p.pos[1] - t.pos[1]).powi(2) + (p.pos[2] - t.pos[2]).powi(2)).sqrt();
            if !(d < TELEPORT) {
                return;
            }
            undo.transforms.push((uid, *t));
            for k in 0..3 {
                t.pos[k] = p.pos[k] + (t.pos[k] - p.pos[k]) * a;
                t.scale[k] = p.scale[k] + (t.scale[k] - p.scale[k]) * a;
            }
            t.rot = nlerp(p.rot, t.rot, a);
        });
        let _ = world.for_each_uid::<(&mut Animator,), _>(|_, uid, (an,)| {
            let Some(p) = self.anims.get(&uid) else { return };
            // the same clip playing on: its clock is blended; a switch or a jump of the time (motion matching) is shown as it is
            if an.clip.0 != p.clip || !an.playing {
                return;
            }
            let moved = an.time - p.time;
            if moved >= 0.0 && moved <= 3.0 * dt * an.speed.abs().max(1.0) + 0.02 {
                undo.times.push((uid, an.time));
                an.time = p.time + moved * a;
            }
        });
        let _ = world.for_each_uid::<(&mut Inertia,), _>(|_, uid, (i,)| {
            let Some(p) = self.anims.get(&uid) else { return };
            let n = (i.n as usize).min(i.gap.len());
            if n == 0 || n as u32 != p.n {
                return;
            }
            let gap = i.gap[n - 1];
            let moved = gap - p.gap;
            if moved >= 0.0 && moved <= 3.0 * dt + 0.02 {
                undo.gaps.push((uid, gap));
                i.gap[n - 1] = p.gap + moved * a;
            }
        });
        undo
    }

    /// Put back what `apply` changed.
    pub fn restore(&self, world: &mut World, undo: Undo) {
        for (uid, t) in undo.transforms {
            if let Some(e) = world.entity(uid) {
                if let Some(c) = world.get_mut::<Transform>(e) {
                    *c = t;
                }
            }
        }
        for (uid, time) in undo.times {
            if let Some(e) = world.entity(uid) {
                if let Some(c) = world.get_mut::<Animator>(e) {
                    c.time = time;
                }
            }
        }
        for (uid, gap) in undo.gaps {
            if let Some(e) = world.entity(uid) {
                if let Some(c) = world.get_mut::<Inertia>(e) {
                    let n = (c.n as usize).min(c.gap.len());
                    if n > 0 {
                        c.gap[n - 1] = gap;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_json::{parse, Json};
    use lunaris_sim::{Command, Sim};

    fn spawn(s: &mut Sim, uid: u64, comps: &str) {
        let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    }

    fn pos(s: &Sim, uid: u64) -> [f32; 3] {
        let e = s.world.entity(uid).unwrap();
        s.world.get::<Transform>(e).unwrap().pos
    }

    #[test]
    fn a_picture_between_ticks_blends_what_moved_and_puts_everything_back() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]}}"#);
        spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]}}"#);
        spawn(&mut s, 3, r#"{"Transform":{"pos":[5,5,5]},"Animator":{"clip":"a"},"Inertia":{}}"#);
        s.step();
        let mut i = Interp::default();
        i.capture(&mut s.world);
        let e1 = s.world.entity(1).unwrap();
        let e2 = s.world.entity(2).unwrap();
        s.world.get_mut::<Transform>(e1).unwrap().pos = [1.0, 0.0, 0.0];
        s.world.get_mut::<Transform>(e2).unwrap().pos = [100.0, 0.0, 0.0]; // a teleport
        let e3 = s.world.entity(3).unwrap();
        let (t0, g0) = {
            let a = s.world.get::<Animator>(e3).unwrap();
            let n = (s.world.get::<Inertia>(e3).unwrap().n as usize).max(1);
            (a.time, s.world.get::<Inertia>(e3).unwrap().gap[n - 1])
        };
        s.world.get_mut::<Animator>(e3).unwrap().time += 1.0 / 60.0;
        {
            let c = s.world.get_mut::<Inertia>(e3).unwrap();
            c.n = c.n.max(1);
            c.gap[(c.n as usize) - 1] = g0 + 1.0 / 60.0;
        }
        let hash = s.state_hash();
        let undo = i.apply(&mut s.world, 0.25, 1.0 / 60.0);
        assert!((pos(&s, 1)[0] - 0.25).abs() < 1e-6, "a quarter of the way: {:?}", pos(&s, 1));
        assert_eq!(pos(&s, 2)[0], 100.0, "a teleport is shown as it is");
        let a = s.world.get::<Animator>(e3).unwrap();
        assert!((a.time - (t0 + 0.25 / 60.0)).abs() < 1e-6, "the animation clock is blended too: {} from {t0}", a.time);
        i.restore(&mut s.world, undo);
        assert_eq!(pos(&s, 1)[0], 1.0, "the world is as the simulation left it");
        assert_eq!(s.state_hash(), hash, "restored exactly");
        // nothing to blend at 0 or 1
        let undo = i.apply(&mut s.world, 1.0, 1.0 / 60.0);
        assert!(undo.transforms.is_empty());
    }
}
