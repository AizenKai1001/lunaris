//! Raycast vehicles: the wheel table, the ride height the springs give, acceleration, braking, steering, ramps, layers, the models of the wheels, and the
//! promises of the engine (replay, snapshot, any thread count, nothing run when there is no vehicle). `cargo test -p lunaris-sim --lib vehicle -- --nocapture`
//! prints the numbers quoted in docs/PHYSICS.md.
use super::*;
use crate::meshcol_gltf::tests::glb_of;
use crate::physics_rot::axes_of;
use crate::png::tests::make_png;
use crate::vehicle::{parse_wheels, work_done};
use components::{AngularVelocity, LocalTransform, Transform, Vehicle, Velocity};
use lunaris_json::{parse, Json};
use std::path::PathBuf;

type V3 = [f32; 3];

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A car of 1200 kg: a box 1.8 wide, 0.7 high and 4.2 long, four wheels of radius 0.35 whose struts start 0.15 below the centre of the box, at 0.8 either side and 1.3
/// in front of and behind it. Springs of 35,000 N/m and dampers of 3,200 N s/m, a rear-wheel drive, brakes on all four, the handbrake on the rear ones.
const WHEELS: &str = "defaults: radius=0.35 rest=0.3 travel=0.2 spring=35000 damper=3200 grip=1.2\n\
fl: -0.8 -0.15 -1.3 steer=1 brake=1\n\
fr: 0.8 -0.15 -1.3 steer=1 brake=1\n\
rl: -0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1\n\
rr: 0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1";

const MASS: f32 = 1200.0;

fn escape(s: &str) -> String {
    s.replace('\n', "\\n")
}

fn spawn_car(s: &mut Sim, uid: u64, pos: V3, wheels: &str, extra: &str) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":{pos:?}}},"Velocity":{{}},"AngularVelocity":{{"damping":0.05}},"Collider":{{"shape":1,"size":[1.8,0.7,4.2]}},"RigidBody":{{"mass":{MASS},"friction":0.3,"restitution":0}},"Vehicle":{{"wheels":"{}"}}{extra}}}"#, escape(wheels)));
}

fn flat(s: &mut Sim) {
    spawn(s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[6000,1,6000]},"RigidBody":{"friction":0.8}}"#);
}

/// The rest height of the chassis's centre over flat ground with these springs: the wheel's centre is `radius` up, its strut `rest - compression` long, the mount
/// point 0.15 above that on the chassis's own y axis.
fn ride_height() -> f32 {
    let x = MASS * 9.8 / 4.0 / 35000.0;
    0.35 + (0.3 - x) + 0.15
}

fn garage() -> Sim {
    let mut s = Sim::new(1);
    s.threads = 1;
    flat(&mut s);
    spawn_car(&mut s, 2, [0.0, ride_height(), 0.0], WHEELS, "");
    s
}

