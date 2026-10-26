//! World events: the table and its mistakes, the scheduler (the first start, the range, never two of a line, the clock that waits for players, `force`), the place (inside the square, clear of walls, not on a roof or a piece, the same every run), the
//! supply drop (the descent, the landing, the switch of the picture, the noise the machines hear, the heat, the claim, the clean-up), a restart in the middle of one, and a connected client that starts nothing.
use super::*;
use crate::components::{Heat, Interactable, NetEntity, Noisy, Perception, Transform};
use crate::events::{descent, parse_table, AirDrop, WorldEvent, WorldEvents};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

const DROP: &str = r#"{"components":{"Name":{"value":"Drop"},"Transform":{},"AirDrop":{"fall":20,"sway":2.5,"rest":0.5,"noise":80,"noise_for":20,"land_class":3,"land_kind":141,"claim_for":8,"empty_life":30},
    "Interactable":{"kind":2,"enabled":false},"Inventory":{"size":8},"NetEntity":{"class":4,"kind":140},"Collider":{"shape":1,"size":[1.2,1,0.9]}}}"#;
const FLARE: &str = r#"{"components":{"Name":{"value":"Flare"},"Transform":{},"Noisy":{},"NetEntity":{"class":3,"kind":142}}}"#;
const TABLE: &str = "event drop kind=7 prefab=Drop marker=Flare every=100..150 first=30 alive=1 height=100 life=60 inside=30 clear=3";

fn world() -> Sim {
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "Drop".into(), def: parse(DROP).unwrap() }).unwrap();
    s.apply(&Command::DefinePrefab { name: "Flare".into(), def: parse(FLARE).unwrap() }).unwrap();
    spawn(&mut s, 90, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[1000,1,1000]},"Collider":{"shape":1}}"#);
    s
}

