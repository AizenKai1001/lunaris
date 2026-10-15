//! Water that follows a heightmap (`WaterBody`, docs/TERRAIN.md "Water bodies"): rivers and lakes at different levels from one 16-bit picture, where 0 is dry land and any other value is the height
//! of the water's surface (`value / 65535 * relief` metres above the entity). This file is the geometry both the renderers and the simulation use (the surface under a point), what the engine does
//! with it every tick (`InWater` on characters that stand in water, `Buoyant` bodies that float, the waves' clock) and nothing else: the picture is the renderers' business.
//!
//! The surface is a grid like the terrain's: a vertex at every `step`-th sample, two triangles to a cell with the same diagonal (from the cell's north-east to its south-west corner). A cell is
//! water when any of its four corners is wet; a dry corner of such a cell takes the average height of the wet samples around it, so the sheet runs on to the middle of the shore cell and the ground
//! (which is higher there) hides the rest. Everything is plain `+ - * /` and `sqrt`, in a fixed order, so every machine finds the same water.
use crate::components::{Buoyant, CharacterBody, Collider, InWater, PhysicsSettings, RigidBody, Terrain, Transform, Velocity, WaterBody};
use crate::terrain::{Heightfield, Placed, Terrains};
use lunaris_ecs::World;

pub type V3 = [f32; 3];

/// One water body placed in the world: where it is and how its picture is used.
#[derive(Clone, Copy)]
pub struct PlacedWater<'a> {
    pub field: &'a Heightfield,
    /// The middle of the water's area (x, z).
    pub centre: [f32; 2],
    /// The height of a value of 0 (the entity's y).
    pub base: f32,
    pub size: [f32; 2],
    pub relief: f32,
    /// Cells across and down.
    pub nx: usize,
    pub nz: usize,
    step: usize,
}

/// Every water body of the world whose picture has been read, in uid order: (uid, its position, the component).
pub fn bodies(world: &mut World, fields: &Terrains) -> Vec<(u64, V3, WaterBody)> {
    if fields.is_empty() {
        return Vec::new();
    }
    let mut raw: Vec<(u64, V3, WaterBody)> = Vec::new();
    let _ = world.for_each_uid::<(&Transform, &WaterBody), _>(|_, uid, (t, w)| raw.push((uid, t.pos, *w)));
    raw.sort_by_key(|r| r.0);
    raw
}