fn pos(s: &Sim, uid: u64) -> V3 {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn lin(s: &Sim, uid: u64) -> V3 {
    s.world.get::<Velocity>(s.world.entity(uid).unwrap()).unwrap().lin
}

fn spin(s: &Sim, uid: u64) -> V3 {
    s.world.get::<AngularVelocity>(s.world.entity(uid).unwrap()).unwrap().w
}

fn car(s: &Sim, uid: u64) -> Vehicle {
    *s.world.get::<Vehicle>(s.world.entity(uid).unwrap()).unwrap()
}

fn drive(s: &mut Sim, uid: u64, f: impl FnOnce(&mut Vehicle)) {
    let e = s.world.entity(uid).unwrap();
    f(s.world.get_mut::<Vehicle>(e).unwrap());
}

fn axes(s: &Sim, uid: u64) -> [V3; 3] {
    axes_of(s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().rot)
}

/// The car's forward direction (its -z), its up and its speed along the way it points.
fn up_y(s: &Sim, uid: u64) -> f32 {
    axes(s, uid)[1][1]
}

fn forward_speed(s: &Sim, uid: u64) -> f32 {
    let a = axes(s, uid);
    let v = lin(s, uid);
    -(v[0] * a[2][0] + v[1] * a[2][1] + v[2] * a[2][2])
}

fn len(v: V3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn dir_for(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lunaris-vehicle-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ------------------------------------------------------------------------------------------------ the table

#[test]
fn a_wheel_table_is_a_line_a_wheel_and_every_mistake_has_its_line() {
    let w = parse_wheels(WHEELS).unwrap();
    assert_eq!(w.len(), 4);
    assert_eq!(w[0].name, "fl");
    assert_eq!(w[0].pos, [-0.8, -0.15, -1.3]);
    assert_eq!((w[0].radius, w[0].rest, w[0].travel, w[0].spring, w[0].damper, w[0].grip), (0.35, 0.3, 0.2, Some(35000.0), Some(3200.0), 1.2), "the defaults line fills the wheels after it");
    assert_eq!((w[0].steer, w[0].drive, w[0].brake, w[0].handbrake), (1.0, 0.0, 1.0, 0.0));
    assert_eq!((w[3].steer, w[3].drive, w[3].brake, w[3].handbrake), (0.0, 1.0, 0.7, 1.0));
    // comments, `;` for a line end, aliases and `auto`
    let w = parse_wheels("# a bike\nf: 0 -0.1 -0.9 r=0.3 k=auto c=2000 steer=1 ; b: 0 -0.1 0.9 mu=1.5 hb=1 drive=1 entity=17").unwrap();
    assert_eq!((w.len(), w[0].radius, w[0].spring, w[0].damper), (2, 0.3, None, Some(2000.0)));
    assert_eq!((w[1].grip, w[1].handbrake, w[1].entity), (1.5, 1.0, 17));
    assert!(parse_wheels("").unwrap().is_empty() && parse_wheels("  # nothing\n").unwrap().is_empty());
    // the mistakes
    let bad = |t: &str| parse_wheels(t).unwrap_err();
    assert!(bad("fl: -0.8 -0.15").contains("needs its mount point"), "{}", bad("fl: -0.8 -0.15"));
    assert!(bad("fl: a b c").contains("not a number"));
    assert!(bad("fl -0.8 -0.15 -1.3").contains("line 1") && bad("fl -0.8 -0.15 -1.3").contains("name: x y z"));
    assert!(bad("a: 0 0 0\nb: 0 0 0 turbo=1").contains("line 2") && bad("a: 0 0 0\nb: 0 0 0 turbo=1").contains("unknown key `turbo`"));
    assert!(bad("a: 0 0 0 radius=big").contains("must be a number"));
    assert!(bad("a: 0 0 0 radius").contains("key=value"));
    assert!(bad("a: 0 0 0\na: 1 0 0").contains("already a wheel called `a`"));
    assert!(bad("a: 0 0 0 entity=-3").contains("uid"));
    let many: String = (0..9).map(|i| format!("w{i}: 0 0 {i}\n")).collect();
    assert!(bad(&many).contains("at most 8"), "{}", bad(&many));
}

// ------------------------------------------------------------------------------------------------ riding

#[test]
fn a_car_settles_at_the_ride_height_its_springs_give_and_stays_there() {
    let mut s = Sim::new(1);
    s.threads = 1;
    flat(&mut s);
    // dropped from 0.3 m above its ride height
    spawn_car(&mut s, 2, [0.0, ride_height() + 0.3, 0.0], WHEELS, "");
    let mut lowest = f32::MAX;
    for _ in 0..360 {
        s.step();
        lowest = lowest.min(pos(&s, 2)[1]);
    }
    let want = ride_height();
    let y = pos(&s, 2)[1];
    let v = car(&s, 2);
    let x = MASS * 9.8 / 4.0 / 35000.0;
    eprintln!("ride height {y:.4} m (wanted {want:.4}); lowest on the way {lowest:.4}; compression {:?} (wanted {x:.4}); speed {:.5}", &v.compression[..4], len(lin(&s, 2)));
    assert!((y - want).abs() < 0.005, "ride height {y}, springs say {want}");
    assert_eq!(v.on_ground, 4);
    assert_eq!(v.contact, 0b1111);
    for i in 0..4 {
        assert!((v.compression[i] - x).abs() < 0.004, "wheel {i}: {} against {x}", v.compression[i]);
    }
    assert!(lowest > want - 0.1, "it did not bottom out: {lowest}");
    assert!(len(lin(&s, 2)) < 0.01 && len(spin(&s, 2)) < 0.01, "at rest: {:?} {:?}", lin(&s, 2), spin(&s, 2));
    assert!(pos(&s, 2)[0].abs() < 0.01 && pos(&s, 2)[2].abs() < 0.01, "and it has not crept: {:?}", pos(&s, 2));
    assert!(up_y(&s, 2) > 0.9999, "level: {}", up_y(&s, 2));
}

#[test]
fn a_car_on_a_slope_holds_still_without_input_and_a_car_in_the_air_falls() {
    // a ramp of 12 degrees rising toward -z (a box turned about x)
    let th = 12.0f32.to_radians();
    let mut s = Sim::new(1);
    s.threads = 1;
    let (h, l) = (0.4f32, 30.0f32);
    let c = [0.0, -(h / 2.0) * th.cos() + (l / 2.0) * th.sin(), -(h / 2.0) * th.sin() - (l / 2.0) * th.cos()];
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":{c:?},"rot":[{},0,0,{}]}},"Collider":{{"shape":1,"size":[20,{h},{l}]}},"RigidBody":{{"friction":0.8}}}}"#, (th / 2.0).sin(), (th / 2.0).cos()));
    // the surface is at height (-z) tan(theta) over the ground; put the car on it with its nose uphill
    let z0 = -8.0f32;
    let surface = -z0 * th.tan();
    let start = [0.0, surface + ride_height() / th.cos(), z0];
    spawn_car(&mut s, 2, start, WHEELS, "");
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Transform>(e).unwrap().rot = [(th / 2.0).sin(), 0.0, 0.0, (th / 2.0).cos()];
    s.step_n(240);
    let p = pos(&s, 2);
    eprintln!("on a 12 degree slope after 4 s: moved {:.3} m along z, speed {:.4}", p[2] - z0, len(lin(&s, 2)));
    assert!((p[2] - z0).abs() < 0.15, "the light brake below 2 m/s holds it: {p:?}");
    assert!(car(&s, 2).on_ground == 4);
    // and with no ground at all it just falls with the chassis's own weight
    let mut t = Sim::new(1);
    t.threads = 1;
    spawn_car(&mut t, 2, [0.0, 10.0, 0.0], WHEELS, "");
    t.step_n(60);
    assert_eq!(car(&t, 2).on_ground, 0);
    assert!((pos(&t, 2)[1] - (10.0 - 0.5 * 9.8)).abs() < 0.2, "free fall: {:?}", pos(&t, 2));
    assert!(car(&t, 2).compression.iter().all(|c| *c == 0.0));
}

// ------------------------------------------------------------------------------------------------ driving

#[test]
fn full_throttle_takes_the_car_from_standing_to_100_km_h_in_a_few_seconds() {
    let mut s = garage();
    s.step_n(120);
    drive(&mut s, 2, |v| v.throttle = 1.0);
    let target = 100.0 / 3.6;
    let mut at = 0u32;
    let mut last = 0.0f32;
    let mut worst_pitch = 0.0f32;
    let mut max_dip = 0.0f32;
    for tick in 1..=1200u32 {
        s.step();
        let v = forward_speed(&s, 2);
        if at == 0 && v >= target {
            at = tick;
        }
        assert!(v >= last - 0.05, "it never slows down under full throttle: {v} after {last} at tick {tick}");
        last = v;
        worst_pitch = worst_pitch.max(axes(&s, 2)[2][1].abs());
        let c = car(&s, 2).compression;
        max_dip = max_dip.max(0.5 * (c[2] + c[3] - c[0] - c[1]));
    }
    let secs = at as f32 / 60.0;
    eprintln!("0 to 100 km/h in {secs:.2} s; after 20 s {:.1} m/s ({:.0} km/h); worst pitch {:.1} degrees; the rear springs are pressed in {:.3} m more than the front ones", last, last * 3.6, worst_pitch.asin().to_degrees(), max_dip);
    assert!(at > 0 && secs > 4.0 && secs < 9.0, "0 to 100 km/h took {secs} s");
    assert!(last > 40.0 && last < 56.0, "after 20 s: {last} m/s (top speed 55, drag and rolling resistance take their share)");
    assert!(worst_pitch < 0.2, "it did not rear up: {worst_pitch}");
    assert!(pos(&s, 2)[0].abs() < 0.5, "and drives straight: {:?}", pos(&s, 2));
    assert_eq!(car(&s, 2).on_ground, 4);
}

#[test]
fn reverse_goes_backwards_to_the_reverse_speed_and_throttle_against_the_motion_brakes_first() {
    let mut s = garage();
    s.step_n(120);
    drive(&mut s, 2, |v| v.throttle = -1.0);
    s.step_n(600);
    let v = forward_speed(&s, 2);
    eprintln!("reverse: {v:.2} m/s after 10 s");
    assert!(v < -8.0 && v > -10.6, "reversing at {v} (the limit is 10)");
    // rolling at 20 m/s forwards, the back pedal is the brake until the car is slow enough to reverse
    let mut t = garage();
    t.step_n(120);
    let e = t.world.entity(2).unwrap();
    t.world.get_mut::<Velocity>(e).unwrap().lin = [0.0, 0.0, -20.0];
    drive(&mut t, 2, |v| v.throttle = -1.0);
    let mut ticks_to_stop = 0;
    for tick in 1..=600u32 {
        t.step();
        if forward_speed(&t, 2) < 0.5 && ticks_to_stop == 0 {
            ticks_to_stop = tick;
        }
    }
    assert!(ticks_to_stop > 60 && ticks_to_stop < 200, "it stopped after {ticks_to_stop} ticks, braking (not instantly, not never)");
    assert!(forward_speed(&t, 2) < -3.0, "and then reversed: {}", forward_speed(&t, 2));
}

#[test]
fn the_brakes_stop_the_car_in_about_the_distance_the_tyres_allow_and_the_nose_dips() {
    let mut s = garage();
    s.step_n(120);
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Velocity>(e).unwrap().lin = [0.0, 0.0, -20.0];
    drive(&mut s, 2, |v| v.brake = 1.0);
    let z0 = pos(&s, 2)[2];
    let mut dip = 0.0f32;
    let mut ticks = 0;
    for tick in 1..=600u32 {
        s.step();
        let v = car(&s, 2);
        dip = dip.max(v.compression[0] + v.compression[1] - v.compression[2] - v.compression[3]);
        if forward_speed(&s, 2) < 0.05 {
            ticks = tick;
            break;
        }
    }
    let dist = z0 - pos(&s, 2)[2];
    let decel = 20.0 * 20.0 / (2.0 * dist);
    eprintln!("braking from 20 m/s: {dist:.1} m in {ticks} ticks, a deceleration of {decel:.1} m/s^2 ({:.2} g); the nose dips: front minus rear compression {dip:.3} m", decel / 9.8);
    assert!(ticks > 0 && dist > 14.0 && dist < 22.0, "stopping distance {dist} m: mu 1.2 allows 17 m");
    assert!(dip > 0.03, "weight moves forward under braking: {dip}");
    assert!(up_y(&s, 2) > 0.99);
}

#[test]
fn full_lock_at_low_speed_drives_a_circle_of_about_the_radius_the_wheelbase_and_angle_give() {
    let mut s = garage();
    s.step_n(120);
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Velocity>(e).unwrap().lin = [0.0, 0.0, -6.0];
    drive(&mut s, 2, |v| v.steer = 1.0);
    let mut radii = Vec::new();
    let mut worst_up = 1.0f32;
    let mut speeds = Vec::new();
    for tick in 1..=900u32 {
        // keeps about 6 m/s: a touch of throttle below it
        let v = forward_speed(&s, 2);
        drive(&mut s, 2, |c| c.throttle = if v < 6.0 { 0.35 } else { 0.0 });
        s.step();
        worst_up = worst_up.min(up_y(&s, 2));
        if tick > 300 {
            let w = spin(&s, 2)[1].abs();
            if w > 0.05 {
                radii.push(len(lin(&s, 2)) / w);
                speeds.push(len(lin(&s, 2)));
            }
        }
    }
    let r = radii.iter().sum::<f32>() / radii.len() as f32;
    let v = speeds.iter().sum::<f32>() / speeds.len() as f32;
    // the wheels turn 32 degrees, less at speed: at 6 m/s 32 * (1 - 0.6 * 6 / 40) = 29.1 degrees, the wheelbase is 2.6 m; the car turns about a point on
    // the line of the rear axle, and its centre (half way between the axles) is that far from it
    let delta = (32.0 * (1.0 - 0.6 * v / 40.0)).to_radians();
    let r_rear = 2.6 / delta.tan();
    let want = (r_rear * r_rear + 1.3 * 1.3).sqrt();
    eprintln!("a circle of radius {r:.2} m at {v:.2} m/s (the kinematic circle: {want:.2} m, front wheels at {:.1} degrees), the worst roll {:.1} degrees", delta.to_degrees(), worst_up.acos().to_degrees());
    assert!(r > 0.8 * want && r < 1.3 * want, "the circle {r} m against {want} m");
    assert!(worst_up > 0.95, "it did not roll: {worst_up}");
    assert_eq!(car(&s, 2).on_ground, 4);
}

#[test]
fn the_steering_angle_fades_with_speed_and_follows_the_wheel_at_its_rate() {
    let mut s = garage();
    s.step_n(120);
    drive(&mut s, 2, |v| v.steer = 1.0);
    s.step_n(6);
    let early = car(&s, 2).steering;
    assert!((early - 18.0).abs() < 1.5, "180 degrees a second: after 6 ticks 18 degrees, not the full lock: {early}");
    s.step_n(60);
    assert!((car(&s, 2).steering - 32.0).abs() < 0.01, "standing still it gets the whole lock: {}", car(&s, 2).steering);
    // the same at 40 m/s: 40 percent of it (steer_fade 0.4 at steer_fade_speed 40)
    let mut t = garage();
    t.step_n(120);
    let e = t.world.entity(2).unwrap();
    t.world.get_mut::<Velocity>(e).unwrap().lin = [0.0, 0.0, -40.0];
    drive(&mut t, 2, |v| v.steer = -1.0);
    t.step_n(20);
    let s40 = car(&t, 2).steering;
    assert!((s40 + 32.0 * 0.4).abs() < 0.8, "at 40 m/s: {s40} (wanted -12.8)");
}

#[test]
fn a_car_does_not_flip_at_low_speed_however_it_is_steered() {
    let mut s = garage();
    s.step_n(120);
    let mut worst = 1.0f32;
    for tick in 0..1500u32 {
        // every pattern of the wheel: full lock one way and the other, changed every half second to a second, with the brake and the throttle in turn
        let phase = (tick / 40) % 7;
        drive(&mut s, 2, |v| {
            v.steer = [1.0, -1.0, 1.0, -0.4, 1.0, -1.0, 0.0][phase as usize];
            v.throttle = if tick % 200 < 140 { 1.0 } else { 0.0 };
            v.brake = if tick % 200 >= 160 { 1.0 } else { 0.0 };
            v.handbrake = if phase == 2 && tick % 40 > 20 { 1.0 } else { 0.0 };
        });
        s.step();
        worst = worst.min(up_y(&s, 2));
        let v = forward_speed(&s, 2);
        assert!(v.is_finite() && pos(&s, 2).iter().all(|x| x.is_finite()), "numbers stay numbers (tick {tick})");
        if v > 14.0 {
            drive(&mut s, 2, |c| c.throttle = 0.0);
        }
    }
    eprintln!("1,500 ticks of violent steering below 14 m/s: the car's up axis never fell below {worst:.3} (a roll of {:.1} degrees)", worst.acos().to_degrees());
    assert!(worst > 0.8, "it rolled over: up.y {worst}");
}

#[test]
fn at_top_speed_with_the_wheel_thrown_from_side_to_side_the_car_stays_a_well_behaved_body() {
    let mut s = garage();
    s.step_n(120);
    drive(&mut s, 2, |v| v.throttle = 1.0);
    let mut fastest = 0.0f32;
    let mut worst_up = 1.0f32;
    for tick in 0..1800u32 {
        // 30 s: full throttle, the wheel hard over one way and the other every 0.4 s
        drive(&mut s, 2, |v| v.steer = if (tick / 24) % 2 == 0 { 1.0 } else { -1.0 });
        s.step();
        let (p, v) = (pos(&s, 2), lin(&s, 2));
        assert!(p.iter().chain(v.iter()).all(|x| x.is_finite()), "numbers at tick {tick}: {p:?} {v:?}");
        fastest = fastest.max(len(v));
        worst_up = worst_up.min(up_y(&s, 2));
        assert!(len(v) < 100.0 && len(spin(&s, 2)) <= 60.01, "tick {tick}: {} m/s, spin {:?}", len(v), spin(&s, 2));
    }
    eprintln!("30 s of full throttle and a wheel thrown from side to side: top {fastest:.1} m/s, the up axis never below {worst_up:.2}");
    assert!(fastest > 15.0, "it did drive: {fastest}");
}

#[test]
fn a_car_that_hits_a_wall_is_stopped_by_its_chassis_and_does_not_go_through() {
    let mut s = garage();
    // a wall one metre thick across the road 40 m ahead (the car faces -z)
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,2,-40]},"Collider":{"shape":1,"size":[30,4,1]},"RigidBody":{"friction":0.5,"restitution":0.1}}"#);
    s.step_n(120);
    drive(&mut s, 2, |v| v.throttle = 1.0);
    let mut hit_at = None;
    let mut before = 0.0f32;
    for tick in 0..900u32 {
        s.step();
        if hit_at.is_none() && s.contacts.iter().any(|c| (c.a == 5 && c.b == 2) || (c.a == 2 && c.b == 5)) {
            hit_at = Some((tick, before)); // (the speed of the tick before: the contact of this one has already slowed it)
        }
        before = forward_speed(&s, 2);
        assert!(pos(&s, 2)[2] > -41.5, "it went through the wall at tick {tick}: {:?}", pos(&s, 2));
    }
    let (tick, v) = hit_at.expect("it reached the wall");
    eprintln!("hit the wall at {:.1} m/s (tick {tick}); after 15 s it is at z = {:.2} ({:.1} m/s)", v, pos(&s, 2)[2], forward_speed(&s, 2));
    assert!(v > 8.0, "it hit at speed: {v}");
    assert!(pos(&s, 2)[2] > -40.0 + 0.5 + 1.8, "and it is on the near side of the wall: {:?}", pos(&s, 2));
}

