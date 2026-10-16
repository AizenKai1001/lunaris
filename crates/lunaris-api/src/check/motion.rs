//! `check` rules for character motion (`CharacterMotion`, `Climbable`; docs/GAMEPLAY.md "Character motion"): it works on a `CharacterBody`, its numbers must mean something, and a ladder needs
//! a climber and a place to stand.
use super::Problems;
use lunaris_ecs::Result;
use lunaris_sim::components::{CharacterBody, CharacterMotion, Climbable, Transform};
use lunaris_sim::Sim;

pub(super) fn run(sim: &mut Sim, problems: &mut Problems) -> Result<()> {
    let mut movers: Vec<(u64, CharacterMotion, bool)> = Vec::new();
    sim.world.for_each_uid::<(&CharacterMotion,), _>(|_, uid, (m,)| movers.push((uid, *m, false)))?;
    movers.sort_by_key(|m| m.0);
    for mover in movers.iter_mut() {
        mover.2 = sim.world.entity(mover.0).is_some_and(|e| sim.world.get::<CharacterBody>(e).is_some() && sim.world.get::<Transform>(e).is_some());
    }
    for (uid, m, has_body) in &movers {
        if !has_body {
            problems.add("warning", Some(*uid), "has a CharacterMotion but no CharacterBody and Transform, so nothing moves it".into(), "put a CharacterBody beside it: the motion changes how the body walks, swims, rolls and climbs");
        }
        let numbers = [m.swim_depth, m.float_depth, m.swim_speed, m.dive_speed, m.wade_speed, m.roll_speed, m.roll_time, m.roll_cooldown, m.climb_speed, m.mantle_height, m.mantle_time];
        if numbers.iter().any(|n| !n.is_finite() || *n < 0.0) {
            problems.add("warning", Some(*uid), "CharacterMotion has a number that is negative or not a number, so the engine falls back to its default for it".into(), "speeds, depths, heights and times are 0 or more");
        }
        if m.float_depth.is_finite() && m.swim_depth.is_finite() && m.float_depth > 0.0 && m.swim_depth > m.float_depth + 0.2 {
            problems.add("info", Some(*uid), format!("swim_depth {} is deeper than float_depth {}: it floats higher than the depth it starts swimming at, so it climbs out of the swimming state and wades on the way", m.swim_depth, m.float_depth), "keep swim_depth a little under float_depth (the defaults are 1.1 and 1.35)");
        }
    }
    let mut ladders: Vec<(u64, Climbable, bool)> = Vec::new();
    sim.world.for_each_uid::<(&Climbable,), _>(|_, uid, (c,)| ladders.push((uid, *c, false)))?;
    ladders.sort_by_key(|l| l.0);
    for ladder in ladders.iter_mut() {
        ladder.2 = sim.world.entity(ladder.0).is_some_and(|e| sim.world.get::<Transform>(e).is_some());
    }
    for (uid, c, has_place) in &ladders {
        if !has_place {
            problems.add("error", Some(*uid), "has a Climbable but no Transform, so the ladder has no place".into(), "add a Transform: its position is the foot of the ladder and its +z side is where the climber stands");
        }
        if !(c.height.is_finite() && c.height > 0.0 && c.width.is_finite() && c.width > 0.0) {
            problems.add("warning", Some(*uid), format!("Climbable.height {} and width {} are not both above 0, so nothing can grab it", c.height, c.width), "metres: a ladder is about 3 tall and 0.8 wide");
        }
        if movers.is_empty() {
            problems.add("info", Some(*uid), "a Climbable ladder and no character has a CharacterMotion, so nobody can climb it".into(), "add a CharacterMotion beside the CharacterBody that should climb");
        }
    }
    Ok(())
}
