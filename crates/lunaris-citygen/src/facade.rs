//! Facades: walls with real openings. Every window is a hole in the wall with reveals (the thickness of the wall seen from outside), a pane set back behind a frame with mullions, a sill and sometimes a lintel
//! and an air-conditioning unit; ground floors are shopfronts (glass, doors, signs, awnings, roller shutters) or lobbies; curtain walls are grids of glass and spandrel panels between mullions and fins.
//! All of it is laid out in the wall's own frame (s along the wall, h up, d out of it) and wears photographed materials at their real size, with the dirt that collects near the ground.

use crate::materials::Lib;
use crate::mesh::*;
use crate::rng::Rng;

/// How thick the outer walls are, and how far behind the outer face the panes of the windows sit.
pub const WALL_T: f32 = 0.30;
pub const RECESS: f32 = 0.16;

/// A door in an outer wall: the opening at the foot of a wall, so that the door leaf (a separate model that swings) can be put in it.
#[derive(Clone, Copy, Debug)]
pub struct DoorSpec {
    /// The middle of the wall at the left jamb, at floor level.
    pub hinge: V3,
    /// Along the opening from the left jamb to the right one (unit), and out of the building (unit).
    pub along: V3,
    pub out: V3,
    pub width: f32,
    pub height: f32,
}

/// The materials a building is dressed in (material numbers of the `Lib`).
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub wall: usize,
    pub wall_tint: V3,
    /// Sills, lintels, cornices, copings.
    pub trim: usize,
    pub trim_tint: V3,
    /// The plinth and the knee wall under shop windows.
    pub base: usize,
    pub base_tint: V3,
    pub frame: usize,
    /// Pane materials and how often each is chosen.
    pub glass: [usize; 4],
    pub glass_w: [f32; 4],
    /// The panes of a window with a light on: warm, cool (they glow at night).
    pub lit: [usize; 2],
    pub roof: usize,
    pub spandrel: usize,
    pub equip: usize,
    pub shutter: usize,
    pub sign: [usize; 6],
    pub awning: [usize; 4],
    /// How much dirt gathers at the foot of the wall (0 none, 0.4 a lot).
    pub dirt: f32,
    /// The inside of the outer walls, the floors and the ceilings.
    pub inner: usize,
    pub inner_tint: V3,
    pub inner_size: [f32; 2],
    pub floor: usize,
    pub floor_tint: V3,
    pub ceiling: usize,
}

/// A rectangular hole in a wall.
#[derive(Clone, Copy, Debug)]
pub struct Opening {
    pub s0: f32,
    pub s1: f32,
    pub h0: f32,
    pub h1: f32,
}

pub fn col(t: V3, k: f32) -> V3 {
    [t[0] * k, t[1] * k, t[2] * k]
}

/// How light the wall is at height `h`: dirt and splashes darken the foot of it.
pub fn shade(h: f32, dirt: f32) -> f32 {
    lerp(1.0 - dirt, 1.0, smooth(0.0, 2.4, h))
}

/// The frame of a wall between two ground-level corners of a footprint: it faces away from `centroid`, `s` runs from the corner that is on the left of someone facing the wall to the one on the right.
pub fn wall_frame(p: V3, q: V3, centroid: V3) -> (Frame, f32) {
    let d = sub(q, p);
    let mut out = norm([d[2], 0.0, -d[0]]);
    let mid = scale(add(p, q), 0.5);
    if dot(out, sub(mid, centroid)) < 0.0 {
        out = scale(out, -1.0);
    }
    let a = norm(cross([0.0, 1.0, 0.0], out));
    let o = if dot(d, a) >= 0.0 { p } else { q };
    (Frame::wall(o, out), len(d))
}

/// Wall between heights h0 and h1 across [s0, s1], with the openings cut out (sorted by `s0`, inside the band): the outer face and, `WALL_T` behind it, the inner face (the walls are hollow shells with
/// real thickness, a floor and a ceiling inside; `length` is the length of the whole wall, the inner faces stop where the walls at its ends begin).
#[allow(clippy::too_many_arguments)]
pub fn wall_band(mb: &mut MeshBuilder, pal: &Palette, f: &Frame, tex: &Tex, tint: V3, s0: f32, s1: f32, h0: f32, h1: f32, ops: &[Opening], length: f32) {
    let c = |h: f32| col(tint, shade(h, pal.dirt));
    let mat = pal.wall;
    let mut cur = s0;
    for o in ops {
        mb.wall_rect(mat, f, cur, o.s0, h0, h1, 0.0, tex, c(h0), c(h1));
        mb.wall_rect(mat, f, o.s0, o.s1, h0, o.h0, 0.0, tex, c(h0), c(o.h0));
        mb.wall_rect(mat, f, o.s0, o.s1, o.h1, h1, 0.0, tex, c(o.h1), c(h1));
        cur = o.s1;
    }
    mb.wall_rect(mat, f, cur, s1, h0, h1, 0.0, tex, c(h0), c(h1));
    // the inside
    let itex = Tex { size: pal.inner_size, off: [0.0, 0.0] };
    let ic = pal.inner_tint;
    let (i0, i1) = (s0.max(WALL_T), s1.min(length - WALL_T));
    let inward = scale(f.c, -1.0);
    let mut cur = i0;
    for o in ops {
        let (a, b) = (o.s0.max(i0), o.s1.min(i1));
        if b <= a {
            continue;
        }
        mb.wall_rect_facing(pal.inner, f, cur, a, h0, h1, -WALL_T, &itex, ic, ic, inward);
        mb.wall_rect_facing(pal.inner, f, a, b, h0, o.h0, -WALL_T, &itex, ic, ic, inward);
        mb.wall_rect_facing(pal.inner, f, a, b, o.h1, h1, -WALL_T, &itex, ic, ic, inward);
        cur = b;
    }
    mb.wall_rect_facing(pal.inner, f, cur, i1, h0, h1, -WALL_T, &itex, ic, ic, inward);
}

/// What can be seen behind the glass of a window.
#[derive(Clone, Copy, Debug)]
pub enum Pane {
    Clear,
    /// A blind lowered over the top `frac` of the pane.
    Blind { frac: f32, mat: usize },
    /// Curtains drawn from both sides by `left` and `right` (shares of the width).
    Curtains { left: f32, right: f32, mat: usize },
}

