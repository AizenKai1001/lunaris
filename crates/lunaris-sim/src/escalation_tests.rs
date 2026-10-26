//! The Core learns: the alert meter that takes a share of all the heat a player gains, the tiers and their margin, what a tier does to the director's waves, machines destroyed near a player, and the Walker that is sent at the top tier,
//! with its cooldown, its cap, the place it needs and its clean-up. The Walker here is a bare prefab of components: no art.
use super::*;
use crate::components::{Heat, HitPoints, Perception, SpawnedBy, Transform};
use crate::escalation::{parse_noises, parse_tiers, tier_for, Escalation, DEFAULT_TIERS};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

const HUNTER: &str = r#"{"components":{"Name":{"value":"Hunter"},"Transform":{},"Team":{"id":2},"Perception":{"lose_time":1.0,"notice_time":0.5,"sight_range":10}}}"#;
const WALKER: &str = r#"{"components":{"Name":{"value":"Walker"},"Transform":{},"Team":{"id":2},"HitPoints":{"value":1500,"max":1500},"CharacterBody":{"radius":1.3,"height":5,"gravity_scale":0},"Perception":{"lose_time":1.0,"notice_time":0.5,"sight_range":10},"Stomp":{}}}"#;

fn world() -> Sim {
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "Hunter".into(), def: parse(HUNTER).unwrap() }).unwrap();
    s.apply(&Command::DefinePrefab { name: "Walker".into(), def: parse(WALKER).unwrap() }).unwrap();
    s
}

fn floor(s: &mut Sim) {
    spawn(s, 90, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[1000,1,1000]},"Collider":{"shape":1}}"#);
}

/// A player (team 1, a capsule at (x, 0, z)) with a `Heat` that does not cool by itself.
fn player(s: &mut Sim, uid: u64, x: f32, z: f32, heat: &str) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{x},0,{z}]}},"CharacterBody":{{"gravity_scale":0}},"HitPoints":{{"value":100,"max":100}},"Team":{{"id":1}},"Heat":{{"decay":0{heat}}}}}"#));
}

fn heat(s: &Sim, uid: u64) -> Heat {
    *s.world.get::<Heat>(s.world.entity(uid).unwrap()).unwrap()
}

fn add_heat(s: &mut Sim, uid: u64, v: f32) {
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<Heat>(e).unwrap().value += v;
}

fn core(s: &Sim, uid: u64) -> Escalation {
    *s.world.get::<Escalation>(s.world.entity(uid).unwrap()).unwrap()
}

fn set_core(s: &mut Sim, uid: u64, f: impl FnOnce(&mut Escalation)) {
    let e = s.world.entity(uid).unwrap();
    f(s.world.get_mut::<Escalation>(e).unwrap());
}

fn res(s: &Sim, name: &str) -> f64 {
    s.world.resource(name).unwrap_or(0.0)
}

fn walkers(s: &mut Sim) -> Vec<(u64, [f32; 3])> {
    let mut v: Vec<(u64, [f32; 3])> = Vec::new();
    let _ = s.world.for_each_uid::<(&SpawnedBy, &crate::stomp::Stomp, &Transform), _>(|_, uid, (_, _, t)| v.push((uid, t.pos)));
    v.sort_by_key(|w| w.0);
    v
}

fn hunters(s: &mut Sim) -> usize {
    let mut n = 0;
    let _ = s.world.for_each::<(&SpawnedBy, &Perception), _>(|_, _| n += 1);
    n
}