#[test]
fn the_handbrake_locks_the_rear_wheels_and_lets_the_tail_slide() {
    let run = |hb: f32| {
        let mut s = garage();
        s.step_n(120);
        let e = s.world.entity(2).unwrap();
        s.world.get_mut::<Velocity>(e).unwrap().lin = [0.0, 0.0, -18.0];
        drive(&mut s, 2, |v| {
            v.steer = 0.5;
            v.handbrake = hb;
        });
        let mut slide = 0.0f32;
        for _ in 0..90 {
            s.step();
            let v = car(&s, 2);
            slide = slide.max(v.slip[2].abs().max(v.slip[3].abs()));
        }
        (slide, forward_speed(&s, 2), car(&s, 2).spin)
    };
    let (slide_off, speed_off, _) = run(0.0);
    let (slide_on, speed_on, spin_on) = run(1.0);
    eprintln!("the rear tyres' sideways slip while turning at 18 m/s: {slide_off:.2} m/s without the handbrake, {slide_on:.2} m/s with it; speed {speed_off:.1} against {speed_on:.1}");
    assert!(slide_on > slide_off + 1.0, "the tail slides with the handbrake on: {slide_on} against {slide_off}");
    assert!(speed_on < speed_off - 3.0, "and it slows the car: {speed_on} against {speed_off}");
    let _ = spin_on;
}

