//! A character controller: a capsule that walks, jumps, climbs low steps, slides along walls and falls, so that a game does not have to
//! build a body out of rigid-body physics. A `CharacterBody` entity has a `Transform` (its feet) and is moved by the engine every tick
//! from `walk` (the horizontal speed it wants, set by a graph system or a plugin from the input) and `jump` (a one-tick request); the
//! engine answers in `grounded` and `fall`. It collides with every non-trigger `Collider` (spheres and axis-aligned boxes, like the
//! rest of the physics) except other characters.
//!
//! Each tick, in uid order, after the graph systems have run (so this tick's input counts) and before attached entities follow their
//! parents (so a camera on the character does): gravity and a requested jump change the vertical speed; the horizontal move is tried
//! as it is and, when a low obstacle blocks it, again lifted by `step_height` (stairs, kerbs) keeping whichever got further; the
//! vertical move comes last; and after every move the capsule is pushed out of whatever it overlaps, which is what makes it slide
//! along walls. Everything is f32 with only `+ - * / sqrt`, in a fixed order, so results are identical on every machine.
use crate::physics::Contact;
use crate::components::{CharacterBody, CharacterMotion, Climbable, Collider, PhysicsSettings, Steer, Terrain, Transform, Velocity};
use crate::layers::{Filter, Layers};
use crate::motion::{self, Ladder};
use crate::pool::Pool;
use crate::meshcol::{MeshStore, PlacedMesh};
use crate::terrain::{Placed, Terrains};
use lunaris_ecs::{Entity, World};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;

type V3 = [f32; 3];

#[derive(Clone, Copy)]
enum Shape {
    Sphere(f32),
    Box(V3),
    /// A heightmap ground: its index in `Grid::placed`.
    Terrain(usize),
    /// A triangle mesh (a `MeshCollider`): its index in `Grid::meshes`.
    Mesh(usize),
}

struct Obstacle {
    /// The entity it is.
    uid: u64,
    centre: V3,
    shape: Shape,
    /// Its collision layer (`CollisionLayer`): a character walks through what it does not collide with.
    filter: Filter,
}

const CELL: f32 = 4.0;
/// Obstacles wider than 80 m (a block of a streamed city: a mesh a hundred metres across) are kept in a coarse grid of squares this wide, so that a character looks at the few of them that are near and not at every one in the world.
const BIG_CELL: f32 = 64.0;
/// Obstacles wider than this are in no grid at all: every character looks at them (a floor a kilometre wide).
const HUGE: f32 = 400.0;
const ITERATIONS: usize = 6;
const EPS: f32 = 1.0e-4;
/// How far (m) a jump that bonks its head on the corner of a ceiling is nudged sideways round it (Celeste's corner correction): a head that overlaps a ceiling's edge by less than this goes on up beside it instead of stopping dead.
const CORNER_NUDGE: f32 = 0.22;
/// The directions of the nudge (x, z), eight round the capsule.
const CORNER_DIRS: [[f32; 2]; 8] = [[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0], [0.7071, 0.7071], [-0.7071, 0.7071], [0.7071, -0.7071], [-0.7071, -0.7071]];

/// A cheap hash for the grid's keys (two small integers): the grid is looked up several times for every capsule every tick and laid out again every tick, and the standard hasher (SipHash) costs more
/// than the rest of such a lookup. The maps are only looked up and filled, never walked, so the hash cannot change what a character does.
#[derive(Default, Clone, Copy)]
pub(crate) struct CellHasher(u64);

impl Hasher for CellHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0.rotate_left(26)
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(5) ^ *b as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    #[inline]
    fn write_i32(&mut self, i: i32) {
        self.0 = (self.0.rotate_left(5) ^ i as u32 as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

type CellMap = HashMap<(i32, i32), Vec<usize>, BuildHasherDefault<CellHasher>>;

fn cell(v: f32) -> i32 {
    (v / CELL).floor() as i32
}

fn big_cell(v: f32) -> i32 {
    (v / BIG_CELL).floor() as i32
}

/// The cells of the last grid on this thread and what they were laid out from: the centre and half width and depth of every obstacle, in order (all that `Grid::new` looks at). A world whose solids stay where they are
/// has the same layout every step, and laying out ten thousand obstacles in hash maps was the biggest part of the characters' cost in the city server (0.57 ms of 4.9); when the layout is the same the maps are
/// reused (the lists hold indices into the obstacles of the new grid, which are the same).
struct GridCache {
    keys: Vec<[f32; 4]>,
    cells: CellMap,
    big: CellMap,
}

thread_local! {
    static GRID_CACHE: std::cell::RefCell<Option<GridCache>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// Tests: how many grids were laid out afresh on this thread, and whether reuse is switched off.
    pub(crate) static GRID_BUILDS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    pub(crate) static GRID_REUSE_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Tests: how many obstacles had their cells moved by a patch, and whether every patched layout is checked against a fresh one.
    pub(crate) static GRID_PATCHES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    pub(crate) static GRID_VERIFY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The obstacles of the world, in a grid over the ground plane (x, z) so that a character only looks at what is near it.
pub(crate) struct Grid<'a> {
    obstacles: Vec<Obstacle>,
    cells: CellMap,
    /// The obstacles wider than 80 m, by squares of `BIG_CELL`.
    big: CellMap,
    /// What the layout was made from (see `GridCache`).
    keys: Vec<[f32; 4]>,
    placed: Vec<Placed<'a>>,
    meshes: Vec<Arc<PlacedMesh>>,
    /// Some entity has a `CollisionLayer`: the obstacles a character meets are filtered by the layer of the one that asks.
    layered: bool,
}

impl Drop for Grid<'_> {
    fn drop(&mut self) {
        let cache = GridCache { keys: std::mem::take(&mut self.keys), cells: std::mem::take(&mut self.cells), big: std::mem::take(&mut self.big) };
        GRID_CACHE.with(|c| *c.borrow_mut() = Some(cache));
    }
}

/// The grid as one capsule sees it: the same grid for all of them, and the layer and mask (`who`) of the capsule that is being moved. Made for each capsule, so that capsules can be walked
/// on several threads at once with the grid shared.
pub(crate) struct View<'g, 'a> {
    grid: &'g Grid<'a>,
    who: Filter,
}

impl<'a> std::ops::Deref for View<'_, 'a> {
    type Target = Grid<'a>;
    fn deref(&self) -> &Grid<'a> {
        self.grid
    }
}

