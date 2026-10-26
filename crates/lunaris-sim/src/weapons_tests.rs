//! Weapons: hitscan and projectile rounds, walls and cover, teams, head and legs, falloff, rate of fire, ammo, reloading, recoil and bloom. No art: boxes, spheres and capsules.
use super::*;
use crate::components::{Bullet, HitPoints, Transform, Weapon};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn weapon(s: &Sim, uid: u64) -> Weapon {
    *s.world.get::<Weapon>(s.world.entity(uid).unwrap()).unwrap()
}

fn hp(s: &Sim, uid: u64) -> HitPoints {
    *s.world.get::<HitPoints>(s.world.entity(uid).unwrap()).unwrap()
}

fn pull(s: &mut Sim, uid: u64, on: bool) {
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<Weapon>(e).unwrap().fire = on;
}

fn bullets(s: &mut Sim) -> usize {
    let mut n = 0;
    let _ = s.world.for_each::<(&Bullet,), _>(|_, _| n += 1);
    n
}

/// A gun at (0, y, 0) pointing down -z with no spread and no recoil unless `extra` (more fields of the Weapon, as JSON) says otherwise.
fn gun(s: &mut Sim, uid: u64, y: f32, extra: &str) {
    let mut parts: Vec<String> = Vec::new();
    for (k, v) in [("spread", "0"), ("bloom_per_shot", "0"), ("recoil", "[0,0]")] {
        if !extra.contains(&format!("\"{k}\"")) {
            parts.push(format!("\"{k}\":{v}"));
        }
    }
    if !extra.is_empty() {
        parts.push(extra.to_string());
    }
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[0,{y},0]}},"Weapon":{{{}}}}}"#, parts.join(",")));
}

/// A capsule target (1.8 m tall, 0.35 m radius) whose axis is at x 0 and whose centre is at height 0.9, `d` metres in front of the gun.
fn dummy(s: &mut Sim, uid: u64, d: f32, extra: &str) {
    let sep = if extra.is_empty() { "" } else { "," };
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[0,0.9,{}]}},"Collider":{{"shape":2,"size":[0.7,1.8,0.7]}},"HitPoints":{{"value":100,"max":100}}{sep}{extra}}}"#, -d));
}

#[test]
fn a_hitscan_round_hurts_the_first_target_and_reports_where_it_hit() {
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, "");
    dummy(&mut s, 2, 10.0, "");
    pull(&mut s, 1, true);
    s.step();
    let w = weapon(&s, 1);
    assert!(w.fired && w.shots == 1 && w.mag == 29, "{w:?}");
    assert_eq!(w.hit.0, 2);
    assert!((w.hit_dist - 9.65).abs() < 0.06, "the surface of the capsule is 9.65 m away: {}", w.hit_dist);
    assert!(w.hit_normal[2] > 0.9, "the normal faces the shooter: {:?}", w.hit_normal);
    assert_eq!(w.hit_zone, 1);
    assert!((hp(&s, 2).value - 75.0).abs() < 1e-3, "25 damage: {}", hp(&s, 2).value);
    assert!((w.hit_damage - 25.0).abs() < 1e-3);
    // the next tick the flags of the shot are gone (the trigger is held, the next round is the cooldown away)
    pull(&mut s, 1, false);
    s.step();
    assert!(!weapon(&s, 1).fired && weapon(&s, 1).hit.0 == 0);
}

#[test]
fn a_wall_stops_the_round_and_cover_protects_what_is_behind_it() {
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, "");
    dummy(&mut s, 2, 10.0, "");
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,2,-5]},"Collider":{"shape":1,"size":[4,4,0.2]}}"#);
    pull(&mut s, 1, true);
    s.step();
    let w = weapon(&s, 1);
    assert_eq!(w.hit.0, 0, "the wall took it");
    assert!((w.hit_dist - 4.9).abs() < 0.05, "{}", w.hit_dist);
    assert!(w.hit_normal[2] > 0.9);
    assert_eq!(hp(&s, 2).value, 100.0, "nothing behind the wall was hurt");
    assert_eq!(w.hit_damage, 0.0);
}

/// A character of 1.8 m standing on a floor, shot from `y` metres up.
fn shoot_character_at(y: f32) -> (u32, f32) {
    let mut s = Sim::new(1);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[200,1,200]}}"#);
    gun(&mut s, 1, y, "");
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,-10]},"CharacterBody":{},"HitPoints":{"value":100,"max":100}}"#);
    s.step_n(40);
    pull(&mut s, 1, true);
    s.step();
    (weapon(&s, 1).hit_zone, hp(&s, 2).value)
}

