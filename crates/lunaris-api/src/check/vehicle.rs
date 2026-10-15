//! `check` rules for raycast vehicles (docs/PHYSICS.md): a `Vehicle` that cannot work (no rigid body to push, a wheel table that does not parse), wheels with
//! numbers a car cannot have, springs that are too stiff to be stable at 60 ticks a second or too soft to hold the car up, and controls that do nothing.
use super::Problems;
use lunaris_ecs::{Result, World};
use lunaris_sim::components::{AngularVelocity, CharacterBody, Collider, Parent, RigidBody, Transform, Vehicle, Velocity};
use lunaris_sim::vehicle::{parse_wheels, MAX_WHEELS};

pub(super) fn run(world: &mut World, problems: &mut Problems, dt: f32, gravity: f32) -> Result<()> {
    let mut cars: Vec<(u64, Vehicle)> = Vec::new();
    world.for_each_uid::<(&Vehicle,), _>(|_, uid, (v,)| cars.push((uid, *v)))?;
    if cars.is_empty() {
        return Ok(());
    }
    cars.sort_by_key(|c| c.0);
    for (uid, v) in cars {
        let Some(e) = world.entity(uid) else { continue };
        let mass = world.get::<RigidBody>(e).map(|r| r.mass);
        // ---- what the chassis needs
        if world.get::<Transform>(e).is_none() {
            problems.add("error", Some(uid), "has a Vehicle but no Transform, so it has no place".into(), "add a Transform: the wheel mount points are relative to it");
        }
        match mass {
            None => problems.add("error", Some(uid), "has a Vehicle but no RigidBody, so there is no chassis for the wheels to push".into(), "add RigidBody {mass: 1200}, a Collider (a box about the size of the body), a Velocity and an AngularVelocity"),
            Some(m) if !(m > 0.0) => problems.add("error", Some(uid), "has a Vehicle but its RigidBody has no mass (mass must be above 0), so it is not a dynamic body".into(), "give the chassis a mass in kilograms: 1200 for a car"),
            Some(m) if !(50.0..=100_000.0).contains(&m) => problems.add("warning", Some(uid), format!("the chassis weighs {m} kg: a car is 800 to 2500, a truck up to 30000"), "the springs and engine force are in newtons: scale them with the mass"),
            _ => {}
        }
        if world.get::<Collider>(e).is_none() {
            problems.add("error", Some(uid), "has a Vehicle but no Collider, so the chassis has no shape (and no inertia)".into(), "add a Collider box about the size of the body: {\"shape\": 1, \"size\": [1.8, 0.7, 4.2]}");
        } else if world.get::<Collider>(e).is_some_and(|c| c.trigger) {
            problems.add("warning", Some(uid), "the chassis Collider is a trigger, so the body never touches anything but the wheels do".into(), "turn `trigger` off to let the body bump into things");
        }
        if world.get::<Velocity>(e).is_none() {
            problems.add("error", Some(uid), "has a Vehicle but no Velocity, so it cannot move".into(), "add a Velocity");
        }
        if world.get::<AngularVelocity>(e).is_none() {
            problems.add("error", Some(uid), "has a Vehicle but no AngularVelocity, so the chassis cannot turn, pitch or roll".into(), "add an AngularVelocity (damping 0.05 is a little air)");
        }
        if world.get::<CharacterBody>(e).is_some() {
            problems.add("warning", Some(uid), "has a CharacterBody and a Vehicle: the character controller moves it as well as the wheels".into(), "put the driver on its own entity");
        }

        // ---- the wheel table
        let text = world.strings().get(v.wheels.0).to_string();
        let wheels = match parse_wheels(&text) {
            Ok(w) => w,
            Err(why) => {
                problems.add("error", Some(uid), format!("Vehicle.wheels: {why}"), "one wheel a line: `fl: -0.8 -0.15 -1.3 steer=1 brake=1` (x y z of the strut's top in the chassis's own coordinates, then radius, rest, travel, spring, damper, steer, drive, brake, handbrake, grip, entity; a `defaults:` line sets the rest). Forward is -z");
                continue;
            }
        };
        if wheels.is_empty() {
            problems.add("warning", Some(uid), "Vehicle.wheels has no wheels, so nothing holds the chassis up or drives it".into(), "one wheel a line: `fl: -0.8 -0.15 -1.3 steer=1 brake=1`");
            continue;
        }
        if wheels.len() < 3 {
            problems.add("warning", Some(uid), format!("the vehicle has {} wheels: it stays upright only while it is balanced on them (a bike would need a rider that leans)", wheels.len()), "use three or more wheels, or lower `roll_influence` and raise `anti_roll`");
        }
        let n = wheels.len().min(MAX_WHEELS) as f32;
        let per_wheel = mass.filter(|m| *m > 0.0).map(|m| m / n);
        for w in &wheels {
            let name = &w.name;
            let mut bad = |what: String, hint: &str| problems.add("warning", Some(uid), format!("wheel `{name}`: {what}"), hint);
            if !(0.05..=2.0).contains(&w.radius) {
                bad(format!("radius {} m is not a wheel (a car's is 0.25 to 0.45)", w.radius), "radius is in metres");
            }
            if !(0.03..=2.0).contains(&w.rest) {
                bad(format!("rest {} m is not a suspension (0.2 to 0.5 for a car)", w.rest), "rest is the strut's length with the wheel hanging in the air, in metres");
            }
            if !(w.travel > 0.0 && w.travel <= w.rest) {
                bad(format!("travel {} m must be above 0 and at most `rest` ({} m): it is how far the suspension can be pressed in", w.travel, w.rest), "0.1 to 0.25 for a car");
            }
            if let Some(k) = w.spring {
                if !(200.0..=5.0e6).contains(&k) {
                    bad(format!("spring {k} N/m is out of the range of cars (20,000 to 80,000 for a family car)"), "newtons per metre; use `auto` to get one that holds the car's weight");
                }
            }
            if let Some(c) = w.damper {
                if !(0.0..=1.0e6).contains(&c) {
                    bad(format!("damper {c} N s/m is out of range (2,000 to 6,000 for a family car)"), "newton seconds per metre; use `auto` for a damping ratio of 0.6");
                }
            }
            if !(0.0..=5.0).contains(&w.grip) {
                bad(format!("grip {} is not a tyre (1 to 1.5 on tarmac, 0.3 on ice)", w.grip), "the friction coefficient of the tyre");
            }
            if w.steer.abs() > 2.0 {
                bad(format!("steer {} turns the wheel {} times the steering angle", w.steer, w.steer.abs()), "1 for a front wheel, 0 for a fixed one, a small negative number for rear-wheel steering");
            }
            if w.drive < 0.0 || w.brake < 0.0 || w.handbrake < 0.0 {
                bad("drive, brake and handbrake are shares and cannot be negative".into(), "0 for none, 1 for a full share");
            }
            if let Some(m) = per_wheel {
                // explicit springs: stable while the spring and the damper move the wheel's share of the mass by well under its own size in a tick
                let k = w.spring.unwrap_or(m * gravity / (0.25 * w.travel.max(0.02)));
                if k * dt * dt / m > 0.5 {
                    bad(format!("spring {k:.0} N/m is too stiff for {m:.0} kg a wheel at {:.0} ticks a second: the car would shake apart", 1.0 / dt), "soften it (a family car: 35,000 N/m for 300 kg a wheel) or use `auto`");
                }
                if let Some(c) = w.damper {
                    if c * dt / m > 1.0 {
                        bad(format!("damper {c:.0} N s/m is too strong for {m:.0} kg a wheel at {:.0} ticks a second", 1.0 / dt), "lower it: a damping ratio of 0.3 to 0.8 is 2 * ratio * sqrt(spring * mass per wheel)");
                    }
                }
                let sag = m * gravity / k;
                if sag > 0.9 * w.travel {
                    bad(format!("the springs are too soft for the car: its weight presses a wheel in {sag:.3} m and the travel is only {} m, so it sits on the bump stops", w.travel), "stiffer springs (weight per wheel / the sag you want), or more `travel`");
                }
            }
            if w.entity != 0 {
                match world.entity(w.entity) {
                    None => bad(format!("entity {} does not exist, so no model follows this wheel", w.entity), "the uid of the wheel's model (a child of the vehicle's entity)"),
                    Some(m) => {
                        if world.get::<Transform>(m).is_none() {
                            bad(format!("entity {} has no Transform", w.entity), "a model that is moved needs a Transform");
                        }
                        if let Some(p) = world.get::<Parent>(m) {
                            if p.target.0 != uid {
                                bad(format!("entity {} has a Parent that is not this vehicle, so the engine leaves it alone", w.entity), "make it a child of the vehicle's entity (Parent {target: <the vehicle>} and a LocalTransform), or take its Parent off");
                            } else if world.get::<lunaris_sim::components::LocalTransform>(m).is_none() {
                                bad(format!("entity {} is a child of the vehicle but has no LocalTransform, so it cannot be placed", w.entity), "add a LocalTransform");
                            }
                        }
                    }
                }
            }
        }

        // ---- the controls
        let sum = |f: &dyn Fn(&lunaris_sim::vehicle::Wheel) -> f32| wheels.iter().map(f).sum::<f32>();
        if sum(&|w| w.drive) <= 0.0 {
            problems.add("warning", Some(uid), "no wheel has `drive`, so the throttle does nothing".into(), "put drive=1 on the driven wheels (the engine force is shared by them)");
        }
        if sum(&|w| w.steer.abs()) <= 0.0 {
            problems.add("warning", Some(uid), "no wheel has `steer`, so the car cannot turn".into(), "put steer=1 on the front wheels");
        }
        if sum(&|w| w.brake) <= 0.0 {
            problems.add("warning", Some(uid), "no wheel has `brake`, so the brakes do nothing".into(), "brake=1 is the default: it was set to 0 on every wheel");
        }
        let finite = [v.engine_force, v.power, v.top_speed, v.reverse_speed, v.brake_force, v.handbrake_force, v.steer_max, v.steer_fade, v.steer_fade_speed, v.steer_rate, v.anti_roll, v.downforce, v.air_drag, v.rolling, v.roll_influence, v.handbrake_grip];
        if finite.iter().any(|x| !x.is_finite()) {
            problems.add("error", Some(uid), "a Vehicle number is not a number (NaN or infinite): the engine uses its default for it".into(), "check engine_force, power, top_speed, brake_force, steer_max ...");
        }
        if sum(&|w| w.drive) > 0.0 && v.engine_force <= 0.0 {
            problems.add("warning", Some(uid), "engine_force is 0 or less, so the car cannot drive".into(), "about 7 to 10 times the weight in kilograms: 9000 N for a 1200 kg car");
        }
        if v.brake_force <= 0.0 {
            problems.add("warning", Some(uid), "brake_force is 0 or less, so the car cannot brake".into(), "about 1.3 times the weight in newtons: 16000 N for a 1200 kg car");
        }
        if v.top_speed <= 0.0 {
            problems.add("warning", Some(uid), "top_speed is 0 or less: the engine never fades out, only the drag stops the car".into(), "metres per second: 55 is 200 km/h");
        }
        if !(0.0..=70.0).contains(&v.steer_max) || v.steer_max == 0.0 {
            problems.add("warning", Some(uid), format!("steer_max {} degrees is not a steering lock (a car's wheels turn 25 to 40)", v.steer_max), "degrees");
        }
        if !(0.0..=1.0).contains(&v.roll_influence) || !(0.0..=1.0).contains(&v.handbrake_grip) || !(0.0..=1.0).contains(&v.steer_fade) {
            problems.add("warning", Some(uid), "roll_influence, handbrake_grip and steer_fade are shares from 0 to 1".into(), "the engine clamps them");
        }
    }
    Ok(())
}