impl View<'_, '_> {
    /// Indices of the obstacles that may touch a capsule around (x, z) with this radius that this capsule collides with (see `Grid::near_for`).
    fn near(&self, x: f32, z: f32, r: f32, out: &mut Vec<usize>) {
        self.grid.near_for(self.who, x, z, r, out);
    }
}

/// Which map of a grid a cell is in: the fine one, the coarse one (obstacles up to 400 m wide), or the one list of the obstacles wider than that.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Where {
    Small,
    Big,
    Sentinel,
}

/// The cells an obstacle with this key (middle x, middle z, half width, half depth) is in.
fn each_cell(key: [f32; 4], mut f: impl FnMut(Where, (i32, i32))) {
    let [mx, mz, hx, hz] = key;
    // large obstacles would fill hundreds of cells: those up to 400 m wide go in the coarse grid, the larger ones (a floor 1000 wide) in one list under a sentinel key
    if hx > 40.0 || hz > 40.0 {
        if hx <= HUGE && hz <= HUGE {
            for cx in big_cell(mx - hx)..=big_cell(mx + hx) {
                for cz in big_cell(mz - hz)..=big_cell(mz + hz) {
                    f(Where::Big, (cx, cz));
                }
            }
        } else {
            f(Where::Sentinel, (i32::MIN, i32::MIN));
        }
        return;
    }
    for cx in cell(mx - hx)..=cell(mx + hx) {
        for cz in cell(mz - hz)..=cell(mz + hz) {
            f(Where::Small, (cx, cz));
        }
    }
}

/// The layout of a list of obstacles, each list of cells in the order of the obstacles.
fn layout(keys: &[[f32; 4]]) -> (CellMap, CellMap) {
    // (about one cell per obstacle: a small box is in one to four)
    let mut cells = CellMap::with_capacity_and_hasher(keys.len() * 2, Default::default());
    let mut big = CellMap::default();
    for (i, &k) in keys.iter().enumerate() {
        each_cell(k, |w, c| {
            if w == Where::Big {
                big.entry(c).or_default().push(i);
            } else {
                cells.entry(c).or_default().push(i);
            }
        });
    }
    (cells, big)
}

