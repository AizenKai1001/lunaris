//! `check` rules for the joint extras and for sleeping (docs/PHYSICS.md): a `JointMotor` or `JointSpring` with no hinge to act on, a `JointCone` with no
//! ball-and-socket, motors and springs that are set to nothing, and a `PhysicsSleep` that is ignored or that switches sleeping off.
use super::Problems;
use lunaris_ecs::{Result, World};
use lunaris_sim::components::{Joint, JointCone, JointMotor, JointSpring, PhysicsSleep};

pub(super) fn run(world: &mut World, problems: &mut Problems) -> Result<()> {
    let mut motors: Vec<(u64, JointMotor)> = Vec::new();
    let mut springs: Vec<(u64, JointSpring)> = Vec::new();
    let mut cones: Vec<(u64, JointCone)> = Vec::new();
    let mut sleeps: Vec<(u64, PhysicsSleep)> = Vec::new();
    world.for_each_uid::<(&JointMotor,), _>(|_, uid, (m,)| motors.push((uid, *m)))?;
    world.for_each_uid::<(&JointSpring,), _>(|_, uid, (s,)| springs.push((uid, *s)))?;
    world.for_each_uid::<(&JointCone,), _>(|_, uid, (c,)| cones.push((uid, *c)))?;
    world.for_each_uid::<(&PhysicsSleep,), _>(|_, uid, (s,)| sleeps.push((uid, *s)))?;
    motors.sort_by_key(|m| m.0);
    springs.sort_by_key(|s| s.0);
    cones.sort_by_key(|c| c.0);
    sleeps.sort_by_key(|s| s.0);
    let kind_of = |world: &mut World, uid: u64| world.entity(uid).and_then(|e| world.get::<Joint>(e)).map(|j| j.kind);

    for (uid, m) in motors {
        match kind_of(world, uid) {
            Some(4) => {}
            Some(k) => problems.add("warning", Some(uid), format!("has a JointMotor but its Joint is kind {k}, not a hinge (4), so the motor does nothing"), "a motor turns a hinge: set the Joint's kind to 4"),
            None => problems.add("warning", Some(uid), "has a JointMotor but no Joint, so the motor does nothing".into(), "add a Joint with kind 4 (a hinge) to the same entity"),
        }
        if !m.speed.is_finite() || !m.max_torque.is_finite() {
            problems.add("error", Some(uid), "JointMotor has a speed or a max_torque that is not a number".into(), "give both in radians per second and newton-metres");
        } else if m.max_torque <= 0.0 {
            problems.add("warning", Some(uid), "JointMotor.max_torque is 0, which is no motor (a speed of 0 with a large torque is a brake)".into(), "set max_torque to the torque the motor may use, for instance 50");
        }
    }
    for (uid, s) in springs {
        match kind_of(world, uid) {
            Some(4) => {}
            Some(k) => problems.add("warning", Some(uid), format!("has a JointSpring but its Joint is kind {k}, not a hinge (4), so the spring does nothing"), "a spring pulls a hinge towards an angle: set the Joint's kind to 4"),
            None => problems.add("warning", Some(uid), "has a JointSpring but no Joint, so the spring does nothing".into(), "add a Joint with kind 4 (a hinge) to the same entity"),
        }
        if !s.stiffness.is_finite() || !s.damping.is_finite() || !s.angle.is_finite() {
            problems.add("error", Some(uid), "JointSpring has a stiffness, damping or angle that is not a number".into(), "give them in newton-metres per radian, newton-metres per radian per second and radians");
        } else if s.stiffness < 0.0 || s.damping < 0.0 {
            problems.add("warning", Some(uid), "JointSpring has a negative stiffness or damping, which is no spring".into(), "both are 0 or more");
        } else if s.stiffness == 0.0 && s.damping == 0.0 {
            problems.add("warning", Some(uid), "JointSpring has no stiffness and no damping, so it does nothing".into(), "a door that closes by itself: stiffness 20, damping 15, angle 0");
        }
    }
    for (uid, c) in cones {
        match kind_of(world, uid) {
            Some(3) => {}
            Some(k) => problems.add("warning", Some(uid), format!("has a JointCone but its Joint is kind {k}, not a ball-and-socket (3), so the limit does nothing"), "a swing limit is for a ball-and-socket: set the Joint's kind to 3"),
            None => problems.add("warning", Some(uid), "has a JointCone but no Joint, so the limit does nothing".into(), "add a Joint with kind 3 (a ball-and-socket) to the same entity"),
        }
        if !c.angle.is_finite() || c.angle <= 0.0 || c.angle >= std::f32::consts::PI {
            problems.add("warning", Some(uid), "JointCone.angle is 0, or pi and more, or not a number, which means no limit".into(), "give the largest swing in radians, between 0 and pi, for instance 0.8");
        }
    }
    if sleeps.len() > 1 {
        let used = sleeps[0].0;
        let others: Vec<String> = sleeps[1..].iter().map(|s| s.0.to_string()).collect();
        problems.add("warning", Some(used), format!("there is more than one PhysicsSleep: this one (the lowest uid) is used, the one on {} is ignored", others.join(", ")), "keep one entity with a PhysicsSleep");
    }
    if let Some((uid, s)) = sleeps.first() {
        if !s.time.is_finite() || s.time <= 0.0 {
            problems.add("warning", Some(*uid), "PhysicsSleep.time is 0 or negative or not a number, so nothing sleeps".into(), "the seconds a group must lie still before it sleeps: 0.5");
        }
    }
    Ok(())
}
