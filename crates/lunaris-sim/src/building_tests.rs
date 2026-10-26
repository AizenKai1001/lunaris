//! Building: the rules text, where a tool puts a piece, snapping, stability, collapse, decay, cupboards, anchors on generated buildings. No art: boxes and numbers.
use super::*;
use crate::building::{parse_rules, yaw_of};
use crate::components::{BuildPiece, BuildTool, BuildUpkeep, HitPoints, Transform};
use lunaris_json::{parse, Json};

/// A foundation that stands on the ground and takes more on its four edges, a wall that stands on an edge, a ceiling that rests on wall tops, and a table.
const RULES: &str = "# a small set of pieces
rules snap=1.5 min=0.2 gap=0.05 slope=0.7 step=90 ground=0.6 audit=1
piece foundation size=4,0.5,4 hp=500 free=1 anchor=1 decay=1 prefab= debris=
  socket n at=0,0.25,-2 yaw=0   type=edge mates=edge,wall_base carry=0.8,wall_base:0
  socket s at=0,0.25,2  yaw=180 type=edge mates=edge,wall_base carry=0.8,wall_base:0
  socket e at=2,0.25,0  yaw=-90 type=edge mates=edge,wall_base carry=0.8,wall_base:0
  socket w at=-2,0.25,0 yaw=90  type=edge mates=edge,wall_base carry=0.8,wall_base:0
piece wall size=4,3,0.2 hp=200 inset=0.12 decay=1
  socket base at=0,-1.5,0 yaw=0 type=wall_base mates=edge carry=0.9 turn=0
  socket top  at=0,1.5,0  yaw=0 type=wall_top
piece ceiling size=4,0.2,4 hp=200
  socket a at=0,-0.1,-2 yaw=0   type=ceil mates=wall_top carry=0.9
  socket b at=0,-0.1,2  yaw=180 type=ceil mates=wall_top carry=0.9
piece table size=1,0.8,1 hp=50 free=1 loose=1
";

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn world() -> Sim {
    let mut s = Sim::new(1);
    let text = RULES.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"BuildRules":{{"text":"{text}"}}}}"#));
    // the ground: a big flat box, top at y = 0 (80 m square)
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[80,1,80]}}"#);
    // the player's tool (owner 7), switched off until it is given a kind
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,5,0]},"BuildTool":{"reach":12,"owner":7}}"#);
    s
}

/// The tool (entity 10) at `from` looking at `at`, with a kind.
fn tool(s: &mut Sim, uid: u64, from: [f32; 3], at: [f32; 3], k: &str) {
    aim(s, uid, from, at);
    kind(s, uid, k);
}

fn aim(s: &mut Sim, uid: u64, from: [f32; 3], at: [f32; 3]) {
    let d = [at[0] - from[0], at[1] - from[1], at[2] - from[2]];
    let q = crate::perception::look_rot(d, true);
    let e = s.world.entity(uid).unwrap();
    let t = s.world.get_mut::<Transform>(e).unwrap();
    t.pos = from;
    t.rot = q;
}

fn kind(s: &mut Sim, uid: u64, k: &str) {
    let sym = s.world.strings_mut().intern(k);
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<BuildTool>(e).unwrap().kind = lunaris_ecs::Sym(sym);
}

fn why(s: &mut Sim, uid: u64) -> String {
    let e = s.world.entity(uid).unwrap();
    let t = *s.world.get::<BuildTool>(e).unwrap();
    s.world.strings().get(t.why.0).to_string()
}

fn tl(s: &mut Sim, uid: u64) -> BuildTool {
    let e = s.world.entity(uid).unwrap();
    *s.world.get::<BuildTool>(e).unwrap()
}

fn press(s: &mut Sim, uid: u64) {
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<BuildTool>(e).unwrap().place = true;
    s.step();
}

