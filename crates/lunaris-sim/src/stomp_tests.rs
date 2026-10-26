//! The stomp and the area damage under it: a wind-up that a player can run out of, a hit that falls with distance, more against structures, worn gear in between, and nothing for the machine's own side.
use super::*;
use crate::components::{HitPoints, Perception};
use crate::stomp::{area_damage, Stomp};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn hp(s: &Sim, uid: u64) -> f32 {
    s.world.get::<HitPoints>(s.world.entity(uid).unwrap()).unwrap().value
}

fn stomp(s: &Sim, uid: u64) -> Stomp {
    *s.world.get::<Stomp>(s.world.entity(uid).unwrap()).unwrap()
}

fn set_pos(s: &mut Sim, uid: u64, pos: [f32; 3]) {
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<crate::components::Transform>(e).unwrap().pos = pos;
}

/// A character (a capsule of a `CharacterBody`, team 1) at (x, 0, z) with 1000 hit points.
fn player(s: &mut Sim, uid: u64, x: f32, z: f32) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{x},0,{z}]}},"CharacterBody":{{"radius":0.35,"height":1.8,"gravity_scale":0}},"HitPoints":{{"value":1000,"max":1000}},"Team":{{"id":1}}}}"#));
}

/// A machine (team 2) at the origin looking down -z, with a short wind-up, that sees a player in front of it.
fn walker(s: &mut Sim, extra: &str) {
    spawn(
        s,
        1,
        &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Team":{{"id":2}},"HitPoints":{{"value":1500,"max":1500}},"Perception":{{"sight_range":30,"sight_fov":200,"notice_time":0.2,"lose_time":30}},"Stomp":{{"windup":0.5,"recover":0.4,"cooldown":1.0,"damage":100,"edge":0.4,"radius":4,"reach":1{extra}}}}}"#),
    );
}

fn events(s: &Sim, name: &str) -> usize {
    s.events.iter().filter(|e| e.name == name).count()
}

#[test]
fn it_winds_up_when_the_target_is_close_then_hurts_what_is_near_its_foot_and_not_what_is_far() {
    let mut s = Sim::new(1);
    walker(&mut s, "");
    player(&mut s, 2, 0.0, -2.5); // in front, inside the trigger
    player(&mut s, 3, 7.5, 0.0); // out of the reach of the blow (4 m of radius round a point 1 m in front)
    // (the machine takes a moment to become alert, then the wind-up starts)
    let mut winding = false;
    for _ in 0..90 {
        s.step();
        if stomp(&s, 1).phase == 1 {
            winding = true;
            break;
        }
    }
    assert!(winding, "the wind-up started once the machine was alert to the player in front of it");
    assert_eq!(hp(&s, 2), 1000.0, "nothing is hurt during the wind-up");
    assert_eq!(events(&s, "stomp"), 0);
    // (the wind-up is 0.5 s: at 60 ticks a second it ends within 40 ticks)
    s.step_n(40);
    let st = stomp(&s, 1);
    assert_eq!(st.strikes, 1, "{st:?}");
    assert_eq!(events(&s, "stomp"), 1);
    assert_eq!(st.hits, 1, "one thing was hurt: {st:?}");
    // the player is 1.5 m from the point of the blow and 1.15 m from its edge (the capsule's radius 0.35): 100 falls to 0.4 times at 4 m
    let lost = 1000.0 - hp(&s, 2);
    let want = 100.0 * (1.0 - 0.6 * (1.15 / 4.0));
    assert!((lost - want).abs() < 6.0, "the stomp took {lost}, about {want}");
    assert_eq!(hp(&s, 3), 1000.0, "far out of reach");
    assert_eq!(hp(&s, 1), 1500.0, "never the machine itself");
    assert!(events(&s, "hit") >= 1, "a hit event for the hit marker");
}

#[test]
fn a_player_who_runs_out_of_the_radius_during_the_wind_up_is_not_hurt() {
    let mut s = Sim::new(1);
    walker(&mut s, "");
    player(&mut s, 2, 0.0, -2.5);
    for _ in 0..90 {
        s.step();
        if stomp(&s, 1).phase == 1 {
            break;
        }
    }
    assert_eq!(stomp(&s, 1).phase, 1);
    // (they run: 9 m away before the foot comes down)
    set_pos(&mut s, 2, [0.0, 0.0, -9.0]);
    s.step_n(45);
    let st = stomp(&s, 1);
    assert_eq!((st.strikes, st.hits), (1, 0), "the foot came down on nothing: {st:?}");
    assert_eq!(hp(&s, 2), 1000.0);
}

