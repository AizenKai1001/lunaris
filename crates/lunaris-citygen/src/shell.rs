//! The collision shell of a building, for a game server (`runtime::Role::Server`): what stops a bullet and a line of sight, and nothing else. A server draws nothing and has no pictures: it needs the walls of a building
//! with the street doors left open (the game's `Door` entities stand in them), a slab for every floor and for the roof, so that nobody shoots or sees through a wall, a floor or a ceiling. No windows, frames, sills, cornices, rooms or furniture:
//! a block is a few hundred triangles here and about a hundred thousand in the file a client draws. The shapes are the building's own (`Spec`: the footprints of its volumes, their floors and heights), and the doors are those the outside
//! makes (`building::exterior_with`), so the walls the server knows are the walls the client sees.

use crate::building::Spec;
use crate::facade::{DoorSpec, WALL_T};
use crate::interior::Levels;
use crate::materials::Lib;
use crate::mesh::*;

/// How thick a floor or roof slab is.
const SLAB: f32 = 0.25;

/// A gap in a wall, along the wall's axis, from `a` to `b`, with its floor at `y0` and its lintel at `y1`.
#[derive(Clone, Copy, Debug)]
struct Gap {
    a: f32,
    b: f32,
    y0: f32,
    y1: f32,
}

/// Add the shell of `spec`, whose street doors are `doors`, to `mb` (world coordinates).
pub fn shell(mb: &mut MeshBuilder, lib: &Lib, spec: &Spec, doors: &[DoorSpec]) {
    let mat = lib.id("far_flat");
    let tex = lib.tex_plain(mat);
    let col = [0.5; 3];
    for v in &spec.vols {
        let f = v.foot;
        let fr = f.frame();
        let lv = Levels { y0: v.y0, floors: v.floors, ground_h: v.ground_h, floor_h: v.floor_h };
        let bottom = v.y0 - 0.3;
        let top = v.top() + v.parapet;
        // the doors of this volume, in lot coordinates: (x0, z0, along x, along z, width, floor, height)
        let local: Vec<(f32, f32, f32, f32, f32, f32, f32)> = doors
            .iter()
            .filter(|d| d.hinge[1] >= v.y0 - 0.6 && d.hinge[1] <= v.y0 + v.ground_h)
            .map(|d| {
                let rel = sub(d.hinge, f.o);
                (dot(rel, f.ex), dot(rel, f.ez), dot(d.along, f.ex), dot(d.along, f.ez), d.width, d.hinge[1], d.height)
            })
            .collect();
        // the gaps of each wall: front (z = z0), back (z = z1) along x; left (x = x0), right (x = x1) along z
        let gaps_on = |along_x: bool, plane: f32| -> Vec<Gap> {
            let mut out: Vec<Gap> = Vec::new();
            for &(px, pz, ax, az, w, y, h) in &local {
                let on_axis = if along_x { az.abs() < 0.3 && (pz - plane).abs() < WALL_T + 0.6 } else { ax.abs() < 0.3 && (px - plane).abs() < WALL_T + 0.6 };
                if !on_axis {
                    continue;
                }
                let (start, dir) = if along_x { (px, ax.signum()) } else { (pz, az.signum()) };
                let (a, b) = if dir >= 0.0 { (start, start + w) } else { (start - w, start) };
                out.push(Gap { a, b, y0: y, y1: y + h });
            }
            out.sort_by(|p, q| p.a.total_cmp(&q.a));
            out
        };
        // a wall: from `s0` to `s1` along its axis and from `d0` to `d1` across it, the gaps cut out
        let mut wall = |mb: &mut MeshBuilder, along_x: bool, s0: f32, s1: f32, d0: f32, d1: f32, gaps: &[Gap]| {
            let mut put = |mb: &mut MeshBuilder, a: f32, b: f32, y0: f32, y1: f32| {
                if b - a < 1e-3 || y1 - y0 < 1e-3 {
                    return;
                }
                let (lo, hi) = if along_x { ([a, y0, d0], [b, y1, d1]) } else { ([d0, y0, a], [d1, y1, b]) };
                mb.cuboid(mat, &fr, lo, hi, 63, &tex, col);
            };
            let mut cursor = s0;
            for g in gaps {
                let (a, b) = (g.a.max(cursor), g.b.min(s1));
                if b <= a {
                    continue;
                }
                put(mb, cursor, a, bottom, top);
                put(mb, a, b, bottom, g.y0);
                put(mb, a, b, g.y1, top);
                cursor = b;
            }
            put(mb, cursor, s1, bottom, top);
        };
        wall(mb, true, f.x0, f.x1, f.z0, f.z0 + WALL_T, &gaps_on(true, f.z0));
        wall(mb, true, f.x0, f.x1, f.z1 - WALL_T, f.z1, &gaps_on(true, f.z1));
        wall(mb, false, f.z0 + WALL_T, f.z1 - WALL_T, f.x0, f.x0 + WALL_T, &gaps_on(false, f.x0));
        wall(mb, false, f.z0 + WALL_T, f.z1 - WALL_T, f.x1 - WALL_T, f.x1, &gaps_on(false, f.x1));
        // a slab under every floor above the ground floor, and over the top floor
        for k in 1..=v.floors {
            let y = lv.line(k);
            mb.cuboid(mat, &fr, [f.x0, y - SLAB, f.z0], [f.x1, y, f.z1], 63, &tex, col);
        }
    }
}

