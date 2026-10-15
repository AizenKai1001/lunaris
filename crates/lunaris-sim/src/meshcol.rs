//! Static triangle-mesh colliders (`MeshCollider`, docs/PHYSICS.md): a level made in a modelling program (a cave, a town, a racetrack)
//! that things can stand on and roll across. This file holds the data and the questions that do not depend on the shape of the thing
//! asking; `meshcol_shapes.rs` has the contact finding for balls, capsules and boxes, and `meshcol_gltf.rs` reads the model file.
//!
//! **Two levels.** A [`CollisionMesh`] is what a model file *is*: its triangles in the model's own coordinates and a bounding volume
//! hierarchy over them (a BVH: a tree of boxes, each holding a few triangles, so that a question about one spot only looks at the few
//! triangles near it, however many the level has). It is read once per file and shared. A [`PlacedMesh`] is that mesh *standing in the
//! world*: the vertices moved by the entity's `Transform` (position, rotation and scale, so even a stretched or mirrored level works)
//! and the boxes of the tree fitted around them again. It is rebuilt only when the entity moves or the file changes.
//!
//! **Determinism.** The tree is built by splitting the triangles at the median along the longest axis of their centres, ordered by
//! (coordinate, triangle number), a strict total order, so the same triangles give the same tree on every machine and every Rust
//! version. Every query then returns its answer in a canonical order (by triangle number): nothing depends on thread count, memory
//! addresses or the order a traversal happened to visit things. All arithmetic is f32 `+ - * /` and `sqrt`.
use crate::components::{ConvexCollider, MeshCollider, Transform};
use crate::hull::Hull;
use lunaris_ecs::{Entity, World};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub type V3 = [f32; 3];

/// The most triangles one model may have to be used as a collider. A bigger one is refused with a message that names this number.
pub const MAX_TRIANGLES: usize = 1_000_000;
/// A model with more triangles than this still works but takes a moment to read and a few megabytes to hold; `check` mentions it.
pub const BIG_TRIANGLES: usize = 200_000;
/// Triangles in a leaf of the tree.
const LEAF: usize = 4;

// ------------------------------------------------------------------------------------------------ small vector helpers
#[inline]
pub(crate) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub(crate) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub(crate) fn mul(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
pub(crate) fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
fn finite3(a: V3) -> bool {
    a[0].is_finite() && a[1].is_finite() && a[2].is_finite()
}

// ------------------------------------------------------------------------------------------------ boxes
/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub lo: V3,
    pub hi: V3,
}

impl Aabb {
    /// A box that contains nothing (growing it by a point gives that point).
    pub const EMPTY: Aabb = Aabb { lo: [f32::INFINITY; 3], hi: [f32::NEG_INFINITY; 3] };

    pub fn grow(&mut self, p: V3) {
        for k in 0..3 {
            self.lo[k] = self.lo[k].min(p[k]);
            self.hi[k] = self.hi[k].max(p[k]);
        }
    }

    pub fn union(&self, o: &Aabb) -> Aabb {
        Aabb { lo: [self.lo[0].min(o.lo[0]), self.lo[1].min(o.lo[1]), self.lo[2].min(o.lo[2])], hi: [self.hi[0].max(o.hi[0]), self.hi[1].max(o.hi[1]), self.hi[2].max(o.hi[2])] }
    }

    pub fn centre(&self) -> V3 {
        [(self.lo[0] + self.hi[0]) * 0.5, (self.lo[1] + self.hi[1]) * 0.5, (self.lo[2] + self.hi[2]) * 0.5]
    }

    pub fn half(&self) -> V3 {
        [(self.hi[0] - self.lo[0]) * 0.5, (self.hi[1] - self.lo[1]) * 0.5, (self.hi[2] - self.lo[2]) * 0.5]
    }

    /// Squared distance from `p` to the box (0 inside it).
    #[inline]
    pub fn dist2(&self, p: V3) -> f32 {
        let mut d2 = 0.0;
        for k in 0..3 {
            let d = (self.lo[k] - p[k]).max(p[k] - self.hi[k]).max(0.0);
            d2 += d * d;
        }
        d2
    }

