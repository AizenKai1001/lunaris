//! `lunaris posetrace SCENE.json --root DIR --uid UID --joints a,b,c --script "40:;80:w;12:w,d;12:w,a" [--hero UID] [--place x,y,z] [--ik] [--world]`: (`--ik` puts the goals of inverse kinematics on the pose as the renderer does, `--world` gives the joints in the world instead of the model's space) play a scene with keys held in time segments (`TICKS:KEY,KEY`) and print, for every tick, a CSV line
//! of the posed character: the tick, the hero's place and yaw, the clip playing, how many clips the animator's inertialization chain holds, and the model-space position of every named joint exactly as the
//! renderer poses it (the inertialised pose). For looking at animation frame by frame: where a limb jumps, where the body turns back and forth, how much a joint accelerates from tick to tick.
use lunaris_api::Engine;
use lunaris_json::Json;
use lunaris_render::rig::PoseLayer;
use lunaris_render::rig::{IkGoal, IkKind, IkPin};
use lunaris_sim::components::{Animator, Ik, MeshRenderer, Transform};
use lunaris_sim::inertia::Inertia;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::process::ExitCode;

pub fn run(positional: &[String], kv: &HashMap<String, String>, root: PathBuf, seed: u64) -> ExitCode {
    let Some(scene) = positional.first() else {
        eprintln!("error: `posetrace` needs a scene: lunaris posetrace SCENE.json --uid 103 --joints pelvis,foot_l --script \"40:;80:w\"");
        return ExitCode::from(2);
    };
    let uid: u64 = kv.get("uid").and_then(|v| v.parse().ok()).unwrap_or(103);
    let hero: u64 = kv.get("hero").and_then(|v| v.parse().ok()).unwrap_or(100);
    let joint_names: Vec<String> = kv.get("joints").map(|j| j.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_else(|| vec!["pelvis".into(), "hand_l".into(), "hand_r".into(), "foot_l".into(), "foot_r".into(), "head".into()]);
    let mut segments: Vec<(u32, Vec<String>)> = Vec::new();
    for part in kv.get("script").map(String::as_str).unwrap_or("60:").split(';') {
        let (n, keys) = part.split_once(':').unwrap_or((part, ""));
        let Ok(n) = n.trim().parse::<u32>() else {
            eprintln!("error: a script segment is TICKS:KEY,KEY (found `{part}`)");
            return ExitCode::from(2);
        };
        segments.push((n, keys.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect()));
    }
    let mut engine = Engine::new(seed, root.clone());
    if let Err(e) = engine.call("load_scene", &Json::obj([("path", Json::str(scene.clone()))])) {
        eprintln!("error: {}", e.to_json().to_string_pretty().unwrap_or_default());
        return ExitCode::FAILURE;
    }
    // `--place x,y,z`: put the hero there first
    if let Some(p) = kv.get("place") {
        let v: Vec<f32> = p.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if v.len() == 3 {
            if let Some(he) = engine.sim.world.entity(hero) {
                if let Some(t) = engine.sim.world.get_mut::<Transform>(he) {
                    t.pos = [v[0], v[1], v[2]];
                }
            }
        }
    }
    // the model of the posed entity
    let mesh_name = {
        let w = &mut engine.sim.world;
        let Some(e) = w.entity(uid) else {
            eprintln!("error: no entity {uid}");
            return ExitCode::FAILURE;
        };
        let Some(m) = w.get::<MeshRenderer>(e) else {
            eprintln!("error: entity {uid} has no MeshRenderer");
            return ExitCode::FAILURE;
        };
        let sym = m.mesh.0;
        w.strings().get(sym).to_string()
    };
    let bytes = match std::fs::read(root.join(&mesh_name)) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: cannot read {mesh_name}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let imported = match lunaris_render::gltf::import(&bytes, root.join(&mesh_name).parent()) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {mesh_name}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(rig) = imported.mesh.rig.as_ref() else {
        eprintln!("error: {mesh_name} has no skeleton");
        return ExitCode::FAILURE;
    };
    let mut joints = Vec::new();
    for n in &joint_names {
        match rig.joint_index(n) {
            Some(j) => joints.push(j),
            None => {
                eprintln!("error: the skeleton has no joint `{n}`");
                return ExitCode::FAILURE;
            }
        }
    }
    println!("tick,hero_x,hero_y,hero_z,yaw,clip,time,chain,fk_gs,fk_stance,fk_line,fk_gw_l,fk_gw_r,fk_lift_l,fk_lift_r,fk_lk_lz,fk_lk_rz,fk_locked,fk_stride,fk_bias,fk_gait,fk_theta,fk_sw_l,fk_sw_r,fk_pitch,fk_tgl_y,fk_tgl_z,fk_tgr_y,fk_tgr_z,fk_wk,fk_smx,ik_gl_w,ik_gl_y,ik_gr_w,ik_gr_y,ik_fl_w,ik_fr_w,{}", joint_names.iter().flat_map(|n| ["x", "y", "z"].map(|c| format!("{n}_{c}"))).collect::<Vec<_>>().join(","));
    let with_ik = kv.contains_key("ik");
    let in_world = kv.contains_key("world");
    let mut held: BTreeSet<String> = BTreeSet::new();
    let mut tick = 0u32;
    for (n, keys) in &segments {
        let want: BTreeSet<String> = keys.iter().cloned().collect();
        let mut values: Vec<(String, Json)> = Vec::new();
        for k in held.difference(&want) {
            values.push((k.clone(), Json::Bool(false)));
        }
        for k in want.difference(&held) {
            values.push((k.clone(), Json::Bool(true)));
        }
        if !values.is_empty() {
            let _ = engine.call("input", &Json::obj([("values", Json::Obj(values))]));
        }
        held = want;
        for _ in 0..*n {
            engine.sim.step();
            tick += 1;
            let w = &mut engine.sim.world;
            let (Some(he), Some(pe)) = (w.entity(hero), w.entity(uid)) else { continue };
            let Some(ht) = w.get::<Transform>(he).map(|t| *t) else { continue };
            let (Some(a), Some(ic), Some(pt)) = (w.get::<Animator>(pe).map(|a| *a), w.get::<Inertia>(pe).map(|i| *i), w.get::<Transform>(pe).map(|t| *t)) else { continue };
            let clip_name = w.strings().get(a.clip.0).to_string();
            let clip = rig.clip_index(&clip_name);
            let chain = lunaris_render::render::inertia_chain(w.strings(), rig, &a, &ic);
            let layer = PoseLayer { clip, time: a.time, looping: a.looping, weight: 1.0, root: None, inertia: chain };
            // the goals of inverse kinematics of this model, as the renderer puts them (in the model's own space, in uid order)
            let model = lunaris_render::math::M4::from_trs(pt.pos, pt.rot, pt.scale);
            let mut goals: Vec<IkGoal> = Vec::new();
            if with_ik {
                let inv = model.inverse_affine();
                let mut found: Vec<(u64, Ik, [f32; 3])> = Vec::new();
                let mut raw = Vec::new();
                let _ = w.for_each_uid::<(&Ik, &Transform), _>(|e, gid, (g, t)| {
                    if g.active && g.weight > 0.0 {
                        raw.push((e, gid, *g, t.pos));
                    }
                });
                for (e, gid, g, pos) in raw {
                    // (as the renderer does: a goal with no `skin` bends the model it is a child of)
                    let skin = if g.skin.0 != 0 { g.skin.0 } else { w.get::<lunaris_sim::components::Parent>(e).map_or(0, |p| p.target.0) };
                    if skin == uid {
                        found.push((gid, g, pos));
                    }
                }
                found.sort_by_key(|g| (match g.1.kind { 3 => 0u8, 1 => 1, _ => 2 }, g.0));
                for (_, ik, gpos) in found {
                    let joint_name = w.strings().get(ik.joint.0).to_string();
                    let Some(joint) = rig.joint_index(&joint_name) else { continue };
                    let kind = match ik.kind {
                        1 => IkKind::Look { axis: ik.axis, max_angle: if ik.max_angle.is_finite() { ik.max_angle } else { 80.0 } },
                        3 => IkKind::Turn { axis: ik.axis, degrees: ik.angle },
                        _ => IkKind::Reach,
                    };
                    let pin = (ik.kind == 2).then(|| {
                        let dir = |v: [f32; 3]| inv.transform_dir(v);
                        let n2 = (ik.axis2.iter().any(|x| *x != 0.0)).then(|| dir(ik.axis2));
                        IkPin { n1: dir(ik.axis), n2, one_sided: ik.one_sided }
                    });
                    let pole = (ik.pole.0 != 0).then(|| w.entity(ik.pole.0).and_then(|e| w.get::<Transform>(e)).map(|t| inv.transform_point(t.pos))).flatten();
                    goals.push(IkGoal { joint, kind, target: inv.transform_point(gpos), pin, pole, weight: ik.weight.min(1.0) });
                }
            }
            let worlds = rig.joint_worlds(&[layer], &goals);
            let yaw = 2.0 * pt.rot[1].atan2(pt.rot[3]);
            let fk = w.get::<lunaris_sim::footik::FootIk>(pe).map(|f| *f).unwrap_or_default();
            let goal_of = |uid: u64| w.entity(uid).and_then(|e| Some((w.get::<Ik>(e)?.weight, w.get::<Transform>(e)?.pos))).unwrap_or((0.0, [0.0; 3]));
            let (gl, gr, fl, fr) = (goal_of(fk.goal_l.0), goal_of(fk.goal_r.0), goal_of(fk.floor_l.0), goal_of(fk.floor_r.0));
            let mut line = format!("{tick},{:.4},{:.4},{:.4},{:.4},{clip_name},{:.4},{},{:.4},{:.4},{:.4},{:.3},{:.3},{:.4},{:.4},{:.4},{:.4},{},{:.3},{:.4},{},{:.3},{:.3},{:.3},{:.3},{:.4},{:.4},{:.4},{:.4},{:.3},{:.3},{:.3},{:.4},{:.3},{:.4},{:.3},{:.3}", ht.pos[0], ht.pos[1], ht.pos[2], yaw, a.time, chain.map_or(0, |c| c.n), fk.gs, fk.stance_y, fk.line_y, fk.gw_l, fk.gw_r, fk.lift_l, fk.lift_r, fk.lk_l[1], fk.lk_r[1], (fk.locked_l as u8) * 2 + fk.locked_r as u8, fk.stride, fk.bias, fk.gait as u8, fk.theta, fk.sw_l, fk.sw_r, fk.pitch, fk.tg_l[1], fk.tg_l[2], fk.tg_r[1], fk.tg_r[2], fk.wk, fk.smx, gl.0, gl.1[1], gr.0, gr.1[1], fl.0, fr.0);
            for &j in &joints {
                let p = [worlds[j].0[12], worlds[j].0[13], worlds[j].0[14]];
                let p = if in_world { model.transform_point(p) } else { p };
                line.push_str(&format!(",{:.4},{:.4},{:.4}", p[0], p[1], p[2]));
            }
            println!("{line}");
        }
    }
    ExitCode::SUCCESS
}