/// What a window of a home has behind it, chosen at random: mostly the dark room, often a blind or curtains.
pub fn random_pane(rng: &mut Rng, lib: &Lib) -> Pane {
    let mats = ["blind_white", "curtain_warm", "curtain_cool", "curtain_red", "curtain_green", "curtain_beige", "curtain_pink"];
    match rng.weighted(&[0.5, 0.22, 0.28]) {
        0 => Pane::Clear,
        1 => {
            let m = if rng.chance(0.7) { "blind_white" } else { rng.pick(&mats) };
            Pane::Blind { frac: rng.range(0.25, 1.0), mat: lib.id(m) }
        }
        _ => {
            let (l, r) = if rng.chance(0.3) { (rng.range(0.3, 0.9), 0.0) } else { (rng.range(0.15, 0.4), rng.range(0.15, 0.4)) };
            Pane::Curtains { left: l, right: r, mat: lib.id(rng.pick(&mats[1..])) }
        }
    }
}

/// A pane of glass behind a frame, in an opening with reveals.
pub struct WindowSpec {
    pub pane: Pane,
    pub frame: usize,
    pub glass: usize,
    pub recess: f32,
    pub bar: f32,
    pub cols: u32,
    pub rows: u32,
    pub sill: bool,
    pub lintel: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn window(mb: &mut MeshBuilder, f: &Frame, o: &Opening, w: &WindowSpec, pal: &Palette, wall_tex: &Tex, trim_tex: &Tex, wall_col: V3) {
    let rd = RECESS;
    let _ = w.recess;
    let shaded = col(wall_col, 0.72);
    // the reveals: the wall's thickness round the hole
    let (s0, s1, h0, h1) = (o.s0, o.s1, o.h0, o.h1);
    mb.quad_tex(pal.wall, [f.at(s0, h0, 0.0), f.at(s0, h0, -rd), f.at(s0, h1, -rd), f.at(s0, h1, 0.0)], [[0.0, h0], [rd, h0], [rd, h1], [0.0, h1]], wall_tex, shaded, f.a);
    mb.quad_tex(pal.wall, [f.at(s1, h0, 0.0), f.at(s1, h0, -rd), f.at(s1, h1, -rd), f.at(s1, h1, 0.0)], [[0.0, h0], [rd, h0], [rd, h1], [0.0, h1]], wall_tex, shaded, scale(f.a, -1.0));
    mb.quad_tex(pal.wall, [f.at(s0, h1, 0.0), f.at(s1, h1, 0.0), f.at(s1, h1, -rd), f.at(s0, h1, -rd)], [[s0, 0.0], [s1, 0.0], [s1, rd], [s0, rd]], wall_tex, col(wall_col, 0.55), scale(f.b, -1.0));
    mb.quad_tex(pal.wall, [f.at(s0, h0, 0.0), f.at(s1, h0, 0.0), f.at(s1, h0, -rd), f.at(s0, h0, -rd)], [[s0, 0.0], [s1, 0.0], [s1, rd], [s0, rd]], wall_tex, col(wall_col, 0.85), f.b);
    // the inner part of the reveals, painted like the room, and a board on the sill inside
    {
        let t = WALL_T;
        let itex = Tex { size: pal.inner_size, off: [0.0, 0.0] };
        let ic = pal.inner_tint;
        let (d0, d1) = (rd, t);
        mb.quad_tex(pal.inner, [f.at(s0, h0, -d0), f.at(s0, h0, -d1), f.at(s0, h1, -d1), f.at(s0, h1, -d0)], [[0.0, h0], [d1 - d0, h0], [d1 - d0, h1], [0.0, h1]], &itex, ic, f.a);
        mb.quad_tex(pal.inner, [f.at(s1, h0, -d0), f.at(s1, h0, -d1), f.at(s1, h1, -d1), f.at(s1, h1, -d0)], [[0.0, h0], [d1 - d0, h0], [d1 - d0, h1], [0.0, h1]], &itex, ic, scale(f.a, -1.0));
        mb.quad_tex(pal.inner, [f.at(s0, h1, -d0), f.at(s1, h1, -d0), f.at(s1, h1, -d1), f.at(s0, h1, -d1)], [[s0, 0.0], [s1, 0.0], [s1, d1 - d0], [s0, d1 - d0]], &itex, col(ic, 0.8), scale(f.b, -1.0));
        mb.quad_tex(pal.inner, [f.at(s0, h0, -d0), f.at(s1, h0, -d0), f.at(s1, h0, -d1), f.at(s0, h0, -d1)], [[s0, 0.0], [s1, 0.0], [s1, d1 - d0], [s0, d1 - d0]], &itex, ic, f.b);
        mb.cuboid(pal.trim, f, [s0 - 0.04, h0, -t - 0.13], [s1 + 0.04, h0 + 0.035, -t], 1 | 2 | 4 | 8 | 32, trim_tex, col(pal.trim_tint, 1.0));
    }
    // the pane
    let z = [[0.0; 2]; 4];
    // the glass is a little lighter at the top (the ceiling) and darker at the floor of the room behind it
    let g = [[0.8; 3], [0.8; 3], [1.1; 3], [1.1; 3]];
    mb.quad(w.glass, [f.at(s0, h0, -rd), f.at(s1, h0, -rd), f.at(s1, h1, -rd), f.at(s0, h1, -rd)], z, g, f.c);
    let dd = -rd + 0.012;
    match w.pane {
        Pane::Clear => {}
        Pane::Blind { frac, mat } => {
            let hb = h1 - frac * (h1 - h0);
            mb.quad(mat, [f.at(s0, hb, dd), f.at(s1, hb, dd), f.at(s1, h1, dd), f.at(s0, h1, dd)], z, [[0.85; 3], [0.85; 3], [1.0; 3], [1.0; 3]], f.c);
            mb.quad(mat, [f.at(s0, hb - 0.03, dd), f.at(s1, hb - 0.03, dd), f.at(s1, hb, dd), f.at(s0, hb, dd)], z, [[0.6; 3]; 4], f.c);
        }
        Pane::Curtains { left, right, mat } => {
            let width = s1 - s0;
            if left > 0.0 {
                let e = s0 + width * left;
                mb.quad(mat, [f.at(s0, h0, dd), f.at(e, h0, dd), f.at(e, h1, dd), f.at(s0, h1, dd)], z, [[0.75; 3], [0.95; 3], [1.0; 3], [0.85; 3]], f.c);
            }
            if right > 0.0 {
                let e = s1 - width * right;
                mb.quad(mat, [f.at(e, h0, dd), f.at(s1, h0, dd), f.at(s1, h1, dd), f.at(e, h1, dd)], z, [[0.95; 3], [0.75; 3], [0.85; 3], [1.0; 3]], f.c);
            }
        }
    }
    // the frame and its bars, a little in front of the pane
    let d = -rd + 0.025;
    let bar = w.bar;
    let mut bars: Vec<(f32, f32, f32, f32)> = vec![(s0, s1, h0, h0 + bar), (s0, s1, h1 - bar, h1), (s0, s0 + bar, h0, h1), (s1 - bar, s1, h0, h1)];
    for i in 1..w.cols {
        let s = lerp(s0, s1, i as f32 / w.cols as f32);
        bars.push((s - bar * 0.5, s + bar * 0.5, h0, h1));
    }
    for i in 1..w.rows {
        let h = lerp(h0, h1, i as f32 / w.rows as f32);
        bars.push((s0, s1, h - bar * 0.5, h + bar * 0.5));
    }
    for (a0, a1, b0, b1) in bars {
        mb.quad(w.frame, [f.at(a0, b0, d), f.at(a1, b0, d), f.at(a1, b1, d), f.at(a0, b1, d)], z, [[1.0; 3]; 4], f.c);
    }
    if w.sill {
        mb.cuboid(pal.trim, f, [s0 - 0.06, h0 - 0.07, 0.0], [s1 + 0.06, h0, 0.10], 31, trim_tex, col(pal.trim_tint, 1.0));
    }
    if w.lintel {
        mb.cuboid(pal.trim, f, [s0 - 0.06, h1, 0.0], [s1 + 0.06, h1 + 0.11, 0.05], 31, trim_tex, col(pal.trim_tint, 0.95));
    }
}

fn pick_glass(rng: &mut Rng, pal: &Palette) -> usize {
    // a share of the windows has a light on (what is lit is told by the pane's material: see `glass_lit_warm`)
    if rng.chance(0.28) {
        return pal.lit[usize::from(rng.chance(0.3))];
    }
    pal.glass[rng.weighted(&pal.glass_w)]
}

/// An air-conditioning outdoor unit hung on the wall under a window: a box with a grille.
fn ac_unit(mb: &mut MeshBuilder, f: &Frame, pal: &Palette, rng: &mut Rng, s: f32, h: f32, lib: &Lib) {
    let w = rng.range(0.7, 0.95);
    let hh = rng.range(0.5, 0.62);
    let tex = lib.tex_plain(pal.equip);
    mb.cuboid(pal.equip, f, [s - w * 0.5, h, 0.0], [s + w * 0.5, h + hh, 0.34], 1 | 2 | 4 | 8 | 16, &tex, [1.0; 3]);
    let rub = lib.id("rubber");
    mb.quad(rub, [f.at(s - w * 0.4, h + hh * 0.15, 0.345), f.at(s + w * 0.4, h + hh * 0.15, 0.345), f.at(s + w * 0.4, h + hh * 0.85, 0.345), f.at(s - w * 0.4, h + hh * 0.85, 0.345)], [[0.0; 2]; 4], [[1.0; 3]; 4], f.c);
}

/// What a ground floor is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ground {
    Shops,
    Lobby,
    Plain,
}

