//! Built-in systems. A system is a pure function of (world, ctx): all state lives in the world, so a simulation can be
//! cloned, snapshotted and replayed exactly. Rules for deterministic systems: no wall-clock time, no thread-dependent
//! order, no order-dependent floating point accumulation across entities, and per-entity randomness through
//! `ctx.entity_rand(uid, salt)`.
use crate::components::*;
use crate::sim::SimCtx;
use crate::components::PhysicsSettings;
use crate::rng::hash_rand;
use lunaris_ecs::{Uid, World};

#[inline]
fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

#[inline]
fn quat_rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    // v + 2 w (u x v) + 2 u x (u x v), with u the vector part: only multiplications and additions, so every machine agrees
    let u = [q[0], q[1], q[2]];
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = cross(u, v);
    let t = [2.0 * t[0], 2.0 * t[1], 2.0 * t[2]];
    let c = cross(u, t);
    [v[0] + q[3] * t[0] + c[0], v[1] + q[3] * t[1] + c[1], v[2] + q[3] * t[2] + c[2]]
}

/// Attached entities (`Parent` + `LocalTransform` + `Transform`) take their world `Transform` from their parent's: position
/// `parent.pos + parent.rot * (parent.scale * local.pos)`, rotation `parent.rot * local.rot`, scale `parent.scale * local.scale`.
/// Parents are handled before their children (a chain of any depth works), so a whole tree is consistent in one pass. An entity
/// whose parent does not exist, or that is part of a cycle, is left where it is. Cheap when nothing is attached.
pub fn hierarchy(world: &mut World) {
    let mut kids: Vec<(u64, lunaris_ecs::Entity, u64, LocalTransform)> = Vec::new();
    world
        .for_each_uid::<(&Parent, &LocalTransform), _>(|e, uid, (p, l)| {
            if p.target.0 != 0 {
                kids.push((uid, e, p.target.0, *l));
            }
        })
        .expect("hierarchy query is valid");
    if kids.is_empty() {
        return;
    }
    kids.sort_by_key(|k| k.0);
    // depth = number of attached ancestors; a chain that does not end is a cycle
    let index: std::collections::HashMap<u64, usize> = kids.iter().enumerate().map(|(i, k)| (k.0, i)).collect();
    let mut order: Vec<(usize, usize)> = Vec::with_capacity(kids.len()); // (depth, index into kids)
    for (i, k) in kids.iter().enumerate() {
        let (mut depth, mut parent) = (0usize, k.2);
        let mut cyclic = false;
        while let Some(&pi) = index.get(&parent) {
            depth += 1;
            if depth > kids.len() {
                cyclic = true;
                break;
            }
            parent = kids[pi].2;
        }
        if !cyclic {
            order.push((depth, i));
        }
    }
    order.sort_unstable();
    for (_, i) in order {
        let (_, entity, parent_uid, local) = kids[i];
        let Some(pe) = world.entity(parent_uid) else { continue };
        let Some(pt) = world.get::<Transform>(pe).copied() else { continue };
        let scaled = [pt.scale[0] * local.pos[0], pt.scale[1] * local.pos[1], pt.scale[2] * local.pos[2]];
        let r = quat_rotate(pt.rot, scaled);
        let mut q = quat_mul(pt.rot, local.rot);
        let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        if n > 0.0 {
            let inv = 1.0 / n;
            q = [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv];
        }
        if let Some(t) = world.get_mut::<Transform>(entity) {
            t.pos = [pt.pos[0] + r[0], pt.pos[1] + r[1], pt.pos[2] + r[2]];
            t.rot = q;
            t.scale = [pt.scale[0] * local.scale[0], pt.scale[1] * local.scale[1], pt.scale[2] * local.scale[2]];
        }
    }
}

fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

