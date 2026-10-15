//! `check` rules for the lighting effects of the GPU renderer (`AutoExposure`, `Ssr`, `LensFlare`, `ReflectionProbe`; docs/GPU.md): only the first of each counts, the numbers must mean something, and an effect that
//! has nothing to work on says so, because the picture would look as if it were not there.
use super::Problems;
use lunaris_ecs::{Result, World};
use lunaris_sim::components::{AutoExposure, LensFlare, Light, MeshRenderer, ReflectionProbe, Ssr, Transform, Water, WaterBody};

pub(super) fn run(world: &mut World, problems: &mut Problems) -> Result<()> {
    let mut meshes = 0usize;
    let mut glossy = 0usize;
    world.for_each_uid::<(&MeshRenderer,), _>(|_, _, (m,)| {
        if m.visible {
            meshes += 1;
            // the roughness the GPU renderer works with: a negative one is worked out from the shininess
            let rough = if m.roughness >= 0.0 && m.roughness.is_finite() { m.roughness } else { (2.0 / (m.shininess.max(1.0) + 2.0)).sqrt().sqrt() };
            if rough < 0.6 {
                glossy += 1;
            }
        }
    })?;
    let mut suns = 0usize;
    world.for_each_uid::<(&Light,), _>(|_, _, (l,)| {
        if l.kind == 0 {
            suns += 1;
        }
    })?;
    let mut waters = 0usize;
    world.for_each_uid::<(&Water,), _>(|_, _, _| waters += 1)?;
    world.for_each_uid::<(&WaterBody,), _>(|_, _, _| waters += 1)?;
    let mut exposures: Vec<(u64, AutoExposure)> = Vec::new();
    world.for_each_uid::<(&AutoExposure,), _>(|_, uid, (a,)| exposures.push((uid, *a)))?;
    exposures.sort_by_key(|f| f.0);
    for (i, (uid, a)) in exposures.iter().enumerate() {
        if i > 0 {
            problems.add("warning", Some(*uid), format!("a second AutoExposure: only the one with the lowest uid ({}) counts", exposures[0].0), "delete this one, or put its numbers in the first");
        }
        if !(a.min.is_finite() && a.max.is_finite()) || a.min <= 0.0 || a.max <= 0.0 {
            problems.add("warning", Some(*uid), format!("AutoExposure min {} and max {} must be numbers above 0 (they are multipliers of the light)", a.min, a.max), "the defaults are min 0.05 and max 16");
        } else if a.min > a.max {
            problems.add("warning", Some(*uid), format!("AutoExposure.min {} is above max {}: the exposure never goes below min, so max counts as min", a.min, a.max), "swap them");
        }
        if !(a.speed.is_finite() && a.speed > 0.0) {
            problems.add("warning", Some(*uid), format!("AutoExposure.speed {} is not above 0, so the exposure never moves after the first picture", a.speed), "1.5 settles in about a second; 4 is quick, 0.5 is slow");
        }
        if !(a.target.is_finite() && a.target > 0.0 && a.target <= 4.0) {
            problems.add("warning", Some(*uid), format!("AutoExposure.target {} is not a brightness between 0 and 4 (the average luminance the picture is brought to)", a.target), "0.18 is middle grey; higher is a brighter picture");
        }
        if meshes == 0 {
            problems.add("warning", Some(*uid), "AutoExposure has no 3D meshes to measure: a flat 2D picture keeps its authored colours".into(), "it is for lit 3D scenes (MeshRenderer, an Environment, lights)");
        }
    }
    let mut reflections: Vec<(u64, Ssr)> = Vec::new();
    world.for_each_uid::<(&Ssr,), _>(|_, uid, (r,)| reflections.push((uid, *r)))?;
    reflections.sort_by_key(|f| f.0);
    for (i, (uid, r)) in reflections.iter().enumerate() {
        if i > 0 {
            problems.add("warning", Some(*uid), format!("a second Ssr: only the one with the lowest uid ({}) counts", reflections[0].0), "delete this one, or put its numbers in the first");
        }
        if !(r.strength.is_finite() && r.strength > 0.0) {
            problems.add("warning", Some(*uid), format!("Ssr.strength {} is not above 0, so there are no reflections", r.strength), "1 is a full mirror image where a surface reflects; 0.5 half of it");
        }
        if !(r.max_distance.is_finite() && r.max_distance > 0.0) || !(r.thickness.is_finite() && r.thickness > 0.0) {
            problems.add("warning", Some(*uid), format!("Ssr.max_distance {} and thickness {} must be numbers above 0 (metres)", r.max_distance, r.thickness), "the defaults are 40 and 0.5");
        }
        if meshes == 0 {
            problems.add("warning", Some(*uid), "Ssr has no 3D meshes to reflect: a flat 2D picture is drawn as authored".into(), "it is for lit 3D scenes");
        } else if glossy == 0 && waters == 0 {
            problems.add("warning", Some(*uid), "Ssr: no mesh is smooth enough to reflect (roughness below about 0.6) and there is no water, so the picture does not change".into(), "give a floor or a ball `roughness` 0.1 to 0.3 (and `metallic` for a mirror), or add a Water");
        }
    }
    let mut flares: Vec<(u64, LensFlare)> = Vec::new();
    world.for_each_uid::<(&LensFlare,), _>(|_, uid, (f,)| flares.push((uid, *f)))?;
    flares.sort_by_key(|f| f.0);
    for (i, (uid, f)) in flares.iter().enumerate() {
        if i > 0 {
            problems.add("warning", Some(*uid), format!("a second LensFlare: only the one with the lowest uid ({}) counts", flares[0].0), "delete this one, or put its numbers in the first");
        }
        if !(f.intensity.is_finite() && f.intensity > 0.0) {
            problems.add("warning", Some(*uid), format!("LensFlare.intensity {} is not above 0, so nothing is drawn", f.intensity), "1 is a sunny day; 0.3 a hint");
        }
        if f.ghosts > 8 {
            problems.add("warning", Some(*uid), format!("LensFlare.ghosts {} is more than there are: 8 are drawn", f.ghosts), "0 to 8");
        }
        if meshes == 0 {
            problems.add("warning", Some(*uid), "LensFlare has no 3D meshes: it is for lit 3D scenes".into(), "a flat 2D picture is drawn as authored");
        } else if suns == 0 {
            problems.add("warning", Some(*uid), "LensFlare needs the sun, the first directional light (Light kind 0), and there is none: no flare is drawn".into(), "add a Light with kind 0 (its direction is where the sun is)");
        }
    }
    let mut probes: Vec<(u64, ReflectionProbe)> = Vec::new();
    world.for_each_uid::<(&ReflectionProbe,), _>(|_, uid, (p,)| probes.push((uid, *p)))?;
    probes.sort_by_key(|f| f.0);
    for (i, (uid, p)) in probes.iter().enumerate() {
        if !world.entity(*uid).is_some_and(|e| world.get::<Transform>(e).is_some()) {
            problems.add("error", Some(*uid), "has a ReflectionProbe but no Transform, so it has no place to capture from and does nothing".into(), "add a Transform: its position is where the six pictures are taken from");
        }
        if i >= 4 {
            problems.add("warning", Some(*uid), format!("a fifth ReflectionProbe: only the four with the lowest uids ({:?}) work at once", probes.iter().take(4).map(|p| p.0).collect::<Vec<_>>()), "make the boxes of the others part of a bigger probe's, or delete this one");
        }
        if !p.size.iter().all(|v| v.is_finite() && *v > 0.0) {
            problems.add("warning", Some(*uid), format!("ReflectionProbe.size {:?} is not three numbers above 0, so its box of influence is a speck", p.size), "the width, height and depth in metres of the room or cave it stands in (the box is centred on the entity)");
        }
        if !(16..=256).contains(&p.resolution) {
            problems.add("warning", Some(*uid), format!("ReflectionProbe.resolution {} is outside 16 to 256: it counts as {}", p.resolution, p.resolution.clamp(16, 256)), "128 is a good size; every probe of the world is captured at the biggest one asked for");
        }
        if meshes == 0 {
            problems.add("warning", Some(*uid), "ReflectionProbe has no 3D meshes to capture and nothing to light: it is for lit 3D scenes".into(), "a flat 2D picture is drawn as authored");
        }
    }
    Ok(())
}
