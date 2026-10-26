//! The inside of a building: floor slabs with a stairwell and an elevator shaft, a corridor, rooms with partitions and doors that stand ajar, and furniture, fittings and plants in every room, floor by floor.
//!
//! Everything is laid out in the building's own plan (x along the front, z into the lot) inside the outer walls. A floor is a tiling of rectangles (front rooms, a corridor, a stair hall, an elevator shaft, rooms at the
//! back), each a room of some kind with its own floor material and furniture. The furniture comes from real models (`props`); what no model exists for (kitchen units, bathroom fittings, counters, beds frames)
//! is built from boxes in the same real materials.

use crate::facade::{Palette, WALL_T};
use crate::materials::Lib;
use crate::mesh::*;
use crate::props::{Prop, Props};
use crate::rng::Rng;

pub const SLAB: f32 = 0.30;
const PART_T: f32 = 0.14;
const DOOR_W: f32 = 0.95;
const DOOR_H: f32 = 2.1;

/// The building's plan: world position of the lot corner, the unit vectors along the front and into the lot, and the outer rectangle in lot coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Foot {
    pub o: V3,
    pub ex: V3,
    pub ez: V3,
    pub x0: f32,
    pub x1: f32,
    pub z0: f32,
    pub z1: f32,
}

impl Foot {
    /// The world point of lot coordinates (x, z) at the absolute height `y`.
    pub fn at(&self, x: f32, z: f32, y: f32) -> V3 {
        [self.o[0] + self.ex[0] * x + self.ez[0] * z, y, self.o[2] + self.ex[2] * x + self.ez[2] * z]
    }

    /// A frame in which (s, h, d) is (x, y, z): cuboids in lot coordinates and absolute heights.
    pub fn frame(&self) -> Frame {
        Frame { o: [self.o[0], 0.0, self.o[2]], a: self.ex, b: [0.0, 1.0, 0.0], c: self.ez }
    }
}

/// The floors of a volume: `y0` is the height of the ground floor's surface, floor 0 is `ground_h` tall, the others `floor_h`.
#[derive(Clone, Copy, Debug)]
pub struct Levels {
    pub y0: f32,
    pub floors: u32,
    pub ground_h: f32,
    pub floor_h: f32,
}

impl Levels {
    /// The height of the middle of floor k.
    pub fn mid(&self, k: u32) -> f32 {
        (self.line(k) + self.line(k + 1)) * 0.5
    }

