//! Spatial queries against what the renderer would draw: ray casts. They use the exact shapes of the built-in meshes and sprites,
//! in each entity's local space, so a ray agrees with what `render` shows.
use crate::math::*;
use lunaris_ecs::{Result, World};
use lunaris_sim::components::{Collider, ConvexCollider, MeshCollider, MeshRenderer, Sprite, Terrain, Transform};
use lunaris_sim::meshcol::CollisionMesh;

#[derive(Clone, Debug, PartialEq)]
pub struct RayHit {
    pub uid: u64,
    /// Distance along the (normalised) ray.
    pub distance: f32,
    pub point: V3,
    pub normal: V3,
}

/// Intersect a local-space ray with the unit shape `kind`; returns (t, local normal) for the nearest hit with t >= 0.
fn hit_shape(kind: &str, o: V3, d: V3) -> Option<(f32, V3)> {
    match kind {
        "cube" => {
            let (mut t0, mut t1) = (f32::NEG_INFINITY, f32::INFINITY);
            let mut n0 = [0.0; 3];
            let mut n1 = [0.0; 3];
            for a in 0..3 {
                if d[a].abs() < 1e-12 {
                    if o[a].abs() > 0.5 {
                        return None;
                    }
                    continue;
                }
                let (mut ta, mut tb) = ((-0.5 - o[a]) / d[a], (0.5 - o[a]) / d[a]);
                let mut na = [0.0; 3];
                let mut nb = [0.0; 3];
                na[a] = -1.0;
                nb[a] = 1.0;
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                    std::mem::swap(&mut na, &mut nb);
                }
                if ta > t0 {
                    t0 = ta;
                    n0 = na;
                }
                if tb < t1 {
                    t1 = tb;
                    n1 = nb;
                }
                if t0 > t1 {
                    return None;
                }
            }
            if t1 < 0.0 {
                None
            } else if t0 >= 0.0 {
                Some((t0, n0))
            } else {
                Some((t1, n1)) // the ray starts inside: report where it leaves
            }
        }
        "sphere" => {
            let a = dot(d, d);
            let b = 2.0 * dot(o, d);
            let c = dot(o, o) - 0.25;
            let disc = b * b - 4.0 * a * c;
            if disc < 0.0 || a == 0.0 {
                return None;
            }
            let s = disc.sqrt();
            let (t0, t1) = ((-b - s) / (2.0 * a), (-b + s) / (2.0 * a));
            let t = if t0 >= 0.0 { t0 } else if t1 >= 0.0 { t1 } else { return None };
            let p = add(o, scale(d, t));
            Some((t, normalize(p)))
        }
        "plane" | "quad" => {
            let axis = if kind == "plane" { 1 } else { 2 };
            if d[axis].abs() < 1e-12 {
                return None;
            }
            let t = -o[axis] / d[axis];
            if t < 0.0 {
                return None;
            }
            let p = add(o, scale(d, t));
            let (u, v) = if axis == 1 { (0, 2) } else { (0, 1) };
            if p[u].abs() > 0.5 || p[v].abs() > 0.5 {
                return None;
            }
            let mut n = [0.0; 3];
            n[axis] = if d[axis] < 0.0 { 1.0 } else { -1.0 };
            Some((t, n))
        }
        "cylinder" => {
            let mut best: Option<(f32, V3)> = None;
            let mut consider = |t: f32, n: V3| {
                if t >= 0.0 && best.is_none_or(|(bt, _)| t < bt) {
                    best = Some((t, n));
                }
            };
            // side: x^2 + z^2 = 0.25 for |y| <= 0.5
            let a = d[0] * d[0] + d[2] * d[2];
            if a > 1e-12 {
                let b = 2.0 * (o[0] * d[0] + o[2] * d[2]);
                let c = o[0] * o[0] + o[2] * o[2] - 0.25;
                let disc = b * b - 4.0 * a * c;
                if disc >= 0.0 {
                    let s = disc.sqrt();
                    for t in [(-b - s) / (2.0 * a), (-b + s) / (2.0 * a)] {
                        let y = o[1] + t * d[1];
                        if y.abs() <= 0.5 {
                            let p = add(o, scale(d, t));
                            consider(t, normalize([p[0], 0.0, p[2]]));
                        }
                    }
                }
            }
            // caps
            if d[1].abs() > 1e-12 {
                for (y, ny) in [(0.5f32, 1.0f32), (-0.5, -1.0)] {
                    let t = (y - o[1]) / d[1];
                    let p = add(o, scale(d, t));
                    if p[0] * p[0] + p[2] * p[2] <= 0.25 {
                        consider(t, [0.0, ny, 0.0]);
                    }
                }
            }
            best
        }
        _ => None,
    }
}