/// One bay of a ground floor: a window part and/or a door, between `a` and `b`.
struct Unit {
    a: f32,
    b: f32,
    win: Option<Opening>,
    door: Option<Opening>,
    roll: bool,
}

/// A doorway through the wall: reveals the thickness of the wall, a dark frame, a threshold. The leaf is a separate model that swings (`DoorSpec` says where).
fn doorway(mb: &mut MeshBuilder, f: &Frame, o: &Opening, pal: &Palette, lib: &Lib, wall_tex: &Tex, doors: &mut Vec<DoorSpec>) {
    let t = WALL_T;
    let (s0, s1, h1) = (o.s0, o.s1, o.h1);
    let c = col(pal.wall_tint, 0.85);
    mb.quad_tex(pal.wall, [f.at(s0, 0.0, 0.0), f.at(s0, 0.0, -t), f.at(s0, h1, -t), f.at(s0, h1, 0.0)], [[0.0, 0.0], [t, 0.0], [t, h1], [0.0, h1]], wall_tex, c, f.a);
    mb.quad_tex(pal.wall, [f.at(s1, 0.0, 0.0), f.at(s1, 0.0, -t), f.at(s1, h1, -t), f.at(s1, h1, 0.0)], [[0.0, 0.0], [t, 0.0], [t, h1], [0.0, h1]], wall_tex, c, scale(f.a, -1.0));
    mb.quad_tex(pal.wall, [f.at(s0, h1, 0.0), f.at(s1, h1, 0.0), f.at(s1, h1, -t), f.at(s0, h1, -t)], [[s0, 0.0], [s1, 0.0], [s1, t], [s0, t]], wall_tex, col(pal.wall_tint, 0.6), scale(f.b, -1.0));
    // a threshold of stone, level with the paving outside and the floor inside
    mb.cuboid(pal.base, f, [s0, 0.0, -t], [s1, 0.02, 0.0], 4 | 16, &lib.tex_plain(pal.base), [0.85; 3]);
    let fm = lib.id("frame_dark");
    let z = [[0.0; 2]; 4];
    let w = [[1.0; 3]; 4];
    for (a0, a1, b0, b1) in [(s0, s0 + 0.09, 0.02, h1), (s1 - 0.09, s1, 0.02, h1), (s0, s1, h1 - 0.1, h1)] {
        mb.quad(fm, [f.at(a0, b0, 0.012), f.at(a1, b0, 0.012), f.at(a1, b1, 0.012), f.at(a0, b1, 0.012)], z, w, f.c);
    }
    doors.push(DoorSpec { hinge: f.at(s0, 0.0, -t * 0.5), along: f.a, out: f.c, width: s1 - s0, height: h1 });
}

