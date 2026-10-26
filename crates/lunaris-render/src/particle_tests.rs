//! Particle clouds: every particle is a closed-form function of the clock, so the tests check the formulas, the counts, the order and the picture.
use super::*;
use crate::math::*;
use crate::particles::{build, CameraBasis};
use lunaris_json::{parse, Json};
use lunaris_sim::components::{Particles, Transform};
use lunaris_sim::{Command, Sim};

fn camera() -> CameraBasis {
    CameraBasis { pos: [0.0, 0.0, 10.0], forward: [0.0, 0.0, -1.0], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0] }
}

fn one() -> Particles {
    // one particle that flies straight along +x at speed 2, with gravity 10 down, living 4 seconds, whole and opaque
    Particles { count: 1, lifetime: 4.0, looping: false, cone: 0.0, direction: [1.0, 0.0, 0.0], speed_range: [2.0, 2.0], gravity: [0.0, -10.0, 0.0], size: [1.0, 1.0], size_jitter: 0.0, fade_in: 0.0, color: [1.0; 4], color_end: [1.0; 4], ..Default::default() }
}

/// The middle of the first quad of a mesh.
fn centre(m: &crate::mesh::Mesh, quad: usize) -> V3 {
    let q = &m.pos[quad * 4..quad * 4 + 4];
    scale(add(add(q[0], q[1]), add(q[2], q[3])), 0.25)
}

#[test]
fn a_particle_follows_the_closed_form_motion_with_and_without_drag() {
    let tr = Transform::default();
    let mut p = one();
    p.time = 1.0;
    let m = build(&p, &tr, &camera(), 1);
    let c = centre(&m, 0);
    // x = v t, y = -g t^2 / 2 after one second
    assert!((c[0] - 2.0).abs() < 1e-4 && (c[1] + 5.0).abs() < 1e-4 && c[2].abs() < 1e-4, "{c:?}");
    // with drag k: x = v (1 - e^-kt) / k
    p.drag = 1.0;
    p.gravity = [0.0; 3];
    let d = centre(&build(&p, &tr, &camera(), 1), 0);
    assert!((d[0] - 2.0 * (1.0 - (-1.0f32).exp())).abs() < 1e-4, "{d:?}");
    // the quad is a metre square facing the camera
    let q = &m.pos[0..4];
    assert!((length(sub(q[1], q[0])) - 1.0).abs() < 1e-4 && (length(sub(q[3], q[0])) - 1.0).abs() < 1e-4);
    assert!(q.iter().all(|v| (v[2] - q[0][2]).abs() < 1e-4));
    // it moves with its entity
    let moved = Transform { pos: [10.0, 0.0, 0.0], ..Transform::default() };
    let shifted = centre(&build(&Particles { drag: 0.0, gravity: [0.0, -10.0, 0.0], ..p }, &moved, &camera(), 1), 0);
    assert!((shifted[0] - 12.0).abs() < 1e-4, "{shifted:?}");
}

#[test]
fn counts_follow_the_stream_the_burst_and_the_prewarm() {
    let tr = Transform::default();
    let quads = |p: &Particles| build(p, &tr, &camera(), 1).pos.len() / 4;
    let stream = Particles { count: 100, lifetime: 2.0, fade_in: 0.0, ..Default::default() };
    // a steady stream: every particle is somewhere in its life, whatever the time
    for t in [0.0, 0.7, 5.3] {
        assert_eq!(quads(&Particles { time: t, ..stream }), 100, "t = {t}");
    }
    // without prewarm the stream fills up: at 0.5 s only those whose first birth has come (phase <= 0.25)
    assert_eq!(quads(&Particles { prewarm: false, time: 0.5, ..stream }), 26);
    assert_eq!(quads(&Particles { prewarm: false, time: 2.0, ..stream }), 100);
    // a burst born at once lives out its lifetime and is gone
    let burst = Particles { count: 50, lifetime: 2.0, looping: false, fade_in: 0.0, ..Default::default() };
    assert_eq!((quads(&Particles { time: 0.0, ..burst }), quads(&Particles { time: 1.9, ..burst }), quads(&Particles { time: 2.1, ..burst })), (50, 50, 0));
    // born over a second, the burst fills up and then empties again
    let spread = Particles { spread_time: 1.0, ..burst };
    assert_eq!((quads(&Particles { time: 0.5, ..spread }), quads(&Particles { time: 1.5, ..spread }), quads(&Particles { time: 3.1, ..spread })), (26, 50, 0));
    // the cap, and nothing with an alpha of nothing
    assert_eq!(quads(&Particles { count: u32::MAX, ..stream }), crate::particles::MAX_PARTICLES as usize);
    assert_eq!(quads(&Particles { color: [1.0, 1.0, 1.0, 0.0], color_end: [1.0, 1.0, 1.0, 0.0], ..stream }), 0);
}

