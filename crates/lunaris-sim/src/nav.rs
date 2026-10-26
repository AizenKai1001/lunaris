//! Navigation: pathfinding over the solid colliders of the world, and agents that follow the paths. An AI mind (or a graph system, or a
//! person through a tool call) says *where* and the engine does the walking: `set NavAgent {target: [x, y, z], active: true}`.
//!
//! **How a path is found.** The ground plane is cut into square cells (0.5 m by default) over the area between the two points plus a
//! margin. Every solid, non-dynamic `Collider` (spheres and boxes, like the rest of the physics) contributes two things per cell it
//! covers: the *top of a box* is a surface an agent can stand on, and the whole collider, grown sideways by the agent's radius, is an
//! obstacle for the vertical span it occupies. A surface is walkable when no obstacle intersects the agent's height above it, so floors,
//! stairs, platforms and bridges all work, several levels per cell included. Neighbouring cells (eight directions, no cutting corners)
//! are connected when the height difference is a step the agent can climb (`step_height`) or a drop it accepts (`max_drop`). A* with the
//! octile distance finds the shortest path (ties are broken by node number, so the result never depends on anything but the world), and
//! a line-of-sight pass over the grid removes the waypoints that were only there because of the cells. The data of a cell is worked out the first time the search looks at it (a search of
//! a hundred nodes looks at a few hundred cells of a window that may have thirty thousand), from the solids, terrain and triangles over that cell, and is exactly what cutting the whole
//! window into cells would have given it: the paths are the same, bit for bit, as those of a grid cut whole.
//!
//! **Terrain.** A solid `Terrain` adds a surface to every cell it covers: the height of the ground at the cell's centre, unless the ground
//! there leans more than 50 degrees (the character controller's default `max_slope`), which is a cliff and not walkable. Gentle slopes are
//! walked because neighbouring cells differ by less than a step; steps, boxes and trees standing on the ground are obstacles as before.
//!
//! A start that is not standing on any box (a top-down game whose floor is only a picture) means one flat open floor at the start's
//! height, so the same call works there; obstacles still block.
//!
//! **How agents follow it.** `step` runs every tick after the graph systems and before the character controller: an agent with
//! `active` set gets a path (status 1 = walking, 3 = no path), then steers toward its next waypoint: a `CharacterBody` gets `walk` set,
//! an entity with a `Velocity` gets `lin.x/z` set, anything else is moved directly. At the end it stops (status 2 = arrived). An agent
//! that makes no progress for a second replans; after three tries it reports no path. Paths are part of the state (they are hashed, copied
//! into snapshots and rebuilt identically by a replay).
//!
//! **Meshes.** A `MeshCollider` (a level made of triangles) adds to each cell its triangles cover: a triangle that faces up within the
//! walkers' steepest slope (50 degrees, like the terrain and the character controller's default) is a surface to stand on, at the height
//! of the triangle over the cell's centre; every triangle (a wall, a ceiling, a steep slope, and a walkable one that is over somebody's
//! head) is an obstacle for the heights it occupies, grown sideways by the agent's radius like a box.
//!
//! Everything is `f32` with only `+ - * / sqrt` and integer costs for the search, in a fixed order: identical on every machine.
use crate::components::{CharacterBody, Collider, NavAgent, NavAvoid, NavLimits, NavLink, NavObstacle, RigidBody, Transform, Velocity};
use crate::layers::{Filter, Layers};
use crate::meshcol::{Aabb, Instance, MeshStore, V3};
use crate::terrain::{grounds, Ground, Placed, Terrains};
use lunaris_ecs::{StateHasher, World};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};
use std::rc::Rc;

/// The most cells one search grid may have (the cell size grows, up to 8 times, to fit a big area).
pub const MAX_CELLS: usize = 250_000;
/// The least `normal.y` of ground an agent walks on: the cosine of 50 degrees.
const TERRAIN_MIN_UP: f32 = 0.642_787_6;
const ORTHO: u32 = 1000;
const DIAG: u32 = 1414;

#[derive(Clone, Copy, Debug)]
pub struct NavParams {
    pub radius: f32,
    pub height: f32,
    pub step_height: f32,
    pub max_drop: f32,
    pub cell: f32,
    /// Keep the data of the cells between searches (see `KeptCells`): the window then starts on a lattice anchored at the world's origin, and the paths are those of that lattice. Off: the window starts at the corner of the box
    /// between the two points, as it always did, and every search works its cells out afresh.
    pub keep: bool,
}

impl Default for NavParams {
    fn default() -> NavParams {
        NavParams { radius: 0.4, height: 1.8, step_height: 0.4, max_drop: 1.2, cell: 0.5, keep: false }
    }
}

