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
//! a line-of-sight pass over the grid removes the waypoints that were only there because of the cells.
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
use crate::components::{CharacterBody, Collider, NavAgent, NavAvoid, NavLink, NavObstacle, RigidBody, Transform, Velocity};
use crate::layers::{Filter, Layers};
use crate::meshcol::{Aabb, Instance, MeshStore, V3};
use crate::terrain::{grounds, Ground, Terrains};
use lunaris_ecs::{StateHasher, World};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

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
}

impl Default for NavParams {
    fn default() -> NavParams {
        NavParams { radius: 0.4, height: 1.8, step_height: 0.4, max_drop: 1.2, cell: 0.5 }
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

/// The search grid: for every cell the heights of the surfaces an agent can stand on.
struct Grid {
    cell: f32,
    x0: f32,
    z0: f32,
    nx: usize,
    nz: usize,
    /// `start[c]..start[c + 1]` indexes `height` and `node_cell` for cell `c`; a node is one surface of one cell.
    start: Vec<u32>,
    height: Vec<f32>,
    node_cell: Vec<u32>,
    /// Cells with a solid mass above the local ground (a wall, a tree): nothing can step off a ledge across them.
    occupied: Vec<bool>,
    /// The `NavLink` edges: from node, to node, cost, speed and hop of the link.
    link_edges: BTreeMap<usize, Vec<(usize, u32, [f32; 2])>>,
    p: NavParams,
}

impl Grid {
    fn build(solids: &[Solid], ground: &[Ground], meshes: &[Instance], bounds: [f32; 4], p: NavParams, start_at: [f32; 3]) -> Result<Grid, NavError> {
        let [bx0, bz0, bx1, bz1] = bounds;
        let mut cell = p.cell;
        let (nx, nz) = loop {
            let nx = (((bx1 - bx0) / cell).ceil() as usize).max(1);
            let nz = (((bz1 - bz0) / cell).ceil() as usize).max(1);
            if nx.saturating_mul(nz) <= MAX_CELLS {
                break (nx, nz);
            }
            cell *= 2.0;
            if cell > p.cell * 8.0 {
                return Err(NavError::TooBig(format!("the area between the two points is {:.0} m by {:.0} m, too big to search ({} cells at the largest cell size, {:.1} m)", bx1 - bx0, bz1 - bz0, nx.saturating_mul(nz), cell)));
            }
        };
        let mut tops: Vec<Vec<f32>> = vec![Vec::new(); nx * nz];
        // what occupies a cell: the part of a solid right over it (`exact`), and the part of a solid whose growth by the agent's radius
        // reaches it (`margin`): only a margin obstacle that rises higher than a step keeps an agent out
        let mut exact: Vec<Vec<(f32, f32)>> = vec![Vec::new(); nx * nz];
        let mut margin: Vec<Vec<(f32, f32)>> = vec![Vec::new(); nx * nz];
        let range = |lo: f32, hi: f32, origin: f32, n: usize| -> (usize, usize) {
            let a = (((lo - origin) / cell).floor() as i64).clamp(0, n as i64 - 1) as usize;
            let b = (((hi - origin) / cell).floor() as i64).clamp(0, n as i64 - 1) as usize;
            (a, b)
        };
        for s in solids {
            // outside the grid altogether
            if s.hi[0] + p.radius < bx0 || s.lo[0] - p.radius > bx1 || s.hi[2] + p.radius < bz0 || s.lo[2] - p.radius > bz1 {
                continue;
            }
            let (ixa, ixb) = range(s.lo[0] - p.radius, s.hi[0] + p.radius, bx0, nx);
            let (iza, izb) = range(s.lo[2] - p.radius, s.hi[2] + p.radius, bz0, nz);
            for iz in iza..=izb {
                let cz = bz0 + (iz as f32 + 0.5) * cell;
                for ix in ixa..=ixb {
                    let cx = bx0 + (ix as f32 + 0.5) * cell;
                    let inside = cx >= s.lo[0] && cx <= s.hi[0] && cz >= s.lo[2] && cz <= s.hi[2];
                    if inside {
                        exact[iz * nx + ix].push((s.lo[1], s.hi[1]));
                        if s.is_box {
                            tops[iz * nx + ix].push(s.hi[1]);
                        }
                    } else if cx >= s.lo[0] - p.radius && cx <= s.hi[0] + p.radius && cz >= s.lo[2] - p.radius && cz <= s.hi[2] + p.radius {
                        margin[iz * nx + ix].push((s.lo[1], s.hi[1]));
                    }
                }
            }
        }
        // the ground of terrains: the height under each cell's centre, where it is not a cliff
        for g in ground {
            let placed = g.placed();
            let half = [placed.size[0] * 0.5, placed.size[1] * 0.5];
            if placed.centre[0] + half[0] < bx0 || placed.centre[0] - half[0] > bx1 || placed.centre[1] + half[1] < bz0 || placed.centre[1] - half[1] > bz1 {
                continue;
            }
            let (ixa, ixb) = range(placed.centre[0] - half[0], placed.centre[0] + half[0], bx0, nx);
            let (iza, izb) = range(placed.centre[1] - half[1], placed.centre[1] + half[1], bz0, nz);
            for iz in iza..=izb {
                let cz = bz0 + (iz as f32 + 0.5) * cell;
                for ix in ixa..=ixb {
                    let cx = bx0 + (ix as f32 + 0.5) * cell;
                    let Some(h) = placed.surface(cx, cz) else { continue };
                    if placed.normal(cx, cz)[1] >= TERRAIN_MIN_UP {
                        tops[iz * nx + ix].push(h);
                    }
                }
            }
        }
        // the triangles of mesh colliders: walkable ones give surfaces, all of them give obstacles (see the module comment)
        for m in meshes {
            let pm = &m.placed;
            if pm.bounds.hi[0] + p.radius < bx0 || pm.bounds.lo[0] - p.radius > bx1 || pm.bounds.hi[2] + p.radius < bz0 || pm.bounds.lo[2] - p.radius > bz1 {
                continue;
            }
            let region = Aabb { lo: [bx0 - p.radius, f32::NEG_INFINITY, bz0 - p.radius], hi: [bx1 + p.radius, f32::INFINITY, bz1 + p.radius] };
            for ti in pm.triangles_near(&region) {
                let tri = pm.tri(ti);
                let [a, b, c] = tri;
                let (tx0, tx1) = (a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
                let (tz0, tz1) = (a[2].min(b[2]).min(c[2]), a[2].max(b[2]).max(c[2]));
                if tx1 + p.radius < bx0 || tx0 - p.radius > bx1 || tz1 + p.radius < bz0 || tz0 - p.radius > bz1 {
                    continue;
                }
                let (ixa, ixb) = range(tx0 - p.radius, tx1 + p.radius, bx0, nx);
                let (iza, izb) = range(tz0 - p.radius, tz1 + p.radius, bz0, nz);
                let n = tri_cross(a, b, c);
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                let walkable = len > 0.0 && n[1] / len >= TERRAIN_MIN_UP;
                // weights of a point over the triangle seen from above: positive inside; a triangle seen edge-on has no inside
                let den = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
                for iz in iza..=izb {
                    let cz = bz0 + (iz as f32 + 0.5) * cell;
                    for ix in ixa..=ixb {
                        let cx = bx0 + (ix as f32 + 0.5) * cell;
                        if den != 0.0 {
                            let wa = ((b[2] - c[2]) * (cx - c[0]) + (c[0] - b[0]) * (cz - c[2])) / den;
                            let wb = ((c[2] - a[2]) * (cx - c[0]) + (a[0] - c[0]) * (cz - c[2])) / den;
                            let wc = 1.0 - wa - wb;
                            if wa >= 0.0 && wb >= 0.0 && wc >= 0.0 {
                                let y = wa * a[1] + wb * b[1] + wc * c[1];
                                exact[iz * nx + ix].push((y, y));
                                if walkable {
                                    tops[iz * nx + ix].push(y);
                                }
                                continue;
                            }
                        }
                        // (a floor that can be walked on is not an obstacle beside the cells it covers, like the ground of a terrain)
                        if !walkable {
                            if let Some((y0, y1)) = clip_height(&tri, cx, cz, p.radius) {
                                margin[iz * nx + ix].push((y0, y1));
                            }
                        }
                    }
                }
            }
        }
        // a start that is not standing on anything (a top-down game where the floor is only a picture) means one flat, open floor at the
        // start's height: obstacles still block it
        let standing = ((start_at[0] - bx0) / cell >= 0.0 && (start_at[2] - bz0) / cell >= 0.0)
            .then(|| (((start_at[2] - bz0) / cell) as usize, ((start_at[0] - bx0) / cell) as usize))
            .filter(|&(iz, ix)| iz < nz && ix < nx)
            .is_some_and(|(iz, ix)| tops[iz * nx + ix].iter().any(|t| (t - start_at[1]).abs() <= 2.0 + p.step_height));
        if !standing {
            for t in tops.iter_mut() {
                if !t.iter().any(|v| (v - start_at[1]).abs() < 0.01) {
                    t.push(start_at[1]);
                }
            }
        }
        let mut start = Vec::with_capacity(nx * nz + 1);
        let mut height = Vec::new();
        let mut node_cell = Vec::new();
        let mut occupied = vec![false; nx * nz];
        for c in 0..nx * nz {
            start.push(height.len() as u32);
            let t = &mut tops[c];
            t.sort_by(|a, b| a.total_cmp(b));
            t.dedup_by(|a, b| (*a - *b).abs() < 0.01);
            let lowest = t.first().copied().unwrap_or(f32::NEG_INFINITY);
            occupied[c] = exact[c].iter().any(|&(_, b1)| b1 > lowest + p.step_height + 0.04);
            for &top in t.iter() {
                // standable: nothing but the surface itself intersects the agent's height above it
                let free = !exact[c].iter().any(|&(b0, b1)| b0 < top + p.height && b1 > top + 0.04) && !margin[c].iter().any(|&(b0, b1)| b0 < top + p.height && b1 > top + p.step_height + 0.04);
                if free {
                    height.push(top);
                    node_cell.push(c as u32);
                }
            }
        }
        start.push(height.len() as u32);
        Ok(Grid { cell, x0: bx0, z0: bz0, nx, nz, start, height, node_cell, occupied, link_edges: BTreeMap::new(), p })
    }

    /// Add the edges of `NavLink`s whose ends are both on ground of this grid (within about one and a half metres of the link's end).
    fn add_links(&mut self, defs: &[LinkDef]) {
        let reach = |g: &Grid, p: [f32; 3]| -> Option<usize> {
            let (n, _) = g.locate(p[0], p[2], p[1])?;
            let c = g.centre(n);
            ((c[0] - p[0]).powi(2) + (c[2] - p[2]).powi(2) <= 2.25).then_some(n)
        };
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

    fn surfaces(&self, c: usize) -> std::ops::Range<usize> {
        self.start[c] as usize..self.start[c + 1] as usize
    }

    fn centre(&self, node: usize) -> [f32; 3] {
        let c = self.node_cell[node] as usize;
        [self.x0 + ((c % self.nx) as f32 + 0.5) * self.cell, self.height[node], self.z0 + ((c / self.nx) as f32 + 0.5) * self.cell]
    }

    /// Can an agent standing at height `from` step to a surface at height `to`?
    fn connects(&self, from: f32, to: f32) -> bool {
        to - from <= self.p.step_height + 1.0e-3 && from - to <= self.p.max_drop + 1.0e-3
    }

    /// The surface of cell `c` that an agent at height `y` can reach, the nearest in height.
    fn reach(&self, c: usize, y: f32) -> Option<usize> {
        self.surfaces(c).filter(|&n| self.connects(y, self.height[n])).min_by(|&a, &b| (self.height[a] - y).abs().total_cmp(&(self.height[b] - y).abs()))
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
    fn locate(&self, x: f32, z: f32, y: f32) -> Option<(usize, bool)> {
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
                    for n in self.surfaces(c) {
                        let dy = (self.height[n] - y).abs();
                        if dy > 2.0 + self.p.step_height {
                            continue; // a roof or a ledge far above or below the point is not the ground it means
                        }
                        // closer in the plane first; at the same distance the surface nearest in height
                        let better = best.is_none_or(|(bd, bh, _, _)| d2 < bd - 1.0e-6 || ((d2 - bd).abs() <= 1.0e-6 && dy < bh));
                        if better {
                            best = Some((d2, dy, n, ring == 0));
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
    fn neighbours(&self, node: usize, out: &mut Vec<(usize, u32)>) {
        out.clear();
        let c = self.node_cell[node] as usize;
        let (ix, iz) = ((c % self.nx) as i64, (c / self.nx) as i64);
        let h = self.height[node];
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
                for n in self.surfaces(cell) {
                    if self.connects(h, self.height[n]) {
                        let rise = ((self.height[n] - h).abs() * 1000.0) as u32;
                        out.push((n, if diagonal { DIAG } else { ORTHO } + rise));
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
            if self.occupied[adjacent] || self.reach(adjacent, h).is_some() {
                continue;
            }
            for n in self.surfaces((fz * self.nx as i64 + fx) as usize) {
                let drop = h - self.height[n];
                if drop > self.p.step_height + 1.0e-3 && drop <= self.p.max_drop + 1.0e-3 {
                    out.push((n, 2 * ORTHO + (drop * 1000.0) as u32));
                }
            }
        }
    }

    /// Can an agent walk the straight line from `a` to `b`, cell by cell, keeping to ground it can step between?
    fn line_ok(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
        let len = (dx * dx + dz * dz).sqrt();
        let n = ((len / (self.cell * 0.5)).ceil() as usize).max(1);
        let mut y = a[1];
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let Some(c) = self.cell_at(a[0] + dx * t, a[2] + dz * t) else { return false };
            match self.reach(c, y) {
                Some(node) => y = self.height[node],
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
    let p = params.sane();
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
    let (dx, dz) = (to[0] - from[0], to[2] - from[2]);
    let dist = (dx * dx + dz * dz).sqrt();
    let margin = (0.75 * dist).max(8.0 + p.radius);
    let near = [from[0].min(to[0]) - margin, from[2].min(to[2]) - margin, from[0].max(to[0]) + margin, from[2].max(to[2]) + margin];
    let first = search(&all, &ground, &level, &link_defs, near, from, to, p);
    // the way round may lead far outside the box around the two points (a maze whose only gap is at the other end): when nothing
    // was found there, look again over the whole map (the cells grow coarser if the map is huge)
    if matches!(first, Err(NavError::NoPath(_))) {
        let mut world_box = [from[0].min(to[0]), from[2].min(to[2]), from[0].max(to[0]), from[2].max(to[2])];
        for s in &all {
            world_box = [world_box[0].min(s.lo[0]), world_box[1].min(s.lo[2]), world_box[2].max(s.hi[0]), world_box[3].max(s.hi[2])];
        }
        for g in &ground {
            let placed = g.placed();
            world_box = [world_box[0].min(placed.centre[0] - placed.size[0] * 0.5), world_box[1].min(placed.centre[1] - placed.size[1] * 0.5), world_box[2].max(placed.centre[0] + placed.size[0] * 0.5), world_box[3].max(placed.centre[1] + placed.size[1] * 0.5)];
        }
        for m in &level {
            let b = &m.placed.bounds;
            world_box = [world_box[0].min(b.lo[0]), world_box[1].min(b.lo[2]), world_box[2].max(b.hi[0]), world_box[3].max(b.hi[2])];
        }
        let m = 2.0 + p.radius;
        let wide = [world_box[0] - m, world_box[1] - m, world_box[2] + m, world_box[3] + m];
        if wide[0] < near[0] || wide[1] < near[1] || wide[2] > near[2] || wide[3] > near[3] {
            let again = search(&all, &ground, &level, &link_defs, wide, from, to, p);
            if again.is_ok() {
                return again;
            }
        }
    }
    first
}

/// One search over the grid cut from `bounds`.
#[allow(clippy::too_many_arguments)]
fn search(all: &[Solid], ground: &[Ground], level: &[Instance], link_defs: &[LinkDef], bounds: [f32; 4], from: [f32; 3], to: [f32; 3], p: NavParams) -> Result<PathResult, NavError> {
    let mut grid = Grid::build(all, ground, level, bounds, p, from)?;
    grid.add_links(link_defs);
    let (start, _) = grid.locate(from[0], from[2], from[1]).ok_or_else(|| NavError::NoStart(format!("the start ({:.1}, {:.1}, {:.1}) is not on ground an agent of radius {:.2} and height {:.2} can stand on (inside a wall, or off the map)", from[0], from[1], from[2], p.radius, p.height)))?;
    let (goal, exact) = grid.locate(to[0], to[2], to[1]).ok_or_else(|| NavError::NoGoal(format!("the target ({:.1}, {:.1}, {:.1}) is not near any ground an agent can stand on", to[0], to[1], to[2])))?;
    let goal_cell = grid.node_cell[goal] as usize;
    let (gx, gz) = ((goal_cell % grid.nx) as i64, (goal_cell / grid.nx) as i64);

    // A*: the cheapest cost so far per node, ties broken by (f, h, node number)
    let mut g = vec![u32::MAX; grid.height.len()];
    let mut came = vec![u32::MAX; grid.height.len()];
    let mut open: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    let mut searched = 0usize;
    let heur = |n: usize| {
        let c = grid.node_cell[n] as usize;
        octile((c % grid.nx) as i64, (c / grid.nx) as i64, gx, gz)
    };
    g[start] = 0;
    open.push(Reverse((heur(start), heur(start), start as u32)));
    let mut next = Vec::new();
    let mut found = false;
    while let Some(Reverse((_, _, node))) = open.pop() {
        let node = node as usize;
        if node == goal {
            found = true;
            break;
        }
        searched += 1;
        let gn = g[node];
        grid.neighbours(node, &mut next);
        for &(m, cost) in &next {
            let ng = gn.saturating_add(cost);
            if ng < g[m] {
                g[m] = ng;
                came[m] = node as u32;
                open.push(Reverse((ng.saturating_add(heur(m)), heur(m), m as u32)));
            }
        }
    }
    if !found {
        return Err(NavError::NoPath(format!("no walkable route from ({:.1}, {:.1}, {:.1}) to ({:.1}, {:.1}, {:.1}) (searched {searched} cells): walls, drops of more than {:.1} m or steps higher than {:.2} m are in the way", from[0], from[1], from[2], to[0], to[1], to[2], p.max_drop, p.step_height)));
    }
    let mut nodes = vec![goal];
    while let Some(&last) = nodes.last() {
        let prev = came[last];
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
        if let Some(v) = grid.link_edges.get(&nodes[k - 1]) {
            if let Some(&(_, _, hs)) = v.iter().find(|e| e.0 == nodes[k]) {
                // a link is only used when walking from the one node to the next is not possible: the grid's own edges are tried by the search first
                let walkable = grid.reach(grid.node_cell[nodes[k]] as usize, grid.height[nodes[k - 1]]).is_some() && {
                    let (a, b) = (grid.node_cell[nodes[k - 1]] as i64, grid.node_cell[nodes[k]] as i64);
                    let (ax, az, bx, bz) = (a % grid.nx as i64, a / grid.nx as i64, b % grid.nx as i64, b / grid.nx as i64);
                    (ax - bx).abs() <= 1 && (az - bz).abs() <= 1
                };
                if !walkable {
                    jump_at[k] = Some(hs);
                }
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
}

/// The paths agents are following, by uid. Part of the simulation state.
#[derive(Clone, Debug, Default)]
pub struct NavState {
    pub paths: BTreeMap<u64, AgentPath>,
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
    }
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
    NavParams { radius: a.radius, height: a.height, step_height: a.step_height, max_drop: a.max_drop, cell: 0.5 }
}

/// One tick of every `NavAgent`, in uid order.
pub fn step(world: &mut World, state: &mut NavState, dt: f32, fields: &Terrains, meshes: &MeshStore) {
    let mut agents: Vec<(u64, lunaris_ecs::Entity, Transform, NavAgent)> = Vec::new();
    if world.for_each_uid::<(&Transform, &NavAgent), _>(|e, uid, (t, a)| agents.push((uid, e, *t, *a))).is_err() {
        return;
    }
    if agents.is_empty() {
        state.paths.clear();
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
    for (uid, entity, t, mut a) in agents {
        let feet = [t.pos[0], t.pos[1] - a.feet, t.pos[2]];
        let was = (a.status, a.active);
        if a.active {
            a.active = false;
            match find_path_as(world, feet, a.target, params_of(&a), fields, meshes, who_plans(&layers, uid)) {
                Ok(r) => {
                    state.paths.insert(uid, AgentPath { points: r.points, jumps: r.jumps, hop_t: 0.0, hop_from: [0.0; 3], next: 1, best: f32::INFINITY, stalled: 0, replans: 0 });
                    a.status = STATUS_WALKING;
                }
                Err(_) => {
                    state.paths.remove(&uid);
                    a.status = STATUS_BLOCKED;
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
                    a.status = STATUS_ARRIVED;
                    state.paths.remove(&uid);
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
                    if path.stalled >= STALL_TICKS {
                        let replans = path.replans + 1;
                        state.paths.remove(&uid);
                        match (replans <= MAX_REPLANS).then(|| find_path_as(world, feet, a.target, params_of(&a), fields, meshes, who_plans(&layers, uid))) {
                            Some(Ok(r)) => {
                                state.paths.insert(uid, AgentPath { points: r.points, jumps: r.jumps, hop_t: 0.0, hop_from: [0.0; 3], next: 1, best: f32::INFINITY, stalled: 0, replans });
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