/// The ground floor of a wall: piers, windows, doorways, shop signs, awnings, roller shutters.
#[allow(clippy::too_many_arguments)]
pub fn ground_floor(mb: &mut MeshBuilder, rng: &mut Rng, lib: &Lib, pal: &Palette, f: &Frame, length: f32, ground_h: f32, wall_tex: &Tex, trim_tex: &Tex, kind: Ground, doors: &mut Vec<DoorSpec>) {
    let base_tex = lib.tex(pal.base, rng);
    let plinth = 0.55;
    let top = ground_h - 0.95;
    let frame_dark = lib.id("frame_dark");
    let door_h = 2.45;
    let mut units: Vec<Unit> = Vec::new();
    match kind {
        Ground::Plain => {
            let n = ((length / 3.6).round() as i32).max(1);
            let bw = length / n as f32;
            let door_i = rng.int(0, n - 1);
            for i in 0..n {
                let (a, b) = (i as f32 * bw, (i + 1) as f32 * bw);
                let c = (a + b) * 0.5;
                if i == door_i {
                    let dw = 1.7f32.min(bw - 0.5).max(1.0);
                    units.push(Unit { a, b, win: None, door: Some(Opening { s0: c - dw * 0.5, s1: c + dw * 0.5, h0: 0.0, h1: door_h }), roll: false });
                } else if bw > 2.4 {
                    units.push(Unit { a, b, win: Some(Opening { s0: c - 0.6, s1: c + 0.6, h0: 1.05, h1: 2.35 }), door: None, roll: false });
                } else {
                    units.push(Unit { a, b, win: None, door: None, roll: false });
                }
            }
        }
        _ => {
            let mut spans: Vec<(f32, f32)> = Vec::new();
            let mut s = 0.0;
            while s < length - 0.2 {
                let w = if kind == Ground::Lobby { rng.range(5.0, 9.0) } else { rng.range(3.6, 8.0) };
                let w = if length - s - w < 3.0 { length - s } else { w };
                spans.push((s, s + w));
                s += w;
            }
            let door_unit = rng.int(0, spans.len() as i32 - 1) as usize;
            for (k, (u0, u1)) in spans.iter().enumerate() {
                let (a, b) = (u0 + 0.25, u1 - 0.25);
                let w = b - a;
                let has_door = k == door_unit || (kind == Ground::Lobby && rng.chance(0.4));
                let dw = if kind == Ground::Lobby { 2.4 } else { 1.9 };
                if has_door {
                    if w >= dw + 2.4 {
                        let right = rng.chance(0.5);
                        let (win, door) = if right {
                            (Opening { s0: a, s1: b - dw - 0.3, h0: plinth, h1: top }, Opening { s0: b - dw, s1: b, h0: 0.0, h1: door_h })
                        } else {
                            (Opening { s0: a + dw + 0.3, s1: b, h0: plinth, h1: top }, Opening { s0: a, s1: a + dw, h0: 0.0, h1: door_h })
                        };
                        units.push(Unit { a, b, win: Some(win), door: Some(door), roll: false });
                    } else {
                        let d2 = dw.min(w);
                        let c = (a + b) * 0.5;
                        units.push(Unit { a, b, win: None, door: Some(Opening { s0: c - d2 * 0.5, s1: c + d2 * 0.5, h0: 0.0, h1: door_h }), roll: false });
                    }
                } else if kind == Ground::Shops && rng.chance(0.2) {
                    units.push(Unit { a, b, win: Some(Opening { s0: a, s1: b, h0: 0.0, h1: top }), door: None, roll: true });
                } else {
                    units.push(Unit { a, b, win: Some(Opening { s0: a, s1: b, h0: plinth, h1: top }), door: None, roll: false });
                }
            }
        }
    }
    let mut ops: Vec<Opening> = units.iter().flat_map(|u| u.win.into_iter().chain(u.door)).collect();
    ops.sort_by(|a, b| a.s0.total_cmp(&b.s0));
    wall_band(mb, pal, f, wall_tex, pal.wall_tint, 0.0, length, 0.0, ground_h, &ops, length);
    // the plinth in front of the wall, broken where a door is
    let mut cur = 0.0;
    let mut door_spans: Vec<(f32, f32)> = ops.iter().filter(|o| o.h0 < 0.1).map(|o| (o.s0, o.s1)).collect();
    door_spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    door_spans.push((length, length));
    for (d0, d1) in door_spans {
        if d0 - cur > 0.05 {
            mb.cuboid(pal.base, f, [cur, 0.0, 0.0], [d0, plinth, 0.035], 4 | 16, &base_tex, col(pal.base_tint, 0.85));
        }
        cur = d1;
    }
    for u in &units {
        let (a, b) = (u.a, u.b);
        if let Some(o) = u.win {
            if u.roll {
                // a closed roller shutter behind a shallow reveal
                mb.quad_tex(pal.shutter, [f.at(a, 0.0, -0.12), f.at(b, 0.0, -0.12), f.at(b, top, -0.12), f.at(a, top, -0.12)], [[a, 0.0], [b, 0.0], [b, top], [a, top]], &lib.tex(pal.shutter, rng), [1.0; 3], f.c);
                for (s, face) in [(a, f.a), (b, scale(f.a, -1.0))] {
                    mb.quad_tex(pal.wall, [f.at(s, 0.0, 0.0), f.at(s, 0.0, -0.12), f.at(s, top, -0.12), f.at(s, top, 0.0)], [[0.0, 0.0], [0.12, 0.0], [0.12, top], [0.0, top]], wall_tex, col(pal.wall_tint, 0.7), face);
                }
                mb.cuboid(frame_dark, f, [a - 0.05, top - 0.05, 0.0], [b + 0.05, top, 0.14], 1 | 2 | 8 | 16, &lib.tex_plain(frame_dark), [1.0; 3]);
            } else if kind == Ground::Plain {
                let spec = WindowSpec { pane: random_pane(rng, lib), frame: pal.frame, glass: pick_glass(rng, pal), recess: RECESS, bar: 0.06, cols: 2, rows: 1, sill: true, lintel: true };
                window(mb, f, &o, &spec, pal, wall_tex, trim_tex, pal.wall_tint);
            } else {
                let glass = lib.id(rng.pick(&["glass_dark", "glass_grey", "interior_warm", "interior_cool", "interior_warm"]));
                let cols = (((o.s1 - o.s0) / 1.5).round() as u32).max(1);
                let spec = WindowSpec { pane: Pane::Clear, frame: frame_dark, glass, recess: RECESS, bar: 0.07, cols, rows: 2, sill: false, lintel: false };
                window(mb, f, &o, &spec, pal, wall_tex, trim_tex, pal.wall_tint);
            }
        }
        if let Some(o) = u.door {
            doorway(mb, f, &o, pal, lib, wall_tex, doors);
        }
        if kind == Ground::Plain {
            continue;
        }
        // the sign band over the unit, with lettering, and sometimes a blade sign and an awning
        let sign_h = ground_h - 0.9;
        let sign = pal.sign[rng.weighted(&[3.0, 2.0, 2.0, 1.5, 1.5, 1.0])];
        let st = lib.tex_plain(sign);
        mb.cuboid(sign, f, [a - 0.05, sign_h, 0.0], [b + 0.05, ground_h - 0.25, 0.09], 1 | 2 | 4 | 8 | 16, &st, [1.0; 3]);
        let dark_sign = matches!(lib.mats[sign].name.as_str(), "sign_black" | "sign_charcoal" | "sign_maroon" | "sign_blue" | "sign_green" | "sign_red");
        // the name of the shop: a picture of letters (8 to 1) as big as the sign's height allows, in the middle of it, narrowed when the sign is short
        let sign_w = b - a;
        let name = rng.int(0, crate::materials::SIGN_NAMES as i32 - 1);
        let ink = lib.id(if dark_sign { "signtext_w" } else { "signtext_b" });
        let (u0, v0, u1, v1) = crate::materials::sign_cell(name as usize);
        let (sh, mid) = (ground_h - 0.25 - sign_h, (sign_h + ground_h - 0.25) * 0.5);
        let tw = (sh * 8.0).min(sign_w - 0.3);
        let (x0, x1) = ((a + b) * 0.5 - tw * 0.5, (a + b) * 0.5 + tw * 0.5);
        mb.quad(ink, [f.at(x0, mid - sh * 0.5, 0.092), f.at(x1, mid - sh * 0.5, 0.092), f.at(x1, mid + sh * 0.5, 0.092), f.at(x0, mid + sh * 0.5, 0.092)], [[u0, v1], [u1, v1], [u1, v0], [u0, v0]], [[1.0; 3]; 4], f.c);
        if rng.chance(0.22) {
            let sc = if rng.chance(0.5) { a + 0.3 } else { b - 0.3 };
            let bs = lib.id(rng.pick(&["sign_red", "sign_blue", "sign_white", "sign_green", "sign_charcoal"]));
            mb.cuboid(bs, f, [sc - 0.04, ground_h - 1.0, 0.12], [sc + 0.04, ground_h + 0.1, 1.05], 1 | 2 | 4 | 8 | 16 | 32, &lib.tex_plain(bs), [1.0; 3]);
        }
        if !u.roll && u.door.is_none() && rng.chance(0.3) {
            let aw = pal.awning[rng.int(0, 3) as usize];
            let (h_hi, h_lo, proj) = (sign_h - 0.02, sign_h - 0.7, 1.3);
            let up = norm(add(f.c, scale(f.b, 0.7)));
            let cream = lib.id("awning_cream");
            let stripes = (((b - a) / 0.3).round() as i32).max(2);
            for k in 0..stripes {
                let (u0, u1) = (lerp(a, b, k as f32 / stripes as f32), lerp(a, b, (k + 1) as f32 / stripes as f32));
                let m = if k % 2 == 0 { aw } else { cream };
                let p = [f.at(u0, h_hi, 0.02), f.at(u1, h_hi, 0.02), f.at(u1, h_lo, proj), f.at(u0, h_lo, proj)];
                mb.quad(m, p, [[0.0; 2]; 4], [[1.0; 3]; 4], up);
                mb.quad(m, p, [[0.0; 2]; 4], [[0.5; 3]; 4], scale(up, -1.0));
            }
            for s in [a, b] {
                mb.quad(aw, [f.at(s, h_hi, 0.02), f.at(s, h_lo, proj), f.at(s, h_lo, 0.02), f.at(s, h_lo, 0.02)], [[0.0; 2]; 4], [[0.8; 3]; 4], if s == a { scale(f.a, -1.0) } else { f.a });
            }
        }
    }
}

