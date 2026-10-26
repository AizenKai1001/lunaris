//! Inertialization: the way Unreal's animation system (and the Game Animation Sample) hides the switch from one clip to another. A crossfade shows both clips at once for a while, so two moving poses
//! are averaged (feet slide, limbs ghost); inertialization shows only the new clip and instead *offsets* it by the difference between the pose the old clip had at the switch and the new one's first
//! pose, and between their velocities, and lets that offset die away smoothly (a quintic Hermite curve: no jump, no jerk, at rest when it ends). A character that is switched many times a second by
//! motion matching therefore keeps the smooth, continuous motion of whatever it was doing.
//!
//! The renderer's pose is a pure function of the world, so the world has to hold what the offset is made of: an `Inertia` on the entity of an `Animator` keeps the chain of clips the animator has
//! played lately (the clip and time each started at, how long each played, how long each offset takes to decay). `animate` (see `systems.rs`) keeps it up to date: a change of `Animator.clip`, or a
//! jump of `Animator.time` (a graph system or motion matching that sets it), is a switch. `Rig::chain_local` (`crates/lunaris-render/src/rig.rs`) works the pose out from it.
use crate::components::Animator;
use lunaris_ecs::{component, World};

/// The most entries of the chain (the same as the renderer's).
pub const MAX: usize = 6;

component! {
    /// Turns the crossfade of the entity's `Animator` into **inertialization** (see `docs/GAMEPLAY.md`, "Inertialization"): when `Animator.clip` changes or `Animator.time` jumps, the new clip is shown
    /// at once, offset by what separated it from the old pose, and the offset decays over `span` seconds (`Animator.fade` is not used). The other fields are the engine's bookkeeping: the clips the
    /// animator has played lately (`n` of them: `clip` as names, `t0` the time each started at, `rate` clip seconds per second, `gap` how long each played, `len` the span of the switch into it, `looping`
    /// whether it looped: an older clip is posed as it was played, a loop that has gone past its end wraps round and one that does not holds its last pose).
    pub struct Inertia {
        pub span: f32 = 0.25,
        pub n: u32 = 0,
        pub clip: [u32; 6] = [0; 6],
        pub t0: [f32; 6] = [0.0; 6],
        pub rate: [f32; 6] = [1.0; 6],
        pub gap: [f32; 6] = [0.0; 6],
        pub len: [f32; 6] = [0.0; 6],
        pub looping: [u32; 6] = [1; 6],
    }
}

/// Called by `animate` before it moves the clocks: notice the switches of every animator that has an `Inertia` and advance its chain by `dt`.
pub fn begin(world: &mut World, dt: f32) {
    sync(world, dt, true);
}

/// Called at the end of a tick, after everything that may set `Animator.clip` or `Animator.time` (motion matching does, after `animate` has run): the switch is noticed at once, so that the picture of this very
/// tick already has its offset (otherwise the new clip would show as it is for one picture, a pop, before the next tick's `animate` made the offset).
pub fn notice(world: &mut World) {
    sync(world, 0.0, false);
}

fn sync(world: &mut World, dt: f32, advance: bool) {
    let _ = world.for_each::<(&mut Animator, &mut Inertia), _>(|_, (a, ch)| {
        if advance {
            // no crossfade: the offset does the blending
            a.fade = 0.0;
            a.blend = 1.0;
        }
        let n = (ch.n as usize).min(MAX);
        if !a.started || n == 0 {
            if !advance {
                return;
            }
            ch.n = 1;
            ch.clip[0] = a.clip.0;
            ch.t0[0] = a.time;
            ch.rate[0] = a.speed;
            ch.gap[0] = 0.0;
            ch.len[0] = 0.0;
            ch.looping[0] = a.looping as u32;
            // (`animate` marks the animator as started right after; the next call carries on from this entry)
            return;
        }
        let switched = a.last_clip.0 != a.clip.0 || !((a.time - a.last_time).abs() <= 1.0e-3);
        let mut n = n;
        if switched && a.time.is_finite() {
            // the entry that was playing: how fast its clip actually went
            let k = n - 1;
            ch.rate[k] = if ch.gap[k] > 1.0e-5 { (a.last_time - ch.t0[k]) / ch.gap[k] } else { a.speed };
            if n == MAX {
                for i in 1..MAX {
                    ch.clip[i - 1] = ch.clip[i];
                    ch.t0[i - 1] = ch.t0[i];
                    ch.rate[i - 1] = ch.rate[i];
                    ch.gap[i - 1] = ch.gap[i];
                    ch.len[i - 1] = ch.len[i];
                    ch.looping[i - 1] = ch.looping[i];
                }
                n -= 1;
            }
            ch.clip[n] = a.clip.0;
            ch.t0[n] = a.time;
            ch.rate[n] = a.speed;
            ch.gap[n] = 0.0;
            ch.len[n] = if ch.span.is_finite() { ch.span.clamp(0.0, 5.0) } else { 0.25 };
            ch.looping[n] = a.looping as u32;
            n += 1;
            if !advance {
                // handled: the next `animate` must not see the same switch again
                a.last_clip = a.clip;
                a.last_time = a.time;
            }
        }
        if advance {
            // the entry playing now has played another step
            if dt.is_finite() && dt > 0.0 {
                ch.gap[n - 1] += dt;
            }
            // its offset has died away: the chain before it no longer matters
            if n > 1 && ch.gap[n - 1] >= ch.len[n - 1] {
                ch.clip[0] = ch.clip[n - 1];
                ch.t0[0] = ch.t0[n - 1];
                ch.rate[0] = ch.rate[n - 1];
                ch.gap[0] = ch.gap[n - 1];
                ch.len[0] = 0.0;
                ch.looping[0] = ch.looping[n - 1];
                n = 1;
            }
        }
        ch.n = n as u32;
    });
}