// ------------------------------------------------------------------------------------------------ the ground

/// A box ramp of 12 degrees rising toward -z, starting at the ground at z = -10 (`rest` is the same everywhere).
fn ramp_world() -> Sim {
    let th = 12.0f32.to_radians();
    let mut s = Sim::new(1);
    s.threads = 1;
    flat(&mut s);
    let (h, l) = (0.4f32, 14.0f32);
    let c = [0.0, -(h / 2.0) * th.cos() + (l / 2.0) * th.sin(), -10.0 - (h / 2.0) * th.sin() - (l / 2.0) * th.cos()];
    spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":{c:?},"rot":[{},0,0,{}]}},"Collider":{{"shape":1,"size":[10,{h},{l}]}},"RigidBody":{{"friction":0.8}}}}"#, (th / 2.0).sin(), (th / 2.0).cos()));
    spawn_car(&mut s, 2, [0.0, ride_height(), -2.0], WHEELS, "");
    s
}

#[test]
fn the_car_climbs_a_ramp_and_sits_on_it_nose_up() {
    let mut s = ramp_world();
    s.step_n(90);
    drive(&mut s, 2, |v| v.throttle = 0.7);
    let mut on_ramp = None;
    let mut worst_up = 1.0f32;
    for tick in 0..420u32 {
        s.step();
        let p = pos(&s, 2);
        if p[2] > -22.5 {
            worst_up = worst_up.min(up_y(&s, 2)); // (the ramp ends at z = -23.7: after that the car is in the air)
        }
        if on_ramp.is_none() && p[2] < -14.0 && car(&s, 2).on_ground == 4 {
            on_ramp = Some((tick, p, axes(&s, 2)));
        }
    }
    let (tick, p, a) = on_ramp.expect("the car reached the ramp with all four wheels down");
    let th = 12.0f32.to_radians();
    let surface = (-10.0 - p[2]) * th.tan();
    let pitch = a[2][1].asin().to_degrees(); // (the car's back axis points down the slope when its nose is up: its y component is the sine of the pitch)
    eprintln!("on the ramp at tick {tick}: z {:.2}, the chassis {:.3} m above the ramp's surface line (ride height / cos {:.3}), the nose up {:.1} degrees (the ramp: 12)", p[2], p[1] - surface, ride_height() / th.cos(), pitch.abs());
    assert!((p[1] - surface - ride_height() / th.cos()).abs() < 0.12, "ride height on the slope: {} over a surface at {surface}", p[1]);
    assert!((pitch.abs() - 12.0).abs() < 3.0, "the car lies on the slope: {pitch} degrees");
    assert!(worst_up > 0.9, "and did not tip: {worst_up}");
    assert!(pos(&s, 2)[2] < -16.0 || pos(&s, 2)[1] > 1.0, "it drove on up it: {:?}", pos(&s, 2));
}