/// Settings of a wall of punched windows.
#[derive(Clone, Copy, Debug)]
pub struct Punched {
    pub bay: f32,
    pub win_w: f32,
    pub win_h: f32,
    pub sill_h: f32,
    pub band_every: u32,
    pub ac_chance: f32,
    pub lintels: bool,
    pub shutters: bool,
    /// The chance that a window on an upper floor has a balcony under it.
    pub balcony: f32,
}

/// A wall of punched windows: `floors` floors, the first `ground_h` tall (laid out by `ground_floor`), the others `floor_h`. Returns the height reached.
#[allow(clippy::too_many_arguments)]
pub fn punched(mb: &mut MeshBuilder, rng: &mut Rng, lib: &Lib, pal: &Palette, f: &Frame, length: f32, floors: u32, ground_h: f32, floor_h: f32, spec: &Punched, ground: Ground, doors: &mut Vec<DoorSpec>) -> f32 {
    let wall_tex = lib.tex(pal.wall, rng);
    let trim_tex = lib.tex(pal.trim, rng);
    ground_floor(mb, rng, lib, pal, f, length, ground_h, &wall_tex, &trim_tex, ground, doors);
    let nb = ((length / spec.bay).round() as i32).max(1);
    let bw = length / nb as f32;
    let ww = (bw * spec.win_w).clamp(0.85, 2.3).min(bw - 0.7).max(0.5);
    for fl in 1..floors {
        let h0 = ground_h + (fl - 1) as f32 * floor_h;
        let floor_tint = rng.range(0.96, 1.04);
        let tint = col(pal.wall_tint, floor_tint);
        let mut ops: Vec<Opening> = Vec::new();
        let mut wins: Vec<(Opening, f32)> = Vec::new();
        for b in 0..nb {
            let c = (b as f32 + 0.5) * bw;
            if rng.chance(0.03) {
                continue;
            }
            let hs = h0 + spec.sill_h;
            let hh = (spec.win_h).min(floor_h - spec.sill_h - 0.4);
            let o = Opening { s0: c - ww * 0.5, s1: c + ww * 0.5, h0: hs, h1: hs + hh };
            ops.push(o);
            wins.push((o, c));
        }
        wall_band(mb, pal, f, &wall_tex, tint, 0.0, length, h0, h0 + floor_h, &ops, length);
        for (o, c) in &wins {
            let glass = pick_glass(rng, pal);
            let cols = if o.s1 - o.s0 > 1.45 { 2 } else { 1 };
            let spec_w = WindowSpec { pane: random_pane(rng, lib), frame: pal.frame, glass, recess: 0.2, bar: 0.055, cols, rows: if o.h1 - o.h0 > 1.7 { 2 } else { 1 }, sill: true, lintel: spec.lintels };
            window(mb, f, o, &spec_w, pal, &wall_tex, &trim_tex, tint);
            if spec.balcony > 0.0 && fl >= 2 && rng.chance(spec.balcony) {
                balcony(mb, f, pal, lib, o.s0 - 0.35, o.s1 + 0.35, h0);
            } else if rng.chance(spec.ac_chance) {
                ac_unit(mb, f, pal, rng, *c, o.h0 - 0.82, lib);
            }
            if spec.shutters && rng.chance(0.5) {
                let sw = 0.42;
                for s in [o.s0 - sw - 0.04, o.s1 + 0.04] {
                    mb.cuboid(pal.shutter, f, [s, o.h0, 0.0], [s + sw, o.h1, 0.045], 1 | 2 | 4 | 8 | 16, &lib.tex(pal.shutter, rng), col(pal.wall_tint, 0.9));
                }
            }
        }
        if spec.band_every > 0 && fl % spec.band_every == 0 {
            mb.cuboid(pal.trim, f, [0.0, h0 - 0.1, 0.0], [length, h0 + 0.1, 0.05], 4 | 8 | 16, &trim_tex, pal.trim_tint);
        }
    }
    ground_h + (floors.saturating_sub(1)) as f32 * floor_h
}