    /// The height of floor line k: the surface of floor k (k = 0 the ground floor); line `floors` is the roof.
    pub fn line(&self, k: u32) -> f32 {
        if k == 0 {
            self.y0
        } else {
            self.y0 + self.ground_h + (k - 1) as f32 * self.floor_h
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Occupancy {
    Retail,
    Lobby,
    Office,
    Apartments,
    /// A house: the rooms of an apartment, one home across the whole width, and no lift shaft.
    Home,
    /// A warehouse: one open hall with shelving, and an office in a corner.
    Warehouse,
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x0: f32,
    pub x1: f32,
    pub z0: f32,
    pub z1: f32,
}

impl Rect {
    pub fn new(x0: f32, x1: f32, z0: f32, z1: f32) -> Rect {
        Rect { x0, x1, z0, z1 }
    }
    fn w(&self) -> f32 {
        self.x1 - self.x0
    }
    fn d(&self) -> f32 {
        self.z1 - self.z0
    }
    fn cx(&self) -> f32 {
        (self.x0 + self.x1) * 0.5
    }
    fn cz(&self) -> f32 {
        (self.z0 + self.z1) * 0.5
    }
    fn overlaps(&self, o: &Rect) -> bool {
        self.x0 < o.x1 - 0.02 && self.x1 > o.x0 + 0.02 && self.z0 < o.z1 - 0.02 && self.z1 > o.z0 + 0.02
    }
    fn ok(&self) -> bool {
        self.w() > 0.02 && self.d() > 0.02
    }
}

/// The pieces of a rectangle left when a hole inside it is cut out.
fn minus(r: Rect, h: Rect) -> Vec<Rect> {
    let mut out = vec![Rect::new(r.x0, r.x1, r.z0, h.z0), Rect::new(r.x0, r.x1, h.z1, r.z1), Rect::new(r.x0, h.x0, h.z0, h.z1), Rect::new(h.x1, r.x1, h.z0, h.z1)];
    out.retain(|p| p.ok());
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    /// The wall runs along x (it has a constant z).
    X,
    Z,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Shop,
    Lobby,
    Office,
    Meeting,
    Living,
    Bedroom,
    Kitchen,
    Bath,
    Storage,
    Warehouse,
    Corridor,
    Stair,
    Shaft,
}

#[derive(Clone, Debug)]
struct Room {
    rect: Rect,
    kind: Kind,
}

#[derive(Clone, Debug)]
struct Wall {
    axis: Axis,
    pos: f32,
    a0: f32,
    a1: f32,
    thick: f32,
    /// Openings along the wall, as (from, to) along its length.
    doors: Vec<(f32, f32)>,
    /// Doors in this wall that get a leaf standing ajar.
    leaves: bool,
}

/// What a plan holds: the rooms and walls of one floor, and where the stairs are.
struct Plan {
    inner: Rect,
    hall: Rect,
    shaft: Option<Rect>,
    rooms: Vec<Room>,
    walls: Vec<Wall>,
}

/// A light inside: where it is and how far it reaches.
#[derive(Clone, Copy, Debug)]
pub struct InLight {
    pub pos: V3,
    pub range: f32,
    pub intensity: f32,
    /// The floor it lights (counted from the ground floor of the volume it belongs to).
    pub floor: u32,
}

/// The inside of a building: one builder per floor (its slab, partitions, stairs up and furniture) and the lights.
pub struct Interior {
    pub floors: Vec<MeshBuilder>,
    pub lights: Vec<InLight>,
}

/// The structure shared by every floor: the stair hall and shaft at the back, a corridor in front of them, the zone in front of that.
struct Structure {
    inner: Rect,
    hall: Rect,
    shaft: Option<Rect>,
    corridor: Option<Rect>,
    front: Rect,
    back_l: Option<Rect>,
    back_r: Option<Rect>,
}

fn structure(rng: &mut Rng, foot: &Foot, lift: bool) -> Option<Structure> {
    let t = WALL_T;
    let inner = Rect::new(foot.x0 + t, foot.x1 - t, foot.z0 + t, foot.z1 - t);
    let (wi, di) = (inner.w(), inner.d());
    if wi < 4.8 || di < 10.0 {
        return None;
    }
    let cd = 5.8f32.min(di * 0.42);
    let hall_w = 2.9;
    let with_shaft = lift && wi >= 9.5;
    let shaft_w = 2.4;
    let core_w = hall_w + if with_shaft { shaft_w } else { 0.0 };
    let corridor_w = if di - cd > 9.0 { 1.8 } else { 0.0 };
    let cx0 = if wi - core_w - 2.0 > 2.0 { rng.range(inner.x0 + 1.0, inner.x1 - core_w - 1.0) } else { inner.x0 };
    let cx0 = (cx0 * 2.0).round() / 2.0;
    let z_hall = inner.z1 - cd;
    let hall = Rect::new(cx0, cx0 + hall_w, z_hall, inner.z1);
    let shaft = with_shaft.then(|| Rect::new(hall.x1, hall.x1 + shaft_w, z_hall, inner.z1));
    let zc0 = z_hall - corridor_w;
    let corridor = (corridor_w > 0.0).then(|| Rect::new(inner.x0, inner.x1, zc0, z_hall));
    let front = Rect::new(inner.x0, inner.x1, inner.z0, zc0);
    let core_x1 = shaft.map_or(hall.x1, |s| s.x1);
    let back_l = (cx0 - inner.x0 > 1.6).then(|| Rect::new(inner.x0, cx0, z_hall, inner.z1));
    let back_r = (inner.x1 - core_x1 > 1.6).then(|| Rect::new(core_x1, inner.x1, z_hall, inner.z1));
    Some(Structure { inner, hall, shaft, corridor, front, back_l, back_r })
}

/// A partition along x at `z` from `a0` to `a1` with door openings (centred at the positions given).
fn wall_x(z: f32, a0: f32, a1: f32, doors: &[f32], thick: f32, leaves: bool) -> Wall {
    Wall { axis: Axis::X, pos: z, a0, a1, thick, doors: doors.iter().map(|c| (c - DOOR_W * 0.5, c + DOOR_W * 0.5)).collect(), leaves }
}

fn wall_z(x: f32, a0: f32, a1: f32, doors: &[f32], thick: f32, leaves: bool) -> Wall {
    Wall { axis: Axis::Z, pos: x, a0, a1, thick, doors: doors.iter().map(|c| (c - DOOR_W * 0.5, c + DOOR_W * 0.5)).collect(), leaves }
}

/// The rooms and walls of floor `k`.
fn plan_floor(rng: &mut Rng, st: &Structure, occ: Occupancy, k: u32, lv: &Levels) -> Plan {
    let mut rooms: Vec<Room> = Vec::new();
    let mut walls: Vec<Wall> = Vec::new();
    let i = st.inner;
    let hall_c = st.hall.cx();
    let top = k + 1 >= lv.floors;
    let _ = top;
    // the stair hall and the shaft (their walls are thick, the hall is open to the corridor by a door)
    rooms.push(Room { rect: st.hall, kind: Kind::Stair });
    let z_h = st.hall.z0;
    // the front wall of the core: the hall's door, the shaft's closed doors (a leaf only on the hall)
    walls.push(wall_x(z_h, st.hall.x0, st.hall.x1, &[hall_c], 0.2, true));
    walls.push(wall_z(st.hall.x0, st.hall.z0, st.hall.z1, &[], 0.2, false));
    walls.push(wall_z(st.hall.x1, st.hall.z0, st.hall.z1, &[], 0.2, false));
    if let Some(s) = st.shaft {
        rooms.push(Room { rect: s, kind: Kind::Shaft });
        walls.push(wall_x(z_h, s.x0, s.x1, &[], 0.2, false));
        walls.push(wall_z(s.x1, s.z0, s.z1, &[], 0.2, false));
    }
    // the corridor
    if let Some(c) = st.corridor {
        rooms.push(Room { rect: c, kind: Kind::Corridor });
    }
    // the rooms at the back, either side of the core: each opens on the corridor (or the front zone)
    let back_side = |rng: &mut Rng, r: Rect, kind_for: Kind, rooms: &mut Vec<Room>, walls: &mut Vec<Wall>| {
        rooms.push(Room { rect: r, kind: kind_for });
        let door = (r.x0 + r.x1) * 0.5;
        walls.push(wall_x(r.z0, r.x0, r.x1, &[door], PART_T, true));
        let _ = rng;
    };
    let back_kind = |occ: Occupancy, rng: &mut Rng, r: &Rect| match occ {
        Occupancy::Apartments | Occupancy::Home => {
            if r.w() < 3.2 {
                Kind::Bath
            } else if rng.chance(0.5) {
                Kind::Bedroom
            } else {
                Kind::Bath
            }
        }
        Occupancy::Office => {
            if r.w() > 4.5 {
                Kind::Meeting
            } else {
                Kind::Storage
            }
        }
        _ => Kind::Storage,
    };
    // walls beside the core separate it from these rooms
    if let Some(r) = st.back_l {
        let kind = back_kind(occ, rng, &r);
        back_side(rng, r, kind, &mut rooms, &mut walls);
        walls.push(wall_z(r.x1, r.z0, r.z1, &[], PART_T, false));
    }
    if let Some(r) = st.back_r {
        let kind = back_kind(occ, rng, &r);
        back_side(rng, r, kind, &mut rooms, &mut walls);
        walls.push(wall_z(r.x0, r.z0, r.z1, &[], PART_T, false));
    }
    // the front zone
    let f = st.front;
    match (occ, k) {
        (Occupancy::Retail, 0) | (Occupancy::Lobby, 0) => {
            // one open room across the front, with a wide opening to the corridor in front of the hall and doors to the others
            let kind = if occ == Occupancy::Retail { Kind::Shop } else { Kind::Lobby };
            rooms.push(Room { rect: f, kind });
            if let Some(c) = st.corridor {
                let mut doors = vec![hall_c];
                for r in [st.back_l, st.back_r].into_iter().flatten() {
                    doors.push((r.x0 + r.x1) * 0.5);
                }
                walls.push(Wall { axis: Axis::X, pos: c.z0, a0: i.x0, a1: i.x1, thick: PART_T, doors: doors.iter().map(|d| (d - 1.1, d + 1.1)).collect(), leaves: false });
            }
        }
        (Occupancy::Office, _) | (Occupancy::Retail, _) | (Occupancy::Lobby, _) | (Occupancy::Warehouse, _) => {
            // an open floor (one room) with a door to the corridor at each end of it and in front of the hall
            rooms.push(Room { rect: f, kind: Kind::Office });
            if let Some(c) = st.corridor {
                let doors = [hall_c, i.x0 + 2.0, i.x1 - 2.0];
                let ds: Vec<f32> = doors.iter().copied().filter(|d| *d > i.x0 + 1.0 && *d < i.x1 - 1.0).collect();
                walls.push(Wall { axis: Axis::X, pos: c.z0, a0: i.x0, a1: i.x1, thick: PART_T, doors: ds.iter().map(|d| (d - 1.1, d + 1.1)).collect(), leaves: false });
            }
        }
        (Occupancy::Apartments | Occupancy::Home, _) => {
            // flats side by side across the front: each a living room with a kitchen corner and a bedroom behind it (a house is one flat wide)
            let n = ((f.w() / if occ == Occupancy::Home { 11.0 } else { 6.0 }).round() as i32).max(1);
            let uw = f.w() / n as f32;
            let mut corridor_doors = Vec::new();
            for u in 0..n {
                let (ux0, ux1) = (f.x0 + u as f32 * uw, f.x0 + (u + 1) as f32 * uw);
                let living_d = (f.d() * 0.55).clamp(3.5, 7.0);
                let living = Rect::new(ux0, ux1, f.z0, f.z0 + living_d);
                let rest = Rect::new(ux0, ux1, f.z0 + living_d, f.z1);
                rooms.push(Room { rect: living, kind: Kind::Living });
                if rest.d() > 3.0 {
                    // behind the living room: a kitchen (towards the corridor) and a bedroom
                    let half = rest.w() * 0.5;
                    let (bed, kit) = if rng.chance(0.5) { (Rect::new(ux0, ux0 + half, rest.z0, rest.z1), Rect::new(ux0 + half, ux1, rest.z0, rest.z1)) } else { (Rect::new(ux0 + half, ux1, rest.z0, rest.z1), Rect::new(ux0, ux0 + half, rest.z0, rest.z1)) };
                    rooms.push(Room { rect: bed, kind: Kind::Bedroom });
                    rooms.push(Room { rect: kit, kind: Kind::Kitchen });
                    // the living room opens on both rooms; the kitchen and bedroom meet at a wall; the flat's front door is in the kitchen's corridor wall
                    walls.push(wall_x(rest.z0, ux0, ux1, &[bed.cx(), kit.cx()], PART_T, true));
                    walls.push(wall_z(ux0 + half, rest.z0, rest.z1, &[], PART_T, false));
                    corridor_doors.push(kit.cx());
                } else {
                    corridor_doors.push(living.cx());
                }
                if u > 0 {
                    walls.push(wall_z(ux0, f.z0, f.z1, &[], PART_T, false));
                }
            }
            if st.corridor.is_some() {
                walls.push(wall_x(f.z1, i.x0, i.x1, &corridor_doors, PART_T, true));
            }
        }
    }
    // when there is no corridor, the front zone runs up to the hall: the hall's door opens on it
    if st.corridor.is_none() {
        // the wall of the hall (above) already has its door; nothing more to do
    }
    Plan { inner: st.inner, hall: st.hall, shaft: st.shaft, rooms, walls }
}

// ------------------------------------------------------------------------------------------------ materials of a floor

/// The floor material of a room kind (a photographed set where one is in the library).
fn floor_mat(lib: &Lib, kind: Kind, pal: &Palette, rng_pick: f32) -> (usize, Tex) {
    let list: &[&str] = match kind {
        Kind::Shop | Kind::Lobby => &["floor_tiles_08", "terrazzo_tiles", "marble_01", "floor_tiles_06"],
        Kind::Office | Kind::Meeting => &["rubber_tiles", "linoleum_brown", "laminate_floor_03"],
        Kind::Living | Kind::Bedroom => &["laminate_floor_02", "wood_floor", "herringbone_parquet", "laminate_floor_03"],
        Kind::Kitchen | Kind::Bath => &["floor_tiles_06", "floor_tiles_08", "terrazzo_tiles"],
        Kind::Corridor => &["rubber_tiles", "floor_tiles_08", "terrazzo_tiles"],
        Kind::Stair | Kind::Shaft | Kind::Storage | Kind::Warehouse => &["concrete_floor_03", "concrete_tiles_02"],
    };
    let present: Vec<&str> = list.iter().copied().filter(|n| lib.has(n)).collect();
    let id = if present.is_empty() { pal.floor } else { lib.id(present[((rng_pick * present.len() as f32) as usize).min(present.len() - 1)]) };
    (id, lib.tex_plain(id))
}

// ------------------------------------------------------------------------------------------------ building a floor

struct Ctx<'a> {
    mb: MeshBuilder,
    rng: Rng,
    lib: &'a Lib,
    props: &'a Props,
    pal: &'a Palette,
    foot: Foot,
    k: u32,
    y: f32,
    ceil: f32,
    occupied: Vec<Rect>,
    lights: Vec<InLight>,
    paint: [usize; 4],
}

impl Ctx<'_> {
    fn fr(&self) -> Frame {
        self.foot.frame()
    }

    /// A horizontal rectangle (lot coordinates) at height `y` facing up or down.
    #[allow(clippy::too_many_arguments)]
    fn flat(&mut self, mat: usize, r: Rect, y: f32, tex: &Tex, tint: V3, up: bool) {
        if !r.ok() {
            return;
        }
        let p = [self.foot.at(r.x0, r.z0, y), self.foot.at(r.x1, r.z0, y), self.foot.at(r.x1, r.z1, y), self.foot.at(r.x0, r.z1, y)];
        let m = [[r.x0, r.z0], [r.x1, r.z0], [r.x1, r.z1], [r.x0, r.z1]];
        // pictures are laid in the plane by lot coordinates, so a floor in one room goes on into the next
        self.mb.quad_tex(mat, p, m, tex, tint, [0.0, if up { 1.0 } else { -1.0 }, 0.0]);
    }

    fn cuboid(&mut self, mat: usize, lo: V3, hi: V3, faces: u8, tint: V3) {
        let fr = self.fr();
        let tex = self.lib.tex_plain(mat);
        self.mb.cuboid(mat, &fr, lo, hi, faces, &tex, tint);
    }

    fn box_in(&mut self, mat: usize, x0: f32, x1: f32, z0: f32, z1: f32, h0: f32, h1: f32, tint: V3) {
        let yy = self.y;
        self.cuboid(mat, [x0, yy + h0, z0], [x1, yy + h1, z1], 63, tint);
        self.occupied.push(Rect::new(x0, x1, z0, z1));
    }

    /// A model placed with its base at (x, z) on this floor, its front (model +z) turned to face `front` (lot x, z).
    fn put(&mut self, prop: &Prop, x: f32, z: f32, front: (f32, f32), height: f32, scale_by: f32) {
        let dir = add(scale(self.foot.ex, front.0), scale(self.foot.ez, front.1));
        let yaw = dir[0].atan2(dir[2]).to_degrees();
        let pos = self.foot.at(x, z, self.y + height);
        prop.place(&mut self.mb, pos, yaw, scale_by, [1.0; 3]);
        if let Some(kind) = container_kind(&prop.name) {
            let s = prop.size();
            self.mb.containers.push([pos[0], pos[1], pos[2], yaw, s[0] * scale_by, s[1] * scale_by, s[2] * scale_by, kind as f32]);
        }
    }

    /// Footprint of a prop turned to face `front`.
    fn footprint(prop: &Prop, x: f32, z: f32, front: (f32, f32), scale_by: f32) -> Rect {
        let s = prop.size();
        let (w, d) = if front.0.abs() > 0.5 { (s[2] * scale_by, s[0] * scale_by) } else { (s[0] * scale_by, s[2] * scale_by) };
        Rect::new(x - w * 0.5, x + w * 0.5, z - d * 0.5, z + d * 0.5)
    }

    /// Place a model if it fits in `room` and clears the other furniture and the doors; returns whether it did.
    fn try_put(&mut self, slugs: &[&str], room: Rect, x: f32, z: f32, front: (f32, f32), height: f32, doors: &[Rect]) -> bool {
        let present: Vec<&str> = slugs.iter().copied().filter(|s| self.props.get(s).is_some()).collect();
        if present.is_empty() {
            return false;
        }
        let slug = present[(self.rng.u64() % present.len() as u64) as usize];
        let prop = self.props.get(slug).unwrap();
        let s = prop.size();
        // models that are not furniture-sized (a scanned set of shelves of 20 m) are left out
        if s[0] > 4.0 || s[1] > 3.0 || s[2] > 4.0 {
            return false;
        }
        let fp = Self::footprint(prop, x, z, front, 1.0);
        let inside = fp.x0 >= room.x0 - 0.02 && fp.x1 <= room.x1 + 0.02 && fp.z0 >= room.z0 - 0.02 && fp.z1 <= room.z1 + 0.02;
        if !inside || self.occupied.iter().any(|o| o.overlaps(&fp)) || doors.iter().any(|d| d.overlaps(&fp)) {
            return false;
        }
        let prop = prop.clone();
        self.put(&prop, x, z, front, height, 1.0);
        self.occupied.push(fp);
        true
    }

    /// Against a wall of `room`: `side` 0 = the wall at z0 (the model faces +z), 1 = at z1, 2 = at x0 (faces +x), 3 = at x1. `t` (0..1) is how far along the wall.
    fn against(&mut self, slugs: &[&str], room: Rect, side: u8, t: f32, gap: f32, height: f32, doors: &[Rect]) -> bool {
        let present: Vec<&str> = slugs.iter().copied().filter(|s| self.props.get(s).is_some()).collect();
        if present.is_empty() {
            return false;
        }
        let slug = present[(self.rng.u64() % present.len() as u64) as usize];
        let s = self.props.get(slug).unwrap().size();
        let (front, depth, along_len) = match side {
            0 => ((0.0, 1.0), s[2], room.w()),
            1 => ((0.0, -1.0), s[2], room.w()),
            2 => ((1.0, 0.0), s[2], room.d()),
            _ => ((-1.0, 0.0), s[2], room.d()),
        };
        let half_w = s[0] * 0.5;
        let a = (half_w + 0.1) + t.clamp(0.0, 1.0) * (along_len - 2.0 * half_w - 0.2).max(0.0);
        let (x, z) = match side {
            0 => (room.x0 + a, room.z0 + depth * 0.5 + gap),
            1 => (room.x0 + a, room.z1 - depth * 0.5 - gap),
            2 => (room.x0 + depth * 0.5 + gap, room.z0 + a),
            _ => (room.x1 - depth * 0.5 - gap, room.z0 + a),
        };
        self.try_put(&[slug], room, x, z, front, height, doors)
    }

    fn light(&mut self, x: f32, z: f32, range: f32, intensity: f32) {
        let p = self.foot.at(x, z, self.ceil - 0.25);
        self.lights.push(InLight { pos: p, range, intensity, floor: self.k });
    }
}

/// The rectangles in front of the doors of a room, which furniture must leave free.
fn door_zones(plan: &Plan, room: Rect) -> Vec<Rect> {
    let mut out = Vec::new();
    for w in &plan.walls {
        for (d0, d1) in &w.doors {
            let r = match w.axis {
                Axis::X => Rect::new(*d0 - 0.15, *d1 + 0.15, w.pos - 1.1, w.pos + 1.1),
                Axis::Z => Rect::new(w.pos - 1.1, w.pos + 1.1, *d0 - 0.15, *d1 + 0.15),
            };
            if r.overlaps(&Rect::new(room.x0 - 0.05, room.x1 + 0.05, room.z0 - 0.05, room.z1 + 0.05)) {
                out.push(r);
            }
        }
    }
    out
}

/// A wall: both faces, the ends and the lintel over each door, with a door leaf ajar in doorways that want one.
fn build_wall(cx: &mut Ctx, w: &Wall, paint_a: usize, paint_b: usize) {
    let y0 = cx.y;
    let y1 = cx.ceil;
    let t = w.thick * 0.5;
    let mut doors = w.doors.clone();
    doors.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cur = w.a0;
    let mut segs: Vec<(f32, f32)> = Vec::new();
    for (d0, d1) in &doors {
        let (d0, d1) = (d0.max(w.a0), d1.min(w.a1));
        if d1 <= d0 {
            continue;
        }
        if d0 > cur {
            segs.push((cur, d0));
        }
        cur = d1;
    }
    if cur < w.a1 {
        segs.push((cur, w.a1));
    }
    let tex = cx.lib.tex_plain(cx.pal.inner);
    let lo_hi = |axis: Axis, u0: f32, u1: f32, h0: f32, h1: f32| -> (V3, V3) {
        match axis {
            Axis::X => ([u0, y0 + h0, w.pos - t], [u1, y0 + h1, w.pos + t]),
            Axis::Z => ([w.pos - t, y0 + h0, u0], [w.pos + t, y0 + h1, u1]),
        }
    };
    let fr = cx.fr();
    let inner_mat = cx.pal.inner;
    // the faces of a box: +s=1 -s=2 +h=4 -h=8 +d=16 -d=32; the two big faces are the sides of the wall
    let (side_pos, side_neg) = match w.axis {
        Axis::X => (16u8, 32u8),
        Axis::Z => (1u8, 2u8),
    };
    let ends = match w.axis {
        Axis::X => 1u8 | 2,
        Axis::Z => 16u8 | 32,
    };
    let seg_boxes: Vec<(V3, V3)> = segs.iter().map(|(a, b)| lo_hi(w.axis, *a, *b, 0.0, y1 - y0)).collect();
    for (lo, hi) in seg_boxes {
        cx.mb.cuboid(inner_mat, &fr, lo, hi, side_pos, &tex, cx.paint_tint(paint_a));
        cx.mb.cuboid(inner_mat, &fr, lo, hi, side_neg, &tex, cx.paint_tint(paint_b));
        cx.mb.cuboid(inner_mat, &fr, lo, hi, ends, &tex, [0.8; 3]);
    }
    for (d0, d1) in &doors {
        let (d0, d1) = (d0.max(w.a0), d1.min(w.a1));
        if d1 <= d0 {
            continue;
        }
        // above the door
        let (lo, hi) = lo_hi(w.axis, d0, d1, DOOR_H, y1 - y0);
        cx.mb.cuboid(inner_mat, &fr, lo, hi, side_pos | side_neg | 8, &tex, [0.9; 3]);
        // door frame: two jambs and a head, proud of the wall a little on both sides
        let trim = cx.lib.id("frame_white");
        let tt = cx.lib.tex_plain(trim);
        let (jb, hd) = (0.05, 0.05);
        for (a0, a1, h0, h1) in [(d0 - jb, d0, 0.0, DOOR_H + hd), (d1, d1 + jb, 0.0, DOOR_H + hd), (d0 - jb, d1 + jb, DOOR_H, DOOR_H + hd)] {
            let (lo, hi) = match w.axis {
                Axis::X => ([a0, y0 + h0, w.pos - t - 0.012], [a1, y0 + h1, w.pos + t + 0.012]),
                Axis::Z => ([w.pos - t - 0.012, y0 + h0, a0], [w.pos + t + 0.012, y0 + h1, a1]),
            };
            cx.mb.cuboid(trim, &fr, lo, hi, 63, &tt, [1.0; 3]);
        }
        if w.leaves {
            // a door leaf is an entity of the game (`runtime::DoorEnt`), hinged at the d0 jamb: it can be opened and shut; some start closed, some ajar, a few wide open
            let roll = cx.rng.f32();
            let side = if cx.rng.chance(0.5) { 1.0 } else { -1.0 };
            let hinge = match w.axis {
                Axis::X => (d0, w.pos),
                Axis::Z => (w.pos, d0),
            };
            let (along, perp) = match w.axis {
                Axis::X => (cx.foot.ex, cx.foot.ez),
                Axis::Z => (cx.foot.ez, cx.foot.ex),
            };
            let out = scale(perp, side);
            let o = cx.foot.at(hinge.0, hinge.1, y0 + 0.01);
            let open = if roll < 0.4 { 0.0 } else if roll < 0.9 { 0.25 + (roll - 0.4) * 0.8 } else { 1.0 };
            // (`door_entity` takes a hand's width off the opening for the leaf and puts the hinge a little inside the jamb: the numbers give it back)
            let hinge_world = sub(o, scale(along, 0.045));
            cx.mb.doors.push([hinge_world[0], hinge_world[1], hinge_world[2], along[0], along[2], out[0], out[2], d1 - d0 + 0.08, DOOR_H + 0.04, open]);
        }
    }
}

impl Ctx<'_> {
    fn paint_tint(&self, i: usize) -> V3 {
        // the paints are plain materials in the library; walls take the inner material's picture with their own tint
        let _ = i;
        let k = [[1.0, 1.0, 1.0], [1.0, 0.95, 0.85], [0.85, 0.95, 0.88], [0.88, 0.9, 1.0]];
        k[i % 4]
    }
}