fn scheduler(s: &mut Sim, table: &str, extra: &str) {
    let t = format!("{table:?}"); // (a Rust string literal is a JSON string for the text of a table)
    spawn(s, 5, &format!(r#"{{"WorldEvents":{{"table":{t}{extra}}}}}"#));
}

fn sch(s: &Sim) -> WorldEvents {
    *s.world.get::<WorldEvents>(s.world.entity(5).unwrap()).unwrap()
}

fn res(s: &Sim, name: &str) -> f64 {
    s.world.resource(name).unwrap_or(0.0)
}

/// Every event of the line (main entities only: no owner), oldest first.
fn drops(s: &mut Sim) -> Vec<(u64, WorldEvent, [f32; 3])> {
    let mut v: Vec<(u64, WorldEvent, [f32; 3])> = Vec::new();
    let _ = s.world.for_each_uid::<(&WorldEvent, &Transform), _>(|_, uid, (w, t)| {
        if w.owner.0 == 0 {
            v.push((uid, *w, t.pos));
        }
    });
    v.sort_by_key(|d| d.0);
    v
}

/// The marker of the event `owner`.
fn marker_of(s: &mut Sim, owner: u64) -> u64 {
    let mut found = 0;
    let _ = s.world.for_each_uid::<(&WorldEvent,), _>(|_, uid, (w,)| {
        if w.owner.0 == owner {
            found = uid;
        }
    });
    found
}

fn markers(s: &mut Sim) -> usize {
    let mut n = 0;
    let _ = s.world.for_each::<(&WorldEvent,), _>(|_, (w,)| n += (w.owner.0 != 0) as usize);
    n
}

fn count(s: &Sim, name: &str) -> usize {
    s.events.iter().filter(|e| e.name == name).count()
}

fn secs(s: &mut Sim, seconds: f32) {
    s.step_n((seconds * 60.0).round() as u64);
}

fn get<T: lunaris_ecs::Component>(s: &Sim, uid: u64) -> T {
    *s.world.get::<T>(s.world.entity(uid).unwrap()).unwrap()
}

fn flat(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[test]
fn the_table_is_read_and_its_mistakes_named() {
    let (lines, problems) = parse_table(TABLE);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(lines.len(), 1);
    let l = &lines[0];
    assert_eq!((l.name.as_str(), l.kind, l.prefab.as_str(), l.marker.as_str()), ("drop", 7, "Drop", "Flare"));
    assert_eq!((l.every, l.first, l.alive, l.height, l.life, l.inside, l.clear), ((100.0, 150.0), 30.0, 1, 100.0, 60.0, 30.0, 3.0));
    let (lines, problems) = parse_table("# a\nevent a prefab=P every=60\nevent b prefab=P kind=3 maxy=0.5\nevent a prefab=P\nevent c\nevent d prefab=P every=9..3\nevent e prefab=P bad\nevent f prefab=P speed=3\nthing x\nevent g prefab=P life=-4\nevent\n");
    assert_eq!(lines.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), vec!["a", "b"], "{lines:?}");
    assert_eq!(lines[0].every, (60.0, 60.0), "a number is a fixed time");
    assert_eq!((lines[0].kind, lines[1].kind), (1, 3), "the kind is the line's number unless it says");
    assert_eq!(lines[1].maxy, 0.5);
    assert_eq!(problems.len(), 8, "a name twice, no prefab, a range backwards, a word that is not key=value, an unknown key, a line that is not an event, a negative number, no name: {problems:#?}");
    let many: String = (0..10).map(|i| format!("event e{i} prefab=P\n")).collect();
    let (lines, problems) = parse_table(&many);
    assert_eq!((lines.len(), problems.len()), (8, 2), "at most eight lines count");
    // the descent is a straight line
    assert_eq!((descent(100.0, 20.0, 0.0), descent(100.0, 20.0, 10.0), descent(100.0, 20.0, 20.0), descent(100.0, 20.0, 99.0)), (100.0, 50.0, 0.0, 0.0));
    assert_eq!(descent(100.0, 0.0, 5.0), 0.0, "no fall time: it is down at once");
}

#[test]
fn the_first_event_starts_after_first_and_the_next_within_the_range() {
    let mut s = world();
    scheduler(&mut s, TABLE, "");
    secs(&mut s, 29.0);
    assert_eq!((drops(&mut s).len(), sch(&s).n), (0, 0), "not before `first`");
    assert_eq!(res(&s, "ev.on"), 0.0);
    secs(&mut s, 2.0);
    let d = drops(&mut s);
    assert_eq!(d.len(), 1, "one event after 31 seconds");
    let (uid, w, pos) = d[0];
    assert_eq!((w.n, w.kind, w.line), (1, 7, 1));
    assert!(w.x.abs() <= 30.0 && w.z.abs() <= 30.0, "inside the square: {w:?}");
    assert!((w.ground - 0.0).abs() < 0.01, "on the floor: {}", w.ground);
    assert!(pos[1] > 90.0 && pos[1] < 101.0, "it begins high above the place and has fallen a little (1 s of 20): {pos:?}");
    assert_eq!(markers(&mut s), 1, "the marker of the event is on the ground");
    // what the clients are told
    assert_eq!((res(&s, "ev.n"), res(&s, "ev.kind"), res(&s, "ev.on")), (1.0, 7.0, 1.0));
    assert!((res(&s, "ev.x") - w.x as f64).abs() < 0.06 && (res(&s, "ev.z") - w.z as f64).abs() < 0.06);
    assert_eq!(count(&s, "world_event_start"), 1);
    let c = sch(&s);
    assert_eq!((c.n, c.kind, c.alive), (1, 7, 1));
    assert!(c.next[0] >= c.clock + 99.0 && c.next[0] <= c.clock + 151.0, "the next is drawn from 100..150 s: {} from {}", c.next[0], c.clock);
    // it ends at its life (60 s) with its marker
    secs(&mut s, 60.0);
    assert_eq!((drops(&mut s).len(), markers(&mut s)), (0, 0), "removed with its marker");
    assert!(s.world.entity(uid).is_none());
    assert_eq!(count(&s, "world_event_end"), 2, "the event and its marker");
    assert_eq!(res(&s, "ev.on"), 0.0);
    assert_eq!(res(&s, "ev.n"), 1.0, "what started stays counted");
    // the second one starts when the drawn time comes, and not before
    let due = sch(&s).next[0];
    let now = sch(&s).clock;
    secs(&mut s, (due - now - 1.0).max(0.0));
    assert_eq!(sch(&s).n, 1, "not yet at {}", sch(&s).clock);
    secs(&mut s, 3.0);
    assert_eq!(sch(&s).n, 2, "now");
    assert_eq!(drops(&mut s)[0].1.n, 2);
}

#[test]
fn two_events_of_a_line_never_overlap_and_a_full_line_waits_for_the_first_to_end() {
    let mut s = world();
    scheduler(&mut s, "event drop prefab=Drop every=10 first=1 alive=1 height=50 life=100", "");
    let mut most = 0;
    for _ in 0..30 {
        secs(&mut s, 10.0);
        most = most.max(drops(&mut s).len());
    }
    assert_eq!(most, 1, "never two alive");
    assert!(sch(&s).n >= 3, "300 s with a life of 100: the next starts after the first has ended: {}", sch(&s).n);
    // two alive may be allowed
    let mut s = world();
    scheduler(&mut s, "event drop prefab=Drop every=10 first=1 alive=2 height=50 life=100", "");
    let mut most = 0;
    for _ in 0..30 {
        secs(&mut s, 10.0);
        most = most.max(drops(&mut s).len());
    }
    assert_eq!(most, 2, "alive=2");
}

#[test]
fn the_clock_runs_only_while_enough_players_are_in_and_force_starts_a_line_at_once() {
    let mut s = world();
    scheduler(&mut s, TABLE, r#","players":1"#);
    secs(&mut s, 100.0);
    assert_eq!((sch(&s).clock, sch(&s).n), (0.0, 0), "nobody in: the clock waits");
    spawn(&mut s, 6, r#"{"Transform":{},"NetAvatar":{}}"#);
    secs(&mut s, 29.0);
    assert_eq!(sch(&s).n, 0);
    secs(&mut s, 2.0);
    assert_eq!(sch(&s).n, 1, "30 s after the first player came in");
    // force: line 1 at once (here the line is full: it waits; after the first is gone it starts at once)
    let mut s = world();
    scheduler(&mut s, "event drop prefab=Drop every=1000 first=1000 height=50 life=100\nevent other prefab=Flare every=1000 first=1000 kind=9", "");
    secs(&mut s, 1.0);
    assert_eq!(sch(&s).n, 0);
    s.world.get_mut::<WorldEvents>(s.world.entity(5).unwrap()).unwrap().force = 2;
    secs(&mut s, 0.1);
    let c = sch(&s);
    assert_eq!((c.n, c.kind, c.force, c.fired[1], c.fired[0]), (1, 9, 0, 1, 0), "force 2 started the second line and was cleared: {c:?}");
    s.world.get_mut::<WorldEvents>(s.world.entity(5).unwrap()).unwrap().force = 1;
    secs(&mut s, 0.1);
    assert_eq!((sch(&s).n, sch(&s).fired[0]), (2, 1));
    s.world.get_mut::<WorldEvents>(s.world.entity(5).unwrap()).unwrap().force = 1;
    secs(&mut s, 0.1);
    assert_eq!(sch(&s).fired[0], 1, "force respects `alive`");
}

#[test]
fn a_place_is_inside_the_square_clear_of_walls_and_not_on_a_roof_or_a_piece_and_the_same_every_run() {
    let build = || {
        let mut s = world();
        // a hut: four walls and a roof 2.6 m up (a flat top is level ground to a ray), and a foundation on the floor
        spawn(&mut s, 91, r#"{"Transform":{"pos":[0,1.3,-4],"scale":[10,2.6,0.4]},"Collider":{"shape":1}}"#);
        spawn(&mut s, 92, r#"{"Transform":{"pos":[0,1.3,4],"scale":[10,2.6,0.4]},"Collider":{"shape":1}}"#);
        spawn(&mut s, 93, r#"{"Transform":{"pos":[-5,1.3,0],"scale":[0.4,2.6,8]},"Collider":{"shape":1}}"#);
        spawn(&mut s, 94, r#"{"Transform":{"pos":[5,1.3,0],"scale":[0.4,2.6,8]},"Collider":{"shape":1}}"#);
        spawn(&mut s, 95, r#"{"Transform":{"pos":[0,2.7,0],"scale":[10.4,0.2,8.4]},"Collider":{"shape":1}}"#);
        spawn(&mut s, 96, r#"{"Transform":{"pos":[20,0.25,20],"scale":[4,0.5,4]},"Collider":{"shape":1},"BuildPiece":{}}"#);
        scheduler(&mut s, "event drop prefab=Drop every=1 first=1 alive=8 height=10 life=1000 inside=30 clear=3.5 maxy=0.3", "");
        s
    };
    let mut s = build();
    let mut places: Vec<[f32; 3]> = Vec::new();
    for _ in 0..8 {
        secs(&mut s, 5.0);
        places = drops(&mut s).iter().map(|d| [d.1.x, d.1.ground, d.1.z]).collect();
    }
    assert!(places.len() >= 6, "eight may be alive and the places are found: {}", places.len());
    for p in &places {
        assert!(p[0].abs() <= 30.0 && p[2].abs() <= 30.0, "inside the square: {p:?}");
        assert!(p[1].abs() < 0.01, "on the floor, never on the roof (2.8 m) or the foundation (0.5 m): {p:?}");
        let off = ((p[0].abs() - 5.2).max(0.0)).hypot((p[2].abs() - 4.2).max(0.0));
        assert!(off >= 2.0, "{p:?} is in the hut or against its walls ({off} m from them)");
        assert!(!((p[0] - 20.0).abs() <= 2.0 && (p[2] - 20.0).abs() <= 2.0), "{p:?} is on the foundation");
    }
    // the same world gives the same places, the same times and the same hash
    let mut t = build();
    for _ in 0..8 {
        secs(&mut t, 5.0);
    }
    let again: Vec<[f32; 3]> = drops(&mut t).iter().map(|d| [d.1.x, d.1.ground, d.1.z]).collect();
    assert_eq!(places, again, "deterministic");
    assert_eq!(s.state_hash(), t.state_hash());
    // two of them are not in the same spot
    for (i, a) in places.iter().enumerate() {
        for b in &places[i + 1..] {
            assert!(flat(*a, *b) > 0.05, "{a:?} {b:?}");
        }
    }
}

#[test]
fn a_supply_drop_falls_in_a_straight_line_lands_at_its_place_and_changes_its_picture() {
    let mut s = world();
    scheduler(&mut s, "event drop kind=7 prefab=Drop marker=Flare first=1 every=1000 height=100 life=100 inside=20", "");
    secs(&mut s, 1.1);
    let (uid, w, _) = drops(&mut s)[0];
    let m0 = marker_of(&mut s, uid);
    assert_eq!(get::<Noisy>(&s, m0).radius, 80.0, "loud from the start, on the ground at the place");
    let mut ys: Vec<(f32, f32)> = Vec::new();
    let mut seen_enabled_early = false;
    for _ in 0..8 {
        secs(&mut s, 2.0);
        let t = get::<Transform>(&s, uid).pos;
        let a = get::<WorldEvent>(&s, uid);
        ys.push((a.age, t[1]));
        let ne = get::<NetEntity>(&s, uid);
        if a.age < 19.0 {
            seen_enabled_early |= get::<Interactable>(&s, uid).enabled;
            assert_eq!((ne.class, ne.kind, ne.flags), (4, 140, 8 | 4), "falling and loud, a moving class: {ne:?}");
            assert!((t[0] - w.x).abs() <= 2.51 && (t[2] - w.z).abs() <= 2.51, "the sway is within 2.5 m to each side: {t:?} vs {} {}", w.x, w.z);
        }
    }
    assert!(!seen_enabled_early, "it cannot be searched while it falls");
    for (age, y) in &ys {
        if *age < 19.5 {
            let want = 0.5 + descent(100.0, 20.0, *age);
            assert!((y - want).abs() < 0.2, "at {age} s: {y} wanted {want}");
        }
    }
    // down, 20 s on from the start: at the place, a still class with the other picture, searchable
    secs(&mut s, 8.0);
    let t = get::<Transform>(&s, uid).pos;
    assert!((t[0] - w.x).abs() < 0.01 && (t[2] - w.z).abs() < 0.01 && (t[1] - 0.5).abs() < 0.01, "at the announced place: {t:?} vs {} {}", w.x, w.z);
    let ne = get::<NetEntity>(&s, uid);
    assert_eq!((ne.class, ne.kind), (3, 141), "a landed crate is still and has its own picture");
    assert!(get::<Interactable>(&s, uid).enabled);
    assert!(get::<AirDrop>(&s, uid).down);
    assert_eq!(count(&s, "airdrop_landed"), 1);
    // it is loud for 20 s more (radius 80 on the marker on the ground, flag 4), then silent
    let m = marker_of(&mut s, uid);
    assert_eq!((get::<Noisy>(&s, m).radius, ne.flags), (80.0, 4));
    secs(&mut s, 18.0);
    assert_eq!((get::<Noisy>(&s, m).radius, get::<NetEntity>(&s, uid).flags), (0.0, 0), "quiet 20 s after the touchdown");
    assert_eq!(count(&s, "airdrop_landed"), 1, "told once");
}

#[test]
fn the_machines_hear_it_while_it_comes_down_and_for_a_while_after_and_not_from_too_far() {
    let mut s = world();
    scheduler(&mut s, "event drop prefab=Drop marker=Flare first=1 every=1000 height=100 life=300 inside=10 clear=2", "");
    secs(&mut s, 1.1);
    let (_, w, _) = drops(&mut s)[0];
    let machine = |s: &mut Sim, uid: u64, dx: f32| {
        spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{},0,{}]}},"Team":{{"id":2}},"HitPoints":{{"value":50,"max":50}},"Perception":{{"sight_range":3,"hearing":1,"engage":false,"lose_time":1,"notice_time":1}}}}"#, w.x + dx, w.z));
    };
    machine(&mut s, 70, 60.0);
    machine(&mut s, 71, -100.0);
    machine(&mut s, 72, 30.0);
    secs(&mut s, 3.0);
    let p = |s: &Sim, uid: u64| get::<Perception>(s, uid);
    assert!(p(&s, 70).heard && p(&s, 70).state == 3, "60 m away: it hears the parachute and goes to look: {:?}", p(&s, 70).state);
    assert!(flat(p(&s, 70).seen_pos, [w.x, 0.0, w.z]) < 3.0 && flat(p(&s, 70).noise_pos, [w.x, 0.0, w.z]) < 3.0, "to where it is: {:?} {:?}", p(&s, 70).seen_pos, p(&s, 70).noise_pos);
    assert!(!p(&s, 71).heard && p(&s, 71).state == 0, "100 m away: beyond 80 m");
    // still loud after it has landed, then not: the machine forgets
    secs(&mut s, 30.0);
    assert!(p(&s, 70).heard, "20 s after the touchdown it is still loud");
    secs(&mut s, 12.0);
    assert!(!p(&s, 70).heard && p(&s, 72).state != 3, "silent after 20 s past the touchdown; the machine stops hearing and loses interest ({})", p(&s, 72).state);
}

#[test]
fn the_heat_is_added_within_range_only_while_it_is_loud_and_never_when_it_is_zero() {
    let table = "event drop prefab=Drop marker=Flare first=1 every=1000 height=100 life=300 inside=5 clear=2";
    let mut s = world();
    s.apply(&Command::DefinePrefab { name: "Drop".into(), def: parse(&DROP.replace(r#""claim_for":8"#, r#""claim_for":8,"heat":0.1,"heat_range":60"#)).unwrap() }).unwrap();
    scheduler(&mut s, table, "");
    secs(&mut s, 1.1);
    let (_, w, _) = drops(&mut s)[0];
    let pl = |s: &mut Sim, uid: u64, dx: f32| spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{},0,{}]}},"Heat":{{"decay":0}}}}"#, w.x + dx, w.z));
    pl(&mut s, 70, 10.0);
    pl(&mut s, 71, 40.0);
    pl(&mut s, 72, 100.0);
    secs(&mut s, 41.0);
    let h = |s: &Sim, uid: u64| get::<Heat>(s, uid).value;
    // 0.1 a second for the 40 s of noise (20 s fall + 20 s after), times (1 - d / 60): 10 m is 5/6 of it, 40 m a third
    assert!((h(&s, 70) - 4.0 * (1.0 - 10.0 / 60.0)).abs() < 0.2, "{}", h(&s, 70));
    assert!((h(&s, 71) - 4.0 * (1.0 - 40.0 / 60.0)).abs() < 0.2, "{}", h(&s, 71));
    assert_eq!(h(&s, 72), 0.0, "out of range");
    let after = h(&s, 70);
    secs(&mut s, 20.0);
    assert_eq!(h(&s, 70), after, "silent: no more heat");
    // heat 0: none at all
    let mut s = world();
    scheduler(&mut s, table, "");
    secs(&mut s, 1.1);
    pl(&mut s, 70, 10.0);
    secs(&mut s, 30.0);
    assert_eq!(h(&s, 70), 0.0);
}

#[test]
fn opening_it_claims_it_once_and_a_crate_that_is_empty_and_shut_goes_away_with_its_marker() {
    let mut s = world();
    scheduler(&mut s, "event drop prefab=Drop marker=Flare first=1 every=1000 height=10 life=500 inside=5 clear=2", "");
    secs(&mut s, 1.1);
    let (uid, _, _) = drops(&mut s)[0];
    // (no `LootContainer` here: a bag with one thing in it)
    {
        let e = s.world.entity(uid).unwrap();
        let inv = s.world.get_mut::<crate::inventory::Inventory>(e).unwrap();
        inv.id[0] = 3;
        inv.count[0] = 2;
    }
    secs(&mut s, 25.0);
    assert!(get::<AirDrop>(&s, uid).down && !get::<AirDrop>(&s, uid).claimed);
    // a player opens it (the engine's `Interactable.open` and `user`)
    let open = |s: &mut Sim, on: bool, user: u64| {
        let e = s.world.entity(uid).unwrap();
        let i = s.world.get_mut::<Interactable>(e).unwrap();
        i.open = on;
        i.user = lunaris_ecs::Uid(user);
    };
    open(&mut s, true, 77);
    secs(&mut s, 0.2);
    let a = get::<AirDrop>(&s, uid);
    assert!(a.claimed && a.claimed_by.0 == 77 && a.opened == 1, "{a:?}");
    assert_eq!(count(&s, "airdrop_claimed"), 1);
    secs(&mut s, 30.0);
    assert!(get::<AirDrop>(&s, uid).claimed, "claimed while it is open");
    assert_eq!(count(&s, "airdrop_claimed"), 1, "told once");
    // shut again: claimed for 8 s more, then free
    open(&mut s, false, 0);
    secs(&mut s, 5.0);
    assert!(get::<AirDrop>(&s, uid).claimed);
    secs(&mut s, 4.0);
    assert!(!get::<AirDrop>(&s, uid).claimed);
    // not empty: it stays (60 s is more than the 30 s an empty one is kept)
    secs(&mut s, 60.0);
    assert!(s.world.entity(uid).is_some());
    // empty it: 30 s after, it and its marker are gone
    {
        let e = s.world.entity(uid).unwrap();
        let inv = s.world.get_mut::<crate::inventory::Inventory>(e).unwrap();
        inv.id[0] = 0;
        inv.count[0] = 0;
    }
    secs(&mut s, 28.0);
    assert!(s.world.entity(uid).is_some(), "28 s empty: still there");
    assert_eq!(markers(&mut s), 1);
    secs(&mut s, 4.0);
    assert!(s.world.entity(uid).is_none(), "gone");
    secs(&mut s, 0.2);
    assert_eq!(markers(&mut s), 0, "the marker follows its event");
    assert_eq!(res(&s, "ev.on"), 0.0);
    // a crate somebody has open is not taken away from under them
    let mut s = world();
    scheduler(&mut s, "event drop prefab=Drop first=1 every=1000 height=10 life=500 inside=5 clear=2", "");
    secs(&mut s, 1.1);
    let (uid2, _, _) = drops(&mut s)[0];
    secs(&mut s, 25.0);
    {
        let e = s.world.entity(uid2).unwrap();
        s.world.get_mut::<Interactable>(e).unwrap().open = true;
    }
    secs(&mut s, 80.0);
    assert!(s.world.entity(uid2).is_some(), "empty but open: it stays");
}

#[test]
fn a_near_line_is_placed_round_a_player_in_a_world_that_is_loaded_only_there() {
    // a streamed city: the ground, the buildings and the sky are there only round the players. Here a ground plate 800 m from the origin and a player on it; nothing at the origin
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "Drop".into(), def: parse(DROP).unwrap() }).unwrap();
    s.apply(&Command::DefinePrefab { name: "Flare".into(), def: parse(FLARE).unwrap() }).unwrap();
    spawn(&mut s, 90, r#"{"Transform":{"pos":[800,-0.5,0],"scale":[300,1,300]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 70, r#"{"Transform":{"pos":[800,0,0]},"NetAvatar":{}}"#);
    // (a wall in the way of some of the places: they are tried again, the drop is clear of it)
    spawn(&mut s, 91, r#"{"Transform":{"pos":[830,2,0]},"Collider":{"shape":1,"size":[4,4,40]}}"#);
    scheduler(&mut s, "event drop prefab=Drop marker=Flare first=1 every=1000 height=100 life=600 near=30..60 clear=2 maxy=0.3", r#","players":1"#);
    secs(&mut s, 2.0);
    let d = drops(&mut s);
    assert_eq!(d.len(), 1, "a place was found near the player, where the world is");
    let at = d[0].2;
    let dist = flat(at, [800.0, 0.0, 0.0]);
    assert!((30.0..=60.5).contains(&dist), "between 30 and 60 m from the player: {dist:.1} at {at:?}");
    assert!(flat(at, [830.0, 0.0, 0.0]) > 4.0, "clear of the wall");
    // the same line with the square `inside` finds nothing at the origin of this world: nothing is loaded there
    let mut t = Sim::new(1);
    t.apply(&Command::DefinePrefab { name: "Drop".into(), def: parse(DROP).unwrap() }).unwrap();
    t.apply(&Command::DefinePrefab { name: "Flare".into(), def: parse(FLARE).unwrap() }).unwrap();
    spawn(&mut t, 90, r#"{"Transform":{"pos":[800,-0.5,0],"scale":[300,1,300]},"Collider":{"shape":1}}"#);
    spawn(&mut t, 70, r#"{"Transform":{"pos":[800,0,0]},"NetAvatar":{}}"#);
    scheduler(&mut t, "event drop prefab=Drop marker=Flare first=1 every=1000 height=100 life=600 inside=30 clear=2 maxy=0.3", r#","players":1"#);
    secs(&mut t, 12.0);
    assert!(drops(&mut t).is_empty(), "the square round the origin has no ground in it: it never starts (what the city's supply drop did)");
    // no player, no near: the line waits (the clock runs only with a player in, and a near line has nowhere to be placed without one)
    let mut u = Sim::new(1);
    u.apply(&Command::DefinePrefab { name: "Drop".into(), def: parse(DROP).unwrap() }).unwrap();
    spawn(&mut u, 90, r#"{"Transform":{"pos":[800,-0.5,0],"scale":[300,1,300]},"Collider":{"shape":1}}"#);
    scheduler(&mut u, "event drop prefab=Drop first=1 every=1000 height=100 life=600 near=30..60 clear=2 maxy=0.3", "");
    secs(&mut u, 12.0);
    assert!(drops(&mut u).is_empty(), "with no avatar a near line has no one to be near: nothing falls on the origin's square, nothing starts");
    // the table says what is wrong with a near
    let (lines, problems) = parse_table("event a prefab=P near=10..40
event b prefab=P near=40..10
event c prefab=P near=5
event d prefab=P near=0..0");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].near, (10.0, 40.0));
    assert_eq!(problems.len(), 3, "{problems:#?}");
}

#[test]
fn a_restart_in_the_middle_of_a_drop_goes_on_where_it_was() {
    let mut s = world();
    scheduler(&mut s, TABLE, "");
    secs(&mut s, 31.0);
    secs(&mut s, 8.0);
    let (uid, w, pos) = drops(&mut s)[0];
    assert!(pos[1] > 40.0 && pos[1] < 80.0, "about 8 s into a 20 s fall: {pos:?}");
    // save, and a new server loads it
    let save = s.save_json();
    let mut t = Sim::new(1);
    t.load_save(&save).unwrap();
    assert_eq!(s.state_hash(), t.state_hash(), "the loaded world is the saved one");
    for sim in [&mut s, &mut t] {
        secs(sim, 20.0);
    }
    assert_eq!(s.state_hash(), t.state_hash(), "and goes on the same way");
    let a = get::<WorldEvent>(&t, uid);
    assert_eq!((a.n, a.age > 27.0), (w.n, true));
    let ne = get::<NetEntity>(&t, uid);
    assert_eq!((ne.class, ne.kind), (3, 141), "it landed after the restart");
    assert_eq!((res(&t, "ev.n"), res(&t, "ev.on")), (1.0, 1.0), "the clients are told again");
    assert!((res(&t, "ev.x") - w.x as f64).abs() < 0.06);
    // the clock went on from the saved one: the next start is the drawn time, not a new `first`
    assert_eq!(sch(&t).n, 1);
    assert_eq!(sch(&t).next[0], sch(&s).next[0]);
    secs(&mut t, 60.0);
    assert_eq!(drops(&mut t).len(), 0, "and ends at its life");
}

#[test]
fn a_connected_client_starts_and_ages_nothing() {
    let mut s = world();
    scheduler(&mut s, "event drop prefab=Drop first=1 every=10 height=10 life=100", "");
    spawn(&mut s, 7, r#"{"NetPlayer":{"connected":true}}"#);
    secs(&mut s, 30.0);
    assert_eq!((sch(&s).n, sch(&s).clock, drops(&mut s).len()), (0, 0.0, 0), "the events are the server's");
    // what a server told it stands: nothing writes the resources
    let _ = s.world.set_resource("ev.n", Some(5.0));
    secs(&mut s, 1.0);
    assert_eq!(res(&s, "ev.n"), 5.0);
}

#[test]
fn a_line_with_a_prefab_that_does_not_exist_waits_and_tries_again_without_harm() {
    let mut s = world();
    scheduler(&mut s, "event ghost prefab=Nothing first=1 every=5", "");
    secs(&mut s, 40.0);
    assert_eq!((sch(&s).n, drops(&mut s).len()), (0, 0));
    s.apply(&Command::DefinePrefab { name: "Nothing".into(), def: parse(FLARE).unwrap() }).unwrap();
    secs(&mut s, 12.0);
    assert_eq!(sch(&s).n, 1, "it starts once the prefab is there");
}

#[test]
fn the_admin_command_lists_the_table_and_starts_an_event_by_name() {
    let mut s = world();
    assert!(crate::events::admin(&mut s.world, "").contains("no world events"), "a world with no scheduler says so");
    scheduler(&mut s, "event drop kind=7 prefab=Drop every=500 first=200 height=50 life=100
event other prefab=Flare every=300 first=100", "");
    secs(&mut s, 1.0);
    let list = crate::events::admin(&mut s.world, "");
    assert!(list.contains("drop (kind 7): next in 199 s, 0 going, 0 started") && list.contains("other (kind 2): next in 99 s"), "{list}");
    assert!(crate::events::admin(&mut s.world, "nothing").contains("no event called nothing (they are: drop, other)"));
    assert!(crate::events::admin(&mut s.world, "drop").contains("starting drop at once"));
    secs(&mut s, 0.1);
    let list = crate::events::admin(&mut s.world, "");
    assert!(list.contains("drop (kind 7): next in 500 s, 1 going, 1 started"), "{list}");
    assert_eq!((sch(&s).n, drops(&mut s).len()), (1, 1));
}

#[test]
fn an_event_that_was_asked_for_and_refused_leaves_the_drawn_time_alone() {
    let mut s = world();
    scheduler(&mut s, "event drop prefab=Drop every=900 first=10 height=50 life=100", "");
    secs(&mut s, 11.0);
    assert_eq!(sch(&s).n, 1);
    let next = sch(&s).next[0];
    assert!(next > 900.0, "{next}");
    s.world.get_mut::<WorldEvents>(s.world.entity(5).unwrap()).unwrap().force = 1;
    secs(&mut s, 1.0);
    assert_eq!((sch(&s).n, sch(&s).next[0]), (1, next), "refused (one is going) and the time of the next stays what was drawn");
}