    #[inline]
    pub fn overlaps(&self, o: &Aabb) -> bool {
        (0..3).all(|k| self.lo[k] <= o.hi[k] && self.hi[k] >= o.lo[k])
    }

    pub fn grown(&self, by: f32) -> Aabb {
        Aabb { lo: [self.lo[0] - by, self.lo[1] - by, self.lo[2] - by], hi: [self.hi[0] + by, self.hi[1] + by, self.hi[2] + by] }
    }
}

/// One node of the tree. A leaf (`count > 0`) holds the triangles `order[link .. link + count]`; any other node has its left child right
/// after it in the list and its right child at index `link`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    pub b: Aabb,
    pub link: u32,
    pub count: u32,
}

/// Visit every triangle of the leaves whose boxes pass `test` (the order of visiting is not specified: callers sort what they collect).
pub(crate) fn walk(nodes: &[Node], order: &[u32], test: &mut impl FnMut(&Aabb) -> bool, visit: &mut impl FnMut(u32)) {
    if nodes.is_empty() {
        return;
    }
    // (a tree of a million triangles is about 18 levels deep: the stack never comes near this size)
    let mut stack = [0u32; 128];
    let mut sp = 1usize;
    while sp > 0 {
        sp -= 1;
        let i = stack[sp] as usize;
        let n = &nodes[i];
        if !test(&n.b) {
            continue;
        }
        if n.count > 0 {
            for k in 0..n.count {
                visit(order[(n.link + k) as usize]);
            }
        } else if sp + 2 <= stack.len() {
            stack[sp] = n.link;
            stack[sp + 1] = i as u32 + 1;
            sp += 2;
        }
    }
}

// ------------------------------------------------------------------------------------------------ the mesh of a file

/// The triangles of a model file in its own coordinates, with the tree over them. Immutable once built, shared by every entity that
/// uses the file.
#[derive(Clone, Debug)]
pub struct CollisionMesh {
    pub verts: Vec<V3>,
    /// The usable triangles (degenerate ones are not kept), as indices into `verts`, in the order the model lists them.
    pub tris: Vec<[u32; 3]>,
    pub(crate) order: Vec<u32>,
    pub(crate) nodes: Vec<Node>,
    pub bounds: Aabb,
    /// FNV-1a of the numbers: the simulation hashes it, so a replay with another model is noticed.
    pub hash: u64,
    /// How many triangles of the file had no area and were left out.
    pub degenerate: usize,
    /// Things the reader noticed (a skeleton, an unsupported primitive).
    pub warnings: Vec<String>,
}

/// One ray hit on a mesh: the distance along the ray, the triangle, and the unit normal on the side the ray came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshRay {
    pub t: f32,
    pub tri: u32,
    pub normal: V3,
}