/// What a ray can hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RayTargets {
    /// Meshes and sprites, exactly as `render` draws them.
    Visuals,
    /// Physics colliders (spheres, and boxes that ignore rotation).
    Colliders,
    Both,
}

/// The nearest entity along a ray (meshes, sprites and colliders), up to `max_distance`. `direction` need not be normalised.
pub fn raycast(world: &mut World, origin: V3, direction: V3, max_distance: f32) -> Result<Option<RayHit>> {
    raycast_targets(world, origin, direction, max_distance, RayTargets::Both)
}

/// Möller-Trumbore against every triangle of an imported model, in its local space; both faces count.
fn hit_mesh(mesh: &crate::mesh::Mesh, o: V3, d: V3) -> Option<(f32, V3)> {
    let mut best: Option<(f32, V3)> = None;
    for tri in mesh.idx.chunks_exact(3) {
        let (a, b, c) = (mesh.pos[tri[0] as usize], mesh.pos[tri[1] as usize], mesh.pos[tri[2] as usize]);
        let (e1, e2) = (sub(b, a), sub(c, a));
        let p = cross(d, e2);
        let det = dot(e1, p);
        if det.abs() < 1e-12 {
            continue;
        }
        let inv = 1.0 / det;
        let tv = sub(o, a);
        let u = dot(tv, p) * inv;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = cross(tv, e1);
        let v = dot(d, q) * inv;
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let t = dot(e2, q) * inv;
        if t >= 0.0 && best.is_none_or(|(bt, _)| t < bt) {
            let mut n = normalize(cross(e1, e2));
            if dot(n, d) > 0.0 {
                n = scale(n, -1.0);
            }
            best = Some((t, n));
        }
    }
    best
}

pub fn raycast_targets(world: &mut World, origin: V3, direction: V3, max_distance: f32, targets: RayTargets) -> Result<Option<RayHit>> {
    raycast_with(world, &mut crate::TextureCache::new(None), origin, direction, max_distance, targets)
}

/// As `raycast_targets`, but imported models (`.glb`) are found through `assets`, so a ray agrees with what `render` draws.
pub fn raycast_with(world: &mut World, assets: &mut crate::TextureCache, origin: V3, direction: V3, max_distance: f32, targets: RayTargets) -> Result<Option<RayHit>> {
    raycast_masked(world, assets, origin, direction, max_distance, targets, u32::MAX)
}