#[test]
fn a_character_has_a_head_a_body_and_legs() {
    let (zone, v) = shoot_character_at(1.7);
    assert_eq!(zone, 2, "the head");
    assert!((v - 50.0).abs() < 1e-3, "twice the damage: {v}");
    let (zone, v) = shoot_character_at(1.0);
    assert_eq!(zone, 1, "the body");
    assert!((v - 75.0).abs() < 1e-3, "{v}");
    let (zone, v) = shoot_character_at(0.3);
    assert_eq!(zone, 3, "the legs");
    assert!((v - 82.5).abs() < 1e-3, "0.7 times the damage: {v}");
}

#[test]
fn damage_falls_off_with_distance_and_there_is_none_beyond_the_range() {
    let mut s = Sim::new(1);
    gun(&mut s, 1, 0.9, r#""damage":20,"range":200,"falloff_start":50,"falloff_min":0.4"#);
    // a big target (a sphere of radius 2) whose surface is 123 m away
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.9,-125]},"Collider":{"shape":0,"size":[4,4,4]},"HitPoints":{"value":100,"max":100}}"#);
    pull(&mut s, 1, true);
    s.step();
    let want = 20.0 * (1.0 - 0.6 * (123.0 - 50.0) / 150.0);
    assert!((100.0 - hp(&s, 2).value - want).abs() < 0.2, "{} against {want}", 100.0 - hp(&s, 2).value);
    // beyond the range nothing happens
    let mut s = Sim::new(1);
    gun(&mut s, 1, 0.9, r#""damage":20,"range":200"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.9,-250]},"Collider":{"shape":0,"size":[4,4,4]},"HitPoints":{"value":100,"max":100}}"#);
    pull(&mut s, 1, true);
    s.step();
    assert_eq!((hp(&s, 2).value, weapon(&s, 1).hit.0), (100.0, 0));
}