// ------------------------------------------------------------------------------------------------ furnishing

fn furnish(cx: &mut Ctx, plan: &Plan, room: &Room, occ: Occupancy) {
    let r = room.rect;
    let doors = door_zones(plan, r);
    let _ = occ;
    match room.kind {
        Kind::Shop => {
            // shelves along both side walls and the back, a counter by the door, a few things on the floor
            for (side, n) in [(2u8, 3), (3, 3), (1, 3)] {
                for k in 0..n {
                    cx.against(&["wooden_display_shelves_01", "steel_frame_shelves_03", "wooden_bookshelf_worn", "drawer_cabinet", "painted_wooden_shelves"], r, side, (k as f32 + 0.5) / n as f32, 0.04, 0.0, &doors);
                }
            }
            let (cxm, cz) = (r.cx(), r.z0 + 3.2);
            if r.d() > 6.0 {
                cx.box_in(cx.lib.id("wood_dark"), cxm - 1.3, cxm + 1.3, cz - 0.4, cz + 0.4, 0.0, 1.0, [1.0; 3]);
                let top = cx.lib.id("countertop");
                cx.cuboid(top, [cxm - 1.35, cx.y + 1.0, cz - 0.45], [cxm + 1.35, cx.y + 1.04, cz + 0.45], 63, [1.0; 3]);
                if let Some(reg) = cx.props.get("CashRegister_01").cloned() {
                    cx.put(&reg, cxm + 0.6, cz, (0.0, -1.0), 1.04, 1.0);
                }
                for k in 0..3 {
                    cx.try_put(&["plastic_crate_01", "cardboard_box_01", "wicker_basket_01", "long_life_food"], r, r.x0 + 1.0 + k as f32 * 1.8, r.z0 + r.d() * 0.55, (0.0, 1.0), 0.0, &doors);
                }
                for k in 0..2 {
                    cx.try_put(&["round_wooden_table_02", "small_wooden_table_01", "side_table_tall_01"], r, r.x0 + r.w() * (0.3 + 0.4 * k as f32), r.z0 + r.d() * 0.75, (0.0, 1.0), 0.0, &doors);
                }
            }
            cx.try_put(&["potted_plant_02", "potted_plant_01", "calathea_orbifolia_01"], r, r.x1 - 0.5, r.z0 + 0.5, (0.0, 1.0), 0.0, &doors);
            // a big shop has rows of stands down the middle, with aisles between them
            if r.w() > 6.0 && r.d() > 9.0 {
                let mut z = r.z0 + 4.8;
                while z < r.z1 - 1.5 {
                    let mut x = r.x0 + 1.6;
                    while x < r.x1 - 1.6 {
                        cx.try_put(&["wooden_display_shelves_01", "steel_frame_shelves_01", "painted_wooden_shelves"], r, x, z, (0.0, 1.0), 0.0, &doors);
                        x += 1.35;
                    }
                    z += 3.6;
                }
            }
            for k in 0..3 {
                cx.light(r.x0 + r.w() * (0.2 + 0.3 * k as f32), r.cz(), 7.0, 5.0);
            }
        }
        Kind::Lobby => {
            // a reception desk, sofas and plants, pictures on the walls
            let (rx, rz) = (r.cx(), r.z1 - 1.8);
            cx.box_in(cx.lib.id("wood_dark"), rx - 1.5, rx + 1.5, rz - 0.4, rz + 0.4, 0.0, 1.05, [1.0; 3]);
            let top = cx.lib.id("countertop");
            cx.cuboid(top, [rx - 1.55, cx.y + 1.05, rz - 0.45], [rx + 1.55, cx.y + 1.09, rz + 0.45], 63, [1.0; 3]);
            if let Some(l) = cx.props.get("classic_laptop").cloned() {
                cx.put(&l, rx - 0.5, rz, (0.0, -1.0), 1.09, 1.0);
            }
            for k in 0..2 {
                cx.against(&["sofa_02", "sofa_03", "mid_century_lounge_chair", "modern_arm_chair_01"], r, 2 + k as u8, 0.3 + 0.4 * k as f32, 0.05, 0.0, &doors);
            }
            cx.try_put(&["modern_coffee_table_02", "modern_coffee_table_01"], r, r.x0 + 2.2, r.cz(), (0.0, 1.0), 0.0, &doors);
            // seating areas across a wide hall: every seven metres a sofa against the back wall with a table and two armchairs in front of it, and between them a plant; pillars down the middle of a very wide one
            let groups = ((r.w() / 7.0).floor() as i32).max(1);
            for g in 0..groups {
                let x = r.x0 + r.w() * (g as f32 + 0.5) / groups as f32;
                if (x - rx).abs() < 2.4 {
                    continue;
                }
                cx.try_put(&["sofa_02", "sofa_03", "mid_century_lounge_chair"], r, x, r.z1 - 0.65, (0.0, -1.0), 0.0, &doors);
                if cx.try_put(&["modern_coffee_table_01", "modern_coffee_table_02", "side_table_01"], r, x, r.z1 - 2.2, (0.0, 1.0), 0.0, &doors) {
                    for dx in [-1.5, 1.5] {
                        cx.try_put(&["modern_arm_chair_01", "ArmChair_01", "mid_century_lounge_chair"], r, x + dx, r.z1 - 2.2, (-dx.signum(), 0.0), 0.0, &doors);
                    }
                }
                if g + 1 < groups {
                    cx.try_put(&["potted_plant_02", "potted_plant_01", "calathea_orbifolia_01", "pachira_aquatica_01"], r, x + r.w() / groups as f32 * 0.5, r.z1 - 0.5, (0.0, 1.0), 0.0, &doors);
                }
            }
            if r.w() > 14.0 {
                let stone = cx.lib.id("concrete_wall_007");
                let n = (r.w() / 6.5).floor() as i32;
                for k in 0..n {
                    let x = r.x0 + r.w() * (k as f32 + 0.5) / n as f32;
                    let (z, hh) = (r.z0 + r.d() * 0.4, cx.ceil - cx.y);
                    let fp = Rect::new(x - 0.3, x + 0.3, z - 0.3, z + 0.3);
                    if !cx.occupied.iter().any(|o| o.overlaps(&fp)) && !doors.iter().any(|d| d.overlaps(&fp)) {
                        cx.box_in(stone, fp.x0, fp.x1, fp.z0, fp.z1, 0.0, hh, [0.85; 3]);
                    }
                }
            }
            for (x, z) in [(r.x0 + 0.5, r.z0 + 0.5), (r.x1 - 0.5, r.z0 + 0.5), (r.x1 - 0.5, r.z1 - 0.5)] {
                cx.try_put(&["potted_plant_02", "potted_plant_01", "calathea_orbifolia_01", "pachira_aquatica_01"], r, x, z, (0.0, 1.0), 0.0, &doors);
            }
            wall_art(cx, r, &doors);
            for k in 0..3 {
                cx.light(r.x0 + r.w() * (0.2 + 0.3 * k as f32), r.cz(), 8.0, 5.0);
            }
        }
        Kind::Office => {
            // rows of desks with chairs, shelves along the walls, a plant or two, a printer corner
            let step_x = 3.4;
            let nx = ((r.w() - 1.0) / step_x).floor().max(1.0) as i32;
            let nz = ((r.d() - 3.0) / 3.2).floor().max(1.0) as i32;
            for iz in 0..nz {
                for ix in 0..nx {
                    let x = r.x0 + 0.5 + step_x * (ix as f32 + 0.5) + (r.w() - 1.0 - step_x * nx as f32) * 0.5;
                    let z = r.z0 + 1.8 + iz as f32 * 3.2;
                    if cx.try_put(&["metal_office_desk", "SchoolDesk_01"], r, x, z, (0.0, 1.0), 0.0, &doors) {
                        cx.try_put(&["dining_chair_02", "SchoolChair_01", "plastic_monobloc_chair_01", "modern_arm_chair_01"], r, x, z + 1.0, (0.0, -1.0), 0.0, &doors);
                        if let Some(l) = cx.props.get("classic_laptop").cloned() {
                            if cx.rng.chance(0.6) {
                                cx.put(&l, x, z, (0.0, 1.0), 0.79, 1.0);
                            }
                        }
                        if let Some(l) = cx.props.get("desk_lamp_arm_01").cloned() {
                            if cx.rng.chance(0.4) {
                                cx.put(&l, x + 0.7, z, (0.0, 1.0), 0.79, 1.0);
                            }
                        }
                    }
                }
            }
            for side in [2u8, 3, 1] {
                for k in 0..3 {
                    cx.against(&["steel_frame_shelves_02", "wooden_bookshelf_worn", "drawer_cabinet", "modern_wooden_cabinet", "vintage_cabinet_01"], r, side, (k as f32 + 0.5) / 3.0, 0.04, 0.0, &doors);
                }
            }
            for (x, z) in [(r.x0 + 0.5, r.z0 + 0.5), (r.x1 - 0.5, r.z0 + 0.5)] {
                cx.try_put(&["potted_plant_02", "potted_plant_01", "calathea_orbifolia_01"], r, x, z, (0.0, 1.0), 0.0, &doors);
            }
            let nl = ((r.w() / 4.5).round() as i32).max(1);
            for k in 0..nl {
                for j in 0..((r.d() / 5.0).round() as i32).max(1) {
                    cx.light(r.x0 + r.w() * (k as f32 + 0.5) / nl as f32, r.z0 + r.d() * (j as f32 + 0.5) / ((r.d() / 5.0).round().max(1.0)), 7.0, 5.0);
                }
            }
        }
        Kind::Meeting => {
            let (x, z) = (r.cx(), r.cz());
            cx.try_put(&["dining_table", "painted_wooden_table", "wooden_table_02"], r, x, z, (0.0, 1.0), 0.0, &doors);
            for k in 0..3 {
                cx.try_put(&["dining_chair_02", "SchoolChair_01", "painted_wooden_chair_01"], r, x - 0.8 + k as f32 * 0.8, z + 0.95, (0.0, -1.0), 0.0, &doors);
                cx.try_put(&["dining_chair_02", "SchoolChair_01", "painted_wooden_chair_01"], r, x - 0.8 + k as f32 * 0.8, z - 0.95, (0.0, 1.0), 0.0, &doors);
            }
            cx.against(&["projector_screen"], r, 1, 0.5, 0.05, 0.0, &doors);
            cx.light(x, z, 6.0, 5.0);
        }
        Kind::Living => {
            // a sofa facing a television, a table between, plants and pictures; a dining table at the window end
            let long_side = if r.w() >= r.d() { 1u8 } else { 3 };
            cx.against(&["sofa_02", "sofa_03", "sofa_01", "mid_century_lounge_chair"], r, long_side, 0.5, 0.05, 0.0, &doors);
            cx.try_put(&["modern_coffee_table_01", "modern_coffee_table_02", "side_table_01"], r, r.cx(), r.cz() + if long_side == 1 { -0.9 } else { 0.0 }, (0.0, 1.0), 0.0, &doors);
            let tv_side = if long_side == 1 { 0u8 } else { 2 };
            if cx.against(&["modern_wooden_cabinet", "drawer_cabinet", "painted_wooden_cabinet"], r, tv_side, 0.5, 0.04, 0.0, &doors) {
                if let Some(tv) = cx.props.get("television_02").cloned() {
                    let (x, z, f) = if tv_side == 0 { (r.cx(), r.z0 + 0.35, (0.0, 1.0)) } else { (r.x0 + 0.35, r.cz(), (1.0, 0.0)) };
                    cx.put(&tv, x, z, f, 0.7, 1.0);
                }
            }
            cx.try_put(&["modern_arm_chair_01", "ArmChair_01"], r, r.x1 - 0.8, r.z0 + 1.0, (-1.0, 0.0), 0.0, &doors);
            for (x, z) in [(r.x1 - 0.4, r.z1 - 0.4), (r.x0 + 0.4, r.z1 - 0.4)] {
                cx.try_put(&["potted_plant_02", "potted_plant_04", "calathea_orbifolia_01", "potted_plant_01"], r, x, z, (0.0, 1.0), 0.0, &doors);
            }
            wall_art(cx, r, &doors);
            cx.light(r.cx(), r.cz(), 6.5, 4.5);
        }
        Kind::Bedroom => {
            let side = if r.w() >= r.d() { 3u8 } else { 1 };
            if cx.against(&["vintage_day_bed", "old_bed_frame", "GothicBed_01"], r, side, 0.5, 0.05, 0.0, &doors) {
                for t in [0.15f32, 0.85] {
                    cx.against(&["painted_wooden_nightstand", "ClassicNightstand_01", "side_table_01"], r, side, t, 0.05, 0.0, &doors);
                }
            }
            cx.against(&["drawer_cabinet", "painted_wooden_cabinet", "modern_wooden_cabinet"], r, 0, 0.8, 0.04, 0.0, &doors);
            cx.try_put(&["potted_plant_04", "potted_plant_02"], r, r.x1 - 0.4, r.z1 - 0.4, (0.0, 1.0), 0.0, &doors);
            wall_art(cx, r, &doors);
            cx.light(r.cx(), r.cz(), 5.5, 3.5);
        }
        Kind::Kitchen => {
            // a run of units along one wall with the stove in it, a fridge, a table with chairs
            let (z0, z1) = (r.z0, r.z1);
            let mut x = r.x0 + 0.05;
            let wood = cx.lib.id("wood_light");
            let top = cx.lib.id("countertop");
            let yy = cx.y;
            let run_end = r.x1 - 0.75;
            while x + 0.6 <= run_end {
                let w = 0.6;
                let stove = x > r.x0 + 0.7 && x + 0.6 <= run_end && cx.rng.chance(0.25) && cx.props.get("electric_stove").is_some();
                if stove {
                    if let Some(s) = cx.props.get("electric_stove").cloned() {
                        let fp = Rect::new(x, x + w, z1 - 0.65, z1);
                        if !cx.occupied.iter().any(|o| o.overlaps(&fp)) {
                            cx.put(&s, x + w * 0.5, z1 - 0.34, (0.0, -1.0), 0.0, 1.0);
                            cx.occupied.push(fp);
                        }
                    }
                } else {
                    let fp = Rect::new(x, x + w, z1 - 0.62, z1);
                    if !doors.iter().any(|d| d.overlaps(&fp)) && !cx.occupied.iter().any(|o| o.overlaps(&fp)) {
                        cx.cuboid(wood, [x, yy, z1 - 0.62], [x + w - 0.01, yy + 0.86, z1], 63, [1.0; 3]);
                        cx.cuboid(top, [x - 0.01, yy + 0.86, z1 - 0.64], [x + w, yy + 0.9, z1], 63, [1.0; 3]);
                        cx.cuboid(wood, [x, yy + 1.5, z1 - 0.34], [x + w - 0.01, yy + 2.1, z1], 63, [0.9; 3]);
                        cx.occupied.push(fp);
                    }
                }
                x += w;
            }
            let steel = cx.lib.id("steel");
            let fp = Rect::new(r.x1 - 0.72, r.x1 - 0.02, z1 - 0.72, z1 - 0.02);
            if !cx.occupied.iter().any(|o| o.overlaps(&fp)) && !doors.iter().any(|d| d.overlaps(&fp)) {
                cx.cuboid(steel, [fp.x0, yy, fp.z0], [fp.x1, yy + 1.8, fp.z1], 63, [0.9; 3]);
                cx.occupied.push(fp);
            }
            let (tx, tz) = (r.cx(), r.z0 + r.d() * 0.4);
            if cx.try_put(&["round_wooden_table_02", "small_wooden_table_01", "painted_wooden_table"], r, tx, tz, (0.0, 1.0), 0.0, &doors) {
                for (dx, dz, f) in [(-0.7, 0.0, (1.0, 0.0)), (0.7, 0.0, (-1.0, 0.0)), (0.0, 0.7, (0.0, -1.0))] {
                    cx.try_put(&["dining_chair_02", "painted_wooden_chair_01", "metal_stool_02"], r, tx + dx, tz + dz, f, 0.0, &doors);
                }
            }
            let _ = z0;
            cx.light(r.cx(), r.cz(), 5.0, 4.0);
        }
        Kind::Bath => {
            // a bath, a toilet and a basin built from boxes of ceramic, with a mirror
            let ce = cx.lib.id("ceramic_white");
            let steel = cx.lib.id("steel");
            let glass = cx.lib.id("door_glass");
            let yy = cx.y;
            let (w, d) = (r.w(), r.d());
            if w >= d {
                let fp = Rect::new(r.x1 - 0.8, r.x1 - 0.02, r.z0 + 0.02, r.z0 + 1.75f32.min(d - 0.1));
                if !doors.iter().any(|dz| dz.overlaps(&fp)) {
                    cx.cuboid(ce, [fp.x0, yy, fp.z0], [fp.x1, yy + 0.55, fp.z1], 63, [1.0; 3]);
                    cx.occupied.push(fp);
                }
                for (k, x) in [r.x0 + 0.45, r.x0 + 1.25].into_iter().enumerate() {
                    let fp = Rect::new(x - 0.2, x + 0.2, r.z1 - 0.75, r.z1 - 0.02);
                    if k == 0 {
                        cx.cuboid(ce, [fp.x0, yy, fp.z0 + 0.3], [fp.x1, yy + 0.4, fp.z1], 63, [1.0; 3]);
                        cx.cuboid(ce, [fp.x0 + 0.02, yy + 0.4, fp.z1 - 0.2], [fp.x1 - 0.02, yy + 0.85, fp.z1], 63, [1.0; 3]);
                    } else {
                        cx.cuboid(ce, [fp.x0 - 0.05, yy + 0.8, fp.z1 - 0.45], [fp.x1 + 0.05, yy + 0.88, fp.z1], 63, [1.0; 3]);
                        cx.cuboid(ce, [fp.x0 + 0.1, yy, fp.z1 - 0.2], [fp.x1 - 0.1, yy + 0.8, fp.z1 - 0.1], 63, [1.0; 3]);
                        cx.cuboid(steel, [x - 0.02, yy + 0.88, fp.z1 - 0.12], [x + 0.02, yy + 1.0, fp.z1 - 0.06], 63, [1.0; 3]);
                        cx.cuboid(glass, [x - 0.35, yy + 1.2, fp.z1 - 0.012], [x + 0.35, yy + 1.9, fp.z1 - 0.004], 63, [1.0; 3]);
                    }
                    cx.occupied.push(fp);
                }
            } else {
                let fp = Rect::new(r.x0 + 0.02, (r.x0 + 1.75).min(r.x1 - 0.1), r.z1 - 0.8, r.z1 - 0.02);
                if !doors.iter().any(|dz| dz.overlaps(&fp)) {
                    cx.cuboid(ce, [fp.x0, yy, fp.z0], [fp.x1, yy + 0.55, fp.z1], 63, [1.0; 3]);
                    cx.occupied.push(fp);
                }
                let fp = Rect::new(r.x1 - 0.75, r.x1 - 0.02, r.z0 + 0.25, r.z0 + 0.65);
                if !doors.iter().any(|dz| dz.overlaps(&fp)) {
                    cx.cuboid(ce, [fp.x0 + 0.3, yy, fp.z0], [fp.x1, yy + 0.4, fp.z1], 63, [1.0; 3]);
                    cx.occupied.push(fp);
                }
            }
            cx.light(r.cx(), r.cz(), 4.0, 3.5);
        }
        Kind::Storage => {
            for side in [2u8, 3, 1] {
                for k in 0..2 {
                    cx.against(&["steel_frame_shelves_02", "steel_frame_shelves_03", "wooden_bookshelf_worn", "painted_wooden_shelves"], r, side, (k as f32 + 0.5) / 2.0, 0.04, 0.0, &doors);
                }
            }
            for k in 0..3 {
                cx.try_put(&["cardboard_box_01", "plastic_crate_01", "wicker_basket_01"], r, r.x0 + 0.7 + 0.8 * k as f32, r.cz(), (0.0, 1.0), 0.0, &doors);
            }
            cx.light(r.cx(), r.cz(), 4.5, 3.0);
        }
        Kind::Warehouse => {
            // rows of shelving across the hall with aisles between them, crates and trolleys in the aisles, lights hung over the aisles
            for rm in &plan.rooms {
                if rm.kind == Kind::Office {
                    cx.occupied.push(rm.rect);
                }
            }
            let aisle = 3.4;
            let mut z = r.z0 + 1.4;
            while z + 1.0 < r.z1 - 1.2 {
                let mut x = r.x0 + 1.2;
                while x < r.x1 - 1.2 {
                    cx.try_put(&["steel_frame_shelves_01", "steel_frame_shelves_02", "steel_frame_shelves_03", "wooden_display_shelves_01"], r, x + 0.5, z, (0.0, 1.0), 0.0, &doors);
                    x += 1.3;
                }
                z += aisle;
            }
            for k in 0..8 {
                let (x, zz) = (r.x0 + r.w() * (0.12 + 0.76 * cx.rng.f32()), r.z0 + r.d() * (0.1 + 0.8 * cx.rng.f32()));
                cx.try_put(&["plastic_crate_01", "cardboard_box_01", "hand_truck", "tool_cart"], r, x, zz, (0.0, 1.0), 0.0, &doors);
                let _ = k;
            }
            let (nx, nz) = (((r.w() / 9.0).round() as i32).max(1), ((r.d() / 8.0).round() as i32).max(1));
            for i in 0..nx {
                for j in 0..nz {
                    cx.light(r.x0 + r.w() * (i as f32 + 0.5) / nx as f32, r.z0 + r.d() * (j as f32 + 0.5) / nz as f32, 12.0, 6.0);
                }
            }
        }
        Kind::Corridor => {
            let n = ((r.w() / 5.0).round() as i32).max(1);
            for k in 0..n {
                cx.light(r.x0 + r.w() * (k as f32 + 0.5) / n as f32, r.cz(), 4.5, 3.0);
            }
        }
        Kind::Stair | Kind::Shaft => {}
    }
}

