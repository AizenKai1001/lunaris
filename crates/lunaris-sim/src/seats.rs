//! Seats (docs/SEATS.md): a place in a vehicle that a character sits in, and what the character's hands then fly. A **`Seat`** is on the vehicle (or on a child entity of it for a second seat) next to an `Interactable`
//! (kind 0, "Enter"): a character's `Interactor` that uses it sits down, is carried with the vehicle and is given back its legs by the use button again. The seat that is the pilot's (`drive` 1) flies the entity's
//! `Helicopter` from the sitter's **`Controls`** (four axes and some buttons: written by a game's graph system from the keys, or by the game server from what the player's computer says). The sitter has a
//! **`Seated`** component for as long as it sits: the game stops its walking, the game server stops believing the place it reports.
//!
//! The step runs inside the gameplay blocks right after the use button has been looked at (`interact`), visits the seats in uid order and the characters' use buttons in uid order, and writes only the sitter's place and
//! velocity, the helicopter's inputs and the seat's own bookkeeping, so it is deterministic. A world without a `Seat` pays for one query.
use crate::components::{Controls, Helicopter, HitPoints, Interactable, Interactor, Parent, Seat, Seated, Transform, Vehicle, Velocity};
use crate::physics_rot::{axes_of, normalize_q, rotate};
use lunaris_ecs::{Entity, Uid, World};
use std::collections::BTreeSet;

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

#[inline]
fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

/// A point given in an entity's own axes, in the world.
fn to_world(t: &Transform, local: V3) -> V3 {
    let r = rotate(&axes_of(normalize_q(t.rot)), [finite(local[0], 0.0), finite(local[1], 0.0), finite(local[2], 0.0)]);
    [t.pos[0] + r[0], t.pos[1] + r[1], t.pos[2] + r[2]]
}

/// What one character's use button is doing: who it belongs to (the entity itself, or what its entity hangs on), whether it is pressed and what it used this tick.
struct Hand {
    owner: u64,
    pressed: bool,
    used: u64,
}