#[test]
fn a_team_is_never_hurt_by_its_own_side_and_the_owner_is_never_hit() {
    // the holder (uid 3, team 1) stands at the muzzle, a teammate (team 1) and an enemy (team 2) are in the line one behind the other
    let setup = |enemy_team: u32| {
        let mut s = Sim::new(1);
        spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0.9,0]},"Collider":{"shape":2,"size":[0.7,1.8,0.7]},"HitPoints":{"value":100,"max":100},"Team":{"id":1}}"#);
        gun(&mut s, 1, 0.9, r#""owner":3"#);
        dummy(&mut s, 2, 8.0, r#""Team":{"id":1}"#);
        dummy(&mut s, 4, 14.0, &format!(r#""Team":{{"id":{enemy_team}}}"#));
        pull(&mut s, 1, true);
        s.step();
        s
    };
    let s = setup(2);
    assert_eq!(hp(&s, 3).value, 100.0, "the holder is not hit by its own round");
    assert_eq!(hp(&s, 2).value, 100.0, "a teammate is not hit and does not stop the round");
    assert!((hp(&s, 4).value - 75.0).abs() < 1e-3, "the enemy behind the teammate is hit: {}", hp(&s, 4).value);
    let s = setup(1);
    assert_eq!((hp(&s, 2).value, hp(&s, 4).value), (100.0, 100.0), "two teammates, nobody hurt");
    // no team (0) hurts and is hurt
    let mut s = Sim::new(1);
    gun(&mut s, 1, 0.9, "");
    dummy(&mut s, 2, 8.0, "");
    pull(&mut s, 1, true);
    s.step();
    assert!(hp(&s, 2).value < 100.0);
}

#[test]
fn the_rate_of_fire_the_ammo_and_the_reload_work() {
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, r#""rate":600,"mag_size":3,"mag":3,"reserve":6,"reload_time":0.5,"fire":true"#);
    // the three rounds of the magazine go at ten a second; the next pull on an empty magazine starts the reload by itself
    let mut ticks = 0;
    while weapon(&s, 1).reload_left <= 0.0 && ticks < 60 {
        s.step();
        ticks += 1;
    }
    let w = weapon(&s, 1);
    assert!(w.reload_left > 0.0, "the empty magazine started a reload");
    assert_eq!((w.shots, w.mag), (3, 0));
    assert!(ticks >= 12 && ticks <= 24, "three rounds at 600 a minute take about 0.2 s: {ticks} ticks");
    pull(&mut s, 1, false);
    s.step_n(60);
    let w = weapon(&s, 1);
    assert_eq!((w.mag, w.reserve, w.reload_left, w.shots), (3, 3, 0.0, 3), "a full magazine again, three rounds fewer behind it");
    // a manual reload of a part-empty magazine takes the reload time and the rounds from the reserve
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, r#""mag_size":10,"mag":4,"reserve":20,"reload_time":0.5"#);
    let e = s.world.entity(1).unwrap();
    s.world.get_mut::<Weapon>(e).unwrap().reload = true;
    s.step();
    assert!(weapon(&s, 1).reload_left > 0.0 && weapon(&s, 1).mag == 4, "under way");
    s.step_n(40);
    let w = weapon(&s, 1);
    assert_eq!((w.mag, w.reserve, w.reload_left), (10, 14, 0.0));
    // an empty gun with no reserve clicks
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, r#""mag":0,"reserve":0,"fire":true"#);
    s.step();
    assert!(weapon(&s, 1).dry && !weapon(&s, 1).fired);
    // infinite reserve never runs dry
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, r#""mag_size":2,"mag":0,"reserve":0,"infinite":true,"reload_time":0.1,"fire":true"#);
    s.step_n(30);
    assert!(weapon(&s, 1).shots >= 2);
}

#[test]
fn a_semi_automatic_weapon_fires_once_for_each_pull() {
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, r#""auto":false,"rate":6000,"fire":true"#);
    s.step_n(30);
    assert_eq!(weapon(&s, 1).shots, 1, "one round for the whole of a held trigger");
    pull(&mut s, 1, false);
    s.step();
    pull(&mut s, 1, true);
    s.step();
    assert_eq!(weapon(&s, 1).shots, 2, "a new pull, a new round");
}

#[test]
fn a_projectile_takes_time_to_arrive_and_falls_on_the_way() {
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, r#""kind":1,"muzzle_speed":60,"gravity":9.81,"damage":40"#);
    // a sphere of radius 3, 30 m away, at the height of the muzzle
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,1.2,-33]},"Collider":{"shape":0,"size":[6,6,6]},"HitPoints":{"value":100,"max":100}}"#);
    pull(&mut s, 1, true);
    s.step();
    pull(&mut s, 1, false);
    assert_eq!(bullets(&mut s), 1, "a bullet is in the air");
    s.step_n(10);
    assert_eq!(hp(&s, 2).value, 100.0, "after 0.17 s it has gone 10 m and has not arrived");
    assert_eq!(bullets(&mut s), 1);
    s.step_n(40);
    let v = hp(&s, 2).value;
    assert!((v - 60.0).abs() < 1e-3, "it arrived and hurt: {v}");
    assert_eq!(bullets(&mut s), 0, "and is gone");
    let w = weapon(&s, 1);
    assert_eq!(w.hit.0, 0, "(the credit is written on the tick it landed, and the next tick clears it)");
    // a bullet that misses everything is gone at its range
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, r#""kind":1,"muzzle_speed":100,"range":50,"gravity":0"#);
    pull(&mut s, 1, true);
    s.step();
    pull(&mut s, 1, false);
    s.step_n(60);
    assert_eq!(bullets(&mut s), 0);
}

#[test]
fn a_projectile_that_lands_tells_the_gun_what_it_hit_and_credits_a_kill() {
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, r#""kind":1,"muzzle_speed":200,"gravity":0,"damage":100"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,1.2,-23]},"Collider":{"shape":0,"size":[6,6,6]},"HitPoints":{"value":50,"max":50}}"#);
    pull(&mut s, 1, true);
    s.step();
    pull(&mut s, 1, false);
    let mut seen = None;
    for _ in 0..30 {
        s.step();
        let w = weapon(&s, 1);
        if w.hit.0 != 0 {
            seen = Some(w);
            break;
        }
    }
    let w = seen.expect("the bullet landed");
    assert_eq!(w.hit.0, 2);
    assert!(w.hit_damage > 0.0);
    assert_eq!(w.kills, 1);
    assert!(hp(&s, 2).dead, "the target died on that tick");
}

