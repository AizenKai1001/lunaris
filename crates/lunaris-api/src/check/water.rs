//! `check` rules for water bodies (`WaterBody`, `Buoyant`, `InWater`; docs/TERRAIN.md "Water bodies"): the picture must be there and have water in it, the numbers must mean something,
//! and a floating body needs what floating needs.
use super::Problems;
use lunaris_ecs::Result;
use lunaris_sim::components::{AngularVelocity, Buoyant, Collider, Floater, RigidBody, Transform, Velocity, Water, WaterBody};
use lunaris_sim::Sim;

/// The most samples `check` looks at to find out whether a picture has any water (a map of 64 million samples is looked at every few rows and columns).
const MAX_SCAN: usize = 1_000_000;

pub(super) fn run(sim: &mut Sim, problems: &mut Problems) -> Result<()> {
    let mut bodies: Vec<(u64, WaterBody)> = Vec::new();
    sim.world.for_each_uid::<(&WaterBody,), _>(|_, uid, (w,)| bodies.push((uid, *w)))?;
    bodies.sort_by_key(|b| b.0);
    for (uid, w) in &bodies {
        let name = sim.world.strings().get(w.heightmap.0).to_string();
        if !sim.world.entity(*uid).is_some_and(|e| sim.world.get::<Transform>(e).is_some()) {
            problems.add("error", Some(*uid), "has a WaterBody but no Transform, so it has no place".into(), "add a Transform: its position is the middle of the water's area, its y is added to the heights of the picture");
        }
        if name.is_empty() {
            problems.add("error", Some(*uid), "WaterBody.heightmap is empty".into(), "name a 16-bit greyscale PNG of the workspace: 0 is dry land, any other value the height of the water (`value / 65535 * relief` metres)");
        } else if let Some(field) = sim.heightfields.get(&name) {
            let stride = ((field.cols * field.rows) as f64 / MAX_SCAN as f64).sqrt().ceil().max(1.0) as usize;
            let wet = (0..field.rows).step_by(stride).any(|r| (0..field.cols).step_by(stride).any(|c| field.sample(c, r) > 0.0));
            if !wet {
                problems.add("warning", Some(*uid), format!("the water picture `{name}` has no water in it (every sample is 0, which is dry land), so nothing is drawn"), "paint the lakes and rivers with the height of their surface (Tellus writes this as water.png)");
            }
        } else {
            let why = sim.terrain_errors.get(&name).cloned().unwrap_or_else(|| "it has not been read".to_string());
            problems.add("error", Some(*uid), format!("the water picture `{name}` could not be used: {why}"), "a greyscale PNG (8 or 16 bits, not interlaced) inside the workspace folder; `reload_terrain` reads it again");
        }
        if !(w.relief.is_finite() && w.relief > 0.0) {
            problems.add("warning", Some(*uid), format!("WaterBody.relief {} is not a height above 0, so the water has no height above the entity", w.relief), "the metres a value of 65535 stands for (Tellus's build.json says: `scale_m` of its water file)");
        }
        if !w.size.iter().all(|s| s.is_finite() && *s > 0.0) {
            problems.add("warning", Some(*uid), format!("WaterBody.size {:?} is not two positive numbers, so the water is a speck", w.size), "the metres the picture covers, the same as the Terrain's size");
        }
    }
    // things that float
    let mut floaters: Vec<(u64, Buoyant)> = Vec::new();
    sim.world.for_each_uid::<(&Buoyant,), _>(|_, uid, (b,)| floaters.push((uid, *b)))?;
    floaters.sort_by_key(|f| f.0);
    for (uid, b) in &floaters {
        let e = sim.world.entity(*uid);
        let has = |f: &dyn Fn(lunaris_ecs::Entity) -> bool| e.is_some_and(f);
        let ok = has(&|e| sim.world.get::<Collider>(e).is_some() && sim.world.get::<RigidBody>(e).is_some_and(|r| r.mass > 0.0) && sim.world.get::<Velocity>(e).is_some() && sim.world.get::<Transform>(e).is_some());
        if !ok {
            problems.add("warning", Some(*uid), "a Buoyant body needs a Collider, a RigidBody (mass above 0), a Velocity and a Transform, so this one does not float".into(), "give it what a falling ball has, then Buoyant {density}");
        }
        if !(b.density.is_finite() && b.density > 0.0) {
            problems.add("warning", Some(*uid), format!("Buoyant.density {} is not above 0 (it counts as 1)", b.density), "kilograms per cubic metre: 500 wood, 920 ice, 1000 neutral, 2500 stone");
        }
        if bodies.is_empty() {
            problems.add("warning", Some(*uid), "a Buoyant body floats on a WaterBody and there is none in the world".into(), "add an entity with Transform and WaterBody");
        }
    }
    // bodies that ride the waves (`Floater`)
    let mut riders: Vec<(u64, Floater)> = Vec::new();
    sim.world.for_each_uid::<(&Floater,), _>(|_, uid, (f,)| riders.push((uid, *f)))?;
    riders.sort_by_key(|f| f.0);
    if !riders.is_empty() {
        let mut sheets = 0usize;
        sim.world.for_each::<(&Water,), _>(|_, _| sheets += 1)?;
        for (uid, f) in &riders {
            let Some(e) = sim.world.entity(*uid) else { continue };
            if !(sim.world.get::<Collider>(e).is_some() && sim.world.get::<RigidBody>(e).is_some_and(|r| r.mass > 0.0) && sim.world.get::<Velocity>(e).is_some() && sim.world.get::<Transform>(e).is_some()) {
                problems.add("warning", Some(*uid), "a Floater needs a Collider, a RigidBody (mass above 0), a Velocity and a Transform, so this one does not float".into(), "give it what a falling ball has, then Floater {density}");
            } else if sim.world.get::<AngularVelocity>(e).is_none() && f.columns > 1 {
                problems.add("info", Some(*uid), "a Floater without an AngularVelocity lifts but never tips: it will not roll with the waves or right itself".into(), "add AngularVelocity {} beside it for a boat, a raft or a log");
            }
            if !(f.density.is_finite() && f.density > 0.0) {
                problems.add("warning", Some(*uid), format!("Floater.density {} is not above 0 (it counts as 1)", f.density), "kilograms per cubic metre: 500 wood, 920 ice, 1000 neutral, 2500 stone");
            }
            if bodies.is_empty() && sheets == 0 {
                problems.add("warning", Some(*uid), "a Floater floats on a Water sheet or a WaterBody and there is none in the world".into(), "add an entity with Transform and Water");
            }
        }
    }
    Ok(())
}
