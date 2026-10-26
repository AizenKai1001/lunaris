//! Buildings: a lot becomes one or more stacked volumes whose walls are dressed by `facade`, with roofs (parapets, equipment, stair houses) on top, and a furnished inside (`interior`) on every floor.

use crate::facade::{self, DoorSpec, Ground, Palette, Punched, WALL_T};
use crate::interior::{self, Foot, InLight, Levels, Occupancy};
use crate::materials::Lib;
use crate::mesh::*;
use crate::props::Props;
use crate::rng::Rng;

/// What is on the other side of a lot's edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// A street or a square: the facade is the show.
    Street,
    /// Another building stands against this wall.
    Party,
    /// Open ground (a courtyard, an alley): seen, but plain.
    Open,
}

/// A rectangle of ground a building stands on: `o` is the corner at the start of the front edge, `ex` runs along the front (unit), `ez` goes from the front into the lot (unit), `w` and `d` are the widths.
/// `sides` are what lies beyond the front (z = 0), right (x = w), back (z = d) and left (x = 0) edges.
#[derive(Clone, Copy, Debug)]
pub struct Lot {
    pub o: V3,
    pub ex: V3,
    pub ez: V3,
    pub w: f32,
    pub d: f32,
    pub sides: [Side; 4],
}

impl Lot {
    pub fn at(&self, x: f32, z: f32) -> V3 {
        add(self.o, add(scale(self.ex, x), scale(self.ez, z)))
    }

    pub fn corners(&self) -> [V3; 4] {
        [self.at(0.0, 0.0), self.at(self.w, 0.0), self.at(self.w, self.d), self.at(0.0, self.d)]
    }

