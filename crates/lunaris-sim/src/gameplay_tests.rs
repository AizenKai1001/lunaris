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