#[test]
fn a_car_that_jumps_the_ramp_lands_on_its_wheels_and_drives_on() {
    let mut s = ramp_world();
    s.step_n(90);
    drive(&mut s, 2, |v| v.throttle = 1.0);
    let (mut flying, mut peak, mut worst_up, mut fastest_fall) = (0u32, 0.0f32, 1.0f32, 0.0f32);
    for _ in 0..720u32 {
        s.step();
        let v = car(&s, 2);
        if v.on_ground == 0 {
            flying += 1;
        }
        peak = peak.max(pos(&s, 2)[1]);
        fastest_fall = fastest_fall.min(lin(&s, 2)[1]);
        if pos(&s, 2)[2] < -24.0 {
            worst_up = worst_up.min(up_y(&s, 2)); // (after the end of the ramp: in the air and on landing)
        }
        assert!(pos(&s, 2).iter().all(|x| x.is_finite()) && len(lin(&s, 2)) < 80.0, "it stays a number and below 80 m/s");
    }
    eprintln!("the jump: {flying} ticks with no wheel on the ground, highest point {peak:.2} m, fastest fall {fastest_fall:.1} m/s, the up axis never below {worst_up:.2}, afterwards {:.1} m/s with {} wheels down", forward_speed(&s, 2), car(&s, 2).on_ground);
    assert!(flying > 10, "it left the ground: {flying} ticks");
    assert!(worst_up > 0.8, "and came down on its wheels: {worst_up}");
    assert_eq!(car(&s, 2).on_ground, 4);
    assert!(forward_speed(&s, 2) > 10.0, "and drives on: {}", forward_speed(&s, 2));
}

#[test]
fn automatic_springs_hold_a_buggy_a_car_and_a_truck_at_the_same_share_of_their_travel() {
    // (mass, chassis box, wheels): the weight of every wheel is the same share of the spring, so the sag is a quarter of the travel on each of them
    let buggy = ("defaults: radius=0.3 rest=0.25 travel=0.16 spring=auto damper=auto\nfl: -0.55 -0.1 -0.8 steer=1\nfr: 0.55 -0.1 -0.8 steer=1\nrl: -0.55 -0.1 0.8 drive=1\nrr: 0.55 -0.1 0.8 drive=1", 150.0f32, [1.3f32, 0.4, 2.3]);
    let sedan_table: &'static str = Box::leak(WHEELS.replace("spring=35000 damper=3200", "spring=auto damper=auto").into_boxed_str());
    let sedan = (sedan_table, MASS, [1.8f32, 0.7, 4.2]);
    let truck = ("defaults: radius=0.55 rest=0.45 travel=0.3 spring=auto damper=auto\nf1: -1.0 -0.2 -3.0 steer=1\nf2: 1.0 -0.2 -3.0 steer=1\nm1: -1.0 -0.2 0.0 drive=1\nm2: 1.0 -0.2 0.0 drive=1\nb1: -1.0 -0.2 3.0 drive=1\nb2: 1.0 -0.2 3.0 drive=1", 12000.0, [2.4, 1.2, 8.0]);
    for (name, (table, mass, size)) in [("buggy", buggy), ("car", sedan), ("truck", truck)] {
        let mut s = Sim::new(1);
        s.threads = 1;
        flat(&mut s);
        spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,1.6,0]}},"Velocity":{{}},"AngularVelocity":{{}},"Collider":{{"shape":1,"size":{size:?}}},"RigidBody":{{"mass":{mass}}},"Vehicle":{{"wheels":"{}","engine_force":{}}}}}"#, escape(table), mass * 7.0));
        s.step_n(360);
        let v = car(&s, 2);
        let travel = parse_wheels(table).unwrap()[0].travel;
        let n = parse_wheels(table).unwrap().len();
        eprintln!("{name}: {mass} kg on {n} wheels: sag {:?} (a quarter of the travel is {:.4})", &v.compression[..n], 0.25 * travel);
        assert_eq!(v.on_ground as usize, n, "{name}");
        for c in &v.compression[..n] {
            assert!((c - 0.25 * travel).abs() < 0.12 * travel, "{name}: a wheel sags {c}, a quarter of the travel is {}", 0.25 * travel);
        }
        // and it drives: full throttle for 4 s, no explosion, it speeds up
        drive(&mut s, 2, |c| c.throttle = 1.0);
        s.step_n(240);
        assert!(forward_speed(&s, 2) > 4.0 && len(lin(&s, 2)) < 80.0 && pos(&s, 2)[1] < 3.0, "{name}: {} m/s at {:?}", forward_speed(&s, 2), pos(&s, 2));
        assert_eq!(car(&s, 2).on_ground as usize, n, "{name}");
    }
}

