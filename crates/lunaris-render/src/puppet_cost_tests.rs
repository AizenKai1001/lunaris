//! A measure, not a test: what posing and skinning one armed mannequin (15336 vertices, 90 joints; the base clip crossfading, two upper-body layers, six turn goals) costs, split into the parts.
//!
//!     PUPPET_ROOT=/path/to/the/game/folder cargo test -p lunaris-render --release puppet_pose_cost -- --ignored --nocapture
use super::*;
use crate::math::*;
use crate::rig::{IkGoal, IkKind, PoseLayer};
use std::time::Instant;

#[test]
#[ignore]
fn puppet_pose_cost() {
    let Ok(root) = std::env::var("PUPPET_ROOT") else { return };
    let mut c = TextureCache::new(Some(std::path::PathBuf::from(root)));
    let model = c.model("assets/models/player_rifle.glb").expect("the puppet loads");
    let rig = model.rig.as_ref().expect("it has a rig");
    let clip = |n: &str| rig.clip_index(n);
    let spine = rig.joint_index("spine_01");
    let layers = vec![
        PoseLayer { clip: clip("rifle_walk"), time: 1.3, looping: true, weight: 1.0, root: None, inertia: None },
        PoseLayer { clip: clip("rifle_run"), time: 0.7, looping: true, weight: 0.4, root: None, inertia: None },
        PoseLayer { clip: clip("rifle_fire"), time: 0.1, looping: true, weight: 0.5, root: spine, inertia: None },
        PoseLayer { clip: clip("rifle_reload"), time: 0.9, looping: true, weight: 0.3, root: spine, inertia: None },
    ];
    let goals: Vec<IkGoal> = ["spine_02", "spine_03", "spine_04", "spine_05", "neck_01", "head"]
        .iter()
        .map(|j| IkGoal { joint: rig.joint_index(j).expect("joint"), kind: IkKind::Turn { axis: [1.0, 0.0, 0.0], degrees: 6.0 }, target: [0.0; 3], pin: None, pole: None, weight: 1.0 })
        .collect();
    let n = 300;
    let time = |label: &str, f: &mut dyn FnMut()| {
        f();
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        let us = t.elapsed().as_secs_f64() * 1e6 / n as f64;
        println!("{label:<52} {us:9.1} us");
        us
    };
    println!("vertices {}, joints {}", model.pos.len(), rig.joints.len());
    let m = rig.skin_matrices_ik(&layers, &goals);
    let a = time("layers + turn goals + matrices (no skinning)", &mut || {
        std::hint::black_box(rig.skin_matrices_ik(&layers, &goals));
    });
    // (what a worn piece on a mannequin costs the picture: a `BoneAttach` thing makes the renderer ask for the joints' places once more, docs/GEAR_VISIBLE.md)
    let j = time("joint_worlds (the places of the joints: what a BoneAttach asks for)", &mut || {
        std::hint::black_box(rig.joint_worlds(&layers, &goals));
    });
    let b = time("clone of the mesh alone", &mut || {
        std::hint::black_box((*model).clone());
    });
    let s = time("skin() (clone + every vertex)", &mut || {
        std::hint::black_box(rig.skin(&model, &m));
    });
    let t = time("the whole pose_layers_ik", &mut || {
        std::hint::black_box(rig.pose_layers_ik(&model, &layers, &goals));
    });
    println!("an armored mannequin (anything fixed to a joint that is drawn) costs joint_worlds on top: {:.0}% of the whole pose", 100.0 * j / t);
    println!("skinning without the clone ~ {:.1} us; the pose maths are {:.0}% of the whole, the clone {:.0}%, the vertices {:.0}%", s - b, 100.0 * a / t, 100.0 * b / t, 100.0 * (s - b) / t);
}