fn fnv(h: &mut u64, x: u32) {
    for b in x.to_le_bytes() {
        *h = (*h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// Is the triangle too thin to have a direction? (Its area is zero within what f32 can say.)
pub(crate) fn is_degenerate(a: V3, b: V3, c: V3) -> bool {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let n = cross(e1, e2);
    let n2 = dot(n, n);
    let s = dot(e1, e1) + dot(e2, e2);
    !(n2.is_finite() && n2 > 1.0e-14 * s * s)
}

impl CollisionMesh {
    /// Build the tree over some triangles. Fails (with a reason) when there is nothing to collide with, when a number is not finite or an
    /// index points outside the vertices, or when there are more than [`MAX_TRIANGLES`].
    pub fn new(verts: Vec<V3>, tris: Vec<[u32; 3]>) -> Result<CollisionMesh, String> {
        if tris.is_empty() {
            return Err("the model has no triangles".to_string());
        }
        if tris.len() > MAX_TRIANGLES {
            return Err(format!("the model has {} triangles; a MeshCollider takes at most {MAX_TRIANGLES} (make a simpler copy of the level for collision)", tris.len()));
        }
        if verts.iter().any(|v| !finite3(*v)) {
            return Err("the model has vertices that are not numbers (NaN or infinity)".to_string());
        }
        if tris.iter().any(|t| t.iter().any(|&i| i as usize >= verts.len())) {
            return Err("a triangle points at a vertex that does not exist".to_string());
        }
        let total = tris.len();
        let kept: Vec<[u32; 3]> = tris.into_iter().filter(|t| !is_degenerate(verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize])).collect();
        if kept.is_empty() {
            return Err(format!("the model has no usable triangles: all {total} have no area"));
        }
        let degenerate = total - kept.len();
        let mut boxes: Vec<Aabb> = Vec::with_capacity(kept.len());
        let mut cents: Vec<V3> = Vec::with_capacity(kept.len());
        let mut bounds = Aabb::EMPTY;
        for t in &kept {
            let mut b = Aabb::EMPTY;
            for &i in t {
                b.grow(verts[i as usize]);
            }
            bounds = bounds.union(&b);
            cents.push(b.centre());
            boxes.push(b);
        }
        let mut order: Vec<u32> = (0..kept.len() as u32).collect();
        let mut nodes: Vec<Node> = Vec::with_capacity(2 * kept.len() / LEAF + 2);
        let n = order.len();
        build_range(&mut nodes, &mut order, 0, &boxes, &cents);
        debug_assert!(n > 0);
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        fnv(&mut h, verts.len() as u32);
        fnv(&mut h, kept.len() as u32);
        for v in &verts {
            for c in v {
                fnv(&mut h, c.to_bits());
            }
        }
        for t in &kept {
            for &i in t {
                fnv(&mut h, i);
            }
        }
        Ok(CollisionMesh { verts, tris: kept, order, nodes, bounds, hash: h, degenerate, warnings: Vec::new() })
    }

    /// Read the model `name` of the workspace `root` (`.glb` / `.gltf`, the same path rules as `MeshRenderer.mesh`).
    pub fn read(root: &Path, name: &str) -> Result<CollisionMesh, String> {
        let raw = crate::meshcol_gltf::read_model(root, name)?;
        let mut m = CollisionMesh::new(raw.verts, raw.tris)?;
        m.warnings = raw.warnings;
        Ok(m)
    }

    pub fn from_bytes(bytes: &[u8], dir: Option<&Path>) -> Result<CollisionMesh, String> {
        let raw = crate::meshcol_gltf::parse_model(bytes, dir)?;
        let mut m = CollisionMesh::new(raw.verts, raw.tris)?;
        m.warnings = raw.warnings;
        Ok(m)
    }

    /// The three corners of triangle `i` in the model's coordinates.
    #[inline]
    pub fn tri(&self, i: u32) -> [V3; 3] {
        let t = self.tris[i as usize];
        [self.verts[t[0] as usize], self.verts[t[1] as usize], self.verts[t[2] as usize]]
    }

    /// The vertices that some triangle uses, each once: the points a convex hull is made of.
    pub fn hull_points(&self) -> Vec<V3> {
        let mut idx: Vec<u32> = self.tris.iter().flatten().copied().collect();
        idx.sort_unstable();
        idx.dedup();
        idx.iter().map(|&i| self.verts[i as usize]).collect()
    }

    /// The nearest hit of a ray (`o` + `d` * t, t from 0 to `max`) with the triangles, in the model's coordinates. `d` need not be a unit
    /// vector (then `t` is in units of its length). Both faces count, the normal faces the ray. The arithmetic is the renderer's
    /// (`lunaris-render/src/query.rs`), so a picture ray and this one find the same triangle at the same distance.
    pub fn ray_local(&self, o: V3, d: V3, max: f32) -> Option<MeshRay> {
        ray_nodes(&self.nodes, &self.order, &|i| self.tri(i), o, d, max)
    }
}

fn build_range(nodes: &mut Vec<Node>, order: &mut [u32], base: usize, boxes: &[Aabb], cents: &[V3]) -> usize {
    let me = nodes.len();
    let mut b = Aabb::EMPTY;
    for &t in order.iter() {
        b = b.union(&boxes[t as usize]);
    }
    nodes.push(Node { b, link: 0, count: 0 });
    let n = order.len();
    if n <= LEAF {
        order.sort_unstable(); // (a leaf lists its triangles by number, whatever the selection below did)
        nodes[me].link = base as u32;
        nodes[me].count = n as u32;
        return me;
    }
    // split at the median of the triangle centres along the longest axis of the box of the centres
    let mut cb = Aabb::EMPTY;
    for &t in order.iter() {
        cb.grow(cents[t as usize]);
    }
    let ext = sub(cb.hi, cb.lo);
    let axis = if ext[0] >= ext[1] && ext[0] >= ext[2] {
        0
    } else if ext[1] >= ext[2] {
        1
    } else {
        2
    };
    let mid = n / 2;
    // (strict total order: coordinate, then triangle number, so the two halves are the same sets whatever the sort does inside)
    order.select_nth_unstable_by(mid, |&x, &y| cents[x as usize][axis].total_cmp(&cents[y as usize][axis]).then(x.cmp(&y)));
    let (l, r) = order.split_at_mut(mid);
    build_range(nodes, l, base, boxes, cents);
    let right = build_range(nodes, r, base + mid, boxes, cents);
    nodes[me].link = right as u32;
    me
}

// ------------------------------------------------------------------------------------------------ rays

/// Distance at which the ray enters the box, if it does within `[0, tmax]`.
#[inline]
fn slab(b: &Aabb, o: V3, d: V3, tmax: f32) -> Option<f32> {
    let (mut t0, mut t1) = (0.0f32, tmax);
    for k in 0..3 {
        // (padded a little, so a ray that grazes the surface of a flat box is not lost to rounding)
        let pad = 1.0e-5 * (1.0 + b.lo[k].abs().max(b.hi[k].abs()));
        let (lo, hi) = (b.lo[k] - pad, b.hi[k] + pad);
        if d[k].abs() < 1.0e-30 {
            if o[k] < lo || o[k] > hi {
                return None;
            }
        } else {
            let inv = 1.0 / d[k];
            let (mut ta, mut tb) = ((lo - o[k]) * inv, (hi - o[k]) * inv);
            if ta > tb {
                std::mem::swap(&mut ta, &mut tb);
            }
            t0 = t0.max(ta);
            t1 = t1.min(tb);
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(t0)
}

/// Moeller-Trumbore against one triangle: (t, unit normal facing the ray). The renderer's `hit_mesh`, line for line.
#[inline]
fn hit_triangle(a: V3, b: V3, c: V3, o: V3, d: V3) -> Option<(f32, V3)> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let tv = sub(o, a);
    let u = dot(tv, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(tv, e1);
    let v = dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(e2, q) * inv;
    if t < 0.0 {
        return None;
    }
    let cr = cross(e1, e2);
    let l = dot(cr, cr).sqrt();
    let mut n = if l > 0.0 { mul(cr, 1.0 / l) } else { cr };
    if dot(n, d) > 0.0 {
        n = mul(n, -1.0);
    }
    Some((t, n))
}

fn ray_nodes(nodes: &[Node], order: &[u32], tri: &dyn Fn(u32) -> [V3; 3], o: V3, d: V3, max: f32) -> Option<MeshRay> {
    if nodes.is_empty() || !finite3(o) || !finite3(d) || !(max >= 0.0) {
        return None;
    }
    let mut best: Option<MeshRay> = None;
    let mut best_t = max;
    let mut stack = [0u32; 128];
    let mut sp = 1usize;
    while sp > 0 {
        sp -= 1;
        let i = stack[sp] as usize;
        let n = &nodes[i];
        match slab(&n.b, o, d, best_t) {
            None => continue,
            Some(_) => {}
        }
        if n.count > 0 {
            for k in 0..n.count {
                let ti = order[(n.link + k) as usize];
                let [a, b, c] = tri(ti);
                if let Some((t, normal)) = hit_triangle(a, b, c, o, d) {
                    // the nearest wins; at the same distance the lower triangle number (what a straight scan of the list finds)
                    let better = match best {
                        None => t <= best_t,
                        Some(h) => t < h.t || (t == h.t && ti < h.tri),
                    };
                    if better {
                        best_t = t;
                        best = Some(MeshRay { t, tri: ti, normal });
                    }
                }
            }
        } else if sp + 2 <= stack.len() {
            // the nearer child is looked at first (it is pushed last)
            let (l, r) = (i + 1, n.link as usize);
            let (tl, tr) = (slab(&nodes[l].b, o, d, best_t), slab(&nodes[r].b, o, d, best_t));
            match (tl, tr) {
                (None, None) => {}
                (Some(_), None) => {
                    stack[sp] = l as u32;
                    sp += 1;
                }
                (None, Some(_)) => {
                    stack[sp] = r as u32;
                    sp += 1;
                }
                (Some(a), Some(b)) => {
                    let (near, far) = if a <= b { (l, r) } else { (r, l) };
                    stack[sp] = far as u32;
                    stack[sp + 1] = near as u32;
                    sp += 2;
                }
            }
        }
    }
    best
}

// ------------------------------------------------------------------------------------------------ a mesh standing in the world

/// A [`CollisionMesh`] placed by a `Transform`: the vertices in world coordinates and the tree's boxes fitted around them.
#[derive(Clone, Debug)]
pub struct PlacedMesh {
    pub mesh: Arc<CollisionMesh>,
    pub verts: Vec<V3>,
    pub(crate) nodes: Vec<Node>,
    pub bounds: Aabb,
}

impl PlacedMesh {
    /// Place `mesh` with a transform. `None` when the transform is not made of finite numbers or throws the model beyond what f32 can
    /// hold sensibly (1e15 metres).
    pub fn new(mesh: Arc<CollisionMesh>, t: &Transform) -> Option<PlacedMesh> {
        if !(finite3(t.pos) && finite3(t.scale) && t.rot.iter().all(|v| v.is_finite())) {
            return None;
        }
        let axes = crate::physics_rot::axes_of(crate::physics_rot::normalize_q(t.rot));
        let rotate = |v: V3| add(add(mul(axes[0], v[0]), mul(axes[1], v[1])), mul(axes[2], v[2]));
        let verts: Vec<V3> = mesh.verts.iter().map(|v| add(t.pos, rotate([v[0] * t.scale[0], v[1] * t.scale[1], v[2] * t.scale[2]]))).collect();
        if verts.iter().any(|v| !finite3(*v) || v.iter().any(|c| c.abs() > 1.0e15)) {
            return None;
        }
        let mut nodes = mesh.nodes.clone();
        // refit: children come after their parent in the list, so going backwards finishes every child before its parent
        for i in (0..nodes.len()).rev() {
            let n = nodes[i];
            let b = if n.count > 0 {
                let mut b = Aabb::EMPTY;
                for k in 0..n.count {
                    let tri = mesh.tris[mesh.order[(n.link + k) as usize] as usize];
                    for &vi in &tri {
                        b.grow(verts[vi as usize]);
                    }
                }
                b
            } else {
                nodes[i + 1].b.union(&nodes[n.link as usize].b)
            };
            nodes[i].b = b;
        }
        let bounds = nodes.first().map_or(Aabb::EMPTY, |n| n.b);
        Some(PlacedMesh { mesh, verts, nodes, bounds })
    }

    /// The three corners of triangle `i` in the world.
    #[inline]
    pub fn tri(&self, i: u32) -> [V3; 3] {
        let t = self.mesh.tris[i as usize];
        [self.verts[t[0] as usize], self.verts[t[1] as usize], self.verts[t[2] as usize]]
    }

    pub fn triangle_count(&self) -> usize {
        self.mesh.tris.len()
    }

    /// Visit the triangles whose leaf boxes pass `test` (unordered).
    pub(crate) fn walk(&self, test: &mut impl FnMut(&Aabb) -> bool, visit: &mut impl FnMut(u32)) {
        walk(&self.nodes, &self.mesh.order, test, visit);
    }

    /// The numbers of the triangles whose leaf boxes touch `b`, ascending (a superset of the triangles that touch the box).
    pub fn triangles_near(&self, b: &Aabb) -> Vec<u32> {
        let mut out = Vec::new();
        self.walk(&mut |nb: &Aabb| nb.overlaps(b), &mut |t| out.push(t));
        out.sort_unstable();
        out
    }

    /// The nearest hit of a ray in world coordinates: `d` should be a unit vector, `max` the longest distance. Returns the hit
    /// distance, the unit normal facing the ray and the triangle.
    pub fn ray(&self, o: V3, d: V3, max: f32) -> Option<MeshRay> {
        ray_nodes(&self.nodes, &self.mesh.order, &|i| self.tri(i), o, d, max)
    }
}

// ------------------------------------------------------------------------------------------------ the store

/// What the world knows of its mesh colliders: the files that have been read (by path in the workspace) and the placed copies of them
/// (kept between ticks so that a level that stands still is placed once). Cloning shares the placed copies, which is harmless: a
/// placed mesh is a pure function of the file and the transform, so it does not matter who made it first.
#[derive(Clone, Default)]
pub struct MeshStore {
    pub by_name: BTreeMap<String, Arc<CollisionMesh>>,
    placed: Arc<Mutex<BTreeMap<u64, Placement>>>,
    /// The convex hulls made of the models, by (model, scale): what a hull is made of depends on both.
    hulls: Arc<Mutex<BTreeMap<(String, [u32; 3]), (u64, Result<Arc<Hull>, String>)>>>,
}

/// One `ConvexCollider` of the world, with its hull and where the hull stands.
#[derive(Clone)]
pub struct HullInstance {
    pub uid: u64,
    pub entity: Entity,
    pub hull: Arc<Hull>,
    /// The centre of mass in the world, and the orientation of the hull's principal axes.
    pub pos: V3,
    pub q: [f32; 4],
    /// The `ConvexCollider.trigger` flag: it reports overlaps and is never in the way.
    pub trigger: bool,
}

impl HullInstance {
    /// The hull's own x, y and z axes in world coordinates.
    pub fn axes(&self) -> [V3; 3] {
        crate::physics_rot::axes_of(self.q)
    }

    /// The box around the hull in the world.
    pub fn bounds(&self) -> Aabb {
        let e = self.hull.world_extent(&self.axes());
        Aabb { lo: sub(self.pos, e), hi: add(self.pos, e) }
    }

    /// The nearest hit of a ray in the world (`d` a unit vector): the distance and the normal of the face (facing the ray).
    pub fn ray(&self, o: V3, d: V3, max: f32) -> Option<(f32, V3)> {
        self.hull.ray_world(self.pos, self.q, o, d, max)
    }
}

struct Placement {
    mesh_hash: u64,
    key: [u32; 10],
    placed: Option<Arc<PlacedMesh>>,
}

/// One `MeshCollider` of the world, placed, with its contact material.
#[derive(Clone)]
pub struct Instance {
    pub uid: u64,
    pub entity: Entity,
    pub placed: Arc<PlacedMesh>,
    pub friction: f32,
    pub restitution: f32,
}

fn transform_key(t: &Transform) -> [u32; 10] {
    [t.pos[0].to_bits(), t.pos[1].to_bits(), t.pos[2].to_bits(), t.rot[0].to_bits(), t.rot[1].to_bits(), t.rot[2].to_bits(), t.rot[3].to_bits(), t.scale[0].to_bits(), t.scale[1].to_bits(), t.scale[2].to_bits()]
}

impl MeshStore {
    /// No file has been read: there is nothing for any system to do.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Every solid `MeshCollider` whose file has been read and whose transform is usable, placed, in uid order.
    pub fn instances(&self, world: &mut World) -> Vec<Instance> {
        if self.by_name.is_empty() {
            return Vec::new();
        }
        let mut raw: Vec<(u64, Entity, Transform, MeshCollider)> = Vec::new();
        let _ = world.for_each_uid::<(&Transform, &MeshCollider), _>(|e, uid, (t, m)| raw.push((uid, e, *t, *m)));
        raw.sort_by_key(|r| r.0);
        let live: BTreeSet<u64> = raw.iter().map(|r| r.0).collect();
        let mut cache = self.placed.lock().unwrap_or_else(|e| e.into_inner());
        cache.retain(|uid, _| live.contains(uid));
        let mut out = Vec::with_capacity(raw.len());
        for (uid, entity, t, m) in raw {
            if !m.solid {
                continue;
            }
            let Some(mesh) = self.by_name.get(world.strings().get(m.mesh.0)) else { continue };
            let key = transform_key(&t);
            let fresh = cache.get(&uid).is_some_and(|p| p.mesh_hash == mesh.hash && p.key == key);
            if !fresh {
                cache.insert(uid, Placement { mesh_hash: mesh.hash, key, placed: PlacedMesh::new(mesh.clone(), &t).map(Arc::new) });
            }
            let Some(placed) = cache.get(&uid).and_then(|p| p.placed.clone()) else { continue };
            out.push(Instance { uid, entity, placed, friction: m.friction.max(0.0), restitution: m.restitution.clamp(0.0, 1.0) });
        }
        out
    }

    /// The convex hull of a model stretched by `scale`: built the first time it is asked for, kept for the next. Fails (with the reason) when the
    /// model has not been read or its points make no solid.
    pub fn hull(&self, name: &str, scale: V3) -> Result<Arc<Hull>, String> {
        let mesh = self.by_name.get(name).ok_or_else(|| "the model has not been read".to_string())?;
        let key = (name.to_string(), [scale[0].to_bits(), scale[1].to_bits(), scale[2].to_bits()]);
        let mut cache = self.hulls.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((hash, r)) = cache.get(&key) {
            if *hash == mesh.hash {
                return r.clone();
            }
        }
        let r = Hull::build(&mesh.hull_points(), scale).map(Arc::new);
        if cache.len() >= 512 {
            cache.clear(); // (a hull whose scale changes every tick would fill it)
        }
        cache.insert(key, (mesh.hash, r.clone()));
        r
    }

    /// Every `ConvexCollider` whose model has been read and makes a solid, in uid order.
    pub fn hull_instances(&self, world: &mut World) -> Vec<HullInstance> {
        if self.by_name.is_empty() {
            return Vec::new();
        }
        let mut raw: Vec<(u64, Entity, Transform, ConvexCollider)> = Vec::new();
        let _ = world.for_each_uid::<(&Transform, &ConvexCollider), _>(|e, uid, (t, c)| raw.push((uid, e, *t, *c)));
        raw.sort_by_key(|r| r.0);
        let mut out = Vec::with_capacity(raw.len());
        for (uid, entity, t, c) in raw {
            let scale = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
            let name = world.strings().get(c.mesh.0).to_string();
            let Ok(hull) = self.hull(&name, scale) else { continue };
            let (pos, q) = hull.pose(&t);
            if pos.iter().all(|v| v.is_finite()) {
                out.push(HullInstance { uid, entity, hull, pos, q, trigger: c.trigger });
            }
        }
        out
    }

    /// The nearest hit of the ray among all mesh colliders: (distance, unit normal facing the ray, uid of the entity). `dir` is a unit vector.
    pub fn ray(&self, world: &mut World, o: V3, dir: V3, max: f32) -> Option<(f32, V3, u64)> {
        let mut best: Option<(f32, V3, u64)> = None;
        for inst in self.instances(world) {
            if let Some(h) = inst.placed.ray(o, dir, max) {
                if best.is_none_or(|b| h.t < b.0) {
                    best = Some((h.t, h.normal, inst.uid));
                }
            }
        }
        best
    }

    /// Is anything of a mesh collider between the two points? (The sensors' line of sight.)
    pub fn blocks(&self, world: &mut World, a: V3, b: V3, skip: [u64; 2]) -> bool {
        if self.by_name.is_empty() {
            return false;
        }
        let d = sub(b, a);
        let len = dot(d, d).sqrt();
        if !(len > 1.0e-5) {
            return false;
        }
        let dir = mul(d, 1.0 / len);
        self.instances(world).iter().any(|inst| inst.uid != skip[0] && inst.uid != skip[1] && inst.placed.ray(a, dir, len).is_some())
    }
}