fn pieces(s: &mut Sim) -> Vec<(u64, String, [f32; 3], f32, f32)> {
    let mut out: Vec<(u64, u32, [f32; 3], f32, f32)> = Vec::new();
    let _ = s.world.for_each_uid::<(&BuildPiece, &Transform), _>(|_, uid, (p, t)| out.push((uid, p.kind.0, t.pos, yaw_of(t.rot), p.stability)));
    out.sort_by_key(|p| p.0);
    out.into_iter().map(|(u, k, p, y, st)| (u, s.world.strings().get(k).to_string(), p, y, st)).collect()
}

fn events(s: &Sim, name: &str) -> Vec<u64> {
    s.events.iter().filter(|e| e.name == name).map(|e| e.uid).collect()
}

/// Put the tool above `x, z` looking straight down at the ground and build a free piece there.
fn put_free(s: &mut Sim, uid: u64, k: &str, x: f32, z: f32) -> bool {
    aim(s, uid, [x, 5.0, z - 0.01], [x, 0.0, z]);
    kind(s, uid, k);
    s.step();
    let ok = tl(s, uid).valid;
    if ok {
        press(s, uid);
    }
    ok
}

#[test]
fn the_rules_are_read_and_what_is_wrong_with_them_is_said() {
    let r = parse_rules(RULES, Some(&["x".to_string()]));
    // (the empty prefab= and debris= are not names)
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.kinds.len(), 4);
    let f = &r.kinds[0];
    assert_eq!((f.size, f.hp, f.free, f.anchor, f.sockets.len()), ([4.0, 0.5, 4.0], 500.0, true, true, 4));
    assert_eq!(f.sockets[0].carry_for("wall_base"), 0.0);
    assert_eq!(f.sockets[0].carry_for("edge"), 0.8);
    assert_eq!(r.kinds[1].sockets[0].turn, 0.0);
    assert_eq!(r.kinds[1].inset, Some(0.12));
    assert!(r.kinds[3].loose);
    let bad = parse_rules("stray\nsocket x type=t\npiece p size=1,1\npiece q size=2,2,2 hp=x\n  socket a at=1,2 type=\n  socket b at=0,0,0 type=t mates=u carry=2\npiece p2 size=1,1,1 prefab=nope\nrules wobble=1 snap=far\n", Some(&[]));
    let text = bad.problems.join("\n");
    for needle in ["`stray`", "belongs under a `piece`", "size=1,1", "hp=x", "at=1,2", "no type", "carry `2`", "can never be placed", "wobble", "snap=far", "prefab=nope"] {
        assert!(text.contains(needle), "{needle} is not said in:\n{text}");
    }
}

#[test]
fn a_free_piece_goes_where_the_tool_looks_and_stands_on_the_ground() {
    let mut s = world();
    tool(&mut s, 10, [0.0, 5.0, 0.0], [3.0, 0.0, 0.0], "foundation");
    s.step();
    let t = tl(&mut s, 10);
    assert!(t.valid, "{}", why(&mut s, 10));
    assert!((t.pos[0] - 3.0).abs() < 0.05 && (t.pos[1] - 0.25).abs() < 0.01 && t.pos[2].abs() < 0.05, "{:?}", t.pos);
    assert_eq!(t.stability, 1.0);
    assert!(pieces(&mut s).is_empty(), "looking builds nothing");
    press(&mut s, 10);
    let p = pieces(&mut s);
    assert_eq!(p.len(), 1);
    assert_eq!((p[0].1.as_str(), p[0].4), ("foundation", 1.0));
    let t = tl(&mut s, 10);
    assert_eq!((t.placed.0, t.built, t.place), (p[0].0, 1, false), "the new piece's uid, the count, the trigger cleared");
    assert_eq!(events(&s, "build_placed"), vec![p[0].0]);
    let e = s.world.entity(p[0].0).unwrap();
    let hp = *s.world.get::<HitPoints>(e).unwrap();
    assert_eq!((hp.value, hp.max), (500.0, 500.0));
    let bp = *s.world.get::<BuildPiece>(e).unwrap();
    assert_eq!((bp.owner, bp.anchored, bp.checked), (7, true, true));
}