/// A wall or a slab of a building as a box for the sound: the middle in the world, the half sizes along the box's own axes (x along the wall's footprint axis, y up, z across), and the turn of the x axis about the vertical in radians
/// (the engine's `Transform.rot` of a box is a turn about y: the buildings of a round city stand at every angle).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AcousticBox {
    pub center: V3,
    pub half: V3,
    pub yaw: f32,
}

/// The walls and the roof of `spec` as boxes that sound is traced through (docs/AUDIO.md of the engine: a client has no collision shell, the sound has these): four walls and the roof slab of every volume, and the slab over its ground floor when it has
/// upper floors; no doors or windows are cut out (the material's `transmission` stands for them). About seven boxes a volume.
pub fn acoustic_boxes(spec: &Spec) -> Vec<AcousticBox> {
    let mut out = Vec::new();
    for v in &spec.vols {
        let f = v.foot;
        let fr = f.frame();
        let lv = Levels { y0: v.y0, floors: v.floors, ground_h: v.ground_h, floor_h: v.floor_h };
        let (bottom, top) = (v.y0 - 0.3, v.top() + v.parapet);
        let yaw = (-fr.a[2]).atan2(fr.a[0]);
        let mut push = |x0: f32, x1: f32, y0: f32, y1: f32, z0: f32, z1: f32| {
            if x1 - x0 < 1e-3 || y1 - y0 < 1e-3 || z1 - z0 < 1e-3 {
                return;
            }
            let local = [0.5 * (x0 + x1), 0.5 * (y0 + y1), 0.5 * (z0 + z1)];
            let center = add(fr.o, add(add(scale(fr.a, local[0]), scale(fr.b, local[1])), scale(fr.c, local[2])));
            out.push(AcousticBox { center, half: [0.5 * (x1 - x0), 0.5 * (y1 - y0), 0.5 * (z1 - z0)], yaw });
        };
        push(f.x0, f.x1, bottom, top, f.z0, f.z0 + WALL_T);
        push(f.x0, f.x1, bottom, top, f.z1 - WALL_T, f.z1);
        push(f.x0, f.x0 + WALL_T, bottom, top, f.z0 + WALL_T, f.z1 - WALL_T);
        push(f.x1 - WALL_T, f.x1, bottom, top, f.z0 + WALL_T, f.z1 - WALL_T);
        push(f.x0, f.x1, lv.line(v.floors) - SLAB, lv.line(v.floors), f.z0, f.z1);
        if v.floors >= 2 {
            push(f.x0, f.x1, lv.line(1) - SLAB, lv.line(1), f.z0, f.z1);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::building::{exterior_with, spec_of};
    use crate::plan::Plan;

    /// The first distance at which the ray `from` + t * `dir` meets a triangle of the builder (Moller and Trumbore), within `max`.
    fn hit(mb: &MeshBuilder, from: V3, dir: V3, max: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        for part in &mb.parts {
            for t in part.idx.chunks_exact(3) {
                let (a, b, c) = (part.pos[t[0] as usize], part.pos[t[1] as usize], part.pos[t[2] as usize]);
                let (e1, e2) = (sub(b, a), sub(c, a));
                let pv = cross(dir, e2);
                let det = dot(e1, pv);
                if det.abs() < 1e-9 {
                    continue;
                }
                let inv = 1.0 / det;
                let tv = sub(from, a);
                let u = dot(tv, pv) * inv;
                if !(0.0..=1.0).contains(&u) {
                    continue;
                }
                let qv = cross(tv, e1);
                let v = dot(dir, qv) * inv;
                if v < 0.0 || u + v > 1.0 {
                    continue;
                }
                let d = dot(e2, qv) * inv;
                if d > 1e-4 && d <= max && best.is_none_or(|b| d < b) {
                    best = Some(d);
                }
            }
        }
        best
    }

    #[test]
    fn the_shell_stops_a_ray_at_a_wall_and_lets_it_through_a_doorway() {
        let lib = Lib::from_json("");
        let plan = Plan::new(9);
        let (mut checked, mut shown) = (0, 0);
        'blocks: for (i, j) in [(2, 3), (0, 0), (5, -4), (-3, 2), (7, 7)] {
            for lp in plan.lots(i, j) {
                let spec = spec_of(&lib, &lp.lot, lp.kind, lp.key);
                let (doors, _) = exterior_with(&mut MeshBuilder::new(), &lib, &spec, false);
                let Some(d) = doors.iter().find(|d| d.width > 0.7 && d.hinge[1] <= spec.vols[0].y0 + 0.6) else { continue };
                let mut mb = MeshBuilder::new();
                shell(&mut mb, &lib, &spec, &doors);
                assert!(mb.triangles() > 20 && mb.triangles() < 20_000, "{} triangles", mb.triangles());
                // a ray at head height from 4 m outside, straight through the middle of the doorway, into the building
                let mid = add(add(d.hinge, scale(d.along, d.width * 0.5)), [0.0, 1.4, 0.0]);
                let from = add(mid, scale(d.out, 4.0));
                let into = scale(d.out, -1.0);
                let through = hit(&mb, from, into, 60.0);
                // and the same ray a metre and a half to the side of the opening, where the wall is
                let wall_at = add(mid, scale(d.along, d.width * 0.5 + 1.5));
                let beside = hit(&mb, add(wall_at, scale(d.out, 4.0)), into, 60.0);
                if let Some(w) = beside {
                    // the wall is about 4 m away; the door's wall must not be what the doorway ray meets first
                    assert!((3.0..5.0).contains(&w), "the wall beside the door is at {w}");
                    assert!(through.is_none_or(|t| t > w + 1.0), "through the doorway {through:?}, the wall beside it at {w}");
                    shown += 1;
                }
                checked += 1;
                if checked >= 6 {
                    break 'blocks;
                }
            }
        }
        assert!(checked >= 3, "buildings with a street door were found: {checked}");
        assert!(shown >= 1, "a wall beside a door was found: {shown}");
    }

    #[test]
    fn a_tower_shell_has_a_slab_for_every_floor_and_is_small() {
        let lib = Lib::from_json("");
        let plan = Plan::new(9);
        let mut tallest: Option<(u32, usize)> = None;
        for j in -12..12 {
            for i in -12..12 {
                for lp in plan.lots(i, j) {
                    let spec = spec_of(&lib, &lp.lot, lp.kind, lp.key);
                    let n = spec.floor_count();
                    if tallest.is_none_or(|t| n > t.0) {
                        let (doors, _) = exterior_with(&mut MeshBuilder::new(), &lib, &spec, false);
                        let mut mb = MeshBuilder::new();
                        shell(&mut mb, &lib, &spec, &doors);
                        tallest = Some((n, mb.triangles()));
                    }
                }
            }
        }
        let (floors, tris) = tallest.unwrap();
        assert!(floors >= 10, "a tall building was found: {floors} floors");
        // 12 triangles a slab and a handful of boxes for the walls
        assert!(tris >= 12 * floors as usize && tris < 60 * floors as usize + 400, "{tris} triangles for {floors} floors");
    }

    #[test]
    fn the_boxes_for_the_sound_stand_where_the_walls_are_and_a_ray_from_a_room_meets_them() {
        let lib = Lib::from_json("");
        let plan = Plan::new(9);
        let mut checked = 0;
        for (i, j) in [(2, 3), (0, 0), (5, -4), (-3, 2)] {
            for lp in plan.lots(i, j) {
                let spec = spec_of(&lib, &lp.lot, lp.kind, lp.key);
                let boxes = acoustic_boxes(&spec);
                assert!(boxes.len() >= 5 * spec.vols.len() && boxes.len() <= 7 * spec.vols.len(), "{} boxes for {} volumes", boxes.len(), spec.vols.len());
                // every wall box is as thick as a wall and no larger than the lot's own size plus the walls
                let (w, d) = (lp.lot.w, lp.lot.d);
                for b in &boxes {
                    assert!(b.half.iter().all(|h| *h > 0.0) && b.half[0] <= 0.5 * w.max(d) + 1.0, "{b:?}");
                }
                // the middle of the first volume's ground floor is inside the walls: a ray from there in any horizontal direction meets a wall box within the lot's size
                let v = &spec.vols[0];
                let fr = v.foot.frame();
                let mid = fr.at(0.5 * (v.foot.x0 + v.foot.x1), v.y0 + 1.5, 0.5 * (v.foot.z0 + v.foot.z1));
                let (sn, cs) = boxes[0].yaw.sin_cos();
                let _ = (sn, cs);
                for k in 0..8 {
                    let a = k as f32 * std::f32::consts::FRAC_PI_4 + 0.3;
                    let dir = [a.cos(), 0.0, a.sin()];
                    let met = boxes.iter().any(|b| {
                        // the ray in the box's own frame (x along (cos yaw, 0, -sin yaw), z along (sin yaw, 0, cos yaw)): a slab test
                        let (s, c) = b.yaw.sin_cos();
                        let (ax, az) = ([c, 0.0, -s], [s, 0.0, c]);
                        let rel = sub(mid, b.center);
                        let (lo, ld) = ([dot(rel, ax), rel[1], dot(rel, az)], [dot(dir, ax), dir[1], dot(dir, az)]);
                        let (mut t0, mut t1) = (f32::NEG_INFINITY, f32::INFINITY);
                        for q in 0..3 {
                            if ld[q].abs() < 1e-9 {
                                if lo[q].abs() > b.half[q] {
                                    return false;
                                }
                                continue;
                            }
                            let (a0, a1) = ((-b.half[q] - lo[q]) / ld[q], (b.half[q] - lo[q]) / ld[q]);
                            t0 = t0.max(a0.min(a1));
                            t1 = t1.min(a0.max(a1));
                        }
                        t0 <= t1 && t1 > 0.0 && t0 < 80.0
                    });
                    assert!(met, "a ray from inside the building at {a} rad meets no box ({} boxes)", boxes.len());
                }
                checked += 1;
            }
        }
        assert!(checked >= 8, "{checked} buildings checked");
    }
}
