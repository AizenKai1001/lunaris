//! `Probe`: the ground under an entity, the water over it and one ray, written where graph systems can read them (docs/GAMEPLAY.md, "Probe"). It runs after the physics step and before the graph
//! systems, with the same rays the wheels of a `Vehicle` use (`vehicle_ray`): static colliders, mesh levels, convex hulls and terrain, filtered by `CollisionLayer`. Everything is f32 with only `+ - * / sqrt`.
use crate::components::{Probe, Transform};
use crate::layers::Layers;
use crate::meshcol::MeshStore;
use crate::motion::turn;
use crate::terrain::Terrains;
use crate::vehicle_ray::RayWorld;
use lunaris_ecs::{Entity, Uid, World};

type V3 = [f32; 3];

fn finite3(v: V3) -> bool {
    v.iter().all(|c| c.is_finite())
}

#[cfg(test)]
thread_local! {
    /// How many times a step gathered the static world (tests check that a step in which nothing casts does not).
    pub(crate) static GATHERS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

pub fn step(world: &mut World, fields: &Terrains, meshes: &MeshStore) {
    let mut probes: Vec<(u64, Entity, Transform, Probe)> = Vec::new();
    let _ = world.for_each_uid::<(&Transform, &Probe), _>(|e, uid, (t, p)| probes.push((uid, e, *t, *p)));
    if probes.is_empty() {
        return;
    }
    probes.sort_by_key(|p| p.0);
    // the static world and the layers are gathered when the first probe casts and not before: a step in which no probe casts (a game that lets its probes
    // cast every few steps sets `ray_max` to 0 and `up` and `depth` to 0 in between) costs nothing of it
    let mut gathered: Option<(RayWorld, Layers)> = None;
    let waters = crate::water::bodies(world, fields);
    for (uid, entity, t, p) in probes {
        let mut out = p;
        let pos = if finite3(t.pos) { t.pos } else { [0.0; 3] };
        // ---- the ground: down from `up` metres above the feet (no cast when `up` + `depth` is 0)
        let up = if p.up.is_finite() { p.up.clamp(0.0, 1000.0) } else { 2.0 };
        let depth = if p.depth.is_finite() { p.depth.clamp(0.0, 10000.0) } else { 100.0 };
        let origin = [pos[0], pos[1] + up, pos[2]];
        let ray_dir_ok = p.ray_max.is_finite() && p.ray_max > 0.0 && finite3(p.ray_dir) && finite3(p.ray_from);
        if up + depth > 0.0 || ray_dir_ok {
            gathered.get_or_insert_with(|| {
                #[cfg(test)]
                GATHERS.with(|g| g.set(g.get() + 1));
                (RayWorld::gather(world, fields, meshes, None), Layers::gather(world))
            });
        }
        let ground = if up + depth > 0.0 {
            let (ray, layers) = gathered.as_ref().expect("gathered above");
            ray.cast(origin, [0.0, -1.0, 0.0], up + depth, layers.of(uid), uid)
        } else {
            None
        };
        match ground {
            Some(h) => {
                out.over_ground = true;
                out.ground = origin[1] - h.t;
                out.ground_normal = h.normal;
                out.ground_uid = Uid(h.uid);
            }
            None => {
                out.over_ground = false;
                out.ground = pos[1];
                out.ground_normal = [0.0, 1.0, 0.0];
                out.ground_uid = Uid(0);
            }
        }
        // ---- the water over this place
        match crate::water::surface_at(&waters, fields, world.strings(), pos[0], pos[2]) {
            Some((y, _)) => {
                out.wet = true;
                out.surface = y;
            }
            None => {
                out.wet = false;
                out.surface = 0.0;
            }
        }
        // ---- the ray
        out.hit = false;
        out.hit_dist = 0.0;
        out.hit_uid = Uid(0);
        if ray_dir_ok {
            let (ray, layers) = gathered.as_ref().expect("gathered above");
            let who = layers.of(uid);
            let (from, dir) = if p.ray_local { (turn(t.rot, p.ray_from), turn(t.rot, p.ray_dir)) } else { (p.ray_from, p.ray_dir) };
            let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
            if len > 1.0e-9 {
                let d = [dir[0] / len, dir[1] / len, dir[2] / len];
                let o = [pos[0] + from[0], pos[1] + from[1], pos[2] + from[2]];
                if let Some(h) = ray.cast(o, d, p.ray_max, who, uid) {
                    out.hit = true;
                    out.hit_dist = h.t;
                    out.hit_point = [o[0] + d[0] * h.t, o[1] + d[1] * h.t, o[2] + d[2] * h.t];
                    out.hit_normal = h.normal;
                    out.hit_uid = Uid(h.uid);
                }
            }
        }
        if let Some(pr) = world.get_mut::<Probe>(entity) {
            *pr = out;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use components::Probe;
    use lunaris_json::{parse, Json};

    fn spawn(s: &mut Sim, uid: u64, comps: &str) {
        let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    }

    fn probe(s: &Sim, uid: u64) -> Probe {
        *s.world.get::<Probe>(s.world.entity(uid).unwrap()).unwrap()
    }

    #[test]
    fn a_probe_reads_the_ground_under_it_and_a_ray_ahead_of_it() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
        // a step 1 m high at x = 3 .. 4 and a wall at z = -5
        spawn(&mut s, 2, r#"{"Transform":{"pos":[3.5,0.5,0],"scale":[1,1,4]},"Collider":{"shape":1}}"#);
        spawn(&mut s, 3, r#"{"Transform":{"pos":[0,2,-5.5],"scale":[10,4,1]},"Collider":{"shape":1}}"#);
        spawn(&mut s, 10, r#"{"Transform":{"pos":[0,3,0]},"Probe":{"ray_max":20,"ray_dir":[0,0,-1],"ray_from":[0,0.5,0]}}"#);
        spawn(&mut s, 11, r#"{"Transform":{"pos":[3.5,3,0]},"Probe":{}}"#);
        s.step_n(2);
        let a = probe(&s, 10);
        assert!(a.over_ground && a.ground.abs() < 1e-4 && a.ground_normal[1] > 0.99 && a.ground_uid.0 == 1, "{a:?}");
        assert!(a.hit && (a.hit_dist - 5.0).abs() < 1e-3 && a.hit_uid.0 == 3 && a.hit_normal[2] > 0.99, "{a:?}");
        assert!((a.hit_point[2] + 5.0).abs() < 1e-3, "{a:?}");
        // over the step the ground is its top
        let b = probe(&s, 11);
        assert!(b.over_ground && (b.ground - 1.0).abs() < 1e-4 && b.ground_uid.0 == 2, "{b:?}");
        // it follows the entity
        let e = s.world.entity(10).unwrap();
        s.world.get_mut::<components::Transform>(e).unwrap().pos = [3.5, 5.0, 0.0];
        s.step_n(1);
        assert!((probe(&s, 10).ground - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_step_in_which_no_probe_casts_does_not_gather_the_world_and_one_that_does_gathers_it_once() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
        // three probes that do not cast (no ground cast: up and depth 0; no ray: ray_max 0)
        for uid in 10..13 {
            spawn(&mut s, uid, r#"{"Transform":{"pos":[1,3,1]},"Probe":{"up":0,"depth":0}}"#);
        }
        super::GATHERS.with(|g| g.set(0));
        s.step_n(3);
        assert_eq!(super::GATHERS.with(|g| g.get()), 0, "nothing cast, nothing gathered");
        let a = probe(&s, 10);
        assert!(!a.over_ground && a.ground == 3.0 && a.ground_normal == [0.0, 1.0, 0.0] && a.ground_uid.0 == 0 && !a.hit && !a.wet, "{a:?}");
        // one of them casts the ground: one gather a step, however many probes cast
        let e = s.world.entity(11).unwrap();
        s.world.get_mut::<components::Probe>(e).unwrap().depth = 10.0;
        let e = s.world.entity(12).unwrap();
        s.world.get_mut::<components::Probe>(e).unwrap().depth = 10.0;
        s.step_n(2);
        assert_eq!(super::GATHERS.with(|g| g.get()), 2, "one gather a step");
        assert!(probe(&s, 11).over_ground && (probe(&s, 11).ground - 0.0).abs() < 1e-4 && probe(&s, 12).over_ground);
        assert!(!probe(&s, 10).over_ground, "the probe that does not cast still does not");
        // a probe that only casts its ray gathers too
        let e = s.world.entity(10).unwrap();
        {
            let p = s.world.get_mut::<components::Probe>(e).unwrap();
            p.ray_max = 10.0;
            p.ray_dir = [0.0, -1.0, 0.0];
            p.ray_from = [0.0, 0.0, 0.0];
        }
        let e11 = s.world.entity(11).unwrap();
        s.world.get_mut::<components::Probe>(e11).unwrap().depth = 0.0;
        let e12 = s.world.entity(12).unwrap();
        s.world.get_mut::<components::Probe>(e12).unwrap().depth = 0.0;
        super::GATHERS.with(|g| g.set(0));
        s.step_n(1);
        assert_eq!(super::GATHERS.with(|g| g.get()), 1);
        let a = probe(&s, 10);
        assert!(a.hit && (a.hit_dist - 3.0).abs() < 1e-3, "{a:?}");
        assert!(!a.over_ground, "its ground is not cast");
    }

    #[test]
    fn a_probe_over_nothing_says_so_and_never_sees_itself() {
        let mut s = Sim::new(1);
        spawn(&mut s, 10, r#"{"Transform":{"pos":[0,3,0]},"Collider":{"shape":1},"Probe":{"ray_max":5,"ray_dir":[0,-1,0],"ray_from":[0,0,0]}}"#);
        s.step_n(2);
        let a = probe(&s, 10);
        assert!(!a.over_ground && !a.hit && !a.wet, "{a:?}");
        assert_eq!(a.ground, 3.0);
    }
}