/// Pictures, a clock and a mirror on the walls of a room (their backs on the wall, their fronts into the room), clear of doors.
fn wall_art(cx: &mut Ctx, r: Rect, doors: &[Rect]) {
    let y = 1.5;
    let kinds = [&["hanging_picture_frame_01", "hanging_picture_frame_02", "hanging_picture_frame_03"][..], &["wall_clock"], &["ornate_mirror_01"]];
    for (n, k) in kinds.iter().enumerate() {
        if !cx.rng.chance(0.7) {
            continue;
        }
        let side = cx.rng.int(0, 3) as u8;
        let t = cx.rng.range(0.25, 0.75);
        let present: Vec<&&str> = k.iter().filter(|s| cx.props.get(s).is_some()).collect();
        if present.is_empty() {
            continue;
        }
        let slug = *present[(cx.rng.u64() % present.len() as u64) as usize];
        let prop = cx.props.get(slug).unwrap().clone();
        let s = prop.size();
        let (x, z, front) = match side {
            0 => (r.x0 + t * r.w(), r.z0 + 0.03, (0.0, 1.0)),
            1 => (r.x0 + t * r.w(), r.z1 - 0.03, (0.0, -1.0)),
            2 => (r.x0 + 0.03, r.z0 + t * r.d(), (1.0, 0.0)),
            _ => (r.x1 - 0.03, r.z0 + t * r.d(), (-1.0, 0.0)),
        };
        let fp = Ctx::footprint(&prop, x, z, front, 1.0);
        let wide = Rect::new(fp.x0 - 0.05, fp.x1 + 0.05, fp.z0 - 0.05, fp.z1 + 0.05);
        let wall_zone = Rect::new(fp.x0 - 0.2, fp.x1 + 0.2, fp.z0 - 0.2, fp.z1 + 0.2);
        let clashes = doors.iter().any(|d| d.overlaps(&wall_zone)) || cx.occupied.iter().any(|o| o.overlaps(&wide) && ((o.x1 - o.x0).min(o.z1 - o.z0) > 0.0) && false);
        if clashes || s[1] > 1.6 {
            continue;
        }
        cx.put(&prop, x, z, front, y + n as f32 * 0.0, 1.0);
    }
}

