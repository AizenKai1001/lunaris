//! The AI toolkit: sight, hearing, awareness, memory, squad alerts, the autopilot, and the searchlight with its exposure. No art: capsules, boxes and nothing drawn.
use super::*;
use crate::components::{Exposure, HitPoints, Perception, Searchlight, Transform, Weapon};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn perception(s: &Sim, uid: u64) -> Perception {
    *s.world.get::<Perception>(s.world.entity(uid).unwrap()).unwrap()
}

fn set_pos(s: &mut Sim, uid: u64, pos: [f32; 3]) {
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<Transform>(e).unwrap().pos = pos;
}

fn pos_of(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn res(s: &Sim, name: &str) -> f64 {
    s.world.resource(name).unwrap_or(0.0)
}

/// An AI at (x, 0, z) looking down -z.
fn ai(s: &mut Sim, uid: u64, x: f32, z: f32, team: u32, extra: &str) {
    let sep = if extra.is_empty() { "" } else { "," };
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{x},0,{z}]}},"Team":{{"id":{team}}},"Perception":{{{extra}}}{sep}}}"#).replace(",}", "}"));
}

/// A capsule target (1.8 m, centre at 0.9) at (x, z).
fn target(s: &mut Sim, uid: u64, x: f32, z: f32, team: u32) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{x},0.9,{z}]}},"Collider":{{"shape":2,"size":[0.7,1.8,0.7]}},"HitPoints":{{"value":1000,"max":1000}},"Team":{{"id":{team}}}}}"#));
}

#[test]
fn an_ai_notices_a_hostile_a_little_at_a_time_and_becomes_alert() {
    let mut s = Sim::new(1);
    ai(&mut s, 1, 0.0, 0.0, 2, r#""sight_range":30,"notice_time":1.0"#);
    target(&mut s, 2, 0.0, -10.0, 1);
    s.step_n(8);
    let p = perception(&s, 1);
    assert!(p.visible && p.awareness > 0.0 && p.awareness < 1.0 && p.state == 1, "suspicious first: {p:?}");
    s.step_n(40);
    let p = perception(&s, 1);
    assert_eq!((p.state, p.target.0, p.seen_count), (2, 2, 1), "alert: {p:?}");
    assert!((p.awareness - 1.0).abs() < 1e-6);
    assert!((p.distance - 10.0).abs() < 1.5, "{}", p.distance);
    assert!((p.seen_pos[2] + 10.0).abs() < 0.2, "it knows where the target is: {:?}", p.seen_pos);
}

#[test]
fn walls_backs_and_friends_hide_things_and_no_team_is_hostile_to_everybody() {
    // a wall between
    let mut s = Sim::new(1);
    ai(&mut s, 1, 0.0, 0.0, 2, "");
    target(&mut s, 2, 0.0, -10.0, 1);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,2,-5]},"Collider":{"shape":1,"size":[6,4,0.3]}}"#);
    s.step_n(60);
    assert_eq!((perception(&s, 1).state, perception(&s, 1).visible), (0, false), "a wall hides it");
    // the same without the wall is seen: take the wall away
    let e = s.world.entity(3).unwrap();
    s.world.despawn(e);
    s.step_n(60);
    assert_eq!(perception(&s, 1).state, 2);
    // behind its back (the field of view is 110 degrees)
    let mut s = Sim::new(1);
    ai(&mut s, 1, 0.0, 0.0, 2, "");
    target(&mut s, 2, 0.0, 10.0, 1);
    s.step_n(60);
    assert_eq!(perception(&s, 1).state, 0, "behind it");
    // a friend of its own team is ignored, a thing of no team is hostile to a machine
    let mut s = Sim::new(1);
    ai(&mut s, 1, 0.0, 0.0, 2, "");
    target(&mut s, 2, 0.0, -10.0, 2);
    s.step_n(60);
    assert_eq!(perception(&s, 1).state, 0, "its own side");
    let mut s = Sim::new(1);
    ai(&mut s, 1, 0.0, 0.0, 2, "");
    target(&mut s, 2, 0.0, -10.0, 0);
    s.step_n(60);
    assert_eq!(perception(&s, 1).state, 2, "a free-for-all player is hostile to a machine");
    // too far
    let mut s = Sim::new(1);
    ai(&mut s, 1, 0.0, 0.0, 2, r#""sight_range":15"#);
    target(&mut s, 2, 0.0, -25.0, 1);
    s.step_n(60);
    assert_eq!(perception(&s, 1).state, 0, "out of range");
}