impl<'a> PlacedWater<'a> {
    pub fn new(field: &'a Heightfield, pos: V3, w: &WaterBody) -> PlacedWater<'a> {
        // at most 2048 cells across, however big the picture (as for the terrain)
        let too_big = (field.cols.max(field.rows) - 1).div_ceil(2048).max(1);
        let step = (w.step.max(1) as usize).max(too_big).min(field.cols.max(field.rows));
        let cells = |n: usize| ((n - 1).div_ceil(step)).max(1);
        let size = [if w.size[0].is_finite() { w.size[0].abs().max(0.01) } else { 1.0 }, if w.size[1].is_finite() { w.size[1].abs().max(0.01) } else { 1.0 }];
        PlacedWater { field, centre: [pos[0], pos[2]], base: pos[1], size, relief: if w.relief.is_finite() { w.relief } else { 0.0 }, nx: cells(field.cols), nz: cells(field.rows), step }
    }

    fn cell(&self) -> [f32; 2] {
        [self.size[0] / self.nx as f32, self.size[1] / self.nz as f32]
    }

    fn corner(&self) -> [f32; 2] {
        [self.centre[0] - self.size[0] * 0.5, self.centre[1] - self.size[1] * 0.5]
    }

    /// The picture's value at vertex (i, j), 0 to 1 (0: dry).
    pub fn value(&self, i: usize, j: usize) -> f32 {
        self.field.sample(i * self.step, j * self.step)
    }

    /// Is there water at vertex (i, j) itself?
    pub fn wet(&self, i: usize, j: usize) -> bool {
        self.value(i, j) > 0.0
    }

    /// The height of the water's surface at vertex (i, j) in world y, `None` when no water is at or next to it. A dry vertex next to water takes the average of the wet ones around it (the eight
    /// neighbours).
    pub fn vertex_y(&self, i: usize, j: usize) -> Option<f32> {
        let v = self.value(i, j);
        if v > 0.0 {
            return Some(self.base + v * self.relief);
        }
        let (mut sum, mut n) = (0.0f32, 0u32);
        for dj in 0..3usize {
            for di in 0..3usize {
                // the neighbours (i - 1 ..= i + 1, j - 1 ..= j + 1) that are on the grid
                if (di == 1 && dj == 1) || i + di < 1 || j + dj < 1 || i + di - 1 > self.nx || j + dj - 1 > self.nz {
                    continue;
                }
                let w = self.value(i + di - 1, j + dj - 1);
                if w > 0.0 {
                    sum += w;
                    n += 1;
                }
            }
        }
        (n > 0).then(|| self.base + (sum / n as f32) * self.relief)
    }

    /// Does the cell (i, j) have water in it (any of its four corners wet)?
    pub fn cell_wet(&self, i: usize, j: usize) -> bool {
        self.wet(i, j) || self.wet(i + 1, j) || self.wet(i, j + 1) || self.wet(i + 1, j + 1)
    }

    /// World position of vertex (i, j) for the mesh: x and z on the grid, y the surface (a dry corner of a wet cell is extended, see above). `None` where the cell has no water.
    pub fn vertex(&self, i: usize, j: usize) -> Option<V3> {
        let (cell, c0) = (self.cell(), self.corner());
        Some([c0[0] + i as f32 * cell[0], self.vertex_y(i, j)?, c0[1] + j as f32 * cell[1]])
    }

    /// The cell under (x, z) and where in it (0 to 1 each way); `None` outside the area.
    fn locate(&self, x: f32, z: f32) -> Option<(usize, usize, f32, f32)> {
        let (cell, c0) = (self.cell(), self.corner());
        let (lx, lz) = ((x - c0[0]) / cell[0], (z - c0[1]) / cell[1]);
        if !(lx >= 0.0 && lz >= 0.0 && lx <= self.nx as f32 && lz <= self.nz as f32) {
            return None;
        }
        let i = (lx.floor() as usize).min(self.nx - 1);
        let j = (lz.floor() as usize).min(self.nz - 1);
        Some((i, j, lx - i as f32, lz - j as f32))
    }

    /// The height of the water's surface at (x, z) in world y: `None` where there is no water (outside the area, or in a cell with no wet corner).
    pub fn surface(&self, x: f32, z: f32) -> Option<f32> {
        let (i, j, fx, fz) = self.locate(x, z)?;
        if !self.cell_wet(i, j) {
            return None;
        }
        let (y00, y10, y01, y11) = (self.vertex_y(i, j)?, self.vertex_y(i + 1, j)?, self.vertex_y(i, j + 1)?, self.vertex_y(i + 1, j + 1)?);
        Some(if fx + fz <= 1.0 { y00 + fx * (y10 - y00) + fz * (y01 - y00) } else { y11 + (1.0 - fx) * (y01 - y11) + (1.0 - fz) * (y10 - y11) })
    }
}

/// The highest water surface at (x, z) among the water bodies of the world, with the uid of its body.
pub fn surface_at(list: &[(u64, V3, WaterBody)], fields: &Terrains, strings: &lunaris_ecs::StringTable, x: f32, z: f32) -> Option<(f32, u64)> {
    let mut best: Option<(f32, u64)> = None;
    for (uid, pos, w) in list {
        let Some(field) = fields.get(strings.get(w.heightmap.0)) else { continue };
        if let Some(y) = PlacedWater::new(field, *pos, w).surface(x, z) {
            if best.is_none_or(|b| y > b.0) {
                best = Some((y, *uid));
            }
        }
    }
    best
}

/// The height of the ground (the highest solid terrain) at (x, z), when there is one.
pub fn ground_at(world: &mut World, fields: &Terrains, x: f32, z: f32) -> Option<f32> {
    let mut best: Option<f32> = None;
    let mut list: Vec<(V3, Terrain, u32)> = Vec::new();
    let _ = world.for_each_uid::<(&Transform, &Terrain), _>(|_, _, (t, g)| list.push((t.pos, *g, g.heightmap.0)));
    for (pos, g, sym) in list {
        if !g.solid {
            continue;
        }
        let Some(field) = fields.get(world.strings().get(sym)) else { continue };
        if let Some(h) = Placed::new(field, pos, &g).surface(x, z) {
            if best.is_none_or(|b| h > b) {
                best = Some(h);
            }
        }
    }
    best
}

/// The clock of the waves, `InWater` on the characters that stand in water, and the floating of `Buoyant` bodies. Called once a tick (the buoyancy part before the physics step, the rest after
/// the characters have walked); each part does nothing, and costs nothing, in a world without the components it works on.
pub struct Water;

impl Water {
    /// The waves of every `WaterBody` run with the simulation (and stop when the world is paused).
    pub fn waves(world: &mut World, dt: f32) {
        let _ = world.for_each::<(&mut WaterBody,), _>(|_, (w,)| {
            if w.speed.is_finite() {
                w.phase = (w.phase + w.speed * dt).rem_euclid(6283.0);
            }
        });
    }