/// Stairs from floor k to floor k + 1 in the hall: two flights side by side joined by a landing at the back. The steps are solid blocks.
fn build_stairs(cx: &mut Ctx, plan: &Plan, rise_total: f32) {
    let h = plan.hall;
    let steel = cx.lib.id("steel");
    let conc = cx.lib.id(if cx.lib.has("concrete_wall_007") { "concrete_wall_007" } else { "equip_paint" });
    let t = 0.1;
    let (x0, x1, z0, z1) = (h.x0 + t, h.x1 - t, h.z0 + t, h.z1 - t);
    let wf = ((x1 - x0) - 0.15) * 0.5;
    let za = 1.0;
    let ld = 1.3;
    let run0 = z0 + za;
    let run1 = z1 - ld;
    let half = rise_total * 0.5;
    let n = (half / 0.175).ceil().max(2.0) as i32;
    let rise = half / n as f32;
    let tread = (run1 - run0) / n as f32;
    let y = cx.y;
    let tex = cx.lib.tex_plain(conc);
    let fr = cx.fr();
    // flight A on the left: rises towards +z from the floor to half a floor
    // (treads and landings are slabs 0.3 m thick, not blocks down to the floor: the flights of the floor above stand over these, and whoever climbs needs the head room between them)
    let th = 0.3;
    for i in 0..n {
        let (a, b) = (run0 + i as f32 * tread, run0 + (i + 1) as f32 * tread);
        let top = y + rise * (i + 1) as f32;
        cx.mb.cuboid(conc, &fr, [x0, (top - th).max(y), a], [x0 + wf, top, b], 63, &tex, [0.85; 3]);
    }
    // the landing across the back at half height
    cx.mb.cuboid(conc, &fr, [x0, y + half - th, run1], [x1, y + half, z1], 63, &tex, [0.85; 3]);
    // flight B on the right: rises towards -z from half a floor to the next floor
    let bx0 = x1 - wf;
    for i in 0..n {
        let (b, a) = (run1 - i as f32 * tread, run1 - (i + 1) as f32 * tread);
        let top = y + half + rise * (i + 1) as f32;
        cx.mb.cuboid(conc, &fr, [bx0, top - th, a], [x1, top, b], 63, &tex, [0.85; 3]);
    }
    // a handrail along the gap between the flights, and a light bar over the landing
    let gap_x = x0 + wf + 0.075;
    let st = cx.lib.tex_plain(steel);
    let rail0 = [gap_x - 0.02, y + 0.9, run0];
    let rail1 = [gap_x + 0.02, y + 0.95 + half, run1];
    let _ = (rail0, rail1);
    for i in 0..n {
        let a = run0 + i as f32 * tread;
        cx.mb.cuboid(steel, &fr, [gap_x - 0.02, y + rise * (i + 1) as f32 + 0.85, a], [gap_x + 0.02, y + rise * (i + 1) as f32 + 0.92, a + tread], 63, &st, [1.0; 3]);
    }
}

