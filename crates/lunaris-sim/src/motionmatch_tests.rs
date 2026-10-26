//! Motion matching: the database file, the predicted path and the choice of the clip.
use super::*;
use crate::components::Animator;
use crate::motionmatch::{schema_groups, trajectory_query, DbClip, MotionDb, MotionMatcher, DIM, POSE_START};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A database of three one-second clips whose trajectories are: standing, running forwards at 5 m/s and running backwards at 5 m/s (frames every 1/30 s).
fn tiny_db() -> MotionDb {
    let mut times = Vec::new();
    let mut feat = Vec::new();
    let mut clips = Vec::new();
    for (name, speed, tags) in [("idle", 0.0f32, 1u32), ("run_f", 5.0, 2), ("run_b", -5.0, 2)] {
        let first = times.len() as u32;
        for k in 0..=30 {
            let mut f = [0.0f32; DIM];
            // velocity now, position and velocity of the future (the local z is the way it faces)
            f[3] = speed;
            f[5] = 1.0;
            f[7] = speed * 0.35;
            f[9] = 1.0;
            f[11] = speed * 0.7;
            f[13] = speed;
            f[15] = 1.0;
            // the pose: the feet swing with the clip's own phase
            f[POSE_START] = 0.1 * ((k as f32) * 0.3).sin() * if speed != 0.0 { 1.0 } else { 0.0 };
            times.push(k as f32 / 30.0);
            feat.extend_from_slice(&f);
        }
        clips.push(DbClip { name: name.to_string(), length: 1.0, looping: true, tags, first, count: 31, from: 0.0, to: 1.0 });
    }
    let mut groups = schema_groups();
    for g in groups.iter_mut() {
        g.scale = 1.0;
    }
    MotionDb::new(30.0, vec!["idle".into(), "run".into()], groups, clips, times, feat)
}

#[test]
fn a_database_survives_its_file() {
    let db = tiny_db();
    let back = MotionDb::from_bytes(&db.to_bytes()).unwrap();
    assert_eq!(db, back);
    assert!(MotionDb::from_bytes(b"nonsense").is_err());
    assert!(MotionDb::from_bytes(&db.to_bytes()[..40]).is_err());
}

#[test]
fn the_cost_is_zero_for_the_same_frame_and_grows_with_the_difference() {
    let db = tiny_db();
    let f = db.frame(40).to_vec();
    assert_eq!(db.cost(&f, &f, true), 0.0);
    let mut q = f.clone();
    q[3] += 1.0;
    let near = db.cost(&q, &f, true);
    q[3] += 3.0;
    assert!(db.cost(&q, &f, true) > near && near > 0.0);
    // a query without a pose only looks at the trajectory
    let mut p = f.clone();
    p[POSE_START] += 5.0;
    assert!(db.cost(&p, &f, true) > 0.0);
    assert_eq!(db.cost(&p, &f, false), 0.0);
}

#[test]
fn the_predicted_path_follows_the_wish() {
    // standing, asked to walk forwards at 4 m/s: in 0.7 s it has speeded up (16 m/s2: 0.25 s) and is well ahead
    let q = trajectory_query([0.0, 1.0], [0.0, 0.0], [0.0, 4.0], [0.0, 1.0], &crate::motionmatch::Dynamics::new(16.0, 22.0, 8.0));
    assert!(q[7] > 0.3 && q[7] < 0.35 * 4.0, "pos at 0.35: {}", q[7]);
    assert!(q[11] > q[7] * 1.5, "pos at 0.7: {}", q[11]);
    assert!((q[13] - 4.0).abs() < 0.05, "velocity at 0.7: {}", q[13]);
    assert!(q[6].abs() < 1e-4 && q[10].abs() < 1e-4, "straight ahead");
    // the same wish to the character's left (its local x is along (z, -x) = (1, 0) when it faces +z): the path goes sideways
    let q = trajectory_query([0.0, 1.0], [0.0, 0.0], [4.0, 0.0], [0.0, 1.0], &crate::motionmatch::Dynamics::new(16.0, 22.0, 8.0));
    assert!(q[10] > 1.0 && q[11].abs() < 1e-4);
    // facing turns towards the wanted facing
    let q = trajectory_query([0.0, 1.0], [0.0, 0.0], [0.0, 0.0], [1.0, 0.0], &crate::motionmatch::Dynamics::new(16.0, 22.0, 8.0));
    assert!(q[14] > 0.5, "facing at 0.7 s: {:?}", &q[14..16]);
}