    /// Put `InWater` on every character whose feet are under the water's surface (with how deep, the surface's height and which body) and take it off the ones that are not. Works on the
    /// characters in uid order, so what it writes never depends on how they are stored.
    pub fn swimmers(world: &mut World, fields: &Terrains) {
        let mut list = bodies(world, fields);
        let any_water = !list.is_empty();
        let mut had: Vec<u64> = Vec::new();
        let _ = world.for_each_uid::<(&InWater,), _>(|_, uid, _| had.push(uid));
        if !any_water && had.is_empty() {
            return;
        }
        let mut walkers: Vec<(u64, V3)> = Vec::new();
        if any_water {
            let _ = world.for_each_uid::<(&CharacterBody, &Transform), _>(|_, uid, (_, t)| walkers.push((uid, t.pos)));
            walkers.sort_by_key(|w| w.0);
        }
        list.sort_by_key(|b| b.0);
        let mut now: Vec<(u64, InWater)> = Vec::new();
        for (uid, pos) in &walkers {
            if let Some((y, body)) = surface_at(&list, fields, world.strings(), pos[0], pos[2]) {
                let depth = y - pos[1];
                if depth > 0.0 {
                    now.push((*uid, InWater { depth, surface: y, body: lunaris_ecs::Uid(body) }));
                }
            }
        }
        // the ones that are no longer in water (or are no longer characters)
        for uid in had {
            if !now.iter().any(|n| n.0 == uid) {
                if let Some(e) = world.entity(uid) {
                    let _ = world.remove_typed::<InWater>(e);
                }
            }
        }
        for (uid, state) in now {
            if let Some(e) = world.entity(uid) {
                match world.get_mut::<InWater>(e) {
                    Some(c) => *c = state,
                    None => {
                        let _ = world.insert(e, state);
                    }
                }
            }
        }
    }