/// As `raycast_with`, but the ray only hits entities whose collision layer (`CollisionLayer.layer`; 0 for an entity without one) is in `mask`
/// (bit `n` = layer `n`; `u32::MAX` is every layer, which is what `raycast_with` asks).
pub fn raycast_masked(world: &mut World, assets: &mut crate::TextureCache, origin: V3, direction: V3, max_distance: f32, targets: RayTargets, mask: u32) -> Result<Option<RayHit>> {
    let dir = normalize(direction);
    if length(dir) == 0.0 {
        return Err(lunaris_ecs::EcsError::new("bad_arguments", "the ray direction must not be zero").at("direction"));
    }
    let want_visuals = targets != RayTargets::Colliders;
    let want_colliders = targets != RayTargets::Visuals;
    let mut meshes: Vec<(u64, MeshRenderer, Transform)> = Vec::new();
    let mut sprites: Vec<(u64, Sprite, Transform)> = Vec::new();
    let mut colliders: Vec<(u64, Collider, Transform)> = Vec::new();
    if want_visuals {
        world.for_each_uid::<(&MeshRenderer, &Transform), _>(|_, uid, (m, t)| {
            if m.visible {
                meshes.push((uid, *m, *t));
            }
        })?;
        world.for_each_uid::<(&Sprite, &Transform), _>(|_, uid, (s, t)| {
            if s.visible {
                sprites.push((uid, *s, *t));
            }
        })?;
    }
    if want_colliders {
        world.for_each_uid::<(&Collider, &Transform), _>(|_, uid, (c, t)| colliders.push((uid, *c, *t)))?;
    }
    let mut grounds: Vec<(u64, Terrain, Transform)> = Vec::new();
    world.for_each_uid::<(&Terrain, &Transform), _>(|_, uid, (g, t)| {
        if (want_visuals && g.visible) || (want_colliders && g.solid) {
            grounds.push((uid, *g, *t));
        }
    })?;
    // level geometry that physics collides with (a `MeshCollider`): the triangles of the model, found with the tree
    let mut level: Vec<(u64, MeshCollider, Transform)> = Vec::new();
    if want_colliders {
        world.for_each_uid::<(&MeshCollider, &Transform), _>(|_, uid, (m, t)| {
            if m.solid {
                level.push((uid, *m, *t));
            }
        })?;
    }
    let mut convex: Vec<(u64, ConvexCollider, Transform)> = Vec::new();
    if want_colliders {
        world.for_each_uid::<(&ConvexCollider, &Transform), _>(|_, uid, (c, t)| {
            if !c.trigger {
                convex.push((uid, *c, *t));
            }
        })?;
    }
    // collision layers: what the ray is not asked to hit is not there for it (a world without a `CollisionLayer` is never filtered)
    if mask != u32::MAX {
        let layers = lunaris_sim::layers::Layers::gather(world);
        let wanted = |uid: u64| (mask >> layers.of(uid).layer) & 1 != 0;
        meshes.retain(|m| wanted(m.0));
        sprites.retain(|m| wanted(m.0));
        colliders.retain(|m| wanted(m.0));
        grounds.retain(|m| wanted(m.0));
        level.retain(|m| wanted(m.0));
        convex.retain(|m| wanted(m.0));
    }
    let mut best: Option<RayHit> = None;
    let mut consider = |uid: u64, kind: &str, model: M4, custom: Option<&crate::mesh::Mesh>, collision: Option<&CollisionMesh>| {
        let inv = model.inverse_affine();
        let o = inv.transform_point(origin);
        let d = inv.transform_dir(dir);
        let found = match (custom, collision) {
            (Some(mesh), _) => hit_mesh(mesh, o, d),
            (None, Some(c)) => c.ray_local(o, d, f32::INFINITY).map(|h| (h.t, h.normal)),
            (None, None) => hit_shape(kind, o, d),
        };
        if let Some((t, n_local)) = found {
            if t <= max_distance && best.as_ref().is_none_or(|b| t < b.distance || (t == b.distance && uid < b.uid)) {
                let nm = model.normal_matrix();
                let n = normalize([
                    nm[0][0] * n_local[0] + nm[0][1] * n_local[1] + nm[0][2] * n_local[2],
                    nm[1][0] * n_local[0] + nm[1][1] * n_local[1] + nm[1][2] * n_local[2],
                    nm[2][0] * n_local[0] + nm[2][1] * n_local[1] + nm[2][2] * n_local[2],
                ]);
                best = Some(RayHit { uid, distance: t, point: add(origin, scale(dir, t)), normal: n });
            }
        }
    };
    for (uid, m, t) in &meshes {
        let name = world.strings().get(m.mesh.0).to_string();
        let model = if crate::mesh::builtin(&name).is_none() && crate::render::is_model_name(&name) { assets.model(&name) } else { None };
        consider(*uid, &name, M4::from_trs(t.pos, t.rot, t.scale), model.as_deref(), None);
    }
    for (uid, s, t) in &sprites {
        consider(*uid, "quad", M4::from_trs(t.pos, t.rot, [t.scale[0] * s.size[0], t.scale[1] * s.size[1], t.scale[2]]), None, None);
    }
    for (uid, m, t) in &level {
        let name = world.strings().get(m.mesh.0).to_string();
        if let Some(c) = assets.collision_mesh(&name) {
            consider(*uid, "", M4::from_trs(t.pos, t.rot, t.scale), None, Some(&c));
        }
    }
    let identity = [0.0, 0.0, 0.0, 1.0];
    for (uid, c, t) in &colliders {
        let sc = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
        if c.shape == 0 {
            let d = c.size[0].abs() * sc[0].max(sc[1]).max(sc[2]);
            consider(*uid, "sphere", M4::from_trs(t.pos, identity, [d, d, d]), None, None);
        } else if c.shape == 2 {
            // a capsule (as physics has it): a cylinder along the entity's y axis with a ball on each end
            let r = 0.5 * c.size[0].abs() * sc[0].max(sc[2]);
            let half = (0.5 * c.size[1].abs() * sc[1] - r).max(0.0);
            let axis = M4::from_trs([0.0; 3], t.rot, [1.0; 3]).transform_dir([0.0, 1.0, 0.0]);
            if half > 1e-6 {
                consider(*uid, "cylinder", M4::from_trs(t.pos, t.rot, [2.0 * r, 2.0 * half, 2.0 * r]), None, None);
            }
            for end in [-half, half] {
                consider(*uid, "sphere", M4::from_trs(add(t.pos, scale(axis, end)), identity, [2.0 * r; 3]), None, None);
            }
        } else {
            // (a box that is turned, or that turns, is an oriented box for physics)
            consider(*uid, "cube", M4::from_trs(t.pos, t.rot, [c.size[0].abs() * sc[0], c.size[1].abs() * sc[1], c.size[2].abs() * sc[2]]), None, None);
        }
    }
    // convex hulls (a `ConvexCollider`): the solid whose corners are the model's, in the world (`consider` is done with `best`)
    for (uid, c, t) in &convex {
        let name = world.strings().get(c.mesh.0).to_string();
        let Some(hull) = assets.hull(&name, [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()]) else { continue };
        let (pos, q) = hull.pose(t);
        if let Some((dist, n)) = hull.ray_world(pos, q, origin, dir, max_distance) {
            if best.as_ref().is_none_or(|b| dist < b.distance || (dist == b.distance && *uid < b.uid)) {
                best = Some(RayHit { uid: *uid, distance: dist, point: add(origin, scale(dir, dist)), normal: n });
            }
        }
    }
    // the ground made from heightmaps (`consider` is done with `best`)
    grounds.sort_by_key(|g| g.0);
    for (uid, g, t) in &grounds {
        let name = world.strings().get(g.heightmap.0).to_string();
        let Some(field) = assets.heightfield(&name) else { continue };
        let placed = lunaris_sim::terrain::Placed::new(&field, t.pos, g);
        if let Some((dist, n)) = placed.ray(origin, dir, max_distance) {
            if best.as_ref().is_none_or(|b| dist < b.distance || (dist == b.distance && *uid < b.uid)) {
                best = Some(RayHit { uid: *uid, distance: dist, point: add(origin, scale(dir, dist)), normal: n });
            }
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_json::parse;
    use lunaris_sim::{Command, Sim};

    fn spawn(s: &mut Sim, uid: u64, comps: &str) {
        let lunaris_json::Json::Obj(components) = parse(comps).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn rays_hit_each_builtin_shape_at_the_right_distance() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"cube"}}"#);
        let h = raycast(&mut s.world, [0.0, 0.0, 5.0], [0.0, 0.0, -1.0], 100.0).unwrap().unwrap();
        assert_eq!(h.uid, 1);
        assert!(near(h.distance, 4.5) && near(h.point[2], 0.5) && near(h.normal[2], 1.0), "{h:?}");
        // a sphere of radius 0.5 scaled by 2 (radius 1)
        let mut s = Sim::new(1);
        spawn(&mut s, 2, r#"{"Transform":{"scale":[2,2,2]},"MeshRenderer":{"mesh":"sphere"}}"#);
        let h = raycast(&mut s.world, [5.0, 0.0, 0.0], [-1.0, 0.0, 0.0], 100.0).unwrap().unwrap();
        assert!(near(h.distance, 4.0) && near(h.normal[0], 1.0), "{h:?}");
        // a floor (plane) seen from above and a cylinder from the side and from the top
        let mut s = Sim::new(1);
        spawn(&mut s, 3, r#"{"Transform":{"pos":[0,-1,0],"scale":[10,1,10]},"MeshRenderer":{"mesh":"plane"}}"#);
        spawn(&mut s, 4, r#"{"Transform":{"pos":[0,2,0]},"MeshRenderer":{"mesh":"cylinder"}}"#);
        let h = raycast(&mut s.world, [1.0, 10.0, 1.0], [0.0, -1.0, 0.0], 100.0).unwrap().unwrap();
        assert!(h.uid == 3 && near(h.distance, 11.0) && near(h.normal[1], 1.0), "{h:?}");
        let h = raycast(&mut s.world, [0.0, 10.0, 0.0], [0.0, -1.0, 0.0], 100.0).unwrap().unwrap();
        assert!(h.uid == 4 && near(h.distance, 7.5), "cylinder top cap: {h:?}");
        let h = raycast(&mut s.world, [5.0, 2.0, 0.0], [-1.0, 0.0, 0.0], 100.0).unwrap().unwrap();
        assert!(h.uid == 4 && near(h.distance, 4.5), "cylinder side: {h:?}");
    }

    #[test]
    fn the_nearest_entity_wins_rotations_apply_and_limits_and_misses_work() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,-3]},"MeshRenderer":{"mesh":"cube"}}"#);
        spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"cube"}}"#);
        let h = raycast(&mut s.world, [0.0, 0.0, 5.0], [0.0, 0.0, -2.0], 100.0).unwrap().unwrap();
        assert_eq!(h.uid, 2, "the nearer cube, and the direction need not be normalised");
        assert!(raycast(&mut s.world, [0.0, 0.0, 5.0], [0.0, 0.0, -1.0], 2.0).unwrap().is_none(), "max distance");
        assert!(raycast(&mut s.world, [3.0, 0.0, 5.0], [0.0, 0.0, -1.0], 100.0).unwrap().is_none(), "a miss");
        assert!(raycast(&mut s.world, [0.0, 0.0, 5.0], [0.0, 0.0, 1.0], 100.0).unwrap().is_none(), "pointing away");
        assert!(raycast(&mut s.world, [0.0; 3], [0.0; 3], 1.0).is_err());
        // a cube rotated 45 degrees about Y has its corner towards +X: the ray from +X hits it at 0.5*sqrt(2)
        let mut s = Sim::new(1);
        spawn(&mut s, 5, r#"{"Transform":{"rot":[0,0.3826834,0,0.9238795]},"MeshRenderer":{"mesh":"cube"}}"#);
        let h = raycast(&mut s.world, [5.0, 0.0, 0.0], [-1.0, 0.0, 0.0], 100.0).unwrap().unwrap();
        assert!(near(h.distance, 5.0 - std::f32::consts::FRAC_1_SQRT_2), "{h:?}");
    }

    #[test]
    fn rays_also_hit_physics_colliders_without_visuals() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1,0]},"Collider":{"shape":0,"size":[2,2,2]}}"#);
        spawn(&mut s, 2, r#"{"Transform":{"pos":[5,0,0],"scale":[2,2,2]},"Collider":{"shape":1,"size":[1,1,1]}}"#);
        let h = raycast(&mut s.world, [0.0, 10.0, 0.0], [0.0, -1.0, 0.0], 100.0).unwrap().unwrap();
        assert!(h.uid == 1 && near(h.distance, 8.0), "{h:?}");
        let h = raycast(&mut s.world, [10.0, 0.0, 0.0], [-1.0, 0.0, 0.0], 100.0).unwrap().unwrap();
        assert!(h.uid == 2 && near(h.distance, 4.0), "{h:?}");
        assert!(raycast_targets(&mut s.world, [0.0, 10.0, 0.0], [0.0, -1.0, 0.0], 100.0, RayTargets::Visuals).unwrap().is_none(), "no visuals to hit");
    }

    #[test]
    fn rays_agree_with_the_image() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,5]},"Camera":{}}"#);
        spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"sphere"}}"#);
        spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0,-4]},"Sprite":{"size":[8,8]}}"#);
        let mut tex = crate::TextureCache::new(None);
        let f = crate::render(&mut s.world, &mut tex, &crate::RenderOptions { width: 101, height: 101, ..Default::default() }).unwrap();
        let centre = f.pick(50, 50);
        let hit = raycast(&mut s.world, [0.0, 0.0, 5.0], [0.0, 0.0, -1.0], 100.0).unwrap().map(|h| h.uid);
        assert_eq!(centre, hit);
        assert_eq!(centre, Some(2));
        // a ray beside the sphere hits the sprite behind it, as the image does
        let corner = f.pick(80, 50);
        assert_eq!(corner, Some(3));
        let p = f.world_to_pixel([1.5, 0.0, -4.0]).unwrap();
        assert!(p[0] > 60.0);
        let dir = [1.5, 0.0, -9.0];
        assert_eq!(raycast(&mut s.world, [0.0, 0.0, 5.0], dir, 100.0).unwrap().map(|h| h.uid), Some(3));
    }
}
