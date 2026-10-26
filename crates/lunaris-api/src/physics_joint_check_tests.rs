//! `check` names the mistakes with the joint extras and with sleeping: a motor or a spring with no hinge, a swing limit with no ball-and-socket, extras that are
//! set to nothing, and a `PhysicsSleep` that is ignored or switches sleeping off; a good setup says nothing.
use crate::tests::engine;
use crate::Engine;
use lunaris_json::{parse, Json};

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &parse(args).unwrap()).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn spawn(e: &mut Engine, uid: u64, components: &str) {
    call(e, "spawn", &format!(r#"{{"uid":{uid},"components":{components}}}"#));
}

fn messages(e: &mut Engine) -> Vec<String> {
    let c = call(e, "check", "{}");
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| format!("{}: {}", p.get("severity").unwrap().as_str().unwrap(), p.get("message").unwrap().as_str().unwrap())).collect()
}

fn hinge(kind: u32) -> String {
    format!(r#"{{"Transform":{{"pos":[0,2,0]}},"Velocity":{{}},"Collider":{{"shape":1,"size":[1,1,1]}},"RigidBody":{{"mass":1}},"AngularVelocity":{{}},"Joint":{{"kind":{kind},"anchor":[0,3,0],"axis":[0,0,1]}}"#)
}

#[test]
fn a_good_motor_spring_cone_and_sleep_say_nothing() {
    let mut e = engine();
    spawn(&mut e, 1, &format!(r#"{},"JointMotor":{{"speed":2,"max_torque":10}},"JointSpring":{{"stiffness":20,"damping":5}}}}"#, hinge(4)));
    spawn(&mut e, 2, &format!(r#"{},"JointCone":{{"angle":0.8}}}}"#, hinge(3)));
    spawn(&mut e, 3, r#"{"PhysicsSleep":{}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("JointMotor") || x.contains("JointSpring") || x.contains("JointCone") || x.contains("PhysicsSleep")), "{m:?}");
}

#[test]
fn a_motor_or_a_spring_without_a_hinge_and_a_cone_without_a_ball_and_socket_are_named() {
    let mut e = engine();
    spawn(&mut e, 1, &format!(r#"{},"JointMotor":{{"speed":2,"max_torque":10}}}}"#, hinge(3)));
    spawn(&mut e, 2, r#"{"Transform":{},"JointSpring":{"stiffness":5}}"#);
    spawn(&mut e, 3, &format!(r#"{},"JointCone":{{"angle":0.5}}}}"#, hinge(4)));
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("JointMotor") && x.contains("not a hinge")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("JointSpring") && x.contains("no Joint")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("JointCone") && x.contains("not a ball-and-socket")), "{m:?}");
}

#[test]
fn extras_set_to_nothing_are_named() {
    let mut e = engine();
    spawn(&mut e, 1, &format!(r#"{},"JointMotor":{{"speed":2,"max_torque":0}}}}"#, hinge(4)));
    spawn(&mut e, 2, &format!(r#"{},"JointSpring":{{"stiffness":0,"damping":0,"angle":1}}}}"#, hinge(4)));
    spawn(&mut e, 3, &format!(r#"{},"JointCone":{{"angle":0}}}}"#, hinge(3)));
    spawn(&mut e, 4, r#"{"PhysicsSleep":{"time":0}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("max_torque is 0")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("no stiffness and no damping")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("no limit")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("nothing sleeps")), "{m:?}");
}

#[test]
fn two_sleep_settings_name_the_one_that_is_used() {
    let mut e = engine();
    spawn(&mut e, 5, r#"{"PhysicsSleep":{}}"#);
    spawn(&mut e, 7, r#"{"PhysicsSleep":{"time":2}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("more than one PhysicsSleep") && x.contains("7")), "{m:?}");
}