pub fn update(world: &mut World, events: &mut Events) {
    let mut seats: Vec<(u64, Entity, Seat)> = Vec::new();
    let _ = world.for_each_uid::<(&Seat,), _>(|e, uid, (s,)| seats.push((uid, e, *s)));
    if seats.is_empty() {
        return;
    }
    seats.sort_by_key(|s| s.0);
    let mut raw: Vec<(u64, Entity, Interactor)> = Vec::new();
    let _ = world.for_each_uid::<(&Interactor,), _>(|e, uid, (i,)| raw.push((uid, e, *i)));
    raw.sort_by_key(|r| r.0);
    let hands: Vec<Hand> = raw.iter().map(|(uid, e, i)| Hand { owner: world.get::<Parent>(*e).map_or(*uid, |p| p.target.0), pressed: i.act, used: i.used.0 }).collect();
    let mut seated: BTreeSet<u64> = seats.iter().filter(|s| s.2.occupant.0 != 0).map(|s| s.2.occupant.0).collect();

    for (suid, se, mut seat) in seats {
        let was = seat;
        let Some(vt) = world.get::<Transform>(se).copied() else { continue };
        if seat.occupant.0 != 0 {
            let occ = seat.occupant.0;
            let oe = world.entity(occ);
            let alive = oe.is_some_and(|oe| world.get::<HitPoints>(oe).is_none_or(|h| !h.dead && h.value > 0.0));
            let pressed = hands.iter().find(|h| h.owner == occ).is_some_and(|h| h.pressed);
            let mut leave = !alive;
            if alive && pressed && !seat.held {
                // (the use button, pressed again: get out; not from a helicopter that is up in the air)
                let high = seat.drive == 1 && world.get::<Helicopter>(se).is_some_and(|h| h.altitude < 0.0 || h.altitude > finite(seat.max_exit_altitude, 3.0));
                let fast = seat.drive == 2 && world.get::<Vehicle>(se).is_some_and(|v| v.speed.abs() > finite(seat.max_exit_speed, 4.0));
                if high || fast {
                    events.push(("seat_denied".to_string(), suid, Some(vt.pos)));
                } else {
                    leave = true;
                }
            }
            seat.held = pressed;
            if leave {
                if let Some(oe) = oe {
                    if alive {
                        if let Some(t) = world.get_mut::<Transform>(oe) {
                            t.pos = to_world(&vt, seat.exit);
                        }
                    }
                    if let Some(v) = world.get_mut::<Velocity>(oe) {
                        v.lin = [0.0; 3];
                    }
                    let _ = world.remove_typed::<Seated>(oe);
                }
                seated.remove(&occ);
                seat.occupant = Uid(0);
                seat.held = false;
                events.push(("seat_exit".to_string(), suid, Some(vt.pos)));
                if seat.drive == 1 {
                    if let Some(h) = world.get_mut::<Helicopter>(se) {
                        h.pilot = Uid(0);
                        h.collective = 0.0;
                        h.pitch = 0.0;
                        h.roll = 0.0;
                        h.yaw = 0.0;
                        h.engine = false;
                    }
                }
                if seat.drive == 2 {
                    // (left parked)
                    if let Some(v) = world.get_mut::<Vehicle>(se) {
                        v.throttle = 0.0;
                        v.steer = 0.0;
                        v.brake = 0.0;
                        v.handbrake = 1.0;
                    }
                }
            } else if let Some(oe) = oe {
                // carried with the vehicle
                let v = world.get::<Velocity>(se).map_or([0.0; 3], |v| v.lin);
                if let Some(t) = world.get_mut::<Transform>(oe) {
                    t.pos = to_world(&vt, seat.offset);
                }
                if let Some(ov) = world.get_mut::<Velocity>(oe) {
                    ov.lin = v;
                }
                if seat.drive == 1 {
                    let c = world.get::<Controls>(oe).copied().unwrap_or_default();
                    let a = c.axes.map(|x| finite(x, 0.0).clamp(-1.0, 1.0));
                    if let Some(h) = world.get_mut::<Helicopter>(se) {
                        h.roll = a[0];
                        h.collective = a[1];
                        h.pitch = a[2];
                        h.yaw = a[3];
                        h.pilot = Uid(occ);
                        h.engine = true;
                    }
                }
                if seat.drive == 2 {
                    let c = world.get::<Controls>(oe).copied().unwrap_or_default();
                    let a = c.axes.map(|x| finite(x, 0.0).clamp(-1.0, 1.0));
                    if let Some(v) = world.get_mut::<Vehicle>(se) {
                        v.throttle = a[2];
                        v.steer = a[3];
                        v.brake = (-a[1]).max(0.0);
                        v.handbrake = if a[1] > 0.5 { 1.0 } else { 0.0 };
                    }
                }
            } else {
                // (the sitter is gone)
                seated.remove(&occ);
                seat.occupant = Uid(0);
                seat.held = false;
                events.push(("seat_exit".to_string(), suid, Some(vt.pos)));
            }
        } else if let Some(who) = hands.iter().find(|h| h.used == suid && !seated.contains(&h.owner)).map(|h| h.owner) {
            // a character used the seat while nobody sits in it: sit down
            if let Some(oe) = world.entity(who).filter(|oe| world.get::<HitPoints>(*oe).is_none_or(|h| !h.dead && h.value > 0.0)) {
                seat.occupant = Uid(who);
                seat.held = true; // (the press that did it is still down: it must be let go before it gets the sitter out)
                seated.insert(who);
                let _ = world.insert(oe, Seated { seat: Uid(suid) });
                if let Some(t) = world.get_mut::<Transform>(oe) {
                    t.pos = to_world(&vt, seat.offset);
                }
                events.push(("seat_enter".to_string(), suid, Some(vt.pos)));
                if seat.drive == 1 {
                    if let Some(h) = world.get_mut::<Helicopter>(se) {
                        h.pilot = Uid(who);
                        h.engine = true;
                    }
                }
                if seat.drive == 2 {
                    if let Some(v) = world.get_mut::<Vehicle>(se) {
                        v.handbrake = 0.0;
                    }
                }
            }
        }
        // an occupied seat cannot be used by anyone else, and says so on the prompt
        if let Some(i) = world.get_mut::<Interactable>(se) {
            i.enabled = seat.occupant.0 == 0;
        }
        if seat.occupant.0 != was.occupant.0 || seat.held != was.held {
            if let Some(s) = world.get_mut::<Seat>(se) {
                *s = seat;
            }
        }
    }
}