/// The elevator shaft's face on this floor: closed steel doors in the wall, and a call button.
fn build_shaft_doors(cx: &mut Ctx, plan: &Plan) {
    let Some(s) = plan.shaft else { return };
    let steel = cx.lib.id("steel");
    let st = cx.lib.tex_plain(steel);
    let fr = cx.fr();
    let y = cx.y;
    let z = s.z0;
    let mid = s.cx();
    cx.mb.cuboid(steel, &fr, [mid - 0.55, y, z - 0.12], [mid - 0.01, y + 2.1, z - 0.05], 63, &st, [0.9; 3]);
    cx.mb.cuboid(steel, &fr, [mid + 0.01, y, z - 0.12], [mid + 0.55, y + 2.1, z - 0.05], 63, &st, [0.9; 3]);
    let frame = cx.lib.id("frame_dark");
    let ft = cx.lib.tex_plain(frame);
    cx.mb.cuboid(frame, &fr, [mid - 0.62, y, z - 0.13], [mid - 0.55, y + 2.17, z - 0.04], 63, &ft, [1.0; 3]);
    cx.mb.cuboid(frame, &fr, [mid + 0.55, y, z - 0.13], [mid + 0.62, y + 2.17, z - 0.04], 63, &ft, [1.0; 3]);
    cx.mb.cuboid(frame, &fr, [mid - 0.62, y + 2.1, z - 0.13], [mid + 0.62, y + 2.17, z - 0.04], 63, &ft, [1.0; 3]);
    cx.mb.cuboid(frame, &fr, [mid + 0.8, y + 1.1, z - 0.06], [mid + 0.9, y + 1.3, z - 0.04], 63, &ft, [1.0; 3]);
}

/// The surfaces of floor `k`: every room's floor at floor line `k`, with the stair opening (and the shaft) cut out.
fn build_floor_tops(cx: &mut Ctx, plan: &Plan, k: u32, lv: &Levels, stair_hole: bool) {
    let y = lv.line(k);
    let hole = stair_hole.then(|| Rect::new(plan.hall.x0 + 0.1, plan.hall.x1 - 0.1, plan.hall.z0 + 1.0, plan.hall.z1 - 0.1));
    let shaft_hole = (k > 0).then_some(plan.shaft).flatten();
    let mut holes: Vec<Rect> = Vec::new();
    holes.extend(hole);
    holes.extend(shaft_hole);
    let rooms: Vec<Room> = plan.rooms.clone();
    for room in &rooms {
        let (m, tex) = floor_mat(cx.lib, room.kind, cx.pal, (room.rect.x0 * 7.31 + room.rect.z0 * 3.17).fract().abs());
        for p in cut_holes(vec![room.rect], &holes) {
            let tint = if room.kind == Kind::Shaft { [0.5; 3] } else { [1.0; 3] };
            cx.flat(m, p, y, &tex, tint, true);
        }
    }
}

/// The ceiling of floor `k`: the underside of the slab at floor line `k + 1` (the roof for the top floor), 0.3 m below that line, with the stair opening and the shaft cut out. It is part of floor `k`, where it is seen from.
fn build_ceiling(cx: &mut Ctx, plan: &Plan, k: u32, lv: &Levels) {
    let slab = k + 1;
    let y = lv.line(slab) - SLAB;
    let stair_hole = slab < lv.floors;
    let hole = stair_hole.then(|| Rect::new(plan.hall.x0 + 0.1, plan.hall.x1 - 0.1, plan.hall.z0 + 1.0, plan.hall.z1 - 0.1));
    let mut holes: Vec<Rect> = Vec::new();
    holes.extend(hole);
    holes.extend(plan.shaft);
    let ceil = cx.pal.ceiling;
    let ct = cx.lib.tex_plain(ceil);
    for p in cut_holes(vec![plan.inner], &holes) {
        cx.flat(ceil, p, y, &ct, [1.0; 3], false);
    }
}