/// A balcony: a slab of concrete at floor level `h0` (1.3 m out, between `s0` and `s1` along the wall) with a steel rail on its three open sides.
pub fn balcony(mb: &mut MeshBuilder, f: &Frame, pal: &Palette, lib: &Lib, s0: f32, s1: f32, h0: f32) {
    let out = 1.3;
    let slab = pal.trim;
    let st = lib.tex_plain(slab);
    mb.cuboid(slab, f, [s0, h0 - 0.16, 0.0], [s1, h0 - 0.02, out], 63, &st, pal.trim_tint);
    let rail = lib.id("frame_dark");
    let rt = lib.tex_plain(rail);
    let top = h0 + 1.02;
    // rails along the front and the two sides, and posts every 40 cm
    mb.cuboid(rail, f, [s0, top, out - 0.04], [s1, top + 0.04, out], 63, &rt, [1.0; 3]);
    mb.cuboid(rail, f, [s0, top, 0.0], [s0 + 0.04, top + 0.04, out], 63, &rt, [1.0; 3]);
    mb.cuboid(rail, f, [s1 - 0.04, top, 0.0], [s1, top + 0.04, out], 63, &rt, [1.0; 3]);
    let n = (((s1 - s0) / 0.4).round() as i32).max(2);
    for k in 0..=n {
        let s = lerp(s0, s1 - 0.025, k as f32 / n as f32);
        mb.cuboid(rail, f, [s, h0 - 0.02, out - 0.035], [s + 0.025, top, out - 0.01], 63, &rt, [1.0; 3]);
    }
    for k in 1..(out / 0.4) as i32 {
        let d = k as f32 * 0.4;
        mb.cuboid(rail, f, [s0 + 0.005, h0 - 0.02, d], [s0 + 0.03, top, d + 0.025], 63, &rt, [1.0; 3]);
        mb.cuboid(rail, f, [s1 - 0.03, h0 - 0.02, d], [s1 - 0.005, top, d + 0.025], 63, &rt, [1.0; 3]);
    }
}

/// A curtain wall: panels of glass and spandrel between mullions, with fins, over a glazed lobby floor. Returns the height reached.
#[allow(clippy::too_many_arguments)]
pub fn curtain(mb: &mut MeshBuilder, rng: &mut Rng, lib: &Lib, pal: &Palette, f: &Frame, length: f32, floors: u32, ground_h: f32, floor_h: f32, module: f32, lobby: bool) -> f32 {
    let trim_tex = lib.tex(pal.trim, rng);
    let n = ((length / module).round() as i32).max(1);
    let mw = length / n as f32;
    let alu = pal.frame;
    let alu_tex = lib.tex_plain(alu);
    // the first floor is the lobby (taller, glazed to the knee, a canopy over the entrance) or, on a tower that stands on a podium, a floor like the others
    let first_h = if lobby { ground_h } else { floor_h };
    let top = first_h + (floors.saturating_sub(1)) as f32 * floor_h;
    let z = [[0.0; 2]; 4];
    let white = [[1.0; 3]; 4];
    if lobby {
        for i in 0..n {
            let (s0, s1) = (i as f32 * mw, (i + 1) as f32 * mw);
            mb.quad(pal.glass[0], [f.at(s0, 0.0, -0.06), f.at(s1, 0.0, -0.06), f.at(s1, ground_h - 0.5, -0.06), f.at(s0, ground_h - 0.5, -0.06)], z, white, f.c);
            mb.wall_rect(pal.spandrel, f, s0, s1, ground_h - 0.5, ground_h, 0.0, &lib.tex_plain(pal.spandrel), [1.0; 3], [1.0; 3]);
        }
        let canopy_s = rng.range(0.25, 0.65) * length;
        let cw = (mw * 2.0).min(length * 0.5);
        mb.cuboid(pal.trim, f, [canopy_s - cw * 0.5, ground_h * 0.62, 0.0], [canopy_s + cw * 0.5, ground_h * 0.62 + 0.28, 3.0], 1 | 2 | 4 | 8 | 16, &trim_tex, pal.trim_tint);
    }
    // the floors above
    let sp = rng.range(0.8, 1.05);
    let tower_glass = pal.glass[rng.weighted(&pal.glass_w)];
    for fl in (if lobby { 1 } else { 0 })..floors {
        let h0 = if lobby { ground_h + (fl - 1) as f32 * floor_h } else { fl as f32 * floor_h };
        for i in 0..n {
            let (s0, s1) = (i as f32 * mw + 0.035, (i + 1) as f32 * mw - 0.035);
            mb.wall_rect(pal.spandrel, f, s0, s1, h0, h0 + sp, -0.02, &lib.tex_plain(pal.spandrel), col([1.0; 3], 0.95), [1.0; 3]);
            let g = if rng.chance(0.2) { pal.lit[usize::from(rng.chance(0.35))] } else if rng.chance(0.12) { pal.glass[1 + rng.int(0, 2) as usize] } else { tower_glass };
            let g = if rng.chance(0.08) { lib.id("blind_white") } else { g };
            mb.quad(g, [f.at(s0, h0 + sp, -0.06), f.at(s1, h0 + sp, -0.06), f.at(s1, h0 + floor_h, -0.06), f.at(s0, h0 + floor_h, -0.06)], z, white, f.c);
        }
        // the floor's transom
        mb.cuboid(alu, f, [0.0, h0 + sp - 0.04, 0.0], [length, h0 + sp + 0.04, 0.08], 1 | 2 | 4 | 8 | 16, &alu_tex, [1.0; 3]);
        mb.cuboid(alu, f, [0.0, h0 - 0.05, 0.0], [length, h0 + 0.05, 0.1], 1 | 2 | 4 | 8 | 16, &alu_tex, [1.0; 3]);
    }
    // mullions over the whole height, and a fin now and then
    let fin_every = rng.int(2, 4);
    for i in 0..=n {
        let s = i as f32 * mw;
        let (s0, s1) = ((s - 0.035).max(0.0), (s + 0.035).min(length));
        mb.cuboid(alu, f, [s0, 0.0, 0.0], [s1, top, 0.09], 1 | 2 | 16, &alu_tex, [1.0; 3]);
        if i > 0 && i < n && i % fin_every == 0 {
            mb.cuboid(pal.trim, f, [s - 0.07, first_h, 0.0], [s + 0.07, top, 0.32], 1 | 2 | 16 | 4, &trim_tex, pal.trim_tint);
        }
    }
    top
}

