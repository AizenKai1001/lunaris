//! Rotation in physics: spinning bodies, oriented boxes, capsules, ball-and-socket joints and hinges.
use super::*;
use components::{AngularVelocity, Joint, Transform, Velocity};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn rot(s: &Sim, uid: u64) -> [f32; 4] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().rot
}

fn lin(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Velocity>(s.world.entity(uid).unwrap()).unwrap().lin
}

fn spin(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<AngularVelocity>(s.world.entity(uid).unwrap()).unwrap().w
}

fn joint(s: &Sim, uid: u64) -> Joint {
    *s.world.get::<Joint>(s.world.entity(uid).unwrap()).unwrap()
}

fn qx(a: f32) -> [f32; 4] {
    [(a * 0.5).sin(), 0.0, 0.0, (a * 0.5).cos()]
}
fn qy(a: f32) -> [f32; 4] {
    [0.0, (a * 0.5).sin(), 0.0, (a * 0.5).cos()]
}
fn qz(a: f32) -> [f32; 4] {
    [0.0, 0.0, (a * 0.5).sin(), (a * 0.5).cos()]
}

/// The body's own axes in world coordinates.
fn axes(q: [f32; 4]) -> [[f32; 3]; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    [[1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)], [2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)], [2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)]]
}

/// How well one of the body's axes points straight up or down (1 = exactly).
fn level(q: [f32; 4]) -> f32 {
    let a = axes(q);
    a[0][1].abs().max(a[1][1].abs()).max(a[2][1].abs())
}

fn len(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    len([a[0] - b[0], a[1] - b[1], a[2] - b[2]])
}

/// A big static floor whose top surface is at y = 0.
fn floor(s: &mut Sim) {
    spawn(s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[200,1,200]},"RigidBody":{"friction":0.8,"restitution":0}}"#);
}