#[test]
fn it_forgets_a_lost_target_and_searches_where_it_was_first() {
    let mut s = Sim::new(1);
    ai(&mut s, 1, 0.0, 0.0, 2, r#""lose_time":2.0"#);
    target(&mut s, 2, 0.0, -10.0, 1);
    s.step_n(60);
    assert_eq!(perception(&s, 1).state, 2);
    // the target steps behind it, out of sight
    set_pos(&mut s, 2, [0.0, 0.9, 12.0]);
    s.step_n(5);
    let p = perception(&s, 1);
    assert_eq!((p.state, p.visible), (3, false), "searching: {p:?}");
    assert!((p.seen_pos[2] + 10.0).abs() < 0.5, "where it was last seen: {:?}", p.seen_pos);
    assert!(p.since_seen > 0.0);
    s.step_n(160);
    assert_eq!(perception(&s, 1).state, 0, "after the lose time it gives up");
    assert_eq!(perception(&s, 1).awareness, 0.0);
}

#[test]
fn it_hears_noises_that_carry_and_a_gunshot_far_away() {
    // a noise behind its back: heard inside the radius, not beyond it, not by a deaf one
    let setup = |z: f32, radius: f32, hearing: f32| {
        let mut s = Sim::new(1);
        ai(&mut s, 1, 0.0, 0.0, 2, &format!(r#""hearing":{hearing}"#));
        spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,0,{z}]}},"Noisy":{{"radius":{radius}}},"Team":{{"id":1}}}}"#));
        s.step_n(3);
        s
    };
    let s = setup(30.0, 40.0, 1.0);
    let p = perception(&s, 1);
    assert!(p.heard && p.awareness >= 0.5 && p.state == 3, "{p:?}");
    assert!((p.seen_pos[2] - 30.0).abs() < 0.1, "it goes to where the noise was: {:?}", p.seen_pos);
    assert_eq!(p.target.0, 2);
    assert!(!perception(&setup(60.0, 40.0, 1.0), 1).heard, "too far");
    assert!(!perception(&setup(30.0, 40.0, 0.0), 1).heard, "deaf");
    assert!(perception(&setup(60.0, 40.0, 2.0), 1).heard, "sharp ears carry the noise twice as far");
    // a gun that fires is heard without a Noisy
    let mut s = Sim::new(1);
    ai(&mut s, 1, 0.0, 0.0, 2, "");
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,1.2,80]},"Team":{"id":1},"Weapon":{"noise":120,"fire":true,"owner":2,"spread":0,"recoil":[0,0]}}"#);
    s.step();
    assert!(perception(&s, 1).heard, "a round fired 80 m behind it carries 120 m");
}