/// A plain wall (a party wall, a side wall): the wall material, some dirt, and a few small windows.
#[allow(clippy::too_many_arguments)]
pub fn plain_wall(mb: &mut MeshBuilder, rng: &mut Rng, lib: &Lib, pal: &Palette, f: &Frame, length: f32, floors: u32, ground_h: f32, floor_h: f32, windows: bool) -> f32 {
    let wall_tex = lib.tex(pal.wall, rng);
    let trim_tex = lib.tex(pal.trim, rng);
    let top = ground_h + (floors.saturating_sub(1)) as f32 * floor_h;
    if !windows || length < 4.0 {
        wall_band(mb, pal, f, &wall_tex, pal.wall_tint, 0.0, length, 0.0, ground_h * 0.5, &[], length);
        wall_band(mb, pal, f, &wall_tex, pal.wall_tint, 0.0, length, ground_h * 0.5, top, &[], length);
        return top;
    }
    let n = ((length / 7.0).round() as i32).max(1);
    let bw = length / n as f32;
    for fl in 0..floors {
        let h0 = if fl == 0 { 0.0 } else { ground_h + (fl - 1) as f32 * floor_h };
        let h1 = if fl == 0 { ground_h } else { h0 + floor_h };
        let mut ops: Vec<Opening> = Vec::new();
        for b in 0..n {
            if rng.chance(0.55) {
                continue;
            }
            let c = (b as f32 + 0.5) * bw;
            let hs = if fl == 0 { 2.1 } else { h0 + 0.95 };
            ops.push(Opening { s0: c - 0.5, s1: c + 0.5, h0: hs, h1: (hs + 1.2).min(h1 - 0.4) });
        }
        wall_band(mb, pal, f, &wall_tex, pal.wall_tint, 0.0, length, h0, h1, &ops, length);
        for o in &ops {
            let spec = WindowSpec { pane: random_pane(rng, lib), frame: pal.frame, glass: pick_glass(rng, pal), recess: 0.18, bar: 0.05, cols: 1, rows: 1, sill: true, lintel: false };
            window(mb, f, o, &spec, pal, &wall_tex, &trim_tex, pal.wall_tint);
        }
    }
    top
}

/// The wall of a warehouse or a factory hall, `height` tall: on the street side a door for people and one to three roller doors (closed) wide enough for a lorry, and a band of high windows (clerestory) along the top;
/// on the other sides only the high windows (none on a party wall).
#[allow(clippy::too_many_arguments)]
pub fn industrial(mb: &mut MeshBuilder, rng: &mut Rng, lib: &Lib, pal: &Palette, f: &Frame, length: f32, height: f32, street: bool, party: bool, doors: &mut Vec<DoorSpec>) -> f32 {
    let wall_tex = lib.tex(pal.wall, rng);
    let trim_tex = lib.tex(pal.trim, rng);
    let mut ops: Vec<Opening> = Vec::new();
    let mut rolls: Vec<Opening> = Vec::new();
    let mut person: Option<Opening> = None;
    if street && length > 8.0 {
        // doors along the wall, each in its own slot so that none overlaps another
        let slots = ((length / 7.0).floor() as i32).clamp(2, 5);
        let sw = length / slots as f32;
        let person_slot = rng.int(0, slots - 1);
        for k in 0..slots {
            let c = (k as f32 + 0.5) * sw;
            if k == person_slot {
                let o = Opening { s0: c - 0.6, s1: c + 0.6, h0: 0.0, h1: 2.35 };
                person = Some(o);
                ops.push(o);
            } else if rng.chance(0.6) {
                let o = Opening { s0: c - 2.0, s1: c + 2.0, h0: 0.0, h1: 4.3_f32.min(height - 2.8) };
                rolls.push(o);
                ops.push(o);
            }
        }
    }
    // the high windows
    let (wh0, wh1) = ((height - 2.7).max(2.5), (height - 1.3).max(3.5));
    let mut wins: Vec<Opening> = Vec::new();
    if !party && length > 5.0 {
        let n = ((length / 3.4).round() as i32).max(1);
        let bw = length / n as f32;
        for b in 0..n {
            let c = (b as f32 + 0.5) * bw;
            let o = Opening { s0: c - (bw * 0.5 - 0.5).min(1.2), s1: c + (bw * 0.5 - 0.5).min(1.2), h0: wh0, h1: wh1 };
            if ops.iter().all(|p| o.s0 > p.s1 + 0.2 || o.s1 < p.s0 - 0.2) {
                wins.push(o);
                ops.push(o);
            }
        }
    }
    ops.sort_by(|a, b| a.s0.total_cmp(&b.s0));
    wall_band(mb, pal, f, &wall_tex, pal.wall_tint, 0.0, length, 0.0, height, &ops, length);
    let frame_dark = lib.id("frame_dark");
    for o in &rolls {
        // a closed roller door behind a shallow reveal, with its box above
        let (a, b, top) = (o.s0, o.s1, o.h1);
        mb.quad_tex(pal.shutter, [f.at(a, 0.0, -0.14), f.at(b, 0.0, -0.14), f.at(b, top, -0.14), f.at(a, top, -0.14)], [[a, 0.0], [b, 0.0], [b, top], [a, top]], &lib.tex(pal.shutter, rng), [1.0; 3], f.c);
        for (s, face) in [(a, f.a), (b, scale(f.a, -1.0))] {
            mb.quad_tex(pal.wall, [f.at(s, 0.0, 0.0), f.at(s, 0.0, -0.14), f.at(s, top, -0.14), f.at(s, top, 0.0)], [[0.0, 0.0], [0.14, 0.0], [0.14, top], [0.0, top]], &wall_tex, col(pal.wall_tint, 0.7), face);
        }
        mb.cuboid(frame_dark, f, [a - 0.05, top - 0.05, 0.0], [b + 0.05, top, 0.16], 1 | 2 | 8 | 16, &lib.tex_plain(frame_dark), [1.0; 3]);
        // a bollard each side
        for s in [a - 0.7, b + 0.7] {
            mb.cuboid(lib.id("sign_yellow"), f, [s - 0.1, 0.0, 0.5], [s + 0.1, 0.9, 0.7], 63, &lib.tex_plain(lib.id("sign_yellow")), [1.0; 3]);
        }
    }
    if let Some(o) = person {
        doorway(mb, f, &o, pal, lib, &wall_tex, doors);
    }
    for o in &wins {
        let spec = WindowSpec { pane: random_pane(rng, lib), frame: pal.frame, glass: pick_glass(rng, pal), recess: 0.18, bar: 0.05, cols: ((o.s1 - o.s0) / 0.9).round().max(1.0) as u32, rows: 1, sill: false, lintel: false };
        window(mb, f, o, &spec, pal, &wall_tex, &trim_tex, pal.wall_tint);
    }
    height
}