fn no_gravity(s: &mut Sim) {
    spawn(s, 900, r#"{"PhysicsSettings":{"gravity":[0,0,0]}}"#);
}

/// A dynamic body that turns: shape, size, position, orientation and the rest of the `RigidBody` as json fields.
fn body(s: &mut Sim, uid: u64, shape: u32, size: [f32; 3], p: [f32; 3], q: [f32; 4], rigid: &str, w: [f32; 3]) {
    spawn(
        s,
        uid,
        &format!(
            r#"{{"Transform":{{"pos":{p:?},"rot":{q:?}}},"Velocity":{{}},"AngularVelocity":{{"w":{w:?},"damping":0}},"Collider":{{"shape":{shape},"size":{size:?}}},"RigidBody":{rigid}}}"#
        ),
    );
}

#[test]
fn a_ball_pushed_along_the_floor_starts_to_roll_and_ends_rolling_without_slipping() {
    let mut s = Sim::new(1);
    floor(&mut s);
    body(&mut s, 2, 0, [1.0, 1.0, 1.0], [-20.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":2,"friction":0.5,"restitution":0}"#, [0.0; 3]);
    s.apply(&Command::Set { uid: 2, component: "Velocity".into(), value: parse(r#"{"lin":[7,0,0]}"#).unwrap() }).unwrap();
    s.step_n(240);
    // a solid ball: friction takes it from 7 m/s sliding to 5 m/s (5/7) rolling, spinning at v / r = 10 rad/s
    let (v, w) = (lin(&s, 2), spin(&s, 2));
    assert!((v[0] - 5.0).abs() < 0.2, "rolling speed {v:?}");
    assert!((w[2] + 10.0).abs() < 0.5, "spin {w:?}: rolling forward turns clockwise about z");
    assert!(w[0].abs() < 0.05 && w[1].abs() < 0.05 && v[2].abs() < 0.01, "stays straight: {w:?} {v:?}");
    assert!((pos(&s, 2)[1] - 0.5).abs() < 0.01, "stays on the floor: {:?}", pos(&s, 2));
}

#[test]
fn a_ball_rolls_down_a_ramp_at_five_sevenths_of_the_sliding_acceleration() {
    let mut s = Sim::new(1);
    // a ramp (an oriented static box) tilted 20 degrees so that it runs down toward +x
    let a = -20.0f32.to_radians();
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,0],"rot":{:?}}},"Collider":{{"shape":1,"size":[60,1,10]}},"RigidBody":{{"friction":1.0,"restitution":0}}}}"#, qz(a)));
    // a ball of radius 0.5 resting on it 20 m up the slope: its centre is 1 m above the ramp's centre line
    let (lx, ly) = (-20.0f32, 1.0f32);
    let p = [lx * a.cos() - ly * a.sin(), lx * a.sin() + ly * a.cos() + 0.002, 0.0];
    body(&mut s, 2, 0, [1.0; 3], p, [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":1.0,"restitution":0}"#, [0.0; 3]);
    s.step_n(180);
    let (v, w) = (lin(&s, 2), spin(&s, 2));
    let speed = (v[0] * v[0] + v[1] * v[1]).sqrt();
    let want = 5.0 / 7.0 * 9.8 * 20.0f32.to_radians().sin() * 2.9; // (about 3 s of acceleration, less the first ticks)
    assert!(speed > 0.9 * want && speed < 1.06 * want, "speed {speed} against {want}");
    assert!((w[2].abs() * 0.5 - speed).abs() < 0.06 * speed, "rolls without slipping: spin {w:?}, speed {speed}");
}

#[test]
fn a_cube_dropped_on_a_corner_tips_over_and_comes_to_rest_on_a_face() {
    let mut s = Sim::new(1);
    floor(&mut s);
    body(&mut s, 2, 1, [1.0; 3], [0.0, 2.0, 0.0], qz(35.0f32.to_radians()), r#"{"mass":1,"friction":0.6,"restitution":0.1}"#, [0.0; 3]);
    s.step_n(60 * 8);
    let (p, q) = (pos(&s, 2), rot(&s, 2));
    assert!((p[1] - 0.5).abs() < 0.02, "lies flat on a face: {p:?}");
    assert!(level(q) > 0.995, "one face is down: {q:?}");
    assert!(len(lin(&s, 2)) < 0.1 && len(spin(&s, 2)) < 0.3, "at rest: {:?} {:?}", lin(&s, 2), spin(&s, 2));
}

#[test]
fn a_tall_box_stands_until_it_is_pushed_over_and_then_lies_on_its_side() {
    let size = [0.2, 1.0, 0.6];
    let mut s = Sim::new(1);
    floor(&mut s);
    body(&mut s, 2, 1, size, [0.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.6,"restitution":0}"#, [0.0; 3]);
    s.step_n(60 * 6);
    let q = rot(&s, 2);
    assert!(axes(q)[1][1] > 0.999 && (pos(&s, 2)[1] - 0.5).abs() < 0.01, "it stands: {:?} {:?}", pos(&s, 2), q);
    assert!(len(lin(&s, 2)) < 0.05 && len(spin(&s, 2)) < 0.1, "still: {:?} {:?}", lin(&s, 2), spin(&s, 2));
    // the same box with a little spin toward +x (clockwise about z) falls over
    let mut t = Sim::new(1);
    floor(&mut t);
    body(&mut t, 2, 1, size, [0.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.6,"restitution":0}"#, [0.0, 0.0, -4.0]);
    t.step_n(60 * 6);
    let (p, q) = (pos(&t, 2), rot(&t, 2));
    assert!(p[1] < 0.15 && axes(q)[1][1].abs() < 0.05, "it lies on its side: {p:?} {q:?}");
    assert!(p[0] > 0.3, "and fell toward +x: {p:?}");
}

#[test]
fn a_row_of_dominoes_falls_one_after_the_other() {
    let mut s = Sim::new(1);
    floor(&mut s);
    for k in 0..4u64 {
        let w = if k == 0 { [0.0, 0.0, -4.0] } else { [0.0; 3] };
        body(&mut s, 2 + k, 1, [0.2, 1.0, 0.6], [0.6 * k as f32, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.6,"restitution":0.05}"#, w);
    }
    s.step_n(60 * 5);
    for k in 0..4u64 {
        // each one has fallen toward +x (clockwise about z: a negative angle) as far as the next one lets it
        let (p, q) = (pos(&s, 2 + k), rot(&s, 2 + k));
        let angle = 2.0 * q[2].atan2(q[3]).to_degrees();
        assert!(angle < -60.0 && p[1] < 0.3, "domino {k} fell: {p:?} {angle} degrees");
        assert!(len(lin(&s, 2 + k)) < 0.05, "and lies still: {:?}", lin(&s, 2 + k));
    }
    // the last one has nothing to lean on: it is flat on the floor
    assert!(pos(&s, 5)[1] < 0.15, "{:?}", pos(&s, 5));
    // while without the push nothing moves
    let mut t = Sim::new(1);
    floor(&mut t);
    for k in 0..4u64 {
        body(&mut t, 2 + k, 1, [0.2, 1.0, 0.6], [0.6 * k as f32, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.6,"restitution":0.05}"#, [0.0; 3]);
    }
    t.step_n(60 * 5);
    for k in 0..4u64 {
        assert!((pos(&t, 2 + k)[1] - 0.5).abs() < 0.01 && level(rot(&t, 2 + k)) > 0.999, "domino {k} stands: {:?}", pos(&t, 2 + k));
    }
}

#[test]
fn capsules_lie_down_and_stand_up() {
    let mut s = Sim::new(1);
    floor(&mut s);
    // radius 0.25, 2 m long in all
    body(&mut s, 2, 2, [0.5, 2.0, 0.5], [0.0, 1.5, 0.0], qz(70.0f32.to_radians()), r#"{"mass":1,"friction":0.5,"restitution":0}"#, [0.0; 3]);
    body(&mut s, 3, 2, [0.5, 2.0, 0.5], [10.0, 1.01, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.5,"restitution":0}"#, [0.0; 3]);
    s.step_n(60 * 6);
    let (p, q) = (pos(&s, 2), rot(&s, 2));
    assert!((p[1] - 0.25).abs() < 0.02 && axes(q)[1][1].abs() < 0.05, "the tilted one lies down: {p:?} {q:?}");
    assert!(len(lin(&s, 2)) < 0.1 && len(spin(&s, 2)) < 0.5, "at rest: {:?} {:?}", lin(&s, 2), spin(&s, 2));
    let (p, q) = (pos(&s, 3), rot(&s, 3));
    assert!((p[1] - 1.0).abs() < 0.02 && axes(q)[1][1] > 0.99, "the upright one stands: {p:?} {q:?}");
}

#[test]
fn a_box_on_a_slope_holds_with_enough_friction_and_slides_without() {
    let slope = |mu: f32| {
        let mut s = Sim::new(1);
        let a = -20.0f32.to_radians();
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,0],"rot":{:?}}},"Collider":{{"shape":1,"size":[60,1,20]}},"RigidBody":{{"friction":{mu},"restitution":0}}}}"#, qz(a)));
        // a box of 1 m on the ramp, 5 m up the slope from the middle
        let (lx, ly) = (-5.0f32, 1.0f32);
        let p = [lx * a.cos() - ly * a.sin(), lx * a.sin() + ly * a.cos() + 0.002, 0.0];
        body(&mut s, 2, 1, [1.0; 3], p, qz(a), &format!(r#"{{"mass":1,"friction":{mu},"restitution":0}}"#), [0.0; 3]);
        s.step_n(60);
        (s.world.get::<Velocity>(s.world.entity(2).unwrap()).unwrap().lin, pos(&s, 2), rot(&s, 2), qz(a))
    };
    let (v, p, q, q0) = slope(0.8);
    let a = -20.0f32.to_radians();
    let start = [-5.0 * a.cos() - a.sin(), -5.0 * a.sin() + a.cos(), 0.0];
    assert!(len(v) < 0.1 && dist(p, start) < 0.15, "it holds: {v:?} {p:?} (started at {start:?})");
    assert!((q[2] - q0[2]).abs() < 0.01, "and does not tip: {q:?}");
    let (v, _, q, q0) = slope(0.1);
    // g (sin 20 - 0.1 cos 20) = 2.4 m/s^2, for one second
    assert!(v[0] > 1.9 && v[0] < 2.7, "it slides: {v:?}");
    assert!((q[2] - q0[2]).abs() < 0.02, "flat on the slope: {q:?}");
}

#[test]
fn a_stack_of_turning_boxes_stays_a_stack() {
    let mut s = Sim::new(1);
    floor(&mut s);
    for i in 0..4u64 {
        body(&mut s, 2 + i, 1, [1.0; 3], [0.0, 0.5 + i as f32 * 1.002, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.7,"restitution":0}"#, [0.0; 3]);
    }
    s.step_n(60 * 6);
    for i in 0..4u64 {
        let p = pos(&s, 2 + i);
        assert!(p[0].abs() < 0.05 && p[2].abs() < 0.05 && (p[1] - (0.5 + i as f32)).abs() < 0.06, "box {i} at {p:?}");
        assert!(level(rot(&s, 2 + i)) > 0.999 && len(spin(&s, 2 + i)) < 0.2, "box {i} is still: {:?}", spin(&s, 2 + i));
    }
}

#[test]
fn a_tumbling_box_in_space_keeps_its_angular_momentum() {
    let mut s = Sim::new(1);
    no_gravity(&mut s);
    body(&mut s, 2, 1, [1.0, 2.0, 0.4], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":3}"#, [2.0, 0.3, 1.5]);
    // the moments of inertia about the box's own axes
    let inertia = [3.0 / 12.0 * (4.0 + 0.16), 3.0 / 12.0 * (1.0 + 0.16), 3.0 / 12.0 * (1.0 + 4.0)];
    let momentum = |s: &Sim| {
        let a = axes(rot(s, 2));
        let w = spin(s, 2);
        let mut l = [0.0f32; 3];
        for k in 0..3 {
            let wk = w[0] * a[k][0] + w[1] * a[k][1] + w[2] * a[k][2];
            for c in 0..3 {
                l[c] += inertia[k] * wk * a[k][c];
            }
        }
        l
    };
    let l0 = len(momentum(&s));
    let mut turned = [0.0f32; 3];
    for _ in 0..600 {
        s.step();
        let w = spin(&s, 2);
        for c in 0..3 {
            turned[c] += w[c].abs();
        }
    }
    let l1 = len(momentum(&s));
    assert!((l1 - l0).abs() < 0.1 * l0, "angular momentum {l0} -> {l1}");
    assert!(len(pos(&s, 2)) < 1e-3, "no force, no drift: {:?}", pos(&s, 2));
    let q = rot(&s, 2);
    assert!((q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3] - 1.0).abs() < 1e-4, "the orientation stays a unit quaternion: {q:?}");
}

/// Where the joint's pivot is on body `a`, in the world.
fn pivot_a(s: &Sim, uid: u64) -> [f32; 3] {
    let (j, a, p) = (joint(s, uid), axes(rot(s, uid)), pos(s, uid));
    let l = j.local_a;
    [p[0] + a[0][0] * l[0] + a[1][0] * l[1] + a[2][0] * l[2], p[1] + a[0][1] * l[0] + a[1][1] * l[1] + a[2][1] * l[2], p[2] + a[0][2] * l[0] + a[1][2] * l[1] + a[2][2] * l[2]]
}

#[test]
fn a_ball_and_socket_swings_a_body_about_a_point_that_is_not_its_centre() {
    let mut s = Sim::new(1);
    // a 3 m plank hung by one end (at the origin) from the world, let go horizontal
    body(&mut s, 2, 1, [3.0, 0.2, 0.2], [1.5, 10.0, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":2,"restitution":0}"#, [0.0; 3]);
    s.apply(&Command::Add { uid: 2, component: "Joint".into(), value: parse(r#"{"kind":3,"anchor":[0,10,0]}"#).unwrap() }).unwrap();
    let mut lowest = 99.0f32;
    for _ in 0..600 {
        s.step();
        let pv = pivot_a(&s, 2);
        assert!(dist(pv, [0.0, 10.0, 0.0]) < 0.05, "the pivot stays at the anchor: {pv:?}");
        lowest = lowest.min(pos(&s, 2)[1]);
    }
    assert!(joint(&s, 2).rest != [0.0; 4], "the joint was bound");
    // the plank swings down like a pendulum: its centre is 1.5 m from the pivot
    assert!(lowest < 8.6 && lowest > 8.4, "lowest point of the centre {lowest}");
    // and (damping aside) comes back up to nearly where it started
    assert!(pos(&s, 2)[1] > 8.0);
}

#[test]
fn a_hinge_lets_a_door_turn_about_its_axis_and_no_other() {
    let mut s = Sim::new(1);
    // a door 1 m wide, 2 m high, hinged on its left edge at the origin about the vertical axis, given a push
    body(&mut s, 2, 1, [1.0, 2.0, 0.1], [0.5, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":5,"restitution":0}"#, [0.0, 3.0, 0.0]);
    s.apply(&Command::Add { uid: 2, component: "Joint".into(), value: parse(r#"{"kind":4,"anchor":[0,1,0],"axis":[0,1,0]}"#).unwrap() }).unwrap();
    for _ in 0..300 {
        s.step();
        let (p, q) = (pos(&s, 2), rot(&s, 2));
        assert!(dist([p[0], 0.0, p[2]], [0.0; 3]) > 0.45 && dist([p[0], 0.0, p[2]], [0.0; 3]) < 0.55 && (p[1] - 1.0).abs() < 0.03, "the centre goes round the hinge: {p:?}");
        assert!(q[0].abs() < 0.02 && q[2].abs() < 0.02, "only turns about y: {q:?}");
    }
    assert!(rot(&s, 2)[1].abs() > 0.3, "it turned: {:?}", rot(&s, 2));
}

#[test]
fn a_hinge_with_limits_stops_the_swing() {
    let mut s = Sim::new(1);
    // an arm 2 m long hinged at the origin about the z axis, let go horizontal: it swings down, but only 0.6 radians
    body(&mut s, 2, 1, [2.0, 0.2, 0.2], [1.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"restitution":0}"#, [0.0; 3]);
    s.apply(&Command::Add { uid: 2, component: "Joint".into(), value: parse(r#"{"kind":4,"anchor":[0,5,0],"axis":[0,0,1],"limit":[-0.6,0.8]}"#).unwrap() }).unwrap();
    let mut lowest = 0.0f32;
    for _ in 0..600 {
        s.step();
        let q = rot(&s, 2);
        let angle = 2.0 * q[2].atan2(q[3]);
        lowest = lowest.min(angle);
        assert!(angle > -0.68, "the limit holds: {angle}");
    }
    assert!(lowest < -0.55, "it did swing down to the limit: {lowest}");
    let q = rot(&s, 2);
    let angle = 2.0 * q[2].atan2(q[3]);
    assert!((angle + 0.6).abs() < 0.06 && len(spin(&s, 2)) < 0.3, "and hangs against it: {angle} {:?}", spin(&s, 2));
}

#[test]
fn a_chain_of_linked_capsules_hangs_together() {
    let mut s = Sim::new(1);
    // three capsules (0.3 m across, 1 m long) end to end under a hook, ball joints between them
    for k in 0..3u64 {
        body(&mut s, 10 + k, 2, [0.3, 1.0, 0.3], [0.0, 9.5 - k as f32, 0.0], qz(0.3), r#"{"mass":1,"restitution":0}"#, [0.0; 3]);
    }
    // (they start tilted a little so that the chain swings)
    for k in 0..3u64 {
        let target = if k == 0 { 0 } else { 9 + k };
        let anchor = 10.0 - k as f32;
        s.apply(&Command::Add { uid: 10 + k, component: "Joint".into(), value: parse(&format!(r#"{{"kind":3,"target":{target},"anchor":[0,{anchor},0]}}"#)).unwrap() }).unwrap();
    }
    for _ in 0..300 {
        s.step();
        for k in 0..3u64 {
            let pv = pivot_a(&s, 10 + k);
            let other = if k == 0 { [0.0, 10.0, 0.0] } else { pos(&s, 9 + k) };
            // (the far end of link k - 1 is not at its centre: compare the two pivots instead)
            let _ = other;
            assert!(pv.iter().all(|v| v.is_finite()), "{pv:?}");
        }
        let (j1, j2) = (joint(&s, 11), joint(&s, 12));
        let (pb1, pb2) = (pos(&s, 10), pos(&s, 11));
        let (a0, a1) = (axes(rot(&s, 10)), axes(rot(&s, 11)));
        let on = |p: [f32; 3], a: [[f32; 3]; 3], l: [f32; 3]| [p[0] + a[0][0] * l[0] + a[1][0] * l[1] + a[2][0] * l[2], p[1] + a[0][1] * l[0] + a[1][1] * l[1] + a[2][1] * l[2], p[2] + a[0][2] * l[0] + a[1][2] * l[1] + a[2][2] * l[2]];
        assert!(dist(on(pos(&s, 11), a1, j1.local_a), on(pb1, a0, j1.local_b)) < 0.06, "link 1 stays on link 0");
        assert!(dist(on(pos(&s, 12), axes(rot(&s, 12)), j2.local_a), on(pb2, a1, j2.local_b)) < 0.06, "link 2 stays on link 1");
    }
    // they hang down, not in a heap
    assert!(pos(&s, 12)[1] < 8.0 && pos(&s, 12)[1] > 6.4, "{:?}", pos(&s, 12));
}

/// Two bodies that overlap, gravity off: the first contact the step reports (depth and normal).
fn overlap(a: (u32, [f32; 3], [f32; 3], [f32; 4]), b: (u32, [f32; 3], [f32; 3], [f32; 4])) -> (Sim, Option<(f32, [f32; 3])>) {
    let mut s = Sim::new(1);
    no_gravity(&mut s);
    body(&mut s, 1, a.0, a.1, a.2, a.3, r#"{"mass":1,"restitution":0}"#, [0.0; 3]);
    body(&mut s, 2, b.0, b.1, b.2, b.3, r#"{"mass":1,"restitution":0}"#, [0.0; 3]);
    s.step();
    let c = s.contacts.iter().find(|c| c.a == 1 && c.b == 2).map(|c| (c.depth, c.normal));
    (s, c)
}

const ID: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

#[test]
fn oriented_boxes_overlap_face_to_face_and_edge_to_edge() {
    // a cube turned about y and a cube on top of it, 0.1 deep
    let (_, c) = overlap((1, [2.0; 3], [0.0, 0.0, 0.0], qy(0.7)), (1, [2.0; 3], [0.0, 1.9, 0.0], ID));
    let (depth, n) = c.expect("overlap");
    assert!((depth - 0.1).abs() < 1e-3 && n[1] > 0.999, "{depth} {n:?}");
    // the other way round: the normal still points from the first to the second
    let (_, c) = overlap((1, [2.0; 3], [0.0, 1.9, 0.0], ID), (1, [2.0; 3], [0.0, 0.0, 0.0], qy(0.7)));
    let (depth, n) = c.expect("overlap");
    assert!((depth - 0.1).abs() < 1e-3 && n[1] < -0.999, "{depth} {n:?}");
    // two cubes meeting edge to edge: one turned 45 degrees about x, the other 45 degrees about z, one above the other
    let (_, c) = overlap((1, [2.0; 3], [0.0, 0.0, 0.0], qx(std::f32::consts::FRAC_PI_4)), (1, [2.0; 3], [0.0, 2.7, 0.0], qz(std::f32::consts::FRAC_PI_4)));
    let (depth, n) = c.expect("edge to edge");
    // the top edge of the first is at height 1.414 and the bottom edge of the second at 2.7 - 1.414: 0.128 deep
    assert!((depth - 0.128).abs() < 0.01 && n[1] > 0.99, "{depth} {n:?}");
    // apart
    assert!(overlap((1, [2.0; 3], [0.0, 0.0, 0.0], qy(0.7)), (1, [2.0; 3], [0.0, 2.2, 0.0], qy(0.3))).1.is_none());
}

#[test]
fn spheres_and_capsules_meet_boxes_and_each_other() {
    // a ball against a turned box: the box's face is at 1 along its own x axis, turned 45 degrees about z
    let r2 = std::f32::consts::FRAC_1_SQRT_2;
    // (the face of the diamond is the line x + y = sqrt 2; the ball's centre is 0.414 from it, its radius 0.5)
    let (_, c) = overlap((1, [2.0; 3], [0.0; 3], qz(std::f32::consts::FRAC_PI_4)), (0, [1.0; 3], [1.0, 1.0, 0.0], ID));
    let (depth, n) = c.expect("ball on a turned box");
    assert!((depth - 0.0858).abs() < 0.002 && (n[0] - r2).abs() < 0.01 && (n[1] - r2).abs() < 0.01, "{depth} {n:?}");
    // two capsules crossing one above the other: 0.25 + 0.25 radius, centres 0.4 apart
    let (_, c) = overlap((2, [0.5, 2.0, 0.5], [0.0, 0.0, 0.0], qz(std::f32::consts::FRAC_PI_2)), (2, [0.5, 2.0, 0.5], [0.0, 0.4, 0.0], qx(std::f32::consts::FRAC_PI_2)));
    let (depth, n) = c.expect("crossing capsules");
    assert!((depth - 0.1).abs() < 1e-3 && n[1] > 0.999, "{depth} {n:?}");
    // a capsule standing on a ball
    let (_, c) = overlap((0, [1.0; 3], [0.0; 3], ID), (2, [0.5, 2.0, 0.5], [0.0, 1.4, 0.0], ID));
    let (depth, n) = c.expect("capsule on a ball");
    assert!((depth - 0.1).abs() < 1e-3 && n[1] > 0.999, "{depth} {n:?}");
    // a capsule lying across the top face of a box
    let (_, c) = overlap((1, [4.0, 1.0, 4.0], [0.0; 3], ID), (2, [0.5, 2.0, 0.5], [0.0, 0.7, 0.0], qz(std::f32::consts::FRAC_PI_2)));
    let (depth, n) = c.expect("capsule on a box");
    assert!((depth - 0.05).abs() < 1e-3 && n[1] > 0.999, "{depth} {n:?}");
    // apart
    assert!(overlap((2, [0.5, 2.0, 0.5], [0.0; 3], ID), (2, [0.5, 2.0, 0.5], [0.0, 0.0, 1.0], ID)).1.is_none());
}

#[test]
fn bodies_without_an_angular_velocity_and_boxes_that_are_not_turned_do_not_rotate() {
    let mut s = Sim::new(1);
    floor(&mut s);
    // a box that lands on one edge, without an AngularVelocity: it never turns (as before)
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,2,0]},"Velocity":{"lin":[1,0,0]},"Collider":{"shape":1,"size":[1,1,1]},"RigidBody":{"mass":1}}"#);
    s.step_n(240);
    assert_eq!(rot(&s, 2), [0.0, 0.0, 0.0, 1.0]);
    assert!((pos(&s, 2)[1] - 0.5).abs() < 0.02);
}

#[test]
fn the_same_world_gives_the_same_bits_whatever_the_order_and_thread_count() {
    let build = |reverse: bool, threads: usize| {
        let mut s = Sim::new(5);
        s.threads = threads;
        floor(&mut s);
        let mut uids: Vec<u64> = (0..30).collect();
        if reverse {
            uids.reverse();
        }
        for i in uids {
            let (gx, gz, gy) = ((i % 5) as f32 * 1.6 - 3.0, ((i / 5) % 3) as f32 * 1.6 - 1.6, 1.0 + (i / 15) as f32 * 1.7);
            let shape = [0u32, 1, 2][(i % 3) as usize];
            let size = [[1.0, 1.0, 1.0], [0.9, 0.5, 1.2], [0.5, 1.6, 0.5]][(i % 3) as usize];
            body(&mut s, 100 + i, shape, size, [gx, gy, gz], qz(0.1 * i as f32), r#"{"mass":1,"restitution":0.2,"friction":0.5}"#, [(i % 4) as f32 - 1.5, 0.5, (i % 3) as f32 - 1.0]);
        }
        // a hinged door and a ball-and-socket chain among them
        body(&mut s, 200, 1, [1.0, 2.0, 0.1], [8.5, 1.0, 0.0], ID, r#"{"mass":4}"#, [0.0, 2.0, 0.0]);
        s.apply(&Command::Add { uid: 200, component: "Joint".into(), value: parse(r#"{"kind":4,"anchor":[8,1,0],"axis":[0,1,0],"limit":[-1,1.2]}"#).unwrap() }).unwrap();
        s.step_n(300);
        s
    };
    let a = build(false, 1);
    assert_eq!(a.state_hash(), build(true, 1).state_hash(), "spawn order changed the result");
    assert_eq!(a.state_hash(), build(false, 4).state_hash(), "thread count changed the result");
    // a snapshot continues exactly like the original
    let mut b = build(false, 1);
    let snapshot = b.clone();
    b.step_n(120);
    let mut c = snapshot;
    c.step_n(120);
    assert_eq!(b.state_hash(), c.state_hash());
    for i in 0..30u64 {
        let p = pos(&a, 100 + i);
        assert!(p.iter().all(|v| v.is_finite()) && p[1] > -0.2 && p[1] < 6.0, "body {i} at {p:?}");
    }
}

#[test]
fn a_double_pendulum_on_two_hinges_swings_in_one_plane_and_a_slanted_hinge_keeps_its_axis() {
    let mut s = Sim::new(1);
    // two arms, 2 m each, hinged about the z axis: the first to the world at the origin, the second to the end of the first
    body(&mut s, 2, 1, [2.0, 0.2, 0.2], [1.0, 10.0, 0.0], ID, r#"{"mass":1,"restitution":0}"#, [0.0; 3]);
    body(&mut s, 3, 1, [2.0, 0.2, 0.2], [3.0, 10.0, 0.0], ID, r#"{"mass":1,"restitution":0}"#, [0.0; 3]);
    s.apply(&Command::Add { uid: 2, component: "Joint".into(), value: parse(r#"{"kind":4,"anchor":[0,10,0],"axis":[0,0,1]}"#).unwrap() }).unwrap();
    s.apply(&Command::Add { uid: 3, component: "Joint".into(), value: parse(r#"{"kind":4,"target":2,"anchor":[2,10,0],"axis":[0,0,1]}"#).unwrap() }).unwrap();
    for _ in 0..600 {
        s.step();
        for uid in [2u64, 3] {
            let q = rot(&s, uid);
            assert!(q[0].abs() < 0.02 && q[1].abs() < 0.02, "arm {uid} turns about z only: {q:?}");
            assert!(pos(&s, uid)[2].abs() < 0.02, "and stays in its plane: {:?}", pos(&s, uid));
        }
        let (j, a, b) = (joint(&s, 3), axes(rot(&s, 3)), axes(rot(&s, 2)));
        let (pa, pb) = (pos(&s, 3), pos(&s, 2));
        let on = |p: [f32; 3], m: [[f32; 3]; 3], l: [f32; 3]| [p[0] + m[0][0] * l[0] + m[1][0] * l[1] + m[2][0] * l[2], p[1] + m[0][1] * l[0] + m[1][1] * l[1] + m[2][1] * l[2], p[2] + m[0][2] * l[0] + m[1][2] * l[1] + m[2][2] * l[2]];
        assert!(dist(on(pa, a, j.local_a), on(pb, b, j.local_b)) < 0.06, "the second arm stays on the end of the first");
    }
    // a hinge about a slanted axis: the body only ever turns about that axis
    let mut t = Sim::new(1);
    let r2 = std::f32::consts::FRAC_1_SQRT_2;
    body(&mut t, 2, 1, [2.0, 0.2, 0.2], [1.0, 10.0, 0.0], ID, r#"{"mass":1,"restitution":0}"#, [0.0; 3]);
    t.apply(&Command::Add { uid: 2, component: "Joint".into(), value: parse(&format!(r#"{{"kind":4,"anchor":[0,10,0],"axis":[{r2},0,{r2}]}}"#)).unwrap() }).unwrap();
    t.step_n(240);
    let q = rot(&t, 2);
    let along = q[0] * r2 + q[2] * r2;
    let across = [q[0] - along * r2, q[1], q[2] - along * r2];
    assert!(along.abs() > 0.1, "it swung: {q:?}");
    assert!(len(across) < 0.03, "about its axis and no other: {q:?}");
    // the joint can be tied down again from its anchor: the body is moved, `rest` zeroed, and the pivot is where `anchor` says
    t.apply(&Command::Set { uid: 2, component: "Joint".into(), value: parse(r#"{"anchor":[0,10,5],"rest":[0,0,0,0]}"#).unwrap() }).unwrap();
    t.apply(&Command::Set { uid: 2, component: "Transform".into(), value: parse(r#"{"pos":[1,10,5],"rot":[0,0,0,1]}"#).unwrap() }).unwrap();
    t.step_n(10);
    assert!(dist(pivot_a(&t, 2), [0.0, 10.0, 5.0]) < 0.05, "{:?}", pivot_a(&t, 2));
}

#[test]
fn a_spinning_ball_bounces_as_high_as_its_restitution_says_and_gravity_can_point_any_way() {
    let mut s = Sim::new(1);
    floor(&mut s);
    body(&mut s, 2, 0, [1.0; 3], [0.0, 5.5, 0.0], ID, r#"{"mass":1,"restitution":0.8,"friction":0.5}"#, [0.0; 3]);
    let mut hit = false;
    let mut peak = 0.0f32;
    for _ in 0..300 {
        s.step();
        hit |= lin(&s, 2)[1] > 0.5;
        if hit {
            peak = peak.max(pos(&s, 2)[1]);
        }
    }
    assert!((peak - 0.5 - 0.64 * 5.0).abs() < 0.5, "e = 0.8 peaked at {peak}");
    // gravity along +x: a box rests against a wall at x = 4.5 (centre 4.0), still and level
    let mut t = Sim::new(1);
    spawn(&mut t, 900, r#"{"PhysicsSettings":{"gravity":[9.8,0,0]}}"#);
    spawn(&mut t, 1, r#"{"Transform":{"pos":[5,0,0]},"Collider":{"shape":1,"size":[1,40,40]},"RigidBody":{"friction":0.8}}"#);
    body(&mut t, 2, 1, [1.0; 3], [3.0, 0.0, 0.0], ID, r#"{"mass":1,"friction":0.6,"restitution":0}"#, [0.0; 3]);
    t.step_n(300);
    let (p, q) = (pos(&t, 2), rot(&t, 2));
    assert!((p[0] - 4.0).abs() < 0.02 && p[1].abs() < 0.05 && p[2].abs() < 0.05, "{p:?}");
    assert!(q[0].abs() < 0.01 && q[1].abs() < 0.01 && q[2].abs() < 0.01 && len(spin(&t, 2)) < 0.05, "{q:?} {:?}", spin(&t, 2));
}

/// A unit quaternion from three numbers in 0..1 (uniform random orientations).
fn random_orientation(u: [f32; 3]) -> [f32; 4] {
    let tau = std::f32::consts::TAU;
    let (a, b) = ((1.0 - u[0]).sqrt(), u[0].sqrt());
    [a * (tau * u[1]).sin(), a * (tau * u[1]).cos(), b * (tau * u[2]).sin(), b * (tau * u[2]).cos()]
}

#[test]
fn bodies_dropped_in_any_orientation_onto_slabs_never_sink_in_or_explode() {
    let mut seed = 0x9e37_79b9u32;
    let mut next = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1 << 24) as f32
    };
    let mut s = Sim::new(1);
    floor(&mut s);
    // two slabs lying every which way in the middle of the floor
    spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,1.0,0],"rot":{:?}}},"Collider":{{"shape":1,"size":[5,0.4,3]}}}}"#, random_orientation([0.31, 0.12, 0.7])));
    spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":[0.5,2.2,0.3],"rot":{:?}}},"Collider":{{"shape":1,"size":[1.5,2.5,0.3]}}}}"#, random_orientation([0.8, 0.5, 0.2])));
    let mut half = std::collections::BTreeMap::new();
    for i in 0..48u64 {
        let shape = (i % 3) as u32;
        let size = match shape {
            0 => [0.3 + 0.5 * next(); 3],
            1 => [0.3 + 0.8 * next(), 0.2 + 0.8 * next(), 0.3 + 0.8 * next()],
            _ => [0.2 + 0.3 * next(), 0.8 + 0.8 * next(), 0.3],
        };
        let p = [-2.5 + 5.0 * next(), 2.0 + 6.0 * next(), -2.0 + 4.0 * next()];
        body(&mut s, 100 + i, shape, size, p, random_orientation([next(), next(), next()]), r#"{"mass":1,"restitution":0.2,"friction":0.5}"#, [4.0 * next() - 2.0, 4.0 * next() - 2.0, 4.0 * next() - 2.0]);
        half.insert(100 + i, (shape, size));
    }
    // two bodies spawned inside each other, at the same place
    body(&mut s, 200, 1, [1.0; 3], [6.0, 3.0, 0.0], qy(0.4), r#"{"mass":1}"#, [0.0; 3]);
    body(&mut s, 201, 1, [1.0; 3], [6.0, 3.0, 0.0], qy(0.4), r#"{"mass":1}"#, [0.0; 3]);
    body(&mut s, 202, 2, [0.5, 2.0, 0.5], [6.0, 3.0, 0.0], ID, r#"{"mass":1}"#, [0.0; 3]);
    for tick in 0..720 {
        s.step();
        if tick % 60 == 59 {
            for (&uid, &(shape, size)) in &half {
                let (p, a) = (pos(&s, uid), axes(rot(&s, uid)));
                assert!(p.iter().all(|v| v.is_finite()), "body {uid} at {p:?} (tick {tick})");
                // how far down the body reaches below its centre
                let down = match shape {
                    0 => size[0] * 0.5,
                    1 => 0.5 * (size[0] * a[0][1].abs() + size[1] * a[1][1].abs() + size[2] * a[2][1].abs()),
                    _ => (0.5 * size[1] - 0.5 * size[0]) * a[1][1].abs() + 0.5 * size[0],
                };
                assert!(p[1] - down > -0.12, "body {uid} sank into the floor: {p:?} reaches {down} down (tick {tick})");
                assert!(len(lin(&s, uid)) < 40.0 && len(spin(&s, uid)) <= 60.001, "body {uid} is going wild: {:?} {:?}", lin(&s, uid), spin(&s, uid));
            }
        }
    }
    let ids: Vec<u64> = half.keys().copied().chain([200, 201, 202]).collect();
    for uid in ids {
        let (p, q) = (pos(&s, uid), rot(&s, uid));
        assert!(p.iter().all(|v| v.is_finite()) && (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3] - 1.0).abs() < 1e-3, "body {uid} {p:?} {q:?}");
    }
}

// ------------------------------------------------------------------------------------------------ fast balls

fn thin_wall(s: &mut Sim) {
    // a wall 5 cm thick at x = 10, 20 m high and wide
    spawn(s, 3, r#"{"Transform":{"pos":[10,10,0]},"Collider":{"shape":1,"size":[0.05,20,20]},"RigidBody":{"restitution":1,"friction":0}}"#);
}

fn shot(s: &mut Sim, spinning: bool, speed: f32) {
    let spin = if spinning { r#""AngularVelocity":{"damping":0},"# } else { "" };
    spawn(
        s,
        2,
        &format!(r#"{{"Transform":{{"pos":[0,10,0]}},"Velocity":{{"lin":[{speed},0,0]}},{spin}"Collider":{{"shape":0,"size":[0.2,0.2,0.2]}},"RigidBody":{{"mass":0.1,"restitution":1,"friction":0,"gravity_scale":0}}}}"#),
    );
}

#[test]
fn a_fast_spinning_ball_cannot_pass_through_a_thin_wall_and_bounces_off_it_at_its_real_speed() {
    // 150 m/s is 2.5 m per tick: the ball is 20 cm across and the wall 5 cm thick
    // (a ball without an AngularVelocity is caught as well: the continuous collision of fast bodies, `tunnel_fixes`, puts it back against the wall
    // it crossed and the contact bounces it with its real speed; it used to fly through)
    let mut plain = Sim::new(1);
    thin_wall(&mut plain);
    shot(&mut plain, false, 150.0);
    let mut plain_bounced = false;
    for _ in 0..30 {
        plain.step();
        assert!(pos(&plain, 2)[0] < 10.0, "a ball without a spin stays on this side of the wall: {:?}", pos(&plain, 2));
        plain_bounced |= lin(&plain, 2)[0] < -100.0;
    }
    assert!(plain_bounced, "and bounces back: {:?}", lin(&plain, 2));
    let mut s = Sim::new(1);
    thin_wall(&mut s);
    shot(&mut s, true, 150.0);
    let mut bounced = false;
    for _ in 0..30 {
        s.step();
        assert!(pos(&s, 2)[0] < 10.0, "it stays on this side of the wall: {:?}", pos(&s, 2));
        bounced |= lin(&s, 2)[0] < -100.0;
    }
    assert!(bounced, "it bounced back at about its own speed: {:?}", lin(&s, 2));
    assert!(lin(&s, 2)[0] < -140.0, "{:?}", lin(&s, 2));
    assert_eq!(s.world.get::<AngularVelocity>(s.world.entity(2).unwrap()).unwrap().stash, [0.0; 4], "the bookkeeping is cleared again");
}

#[test]
fn a_fast_spinning_ball_lands_on_a_thin_floor_and_a_slow_one_is_left_alone() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Collider":{"shape":1,"size":[50,0.02,50]},"RigidBody":{"restitution":0.5,"friction":0.5}}"#);
    body(&mut s, 2, 0, [0.2; 3], [0.0, 30.0, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"restitution":0.5,"friction":0.5}"#, [0.0; 3]);
    let mut lowest = 99.0f32;
    for _ in 0..240 {
        s.step();
        lowest = lowest.min(pos(&s, 2)[1]);
    }
    assert!(lowest > 0.0, "it never went through: {lowest}");
    // it fell 30 m (24 m/s at the floor: well over its radius a tick) and bounced to about a quarter of the height
    assert!(pos(&s, 2)[1] > 0.0 && lin(&s, 2)[1].abs() < 30.0);
    // the same ball in a calm roll: no velocity is ever changed by the sweep
    let mut t = Sim::new(1);
    spawn(&mut t, 1, r#"{"Transform":{"pos":[0,0,0]},"Collider":{"shape":1,"size":[50,0.02,50]},"RigidBody":{"friction":0.5}}"#);
    body(&mut t, 2, 0, [0.2; 3], [0.0, 0.11, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.5,"restitution":0}"#, [0.0; 3]);
    t.apply(&Command::Set { uid: 2, component: "Velocity".into(), value: parse(r#"{"lin":[3,0,0]}"#).unwrap() }).unwrap();
    t.step_n(60);
    assert!((lin(&t, 2)[0] - 2.14).abs() < 0.15, "rolls at 5/7 of 3 m/s: {:?}", lin(&t, 2));
}

// ------------------------------------------------------------------------------------------------ odd numbers, piles, ground

#[test]
fn nonsense_numbers_never_panic_and_never_spread() {
    let run = || {
        let mut s = Sim::new(1);
        floor(&mut s);
        // a turn that is not a number, a spin that is, a capsule with no size, a box with no thickness, a body with a huge mass and one with a tiny one
        spawn(&mut s, 2, r#"{"Transform":{"pos":[0,1,0],"rot":[1e30,0,0,0]},"Velocity":{},"AngularVelocity":{"w":[1e30,0,0]},"Collider":{"shape":1,"size":[1,1,1]},"RigidBody":{"mass":1}}"#);
        spawn(&mut s, 3, r#"{"Transform":{"pos":[3,1,0]},"Velocity":{},"AngularVelocity":{"w":[0,2,0]},"Collider":{"shape":2,"size":[0,0,0]},"RigidBody":{"mass":1}}"#);
        spawn(&mut s, 4, r#"{"Transform":{"pos":[6,1,0]},"Velocity":{},"AngularVelocity":{},"Collider":{"shape":1,"size":[1,0,1]},"RigidBody":{"mass":1}}"#);
        spawn(&mut s, 5, r#"{"Transform":{"pos":[9,1,0]},"Velocity":{},"AngularVelocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1e30}}"#);
        spawn(&mut s, 6, r#"{"Transform":{"pos":[12,1,0]},"Velocity":{},"AngularVelocity":{},"Collider":{"shape":2,"size":[1,1,1]},"RigidBody":{"mass":1e-30}}"#);
        spawn(&mut s, 7, r#"{"Transform":{"pos":[15,1,0]},"Velocity":{"lin":[1e20,0,0]},"AngularVelocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1}}"#);
        spawn(&mut s, 8, r#"{"Transform":{"pos":[18,1,0]},"Velocity":{},"AngularVelocity":{},"Collider":{"shape":1,"size":[1,1,1]},"RigidBody":{"mass":1},"Joint":{"kind":4,"anchor":[18,1,0],"axis":[0,0,0],"limit":[1,-1]}}"#);
        spawn(&mut s, 9, r#"{"Transform":{"pos":[21,1,0]},"Velocity":{},"AngularVelocity":{},"Collider":{"shape":1,"size":[1,1,1]},"RigidBody":{"mass":1},"Joint":{"kind":3,"target":9,"anchor":[21,1,0]}}"#);
        spawn(&mut s, 10, r#"{"Transform":{"pos":[24,1,0]},"Velocity":{},"AngularVelocity":{},"Collider":{"shape":1,"size":[1,1,1]},"RigidBody":{"mass":1},"Joint":{"kind":4,"target":999,"anchor":[24,1,0]}}"#);
        s.step_n(180);
        s
    };
    let (a, b) = (run(), run());
    assert_eq!(a.state_hash(), b.state_hash());
    // the ordinary ones are fine, whatever happened to the others
    for uid in [3u64, 4, 5, 6, 8, 9, 10] {
        let p = pos(&a, uid);
        assert!(p.iter().all(|v| v.is_finite()), "body {uid} at {p:?}");
    }
}

#[test]
fn a_pile_of_turning_things_stays_in_its_box_and_costs_little() {
    let build = |threads: usize| {
        let mut s = Sim::new(9);
        s.threads = threads;
        floor(&mut s);
        for (i, (p, size)) in [([-6.0, 2.0, 0.0], [1.0, 4.0, 14.0]), ([6.0, 2.0, 0.0], [1.0, 4.0, 14.0]), ([0.0, 2.0, -6.0], [14.0, 4.0, 1.0]), ([0.0, 2.0, 6.0], [14.0, 4.0, 1.0])].iter().enumerate() {
            spawn(&mut s, 100 + i as u64, &format!(r#"{{"Transform":{{"pos":{p:?}}},"Collider":{{"shape":1,"size":{size:?}}}}}"#));
        }
        for i in 0..90u64 {
            let (gx, gz, gy) = ((i % 6) as f32 * 1.6 - 4.0, ((i / 6) % 5) as f32 * 1.6 - 3.2, 1.0 + (i / 30) as f32 * 1.5);
            let shape = [0u32, 1, 2][(i % 3) as usize];
            let size = [[0.8, 0.8, 0.8], [0.9, 0.5, 0.7], [0.4, 1.4, 0.4]][(i % 3) as usize];
            body(&mut s, 1000 + i, shape, size, [gx, gy, gz], qz(0.37 * i as f32), r#"{"mass":1,"restitution":0.2,"friction":0.5}"#, [(i % 5) as f32 - 2.0, 0.0, (i % 7) as f32 - 3.0]);
        }
        s
    };
    let mut s = build(1);
    let t = std::time::Instant::now();
    s.step_n(360);
    let per_tick = t.elapsed().as_secs_f64() * 1000.0 / 360.0;
    println!("90 turning bodies: {per_tick:.2} ms per tick");
    assert!(per_tick < 25.0, "90 turning bodies took {per_tick:.1} ms per tick");
    for i in 0..90u64 {
        let p = pos(&s, 1000 + i);
        assert!(p.iter().all(|v| v.is_finite()) && p[0].abs() < 6.0 && p[2].abs() < 6.0 && p[1] > -0.1 && p[1] < 6.0, "body {i} at {p:?}");
    }
    let mut b = build(4);
    b.step_n(360);
    assert_eq!(s.state_hash(), b.state_hash(), "the thread count changed the result");
}

/// A fresh workspace folder with 33 x 33 heightmaps in it.
fn heightmaps(name: &str, pictures: &[(&str, &dyn Fn(usize, usize) -> u16)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lunaris-rot-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, f) in pictures {
        let samples: Vec<u16> = (0..33 * 33).map(|i| f(i % 33, i / 33)).collect();
        std::fs::write(dir.join(file), crate::png::tests::make_png(33, 33, 16, 0, 4, &samples)).unwrap();
    }
    dir
}

#[test]
fn balls_roll_and_boxes_and_capsules_settle_on_a_heightmap() {
    // a round hill, 64 x 64 metres, 10 metres high
    let hill = |x: usize, z: usize| {
        let (dx, dz) = (x as f32 - 16.0, z as f32 - 16.0);
        let d = (dx * dx + dz * dz).sqrt() / 16.0;
        ((-3.0 * d * d).exp() * 65535.0) as u16
    };
    let flat = |_: usize, _: usize| 32768u16;
    let dir = heightmaps("hill", &[("hill.png", &hill), ("flat.png", &flat)]);
    let mut s = Sim::new(1);
    s.threads = 1;
    s.asset_root = Some(dir);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Terrain":{"heightmap":"hill.png","size":[64,64],"height":10,"friction":0.8,"restitution":0}}"#);
    // (a flat piece of ground of its own, 100 m away, for the capsule: on a slope it would roll for ever)
    spawn(&mut s, 5, r#"{"Transform":{"pos":[100,0,0]},"Terrain":{"heightmap":"flat.png","size":[64,64],"height":10,"friction":0.8,"restitution":0}}"#);
    body(&mut s, 2, 0, [1.0; 3], [8.0, 12.0, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.8,"restitution":0}"#, [0.0; 3]);
    body(&mut s, 3, 1, [1.0, 0.5, 2.0], [-8.0, 12.0, 3.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.8,"restitution":0}"#, [0.0; 3]);
    body(&mut s, 4, 2, [0.5, 2.0, 0.5], [100.0, 8.0, 0.0], qz(1.0), r#"{"mass":1,"friction":0.8,"restitution":0}"#, [0.0; 3]);
    s.step_n(420);
    let ball = pos(&s, 2);
    assert!(ball[0] > 11.0, "the ball rolled downhill, away from the top: {ball:?}");
    assert!(len(spin(&s, 2)) > 1.0 || len(lin(&s, 2)) < 0.2, "and it rolls (or it has come to rest): {:?}", spin(&s, 2));
    s.step_n(600);
    // (the ball has rolled off the rim by now; the box and the capsule are what stay)
    for uid in [3u64, 4] {
        let p = pos(&s, uid);
        assert!(p.iter().all(|v| v.is_finite()), "body {uid} at {p:?}");
        assert!(len(lin(&s, uid)) < 0.25, "body {uid} has come to rest: {:?}", lin(&s, uid));
    }
    // the box lies on the slope: its short axis (local y) points about the way the ground faces
    let (bp, bq) = (pos(&s, 3), rot(&s, 3));
    let ground_at = |x: f32, z: f32| {
        let e = s.world.entity(1).unwrap();
        let (t, g) = (*s.world.get::<Transform>(e).unwrap(), *s.world.get::<components::Terrain>(e).unwrap());
        let name = s.world.strings().get(g.heightmap.0).to_string();
        crate::terrain::Placed::new(s.heightfields.get(&name).expect("loaded"), t.pos, &g).surface(x, z).expect("over the ground")
    };
    let h = ground_at(bp[0], bp[2]);
    assert!(bp[1] - h > 0.2 && bp[1] - h < 1.4, "the box sits on the ground: {bp:?} over {h}");
    let (dx, dz) = (ground_at(bp[0] + 0.5, bp[2]) - ground_at(bp[0] - 0.5, bp[2]), ground_at(bp[0], bp[2] + 0.5) - ground_at(bp[0], bp[2] - 0.5));
    let n = { let l = (dx * dx + 1.0 + dz * dz).sqrt(); [-dx / l, 1.0 / l, -dz / l] };
    let up = axes(bq)[1];
    let cos = (up[0] * n[0] + up[1] * n[1] + up[2] * n[2]).abs();
    assert!(cos > 0.93, "the box lies flat on the slope: {cos}");
    // the capsule lies on its ground: its middle is a radius (0.25) above it, its axis level
    let cp = pos(&s, 4);
    assert!((cp[1] - 5.0 - 0.25).abs() < 0.03 && axes(rot(&s, 4))[1][1].abs() < 0.05, "the capsule lies on the ground: {cp:?} {:?}", rot(&s, 4));
}

// ------------------------------------------------------------------------------------------------ hinge motors and swing limits

fn add(s: &mut Sim, uid: u64, component: &str, json: &str) {
    s.apply(&Command::Add { uid, component: component.into(), value: parse(json).unwrap() }).unwrap();
}

/// A wheel 2 m across on a hinge through its centre about z, so gravity has no torque on it.
fn wheel(motor: &str) -> Sim {
    let mut s = Sim::new(1);
    body(&mut s, 2, 1, [2.0, 2.0, 0.4], [0.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":2,"restitution":0}"#, [0.0; 3]);
    add(&mut s, 2, "Joint", r#"{"kind":4,"anchor":[0,5,0],"axis":[0,0,1]}"#);
    add(&mut s, 2, "JointMotor", motor);
    s
}

#[test]
fn a_hinge_motor_turns_a_wheel_at_its_speed_and_a_weak_one_only_gets_it_going() {
    let mut strong = wheel(r#"{"speed":3,"max_torque":50}"#);
    strong.step_n(120);
    assert!((spin(&strong, 2)[2] - 3.0).abs() < 0.1, "the wheel turns at the motor's speed: {:?}", spin(&strong, 2));
    assert!(spin(&strong, 2)[0].abs() < 0.05 && spin(&strong, 2)[1].abs() < 0.05, "and only about the axis: {:?}", spin(&strong, 2));
    // the other way round
    let mut back = wheel(r#"{"speed":-2,"max_torque":50}"#);
    back.step_n(120);
    assert!((spin(&back, 2)[2] + 2.0).abs() < 0.1, "{:?}", spin(&back, 2));
    // a motor of half a newton-metre on a wheel whose inertia is 1.33 gives 0.375 radians per second squared: 0.375 after a second
    let mut weak = wheel(r#"{"speed":3,"max_torque":0.5}"#);
    weak.step_n(60);
    let w = spin(&weak, 2)[2];
    assert!(w > 0.25 && w < 0.5, "a weak motor speeds it up slowly: {w}");
    // no torque, no motor
    let mut off = wheel(r#"{"speed":3,"max_torque":0}"#);
    off.step_n(60);
    assert!(spin(&off, 2)[2].abs() < 0.01, "{:?}", spin(&off, 2));
}

/// An arm 2 m long and 5 kg hinged at the origin about z, let go horizontal (gravity pulls on it with 49 newton-metres).
fn arm(motor: &str) -> Sim {
    let mut s = Sim::new(1);
    body(&mut s, 2, 1, [2.0, 0.2, 0.2], [1.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":5,"restitution":0}"#, [0.0; 3]);
    add(&mut s, 2, "Joint", r#"{"kind":4,"anchor":[0,5,0],"axis":[0,0,1]}"#);
    add(&mut s, 2, "JointMotor", motor);
    s
}

fn angle_z(s: &Sim, uid: u64) -> f32 {
    let q = rot(s, uid);
    2.0 * q[2].atan2(q[3])
}

#[test]
fn a_strong_motor_at_speed_zero_holds_an_arm_and_a_weak_one_cannot() {
    let mut held = arm(r#"{"speed":0,"max_torque":300}"#);
    held.step_n(180);
    assert!(angle_z(&held, 2).abs() < 0.1, "a strong brake holds the arm where it was: {}", angle_z(&held, 2));
    let mut falls = arm(r#"{"speed":0,"max_torque":10}"#);
    falls.step_n(180);
    assert!(angle_z(&falls, 2) < -0.5, "a weak one is overpowered by the load: {}", angle_z(&falls, 2));
    // a motor that lifts: it turns the arm up against gravity, at its speed while it has the torque
    let mut lifts = arm(r#"{"speed":1,"max_torque":300}"#);
    lifts.step_n(60);
    assert!(angle_z(&lifts, 2) > 0.7 && angle_z(&lifts, 2) < 1.3, "the arm is lifted at about one radian a second: {}", angle_z(&lifts, 2));
}

/// A rod 2 m long hung by its top end from a ball-and-socket at (0, 10, 0), given a push. The angle between the rod and the vertical.
fn rod_swing(cone: Option<&str>) -> f32 {
    let mut s = Sim::new(1);
    body(&mut s, 2, 1, [0.2, 2.0, 0.2], [0.0, 9.0, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":2,"restitution":0}"#, [0.0; 3]);
    add(&mut s, 2, "Joint", r#"{"kind":3,"anchor":[0,10,0],"axis":[0,1,0]}"#);
    if let Some(c) = cone {
        add(&mut s, 2, "JointCone", c);
    }
    // (the joint binds itself to the two bodies on the first physics tick: push the rod after that, so that the limit is measured from the vertical)
    s.step_n(2);
    s.apply(&Command::Set { uid: 2, component: "AngularVelocity".into(), value: parse(r#"{"w":[0,0,15]}"#).unwrap() }).unwrap();
    let mut widest = 0.0f32;
    for _ in 0..240 {
        s.step();
        let a = axes(rot(&s, 2));
        widest = widest.max(a[1][1].clamp(-1.0, 1.0).acos());
    }
    widest
}

#[test]
fn a_ball_and_socket_with_a_cone_limit_swings_no_further_than_its_angle() {
    let free = rod_swing(None);
    assert!(free > 1.0, "without a limit the pushed rod swings far: {free}");
    let limited = rod_swing(Some(r#"{"angle":0.5}"#));
    assert!(limited < 0.6, "the limit holds: {limited}");
    assert!(limited > 0.4, "and the rod did swing out to it: {limited}");
    // an angle of 0 means no limit
    let none = rod_swing(Some(r#"{"angle":0}"#));
    assert!((none - free).abs() < 1e-5, "{none} {free}");
}

// ------------------------------------------------------------------------------------------------ stacks

/// A tower of `n` one-metre boxes (each can turn), started a hair out of line, stepped for ten seconds: how far the top box drifted sideways, how far it
/// sank, and the top speed at the end.
fn tower(n: u64, reference: bool) -> (f32, f32, f32) {
    let mut s = Sim::new(1);
    s.reference_physics = reference;
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[60,1,60]}}"#);
    for k in 0..n {
        let off = ((k * 7919) % 13) as f32 * 0.0008 - 0.005;
        body(&mut s, 10 + k, 1, [1.0, 1.0, 1.0], [off, 0.5 + k as f32, -off * 0.7], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"restitution":0,"friction":0.6}"#, [0.0; 3]);
    }
    s.step_n(600);
    let top = pos(&s, 10 + n - 1);
    let fastest = (0..n).map(|k| len(lin(&s, 10 + k))).fold(0.0f32, f32::max);
    ((top[0] * top[0] + top[2] * top[2]).sqrt(), (n as f32 - 0.5) - top[1], fastest)
}

#[test]
fn towers_of_turning_boxes_stand_ten_boxes_high_and_more() {
    // (with 20 solver rounds and no memory a tower of ten collapsed: the top box ended 7 m away at 10 m/s. The solver now starts every tick from the impulses it
    // ended the last one with, and gives few contact points many rounds: towers of 16 stand still to within a few millimetres)
    for (n, drift, speed) in [(4u64, 0.02f32, 0.01f32), (8, 0.03, 0.02), (10, 0.03, 0.02), (12, 0.03, 0.02), (16, 0.05, 0.03)] {
        for reference in [false, true] {
            let (d, sank, v) = tower(n, reference);
            assert!(d < drift && sank < 0.05 && v < speed, "a tower of {n} boxes (reference {reference}): drifted {d} m, sank {sank} m, top speed {v} (limits {drift} {speed})");
        }
    }
}

#[test]
fn a_pyramid_of_boxes_stands() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[60,1,60]}}"#);
    let mut ids = Vec::new();
    let mut uid = 10u64;
    for r in 0..7u64 {
        let count = 7 - r;
        for c in 0..count {
            let x = (c as f32 - (count as f32 - 1.0) * 0.5) * 1.002;
            body(&mut s, uid, 1, [1.0, 1.0, 1.0], [x, 0.5 + r as f32, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"restitution":0,"friction":0.6}"#, [0.0; 3]);
            ids.push((uid, x, 0.5 + r as f32));
            uid += 1;
        }
    }
    s.step_n(600);
    for (id, x, y) in ids {
        let p = pos(&s, id);
        assert!((p[0] - x).abs() < 0.03 && (p[1] - y).abs() < 0.03 && p[2].abs() < 0.03, "box {id} moved from {:?} to {p:?}", [x, y, 0.0]);
    }
}

// ------------------------------------------------------------------------------------------------ the memory between ticks

/// `n` one-metre boxes in a tower on a floor, each started a hair out of line (the same world as `tower`, not yet stepped).
fn tower_sim(n: u64) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[60,1,60]}}"#);
    for k in 0..n {
        let off = ((k * 7919) % 13) as f32 * 0.0008 - 0.005;
        body(&mut s, 10 + k, 1, [1.0, 1.0, 1.0], [off, 0.5 + k as f32, -off * 0.7], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"restitution":0,"friction":0.6}"#, [0.0; 3]);
    }
    s
}

#[test]
fn the_solver_remembers_what_each_contact_carries_and_it_is_part_of_the_state() {
    let mut s = tower_sim(6);
    assert!(s.contact_memory.is_empty());
    s.step_n(120);
    // 5 pairs of boxes and one pair of the lowest box with the floor
    assert_eq!(s.contact_memory.pair_count(), 6);
    // each pair carries the weight of everything above it for one tick (m g dt, the boxes weigh 1 kg)
    let dt = 1.0 / 60.0;
    for (a, b, above) in [(1u64, 10u64, 6.0f32), (10, 11, 5.0), (11, 12, 4.0), (12, 13, 3.0), (13, 14, 2.0), (14, 15, 1.0)] {
        let pts = s.contact_memory.find(a, b).unwrap_or_else(|| panic!("no memory of the pair {a} {b}"));
        let carried: f32 = pts.iter().map(|p| p.jn).sum();
        let weight = above * 9.8 * dt;
        assert!((carried - weight).abs() < 0.06 * weight, "pair {a} {b} carries {carried}, the weight above is {weight}");
    }
    // it is state: a copy without it is not the same state
    let mut cleared = s.clone();
    cleared.contact_memory.clear();
    assert_ne!(cleared.state_hash(), s.state_hash());
    // and `clear` empties it
    s.clear();
    assert!(s.contact_memory.is_empty());
}

#[test]
fn rewinding_and_forking_bring_the_contact_memory_back() {
    let mut s = tower_sim(5);
    s.step_n(40);
    let c = s.checkpoint();
    s.step_n(30);
    let h = s.state_hash();
    let mut fork = s.clone();
    s.rewind_to(&c).unwrap();
    assert!(!s.contact_memory.is_empty());
    s.step_n(30);
    assert_eq!(s.state_hash(), h, "stepping again from a checkpoint gives the same history, contact memory included");
    s.step_n(40);
    fork.step_n(40);
    assert_eq!(s.state_hash(), fork.state_hash(), "a fork stays in lockstep");
}

#[test]
fn a_recording_started_in_the_middle_of_a_run_replays_exactly_even_through_json() {
    let mut s = tower_sim(5);
    s.step_n(40);
    s.start_recording().unwrap();
    s.step_n(30);
    let rec = s.stop_recording().unwrap();
    assert!(!rec.start_contacts.is_empty(), "the recording keeps what the solver remembered when it began");
    assert_eq!(Sim::replay(&s, &rec).unwrap().state_hash(), rec.final_hash);
    let text = rec.to_json().to_string_compact().unwrap();
    let back = crate::sim::Recording::from_json(&parse(&text).unwrap()).unwrap();
    assert_eq!(back.start_contacts, rec.start_contacts);
    assert_eq!(Sim::replay(&s, &back).unwrap().state_hash(), rec.final_hash);
}

// ------------------------------------------------------------------------------------------------ hinge springs

/// A bar 2 m long and 5 kg hinged at its end about z, in space (no gravity), with a `JointSpring`.
fn sprung_arm(spring: &str, w0: f32) -> Sim {
    let mut s = Sim::new(1);
    no_gravity(&mut s);
    body(&mut s, 2, 1, [2.0, 0.2, 0.2], [1.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":5,"restitution":0}"#, [0.0, 0.0, w0]);
    add(&mut s, 2, "Joint", r#"{"kind":4,"anchor":[0,5,0],"axis":[0,0,1]}"#);
    add(&mut s, 2, "JointSpring", spring);
    s
}

#[test]
fn a_hinge_spring_turns_the_body_to_its_angle_and_settles_there() {
    // inertia about the end is about 6.7 kg m^2: with a stiffness of 20 the arm swings at 1.7 radians per second, and a damping of 15 settles it in a few seconds
    let mut s = sprung_arm(r#"{"stiffness":20,"damping":15,"angle":1.0}"#, 0.0);
    s.step_n(30);
    let early = angle_z(&s, 2);
    assert!(early > 0.05 && early < 1.0, "it sets off towards the angle: {early}");
    s.step_n(450);
    assert!((angle_z(&s, 2) - 1.0).abs() < 0.03, "and settles on it: {}", angle_z(&s, 2));
    assert!(spin(&s, 2)[2].abs() < 0.02, "at rest: {:?}", spin(&s, 2));
    // the other way round
    let mut back = sprung_arm(r#"{"stiffness":20,"damping":15,"angle":-0.7}"#, 0.0);
    back.step_n(480);
    assert!((angle_z(&back, 2) + 0.7).abs() < 0.03, "{}", angle_z(&back, 2));
}

#[test]
fn a_spring_without_damping_swings_and_with_damping_only_slows_a_turning_body() {
    // undamped: it overshoots the angle and swings back past it
    let mut s = sprung_arm(r#"{"stiffness":20,"damping":0,"angle":0.5}"#, 0.0);
    let mut most = 0.0f32;
    for _ in 0..240 {
        s.step();
        most = most.max(angle_z(&s, 2));
    }
    assert!(most > 0.9, "an undamped spring swings out to twice the angle: {most}");
    // damping alone (no stiffness): a spinning arm slows down and stops, and the angle it reaches is not pulled anywhere
    let mut d = sprung_arm(r#"{"stiffness":0,"damping":10}"#, 2.0);
    d.step_n(60);
    let w1 = spin(&d, 2)[2];
    d.step_n(240);
    let w2 = spin(&d, 2)[2];
    assert!(w1 > 0.05 && w1 < 1.2, "slowed after one second: {w1}");
    assert!(w2 >= 0.0 && w2 < 0.05, "stopped after five: {w2}");
    // nothing set: nothing happens
    let mut none = sprung_arm(r#"{"stiffness":0,"damping":0,"angle":1.0}"#, 0.0);
    none.step_n(120);
    assert!(angle_z(&none, 2).abs() < 1e-4, "{}", angle_z(&none, 2));
}