impl NavParams {
    fn sane(mut self) -> NavParams {
        self.radius = self.radius.abs().clamp(0.02, 20.0);
        self.height = self.height.abs().clamp(0.1, 100.0);
        self.step_height = self.step_height.abs().min(self.height);
        self.max_drop = self.max_drop.abs();
        self.cell = self.cell.abs().clamp(0.1, 10.0);
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NavError {
    /// The start is not on ground an agent can stand on (inside a wall, off the world).
    NoStart(String),
    /// The target is not near any walkable ground.
    NoGoal(String),
    /// Both ends are on walkable ground but nothing connects them.
    NoPath(String),
    /// The area is too big to search at this cell size.
    TooBig(String),
}

impl NavError {
    pub fn code(&self) -> &'static str {
        match self {
            NavError::NoStart(_) => "nav_no_start",
            NavError::NoGoal(_) => "nav_no_goal",
            NavError::NoPath(_) => "nav_no_path",
            NavError::TooBig(_) => "nav_too_big",
        }
    }
    pub fn message(&self) -> &str {
        match self {
            NavError::NoStart(m) | NavError::NoGoal(m) | NavError::NoPath(m) | NavError::TooBig(m) => m,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PathResult {
    /// For each point: `Some([speed, hop])` when it is reached from the one before by a `NavLink` (a jump), `None` when it is walked to.
    pub jumps: Vec<Option<[f32; 2]>>,
    pub points: Vec<[f32; 3]>,
    pub length: f32,
    /// False when the target itself is not walkable and the path ends at the nearest ground that is.
    pub exact: bool,
    /// Cells the search looked at.
    pub searched: usize,
    pub cell: f32,
}

/// A `NavLink` in world coordinates.
#[derive(Clone, Copy)]
struct LinkDef {
    from: [f32; 3],
    to: [f32; 3],
    two_way: bool,
    speed: f32,
    hop: f32,
}

fn links(world: &mut World) -> Vec<LinkDef> {
    let mut raw: Vec<(u64, Transform, NavLink)> = Vec::new();
    let _ = world.for_each_uid::<(&Transform, &NavLink), _>(|_, uid, (t, l)| raw.push((uid, *t, *l)));
    raw.sort_by_key(|r| r.0);
    raw.into_iter()
        .filter(|(_, _, l)| l.enabled)
        .map(|(_, t, l)| LinkDef { from: [t.pos[0] + l.from[0], t.pos[1] + l.from[1], t.pos[2] + l.from[2]], to: [t.pos[0] + l.to[0], t.pos[1] + l.to[1], t.pos[2] + l.to[2]], two_way: l.two_way, speed: l.speed.max(0.1), hop: l.hop.max(0.0) })
        .filter(|l| l.from.iter().chain(l.to.iter()).all(|v| v.is_finite()))
        .collect()
}

struct Solid {
    lo: [f32; 3],
    hi: [f32; 3],
    /// A box can be stood on; a sphere only blocks.
    is_box: bool,
}

/// The solid, non-moving colliders that agents walk on and around. `who` is the collision layer of the agent that plans (`None`: every layer counts):
/// what it does not collide with is not in its way and not under its feet.
fn solids(world: &mut World, meshes: &MeshStore, who: Option<Filter>) -> Vec<Solid> {
    let layers = Layers::gather(world);
    let who = who.filter(|_| !layers.is_empty());
    let counts = |uid: u64| who.is_none_or(|w| w.collides(layers.of(uid)));
    let mut skip: BTreeSet<u64> = BTreeSet::new();
    let _ = world.for_each_uid::<(&CharacterBody,), _>(|_, uid, _| {
        skip.insert(uid);
    });
    let _ = world.for_each_uid::<(&NavAgent,), _>(|_, uid, _| {
        skip.insert(uid);
    });
    let _ = world.for_each_uid::<(&RigidBody, &Velocity), _>(|_, uid, (rb, _)| {
        if rb.mass > 0.0 {
            skip.insert(uid); // a dynamic body moves: it is not part of the map
        }
    });
    // a moving thing that says it is an obstacle counts, wherever it is now
    let _ = world.for_each_uid::<(&NavObstacle,), _>(|_, uid, (o,)| {
        if o.on {
            skip.remove(&uid);
        }
    });
    let mut raw: Vec<(u64, Transform, Collider)> = Vec::new();
    let _ = world.for_each_uid::<(&Transform, &Collider), _>(|_, uid, (t, c)| raw.push((uid, *t, *c)));
    raw.sort_by_key(|r| r.0);
    let mut out = Vec::with_capacity(raw.len());
    for (uid, t, c) in raw {
        if c.trigger || skip.contains(&uid) || !counts(uid) {
            continue;
        }
        let s = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
        let h = if c.shape == 0 {
            let r = 0.5 * c.size[0].abs() * s[0].max(s[1]).max(s[2]);
            [r, r, r]
        } else {
            [0.5 * c.size[0].abs() * s[0], 0.5 * c.size[1].abs() * s[1], 0.5 * c.size[2].abs() * s[2]]
        };
        out.push(Solid { lo: [t.pos[0] - h[0], t.pos[1] - h[1], t.pos[2] - h[2]], hi: [t.pos[0] + h[0], t.pos[1] + h[1], t.pos[2] + h[2]], is_box: c.shape != 0 });
    }
    // a convex hull that stays where it is (a rock) is, for the planner, the box around it
    for h in meshes.hull_instances(world) {
        if h.trigger || skip.contains(&h.uid) || !counts(h.uid) {
            continue;
        }
        let b = h.bounds();
        out.push(Solid { lo: b.lo, hi: b.hi, is_box: true });
    }
    out
}

fn tri_cross(a: V3, b: V3, c: V3) -> V3 {
    let (e1, e2) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]]
}

/// The lowest and the highest height of the part of a triangle that is over the square of half-size `r` around (`cx`, `cz`), `None` when
/// the triangle does not reach the square.
fn clip_height(t: &[V3; 3], cx: f32, cz: f32, r: f32) -> Option<(f32, f32)> {
    let mut poly = [[0.0f32; 3]; 8];
    poly[..3].copy_from_slice(t);
    let mut n = 3usize;
    for (axis, bound, sign) in [(0usize, cx - r, 1.0f32), (0, cx + r, -1.0), (2, cz - r, 1.0), (2, cz + r, -1.0)] {
        let mut out = [[0.0f32; 3]; 8];
        let mut m = 0usize;
        for i in 0..n {
            let (p, q) = (poly[i], poly[(i + 1) % n]);
            let (dp, dq) = (sign * (p[axis] - bound), sign * (q[axis] - bound));
            if dp >= 0.0 {
                out[m] = p;
                m += 1;
            }
            if (dp >= 0.0) != (dq >= 0.0) {
                let s = dp / (dp - dq);
                out[m] = [p[0] + (q[0] - p[0]) * s, p[1] + (q[1] - p[1]) * s, p[2] + (q[2] - p[2]) * s];
                m += 1;
            }
        }
        if m == 0 {
            return None;
        }
        poly = out;
        n = m;
    }
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for p in &poly[..n] {
        lo = lo.min(p[1]);
        hi = hi.max(p[1]);
    }
    Some((lo, hi))
}

/// How many surfaces of one cell have a node number: a node is `cell * NODES_PER_CELL + k`, `k` counting from the lowest surface of the cell (a cell with more surfaces than this, a
/// tower of floors, has the highest ignored). The number only has to keep the *order* of the nodes (cell first, then height): the search breaks ties by node number, so the paths
/// are the ones a grid numbered 0, 1, 2 ... in that order would give.
const NODES_PER_CELL: usize = 32;

/// The side, in cells, of a block of the coarse index of a grid's solids (see `Grid::buckets`).
const BUCKET: usize = 8;

/// A triangle of a mesh collider as a cell wants it: the corners, the cells its box (grown by the agent's radius) reaches in the window, whether it can be walked on, and the denominator of the weights of a point over it seen from above.
struct TriRec {
    tri: [V3; 3],
    ixa: usize,
    ixb: usize,
    iza: usize,
    izb: usize,
    walkable: bool,
    den: f32,
}

/// What one cell holds: the heights of the surfaces an agent can stand on, the lowest first, and whether something solid stands in it.
struct CellData {
    heights: Vec<f32>,
    occupied: bool,
}

/// The cells that the searches of this thread have worked out, kept for the next ones: the data of a cell is a function of the world's solids, ground and mesh levels and of the agent's size, not of the window a search
/// happens to cut, and a city server runs sixty searches in the time a door swings, most of them over the same streets (the cells are most of what a search costs: 3 microseconds each, a few thousand a search). The world is
/// recognised by a hash of its solids, grounds and levels (`world_signature`); when it differs from the hash the cells were made for, they are dropped. Windows are cut on a lattice of the cell size, so that the cell
/// (i, j) of one search is the cell (i, j) of the next.
struct KeptCells {
    sig: u64,
    /// The cell size and the agent's radius, height and step (the numbers a cell's data depends on), as bits.
    key: [u32; 4],
    cells: IdMap<u64, Rc<CellData>>,
}

thread_local! {
    /// One set of cells for each size of agent (a few: a game has a handful of kinds), the oldest dropped.
    static KEPT: std::cell::RefCell<Vec<KeptCells>> = const { std::cell::RefCell::new(Vec::new()) };
}

thread_local! {
    /// Off while a test compares the kept cells with fresh ones.
    static KEEP_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Switch the keeping of cells between searches on or off (and drop what is kept): for the test that makes sure the paths are the same either way.
#[cfg(test)]
pub(crate) fn keep_cells(on: bool) {
    KEEP_OFF.with(|k| k.set(!on));
    KEPT.with(|k| k.borrow_mut().clear());
}

/// How many cells are kept now, over all sizes of agent (for tests).
#[cfg(test)]
pub(crate) fn kept_cells() -> usize {
    KEPT.with(|k| k.borrow().iter().map(|e| e.cells.len()).sum())
}

/// The most cells kept for one size of agent (a city of a few kilometres at half a metre has a few million; this is the working set of the streets that are walked).
const MAX_KEPT: usize = 600_000;
/// The most sizes of agent that have cells kept.
const MAX_KEPT_SETS: usize = 4;

/// A hash of what the cells' data is made of.
fn world_signature(all: &[Solid], ground: &[Ground], level: &[Instance]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |v: u64| {
        h = (h ^ v).wrapping_mul(0x0000_0100_0000_01b3);
        h ^= h >> 29;
    };
    eat(all.len() as u64);
    for s in all {
        for v in s.lo.iter().chain(s.hi.iter()) {
            eat(v.to_bits() as u64);
        }
        eat(s.is_box as u64);
    }
    eat(ground.len() as u64);
    for g in ground {
        let p = g.placed();
        let (lo, hi) = p.y_range();
        eat(g.uid);
        for v in [p.centre[0], p.centre[1], p.size[0], p.size[1], lo, hi] {
            eat(v.to_bits() as u64);
        }
    }
    eat(level.len() as u64);
    for m in level {
        eat(m.uid);
        for v in m.placed.bounds.lo.iter().chain(m.placed.bounds.hi.iter()) {
            eat(v.to_bits() as u64);
        }
        eat(m.placed.verts.len() as u64);
        eat(std::sync::Arc::as_ptr(&m.placed) as usize as u64);
    }
    h
}

/// A hasher for the maps of cells and nodes, which are never walked in order: the keys are small whole numbers, mixed by one multiplication.
#[derive(Default)]
struct IdHasher(u64);

impl std::hash::Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn write_u32(&mut self, i: u32) {
        self.0 = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    fn write_usize(&mut self, i: usize) {
        self.0 = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type IdMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<IdHasher>>;

/// The inclusive range of cells (along one axis of a grid with `n` cells starting at `origin`) that `lo..hi` reaches.
fn cell_range(lo: f32, hi: f32, origin: f32, cell: f32, n: usize) -> (usize, usize) {
    let a = (((lo - origin) / cell).floor() as i64).clamp(0, n as i64 - 1) as usize;
    let b = (((hi - origin) / cell).floor() as i64).clamp(0, n as i64 - 1) as usize;
    (a, b)
}

/// The search grid: a window of square cells over the ground, each with the heights of the surfaces an agent can stand on. **The data of a cell is worked out the first time the
/// search looks at it**, from the solids, the terrain and the mesh triangles over it: a search that visits a hundred nodes looks at a few hundred cells of a window that may have thirty
/// thousand, and cutting every triangle of the whole window into the cells it covers (what a grid used to be built from) was most of the cost of a search in a city. A cell's data is
/// exactly what the whole-window cut gave it, so paths and every number that depends on them are unchanged.
struct Grid<'a> {
    cell: f32,
    x0: f32,
    z0: f32,
    nx: usize,
    nz: usize,
    /// The solids that reach the window, each with the inclusive range of cells it reaches: (solid, ixa, ixb, iza, izb).
    solids: Vec<(&'a Solid, usize, usize, usize, usize)>,
    /// A coarse index of `solids` for the cells that are worked out (built on the first one): the indices of the solids that reach each block of `BUCKET` x `BUCKET` cells, in the order of `solids`, so that
    /// a cell looks at the few solids over its block instead of every solid of the window (a city window has hundreds, and looking at them all was most of the microseconds a cell cost).
    buckets: std::cell::OnceCell<Vec<Vec<u32>>>,
    /// The mesh triangles that reach each block of `BUCKET` x `BUCKET` cells, worked out the first time a cell of the block is (the tree of every mesh is asked once for the block, not once for each of its 64 cells, and the corner
    /// arithmetic of a triangle is done once): 26 meshes were asked, and 60 triangles tested, for every cell of a city window.
    tri_blocks: std::cell::RefCell<IdMap<usize, Rc<Vec<TriRec>>>>,
    /// The same for terrains.
    ground: Vec<(Placed<'a>, usize, usize, usize, usize)>,
    meshes: &'a [Instance],
    bounds: [f32; 4],
    /// The height of one flat floor under everything, when the start is not standing on any ground (see `build`).
    flat: Option<f32>,
    cells: IdMap<usize, Rc<CellData>>,
    /// The cell (0, 0) of the window is the cell (`ix0`, `iz0`) of the lattice of this size over the world.
    ix0: i64,
    iz0: i64,
    /// The hash of the world, which is what the kept cells are for, and whether this grid may use them (not with a flat floor made up for a start that stands on nothing).
    sig: u64,
    kept: bool,
    /// The `NavLink` edges: from node, to node, cost, speed and hop of the link.
    link_edges: BTreeMap<usize, Vec<(usize, u32, [f32; 2])>>,
    p: NavParams,
}

/// What a grid works out lazily: the buckets of solids, the triangles of mesh blocks and the cells' data. A search done a few cells at a time (`NavJob`) keeps them from one step to the next, so that a step does not
/// work out again the cells the last one looked at. They are derived data and not part of the simulation's state (they hold `Rc`s, which the state may not: a copy of it goes to another thread to be saved): they are kept
/// on the thread in `JOB_CACHES`, by the serial number of the job, and a copy of a job gets a number of its own and so starts without them.
#[derive(Default)]
struct GridCaches {
    buckets: std::cell::OnceCell<Vec<Vec<u32>>>,
    tri_blocks: IdMap<usize, Rc<Vec<TriRec>>>,
    cells: IdMap<usize, Rc<CellData>>,
}

thread_local! {
    static JOB_CACHES: std::cell::RefCell<BTreeMap<u64, GridCaches>> = const { std::cell::RefCell::new(BTreeMap::new()) };
}

/// The most jobs whose caches are kept on a thread (more means a few simulations share it, which is not worth keeping them for).
const MAX_JOB_CACHES: usize = 8;

fn next_job_serial() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl<'a> Grid<'a> {
    /// Take the lazily worked out data out of the grid (`restore` puts it into a grid made from the same world and window).
    fn save(&mut self) -> GridCaches {
        GridCaches { buckets: std::mem::take(&mut self.buckets), tri_blocks: std::mem::take(self.tri_blocks.get_mut()), cells: std::mem::take(&mut self.cells) }
    }

    fn restore(&mut self, c: GridCaches) {
        self.buckets = c.buckets;
        *self.tri_blocks.get_mut() = c.tri_blocks;
        self.cells = c.cells;
    }

    #[allow(clippy::too_many_arguments)]
    fn build(solids: &'a [Solid], ground: &'a [Ground], meshes: &'a [Instance], bounds: [f32; 4], p: NavParams, start_at: [f32; 3], sig: u64) -> Result<Grid<'a>, NavError> {
        let [bx0, bz0, bx1, bz1] = bounds;
        let mut cell = p.cell;
        // (with `keep` the window starts on a line of the lattice of cells, wherever the two points are: see `KeptCells`; without it, at the corner of the box)
        let (nx, nz, ix0, iz0, x0, z0) = loop {
            let (ix0, iz0) = if p.keep { ((bx0 / cell).floor() as i64, (bz0 / cell).floor() as i64) } else { (0, 0) };
            let (ox, oz) = if p.keep { (ix0 as f32 * cell, iz0 as f32 * cell) } else { (bx0, bz0) };
            let nx = (((bx1 - ox) / cell).ceil() as usize).max(1);
            let nz = (((bz1 - oz) / cell).ceil() as usize).max(1);
            if nx.saturating_mul(nz) <= MAX_CELLS {
                break (nx, nz, ix0, iz0, ox, oz);
            }
            cell *= 2.0;
            if cell > p.cell * 8.0 {
                return Err(NavError::TooBig(format!("the area between the two points is {:.0} m by {:.0} m, too big to search ({} cells at the largest cell size, {:.1} m)", bx1 - bx0, bz1 - bz0, nx.saturating_mul(nz), cell)));
            }
        };
        let mut reaching = Vec::new();
        for s in solids {
            // outside the grid altogether
            if s.hi[0] + p.radius < bx0 || s.lo[0] - p.radius > bx1 || s.hi[2] + p.radius < bz0 || s.lo[2] - p.radius > bz1 {
                continue;
            }
            let (ixa, ixb) = cell_range(s.lo[0] - p.radius, s.hi[0] + p.radius, x0, cell, nx);
            let (iza, izb) = cell_range(s.lo[2] - p.radius, s.hi[2] + p.radius, z0, cell, nz);
            reaching.push((s, ixa, ixb, iza, izb));
        }
        let mut grounds = Vec::new();
        for g in ground {
            let placed = g.placed();
            let half = [placed.size[0] * 0.5, placed.size[1] * 0.5];
            if placed.centre[0] + half[0] < bx0 || placed.centre[0] - half[0] > bx1 || placed.centre[1] + half[1] < bz0 || placed.centre[1] - half[1] > bz1 {
                continue;
            }
            let (ixa, ixb) = cell_range(placed.centre[0] - half[0], placed.centre[0] + half[0], x0, cell, nx);
            let (iza, izb) = cell_range(placed.centre[1] - half[1], placed.centre[1] + half[1], z0, cell, nz);
            grounds.push((placed, ixa, ixb, iza, izb));
        }
        let mut grid = Grid { cell, x0, z0, nx, nz, solids: reaching, buckets: std::cell::OnceCell::new(), tri_blocks: std::cell::RefCell::new(IdMap::default()), ground: grounds, meshes, bounds, flat: None, cells: IdMap::default(), ix0, iz0, sig, kept: false, link_edges: BTreeMap::new(), p };
        // a start that is not standing on anything (a top-down game where the floor is only a picture) means one flat, open floor at the
        // start's height: obstacles still block it
        let standing = ((start_at[0] - x0) / cell >= 0.0 && (start_at[2] - z0) / cell >= 0.0)
            .then(|| (((start_at[2] - z0) / cell) as usize, ((start_at[0] - x0) / cell) as usize))
            .filter(|&(iz, ix)| iz < nz && ix < nx)
            .is_some_and(|(iz, ix)| grid.raw_cell(iz * nx + ix).0.iter().any(|t| (t - start_at[1]).abs() <= 2.0 + p.step_height));
        if !standing {
            grid.flat = Some(start_at[1]);
        }
        // (the kept cells are for this size of agent and this world: another world's are dropped; a flat floor made up for this start is no part of the world)
        if p.keep && grid.flat.is_none() && !KEEP_OFF.with(|k| k.get()) {
            grid.kept = true;
            let key = [cell.to_bits(), p.radius.to_bits(), p.height.to_bits(), p.step_height.to_bits()];
            KEPT.with(|k| {
                let mut k = k.borrow_mut();
                match k.iter().position(|e| e.key == key) {
                    Some(i) => {
                        if k[i].sig != sig {
                            k[i].sig = sig;
                            k[i].cells.clear();
                        }
                    }
                    None => {
                        if k.len() >= MAX_KEPT_SETS {
                            k.remove(0);
                        }
                        k.push(KeptCells { sig, key, cells: IdMap::default() });
                    }
                }
            });
        }
        Ok(grid)
    }

    /// What lies over cell `c`, before it is turned into surfaces: the tops that can be stood on, the part of solids right over the cell's centre (`exact`) and the part of solids
    /// whose growth by the agent's radius reaches it (`margin`: only one that rises higher than a step keeps an agent out).
    fn raw_cell(&self, c: usize) -> (Vec<f32>, Vec<(f32, f32)>, Vec<(f32, f32)>) {
        let p = self.p;
        let (ix, iz) = (c % self.nx, c / self.nx);
        let (cx, cz) = (self.x0 + (ix as f32 + 0.5) * self.cell, self.z0 + (iz as f32 + 0.5) * self.cell);
        let (mut tops, mut exact, mut margin) = (Vec::new(), Vec::new(), Vec::new());
        let bnx = self.nx.div_ceil(BUCKET);
        let buckets = self.buckets.get_or_init(|| {
            let bnz = self.nz.div_ceil(BUCKET);
            let mut b: Vec<Vec<u32>> = vec![Vec::new(); bnx * bnz];
            for (k, &(_, ixa, ixb, iza, izb)) in self.solids.iter().enumerate() {
                for bz in iza / BUCKET..=izb / BUCKET {
                    for bx in ixa / BUCKET..=ixb / BUCKET {
                        b[bz * bnx + bx].push(k as u32);
                    }
                }
            }
            b
        });
        for &k in &buckets[(iz / BUCKET) * bnx + ix / BUCKET] {
            let (s, ixa, ixb, iza, izb) = self.solids[k as usize];
            if ix < ixa || ix > ixb || iz < iza || iz > izb {
                continue;
            }
            let inside = cx >= s.lo[0] && cx <= s.hi[0] && cz >= s.lo[2] && cz <= s.hi[2];
            if inside {
                exact.push((s.lo[1], s.hi[1]));
                if s.is_box {
                    tops.push(s.hi[1]);
                }
            } else if cx >= s.lo[0] - p.radius && cx <= s.hi[0] + p.radius && cz >= s.lo[2] - p.radius && cz <= s.hi[2] + p.radius {
                margin.push((s.lo[1], s.hi[1]));
            }
        }
        // the ground of terrains: the height under the cell's centre, where it is not a cliff
        for (placed, ixa, ixb, iza, izb) in &self.ground {
            if ix < *ixa || ix > *ixb || iz < *iza || iz > *izb {
                continue;
            }
            let Some(h) = placed.surface(cx, cz) else { continue };
            if placed.normal(cx, cz)[1] >= TERRAIN_MIN_UP {
                tops.push(h);
            }
        }
        // the triangles of mesh colliders: walkable ones give surfaces, all of them give obstacles (see the module comment). A triangle belongs to this cell when the cell is in the
        // range its box, grown by the radius, reaches; the triangles of the cell's block (the tree is asked for the block, a little more than needed) are tested one by one, and that exact test decides
        if !self.meshes.is_empty() {
            let tris = self.mesh_block((iz / BUCKET) * bnx + ix / BUCKET, ix / BUCKET, iz / BUCKET);
            for t in tris.iter() {
                if ix < t.ixa || ix > t.ixb || iz < t.iza || iz > t.izb {
                    continue;
                }
                let [a, b, c3] = t.tri;
                // weights of a point over the triangle seen from above: positive inside; a triangle seen edge-on has no inside
                let den = t.den;
                if den != 0.0 {
                    let wa = ((b[2] - c3[2]) * (cx - c3[0]) + (c3[0] - b[0]) * (cz - c3[2])) / den;
                    let wb = ((c3[2] - a[2]) * (cx - c3[0]) + (a[0] - c3[0]) * (cz - c3[2])) / den;
                    let wc = 1.0 - wa - wb;
                    if wa >= 0.0 && wb >= 0.0 && wc >= 0.0 {
                        let y = wa * a[1] + wb * b[1] + wc * c3[1];
                        exact.push((y, y));
                        if t.walkable {
                            tops.push(y);
                        }
                        continue;
                    }
                }
                // (a floor that can be walked on is not an obstacle beside the cells it covers, like the ground of a terrain)
                if !t.walkable {
                    if let Some((y0, y1)) = clip_height(&t.tri, cx, cz, p.radius) {
                        margin.push((y0, y1));
                    }
                }
            }
        }
        (tops, exact, margin)
    }

    /// The mesh triangles that can matter to a cell of block (`bx`, `bz`), in the order the cells used to find them (the meshes in order, each one's triangles ascending), kept for the block.
    /// A triangle is in the list when a leaf box of its mesh's tree touches the block's region (every cell's own region lies inside it) and the cells its box reaches overlap the block: the cell's test on the range then
    /// decides exactly as before, because a triangle whose box reaches a cell has a leaf box that touches that cell's region.
    fn mesh_block(&self, key: usize, bx: usize, bz: usize) -> Rc<Vec<TriRec>> {
        if let Some(v) = self.tri_blocks.borrow().get(&key) {
            return v.clone();
        }
        let p = self.p;
        let [wx0, wz0, wx1, wz1] = self.bounds;
        let (ca, cb) = (bx * BUCKET, ((bx + 1) * BUCKET).min(self.nx) - 1);
        let (da, db) = (bz * BUCKET, ((bz + 1) * BUCKET).min(self.nz) - 1);
        let slack = 0.5 * self.cell;
        let reach = 0.5 * self.cell + p.radius + slack;
        let region = Aabb {
            lo: [self.x0 + (ca as f32 + 0.5) * self.cell - reach, f32::NEG_INFINITY, self.z0 + (da as f32 + 0.5) * self.cell - reach],
            hi: [self.x0 + (cb as f32 + 0.5) * self.cell + reach, f32::INFINITY, self.z0 + (db as f32 + 0.5) * self.cell + reach],
        };
        let mut out = Vec::new();
        for m in self.meshes {
            let pm = &m.placed;
            if pm.bounds.hi[0] + p.radius < wx0 || pm.bounds.lo[0] - p.radius > wx1 || pm.bounds.hi[2] + p.radius < wz0 || pm.bounds.lo[2] - p.radius > wz1 {
                continue;
            }
            if pm.bounds.hi[0] < region.lo[0] || pm.bounds.lo[0] > region.hi[0] || pm.bounds.hi[2] < region.lo[2] || pm.bounds.lo[2] > region.hi[2] {
                continue;
            }
            for ti in pm.triangles_near(&region) {
                let tri = pm.tri(ti);
                let [a, b, c3] = tri;
                let (tx0, tx1) = (a[0].min(b[0]).min(c3[0]), a[0].max(b[0]).max(c3[0]));
                let (tz0, tz1) = (a[2].min(b[2]).min(c3[2]), a[2].max(b[2]).max(c3[2]));
                if tx1 + p.radius < wx0 || tx0 - p.radius > wx1 || tz1 + p.radius < wz0 || tz0 - p.radius > wz1 {
                    continue;
                }
                let (ixa, ixb) = cell_range(tx0 - p.radius, tx1 + p.radius, self.x0, self.cell, self.nx);
                let (iza, izb) = cell_range(tz0 - p.radius, tz1 + p.radius, self.z0, self.cell, self.nz);
                if ixb < ca || ixa > cb || izb < da || iza > db {
                    continue;
                }
                let n = tri_cross(a, b, c3);
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                let walkable = len > 0.0 && n[1] / len >= TERRAIN_MIN_UP;
                let den = (b[2] - c3[2]) * (a[0] - c3[0]) + (c3[0] - b[0]) * (a[2] - c3[2]);
                out.push(TriRec { tri, ixa, ixb, iza, izb, walkable, den });
            }
        }
        let v = Rc::new(out);
        self.tri_blocks.borrow_mut().insert(key, v.clone());
        v
    }

    /// The surfaces of cell `c` (worked out on the first look, then kept).
    fn data(&mut self, c: usize) -> Rc<CellData> {
        if let Some(d) = self.cells.get(&c) {
            return d.clone();
        }
        // (the cell of the lattice that this is, and whether an earlier search has worked it out)
        let lattice = (((self.ix0 + (c % self.nx) as i64) as i32 as u32 as u64) << 32) | ((self.iz0 + (c / self.nx) as i64) as i32 as u32 as u64);
        let key = [self.cell.to_bits(), self.p.radius.to_bits(), self.p.height.to_bits(), self.p.step_height.to_bits()];
        if self.kept {
            let found = KEPT.with(|k| k.borrow().iter().find(|e| e.key == key && e.sig == self.sig).and_then(|e| e.cells.get(&lattice).cloned()));
            if let Some(d) = found {
                self.cells.insert(c, d.clone());
                return d;
            }
        }
        let p = self.p;
        let (mut tops, exact, margin) = self.raw_cell(c);
        if let Some(y) = self.flat {
            if !tops.iter().any(|v| (v - y).abs() < 0.01) {
                tops.push(y);
            }
        }
        tops.sort_by(|a, b| a.total_cmp(b));
        tops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        let lowest = tops.first().copied().unwrap_or(f32::NEG_INFINITY);
        let occupied = exact.iter().any(|&(_, b1)| b1 > lowest + p.step_height + 0.04);
        let mut heights = Vec::new();
        for &top in &tops {
            // standable: nothing but the surface itself intersects the agent's height above it
            let free = !exact.iter().any(|&(b0, b1)| b0 < top + p.height && b1 > top + 0.04) && !margin.iter().any(|&(b0, b1)| b0 < top + p.height && b1 > top + p.step_height + 0.04);
            if free && heights.len() < NODES_PER_CELL {
                heights.push(top);
            }
        }
        let d = Rc::new(CellData { heights, occupied });
        self.cells.insert(c, d.clone());
        if self.kept {
            KEPT.with(|k| {
                if let Some(e) = k.borrow_mut().iter_mut().find(|e| e.key == key && e.sig == self.sig) {
                    if e.cells.len() >= MAX_KEPT {
                        e.cells.clear();
                    }
                    e.cells.insert(lattice, d.clone());
                }
            });
        }
        d
    }

    fn node_cell(node: usize) -> usize {
        node / NODES_PER_CELL
    }

    fn height_of(&mut self, node: usize) -> f32 {
        self.data(node / NODES_PER_CELL).heights[node % NODES_PER_CELL]
    }

    /// Add the edges of `NavLink`s whose ends are both on ground of this grid (within about one and a half metres of the link's end).
    fn add_links(&mut self, defs: &[LinkDef]) {
        fn reach(g: &mut Grid, p: [f32; 3]) -> Option<usize> {
            let (n, _) = g.locate(p[0], p[2], p[1])?;
            let c = g.centre(n);
            ((c[0] - p[0]).powi(2) + (c[2] - p[2]).powi(2) <= 2.25).then_some(n)
        }
        for d in defs {
            let (Some(a), Some(b)) = (reach(self, d.from), reach(self, d.to)) else { continue };
            if a == b {
                continue;
            }
            let len = ((d.to[0] - d.from[0]).powi(2) + (d.to[1] - d.from[1]).powi(2) + (d.to[2] - d.from[2]).powi(2)).sqrt();
            let cost = ((len / self.cell) * ORTHO as f32) as u32 + ORTHO;
            self.link_edges.entry(a).or_default().push((b, cost, [d.speed, d.hop]));
            if d.two_way {
                self.link_edges.entry(b).or_default().push((a, cost, [d.speed, d.hop]));
            }
        }
    }

    fn centre(&mut self, node: usize) -> [f32; 3] {
        let c = node / NODES_PER_CELL;
        [self.x0 + ((c % self.nx) as f32 + 0.5) * self.cell, self.height_of(node), self.z0 + ((c / self.nx) as f32 + 0.5) * self.cell]
    }

    /// Can an agent standing at height `from` step to a surface at height `to`?
    fn connects(&self, from: f32, to: f32) -> bool {
        to - from <= self.p.step_height + 1.0e-3 && from - to <= self.p.max_drop + 1.0e-3
    }

    /// The surface of cell `c` that an agent at height `y` can reach, the nearest in height.
    fn reach(&mut self, c: usize, y: f32) -> Option<usize> {
        let d = self.data(c);
        (0..d.heights.len()).filter(|&k| self.connects(y, d.heights[k])).min_by(|&a, &b| (d.heights[a] - y).abs().total_cmp(&(d.heights[b] - y).abs())).map(|k| c * NODES_PER_CELL + k)
    }

    fn cell_at(&self, x: f32, z: f32) -> Option<usize> {
        let (fx, fz) = ((x - self.x0) / self.cell, (z - self.z0) / self.cell);
        if fx < 0.0 || fz < 0.0 {
            return None;
        }
        let (ix, iz) = (fx as usize, fz as usize);
        (ix < self.nx && iz < self.nz).then_some(iz * self.nx + ix)
    }

    /// The walkable node nearest to a point (searching up to four cells around it), and whether it is in the point's own cell.
    fn locate(&mut self, x: f32, z: f32, y: f32) -> Option<(usize, bool)> {
        let (cx, cz) = (((x - self.x0) / self.cell).floor() as i64, ((z - self.z0) / self.cell).floor() as i64);
        let mut best: Option<(f32, f32, usize, bool)> = None;
        for ring in 0..=4i64 {
            for iz in cz - ring..=cz + ring {
                for ix in cx - ring..=cx + ring {
                    if (ix - cx).abs().max((iz - cz).abs()) != ring || ix < 0 || iz < 0 || ix >= self.nx as i64 || iz >= self.nz as i64 {
                        continue;
                    }
                    let c = iz as usize * self.nx + ix as usize;
                    let (px, pz) = (self.x0 + (ix as f32 + 0.5) * self.cell - x, self.z0 + (iz as f32 + 0.5) * self.cell - z);
                    let d2 = px * px + pz * pz;
                    let data = self.data(c);
                    for (k, &height) in data.heights.iter().enumerate() {
                        let dy = (height - y).abs();
                        if dy > 2.0 + self.p.step_height {
                            continue; // a roof or a ledge far above or below the point is not the ground it means
                        }
                        // closer in the plane first; at the same distance the surface nearest in height
                        let better = best.is_none_or(|(bd, bh, _, _)| d2 < bd - 1.0e-6 || ((d2 - bd).abs() <= 1.0e-6 && dy < bh));
                        if better {
                            best = Some((d2, dy, c * NODES_PER_CELL + k, ring == 0));
                        }
                    }
                }
            }
            if best.is_some() && ring >= 1 {
                break;
            }
        }
        best.map(|b| (b.2, b.3))
    }

    /// The neighbours of a node: (node, cost), eight directions, never cutting a corner.
    fn neighbours(&mut self, node: usize, out: &mut Vec<(usize, u32)>) {
        out.clear();
        let c = node / NODES_PER_CELL;
        let (ix, iz) = ((c % self.nx) as i64, (c / self.nx) as i64);
        let h = self.height_of(node);
        for dz in -1i64..=1 {
            for dx in -1i64..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let (jx, jz) = (ix + dx, iz + dz);
                if jx < 0 || jz < 0 || jx >= self.nx as i64 || jz >= self.nz as i64 {
                    continue;
                }
                let diagonal = dx != 0 && dz != 0;
                if diagonal {
                    // both cells beside the corner must be walkable from here
                    let side_a = (iz * self.nx as i64 + jx) as usize;
                    let side_b = (jz * self.nx as i64 + ix) as usize;
                    if self.reach(side_a, h).is_none() || self.reach(side_b, h).is_none() {
                        continue;
                    }
                }
                let cell = (jz * self.nx as i64 + jx) as usize;
                let data = self.data(cell);
                for (k, &height) in data.heights.iter().enumerate() {
                    if self.connects(h, height) {
                        let rise = ((height - h).abs() * 1000.0) as u32;
                        out.push((cell * NODES_PER_CELL + k, if diagonal { DIAG } else { ORTHO } + rise));
                    }
                }
            }
        }
        if let Some(v) = self.link_edges.get(&node) {
            out.extend(v.iter().map(|&(to, cost, _)| (to, cost)));
        }
        // stepping off a ledge: the cell beside a platform edge is too close to the platform's side for an agent to stand in, but one
        // can fall past it onto the ground two cells away (when the cell beside the edge is open air, not a wall)
        for (dx, dz) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
            let (ax, az, fx, fz) = (ix + dx, iz + dz, ix + 2 * dx, iz + 2 * dz);
            if fx < 0 || fz < 0 || fx >= self.nx as i64 || fz >= self.nz as i64 || ax < 0 || az < 0 || ax >= self.nx as i64 || az >= self.nz as i64 {
                continue;
            }
            let adjacent = (az * self.nx as i64 + ax) as usize;
            if self.data(adjacent).occupied || self.reach(adjacent, h).is_some() {
                continue;
            }
            let far = (fz * self.nx as i64 + fx) as usize;
            let data = self.data(far);
            for (k, &height) in data.heights.iter().enumerate() {
                let drop = h - height;
                if drop > self.p.step_height + 1.0e-3 && drop <= self.p.max_drop + 1.0e-3 {
                    out.push((far * NODES_PER_CELL + k, 2 * ORTHO + (drop * 1000.0) as u32));
                }
            }
        }
    }

    /// Can an agent walk the straight line from `a` to `b`, cell by cell, keeping to ground it can step between?
    fn line_ok(&mut self, a: [f32; 3], b: [f32; 3]) -> bool {
        let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
        let len = (dx * dx + dz * dz).sqrt();
        let n = ((len / (self.cell * 0.5)).ceil() as usize).max(1);
        let mut y = a[1];
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let Some(c) = self.cell_at(a[0] + dx * t, a[2] + dz * t) else { return false };
            match self.reach(c, y) {
                Some(node) => y = self.height_of(node),
                None => return false,
            }
        }
        (y - b[1]).abs() <= self.p.step_height + 0.05
    }
}

fn octile(ax: i64, az: i64, bx: i64, bz: i64) -> u32 {
    let (dx, dz) = ((ax - bx).unsigned_abs() as u32, (az - bz).unsigned_abs() as u32);
    ORTHO * dx.max(dz) + (DIAG - ORTHO) * dx.min(dz)
}

/// The shortest walkable path from `from` to `to` (feet positions), or why there is none.
pub fn find_path(world: &mut World, from: [f32; 3], to: [f32; 3], params: NavParams) -> Result<PathResult, NavError> {
    find_path_on(world, from, to, params, &Terrains::new())
}

/// [`find_path`] over a world with terrain: `fields` are the heightmaps that have been read (`Sim::heightfields`).
pub fn find_path_on(world: &mut World, from: [f32; 3], to: [f32; 3], params: NavParams, fields: &Terrains) -> Result<PathResult, NavError> {
    find_path_with(world, from, to, params, fields, &MeshStore::default())
}

/// [`find_path_on`] over a world that also has mesh colliders: `meshes` are the models that have been read (`Sim::meshes`).
pub fn find_path_with(world: &mut World, from: [f32; 3], to: [f32; 3], params: NavParams, fields: &Terrains, meshes: &MeshStore) -> Result<PathResult, NavError> {
    find_path_as(world, from, to, params, fields, meshes, None)
}

/// [`find_path_with`] for an agent on a collision layer (`who`: its `CollisionLayer`): what it does not collide with does not block it and cannot be stood on.
/// `None` plans as if every layer counted (what the `path` tool does for two bare points).
pub fn find_path_as(world: &mut World, from: [f32; 3], to: [f32; 3], params: NavParams, fields: &Terrains, meshes: &MeshStore, who: Option<Filter>) -> Result<PathResult, NavError> {
    find_path_capped(world, from, to, params, fields, meshes, who, 0)
}

/// [`find_path_as`] that gives up as "no path" after expanding `cap` cells (0: no limit): what `NavLimits.max_searched` asks of the agents' searches.
#[allow(clippy::too_many_arguments)]
pub fn find_path_capped(world: &mut World, from: [f32; 3], to: [f32; 3], params: NavParams, fields: &Terrains, meshes: &MeshStore, who: Option<Filter>, cap: usize) -> Result<PathResult, NavError> {
    let started = trace_on().then(std::time::Instant::now);
    let r = find_path_inner(world, from, to, params, fields, meshes, who, cap);
    if let Some(t0) = started {
        // (a diagnostic of the cost of a search, `LUNARIS_NAV_TRACE=1`: the time is only printed, it never enters the simulation)
        let (dx, dz) = (to[0] - from[0], to[2] - from[2]);
        let (kind, detail) = match &r {
            Ok(p) => ("ok", format!("cell {:.2} searched {}", p.cell, p.searched)),
            Err(e) => (e.code(), String::new()),
        };
        eprintln!("nav-trace: {:>8} us  {:>6.1} m  {kind} {detail}", t0.elapsed().as_micros(), (dx * dx + dz * dz).sqrt());
    }
    r
}

fn trace_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("LUNARIS_NAV_TRACE").is_some())
}

/// The widest span, in metres, of a world that is searched again when the first search found no way: the way round may lead outside the box round the two points in a small level (a maze whose only gap is at the other end), but in a streamed world
/// of hundreds of blocks the same second look is a grid over the whole city for every target that cannot be reached (a closed door, a player in a room), over a second on a server; there the answer is "no path".
const WIDE_LIMIT: f32 = 300.0;

/// `WIDE_LIMIT`, or what `LUNARIS_NAV_WIDE_LIMIT` says (metres; a huge number gives the old behaviour: a second look over the whole world, for comparing).
fn wide_limit() -> f32 {
    static LIMIT: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *LIMIT.get_or_init(|| std::env::var("LUNARIS_NAV_WIDE_LIMIT").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(WIDE_LIMIT))
}

/// What the cells of a search are worked out from: the solids, grounds, mesh levels and links of the world as the agent that plans sees them.
struct NavScene {
    all: Vec<Solid>,
    ground: Vec<Ground>,
    level: Vec<Instance>,
    link_defs: Vec<LinkDef>,
    sig: u64,
}

fn gather_scene(world: &mut World, fields: &Terrains, meshes: &MeshStore, who: Option<Filter>) -> NavScene {
    let all = solids(world, meshes, who);
    let layers = Layers::gather(world);
    let who = who.filter(|_| !layers.is_empty());
    let mut ground = grounds(world, fields);
    let mut level: Vec<Instance> = meshes.instances(world);
    if let Some(w) = who {
        ground.retain(|g| w.collides(layers.of(g.uid)));
        level.retain(|m| w.collides(layers.of(m.uid)));
    }
    let link_defs = links(world);
    let sig = world_signature(&all, &ground, &level);
    NavScene { all, ground, level, link_defs, sig }
}

/// The box a search between two points is made in: the two points and a margin round them.
fn near_box(from: [f32; 3], to: [f32; 3], p: &NavParams) -> [f32; 4] {
    let (dx, dz) = (to[0] - from[0], to[2] - from[2]);
    let dist = (dx * dx + dz * dz).sqrt();
    let margin = (0.75 * dist).max(8.0 + p.radius);
    [from[0].min(to[0]) - margin, from[2].min(to[2]) - margin, from[0].max(to[0]) + margin, from[2].max(to[2]) + margin]
}

/// The box over the whole map that is searched again when the search in `near` found no way (see `find_path_inner`), or `None` when that is not worth it: the map is wider than `wide_limit` or the box is no bigger than `near`.
fn wide_box(scene: &NavScene, from: [f32; 3], to: [f32; 3], p: &NavParams, near: [f32; 4]) -> Option<[f32; 4]> {
    let mut world_box = [from[0].min(to[0]), from[2].min(to[2]), from[0].max(to[0]), from[2].max(to[2])];
    for s in &scene.all {
        world_box = [world_box[0].min(s.lo[0]), world_box[1].min(s.lo[2]), world_box[2].max(s.hi[0]), world_box[3].max(s.hi[2])];
    }
    for g in &scene.ground {
        let placed = g.placed();
        world_box = [world_box[0].min(placed.centre[0] - placed.size[0] * 0.5), world_box[1].min(placed.centre[1] - placed.size[1] * 0.5), world_box[2].max(placed.centre[0] + placed.size[0] * 0.5), world_box[3].max(placed.centre[1] + placed.size[1] * 0.5)];
    }
    for m in &scene.level {
        let b = &m.placed.bounds;
        world_box = [world_box[0].min(b.lo[0]), world_box[1].min(b.lo[2]), world_box[2].max(b.hi[0]), world_box[3].max(b.hi[2])];
    }
    let m = 2.0 + p.radius;
    let wide = [world_box[0] - m, world_box[1] - m, world_box[2] + m, world_box[3] + m];
    let span = (wide[2] - wide[0]).max(wide[3] - wide[1]);
    (span <= wide_limit() && (wide[0] < near[0] || wide[1] < near[1] || wide[2] > near[2] || wide[3] > near[3])).then_some(wide)
}

fn find_path_inner(world: &mut World, from: [f32; 3], to: [f32; 3], params: NavParams, fields: &Terrains, meshes: &MeshStore, who: Option<Filter>, cap: usize) -> Result<PathResult, NavError> {
    let p = params.sane();
    let scene = gather_scene(world, fields, meshes, who);
    let near = near_box(from, to, &p);
    let first = search(&scene, near, from, to, p, cap);
    // the way round may lead far outside the box around the two points (a maze whose only gap is at the other end): when nothing
    // was found there, look again over the whole map (the cells grow coarser if the map is huge)
    if matches!(first, Err(NavError::NoPath(_))) {
        if let Some(wide) = wide_box(&scene, from, to, &p, near) {
            let again = search(&scene, wide, from, to, p, cap);
            if again.is_ok() {
                return again;
            }
        }
    }
    first
}

/// One search over the grid cut from `bounds`.
fn search(scene: &NavScene, bounds: [f32; 4], from: [f32; 3], to: [f32; 3], p: NavParams, cap: usize) -> Result<PathResult, NavError> {
    let mut grid = Grid::build(&scene.all, &scene.ground, &scene.level, bounds, p, from, scene.sig)?;
    grid.add_links(&scene.link_defs);
    let mut run = SearchRun::begin(&mut grid, from, to, &p)?;
    match run.advance(&mut grid, from, to, &p, usize::MAX, cap)? {
        true => run.finish(&mut grid, from, to),
        // (a search with no limit on the cells it expands goes on until it is over)
        false => unreachable!("a search with no limit on the cells it expands is paused"),
    }
}

/// A search that can be stopped and taken up again: A* over the nodes of a grid, whose state (the cheapest cost found to each node, where it came from, the nodes still to look at) does not borrow the grid, so that the grid can
/// be made again from the world and the search go on from where it was (`NavLimits`, the resource `nav.cells_per_step`).
#[derive(Clone)]
struct SearchRun {
    g: IdMap<u32, u32>,
    came: IdMap<u32, u32>,
    open: BinaryHeap<Reverse<(u32, u32, u32)>>,
    searched: usize,
    goal: usize,
    exact: bool,
    gx: i64,
    gz: i64,
}

impl SearchRun {
    /// The start and the goal found on the grid, and the first node to look at.
    fn begin(grid: &mut Grid, from: [f32; 3], to: [f32; 3], p: &NavParams) -> Result<SearchRun, NavError> {
        let (start, _) = grid.locate(from[0], from[2], from[1]).ok_or_else(|| NavError::NoStart(format!("the start ({:.1}, {:.1}, {:.1}) is not on ground an agent of radius {:.2} and height {:.2} can stand on (inside a wall, or off the map)", from[0], from[1], from[2], p.radius, p.height)))?;
        let (goal, exact) = grid.locate(to[0], to[2], to[1]).ok_or_else(|| NavError::NoGoal(format!("the target ({:.1}, {:.1}, {:.1}) is not near any ground an agent can stand on", to[0], to[1], to[2])))?;
        let goal_cell = Grid::node_cell(goal);
        let nx = grid.nx;
        let (gx, gz) = ((goal_cell % nx) as i64, (goal_cell / nx) as i64);
        // A*: the cheapest cost so far per node (only the nodes the search reaches have one), ties broken by (f, h, node number)
        let mut run = SearchRun { g: IdMap::default(), came: IdMap::default(), open: BinaryHeap::new(), searched: 0, goal, exact, gx, gz };
        let h = run.heur(nx, start);
        run.g.insert(start as u32, 0);
        run.open.push(Reverse((h, h, start as u32)));
        Ok(run)
    }

    fn heur(&self, nx: usize, n: usize) -> u32 {
        let c = Grid::node_cell(n);
        octile((c % nx) as i64, (c / nx) as i64, self.gx, self.gz)
    }

    /// Expand up to `budget` more nodes. `Ok(true)`: the goal is reached (`finish` makes the path); `Ok(false)`: the budget is spent and the search is not over; `Err`: it gave up after `cap` nodes (0: no limit) or has none left to look at.
    fn advance(&mut self, grid: &mut Grid, from: [f32; 3], to: [f32; 3], p: &NavParams, budget: usize, cap: usize) -> Result<bool, NavError> {
        let nx = grid.nx;
        let mut next = Vec::new();
        let mut spent = 0usize;
        loop {
            if spent >= budget {
                return Ok(false);
            }
            let Some(Reverse((_, _, node))) = self.open.pop() else { break };
            let node = node as usize;
            if node == self.goal {
                return Ok(true);
            }
            self.searched += 1;
            spent += 1;
            if cap > 0 && self.searched > cap {
                return Err(NavError::NoPath(format!("gave up after {cap} cells (the search is limited by `NavLimits.max_searched`)")));
            }
            let gn = self.g[&(node as u32)];
            grid.neighbours(node, &mut next);
            for &(m, cost) in &next {
                let ng = gn.saturating_add(cost);
                if ng < self.g.get(&(m as u32)).copied().unwrap_or(u32::MAX) {
                    self.g.insert(m as u32, ng);
                    self.came.insert(m as u32, node as u32);
                    let h = self.heur(nx, m);
                    self.open.push(Reverse((ng.saturating_add(h), h, m as u32)));
                }
            }
        }
        let searched = self.searched;
        Err(NavError::NoPath(format!("no walkable route from ({:.1}, {:.1}, {:.1}) to ({:.1}, {:.1}, {:.1}) (searched {searched} cells): walls, drops of more than {:.1} m or steps higher than {:.2} m are in the way", from[0], from[1], from[2], to[0], to[1], to[2], p.max_drop, p.step_height)))
    }

    /// The path of a search whose goal has been reached.
    fn finish(&mut self, grid: &mut Grid, from: [f32; 3], to: [f32; 3]) -> Result<PathResult, NavError> {
        let (goal, exact, searched) = (self.goal, self.exact, self.searched);
        let came = &self.came;
        let nx = grid.nx;
    let mut nodes = vec![goal];
    while let Some(&last) = nodes.last() {
        let prev = came.get(&(last as u32)).copied().unwrap_or(u32::MAX);
        if prev == u32::MAX {
            break;
        }
        nodes.push(prev as usize);
    }
    nodes.reverse();
    let mut pts: Vec<[f32; 3]> = nodes.iter().map(|&n| grid.centre(n)).collect();
    // the ends are the real positions, not the cell centres
    pts[0] = [from[0], pts[0][1], from[2]];
    if exact {
        let last = pts.len() - 1;
        pts[last] = [to[0], pts[last][1], to[2]];
    }
    // which steps are jumps over a `NavLink`
    let mut jump_at: Vec<Option<[f32; 2]>> = vec![None; pts.len()];
    for k in 1..nodes.len() {
        let link = grid.link_edges.get(&nodes[k - 1]).and_then(|v| v.iter().find(|e| e.0 == nodes[k]).map(|&(_, _, hs)| hs));
        if let Some(hs) = link {
            // a link is only used when walking from the one node to the next is not possible: the grid's own edges are tried by the search first
            let from_height = grid.height_of(nodes[k - 1]);
            let walkable = grid.reach(Grid::node_cell(nodes[k]), from_height).is_some() && {
                let (a, b) = (Grid::node_cell(nodes[k - 1]) as i64, Grid::node_cell(nodes[k]) as i64);
                let (ax, az, bx, bz) = (a % nx as i64, a / nx as i64, b % nx as i64, b / nx as i64);
                (ax - bx).abs() <= 1 && (az - bz).abs() <= 1
            };
            if !walkable {
                jump_at[k] = Some(hs);
            }
        }
    }
    // drop the waypoints that only the cells needed: from each point go as far ahead as the straight line stays on walkable ground (a jump is
    // always kept as the step it is)
    let mut out = vec![pts[0]];
    let mut jumps: Vec<Option<[f32; 2]>> = vec![None];
    let mut i = 0;
    while i + 1 < pts.len() {
        if jump_at[i + 1].is_some() {
            out.push(pts[i + 1]);
            jumps.push(jump_at[i + 1]);
            i += 1;
            continue;
        }
        let mut limit = (i + 64).min(pts.len() - 1);
        if let Some(k) = (i + 1..=limit).find(|&k| jump_at[k].is_some()) {
            limit = k - 1;
        }
        let mut j = limit;
        while j > i + 1 && !grid.line_ok(pts[i], pts[j]) {
            j -= 1;
        }
        out.push(pts[j]);
        jumps.push(None);
        i = j;
    }
    let length = out.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2) + (w[1][2] - w[0][2]).powi(2)).sqrt()).sum();
    Ok(PathResult { points: out, jumps, length, exact, searched, cell: grid.cell })
}
}

