//! The parts of a heavy machine: its legs (patrol, chase, turn at a rate, hold still while it stomps), its gun (the barrel's pitch from the ballistics of the shell, the fire gate, the splash, the shells shown to the players)
//! and a part that is shot as a thing of its own (the rear core that passes what it takes on to the machine, times a factor). No art: capsules, boxes and nothing drawn.
use super::*;
use crate::components::{Bullet, HitPoints, NavAgent, NetEntity, Transform};
use crate::physics_rot::{axes_of, normalize_q};
use crate::walker::{launch_angle, parse_patrol, Cannon, Hitbox, Pursuit};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn hp(s: &Sim, uid: u64) -> f32 {
    s.world.get::<HitPoints>(s.world.entity(uid).unwrap()).unwrap().value
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn set_pos(s: &mut Sim, uid: u64, p: [f32; 3]) {
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<Transform>(e).unwrap().pos = p;
}

/// The way an entity faces on the ground: (x, z) of its forward (local -z).
fn forward(s: &Sim, uid: u64) -> [f32; 2] {
    let q = s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().rot;
    let ax = axes_of(normalize_q(q));
    [-ax[2][0], -ax[2][2]]
}

fn pursuit(s: &Sim, uid: u64) -> Pursuit {
    *s.world.get::<Pursuit>(s.world.entity(uid).unwrap()).unwrap()
}

fn cannon(s: &Sim, uid: u64) -> Cannon {
    *s.world.get::<Cannon>(s.world.entity(uid).unwrap()).unwrap()
}

fn floor(s: &mut Sim) {
    spawn(s, 90, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[300,1,300]},"Collider":{"shape":1}}"#);
}

/// A character (team 1) at (x, 0, z) with 1000 hit points that stays where it is put.
fn player(s: &mut Sim, uid: u64, x: f32, z: f32) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{x},0,{z}]}},"CharacterBody":{{"radius":0.35,"height":1.8,"gravity_scale":0}},"HitPoints":{{"value":1000,"max":1000}},"Team":{{"id":1}}}}"#));
}

/// A walking machine (team 2) at the origin looking down -z: a big capsule, a nav agent that fits, senses, and legs.
fn legs(s: &mut Sim, pursuit_extra: &str, extra: &str) {
    spawn(
        s,
        1,
        &format!(
            r#"{{"Transform":{{"pos":[0,0,0]}},"Team":{{"id":2}},"CharacterBody":{{"radius":1.3,"height":5,"step_height":1}},"NavAgent":{{"speed":3,"radius":1.3,"height":5,"step_height":1,"arrive":2.4}},"HitPoints":{{"value":1500,"max":1500}},"Perception":{{"sight_range":60,"sight_fov":360,"sight_eye":4.4,"notice_time":0.3,"lose_time":3}},"Pursuit":{{"calm_speed":4,"alert_speed":4,"turn_rate":90{pursuit_extra}}}{extra}}}"#
        ),
    );
}

#[test]
fn the_patrol_text_is_read_and_the_launch_angle_lands_the_shell_where_it_is_aimed() {
    let p = parse_patrol("# a loop\n10 0\n10 -10  # corner\nx y\n5\n-3.5 4e1");
    assert_eq!(p, vec![[10.0, 0.0], [10.0, -10.0], [-3.5, 40.0]]);
    for (v, g, x, y) in [(60.0f32, 9.81f32, 40.0f32, -2.7f32), (60.0, 9.81, 80.0, -2.7), (45.0, 9.81, 30.0, 6.0), (100.0, 9.81, 150.0, 0.0), (60.0, 0.0, 50.0, 10.0)] {
        let a = launch_angle(v, g, x, y).unwrap();
        // (the height of the shell at x: x tan a - g x^2 / (2 v^2 cos^2 a))
        let h = x * a.tan() - g * x * x / (2.0 * v * v * a.cos() * a.cos());
        assert!((h - y).abs() < 0.03, "v {v} g {g} x {x} y {y}: lands at {h}");
    }
    assert!(launch_angle(20.0, 9.81, 100.0, 0.0).is_none(), "20 m/s does not carry a shell 100 m");
    // the flatter way: a shell that is carried by a faster muzzle is aimed lower
    assert!(launch_angle(120.0, 9.81, 60.0, 0.0).unwrap() < launch_angle(60.0, 9.81, 60.0, 0.0).unwrap());
}