    pub fn foot(&self) -> Foot {
        Foot { o: self.o, ex: self.ex, ez: self.ez, x0: 0.0, x1: self.w, z0: 0.0, z1: self.d }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A mid-rise of brick or stone with punched windows and shops below.
    Masonry,
    /// A plastered block of flats.
    Apartment,
    /// A podium and a glass tower.
    Tower,
    /// A narrow building of two to four floors with a shop below.
    Shophouse,
    /// A house of one to three floors with a pitched roof, set back from the street behind a lawn.
    House,
    /// A hall for storage or work: metal or concrete walls, roller doors, high windows, one tall floor.
    Warehouse,
    /// A supertall: a podium, one to three tiers stepping in, and a crown (flat with equipment, stepped, or a spire).
    Skyscraper,
}

/// What a building makes: its outside is added to the builder given, its inside comes back by floor, with the doors and lights.
pub struct Built {
    pub height: f32,
    pub doors: Vec<DoorSpec>,
    pub lights: Vec<InLight>,
    /// One builder per floor of each volume, bottom to top (slab, partitions, stairs, furniture), and the height of the middle of each.
    pub interior: Vec<MeshBuilder>,
    pub floor_mid: Vec<f32>,
}

fn choose(rng: &mut Rng, lib: &Lib, names: &[&str]) -> usize {
    let present: Vec<&str> = names.iter().copied().filter(|n| lib.has(n)).collect();
    if present.is_empty() {
        return lib.id("equip_paint");
    }
    lib.id(rng.pick(&present))
}

fn tint(rng: &mut Rng, lo: f32, hi: f32) -> V3 {
    let k = rng.range(lo, hi);
    [k * rng.range(0.97, 1.03), k, k * rng.range(0.96, 1.02)]
}

/// The materials of a kind of building, chosen from what the library has.
pub fn palette(lib: &Lib, kind: Kind, rng: &mut Rng) -> Palette {
    let (wall, wall_tint) = match kind {
        Kind::Masonry => (choose(rng, lib, &["brick_wall_001", "brick_wall_003", "brick_wall_09", "brown_brick_02", "dark_brick_wall", "large_red_bricks", "yellow_brick", "concrete_slab_wall_02"]), tint(rng, 0.78, 1.0)),
        Kind::Apartment => (choose(rng, lib, &["beige_wall_001", "painted_plaster_wall", "plastered_wall_05", "grey_plaster_03", "rough_concrete", "concrete_slab_wall_02", "plaster_grey_04"]), tint(rng, 0.82, 1.05)),
        Kind::Tower | Kind::Skyscraper => (choose(rng, lib, &["concrete_slab_wall_02", "concrete_panels", "concrete_wall_007", "concrete_tile_facade", "grey_plaster_03", "rough_concrete", "plaster_grey_04"]), tint(rng, 0.72, 1.06)),
        Kind::Shophouse => (choose(rng, lib, &["beige_wall_001", "painted_plaster_wall", "brick_wall_003", "brown_brick_02", "plastered_wall_05", "yellow_brick"]), tint(rng, 0.85, 1.05)),
        Kind::Warehouse => (choose(rng, lib, &["box_profile_metal_sheet", "factory_wall", "concrete_panels", "metal_plate_02", "concrete_wall_003"]), tint(rng, 0.8, 1.05)),
        Kind::House => (choose(rng, lib, &["white_stucco", "beige_wall_001", "yellow_plaster_02", "painted_brick", "brick_wall_003", "wood_planks_grey", "plastered_wall_05", "brown_brick_02"]), tint(rng, 0.88, 1.06)),
    };
    let trim = choose(rng, lib, &["concrete_wall_007", "rough_concrete", "plaster_grey_04"]);
    let base = choose(rng, lib, &["concrete_tile_facade", "dark_brick_wall", "concrete_wall_007"]);
    let frame = lib.id(rng.pick(&["frame_white", "frame_dark", "frame_alu", "frame_brown"]));
    let tall = matches!(kind, Kind::Tower | Kind::Skyscraper);
    let glass_mix: [usize; 4] = [lib.id(if tall { rng.pick(&["glass_blue", "glass_blue", "glass_green", "glass_teal", "glass_bronze", "glass_silver", "glass_black", "glass_grey"]) } else { "glass_dark" }), lib.id("glass_grey"), lib.id(rng.pick(&["curtain_warm", "curtain_cool"])), lib.id("blind_white")];
    let inner = choose(rng, lib, &["white_plaster_02", "white_plaster_02", "painted_plaster_wall", "beige_wall_001", "plaster_grey_04"]);
    let inner_tints = [[1.0, 1.0, 1.0], [1.0, 0.94, 0.82], [0.88, 0.95, 0.9], [0.9, 0.92, 1.0]];
    Palette {
        wall,
        wall_tint,
        trim,
        trim_tint: tint(rng, 0.85, 1.05),
        base,
        base_tint: [0.8, 0.8, 0.8],
        frame,
        glass: glass_mix,
        glass_w: [0.62, 0.14, 0.14, 0.10],
        lit: [lib.id("glass_lit_warm"), lib.id("glass_lit_cool")],
        roof: if kind == Kind::House { choose(rng, lib, &["clay_roof_tiles_03", "roof_slates_03", "grey_roof_tiles"]) } else { choose(rng, lib, &["bitumen", "grey_roof_01"]) },
        spandrel: lib.id(rng.pick(&["spandrel_dark", "spandrel_blue", "spandrel_cream"])),
        equip: lib.id("equip_paint"),
        shutter: choose(rng, lib, &["painted_metal_shutter"]),
        sign: [lib.id("sign_red"), lib.id("sign_blue"), lib.id("sign_white"), lib.id("sign_yellow"), lib.id("sign_black"), lib.id("sign_green")],
        awning: [lib.id("awning_red"), lib.id("awning_green"), lib.id("awning_navy"), lib.id("awning_cream")],
        dirt: rng.range(0.22, 0.4),
        inner,
        inner_tint: rng.pick(&inner_tints),
        // (a picture covers 1.7 times its real size inside, so that the pattern of a plaster does not repeat so often down a long wall)
        inner_size: { let z = lib.mats[inner].size; [z[0] * 1.7, z[1] * 1.7] },
        floor: choose(rng, lib, &["laminate_floor_02", "wood_floor", "wood_dark"]),
        floor_tint: [1.0; 3],
        ceiling: lib.id("ceiling_white"),
    }
}

/// How the walls of a volume are made.
#[derive(Clone, Copy, Debug)]
enum Walls {
    Punched(Punched, Ground),
    Curtain { module: f32, lobby: bool },
    Industrial,
}

/// The outside of one volume: its four walls and their crowns. Returns the height of the last floor line above the ground floor's surface.
#[allow(clippy::too_many_arguments)]
fn volume(mb: &mut MeshBuilder, rng: &mut Rng, lib: &Lib, pal: &Palette, foot: &Foot, sides: [Side; 4], y0: f32, floors: u32, ground_h: f32, floor_h: f32, walls: Walls, cornice: f32, parapet: f32, doors: &mut Vec<DoorSpec>) -> f32 {
    let corners = [foot.at(foot.x0, foot.z0, y0), foot.at(foot.x1, foot.z0, y0), foot.at(foot.x1, foot.z1, y0), foot.at(foot.x0, foot.z1, y0)];
    let centroid = scale(add(add(corners[0], corners[1]), add(corners[2], corners[3])), 0.25);
    let mut top = 0.0;
    for e in 0..4 {
        let (p, q) = (corners[e], corners[(e + 1) % 4]);
        let (fr, length) = facade::wall_frame(p, q, centroid);
        let mut r = rng.fork(e as u64 + 1);
        let t = match (sides[e], walls) {
            (Side::Street, Walls::Punched(spec, ground)) => facade::punched(mb, &mut r, lib, pal, &fr, length, floors, ground_h, floor_h, &spec, ground, doors),
            (Side::Street, Walls::Curtain { module, lobby }) => facade::curtain(mb, &mut r, lib, pal, &fr, length, floors, ground_h, floor_h, module, lobby),
            (side, Walls::Industrial) => facade::industrial(mb, &mut r, lib, pal, &fr, length, ground_h, side == Side::Street, side == Side::Party, doors),
            (Side::Party, _) => facade::plain_wall(mb, &mut r, lib, pal, &fr, length, floors, ground_h, floor_h, false),
            (Side::Open, _) => facade::plain_wall(mb, &mut r, lib, pal, &fr, length, floors, ground_h, floor_h, true),
        };
        top = t;
        let street = sides[e] == Side::Street;
        facade::crown(mb, &mut r, lib, pal, &fr, length, t, if street { cornice } else { 0.0 }, parapet);
    }
    top
}

/// The roof of a flat-roofed volume at height `y` (the parapet stands on the walls, `crown` made it): the slab, and what stands on a roof: air handlers, a stair house, a tank, an antenna.
#[allow(clippy::too_many_arguments)]
fn flat_roof(mb: &mut MeshBuilder, rng: &mut Rng, lib: &Lib, pal: &Palette, corners: [V3; 4], y: f32, equipment: u32) {
    let tex = lib.tex(pal.roof, rng);
    let p = [[corners[0][0], y, corners[0][2]], [corners[1][0], y, corners[1][2]], [corners[2][0], y, corners[2][2]], [corners[3][0], y, corners[3][2]]];
    let metres = [[p[0][0], p[0][2]], [p[1][0], p[1][2]], [p[2][0], p[2][2]], [p[3][0], p[3][2]]];
    mb.quad_tex(pal.roof, p, metres, &tex, [0.9; 3], [0.0, 1.0, 0.0]);
    // equipment goes in the frame of the first edge: s along it, d from 0 (the wall) inwards (negative)
    let centroid = scale(add(add(corners[0], corners[1]), add(corners[2], corners[3])), 0.25);
    let (fr, length) = facade::wall_frame([corners[0][0], y, corners[0][2]], [corners[1][0], y, corners[1][2]], [centroid[0], y, centroid[2]]);
    let depth = len(sub(corners[1], corners[2]));
    let (margin, eq_tex) = (1.6, lib.tex_plain(pal.equip));
    if length < 6.0 || depth < 6.0 {
        return;
    }
    for _ in 0..equipment {
        let (bw, bd, bh) = (rng.range(1.2, 3.8), rng.range(1.0, 2.6), rng.range(0.9, 2.2));
        let s = rng.range(margin, length - margin - bw);
        let d = -rng.range(margin + bd, depth - margin);
        let kind = rng.int(0, 3);
        let m = if kind == 0 { lib.id("equip_metal") } else { pal.equip };
        mb.cuboid(m, &fr, [s, 0.0, d - bd], [s + bw, bh, d], 1 | 2 | 4 | 16 | 32, &eq_tex, [1.0; 3]);
        // a fan or a grille on top of an air handler
        if kind <= 1 {
            let rub = lib.id("rubber");
            let (cx, cz) = (s + bw * 0.5, d - bd * 0.5);
            let r = (bw.min(bd) * 0.32).max(0.2);
            mb.quad(rub, [fr.at(cx - r, bh + 0.01, cz - r), fr.at(cx + r, bh + 0.01, cz - r), fr.at(cx + r, bh + 0.01, cz + r), fr.at(cx - r, bh + 0.01, cz + r)], [[0.0; 2]; 4], [[1.0; 3]; 4], fr.b);
        }
    }
    if rng.chance(0.7) {
        // the stair house
        let (bw, bd, bh) = (rng.range(3.0, 4.5), rng.range(2.6, 3.8), 2.9);
        let s = rng.range(margin, length - margin - bw);
        let d = -rng.range(margin + bd, depth - margin);
        let t = lib.tex(pal.wall, rng);
        mb.cuboid(pal.wall, &fr, [s, 0.0, d - bd], [s + bw, bh, d], 1 | 2 | 4 | 16 | 32, &t, col3(pal.wall_tint, 0.9));
        mb.cuboid(pal.trim, &fr, [s - 0.05, bh, d - bd - 0.05], [s + bw + 0.05, bh + 0.12, d + 0.05], 1 | 2 | 4 | 16 | 32, &lib.tex_plain(pal.trim), pal.trim_tint);
    }
    if rng.chance(0.3) {
        // a water tank on legs
        let (r, h) = (rng.range(1.0, 1.6), rng.range(2.0, 2.8));
        let s = rng.range(margin + r, length - margin - r);
        let d = -rng.range(margin + r, depth - margin - r);
        mb.cuboid(pal.equip, &fr, [s - r, 1.2, d - r], [s + r, 1.2 + h, d + r], 1 | 2 | 4 | 8 | 16 | 32, &eq_tex, [1.0; 3]);
        for (a, b) in [(-r, -r), (r - 0.12, -r), (-r, r - 0.12), (r - 0.12, r - 0.12)] {
            mb.cuboid(lib.id("equip_metal"), &fr, [s + a, 0.0, d + b], [s + a + 0.12, 1.2, d + b + 0.12], 1 | 2 | 16 | 32, &eq_tex, [1.0; 3]);
        }
    }
    if rng.chance(0.5) {
        let s = rng.range(margin, length - margin);
        let d = -rng.range(margin, depth - margin);
        mb.cuboid(lib.id("frame_dark"), &fr, [s, 0.0, d], [s + 0.1, rng.range(3.0, 9.0), d + 0.1], 1 | 2 | 16 | 32 | 4, &eq_tex, [1.0; 3]);
    }
}

/// A gabled roof over a footprint, its ridge along the front: two sloping planes of tiles with an overhang at the eaves, a gable of wall at each end and a chimney.
fn pitched_roof(mb: &mut MeshBuilder, rng: &mut Rng, lib: &Lib, pal: &Palette, foot: &Foot, y: f32) {
    let (x0, x1, z0, z1) = (foot.x0, foot.x1, foot.z0, foot.z1);
    let e = 0.5;
    let pitch = rng.range(27.0f32, 36.0).to_radians();
    let (zc, half) = ((z0 + z1) * 0.5, (z1 - z0) * 0.5);
    let rise = half * pitch.tan();
    let eave = y - e * pitch.tan() + 0.02;
    let tex = lib.tex(pal.roof, rng);
    let at = |x: f32, z: f32, h: f32| foot.at(x, z, h);
    let slope = ((half + e).powi(2) + (rise + (y - eave)).powi(2)).sqrt();
    let (st, ct) = (pitch.sin(), pitch.cos());
    for (zf, zs, towards) in [(z0 - e, z0 - e, -1.0f32), (z1 + e, z1 + e, 1.0)] {
        let _ = zs;
        let want = add(scale(foot.ez, towards * st), [0.0, ct, 0.0]);
        let p = [at(x0 - e, zf, eave), at(x1 + e, zf, eave), at(x1 + e, zc, y + rise), at(x0 - e, zc, y + rise)];
        mb.quad_tex(pal.roof, p, [[x0 - e, 0.0], [x1 + e, 0.0], [x1 + e, slope], [x0 - e, slope]], &tex, [0.92; 3], want);
    }
    // the gables
    let wt = lib.tex(pal.wall, rng);
    for (x, out) in [(x0, -1.0f32), (x1, 1.0)] {
        let p = [at(x, z0, y), at(x, z1, y), at(x, zc, y + rise), at(x, zc, y + rise)];
        mb.quad_tex(pal.wall, p, [[z0, y], [z1, y], [zc, y + rise], [zc, y + rise]], &wt, col3(pal.wall_tint, 0.9), scale(foot.ex, out));
    }
    // a chimney on the ridge
    if rng.chance(0.7) {
        let cx = rng.range(x0 + 1.5, x1 - 1.5);
        let fr = foot.frame();
        let brick = lib.id(if lib.has("dark_brick_wall") { "dark_brick_wall" } else { "equip_paint" });
        let bt = lib.tex(brick, rng);
        mb.cuboid(brick, &fr, [cx, y, zc - 0.3], [cx + 0.7, y + rise + 1.0, zc + 0.3], 1 | 2 | 4 | 16 | 32, &bt, [0.8; 3]);
        mb.cuboid(pal.trim, &fr, [cx - 0.06, y + rise + 1.0, zc - 0.36], [cx + 0.76, y + rise + 1.12, zc + 0.36], 1 | 2 | 4 | 16 | 32, &lib.tex_plain(pal.trim), pal.trim_tint);
    }
}

/// A spire: three boxes, each narrower than the one under it, and a mast, of steel, standing on `base` (the middle of its foot) and `h` high overall.
fn spire(mb: &mut MeshBuilder, lib: &Lib, base: V3, w: f32, h: f32) {
    let steel = lib.id("steel");
    let t = lib.tex_plain(steel);
    let fr = Frame { o: base, a: [1.0, 0.0, 0.0], b: [0.0, 1.0, 0.0], c: [0.0, 0.0, 1.0] };
    let mut y = 0.0;
    for (k, (width, tall)) in [(w, 0.30), (w * 0.62, 0.28), (w * 0.34, 0.22)].into_iter().enumerate() {
        let top = y + h * tall;
        let half = width * 0.5;
        mb.cuboid(steel, &fr, [-half, y, -half], [half, top, half], if k == 0 { 63 } else { 63 & !16 }, &t, [0.85; 3]);
        y = top;
    }
    let mast = 0.12;
    mb.cuboid(steel, &fr, [-mast, y, -mast], [mast, h * 1.18, mast], 63 & !16, &t, [0.9; 3]);
}

/// How a door's leaf stands when it is left in the model: how far it is turned (radians), and whether it is glass (a shop's, often).
fn leaf_style(rng: &mut Rng, d: &DoorSpec, shop: bool) -> (f32, bool) {
    let angle = if d.width < 1.4 { rng.range(70.0, 82.0) } else { rng.range(50.0, 78.0) }.to_radians();
    let glass = shop && rng.chance(0.65);
    (angle, glass)
}

/// A door leaf hinged on the left jamb of `d`, turned `angle` radians into the building from the plane of the wall: a slab of wood (or a pane of glass in a dark frame for a shop), with a handle.
fn door_leaf(mb: &mut MeshBuilder, lib: &Lib, d: &DoorSpec, angle: f32, glass: bool) {
    let (sa, ca) = (angle.sin(), angle.cos());
    // the leaf's own frame: along the leaf, up, and out of the leaf's front face (a right-handed frame as `Frame::wall` makes)
    let along = add(scale(d.along, ca), scale(d.out, -sa));
    let out = add(scale(d.out, ca), scale(d.along, sa));
    let up = [0.0, 1.0, 0.0];
    let hinge = add(d.hinge, scale(d.along, 0.045));
    let fr = Frame { o: hinge, a: along, b: up, c: out };
    let (w, h, t) = (d.width - 0.1, d.height - 0.06, 0.045);
    let mat = lib.id(if glass { "door_glass" } else { "door_wood" });
    let tex = lib.tex_plain(mat);
    mb.cuboid(mat, &fr, [0.0, 0.03, -t * 0.5], [w, h, t * 0.5], 63, &tex, [1.0; 3]);
    if glass {
        let fm = lib.id("frame_dark");
        let ft = lib.tex_plain(fm);
        for (a0, a1, b0, b1) in [(0.0, 0.07, 0.03, h), (w - 0.07, w, 0.03, h), (0.0, w, h - 0.07, h), (0.0, w, 0.03, 0.2)] {
            mb.cuboid(fm, &fr, [a0, b0, -t * 0.7], [a1, b1, t * 0.7], 63, &ft, [1.0; 3]);
        }
    }
    // a handle on the free edge
    let steel = lib.id("steel");
    let st = lib.tex_plain(steel);
    mb.cuboid(steel, &fr, [w - 0.14, 1.0, t * 0.5], [w - 0.1, 1.04, t * 0.5 + 0.09], 63, &st, [1.0; 3]);
}

/// The lot of a house: lawn round it (in strips, so that nothing lies in the house itself), a path from its door to the pavement, a step at the door.
fn yard(mb: &mut MeshBuilder, rng: &mut Rng, lib: &Lib, spec: &Spec, doors: &[DoorSpec]) {
    let lot = &spec.lot;
    let hf = &spec.vols[0].foot;
    let y = lot.o[1] + 0.012;
    let grass = lib.id(if lib.has("grass_ground") { "grass_ground" } else { "equip_paint" });
    let gt = lib.tex_plain(grass);
    let tint = [0.80, 1.14, 0.62];
    let mut strip = |x0: f32, x1: f32, z0: f32, z1: f32| {
        if x1 - x0 < 0.05 || z1 - z0 < 0.05 {
            return;
        }
        let p = [lot.at(x0, z0), lot.at(x1, z0), lot.at(x1, z1), lot.at(x0, z1)].map(|c| [c[0], y, c[2]]);
        mb.quad_tex(grass, p, [[p[0][0], p[0][2]], [p[1][0], p[1][2]], [p[2][0], p[2][2]], [p[3][0], p[3][2]]], &gt, tint, [0.0, 1.0, 0.0]);
    };
    let wall = WALL_T;
    strip(0.0, lot.w, 0.0, hf.z0);
    strip(0.0, lot.w, hf.z1, lot.d);
    strip(0.0, hf.x0, hf.z0, hf.z1);
    strip(hf.x1, lot.w, hf.z0, hf.z1);
    let _ = wall;
    // the path
    if let Some(dr) = doors.first() {
        let paving = lib.id(if lib.has("concrete_pavers") { "concrete_pavers" } else { "equip_paint" });
        let pt = lib.tex_plain(paving);
        let mid = [dr.hinge[0] + dr.along[0] * dr.width * 0.5, lot.o[1] + 0.02, dr.hinge[2] + dr.along[2] * dr.width * 0.5];
        let w = 0.7;
        let length = (hf.z0 - 0.0).max(1.0);
        let a = [mid[0] - dr.along[0] * w, mid[1], mid[2] - dr.along[2] * w];
        let b = [mid[0] + dr.along[0] * w, mid[1], mid[2] + dr.along[2] * w];
        let c = add(b, scale(dr.out, length));
        let d = add(a, scale(dr.out, length));
        mb.quad_tex(paving, [a, b, c, d], [[a[0], a[2]], [b[0], b[2]], [c[0], c[2]], [d[0], d[2]]], &pt, [0.95; 3], [0.0, 1.0, 0.0]);
        // a step up to the door
        let step = lib.id(if lib.has("concrete_wall_007") { "concrete_wall_007" } else { "equip_paint" });
        let st = lib.tex_plain(step);
        let fr = Frame { o: [mid[0], 0.0, mid[2]], a: dr.along, b: [0.0, 1.0, 0.0], c: dr.out };
        mb.cuboid(step, &fr, [-1.1, lot.o[1], 0.0], [1.1, lot.o[1] + 0.12, 0.7], 1 | 2 | 4 | 16, &st, [0.9; 3]);
    }
    let _ = rng;
}

fn col3(t: V3, k: f32) -> V3 {
    [t[0] * k, t[1] * k, t[2] * k]
}

/// A foot grown by `by` metres on every side (the inside of a curtain wall begins a hand's width behind the glass, not a wall's thickness).
fn grown(f: &Foot, by: f32) -> Foot {
    Foot { x0: f.x0 - by, x1: f.x1 + by, z0: f.z0 - by, z1: f.z1 + by, ..*f }
}

/// One block of a building: its footprint, its floors and how its walls are made. A tower has two (the podium and the tower on it).
#[derive(Clone, Copy, Debug)]
pub struct VolSpec {
    pub foot: Foot,
    /// Height of the surface of this volume's first floor.
    pub y0: f32,
    pub floors: u32,
    pub ground_h: f32,
    pub floor_h: f32,
    walls: Walls,
    pub cornice: f32,
    pub parapet: f32,
    pub pal: Palette,
    /// The footprint of the inside (a curtain wall's inside begins a hand's width behind the glass) and the height of its first floor (a hair above the roof below, for a tower).
    pub inner_foot: Foot,
    pub inner_y0: f32,
}

impl VolSpec {
    /// A glass tower: its walls are a curtain wall (no punched windows).
    pub fn is_curtain(&self) -> bool {
        matches!(self.walls, Walls::Curtain { .. })
    }