// ------------------------------------------------------------------------------------------------------------- searches done a few cells a step

/// A search of an agent that is done a few cells a step (`NavLimits` with the resource `nav.cells_per_step`): the points it goes between, the box of the grid it is over and where it has got to.
/// The world it searches is the world of the step it first ran in (`scene`, gathered once: gathering it again every step cost about as much as the cells), and the grid is made again from it at every step; the nodes it has
/// looked at keep the costs they were found with. The agent walks in a world that has moved on, as it does when a search is answered late.
pub(crate) struct NavJob {
    from: [f32; 3],
    goal: [f32; 3],
    /// The goal is a point on the way to the target, not the target (`NavLimits.max_distance`).
    partial: bool,
    params: NavParams,
    who: Option<Filter>,
    cap: usize,
    bounds: [f32; 4],
    /// The second look over the whole map is the one that is running, and the error of the first look.
    wide: bool,
    first: Option<NavError>,
    run: Option<SearchRun>,
    scene: Option<std::sync::Arc<NavScene>>,
    /// Which entry of `JOB_CACHES` is this job's; a copy of the job has another.
    serial: u64,
    /// The tick the search was started (the oldest goes first).
    pub(crate) started: u64,
    /// The nodes expanded for it so far, in both looks.
    pub(crate) expanded: usize,
}

