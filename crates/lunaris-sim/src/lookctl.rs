//! `MouseLook`: first-person (and free) look from the mouse and the right stick. The player (`lunaris play`) writes `input.mouse_dx` and `input.mouse_dy` (the mouse's
//! motion in pixels since the last tick: right and down are positive) and `input.mouse_wheel`, and game pads write `input.gamepad.rx` and `.ry`; an entity with a
//! `MouseLook` turns by them every tick, after the graph systems.
//!
//! * On a free entity (a camera that is not attached to anything) the whole look (`yaw` about the vertical, then `pitch`) goes into its `Transform.rot`.
//! * On an entity attached to a body (`Parent` and `LocalTransform`: the usual first-person camera on a character) only the `pitch` goes into the
//!   `LocalTransform.rot`, and the yaw turns the body (`body`, which is the parent when it is 0): the body then faces where you look, and `forward` and `right` (set
//!   here, on the ground plane) tell a graph system which way "ahead" is: `CharacterBody.walk = right * action.move_x + forward * action.move_y`.
use crate::components::{LocalTransform, MouseLook, Parent, Transform};
use crate::detmath;
use lunaris_ecs::{Entity, World};

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

/// The rotation of `yaw` about the vertical followed by `pitch` about the side (x, y, z, w).
pub fn look_quat(yaw: f32, pitch: f32) -> [f32; 4] {
    let (sy, cy) = (detmath::sin(yaw as f64 * 0.5) as f32, detmath::cos(yaw as f64 * 0.5) as f32);
    let (sx, cx) = (detmath::sin(pitch as f64 * 0.5) as f32, detmath::cos(pitch as f64 * 0.5) as f32);
    [cy * sx, sy * cx, -sy * sx, cy * cx]
}