/// Particles: age and retire the ones that exist (gravity, colour and size along their life), then let every `Emitter` throw new ones.
/// Emitters are visited in uid order and every random number is `hash_rand(seed, tick, emitter, index * 4 + k)`, so the result does not
/// depend on how the world is stored.
pub fn particles(world: &mut World, ctx: &mut SimCtx) {
    let dt = ctx.dt;
    // ---- existing particles
    let mut dead: Vec<u64> = Vec::new();
    let mut alive: std::collections::BTreeMap<u64, u32> = std::collections::BTreeMap::new();
    world
        .for_each_uid::<(&mut Particle, &mut Transform, &mut Velocity), _>(|_, uid, (p, t, v)| {
            p.age += dt;
            if p.age >= p.life {
                dead.push(uid);
                return;
            }
            *alive.entry(p.emitter.0).or_insert(0) += 1;
            v.lin[0] += p.gravity[0] * dt;
            v.lin[1] += p.gravity[1] * dt;
            v.lin[2] += p.gravity[2] * dt;
            let k = (p.age / p.life).clamp(0.0, 1.0);
            let size = p.size0 + (p.size1 - p.size0) * k;
            t.scale = [size, size, size];
        })
        .expect("particle query is valid");
    world
        .for_each::<(&Particle, &mut Sprite), _>(|_, (p, s)| s.color = lerp4(p.color0, p.color1, (p.age / p.life).clamp(0.0, 1.0)))
        .expect("particle sprite query is valid");
    world
        .for_each::<(&Particle, &mut MeshRenderer), _>(|_, (p, m)| m.color = lerp4(p.color0, p.color1, (p.age / p.life).clamp(0.0, 1.0)))
        .expect("particle mesh query is valid");
    // ---- particles with an `EmitterFx`: drag, turbulence, a floor, a middle colour and size
    {
        let (seed, tick) = (ctx.seed, ctx.tick);
        let mut fx_dead: Vec<u64> = Vec::new();
        world
            .for_each_uid::<(&Particle, &ParticleFx, &mut Transform, &mut Velocity), _>(|_, uid, (p, fx, t, v)| {
                if p.age >= p.life {
                    return;
                }
                if fx.drag > 0.0 {
                    let k = 1.0 / (1.0 + fx.drag * dt);
                    v.lin = [v.lin[0] * k, v.lin[1] * k, v.lin[2] * k];
                }
                if fx.turbulence != 0.0 {
                    for axis in 0..3 {
                        v.lin[axis] += (hash_rand(seed, tick, uid, 100 + axis as u64) * 2.0 - 1.0) * fx.turbulence * dt;
                    }
                }
                if fx.floor && t.pos[1] + v.lin[1] * dt <= fx.floor_y && v.lin[1] <= 0.0 {
                    if fx.stick {
                        fx_dead.push(uid);
                    } else {
                        t.pos[1] = fx.floor_y;
                        v.lin[1] = -v.lin[1] * fx.bounce.clamp(0.0, 1.0);
                        v.lin[0] *= 0.8;
                        v.lin[2] *= 0.8;
                    }
                }
                if fx.mid > 0.0 && fx.mid < 1.0 {
                    let k = (p.age / p.life).clamp(0.0, 1.0);
                    let size = if k < fx.mid { p.size0 + (fx.size_mid - p.size0) * (k / fx.mid) } else { fx.size_mid + (p.size1 - fx.size_mid) * ((k - fx.mid) / (1.0 - fx.mid)) };
                    t.scale = [size, size, size];
                }
            })
            .expect("particle fx query is valid");
        let mid_color = |p: &Particle, fx: &ParticleFx| {
            let k = (p.age / p.life).clamp(0.0, 1.0);
            if k < fx.mid { lerp4(p.color0, fx.color_mid, k / fx.mid) } else { lerp4(fx.color_mid, p.color1, (k - fx.mid) / (1.0 - fx.mid)) }
        };
        world.for_each::<(&Particle, &ParticleFx, &mut Sprite), _>(|_, (p, fx, s)| {
            if fx.mid > 0.0 && fx.mid < 1.0 {
                s.color = mid_color(p, fx);
            }
        }).expect("particle fx sprite query is valid");
        world.for_each::<(&Particle, &ParticleFx, &mut MeshRenderer), _>(|_, (p, fx, m)| {
            if fx.mid > 0.0 && fx.mid < 1.0 {
                m.color = mid_color(p, fx);
            }
        }).expect("particle fx mesh query is valid");
        dead.extend(fx_dead);
    }
    for uid in dead {
        if let Some(e) = world.entity(uid) {
            world.despawn(e);
        }
    }
    // ---- emitters
    let mut emitters: Vec<(u64, Emitter, Transform)> = Vec::new();
    world.for_each_uid::<(&Emitter, &Transform), _>(|_, uid, (e, t)| emitters.push((uid, *e, *t))).expect("emitter query is valid");
    if emitters.is_empty() {
        return;
    }
    emitters.sort_by_key(|e| e.0);
    let (seed, tick) = (ctx.seed, ctx.tick);
    let ids: Option<[lunaris_ecs::ComponentId; 4]> = (|| Some([world.component_id("Transform")?, world.component_id("Velocity")?, world.component_id("Particle")?, world.component_id("Sprite")?]))();
    let mesh_id = world.component_id("MeshRenderer");
    let Some([c_transform, c_velocity, c_particle, c_sprite]) = ids else { return };
    for (uid, em, tr) in emitters {
        let mut carry = em.carry;
        let mut n = 0u32;
        if em.active {
            carry += em.rate.max(0.0) * dt;
            n = carry.floor() as u32;
            carry -= n as f32;
            n = n.saturating_add(em.burst);
        }
        let room = em.max.saturating_sub(alive.get(&uid).copied().unwrap_or(0));
        let n = n.min(room).min(2000);
        let dir = {
            let d = em.dir;
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if len > 1e-6 { [d[0] / len, d[1] / len, d[2] / len] } else { [0.0, 1.0, 0.0] }
        };
        // two axes perpendicular to `dir`
        let helper = if dir[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
        let a = {
            let c = [dir[1] * helper[2] - dir[2] * helper[1], dir[2] * helper[0] - dir[0] * helper[2], dir[0] * helper[1] - dir[1] * helper[0]];
            let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
            [c[0] / l, c[1] / l, c[2] / l]
        };
        let b = [dir[1] * a[2] - dir[2] * a[1], dir[2] * a[0] - dir[0] * a[2], dir[0] * a[1] - dir[1] * a[0]];
        let cone = (em.spread.clamp(0.0, 1.0) as f64) * crate::detmath::PI;
        let cos_cone = crate::detmath::cos(cone) as f32;
        let mesh_id = if em.mesh.0 != 0 { mesh_id } else { None };
        let fx: Option<EmitterFx> = world.entity(uid).and_then(|e| world.get::<EmitterFx>(e)).copied();
        for i in 0..n {
            let r = |k: u64| hash_rand(seed, tick, uid, i as u64 * 4 + k);
            // a direction uniformly distributed inside the cone
            let cos_t = 1.0 - r(0) * (1.0 - cos_cone);
            let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
            let phi = r(1) as f64 * 2.0 * crate::detmath::PI;
            let (cp, sp) = (crate::detmath::cos(phi) as f32, crate::detmath::sin(phi) as f32);
            let d = [dir[0] * cos_t + (a[0] * cp + b[0] * sp) * sin_t, dir[1] * cos_t + (a[1] * cp + b[1] * sp) * sin_t, dir[2] * cos_t + (a[2] * cp + b[2] * sp) * sin_t];
            let speed = em.speed * (1.0 + (r(2) * 2.0 - 1.0) * em.speed_var.clamp(0.0, 1.0));
            let life = (em.life * (1.0 + (r(3) * 2.0 - 1.0) * em.life_var.clamp(0.0, 1.0))).max(0.01);
            let (mut start, mut ang) = (tr.pos, [0.0f32; 3]);
            if let Some(f) = &fx {
                let rx = |k: u64| hash_rand(seed, tick, uid.wrapping_add(0x5EED_0000_0000), i as u64 * 8 + k);
                let off = match f.shape {
                    1 => {
                        // inside a sphere: a point of the cube that falls inside it (a few tries), else its direction on the surface
                        let mut p = [0.0f32; 3];
                        let mut found = false;
                        for t in 0..6u64 {
                            p = [rx(t * 3) * 2.0 - 1.0, rx(t * 3 + 1) * 2.0 - 1.0, rx(t * 3 + 2) * 2.0 - 1.0];
                            if p[0] * p[0] + p[1] * p[1] + p[2] * p[2] <= 1.0 {
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt().max(1e-6);
                            p = [p[0] / l, p[1] / l, p[2] / l];
                        }
                        [p[0] * f.extent[0], p[1] * f.extent[0], p[2] * f.extent[0]]
                    }
                    2 => [(rx(0) * 2.0 - 1.0) * f.extent[0], (rx(1) * 2.0 - 1.0) * f.extent[1], (rx(2) * 2.0 - 1.0) * f.extent[2]],
                    3 => {
                        let (rr, ph) = (rx(0).sqrt() * f.extent[0], rx(1) as f64 * 2.0 * crate::detmath::PI);
                        [rr * crate::detmath::cos(ph) as f32, 0.0, rr * crate::detmath::sin(ph) as f32]
                    }
                    _ => [0.0; 3],
                };
                start = [tr.pos[0] + off[0], tr.pos[1] + off[1], tr.pos[2] + off[2]];
                if f.spin != 0.0 {
                    ang = [0.0, 0.0, (rx(7) * 2.0 - 1.0) * f.spin];
                }
            }
            let t = Transform { pos: start, rot: [0.0, 0.0, 0.0, 1.0], scale: [em.size0, em.size0, em.size0] };
            let v = Velocity { lin: [d[0] * speed, d[1] * speed, d[2] * speed], ang };
            let p = Particle { age: 0.0, life, size0: em.size0, size1: em.size1, color0: em.color0, color1: em.color1, gravity: em.gravity, emitter: Uid(uid) };
            let sprite = Sprite { color: em.color0, texture: em.texture, ..Sprite::default() };
            let mesh = MeshRenderer { mesh: em.mesh, color: em.color0, ..MeshRenderer::default() };
            // SAFETY: each pointer references a live value of exactly the component's type.
            let spawned = unsafe {
                match mesh_id {
                    Some(c_mesh) => world.spawn_raw(None, &[(c_transform, (&t as *const Transform).cast()), (c_velocity, (&v as *const Velocity).cast()), (c_particle, (&p as *const Particle).cast()), (c_mesh, (&mesh as *const MeshRenderer).cast())]),
                    None => world.spawn_raw(None, &[(c_transform, (&t as *const Transform).cast()), (c_velocity, (&v as *const Velocity).cast()), (c_particle, (&p as *const Particle).cast()), (c_sprite, (&sprite as *const Sprite).cast())]),
                }
            };
            match spawned {
                Ok(pe) => {
                    if let Some(f) = &fx {
                        let _ = world.insert(pe, ParticleFx { drag: f.drag, turbulence: f.turbulence, floor: f.floor, floor_y: f.floor_y, bounce: f.bounce, stick: f.stick, mid: f.mid, color_mid: f.color_mid, size_mid: f.size_mid });
                    }
                }
                Err(_) => break,
            }
        }
        if let Some(e) = world.entity(uid) {
            if let Some(em_mut) = world.get_mut::<Emitter>(e) {
                em_mut.carry = carry;
                em_mut.burst = 0;
            }
        }
    }
}

/// Advance the clock of every `Animator` that is playing. The pose itself is worked out by the renderer from `time`, so this is all
/// the simulation needs to do (and it is deterministic and replay-exact like any other system).
pub fn animate(world: &mut World, ctx: &mut SimCtx) {
    let dt = ctx.dt;
    // animators with an `Inertia` notice their switches before the clocks move (and do not crossfade)
    crate::inertia::begin(world, dt);
    world
        .for_each::<(&mut Animator,), _>(|_, (a,)| {
            if !a.started {
                a.started = true;
                a.last_clip = a.clip;
                a.blend = 1.0;
            } else if a.last_clip.0 != a.clip.0 {
                // the clip was changed: keep the old one playing underneath and fade to the new one
                if a.fade > 0.0 && a.fade.is_finite() {
                    // (a change while still fading starts the new fade from the clip that was showing)
                    a.prev_clip = a.last_clip;
                    a.prev_time = if a.blend >= 1.0 { a.last_time } else { a.prev_time };
                    a.blend = 0.0;
                } else {
                    a.blend = 1.0;
                }
                a.last_clip = a.clip;
            }
            if a.playing && a.speed.is_finite() {
                a.time += a.speed * dt;
                a.prev_time += a.speed * dt;
                // keep the number small and exact enough: a day of play must not lose the fraction of a second
                if a.time > 1.0e6 || a.time < -1.0e6 {
                    a.time = 0.0;
                }
                if a.prev_time > 1.0e6 || a.prev_time < -1.0e6 {
                    a.prev_time = 0.0;
                }
            }
            if a.blend < 1.0 {
                a.blend = if a.fade > 0.0 { (a.blend + dt / a.fade).min(1.0) } else { 1.0 };
            }
            a.last_time = a.time;
        })
        .expect("animator query is valid");
}

/// Buttons: from the mouse resources (`input.mouse_x`, `input.mouse_y` as fractions of the picture, `input.mouse_left`), set `hover`,
/// `pressed` and (for one tick) `clicked` on every visible `Ui` that is a `button`. Runs before the graph systems, so a system that
/// reacts to `Ui.clicked` sees it in the tick the mouse was released.
pub fn ui(world: &mut World, _ctx: &mut SimCtx) {
    let (mx, my) = (world.resource("input.mouse_x"), world.resource("input.mouse_y"));
    let down = world.resource("input.mouse_left").is_some_and(|v| v > 0.5);
    let pointer = mx.zip(my).filter(|(x, y)| x.is_finite() && y.is_finite());
    world
        .for_each::<(&mut Ui,), _>(|_, (u,)| {
            if !u.button {
                return;
            }
            let inside = u.visible
                && pointer.is_some_and(|(x, y)| {
                    let (x, y) = (x as f32, y as f32);
                    x >= u.pos[0] && x <= u.pos[0] + u.size[0] && y >= u.pos[1] && y <= u.pos[1] + u.size[1]
                });
            let was_pressed = u.pressed;
            u.hover = inside;
            u.pressed = inside && down;
            u.clicked = was_pressed && inside && !down;
        })
        .expect("ui query is valid");
}

/// Integrate position by linear velocity and rotation by angular velocity.
pub fn movement(world: &mut World, ctx: &mut SimCtx) {
    let dt = ctx.dt;
    // a character with a `CharacterMotion` is moved by the character controller (a `Steer` on it sets its walk from this velocity): moving it here too would be a second push
    let mut walkers: Vec<u64> = Vec::new();
    let _ = world.for_each_uid::<(&crate::components::CharacterMotion,), _>(|_, uid, _| walkers.push(uid));
    walkers.sort_unstable();
    world
        .par_for_each_uid::<(&mut Transform, &Velocity), _>(ctx.threads, |_, uid, (t, v)| {
            if !walkers.is_empty() && walkers.binary_search(&uid).is_ok() {
                return;
            }
            t.pos[0] += v.lin[0] * dt;
            t.pos[1] += v.lin[1] * dt;
            t.pos[2] += v.lin[2] * dt;
            if v.ang[0] != 0.0 || v.ang[1] != 0.0 || v.ang[2] != 0.0 {
                let w = [v.ang[0], v.ang[1], v.ang[2], 0.0];
                let dq = quat_mul(w, t.rot);
                let mut q = [t.rot[0] + 0.5 * dt * dq[0], t.rot[1] + 0.5 * dt * dq[1], t.rot[2] + 0.5 * dt * dq[2], t.rot[3] + 0.5 * dt * dq[3]];
                let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
                if n > 0.0 {
                    let inv = 1.0 / n;
                    q = [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv];
                    t.rot = q;
                }
            }
        })
        .expect("movement query is valid");
}

/// Nudge velocities randomly (per entity, independent of storage order).
pub fn wander(world: &mut World, ctx: &mut SimCtx) {
    let dt = ctx.dt;
    let (seed, tick) = (ctx.seed, ctx.tick);
    world
        .par_for_each_uid::<(&mut Velocity, &Wander), _>(ctx.threads, |_, uid, (v, w)| {
            for axis in 0..3 {
                let r = crate::rng::hash_rand(seed, tick, uid, axis as u64) - 0.5;
                v.lin[axis] += r * 2.0 * w.strength * dt;
            }
        })
        .expect("wander query is valid");
}

/// Rigid-body physics (see `physics`). Gravity comes from the world's `PhysicsSettings` entity with the lowest uid.
thread_local! {
    /// While a profile runs: the milliseconds the parts of `physics` took since the profile took them (`PHYSICS_PARTS`).
    static PHYSICS_MS: std::cell::RefCell<[f64; 4]> = const { std::cell::RefCell::new([0.0; 4]) };
    /// The same for the parts of the reference step of the bodies (`physics::REF_STAGES`).
    static PHYSICS_REF_MS: std::cell::RefCell<[f64; 12]> = const { std::cell::RefCell::new([0.0; 12]) };
}

pub(crate) fn take_physics_ref_ms() -> [f64; 12] {
    PHYSICS_REF_MS.with(|m| std::mem::take(&mut *m.borrow_mut()))
}

/// What `PHYSICS_MS` counts: the bodies' step, the raycast vehicles, the probes, and the settings and the rest.
pub(crate) const PHYSICS_PARTS: [&str; 4] = ["bodies", "vehicles", "probes", "settings"];

pub(crate) fn take_physics_ms() -> [f64; 4] {
    PHYSICS_MS.with(|m| std::mem::take(&mut *m.borrow_mut()))
}

pub fn physics(world: &mut World, ctx: &mut SimCtx) {
    let timed = crate::vehicle_ray::timing_on();
    let mut clock = timed.then(std::time::Instant::now);
    let mut lap = |part: usize| {
        if let Some(t) = clock.as_mut() {
            let now = std::time::Instant::now();
            PHYSICS_MS.with(|m| m.borrow_mut()[part] += now.duration_since(*t).as_secs_f64() * 1e3);
            *t = now;
        }
    };
    if timed {
        crate::physics::reference_profile(true);
    }
    let mut gravity = [0.0f32, -9.8, 0.0];
    let mut lowest = u64::MAX;
    world
        .for_each_uid::<(&PhysicsSettings,), _>(|_, uid, (p,)| {
            if uid < lowest {
                lowest = uid;
                gravity = p.gravity;
            }
        })
        .expect("physics settings query is valid");
    ctx.gravity = gravity;
    lap(3);
    ctx.contacts = if ctx.reference_physics {
        crate::physics::step_reference(world, gravity, ctx.dt, &ctx.terrains, &ctx.meshes, &mut ctx.contact_memory, &mut ctx.sleep)
    } else {
        crate::physics::step(world, gravity, ctx.dt, &ctx.terrains, &ctx.meshes, &ctx.physics, ctx.threads, &mut ctx.contact_memory, &mut ctx.sleep)
    };
    lap(0);
    if timed {
        let ns = crate::physics::reference_stage_ns();
        PHYSICS_REF_MS.with(|m| {
            let mut m = m.borrow_mut();
            for k in 0..12 {
                m[k] += ns[k] as f64 * 1e-6;
            }
        });
    }
    // raycast vehicles push their chassis with what the wheels meet (a world without a `Vehicle` finds none and goes on)
    crate::vehicle::step(world, gravity, ctx.dt, &ctx.terrains, &ctx.meshes);
    lap(1);
    // what the entities with a `Probe` can see of the ground, the water and one ray, for the graph systems that run next
    crate::probe::step(world, &ctx.terrains, &ctx.meshes);
    lap(2);
}