#[test]
fn after_a_stomp_it_stands_and_waits_before_the_next_one_and_a_dead_machine_stomps_no_more() {
    let mut s = Sim::new(1);
    walker(&mut s, "");
    player(&mut s, 2, 0.0, -2.5);
    s.step_n(200);
    let first = stomp(&s, 1).strikes;
    assert!(first >= 1, "{first}");
    // (windup 0.5 + recover 0.4 + cooldown 1.0 = 1.9 s between blows: 4 seconds hold two at most)
    assert!(first <= 3, "{first}");
    // it dies: no more blows
    let e = s.world.entity(1).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
    s.step_n(5);
    let n = stomp(&s, 1).strikes;
    s.step_n(300);
    assert_eq!(stomp(&s, 1).strikes, n, "a wreck does not stomp");
    assert_eq!(stomp(&s, 1).phase, 0);
}

#[test]
fn the_blow_falls_with_distance_and_structures_take_more_and_the_machines_side_none() {
    let mut s = Sim::new(1);
    // a centre at the origin: a character 1.35 m from it (1 m from its edge to the middle of the blow), another 3.35 m away, a wall piece 2 m away, a machine of the blow's own team beside it, a thing too high to be reached
    player(&mut s, 2, 1.35, 0.0);
    player(&mut s, 3, 3.35, 0.0);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,1.5,-2.5],"scale":[2,3,1]},"Collider":{"shape":1,"size":[1,1,1]},"HitPoints":{"value":900,"max":900}}"#);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,0,1.0]},"CharacterBody":{"gravity_scale":0},"HitPoints":{"value":1000,"max":1000},"Team":{"id":2}}"#);
    spawn(&mut s, 6, r#"{"Transform":{"pos":[0.5,6.0,0.5]},"CharacterBody":{"gravity_scale":0},"HitPoints":{"value":1000,"max":1000},"Team":{"id":1}}"#);
    let mut ev = Vec::new();
    let o = area_damage(&mut s.world, &mut ev, [0.0, 0.0, 0.0], 4.0, 100.0, 0.4, 3.0, 1, 2, &[], 1.0, 3.0);
    // 1 m from the surface of the capsule: 100 * (1 - 0.6 * 0.25); 3 m: 100 * (1 - 0.6 * 0.75)
    assert!((1000.0 - hp(&s, 2) - 85.0).abs() < 1.5, "{}", 1000.0 - hp(&s, 2));
    assert!((1000.0 - hp(&s, 3) - 55.0).abs() < 1.5, "{}", 1000.0 - hp(&s, 3));
    // the wall is 1 m thick in z (z from -3 to -2): its near face is 2 m from the centre, and it is no character: three times as much (100 * 0.7 * 3 = 210)
    assert!((900.0 - hp(&s, 4) - 210.0).abs() < 2.0, "{}", 900.0 - hp(&s, 4));
    assert_eq!(hp(&s, 5), 1000.0, "the blow's own team is spared");
    assert_eq!(hp(&s, 6), 1000.0, "a thing 6 m up is not at the height of the blow");
    assert_eq!(o.hits, 3, "{o:?}");
    assert!((o.dealt - (85.0 + 55.0 + 210.0)).abs() < 5.0, "{o:?}");
    // (the same blow twice in a row: the second is the same, nothing is remembered)
    let again = area_damage(&mut s.world, &mut ev, [0.0, 0.0, 0.0], 4.0, 100.0, 0.4, 3.0, 1, 2, &[2], 1.0, 3.0);
    assert_eq!(again.hits, 2, "a uid in `skip` is never hurt: {again:?}");
    // nothing from nothing
    for (r, d) in [(0.0, 100.0), (4.0, 0.0), (-1.0, 100.0), (f32::NAN, 100.0), (4.0, f32::NAN)] {
        assert_eq!(area_damage(&mut s.world, &mut ev, [0.0; 3], r, d, 0.4, 3.0, 1, 2, &[], 1.0, 3.0).hits, 0, "radius {r} damage {d}");
    }
}

