//! Input actions, state machines, tweens, camera rigs and sensors.
use super::*;
use crate::gameplay::{ease, parse_machine};
use components::{Animator, CameraRig, InputMap, PathFollower, Sensor, StateMachine, Timeline, Transform, Tween, Velocity};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn res(s: &Sim, name: &str) -> f64 {
    s.world.resource(name).unwrap_or(f64::NAN)
}

fn setr(s: &mut Sim, name: &str, v: f64) {
    s.world.set_resource(name, Some(v)).unwrap();
}

fn state_of(s: &Sim, uid: u64) -> (String, u32) {
    let sm = s.world.get::<StateMachine>(s.world.entity(uid).unwrap()).unwrap();
    (s.world.strings().get(sm.state.0).to_string(), sm.index)
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

#[test]
fn actions_turn_inputs_into_named_values_with_edges_and_rebind() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"InputMap":{"bindings":"jump = space, gamepad.a\nmove_x = d, a:-1\ndeadzone = 0.2"}}"#);
    s.step();
    assert_eq!(res(&s, "action.jump"), 0.0);
    setr(&mut s, "input.d", 1.0);
    s.step();
    assert_eq!((res(&s, "action.move_x"), res(&s, "action.move_x.pressed")), (1.0, 1.0));
    s.step();
    assert_eq!((res(&s, "action.move_x"), res(&s, "action.move_x.pressed")), (1.0, 0.0), "pressed lasts one tick");
    setr(&mut s, "input.a", 1.0);
    s.step();
    assert_eq!(res(&s, "action.move_x"), 0.0, "d and a cancel");
    assert_eq!(res(&s, "action.move_x.released"), 1.0);
    setr(&mut s, "input.a", 0.0);
    setr(&mut s, "input.d", 0.1);
    s.step();
    assert_eq!(res(&s, "action.move_x"), 0.0, "under the dead zone");
    setr(&mut s, "input.gamepad.a", 1.0);
    s.step();
    assert_eq!(res(&s, "action.jump"), 1.0);
    // rebinding: jump is now on `q`
    let e = s.world.entity(1).unwrap();
    let text = s.world.strings_mut().intern("jump = q");
    s.world.get_mut::<InputMap>(e).unwrap().bindings.0 = text;
    s.step();
    assert_eq!(res(&s, "action.jump"), 0.0);
    setr(&mut s, "input.q", 1.0);
    s.step();
    assert_eq!(res(&s, "action.jump"), 1.0);
    // bad text is reported
    assert!(crate::gameplay::parse_actions("jump = ").is_err() && crate::gameplay::parse_actions("a.b = x").is_err() && crate::gameplay::parse_actions("x = a:z").is_err());
}

const MACHINE: &str = "state idle clip=Idle\nstate walk clip=Walk speed=1.5\nstate jump clip=Jump once\n\
idle -> walk when $speed > 0.5 fade 0.1\nwalk -> idle when $speed <= 0.5\n* -> jump when $jump >= 1\njump -> idle after 0.5\n\
enter walk: res steps += 1, event started\nexit walk: set Velocity.lin[0] = 7\nenter jump: set Velocity.lin[1] = 5\n# comments are fine";

#[test]
fn a_state_machine_drives_the_animator_and_runs_actions() {
    let mut s = Sim::new(1);
    let text = MACHINE.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{}},"Velocity":{{}},"Animator":{{}},"StateMachine":{{"machine":"{text}"}}}}"#));
    s.step();
    assert_eq!(state_of(&s, 1), ("idle".to_string(), 0));
    let clip = |s: &Sim| {
        let a = s.world.get::<Animator>(s.world.entity(1).unwrap()).unwrap();
        s.world.strings().get(a.clip.0).to_string()
    };
    assert_eq!(clip(&s), "Idle");
    setr(&mut s, "speed", 1.0);
    s.step();
    assert_eq!(state_of(&s, 1), ("walk".to_string(), 1));
    assert_eq!(clip(&s), "Walk");
    let a = *s.world.get::<Animator>(s.world.entity(1).unwrap()).unwrap();
    assert!((a.fade - 0.1).abs() < 1e-6 && (a.speed - 1.5).abs() < 1e-6, "{a:?}");
    assert_eq!(res(&s, "steps"), 1.0);
    assert!(s.events.iter().any(|e| e.name == "started" && e.uid == 1), "{:?}", s.events);
    setr(&mut s, "speed", 0.0);
    s.step();
    assert_eq!(state_of(&s, 1).0, "idle");
    assert_eq!(s.world.get::<Velocity>(s.world.entity(1).unwrap()).unwrap().lin[0], 7.0, "the exit action ran");
    // an any-state transition, then a timed one
    setr(&mut s, "jump", 1.0);
    s.step();
    assert_eq!(state_of(&s, 1).0, "jump");
    assert_eq!(s.world.get::<Velocity>(s.world.entity(1).unwrap()).unwrap().lin[1], 5.0);
    assert!(!s.world.get::<Animator>(s.world.entity(1).unwrap()).unwrap().looping, "`once`");
    setr(&mut s, "jump", 0.0);
    s.step_n(20);
    assert_eq!(state_of(&s, 1).0, "jump", "0.5 s have not passed");
    s.step_n(15);
    assert_eq!(state_of(&s, 1).0, "idle");
    let time = s.world.get::<StateMachine>(s.world.entity(1).unwrap()).unwrap().time;
    assert!(time < 0.1, "{time}");
}

#[test]
fn state_machine_text_reports_what_is_wrong() {
    assert!(parse_machine(MACHINE).is_ok());
    for (bad, word) in [
        ("", "no states"),
        ("state a\na -> b", "not declared"),
        ("state a\nstate a", "twice"),
        ("state a\nfoo bar", "not a state"),
        ("state a\nstate b\na -> b when $x >", "line 3"),
        ("state a\nenter a: dance", "not an action"),
        ("state a clip", "unknown state option"),
        ("state a\na -> a after x", "must be a number"),
    ] {
        let e = parse_machine(bad).unwrap_err();
        assert!(e.contains(word), "{bad:?} gave {e:?}");
    }
}

#[test]
fn tweens_ease_loop_ping_pong_wait_and_reach_other_entities() {
    for k in 0..=13 {
        assert!(ease(k, 0.0).abs() < 1e-5 && (ease(k, 1.0) - 1.0).abs() < 1e-4, "ease {k}: {} {}", ease(k, 0.0), ease(k, 1.0));
    }
    assert!((ease(2, 0.5) - 0.75).abs() < 1e-6 && (ease(1, 0.5) - 0.25).abs() < 1e-6 && (ease(13, 0.5) - 0.5).abs() < 1e-6);
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{},"Tween":{"path":"Transform.pos[1]","from":0,"to":10,"duration":1}}"#);
    s.step_n(30);
    assert!((pos(&s, 1)[1] - 5.0).abs() < 0.1, "{:?}", pos(&s, 1));
    s.step_n(40);
    assert_eq!(pos(&s, 1)[1], 10.0);
    let tw = *s.world.get::<Tween>(s.world.entity(1).unwrap()).unwrap();
    assert!(!tw.active && tw.done);
    // ping-pong
    spawn(&mut s, 2, r#"{"Transform":{},"Tween":{"path":"Transform.pos[0]","from":0,"to":4,"duration":1,"mode":2}}"#);
    s.step_n(90);
    assert!((pos(&s, 2)[0] - 2.0).abs() < 0.1, "going back: {:?}", pos(&s, 2));
    s.step_n(30);
    assert!(pos(&s, 2)[0].abs() < 0.1, "{:?}", pos(&s, 2));
    // a delay, and a tween that moves another entity
    spawn(&mut s, 3, r#"{"Transform":{}}"#);
    spawn(&mut s, 4, r#"{"Tween":{"path":"Transform.pos[2]","entity":3,"from":0,"to":8,"duration":1,"delay":0.5,"ease":2}}"#);
    s.step_n(20);
    assert_eq!(pos(&s, 3)[2], 0.0, "still waiting");
    s.step_n(40);
    assert!((pos(&s, 3)[2] - 6.0).abs() < 0.01, "out-quad at half its time is 3/4 of the way: {:?}", pos(&s, 3));
    // a path that does not exist changes nothing and does not stop the world
    spawn(&mut s, 5, r#"{"Transform":{},"Tween":{"path":"Nope.x","to":3}}"#);
    s.step_n(5);
}

fn forward(q: [f32; 4]) -> [f32; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    [-(2.0 * (x * z + w * y)), -(2.0 * (y * z - w * x)), -(1.0 - 2.0 * (x * x + y * y))]
}

#[test]
fn a_camera_rigs_roll_leans_the_picture_about_the_way_it_looks() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,5]},"Camera":{},"CameraRig":{"target":1,"follow_speed":0,"offset":[0,0,5],"look_offset":[0,0,0],"roll":0.3}}"#);
    s.step();
    let q = s.world.get::<Transform>(s.world.entity(2).unwrap()).unwrap().rot;
    // (looking straight at the target, down -z: the picture's up vector leans to the right by the roll)
    let (sx, cx) = (0.15f32.sin(), 0.15f32.cos());
    assert!((q[2] + sx).abs() < 1e-4 && (q[3] - cx).abs() < 1e-4 && q[0].abs() < 1e-4 && q[1].abs() < 1e-4, "{q:?}");
    let f = forward(q);
    assert!(f[0].abs() < 1e-4 && f[1].abs() < 1e-4 && (f[2] + 1.0).abs() < 1e-4, "the way it looks is the same: {f:?}");
}

fn orbit_rig(extra: &str) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]}}"#);
    spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,0,5]}},"Camera":{{}},"CameraRig":{{"target":1,"mode":1,"yaw":0,"pitch":0,"distance":5,"follow_speed":0,"look_offset":[0,0,0],{extra}}}}}"#));
    s
}

fn set_pos(s: &mut Sim, uid: u64, p: [f32; 3]) {
    s.world.get_mut::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos = p;
}

fn rig(s: &Sim) -> CameraRig {
    *s.world.get::<CameraRig>(s.world.entity(2).unwrap()).unwrap()
}

/// Moves the target `v` m/s along `axis` for `ticks` ticks, returning the pivot's trail (target minus pivot) on that axis.
fn drive_target(s: &mut Sim, axis: usize, v: f32, ticks: u32) -> f32 {
    for _ in 0..ticks {
        let mut p = pos(s, 1);
        p[axis] += v / 60.0;
        set_pos(s, 1, p);
        s.step();
    }
    pos(s, 1)[axis] - rig(s).pivot[axis]
}