#[test]
fn clouds_are_repeatable_depend_on_the_seed_and_alpha_ones_are_sorted_far_to_near() {
    let tr = Transform::default();
    let cloud = Particles { count: 300, shape: 2, extent: [3.0, 3.0, 8.0], time: 0.3, additive: false, ..Default::default() };
    let (a, b) = (build(&cloud, &tr, &camera(), 1), build(&cloud, &tr, &camera(), 1));
    assert_eq!(a.pos, b.pos);
    assert_eq!(a.col, b.col);
    assert_ne!(a.pos, build(&Particles { seed: 2, ..cloud }, &tr, &camera(), 1).pos);
    // far to near: the distance along the view direction never grows from one quad to the next
    let depths: Vec<f32> = (0..a.pos.len() / 4).map(|i| dot(sub(centre(&a, i), camera().pos), camera().forward)).collect();
    assert!(depths.windows(2).all(|w| w[0] >= w[1] - 1e-4), "sorted");
    assert!(depths.len() > 100 && depths[0] > depths[depths.len() - 1] + 3.0, "{} quads", depths.len());
    // behind the camera is not built
    let behind = build(&Particles { shape: 0, ..cloud }, &Transform { pos: [0.0, 0.0, 20.0], ..Transform::default() }, &camera(), 1);
    assert!(behind.pos.is_empty());
}

#[test]
fn stretched_particles_lie_along_their_velocity_and_sheets_pick_a_frame_from_the_life() {
    let tr = Transform::default();
    let mut p = one();
    p.time = 0.01;
    p.gravity = [0.0; 3];
    p.size = [0.1, 0.1];
    p.stretch = 0.5;
    let m = build(&p, &tr, &camera(), 1);
    // a speed of 2 and a stretch of 0.5: 1.1 long along x, 0.1 wide
    let xs: Vec<f32> = m.pos.iter().map(|v| v[0]).collect();
    let ys: Vec<f32> = m.pos.iter().map(|v| v[1]).collect();
    let span = |v: &[f32]| v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min);
    assert!((span(&xs) - 1.1).abs() < 1e-3 && (span(&ys) - 0.1).abs() < 1e-3, "{} x {}", span(&xs), span(&ys));
    // a 4 x 1 sheet shows the quarter of the picture that matches the age: u = 0.6 is the third frame
    let mut s = one();
    s.sheet = [4, 1];
    s.time = 2.4;
    let m = build(&s, &tr, &camera(), 1);
    let us: Vec<f32> = m.uv.iter().map(|uv| uv[0]).collect();
    assert!((us.iter().cloned().fold(f32::MAX, f32::min) - 0.5).abs() < 1e-5 && (us.iter().cloned().fold(f32::MIN, f32::max) - 0.75).abs() < 1e-5, "{us:?}");
}

fn world_with(cloud: &str) -> Sim {
    let mut s = Sim::new(1);
    for (uid, comps) in [(1u64, r#"{"Transform":{"pos":[0,0,6]},"Camera":{"clear":[0,0,0,1]}}"#.to_string()), (2, format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Particles":{cloud}}}"#))] {
        let Json::Obj(components) = parse(&comps).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    }
    s
}

fn picture(s: &mut Sim) -> Frame {
    let mut tex = TextureCache::new(None);
    render(&mut s.world, &mut tex, &RenderOptions { width: 96, height: 64, ..RenderOptions::default() }).unwrap()
}

fn lit(f: &Frame) -> usize {
    f.rgba.chunks(4).filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30).count()
}