fn flat(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[test]
fn the_text_of_the_tiers_and_of_the_loud_events_is_read_and_its_mistakes_named() {
    let (rows, problems) = parse_tiers(DEFAULT_TIERS);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(rows.len(), 3);
    assert_eq!((rows[0].at, rows[1].at, rows[2].at), (2.0, 5.0, 9.0));
    assert_eq!((rows[1].hunters, rows[1].cooldown, rows[1].alive, rows[1].per_heat), (1.5, 0.7, 2, 1.5));
    assert_eq!((rows[0].hunters, rows[0].cooldown, rows[0].alive, rows[0].per_heat), (1.0, 1.0, 0, 1.0), "what a line leaves out is neutral");
    let (rows, problems) = parse_tiers("# a\ntier 1 at=2\ntier 3 at=5\ntier 2 at=1 hunters=2\ntier 2 at=4 bad\nthing 4\n");
    assert_eq!(rows.iter().map(|r| r.at).collect::<Vec<_>>(), vec![2.0, 4.0], "{rows:?}");
    assert_eq!(problems.len(), 4, "a gap in the numbers, a threshold that does not rise, a word that is not key=value, a line that is not a tier: {problems:#?}");
    let (noises, problems) = parse_noises("explosion 1.5\nbad\n# x\nblast 0.5 # c\nneg -1\nexplosion 2");
    assert_eq!(noises, vec![("blast".to_string(), 0.5), ("explosion".to_string(), 2.0)], "the later line for a name wins");
    assert_eq!(problems.len(), 2, "{problems:?}");
}

#[test]
fn a_tier_holds_until_the_alert_is_a_margin_below_it() {
    let (rows, _) = parse_tiers(DEFAULT_TIERS);
    let t = |alert: f32, cur: u32| tier_for(&rows, alert, cur, 0.5);
    assert_eq!((t(0.0, 0), t(1.9, 0), t(2.0, 0), t(5.2, 1), t(9.5, 0)), (0, 0, 1, 2, 3));
    assert_eq!(t(4.8, 2), 2, "5 was the threshold, 4.8 is within the margin");
    assert_eq!(t(4.4, 2), 1, "below the margin");
    assert_eq!(t(1.6, 1), 1);
    assert_eq!(t(1.4, 1), 0);
    assert_eq!(t(8.0, 3), 2, "falls one tier at a time to the one it has reached");
    assert_eq!(t(0.1, 3), 0, "and all the way down");
    assert_eq!(t(100.0, 0), 3, "never above the last tier");
}

#[test]
fn heat_from_any_source_feeds_the_alert_and_the_tier_rises_then_falls_with_time() {
    let mut s = world();
    player(&mut s, 2, 0.0, 0.0, "");
    spawn(&mut s, 3, r#"{"Director":{"prefab":"Hunter","threshold":50}}"#);
    spawn(&mut s, 4, r#"{"Escalation":{"tiers":"tier 1 at=1\ntier 2 at=3\ntier 3 at=6","heat_gain":0.5,"decay":0,"hyst":0.25}}"#);
    s.step_n(10);
    assert_eq!((heat(&s, 2).alert, heat(&s, 2).tier), (0.0, 0), "nothing happened");
    // (something else than the director raises the heat: a helicopter, a trace, a script: 0.5 heat ten times = 5 heat = 2.5 alert)
    let mut seen = Vec::new();
    for _ in 0..10 {
        add_heat(&mut s, 2, 0.5);
        s.step_n(15);
        seen.push(heat(&s, 2).tier);
    }
    let h = heat(&s, 2);
    assert!((h.alert - 2.5).abs() < 0.05, "half of the 5 heat that was gained: {}", h.alert);
    assert_eq!(h.tier, 1);
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "the tier only rose: {seen:?}");
    // a lot more: tier 3
    add_heat(&mut s, 2, 4.0);
    add_heat(&mut s, 2, 3.0);
    s.step_n(5);
    let h = heat(&s, 2);
    assert!((h.alert - 6.0).abs() < 0.06 && h.tier == 3, "{h:?}");
    assert_eq!((res(&s, "ai.tier"), (res(&s, "ai.alert") * 10.0).round() / 10.0), (3.0, 6.0), "told to the game");
    let c = core(&s, 4);
    assert_eq!((c.tier, c.alert.round()), (3, 6.0));
    assert!(s.events.iter().filter(|e| e.name == "tier" && e.uid == 2).count() >= 2, "the tier changed 0 to 1, then 1 to 3");
    // time passes and nothing happens: it falls slowly (here 1 a second), the tier with it, never flickering
    set_core(&mut s, 4, |c| c.decay = 1.0);
    let mut falling = Vec::new();
    for _ in 0..8 {
        s.step_n(60);
        falling.push(heat(&s, 2).tier);
    }
    assert!(falling.windows(2).all(|w| w[0] >= w[1]), "the tier only fell: {falling:?}");
    let h = heat(&s, 2);
    assert_eq!((h.tier, h.alert), (0, 0.0), "after eight seconds it is calm again: {h:?}");
    assert_eq!(res(&s, "ai.tier"), 0.0);
}

/// The server tells the clients the Core's state (`NetHost.sync`: `ai.tier`, `ai.alert`, `ai.walkers`); a connected client runs the same scene, so its own director must not put its own numbers (all zero: nothing raises the client's heat) over them.
#[test]
fn a_connected_client_keeps_the_state_of_the_core_that_the_server_told_it() {
    for connected in [false, true] {
        let mut s = world();
        player(&mut s, 2, 0.0, 0.0, "");
        spawn(&mut s, 3, r#"{"Director":{"prefab":"Hunter","threshold":50}}"#);
        spawn(&mut s, 4, r#"{"Escalation":{"tiers":"tier 1 at=1"}}"#);
        spawn(&mut s, 5, &format!(r#"{{"NetPlayer":{{"connected":{connected}}}}}"#));
        for (name, v) in [("ai.tier", 3.0), ("ai.alert", 7.5), ("ai.walkers", 2.0)] {
            s.world.set_resource(name, Some(v)).unwrap();
        }
        s.step_n(30);
        let told = (res(&s, "ai.tier"), res(&s, "ai.alert"), res(&s, "ai.walkers"));
        if connected {
            assert_eq!(told, (3.0, 7.5, 2.0), "the client keeps what the server told it");
        } else {
            assert_eq!(told, (0.0, 0.0, 0.0), "a world that is its own server works them out from its players");
        }
    }
}

#[test]
fn a_machine_destroyed_near_a_player_counts_and_a_walker_counts_more_and_far_players_feel_nothing() {
    let mut s = world();
    player(&mut s, 2, 0.0, 0.0, "");
    player(&mut s, 5, 300.0, 0.0, "");
    spawn(&mut s, 3, r#"{"Director":{"prefab":"Hunter","threshold":50}}"#);
    spawn(&mut s, 4, r#"{"Escalation":{"kill_gain":1.0,"heavy_gain":4.0,"share":100,"decay":0,"heat_gain":0}}"#);
    // a machine 30 m away (it has senses: that is what a machine is), and the gun of player 2 that kills it with one round
    spawn(&mut s, 6, r#"{"Transform":{"pos":[0,1,-30]},"Collider":{"shape":0,"size":[1,1,1]},"HitPoints":{"value":10,"max":10},"Team":{"id":2},"Perception":{}}"#);
    spawn(&mut s, 7, r#"{"Transform":{"pos":[0,1,0]},"Weapon":{"owner":2,"damage":25,"rate":600,"auto":false,"fire":true,"infinite":true,"spread":0,"recoil":[0,0],"noise":0,"range":100}}"#);
    s.step_n(4);
    let a = heat(&s, 2).alert;
    // (the event is at the place of the hit, 29.5 m from the player: 1 times (1 - 29.5 / 100))
    assert!((a - 0.705).abs() < 0.03, "a machine destroyed 30 m away: {a}");
    assert_eq!(heat(&s, 5).alert, 0.0, "300 m away nothing was felt");
    assert_eq!(core(&s, 4).kills, 1);
    // a Walker (a machine with a Stomp) is worth four times that
    // (the wreck of the first stays where it fell and is a wall to a round: the next one stands beside it)
    spawn(&mut s, 8, r#"{"Transform":{"pos":[6,1,-30]},"Collider":{"shape":0,"size":[1,1,1]},"HitPoints":{"value":10,"max":10},"Team":{"id":2},"Perception":{},"Stomp":{}}"#);
    spawn(&mut s, 9, r#"{"Transform":{"pos":[6,1,0]},"Weapon":{"owner":2,"damage":25,"rate":600,"auto":false,"fire":true,"infinite":true,"spread":0,"recoil":[0,0],"noise":0,"range":100}}"#);
    s.step_n(4);
    let b = heat(&s, 2).alert - a;
    assert!((b - 4.0 * 0.705).abs() < 0.12, "a Walker destroyed: {b}");
    assert_eq!(core(&s, 4).kills, 2);
    // a thing without senses (a crate, a player) is no machine
    spawn(&mut s, 10, r#"{"Transform":{"pos":[-6,1,-30]},"Collider":{"shape":0,"size":[1,1,1]},"HitPoints":{"value":10,"max":10},"Team":{"id":2}}"#);
    spawn(&mut s, 11, r#"{"Transform":{"pos":[-6,1,0]},"Weapon":{"owner":2,"damage":25,"rate":600,"auto":false,"fire":true,"infinite":true,"spread":0,"recoil":[0,0],"noise":0,"range":100}}"#);
    let before = heat(&s, 2).alert;
    s.step_n(4);
    assert_eq!(heat(&s, 2).alert, before);
    assert_eq!(core(&s, 4).kills, 2);
}

#[test]
fn the_loud_events_a_game_names_count_by_distance_and_the_machines_waves_grow_with_the_tier() {
    // (an event with a place that the game's own systems make: an explosion)
    let mut s = world();
    player(&mut s, 2, 0.0, 0.0, "");
    spawn(&mut s, 3, r#"{"Director":{"prefab":"Hunter","threshold":50}}"#);
    spawn(&mut s, 4, r#"{"Escalation":{"noise":"explosion 2","share":100,"decay":0,"heat_gain":0}}"#);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[50,0,0]},"Lifetime":{"seconds":5}}"#);
    s.apply(&Command::DefineSystem { def: parse(r#"{"name":"boom","each":["Transform","Lifetime"],"do":[{"if":{"eq":[{"tick":true},2]},"then":[{"emit":"explosion"}]}]}"#).unwrap() }).unwrap();
    s.step_n(4);
    let a = heat(&s, 2).alert;
    assert!((a - 1.0).abs() < 0.05, "2 times (1 - 50 / 100): {a}");
    // the tier scales the waves: a player of tier 2 is sent three times the hunters, and waits half as long
    let waves = |alert: f32| {
        let mut s = world();
        player(&mut s, 2, 0.0, 0.0, &format!(r#","value":2,"alert":{alert}"#));
        spawn(&mut s, 3, r#"{"Director":{"prefab":"Hunter","threshold":1,"per_heat":0,"max_alive":2,"cooldown":10,"min_dist":10,"max_dist":15,"cleanup":0}}"#);
        spawn(&mut s, 4, r#"{"Escalation":{"tiers":"tier 1 at=1\ntier 2 at=3 hunters=3 cooldown=0.5 alive=2","decay":0}}"#);
        s.step_n(3);
        (hunters(&mut s), heat(&s, 2).cooldown)
    };
    let (n1, c1) = waves(1.5);
    let (n2, c2) = waves(3.5);
    let (n0, _) = waves(0.0);
    assert_eq!((n0, n1, n2), (1, 1, 3), "no tier and tier 1 send one, tier 2 sends three (and may have two more alive)");
    assert!(c1 > 9.9 && c2 < 5.1 && c2 > 4.8, "{c1} {c2}");
}

#[test]
fn at_the_walker_tier_a_walker_is_sent_with_a_cooldown_a_cap_and_a_clean_up() {
    let mut s = world();
    floor(&mut s);
    player(&mut s, 2, 0.0, 0.0, r#","alert":5"#);
    player(&mut s, 3, 300.0, 0.0, r#","alert":5"#);
    spawn(&mut s, 4, r#"{"Director":{"prefab":"Hunter","threshold":50}}"#);
    spawn(&mut s, 5, r#"{"Escalation":{"tiers":"tier 1 at=1","walker":"Walker","walker_tier":1,"walker_max":1,"walker_cooldown":100,"walker_min_dist":30,"walker_max_dist":40,"walker_near":150,"walker_cleanup":5,"decay":0,"heat_gain":0}}"#);
    s.step_n(20);
    let w = walkers(&mut s);
    assert_eq!(w.len(), 1, "one Walker: the cap of the server is 1: {w:?}");
    let d = flat(w[0].1, [0.0; 3]);
    assert!(d >= 29.0 && d <= 41.0, "on the ring round the first player: {d}");
    let (wuid, wpos) = w[0];
    let e = s.world.entity(wuid).unwrap();
    let p = *s.world.get::<Perception>(e).unwrap();
    assert_eq!((p.state, p.target.0), (3, 2), "it is searching for the player it was sent for: {p:?}");
    assert!(flat(p.seen_pos, [0.0; 3]) < 0.1, "at the player's place: {:?}", p.seen_pos);
    let q = s.world.get::<Transform>(e).unwrap().rot;
    let ax = crate::physics_rot::axes_of(crate::physics_rot::normalize_q(q));
    let (fx, fz) = (-ax[2][0], -ax[2][2]);
    let dot = (fx * -wpos[0] + fz * -wpos[2]) / ((fx * fx + fz * fz).sqrt() * flat(wpos, [0.0; 3]));
    assert!(dot > 0.97, "it faces the player: {dot}");
    assert!((wpos[1] - 0.3).abs() < 0.05, "on the ground: {:?}", wpos);
    let sp = *s.world.get::<SpawnedBy>(e).unwrap();
    assert_eq!(sp.spawner.0, 5, "it is the Core's");
    assert!(heat(&s, 2).walker_cd > 98.0 && heat(&s, 3).walker_cd == 0.0, "the first player waits for the next, the second has not been sent one");
    let c = core(&s, 5);
    assert_eq!((c.sent, c.walkers), (1, 1));
    assert_eq!(res(&s, "ai.walkers"), 1.0);
    assert!(s.events.iter().any(|e| e.name == "walker" && e.uid == wuid));
    // the cap goes up: the second player (300 m from the first Walker, farther than `walker_near`) is sent one; the first is on its cooldown
    set_core(&mut s, 5, |c| c.walker_max = 2);
    s.step_n(20);
    assert_eq!(walkers(&mut s).len(), 2);
    assert!(flat(walkers(&mut s)[1].1, [300.0, 0.0, 0.0]) < 41.0);
    // a player whose cooldown is over but who has a Walker within `walker_near` is not sent another
    set_core(&mut s, 5, |c| c.walker_max = 3);
    let e2 = s.world.entity(2).unwrap();
    s.world.get_mut::<Heat>(e2).unwrap().walker_cd = 0.0;
    s.step_n(20);
    assert_eq!(walkers(&mut s).len(), 2, "one Walker at a time for a player");
    // the Walker that is destroyed is not counted, and its wreck does not keep the player from the next
    s.world.get_mut::<HitPoints>(s.world.entity(wuid).unwrap()).unwrap().value = 0.0;
    s.step_n(10);
    s.step_n(20);
    let alive = walkers(&mut s).len();
    assert_eq!(alive, 3, "the wreck stays, the player has a new Walker: {alive}");
    assert_eq!(core(&s, 5).walkers, 2, "the count has the two alive ones");
    // the ones that have been calm for 5 s are sent away (they had nothing to look at), the wreck is the picture's to remove
    s.step_n(60 * 9);
    let left = walkers(&mut s);
    assert!(left.iter().all(|w| w.0 == wuid), "only the wreck is left: {left:?}");
    assert_eq!(core(&s, 5).walkers, 0);
}

#[test]
fn no_ground_no_walker_and_no_walker_below_the_tier() {
    let mut s = world();
    player(&mut s, 2, 0.0, 0.0, r#","alert":5"#);
    spawn(&mut s, 4, r#"{"Director":{"prefab":"Hunter","threshold":50}}"#);
    spawn(&mut s, 5, r#"{"Escalation":{"tiers":"tier 1 at=1\ntier 2 at=9","walker":"Walker","walker_tier":1,"walker_cooldown":100,"decay":0,"heat_gain":0}}"#);
    s.step_n(30);
    assert!(walkers(&mut s).is_empty(), "there is nothing to stand on");
    let cd = heat(&s, 2).walker_cd;
    assert!(cd > 0.0 && cd <= 4.0, "it tries again in a few seconds: {cd}");
    // with a floor, but a tier that the player has not reached
    let mut t = world();
    floor(&mut t);
    player(&mut t, 2, 0.0, 0.0, r#","alert":5"#);
    spawn(&mut t, 4, r#"{"Director":{"prefab":"Hunter","threshold":50}}"#);
    spawn(&mut t, 5, r#"{"Escalation":{"tiers":"tier 1 at=1\ntier 2 at=9","walker":"Walker","walker_tier":2,"decay":0,"heat_gain":0}}"#);
    t.step_n(30);
    assert!(walkers(&mut t).is_empty(), "the player is in tier 1 and Walkers come at tier 2");
    assert_eq!(heat(&t, 2).tier, 1);
    // the player stands in a small yard (20 m across) inside a solid mass 100 m high (a ray from a little above the player's level starts inside it and finds no ground): the ring round the player is all inside it, no place
    let mut u = world();
    spawn(&mut u, 90, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[20,1,20]},"Collider":{"shape":1}}"#);
    for (k, (x, z, sx, sz)) in [(155.0, 0.0, 290.0, 600.0), (-155.0, 0.0, 290.0, 600.0), (0.0, -155.0, 20.0, 290.0), (0.0, 155.0, 20.0, 290.0)].iter().enumerate() {
        spawn(&mut u, 20 + k as u64, &format!(r#"{{"Transform":{{"pos":[{x},50,{z}],"scale":[{sx},100,{sz}]}},"Collider":{{"shape":1}}}}"#));
    }
    player(&mut u, 2, 0.0, 0.0, r#","alert":5"#);
    spawn(&mut u, 4, r#"{"Director":{"prefab":"Hunter","threshold":50}}"#);
    spawn(&mut u, 5, r#"{"Escalation":{"tiers":"tier 1 at=1","walker":"Walker","walker_tier":1,"walker_min_dist":30,"walker_max_dist":40,"decay":0,"heat_gain":0}}"#);
    u.step_n(30);
    assert!(walkers(&mut u).is_empty(), "no place on the ring where it fits");
}

#[test]
fn a_scene_without_an_escalation_is_the_old_director() {
    let mut s = world();
    player(&mut s, 2, 0.0, 0.0, "");
    spawn(&mut s, 4, r#"{"Director":{"prefab":"Hunter","threshold":50}}"#);
    add_heat(&mut s, 2, 3.0);
    s.step_n(30);
    let h = heat(&s, 2);
    assert_eq!((h.alert, h.tier), (0.0, 0), "nothing of the meter moves without an Escalation: {h:?}");
    assert!((h.value - 3.0).abs() < 1e-4);
    assert_eq!(res(&s, "ai.tier"), 0.0);
}

#[test]
fn a_player_caught_in_the_beam_and_a_pilot_who_flies_raise_the_alert_through_their_heat() {
    // the Core's beam holds a player for a second to detect it, and the director turns detection into heat (1 a second here): 2 s of it is 2 heat, half of it alert
    let mut s = world();
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.6,0]},"Team":{"id":2},"Searchlight":{"range":60,"angle":8,"hold":1.0}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.9,-20]},"Collider":{"shape":2,"size":[0.7,1.8,0.7]},"HitPoints":{"value":100,"max":100},"Team":{"id":1},"Heat":{"decay":0}}"#);
    spawn(&mut s, 3, r#"{"Director":{"prefab":"Hunter","threshold":50,"light_heat":1.0}}"#);
    spawn(&mut s, 4, r#"{"Escalation":{"decay":0,"tiers":"tier 1 at=0.5"}}"#);
    s.step_n(180);
    let h = heat(&s, 2);
    assert!(h.alert > 0.8 && h.alert < 1.2 && h.tier == 1, "being detected is heat and the alert takes half of it: {h:?}");
    // a helicopter's noise is heat on its pilot (2 a second at full noise): 3 s of flight, 6 heat, 3 alert
    let mut s = world();
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[2000,1,2000]}}"#);
    spawn(&mut s, 5, r#"{"Transform":{},"Heat":{"decay":0}}"#);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,60,0]},"Collider":{"shape":1,"size":[2,2,6]},"RigidBody":{"mass":2000},"Velocity":{},"AngularVelocity":{"damping":0},"HitPoints":{"value":1000,"max":1000,"last":1000},"Helicopter":{"spool":1,"collective":1,"heat":2,"pilot":5}}"#);
    spawn(&mut s, 3, r#"{"Director":{"prefab":"Hunter","threshold":50}}"#);
    spawn(&mut s, 4, r#"{"Escalation":{"decay":0,"tiers":"tier 1 at=2"}}"#);
    s.step_n(180);
    let h = heat(&s, 5);
    assert!(h.alert > 2.5 && h.alert < 3.5 && h.tier == 1, "a pilot who flies is noisy: {h:?}");
}

#[test]
fn the_core_replays_to_the_same_world_walkers_and_all() {
    // two players, heat that comes in jumps, hunters and Walkers sent, a cooldown, a cap, clean-up: run twice, the same world at every checkpoint
    let run = || {
        let mut s = world();
        floor(&mut s);
        player(&mut s, 2, 0.0, 0.0, r#","alert":5"#);
        player(&mut s, 3, 300.0, 0.0, r#","alert":5"#);
        spawn(&mut s, 4, r#"{"Director":{"prefab":"Hunter","threshold":3}}"#);
        spawn(&mut s, 5, r#"{"Escalation":{"tiers":"tier 1 at=1\ntier 2 at=4 hunters=2\n","walker":"Walker","walker_tier":2,"walker_max":2,"walker_cooldown":5,"walker_min_dist":30,"walker_max_dist":40,"walker_near":150,"walker_cleanup":5,"decay":0.1}}"#);
        let mut hashes = Vec::new();
        let mut most = (0, 0);
        for t in 0..900 {
            if t % 200 == 100 {
                add_heat(&mut s, 2, 6.0);
                add_heat(&mut s, 3, 3.0);
            }
            s.step();
            most = (most.0.max(walkers(&mut s).len()), most.1.max(hunters(&mut s)));
            if t % 100 == 99 {
                hashes.push(s.state_hash());
            }
        }
        (hashes, most)
    };
    let (a, most_a) = run();
    let (b, most_b) = run();
    assert_eq!(a, b, "the same inputs give the same world, checkpoint for checkpoint");
    assert_eq!(most_a, most_b);
    assert!(most_a.0 >= 1 && most_a.1 >= 1, "the run was not empty: at most {} Walkers and {} hunters at a time", most_a.0, most_a.1);
}

const LOUD_GUN: &str = r#""Weapon":{"kind":0,"damage":1,"rate":600,"auto":true,"fire":true,"infinite":true,"mag_size":1000000,"mag":1000000,"reload_time":0,"noise":100,"range":50,"spread":0,"recoil":[0,0]}"#;

#[test]
fn a_gun_on_a_child_of_the_player_that_is_heard_is_the_players_heat() {
    // the avatar's gun is a child of its head with no `owner` of its own: the director takes the owner the way the weapons system does, from the parent (before, the shots of a player's tool were nobody's heat)
    let mut s = world();
    floor(&mut s);
    player(&mut s, 2, 0.0, 0.0, "");
    spawn(&mut s, 3, &format!(r#"{{"Parent":{{"target":2}},"LocalTransform":{{"pos":[0,1.5,0]}},"Transform":{{"pos":[0,1.5,0]}},{LOUD_GUN}}}"#));
    spawn(&mut s, 4, r#"{"Director":{"prefab":"Hunter","threshold":50,"shot_heat":0.15,"light_heat":0}}"#);
    s.step_n(60);
    let h = heat(&s, 2);
    assert!(h.value > 1.0 && h.value < 2.0, "ten shots a second, 0.15 each: {h:?}");
    // a gun with no owner and no parent (a machine's own) is nobody's heat, and neither is a quiet one
    let mut t = world();
    floor(&mut t);
    player(&mut t, 2, 0.0, 0.0, "");
    spawn(&mut t, 3, &format!(r#"{{"Transform":{{"pos":[10,1.5,0]}},{LOUD_GUN}}}"#));
    spawn(&mut t, 5, &format!(r#"{{"Parent":{{"target":2}},"LocalTransform":{{"pos":[0,1.5,0]}},"Transform":{{"pos":[0,1.5,0]}},{}}}"#, LOUD_GUN.replace("\"noise\":100", "\"noise\":0")));
    spawn(&mut t, 4, r#"{"Director":{"prefab":"Hunter","threshold":50,"shot_heat":0.15,"light_heat":0}}"#);
    t.step_n(60);
    assert_eq!(heat(&t, 2).value, 0.0, "a machine's gun and a quiet gun raise nothing");
}