#[test]
fn a_lethal_hitscan_round_counts_a_kill_and_kills_in_the_same_tick() {
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, "");
    dummy(&mut s, 2, 10.0, "");
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 20.0;
    pull(&mut s, 1, true);
    s.step();
    assert!(hp(&s, 2).dead && hp(&s, 2).value == 0.0);
    assert_eq!(weapon(&s, 1).kills, 1);
    // a dead target takes no more rounds and gives no more kills
    s.step_n(30);
    assert_eq!(weapon(&s, 1).kills, 1);
}

#[test]
fn a_shotgun_fires_its_pellets_inside_a_cone_and_the_same_every_time() {
    let run = |target_radius: f32| {
        let mut s = Sim::new(7);
        gun(&mut s, 1, 0.9, r#""pellets":8,"spread":4,"damage":10"#);
        let size = target_radius * 2.0;
        spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,0.9,-10]}},"Collider":{{"shape":0,"size":[{size},{size},{size}]}},"HitPoints":{{"value":1000,"max":1000}}}}"#));
        pull(&mut s, 1, true);
        s.step();
        1000.0 - hp(&s, 2).value
    };
    assert!((run(3.0) - 80.0).abs() < 0.2, "all eight pellets hit a big target 10 m away: {}", run(3.0));
    let narrow = run(0.1);
    assert!(narrow < 80.0, "a narrow target takes fewer: {narrow}");
    assert_eq!(run(0.4), run(0.4), "the pattern does not change from run to run");
}

#[test]
fn recoil_piles_up_and_settles_and_bloom_widens_the_cone_up_to_a_limit() {
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, r#""recoil":[1.0,0.0],"recoil_recover":4,"bloom_per_shot":0.5,"bloom_max":2,"bloom_recover":4,"rate":600,"fire":true"#);
    s.step_n(20);
    let w = weapon(&s, 1);
    assert!(w.kick[0] > 1.0, "the shots have kicked the aim up: {}", w.kick[0]);
    assert!(w.bloom > 0.0 && w.bloom <= 2.0 + 1e-6, "{}", w.bloom);
    pull(&mut s, 1, false);
    s.step_n(120);
    let w = weapon(&s, 1);
    assert_eq!((w.kick, w.bloom), ([0.0, 0.0], 0.0), "everything settles");
    // a weapon that is switched off does nothing
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, r#""active":false,"fire":true"#);
    s.step_n(10);
    assert_eq!(weapon(&s, 1).shots, 0);
}

fn res(s: &Sim, name: &str) -> f64 {
    s.world.resource(name).unwrap_or(f64::NAN)
}