#[test]
fn a_rigs_pivot_spring_trails_a_target_at_two_over_the_frequency_and_settles_without_overshoot() {
    let mut s = orbit_rig(r#""lag":12"#);
    s.step();
    assert_eq!(rig(&s).pivot, [0.0, 0.0, 0.0], "the pivot starts on the target");
    // a target at a steady 6 m/s is trailed by 2 v / w = 1 m
    let trail = drive_target(&mut s, 0, 6.0, 120);
    assert!((trail - 1.0).abs() < 0.07, "steady trail {trail} (a tick short of 1 m: the spring sees the target after it has moved)");
    // the camera orbits the pivot, not the target: 5 m behind it
    let c = pos(&s, 2);
    let p = rig(&s).pivot;
    assert!((c[0] - p[0]).abs() < 1e-4 && (c[2] - p[2] - 5.0).abs() < 1e-4, "{c:?} {p:?}");
    // when the target stops the pivot comes to it from behind and never passes it
    let mut worst = 0.0f32;
    for _ in 0..150 {
        s.step();
        worst = worst.min(pos(&s, 1)[0] - rig(&s).pivot[0]);
    }
    assert!(worst > -1e-3, "no overshoot: {worst}");
    assert!((pos(&s, 1)[0] - rig(&s).pivot[0]).abs() < 1e-3, "settled");
}

#[test]
fn a_rigs_lead_aims_ahead_of_a_moving_target_and_the_spring_goes_up_more_softly() {
    // lead 2 / w keeps up: no trail at a steady speed; more runs ahead
    let mut s = orbit_rig(r#""lag":12,"lead":0.1667"#);
    s.step();
    let trail = drive_target(&mut s, 0, 6.0, 150);
    assert!(trail.abs() < 0.08, "keeps up: {trail}");
    let mut s = orbit_rig(r#""lag":12,"lead":0.3"#);
    s.step();
    let trail = drive_target(&mut s, 0, 6.0, 150);
    assert!(trail < -0.7 && trail > -0.95, "runs ahead by about v (lead - 2/w) = 0.8 m: {trail}");
    // up it is softer (`lag_up`) and the lead does not apply to it
    let mut s = orbit_rig(r#""lag":12,"lag_up":4,"lead":0.3"#);
    s.step();
    let up = drive_target(&mut s, 1, 2.0, 200);
    assert!((up - 1.0).abs() < 0.03, "2 v / 4 = 1 m up: {up}");
}

#[test]
fn a_rigs_lag_max_keeps_the_pivot_within_reach_of_a_fast_target() {
    let mut s = orbit_rig(r#""lag":3,"lag_max":1.5,"lag_max_up":0.5"#);
    s.step();
    let trail = drive_target(&mut s, 0, 10.0, 180);
    assert!((trail - 1.5).abs() < 1e-3, "sideways {trail}");
    let up = drive_target(&mut s, 1, 8.0, 120);
    assert!((up - 0.5).abs() < 1e-3, "up {up}");
    // and a fall: the pivot trails below by at most the same
    let down = drive_target(&mut s, 1, -20.0, 60);
    assert!((down + 0.5).abs() < 1e-3, "down {down}");
}

#[test]
fn a_target_that_is_put_somewhere_else_takes_the_pivot_with_it() {
    let mut s = orbit_rig(r#""lag":8"#);
    s.step();
    drive_target(&mut s, 0, 4.0, 30);
    set_pos(&mut s, 1, [200.0, 30.0, -80.0]);
    s.step();
    assert_eq!(rig(&s).pivot, [200.0, 30.0, -80.0]);
    let c = pos(&s, 2);
    assert!((c[0] - 200.0).abs() < 1e-3 && (c[1] - 30.0).abs() < 1e-3 && (c[2] + 75.0).abs() < 1e-3, "{c:?}");
}

#[test]
fn a_rigs_spring_follows_the_target_but_never_the_mouse() {
    let mut s = orbit_rig(r#""lag":6,"lag_up":3"#);
    s.step();
    drive_target(&mut s, 0, 8.0, 60);
    // the mouse turns the yaw a quarter turn in one tick: the camera is round the pivot at once, looking at it
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<CameraRig>(e).unwrap().yaw = std::f32::consts::FRAC_PI_2;
    drive_target(&mut s, 0, 8.0, 1);
    let (c, p) = (pos(&s, 2), rig(&s).pivot);
    assert!((c[0] - p[0] - 5.0).abs() < 1e-3 && (c[2] - p[2]).abs() < 1e-3, "{c:?} {p:?}");
    let q = s.world.get::<Transform>(e).unwrap().rot;
    let f = forward(q);
    assert!((f[0] + 1.0).abs() < 1e-3 && f[2].abs() < 1e-3, "{f:?}");
}

#[test]
fn a_rigs_spring_is_deterministic() {
    let run = || {
        let mut s = orbit_rig(r#""lag":9,"lag_up":5,"lead":0.2,"lag_max":2"#);
        s.step();
        for i in 0..200u32 {
            let v = if i % 50 < 25 { 7.0 } else { -3.0 };
            drive_target(&mut s, if i % 3 == 0 { 1 } else { 0 }, v, 1);
        }
        (pos(&s, 2).map(f32::to_bits), rig(&s).pivot.map(f32::to_bits), s.state_hash())
    };
    assert_eq!(run(), run());
}

#[test]
fn a_camera_rig_follows_looks_orbits_and_shakes_then_settles() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[10,0,0]}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,1,0]},"Camera":{},"CameraRig":{"target":1,"follow_speed":0,"offset":[0,4,8],"look_offset":[0,1,0]}}"#);
    s.step();
    let p = pos(&s, 2);
    assert_eq!(p, [10.0, 4.0, 8.0]);
    let q = s.world.get::<Transform>(s.world.entity(2).unwrap()).unwrap().rot;
    let f = forward(q);
    let want: [f32; 3] = [0.0, 1.0 - 4.0, -8.0];
    let len = (want[1] * want[1] + want[2] * want[2]).sqrt();
    assert!(f[0].abs() < 1e-4 && (f[1] - want[1] / len).abs() < 1e-4 && (f[2] - want[2] / len).abs() < 1e-4, "{f:?}");
    // eased following: one step covers a part of the way
    s.world.get_mut::<CameraRig>(s.world.entity(2).unwrap()).unwrap().follow_speed = 3.0;
    s.world.get_mut::<Transform>(s.world.entity(1).unwrap()).unwrap().pos = [20.0, 0.0, 0.0];
    s.step();
    let x = pos(&s, 2)[0];
    assert!(x > 10.0 && x < 11.0, "{x}");
    s.step_n(300);
    assert!((pos(&s, 2)[0] - 20.0).abs() < 0.01);
    // orbit
    {
        let e = s.world.entity(2).unwrap();
        let r = s.world.get_mut::<CameraRig>(e).unwrap();
        r.mode = 1;
        r.follow_speed = 0.0;
        r.yaw = 0.0;
        r.pitch = 0.0;
        r.distance = 5.0;
    }
    s.step();
    let p = pos(&s, 2);
    assert!((p[0] - 20.0).abs() < 1e-4 && (p[1] - 1.0).abs() < 1e-4 && (p[2] - 5.0).abs() < 1e-4, "{p:?}");
    // shake: moves the camera, then calms down exactly
    s.world.get_mut::<CameraRig>(s.world.entity(2).unwrap()).unwrap().shake = 1.0;
    s.step();
    let shaken = pos(&s, 2);
    assert!(shaken != [20.0, 1.0, 5.0], "{shaken:?}");
    s.step_n(120);
    assert_eq!(pos(&s, 2), [20.0, 1.0, 5.0]);
}

#[test]
fn a_sensor_sees_what_is_near_in_front_and_not_behind_a_wall() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{},"Sensor":{"radius":12,"fov":120,"filter":"Velocity"}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,-5]},"Velocity":{}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0,5]},"Velocity":{}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,0,-20]},"Velocity":{}}"#);
    let sensor = |s: &Sim| *s.world.get::<Sensor>(s.world.entity(1).unwrap()).unwrap();
    s.step();
    let a = sensor(&s);
    assert_eq!((a.seen.0, a.count), (2, 1), "{a:?}");
    assert!((a.distance - 5.0).abs() < 1e-4);
    s.world.get_mut::<Sensor>(s.world.entity(1).unwrap()).unwrap().fov = 360.0;
    s.step();
    assert_eq!(sensor(&s).count, 2, "all around: in front and behind, not the far one");
    // a wall in between hides the one in front once line of sight counts
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,0,-2.5]},"Collider":{"shape":1,"size":[4,4,0.5]}}"#);
    s.step();
    assert_eq!(sensor(&s).seen.0, 2);
    s.world.get_mut::<Sensor>(s.world.entity(1).unwrap()).unwrap().los = true;
    s.step();
    let a = sensor(&s);
    assert_eq!((a.seen.0, a.count), (3, 1), "only the one behind is visible: {a:?}");
    // nobody
    s.world.get_mut::<Sensor>(s.world.entity(1).unwrap()).unwrap().radius = 1.0;
    s.step();
    assert_eq!((sensor(&s).seen.0, sensor(&s).distance), (0, 0.0));
}