#[test]
fn a_piece_cannot_go_through_another_and_the_tool_says_why() {
    let mut s = world();
    assert!(put_free(&mut s, 10, "foundation", 0.0, 0.0));
    // looking at the top of the foundation, away from its edges: a foundation does not stand on another piece
    aim(&mut s, 10, [0.0, 5.0, 0.01], [0.0, 0.0, 0.0]);
    s.step();
    assert!(!tl(&mut s, 10).valid);
    assert_eq!(why(&mut s, 10), "no_ground");
    // a foundation joined to its east edge: fine, once; the same edge again is taken
    aim(&mut s, 10, [2.4, 5.0, 0.3], [2.4, 0.0, 0.3]);
    s.step();
    assert!(tl(&mut s, 10).valid, "{}", why(&mut s, 10));
    press(&mut s, 10);
    assert_eq!(pieces(&mut s).len(), 2);
    s.step();
    assert!(!tl(&mut s, 10).valid);
    assert_eq!(why(&mut s, 10), "blocked");
    press(&mut s, 10);
    assert_eq!(pieces(&mut s).len(), 2, "a refused place builds nothing");
    assert_eq!(events(&s, "build_refused"), vec![10]);
}

#[test]
fn a_foundation_snaps_to_the_edge_of_another_and_a_wall_to_its_top_edge() {
    let mut s = world();
    assert!(put_free(&mut s, 10, "foundation", 0.0, 0.0));
    let base = pieces(&mut s)[0].0;
    // aim near the east edge, a little out on the ground: the new foundation joins the east edge
    aim(&mut s, 10, [3.2, 5.0, 0.3], [2.6, 0.0, 0.3]);
    s.step();
    let t = tl(&mut s, 10);
    assert!(t.valid, "{}", why(&mut s, 10));
    assert_eq!(t.snapped.0, base);
    assert!((t.pos[0] - 4.0).abs() < 0.02 && t.pos[2].abs() < 0.02 && (t.pos[1] - 0.0).abs() < 0.3, "{:?}", t.pos);
    press(&mut s, 10);
    assert_eq!(pieces(&mut s).len(), 2);
    // a wall on the north edge: the tool looks at the top of the edge from inside
    kind(&mut s, 10, "wall");
    aim(&mut s, 10, [0.0, 3.0, -0.5], [0.0, 0.25, -1.8]);
    s.step();
    let t = tl(&mut s, 10);
    assert!(t.valid, "{}", why(&mut s, 10));
    assert_eq!(t.snapped.0, base);
    // the wall stands on the edge: its middle 1.5 above the foundation's top (0.5), at the edge
    assert!((t.pos[0]).abs() < 0.02 && (t.pos[1] - 2.0).abs() < 0.02 && (t.pos[2] + 2.0).abs() < 0.02, "{:?}", t.pos);
    // wall_base mates edge with carry 0.9 and the foundation's own carry for wall_base is 0: the wall takes 0.9 of the foundation's 1
    assert!((t.stability - 0.9).abs() < 1.0e-5, "{}", t.stability);
    let yaw_normal = t.yaw;
    // flipped, the wall faces the other way
    let e = s.world.entity(10).unwrap();
    s.world.get_mut::<BuildTool>(e).unwrap().flip = true;
    s.step();
    let flipped = tl(&mut s, 10).yaw;
    assert!(((flipped - yaw_normal).abs() - 180.0).abs() < 0.01, "{yaw_normal} {flipped}");
    press(&mut s, 10);
    assert_eq!(pieces(&mut s).len(), 3);
    // the same place twice is blocked
    s.step();
    assert!(!tl(&mut s, 10).valid);
    assert_eq!(why(&mut s, 10), "blocked");
}