/// Rectangles with holes cut out of them.
fn cut_holes(mut pieces: Vec<Rect>, holes: &[Rect]) -> Vec<Rect> {
    for h in holes {
        let mut next = Vec::new();
        for p in pieces {
            if p.overlaps(h) {
                let cut = Rect::new(h.x0.max(p.x0), h.x1.min(p.x1), h.z0.max(p.z0), h.z1.min(p.z1));
                next.extend(minus(p, cut));
            } else {
                next.push(p);
            }
        }
        pieces = next;
    }
    pieces
}

/// The one floor of a warehouse: an open hall (with an office walled off in the back left corner, a door into the hall), no stairs and no lift.
fn warehouse_plan(foot: &Foot) -> Option<Plan> {
    let t = WALL_T;
    let inner = Rect::new(foot.x0 + t, foot.x1 - t, foot.z0 + t, foot.z1 - t);
    if inner.w() < 8.0 || inner.d() < 8.0 {
        return None;
    }
    let office = Rect::new(inner.x0, inner.x0 + 5.0, inner.z1 - 4.2, inner.z1);
    let hall = Rect::new(inner.x0, inner.x1, inner.z0, inner.z1);
    let walls = vec![wall_x(office.z0, office.x0, office.x1, &[office.x0 + 2.5], PART_T, true), wall_z(office.x1, office.z0, office.z1, &[], PART_T, false)];
    // the hall is drawn as the whole floor but furnished without the office corner (the office is furnished first and its rectangle is occupied)
    let rooms = vec![Room { rect: office, kind: Kind::Office }, Room { rect: hall, kind: Kind::Warehouse }];
    Some(Plan { inner, hall: Rect::new(inner.x1 - 0.5, inner.x1 - 0.4, inner.z1 - 0.5, inner.z1 - 0.4), shaft: None, rooms, walls })
}

/// The rooms of floor `k` as (what kind of room, its rectangle in lot coordinates x0 x1 z0 z1), for tests and tours (tools/citygen/rooms.py): None when the inside is too small to have rooms.
pub fn room_list(key: u64, foot: &Foot, lv: &Levels, occ: Occupancy, k: u32) -> Option<Vec<(String, [f32; 4])>> {
    let mut srng = Rng::keyed(&[key, 0xA11]);
    let mut fr = Rng::keyed(&[key, 0x1000 + k as u64]);
    let plan = if occ == Occupancy::Warehouse {
        warehouse_plan(foot)?
    } else {
        let st = structure(&mut srng, foot, occ != Occupancy::Home)?;
        plan_floor(&mut fr, &st, occ, k, lv)
    };
    Some(plan.rooms.iter().map(|r| (format!("{:?}", r.kind), [r.rect.x0, r.rect.x1, r.rect.z0, r.rect.z1])).collect())
}

/// Where somebody walks to climb from floor `k` to the next: the points (world) at the foot of the first flight, on the landing after it, on the landing in front of the second flight, and where the second flight comes out on the floor above.
/// For tests (tools/citygen/climb.py): None when the building has no stairs from this floor.
pub fn stairs_path(key: u64, foot: &Foot, lv: &Levels, home: bool, k: u32) -> Option<[V3; 4]> {
    if k + 1 >= lv.floors {
        return None;
    }
    let mut srng = Rng::keyed(&[key, 0xA11]);
    let st = structure(&mut srng, foot, !home)?;
    let t = 0.1;
    let (x0, x1, z0, z1) = (st.hall.x0 + t, st.hall.x1 - t, st.hall.z0 + t, st.hall.z1 - t);
    let wf = ((x1 - x0) - 0.15) * 0.5;
    let (run0, run1) = (z0 + 1.0, z1 - 1.3);
    let (xa, xb) = (x0 + wf * 0.5, x1 - wf * 0.5);
    let (ya, yb) = (lv.line(k), lv.line(k + 1));
    let half = (yb - ya) * 0.5;
    Some([foot.at(xa, run0 - 0.6, ya + 0.05), foot.at(xa, run1 + 0.5, ya + half + 0.05), foot.at(xb, run1 + 0.5, ya + half + 0.05), foot.at(xb, run0 - 0.6, yb + 0.05)])
}

/// Build one floor of a building: the same floor whatever else is built, from the building's `key` (the stair hall and the corridor are the same on every floor, so floors can be made one at a time as somebody comes near).
/// None when the inside is too small to have rooms.
#[allow(clippy::too_many_arguments)]
pub fn build_floor(key: u64, lib: &Lib, props: &Props, pal: &Palette, foot: &Foot, lv: &Levels, occ: Occupancy, k: u32, entrances: &[Rect]) -> Option<(MeshBuilder, Vec<InLight>)> {
    let mut srng = Rng::keyed(&[key, 0xA11]);
    let mut fr = Rng::keyed(&[key, 0x1000 + k as u64]);
    let plan = if occ == Occupancy::Warehouse {
        warehouse_plan(foot)?
    } else {
        let st = structure(&mut srng, foot, occ != Occupancy::Home)?;
        plan_floor(&mut fr, &st, occ, k, lv)
    };
    let y = lv.line(k);
    let ceil = lv.line(k + 1) - SLAB;
    let mut cx = Ctx { mb: MeshBuilder::new(), rng: fr.fork(7), lib, props, pal, foot: *foot, k, y, ceil, occupied: entrances.to_vec(), lights: Vec::new(), paint: [pal.inner; 4] };
    // the floor of this floor (k = 0: the ground, no ceiling under it); the stair opening is in every floor above the ground
    build_floor_tops(&mut cx, &plan, k, lv, k > 0);
    build_ceiling(&mut cx, &plan, k, lv);
    // walls and doors
    let walls = plan.walls.clone();
    for (n, w) in walls.iter().enumerate() {
        build_wall(&mut cx, w, (k as usize + n) % 4, (k as usize + n + 1) % 4);
    }
    build_shaft_doors(&mut cx, &plan);
    if k + 1 < lv.floors {
        let rise = lv.line(k + 1) - lv.line(k);
        build_stairs(&mut cx, &plan, rise);
    }
    // furniture, room by room
    let rooms = plan.rooms.clone();
    for room in &rooms {
        furnish(&mut cx, &plan, room, occ);
    }
    Some((cx.mb, cx.lights))
}

/// What a game server needs of one floor (`building::server_floor`): the partitions that stop a bullet and a look, the doors that have a leaf, and the places where something to search stands. Nothing that is drawn.
pub struct ServerFloor {
    /// The partitions, and the lintels over their doorways, as boxes in lot coordinates with absolute heights (the whole height of the floor).
    pub boxes: Vec<(V3, V3)>,
    /// The doorways that have a leaf: the entries of `MeshBuilder::doors` (the hinge in the world, along x z, out x z, width, height, how open).
    pub doors: Vec<[f32; 10]>,
    /// Where a container stands: lot coordinates (x, z), the turn in degrees, the name of the kind of room it is in.
    pub spots: Vec<(f32, f32, f32, &'static str)>,
    /// The height of the surface of the floor.
    pub y: f32,
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Shop => "Shop",
        Kind::Lobby => "Lobby",
        Kind::Office => "Office",
        Kind::Meeting => "Meeting",
        Kind::Living => "Living",
        Kind::Bedroom => "Bedroom",
        Kind::Kitchen => "Kitchen",
        Kind::Bath => "Bath",
        Kind::Storage => "Storage",
        Kind::Warehouse => "Warehouse",
        Kind::Corridor => "Corridor",
        Kind::Stair => "Stair",
        Kind::Shaft => "Shaft",
    }
}

/// The names of the kinds of room that can hold a container, in the order the server numbers them.
pub const SEARCH_ROOMS: [&str; 8] = ["Shop", "Office", "Meeting", "Living", "Bedroom", "Kitchen", "Storage", "Warehouse"];

