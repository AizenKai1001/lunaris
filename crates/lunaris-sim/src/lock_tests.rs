//! Code locks: a door or a box that only the players who know the code can use, the code entered at a lock the player looks at, the lockout after three wrong codes, a new code that takes the others out, a lock fitted from the bag, and that it is all in
//! the world's save. No art: boxes.
use super::*;
use crate::components::{Door, HitPoints, Interactable, Interactor, NetEntity, Transform};
use crate::inventory::Inventory;
use crate::lock::{CodeLock, Keypad, FLAG_LOCKED};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn events(s: &Sim, name: &str) -> Vec<(u64, Option<[f32; 3]>)> {
    s.events.iter().filter(|e| e.name == name).map(|e| (e.uid, e.pos)).collect()
}

fn step(s: &mut Sim, n: usize) {
    for _ in 0..n {
        s.step();
    }
}

const ITEMS: &str = "item lock | Code lock | stack=5\\nitem pistol | Pistol | stack=1\\n";

/// A player: the avatar `uid` (at x, with a bag, a keypad and the name `name`) and its head `uid + 1` (1.6 m up, looking down -z, with the use button).
fn player(s: &mut Sim, uid: u64, name: &str, x: f32, locks: u32) {
    let mut id = vec!["0".to_string(); 48];
    let mut count = vec!["0".to_string(); 48];
    if locks > 0 {
        (id[0], count[0]) = ("1".into(), locks.to_string());
    }
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{x},0,0]}},"HitPoints":{{"value":100,"max":100,"last":100}},"Inventory":{{"size":12,"id":[{}],"count":[{}]}},"NetAvatar":{{"name":"{name}","head":{}}},"Keypad":{{"item":"lock"}}}}"#, id.join(","), count.join(","), uid + 1));
    spawn(s, uid + 1, &format!(r#"{{"Transform":{{"pos":[{x},1.6,0]}},"Interactor":{{"reach":3,"look":false,"inventory":{uid}}}}}"#));
}

fn world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 900, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[200,1,200]}}"#);
    s
}

/// A door 2 m in front of the players (z = -2), with a code lock of this code that the player named `keeper` is in.
fn locked_door(s: &mut Sim, uid: u64, code: i32, keeper: &str) {
    let name = format!("\"{keeper}\"");
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[0,1.05,-2]}},"Collider":{{"shape":1,"size":[1.1,2.1,0.12]}},"Interactable":{{"kind":1,"prompt":"Open"}},"Door":{{"hinge":[-0.55,0,0],"angle_open":-100,"speed":200}},"NetEntity":{{"class":3,"kind":30}},"CodeLock":{{"code":{code},"auth":[{name},"","","","","","",""]}}}}"#));
}

fn door_open(s: &Sim, uid: u64) -> bool {
    s.world.get::<Door>(s.world.entity(uid).unwrap()).unwrap().open
}

/// Shut the door again and let it swing back (a door that stands open has swung away from where the players aim).
fn shut(s: &mut Sim, uid: u64) {
    s.world.get_mut::<Door>(s.world.entity(uid).unwrap()).unwrap().open = false;
    step(s, 60);
}

fn lock(s: &Sim, uid: u64) -> CodeLock {
    *s.world.get::<CodeLock>(s.world.entity(uid).unwrap()).unwrap()
}

fn names(s: &Sim, l: &CodeLock) -> Vec<String> {
    l.auth.iter().filter(|a| a.0 != 0).map(|a| s.world.strings().get(a.0).to_string()).collect()
}

fn denied(s: &Sim, head: u64) -> String {
    let i = s.world.get::<Interactor>(s.world.entity(head).unwrap()).unwrap();
    s.world.strings().get(i.denied.0).to_string()
}

/// The player presses the use button (the head's), for one tick.
fn press_use(s: &mut Sim, head: u64) {
    s.events.clear();
    s.world.get_mut::<Interactor>(s.world.entity(head).unwrap()).unwrap().act = true;
    s.step();
    s.world.get_mut::<Interactor>(s.world.entity(head).unwrap()).unwrap().act = false;
}