#[test]
fn a_piece_that_needs_a_socket_and_finds_none_says_so() {
    let mut s = world();
    tool(&mut s, 10, [0.0, 5.0, 0.0], [3.0, 0.0, 0.0], "wall");
    s.step();
    assert!(!tl(&mut s, 10).valid);
    assert_eq!(why(&mut s, 10), "no_socket");
    kind(&mut s, 10, "nonsense");
    s.step();
    assert_eq!(why(&mut s, 10), "unknown_piece");
    // looking at the sky meets nothing
    kind(&mut s, 10, "foundation");
    aim(&mut s, 10, [0.0, 5.0, 0.0], [0.0, 50.0, 0.0]);
    s.step();
    assert_eq!(why(&mut s, 10), "no_aim");
    // a tool with no kind is off and says nothing
    kind(&mut s, 10, "");
    s.step();
    assert_eq!((tl(&mut s, 10).valid, why(&mut s, 10)), (false, String::new()));
}

#[test]
fn a_tower_hangs_on_thinner_threads_and_a_piece_too_far_up_is_unstable() {
    let mut s = world();
    assert!(put_free(&mut s, 10, "foundation", 0.0, 0.0));
    let found = pieces(&mut s)[0].0;
    // wall on the north edge, ceiling on its top, wall on the ceiling's far side ... stability falls by 0.9 each step
    kind(&mut s, 10, "wall");
    aim(&mut s, 10, [0.0, 3.0, -0.5], [0.0, 0.25, -1.8]);
    s.step();
    assert!(tl(&mut s, 10).valid);
    press(&mut s, 10);
    let wall = pieces(&mut s).iter().find(|p| p.1 == "wall").unwrap().0;
    assert_eq!(tl(&mut s, 10).snapped.0, found);
    // the ceiling joins the wall's top: aim at the top of the wall
    kind(&mut s, 10, "ceiling");
    aim(&mut s, 10, [0.0, 6.0, 1.5], [0.0, 3.25, -2.0]);
    s.step();
    let t = tl(&mut s, 10);
    assert!(t.valid, "{}", why(&mut s, 10));
    assert_eq!(t.snapped.0, wall);
    assert!((t.stability - 0.81).abs() < 1.0e-5, "0.9 of the wall's 0.9: {}", t.stability);
    press(&mut s, 10);
    let ceil = pieces(&mut s).into_iter().find(|p| p.1 == "ceiling").unwrap();
    assert!((ceil.4 - 0.81).abs() < 1.0e-5);
}

#[test]
fn what_hangs_on_the_ground_falls_when_the_ground_piece_is_destroyed() {
    let mut s = world();
    assert!(put_free(&mut s, 10, "foundation", 0.0, 0.0));
    let found = pieces(&mut s)[0].0;
    kind(&mut s, 10, "wall");
    aim(&mut s, 10, [0.0, 3.0, -0.5], [0.0, 0.25, -1.8]);
    s.step();
    press(&mut s, 10);
    kind(&mut s, 10, "ceiling");
    aim(&mut s, 10, [0.0, 6.0, 1.5], [0.0, 3.25, -2.0]);
    s.step();
    press(&mut s, 10);
    kind(&mut s, 10, "");
    assert_eq!(pieces(&mut s).len(), 3);
    // shoot the foundation to pieces (its hit points go to 0)
    let e = s.world.entity(found).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
    s.step_n(2);
    assert!(pieces(&mut s).is_empty(), "the wall and the ceiling had nothing left to hold them: {:?}", pieces(&mut s));
    assert_eq!(events(&s, "build_destroyed"), vec![found]);
    let fell = events(&s, "build_collapse");
    assert_eq!(fell.len(), 2, "{fell:?}");
}

#[test]
fn a_second_foundation_holds_up_what_the_first_one_would_leave() {
    let mut s = world();
    assert!(put_free(&mut s, 10, "foundation", -6.0, 0.0));
    let a = pieces(&mut s)[0].0;
    assert!(put_free(&mut s, 10, "foundation", 6.0, 0.0));
    let b = pieces(&mut s).iter().find(|p| p.0 != a).unwrap().0;
    // a wall on the east edge of `a`, and a ceiling on it that reaches towards `b`: both hold through `a` only
    kind(&mut s, 10, "wall");
    aim(&mut s, 10, [-5.0, 3.0, -0.5], [-4.0, 0.25, -0.2]);
    s.step();
    let t = tl(&mut s, 10);
    assert!(t.valid, "{}", why(&mut s, 10));
    press(&mut s, 10);
    let wall = pieces(&mut s).iter().find(|p| p.1 == "wall").unwrap().0;
    // destroy `b`: nothing changes for the wall
    let e = s.world.entity(b).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
    kind(&mut s, 10, "");
    s.step_n(2);
    assert_eq!(events(&s, "build_destroyed"), vec![b]);
    assert!(pieces(&mut s).iter().any(|p| p.0 == wall));
    assert!(pieces(&mut s).iter().any(|p| p.0 == a));
}