    /// The height of the roof level of this volume (the top of its top floor's walls, before the parapet).
    pub fn top(&self) -> f32 {
        self.y0 + self.ground_h + (self.floors - 1) as f32 * self.floor_h
    }
}

/// What a building is, decided from its key alone (no geometry is made): the volumes, the roofs, how the floors are used.
#[derive(Clone, Debug)]
pub struct Spec {
    pub kind: Kind,
    pub key: u64,
    pub lot: Lot,
    pub vols: Vec<VolSpec>,
    /// (the corners of a roof at its height, how many pieces of equipment stand on it)
    roofs: Vec<([V3; 4], u32)>,
    ground: Ground,
    offices: bool,
    /// The roof is a gabled one over the whole footprint (a house), not a flat one with equipment.
    pitched: bool,
    /// Spires on the roofs: (the middle of the base at roof height, the width of the base, the height).
    pub spires: Vec<(V3, f32, f32)>,
}

impl Spec {
    pub fn floor_count(&self) -> u32 {
        self.vols.iter().map(|v| v.floors).sum()
    }

    /// (the volume, the floor within it) of floor `k` counted from the bottom of the building.
    fn locate(&self, k: u32) -> (usize, u32) {
        let mut k = k;
        for (i, v) in self.vols.iter().enumerate() {
            if k < v.floors {
                return (i, k);
            }
            k -= v.floors;
        }
        let last = self.vols.len() - 1;
        (last, self.vols[last].floors - 1)
    }