/// The player's keypad request (what the game sets from the player's command), then enough steps that the next may be heard.
fn ask(s: &mut Sim, avatar: u64, act: u32, code: i32) {
    s.events.clear();
    let k = s.world.get_mut::<Keypad>(s.world.entity(avatar).unwrap()).unwrap();
    k.act = act;
    k.code = code;
    s.step();
    // (a right code presses the head's use button for the player; the game server lets go of it with the next report of the buttons, which is what this does)
    if let Some(h) = s.world.entity(avatar + 1) {
        if let Some(i) = s.world.get_mut::<Interactor>(h) {
            i.act = false;
        }
    }
}

fn keypad_denied(s: &Sim, avatar: u64) -> String {
    let k = s.world.get::<Keypad>(s.world.entity(avatar).unwrap()).unwrap();
    s.world.strings().get(k.denied.0).to_string()
}

#[test]
fn a_door_with_a_lock_refuses_a_player_who_is_not_in_and_asks_for_the_code() {
    let mut s = world();
    player(&mut s, 10, "Ada", -0.2, 0);
    player(&mut s, 20, "Bo", 0.2, 0);
    locked_door(&mut s, 50, 1234, "Ada");
    step(&mut s, 2);
    // Bo is not in: refused, and told (the event is about Bo's head, which is how the server tells a player)
    press_use(&mut s, 21);
    assert!(!door_open(&s, 50), "the door stays shut");
    assert_eq!(denied(&s, 21), "locked");
    assert_eq!(events(&s, "lock_prompt").iter().map(|e| e.0).collect::<Vec<_>>(), vec![21]);
    assert_eq!(events(&s, "locked").iter().map(|e| e.0).collect::<Vec<_>>(), vec![50]);
    // Ada, the keeper, opens it
    step(&mut s, 30);
    press_use(&mut s, 11);
    assert!(door_open(&s, 50), "the keeper is in");
    assert!(events(&s, "lock_prompt").is_empty());
}

#[test]
fn the_right_code_puts_the_player_in_and_opens_the_door_at_once_and_it_is_this_players_only() {
    let mut s = world();
    player(&mut s, 10, "Ada", -0.2, 0);
    player(&mut s, 20, "Bo", 0.2, 0);
    locked_door(&mut s, 50, 1234, "Ada");
    step(&mut s, 2);
    ask(&mut s, 20, 1, 1234);
    assert_eq!(events(&s, "lock_ok").iter().map(|e| e.0).collect::<Vec<_>>(), vec![21], "about Bo's head");
    assert!(door_open(&s, 50), "the door opened in the same step, as if Bo had pressed the use button");
    assert_eq!(names(&s, &lock(&s, 50)), vec!["Ada", "Bo"]);
    assert!(events(&s, "locked").is_empty(), "no refusal");
    // the request is answered and cleared
    assert_eq!(s.world.get::<Keypad>(s.world.entity(20).unwrap()).unwrap().act, 0);
    // a third player who has not entered it is still out
    player(&mut s, 30, "Cy", 0.0, 0);
    shut(&mut s, 50);
    press_use(&mut s, 31);
    assert_eq!(denied(&s, 31), "locked");
    assert!(!door_open(&s, 50));
}

#[test]
fn a_wrong_code_is_told_and_three_lock_the_player_out_for_thirty_seconds_and_nobody_else() {
    let mut s = world();
    player(&mut s, 10, "Ada", -0.2, 0);
    player(&mut s, 20, "Bo", 0.2, 0);
    locked_door(&mut s, 50, 1234, "Ada");
    step(&mut s, 2);
    ask(&mut s, 20, 1, 1);
    assert_eq!(events(&s, "lock_bad").len(), 1);
    assert_eq!(keypad_denied(&s, 20), "wrong_code");
    step(&mut s, 30);
    ask(&mut s, 20, 1, 2);
    assert_eq!(events(&s, "lock_bad").len(), 1, "the second");
    step(&mut s, 30);
    ask(&mut s, 20, 1, 3);
    assert_eq!(events(&s, "lock_out").len(), 1, "the third wrong code locks Bo out");
    assert_eq!(events(&s, "lock_bad").len(), 0);
    assert_eq!(keypad_denied(&s, 20), "locked_out");
    // the right code does not help while it lasts, and does not extend it
    step(&mut s, 30);
    ask(&mut s, 20, 1, 1234);
    assert_eq!(events(&s, "lock_out").len(), 1);
    assert!(!door_open(&s, 50));
    assert_eq!(names(&s, &lock(&s, 50)), vec!["Ada"], "Bo is not in");
    // another player is not held by it
    player(&mut s, 30, "Cy", 0.0, 0);
    step(&mut s, 2);
    ask(&mut s, 30, 1, 1234);
    assert_eq!(events(&s, "lock_ok").len(), 1);
    assert_eq!(names(&s, &lock(&s, 50)), vec!["Ada", "Cy"]);
    shut(&mut s, 50);
    // 29 seconds after it began: still out; after 30, three new tries and the right one works
    step(&mut s, 1655);
    ask(&mut s, 20, 1, 1234);
    assert_eq!(events(&s, "lock_out").len(), 1, "29 seconds in");
    step(&mut s, 100);
    ask(&mut s, 20, 1, 1234);
    assert_eq!(events(&s, "lock_ok").len(), 1, "after the thirty seconds");
    assert!(names(&s, &lock(&s, 50)).contains(&"Bo".to_string()));
}