#[test]
fn a_foundation_chain_over_open_air_loses_a_fifth_each_step_and_stops_where_it_cannot_stand() {
    let mut s = world();
    // the ground is 80 m square: a foundation at x = 36 still stands on it, the next ones are over nothing
    assert!(put_free(&mut s, 10, "foundation", 36.0, 0.0));
    let mut stab = vec![1.0f32];
    let mut x = 36.0;
    loop {
        // look at the top of the last one, near its east edge
        aim(&mut s, 10, [x + 1.5, 5.0, 0.3], [x + 1.5, 0.5, 0.3]);
        s.step();
        let t = tl(&mut s, 10);
        if !t.valid {
            assert_eq!(why(&mut s, 10), "unstable", "{:?}", t.pos);
            break;
        }
        stab.push(t.stability);
        press(&mut s, 10);
        x += 4.0;
        assert!(stab.len() < 20, "it must stop");
    }
    // 0.8^k stays above the minimum 0.2 up to k = 7 (0.2097) and the eighth (0.1678) is refused: the chain over the edge of the ground is 7 long, the first of them may rest on the ground still
    // the second still rests on the ground (its middle is on the edge of it), then seven over nothing: 0.8 to the power of 1 ... 7; the eighth would be 0.168, under 0.2
    assert_eq!(stab.len(), 9, "{stab:?}");
    for (k, s) in stab.iter().enumerate().skip(2) {
        assert!((s - 0.8f32.powi(k as i32 - 1)).abs() < 1.0e-4, "{k}: {s} in {stab:?}");
    }
    for w in stab.windows(2).skip(1) {
        assert!(w[1] <= w[0] + 1.0e-6, "it never gets better going out: {stab:?}");
    }
}

#[test]
fn a_cupboard_keeps_other_owners_out_and_pays_for_decay() {
    let mut s = world();
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,0,0]},"BuildUpkeep":{"owner":3,"radius":10,"protect":0}}"#);
    // owner 7 may not build inside owner 3's land
    assert!(!put_free(&mut s, 10, "foundation", 3.0, 3.0));
    assert_eq!(why(&mut s, 10), "not_yours");
    // the owner may
    let e = s.world.entity(10).unwrap();
    s.world.get_mut::<BuildTool>(e).unwrap().owner = 3;
    assert!(put_free(&mut s, 10, "foundation", 3.0, 3.0));
    // not exclusive: anyone
    let c = s.world.entity(5).unwrap();
    s.world.get_mut::<BuildUpkeep>(c).unwrap().exclusive = false;
    s.world.get_mut::<BuildTool>(e).unwrap().owner = 7;
    assert!(put_free(&mut s, 10, "foundation", -3.0, 3.0));
    // decay: without upkeep the foundations (decay 1 hp a second) lose hit points; with it they do not
    kind(&mut s, 10, "");
    s.step_n(90);
    let hp = |s: &mut Sim, uid: u64| s.world.get::<HitPoints>(s.world.entity(uid).unwrap()).unwrap().value;
    let ids: Vec<u64> = pieces(&mut s).iter().map(|p| p.0).collect();
    assert_eq!(ids.len(), 2);
    let cover = s.world.get::<BuildUpkeep>(s.world.entity(5).unwrap()).unwrap().covered;
    assert_eq!(cover, 2, "the cupboard covers both");
    let before: Vec<f32> = ids.iter().map(|u| hp(&mut s, *u)).collect();
    assert!(before.iter().all(|h| *h < 500.0 && *h > 480.0), "three seconds of decay at 1 a second: {before:?}");
    let c = s.world.entity(5).unwrap();
    s.world.get_mut::<BuildUpkeep>(c).unwrap().protect = 100.0;
    s.step_n(60);
    let after: Vec<f32> = ids.iter().map(|u| hp(&mut s, *u)).collect();
    assert_eq!(before, after, "protected: no decay");
    let left = s.world.get::<BuildUpkeep>(s.world.entity(5).unwrap()).unwrap().protect;
    assert!(left < 100.0 && left > 95.0, "{left}: it runs down a second each second");
}