impl<'a> Grid<'a> {
    fn new(obstacles: Vec<Obstacle>, placed: Vec<Placed<'a>>, meshes: Vec<Arc<PlacedMesh>>, layered: bool) -> Grid<'a> {
        // what the layout depends on: the middle and the half width and depth of each obstacle
        let keys: Vec<[f32; 4]> = obstacles
            .iter()
            .map(|o| {
                let (hx, hz) = match o.shape {
                    Shape::Sphere(r) => (r, r),
                    Shape::Box(h) => (h[0], h[2]),
                    Shape::Terrain(_) => (f32::INFINITY, f32::INFINITY),
                    Shape::Mesh(k) => {
                        let h = meshes[k].bounds.half();
                        (h[0], h[2])
                    }
                };
                [o.centre[0], o.centre[2], hx, hz]
            })
            .collect();
        #[cfg(test)]
        let reuse_off = GRID_REUSE_OFF.with(|f| f.get());
        #[cfg(not(test))]
        let reuse_off = false;
        if !reuse_off {
            if let Some(mut k) = GRID_CACHE.with(|c| c.borrow_mut().take()) {
                // the same obstacles in the same order, with some moved (a car, a door, a drone: a handful among thousands): only their cells are touched, and each list stays in the
                // order of the obstacles, so the layout is exactly the one a fresh build makes. Many changed (an obstacle came or went and the numbers of the rest shifted): a fresh build is cheaper.
                if k.keys.len() == keys.len() {
                    let changed: Vec<usize> = (0..keys.len()).filter(|&i| k.keys[i] != keys[i]).collect();
                    if changed.len() * 4 <= keys.len() {
                        for &i in &changed {
                            each_cell(k.keys[i], |w, c| {
                                let map = if w == Where::Big { &mut k.big } else { &mut k.cells };
                                if let Some(list) = map.get_mut(&c) {
                                    if let Ok(p) = list.binary_search(&i) {
                                        list.remove(p);
                                        if list.is_empty() {
                                            map.remove(&c);
                                        }
                                    }
                                }
                            });
                        }
                        for &i in &changed {
                            each_cell(keys[i], |w, c| {
                                let list = if w == Where::Big { k.big.entry(c).or_default() } else { k.cells.entry(c).or_default() };
                                if let Err(p) = list.binary_search(&i) {
                                    list.insert(p, i);
                                }
                            });
                        }
                        #[cfg(test)]
                        {
                            GRID_PATCHES.with(|b| b.set(b.get() + changed.len() as u32));
                            if GRID_VERIFY.with(|f| f.get()) {
                                let (cells, big) = layout(&keys);
                                assert!(cells == k.cells && big == k.big, "a patched layout is the one a fresh build makes");
                            }
                        }
                        return Grid { obstacles, cells: k.cells, big: k.big, keys, placed, meshes, layered };
                    }
                }
            }
        }
        #[cfg(test)]
        GRID_BUILDS.with(|b| b.set(b.get() + 1));
        let (cells, big) = layout(&keys);
        Grid { obstacles, cells, big, keys, placed, meshes, layered }
    }

    /// Indices of the obstacles that may touch a capsule around (x, z) with this radius, ascending and without duplicates, leaving out what the layers of `who` say it walks through.
    fn near_for(&self, who: Filter, x: f32, z: f32, r: f32, out: &mut Vec<usize>) {
        out.clear();
        for cx in cell(x - r)..=cell(x + r) {
            for cz in cell(z - r)..=cell(z + r) {
                if let Some(v) = self.cells.get(&(cx, cz)) {
                    out.extend_from_slice(v);
                }
            }
        }
        if !self.big.is_empty() {
            for cx in big_cell(x - r)..=big_cell(x + r) {
                for cz in big_cell(z - r)..=big_cell(z + r) {
                    if let Some(v) = self.big.get(&(cx, cz)) {
                        out.extend_from_slice(v);
                    }
                }
            }
        }
        if let Some(v) = self.cells.get(&(i32::MIN, i32::MIN)) {
            out.extend_from_slice(v);
        }
        out.sort_unstable();
        out.dedup();
        if self.layered {
            // (what the character does not collide with is not there for it)
            out.retain(|&i| who.collides(self.obstacles[i].filter));
        }
    }
}

/// Sphere centre `c`, radius `r`, against a box: the push that moves the sphere out (pointing away from the box) and its depth.
fn sphere_vs_box(c: V3, r: f32, bc: V3, h: V3) -> Option<(V3, f32)> {
    let d = [c[0] - bc[0], c[1] - bc[1], c[2] - bc[2]];
    let closest = [d[0].clamp(-h[0], h[0]), d[1].clamp(-h[1], h[1]), d[2].clamp(-h[2], h[2])];
    if d[0].abs() <= h[0] && d[1].abs() <= h[1] && d[2].abs() <= h[2] {
        // the centre is inside the box: leave through the nearest face
        let mut best = (f32::INFINITY, 0usize);
        for axis in 0..3 {
            let to_face = h[axis] - d[axis].abs();
            if to_face < best.0 {
                best = (to_face, axis);
            }
        }
        let mut n = [0.0; 3];
        n[best.1] = if d[best.1] >= 0.0 { 1.0 } else { -1.0 };
        return Some((n, best.0 + r));
    }
    let diff = [d[0] - closest[0], d[1] - closest[1], d[2] - closest[2]];
    let dist2 = diff[0] * diff[0] + diff[1] * diff[1] + diff[2] * diff[2];
    if dist2 >= r * r {
        return None;
    }
    let dist = dist2.sqrt();
    Some(([diff[0] / dist, diff[1] / dist, diff[2] / dist], r - dist))
}

/// The vertical capsule with its feet at `pos`: the segment its centres sweep, bottom and top.
fn segment(pos: V3, r: f32, height: f32) -> (f32, f32) {
    let (y0, y1) = (pos[1] + r, pos[1] + height - r);
    if y1 < y0 {
        let m = pos[1] + height * 0.5;
        (m, m)
    } else {
        (y0, y1)
    }
}

/// One overlap of the capsule with an obstacle: the push out (unit, away from the obstacle) and how deep.
fn overlap(pos: V3, r: f32, height: f32, o: &Obstacle, placed: &[Placed], meshes: &[Arc<PlacedMesh>]) -> Option<(V3, f32)> {
    let (y0, y1) = segment(pos, r, height);
    match o.shape {
        // a mesh: the capsule (the segment its two end balls sweep, with the radius) against the triangles
        Shape::Mesh(k) => meshes[k].capsule_push([pos[0], y0, pos[2]], [pos[0], y1, pos[2]], r).map(|(n, d, _)| (n, d)),
        Shape::Terrain(k) => {
            // the two balls of the capsule: the lower one finds the ground, the upper one a cliff
            let low = placed[k].sphere([pos[0], y0, pos[2]], r);
            let high = if y1 > y0 { placed[k].sphere([pos[0], y1, pos[2]], r) } else { None };
            match (low, high) {
                (Some(a), Some(b)) => Some(if b.1 > a.1 { b } else { a }),
                (a, None) => a,
                (None, b) => b,
            }
        }
        Shape::Sphere(sr) => {
            let ys = o.centre[1].clamp(y0, y1);
            let c = [pos[0], ys, pos[2]];
            let d = [c[0] - o.centre[0], c[1] - o.centre[1], c[2] - o.centre[2]];
            let reach = r + sr;
            let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            if d2 >= reach * reach {
                return None;
            }
            let dist = d2.sqrt();
            let n = if dist > 1.0e-9 { [d[0] / dist, d[1] / dist, d[2] / dist] } else { [0.0, 1.0, 0.0] };
            Some((n, reach - dist))
        }
        Shape::Box(h) => {
            let (lo, hi) = (o.centre[1] - h[1], o.centre[1] + h[1]);
            let ys = if y1 < lo {
                y1
            } else if y0 > hi {
                y0
            } else {
                (y0.max(lo) + y1.min(hi)) * 0.5
            };
            sphere_vs_box([pos[0], ys, pos[2]], r, o.centre, h)
        }
    }
}

/// Push the capsule out of everything it overlaps (a few rounds, so that corners settle). Returns the pushes' largest upward and
/// downward components of the contact normals: `(floor, ceiling)` = whether something pushed it up from below / down from above
/// at a slope no steeper than `min_up`.
fn depenetrate(pos: &mut V3, r: f32, height: f32, grid: &View<'_, '_>, near: &mut Vec<usize>, min_up: f32) -> (bool, bool) {
    let (floor, ceiling, _) = depenetrate_rim(pos, r, height, grid, near, min_up);
    (floor, ceiling)
}

/// `depenetrate`, and whether the capsule's round foot rests on the rim of a box (the push of an edge or corner: up, but at a slant no floor has; the faces of boxes are axis-aligned, so a slanted push from one is a rim).
fn depenetrate_rim(pos: &mut V3, r: f32, height: f32, grid: &View<'_, '_>, near: &mut Vec<usize>, min_up: f32) -> (bool, bool, bool) {
    let (mut floor, mut ceiling, mut rim) = (false, false, false);
    for _ in 0..ITERATIONS {
        let mut moved = false;
        grid.near(pos[0], pos[2], r + EPS, near);
        for &i in near.iter() {
            let o = &grid.obstacles[i];
            // cheap vertical reject
            let top = match o.shape {
                Shape::Sphere(sr) => (o.centre[1] - sr, o.centre[1] + sr),
                Shape::Box(h) => (o.centre[1] - h[1], o.centre[1] + h[1]),
                Shape::Terrain(k) => grid.placed[k].y_range(),
                Shape::Mesh(k) => (grid.meshes[k].bounds.lo[1], grid.meshes[k].bounds.hi[1]),
            };
            if top.1 < pos[1] - EPS || top.0 > pos[1] + height + EPS {
                continue;
            }
            let hit = match o.shape {
                // a face of a mesh that leans more than `max_slope` allows is not ground but a wall: it pushes the capsule out sideways and
                // never up, so that walking into it does not slide the character up the slope
                Shape::Mesh(k) => {
                    let (y0, y1) = segment(*pos, r, height);
                    grid.meshes[k].capsule_push([pos[0], y0, pos[2]], [pos[0], y1, pos[2]], r).map(|(n, d, face)| {
                        let h = (n[0] * n[0] + n[2] * n[2]).sqrt();
                        if face && n[1] > 0.0 && n[1] < min_up && h > 1.0e-3 {
                            ([n[0] / h, 0.0, n[2] / h], d / h.max(0.3))
                        } else {
                            (n, d)
                        }
                    })
                }
                _ => overlap(*pos, r, height, o, &grid.placed, &grid.meshes),
            };
            if let Some((n, depth)) = hit {
                if depth <= EPS {
                    continue;
                }
                for k in 0..3 {
                    pos[k] += n[k] * depth;
                }
                moved = true;
                if n[1] >= min_up {
                    floor = true;
                } else if n[1] <= -0.5 {
                    ceiling = true;
                } else if n[1] > 0.15 && matches!(o.shape, Shape::Box(_)) {
                    rim = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
    (floor, ceiling, rim)
}

/// Does the capsule at `pos` lean on terrain that is steeper than `min_up` allows (the cosine of the steepest walkable slope)?
pub(crate) fn too_steep(pos: V3, r: f32, height: f32, grid: &View<'_, '_>, min_up: f32) -> bool {
    let probe = [pos[0], pos[1] - 0.02, pos[2]];
    grid.obstacles.iter().filter(|o| !grid.layered || grid.who.collides(o.filter)).any(|o| match o.shape {
        Shape::Terrain(_) => overlap(probe, r, height, o, &grid.placed, &grid.meshes).is_some_and(|(n, d)| d > EPS && n[1] < min_up),
        // a mesh: only the face of a triangle can be a slope (the rim of a ledge is not), and any of the capsule's balls may lean on one
        Shape::Mesh(k) => {
            let (y0, y1) = segment(probe, r, height);
            grid.meshes[k].leans_on_steep_face([probe[0], y0, probe[2]], r, min_up) || (y1 > y0 && grid.meshes[k].leans_on_steep_face([probe[0], y1, probe[2]], r, min_up))
        }
        _ => false,
    })
}

/// Is the capsule at `pos` overlapping anything?
pub(crate) fn blocked(pos: V3, r: f32, height: f32, grid: &View<'_, '_>, near: &mut Vec<usize>) -> bool {
    grid.near(pos[0], pos[2], r + EPS, near);
    near.iter().any(|&i| overlap(pos, r, height, &grid.obstacles[i], &grid.placed, &grid.meshes).is_some_and(|(_, d)| d > 0.002))
}

/// Lower the (free) capsule at `pos` by up to `drop` until it touches something. Returns the new position and whether it touched.
pub(crate) fn lower_to_contact(pos: V3, drop: f32, r: f32, height: f32, grid: &View<'_, '_>, near: &mut Vec<usize>) -> (V3, bool) {
    let at = |d: f32| [pos[0], pos[1] - d, pos[2]];
    if !blocked(at(drop), r, height, grid, near) {
        return (at(drop), false);
    }
    // bisection between "free" (0) and "blocked" (drop)
    let (mut lo, mut hi) = (0.0f32, drop);
    for _ in 0..10 {
        let mid = (lo + hi) * 0.5;
        if blocked(at(mid), r, height, grid, near) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    (at(lo), true)
}

/// Move the characters. Returns the solid contacts of the characters that have a `CharacterMotion` with `report_contacts` (a normal like a physics contact's: from the character to what it touches): the
/// simulation hands them to the graph systems of the next tick, so that `on_contact` sees a character that walks into a wall, which physics contacts never did. While the simulation is profiled
/// (`prof`) the time goes under three names: "characters: obstacles" (gathering every solid of the world), "characters: grid" (laying them out in the grid) and "characters: walking" (moving each capsule).
pub fn step(world: &mut World, dt: f32, terrains: &Terrains, mesh_store: &MeshStore, mut prof: Option<&mut crate::sim::StepProfile>, workers: &mut WalkPool, threads: usize) -> Vec<Contact> {
    let mut lap = std::time::Instant::now();
    let mut stamp = |prof: &mut Option<&mut crate::sim::StepProfile>, name: &str| {
        if let Some(p) = prof.as_deref_mut() {
            p.add(name, lap.elapsed().as_secs_f64() * 1e3);
            lap = std::time::Instant::now();
        }
    };
    let mut reported: Vec<Contact> = Vec::new();
    // ---- the characters (uid order); nothing to do without them
    let mut chars: Vec<(u64, Entity, Transform, CharacterBody)> = Vec::new();
    if world.for_each_uid::<(&Transform, &CharacterBody), _>(|e, uid, (t, c)| chars.push((uid, e, *t, *c))).is_err() || chars.is_empty() {
        return reported;
    }
    chars.sort_by_key(|c| c.0);
    // ---- the obstacles: every solid collider that is not a character
    let mut raw: Vec<(u64, Transform, Collider)> = Vec::new();
    let mut bodies = crate::vehicle_ray::UidSet::default();
    world.for_each_uid::<(&CharacterBody,), _>(|_, uid, _| {
        bodies.insert(uid);
    })
    .expect("character query is valid");
    world.for_each_uid::<(&Transform, &Collider), _>(|_, uid, (t, c)| raw.push((uid, *t, *c))).expect("collider query is valid");
    raw.sort_by_key(|r| r.0);
    let layers = Layers::gather(world);
    let mut obstacles = Vec::with_capacity(raw.len());
    for (uid, t, c) in raw {
        if c.trigger || bodies.contains(&uid) {
            continue;
        }
        let s = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
        let shape = if c.shape == 0 { Shape::Sphere(0.5 * c.size[0].abs() * s[0].max(s[1]).max(s[2])) } else { Shape::Box([0.5 * c.size[0].abs() * s[0], 0.5 * c.size[1].abs() * s[1], 0.5 * c.size[2].abs() * s[2]]) };
        obstacles.push(Obstacle { uid, centre: t.pos, shape, filter: layers.of(uid) });
    }
    // the ground made from heightmaps
    let mut placed: Vec<Placed> = Vec::new();
    if !terrains.is_empty() {
        let mut ground: Vec<(u64, Transform, Terrain)> = Vec::new();
        world.for_each_uid::<(&Transform, &Terrain), _>(|_, uid, (t, g)| ground.push((uid, *t, *g))).expect("terrain query is valid");
        ground.sort_by_key(|g| g.0);
        for (uid, t, g) in ground {
            if !g.solid {
                continue;
            }
            let name = world.strings().get(g.heightmap.0);
            let Some(field) = terrains.get(name) else { continue };
            let p = Placed::new(field, t.pos, &g);
            let (lo, hi) = p.y_range();
            placed.push(p);
            obstacles.push(Obstacle { uid, centre: [p.centre[0], (lo + hi) * 0.5, p.centre[1]], shape: Shape::Terrain(placed.len() - 1), filter: layers.of(uid) });
        }
    }
    // the level geometry of mesh colliders (a world without any has an empty store)
    let mut meshes: Vec<Arc<PlacedMesh>> = Vec::new();
    if !mesh_store.is_empty() {
        // a convex hull (a rock, a crate) is, for a character, the box around it
        for h in mesh_store.hull_instances(world) {
            if h.trigger || bodies.contains(&h.uid) {
                continue;
            }
            let b = h.bounds();
            obstacles.push(Obstacle { uid: h.uid, centre: b.centre(), shape: Shape::Box(b.half()), filter: layers.of(h.uid) });
        }
        for inst in mesh_store.instances(world) {
            meshes.push(inst.placed.clone());
            let c = inst.placed.bounds.centre();
            obstacles.push(Obstacle { uid: inst.uid, centre: c, shape: Shape::Mesh(meshes.len() - 1), filter: layers.of(inst.uid) });
        }
    }
    stamp(&mut prof, "characters: obstacles");
    let grid = Grid::new(obstacles, placed, meshes, !layers.is_empty());
    stamp(&mut prof, "characters: grid");
    let mut ps: Vec<(u64, Entity, PhysicsSettings)> = Vec::new();
    world.for_each_uid::<(&PhysicsSettings,), _>(|e, uid, (p,)| ps.push((uid, e, *p))).expect("settings query is valid");
    ps.sort_by_key(|p| p.0);
    let gravity = ps.first().map(|p| p.2.gravity[1]).unwrap_or(-9.8);
    // the characters that swim, roll and climb (`CharacterMotion`), the ladders they climb and the water they swim in: none of it is looked at in a world without them
    let mut motions: HashMap<u64, CharacterMotion> = HashMap::new();
    let _ = world.for_each_uid::<(&CharacterMotion,), _>(|_, uid, (m,)| {
        motions.insert(uid, *m);
    });
    let mut ladders: Vec<Ladder> = Vec::new();
    let mut waters = Vec::new();
    let mut sheets = Vec::new();
    // a `Steer` on a character with a motion walks it: its walk is the velocity the steering worked out (on the ground plane), and the velocity is then what the character really did
    let mut steered: HashMap<u64, [f32; 2]> = HashMap::new();
    if !motions.is_empty() {
        let _ = world.for_each_uid::<(&Steer, &Velocity), _>(|_, uid, (s, v)| {
            if s.active && s.plane == 0 && motions.contains_key(&uid) {
                steered.insert(uid, [v.lin[0], v.lin[2]]);
            }
        });
        let _ = world.for_each_uid::<(&Transform, &Climbable), _>(|_, uid, (t, l)| ladders.push(Ladder { uid, pos: t.pos, rot: t.rot, c: *l }));
        ladders.sort_by_key(|l| l.uid);
        waters = crate::water::bodies(world, terrains);
        sheets = crate::water::swim_sheets(world);
    }

    let cx = Walk { dt, gravity, terrains, strings: world.strings(), motions: &motions, steered: &steered, ladders: &ladders, waters: &waters, sheets: &sheets, layers: &layers, grid: &grid };
    // A capsule's walk reads the grid, the layers and the water and writes nothing but its own result: the capsules are walked on several threads when there are enough of them to pay for it,
    // and their results put back in uid order, so that nothing depends on the number of threads.
    let n = chars.len();
    let tasks = if threads > 1 && n >= WALK_PAR_MIN { n.div_ceil(WALK_CHUNK).min(threads * 4) } else { 1 };
    let per = n.div_ceil(tasks);
    let slots: Vec<std::sync::Mutex<Vec<Walked>>> = (0..tasks).map(|_| std::sync::Mutex::new(Vec::new())).collect();
    let work = |i: usize| {
        let mut near: Vec<usize> = Vec::new();
        let out: Vec<Walked> = chars[(i * per).min(n)..((i + 1) * per).min(n)].iter().map(|ch| walk_one(&cx, ch, &mut near)).collect();
        *slots[i].lock().unwrap_or_else(|e| e.into_inner()) = out;
    };
    if tasks > 1 {
        if workers.0.as_ref().map(|p| p.threads()) != Some(threads) {
            workers.0 = Some(Arc::new(Pool::new(threads - 1)));
        }
        if let Some(p) = workers.0.as_deref() {
            p.run(tasks, &work);
        }
    } else {
        work(0);
    }
    let walked: Vec<Walked> = slots.into_iter().flat_map(|s| s.into_inner().unwrap_or_else(|e| e.into_inner())).collect();
    stamp(&mut prof, "characters: walking");
    // put the results in the world, in uid order
    for w in walked {
        if w.carried {
            // (pulled up a ledge: nothing else of the walk was done)
            if let Some(tr) = world.get_mut::<Transform>(w.entity) {
                tr.pos = w.pos;
            }
            if let Some(cb) = world.get_mut::<CharacterBody>(w.entity) {
                cb.fall = 0.0;
                cb.grounded = false;
                cb.jump = 0.0;
            }
        } else {
            if let Some(tr) = world.get_mut::<Transform>(w.entity) {
                tr.pos = w.pos;
            }
            if let Some(cb) = world.get_mut::<CharacterBody>(w.entity) {
                cb.fall = w.fall;
                cb.grounded = w.grounded;
                cb.jump = 0.0;
            }
            if let Some(v) = w.vel {
                if let Some(vel) = world.get_mut::<Velocity>(w.entity) {
                    vel.lin[0] = v[0];
                    vel.lin[2] = v[1];
                }
            }
            reported.extend(w.contacts);
        }
        if let Some(m) = w.motion {
            if let Some(cm) = world.get_mut::<CharacterMotion>(w.entity) {
                *cm = m;
            }
        }
        let _ = w.uid;
    }
    stamp(&mut prof, "characters: write-back");
    reported
}

/// The least number of capsules worth walking on several threads (fewer is quicker on one: handing work to the threads costs about as much as walking a dozen of them).
const WALK_PAR_MIN: usize = 24;
/// The least number of capsules a task is given.
const WALK_CHUNK: usize = 8;

/// The worker threads the controller walks capsules on, made when there are enough capsules to pay for them. A clone of a simulation starts without them.
#[derive(Default)]
pub struct WalkPool(Option<Arc<Pool>>);

impl Clone for WalkPool {
    fn clone(&self) -> WalkPool {
        WalkPool(None)
    }
}

/// What the walk of one capsule reads (everything shared, nothing written): the same for every capsule of a tick.
#[derive(Clone, Copy)]
struct Walk<'w, 'a> {
    dt: f32,
    gravity: f32,
    terrains: &'w Terrains,
    strings: &'w lunaris_ecs::StringTable,
    motions: &'w HashMap<u64, CharacterMotion>,
    steered: &'w HashMap<u64, [f32; 2]>,
    ladders: &'w [Ladder],
    waters: &'w [(u64, V3, crate::components::WaterBody)],
    sheets: &'w [(u64, f32, [f32; 2], [f32; 2])],
    layers: &'w Layers,
    grid: &'w Grid<'a>,
}

/// What came of walking one capsule: written into the world afterwards, in uid order.
struct Walked {
    entity: Entity,
    uid: u64,
    /// It was pulled up a ledge (by `CharacterMotion`): `pos` is where it is and nothing else of the walk was done.
    carried: bool,
    pos: V3,
    fall: f32,
    grounded: bool,
    /// The horizontal velocity a steered capsule really had, when it was steered.
    vel: Option<[f32; 2]>,
    motion: Option<CharacterMotion>,
    contacts: Vec<Contact>,
}

/// Walk one capsule: its move, with `near` as scratch space.
fn walk_one(cx: &Walk<'_, '_>, ch: &(u64, Entity, Transform, CharacterBody), near: &mut Vec<usize>) -> Walked {
    let Walk { dt, gravity, terrains, strings, motions, steered, ladders, waters, sheets, layers, grid: base } = *cx;
    let (uid, entity, t, c) = *ch;
    let mut c = c;
    if let Some(w) = steered.get(&uid) {
        c.walk = *w;
    }
    let grid = View { grid: base, who: layers.of(uid) };
    let r = c.radius.abs().clamp(0.02, 50.0);
    let height = c.height.abs().max(2.0 * r);
    let step_h = c.step_height.clamp(0.0, height * 0.5);
    let min_up = c.max_slope.clamp(1.0, 89.0).to_radians().cos();
    let mut pos = t.pos;
    let mut fall = c.fall;
    let mut grounded = c.grounded;
    let (mut walk, mut jump, mut gravity_on) = (c.walk, c.jump, true);
    // ---- swimming, rolling, climbing and being pulled up a ledge decide what this tick's walk, jump and gravity are
    let mut motion = motions.get(&uid).copied();
    if let Some(m) = motion.as_mut() {
        let surface = crate::water::surface_any(waters, sheets, terrains, strings, pos[0], pos[2]).map(|s| s.0);
        let plan = {
            let env = motion::Env { grid: &grid, r, height, step_h, min_up, dt, surface, ladders: &ladders };
            motion::pre(m, &c, pos, grounded, fall, &env)
        };
        (walk, jump, gravity_on, fall) = (plan.walk, plan.jump, plan.gravity, plan.fall);
        if let Some(at) = plan.carried {
            return Walked { entity, uid, carried: true, pos: at, fall: 0.0, grounded: false, vel: None, motion: Some(*m), contacts: Vec::new() };
        }
    }
    // ---- vertical speed
    if grounded && fall < 0.0 {
        fall = 0.0;
    }
    if gravity_on {
        fall += gravity * c.gravity_scale * dt;
    }
    if jump > 0.0 && grounded {
        fall = jump;
        grounded = false;
    }
    // ---- horizontal move, plain and lifted over a step; keep the one that got further
    let mut stepped = false;
    let delta = [if walk[0].is_finite() { walk[0] * dt } else { 0.0 }, 0.0, if walk[1].is_finite() { walk[1] * dt } else { 0.0 }];
    if delta[0] != 0.0 || delta[2] != 0.0 {
        let mut plain = [pos[0] + delta[0], pos[1], pos[2] + delta[2]];
        depenetrate(&mut plain, r, height, &grid, &mut *near, min_up);
        // (how far along the wanted move a position got, in metres: a slow walk moves a few millimetres a tick, which a measure in square metres would lose in the tolerance)
        let wanted = (delta[0] * delta[0] + delta[2] * delta[2]).sqrt().max(1.0e-12);
        let got = |p: V3| ((p[0] - pos[0]) * delta[0] + (p[2] - pos[2]) * delta[2]) / wanted;
        let mut best = plain;
        if step_h > 0.0 && grounded && got(plain) < wanted * 0.95 {
            // lifted: up by the step height (if there is room), across, then down onto whatever is there
            let up = [pos[0], pos[1] + step_h, pos[2]];
            if !blocked(up, r, height, &grid, &mut *near) {
                let mut lifted = [up[0] + delta[0], up[1], up[2] + delta[2]];
                depenetrate(&mut lifted, r, height, &grid, &mut *near, min_up);
                // settle down onto the step: lower until it touches (a capsule that is not yet over the edge rests on the edge and
                // rolls up over it on the next ticks)
                if !blocked(lifted, r, height, &grid, &mut *near) {
                    let (down, _) = lower_to_contact(lifted, step_h, r, height, &grid, &mut *near);
                    // (a smooth slope of terrain steeper than `max_slope` is not a step)
                    if got(down) > got(plain) + EPS && down[1] >= pos[1] - EPS && !too_steep(down, r, height, &grid, min_up) {
                        best = down;
                        stepped = true;
                    }
                }
            }
        }
        pos = best;
    }
    // ---- vertical move
    let before_vertical = pos;
    pos[1] += fall * dt;
    // (a rise that would put the head into the corner of a ceiling: the smallest nudge sideways, towards where the character walks first, that clears it is taken, and the jump goes on)
    if fall > 0.0 {
        let mut probe = pos;
        let (_, bonk, _) = depenetrate_rim(&mut probe, r, height, &grid, &mut *near, min_up);
        if bonk {
            let w = (walk[0] * walk[0] + walk[1] * walk[1]).sqrt();
            let mut dirs = CORNER_DIRS;
            if w > 0.05 {
                let toward = |d: &[f32; 2]| d[0] * walk[0] / w + d[1] * walk[1] / w;
                dirs.sort_by(|a, b| toward(b).total_cmp(&toward(a)));
            }
            'nudge: for k in 1..=6 {
                let d = CORNER_NUDGE * k as f32 / 6.0;
                for dir in dirs {
                    let p = [pos[0] + dir[0] * d, pos[1], pos[2] + dir[1] * d];
                    // (a place where nothing pushes the head down and nothing pushes the capsule sideways)
                    let mut test = p;
                    let (_, ceil, _) = depenetrate_rim(&mut test, r, height, &grid, &mut *near, min_up);
                    if !ceil && (test[0] - p[0]).abs() < 1.0e-3 && (test[2] - p[2]).abs() < 1.0e-3 && (test[1] - p[1]).abs() < 1.0e-3 {
                        pos = p;
                        break 'nudge;
                    }
                }
            }
        }
    }
    let (floor, ceiling, rim) = depenetrate_rim(&mut pos, r, height, &grid, &mut *near, min_up);
    if floor && fall <= 0.0 {
        grounded = true;
        fall = 0.0;
    } else if floor {
        // pushed up by a slope while rising: not on the ground yet
        grounded = false;
    } else {
        grounded = false;
    }
    // (a capsule that stepped up and rests on the rim of the step, its round foot on the edge and not yet over it, is on the ground: it goes on over the edge by stepping again next tick, instead of falling back)
    if stepped && !grounded && fall <= 0.0 {
        grounded = true;
        fall = 0.0;
    }
    // (standing still on the rim of a step, the round foot on the edge and not over the tread: the push of the edge would roll the character back down the flight, a step at a time. It stays where it is: the edge pushes it
    // sideways and up, which no floor does, and nothing asked it to move)
    if (c.grounded || rim) && !floor && jump <= 0.0 && gravity_on && walk[0].abs() < 0.02 && walk[1].abs() < 0.02 && (pos[0] - before_vertical[0]).abs() + (pos[2] - before_vertical[2]).abs() > 1.0e-6 {
        pos = before_vertical;
        grounded = true;
        fall = 0.0;
    }
    if ceiling && fall > 0.0 {
        fall = 0.0;
    }
    // ---- stay on the ground going down stairs and slopes, and find the ground when standing still
    if !grounded && fall <= 0.0 && c.grounded && jump <= 0.0 && gravity_on && !blocked(pos, r, height, &grid, &mut *near) {
        let (down, touched) = lower_to_contact(pos, step_h.max(0.05), r, height, &grid, &mut *near);
        if touched {
            pos = down;
            grounded = true;
            fall = 0.0;
        }
    }
    if !grounded {
        // a tiny probe: resting exactly on the surface counts as grounded
        let mut probe = [pos[0], pos[1] - 0.02, pos[2]];
        let (f, _) = depenetrate(&mut probe, r, height, &grid, &mut *near, min_up);
        if f && fall <= 0.0 {
            grounded = true;
            fall = 0.0;
        }
    }
    let vel = (steered.contains_key(&uid) && dt > 0.0).then(|| [(pos[0] - t.pos[0]) / dt, (pos[2] - t.pos[2]) / dt]);
    let mut contacts = Vec::new();
    if let Some(m) = motion {
        if m.report_contacts {
            touching(uid, pos, r, height, &grid, near, &mut contacts);
        }
    }
    Walked { entity, uid, carried: false, pos, fall, grounded, vel, motion, contacts }
}

/// The solid things the capsule at `pos` touches (within a margin of 3 cm, and the floor under its feet): a contact for each, in the order of the obstacles.
fn touching(uid: u64, pos: V3, r: f32, height: f32, grid: &View<'_, '_>, near: &mut Vec<usize>, out: &mut Vec<Contact>) {
    const MARGIN: f32 = 0.03;
    let probe = [pos[0], pos[1] - 0.02, pos[2]];
    grid.near(probe[0], probe[2], r + MARGIN + EPS, near);
    for &i in near.iter() {
        let o = &grid.obstacles[i];
        if o.uid == uid {
            continue;
        }
        if let Some((n, depth)) = overlap(probe, r + MARGIN, height, o, &grid.placed, &grid.meshes) {
            if depth > 0.0 {
                out.push(Contact { a: uid, b: o.uid, normal: [-n[0], -n[1], -n[2]], depth: (depth - MARGIN).max(0.0), trigger: false });
            }
        }
    }
}