    fn occupancy(&self, vol: usize, k: u32) -> Occupancy {
        match self.kind {
            Kind::Masonry => {
                if k == 0 {
                    if self.ground == Ground::Shops { Occupancy::Retail } else { Occupancy::Lobby }
                } else if self.offices {
                    Occupancy::Office
                } else {
                    Occupancy::Apartments
                }
            }
            Kind::Apartment => {
                if k == 0 { Occupancy::Lobby } else { Occupancy::Apartments }
            }
            Kind::Shophouse => {
                if k == 0 { Occupancy::Retail } else { Occupancy::Apartments }
            }
            Kind::Tower | Kind::Skyscraper => {
                if vol == 0 && k == 0 { Occupancy::Lobby } else { Occupancy::Office }
            }
            Kind::House => Occupancy::Home,
            Kind::Warehouse => Occupancy::Warehouse,
        }
    }

    /// The height of the middle of floor `k` of the building (counted from the ground floor).
    pub fn mid(&self, k: u32) -> f32 {
        let (v, kk) = self.locate(k);
        let vs = &self.vols[v];
        Levels { y0: vs.inner_y0, floors: vs.floors, ground_h: vs.ground_h, floor_h: vs.floor_h }.mid(kk)
    }

    /// The building as data for a modelling program (`export.rs`): its volumes (footprint, floors, heights, how the walls are made), roofs, spires, and the doors the outside makes (`doors`, with whether each leaf is glass).
    pub fn to_json(&self, doors: &[DoorSpec], glass: &[bool]) -> lunaris_json::Json {
        use lunaris_json::Json;
        let n = |x: f32| Json::Float(x as f64);
        let v3 = |a: V3| Json::arr(a.map(n));
        let foot = |f: &Foot| Json::obj([("o", v3(f.o)), ("ex", v3(f.ex)), ("ez", v3(f.ez)), ("x0", n(f.x0)), ("x1", n(f.x1)), ("z0", n(f.z0)), ("z1", n(f.z1))]);
        let side = |s: &Side| Json::str(match s { Side::Street => "street", Side::Party => "party", Side::Open => "open" });
        let ground = |g: &Ground| Json::str(match g { Ground::Shops => "shops", Ground::Lobby => "lobby", Ground::Plain => "plain" });
        let vols: Vec<Json> = self
            .vols
            .iter()
            .map(|v| {
                let (style, walls) = match v.walls {
                    Walls::Punched(p, g) => ("punched", Json::obj([("bay", n(p.bay)), ("win_w", n(p.win_w)), ("win_h", n(p.win_h)), ("sill_h", n(p.sill_h)), ("band_every", Json::from(p.band_every as i64)), ("ac_chance", n(p.ac_chance)), ("lintels", Json::Bool(p.lintels)), ("shutters", Json::Bool(p.shutters)), ("balcony", n(p.balcony)), ("ground", ground(&g))])),
                    Walls::Curtain { module, lobby } => ("curtain", Json::obj([("module", n(module)), ("lobby", Json::Bool(lobby))])),
                    Walls::Industrial => ("industrial", Json::obj(Vec::<(&str, Json)>::new())),
                };
                Json::obj([("foot", foot(&v.foot)), ("y0", n(v.y0)), ("floors", Json::from(v.floors as i64)), ("ground_h", n(v.ground_h)), ("floor_h", n(v.floor_h)), ("cornice", n(v.cornice)), ("parapet", n(v.parapet)), ("style", Json::str(style)), ("walls", walls)])
            })
            .collect();
        let roofs: Vec<Json> = self.roofs.iter().map(|(c, eq)| Json::obj([("corners", Json::arr(c.iter().map(|p| v3(*p)))), ("equipment", Json::from(*eq as i64))])).collect();
        let spires: Vec<Json> = self.spires.iter().map(|(b, w, h)| Json::obj([("base", v3(*b)), ("w", n(*w)), ("h", n(*h))])).collect();
        let doors: Vec<Json> = doors.iter().enumerate().map(|(k, d)| Json::obj([("hinge", v3(d.hinge)), ("along", v3(d.along)), ("out", v3(d.out)), ("width", n(d.width)), ("height", n(d.height)), ("glass", Json::Bool(glass.get(k).copied().unwrap_or(false)))])).collect();
        Json::obj([
            ("kind", Json::str(format!("{:?}", self.kind))),
            ("key", Json::str(self.key.to_string())),
            ("lot", Json::obj([("o", v3(self.lot.o)), ("ex", v3(self.lot.ex)), ("ez", v3(self.lot.ez)), ("w", n(self.lot.w)), ("d", n(self.lot.d)), ("sides", Json::arr(self.lot.sides.iter().map(side)))])),
            ("floors", Json::from(self.floor_count() as i64)),
            ("height", n(self.height())),
            ("ground", ground(&self.ground)),
            ("offices", Json::Bool(self.offices)),
            ("pitched", Json::Bool(self.pitched)),
            ("vols", Json::Arr(vols)),
            ("roofs", Json::Arr(roofs)),
            ("spires", Json::Arr(spires)),
            ("doors", Json::Arr(doors)),
        ])
    }