#[test]
fn a_piece_that_decays_to_nothing_is_destroyed_and_takes_what_it_held() {
    let mut s = world();
    assert!(put_free(&mut s, 10, "foundation", 0.0, 0.0));
    kind(&mut s, 10, "wall");
    aim(&mut s, 10, [0.0, 3.0, -0.5], [0.0, 0.25, -1.8]);
    s.step();
    press(&mut s, 10);
    kind(&mut s, 10, "");
    let found = pieces(&mut s).iter().find(|p| p.1 == "foundation").unwrap().0;
    let e = s.world.entity(found).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 1.5;
    // two audits later (a second apart: 60 ticks a second) the foundation has decayed away and the wall with it
    s.step_n(200);
    assert!(pieces(&mut s).is_empty(), "{:?}", pieces(&mut s));
    assert!(events(&s, "build_destroyed").contains(&found));
}

#[test]
fn furniture_stands_on_a_floor_and_falls_with_it() {
    let mut s = world();
    assert!(put_free(&mut s, 10, "foundation", 0.0, 0.0));
    let found = pieces(&mut s)[0].0;
    kind(&mut s, 10, "table");
    aim(&mut s, 10, [0.0, 5.0, 0.01], [0.0, 0.25, 0.0]);
    s.step();
    let t = tl(&mut s, 10);
    assert!(t.valid, "{}", why(&mut s, 10));
    assert!((t.pos[1] - 0.9).abs() < 0.01, "on the top of the foundation (0.5) and half its own height (0.4): {:?}", t.pos);
    press(&mut s, 10);
    let table = pieces(&mut s).iter().find(|p| p.1 == "table").unwrap().0;
    let on = s.world.get::<BuildPiece>(s.world.entity(table).unwrap()).unwrap().on.0;
    assert_eq!(on, found);
    kind(&mut s, 10, "");
    let e = s.world.entity(found).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
    s.step_n(2);
    assert!(pieces(&mut s).is_empty());
    assert_eq!(events(&s, "build_fell"), vec![table]);
}