#[test]
fn wheels_stand_on_mesh_levels_terrain_and_hulls_like_on_boxes() {
    let d = dir_for("grounds");
    std::fs::write(d.join("floor.glb"), glb_of(&[[[-100.0, 0.0, -100.0], [-100.0, 0.0, 100.0], [100.0, 0.0, 100.0]], [[-100.0, 0.0, -100.0], [100.0, 0.0, 100.0], [100.0, 0.0, -100.0]]])).unwrap();
    // a unit cube, for a slab of a hull
    let c = |x: f32, y: f32, z: f32| [x * 0.5, y * 0.5, z * 0.5];
    let quad = |a, b, cc, dd| vec![[a, b, cc], [a, cc, dd]];
    let mut cube: Vec<[V3; 3]> = Vec::new();
    cube.extend(quad(c(-1.0, 1.0, -1.0), c(-1.0, 1.0, 1.0), c(1.0, 1.0, 1.0), c(1.0, 1.0, -1.0)));
    cube.extend(quad(c(-1.0, -1.0, -1.0), c(1.0, -1.0, -1.0), c(1.0, -1.0, 1.0), c(-1.0, -1.0, 1.0)));
    cube.extend(quad(c(-1.0, -1.0, -1.0), c(-1.0, 1.0, -1.0), c(1.0, 1.0, -1.0), c(1.0, -1.0, -1.0)));
    cube.extend(quad(c(-1.0, -1.0, 1.0), c(1.0, -1.0, 1.0), c(1.0, 1.0, 1.0), c(-1.0, 1.0, 1.0)));
    cube.extend(quad(c(-1.0, -1.0, -1.0), c(-1.0, -1.0, 1.0), c(-1.0, 1.0, 1.0), c(-1.0, 1.0, -1.0)));
    cube.extend(quad(c(1.0, -1.0, -1.0), c(1.0, 1.0, -1.0), c(1.0, 1.0, 1.0), c(1.0, -1.0, 1.0)));
    std::fs::write(d.join("cube.glb"), glb_of(&cube)).unwrap();
    std::fs::write(d.join("flat.png"), make_png(33, 33, 16, 0, 4, &vec![0u16; 33 * 33])).unwrap();
    let grounds = [
        ("a box", r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[300,1,300]}}"#),
        ("a mesh of triangles", r#"{"Transform":{},"MeshCollider":{"mesh":"floor.glb"}}"#),
        ("a terrain", r#"{"Transform":{},"Terrain":{"heightmap":"flat.png","size":[300,300],"height":10}}"#),
        ("a static hull", r#"{"Transform":{"pos":[0,-0.5,0],"scale":[300,1,300]},"ConvexCollider":{"mesh":"cube.glb"}}"#),
        ("a capsule lying down", r#"{"Transform":{"pos":[0,-200,0],"rot":[0,0,0.70710678,0.70710678]},"Collider":{"shape":2,"size":[400,2000,400]}}"#),
        ("a huge ball", r#"{"Transform":{"pos":[0,-5000,0]},"Collider":{"shape":0,"size":[10000,10000,10000]}}"#),
    ];
    for (name, ground) in grounds {
        let mut s = Sim::new(1);
        s.threads = 1;
        s.asset_root = Some(d.clone());
        spawn(&mut s, 1, ground);
        spawn_car(&mut s, 2, [0.0, ride_height() + 0.2, 0.0], WHEELS, "");
        s.step_n(300);
        let y = pos(&s, 2)[1];
        let v = car(&s, 2);
        eprintln!("on {name}: the chassis at {y:.4} (box: {:.4}), {} wheels down", ride_height(), v.on_ground);
        assert_eq!(v.on_ground, 4, "{name}");
        assert!((y - ride_height()).abs() < 0.01, "{name}: ride height {y}");
        assert!(len(lin(&s, 2)) < 0.02, "{name}: at rest");
    }
}

#[test]
fn wheels_ignore_what_moves_and_what_the_chassis_does_not_collide_with() {
    // a crate lying on the ground under the car does not hold it up: it is a body that moves
    let mut s = Sim::new(1);
    s.threads = 1;
    flat(&mut s);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,0.5,0]},"Velocity":{},"Collider":{"shape":1,"size":[8,1,8]},"RigidBody":{"mass":5000}}"#);
    // (the struts start 1.4 m up and reach 0.65 m down: the crate's top at 1.0 is in reach, the floor at 0 is not)
    spawn_car(&mut s, 2, [0.0, 1.55, 0.0], WHEELS, "");
    s.step();
    assert_eq!(car(&s, 2).on_ground, 0, "a body that moves is not ground for the wheels");
    // layers: ground and car that want each other, one that does not
    let run = |ground_layer: &str, car_layer: &str| {
        let mut s = Sim::new(1);
        s.threads = 1;
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,-0.5,0]}},"Collider":{{"shape":1,"size":[300,1,300]}}{ground_layer}}}"#));
        spawn_car(&mut s, 2, [0.0, ride_height() + 0.05, 0.0], WHEELS, car_layer);
        s.step_n(240);
        (pos(&s, 2)[1], car(&s, 2).on_ground)
    };
    let (y, wheels) = run(r#","CollisionLayer":{"layer":1,"mask":4}"#, r#","CollisionLayer":{"layer":2,"mask":2}"#);
    assert!((y - ride_height()).abs() < 0.01 && wheels == 4, "layers that agree: {y}");
    let (y, wheels) = run(r#","CollisionLayer":{"layer":1,"mask":8}"#, r#","CollisionLayer":{"layer":2,"mask":2}"#);
    assert!(y < -3.0 && wheels == 0, "the ground leaves the car's layer out: it falls through ({y}, {wheels} wheels)");
    let (y, wheels) = run(r#","CollisionLayer":{"layer":1,"mask":4}"#, r#","CollisionLayer":{"layer":2,"mask":8}"#);
    assert!(y < -3.0 && wheels == 0, "the car leaves the ground's layer out: {y}");
    let (y, wheels) = run("", "");
    assert!((y - ride_height()).abs() < 0.01 && wheels == 4, "no layers at all: {y}");
}

// ------------------------------------------------------------------------------------------------ the models of the wheels

#[test]
fn wheel_models_take_their_place_on_the_strut_steer_and_roll() {
    let mut s = Sim::new(1);
    s.threads = 1;
    flat(&mut s);
    let table = format!("{}", WHEELS.replace("fl: -0.8 -0.15 -1.3 steer=1 brake=1", "fl: -0.8 -0.15 -1.3 steer=1 brake=1 entity=10").replace("rr: 0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1", "rr: 0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1 entity=11"));
    spawn_car(&mut s, 2, [0.0, ride_height(), 0.0], &table, "");
    // the front left model is a child of the chassis; the rear right one is a free entity
    spawn(&mut s, 10, r#"{"Transform":{},"Parent":{"target":2},"LocalTransform":{"pos":[0,0,0]}}"#);
    spawn(&mut s, 11, r#"{"Transform":{}}"#);
    s.step_n(120);
    let x = MASS * 9.8 / 4.0 / 35000.0;
    let local = s.world.get::<LocalTransform>(s.world.entity(10).unwrap()).unwrap();
    assert!((local.pos[0] + 0.8).abs() < 1e-5 && (local.pos[2] + 1.3).abs() < 1e-5, "{:?}", local.pos);
    assert!((local.pos[1] - (-0.15 - (0.3 - x))).abs() < 0.005, "the wheel's centre hangs `rest - compression` below the mount point: {}", local.pos[1]);
    let free = pos(&s, 11);
    assert!((free[0] - 0.8).abs() < 0.01 && (free[2] - 1.3).abs() < 0.01 && (free[1] - 0.35).abs() < 0.01, "the free model stands with its centre one radius over the ground: {free:?}");
    // driving and steering: the wheel rolls (its quaternion turns about x) and the front one steers (about y)
    drive(&mut s, 2, |v| {
        v.throttle = 0.5;
        v.steer = 1.0;
    });
    s.step_n(90);
    let local = s.world.get::<LocalTransform>(s.world.entity(10).unwrap()).unwrap();
    let q = local.rot;
    assert!(q[1].abs() > 0.05, "the front wheel is steered about the vertical: {q:?}");
    assert!(car(&s, 2).spin[0].abs() > 0.0 && car(&s, 2).spin[2].abs() > 0.0, "and every wheel has rolled");
    let rr = s.world.get::<Transform>(s.world.entity(11).unwrap()).unwrap().rot;
    assert!(rr.iter().all(|c| c.is_finite()) && (rr.iter().map(|c| c * c).sum::<f32>() - 1.0).abs() < 1e-3, "a unit quaternion: {rr:?}");
}

// ------------------------------------------------------------------------------------------------ the promises of the engine

fn scenario(threads: usize) -> Sim {
    let mut s = Sim::new(1);
    s.threads = threads;
    flat(&mut s);
    spawn_car(&mut s, 2, [0.0, ride_height() + 0.1, 0.0], WHEELS, "");
    spawn_car(&mut s, 3, [8.0, ride_height() + 0.1, 0.0], WHEELS, "");
    let ramp = r#"{"Transform":{"pos":[0,0.5,-20],"rot":[0.1,0,0,0.995]},"Collider":{"shape":1,"size":[12,0.4,10]},"RigidBody":{"friction":0.8}}"#;
    spawn(&mut s, 4, ramp);
    for i in 0..30u64 {
        spawn(&mut s, 20 + i, &format!(r#"{{"Transform":{{"pos":[{},{},{}]}},"Velocity":{{}},"AngularVelocity":{{}},"Collider":{{"shape":1,"size":[0.6,0.6,0.6]}},"RigidBody":{{"mass":20}}}}"#, -6.0 + (i % 5) as f32 * 1.1, 0.3 + (i / 5) as f32 * 0.65, -12.0 - (i / 5) as f32 * 0.3));
    }
    s
}

fn play(s: &mut Sim, from: u32, to: u32) {
    for tick in from..to {
        drive(s, 2, |v| {
            v.throttle = if tick % 300 < 220 { 0.8 } else { -0.5 };
            v.steer = ((tick / 50) % 4) as f32 * 0.5 - 0.7;
            v.brake = if tick % 300 > 270 { 0.6 } else { 0.0 };
        });
        drive(s, 3, |v| {
            v.throttle = 1.0;
            v.steer = if tick % 120 < 60 { 0.3 } else { -0.3 };
            v.handbrake = if tick % 240 > 200 { 1.0 } else { 0.0 };
        });
        s.step();
    }
}

#[test]
fn a_run_is_the_same_for_any_thread_count_and_from_a_snapshot_and_a_replay() {
    let hashes: Vec<u64> = [1usize, 2, 5, 8].iter().map(|t| {
        let mut s = scenario(*t);
        play(&mut s, 0, 420);
        s.state_hash()
    }).collect();
    assert!(hashes.windows(2).all(|h| h[0] == h[1]), "the thread count changed the result: {hashes:x?}");
    // a copy taken in the middle plays on to the same hash
    let mut a = scenario(1);
    play(&mut a, 0, 200);
    let mut b = a.clone();
    play(&mut a, 200, 420);
    play(&mut b, 200, 420);
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(a.state_hash(), hashes[0], "and it is the same run as the one that never stopped");
    // a checkpoint taken in the middle, rewound to after the run went on, plays the same future
    let mut c = scenario(1);
    play(&mut c, 0, 200);
    let cp = c.checkpoint();
    play(&mut c, 200, 420);
    let end = c.state_hash();
    c.rewind_to(&cp).unwrap();
    play(&mut c, 200, 420);
    assert_eq!(c.state_hash(), end);
    // a recording of the inputs replays to the same end: the inputs are `Set` commands on the Vehicle, recorded as they are given
    let mut r = scenario(1);
    let template = r.clone();
    r.start_recording().unwrap();
    for tick in 0..300u32 {
        let (th, st) = (if tick % 200 < 150 { 1.0 } else { 0.0 }, ((tick / 40) % 3) as f32 - 1.0);
        r.apply(&Command::Set { uid: 2, component: "Vehicle".into(), value: parse(&format!(r#"{{"throttle":{th},"steer":{st}}}"#)).unwrap() }).unwrap();
        r.step();
    }
    let rec = r.stop_recording().unwrap();
    let again = Sim::replay(&template, &rec).unwrap();
    assert_eq!(again.state_hash(), rec.final_hash);
    assert_eq!(again.state_hash(), r.state_hash());
}

#[test]
fn a_world_without_a_vehicle_never_runs_the_vehicle_code() {
    let mut s = Sim::new(1);
    s.threads = 1;
    flat(&mut s);
    for i in 0..10u64 {
        spawn(&mut s, 10 + i, &format!(r#"{{"Transform":{{"pos":[{},2,0]}},"Velocity":{{}},"AngularVelocity":{{}},"Collider":{{"shape":1,"size":[0.8,0.8,0.8]}},"RigidBody":{{"mass":1}}}}"#, i as f32));
    }
    let before = work_done();
    s.step_n(120);
    assert_eq!(work_done(), before, "no vehicle: the step found nothing to do");
    // with one it does
    let mut t = garage();
    let before = work_done();
    t.step_n(10);
    assert_eq!(work_done(), before + 10);
    // a vehicle that cannot work (no wheels, a bad table, no rigid body) is left alone and nothing panics
    let mut u = Sim::new(1);
    u.threads = 1;
    flat(&mut u);
    spawn_car(&mut u, 2, [0.0, 2.0, 0.0], "", "");
    spawn_car(&mut u, 3, [5.0, 2.0, 0.0], "fl: nonsense", "");
    spawn(&mut u, 4, r#"{"Transform":{"pos":[9,2,0]},"Vehicle":{"wheels":"fl: 0 0 0"}}"#);
    u.step_n(30);
    assert_eq!(car(&u, 2).on_ground, 0);
    assert!(pos(&u, 2)[1].is_finite());
}

#[test]
fn nonsense_inputs_and_numbers_do_not_break_the_car() {
    let mut s = garage();
    s.step_n(60);
    drive(&mut s, 2, |v| {
        v.throttle = f32::NAN;
        v.steer = f32::INFINITY;
        v.brake = -5.0;
        v.handbrake = 1.0e9;
        v.engine_force = f32::NAN;
        v.steer_max = 1.0e9;
        v.anti_roll = -1.0e9;
        v.roll_influence = 100.0;
        v.top_speed = -3.0;
    });
    s.step_n(120);
    let p = pos(&s, 2);
    assert!(p.iter().all(|x| x.is_finite()) && len(lin(&s, 2)).is_finite(), "{p:?}");
    // absurd wheels
    let mut t = Sim::new(1);
    t.threads = 1;
    flat(&mut t);
    spawn_car(&mut t, 2, [0.0, 1.0, 0.0], "a: 0 0 0 radius=0 rest=0 travel=-4 spring=0 damper=-1 grip=1e30 steer=1e30\nb: 1 0 0 radius=1e9 rest=1e9 spring=1e30 damper=1e30", "");
    t.step_n(120);
    assert!(pos(&t, 2).iter().all(|x| x.is_finite()), "{:?}", pos(&t, 2));
}

// ------------------------------------------------------------------------------------------------ what it costs

#[test]
fn what_a_vehicle_costs() {
    let build = |cars: u64, boxes: u64| {
        let mut s = Sim::new(1);
        s.threads = 1;
        flat(&mut s);
        for i in 0..cars {
            spawn_car(&mut s, 100 + i, [(i % 10) as f32 * 8.0, ride_height(), (i / 10) as f32 * 8.0], WHEELS, "");
        }
        for i in 0..boxes {
            spawn(&mut s, 1000 + i, &format!(r#"{{"Transform":{{"pos":[{},0.5,{}]}},"Collider":{{"shape":1,"size":[1,1,1]}}}}"#, (i % 100) as f32 * 3.0 - 150.0, (i / 100) as f32 * 3.0 - 150.0));
        }
        s
    };
    let time = |cars: u64, boxes: u64, drive_them: bool| {
        let mut s = build(cars, boxes);
        s.step_n(60);
        if drive_them {
            for i in 0..cars {
                drive(&mut s, 100 + i, |v| {
                    v.throttle = 0.5;
                    v.steer = 0.3;
                });
            }
        }
        let t0 = std::time::Instant::now();
        s.step_n(240);
        t0.elapsed().as_secs_f64() * 1000.0 / 240.0
    };
    let none = time(0, 0, false);
    let one = time(1, 0, true);
    let ten = time(10, 0, true);
    let fifty = time(50, 0, true);
    let one_big = time(1, 5000, true);
    let none_big = time(0, 5000, false);
    eprintln!("a tick: {none:.4} ms with no car, {one:.4} ms with one driven car, {ten:.4} ms with 10, {fifty:.4} ms with 50; with 5,000 static boxes about: {none_big:.4} ms without a car, {one_big:.4} ms with one");
    eprintln!("per vehicle: {:.1} microseconds at 10 cars, {:.1} at 50 (the physics of the chassis included)", (ten - none) * 1000.0 / 10.0, (fifty - none) * 1000.0 / 50.0);
    // the vehicle step alone, called over and over on a world that does not move (so the wheels keep touching the ground): what the wheels and the tyres cost
    let alone = |cars: u64, boxes: u64| {
        let mut s = build(cars, boxes);
        s.step_n(60);
        for i in 0..cars {
            drive(&mut s, 100 + i, |v| {
                v.throttle = 0.5;
                v.steer = 0.3;
            });
        }
        let (fields, meshes) = (s.heightfields.clone(), s.meshes.clone());
        let t0 = std::time::Instant::now();
        for _ in 0..2000 {
            crate::vehicle::step(&mut s.world, [0.0, -9.8, 0.0], 1.0 / 60.0, &fields, &meshes);
        }
        t0.elapsed().as_secs_f64() * 1e6 / 2000.0
    };
    let (a1, a10, a50, a1_big) = (alone(1, 0), alone(10, 0), alone(50, 0), alone(1, 5000));
    eprintln!("the vehicle step alone: {a1:.1} microseconds for one car, {:.1} each at 10, {:.1} each at 50; one car among 5,000 static boxes: {a1_big:.1} microseconds", a10 / 10.0, a50 / 50.0);
    assert!(one < 1.0 && ten < 5.0, "a vehicle is cheap: {one} ms, {ten} ms");
}