/// The same plan `build_floor` makes of floor `k` (the rooms and the walls are a function of the key, the footprint and the floor, not of the furniture), as the server needs it: no picture, no furniture.
pub fn server_floor(key: u64, foot: &Foot, lv: &Levels, occ: Occupancy, k: u32) -> Option<ServerFloor> {
    let mut srng = Rng::keyed(&[key, 0xA11]);
    let mut fr = Rng::keyed(&[key, 0x1000 + k as u64]);
    let plan = if occ == Occupancy::Warehouse {
        warehouse_plan(foot)?
    } else {
        let st = structure(&mut srng, foot, occ != Occupancy::Home)?;
        plan_floor(&mut fr, &st, occ, k, lv)
    };
    let y0 = lv.line(k);
    let tall = lv.line(k + 1) - SLAB - y0;
    let mut boxes: Vec<(V3, V3)> = Vec::new();
    let mut doors: Vec<[f32; 10]> = Vec::new();
    for w in &plan.walls {
        let t = w.thick * 0.5;
        let lo_hi = |u0: f32, u1: f32, h0: f32, h1: f32| -> (V3, V3) {
            match w.axis {
                Axis::X => ([u0, y0 + h0, w.pos - t], [u1, y0 + h1, w.pos + t]),
                Axis::Z => ([w.pos - t, y0 + h0, u0], [w.pos + t, y0 + h1, u1]),
            }
        };
        let mut gaps = w.doors.clone();
        gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut cur = w.a0;
        for (d0, d1) in &gaps {
            let (d0, d1) = (d0.max(w.a0), d1.min(w.a1));
            if d1 <= d0 {
                continue;
            }
            if d0 > cur {
                boxes.push(lo_hi(cur, d0, 0.0, tall));
            }
            cur = d1;
            boxes.push(lo_hi(d0, d1, DOOR_H, tall));
            if w.leaves {
                // (the leaf hangs at the d0 jamb as in `build_wall`; the side it swings to is the +perpendicular one)
                let hinge = match w.axis {
                    Axis::X => (d0, w.pos),
                    Axis::Z => (w.pos, d0),
                };
                let (along, perp) = match w.axis {
                    Axis::X => (foot.ex, foot.ez),
                    Axis::Z => (foot.ez, foot.ex),
                };
                let o = foot.at(hinge.0, hinge.1, y0 + 0.01);
                let hw = sub(o, scale(along, 0.045));
                doors.push([hw[0], hw[1], hw[2], along[0], along[2], perp[0], perp[2], d1 - d0 + 0.08, DOOR_H + 0.04, 0.0]);
            }
        }
        if cur < w.a1 {
            boxes.push(lo_hi(cur, w.a1, 0.0, tall));
        }
    }
    // the things to search: in most storerooms and kitchens, now and then in the other rooms, in a corner out of the way of the doors
    let mut spots: Vec<(f32, f32, f32, &'static str)> = Vec::new();
    let mut rr = Rng::keyed(&[key, 0x5E57, k as u64]);
    for room in &plan.rooms {
        let p = match room.kind {
            Kind::Storage => 0.9,
            Kind::Shop => 0.8,
            Kind::Warehouse => 1.0,
            Kind::Kitchen => 0.6,
            Kind::Bedroom => 0.5,
            Kind::Office => 0.3,
            Kind::Living => 0.3,
            Kind::Meeting => 0.15,
            _ => 0.0,
        };
        let chosen = rr.chance(p);
        let turn = rr.u64();
        let r = room.rect;
        if !chosen || r.w() < 2.4 || r.d() < 2.4 {
            continue;
        }
        let zones = door_zones(&plan, r);
        let inset = 0.75;
        let corners = [(r.x0 + inset, r.z0 + inset), (r.x1 - inset, r.z0 + inset), (r.x0 + inset, r.z1 - inset), (r.x1 - inset, r.z1 - inset)];
        for n in 0..4 {
            let (cx, cz) = corners[(turn as usize + n) % 4];
            let spot = Rect::new(cx - 0.55, cx + 0.55, cz - 0.55, cz + 0.55);
            if zones.iter().all(|z| !z.overlaps(&spot)) {
                spots.push((cx, cz, ((turn >> 8) % 4) as f32 * 90.0, kind_name(room.kind)));
                break;
            }
        }
    }
    Some(ServerFloor { boxes, doors, spots, y: y0 })
}

/// Build the inside of a building over `foot`: one builder per floor.
#[allow(clippy::too_many_arguments)]
pub fn build(rng: &mut Rng, lib: &Lib, props: &Props, pal: &Palette, foot: &Foot, lv: &Levels, occupancy: &dyn Fn(u32) -> Occupancy) -> Interior {
    let key = rng.u64();
    let mut floors: Vec<MeshBuilder> = Vec::new();
    let mut lights: Vec<InLight> = Vec::new();
    for k in 0..lv.floors {
        match build_floor(key, lib, props, pal, foot, lv, occupancy(k), k, &[]) {
            Some((mb, l)) => {
                lights.extend(l);
                floors.push(mb);
            }
            None => return Interior { floors: Vec::new(), lights: Vec::new() },
        }
    }
    Interior { floors, lights }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn foot() -> Foot {
        Foot { o: [0.0, 0.0, 0.0], ex: [1.0, 0.0, 0.0], ez: [0.0, 0.0, 1.0], x0: 0.0, x1: 18.0, z0: 0.0, z1: 22.0 }
    }

    fn pal(lib: &Lib) -> Palette {
        Palette {
            wall: lib.id("wall_white"),
            wall_tint: [1.0; 3],
            trim: lib.id("wall_white"),
            trim_tint: [1.0; 3],
            base: lib.id("wall_white"),
            base_tint: [1.0; 3],
            frame: lib.id("frame_white"),
            glass: [lib.id("glass_dark"); 4],
            lit: [lib.id("glass_lit_warm"), lib.id("glass_lit_cool")],
            glass_w: [1.0, 0.0, 0.0, 0.0],
            roof: lib.id("wall_white"),
            spandrel: lib.id("spandrel_dark"),
            equip: lib.id("equip_paint"),
            shutter: lib.id("steel"),
            sign: [lib.id("sign_red"); 6],
            awning: [lib.id("awning_red"); 4],
            dirt: 0.2,
            inner: lib.id("wall_white"),
            inner_tint: [1.0; 3],
            inner_size: [1.0, 1.0],
            floor: lib.id("wood_dark"),
            floor_tint: [1.0; 3],
            ceiling: lib.id("ceiling_white"),
        }
    }

    #[test]
    fn the_servers_floor_has_the_doors_of_the_floor_that_is_drawn_and_its_spots_stand_inside() {
        let lib = Lib::from_json("");
        let props = Props { map: Default::default() };
        let lv = Levels { y0: 0.0, floors: 6, ground_h: 4.4, floor_h: 3.2 };
        let p = pal(&lib);
        let mut spots = 0;
        for (k, occ) in [(0, Occupancy::Retail), (1, Occupancy::Apartments), (2, Occupancy::Office), (3, Occupancy::Home), (4, Occupancy::Apartments), (0, Occupancy::Warehouse)] {
            let Some((drawn, _)) = build_floor(5, &lib, &props, &p, &foot(), &lv, occ, k, &[]) else {
                assert!(server_floor(5, &foot(), &lv, occ, k).is_none(), "{occ:?}: a floor that is not drawn has no server floor");
                continue;
            };
            let served = server_floor(5, &foot(), &lv, occ, k).expect("a floor that is drawn has a server floor");
            assert_eq!(drawn.doors.len(), served.doors.len(), "{occ:?} floor {k}: the doors with leaves");
            for (a, b) in drawn.doors.iter().zip(&served.doors) {
                // the hinge, the way along the leaf, the size: the same doorway (the side it opens to and how far it starts open are the server's to say)
                for n in [0, 1, 2, 3, 4, 7, 8] {
                    assert!((a[n] - b[n]).abs() < 1e-3, "{occ:?} floor {k}: door entry {n}: {} against {}", a[n], b[n]);
                }
            }
            assert!(!served.boxes.is_empty(), "{occ:?}: partitions");
            for (lo, hi) in &served.boxes {
                assert!(lo[0] >= foot().x0 && hi[0] <= foot().x1 && lo[2] >= foot().z0 && hi[2] <= foot().z1 && hi[1] > lo[1], "{lo:?} {hi:?}");
            }
            for (x, z, _, name) in &served.spots {
                assert!(*x > foot().x0 && *x < foot().x1 && *z > foot().z0 && *z < foot().z1, "{name} at {x} {z}");
                assert!(SEARCH_ROOMS.contains(name), "{name}");
                spots += 1;
            }
        }
        assert!(spots >= 3, "{spots} places to search");
    }

    #[test]
    fn a_plan_tiles_the_inside_without_overlaps_and_the_stairs_reach_the_next_floor() {
        let mut rng = Rng::new(3);
        let st = structure(&mut rng, &foot(), true).unwrap();
        let lv = Levels { y0: 0.0, floors: 6, ground_h: 4.4, floor_h: 3.2 };
        for (k, occ) in [(0, Occupancy::Retail), (1, Occupancy::Apartments), (2, Occupancy::Office)] {
            let plan = plan_floor(&mut rng, &st, occ, k, &lv);
            // the rooms cover the inner rectangle exactly
            let area: f32 = plan.rooms.iter().map(|r| r.rect.w() * r.rect.d()).sum();
            assert!((area - st.inner.w() * st.inner.d()).abs() < 0.2, "{occ:?}: rooms cover {area}, the inside is {}", st.inner.w() * st.inner.d());
            for (n, a) in plan.rooms.iter().enumerate() {
                for b in plan.rooms.iter().skip(n + 1) {
                    assert!(!a.rect.overlaps(&b.rect), "{:?} and {:?} overlap", a.rect, b.rect);
                }
            }
            // the hall has a door
            assert!(plan.walls.iter().any(|w| w.axis == Axis::X && (w.pos - plan.hall.z0).abs() < 1e-3 && !w.doors.is_empty()));
        }
    }

    #[test]
    fn an_interior_has_a_floor_builder_for_every_floor_with_slabs_walls_and_stairs() {
        let lib = Lib::from_json("");
        let props = Props { map: Default::default() };
        let mut rng = Rng::new(9);
        let lv = Levels { y0: 0.15, floors: 5, ground_h: 4.4, floor_h: 3.2 };
        let p = pal(&lib);
        let inter = build(&mut rng, &lib, &props, &p, &foot(), &lv, &|k| if k == 0 { Occupancy::Lobby } else { Occupancy::Apartments });
        assert_eq!(inter.floors.len(), 5);
        for (k, f) in inter.floors.iter().enumerate() {
            assert!(f.triangles() > 100, "floor {k}: {} triangles", f.triangles());
        }
        let (lo, hi) = inter.floors[1].bounds().unwrap();
        assert!(lo[1] >= lv.line(1) - SLAB - 0.01 && hi[1] <= lv.line(2) + 0.01 + 3.0, "{lo:?} {hi:?}");
        assert!(!inter.lights.is_empty());
    }
}

/// What a piece of furniture is to someone who searches it, by the model's name: 1 a cabinet or chest of drawers, 2 shelves, 3 a bedside table, 4 a crate, box or basket, 5 a desk or a cart, 6 a stove or a microwave, 7 a barrel. None for what holds nothing worth taking (a bed, a sofa, a table).
pub fn container_kind(slug: &str) -> Option<u32> {
    match slug {
        "drawer_cabinet" | "painted_wooden_cabinet" | "modern_wooden_cabinet" | "vintage_cabinet_01" => Some(1),
        "wooden_bookshelf_worn" | "painted_wooden_shelves" | "wooden_display_shelves_01" | "steel_frame_shelves_01" | "steel_frame_shelves_02" | "steel_frame_shelves_03" => Some(2),
        "painted_wooden_nightstand" | "ClassicNightstand_01" => Some(3),
        "plastic_crate_01" | "cardboard_box_01" | "wicker_basket_01" | "wooden_crate_02" => Some(4),
        "metal_office_desk" | "tool_cart" => Some(5),
        "electric_stove" | "vintage_microwave" => Some(6),
        "Barrel_01" | "barrel_03" | "metal_jerrycan" => Some(7),
        _ => None,
    }
}