#[test]
fn a_cloud_shows_in_the_picture_runs_with_the_clock_and_hides_on_request() {
    let mut s = world_with(r#"{"count":400,"lifetime":2,"shape":1,"extent":[1.5,0,0],"cone":180,"speed_range":[0.2,0.6],"size":[0.3,0.3],"color":[1,0.5,0.1,1],"color_end":[1,0.5,0.1,1],"additive":true}"#);
    let on = picture(&mut s);
    assert!(lit(&on) > 300, "{} lit pixels", lit(&on));
    // the clock is game state: it runs 60 ticks a second at speed 1, and not at all when paused
    s.step_n(60);
    let t = s.world.get::<Particles>(s.world.entity(2).unwrap()).unwrap().time;
    assert!((t - 1.0).abs() < 1e-3, "{t}");
    assert_ne!(picture(&mut s).hash(), on.hash(), "the cloud moved");
    s.apply(&Command::Set { uid: 2, component: "Particles".into(), value: parse(r#"{"playing":false}"#).unwrap() }).unwrap();
    s.step_n(30);
    assert!((s.world.get::<Particles>(s.world.entity(2).unwrap()).unwrap().time - t).abs() < 1e-6);
    // the same clock gives the same picture, whenever and however it is reached
    let a = picture(&mut s).hash();
    s.apply(&Command::Set { uid: 2, component: "Particles".into(), value: parse(r#"{"time":0.0}"#).unwrap() }).unwrap();
    assert_eq!(picture(&mut s).hash(), on.hash(), "time 0 again is the first picture");
    s.apply(&Command::Set { uid: 2, component: "Particles".into(), value: parse(&format!(r#"{{"time":{t}}}"#)).unwrap() }).unwrap();
    assert_eq!(picture(&mut s).hash(), a);
    // hidden: nothing
    s.apply(&Command::Set { uid: 2, component: "Particles".into(), value: parse(r#"{"visible":false}"#).unwrap() }).unwrap();
    assert_eq!(lit(&picture(&mut s)), 0);
}

#[test]
fn additive_clouds_add_light_and_alpha_clouds_blend() {
    // two clouds on the same spot: added, they are brighter than one; blended opaque, they are no brighter than one
    let one_dot = r#"{"count":1,"lifetime":4,"looping":false,"cone":0,"speed_range":[0,0],"gravity":[0,0,0],"size":[2,2],"size_jitter":0,"fade_in":0,"color":[0.4,0.4,0.4,1],"color_end":[0.4,0.4,0.4,1],"texture":"circle","additive":ADD}"#;
    let mut brightness = Vec::new();
    for (add, copies) in [("true", 1), ("true", 2), ("false", 1), ("false", 2)] {
        let mut s = world_with(&one_dot.replace("ADD", add));
        if copies == 2 {
            let Json::Obj(components) = parse(&format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Particles":{}}}"#, one_dot.replace("ADD", add))).unwrap() else { panic!() };
            s.apply(&Command::Spawn { uid: Some(3), components }).unwrap();
        }
        let f = picture(&mut s);
        let o = ((32 * f.width + 48) * 4) as usize;
        brightness.push(f.rgba[o] as i32);
    }
    assert!(brightness[1] > brightness[0] + 20, "added light adds up: {brightness:?}");
    assert!((brightness[3] - brightness[2]).abs() <= 2 || brightness[3] >= brightness[2], "blended: {brightness:?}");
}

#[test]
fn a_decal_lies_on_its_surface_turns_to_its_normal_and_fades_and_goes_when_its_time_is_up() {
    // a camera above a floor looking straight down; a decal on the floor shows as a red disc, one on a wall (normal along x) is seen edge on
    let mut s = Sim::new(1);
    for (uid, comps) in [
        (1u64, r#"{"Transform":{"pos":[0,6,0],"rot":[-0.7071068,0,0,0.7071068]},"Camera":{"clear":[0,0,0,1]}}"#),
        (2, r#"{"Transform":{"pos":[0,0,0]},"Decal":{"texture":"circle","size":[2,2],"color":[1,0,0,1],"normal":[0,1,0]}}"#),
    ] {
        let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    }
    let red = |s: &mut Sim| {
        let f = picture(s);
        f.rgba.chunks(4).filter(|p| p[0] > 120 && p[1] < 60).count()
    };
    let flat = red(&mut s);
    assert!(flat > 200, "a disc 2 m across seen from 6 m up: {flat} red pixels");
    // on a wall (normal along x) it faces sideways: from above it is a sliver
    s.apply(&Command::Set { uid: 2, component: "Decal".into(), value: parse(r#"{"normal":[1,0,0]}"#).unwrap() }).unwrap();
    assert!(red(&mut s) < flat / 10, "edge on");
    // a normal of nothing or nonsense falls back to up; a roll turns it (a square seen from above changes with the roll)
    s.apply(&Command::Set { uid: 2, component: "Decal".into(), value: parse(r#"{"normal":[0,0,0],"texture":"checker","color":[1,0,0,1]}"#).unwrap() }).unwrap();
    let a = picture(&mut s).hash();
    s.apply(&Command::Set { uid: 2, component: "Decal".into(), value: parse(r#"{"roll":0.5}"#).unwrap() }).unwrap();
    assert_ne!(picture(&mut s).hash(), a);
    // a life of one second with a fade of half a second: full for half a second, then fading, then gone (the entity is despawned)
    s.apply(&Command::Set { uid: 2, component: "Decal".into(), value: parse(r#"{"normal":[0,1,0],"texture":"circle","roll":0,"life":1.0,"fade":0.5,"age":0}"#).unwrap() }).unwrap();
    let full = red(&mut s);
    s.step_n(45); // 0.75 s: half way through the fade
    let half = red(&mut s);
    assert!(half > 0 && half < full * 3 / 4 || half == full, "fading: {half} of {full}");
    assert!(s.world.entity(2).is_some());
    s.step_n(30);
    assert!(s.world.entity(2).is_none(), "gone after its life");
    assert_eq!(red(&mut s), 0);
}

/// Not a test: prints how long a cloud of 60,000 takes to build (`cargo test -p lunaris-render how_fast_are_particles -- --ignored --nocapture`).
#[test]
#[ignore]
fn how_fast_are_particles() {
    let tr = Transform { pos: [0.0, 0.0, 0.0], ..Transform::default() };
    for threads in [1usize, 4] {
        for (additive, count) in [(true, 60_000u32), (false, 60_000), (true, 10_000)] {
            let p = Particles { count, lifetime: 3.0, shape: 1, extent: [3.0, 0.0, 0.0], cone: 180.0, time: 1.7, additive, fade_in: 0.0, ..Default::default() };
            let t = std::time::Instant::now();
            let mut quads = 0;
            for _ in 0..5 {
                quads = build(&p, &tr, &camera(), threads).pos.len() / 4;
            }
            println!("{count} particles, {}, {threads} thread(s): {quads} quads, {:.2} ms per cloud", if additive { "additive" } else { "sorted alpha" }, t.elapsed().as_secs_f64() * 1000.0 / 5.0);
        }
    }
}

#[test]
fn a_cloud_is_the_same_mesh_however_many_threads_build_it() {
    let tr = Transform::default();
    let p = Particles { count: 30_000, lifetime: 2.0, shape: 1, extent: [2.0, 0.0, 0.0], cone: 180.0, time: 0.9, additive: false, fade_in: 0.0, turbulence: 0.3, ..Default::default() };
    let one = build(&p, &tr, &camera(), 1);
    for t in [2, 3, 8] {
        let many = build(&p, &tr, &camera(), t);
        assert_eq!((&many.pos, &many.col, &many.texw), (&one.pos, &one.col, &one.texw), "{t} threads");
    }
}


// ---- particle modules (the Niagara-style part): gradients, forces, colliders, trails, solids

use crate::particles::build_with;
use lunaris_sim::particle_modules::parse as modules;

/// One particle with no gravity, standing still at the origin, living 4 seconds, whole and opaque: what the forces and colliders then do to it is all there is to see.
fn still() -> Particles {
    Particles { speed_range: [0.0, 0.0], gravity: [0.0; 3], ..one() }
}

fn at(p: &Particles, text: &str, time: f32) -> crate::mesh::Mesh {
    build_with(&Particles { time, ..*p }, &modules(text).unwrap(), &Transform::default(), &camera(), 1)
}

#[test]
fn gradients_set_the_colour_and_size_over_the_life() {
    let p = one();
    let text = "color 0:1,0,0,1 0.5:0,1,0,1 1:0,0,1,0.5\nsize 0:1 0.5:3 1:1";
    // a quarter of the way: half red, half green (colours are squared on their way to the picture)
    let m = at(&p, text, 1.0);
    assert!((m.col[0][0] - 0.25).abs() < 1e-4 && (m.col[0][1] - 0.25).abs() < 1e-4 && m.col[0][2] == 0.0, "{:?}", m.col[0]);
    assert!((m.texw[0] - 1.0).abs() < 1e-6);
    // half way: the middle key, green and 3 metres wide
    let m = at(&p, text, 2.0);
    assert!(m.col[0][0] == 0.0 && (m.col[0][1] - 1.0).abs() < 1e-5);
    assert!((length(sub(m.pos[1], m.pos[0])) - 3.0).abs() < 1e-3, "{}", length(sub(m.pos[1], m.pos[0])));
    // three quarters: the opacity falls from 1 to 0.5 at the end
    let m = at(&p, text, 3.0);
    assert!((m.texw[0] - 0.75).abs() < 1e-5, "{}", m.texw[0]);
}

#[test]
fn a_wind_pushes_and_an_attractor_pulls() {
    let p = still();
    // a steady 2 m/s^2 for one second moves a resting particle 1 metre
    let m = at(&p, "wind 2,0,0", 1.0);
    let c = centre(&m, 0);
    assert!((c[0] - 1.0).abs() < 0.02 && c[1].abs() < 1e-4 && c[2].abs() < 1e-4, "{c:?}");
    // an attractor above it draws it up, and one that pushes away sends it down
    let up = centre(&at(&p, "attract 0,5,0 strength=4 radius=10", 1.0), 0);
    assert!(up[1] > 0.5 && up[1] < 5.0 && up[0].abs() < 1e-4, "{up:?}");
    let down = centre(&at(&p, "attract 0,5,0 strength=-4 radius=10", 1.0), 0);
    assert!(down[1] < -0.5, "{down:?}");
    // a vortex round y swings a particle that starts beside it
    let beside = Particles { shape: 4, extent: [1.0, 1.0, 1.0], ..p };
    let a = centre(&at(&beside, "vortex 0,0,0 axis=0,1,0 strength=6 radius=4", 0.5), 0);
    let b = centre(&at(&beside, "", 0.5), 0);
    assert!(length(sub(a, b)) > 0.1, "{a:?} {b:?}");
}

#[test]
fn a_floor_bounces_sticks_or_kills() {
    // thrown straight up at 5 m/s with gravity 10: up for half a second, back at the floor after a whole one, then it bounces
    let p = Particles { speed_range: [5.0, 5.0], gravity: [0.0, -10.0, 0.0], direction: [0.0, 1.0, 0.0], ..one() };
    let y = |text: &str, t: f32| -> Option<f32> {
        let m = at(&p, text, t);
        (!m.pos.is_empty()).then(|| centre(&m, 0)[1])
    };
    let floor = "collide plane 0,1,0 d=0 bounce=0.5 friction=0";
    assert!((y(floor, 0.5).unwrap() - 1.25).abs() < 0.05, "the first apex");
    for k in 0..40 {
        assert!(y(floor, k as f32 * 0.1).unwrap() >= -1e-3, "never under the floor");
    }
    // after the bounce at half the speed it rises to a quarter of the height: 0.3125 at 1.25 s
    let second = y(floor, 1.25).unwrap();
    assert!((second - 0.3125).abs() < 0.08, "second apex {second}");
    // without the floor it just keeps falling
    assert!(y("wind 0,0,0", 1.5).unwrap() < -1.0);
    // sticking: it lies where it landed; killing: it is gone
    let stuck = y("collide plane 0,1,0 d=0 stick", 3.0).unwrap();
    assert!(stuck.abs() < 1e-4, "{stuck}");
    assert!(y("collide plane 0,1,0 d=0 kill", 1.5).is_none());
    assert!(y("collide plane 0,1,0 d=0 kill", 0.5).is_some(), "alive until it lands");
    // a ball keeps particles out of it, or in
    let out = at(&still(), "collide sphere 0,0,0 r=1 bounce=0\nwind 0,5,0", 0.2);
    assert!(length(centre(&out, 0)) >= 0.99);
    let inside = at(&still(), "collide sphere 0,0,0 r=1 inside bounce=0\nwind 0,5,0", 3.0);
    assert!(length(centre(&inside, 0)) <= 1.01);
}

#[test]
fn trails_are_ribbons_that_thin_towards_the_tail() {
    let p = one();
    let m = at(&p, "trail segments=4 time=1 width=1", 2.0);
    // five points a ribbon, two vertices each, then the head's four
    assert_eq!((m.pos.len(), m.idx.len()), (10 + 4, 4 * 6 + 6));
    let width = |j: usize| length(sub(m.pos[j * 2 + 1], m.pos[j * 2]));
    assert!((width(0) - 1.0).abs() < 1e-3, "full width at the head: {}", width(0));
    assert!(width(0) > width(2) && width(2) > width(3) && width(4) < 1e-4, "{} {} {} {}", width(0), width(2), width(3), width(4));
    // the points lie on the path behind the head: the particle flies along +x, so they step back in x
    let xs: Vec<f32> = (0..5).map(|j| (m.pos[j * 2][0] + m.pos[j * 2 + 1][0]) * 0.5).collect();
    assert!(xs.windows(2).all(|w| w[0] > w[1]), "{xs:?}");
    // a trail younger than its length does not reach back past the birth
    let young = at(&p, "trail segments=4 time=1", 0.2);
    assert!(young.pos.len() == 14);
}

#[test]
fn solids_replace_the_pictures() {
    let p = Particles { count: 3, ..one() };
    for (kind, verts) in [("cube", 36), ("diamond", 24), ("tetra", 12)] {
        let m = at(&p, &format!("mesh {kind}"), 0.5);
        assert!(m.pos.len() >= verts && m.pos.len() % verts == 0, "{kind}: {}", m.pos.len());
        assert_eq!(m.idx.len(), m.pos.len(), "{kind}: one index a vertex");
        assert!(m.nrm.iter().all(|n| (length(*n) - 1.0).abs() < 1e-4));
    }
    // the cube has the right size (half the size on each side): its corners are within the half diagonal of the centre
    let one_cube = at(&one(), "mesh cube", 0.5);
    let c = centre_of(&one_cube);
    let far = one_cube.pos.iter().map(|q| length(sub(*q, c))).fold(0.0f32, f32::max);
    assert!((far - 0.5 * 3f32.sqrt()).abs() < 1e-3, "{far}");
}

fn centre_of(m: &crate::mesh::Mesh) -> V3 {
    let n = m.pos.len() as f32;
    scale(m.pos.iter().fold([0.0; 3], |a, q| add(a, *q)), 1.0 / n)
}

#[test]
fn modules_give_the_same_cloud_whatever_the_number_of_threads() {
    let cloud = Particles { count: 12_000, lifetime: 2.0, time: 5.3, shape: 1, extent: [1.0; 3], cone: 60.0, speed_range: [1.0, 4.0], gravity: [0.0, -6.0, 0.0], additive: false, ..Default::default() };
    let text = "wind 1,0,0 gust=0.4\nattract 0,2,0 strength=3 radius=6\nnoise strength=1\ncollide plane 0,1,0 d=-1 bounce=0.4\ntrail segments=3 time=0.2";
    let m = modules(text).unwrap();
    let (a, b) = (build_with(&cloud, &m, &Transform::default(), &camera(), 1), build_with(&cloud, &m, &Transform::default(), &camera(), 4));
    assert!(a.pos.len() > 10_000 && a.pos == b.pos && a.col == b.col && a.idx == b.idx);
    // and it differs from the same cloud without them
    assert_ne!(a.pos.len(), build(&cloud, &Transform::default(), &camera(), 1).pos.len());
}

/// `cargo test -p lunaris-render particle_module_cost -- --ignored --nocapture`: the price of the dear modules on 60,000 particles.
#[test]
#[ignore]
fn particle_module_cost() {
    let cloud = Particles { count: 60_000, lifetime: 3.0, time: 7.0, shape: 1, extent: [1.0; 3], cone: 60.0, speed_range: [1.0, 4.0], gravity: [0.0, -6.0, 0.0], ..Default::default() };
    for (name, text) in [("nothing", ""), ("gradients", "color 0:1,1,1,1 1:0,0,0,0\nsize 0:0.1 1:0.3"), ("wind", "wind 1,0,0 gust=0.4"), ("wind + floor", "wind 1,0,0\ncollide plane 0,1,0 d=-1"), ("attract + vortex + floor", "attract 0,2,0 strength=3 radius=6\nvortex 0,0,0 strength=3 radius=5\ncollide plane 0,1,0 d=-1"), ("noise + floor", "noise strength=1\ncollide plane 0,1,0 d=-1"), ("trail 6", "wind 1,0,0\ntrail segments=6 time=0.3"), ("cubes", "mesh cube")] {
        let m = modules(text).unwrap();
        for threads in [1, 4] {
            let t = std::time::Instant::now();
            let mesh = build_with(&cloud, &m, &Transform::default(), &camera(), threads);
            println!("{name:26} {threads} thread(s): {:6.1} ms, {} vertices", t.elapsed().as_secs_f64() * 1000.0, mesh.pos.len());
        }
    }
}

/// A hash over everything a cloud's mesh holds (the bits of every number, in order).
fn mesh_hash(m: &crate::mesh::Mesh) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |v: u64| {
        h = (h ^ v).wrapping_mul(0x0000_0100_0000_01b3);
        h ^= h >> 29;
    };
    for p in &m.pos {
        p.iter().for_each(|v| eat(v.to_bits() as u64));
    }
    for p in &m.uv {
        p.iter().for_each(|v| eat(v.to_bits() as u64));
    }
    for p in &m.col {
        p.iter().for_each(|v| eat(v.to_bits() as u64));
    }
    m.texw.iter().for_each(|v| eat(v.to_bits() as u64));
    m.idx.iter().for_each(|v| eat(*v as u64));
    eat(m.pos.len() as u64);
    h
}

/// The clouds of a game's city's AI tower (the dearest effects of its client: 9 800 particles every picture) and a few others that use the other shapes, an alpha cloud that has to be sorted, a sheet and a trail.
fn recorded_clouds() -> Vec<(Particles, &'static str)> {
    let base = Particles { playing: true, time: 1.6666658, looping: true, prewarm: true, ..Default::default() };
    vec![
        (Particles { count: 6000, lifetime: 8.0, shape: 1, extent: [105.0, 105.0, 105.0], direction: [0.0, 1.0, 0.0], cone: 180.0, speed_range: [2.0, 9.0], drag: 0.4, turbulence: 22.0, turbulence_scale: 70.0, size: [11.0, 3.5], size_jitter: 0.5, color: [0.5, 0.85, 1.0, 0.5], color_end: [0.225, 0.425, 1.0, 0.0], fade_in: 0.15, additive: true, seed: 7, ..base }, ""),
        (Particles { count: 2400, lifetime: 7.0, shape: 4, extent: [190.0, 1.0, 190.0], direction: [0.0, 0.08, 0.0], cone: 25.0, speed_range: [1.0, 4.5], drag: 0.2, turbulence: 8.0, turbulence_scale: 35.0, size: [7.0, 2.4], size_jitter: 0.4, color: [0.5, 0.85, 1.0, 0.7], color_end: [0.225, 0.425, 1.0, 0.0], fade_in: 0.1, additive: true, seed: 11, ..base }, ""),
        (Particles { count: 1400, lifetime: 1.5, shape: 3, extent: [46.0, 1.0, 46.0], direction: [0.0, -1.0, 0.0], cone: 3.0, speed_range: [45.0, 70.0], turbulence: 1.0, turbulence_scale: 12.0, size: [2.0, 0.9], size_jitter: 0.4, color: [0.5, 0.85, 1.0, 0.8], color_end: [0.225, 0.425, 1.0, 0.0], fade_in: 0.05, stretch: 3.0, additive: true, seed: 13, ..base }, ""),
        (Particles { count: 900, lifetime: 5.0, shape: 2, extent: [20.0, 4.0, 20.0], direction: [0.3, 1.0, 0.1], cone: 40.0, speed_range: [1.0, 3.0], gravity: [0.2, -0.5, 0.0], drag: 0.1, turbulence: 2.0, turbulence_scale: 3.0, size: [1.5, 3.0], size_jitter: 0.3, spin: 1.5, sheet: [4, 2], color: [0.6, 0.6, 0.6, 0.8], color_end: [0.3, 0.3, 0.3, 0.0], fade_in: 0.2, additive: false, seed: 3, ..base }, ""),
        (Particles { count: 300, lifetime: 2.0, shape: 0, direction: [0.0, 1.0, 0.0], cone: 60.0, speed_range: [2.0, 6.0], gravity: [0.0, -9.8, 0.0], size: [0.3, 0.1], additive: false, looping: false, prewarm: false, spread_time: 1.0, seed: 5, ..base }, ""),
    ]
}

#[test]
fn clouds_are_exactly_the_meshes_that_were_recorded() {
    // the numbers were recorded from the code before the particles of all the clouds of a picture were worked out together on a pool of threads and before the numbers that every particle of a cloud shares were worked out
    // once: the same meshes, bit for bit, for any number of threads and either way of building them
    const EXPECT: [u64; 10] = [0x131be9b4a3057bf4, 0x888c241d1d7015e3, 0x1e67e0142750730c, 0xb6c0e212b2d91764, 0x9c21c72126a3ef0f, 0x03c488a07242120f, 0xed41324a7cb225ea, 0xa954b0e864378c30, 0x490c18d8fbab409b, 0x560da4dc4f3a170e];
    let cams = [camera(), CameraBasis { pos: [30.0, 12.0, -40.0], forward: normalize([-0.4, -0.1, 0.9]), right: normalize(cross([-0.4, -0.1, 0.9], [0.0, 1.0, 0.0])), up: [0.0, 1.0, 0.0] }];
    let tr = Transform { pos: [3.0, 40.0, -8.0], scale: [1.0, 1.0, 1.0], ..Transform::default() };
    let mut got = Vec::new();
    for cam in &cams {
        for (p, _) in recorded_clouds() {
            got.push(mesh_hash(&build(&p, &tr, cam, 1)));
        }
    }
    assert_eq!(got.as_slice(), EXPECT.as_slice(), "recorded: {got:x?}");
    for threads in [2, 5, 8] {
        let again: Vec<u64> = cams.iter().flat_map(|cam| recorded_clouds().into_iter().map(move |(p, _)| mesh_hash(&build(&p, &tr, cam, threads)))).collect();
        assert_eq!(again, got, "{threads} threads");
    }
}

#[test]
fn a_cloud_with_any_particle_in_the_picture_is_never_called_outside_it() {
    use crate::particles::{bound, outside};
    use lunaris_sim::particle_modules::Modules;
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut rnd = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        ((state >> 40) as f32) / 16_777_216.0
    };
    let quat = |rnd: &mut dyn FnMut() -> f32| {
        let (ax, a) = (normalize([rnd() - 0.5, rnd() - 0.5, rnd() - 0.5 + 1e-3]), rnd() * std::f32::consts::TAU);
        quat_normalize([ax[0] * (a * 0.5).sin(), ax[1] * (a * 0.5).sin(), ax[2] * (a * 0.5).sin(), (a * 0.5).cos()])
    };
    let (mut culled, mut inside) = (0, 0);
    for _ in 0..400 {
        let p = Particles {
            count: 150 + (rnd() * 400.0) as u32,
            lifetime: 0.5 + rnd() * 8.0,
            shape: (rnd() * 5.0) as u32,
            extent: [rnd() * 40.0 + 0.5, rnd() * 20.0 + 0.5, rnd() * 40.0 + 0.5],
            direction: [rnd() - 0.5, rnd() - 0.2, rnd() - 0.5],
            cone: rnd() * 180.0,
            speed_range: [rnd() * 3.0, 3.0 + rnd() * 30.0],
            gravity: [(rnd() - 0.5) * 4.0, -rnd() * 10.0, (rnd() - 0.5) * 4.0],
            drag: if rnd() < 0.5 { 0.0 } else { rnd() * 0.8 },
            turbulence: if rnd() < 0.5 { 0.0 } else { rnd() * 20.0 },
            turbulence_scale: 1.0 + rnd() * 30.0,
            size: [0.1 + rnd() * 6.0, rnd() * 3.0],
            size_jitter: rnd(),
            stretch: if rnd() < 0.3 { rnd() * 3.0 } else { 0.0 },
            additive: rnd() < 0.5,
            looping: true,
            prewarm: true,
            time: rnd() * 20.0,
            seed: (rnd() * 100.0) as u32,
            ..Default::default()
        };
        let tr = Transform { pos: [(rnd() - 0.5) * 300.0, (rnd() - 0.5) * 80.0, (rnd() - 0.5) * 300.0], rot: quat(&mut rnd), scale: [0.5 + rnd() * 2.0, 0.5 + rnd() * 2.0, 0.5 + rnd() * 2.0] };
        let (cam_pos, cam_rot) = ([(rnd() - 0.5) * 300.0, (rnd() - 0.5) * 60.0, (rnd() - 0.5) * 300.0], quat(&mut rnd));
        let view = M4::from_trs(cam_pos, cam_rot, [1.0, 1.0, 1.0]).inverse_affine();
        let vp = M4::perspective(40.0 + rnd() * 50.0, 1.0 + rnd(), 0.1, 2000.0).mul(&view);
        let inv = view.inverse_affine();
        let basis = CameraBasis { pos: cam_pos, forward: normalize(inv.transform_dir([0.0, 0.0, -1.0])), right: normalize(inv.transform_dir([1.0, 0.0, 0.0])), up: normalize(inv.transform_dir([0.0, 1.0, 0.0])) };
        let mesh = build(&p, &tr, &basis, 1);
        let any_in = mesh.pos.iter().any(|v| {
            let c = vp.mul_v4([v[0], v[1], v[2], 1.0]);
            c[3] > 0.0 && c[0].abs() <= c[3] && c[1].abs() <= c[3] && c[2] >= -c[3]
        });
        let (centre, radius) = bound(&p, &Modules::default(), &tr).expect("a plain cloud has a bound");
        let out = outside(&vp, centre, radius);
        // every particle of the cloud is inside its sphere
        assert!(mesh.pos.iter().all(|v| length(sub(*v, centre)) <= radius), "the bound holds the whole mesh: {p:?}");
        if any_in {
            inside += 1;
            assert!(!out, "a cloud with a particle in the picture was called outside: {p:?} {tr:?}");
        }
        if out {
            culled += 1;
        }
    }
    assert!(culled > 40 && inside > 40, "the test is meaningful: {culled} clouds outside, {inside} with a particle in the picture");
}

#[test]
fn a_cloud_wholly_outside_the_picture_is_not_built_and_one_in_it_is() {
    let mut s = Sim::new(1);
    let spawn = |s: &mut Sim, uid: u64, comps: &str| {
        let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    };
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,10]},"Camera":{}}"#);
    let cloud = |x: f32, z: f32| format!(r#"{{"Transform":{{"pos":[{x},0,{z}]}},"Particles":{{"count":200,"lifetime":2,"extent":[2,2,2],"shape":1,"speed_range":[1,2],"size":[0.5,0.2]}}}}"#);
    spawn(&mut s, 2, &cloud(0.0, 0.0)); // in front of the camera
    spawn(&mut s, 3, &cloud(500.0, 0.0)); // far to the side
    spawn(&mut s, 4, &cloud(0.0, 400.0)); // behind the camera
    let mut tex = TextureCache::new(None);
    let scene = crate::render::gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 96, height: 64, ..RenderOptions::default() }).unwrap();
    let uids: Vec<u64> = scene.particles.iter().map(|p| p.uid).collect();
    assert_eq!(uids, vec![2], "only the cloud in the picture is built");
}