#[test]
fn an_alert_is_shouted_to_the_squad() {
    let mut s = Sim::new(1);
    ai(&mut s, 1, 0.0, 0.0, 2, r#""shout":30"#);
    // a squad mate 20 m to the side, facing away from the target, and one 60 m away
    ai(&mut s, 3, 20.0, 0.0, 2, r#""sight_range":5"#);
    ai(&mut s, 4, 60.0, 0.0, 2, r#""sight_range":5"#);
    target(&mut s, 2, 0.0, -10.0, 1);
    s.step_n(60);
    assert_eq!(perception(&s, 1).state, 2);
    let near = perception(&s, 3);
    assert_eq!(near.state, 3, "the mate searches: {near:?}");
    assert!((near.seen_pos[2] + 10.0).abs() < 0.5 && near.target.0 == 2);
    assert_eq!(perception(&s, 4).state, 0, "out of earshot");
}

#[test]
fn the_autopilot_chases_turns_to_face_and_shoots() {
    let mut s = Sim::new(1);
    spawn(&mut s, 9, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[300,1,300]}}"#);
    // the machine has a gun of its own at chest height, a NavAgent and an autopilot
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Team":{"id":2},"NavAgent":{"speed":4},"Perception":{"engage":true,"sight_range":60,"attack_range":25,"notice_time":0.5},"Weapon":{"muzzle":[0,1.4,0],"owner":1,"damage":20,"rate":300,"spread":0,"recoil":[0,0],"bloom_per_shot":0}}"#);
    // a target 40 m ahead, out of attack range
    target(&mut s, 2, 0.0, -40.0, 1);
    s.step_n(30);
    assert_eq!(perception(&s, 1).state, 2);
    assert_eq!(s.world.get::<Weapon>(s.world.entity(1).unwrap()).unwrap().fire, false, "too far to shoot");
    s.step_n(120);
    let z = pos_of(&s, 1)[2];
    assert!(z < -5.0, "it has walked towards the target: z {z}");
    let hp = s.world.get::<HitPoints>(s.world.entity(2).unwrap()).unwrap().value;
    assert_eq!(hp, 1000.0, "and has not shot yet from 30 m");
    s.step_n(600);
    let hp = s.world.get::<HitPoints>(s.world.entity(2).unwrap()).unwrap().value;
    assert!(hp < 900.0, "it closed to attack range and shot: {hp}");
    assert!(perception(&s, 1).distance <= 25.0 + 0.5);
    // it turns to face a target to the side that it can see
    let mut s = Sim::new(1);
    ai(&mut s, 1, 0.0, 0.0, 2, r#""engage":true,"notice_time":0.3"#);
    target(&mut s, 2, 10.0, -10.0, 1);
    s.step_n(30);
    let q = s.world.get::<Transform>(s.world.entity(1).unwrap()).unwrap().rot;
    // forward is local -z turned by the quaternion: about 45 degrees to the right of -z, towards +x
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let fwd = [-(2.0 * (x * z + w * y)), -(2.0 * (y * z - w * x)), -(1.0 - 2.0 * (x * x + y * y))];
    assert!(fwd[0] > 0.6 && fwd[2] < -0.6, "it faces the target: {fwd:?}");
}

/// A melee machine (a crawler: 2.2 m to bite) and a player who stands still: the agent stops within `arrive` of its goal, so the goal ring is nearer by that much, and the machine ends inside its own reach (it ended at 2.75 m before, never biting). (An `arrive` above about 0.6 of the reach cannot be honoured: the ring cannot be nearer than 0.45 of it.)
#[test]
fn a_melee_machine_stops_inside_its_reach_whatever_the_agents_arrive_distance() {
    for arrive in [0.3, 0.6, 1.0] {
        let mut s = Sim::new(1);
        spawn(&mut s, 9, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[300,1,300]}}"#);
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Team":{{"id":2}},"NavAgent":{{"speed":4,"arrive":{arrive}}},"Perception":{{"engage":true,"sight_range":60,"attack_range":2.2,"notice_time":0.3}}}}"#));
        target(&mut s, 2, 0.0, -30.0, 1);
        s.step_n(900);
        let d = perception(&s, 1).distance;
        assert!(d > 0.0 && d <= 2.2, "arrive {arrive}: it stopped {d} m from a player who stands still (reach 2.2)");
    }
}

#[test]
fn a_searchlight_lights_exposes_and_detects_and_the_exposure_drains() {
    let mut s = Sim::new(1);
    spawn(&mut s, 9, r#"{"EventWatch":{"names":"lit, unlit, detected"}}"#);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.6,0]},"Team":{"id":2},"Searchlight":{"range":60,"angle":8,"hold":1.0}}"#);
    target(&mut s, 2, 0.0, -20.0, 1);
    // a friend in the beam is not lit
    target(&mut s, 3, 0.0, -30.0, 2);
    s.step_n(30);
    let l = *s.world.get::<Searchlight>(s.world.entity(1).unwrap()).unwrap();
    let ex = *s.world.get::<Exposure>(s.world.entity(2).unwrap()).unwrap();
    assert_eq!((l.lit, l.target.0), (1, 2), "only the hostile one");
    assert!(ex.lit && !ex.detected && ex.value > 0.4 && ex.value < 0.6, "half exposed after half the hold: {ex:?}");
    assert!(s.world.get::<Exposure>(s.world.entity(3).unwrap()).is_none(), "a friend gets no exposure");
    s.step_n(40);
    let l = *s.world.get::<Searchlight>(s.world.entity(1).unwrap()).unwrap();
    let ex = *s.world.get::<Exposure>(s.world.entity(2).unwrap()).unwrap();
    assert!(ex.detected && ex.value == 1.0 && l.detected && l.exposure == 1.0, "caught: {l:?} {ex:?}");
    // out of the light it drains (0.5 a second): gone in two seconds
    set_pos(&mut s, 2, [15.0, 0.9, -20.0]);
    s.step_n(30);
    let ex = *s.world.get::<Exposure>(s.world.entity(2).unwrap()).unwrap();
    assert!(!ex.lit && ex.value < 1.0 && ex.value > 0.3, "{ex:?}");
    s.step_n(120);
    let ex = *s.world.get::<Exposure>(s.world.entity(2).unwrap()).unwrap();
    assert_eq!((ex.value, ex.detected), (0.0, false));
    // the events (read one tick later through the EventWatch) were announced
    let mut s = Sim::new(1);
    spawn(&mut s, 9, r#"{"EventWatch":{"names":"lit, unlit, detected"}}"#);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.6,0]},"Searchlight":{"range":60,"angle":8,"hold":0.5}}"#);
    target(&mut s, 2, 0.0, -20.0, 1);
    let mut seen = [0.0f64; 3];
    for i in 0..120 {
        if i == 60 {
            set_pos(&mut s, 2, [15.0, 0.9, -20.0]);
        }
        s.step();
        for (k, n) in ["event.lit", "event.detected", "event.unlit"].iter().enumerate() {
            seen[k] = seen[k].max(res(&s, n));
        }
    }
    assert!(seen[0] >= 1.0 && seen[1] >= 1.0 && seen[2] >= 1.0, "lit, detected and unlit were all announced: {seen:?}");
}

#[test]
fn a_walls_shadow_and_a_sweeping_beam() {
    // a wall in the beam leaves what is behind it in the dark
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.6,0]},"Searchlight":{"range":60,"angle":8}}"#);
    target(&mut s, 2, 0.0, -20.0, 1);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,2,-10]},"Collider":{"shape":1,"size":[8,4,0.3]}}"#);
    s.step_n(30);
    assert_eq!(s.world.get::<Searchlight>(s.world.entity(1).unwrap()).unwrap().lit, 0, "the wall casts a shadow");
    // a lighthouse: 90 degrees a second turns the beam off the target and round to it again
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.6,0]},"Searchlight":{"range":60,"angle":8,"sweep":90,"hold":10}}"#);
    target(&mut s, 2, 0.0, -20.0, 1);
    let mut lit: Vec<bool> = Vec::new();
    for _ in 0..330 {
        s.step();
        lit.push(s.world.get::<Searchlight>(s.world.entity(1).unwrap()).unwrap().lit > 0);
    }
    let changes = lit.windows(2).filter(|w| w[0] != w[1]).count();
    assert!(lit[0] && !lit[60], "it starts on the target and has turned away half a second later");
    assert!(changes >= 3, "it turned off, and came round again: {changes} changes");
}