#[test]
fn it_walks_a_patrol_facing_the_way_it_goes_and_waits_at_each_point() {
    let mut s = Sim::new(1);
    floor(&mut s);
    legs(&mut s, r#","patrol":"12 0\n12 -12\n0 -12","wait":1.0"#, "");
    let mut stood: Vec<(u32, u32)> = Vec::new(); // (point it was at, ticks it stood there)
    let mut faced = false;
    let mut last_at = u32::MAX;
    for _ in 0..1500 {
        s.step();
        let p = pursuit(&s, 1);
        if p.arrived {
            if last_at != p.patrol_at {
                stood.push((p.patrol_at, 0));
                last_at = p.patrol_at;
            }
            stood.last_mut().unwrap().1 += 1;
        }
        // on the way to the first point (12, 0) it faces +x
        if p.mode == 1 && p.patrol_at == 0 && pos(&s, 1)[0] > 3.0 && pos(&s, 1)[0] < 8.0 {
            let f = forward(&s, 1);
            faced |= f[0] > 0.9;
        }
    }
    assert!(faced, "it faced the way it went");
    assert!(stood.len() >= 3, "it stood at the first three points: {stood:?}");
    assert_eq!(stood.iter().take(3).map(|x| x.0).collect::<Vec<_>>(), vec![0, 1, 2], "in the order of the text: {stood:?}");
    // (a wait of a second is 60 ticks)
    for (k, n) in stood.iter().take(3) {
        assert!(*n >= 50 && *n <= 70, "it waited about a second at point {k}: {n} ticks");
    }
}

#[test]
fn it_walks_up_to_a_target_it_sees_and_stops_close_turning_at_a_limited_rate() {
    let mut s = Sim::new(1);
    floor(&mut s);
    legs(&mut s, "", "");
    player(&mut s, 2, 0.0, -30.0);
    s.step_n(90);
    assert!(pursuit(&s, 1).mode == 2, "it chases a target it sees: {:?}", pursuit(&s, 1));
    assert!(pos(&s, 1)[2] < -2.0, "and has set off: {:?}", pos(&s, 1));
    s.step_n(900);
    let at = pos(&s, 1);
    let gap = ((at[0] - 0.0).powi(2) + (at[2] + 30.0).powi(2)).sqrt();
    assert!(gap < 3.4 && gap > 1.5, "it stopped close to the player without walking into it: {gap}");
    let f = forward(&s, 1);
    assert!(f[1] < -0.95, "and faces it: {f:?}");
    let nav = s.world.get::<NavAgent>(s.world.entity(1).unwrap()).unwrap().status;
    assert_ne!(nav, 3, "holding close to a target is not a walk that has no way: the agent was not left to declare it blocked");
    // the body turns at a rate: the player jumps behind it (180 degrees), and it takes about 2 s at 90 degrees a second
    set_pos(&mut s, 2, [at[0], 0.0, at[2] + 20.0]);
    let mut prev = forward(&s, 1);
    let mut worst = 0.0f32;
    let mut turned = 0usize;
    for k in 0..400 {
        s.step();
        let f = forward(&s, 1);
        let step = (prev[0] * f[1] - prev[1] * f[0]).atan2(prev[0] * f[0] + prev[1] * f[1]).abs().to_degrees();
        worst = worst.max(step);
        prev = f;
        if f[1] > 0.95 && turned == 0 {
            turned = k;
        }
    }
    assert!(worst <= 90.0 / 60.0 + 0.2, "it never turned faster than 90 degrees a second: {worst} degrees a tick");
    assert!(turned > 60, "half a circle took {turned} ticks, more than a second");
    assert!(turned > 0, "and it did turn all the way round");
}

#[test]
fn it_stands_still_while_its_stomp_is_at_work() {
    let mut s = Sim::new(1);
    floor(&mut s);
    legs(&mut s, "", r#","Stomp":{"windup":1.5,"recover":1.0,"trigger":6,"damage":10,"cooldown":50}"#);
    player(&mut s, 2, 0.0, -4.5);
    let mut wound = false;
    for _ in 0..120 {
        s.step();
        if s.world.get::<crate::stomp::Stomp>(s.world.entity(1).unwrap()).unwrap().phase == 1 {
            wound = true;
            break;
        }
    }
    assert!(wound);
    let at = pos(&s, 1);
    s.step_n(30);
    let e = s.world.entity(1).unwrap();
    assert_eq!(s.world.get::<NavAgent>(e).unwrap().speed, 0.0, "its legs are told to stop");
    let moved = ((pos(&s, 1)[0] - at[0]).powi(2) + (pos(&s, 1)[2] - at[2]).powi(2)).sqrt();
    assert!(moved < 0.2, "it did not walk during the wind-up: {moved}");
}

#[test]
fn a_dead_machine_stands_still() {
    let mut s = Sim::new(1);
    floor(&mut s);
    legs(&mut s, r#","patrol":"20 0\n20 -20""#, "");
    s.step_n(60);
    let e = s.world.entity(1).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
    s.step_n(5);
    let at = pos(&s, 1);
    s.step_n(120);
    let moved = ((pos(&s, 1)[0] - at[0]).powi(2) + (pos(&s, 1)[2] - at[2]).powi(2)).sqrt();
    assert!(moved < 0.05, "a wreck does not walk: {moved}");
}

/// A machine with a gun on a child: the weapon is a slow shell of a cannon.
fn gunner(s: &mut Sim, cannon_extra: &str) {
    spawn(
        s,
        1,
        &format!(
            r#"{{"Transform":{{"pos":[0,0,0]}},"Team":{{"id":2}},"CharacterBody":{{"radius":1.3,"height":5,"gravity_scale":0}},"HitPoints":{{"value":1500,"max":1500}},"Perception":{{"sight_range":150,"sight_fov":360,"sight_eye":4.4,"notice_time":0.2,"lose_time":30}},"Cannon":{{{}}}}}"#,
            cannon_extra.trim_start_matches(',')
        ),
    );
    spawn(
        s,
        2,
        r#"{"Parent":{"target":1},"LocalTransform":{"pos":[0,3.7,-1.2]},"Transform":{"pos":[0,3.7,-1.2]},"Weapon":{"kind":1,"damage":60,"rate":60,"auto":true,"infinite":true,"muzzle_speed":60,"gravity":9.81,"spread":0,"recoil":[0,0],"noise":0,"range":200,"falloff_start":200,"muzzle":[0,0,-1.0],"head_mult":1,"limb_mult":1}}"#,
    );
}

#[test]
fn the_barrel_is_aimed_for_the_shell_and_a_shell_lands_on_a_target_that_stands() {
    let mut s = Sim::new(1);
    floor(&mut s);
    gunner(&mut s, "");
    player(&mut s, 3, 0.0, -40.0);
    s.step_n(40);
    let near = cannon(&s, 1).pitch;
    // a farther one needs the barrel higher
    let mut t = Sim::new(1);
    floor(&mut t);
    gunner(&mut t, "");
    player(&mut t, 3, 0.0, -90.0);
    t.step_n(40);
    let far = cannon(&t, 1).pitch;
    assert!(far > near + 2.0, "the barrel rises with the distance: {near} at 40 m, {far} at 90 m");
    // the shells: one a second; they hit (60 each: the body, so no head or limb factor)
    s.step_n(400);
    let c = cannon(&s, 1);
    assert!(c.shots >= 5, "it fired: {c:?}");
    let lost = 1000.0 - hp(&s, 3);
    assert!(lost >= 60.0 && (lost / 60.0 - (lost / 60.0).round()).abs() < 0.01, "whole shells: {lost}");
    assert!(lost / 60.0 >= (c.shots as f32 - 2.0).max(1.0), "nearly every shell hit a target that stands still: {lost} of {} shots", c.shots);
    assert!(c.aimed && c.firing, "{c:?}");
}

#[test]
fn a_barrel_that_turns_follows_a_target_off_the_bodys_axis_and_the_shells_land_on_it() {
    let mut s = Sim::new(1);
    floor(&mut s);
    gunner(&mut s, "");
    // 20 degrees to the right of the way the body faces (the default reach of the barrel is 25)
    player(&mut s, 3, 14.5, -40.0);
    s.step_n(300);
    let c = cannon(&s, 1);
    assert!(c.yaw < -15.0 && c.yaw > -25.0, "the barrel has turned to the right: {c:?}");
    assert!(c.shots >= 3, "{c:?}");
    let lost = 1000.0 - hp(&s, 3);
    assert!(lost >= 120.0, "and its shells land on the target that stands there: {lost}");
    // 40 degrees is more than the barrel reaches: it does not fire
    let mut t = Sim::new(1);
    floor(&mut t);
    gunner(&mut t, "");
    player(&mut t, 3, 33.6, -40.0);
    t.step_n(300);
    assert_eq!(cannon(&t, 1).shots, 0, "out of the reach of the barrel: {:?}", cannon(&t, 1));
    // a barrel that cannot depress far enough for a target that close does not fire (the shells would fly over it)
    let mut u = Sim::new(1);
    floor(&mut u);
    gunner(&mut u, r#","min_range":1"#);
    player(&mut u, 3, 0.0, -5.0);
    u.step_n(200);
    assert_eq!(cannon(&u, 1).shots, 0, "the target is under the barrel's reach: {:?}", cannon(&u, 1));
}

#[test]
fn it_does_not_fire_when_the_body_does_not_face_the_target_or_it_is_too_close_or_too_far() {
    // (a target 120 degrees off the way it faces: seen, since it sees all round, but out of the arc: the body is not turned to it)
    let mut s = Sim::new(1);
    floor(&mut s);
    gunner(&mut s, "");
    player(&mut s, 3, 30.0, 17.0);
    s.step_n(200);
    assert_eq!(cannon(&s, 1).shots, 0, "{:?}", cannon(&s, 1));
    // too close
    let mut s = Sim::new(1);
    floor(&mut s);
    gunner(&mut s, r#","min_range":10"#);
    player(&mut s, 3, 0.0, -6.0);
    s.step_n(200);
    assert_eq!(cannon(&s, 1).shots, 0);
    // too far
    let mut s = Sim::new(1);
    floor(&mut s);
    gunner(&mut s, r#","range":30"#);
    player(&mut s, 3, 0.0, -60.0);
    s.step_n(200);
    assert_eq!(cannon(&s, 1).shots, 0);
}

#[test]
fn a_shell_that_lands_also_hurts_what_is_near_the_place_but_not_what_it_hit_twice() {
    let mut s = Sim::new(1);
    floor(&mut s);
    gunner(&mut s, r#","blast":40,"blast_radius":3,"blast_edge":0.5,"structure":2"#);
    player(&mut s, 3, 0.0, -40.0);
    // a wall piece 1.5 m beside the player and another far away
    spawn(&mut s, 4, r#"{"Transform":{"pos":[2.0,1.5,-40.0],"scale":[1,3,3]},"Collider":{"shape":1,"size":[1,1,1]},"HitPoints":{"value":900,"max":900}}"#);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[30.0,1.5,-40.0],"scale":[1,3,3]},"Collider":{"shape":1,"size":[1,1,1]},"HitPoints":{"value":900,"max":900}}"#);
    s.step_n(120);
    let c = cannon(&s, 1);
    assert!(c.shots >= 1 && c.blasts >= 1, "{c:?}");
    // the player took the shell itself (60 each) and no splash on top of it
    let lost = 1000.0 - hp(&s, 3);
    assert!((lost / 60.0 - (lost / 60.0).round()).abs() < 0.01, "only whole shells, no splash for the one that was hit: {lost}");
    assert!(900.0 - hp(&s, 4) > 20.0, "the wall beside the place took the splash: {}", 900.0 - hp(&s, 4));
    assert_eq!(hp(&s, 5), 900.0, "the far one is untouched");
    assert!(s.events.iter().any(|e| e.name == "cannon_blast"));
}

#[test]
fn a_shell_that_bursts_on_a_structure_hurts_it_by_the_shell_and_by_the_splash() {
    // a wall piece is a thing with hit points that is no friend of the machine: it is a target like a player (the machine shells what is nearest), and the shell bursts on it
    let mut s = Sim::new(1);
    floor(&mut s);
    gunner(&mut s, r#","blast":40,"blast_radius":3,"blast_edge":0.5,"structure":2.5"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0.0,1.5,-30.0],"scale":[6,3,0.5]},"Collider":{"shape":1,"size":[1,1,1]},"HitPoints":{"value":900,"max":900}}"#);
    let mut shells = 0;
    for _ in 0..400 {
        s.step();
        shells = cannon(&s, 1).blasts;
        if shells >= 1 {
            break;
        }
    }
    s.step_n(3);
    let lost = 900.0 - hp(&s, 4);
    // 70... here 60 for the shell itself (the test gun's damage) and 40 times 2.5 for the splash, a little less where it burst a hand's breadth from the middle of the splash
    assert!(shells >= 1, "it shelled the wall: {:?}", cannon(&s, 1));
    assert!(lost > 60.0 + 80.0 && lost < 60.0 + 100.0 + 1.0, "the shell and its splash: {lost}");
}

#[test]
fn shells_are_shown_to_the_players_when_the_cannon_says_so() {
    let mut s = Sim::new(1);
    floor(&mut s);
    gunner(&mut s, r#","net_kind":77"#);
    player(&mut s, 3, 0.0, -60.0);
    let mut seen = 0;
    for _ in 0..200 {
        s.step();
        let mut shells: Vec<NetEntity> = Vec::new();
        let _ = s.world.for_each::<(&Bullet, &NetEntity), _>(|_, (_, n)| shells.push(*n));
        for n in &shells {
            assert_eq!((n.class, n.kind), (2, 77), "a shell in the air is a thing in flight of the kind the cannon names");
        }
        seen += shells.len();
    }
    assert!(seen > 0, "a shell was in the air and had its NetEntity");
    // without a kind, none
    let mut t = Sim::new(1);
    floor(&mut t);
    gunner(&mut t, "");
    player(&mut t, 3, 0.0, -60.0);
    t.step_n(200);
    let mut n = 0;
    let _ = t.world.for_each::<(&Bullet, &NetEntity), _>(|_, _| n += 1);
    assert_eq!(n, 0);
}

/// A machine with a core on its back: a sphere that sticks out 0.15 m behind the capsule, a ghost that nothing collides with.
fn cored(s: &mut Sim, hp: f32) {
    spawn(s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Team":{{"id":2}},"CharacterBody":{{"radius":1.3,"height":5,"gravity_scale":0}},"HitPoints":{{"value":{hp},"max":{hp}}}}}"#));
    spawn(
        s,
        2,
        r#"{"Parent":{"target":1},"LocalTransform":{"pos":[0,2.8,1.5]},"Transform":{"pos":[0,2.8,1.5]},"Collider":{"shape":0,"size":[0.9,0.9,0.9]},"HitPoints":{"value":1000000,"max":1000000},"Team":{"id":2},"CollisionLayer":{"layer":30,"mask":0},"Hitbox":{"mult":3}}"#,
    );
    player(s, 4, 60.0, 60.0);
}

/// One round of 50 from a gun of team 1 at (0, 2.8, z) looking along -z (from behind the machine) or +z (from in front of it).
fn one_shot(s: &mut Sim, uid: u64, z: f32, from_behind: bool) {
    let rot = if from_behind { "[0,0,0,1]" } else { "[0,1,0,0]" };
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[0,2.8,{z}],"rot":{rot}}},"Weapon":{{"owner":4,"damage":50,"rate":600,"auto":false,"fire":true,"infinite":true,"spread":0,"recoil":[0,0],"noise":0,"range":100,"falloff_start":100,"head_mult":1,"limb_mult":1}}}}"#));
}

#[test]
fn a_part_that_sticks_out_takes_the_shot_from_its_side_and_passes_it_on_times_its_factor() {
    // from behind: the core is in the way of the body
    let mut s = Sim::new(1);
    cored(&mut s, 1000.0);
    one_shot(&mut s, 5, 12.0, true);
    s.step_n(3);
    assert!((1000.0 - hp(&s, 1) - 150.0).abs() < 0.5, "50 times 3 from behind: {}", 1000.0 - hp(&s, 1));
    assert_eq!(hp(&s, 2), 1_000_000.0, "the part is as good as new: what it took was put back");
    let h = *s.world.get::<Hitbox>(s.world.entity(2).unwrap()).unwrap();
    assert!((h.taken - 50.0).abs() < 0.5 && h.hits == 1, "{h:?}");
    // from the front: the body is in the way of the core, and takes the shot at its own rate
    let mut t = Sim::new(1);
    cored(&mut t, 1000.0);
    one_shot(&mut t, 5, -12.0, false);
    t.step_n(3);
    assert!((1000.0 - hp(&t, 1) - 50.0).abs() < 0.5, "50 from the front: {}", 1000.0 - hp(&t, 1));
    assert_eq!(t.world.get::<Hitbox>(t.world.entity(2).unwrap()).unwrap().taken, 0.0);
    // the part is a ghost: it does not stop a walk, and the machine's own team shoots through it
    assert_eq!(hp(&t, 4), 1000.0);
}

#[test]
fn a_part_that_takes_the_last_of_a_machine_kills_it_and_says_so() {
    let mut s = Sim::new(1);
    cored(&mut s, 100.0);
    one_shot(&mut s, 5, 12.0, true);
    s.step_n(3);
    assert!(s.world.get::<HitPoints>(s.world.entity(1).unwrap()).unwrap().dead, "150 against 100 hit points");
    assert!(s.events.iter().any(|e| e.name == "kill" && e.uid == 1), "the machine is the one killed");
    assert!(s.events.iter().any(|e| e.name == "died" && e.uid == 1));
}

/// The whole machine of the game in one: legs, senses, a stomp, a cannon with its gun on a child, a core on the back, and a character to hunt.
fn whole(s: &mut Sim) {
    floor(s);
    spawn(
        s,
        1,
        r#"{"Transform":{"pos":[0,0,0]},"Team":{"id":2},"CharacterBody":{"radius":1.3,"height":5,"step_height":1},"NavAgent":{"speed":3.4,"radius":1.3,"height":5,"step_height":1,"arrive":2.4},"HitPoints":{"value":1500,"max":1500},"Perception":{"sight_range":70,"sight_fov":360,"sight_eye":4.4,"notice_time":0.4,"lose_time":10},"Pursuit":{"calm_speed":1.8,"alert_speed":3.4,"turn_rate":55},"Stomp":{"windup":1.1,"recover":1.4,"trigger":3.2,"damage":80},"Cannon":{"blast":45,"blast_radius":3}}"#,
    );
    spawn(
        s,
        2,
        r#"{"Parent":{"target":1},"LocalTransform":{"pos":[1.38,3.7,-2.05]},"Transform":{"pos":[1.38,3.7,-2.05]},"Weapon":{"kind":1,"damage":70,"rate":24,"auto":true,"infinite":true,"muzzle_speed":55,"gravity":9.81,"spread":0.25,"recoil":[0,0],"noise":190,"range":150,"falloff_start":150,"muzzle":[0,0,-1.0],"head_mult":1,"limb_mult":1}}"#,
    );
    spawn(
        s,
        3,
        r#"{"Parent":{"target":1},"LocalTransform":{"pos":[0,3.4,1.2]},"Transform":{"pos":[0,3.4,1.2]},"Collider":{"shape":0,"size":[1,1,1]},"HitPoints":{"value":1000000,"max":1000000},"Team":{"id":2},"CollisionLayer":{"layer":30,"mask":0},"Hitbox":{"mult":3}}"#,
    );
    player(s, 4, 0.0, -30.0);
}

#[test]
fn the_same_machine_in_the_same_world_replays_to_the_same_state() {
    // the machine hunts a character that walks round it for 15 seconds (it sees it, walks up to it, shells it, stomps it): twice from the same start must be the same world at every checkpoint, and another start must not be
    let run = |dx: f32| {
        let mut s = Sim::new(1);
        whole(&mut s);
        let mut hashes = Vec::new();
        for t in 0..900u32 {
            let a = t as f32 * 0.03;
            set_pos(&mut s, 4, [dx + 10.0 * a.sin(), 0.0, -30.0 + 6.0 * a.cos()]);
            s.step();
            if t % 150 == 149 {
                hashes.push(s.state_hash());
            }
        }
        (hashes, cannon(&s, 1).shots, hp(&s, 4))
    };
    let (a, shots, hurt) = run(0.0);
    let (b, shots_b, hurt_b) = run(0.0);
    let (c, _, _) = run(1.0);
    assert_eq!(a, b, "the same inputs give the same world, checkpoint for checkpoint");
    assert_eq!((shots, hurt), (shots_b, hurt_b));
    assert_ne!(a, c, "and a character one metre to the side gives another");
    assert!(shots >= 2 && hurt < 1000.0, "the run was not empty: {shots} shells, the character has {hurt} of 1000");
}