    /// Float the `Buoyant` bodies: an upward acceleration of gravity times how much heavier water is than the body (1000 / `density`) times the share of the body that is under the surface,
    /// and water drag on its velocity (and spin) by the same share. Run before the physics step, which integrates the velocity.
    pub fn buoyancy(world: &mut World, fields: &Terrains, dt: f32) {
        let mut bobbers: Vec<(u64, Transform, Collider, Buoyant, f32)> = Vec::new();
        let _ = world.for_each_uid::<(&Buoyant, &Transform, &Collider, &RigidBody, &Velocity), _>(|_, uid, (b, t, c, rb, _)| {
            if rb.mass > 0.0 && !c.trigger {
                bobbers.push((uid, *t, *c, *b, rb.gravity_scale));
            }
        });
        if bobbers.is_empty() {
            return;
        }
        bobbers.sort_by_key(|b| b.0);
        let list = bodies(world, fields);
        let mut gravity = [0.0f32, -9.8, 0.0];
        let mut lowest = u64::MAX;
        let _ = world.for_each_uid::<(&PhysicsSettings,), _>(|_, uid, (p,)| {
            if uid < lowest {
                lowest = uid;
                gravity = p.gravity;
            }
        });
        for (uid, t, c, b, gravity_scale) in bobbers {
            let Some(e) = world.entity(uid) else { continue };
            let surface = surface_at(&list, fields, world.strings(), t.pos[0], t.pos[2]).map(|s| s.0);
            let share = surface.map_or(0.0, |s| submerged(&t, &c, s));
            if let Some(buoyant) = world.get_mut::<Buoyant>(e) {
                buoyant.submerged = share;
            }
            if share <= 0.0 || !share.is_finite() {
                continue;
            }
            let density = if b.density.is_finite() && b.density > 1.0 { b.density } else { 1.0 };
            // gravity is a vector: the water pushes the other way, harder the lighter the body is for its size
            let up = (1000.0 / density) * share * gravity_scale;
            let damp = 1.0 / (1.0 + b.drag.max(0.0) * share * dt);
            if let Some(v) = world.get_mut::<Velocity>(e) {
                for k in 0..3 {
                    v.lin[k] = (v.lin[k] - gravity[k] * up * dt) * damp;
                }
            }
            if let Some(a) = world.get_mut::<crate::components::AngularVelocity>(e) {
                for k in 0..3 {
                    a.w[k] *= damp;
                }
            }
        }
    }
}

/// How much of a body (0 to 1) is under a surface at height `surface`: the share of a sphere's volume below it (a spherical cap), or of a box's, a capsule's or any other shape's height.
pub fn submerged(t: &Transform, c: &Collider, surface: f32) -> f32 {
    let s = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
    match c.shape {
        0 => {
            let r = (c.size[0] * 0.5 * s[0].max(s[1]).max(s[2])).max(1.0e-4);
            let d = (surface - (t.pos[1] - r)).clamp(0.0, 2.0 * r);
            // the volume of a cap of depth d over the volume of the ball
            (d * d * (3.0 * r - d) / (4.0 * r * r * r)).clamp(0.0, 1.0)
        }
        _ => {
            // the height of the body as it stands: a turned box reaches as far up as its three axes allow
            let half = if c.shape == 1 {
                let q = t.rot;
                let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
                // the y components of the box's three axes (the second row of the rotation matrix)
                let row = [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)];
                (0..3).map(|k| row[k].abs() * c.size[k] * 0.5 * s[k]).sum::<f32>()
            } else {
                c.size[1] * 0.5 * s[1]
            }
            .max(1.0e-4);
            ((surface - (t.pos[1] - half)) / (2.0 * half)).clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 5 x 5 picture: dry (0) except a lake of three samples in a row at level 0.5 and a stream sample at 0.25.
    fn lake() -> Heightfield {
        let mut s = vec![0u16; 25];
        for i in 1..4 {
            s[2 * 5 + i] = 32768; // row 2, columns 1 to 3
        }
        s[4 * 5 + 4] = 16384;
        Heightfield::new(5, 5, s).unwrap()
    }

    fn placed(f: &Heightfield) -> PlacedWater<'_> {
        // 40 x 40 metres, 20 metres of relief, base 1 m, cells of 10 m
        let w = WaterBody { size: [40.0, 40.0], relief: 20.0, step: 1, ..WaterBody::default() };
        PlacedWater::new(f, [0.0, 1.0, 0.0], &w)
    }

    #[test]
    fn the_surface_is_where_the_picture_has_water_and_dry_corners_are_extended_to_the_shore() {
        let f = lake();
        let p = placed(&f);
        let level = 1.0 + 0.5000076 * 20.0;
        // over the lake's samples
        assert!((p.surface(0.0, 0.0).unwrap() - level).abs() < 0.01, "{:?}", p.surface(0.0, 0.0));
        assert!((p.surface(-10.0, 0.0).unwrap() - level).abs() < 0.01);
        // between the lake and the dry ground the cell is still water (its dry corner takes the lake's height), so the sheet reaches the middle of the shore cell
        assert!((p.surface(-15.0, 0.0).unwrap() - level).abs() < 0.01);
        // far from any water: none, and none outside the area
        assert_eq!(p.surface(-18.0, -18.0), None);
        assert_eq!(p.surface(0.0, 25.0), None);
        assert_eq!(p.surface(f32::NAN, 0.0), None);
        // the lone stream sample at the south-east corner is at its own, lower level (a little over it: the dry corners next to it also take the lake's height into their average)
        let s = p.surface(19.0, 19.0).unwrap();
        assert!(s < level - 4.0 && (s - (1.0 + 0.25 * 20.0)).abs() < 0.3, "{s}");
        assert!((p.vertex(4, 4).unwrap()[1] - 6.0).abs() < 0.01);
        // vertices: wet and dry, and a vertex far from water has none
        assert!(p.wet(2, 2) && !p.wet(0, 0));
        assert!(p.vertex(2, 2).is_some() && p.vertex(0, 0).is_none());
        let shore = p.vertex(0, 2).unwrap();
        assert!((shore[1] - level).abs() < 0.01 && shore[0] == -20.0 && shore[2] == 0.0, "{shore:?}");
    }

    #[test]
    fn a_coarser_step_uses_fewer_cells_and_odd_numbers_never_panic() {
        let f = lake();
        let w = WaterBody { size: [40.0, 40.0], relief: 20.0, step: 2, ..WaterBody::default() };
        let p = PlacedWater::new(&f, [0.0, 0.0, 0.0], &w);
        assert_eq!((p.nx, p.nz), (2, 2));
        for size in [[0.0f32, 0.0], [f32::NAN, 5.0], [1e30, 1e30]] {
            for relief in [0.0f32, -5.0, f32::INFINITY, f32::NAN] {
                let w = WaterBody { size, relief, step: 3, ..WaterBody::default() };
                let p = PlacedWater::new(&f, [1.0, 2.0, 3.0], &w);
                let _ = p.surface(0.0, 0.0);
                let _ = p.vertex(1, 1);
                let _ = p.vertex(9, 9);
            }
        }
    }

    #[test]
    fn how_much_of_a_body_is_under_water() {
        let t = |y: f32| Transform { pos: [0.0, y, 0.0], ..Transform::default() };
        let ball = Collider { shape: 0, size: [2.0, 2.0, 2.0], trigger: false }; // radius 1
        assert_eq!(submerged(&t(5.0), &ball, 0.0), 0.0);
        assert!((submerged(&t(0.0), &ball, 0.0) - 0.5).abs() < 1e-6, "half under when the surface is at its middle");
        assert_eq!(submerged(&t(-5.0), &ball, 0.0), 1.0);
        // a quarter of the depth is much less than a quarter of the volume (a cap): 0.5 m deep of a 2 m ball
        let cap = submerged(&t(0.5), &ball, 0.0);
        assert!(cap > 0.1 && cap < 0.2, "{cap}");
        let cube = Collider { shape: 1, size: [1.0, 1.0, 1.0], trigger: false };
        assert!((submerged(&t(0.25), &cube, 0.0) - 0.25).abs() < 1e-6);
        // a cube turned 45 degrees about z is taller than it was: its corner reaches 0.707 up
        let turned = Transform { rot: [0.0, 0.0, 0.38268343, 0.9238795], ..t(0.0) };
        assert!((submerged(&turned, &cube, 0.0) - 0.5).abs() < 1e-5);
        assert!((submerged(&Transform { pos: [0.0, -0.5, 0.0], ..turned }, &cube, 0.0) - (0.5 + 0.5 / 1.4142135)).abs() < 1e-4);
    }
}