fn world_with(db: &MotionDb, extra: &str) -> (Sim, std::path::PathBuf) {
    // one folder a world: the tests run side by side and one that finishes deletes its folder
    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("lunaris_mm_test_{}_{}", std::process::id(), COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t.mmdb"), db.to_bytes()).unwrap();
    let mut s = Sim::new(1);
    s.asset_root = Some(dir.clone());
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Animator":{{"clip":"idle","looping":true}},"MotionMatcher":{{"db":"t.mmdb"{extra}}}}}"#));
    (s, dir)
}

fn clip_name(s: &Sim) -> String {
    let e = s.world.entity(1).unwrap();
    s.world.strings().get(s.world.get::<Animator>(e).unwrap().clip.0).to_string()
}

#[test]
fn standing_still_picks_the_idle_and_asking_to_run_forwards_picks_the_forward_run() {
    let db = tiny_db();
    let (mut s, dir) = world_with(&db, "");
    for _ in 0..12 {
        s.step();
    }
    assert_eq!(clip_name(&s), "idle");
    let e = s.world.entity(1).unwrap();
    assert_eq!(s.world.get::<MotionMatcher>(e).unwrap().clip, 1, "an animator that already plays the best clip: `clip` says which (1 + its number)");
    // the player asks for 5 m/s ahead
    {
        let m = s.world.get_mut::<MotionMatcher>(e).unwrap();
        m.vel = [0.0, 5.0];
        m.face = [0.0, 1.0];
    }
    for _ in 0..30 {
        s.step();
    }
    assert_eq!(clip_name(&s), "run_f", "cost {}", s.world.get::<MotionMatcher>(e).unwrap().cost);
    // and backwards
    {
        let m = s.world.get_mut::<MotionMatcher>(e).unwrap();
        m.vel = [0.0, -5.0];
        m.face = [0.0, 1.0];
    }
    for _ in 0..40 {
        s.step();
    }
    assert_eq!(clip_name(&s), "run_b");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_forced_search_ignores_the_clip_that_is_playing() {
    let db = tiny_db();
    let (mut s, dir) = world_with(&db, "");
    for _ in 0..12 {
        s.step();
    }
    assert_eq!(clip_name(&s), "idle");
    let e = s.world.entity(1).unwrap();
    // only the run clips may be picked now, but the idle that is playing is still the best match for standing still: it goes on
    s.world.get_mut::<MotionMatcher>(e).unwrap().tags = 2;
    for _ in 0..30 {
        s.step();
    }
    assert_eq!(clip_name(&s), "idle");
    // a forced search is a fresh choice among the clips that are allowed
    s.world.get_mut::<MotionMatcher>(e).unwrap().force = true;
    s.step();
    assert_ne!(clip_name(&s), "idle");
    assert!(!s.world.get::<MotionMatcher>(e).unwrap().force, "the request is used up");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_clips_search_window_limits_the_frames_that_may_be_picked() {
    let mut db = tiny_db();
    let mut clips = db.clips.clone();
    clips[1].from = 0.5;
    clips[1].to = 0.6;
    db = MotionDb::new(db.rate, db.tag_names.clone(), db.groups.clone(), clips, db.times.clone(), db.feat.clone());
    let (mut s, dir) = world_with(&db, r#","vel":[0,5],"face":[0,1]"#);
    for _ in 0..30 {
        s.step();
    }
    assert_eq!(clip_name(&s), "run_f");
    let e = s.world.entity(1).unwrap();
    let t = s.world.get::<Animator>(e).unwrap().time;
    // it was picked inside [0.5, 0.6] and has played about half a second since at most
    assert!(t >= 0.5 - 1e-3, "{t}");
    let back = MotionDb::from_bytes(&db.to_bytes()).unwrap();
    assert_eq!((back.clips[1].from, back.clips[1].to), (0.5, 0.6));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_predicted_path_uses_the_decel_for_turning_round_and_a_steady_turn_rate() {
    use crate::motionmatch::Dynamics;
    // running at 5 m/s with the wish to go the other way: the velocity is cut back at `decel` (10), not at the stop's `brake` (22)
    let quick = trajectory_query([0.0, 1.0], [0.0, 5.0], [0.0, -5.0], [0.0, -1.0], &Dynamics::new(8.0, 22.0, 8.0));
    let soft = trajectory_query([0.0, 1.0], [0.0, 5.0], [0.0, -5.0], [0.0, -1.0], &Dynamics { decel: 10.0, ..Dynamics::new(8.0, 22.0, 8.0) });
    assert!(soft[7] > quick[7] + 0.2, "slower braking carries it further: {} against {}", soft[7], quick[7]);
    // no wish at all is a stop, at the brake
    let stop = trajectory_query([0.0, 1.0], [0.0, 5.0], [0.0, 0.0], [0.0, 0.0], &Dynamics { decel: 10.0, ..Dynamics::new(8.0, 22.0, 8.0) });
    assert!(stop[7] < soft[7], "{} {}", stop[7], soft[7]);
    // a steady turn of 3 rad/s towards a facing 150 degrees away: after 0.35 s the facing has turned about 1.05 rad (a smooth approach at 8/s would have turned most of the way by then)
    let want = [0.5, -0.866_025_4];
    let steady = trajectory_query([0.0, 1.0], [0.0, 0.0], [0.0, 0.0], want, &Dynamics { turn_rate: 3.0, ..Dynamics::new(8.0, 22.0, 8.0) });
    let angle = steady[8].atan2(steady[9]).abs();
    assert!((angle - 1.05).abs() < 0.12, "{angle}");
    let smooth = trajectory_query([0.0, 1.0], [0.0, 0.0], [0.0, 0.0], want, &Dynamics::new(8.0, 22.0, 8.0));
    assert!(smooth[8].atan2(smooth[9]).abs() > angle + 0.3, "the smooth approach has turned further after a moment: {} {}", smooth[8].atan2(smooth[9]), angle);
}

#[test]
fn a_change_of_the_wish_searches_at_once() {
    let db = tiny_db();
    let (mut s, dir) = world_with(&db, r#","search_interval":10.0"#);
    for _ in 0..5 {
        s.step();
    }
    let e = s.world.entity(1).unwrap();
    let before = s.world.get::<MotionMatcher>(e).unwrap().searches;
    s.step();
    assert_eq!(s.world.get::<MotionMatcher>(e).unwrap().searches, before, "no reason to search yet");
    {
        let m = s.world.get_mut::<MotionMatcher>(e).unwrap();
        m.vel = [0.0, 5.0];
        m.face = [0.0, 1.0];
    }
    s.step();
    assert_eq!(s.world.get::<MotionMatcher>(e).unwrap().searches, before + 1, "the wish moved by 5 m/s: a search this very step");
    s.step();
    assert_eq!(s.world.get::<MotionMatcher>(e).unwrap().searches, before + 1);
    s.world.get_mut::<MotionMatcher>(e).unwrap().tags = 2;
    s.step();
    assert_eq!(s.world.get::<MotionMatcher>(e).unwrap().searches, before + 2, "so did new tags");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn tags_say_which_clips_may_be_picked() {
    let db = tiny_db();
    // only the idle tag: asked to run, it still picks the idle
    let (mut s, dir) = world_with(&db, r#","tags":1,"vel":[0,5],"face":[0,1]"#);
    for _ in 0..30 {
        s.step();
    }
    assert_eq!(clip_name(&s), "idle");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn motion_matching_is_deterministic() {
    let db = tiny_db();
    let run = || {
        let (mut s, dir) = world_with(&db, r#","vel":[0,5],"face":[0,1]"#);
        for _ in 0..90 {
            s.step();
        }
        let h = s.state_hash();
        let _ = std::fs::remove_dir_all(dir);
        h
    };
    assert_eq!(run(), run());
}