pub fn update(world: &mut World, dt: f32) {
    let mut found: Vec<(u64, Entity, MouseLook)> = Vec::new();
    let _ = world.for_each_uid::<(&MouseLook,), _>(|e, uid, (m,)| found.push((uid, e, *m)));
    if found.is_empty() {
        return;
    }
    found.sort_by_key(|f| f.0);
    let res = |w: &World, n: &str| w.resource(n).filter(|v| v.is_finite()).unwrap_or(0.0) as f32;
    let (dx, dy) = (res(world, "input.mouse_dx"), res(world, "input.mouse_dy"));
    let (rx, ry) = (res(world, "input.gamepad.rx"), res(world, "input.gamepad.ry"));
    for (_, e, mut m) in found {
        if m.enabled {
            let sens = finite(m.sensitivity, 0.0022);
            let stick = finite(m.stick_speed, 2.5) * dt;
            let flip = if m.invert_y { -1.0 } else { 1.0 };
            m.yaw = finite(m.yaw, 0.0) - dx * sens - rx * stick;
            m.pitch = finite(m.pitch, 0.0) - dy * sens * flip + ry * stick * flip;
        }
        // keep the yaw a small number and the pitch within what a neck can do
        let tau = std::f32::consts::TAU;
        if m.yaw.abs() > 1000.0 {
            m.yaw %= tau;
        }
        let (lo, hi) = (finite(m.pitch_min, -1.5).clamp(-1.57, 1.57), finite(m.pitch_max, 1.5).clamp(-1.57, 1.57));
        m.pitch = m.pitch.clamp(lo.min(hi), hi.max(lo));
        let (sy, cy) = (detmath::sin(m.yaw as f64) as f32, detmath::cos(m.yaw as f64) as f32);
        m.forward = [-sy, 0.0, -cy];
        m.right = [cy, 0.0, -sy];
        // attached to a body: the pitch is the camera's own, the yaw is the body's
        let attached = world.get::<Parent>(e).map(|p| p.target.0).filter(|t| *t != 0).filter(|_| world.get::<LocalTransform>(e).is_some());
        match attached {
            Some(parent) => {
                if let Some(l) = world.get_mut::<LocalTransform>(e) {
                    l.rot = look_quat(0.0, m.pitch);
                }
                let body = if m.body.0 != 0 { m.body.0 } else { parent };
                if let Some(b) = world.entity(body) {
                    if let Some(t) = world.get_mut::<Transform>(b) {
                        t.rot = look_quat(m.yaw, 0.0);
                    }
                }
            }
            None => {
                if let Some(t) = world.get_mut::<Transform>(e) {
                    t.rot = look_quat(m.yaw, m.pitch);
                }
                if m.body.0 != 0 {
                    if let Some(b) = world.entity(m.body.0) {
                        if let Some(t) = world.get_mut::<Transform>(b) {
                            t.rot = look_quat(m.yaw, 0.0);
                        }
                    }
                }
            }
        }
        if let Some(w) = world.get_mut::<MouseLook>(e) {
            *w = m;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::Sim;
    use lunaris_json::parse as json;

    fn spawn(s: &mut Sim, uid: u64, comps: &str) {
        s.apply(&Command::Spawn { uid: Some(uid), components: json(comps).unwrap().as_object().unwrap().to_vec() }).unwrap();
    }

    fn set(s: &mut Sim, name: &str, v: f64) {
        s.apply(&Command::SetResource { name: name.into(), value: Some(v) }).unwrap();
    }

    fn look(s: &Sim, uid: u64) -> MouseLook {
        *s.world.get::<MouseLook>(s.world.entity(uid).unwrap()).unwrap()
    }

    fn rot(s: &Sim, uid: u64) -> [f32; 4] {
        s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().rot
    }

    #[test]
    fn the_quaternion_turns_the_way_it_should() {
        // a quarter turn to the left (yaw +90 degrees) points the camera's -z at -x
        let q = look_quat(std::f32::consts::FRAC_PI_2, 0.0);
        assert!((q[1] - 0.70710677).abs() < 1e-5 && (q[3] - 0.70710677).abs() < 1e-5 && q[0].abs() < 1e-6);
        // looking up tilts -z towards +y: rotate (0,0,-1) by the pitch quaternion
        let q = look_quat(0.0, 0.5);
        let (x, w) = (q[0], q[3]);
        // rotation about x by 0.5: y' = -z * sin(0.5) for z = -1
        let (s, c) = (2.0 * x * w, 1.0 - 2.0 * x * x);
        assert!((s - 0.5f32.sin()).abs() < 1e-4 && (c - 0.5f32.cos()).abs() < 1e-4);
    }

    #[test]
    fn mouse_motion_turns_a_free_camera_and_the_pitch_stops_at_the_limits() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{},"Camera":{},"MouseLook":{"sensitivity":0.01}}"#);
        s.step();
        assert_eq!((look(&s, 1).yaw, look(&s, 1).pitch), (0.0, 0.0));
        assert_eq!(rot(&s, 1), [0.0, 0.0, 0.0, 1.0]);
        // 100 pixels right turn right (the yaw falls), 50 down look down
        set(&mut s, "input.mouse_dx", 100.0);
        set(&mut s, "input.mouse_dy", 50.0);
        s.step();
        let m = look(&s, 1);
        assert!((m.yaw + 1.0).abs() < 1e-5 && (m.pitch + 0.5).abs() < 1e-5, "{} {}", m.yaw, m.pitch);
        // the way ahead on the ground follows the yaw: yaw -1 turns right of -z
        assert!(m.forward[0] > 0.8 && m.forward[2] < -0.5 && m.forward[1] == 0.0, "{:?}", m.forward);
        assert!(m.right[2] > 0.8, "{:?}", m.right);
        // a long way down stops at the limit (1.5 radians by default), and inverted y flips it
        set(&mut s, "input.mouse_dy", 100000.0);
        s.step();
        assert!((look(&s, 1).pitch + 1.5).abs() < 1e-6);
        set(&mut s, "input.mouse_dx", 0.0);
        set(&mut s, "input.mouse_dy", 0.0);
        s.apply(&Command::Set { uid: 1, component: "MouseLook".into(), value: json(r#"{"invert_y":true,"pitch":0}"#).unwrap() }).unwrap();
        set(&mut s, "input.mouse_dy", 10.0);
        s.step();
        assert!(look(&s, 1).pitch > 0.0);
        // disabled: nothing moves
        s.apply(&Command::Set { uid: 1, component: "MouseLook".into(), value: json(r#"{"enabled":false,"yaw":0.25}"#).unwrap() }).unwrap();
        set(&mut s, "input.mouse_dx", 500.0);
        s.step();
        assert_eq!(look(&s, 1).yaw, 0.25);
    }

    #[test]
    fn the_right_stick_looks_too() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{},"MouseLook":{"stick_speed":2}}"#);
        set(&mut s, "input.gamepad.rx", 1.0);
        set(&mut s, "input.gamepad.ry", 1.0);
        s.step_n(60);
        let m = look(&s, 1);
        assert!((m.yaw + 2.0).abs() < 0.01, "a second of full stick at 2 rad/s: {}", m.yaw);
        assert!(m.pitch > 1.4, "up is positive and the pitch stops at its limit: {}", m.pitch);
    }

    #[test]
    fn a_camera_on_a_body_takes_the_pitch_and_the_body_takes_the_yaw() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[1,0,2]}}"#);
        spawn(&mut s, 2, r#"{"Transform":{},"Parent":{"target":1},"LocalTransform":{"pos":[0,1.6,0]},"Camera":{},"MouseLook":{"sensitivity":0.01}}"#);
        set(&mut s, "input.mouse_dx", -100.0);
        set(&mut s, "input.mouse_dy", -30.0);
        s.step();
        // the body turned left by one radian and keeps its place; the camera tilted up 0.3 and kept its place on the body
        let body = rot(&s, 1);
        let want = look_quat(1.0, 0.0);
        assert!((0..4).all(|i| (body[i] - want[i]).abs() < 1e-5), "{body:?} {want:?}");
        let cam = s.world.get::<LocalTransform>(s.world.entity(2).unwrap()).unwrap();
        let want = look_quat(0.0, 0.3);
        assert!((0..4).all(|i| (cam.rot[i] - want[i]).abs() < 1e-5) && cam.pos == [0.0, 1.6, 0.0]);
        // the camera's world turn is the body's yaw and its own pitch: its position follows the body
        let world = s.world.get::<Transform>(s.world.entity(2).unwrap()).unwrap();
        assert!((world.pos[0] - 1.0).abs() < 1e-4 && (world.pos[1] - 1.6).abs() < 1e-4 && (world.pos[2] - 2.0).abs() < 1e-4, "{:?}", world.pos);
    }

    #[test]
    fn an_entity_without_a_mouse_look_is_left_alone() {
        let mut b = Sim::new(1);
        spawn(&mut b, 1, r#"{"Transform":{"pos":[1,2,3]}}"#);
        set(&mut b, "input.mouse_dx", 50.0);
        b.step_n(10);
        assert_eq!(b.world.get::<Transform>(b.world.entity(1).unwrap()).unwrap().rot, [0.0, 0.0, 0.0, 1.0]);
    }
}