#[test]
fn all_of_it_is_deterministic_and_survives_a_clone() {
    let build = || {
        let mut s = Sim::new(7);
        spawn(&mut s, 1, r#"{"InputMap":{"bindings":"go = d"}}"#);
        spawn(&mut s, 2, r#"{"Transform":{},"Tween":{"path":"Transform.pos[0]","to":5,"mode":1,"ease":11}}"#);
        spawn(&mut s, 3, r#"{"Transform":{},"CameraRig":{"target":2,"shake":0.8}}"#);
        spawn(&mut s, 4, r#"{"Transform":{},"StateMachine":{"machine":"state a\nstate b\na -> b when $action.go > 0.5\nb -> a after 0.2"}}"#);
        setr(&mut s, "input.d", 1.0);
        s
    };
    let (mut a, mut b) = (build(), build());
    a.step_n(200);
    b.step_n(100);
    let mut c = b.clone();
    b.step_n(100);
    c.step_n(100);
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(b.state_hash(), c.state_hash());
}

const SCRIPT: &str = "at 0 set Velocity.lin[0] = 1\nat 0.5 set Velocity.lin[1] = 2\nat 1 event boom\nat 0 ramp Transform.pos[2] 0 -> 6 over 2\nat 3 res finished += 1\nat 3 end";

fn timeline_world(extra: &str) -> Sim {
    let mut s = Sim::new(1);
    let text = SCRIPT.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{}},"Velocity":{{}},"Timeline":{{"script":"{text}"{extra}}}}}"#));
    s
}

fn booms(s: &Sim) -> usize {
    s.events.iter().filter(|e| e.name == "boom").count()
}

#[test]
fn a_timeline_fires_steps_in_time_ramps_values_and_ends() {
    let mut s = timeline_world("");
    let lin = |s: &Sim| s.world.get::<Velocity>(s.world.entity(1).unwrap()).unwrap().lin;
    s.step();
    assert_eq!(lin(&s)[0], 1.0, "a step at 0 fires on the first tick");
    assert_eq!(lin(&s)[1], 0.0);
    s.step_n(30);
    assert_eq!(lin(&s)[1], 2.0, "at 0.5");
    assert_eq!(booms(&s), 0);
    s.step_n(30);
    assert_eq!(booms(&s), 1, "at 1");
    assert!((pos(&s, 1)[2] - 3.0).abs() < 0.1, "half way through the ramp: {:?}", pos(&s, 1));
    s.step_n(60);
    assert!((pos(&s, 1)[2] - 6.0).abs() < 1e-4, "the ramp ended on its last value");
    s.step_n(120);
    assert_eq!(booms(&s), 1, "nothing fires twice");
    assert_eq!(res(&s, "finished"), 1.0);
    let tl = *s.world.get::<Timeline>(s.world.entity(1).unwrap()).unwrap();
    assert!(tl.done && !tl.playing && (tl.time - 3.0).abs() < 1e-4, "{tl:?}");
}

#[test]
fn seeking_a_timeline_puts_its_numbers_where_they_would_be_at_that_time_and_runs_nothing_else() {
    let mut s = timeline_world("");
    let e = s.world.entity(1).unwrap();
    let lin = |s: &Sim| s.world.get::<Velocity>(s.world.entity(1).unwrap()).unwrap().lin;
    // 1.0 s: the ramp (0 -> 6 over 2 s) is half way, the `set` at 0 and at 0.5 have both happened, and the event at 1 did not run
    let (written, length) = gameplay::seek_timeline(&mut s.world, e, 1.0).unwrap();
    assert_eq!((written, length), (3, 3.0));
    assert!((pos(&s, 1)[2] - 3.0).abs() < 1e-5, "{:?}", pos(&s, 1));
    assert_eq!((lin(&s)[0], lin(&s)[1]), (1.0, 2.0));
    assert_eq!(booms(&s), 0, "events are not fired by a seek");
    assert!(res(&s, "finished").is_nan(), "neither are resources: it was never set");
    assert!((s.world.get::<Timeline>(e).unwrap().time - 1.0).abs() < 1e-6);
    // back to 0.25 s: the ramp's value then; the set at 0.5 has not happened, but a `set` has no earlier value to return to, so it keeps what it had
    gameplay::seek_timeline(&mut s.world, e, 0.25).unwrap();
    assert!((pos(&s, 1)[2] - 0.75).abs() < 1e-5, "0.25 / 2 of 6");
    assert_eq!(lin(&s)[0], 1.0);
    // before the start of a ramp that begins later: its starting value
    let mut late = Sim::new(1);
    spawn(&mut late, 1, r#"{"Transform":{"pos":[0,9,0]},"Timeline":{"script":"at 2 ramp Transform.pos[1] 4 -> 8 over 2"}}"#);
    let le = late.world.entity(1).unwrap();
    gameplay::seek_timeline(&mut late.world, le, 0.0).unwrap();
    assert_eq!(pos(&late, 1)[1], 4.0, "the ramp has not begun: the number is at its `from`");
    // beyond the end: clamped, and every ramp at its last value
    let (_, len) = gameplay::seek_timeline(&mut late.world, le, 99.0).unwrap();
    assert_eq!(len, 4.0);
    assert_eq!(pos(&late, 1)[1], 8.0);
    assert_eq!(late.world.get::<Timeline>(le).unwrap().time, 4.0);
    // `+=` steps are left out (they cannot be undone by going back); an entity without a timeline is refused
    let mut adds = Sim::new(1);
    spawn(&mut adds, 1, r#"{"Transform":{},"Timeline":{"script":"at 1 set Transform.pos[0] += 5\nat 2 end"}}"#);
    let ae = adds.world.entity(1).unwrap();
    assert_eq!(gameplay::seek_timeline(&mut adds.world, ae, 1.5).unwrap().0, 0);
    assert_eq!(pos(&adds, 1)[0], 0.0);
    spawn(&mut adds, 2, r#"{"Transform":{}}"#);
    let bare = adds.world.entity(2).unwrap();
    assert!(gameplay::seek_timeline(&mut adds.world, bare, 1.0).is_err());
}

#[test]
fn a_timeline_can_loop_run_faster_and_be_played_again() {
    let mut s = timeline_world(r#","looping":true"#);
    s.step_n(270); // 4.5 s: boom at 1 and at 4
    assert_eq!(booms(&s), 2);
    assert_eq!(res(&s, "finished"), 1.0, "once at 3 (the second round has not reached it)");
    let mut f = timeline_world(r#","speed":2"#);
    f.step_n(60);
    assert_eq!(booms(&f), 1, "at double speed 1 s of the script takes half a second");
    // play again
    f.step_n(120);
    assert!(f.world.get::<Timeline>(f.world.entity(1).unwrap()).unwrap().done);
    {
        let tl = f.world.get_mut::<Timeline>(f.world.entity(1).unwrap()).unwrap();
        tl.time = 0.0;
        tl.done = false;
        tl.playing = true;
    }
    f.step_n(60);
    assert_eq!(booms(&f), 2);
    // determinism
    let (mut a, mut b) = (timeline_world(r#","looping":true"#), timeline_world(r#","looping":true"#));
    a.step_n(500);
    b.step_n(200);
    let mut c = b.clone();
    b.step_n(300);
    c.step_n(300);
    assert_eq!((a.state_hash(), b.state_hash()), (b.state_hash(), c.state_hash()));
}

#[test]
fn timeline_text_reports_what_is_wrong() {
    assert!(crate::gameplay::parse_timeline(SCRIPT).is_ok());
    for (bad, word) in [("", "empty"), ("set X.y = 1", "starts with"), ("at x event a", "must be a number"), ("at -1 event a", "negative"), ("at 1 ramp A.b 1 2 over 1", "FROM -> TO"), ("at 1 ramp A.b 1 -> 2", "over"), ("at 1 ramp A.b 1 -> 2 over 1 slowly", "unexpected"), ("at 1 dance", "not an action")] {
        let e = crate::gameplay::parse_timeline(bad).unwrap_err();
        assert!(e.contains(word), "{bad:?} gave {e:?}");
    }
}

fn square(extra: &str) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[100,0,0]},"Spline":{"points":"0 0 0; 10 0 0\n10 0 10; 0 0 10","smooth":false,"closed":true}}"#);
    spawn(&mut s, 2, &format!(r#"{{"Transform":{{}},"PathFollower":{{"path":1,"speed":10{extra}}}}}"#));
    s
}

#[test]
fn a_follower_travels_a_path_at_speed_loops_stops_or_turns_back_and_faces_forward() {
    let mut s = square("");
    s.step_n(60);
    let p = pos(&s, 2);
    assert!((p[0] - 110.0).abs() < 0.2 && p[2].abs() < 0.2, "10 m along the first side (the path is relative to its entity): {p:?}");
    let q = s.world.get::<Transform>(s.world.entity(2).unwrap()).unwrap().rot;
    // moving along +x one second in: it faces +x
    let f = forward(q);
    s.step_n(90);
    let p = pos(&s, 2);
    assert!((p[0] - 105.0).abs() < 0.3 && (p[2] - 10.0).abs() < 0.3, "25 m along a closed square of 40: {p:?}");
    s.step_n(90); // 40 m more: round to 15 m
    assert!(s.world.get::<PathFollower>(s.world.entity(2).unwrap()).unwrap().distance < 40.0 && !s.world.get::<PathFollower>(s.world.entity(2).unwrap()).unwrap().done);
    let _ = f;
    // facing: on the first side it looks along +x
    let mut t = square("");
    t.step_n(30);
    let f = forward(t.world.get::<Transform>(t.world.entity(2).unwrap()).unwrap().rot);
    assert!((f[0] - 1.0).abs() < 1e-3 && f[1].abs() < 1e-3 && f[2].abs() < 1e-3, "{f:?}");
    // once: it stops at the end of an open path and is done
    let mut o = Sim::new(1);
    spawn(&mut o, 1, r#"{"Transform":{},"Spline":{"points":"0 0 0; 20 0 0","smooth":false}}"#);
    spawn(&mut o, 2, r#"{"Transform":{},"PathFollower":{"path":1,"speed":10,"mode":0}}"#);
    o.step_n(180);
    let fo = *o.world.get::<PathFollower>(o.world.entity(2).unwrap()).unwrap();
    assert!(fo.done && (pos(&o, 2)[0] - 20.0).abs() < 1e-4, "{fo:?} {:?}", pos(&o, 2));
    // ping-pong: out and back
    let mut pp = Sim::new(1);
    spawn(&mut pp, 1, r#"{"Transform":{},"Spline":{"points":"0 0 0; 20 0 0","smooth":false}}"#);
    spawn(&mut pp, 2, r#"{"Transform":{},"PathFollower":{"path":1,"speed":10,"mode":2,"align":false}}"#);
    pp.step_n(180);
    assert!((pos(&pp, 2)[0] - 10.0).abs() < 0.3, "3 s: out 20 m and back 10 m: {:?}", pos(&pp, 2));
    // the path can move: the follower goes with it
    s.world.get_mut::<Transform>(s.world.entity(1).unwrap()).unwrap().pos = [0.0, 5.0, 0.0];
    s.step();
    assert!(pos(&s, 2)[0] < 20.0 && (pos(&s, 2)[1] - 5.0).abs() < 1e-3, "{:?}", pos(&s, 2));
}

#[test]
fn a_smooth_spline_curves_through_its_points_and_text_errors_are_reported() {
    let table = crate::gameplay::PathTable::build(&crate::gameplay::parse_points("0 0 0; 10 0 0; 10 0 10; 20 0 10").unwrap(), true, false);
    let straight = crate::gameplay::PathTable::build(&crate::gameplay::parse_points("0 0 0; 10 0 0; 10 0 10; 20 0 10").unwrap(), false, false);
    assert!(table.length() < straight.length() + 1.0 && table.length() > 25.0, "{} {}", table.length(), straight.length());
    let (end, _) = table.at(table.length());
    assert!((end[0] - 20.0).abs() < 1e-4 && (end[2] - 10.0).abs() < 1e-4, "ends on the last point: {end:?}");
    let (start, dir) = table.at(0.0);
    assert!(start == [0.0, 0.0, 0.0] && dir[0] > 0.9, "{start:?} {dir:?}");
    // the curve cuts the corner: its middle is not on the straight lines' corner
    let (mid, _) = table.at(table.length() * 0.5);
    assert!(mid[0] > 5.0 && mid[0] < 15.0 && mid[2] > 0.5 && mid[2] < 9.5, "{mid:?}");
    for bad in ["", "1 2 3", "1 2; 3 4 5", "a b c; 1 2 3"] {
        assert!(crate::gameplay::parse_points(bad).is_err(), "{bad}");
    }
}

fn particles(s: &Sim) -> Vec<(u64, [f32; 3], [f32; 3], [f32; 3])> {
    let mut out = Vec::new();
    let mut w = s.world.clone();
    let _ = w.for_each_uid::<(&components::Particle, &Transform, &Velocity), _>(|_, uid, (_, t, v)| out.push((uid, t.pos, v.lin, t.scale)));
    out
}

#[test]
fn emitter_effects_shape_the_start_drag_spin_bounce_stick_and_the_middle_stops() {
    let mk = |fx: &str, em: &str| {
        let mut s = Sim::new(3);
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[10,5,0]}},"Emitter":{{"rate":0,"burst":60{em}}},"EmitterFx":{fx}}}"#));
        s
    };
    const STILL: &str = r#","life":3,"speed":0,"gravity":[0,0,0]"#;
    // a sphere, a box and a disc: every particle starts inside its shape
    let sphere = { let mut s = mk(r#"{"shape":1,"extent":[2,2,2]}"#, STILL); s.step(); s };
    let ps = particles(&sphere);
    assert_eq!(ps.len(), 60);
    assert!(ps.iter().all(|p| ((p.1[0] - 10.0).powi(2) + (p.1[1] - 5.0).powi(2) + p.1[2].powi(2)).sqrt() <= 2.001), "inside the sphere");
    assert!(ps.iter().any(|p| (p.1[1] - 5.0).abs() > 0.8), "spread over its height");
    let bx = { let mut s = mk(r#"{"shape":2,"extent":[3,0.5,1]}"#, STILL); s.step(); s };
    assert!(particles(&bx).iter().all(|p| (p.1[0] - 10.0).abs() <= 3.001 && (p.1[1] - 5.0).abs() <= 0.501 && p.1[2].abs() <= 1.001));
    let disc = { let mut s = mk(r#"{"shape":3,"extent":[4,0,0]}"#, STILL); s.step(); s };
    assert!(particles(&disc).iter().all(|p| (p.1[1] - 5.0).abs() < 1e-4 && ((p.1[0] - 10.0).powi(2) + p.1[2].powi(2)).sqrt() <= 4.001));
    // drag slows them down, no drag does not
    let fast = r#","life":3,"speed":10,"gravity":[0,0,0]"#;
    let (mut d, mut n) = (mk(r#"{"drag":3}"#, fast), mk(r#"{}"#, fast));
    d.step_n(60);
    n.step_n(60);
    let speed = |s: &Sim| particles(s).iter().map(|p| (p.2[0] * p.2[0] + p.2[1] * p.2[1] + p.2[2] * p.2[2]).sqrt()).fold(0.0f32, f32::max);
    assert!(speed(&d) < 3.0 && speed(&n) > 8.0, "{} {}", speed(&d), speed(&n));
    // spin turns sprites about z
    let mut sp = mk(r#"{"spin":4}"#, STILL);
    sp.step();
    let mut w = sp.world.clone();
    let mut spun = 0;
    let _ = w.for_each::<(&components::Particle, &Velocity), _>(|_, (_, v)| spun += (v.ang[2].abs() > 0.0) as u32);
    assert!(spun > 30, "{spun}");
    // a floor: particles fall onto it and bounce, never going far below; with stick they are gone
    let fall = r#","life":5,"speed":0,"gravity":[0,-9.8,0]"#;
    let mut b = mk(r#"{"floor":true,"floor_y":0,"bounce":0.5}"#, fall);
    for _ in 0..120 {
        b.step();
        assert!(particles(&b).iter().all(|p| p.1[1] > -0.2), "nothing sinks through the floor");
    }
    assert!(!particles(&b).is_empty());
    let mut st = mk(r#"{"floor":true,"floor_y":0,"stick":true}"#, fall);
    st.step_n(120);
    assert!(particles(&st).is_empty(), "stuck particles are removed");
    // the middle size: at half of its life a particle has the middle size
    let mut m = mk(r#"{"mid":0.5,"size_mid":2}"#, r#","life":3,"speed":0,"gravity":[0,0,0],"size0":0.5,"size1":0,"life_var":0"#);
    m.step_n(90); // 1.5 s of 3 s
    assert!(particles(&m).iter().all(|p| (p.3[0] - 2.0).abs() < 0.1), "{:?}", particles(&m).first());
    // deterministic
    let (mut a, mut c) = (mk(r#"{"shape":1,"turbulence":3,"drag":1,"spin":2,"floor":true}"#, fall), mk(r#"{"shape":1,"turbulence":3,"drag":1,"spin":2,"floor":true}"#, fall));
    a.step_n(90);
    c.step_n(90);
    assert_eq!(a.state_hash(), c.state_hash());
}

#[test]
fn a_stream_zone_loads_its_prefab_near_the_target_and_unloads_it_far_away() {
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "House".into(), def: parse(r#"{"components":{"Name":{"value":"House"},"Transform":{}},"children":[{"name":"Roof","components":{"LocalTransform":{"pos":[0,3,0]}}}]}"#).unwrap() }).unwrap();
    spawn(&mut s, 1, r#"{"Transform":{"pos":[100,0,0]},"StreamZone":{"prefab":"House","radius":50,"hysteresis":10,"target":2}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]}}"#);
    let count = |s: &Sim| s.world.len();
    let zone = |s: &Sim| s.world.get::<components::StreamZone>(s.world.entity(1).unwrap()).unwrap().loaded;
    s.step();
    assert_eq!((count(&s), zone(&s)), (2, false), "too far");
    s.world.get_mut::<Transform>(s.world.entity(2).unwrap()).unwrap().pos = [60.0, 0.0, 0.0];
    s.step();
    assert_eq!((count(&s), zone(&s)), (4, true), "the house and its roof are there");
    assert_eq!(pos(&s, 3), [100.0, 0.0, 0.0]);
    s.step_n(5);
    assert_eq!(count(&s), 4, "loaded once");
    // a little farther, inside the margin: it stays
    s.world.get_mut::<Transform>(s.world.entity(2).unwrap()).unwrap().pos = [45.0, 0.0, 0.0];
    s.step();
    assert!(zone(&s) && count(&s) == 4);
    // beyond radius + hysteresis: gone, with its child
    s.world.get_mut::<Transform>(s.world.entity(2).unwrap()).unwrap().pos = [30.0, 0.0, 0.0];
    s.step();
    assert_eq!((count(&s), zone(&s)), (2, false));
    // and it comes back
    s.world.get_mut::<Transform>(s.world.entity(2).unwrap()).unwrap().pos = [90.0, 0.0, 0.0];
    s.step();
    assert_eq!((count(&s), zone(&s)), (4, true));
    // with no target named it follows the active camera
    let mut c = Sim::new(1);
    c.apply(&Command::DefinePrefab { name: "Rock".into(), def: parse(r#"{"components":{"Transform":{}}}"#).unwrap() }).unwrap();
    spawn(&mut c, 1, r#"{"Transform":{"pos":[0,0,0]},"StreamZone":{"prefab":"Rock","radius":10}}"#);
    spawn(&mut c, 2, r#"{"Transform":{"pos":[5,0,0]},"Camera":{}}"#);
    c.step();
    assert!(c.world.get::<components::StreamZone>(c.world.entity(1).unwrap()).unwrap().loaded);
}

#[test]
fn a_stream_zone_with_a_vertical_limit_is_the_floor_of_a_building_loaded_only_while_the_target_is_at_its_height() {
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "Floor".into(), def: parse(r#"{"components":{"Name":{"value":"Floor"},"Transform":{}}}"#).unwrap() }).unwrap();
    // the floor at 30 m, within 4 m of height (and 2 m of margin), anywhere within 50 m across
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,30,0]},"StreamZone":{"prefab":"Floor","radius":50,"hysteresis":2,"vertical":4,"target":2}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]}}"#);
    let zone = |s: &Sim| s.world.get::<components::StreamZone>(s.world.entity(1).unwrap()).unwrap().loaded;
    let set_y = |s: &mut Sim, y: f32| s.world.get_mut::<Transform>(s.world.entity(2).unwrap()).unwrap().pos = [0.0, y, 0.0];
    s.step();
    assert!(!zone(&s), "at street level the floor 30 m up is not there");
    set_y(&mut s, 27.0);
    s.step();
    assert!(zone(&s), "within 4 m of its height it is");
    set_y(&mut s, 25.0);
    s.step();
    assert!(zone(&s), "5 m below: still inside the margin of 2 m");
    set_y(&mut s, 20.0);
    s.step();
    assert!(!zone(&s), "10 m below: gone");
}

#[test]
fn a_proc_grid_keeps_the_zones_a_provider_makes_for_the_cells_near_the_camera_and_what_they_load() {
    use crate::procgen::{self, Provider, ZoneDef};
    use std::sync::Arc;
    struct Cells;
    impl Provider for Cells {
        fn claims(&self, _: &str) -> bool {
            false
        }
        fn file(&self, _: &str) -> Option<Result<Vec<u8>, String>> {
            None
        }
        fn file_ready(&self, _: &str) -> bool {
            true
        }
        fn prefetch_file(&self, _: &str) {}
        fn prefab(&self, name: &str) -> Option<String> {
            name.starts_with("gen:rock").then(|| r#"{"components":{"Name":{"value":"Rock"},"Transform":{}}}"#.to_string())
        }
        fn zones(&self, _: &str, i: i32, j: i32) -> Vec<ZoneDef> {
            // one zone in the middle of each cell that loads a rock when the camera is within 60 m of it
            vec![ZoneDef { name: format!("cell {i} {j}"), pos: [(i as f32 + 0.5) * 100.0, 0.0, (j as f32 + 0.5) * 100.0], prefab: format!("gen:rock:{i}:{j}"), radius: 60.0, hysteresis: 10.0, ..Default::default() }]
        }
    }
    let _g = procgen::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    procgen::set_provider(Some(Arc::new(Cells)));
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[50,1,50]},"Camera":{}}"#);
    spawn(&mut s, 2, r#"{"ProcGrid":{"grid":"test","cell":100,"radius":150,"hysteresis":50,"per_tick":50}}"#);
    let zones = |s: &Sim| {
        let mut n = 0;
        let _ = s.world.clone().for_each_uid::<(&components::GridZone,), _>(|_, _, _| n += 1);
        n
    };
    let rocks = |s: &Sim| {
        let mut n = 0;
        let _ = s.world.clone().for_each_uid::<(&components::SpawnedBy,), _>(|_, _, _| n += 1);
        n
    };
    s.step_n(3);
    // cells whose middle is within 150 m of (50, 50): the 3 x 3 around it, minus none: the corners are at 141 m
    assert_eq!(zones(&s), 9, "nine cells, one zone each");
    assert_eq!(rocks(&s), 1, "only the rock of the cell the camera is in is within 60 m of its zone");
    // the camera drives a long way east: the cells behind it go (with their rocks), new ones come
    s.world.get_mut::<Transform>(s.world.entity(1).unwrap()).unwrap().pos = [1050.0, 1.0, 50.0];
    s.step_n(3);
    assert_eq!(zones(&s), 9);
    assert_eq!(rocks(&s), 1);
    assert!(s.world.len() < 40, "nothing piles up: {} entities", s.world.len());
    // a server: no active camera, a `StreamAnchor` on each of two players far apart: the cells round both, none round the camera's last place
    s.world.get_mut::<components::Camera>(s.world.entity(1).unwrap()).unwrap().active = false;
    spawn(&mut s, 3, r#"{"Transform":{"pos":[50,1,50]},"StreamAnchor":{}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[2050,1,50]},"StreamAnchor":{}}"#);
    s.step_n(6);
    assert_eq!(zones(&s), 18, "nine cells round each anchor");
    assert_eq!(rocks(&s), 2, "and the rock of the cell each stands in");
    // the second walks to the first: the cells of the two overlap and are not made twice
    s.world.get_mut::<Transform>(s.world.entity(4).unwrap()).unwrap().pos = [150.0, 1.0, 50.0];
    s.step_n(6);
    assert_eq!(zones(&s), 12, "the union of the two 3 x 3 squares: 3 x 4 cells");
    procgen::set_provider(None);
}

#[test]
fn the_zones_of_all_the_cells_made_in_a_tick_are_spawned_in_one_batch_with_the_uids_they_would_get_one_by_one() {
    use crate::procgen::{self, Provider, ZoneDef};
    use std::sync::Arc;
    // cells with 3, 0, 2, 1 and 0 zones (the cells with none are remembered by a marker entity)
    struct Mixed;
    impl Provider for Mixed {
        fn claims(&self, _: &str) -> bool {
            false
        }
        fn file(&self, _: &str) -> Option<Result<Vec<u8>, String>> {
            None
        }
        fn file_ready(&self, _: &str) -> bool {
            true
        }
        fn prefetch_file(&self, _: &str) {}
        fn prefab(&self, _: &str) -> Option<String> {
            None
        }
        fn zones(&self, _: &str, i: i32, j: i32) -> Vec<ZoneDef> {
            let count = match (i, j) {
                (0, 0) => 3,
                (-1, 0) => 2,
                (0, 1) => 1,
                _ => 0,
            };
            (0..count).map(|k| ZoneDef { name: format!("z {i} {j} {k}"), pos: [(i as f32 + 0.5) * 100.0, 0.0, (j as f32 + 0.5) * 100.0], prefab: "nothing".to_string(), radius: 5.0, hysteresis: 1.0, ..Default::default() }).collect()
        }
    }
    let _g = procgen::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    procgen::set_provider(Some(Arc::new(Mixed)));
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[50,1,50]},"Camera":{}}"#);
    spawn(&mut s, 2, r#"{"ProcGrid":{"grid":"mixed","cell":100,"radius":120,"hysteresis":10,"per_tick":50}}"#);
    let next = s.world.next_uid();
    s.step_n(1);
    // nearest first (the camera's own cell, then the four at 100 m by their numbers): (0,0) 3 zones, (-1,0) 2, (0,-1) a marker, (0,1) 1, (1,0) a marker
    let expect: [(i32, i32); 8] = [(0, 0), (0, 0), (0, 0), (-1, 0), (-1, 0), (0, -1), (0, 1), (1, 0)];
    for (k, (i, j)) in expect.iter().enumerate() {
        let e = s.world.entity(next + k as u64).unwrap_or_else(|| panic!("uid {} exists", next + k as u64));
        let z = s.world.get::<components::GridZone>(e).expect("a GridZone marks it");
        assert_eq!((z.i as i32, z.j as i32, z.grid.0), (*i, *j, 2), "entity {k}");
    }
    assert!(s.world.entity(next + 8).is_none(), "and nothing more");
    let (len, uid_after) = (s.world.len(), s.world.next_uid());
    assert_eq!(uid_after, next + 8);
    // nothing is made twice, and the cells that are left behind go with their zones and markers
    s.step_n(3);
    assert_eq!((s.world.len(), s.world.next_uid()), (len, uid_after));
    s.world.get_mut::<Transform>(s.world.entity(1).unwrap()).unwrap().pos = [5050.0, 1.0, 50.0];
    s.step_n(3);
    assert!(s.world.entity(next).is_none() && s.world.entity(next + 7).is_none(), "the old cells and their markers are gone");
    procgen::set_provider(None);
}

#[test]
fn stream_zones_follow_every_anchor_and_a_zone_goes_only_when_all_have_left() {
    // a server has no camera and many players: every `StreamAnchor` keeps the zones round it (the zones that name no target), a zone is loaded while any anchor is near and let go when all are far
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "House".into(), def: parse(r#"{"components":{"Name":{"value":"House"},"Transform":{}}}"#).unwrap() }).unwrap();
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"StreamZone":{"prefab":"House","radius":50,"hysteresis":10}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[1000,0,0]},"StreamZone":{"prefab":"House","radius":50,"hysteresis":10}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[20,0,0]},"StreamAnchor":{}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[1010,0,0]},"StreamAnchor":{}}"#);
    let loaded = |s: &Sim, u: u64| s.world.get::<components::StreamZone>(s.world.entity(u).unwrap()).unwrap().loaded;
    s.step();s.step();
    assert!(loaded(&s, 1) && loaded(&s, 2), "each zone is loaded by the anchor near it");
    // one anchor walks away from the first zone: it goes (no other anchor is near it), the second stays
    s.world.get_mut::<Transform>(s.world.entity(3).unwrap()).unwrap().pos = [500.0, 0.0, 0.0];
    s.step();s.step();
    assert!(!loaded(&s, 1), "the zone goes when its only anchor has left");
    assert!(loaded(&s, 2));
    // two anchors near the same zone: it stays while either is
    s.world.get_mut::<Transform>(s.world.entity(3).unwrap()).unwrap().pos = [1020.0, 0.0, 0.0];
    s.step();s.step();
    s.world.get_mut::<Transform>(s.world.entity(4).unwrap()).unwrap().pos = [-900.0, 0.0, 0.0];
    s.step();s.step();
    assert!(loaded(&s, 2), "the zone stays: an anchor is still near it");
    s.world.get_mut::<Transform>(s.world.entity(3).unwrap()).unwrap().pos = [-500.0, 0.0, 500.0];
    s.step();s.step();
    assert!(!loaded(&s, 2), "and goes when the last has left");
    // an anchor that is switched off is no anchor
    s.world.get_mut::<Transform>(s.world.entity(4).unwrap()).unwrap().pos = [0.0, 0.0, 0.0];
    s.step();s.step();
    assert!(loaded(&s, 1));
    s.world.get_mut::<components::StreamAnchor>(s.world.entity(4).unwrap()).unwrap().on = false;
    s.step();s.step();
    assert!(!loaded(&s, 1));
}

#[test]
fn a_stream_zone_with_a_min_radius_is_a_ring_that_is_loaded_only_far_enough_away() {
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "Far".into(), def: parse(r#"{"components":{"Name":{"value":"Far"},"Transform":{}}}"#).unwrap() }).unwrap();
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"StreamZone":{"prefab":"Far","radius":1000,"min_radius":300,"hysteresis":50,"target":2}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[100,0,0]}}"#);
    let at = |s: &mut Sim, x: f32| {
        s.world.get_mut::<Transform>(s.world.entity(2).unwrap()).unwrap().pos = [x, 0.0, 0.0];
        s.step();
        s.world.get::<components::StreamZone>(s.world.entity(1).unwrap()).unwrap().loaded
    };
    assert!(!at(&mut s, 100.0), "nearer than the ring: not loaded");
    assert!(at(&mut s, 400.0), "inside the ring");
    assert!(at(&mut s, 280.0), "a little nearer than the ring, inside the margin: it stays");
    assert!(!at(&mut s, 200.0), "nearer than min_radius minus the margin: gone");
    assert!(!at(&mut s, 280.0), "and it does not come back until the target is out in the ring again");
    assert!(at(&mut s, 350.0));
    assert!(at(&mut s, 1040.0), "beyond the outer edge but inside the margin: it stays");
    assert!(!at(&mut s, 1100.0), "beyond radius plus the margin: gone");
}

#[test]
fn a_stream_zone_that_requires_another_loads_after_it_and_leaves_with_it() {
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "Flag".into(), def: parse(r#"{"components":{"Name":{"value":"Flag"},"Transform":{}}}"#).unwrap() }).unwrap();
    s.apply(&Command::DefinePrefab { name: "Tile".into(), def: parse(r#"{"components":{"Name":{"value":"Tile"},"Transform":{}}}"#).unwrap() }).unwrap();
    // the near ring: a flag that is loaded within 50 m; a coarse tile in the ring beyond; a fine tile that is there only while the flag is
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"StreamZone":{"prefab":"Flag","radius":50,"hysteresis":10,"target":9}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"StreamZone":{"prefab":"Tile","radius":1000,"min_radius":60,"hysteresis":10,"target":9}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0,0]},"StreamZone":{"prefab":"Tile","radius":1000,"hysteresis":10,"target":9,"requires":1}}"#);
    spawn(&mut s, 9, r#"{"Transform":{"pos":[500,0,0]}}"#);
    let loaded = |s: &Sim| -> Vec<bool> { (1..=3).map(|u| s.world.get::<components::StreamZone>(s.world.entity(u).unwrap()).unwrap().loaded).collect() };
    let at = |s: &mut Sim, x: f32| {
        s.world.get_mut::<Transform>(s.world.entity(9).unwrap()).unwrap().pos = [x, 0.0, 0.0];
        s.step();
        s.step();
        loaded(s)
    };
    assert_eq!(at(&mut s, 500.0), [false, true, false], "far: the coarse tile only");
    assert_eq!(at(&mut s, 40.0), [true, false, true], "near: the flag, and the fine tile that needs it; the coarse one is gone");
    assert_eq!(at(&mut s, 500.0), [false, true, false], "and back");
    // a zone that requires one that does not exist never loads
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,0,0]},"StreamZone":{"prefab":"Tile","radius":1000,"target":9,"requires":77}}"#);
    s.step_n(3);
    assert!(!s.world.get::<components::StreamZone>(s.world.entity(4).unwrap()).unwrap().loaded);
}

#[test]
fn stream_zones_load_one_a_tick_nearest_first_when_the_world_says_so() {
    let mk = |limit: Option<u32>| {
        let mut s = Sim::new(1);
        s.apply(&Command::DefinePrefab { name: "Rock".into(), def: parse(r#"{"components":{"Name":{"value":"Rock"},"Transform":{}}}"#).unwrap() }).unwrap();
        // three zones at 10, 30 and 20 metres from the target, in this uid order
        spawn(&mut s, 1, r#"{"Transform":{"pos":[10,0,0]},"StreamZone":{"prefab":"Rock","radius":100,"target":4}}"#);
        spawn(&mut s, 2, r#"{"Transform":{"pos":[30,0,0]},"StreamZone":{"prefab":"Rock","radius":100,"target":4}}"#);
        spawn(&mut s, 3, r#"{"Transform":{"pos":[20,0,0]},"StreamZone":{"prefab":"Rock","radius":100,"target":4}}"#);
        spawn(&mut s, 4, r#"{"Transform":{"pos":[0,0,0]}}"#);
        if let Some(l) = limit {
            s.world.set_resource("stream.loads_per_tick", Some(l as f64)).unwrap();
        }
        s
    };
    let loaded = |s: &Sim| -> Vec<bool> { (1..=3).map(|u| s.world.get::<components::StreamZone>(s.world.entity(u).unwrap()).unwrap().loaded).collect() };
    let mut free = mk(None);
    free.step();
    assert_eq!(loaded(&free), [true, true, true], "no limit: all at once");
    let mut one = mk(Some(1));
    one.step();
    assert_eq!(loaded(&one), [true, false, false], "the nearest first");
    one.step();
    assert_eq!(loaded(&one), [true, false, true], "then the next nearest");
    one.step();
    assert_eq!(loaded(&one), [true, true, true]);
    let before = one.world.len();
    one.step_n(5);
    assert_eq!(one.world.len(), before, "nothing loads twice");
    let mut two = mk(Some(2));
    two.step();
    assert_eq!(loaded(&two), [true, false, true], "two a tick");
    // the same world played twice is the same world
    let (mut a, mut b) = (mk(Some(1)), mk(Some(1)));
    a.step_n(4);
    b.step_n(4);
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn health_regenerates_after_a_pause_protects_in_grace_and_dies_once() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[1,2,3]},"HitPoints":{"value":100,"max":100,"regen":10,"regen_delay":1,"remove_after":0.5}}"#);
    let hp = |s: &Sim| *s.world.get::<components::HitPoints>(s.world.entity(1).unwrap()).unwrap();
    let hit = |s: &mut Sim, v: f32| s.world.get_mut::<components::HitPoints>(s.world.entity(1).unwrap()).unwrap().value = v;
    s.step_n(5);
    assert_eq!(hp(&s).value, 100.0);
    hit(&mut s, 60.0);
    s.step();
    assert_eq!(hp(&s).hurt, 0.0, "a hit resets the time since the last one");
    s.step_n(30);
    assert!((hp(&s).value - 60.0).abs() < 1e-3, "no regeneration during the pause: {}", hp(&s).value);
    s.step_n(60); // 1.5 s more: the pause (1 s) ended half a second ago, 5 points
    assert!(hp(&s).value > 60.0 && hp(&s).value < 70.0, "{}", hp(&s).value);
    s.step_n(600);
    assert_eq!(hp(&s).value, 100.0, "never above the maximum");
    // grace: damage is undone while `invulnerable` lasts (once it has begun to run)
    s.world.get_mut::<components::HitPoints>(s.world.entity(1).unwrap()).unwrap().invulnerable = 0.5;
    s.step();
    hit(&mut s, 70.0);
    s.step();
    assert_eq!(hp(&s).value, 100.0);
    s.step_n(40);
    hit(&mut s, 70.0);
    s.step();
    assert_eq!(hp(&s).value, 70.0, "the grace is over");
    // a hit and the grace it earns, set in the same tick (what a contact system does): the hit stays, the next ones are refused until the grace is over
    s.step_n(5);
    hit(&mut s, 50.0);
    s.world.get_mut::<components::HitPoints>(s.world.entity(1).unwrap()).unwrap().invulnerable = 0.5;
    s.step();
    assert_eq!(hp(&s).value, 50.0, "the hit that earned the grace counts");
    hit(&mut s, 30.0);
    s.step();
    assert_eq!(hp(&s).value, 50.0, "the next one is refused");
    s.step_n(40);
    hit(&mut s, 30.0);
    s.step();
    assert_eq!(hp(&s).value, 30.0, "after the grace the next one counts");
    // death: once, with an event and a lifetime that removes the entity
    hit(&mut s, 0.0);
    s.step();
    assert!(hp(&s).dead);
    assert_eq!(s.events.iter().filter(|e| e.name == "died").count(), 1);
    assert_eq!(s.events.iter().find(|e| e.name == "died").unwrap().pos, Some([1.0, 2.0, 3.0]));
    s.step_n(10);
    assert_eq!(s.events.iter().filter(|e| e.name == "died").count(), 1, "only once");
    hit(&mut s, 50.0);
    s.step();
    assert!(hp(&s).dead, "the dead stay dead (regeneration does not revive)");
    s.step_n(40);
    assert!(s.world.entity(1).is_none(), "removed half a second after death");
}

fn tree_world(script: &str) -> Sim {
    let mut s = Sim::new(1);
    let text = script.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{}},"BehaviorTree":{{"script":"{text}"}}}}"#));
    s
}

fn bt_status(s: &Sim) -> u32 {
    s.world.get::<components::BehaviorTree>(s.world.entity(1).unwrap()).unwrap().status
}

#[test]
fn a_behaviour_tree_picks_by_priority_waits_and_remembers_where_it_was() {
    let tree = "selector\n  sequence\n    cond $alert > 0\n    act res chasing += 1\n    wait 0.5\n    act event caught\n  sequence\n    act res patrolling += 1\n    wait 0.2";
    let mut s = tree_world(tree);
    s.step_n(60);
    let patrol = res(&s, "patrolling");
    assert!(patrol >= 4.0 && patrol <= 6.0, "a patrol every 0.2 s: {patrol}");
    assert!(res(&s, "chasing").is_nan(), "never chased");
    // the alarm: the running patrol round finishes, then the chase has priority
    setr(&mut s, "alert", 1.0);
    s.step_n(120);
    assert!(res(&s, "chasing") >= 2.0, "{}", res(&s, "chasing"));
    let caught = s.events.iter().filter(|e| e.name == "caught").count();
    assert!(caught >= 1 && caught as f64 <= res(&s, "chasing"), "caught {caught}");
    let before = res(&s, "patrolling");
    s.step_n(60);
    assert_eq!(res(&s, "patrolling"), before, "no more patrolling while the alarm is on");
    // after the alarm the patrol starts again
    setr(&mut s, "alert", 0.0);
    s.step_n(90);
    assert!(res(&s, "patrolling") > before);
    assert!(matches!(bt_status(&s), 1 | 2));
}

#[test]
fn decorators_and_parallel_work() {
    // repeat 3 times, then succeed
    let mut r = tree_world("repeat 3\n  act res n += 1");
    r.step();
    assert_eq!((res(&r, "n"), bt_status(&r)), (1.0, 1));
    r.step_n(2);
    assert_eq!((res(&r, "n"), bt_status(&r)), (3.0, 2), "three rounds, then success");
    // invert and succeed
    let mut i = tree_world("sequence\n  invert\n    cond $never > 0\n  succeed\n    cond $never > 0\n  act res ok += 1");
    i.step();
    assert_eq!((res(&i, "ok"), bt_status(&i)), (1.0, 2));
    // cooldown: the action happens, then is refused for a second
    let mut c = tree_world("cooldown 1\n  act res shots += 1");
    c.step_n(150); // 2.5 s: shots at 0, at 1.0 and at 2.0
    assert_eq!(res(&c, "shots"), 3.0);
    // parallel: both children every tick; one failing fails it
    let mut p = tree_world("parallel\n  act res a += 1\n  cond $go > 0");
    p.step();
    assert_eq!((res(&p, "a"), bt_status(&p)), (1.0, 3));
    setr(&mut p, "go", 1.0);
    p.step();
    assert_eq!((res(&p, "a"), bt_status(&p)), (2.0, 2));
    // editing the text starts the tree afresh; a disabled tree does nothing
    let mut e = tree_world("sequence\n  wait 5");
    e.step_n(10);
    assert_eq!(bt_status(&e), 1);
    e.world.get_mut::<components::BehaviorTree>(e.world.entity(1).unwrap()).unwrap().enabled = false;
    let mem_before = e.scripts.bt.clone();
    e.step_n(20);
    assert_eq!(e.scripts.bt, mem_before, "a disabled tree does not run or change");
}

#[test]
fn behaviour_tree_text_reports_what_is_wrong_and_the_tree_is_deterministic() {
    assert!(crate::gameplay::parse_tree("selector\n  act event x").is_ok());
    for (bad, word) in [("", "empty"), ("sequence", "children"), ("act event x\n  act event y", "children"), ("dance", "not a node"), ("selector\n  cond $x >", "line 2"), ("selector\n\tact event x", "tabs"), ("selector\n  act event a\nselector\n  act event b", "one root"), ("repeat 2\n  act event a\n  act event b", "children")] {
        let e = crate::gameplay::parse_tree(bad).unwrap_err();
        assert!(e.contains(word), "{bad:?} gave {e:?}");
    }
    let run = || {
        let mut s = tree_world("sequence\n  act res a += 1\n  wait 0.3\n  repeat 2\n    act res b += 1");
        s.step_n(200);
        s
    };
    let (a, b) = (run(), run());
    assert_eq!(a.state_hash(), b.state_hash());
    let mut half = tree_world("sequence\n  act res a += 1\n  wait 0.3\n  repeat 2\n    act res b += 1");
    half.step_n(100);
    let mut c = half.clone();
    half.step_n(100);
    c.step_n(100);
    assert_eq!(half.state_hash(), c.state_hash());
}

#[test]
fn water_waves_run_with_the_simulation_and_are_deterministic() {
    let build = || {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1,0]},"Water":{"speed":2}}"#);
        s
    };
    let phase = |s: &Sim| s.world.get::<components::Water>(s.world.entity(1).unwrap()).unwrap().phase;
    let mut a = build();
    a.step_n(60);
    assert!((phase(&a) - 2.0).abs() < 1e-3, "{}", phase(&a));
    a.step_n(60);
    let mut b = build();
    b.step_n(120);
    assert_eq!(a.state_hash(), b.state_hash());
    // paused with the world
    a.world.set_resource("time.scale", Some(0.0)).unwrap();
    let before = phase(&a);
    a.step_n(30);
    assert_eq!(phase(&a), before);
}

#[test]
fn animation_events_fire_each_time_a_moment_of_the_clip_is_passed_and_not_after_a_switch() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[1,2,3]},"Animator":{"clip":"walk","looping":true},"AnimEvents":{"events":"walk length=0.8: 0.1 step_left, 0.5 step_right\nattack length=0.6: 0.25 hit"}}"#);
    let count = |s: &Sim, name: &str| s.events.iter().filter(|e| e.name == name).count();
    s.step_n(140); // 2.33 s: the moments 0.1, 0.9, 1.7 and 0.5, 1.3, 2.1
    assert_eq!((count(&s, "step_left"), count(&s, "step_right"), count(&s, "hit")), (3, 3, 0));
    let e = s.events.iter().find(|e| e.name == "step_left").unwrap();
    assert_eq!((e.uid, e.pos), (1, Some([1.0, 2.0, 3.0])), "from the entity, at its place");
    // another clip: its own moments only, and the switch itself passes nothing
    s.events.clear();
    s.apply(&Command::Set { uid: 1, component: "Animator".into(), value: parse(r#"{"clip":"attack","time":0.0,"looping":false}"#).unwrap() }).unwrap();
    s.step_n(60);
    assert_eq!((count(&s, "hit"), count(&s, "step_left")), (1, 0), "a clip that does not loop fires once");
    // scrubbing back fires nothing, playing on again passes the moment again
    s.events.clear();
    s.apply(&Command::Set { uid: 1, component: "Animator".into(), value: parse(r#"{"time":0.0}"#).unwrap() }).unwrap();
    s.step_n(2);
    assert_eq!(count(&s, "hit"), 0);
    s.step_n(30);
    assert_eq!(count(&s, "hit"), 1);
    // a moment at time 0 fires at the very start
    let mut t = Sim::new(1);
    spawn(&mut t, 1, r#"{"Animator":{"clip":"jump","looping":false},"AnimEvents":{"events":"jump length=1: 0 takeoff, 0.5 apex"}}"#);
    t.step_n(2);
    assert_eq!(t.events.iter().filter(|e| e.name == "takeoff").count(), 1);
    // bad text is reported by the parser
    for (bad, word) in [("walk: 0.1 x", "length"), ("walk length=0.8: 0.9 x", "must be from 0 to the clip's length"), ("walk length=0.8: 0.1", "TIME name"), ("walk 0.1 x", "expected")] {
        let e = crate::gameplay::parse_anim_events(bad).unwrap_err();
        assert!(e.contains(word), "{bad:?}: {e}");
    }
}

#[test]
fn root_motion_moves_the_entity_by_how_far_the_clip_travels_and_carries_on_between_loops() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Animator":{"clip":"walk","looping":true},"RootMotion":{"curves":"walk length=1: 0 0 0 0, 0.5 0 0 0.5, 1 0 0 2"}}"#);
    s.step_n(90); // 1.5 s: one whole loop (2 m) and half of the next (0.5 m)
    let p = pos(&s, 1);
    assert!(p[0].abs() < 1e-4 && (p[2] - 2.5).abs() < 2e-3, "{p:?}");
    // turned a quarter round (y) the same travel goes along x; the scale stretches it
    let mut t = Sim::new(1);
    spawn(&mut t, 1, r#"{"Transform":{"pos":[0,0,0],"rot":[0,0.70710677,0,0.70710677]},"Animator":{"clip":"walk","looping":false},"RootMotion":{"curves":"walk length=1: 0 0 0 0, 1 0 0 2","scale":2}}"#);
    t.step_n(60);
    let q = pos(&t, 1);
    assert!((q[0] - 4.0).abs() < 2e-3 && q[2].abs() < 2e-3, "{q:?}");
    // a clip that does not loop stops where it ends; a clip with no curves does not move the entity
    t.step_n(60);
    assert!((pos(&t, 1)[0] - 4.0).abs() < 2e-3);
    let mut u = Sim::new(1);
    spawn(&mut u, 1, r#"{"Transform":{},"Animator":{"clip":"idle"},"RootMotion":{"curves":"walk length=1: 0 0 0 0, 1 0 0 2"}}"#);
    u.step_n(30);
    assert_eq!(pos(&u, 1), [0.0; 3]);
    for (bad, word) in [("walk length=1: 0 0 0 0", "at least two keys"), ("walk length=1: 0 0 0 0, 0 1 1 1", "increase"), ("walk length=1: 0 0 0 0, 1 a b c", "not a number"), ("walk length=1: 0 0, 1 0 0 0", "TIME x y z")] {
        let e = crate::gameplay::parse_root_motion(bad).unwrap_err();
        assert!(e.contains(word), "{bad:?}: {e}");
    }
}

#[test]
fn watched_events_become_resources_for_the_tick_after_they_happen() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"EventWatch":{"names":"ping, boom nope-name"}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[4,5,6]}}"#);
    // a system that emits `ping` at tick 3, and one that counts what it sees
    s.apply(&Command::DefineSystem { def: parse(r#"{"name":"pinger","each":["Transform"],"do":[{"if":{"eq":[{"tick":true},3]},"then":[{"emit":"ping"}]}]}"#).unwrap() }).unwrap();
    s.apply(&Command::DefineSystem { def: parse(r#"{"name":"counter","each":["Transform"],"do":[{"if":{"gt":[{"res":"event.ping"},0]},"then":[{"res_add":"seen","by":1}]}]}"#).unwrap() }).unwrap();
    s.step_n(3);
    assert_eq!(res(&s, "event.ping"), 0.0, "not yet: it is emitted during tick 3");
    s.step(); // tick 3: emitted
    assert_eq!(res(&s, "event.ping"), 0.0);
    s.step(); // tick 4: the resource says it happened in the tick before
    assert_eq!(res(&s, "event.ping"), 1.0);
    assert_eq!((res(&s, "event.ping.x"), res(&s, "event.ping.y"), res(&s, "event.ping.z")), (4.0, 5.0, 6.0), "where it was emitted");
    assert_eq!(res(&s, "event.ping.uid"), 2.0);
    s.step_n(3);
    assert_eq!(res(&s, "event.ping"), 0.0, "and it is 0 again");
    assert!(res(&s, "seen") >= 1.0, "the counting system saw it: {}", res(&s, "seen"));
    assert!(s.world.resource("event.boom").is_some() && s.world.resource("event.nope-name").is_none(), "only valid names are published");
    // a scene without an EventWatch gets no event resources
    let mut t = Sim::new(1);
    spawn(&mut t, 2, r#"{"Transform":{}}"#);
    t.apply(&Command::DefineSystem { def: parse(r#"{"name":"pinger","each":["Transform"],"do":[{"emit":"ping"}]}"#).unwrap() }).unwrap();
    t.step_n(5);
    assert!(t.world.resource("event.ping").is_none());
}


#[test]
fn several_events_of_a_name_are_told_apart_in_slots_and_the_first_is_still_where_it_was() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"EventWatch":{"names":"ping"}}"#);
    for uid in 2..=7u64 {
        spawn(&mut s, uid, &format!(r#"{{"Transform":{{"pos":[{},{},{}]}}}}"#, uid * 10, uid, -(uid as i64)));
    }
    s.apply(&Command::DefineSystem { def: parse(r#"{"name":"pinger","each":["Transform"],"do":[{"if":{"eq":[{"tick":true},3]},"then":[{"emit":"ping"}]}]}"#).unwrap() }).unwrap();
    s.step_n(3);
    assert!(s.world.resource("event.ping.n").is_none(), "no event yet: the slot resources do not exist");
    s.step(); // tick 3: six pings
    s.step(); // tick 4: told
    assert_eq!(res(&s, "event.ping"), 6.0, "all of them are counted");
    assert_eq!(res(&s, "event.ping.n"), 4.0, "four slots are filled");
    let slot = |k: usize| (res(&s, &format!("event.ping.{k}.uid")), res(&s, &format!("event.ping.{k}.x")), res(&s, &format!("event.ping.{k}.y")), res(&s, &format!("event.ping.{k}.z")));
    let uids: Vec<f64> = (0..4).map(|k| slot(k).0).collect();
    assert!(uids.windows(2).all(|w| w[0] < w[1]) && uids.iter().all(|u| (2.0..=7.0).contains(u)), "four different emitters, in uid order: {uids:?}");
    for k in 0..4 {
        let (u, x, y, z) = slot(k);
        assert_eq!((x, y, z), (u * 10.0, u, -u), "slot {k} is where its own emitter was");
    }
    assert_eq!(slot(0), (res(&s, "event.ping.uid"), res(&s, "event.ping.x"), res(&s, "event.ping.y"), res(&s, "event.ping.z")), "slot 0 is the first event: `.x .y .z .uid` as before");
    assert!(s.world.resource("event.ping.4.x").is_none(), "there are only four slots");
    // the next tick has none: n goes back to 0 (once it exists) and the count too
    s.step();
    assert_eq!((res(&s, "event.ping"), res(&s, "event.ping.n")), (0.0, 0.0));
    // fewer than four fill fewer slots
    let mut t = Sim::new(1);
    spawn(&mut t, 1, r#"{"EventWatch":{"names":"ping"}}"#);
    spawn(&mut t, 2, r#"{"Transform":{"pos":[5,0,0]}}"#);
    spawn(&mut t, 3, r#"{"Transform":{"pos":[6,0,0]}}"#);
    t.apply(&Command::DefineSystem { def: parse(r#"{"name":"pinger","each":["Transform"],"do":[{"if":{"eq":[{"tick":true},1]},"then":[{"emit":"ping"}]}]}"#).unwrap() }).unwrap();
    t.step_n(3);
    assert_eq!((res(&t, "event.ping.n"), res(&t, "event.ping.1.x")), (2.0, 6.0), "two events fill two slots");
    assert!(t.world.resource("event.ping.2.x").is_none());
}

#[test]
fn while_a_text_field_has_the_focus_the_actions_read_zero_and_come_back_with_it() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"InputMap":{"bindings":"reload = r\nmove_y = w, s:-1"}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.1],"button":true},"UiInput":{"max_len":20}}"#);
    s.step();
    setr(&mut s, "input.w", 1.0);
    s.step();
    assert_eq!(res(&s, "action.move_y"), 1.0, "no field has the focus: the key is an action");
    // the field takes the focus (a game's own system does that when the chat key is pressed)
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<components::UiInput>(e).unwrap().focused = true;
    s.step();
    assert_eq!((res(&s, "action.move_y"), res(&s, "action.move_y.released")), (0.0, 1.0), "typing: the held key is released and reads 0");
    setr(&mut s, "input.r", 1.0);
    s.step();
    assert_eq!((res(&s, "action.reload"), res(&s, "action.reload.pressed")), (0.0, 0.0), "a key pressed while typing is no action");
    // the focus goes: what is still held is an action again
    s.world.get_mut::<components::UiInput>(e).unwrap().focused = false;
    s.step();
    assert_eq!((res(&s, "action.move_y"), res(&s, "action.reload")), (1.0, 1.0));
}

#[test]
fn a_crowd_of_anchors_loads_and_drops_exactly_the_zones_and_cells_that_measuring_every_anchor_would() {
    // the server's way of working: sixty players spread over three kilometres, two hundred loose zones and a grid of cells; after every step the zones that are loaded and the cells that exist are what a plain measurement of every
    // anchor against every zone (with the margin that keeps a loaded one) gives, as the players move about
    use crate::procgen::{self, Provider, ZoneDef};
    use std::sync::Arc;
    struct Cells;
    impl Provider for Cells {
        fn claims(&self, _: &str) -> bool {
            false
        }
        fn file(&self, _: &str) -> Option<Result<Vec<u8>, String>> {
            None
        }
        fn file_ready(&self, _: &str) -> bool {
            true
        }
        fn prefetch_file(&self, _: &str) {}
        fn prefab(&self, _: &str) -> Option<String> {
            None
        }
        fn zones(&self, _: &str, i: i32, j: i32) -> Vec<ZoneDef> {
            vec![ZoneDef { name: format!("cell {i} {j}"), pos: [(i as f32 + 0.5) * 100.0, 0.0, (j as f32 + 0.5) * 100.0], prefab: "gen:none".into(), radius: 60.0, hysteresis: 10.0, ..Default::default() }]
        }
    }
    let _g = procgen::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    procgen::set_provider(Some(Arc::new(Cells)));
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "House".into(), def: parse(r#"{"components":{"Name":{"value":"House"},"Transform":{}}}"#).unwrap() }).unwrap();
    let mut seed = 12345u64;
    let mut rnd = |m: f32| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (((seed >> 33) as f32 / (1u64 << 31) as f32) * m).floor()
    };
    let mut anchors: Vec<[f32; 2]> = (0..60).map(|_| [rnd(3000.0), rnd(3000.0)]).collect();
    for (k, p) in anchors.iter().enumerate() {
        spawn(&mut s, 10 + k as u64, &format!(r#"{{"Transform":{{"pos":[{},1,{}]}},"StreamAnchor":{{}}}}"#, p[0], p[1]));
    }
    let zones: Vec<([f32; 2], f32)> = (0..200).map(|_| ([rnd(3000.0), rnd(3000.0)], if rnd(2.0) < 1.0 { 50.0 } else { 150.0 })).collect();
    for (k, (p, r)) in zones.iter().enumerate() {
        spawn(&mut s, 1000 + k as u64, &format!(r#"{{"Transform":{{"pos":[{},0,{}]}},"StreamZone":{{"prefab":"House","radius":{r},"hysteresis":20}}}}"#, p[0], p[1]));
    }
    spawn(&mut s, 2, r#"{"ProcGrid":{"grid":"test","cell":100,"radius":250,"hysteresis":60,"per_tick":100000}}"#);
    let dist = |a: &[f32; 2], p: [f32; 2]| ((a[0] - p[0]).powi(2) + (a[1] - p[1]).powi(2)).sqrt();
    let mut loaded = vec![false; zones.len()];
    let mut cells: std::collections::BTreeSet<(i32, i32)> = std::collections::BTreeSet::new();
    let (mut ever_loaded, mut ever_gone) = (0, 0);
    for round in 0..6 {
        s.step();
        for (k, (p, r)) in zones.iter().enumerate() {
            let near = |lim: f32| anchors.iter().any(|a| dist(a, *p) <= lim);
            let was = loaded[k];
            loaded[k] = if was { near(r + 20.0) } else { near(*r) };
            ever_loaded += (!was && loaded[k]) as u32;
            ever_gone += (was && !loaded[k]) as u32;
            let actual = s.world.get::<components::StreamZone>(s.world.entity(1000 + k as u64).unwrap()).unwrap().loaded;
            assert_eq!(actual, loaded[k], "round {round}: the zone {k} at {p:?} radius {r}");
        }
        // the cells: those that no anchor is within 310 m of are dropped, those within 250 m of an anchor are made (all at once: `per_tick` is large)
        cells.retain(|&(i, j)| anchors.iter().any(|a| dist(a, [(i as f32 + 0.5) * 100.0, (j as f32 + 0.5) * 100.0]) <= 310.0));
        for j in -6..40 {
            for i in -6..40 {
                if anchors.iter().any(|a| dist(a, [(i as f32 + 0.5) * 100.0, (j as f32 + 0.5) * 100.0]) <= 250.0) {
                    cells.insert((i, j));
                }
            }
        }
        let mut actual: std::collections::BTreeSet<(i32, i32)> = std::collections::BTreeSet::new();
        let _ = s.world.clone().for_each_uid::<(&components::GridZone,), _>(|_, _, (z,)| {
            actual.insert((z.i as i32, z.j as i32));
        });
        assert_eq!(actual, cells, "round {round}: the cells of the grid");
        // everybody walks up to 600 m; some far enough to leave the zones they were in
        for (k, a) in anchors.iter_mut().enumerate() {
            a[0] = (a[0] + rnd(1200.0) - 600.0).clamp(0.0, 3000.0);
            a[1] = (a[1] + rnd(1200.0) - 600.0).clamp(0.0, 3000.0);
            s.world.get_mut::<Transform>(s.world.entity(10 + k as u64).unwrap()).unwrap().pos = [a[0], 1.0, a[1]];
        }
    }
    assert!(ever_loaded > 20 && ever_gone > 20, "the test moved zones in and out: {ever_loaded} loaded, {ever_gone} dropped");
    procgen::set_provider(None);
}

#[test]
fn a_proc_grid_can_look_at_its_cells_only_every_few_steps() {
    use crate::procgen::{self, Provider, ZoneDef};
    use std::sync::Arc;
    struct Cells;
    impl Provider for Cells {
        fn claims(&self, _: &str) -> bool {
            false
        }
        fn file(&self, _: &str) -> Option<Result<Vec<u8>, String>> {
            None
        }
        fn file_ready(&self, _: &str) -> bool {
            true
        }
        fn prefetch_file(&self, _: &str) {}
        fn prefab(&self, _: &str) -> Option<String> {
            None
        }
        fn zones(&self, _: &str, i: i32, j: i32) -> Vec<ZoneDef> {
            vec![ZoneDef { name: format!("cell {i} {j}"), pos: [(i as f32 + 0.5) * 100.0, 0.0, (j as f32 + 0.5) * 100.0], prefab: "gen:none".into(), radius: 60.0, hysteresis: 10.0, ..Default::default() }]
        }
    }
    let _g = procgen::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    procgen::set_provider(Some(Arc::new(Cells)));
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[50,1,50]},"StreamAnchor":{}}"#);
    spawn(&mut s, 2, r#"{"ProcGrid":{"grid":"test","cell":100,"radius":150,"hysteresis":50,"per_tick":50}}"#);
    s.world.set_resource("procgrid.every", Some(4.0)).unwrap();
    let cells = |s: &Sim| {
        let mut n = 0;
        let _ = s.world.clone().for_each_uid::<(&components::GridZone,), _>(|_, _, _| n += 1);
        n
    };
    // the first step is a step to look in (tick 0), the next three are not
    s.step();
    assert_eq!(cells(&s), 9);
    s.world.get_mut::<Transform>(s.world.entity(1).unwrap()).unwrap().pos = [5050.0, 1.0, 50.0];
    s.step_n(3);
    assert_eq!(s.world.resource("procgrid.every"), Some(4.0));
    let mid = cells(&s);
    s.step_n(4);
    assert_eq!(cells(&s), 9, "after a step that looks, the nine cells round the anchor's new place (those it left are gone) ({mid} between)");
    procgen::set_provider(None);
}

/// The way `despawn_trees` found a tree before: a map of every child of every parent in the world, and a walk of it.
fn doomed_reference(world: &mut lunaris_ecs::World, roots: Vec<u64>) -> Vec<u64> {
    let mut children: std::collections::BTreeMap<u64, Vec<u64>> = std::collections::BTreeMap::new();
    let _ = world.for_each_uid::<(&components::Parent,), _>(|_, uid, (p,)| children.entry(p.target.0).or_default().push(uid));
    let mut doomed: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
    let mut stack = roots;
    while let Some(u) = stack.pop() {
        if !doomed.insert(u) {
            continue;
        }
        if let Some(kids) = children.get(&u) {
            stack.extend(kids.iter().copied());
        }
    }
    doomed.into_iter().collect()
}

#[test]
fn the_tree_found_for_a_despawn_is_the_one_a_map_of_all_children_finds_and_costs_less() {
    let mut s = Sim::new(1);
    // 400 buildings, each with 20 parts, some of the parts with parts of their own, and 30 000 loose entities with no parent
    let mut next = 1u64;
    let mut roots = Vec::new();
    for b in 0..400u64 {
        let root = next;
        next += 1;
        spawn(&mut s, root, r#"{"Transform":{}}"#);
        if b % 20 == 3 {
            roots.push(root);
        }
        for k in 0..20u64 {
            let part = next;
            next += 1;
            spawn(&mut s, part, &format!(r#"{{"Transform":{{}},"Parent":{{"target":{root}}}}}"#));
            if k % 5 == 0 {
                let sub = next;
                next += 1;
                spawn(&mut s, sub, &format!(r#"{{"Transform":{{}},"Parent":{{"target":{part}}}}}"#));
            }
        }
    }
    for _ in 0..30_000 {
        spawn(&mut s, next, r#"{"Transform":{}}"#);
        next += 1;
    }
    // a part of one building as a root too (its own subtree is inside another root's), a root that does not exist, and a repeated one
    roots.push(8);
    roots.push(9_999_999);
    roots.push(roots[0]);
    let a = crate::gameplay::doomed_trees(&mut s.world, roots.clone());
    let b = doomed_reference(&mut s.world, roots.clone());
    assert_eq!(a, b);
    assert!(a.len() > 20 * 20 && a.windows(2).all(|w| w[0] < w[1]), "{}", a.len());
    // the same in a world with no parents, and with no roots
    assert_eq!(crate::gameplay::doomed_trees(&mut s.world, Vec::new()), Vec::<u64>::new());
    // the cost (printed; run with --nocapture)
    let time = |f: &mut dyn FnMut()| {
        let t = std::time::Instant::now();
        for _ in 0..20 {
            f();
        }
        t.elapsed().as_secs_f64() * 1000.0 / 20.0
    };
    let new_ms = time(&mut || {
        crate::gameplay::doomed_trees(&mut s.world, roots.clone());
    });
    let old_ms = time(&mut || {
        doomed_reference(&mut s.world, roots.clone());
    });
    eprintln!("finding the tree of {} roots among {} parents: {new_ms:.3} ms now, {old_ms:.3} ms with a map of all children", roots.len(), 400 * 20 + 400 * 4);
    // and despawning really removes them all and nothing else
    let before = s.world.len();
    crate::gameplay::despawn_trees(&mut s.world, roots);
    assert_eq!(s.world.len(), before - a.iter().filter(|u| **u < next).count());
}

#[test]
fn the_cells_near_the_anchors_are_the_same_in_a_flat_array_and_in_a_map() {
    use crate::gameplay::cells_near_anchors;
    let cell = 100.0f32;
    let from = |i: i32, j: i32, c: &[f32; 3]| {
        let (x, z) = ((i as f32 + 0.5) * cell, (j as f32 + 0.5) * cell);
        ((x - c[0]).powi(2) + (z - c[2]).powi(2)).sqrt()
    };
    let mut state = 12345u64;
    let mut next = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as f32 / (1u64 << 31) as f32
    };
    let anchors: Vec<[f32; 3]> = (0..60).map(|_| [next() * 4000.0 - 2000.0, 0.0, next() * 4000.0 - 2000.0]).collect();
    for r in [150.0f32, 400.0, 900.0] {
        let mut dense = cells_near_anchors(&anchors, r, cell, usize::MAX, from);
        let mut map = cells_near_anchors(&anchors, r, cell, 0, from);
        dense.sort_by_key(|a| a.0);
        map.sort_by_key(|a| a.0);
        assert!(!dense.is_empty());
        assert_eq!(dense, map, "radius {r}: the same cells with the same distances");
    }
    // anchors a thousand kilometres apart: the box round them is far too big for an array, and a map does it; an anchor that is not a number has no cells
    let far = [[0.0f32, 0.0, 0.0], [1.0e9, 0.0, 1.0e9], [f32::NAN, 0.0, 0.0]];
    assert!(!cells_near_anchors(&far, 150.0, cell, 1 << 22, from).is_empty());
    assert!(cells_near_anchors(&[[f32::NAN, 0.0, 0.0]], 150.0, cell, 1 << 22, from).is_empty());
    assert!(cells_near_anchors(&[], 150.0, cell, 1 << 22, from).is_empty());
}

/// `cargo test --release -p lunaris-sim cells_near_anchors_cost -- --ignored --nocapture`: what a call costs for the grids of a city server with two hundred players.
#[test]
#[ignore]
fn cells_near_anchors_cost() {
    use crate::gameplay::cells_near_anchors;
    let mut state = 99u64;
    let mut next = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as f32 / (1u64 << 31) as f32
    };
    let anchors: Vec<[f32; 3]> = (0..210).map(|_| [next() * 1660.0 - 830.0, 0.0, next() * 1660.0 - 830.0]).collect();
    for (name, cell, r) in [("s", 102.2f32, 362.2f32), ("r", 102.2, 183.4), ("b0", 102.2, 317.2), ("b1", 102.2, 562.2), ("c2", 408.8, 4506.6), ("in", 102.2, 197.4), ("t", 102.2, 247.2)] {
        let from = |i: i32, j: i32, c: &[f32; 3]| {
            let (x, z) = ((i as f32 + 0.5) * cell, (j as f32 + 0.5) * cell);
            ((x - c[0]).powi(2) + (z - c[2]).powi(2)).sqrt()
        };
        for (what, limit) in [("flat", usize::MAX), ("map", 0usize)] {
            let t = std::time::Instant::now();
            let mut n = 0;
            for _ in 0..20 {
                n = cells_near_anchors(&anchors, r, cell, limit, from).len();
            }
            println!("{name:>3} {what}: {:8.3} ms a call, {n} cells", t.elapsed().as_secs_f64() * 1e3 / 20.0);
        }
    }
}
