//! Inertialization on the real mannequin of a game (`LUNARIS_TEST_MANNEQUIN=path/to/mannequin.glb cargo test --release -p lunaris-render inertia_real -- --ignored --nocapture`): the pose is continuous where the
//! switch happens (position and velocity of the pelvis, hands and feet), ends exactly on the new clip, and never moves faster than the clips themselves do.
use crate::rig::{InertiaChain, PoseLayer, Rig, INERTIA_MAX};

fn rig() -> Option<Rig> {
    let path = std::env::var("LUNARIS_TEST_MANNEQUIN").ok()?;
    let bytes = std::fs::read(path).ok()?;
    let imported = crate::gltf::import(&bytes, None).ok()?;
    imported.mesh.rig.as_ref().map(|r| (**r).clone())
}

fn positions(rig: &Rig, layer: PoseLayer, joints: &[usize]) -> Vec<[f32; 3]> {
    let w = rig.joint_worlds(&[layer], &[]);
    joints.iter().map(|&j| [w[j].0[12], w[j].0[13], w[j].0[14]]).collect()
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[test]
#[ignore]
fn switches_between_real_clips_are_continuous_smooth_and_end_on_the_new_clip() {
    let Some(rig) = rig() else {
        eprintln!("LUNARIS_TEST_MANNEQUIN is not set: nothing to check");
        return;
    };
    let names = ["pelvis", "hand_r", "hand_l", "foot_l", "foot_r", "head"];
    let joints: Vec<usize> = names.iter().map(|n| rig.joint_index(n).unwrap_or_else(|| panic!("no joint {n}"))).collect();
    let pairs = [
        ("M_Neutral_Run_Loop_F", 1.0, "M_Neutral_Run_Stop_F_Lfoot", 0.55),
        // a loop that has gone past its end (it wraps) into a clip that does not loop (it would hold its last pose)
        ("M_Neutral_Run_Loop_F_R_20", 2.6, "M_Neutral_Run_Box_F_LL_Rfoot", 0.7),
        ("M_Neutral_Stand_Idle_Loop", 2.0, "M_Neutral_Run_Start_F_Rfoot", 0.2),
        ("M_Neutral_Run_Turn_L_090_Lfoot", 0.7, "M_Neutral_Run_Turn_L_180_Rfoot", 0.9),
        ("M_Neutral_Walk_Loop_F", 1.3, "M_Neutral_Run_Loop_F", 0.4),
        ("M_Neutral_Jump_F_Start_Run_Rfoot", 0.9, "M_Neutral_Jump_F_Land_Run_Light_Lfoot", 0.55),
    ];
    let h = 1.0 / 60.0;
    for (from, from_t, to, to_t) in pairs {
        let (a, b) = (rig.clip_index(from).unwrap_or_else(|| panic!("no clip {from}")), rig.clip_index(to).unwrap_or_else(|| panic!("no clip {to}")));
        let span = 0.3;
        let chain_at = |u: f32| {
            let mut c = InertiaChain { n: 2, looping: [false; INERTIA_MAX], clip: [None; INERTIA_MAX], t0: [0.0; INERTIA_MAX], rate: [1.0; INERTIA_MAX], gap: [0.0; INERTIA_MAX], span: [0.0; INERTIA_MAX] };
            c.clip[0] = Some(a);
            c.looping[0] = rig.clips[a].name.contains("Loop");
            c.t0[0] = from_t - 0.4;
            c.gap[0] = 0.4;
            c.clip[1] = Some(b);
            c.t0[1] = to_t;
            c.gap[1] = u;
            c.span[1] = span;
            PoseLayer { clip: Some(b), time: to_t + u, looping: false, weight: 1.0, root: None, inertia: Some(c) }
        };
        let old = |u: f32| positions(&rig, PoseLayer { clip: Some(a), time: from_t + u, looping: rig.clips[a].name.contains("Loop"), weight: 1.0, root: None, inertia: None }, &joints);
        let new = |u: f32| positions(&rig, PoseLayer { clip: Some(b), time: to_t + u, looping: false, weight: 1.0, root: None, inertia: None }, &joints);
        // continuous at the switch
        let p0 = positions(&rig, chain_at(0.0), &joints);
        let worst_start = p0.iter().zip(old(0.0).iter()).map(|(x, y)| dist(*x, *y)).fold(0.0f32, f32::max);
        assert!(worst_start < 0.002, "{from} -> {to}: starts {worst_start} m from the old pose");
        // ends on the new clip
        let pe = positions(&rig, chain_at(span + 0.001), &joints);
        let worst_end = pe.iter().zip(new(span + 0.001).iter()).map(|(x, y)| dist(*x, *y)).fold(0.0f32, f32::max);
        assert!(worst_end < 1e-4, "{from} -> {to}: ends {worst_end} m from the new clip");
        // the speed of every joint through the blend against the speeds of the clips themselves
        let steps = (span / h) as usize + 2;
        let mut prev = p0.clone();
        let (mut worst_speed, mut clip_speed) = (0.0f32, 0.0f32);
        for k in 1..=steps {
            let u = k as f32 * h;
            let now = positions(&rig, chain_at(u), &joints);
            for j in 0..joints.len() {
                worst_speed = worst_speed.max(dist(now[j], prev[j]) / h);
            }
            prev = now;
            let (o0, o1, n0, n1) = (old(u - h), old(u), new(u - h), new(u));
            for j in 0..joints.len() {
                clip_speed = clip_speed.max(dist(o0[j], o1[j]) / h).max(dist(n0[j], n1[j]) / h);
            }
        }
        eprintln!("{from} -> {to}: starts {:.1} mm off, fastest joint {:.2} m/s (the clips' own fastest {:.2} m/s)", worst_start * 1000.0, worst_speed, clip_speed);
        assert!(worst_speed < clip_speed * 2.0 + 0.5, "{from} -> {to}: the blend moves a joint at {worst_speed} m/s, the clips at most {clip_speed}");
    }
}