#[test]
fn the_examples_of_the_docs_work_a_behaviour_tree_on_the_state_and_footsteps_from_the_speed() {
    // a behaviour tree that attacks when alert and patrols otherwise
    let mut s = Sim::new(1);
    spawn(&mut s, 9, r#"{"EventWatch":{"names":"attack, patrol, investigate"}}"#);
    // (the text of the tree sits inside a JSON string: its line ends are the two characters backslash and n)
    let tree = ["selector", "  sequence", "    cond Perception.state == 2", "    act event attack", "  sequence", "    cond Perception.state == 3", "    act event investigate", "  act event patrol"].join("\\n");
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Team":{{"id":2}},"Perception":{{"notice_time":0.3}},"BehaviorTree":{{"script":"{tree}"}}}}"#));
    s.step_n(5);
    assert!(res(&s, "event.patrol") >= 1.0, "nothing in sight: it patrols");
    target(&mut s, 2, 0.0, -10.0, 1);
    let mut attacked = false;
    for _ in 0..60 {
        s.step();
        attacked |= res(&s, "event.attack") >= 1.0;
    }
    assert!(attacked, "alert: it attacks");
    // footsteps: a graph system turns the speed into a noise radius
    let mut s = Sim::new(1);
    s.apply(&Command::DefineSystem {
        def: parse(r#"{"name":"footsteps","each":["Noisy","Velocity"],"do":[{"set":"Noisy.radius","to":{"select":[{"gt":[{"hypot":[{"get":"Velocity.lin[0]"},{"get":"Velocity.lin[2]"}]},5]},14,4]}}]}"#).unwrap(),
    })
    .unwrap();
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Noisy":{},"Velocity":{"lin":[1,0,0]}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[50,0,0]},"Noisy":{},"Velocity":{"lin":[7,0,0]}}"#);
    s.step();
    let r = |s: &Sim, uid: u64| s.world.get::<crate::components::Noisy>(s.world.entity(uid).unwrap()).unwrap().radius;
    assert_eq!((r(&s, 1), r(&s, 2)), (4.0, 14.0), "a walk is quiet, a sprint is loud");
}

#[test]
fn an_ai_that_looks_every_second_step_notices_in_the_same_time() {
    let mut times = Vec::new();
    for every in [None, Some(2.0)] {
        let mut s = Sim::new(1);
        ai(&mut s, 1, 0.0, 0.0, 2, r#""sight_range":30,"notice_time":1.0"#);
        target(&mut s, 2, 0.0, -10.0, 1);
        if let Some(e) = every {
            s.world.set_resource("gameplay.every.perception", Some(e)).unwrap();
        }
        let mut alert_at = None;
        for step in 1..=120u32 {
            s.step_n(1);
            if alert_at.is_none() && perception(&s, 1).state == 2 {
                alert_at = Some(step);
            }
        }
        times.push(alert_at.expect("it noticed"));
    }
    assert!((times[0] as i32 - times[1] as i32).abs() <= 3, "alert after {} steps looking every step and {} looking every second: the same second, give or take a step of its own", times[0], times[1]);
}