impl NavJob {
    /// The nodes the search has expanded so far, in both looks.
    #[cfg(test)]
    pub(crate) fn expanded(&self) -> usize {
        self.expanded
    }
}

impl Clone for NavJob {
    fn clone(&self) -> NavJob {
        NavJob {
            from: self.from,
            goal: self.goal,
            partial: self.partial,
            params: self.params,
            who: self.who,
            cap: self.cap,
            bounds: self.bounds,
            wide: self.wide,
            first: self.first.clone(),
            run: self.run.clone(),
            scene: self.scene.clone(),
            serial: next_job_serial(),
            started: self.started,
            expanded: self.expanded,
        }
    }
}

impl std::fmt::Debug for NavJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NavJob {{ started: {}, expanded: {}, wide: {} }}", self.started, self.expanded, self.wide)
    }
}

enum JobStep {
    Done(Result<PathResult, NavError>),
    Paused,
}

/// What a finished search gives back to its agent.
struct JobDone {
    result: Result<PathResult, NavError>,
    partial: bool,
    from: [f32; 3],
    goal: [f32; 3],
}

/// Work on a search for at most `budget` more nodes: what became of it, and how many nodes were expanded.
fn advance_job(job: &mut NavJob, scene: &NavScene, budget: usize) -> (JobStep, usize) {
    let p = job.params;
    let mut spent = 0usize;
    // (a failure of the second look over the whole map is the failure of the first)
    let fail = |job: &mut NavJob, e: NavError| if job.wide { job.first.take().unwrap_or(e) } else { e };
    loop {
        let mut grid = match Grid::build(&scene.all, &scene.ground, &scene.level, job.bounds, p, job.from, scene.sig) {
            Ok(g) => g,
            Err(e) => return (JobStep::Done(Err(fail(job, e))), spent),
        };
        grid.add_links(&scene.link_defs);
        if let Some(c) = JOB_CACHES.with(|m| m.borrow_mut().remove(&job.serial)) {
            grid.restore(c);
        }
        if job.run.is_none() {
            match SearchRun::begin(&mut grid, job.from, job.goal, &p) {
                Ok(r) => job.run = Some(r),
                Err(e) => return (JobStep::Done(Err(fail(job, e))), spent),
            }
        }
        let Some(run) = job.run.as_mut() else { unreachable!("the run was made above") };
        let before = run.searched;
        let r = run.advance(&mut grid, job.from, job.goal, &p, budget - spent, job.cap);
        spent += run.searched - before;
        match r {
            Ok(true) => {
                let out = run.finish(&mut grid, job.from, job.goal);
                return (JobStep::Done(out.map_err(|e| fail(job, e))), spent);
            }
            Ok(false) => {
                let kept = grid.save();
                JOB_CACHES.with(|m| {
                    let mut m = m.borrow_mut();
                    if m.len() >= MAX_JOB_CACHES {
                        m.clear();
                    }
                    m.insert(job.serial, kept);
                });
                return (JobStep::Paused, spent);
            }
            Err(e) => {
                if !job.wide && matches!(e, NavError::NoPath(_)) {
                    if let Some(wide) = wide_box(scene, job.from, job.goal, &p, job.bounds) {
                        job.first = Some(e);
                        job.bounds = wide;
                        job.wide = true;
                        job.run = None;
                        if spent >= budget {
                            return (JobStep::Paused, spent);
                        }
                        continue;
                    }
                }
                return (JobStep::Done(Err(fail(job, e))), spent);
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------------------- agents

#[derive(Clone, Debug)]
pub struct AgentPath {
    pub points: Vec<[f32; 3]>,
    /// As `PathResult::jumps`.
    pub jumps: Vec<Option<[f32; 2]>>,
    /// How far through the jump to `points[next]` the agent is (0 when it is walking), and where the jump started.
    hop_t: f32,
    hop_from: [f32; 3],
    pub next: usize,
    /// The least distance to the waypoint seen so far, and for how many ticks it has not got smaller.
    best: f32,
    stalled: u32,
    replans: u32,
    /// Ticks since the path was found (counted only while `NavLimits` exists).
    pub(crate) age: u32,
    /// The path leads to a point on the way to the target, not to the target (the search was limited): at its end the agent asks again.
    pub(crate) partial: bool,
}

/// The paths agents are following, by uid. Part of the simulation state.
#[derive(Clone, Debug, Default)]
pub struct NavState {
    pub paths: BTreeMap<u64, AgentPath>,
    /// With `NavLimits`: the agents that asked for a search and are waiting for their turn, by the tick they first asked (first come, first served).
    pub(crate) waiting: BTreeMap<u64, u64>,
    /// With `NavLimits.blocked_wait`: the agents whose last search found no way, by the tick until which they do not search again.
    pub(crate) blocked: BTreeMap<u64, u64>,
    /// Ticks counted while `NavLimits` exists.
    pub(crate) tick: u64,
    pub(crate) limited: bool,
    /// With the resource `nav.cells_per_step`: the searches that are on, by agent.
    pub(crate) jobs: BTreeMap<u64, NavJob>,
}

impl NavState {
    pub fn hash_into(&self, h: &mut StateHasher) {
        h.u64(self.paths.len() as u64);
        for (uid, p) in &self.paths {
            h.u64(*uid);
            h.u64(p.next as u64);
            h.u64(p.stalled as u64 | (p.replans as u64) << 32);
            h.u64(p.best.to_bits() as u64);
            // (only paths that use a link have a hop to hash: a path without one hashes as it always did)
            if p.jumps.iter().any(|j| j.is_some()) {
                h.u64(p.hop_t.to_bits() as u64);
                for v in p.hop_from {
                    h.u64(v.to_bits() as u64);
                }
            }
            for pt in &p.points {
                for v in pt {
                    h.u64(v.to_bits() as u64);
                }
            }
        }
        // (a world without `NavLimits` hashes as it always did)
        if self.limited {
            h.u64(self.tick);
            h.u64(self.waiting.len() as u64);
            for (uid, since) in &self.waiting {
                h.u64(*uid);
                h.u64(*since);
            }
            // (a world whose agents have never been blocked hashes as it did)
            if !self.blocked.is_empty() {
                h.u64(self.blocked.len() as u64);
                for (uid, until) in &self.blocked {
                    h.u64(*uid);
                    h.u64(*until);
                }
            }
            for p in self.paths.values() {
                h.u64(p.age as u64 | (p.partial as u64) << 32);
            }
            // (a world that does not slice its searches hashes as it did)
            if !self.jobs.is_empty() {
                h.u64(self.jobs.len() as u64);
                for (uid, j) in &self.jobs {
                    h.u64(*uid);
                    h.u64(j.started);
                    h.u64(j.wide as u64);
                    h.u64(j.expanded as u64);
                    h.u64(j.run.as_ref().map_or(0, |r| r.open.len() as u64));
                }
            }
        }
    }
}

/// The goal of a search that may not be longer than `max` metres: the real goal when it is near enough, otherwise the point `max` metres from `from` on the way to it (with
/// `true`: the path ends short of the target and the agent asks again when it gets there).
fn limited_goal(from: [f32; 3], to: [f32; 3], max: f32) -> ([f32; 3], bool) {
    let (dx, dz) = (to[0] - from[0], to[2] - from[2]);
    let d = (dx * dx + dz * dz).sqrt();
    if max.is_nan() || max <= 0.0 || d <= max {
        return (to, false);
    }
    let k = max / d;
    ([from[0] + dx * k, from[1] + (to[1] - from[1]) * k, from[2] + dz * k], true)
}

const STATUS_WALKING: u32 = 1;
const STATUS_ARRIVED: u32 = 2;
const STATUS_BLOCKED: u32 = 3;
const STALL_TICKS: u32 = 60;
const MAX_REPLANS: u32 = 3;

/// The layer an agent plans on: its own `CollisionLayer` when any entity of the world has one, otherwise none (nothing is filtered).
fn who_plans(layers: &Layers, uid: u64) -> Option<Filter> {
    (!layers.is_empty()).then(|| layers.of(uid))
}

fn params_of(a: &NavAgent) -> NavParams {
    NavParams { radius: a.radius, height: a.height, step_height: a.step_height, max_drop: a.max_drop, cell: 0.5, keep: false }
}

thread_local! {
    /// The milliseconds `step` spent in the parts of `NAV_PARTS` since a profile last took them (`take_nav_ms`).
    static NAV_MS: std::cell::RefCell<[f64; 3]> = const { std::cell::RefCell::new([0.0; 3]) };
}

/// What `NAV_MS` counts: gathering the world for a search done a few cells at a time, working on those searches, and the searches that are done whole.
pub(crate) const NAV_PARTS: [&str; 3] = ["scene", "jobs", "searches"];

fn nav_time(part: usize, since: std::time::Instant) {
    NAV_MS.with(|m| m.borrow_mut()[part] += since.elapsed().as_secs_f64() * 1e3);
}

/// The milliseconds spent in the parts of `NAV_PARTS` since the last call.
pub(crate) fn take_nav_ms() -> [f64; 3] {
    NAV_MS.with(|m| std::mem::take(&mut *m.borrow_mut()))
}

/// One tick of every `NavAgent`, in uid order.
pub fn step(world: &mut World, state: &mut NavState, dt: f32, fields: &Terrains, meshes: &MeshStore) {
    let mut agents: Vec<(u64, lunaris_ecs::Entity, Transform, NavAgent)> = Vec::new();
    if world.for_each_uid::<(&Transform, &NavAgent), _>(|e, uid, (t, a)| agents.push((uid, e, *t, *a))).is_err() {
        return;
    }
    if agents.is_empty() {
        state.paths.clear();
        state.jobs.clear();
        return;
    }
    agents.sort_by_key(|a| a.0);
    // who keeps out of whose way: agents with a `NavAvoid`, and everything they may meet (agents and characters, where they were at the start of the tick)
    let mut avoid: BTreeMap<u64, NavAvoid> = BTreeMap::new();
    let _ = world.for_each_uid::<(&NavAvoid,), _>(|_, uid, (v,)| {
        avoid.insert(uid, *v);
    });
    let mut others: BTreeMap<u64, ([f32; 3], f32)> = BTreeMap::new();
    if !avoid.is_empty() {
        let _ = world.for_each_uid::<(&Transform, &NavAgent), _>(|_, uid, (t, n)| {
            others.insert(uid, (t.pos, n.radius));
        });
        let _ = world.for_each_uid::<(&Transform, &CharacterBody), _>(|_, uid, (t, c)| {
            others.entry(uid).or_insert((t.pos, c.radius));
        });
    }
    let alive: BTreeSet<u64> = agents.iter().map(|a| a.0).collect();
    state.paths.retain(|uid, _| alive.contains(uid));
    let layers = Layers::gather(world);
    // `NavLimits` (one entity, the lowest uid): a budget of searches for the tick, a least time between two searches of an agent and a longest search
    let limits = {
        let mut found: Option<(u64, NavLimits)> = None;
        let _ = world.for_each_uid::<(&NavLimits,), _>(|_, uid, (l,)| {
            if found.as_ref().is_none_or(|f| uid < f.0) {
                found = Some((uid, *l));
            }
        });
        found.map(|f| f.1)
    };
    state.limited = limits.is_some();
    // the resource `nav.cells_per_step` (a whole number, 0 or missing: none): the most nodes all the searches of one step expand together; a search that is not over then goes on in the next step (oldest first), so that a hard search
    // (a target that cannot be reached, which is looked for in every direction up to `max_searched`) costs a few steps a little each and not one step a great deal
    let slice = if limits.is_some() { world.resource("nav.cells_per_step").filter(|v| v.is_finite() && *v >= 1.0).map_or(0, |v| v.min(1.0e9) as usize) } else { 0 };
    // who gets a search this tick: the agents that ask (a request that is on, a stalled path to replan), the one that has waited longest first, up to the budget
    let mut granted: BTreeSet<u64> = BTreeSet::new();
    if let Some(lim) = limits {
        state.tick += 1;
        let min_ticks = if lim.min_replan > 0.0 && dt > 0.0 { (lim.min_replan / dt).ceil() as u32 } else { 0 };
        for p in state.paths.values_mut() {
            p.age = p.age.saturating_add(1);
        }
        // (a search that is on is served by the budget of its step; its agent asks for nothing more, and when the agent's request is turned off the search is dropped)
        if slice > 0 {
            let wanting: BTreeSet<u64> = agents.iter().filter(|a| a.3.active).map(|a| a.0).collect();
            state.jobs.retain(|uid, _| wanting.contains(uid));
        } else {
            state.jobs.clear();
        }
        let mut asking: Vec<(u64, u64)> = Vec::new();
        for (uid, _, _, a) in &agents {
            let path = state.paths.get(uid);
            let wants = (a.active && path.is_none_or(|p| p.age >= min_ticks)) || (a.status == STATUS_WALKING && path.is_some_and(|p| p.stalled >= STALL_TICKS));
            // (an agent whose last search found no way waits `blocked_wait` before it asks again: its request stays on, and it is not asking)
            if wants && state.blocked.get(uid).is_none_or(|until| *until <= state.tick) && !state.jobs.contains_key(uid) {
                let since = *state.waiting.entry(*uid).or_insert(state.tick);
                asking.push((since, *uid));
            }
        }
        // whoever stopped asking (the request was turned off, the agent is gone) is forgotten
        state.waiting.retain(|uid, _| asking.iter().any(|(_, u)| u == uid));
        state.blocked.retain(|uid, until| *until > state.tick && alive.contains(uid));
        asking.sort();
        let budget = if lim.searches_per_tick == 0 { usize::MAX } else { lim.searches_per_tick as usize };
        let budget = if slice > 0 { budget.saturating_sub(state.jobs.len()) } else { budget };
        for (_, uid) in asking.into_iter().take(budget) {
            granted.insert(uid);
            state.waiting.remove(&uid);
        }
    } else {
        state.waiting.clear();
        state.blocked.clear();
        state.jobs.clear();
        state.tick = 0;
    }
    let max_search = limits.map(|l| l.max_distance).unwrap_or(0.0);
    let cap = limits.map(|l| l.max_searched as usize).unwrap_or(0);
    let keep = limits.is_some_and(|l| l.keep_cells);
    let blocked_ticks = match limits {
        Some(l) if l.blocked_wait > 0.0 && dt > 0.0 => (l.blocked_wait / dt).ceil() as u64,
        _ => 0,
    };
    // the searches that are done a few nodes at a time: the ones that are on go on, the oldest first, and the agents that were granted a search start theirs, until the nodes of the step are spent
    let mut finished: BTreeMap<u64, JobDone> = BTreeMap::new();
    if slice > 0 {
        for (uid, _, t, a) in &agents {
            if a.active && granted.contains(uid) && !state.jobs.contains_key(uid) {
                let feet = [t.pos[0], t.pos[1] - a.feet, t.pos[2]];
                let (goal, partial) = limited_goal(feet, a.target, max_search);
                let params = NavParams { keep, ..params_of(a) }.sane();
                let bounds = near_box(feet, goal, &params);
                state.jobs.insert(*uid, NavJob { from: feet, goal, partial, params, who: who_plans(&layers, *uid), cap, bounds, wide: false, first: None, run: None, scene: None, serial: next_job_serial(), started: state.tick, expanded: 0 });
            }
        }
        let mut order: Vec<(u64, u64)> = state.jobs.iter().map(|(uid, j)| (j.started, *uid)).collect();
        order.sort_unstable();
        let mut left = slice;
        let mut scenes: Vec<(Option<Filter>, std::sync::Arc<NavScene>)> = Vec::new();
        for (_, uid) in order {
            if left == 0 {
                break;
            }
            let Some(job) = state.jobs.get_mut(&uid) else { continue };
            // (the agents of one layer that start in one step see the same world: it is gathered once for them)
            let scene = match &job.scene {
                Some(s) => s.clone(),
                None => {
                    let s = match scenes.iter().find(|(w, _)| *w == job.who) {
                        Some((_, s)) => s.clone(),
                        None => {
                            let t_scene = std::time::Instant::now();
                            let s = std::sync::Arc::new(gather_scene(world, fields, meshes, job.who));
                            nav_time(0, t_scene);
                            scenes.push((job.who, s.clone()));
                            s
                        }
                    };
                    job.scene = Some(s.clone());
                    s
                }
            };
            let t_job = std::time::Instant::now();
            let (what, spent) = advance_job(job, &scene, left);
            nav_time(1, t_job);
            job.expanded += spent;
            left -= spent.min(left);
            if let JobStep::Done(result) = what {
                finished.insert(uid, JobDone { result, partial: job.partial, from: job.from, goal: job.goal });
                state.jobs.remove(&uid);
            }
        }
    }
    // (the caches of searches that were dropped or finished are not kept)
    JOB_CACHES.with(|m| {
        let mut m = m.borrow_mut();
        if !m.is_empty() {
            m.retain(|serial, _| state.jobs.values().any(|j| j.serial == *serial));
        }
    });
    for (uid, entity, t, mut a) in agents {
        let feet = [t.pos[0], t.pos[1] - a.feet, t.pos[2]];
        let was = (a.status, a.active);
        let outcome = if slice > 0 {
            finished.remove(&uid).map(|f| (f.result, f.partial, f.goal, f.from))
        } else if a.active && (limits.is_none() || granted.contains(&uid)) {
            let (goal, partial) = limited_goal(feet, a.target, max_search);
            let t_search = std::time::Instant::now();
            let found = find_path_capped(world, feet, goal, NavParams { keep, ..params_of(&a) }, fields, meshes, who_plans(&layers, uid), cap);
            nav_time(2, t_search);
            Some((found, partial, goal, feet))
        } else {
            None
        };
        if let Some((result, partial, goal, feet)) = outcome {
            a.active = false;
            match result {
                Ok(r) => {
                    state.paths.insert(uid, AgentPath { points: r.points, jumps: r.jumps, hop_t: 0.0, hop_from: [0.0; 3], next: 1, best: f32::INFINITY, stalled: 0, replans: 0, age: 0, partial });
                    a.status = STATUS_WALKING;
                }
                // a limited search whose point on the way has no ground to stand on: walk straight to it (and ask again from there)
                Err(_) if partial => {
                    state.paths.insert(uid, AgentPath { points: vec![feet, goal], jumps: vec![None, None], hop_t: 0.0, hop_from: [0.0; 3], next: 1, best: f32::INFINITY, stalled: 0, replans: 0, age: 0, partial: true });
                    a.status = STATUS_WALKING;
                }
                Err(_) => {
                    state.paths.remove(&uid);
                    a.status = STATUS_BLOCKED;
                    if blocked_ticks > 0 {
                        state.blocked.insert(uid, state.tick + blocked_ticks);
                    }
                }
            }
        }
        let mut walk: Option<[f32; 2]> = None;
        if a.status == STATUS_WALKING {
            if let Some(path) = state.paths.get_mut(&uid) {
                let last = path.points.len() - 1;
                // a jump over a link: the agent is carried along an arc, and nothing else happens to it meanwhile
                if let Some(Some([jspeed, hop])) = path.jumps.get(path.next).copied() {
                    if path.hop_t == 0.0 {
                        path.hop_from = feet;
                    }
                    let (from_p, to) = (path.hop_from, path.points[path.next]);
                    let len = ((to[0] - from_p[0]).powi(2) + (to[1] - from_p[1]).powi(2) + (to[2] - from_p[2]).powi(2)).sqrt().max(0.01);
                    path.hop_t = (path.hop_t + jspeed * dt / len).clamp(1.0e-6, 1.0);
                    let t = path.hop_t;
                    let pos = [from_p[0] + (to[0] - from_p[0]) * t, from_p[1] + (to[1] - from_p[1]) * t + hop * 4.0 * t * (1.0 - t), from_p[2] + (to[2] - from_p[2]) * t];
                    if t >= 1.0 {
                        path.next += 1;
                        path.hop_t = 0.0;
                        path.best = f32::INFINITY;
                        path.stalled = 0;
                    }
                    if let Some(tr) = world.get_mut::<Transform>(entity) {
                        tr.pos = [pos[0], pos[1] + a.feet, pos[2]];
                    }
                    if let Some(c) = world.get_mut::<CharacterBody>(entity) {
                        c.walk = [0.0, 0.0];
                        c.fall = 0.0;
                    }
                    if let Some(v) = world.get_mut::<Velocity>(entity) {
                        v.lin = [0.0; 3];
                    }
                    continue;
                }
                // skip the waypoints we are already at
                while path.next < last {
                    let w = path.points[path.next];
                    let (dx, dz) = (w[0] - feet[0], w[2] - feet[2]);
                    if (dx * dx + dz * dz).sqrt() <= (a.speed.abs() * dt * 1.5).max(0.2) {
                        path.next += 1;
                        path.best = f32::INFINITY;
                        path.stalled = 0;
                    } else {
                        break;
                    }
                }
                let w = path.points[path.next.min(last)];
                let (dx, dz) = (w[0] - feet[0], w[2] - feet[2]);
                let dist = (dx * dx + dz * dz).sqrt();
                if path.next >= last && dist <= a.arrive.abs().max(0.05) {
                    if path.partial {
                        // the end of the part that was searched, not of the way: ask for the rest (the target is still the real one) and wait here meanwhile
                        a.active = true;
                    } else {
                        a.status = STATUS_ARRIVED;
                        state.paths.remove(&uid);
                    }
                } else {
                    // remaining path length, for whoever is watching
                    let mut rem = dist;
                    for k in path.next.min(last)..last {
                        let (p, q) = (path.points[k], path.points[k + 1]);
                        rem += ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).sqrt();
                    }
                    a.remaining = rem;
                    // no progress for a second: plan again from where we are; three failures in a row mean the way is blocked
                    if dist < path.best - 0.02 {
                        path.best = dist;
                        path.stalled = 0;
                    } else {
                        path.stalled += 1;
                    }
                    if path.stalled >= STALL_TICKS && (limits.is_none() || granted.contains(&uid)) {
                        let replans = path.replans + 1;
                        state.paths.remove(&uid);
                        let (goal, partial) = limited_goal(feet, a.target, max_search);
                        match (replans <= MAX_REPLANS).then(|| find_path_capped(world, feet, goal, params_of(&a), fields, meshes, who_plans(&layers, uid), cap)) {
                            Some(Ok(r)) => {
                                state.paths.insert(uid, AgentPath { points: r.points, jumps: r.jumps, hop_t: 0.0, hop_from: [0.0; 3], next: 1, best: f32::INFINITY, stalled: 0, replans, age: 0, partial });
                            }
                            _ => {
                                a.status = STATUS_BLOCKED;
                            }
                        }
                    } else {
                        let speed = a.speed.abs();
                        let mut w = [dx / dist * speed, dz / dist * speed];
                        if let Some(av) = avoid.get(&uid) {
                            // keep out of the way of the others: a push away from each one that is close, turned to the right when it comes head on
                            let (mut px, mut pz) = (0.0f32, 0.0f32);
                            for (ouid, (opos, orad)) in &others {
                                if *ouid == uid {
                                    continue;
                                }
                                let (ox, oz) = (feet[0] - opos[0], feet[2] - opos[2]);
                                let d = (ox * ox + oz * oz).sqrt();
                                let reach = (a.radius + orad) * av.range.max(1.0);
                                if d > 1.0e-4 && d < reach {
                                    let k = (1.0 - d / reach) * av.strength.max(0.0);
                                    px += ox / d * k;
                                    pz += oz / d * k;
                                }
                            }
                            let push = (px * px + pz * pz).sqrt();
                            if push > 1.0e-5 {
                                let (fx, fz) = (dx / dist, dz / dist);
                                if (px * fx + pz * fz) / push < -0.3 {
                                    px += -fz * push * 0.6;
                                    pz += fx * push * 0.6;
                                }
                                let (vx, vz) = (w[0] + px * speed * 1.5, w[1] + pz * speed * 1.5);
                                let l = (vx * vx + vz * vz).sqrt();
                                if l > 1.0e-5 {
                                    w = [vx / l * speed, vz / l * speed];
                                }
                            }
                        }
                        walk = Some(w);
                    }
                }
            } else {
                a.status = STATUS_BLOCKED;
            }
        }
        if matches!(a.status, STATUS_ARRIVED | STATUS_BLOCKED) {
            a.remaining = 0.0;
        }
        // write back, and move the body
        if let Some(na) = world.get_mut::<NavAgent>(entity) {
            *na = a;
        }
        let controlled = a.status == STATUS_WALKING || was.0 == STATUS_WALKING || was.1;
        if controlled {
            let [vx, vz] = walk.unwrap_or([0.0, 0.0]);
            if let Some(c) = world.get_mut::<CharacterBody>(entity) {
                c.walk = [vx, vz];
            } else if let Some(v) = world.get_mut::<Velocity>(entity) {
                v.lin[0] = vx;
                v.lin[2] = vz;
            } else if let Some(tr) = world.get_mut::<Transform>(entity) {
                tr.pos[0] += vx * dt;
                tr.pos[2] += vz * dt;
            }
        }
    }
}