#[test]
fn a_socket_on_a_generated_building_holds_a_piece_up_with_no_ground() {
    let mut s = world();
    // the facade of a generated building: a wall 30 m high whose front is the plane z = -20 ...
    spawn(&mut s, 31, r#"{"Transform":{"pos":[0,10,-20.5]},"Collider":{"shape":1,"size":[40,40,1]}}"#);
    // ... and a point on it 10 m up that faces out into the street (turned half a circle: its forward, -z, points into the building), of type `edge` which the foundation's sockets plug into
    spawn(&mut s, 30, r#"{"Transform":{"pos":[0,10,-20],"rot":[0,1,0,0]},"BuildSocket":{"kind":"edge"}}"#);
    // aim at the point: the foundation hangs from it out over the street, held by the facade (carry 0.8)
    tool(&mut s, 10, [0.0, 14.0, -14.0], [0.0, 10.0, -20.0], "foundation");
    s.step();
    let t = tl(&mut s, 10);
    assert!(t.valid, "{} at {:?}", why(&mut s, 10), t.pos);
    assert_eq!(t.snapped.0, 30);
    assert!((t.stability - 0.8).abs() < 1.0e-5, "{}", t.stability);
    press(&mut s, 10);
    assert_eq!(pieces(&mut s).len(), 1);
    kind(&mut s, 10, "");
    s.step_n(40);
    assert_eq!(pieces(&mut s).len(), 1, "it hangs from the socket for as long as it is there");
    // the building goes: the audit finds the piece with nothing to hold it
    let e = s.world.entity(30).unwrap();
    s.world.despawn(e);
    s.step_n(130);
    assert!(pieces(&mut s).is_empty());
    assert_eq!(events(&s, "build_collapse").len(), 1);
}

#[test]
fn a_loose_socket_is_only_a_place_to_snap_to_and_the_tool_does_not_call_a_piece_on_it_valid() {
    let mut s = world();
    spawn(&mut s, 31, r#"{"Transform":{"pos":[0,10,-20.5]},"Collider":{"shape":1,"size":[40,40,1]}}"#);
    // the same point as above but with `anchor` off: nothing is solid ground to what joins it
    spawn(&mut s, 30, r#"{"Transform":{"pos":[0,10,-20],"rot":[0,1,0,0]},"BuildSocket":{"kind":"edge","anchor":false}}"#);
    tool(&mut s, 10, [0.0, 14.0, -14.0], [0.0, 10.0, -20.0], "foundation");
    s.step();
    let t = tl(&mut s, 10);
    assert!(!t.valid, "a piece that the socket cannot hold is refused at once, not built and then collapsed: {:?}", t.pos);
    assert_eq!(why(&mut s, 10), "unstable");
    press(&mut s, 10);
    s.step_n(40);
    assert!(pieces(&mut s).is_empty());
    assert!(events(&s, "build_collapse").is_empty(), "nothing was built to collapse");
}

#[test]
fn pieces_put_in_a_scene_by_hand_find_out_if_they_stand_on_the_ground() {
    let mut s = world();
    let piece = |kind: &str, pos: [f32; 3]| {
        format!(r#"{{"Transform":{{"pos":[{},{},{}]}},"BuildPiece":{{"kind":"{kind}"}},"HitPoints":{{"value":500,"max":500,"last":500}}}}"#, pos[0], pos[1], pos[2])
    };
    // one foundation on the ground, one in the air (nothing under it), a wall on the first one's north edge
    spawn(&mut s, 40, &piece("foundation", [0.0, 0.25, 0.0]));
    spawn(&mut s, 41, &piece("foundation", [0.0, 20.25, 0.0]));
    spawn(&mut s, 42, &piece("wall", [0.0, 2.0, -2.0]));
    s.step_n(70);
    let p = pieces(&mut s);
    let ids: Vec<u64> = p.iter().map(|p| p.0).collect();
    assert_eq!(ids, vec![40, 42], "the one in the air collapsed, the others stand: {p:?}");
    let wall = p.iter().find(|p| p.0 == 42).unwrap();
    assert!((wall.4 - 0.9).abs() < 1.0e-5, "its stability was worked out: {}", wall.4);
    let e = s.world.entity(40).unwrap();
    let bp = *s.world.get::<BuildPiece>(e).unwrap();
    assert!(bp.anchored && bp.checked);
}

/// What a world full of pieces costs a tick (run by hand: `cargo test -p lunaris-sim --lib big_base --release -- --ignored --nocapture`).
#[test]
#[ignore]
fn big_base_costs_what_the_docs_say() {
    for side in [30usize, 70, 141] {
        let mut s = world();
        let mut text = String::new();
        let mut n = 0u64;
        for i in 0..side {
            for j in 0..side {
                n += 1;
                let (x, z) = (i as f32 * 4.0 - side as f32 * 2.0, j as f32 * 4.0 - side as f32 * 2.0 + 100.0);
                text.push_str(&format!(r#"{{"Transform":{{"pos":[{x},0.25,{z}]}},"BuildPiece":{{"kind":"foundation","anchored":true,"checked":true}},"HitPoints":{{"value":500,"max":500,"last":500}}}}"#));
                text.push('\n');
            }
        }
        for (k, line) in text.lines().enumerate() {
            spawn(&mut s, 1000 + k as u64, line);
        }
        let time = |s: &mut Sim, ticks: usize| -> Vec<f64> {
            (0..ticks)
                .map(|_| {
                    let t = std::time::Instant::now();
                    s.step();
                    t.elapsed().as_secs_f64() * 1000.0
                })
                .collect()
        };
        let stats = |mut v: Vec<f64>| {
            v.sort_by(|a, b| a.total_cmp(b));
            (v[v.len() / 2], v[v.len() - 1])
        };
        let first = time(&mut s, 70);
        let idle = time(&mut s, 120);
        let (med, max) = stats(idle);
        // a tool looking at the ground near the base
        kind(&mut s, 10, "foundation");
        aim(&mut s, 10, [0.0, 5.0, 0.0], [0.0, 0.0, 0.0]);
        let tool_ticks = time(&mut s, 60);
        let (tmed, tmax) = stats(tool_ticks);
        kind(&mut s, 10, "");
        // one piece destroyed
        let e = s.world.entity(1000 + n / 2).unwrap();
        s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
        let kill = time(&mut s, 2);
        println!("{n:6} pieces: first audit tick {:.1} ms, idle median {med:.3} ms (worst {max:.1} ms: the audits), tool active median {tmed:.3} ms (worst {tmax:.1}), a destruction {:.1} ms", first.iter().cloned().fold(0.0, f64::max), kill.iter().cloned().fold(0.0, f64::max));
    }
}

#[test]
fn building_is_deterministic() {
    let run = || {
        let mut s = world();
        assert!(put_free(&mut s, 10, "foundation", 0.0, 0.0));
        kind(&mut s, 10, "wall");
        aim(&mut s, 10, [0.0, 3.0, -0.5], [0.0, 0.25, -1.8]);
        s.step();
        press(&mut s, 10);
        kind(&mut s, 10, "foundation");
        aim(&mut s, 10, [3.2, 5.0, 0.3], [2.6, 0.0, 0.3]);
        s.step();
        press(&mut s, 10);
        s.step_n(100);
        s.world.state_hash()
    };
    assert_eq!(run(), run());
}

#[test]
fn a_piece_with_a_prefab_is_made_from_it_and_a_fallen_one_leaves_rubble() {
    let mut s = Sim::new(1);
    let rules = "rules step=90\npiece crate size=1,1,1 hp=0 free=1 anchor=1 prefab=crate_model debris=crate_rubble\n".replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"BuildRules":{{"text":"{rules}"}}}}"#));
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[80,1,80]}}"#);
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,5,0]},"BuildTool":{"reach":12,"owner":7}}"#);
    s.apply(&Command::DefinePrefab { name: "crate_model".into(), def: parse(r#"{"components":{"Name":{"value":"CrateModel"},"HitPoints":{"value":30,"max":30,"last":30},"Collider":{"shape":1,"size":[1,1,1]}}}"#).unwrap() }).unwrap();
    s.apply(&Command::DefinePrefab { name: "crate_rubble".into(), def: parse(r#"{"components":{"Name":{"value":"Rubble"}}}"#).unwrap() }).unwrap();
    tool(&mut s, 10, [0.0, 5.0, 0.0], [0.0, 0.0, 0.0], "crate");
    s.step();
    assert!(tl(&mut s, 10).valid, "{}", why(&mut s, 10));
    press(&mut s, 10);
    let p = pieces(&mut s);
    assert_eq!(p.len(), 1);
    let e = s.world.entity(p[0].0).unwrap();
    let name = s.world.get::<crate::components::Name>(e).unwrap().value.0;
    assert_eq!(s.world.strings().get(name), "CrateModel");
    assert_eq!(s.world.get::<HitPoints>(e).unwrap().value, 30.0, "the prefab's own hit points when the rules give none");
    kind(&mut s, 10, "");
    s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
    s.step_n(2);
    assert!(pieces(&mut s).is_empty());
    let found = s.world.entities_by_uid().into_iter().filter(|e| s.world.get::<crate::components::Name>(*e).is_some_and(|n| s.world.strings().get(n.value.0) == "Rubble")).count();
    assert_eq!(found, 1, "the rubble prefab was made where it fell");
}