#[test]
fn a_blow_that_kills_says_so_and_a_thing_in_its_grace_period_takes_nothing() {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,-1]},"CharacterBody":{"gravity_scale":0},"HitPoints":{"value":30,"max":30},"Team":{"id":1}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[1,0,0]},"CharacterBody":{"gravity_scale":0},"HitPoints":{"value":30,"max":30,"invulnerable":5},"Team":{"id":1}}"#);
    let mut ev: Vec<(String, u64, Option<[f32; 3]>)> = Vec::new();
    let o = area_damage(&mut s.world, &mut ev, [0.0; 3], 4.0, 100.0, 0.4, 1.0, 1, 2, &[], 1.0, 3.0);
    assert_eq!(o.kills, 1, "{o:?}");
    assert!(ev.iter().any(|e| e.0 == "kill" && e.1 == 2), "{ev:?}");
    assert!(!ev.iter().any(|e| e.0 == "kill" && e.1 == 3));
    assert_eq!(hp(&s, 3), 30.0, "the one in its grace period took nothing");
}

#[test]
fn worn_gear_takes_its_share_of_a_stomp_and_wears() {
    use crate::inventory::Inventory;
    let mut s = Sim::new(1);
    spawn(&mut s, 50, r#"{"ItemTable":{"text":"item plate | Plate | stack=1 class=2 power=50 dur=1000\n"}}"#);
    let mut id = vec!["0".to_string(); 48];
    let mut count = vec!["0".to_string(); 48];
    let mut dur = vec!["0".to_string(); 48];
    (id[0], count[0], dur[0]) = ("1".into(), "1".into(), "1000".into());
    spawn(
        &mut s,
        2,
        &format!(
            r#"{{"Transform":{{"pos":[0,0,-1]}},"CharacterBody":{{"gravity_scale":0}},"HitPoints":{{"value":1000,"max":1000}},"Team":{{"id":1}},"Inventory":{{"size":4,"id":[{}],"count":[{}],"dur":[{}]}},"Equipment":{{"slots":"slot 0 accepts=2 zones=body"}}}}"#,
            id.join(","),
            count.join(","),
            dur.join(",")
        ),
    );
    s.step();
    let mut ev = Vec::new();
    area_damage(&mut s.world, &mut ev, [0.0, 0.0, -1.0], 4.0, 100.0, 1.0, 1.0, 1, 2, &[], 1.0, 3.0);
    assert!((1000.0 - hp(&s, 2) - 50.0).abs() < 0.5, "the plate stopped half: {}", 1000.0 - hp(&s, 2));
    let inv = s.world.get::<Inventory>(s.world.entity(2).unwrap()).unwrap();
    assert!((inv.dur[0] - 950.0).abs() < 0.5, "and wore by what it stopped: {}", inv.dur[0]);
    // a stomp that lands in the legs (zone 3) meets no plate there
    let before = hp(&s, 2);
    area_damage(&mut s.world, &mut ev, [0.0, 0.0, -1.0], 4.0, 100.0, 1.0, 1.0, 3, 2, &[], 1.0, 3.0);
    assert!((before - hp(&s, 2) - 100.0).abs() < 0.5, "legs: {}", before - hp(&s, 2));
}

#[test]
fn a_machine_that_is_not_alert_does_not_wind_up_and_the_numbers_are_the_data() {
    let mut s = Sim::new(1);
    // a target behind a wall is not seen: no wind-up
    walker(&mut s, "");
    spawn(&mut s, 7, r#"{"Transform":{"pos":[0,1.5,-1.5],"scale":[8,3,0.4]},"Collider":{"shape":1,"size":[1,1,1]}}"#);
    player(&mut s, 2, 0.0, -2.5);
    s.step_n(120);
    let st = stomp(&s, 1);
    assert_eq!((st.phase, st.strikes), (0, 0), "{st:?} {:?}", *s.world.get::<Perception>(s.world.entity(1).unwrap()).unwrap());
    // `check`-style sanity: the defaults are the ones the docs give
    let d = Stomp::default();
    assert_eq!((d.radius, d.trigger, d.windup, d.recover, d.cooldown, d.damage, d.structure), (4.0, 3.2, 1.1, 1.4, 3.0, 90.0, 3.0));
}