/// The top of a wall: a cornice (optional) and a parapet of height `parapet` with a coping. `h` is the height of the last floor line.
#[allow(clippy::too_many_arguments)]
pub fn crown(mb: &mut MeshBuilder, rng: &mut Rng, lib: &Lib, pal: &Palette, f: &Frame, length: f32, h: f32, cornice: f32, parapet: f32) -> f32 {
    let wall_tex = lib.tex(pal.wall, rng);
    let trim_tex = lib.tex(pal.trim, rng);
    let mut top = h;
    if cornice > 0.0 {
        mb.cuboid(pal.trim, f, [-0.02, h - 0.28, 0.0], [length + 0.02, h + 0.06, cornice], 1 | 2 | 4 | 8 | 16, &trim_tex, pal.trim_tint);
    }
    if parapet > 0.0 {
        mb.wall_rect(pal.wall, f, 0.0, length, h, h + parapet, 0.0, &wall_tex, col(pal.wall_tint, 0.95), pal.wall_tint);
        // the inner face and the coping
        mb.cuboid(pal.trim, f, [0.0, h + parapet - 0.07, -0.26], [length, h + parapet, 0.04], 4 | 32 | 8, &trim_tex, col(pal.trim_tint, 0.95));
        mb.wall_rect(pal.wall, f, 0.0, length, h, h + parapet - 0.07, -0.26, &wall_tex, col(pal.wall_tint, 0.6), col(pal.wall_tint, 0.8));
        top = h + parapet;
    }
    top
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pal(lib: &Lib) -> Palette {
        Palette {
            wall: lib.id("brick_wall_001"),
            wall_tint: [1.0; 3],
            trim: lib.id("concrete_wall_007"),
            trim_tint: [0.9; 3],
            base: lib.id("concrete_wall_007"),
            base_tint: [0.7; 3],
            frame: lib.id("frame_white"),
            glass: [lib.id("glass_dark"), lib.id("glass_blue"), lib.id("curtain_warm"), lib.id("blind_white")],
            lit: [lib.id("glass_lit_warm"), lib.id("glass_lit_cool")],
            glass_w: [0.6, 0.2, 0.1, 0.1],
            roof: lib.id("bitumen"),
            spandrel: lib.id("spandrel_dark"),
            equip: lib.id("equip_paint"),
            shutter: lib.id("painted_metal_shutter"),
            sign: [lib.id("sign_red"), lib.id("sign_blue"), lib.id("sign_white"), lib.id("sign_yellow"), lib.id("sign_black"), lib.id("sign_green")],
            awning: [lib.id("awning_red"), lib.id("awning_green"), lib.id("awning_navy"), lib.id("awning_cream")],
            dirt: 0.3,
            inner: lib.id("wall_white"),
            inner_tint: [1.0; 3],
            inner_size: [1.0, 1.0],
            floor: lib.id("wood_dark"),
            floor_tint: [1.0; 3],
            ceiling: lib.id("ceiling_white"),
        }
    }

    #[test]
    fn a_punched_wall_has_windows_with_reveals_and_every_face_looks_outwards_or_into_a_hole() {
        let lib = Lib::from_json("");
        let p = pal(&lib);
        let mut mb = MeshBuilder::new();
        let mut rng = Rng::new(5);
        let f = Frame::wall([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let spec = Punched { bay: 3.6, win_w: 0.45, win_h: 1.6, sill_h: 0.9, band_every: 0, ac_chance: 0.2, lintels: true, shutters: false, balcony: 0.0 };
        let mut doors = Vec::new();
        let top = punched(&mut mb, &mut rng, &lib, &p, &f, 18.0, 5, 4.4, 3.2, &spec, Ground::Shops, &mut doors);
        assert!(!doors.is_empty());
        assert!((top - (4.4 + 4.0 * 3.2)).abs() < 1e-4);
        assert!(mb.triangles() > 500, "{}", mb.triangles());
        // nothing sticks out of the wall by more than an awning's reach, nothing sinks behind the glass by more than a reveal
        let (lo, hi) = mb.bounds().unwrap();
        assert!(hi[2] < 3.1 && lo[2] > -0.5, "{lo:?} {hi:?}");
        assert!(hi[1] > 15.0 && lo[1] >= -0.01);
        // every normal is unit length
        for part in &mb.parts {
            for n in &part.nrm {
                assert!((len(*n) - 1.0).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn a_curtain_wall_reaches_its_height_and_a_crown_adds_the_parapet() {
        let lib = Lib::from_json("");
        let p = pal(&lib);
        let mut mb = MeshBuilder::new();
        let mut rng = Rng::new(9);
        let f = Frame::wall([0.0; 3], [0.0, 0.0, 1.0]);
        let top = curtain(&mut mb, &mut rng, &lib, &p, &f, 24.0, 12, 5.5, 3.9, 1.6, true);
        assert!((top - (5.5 + 11.0 * 3.9)).abs() < 1e-3);
        let t = crown(&mut mb, &mut rng, &lib, &p, &f, 24.0, top, 0.0, 1.0);
        assert!((t - top - 1.0).abs() < 1e-5);
    }

    #[test]
    fn wall_frames_face_away_from_the_middle_of_the_footprint() {
        let c = [5.0, 0.0, 5.0];
        for (p, q) in [([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]), ([10.0, 0.0, 0.0], [10.0, 0.0, 10.0]), ([10.0, 0.0, 10.0], [0.0, 0.0, 10.0]), ([0.0, 0.0, 10.0], [0.0, 0.0, 0.0])] {
            let (f, l) = wall_frame(p, q, c);
            assert!((l - 10.0).abs() < 1e-4);
            let mid = scale(add(p, q), 0.5);
            assert!(dot(f.c, sub(mid, c)) > 0.0);
            // s runs from the origin along the wall
            let end = f.at(l, 0.0, 0.0);
            assert!(len(sub(end, p)) < 1e-3 || len(sub(end, q)) < 1e-3);
        }
    }
}