#[test]
fn a_new_code_takes_everybody_elses_authorisation_away_and_only_a_keeper_may_set_one() {
    let mut s = world();
    player(&mut s, 10, "Ada", -0.2, 0);
    player(&mut s, 20, "Bo", 0.2, 0);
    locked_door(&mut s, 50, 1234, "Ada");
    step(&mut s, 2);
    ask(&mut s, 20, 1, 1234);
    assert_eq!(names(&s, &lock(&s, 50)).len(), 2);
    shut(&mut s, 50);
    // a stranger cannot change it; a code out of range is refused
    player(&mut s, 30, "Cy", 0.0, 0);
    step(&mut s, 2);
    ask(&mut s, 30, 2, 5555);
    assert_eq!(events(&s, "lock_refused_not_yours").len(), 1);
    assert_eq!(lock(&s, 50).code, 1234);
    ask(&mut s, 10, 2, 10000);
    assert_eq!(events(&s, "lock_refused_bad_code").len(), 1);
    step(&mut s, 30);
    ask(&mut s, 10, 2, -3);
    assert_eq!(events(&s, "lock_refused_bad_code").len(), 1);
    // Ada changes it: only she is in
    step(&mut s, 30);
    ask(&mut s, 10, 2, 4321);
    assert_eq!(events(&s, "lock_new").len(), 1);
    assert_eq!(lock(&s, 50).code, 4321);
    assert_eq!(names(&s, &lock(&s, 50)), vec!["Ada"]);
    // Bo is out again, and the old code does not work for him; the new one does
    step(&mut s, 30);
    press_use(&mut s, 21);
    assert_eq!(denied(&s, 21), "locked");
    step(&mut s, 30);
    ask(&mut s, 20, 1, 1234);
    assert_eq!(events(&s, "lock_bad").len(), 1);
    step(&mut s, 30);
    ask(&mut s, 20, 1, 4321);
    assert_eq!(events(&s, "lock_ok").len(), 1);
    // 0 is a code too, and a code with leading zeros is the number
    shut(&mut s, 50);
    ask(&mut s, 10, 2, 0);
    assert_eq!(lock(&s, 50).code, 0);
}