    /// The height of the top of the building.
    pub fn height(&self) -> f32 {
        let v = self.vols.last().unwrap();
        v.y0 - self.vols[0].y0 + v.ground_h + (v.floors - 1) as f32 * v.floor_h + v.parapet
    }
}

/// Decide a building from its lot, its kind and its key.
pub fn spec_of(lib: &Lib, lot: &Lot, kind: Kind, key: u64) -> Spec {
    let mut rng = Rng::keyed(&[key, 1]);
    let pal = palette(lib, kind, &mut rng);
    let foot = lot.foot();
    let y0 = lot.o[1];
    let corners = [foot.at(0.0, 0.0, y0), foot.at(lot.w, 0.0, y0), foot.at(lot.w, lot.d, y0), foot.at(0.0, lot.d, y0)];
    let one = |floors: u32, ground_h: f32, floor_h: f32, walls: Walls, cornice: f32, parapet: f32| VolSpec { foot, y0, floors, ground_h, floor_h, walls, cornice, parapet, pal, inner_foot: foot, inner_y0: y0 };
    match kind {
        Kind::Masonry => {
            let floors = rng.int(5, 8) as u32;
            let ground_h = rng.range(4.2, 4.8);
            let floor_h = rng.range(3.2, 3.5);
            let spec = Punched { bay: rng.range(3.3, 4.1), win_w: rng.range(0.40, 0.48), win_h: rng.range(1.65, 1.9), sill_h: rng.range(0.85, 1.0), band_every: if rng.chance(0.5) { 2 } else { 0 }, ac_chance: 0.16, lintels: rng.chance(0.6), shutters: rng.chance(0.15), balcony: if rng.chance(0.25) { 0.2 } else { 0.0 } };
            let ground = if rng.chance(0.7) { Ground::Shops } else { Ground::Lobby };
            let cornice = rng.range(0.3, 0.5);
            let eq = rng.int(1, 4) as u32;
            let offices = rng.chance(0.5);
            let v = one(floors, ground_h, floor_h, Walls::Punched(spec, ground), cornice, 0.9);
            let top = y0 + ground_h + (floors - 1) as f32 * floor_h;
            Spec { kind, key, lot: *lot, vols: vec![v], roofs: vec![(corners.map(|c| [c[0], top, c[2]]), eq)], ground, offices, pitched: false, spires: Vec::new() }
        }
        Kind::Apartment => {
            let floors = rng.int(6, 14) as u32;
            let ground_h = rng.range(3.6, 4.2);
            let floor_h = rng.range(2.9, 3.1);
            let spec = Punched { bay: rng.range(2.9, 3.4), win_w: rng.range(0.50, 0.62), win_h: rng.range(1.5, 1.8), sill_h: rng.range(0.85, 0.95), band_every: 0, ac_chance: 0.38, lintels: false, shutters: false, balcony: if rng.chance(0.6) { rng.range(0.2, 0.55) } else { 0.0 } };
            let ground = if rng.chance(0.5) { Ground::Lobby } else { Ground::Plain };
            let eq = rng.int(1, 3) as u32;
            let v = one(floors, ground_h, floor_h, Walls::Punched(spec, ground), 0.0, 0.8);
            let top = y0 + ground_h + (floors - 1) as f32 * floor_h;
            Spec { kind, key, lot: *lot, vols: vec![v], roofs: vec![(corners.map(|c| [c[0], top, c[2]]), eq)], ground, offices: false, pitched: false, spires: Vec::new() }
        }
        Kind::Shophouse => {
            let floors = rng.int(2, 4) as u32;
            let ground_h = rng.range(3.8, 4.4);
            let floor_h = rng.range(3.0, 3.3);
            let spec = Punched { bay: rng.range(3.0, 3.6), win_w: rng.range(0.45, 0.55), win_h: rng.range(1.6, 1.9), sill_h: rng.range(0.8, 0.95), band_every: 0, ac_chance: 0.3, lintels: rng.chance(0.5), shutters: rng.chance(0.3), balcony: if rng.chance(0.2) { 0.3 } else { 0.0 } };
            let cornice = rng.range(0.2, 0.4);
            let eq = rng.int(0, 2) as u32;
            let v = one(floors, ground_h, floor_h, Walls::Punched(spec, Ground::Shops), cornice, 0.7);
            let top = y0 + ground_h + (floors - 1) as f32 * floor_h;
            Spec { kind, key, lot: *lot, vols: vec![v], roofs: vec![(corners.map(|c| [c[0], top, c[2]]), eq)], ground: Ground::Shops, offices: false, pitched: false, spires: Vec::new() }
        }
        Kind::Tower => {
            // a podium of punched stone and a glass tower set back from it
            let pod_floors = rng.int(3, 5) as u32;
            let pod_ground = rng.range(5.0, 6.5);
            let pod_floor_h = rng.range(4.0, 4.6);
            let spec = Punched { bay: rng.range(4.0, 5.0), win_w: rng.range(0.55, 0.7), win_h: rng.range(2.4, 2.9), sill_h: 0.9, band_every: 0, ac_chance: 0.0, lintels: true, shutters: false, balcony: 0.0 };
            let mut pal_pod = pal;
            pal_pod.wall_tint = col3(pal.wall_tint, 0.95);
            let pod_cornice = rng.range(0.3, 0.5);
            let mut pod = one(pod_floors, pod_ground, pod_floor_h, Walls::Punched(spec, Ground::Lobby), pod_cornice, 0.0);
            pod.pal = pal_pod;
            let pod_abs = y0 + pod_ground + (pod_floors - 1) as f32 * pod_floor_h;
            // the tower: inset on every side that is not a party wall
            let inset = |s: Side, m: f32| if s == Side::Party { 0.0 } else { m };
            let m = rng.range(3.0, 6.0).min(lot.w * 0.2).min(lot.d * 0.2);
            let tfoot = Foot { x0: inset(lot.sides[3], m), x1: lot.w - inset(lot.sides[1], m), z0: inset(lot.sides[0], m), z1: lot.d - inset(lot.sides[2], m), ..foot };
            let floors = rng.int(12, 36) as u32;
            let floor_h = rng.range(3.8, 4.1);
            let module = rng.range(1.5, 1.9);
            let eq = rng.int(1, 3) as u32;
            let tower = VolSpec { foot: tfoot, y0: pod_abs, floors, ground_h: floor_h, floor_h, walls: Walls::Curtain { module, lobby: false }, cornice: 0.0, parapet: 1.0, pal, inner_foot: grown(&tfoot, WALL_T - 0.08), inner_y0: pod_abs + 0.012 };
            let tc = [tfoot.at(tfoot.x0, tfoot.z0, 0.0), tfoot.at(tfoot.x1, tfoot.z0, 0.0), tfoot.at(tfoot.x1, tfoot.z1, 0.0), tfoot.at(tfoot.x0, tfoot.z1, 0.0)];
            let top = pod_abs + floor_h * floors as f32;
            Spec { kind, key, lot: *lot, vols: vec![pod, tower], roofs: vec![(corners.map(|c| [c[0], pod_abs, c[2]]), 0), (tc.map(|c| [c[0], top, c[2]]), eq)], ground: Ground::Lobby, offices: true, pitched: false, spires: Vec::new() }
        }
        Kind::Skyscraper => {
            // a podium of punched stone, then one to three tiers (each a step in from the one below), then a crown
            let pod_floors = rng.int(3, 6) as u32;
            let pod_ground = rng.range(5.5, 7.5);
            let pod_floor_h = rng.range(4.2, 5.0);
            let pod_spec = Punched { bay: rng.range(4.2, 5.4), win_w: rng.range(0.55, 0.7), win_h: rng.range(2.6, 3.2), sill_h: 0.9, band_every: 0, ac_chance: 0.0, lintels: true, shutters: false, balcony: 0.0 };
            let mut pal_pod = pal;
            pal_pod.wall_tint = col3(pal.wall_tint, 0.95);
            let mut pod = one(pod_floors, pod_ground, pod_floor_h, Walls::Punched(pod_spec, Ground::Lobby), rng.range(0.3, 0.5), 0.0);
            pod.pal = pal_pod;
            let pod_abs = y0 + pod_ground + (pod_floors - 1) as f32 * pod_floor_h;
            let inset = |s: Side, m: f32| if s == Side::Party { 0.0 } else { m };
            let m0 = rng.range(3.0, 6.0).min(lot.w * 0.15).min(lot.d * 0.15);
            let mut tfoot = Foot { x0: inset(lot.sides[3], m0), x1: lot.w - inset(lot.sides[1], m0), z0: inset(lot.sides[0], m0), z1: lot.d - inset(lot.sides[2], m0), ..foot };
            let total = (32.0 + 46.0 * rng.f32().powf(1.5)) as u32;
            // the footprints of the tiers, each a step in from the last, as many as the lot leaves room for (a tier narrower than twelve metres has no floor plan)
            let want = 1 + rng.weighted(&[0.25, 0.4, 0.35]);
            let mut foots = vec![tfoot];
            while foots.len() < want {
                let f = *foots.last().expect("one");
                let m = rng.range(3.5, 7.5).min((f.x1 - f.x0) * 0.2).min((f.z1 - f.z0) * 0.2);
                let nf = Foot { x0: f.x0 + m, x1: f.x1 - m, z0: f.z0 + m, z1: f.z1 - m, ..foot };
                if nf.x1 - nf.x0 < 12.0 || nf.z1 - nf.z0 < 11.0 {
                    break;
                }
                foots.push(nf);
            }
            let shares: &[f32] = match foots.len() {
                1 => &[1.0],
                2 => &[0.62, 0.38],
                _ => &[0.5, 0.3, 0.2],
            };
            // a glass skin, or (now and then) a punched one of stone or concrete
            let stone = rng.chance(0.28);
            let module = rng.range(1.5, 1.9);
            let floor_h = rng.range(3.9, 4.2);
            let stone_spec = Punched { bay: rng.range(3.4, 4.2), win_w: rng.range(0.40, 0.52), win_h: rng.range(2.0, 2.4), sill_h: 0.9, band_every: if rng.chance(0.5) { 4 } else { 0 }, ac_chance: 0.0, lintels: true, shutters: false, balcony: 0.0 };
            let mut vols = vec![pod];
            let mut roofs: Vec<([V3; 4], u32)> = vec![(corners.map(|c| [c[0], pod_abs, c[2]]), 0)];
            let mut y_abs = pod_abs;
            let mut left = total;
            for (t, share) in shares.iter().enumerate() {
                let floors = if t + 1 == shares.len() { left } else { ((total as f32 * share).round() as u32).clamp(4, left.saturating_sub(4).max(4)) }.max(3);
                left = left.saturating_sub(floors);
                tfoot = foots[t];
                let walls = if stone { Walls::Punched(stone_spec, Ground::Plain) } else { Walls::Curtain { module, lobby: false } };
                let tier = VolSpec { foot: tfoot, y0: y_abs, floors, ground_h: floor_h, floor_h, walls, cornice: if stone { 0.35 } else { 0.0 }, parapet: 1.0, pal, inner_foot: grown(&tfoot, WALL_T - 0.08), inner_y0: y_abs + 0.012 };
                let top = y_abs + floor_h * floors as f32;
                let tc = [tfoot.at(tfoot.x0, tfoot.z0, 0.0), tfoot.at(tfoot.x1, tfoot.z0, 0.0), tfoot.at(tfoot.x1, tfoot.z1, 0.0), tfoot.at(tfoot.x0, tfoot.z1, 0.0)];
                vols.push(tier);
                roofs.push((tc.map(|c| [c[0], top, c[2]]), 0));
                y_abs = top;
            }
            // the crown: equipment on the roof, a smaller block on top of it, or a spire
            let mut spires: Vec<(V3, f32, f32)> = Vec::new();
            let centre = |f: &Foot, y: f32| f.at((f.x0 + f.x1) * 0.5, (f.z0 + f.z1) * 0.5, y);
            match rng.weighted(&[0.4, 0.25, 0.35]) {
                1 => {
                    let w = tfoot.x1 - tfoot.x0;
                    let d = tfoot.z1 - tfoot.z0;
                    let (ax, az) = (w * rng.range(0.14, 0.22), d * rng.range(0.14, 0.22));
                    let cfoot = Foot { x0: tfoot.x0 + ax, x1: tfoot.x1 - ax, z0: tfoot.z0 + az, z1: tfoot.z1 - az, ..foot };
                    if cfoot.x1 - cfoot.x0 < 9.0 || cfoot.z1 - cfoot.z0 < 8.5 {
                        // too narrow for a floor plan: a spire on the roof instead
                        spires.push((centre(&tfoot, y_abs), (w.min(d) * 0.22).clamp(2.0, 6.0), rng.range(18.0, 50.0)));
                    } else {
                    let floors = rng.int(3, 5) as u32;
                    let crown = VolSpec { foot: cfoot, y0: y_abs, floors, ground_h: floor_h, floor_h, walls: Walls::Curtain { module, lobby: false }, cornice: 0.0, parapet: 0.8, pal, inner_foot: grown(&cfoot, WALL_T - 0.08), inner_y0: y_abs + 0.012 };
                    let top = y_abs + floor_h * floors as f32;
                    let cc = [cfoot.at(cfoot.x0, cfoot.z0, 0.0), cfoot.at(cfoot.x1, cfoot.z0, 0.0), cfoot.at(cfoot.x1, cfoot.z1, 0.0), cfoot.at(cfoot.x0, cfoot.z1, 0.0)];
                    vols.push(crown);
                    roofs.push((cc.map(|c| [c[0], top, c[2]]), 0));
                    if rng.chance(0.55) {
                        spires.push((centre(&cfoot, top), (cfoot.x1 - cfoot.x0).min(cfoot.z1 - cfoot.z0) * 0.3, rng.range(14.0, 40.0)));
                    }
                    }
                }
                2 => {
                    let w = (tfoot.x1 - tfoot.x0).min(tfoot.z1 - tfoot.z0);
                    spires.push((centre(&tfoot, y_abs), (w * 0.22).clamp(2.0, 6.0), rng.range(18.0, 55.0)));
                }
                _ => {
                    let last = roofs.len() - 1;
                    roofs[last].1 = rng.int(1, 3) as u32;
                }
            }
            Spec { kind, key, lot: *lot, vols, roofs, ground: Ground::Lobby, offices: true, pitched: false, spires }
        }
        Kind::Warehouse => {
            let height = rng.range(6.5, 9.0);
            let v = VolSpec { foot, y0, floors: 1, ground_h: height, floor_h: height, walls: Walls::Industrial, cornice: 0.0, parapet: 0.7, pal, inner_foot: foot, inner_y0: y0 };
            let top = y0 + height;
            let eq = rng.int(2, 6) as u32;
            Spec { kind, key, lot: *lot, vols: vec![v], roofs: vec![(corners.map(|c| [c[0], top, c[2]]), eq)], ground: Ground::Plain, offices: false, pitched: false, spires: Vec::new() }
        }
        Kind::House => {
            // set back behind a lawn, a gap at each side, a yard behind; every wall has windows (a house has no party walls)
            let floors: u32 = if rng.chance(0.25) { 1 } else if rng.chance(0.75) { 2 } else { 3 };
            let gap = rng.range(2.2, 3.2);
            let rear = 1.6;
            let d_h = (lot.d - 4.2 - rear).clamp(10.8, 13.0);
            let setback = (lot.d - d_h - rear).clamp(3.2, 6.8);
            let hfoot = Foot { x0: gap, x1: lot.w - gap, z0: setback, z1: setback + d_h, ..foot };
            let (ground_h, floor_h) = (2.9, 2.8);
            let spec = Punched { bay: rng.range(2.9, 3.4), win_w: rng.range(0.42, 0.5), win_h: rng.range(1.3, 1.5), sill_h: rng.range(0.85, 0.95), band_every: 0, ac_chance: 0.08, lintels: rng.chance(0.5), shutters: rng.chance(0.5), balcony: 0.0 };
            let v = VolSpec { foot: hfoot, y0, floors, ground_h, floor_h, walls: Walls::Punched(spec, Ground::Plain), cornice: 0.1, parapet: 0.0, pal, inner_foot: hfoot, inner_y0: y0 };
            let top = y0 + ground_h + (floors - 1) as f32 * floor_h;
            let hc = [hfoot.at(hfoot.x0, hfoot.z0, top), hfoot.at(hfoot.x1, hfoot.z0, top), hfoot.at(hfoot.x1, hfoot.z1, top), hfoot.at(hfoot.x0, hfoot.z1, top)];
            let mut own = *lot;
            own.sides = [Side::Street, Side::Open, Side::Open, Side::Open];
            Spec { kind, key, lot: own, vols: vec![v], roofs: vec![(hc, 0)], ground: Ground::Plain, offices: false, pitched: true, spires: Vec::new() }
        }
    }
}

/// The outside of the building: walls, crowns and roofs added to `mb`. Returns the doors.
pub fn exterior(mb: &mut MeshBuilder, lib: &Lib, spec: &Spec) -> Vec<DoorSpec> {
    exterior_with(mb, lib, spec, true).0
}

/// The outside of the building as `exterior` makes it, and with `leaves` false without the door leaves (the game puts a `Door` entity in every doorway instead); for each door whether its leaf is glass (a shop's).
pub fn exterior_with(mb: &mut MeshBuilder, lib: &Lib, spec: &Spec, leaves: bool) -> (Vec<DoorSpec>, Vec<bool>) {
    let mut rng = Rng::keyed(&[spec.key, 2]);
    let mut doors = Vec::new();
    for v in &spec.vols {
        volume(mb, &mut rng, lib, &v.pal, &v.foot, spec.lot.sides, v.y0, v.floors, v.ground_h, v.floor_h, v.walls, v.cornice, v.parapet, &mut doors);
    }
    for (corners, eq) in &spec.roofs {
        let y = corners[0][1];
        if spec.pitched {
            pitched_roof(mb, &mut rng, lib, &spec.vols[0].pal, &spec.vols[0].foot, y);
        } else {
            flat_roof(mb, &mut rng, lib, &spec.vols[0].pal, *corners, y, *eq);
        }
    }
    for (base, w, h) in &spec.spires {
        spire(mb, lib, *base, *w, *h);
    }
    if spec.kind == Kind::House {
        yard(mb, &mut rng, lib, spec, &doors);
    }
    // a leaf in every doorway, standing ajar into the building (wide enough to pass: the most it turns is 80 degrees)
    let mut leaf_rng = Rng::keyed(&[spec.key, 0xD00]);
    let mut glass = Vec::with_capacity(doors.len());
    for d in &doors {
        let (angle, g) = leaf_style(&mut leaf_rng, d, spec.ground == Ground::Shops);
        glass.push(g);
        if leaves {
            door_leaf(mb, lib, d, angle, g);
        }
    }
    (doors, glass)
}

/// The rooms of floor `k` (counted from the ground floor of the building's first volume): (kind of room, middle of it in the world at the height of the floor, width along x of the lot, depth along z of the lot).
pub fn rooms(spec: &Spec, k: u32) -> Vec<(String, V3, f32, f32)> {
    let (vi, kk) = spec.locate(k);
    let v = &spec.vols[vi];
    let lv = Levels { y0: v.inner_y0, floors: v.floors, ground_h: v.ground_h, floor_h: v.floor_h };
    let Some(list) = interior::room_list(spec.key ^ (0x77 + vi as u64), &v.inner_foot, &lv, spec.occupancy(vi, kk), kk) else { return Vec::new() };
    list.into_iter().map(|(name, r)| (name, v.inner_foot.at((r[0] + r[1]) * 0.5, (r[2] + r[3]) * 0.5, lv.line(kk)), r[1] - r[0], r[3] - r[2])).collect()
}

/// The way up from floor `k` of a building (counted from the ground floor of its first volume): see `interior::stairs_path`.
pub fn stairs_path(spec: &Spec, k: u32) -> Option<[V3; 4]> {
    let (vi, kk) = spec.locate(k);
    let v = &spec.vols[vi];
    let lv = Levels { y0: v.inner_y0, floors: v.floors, ground_h: v.ground_h, floor_h: v.floor_h };
    interior::stairs_path(spec.key ^ (0x77 + vi as u64), &v.inner_foot, &lv, spec.kind == Kind::House, kk)
}

/// The strips of floor in front of the doors from the street, 2.8 m deep and a little wider than the door, in the lot coordinates of `foot`.
fn door_strips(lib: &Lib, spec: &Spec, foot: &Foot) -> Vec<interior::Rect> {
    let doors = exterior(&mut MeshBuilder::new(), lib, spec);
    let to_lot = |p: V3| (dot(sub(p, foot.o), foot.ex), dot(sub(p, foot.o), foot.ez));
    doors
        .iter()
        .map(|d| {
            let mid = add(d.hinge, scale(d.along, d.width * 0.5));
            let (a, b) = (sub(mid, scale(d.along, d.width * 0.5 + 0.5)), add(mid, scale(d.along, d.width * 0.5 + 0.5)));
            let c = add(b, scale(d.out, -2.8));
            let e = add(a, scale(d.out, -2.8));
            let pts = [to_lot(a), to_lot(b), to_lot(c), to_lot(e)];
            let (x0, x1) = (pts.iter().fold(f32::MAX, |m, p| m.min(p.0)), pts.iter().fold(f32::MIN, |m, p| m.max(p.0)));
            let (z0, z1) = (pts.iter().fold(f32::MAX, |m, p| m.min(p.1)), pts.iter().fold(f32::MIN, |m, p| m.max(p.1)));
            interior::Rect::new(x0, x1, z0, z1)
        })
        .collect()
}

/// Floor `k` of the inside (counted from the ground floor of the whole building), its lights, and the height of its middle; None when the building has no inside.
pub fn inside_floor(lib: &Lib, props: &Props, spec: &Spec, k: u32) -> Option<(MeshBuilder, Vec<InLight>, f32)> {
    let (vi, kk) = spec.locate(k);
    let v = &spec.vols[vi];
    let lv = Levels { y0: v.inner_y0, floors: v.floors, ground_h: v.ground_h, floor_h: v.floor_h };
    // what stands just inside a street door would be in the way of whoever comes in: the doors are asked of the outside (it is cheap, a millisecond) and that strip is kept clear
    let entrances: Vec<interior::Rect> = if k == 0 { door_strips(lib, spec, &v.inner_foot) } else { Vec::new() };
    let (mb, mut lights) = interior::build_floor(spec.key ^ (0x77 + vi as u64), lib, props, &v.pal, &v.inner_foot, &lv, spec.occupancy(vi, kk), kk, &entrances)?;
    for l in &mut lights {
        l.floor = k;
    }
    Some((mb, lights, lv.mid(kk)))
}

/// What a game server needs of floor `k` (the partitions, the doors with leaves, the places to search), with the footprint its lot coordinates are in; None when the building has no inside.
pub fn server_floor(spec: &Spec, k: u32) -> Option<(interior::ServerFloor, interior::Foot)> {
    let (vi, kk) = spec.locate(k);
    let v = &spec.vols[vi];
    let lv = Levels { y0: v.inner_y0, floors: v.floors, ground_h: v.ground_h, floor_h: v.floor_h };
    let f = interior::server_floor(spec.key ^ (0x77 + vi as u64), &v.inner_foot, &lv, spec.occupancy(vi, kk), kk)?;
    Some((f, v.inner_foot))
}

/// A building on a lot, all of it: the outside added to `mb`, and its inside returned by floor with the doors and lights. `props` supplies the furniture.
pub fn generate(mb: &mut MeshBuilder, lib: &Lib, props: &Props, lot: &Lot, kind: Kind, rng: &mut Rng) -> Built {
    let key = rng.u64();
    let spec = spec_of(lib, lot, kind, key);
    let doors = exterior(mb, lib, &spec);
    let mut interior = Vec::new();
    let mut floor_mid = Vec::new();
    let mut lights = Vec::new();
    for k in 0..spec.floor_count() {
        match inside_floor(lib, props, &spec, k) {
            Some((f, l, mid)) => {
                interior.push(f);
                floor_mid.push(mid);
                lights.extend(l);
            }
            None => {
                interior.clear();
                floor_mid.clear();
                lights.clear();
                break;
            }
        }
    }
    Built { height: spec.height(), doors, lights, interior, floor_mid }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lot() -> Lot {
        Lot { o: [0.0, 0.15, 0.0], ex: [1.0, 0.0, 0.0], ez: [0.0, 0.0, 1.0], w: 18.0, d: 22.0, sides: [Side::Street, Side::Party, Side::Open, Side::Party] }
    }

    #[test]
    fn every_kind_makes_a_building_with_doors_and_a_furnished_inside_that_stays_over_its_lot() {
        let lib = Lib::from_json("");
        let props = Props { map: Default::default() };
        for (i, kind) in [Kind::Masonry, Kind::Apartment, Kind::Tower, Kind::Shophouse, Kind::House, Kind::Warehouse, Kind::Skyscraper].into_iter().enumerate() {
            let mut mb = MeshBuilder::new();
            let mut rng = Rng::keyed(&[3, i as u64]);
            let b = generate(&mut mb, &lib, &props, &lot(), kind, &mut rng);
            let (lo, hi) = mb.bounds().unwrap();
            assert!(b.height > 2.5 && b.height < 420.0, "{kind:?} height {}", b.height);
            assert!(mb.triangles() > 200, "{kind:?} {} triangles", mb.triangles());
            assert!(lo[0] > -0.7 && hi[0] < 18.7 && lo[2] > -3.5 && hi[2] < 22.7, "{kind:?} {lo:?} {hi:?}");
            assert!(lo[1] > 0.14);
            assert!(!b.doors.is_empty(), "{kind:?} has no door to go in by");
            assert!(!b.interior.is_empty(), "{kind:?}: {} floors inside", b.interior.len());
            assert!(b.interior.iter().all(|f| f.triangles() > 50));
        }
    }

    #[test]
    fn a_floor_is_the_same_whether_it_is_made_alone_or_with_the_others() {
        let lib = Lib::from_json("");
        let props = Props { map: Default::default() };
        let spec = spec_of(&lib, &lot(), Kind::Masonry, 99);
        let alone = inside_floor(&lib, &props, &spec, 2).unwrap();
        let all: Vec<_> = (0..spec.floor_count()).map(|k| inside_floor(&lib, &props, &spec, k).unwrap()).collect();
        assert_eq!(alone.0.triangles(), all[2].0.triangles());
        assert_eq!(alone.1.len(), all[2].1.len());
        assert!((alone.2 - all[2].2).abs() < 1e-6);
        assert!(spec.mid(0) < spec.mid(1) && spec.mid(1) < spec.mid(2));
    }

    #[test]
    fn the_same_lot_and_seed_give_the_same_building() {
        let lib = Lib::from_json("");
        let props = Props { map: Default::default() };
        let make = || {
            let mut mb = MeshBuilder::new();
            let mut rng = Rng::keyed(&[11, 4]);
            let b = generate(&mut mb, &lib, &props, &lot(), Kind::Masonry, &mut rng);
            (mb.triangles(), mb.bounds().unwrap().1, b.interior.iter().map(|f| f.triangles()).sum::<usize>())
        };
        assert_eq!(make(), make());
    }
}