#[test]
fn a_graph_system_pulls_the_trigger_from_the_mouse_and_reloads_from_a_key() {
    // the example of docs/COMBAT.md
    let mut s = Sim::new(1);
    s.apply(&Command::DefineSystem {
        def: parse(r#"{"name":"trigger","each":["Weapon"],"do":[{"set":"Weapon.fire","to":{"gt":[{"res":"input.mouse_left"},0.5]}},{"set":"Weapon.reload","to":{"gt":[{"res":"input.r"},0.5]}}]}"#).unwrap(),
    })
    .unwrap();
    gun(&mut s, 1, 1.2, r#""mag_size":10,"mag":4,"reserve":20,"reload_time":0.5"#);
    s.world.set_resource("input.mouse_left", Some(1.0)).unwrap();
    s.step();
    s.step();
    assert!(weapon(&s, 1).shots >= 1, "the button fires the gun");
    s.world.set_resource("input.mouse_left", Some(0.0)).unwrap();
    s.step_n(30);
    let shots = weapon(&s, 1).shots;
    s.step_n(30);
    assert_eq!(weapon(&s, 1).shots, shots, "released: no more rounds");
    s.world.set_resource("input.r", Some(1.0)).unwrap();
    s.step();
    s.world.set_resource("input.r", Some(0.0)).unwrap();
    assert!(weapon(&s, 1).reload_left > 0.0, "the key reloads");
}

#[test]
fn the_events_of_a_shot_reach_the_game_through_an_event_watch() {
    let mut s = Sim::new(1);
    spawn(&mut s, 9, r#"{"EventWatch":{"names":"shot, hit, kill, impact"}}"#);
    gun(&mut s, 1, 1.2, "");
    dummy(&mut s, 2, 10.0, "");
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 10.0;
    pull(&mut s, 1, true);
    s.step();
    pull(&mut s, 1, false);
    let mut seen: (f64, f64, f64, f64) = (0.0, 0.0, 0.0, 0.0);
    for _ in 0..3 {
        s.step();
        seen = (seen.0.max(res(&s, "event.shot")), seen.1.max(res(&s, "event.hit")), seen.2.max(res(&s, "event.kill")), seen.3.max(res(&s, "event.hit.uid")));
    }
    assert!(seen.0 >= 1.0 && seen.1 >= 1.0 && seen.2 >= 1.0, "shot, hit and kill were announced: {seen:?}");
    assert_eq!(seen.3, 2.0, "the hit names the victim");
}

// ------------------------------------------------------------------------------------------------ lag compensation (`Rewind`)

#[test]
fn a_shooter_whose_view_is_rewound_hits_where_the_target_was_not_where_it_is() {
    // the target is now 5 m to the side of the line of fire, and was on it when the shooter pulled the trigger
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, "");
    dummy(&mut s, 2, 10.0, "");
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Transform>(e).unwrap().pos = [5.0, 0.9, -10.0];
    pull(&mut s, 1, true);
    s.step();
    assert_eq!(weapon(&s, 1).hit.0, 0, "the round goes past the target where it is");
    // ... told so, the round meets it where the shooter saw it
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, "");
    dummy(&mut s, 2, 10.0, "");
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Transform>(e).unwrap().pos = [5.0, 0.9, -10.0];
    s.scripts.rewind.shooters.insert(1, vec![(2, [0.0, 0.9, -10.0])]);
    pull(&mut s, 1, true);
    s.step();
    let w = weapon(&s, 1);
    assert_eq!(w.hit.0, 2, "{w:?}");
    assert!((w.hit_dist - 9.65).abs() < 0.06, "the hit is where the shooter saw the target: {}", w.hit_dist);
    assert!(hp(&s, 2).value < 100.0, "and the damage is the target's, wherever it is now");
}

#[test]
fn a_rewind_is_the_shooters_own_and_it_can_take_a_target_out_of_the_line_of_fire_too() {
    // two guns at the same place, a target that stands on the line now and stood off it before
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, "");
    gun(&mut s, 3, 1.2, "");
    dummy(&mut s, 2, 10.0, "");
    s.scripts.rewind.shooters.insert(1, vec![(2, [5.0, 0.9, -10.0]), (99, [0.0, 0.0, 0.0])]);
    pull(&mut s, 1, true);
    pull(&mut s, 3, true);
    s.step();
    assert_eq!(weapon(&s, 1).hit.0, 0, "the gun that saw it elsewhere misses");
    assert_eq!(weapon(&s, 3).hit.0, 2, "the other gun has no rewind and hits it where it is");
    assert_eq!(hp(&s, 2).value, 100.0 - weapon(&s, 3).damage, "one round, from the gun that hit");
}

#[test]
fn a_rewind_moves_a_character_with_its_head_and_legs_and_a_box_with_its_faces() {
    // the zone of a hit is measured from the feet of the character at the place it is tested at
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.2, "");
    spawn(&mut s, 2, r#"{"Transform":{"pos":[3,5,-10]},"CharacterBody":{"radius":0.35,"height":1.8,"gravity_scale":0},"HitPoints":{"value":100,"max":100}}"#);
    s.scripts.rewind.shooters.insert(1, vec![(2, [0.0, 0.0, -10.0])]);
    pull(&mut s, 1, true);
    s.step();
    let w = weapon(&s, 1);
    assert_eq!((w.hit.0, w.hit_zone), (2, 1), "a round at 1.2 m hits the body of a character standing at the rewound place: {w:?}");
    // a box
    let mut s = Sim::new(1);
    gun(&mut s, 1, 1.0, "");
    spawn(&mut s, 2, r#"{"Transform":{"pos":[4,1.0,-8]},"Collider":{"shape":1,"size":[2,2,2]},"HitPoints":{"value":100,"max":100}}"#);
    s.scripts.rewind.shooters.insert(1, vec![(2, [0.0, 1.0, -8.0])]);
    pull(&mut s, 1, true);
    s.step();
    let w = weapon(&s, 1);
    assert_eq!(w.hit.0, 2, "{w:?}");
    assert!((w.hit_dist - 7.0).abs() < 0.05, "the near face of a 2 m box centred 8 m away: {}", w.hit_dist);
}