#[test]
fn a_lock_is_fitted_from_the_bag_to_a_door_or_a_box_and_the_fitter_is_its_keeper() {
    let mut s = world();
    player(&mut s, 10, "Ada", -0.2, 2);
    player(&mut s, 20, "Bo", 0.2, 0);
    spawn(&mut s, 50, r#"{"Transform":{"pos":[0,1.05,-2]},"Collider":{"shape":1,"size":[1.1,2.1,0.12]},"Interactable":{"kind":1},"Door":{},"HitPoints":{"value":200,"max":200,"last":200},"NetEntity":{"class":3,"kind":30}}"#);
    step(&mut s, 2);
    // without the item, nothing
    ask(&mut s, 20, 3, 0);
    assert_eq!(events(&s, "lock_refused_no_item").len(), 1);
    assert!(s.world.get::<CodeLock>(s.world.entity(50).unwrap()).is_none());
    step(&mut s, 30);
    ask(&mut s, 10, 3, 0);
    assert_eq!(events(&s, "lock_placed").iter().map(|e| e.0).collect::<Vec<_>>(), vec![11], "about Ada's head: her screen asks for the code");
    let l = lock(&s, 50);
    assert_eq!((l.code, l.tries, l.lockout), (-1, 3, 30.0));
    assert_eq!(names(&s, &l), vec!["Ada"]);
    assert_eq!(s.world.strings().get(l.owner.0), "Ada");
    let inv = s.world.get::<Inventory>(s.world.entity(10).unwrap()).unwrap();
    assert_eq!(inv.count[0], 1, "one lock left the bag");
    assert_eq!(s.world.get::<NetEntity>(s.world.entity(50).unwrap()).unwrap().flags & FLAG_LOCKED, FLAG_LOCKED, "the clients can see it has a lock");
    // a lock with no code is closed to everybody but its keeper
    step(&mut s, 30);
    press_use(&mut s, 21);
    assert_eq!(denied(&s, 21), "locked");
    ask(&mut s, 20, 1, 1234);
    assert_eq!(events(&s, "lock_refused_no_code").len(), 1, "Bo cannot try a code on it");
    // Ada chooses the code (her screen has just asked her to)
    step(&mut s, 30);
    ask(&mut s, 10, 2, 777);
    assert_eq!((lock(&s, 50).code, events(&s, "lock_new").len()), (777, 1));
    step(&mut s, 30);
    press_use(&mut s, 11);
    assert!(door_open(&s, 50), "Ada can use it");
    // (she shuts it again: a door that stands open has swung away from where she looks)
    shut(&mut s, 50);
    // a keeper who asks for the lock again is offered the page that changes the code (and no second lock is used); a stranger is refused
    step(&mut s, 30);
    ask(&mut s, 10, 3, 0);
    assert_eq!(events(&s, "lock_setup").len(), 1);
    assert_eq!(s.world.get::<Inventory>(s.world.entity(10).unwrap()).unwrap().count[0], 1);
    step(&mut s, 30);
    ask(&mut s, 20, 3, 0);
    assert_eq!(events(&s, "lock_refused_not_yours").len(), 1);
}

#[test]
fn only_doors_and_containers_that_stay_can_be_given_a_lock() {
    let mut s = world();
    player(&mut s, 10, "Ada", 0.0, 5);
    // a pickup, a thing that goes away by itself (a corpse), and a switch
    spawn(&mut s, 50, r#"{"Transform":{"pos":[0,1.0,-2]},"Collider":{"shape":1,"size":[1,2,0.5]},"Interactable":{"kind":3,"item":"pistol 1"}}"#);
    step(&mut s, 2);
    ask(&mut s, 10, 3, 0);
    assert_eq!(events(&s, "lock_refused_not_lockable").len(), 1, "a pickup");
    s.world.despawn(s.world.entity(50).unwrap());
    spawn(&mut s, 51, r#"{"Transform":{"pos":[0,1.0,-2]},"Collider":{"shape":1,"size":[1,2,0.5]},"Interactable":{"kind":2},"Inventory":{"size":8},"Lifetime":{"seconds":300}}"#);
    step(&mut s, 30);
    ask(&mut s, 10, 3, 0);
    assert_eq!(events(&s, "lock_refused_not_lockable").len(), 1, "a corpse");
    s.world.despawn(s.world.entity(51).unwrap());
    spawn(&mut s, 52, r#"{"Transform":{"pos":[0,1.0,-2]},"Collider":{"shape":1,"size":[1,2,0.5]},"Interactable":{"kind":2},"Inventory":{"size":8}}"#);
    step(&mut s, 30);
    ask(&mut s, 10, 3, 0);
    assert_eq!(events(&s, "lock_refused_not_lockable").len(), 1, "a box that cannot be destroyed (a loot crate of the world) cannot be locked away from everybody");
    s.world.despawn(s.world.entity(52).unwrap());
    spawn(&mut s, 53, r#"{"Transform":{"pos":[0,1.0,-2]},"Collider":{"shape":1,"size":[1,2,0.5]},"Interactable":{"kind":2},"Inventory":{"size":8},"HitPoints":{"value":120,"max":120,"last":120}}"#);
    step(&mut s, 30);
    ask(&mut s, 10, 3, 0);
    assert_eq!(events(&s, "lock_placed").len(), 1, "a box that can be broken takes it");
    assert_eq!(s.world.get::<Inventory>(s.world.entity(10).unwrap()).unwrap().count[0], 4);
}

#[test]
fn a_box_with_a_lock_does_not_open_for_a_stranger_and_opens_for_the_one_who_knows_the_code() {
    let mut s = world();
    player(&mut s, 10, "Ada", 0.0, 0);
    spawn(&mut s, 52, r#"{"Transform":{"pos":[0,1.0,-2]},"Collider":{"shape":1,"size":[1,2,1]},"Interactable":{"kind":2},"Inventory":{"size":8},"CodeLock":{"code":42}}"#);
    step(&mut s, 2);
    press_use(&mut s, 11);
    assert!(!s.world.get::<Interactable>(s.world.entity(52).unwrap()).unwrap().open);
    assert_eq!(denied(&s, 11), "locked");
    step(&mut s, 30);
    ask(&mut s, 10, 1, 42);
    assert!(s.world.get::<Interactable>(s.world.entity(52).unwrap()).unwrap().open, "the box opened for her");
    assert_eq!(events(&s, "container_open").len(), 1);
}

#[test]
fn a_code_is_tried_only_at_a_lock_the_player_looks_at_within_its_reach() {
    let mut s = world();
    player(&mut s, 10, "Ada", 0.0, 0);
    locked_door(&mut s, 50, 1234, "Bo");
    step(&mut s, 2);
    // looking away (the head turned half a circle)
    s.world.get_mut::<Transform>(s.world.entity(11).unwrap()).unwrap().rot = [0.0, 1.0, 0.0, 0.0];
    ask(&mut s, 10, 1, 1);
    assert_eq!(events(&s, "lock_refused_no_target").len(), 1);
    assert_eq!(lock(&s, 50).strikes, [0; 4], "nothing was counted against the lock's code");
    // far (the player 5 m from the door)
    s.world.get_mut::<Transform>(s.world.entity(11).unwrap()).unwrap().rot = [0.0, 0.0, 0.0, 1.0];
    s.world.get_mut::<Transform>(s.world.entity(11).unwrap()).unwrap().pos = [0.0, 1.6, 3.0];
    step(&mut s, 30);
    ask(&mut s, 10, 1, 1);
    assert_eq!(events(&s, "lock_refused_no_target").len(), 1);
    // a wall in front hides it
    s.world.get_mut::<Transform>(s.world.entity(11).unwrap()).unwrap().pos = [0.0, 1.6, 0.0];
    spawn(&mut s, 60, r#"{"Transform":{"pos":[0,1.5,-1]},"Collider":{"shape":1,"size":[3,3,0.2]}}"#);
    step(&mut s, 30);
    ask(&mut s, 10, 1, 1);
    assert_eq!(events(&s, "lock_refused_no_target").len(), 1, "behind a wall");
    assert_eq!(lock(&s, 50).strikes, [0; 4]);
    // (and a thing with no lock says so)
    s.world.despawn(s.world.entity(60).unwrap());
    s.world.remove_typed::<CodeLock>(s.world.entity(50).unwrap()).unwrap();
    step(&mut s, 30);
    ask(&mut s, 10, 1, 1);
    assert_eq!(events(&s, "lock_refused_no_lock").len(), 1);
}

#[test]
fn a_request_is_not_heard_again_for_a_moment_and_a_dead_player_asks_nothing() {
    let mut s = world();
    player(&mut s, 10, "Ada", 0.0, 0);
    locked_door(&mut s, 50, 1234, "Bo");
    step(&mut s, 2);
    ask(&mut s, 10, 1, 1);
    assert_eq!(events(&s, "lock_bad").len(), 1);
    // ten times in the next ten ticks: none is heard (0.4 s between two)
    for _ in 0..10 {
        ask(&mut s, 10, 1, 2);
        assert!(events(&s, "lock_bad").is_empty() && events(&s, "lock_out").is_empty());
    }
    assert_eq!(lock(&s, 50).strikes.iter().sum::<u32>(), 1);
    step(&mut s, 30);
    s.world.get_mut::<HitPoints>(s.world.entity(10).unwrap()).unwrap().dead = true;
    ask(&mut s, 10, 1, 3);
    assert_eq!(events(&s, "lock_refused_dead").len(), 1);
    assert_eq!(lock(&s, 50).strikes.iter().sum::<u32>(), 1);
}

#[test]
fn what_a_lock_knows_survives_a_save_and_a_load() {
    let mut s = world();
    player(&mut s, 10, "Ada", -0.2, 0);
    player(&mut s, 20, "Bo", 0.2, 0);
    locked_door(&mut s, 50, 1234, "Ada");
    step(&mut s, 2);
    ask(&mut s, 20, 1, 1234);
    step(&mut s, 30);
    ask(&mut s, 10, 1, 99); // (a keeper is in: nothing is counted)
    let save = s.save_json();
    let mut t = Sim::new(1);
    t.load_save(&save).unwrap();
    let l = lock(&t, 50);
    assert_eq!(l.code, 1234);
    assert_eq!(names(&t, &l), vec!["Ada", "Bo"]);
    assert_eq!((l.tries, l.lockout), (3, 30.0));
}

#[test]
fn a_player_with_no_name_is_known_by_its_number() {
    let mut s = world();
    // (no NetAvatar: a scene that is not a game server)
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,0,0]},"HitPoints":{"value":100,"max":100},"Inventory":{"size":4},"Keypad":{"item":"lock"}}"#);
    spawn(&mut s, 11, r#"{"Transform":{"pos":[0,1.6,0]},"Parent":{"target":10},"LocalTransform":{"pos":[0,1.6,0]},"Interactor":{"reach":3,"look":false}}"#);
    spawn(&mut s, 50, r#"{"Transform":{"pos":[0,1.05,-2]},"Collider":{"shape":1,"size":[1.1,2.1,0.12]},"Interactable":{"kind":1},"Door":{},"CodeLock":{"code":7}}"#);
    s.world.get_mut::<Keypad>(s.world.entity(10).unwrap()).unwrap().aim = lunaris_ecs::Uid(11);
    step(&mut s, 2);
    press_use(&mut s, 11);
    assert!(!door_open(&s, 50));
    step(&mut s, 30);
    ask(&mut s, 10, 1, 7);
    assert!(door_open(&s, 50));
    let l = lock(&s, 50);
    assert_eq!(names(&s, &l), vec!["#10"]);
}

#[test]
fn a_door_with_a_lock_is_broken_by_a_blast_like_any_door_and_the_way_is_free() {
    let mut s = world();
    spawn(&mut s, 50, r#"{"Transform":{"pos":[0,1.05,-2]},"Collider":{"shape":1,"size":[1.1,2.1,0.12]},"Interactable":{"kind":1},"Door":{},"HitPoints":{"value":200,"max":200,"last":200,"remove_after":0.05},"CodeLock":{"code":1234}}"#);
    spawn(&mut s, 9, r#"{"Transform":{"pos":[0,1.0,-1.5]},"Explosive":{"fuse":0,"radius":5,"damage":120,"structure_mult":2.8,"cover":1}}"#);
    step(&mut s, 8);
    assert!(s.world.entity(50).is_none(), "the door and its lock are gone: 120 x 2.8 at half a metre");
}

#[test]
fn a_text_field_tells_its_text_as_a_number() {
    let mut s = Sim::new(1);
    spawn(&mut s, 5, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.2,0.05],"button":true},"UiInput":{"text":"","max_len":12,"focused":true}}"#);
    let number = |s: &Sim| s.world.get::<crate::components::UiInput>(s.world.entity(5).unwrap()).unwrap().number;
    s.step();
    assert_eq!(number(&s), -1, "nothing typed");
    for (text, want) in [("4711", 4711), ("0042", 42), ("7", 7), ("0", 0), ("12a", -1), ("-5", -1), ("1 2", -1), ("123456789", 123456789), ("1234567890", -1)] {
        crate::ui_kit::clear_focused(&mut s.world);
        crate::ui_kit::type_into_focused(&mut s.world, text);
        s.step();
        assert_eq!(number(&s), want, "the text `{text}`");
    }
}

/// What idle keypads cost a tick (run by hand: `cargo test -p lunaris-sim --lib lock_costs --release -- --ignored --nocapture`).
#[test]
#[ignore]
fn lock_costs_what_the_docs_say() {
    let mut s = world();
    for k in 0..200u64 {
        player(&mut s, 1000 + k * 2, &format!("p{k}"), (k % 20) as f32 * 3.0, 0);
    }
    s.profile = Some(Default::default());
    step(&mut s, 600);
    let p = s.profile.take().unwrap();
    let st = p.stages.get("gameplay lock::update").unwrap();
    println!("200 keypads, no request: lock::update {:.4} ms a tick on average, {:.4} ms at most", st.total_ms / st.calls as f64, st.max_ms);
    locked_door(&mut s, 50, 1234, "p0");
    let t = std::time::Instant::now();
    ask(&mut s, 1000, 1, 5);
    println!("one request at a lock (the tick of the request): {:.3} ms", t.elapsed().as_secs_f64() * 1000.0);
}
