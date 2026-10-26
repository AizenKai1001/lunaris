//! Real models: street furniture, trees, furniture and fittings from Poly Haven's CC0 library (tools/citygen/fetch_models.py), read from their glTF files and placed, scaled and turned, into the mesh being built.
//! Their pictures (colour, normal, occlusion-roughness-metallic) become materials of the library like any other, so a lamp post and a wall share one model and one set of draws.

use crate::materials::{Lib, MatDef};
use crate::mesh::*;
use lunaris_json::Json;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct PropPart {
    pub mat: usize,
    /// A cut-out material (leaves, fences): its triangles are cards that are thinned, not merged, when the model is simplified.
    pub cut: bool,
    pub pos: Vec<V3>,
    pub nrm: Vec<V3>,
    pub uv: Vec<[f32; 2]>,
    pub idx: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Prop {
    pub name: String,
    pub parts: Vec<PropPart>,
    pub lo: V3,
    pub hi: V3,
    /// The material of a clump of this tree's leaves (`clumpify`), which far-off copies wear on a few crossed cards.
    pub crown: Option<usize>,
}

impl Prop {
    pub fn size(&self) -> V3 {
        sub(self.hi, self.lo)
    }

    pub fn triangles(&self) -> usize {
        self.parts.iter().map(|p| p.idx.len() / 3).sum()
    }

    /// A simpler copy with about `target` triangles, by quadric error metrics (`simplify`): furniture photographed at 100,000 triangles becomes something a city of a million rooms can hold, and keeps its silhouette
    /// and its pictures. The budget is shared among the parts (materials) in proportion to their triangles. The cut-out parts of a tree (leaf cards) are not simplified that way, which would melt the cards into a few
    /// big shards: they are thinned (`thin_cards`: some cards are dropped and the rest made bigger by the area that was lost), and take 60% of the budget when there is bark too.
    pub fn decimated(&self, target: usize) -> Prop {
        if self.triangles() <= target {
            return self.clone();
        }
        let tris = |parts: &[&PropPart]| parts.iter().map(|p| p.idx.len() / 3).sum::<usize>();
        let cut: Vec<&PropPart> = self.parts.iter().filter(|p| p.cut).collect();
        let solid: Vec<&PropPart> = self.parts.iter().filter(|p| !p.cut).collect();
        let (cut_budget, solid_budget) = match (cut.is_empty(), solid.is_empty()) {
            (false, false) => (target * 6 / 10, target - target * 6 / 10),
            (false, true) => (target, 0),
            _ => (0, target),
        };
        let mut parts: Vec<PropPart> = cut.iter().map(|p| thin_cards(p, (cut_budget * (p.idx.len() / 3) / tris(&cut).max(1)).max(8))).collect();
        if !solid.is_empty() {
            // very heavy models are first brought down by clustering (fast), then simplified by quadric error metrics
            let mut base = Prop { name: self.name.clone(), parts: solid.iter().map(|p| (*p).clone()).collect(), lo: self.lo, hi: self.hi, crown: self.crown };
            if base.triangles() > 250_000 {
                let (mut lo, mut hi) = (2usize, 512usize);
                let mut best = cluster(&base, lo);
                while hi > lo + 1 {
                    let mid = (lo + hi) / 2;
                    let c = cluster(&base, mid);
                    if c.triangles() <= 250_000 {
                        lo = mid;
                        best = c;
                    } else {
                        hi = mid;
                    }
                }
                base = best;
            }
            let total = base.triangles().max(1);
            parts.extend(base.parts.iter().map(|p| crate::simplify::simplify(p, (solid_budget * (p.idx.len() / 3) / total).max(12))).filter(|p| !p.idx.is_empty()));
        }
        Prop { name: self.name.clone(), parts, lo: self.lo, hi: self.hi, crown: self.crown }
    }

    /// Put a copy into `mb`: turned `yaw` degrees about up, scaled, with its lowest point (the middle of its base) at `pos`.
    pub fn place(&self, mb: &mut MeshBuilder, pos: V3, yaw: f32, scale_by: f32, tint: V3) {
        let (s, c) = (yaw.to_radians().sin(), yaw.to_radians().cos());
        let cx = (self.lo[0] + self.hi[0]) * 0.5;
        let cz = (self.lo[2] + self.hi[2]) * 0.5;
        let xf = |p: V3| -> V3 {
            let (x, y, z) = ((p[0] - cx) * scale_by, (p[1] - self.lo[1]) * scale_by, (p[2] - cz) * scale_by);
            [pos[0] + x * c + z * s, pos[1] + y, pos[2] - x * s + z * c]
        };
        let xn = |n: V3| -> V3 { [n[0] * c + n[2] * s, n[1], -n[0] * s + n[2] * c] };
        for part in &self.parts {
            if mb.parts.len() <= part.mat {
                mb.parts.resize_with(part.mat + 1, Part::default);
            }
            let dst = &mut mb.parts[part.mat];
            let base = dst.pos.len() as u32;
            for i in 0..part.pos.len() {
                dst.pos.push(xf(part.pos[i]));
                dst.nrm.push(xn(part.nrm[i]));
                dst.uv.push(part.uv[i]);
                dst.col.push(tint);
            }
            dst.idx.extend(part.idx.iter().map(|i| i + base));
        }
    }
}

/// A hash of an integer to [0, 1): which cards stay.
fn unit_hash(mut x: u64) -> f32 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((x ^ (x >> 31)) >> 40) as f32 / (1u64 << 24) as f32
}

/// Thin a part made of many small separate pieces (the leaf cards of a tree) to about `target` triangles: the pieces (triangles that share corners) are told apart, a hashed fraction of them is kept, and every kept
/// piece is scaled about its middle by the square root of what was dropped (at most 2.4), so that the crown is as full as it was with fewer, bigger leaves.
fn thin_cards(part: &PropPart, target: usize) -> PropPart {
    let tris = part.idx.len() / 3;
    if tris <= target {
        return part.clone();
    }
    // weld the corners by position, then join the triangles that share a welded corner
    let q = |v: V3| ((v[0] * 400.0).round() as i32, (v[1] * 400.0).round() as i32, (v[2] * 400.0).round() as i32);
    let mut weld: HashMap<(i32, i32, i32), u32> = HashMap::new();
    let ids: Vec<u32> = part.pos.iter().map(|&v| {
        let n = weld.len() as u32;
        *weld.entry(q(v)).or_insert(n)
    }).collect();
    let mut parent: Vec<u32> = (0..weld.len() as u32).collect();
    fn find(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            p[x as usize] = p[p[x as usize] as usize];
            x = p[x as usize];
        }
        x
    }
    for t in part.idx.chunks_exact(3) {
        let (a, b, c) = (ids[t[0] as usize], ids[t[1] as usize], ids[t[2] as usize]);
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        parent[ra as usize] = rb;
        let (rb, rc) = (find(&mut parent, b), find(&mut parent, c));
        parent[rb as usize] = rc;
    }
    // the pieces: root -> (triangles, sum of corners, corners)
    let mut pieces: HashMap<u32, (Vec<usize>, V3, u32)> = HashMap::new();
    for (ti, t) in part.idx.chunks_exact(3).enumerate() {
        let root = find(&mut parent, ids[t[0] as usize]);
        let e = pieces.entry(root).or_insert_with(|| (Vec::new(), [0.0; 3], 0));
        e.0.push(ti);
        for k in 0..3 {
            e.1 = add(e.1, part.pos[t[k] as usize]);
        }
        e.2 += 3;
    }
    let keep = (target as f32 / tris as f32).clamp(0.0, 1.0);
    let grow = (1.0 / keep).sqrt().min(2.4);
    let mut roots: Vec<u32> = pieces.keys().copied().collect();
    roots.sort_unstable();
    let mut out = PropPart { mat: part.mat, cut: true, pos: Vec::new(), nrm: Vec::new(), uv: Vec::new(), idx: Vec::new() };
    for root in roots {
        if unit_hash(root as u64) >= keep {
            continue;
        }
        let (list, sum, n) = &pieces[&root];
        let c = scale(*sum, 1.0 / *n as f32);
        let mut local: HashMap<u32, u32> = HashMap::new();
        for &ti in list {
            for k in 0..3 {
                let v = part.idx[ti * 3 + k];
                let id = *local.entry(v).or_insert_with(|| {
                    out.pos.push(add(c, scale(sub(part.pos[v as usize], c), grow)));
                    out.nrm.push(part.nrm[v as usize]);
                    out.uv.push(part.uv[v as usize]);
                    out.pos.len() as u32 - 1
                });
                out.idx.push(id);
            }
        }
    }
    out
}

/// Cluster the vertices of every part on a grid of `res` cells along the longest side.
fn cluster(p: &Prop, res: usize) -> Prop {
    let size = p.size();
    let cell = size[0].max(size[1]).max(size[2]).max(1e-4) / res as f32;
    let mut parts = Vec::new();
    for part in &p.parts {
        let mut index: HashMap<(i32, i32, i32), u32> = HashMap::new();
        let mut sums: Vec<(V3, V3, [f32; 2], u32)> = Vec::new();
        let mut map: Vec<u32> = Vec::with_capacity(part.pos.len());
        for i in 0..part.pos.len() {
            let v = part.pos[i];
            let key = (((v[0] - p.lo[0]) / cell) as i32, ((v[1] - p.lo[1]) / cell) as i32, ((v[2] - p.lo[2]) / cell) as i32);
            let id = *index.entry(key).or_insert_with(|| {
                sums.push(([0.0; 3], [0.0; 3], part.uv[i], 0));
                sums.len() as u32 - 1
            });
            let s = &mut sums[id as usize];
            s.0 = add(s.0, v);
            s.1 = add(s.1, part.nrm[i]);
            s.3 += 1;
            map.push(id);
        }
        let mut idx = Vec::new();
        for t in part.idx.chunks_exact(3) {
            let (a, b, c) = (map[t[0] as usize], map[t[1] as usize], map[t[2] as usize]);
            if a != b && b != c && a != c {
                idx.extend_from_slice(&[a, b, c]);
            }
        }
        if idx.is_empty() {
            continue;
        }
        // keep only the clusters that triangles use
        let mut used = vec![u32::MAX; sums.len()];
        let mut out = PropPart { mat: part.mat, cut: part.cut, pos: Vec::new(), nrm: Vec::new(), uv: Vec::new(), idx: Vec::with_capacity(idx.len()) };
        for i in idx {
            if used[i as usize] == u32::MAX {
                used[i as usize] = out.pos.len() as u32;
                let s = &sums[i as usize];
                out.pos.push(scale(s.0, 1.0 / s.3 as f32));
                out.nrm.push(norm(s.1));
                out.uv.push(s.2);
            }
            out.idx.push(used[i as usize]);
        }
        parts.push(out);
    }
    Prop { name: p.name.clone(), parts, lo: p.lo, hi: p.hi, crown: p.crown }
}

/// How many triangles a model may keep when it is placed in a room: furniture is simplified hard (there are tens of pieces in a room and thousands of rooms near the player), small things more.
/// Street things and trees are not furniture: 0 leaves them out of a room.
pub fn indoor_budget(slug: &str) -> usize {
    let s = slug.to_lowercase();
    let has = |ks: &[&str]| ks.iter().any(|k| s.contains(k));
    if has(&["tree", "pine", "fir_", "jacaranda", "shrub", "street_lamp", "pole", "hydrant", "barrier", "manhole", "utility", "security_light", "aircon", "fire_escape", "seating", "chalkboard", "planter", "trash", "rollershutter", "exterior", "covered_car", "picnic", "trashbag", "compost", "wetfloor", "payphone", "generator", "chainlink", "propane", "jerrycan", "old_tyre", "power_box", "painted_wooden_bench", "barrel", "crate_02"]) {
        0
    } else if has(&["chair", "stool"]) {
        2200
    } else if has(&["table", "desk"]) {
        2800
    } else if has(&["sofa", "bed", "armchair", "daybed", "lounge"]) {
        4500
    } else if has(&["shelf", "shelves", "cabinet", "rack", "drawer", "bookshelf", "nightstand", "console", "cart", "truck"]) {
        2400
    } else if has(&["plant", "calathea", "pachira"]) {
        5000
    } else if has(&["lamp", "light", "fluorescent", "chandelier"]) {
        1800
    } else if has(&["frame", "clock", "mirror"]) {
        500
    } else {
        2200
    }
}

pub struct Props {
    pub map: HashMap<String, Prop>,
}

impl Props {
    pub fn get(&self, slug: &str) -> Option<&Prop> {
        self.map.get(slug)
    }

    /// The first of these slugs that was loaded.
    pub fn first<'a>(&self, slugs: &[&'a str]) -> Option<&'a str> {
        slugs.iter().copied().find(|s| self.map.contains_key(*s))
    }

    /// Load every `<dir>/<slug>/<slug>.gltf` (the folder `city/props` of the workspace) and simplify it to the triangle budget `budget(slug)` gives (0 leaves it out, unread): a scanned plant of 170,000 triangles is not furniture for a city of a million rooms. Materials are added to the library with picture paths relative to the folder above `dir` (where the cell models are written).
    pub fn load_dir(lib: &mut Lib, dir: &Path, budget: &dyn Fn(&str) -> usize) -> Props {
        Props::load_dir_with(lib, dir, budget, &|_| 0)
    }

    /// The same, and a model whose `cards(slug)` is not 0 has its leaf cards (cut-out parts) replaced by that many big cards wearing a picture of a clump of its own leaves (`clumpify`; `tools/citygen/make_clumps.py` makes the picture).
    ///
    /// A folder that holds an empty file called `voxel` is a model whose geometry is its own (flat quads and one small palette picture: simplifying it would wreck it): it is loaded as it is, with the triangles it has, and none of `decimated`, `clumpify`, `attach_crown`
    /// or `drop_ground_sheets` touches it. A budget of 0 still leaves it out.
    pub fn load_dir_with(lib: &mut Lib, dir: &Path, budget: &dyn Fn(&str) -> usize, cards: &dyn Fn(&str) -> usize) -> Props {
        let mut map = HashMap::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            let mut slugs: Vec<String> = rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
            slugs.sort();
            for slug in slugs {
                let gltf = dir.join(&slug).join(format!("{slug}.gltf"));
                let b = budget(&slug);
                if b > 0 && gltf.exists() {
                    match load(lib, &slug, &gltf) {
                        Ok(p) => {
                            if dir.join(&slug).join("voxel").exists() {
                                map.insert(slug, p);
                                continue;
                            }
                            let n = cards(&slug);
                            let p = if n > 0 {
                                clumpify(lib, p, dir, n)
                            } else if slug.starts_with("kit_") {
                                attach_crown(lib, p, dir)
                            } else {
                                p
                            };
                            map.insert(slug, p.decimated(b));
                        }
                        Err(e) => eprintln!("prop {slug}: {e}"),
                    }
                }
            }
        }
        Props { map }
    }
}

/// How many triangles a model may keep outdoors (a street, a park): 0 leaves it out. The trunk and branches of a tree keep their budget and the crown is `outdoor_cards` big cards on top of it.
pub fn outdoor_budget(slug: &str) -> usize {
    let s = slug.to_lowercase();
    let has = |ks: &[&str]| ks.iter().any(|k| s.contains(k));
    if s.starts_with("kit_") {
        // the dome city's own trees and shrubs (art/city/blender/make_trees.py) are made to be shown as they are: thousands of triangles of bark and single leaves
        6000
    } else if has(&["tree", "pine", "fir_", "jacaranda"]) {
        4500
    } else if has(&["shrub"]) {
        800
    } else if has(&["street_lamp_01"]) {
        3200
    } else if has(&["street_lamp_02"]) {
        1800
    } else if has(&["hydrant"]) {
        1400
    } else if has(&["seating"]) {
        3000
    } else if has(&["utility_box", "planter"]) {
        1600
    } else if has(&["manhole"]) {
        500
    } else if has(&["trash", "barrier"]) {
        1500
    } else if has(&["covered_car", "wreck_"]) {
        6000
    } else if has(&["painted_wooden_bench"]) {
        800
    } else if has(&["picnic"]) {
        5000
    } else if has(&["trashbag"]) {
        1500
    } else if has(&["standing_chalkboard"]) {
        1500
    } else if has(&["wetfloorsign"]) {
        400
    } else {
        0
    }
}

/// The crown of an outdoor model in big cards: trees 520, shrubs 90, nothing else.
pub fn outdoor_cards(slug: &str) -> usize {
    let s = slug.to_lowercase();
    if s.starts_with("kit_") {
        0
    } else if ["tree", "pine", "fir_", "jacaranda"].iter().any(|k| s.contains(k)) {
        520
    } else if s.contains("shrub") {
        90
    } else {
        0
    }
}

/// Some tree models carry a flat patch of ground cover or a cloth of roots round the foot: the triangles that lie flat within a few centimetres of the lowest point and spread over half a metre are taken out.
fn drop_ground_sheets(part: &PropPart, floor: f32) -> PropPart {
    let mut out = PropPart { mat: part.mat, cut: part.cut, pos: part.pos.clone(), nrm: part.nrm.clone(), uv: part.uv.clone(), idx: Vec::with_capacity(part.idx.len()) };
    for t in part.idx.chunks_exact(3) {
        let v: Vec<V3> = t.iter().map(|&i| part.pos[i as usize]).collect();
        let top = v.iter().fold(f32::MIN, |m, p| m.max(p[1]));
        let ext = |k: usize| v.iter().fold(f32::MIN, |m, p| m.max(p[k])) - v.iter().fold(f32::MAX, |m, p| m.min(p[k]));
        if top < floor + 0.3 && ext(0).max(ext(2)) > 0.5 {
            continue;
        }
        out.idx.extend_from_slice(t);
    }
    out
}

/// A kit tree keeps its own leaf cards; only its far-off copies (level 1) need the picture of a clump of its leaves (`<slug>_leaves_clump.png` beside the leaf atlas) to wear on a few crossed cards: the material of that picture becomes the model's `crown`.
fn attach_crown(lib: &mut Lib, prop: Prop, dir: &Path) -> Prop {
    let root = dir.parent().unwrap_or(dir);
    let mut crown = None;
    for part in &prop.parts {
        if !part.cut {
            continue;
        }
        let clump = lib.mats[part.mat].diff.as_ref().map(|d| d.replace("_diff_1k.png", "_clump.png")).filter(|c| c != lib.mats[part.mat].diff.as_ref().unwrap() && root.join(c).is_file());
        let Some(clump) = clump else { continue };
        let mut def = lib.mats[part.mat].clone();
        def.name = format!("{}:clump", def.name);
        def.diff = Some(clump);
        def.nor = None;
        def.arm = None;
        def.color = [1.0; 3];
        def.rough = 0.85;
        def.metal = 0.0;
        def.double_sided = true;
        def.alpha_cutoff = 0.35;
        crown = Some(lib.add_material(def));
        break;
    }
    Prop { crown, ..prop }
}

/// Replace the cut-out parts of a tree (a million cards, one leaf each) by `cards` big ones: each is put where a hashed-picked triangle of the leaves is, turned the way that leaf faces (and a good deal upwards, like the
/// light on a crown), and wears the picture of a dense clump of the tree's own leaves (`<name>_clump.png` beside the leaf atlas, from tools/citygen/make_clumps.py). A part whose atlas has no clump picture stays as it was.
fn clumpify(lib: &mut Lib, prop: Prop, dir: &Path, cards: usize) -> Prop {
    let root = dir.parent().unwrap_or(dir);
    let size = prop.size();
    let crown = size[0].max(size[2]).max(size[1] * 0.5);
    let card = (crown * 0.17).clamp(0.5, 3.2);
    let mut rng = crate::rng::Rng::keyed(&[prop.name.len() as u64, prop.name.bytes().fold(7u64, |h, b| h.wrapping_mul(131).wrapping_add(b as u64))]);
    let mut parts: Vec<PropPart> = Vec::new();
    let mut crown_mat = None;
    let cut_total: usize = prop.parts.iter().filter(|p| p.cut).map(|p| p.idx.len() / 3).sum();
    for part in &prop.parts {
        let clump = if part.cut { lib.mats[part.mat].diff.as_ref().map(|d| d.replace("_diff_1k.png", "_clump.png")).filter(|c| c != lib.mats[part.mat].diff.as_ref().unwrap() && root.join(c).is_file()) } else { None };
        let Some(clump) = clump else {
            parts.push(drop_ground_sheets(part, prop.lo[1]));
            continue;
        };
        let mut def = lib.mats[part.mat].clone();
        def.name = format!("{}:clump", def.name);
        def.diff = Some(clump);
        def.nor = None;
        def.arm = None;
        def.color = [1.0; 3];
        def.rough = 0.85;
        def.metal = 0.0;
        def.double_sided = true;
        def.alpha_cutoff = 0.35;
        let mat = lib.add_material(def);
        crown_mat.get_or_insert(mat);
        // triangles by area, to pick from
        let tris = part.idx.len() / 3;
        let mut cum: Vec<f32> = Vec::with_capacity(tris);
        let mut total = 0.0f32;
        for t in part.idx.chunks_exact(3) {
            let (a, b, c) = (part.pos[t[0] as usize], part.pos[t[1] as usize], part.pos[t[2] as usize]);
            total += 0.5 * len(cross(sub(b, a), sub(c, a)));
            cum.push(total);
        }
        if total <= 0.0 {
            continue;
        }
        let n = (cards * (tris / 3).max(1) / (cut_total / 3).max(1)).clamp(8, cards);
        let mut out = PropPart { mat, cut: true, pos: Vec::new(), nrm: Vec::new(), uv: Vec::new(), idx: Vec::new() };
        for _ in 0..n {
            let r = rng.f32() * total;
            let ti = cum.partition_point(|&c| c < r).min(tris - 1);
            let t = &part.idx[ti * 3..ti * 3 + 3];
            let (a, b, c) = (part.pos[t[0] as usize], part.pos[t[1] as usize], part.pos[t[2] as usize]);
            let (mut u, mut v) = (rng.f32(), rng.f32());
            if u + v > 1.0 {
                u = 1.0 - u;
                v = 1.0 - v;
            }
            let centre = add(a, add(scale(sub(b, a), u), scale(sub(c, a), v)));
            let face = norm(cross(sub(b, a), sub(c, a)));
            // the card turns a good deal towards the sky, with some chance in it
            let wish = norm([face[0] + rng.range(-0.6, 0.6), face[1].abs() + 0.9 + rng.range(0.0, 0.5), face[2] + rng.range(-0.6, 0.6)]);
            let side = norm(cross(wish, if wish[1].abs() > 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }));
            let up = cross(wish, side);
            let h = card * rng.range(0.8, 1.25) * 0.5;
            let base = out.pos.len() as u32;
            for (du, dv, uv) in [(-1.0, -1.0, [0.0, 1.0]), (1.0, -1.0, [1.0, 1.0]), (1.0, 1.0, [1.0, 0.0]), (-1.0, 1.0, [0.0, 0.0])] {
                out.pos.push(add(centre, add(scale(side, du * h), scale(up, dv * h))));
                out.nrm.push([0.0, 1.0, 0.0]);
                out.uv.push(uv);
            }
            out.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        parts.push(out);
    }
    Prop { parts, crown: crown_mat, ..prop }
}

// ------------------------------------------------------------------------------------------------ glTF reading

type M4 = [[f32; 4]; 4];

fn identity() -> M4 {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

fn mul(a: &M4, b: &M4) -> M4 {
    let mut r = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            r[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

fn node_matrix(n: &Json) -> M4 {
    if let Some(m) = n.get("matrix").and_then(|m| m.as_array()) {
        if m.len() == 16 {
            // glTF is column-major
            let mut r = identity();
            for c in 0..4 {
                for rr in 0..4 {
                    r[rr][c] = m[c * 4 + rr].as_f64().unwrap_or(0.0) as f32;
                }
            }
            return r;
        }
    }
    let vec = |k: &str, d: &[f32]| -> Vec<f32> { n.get(k).and_then(|a| a.as_array()).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_else(|| d.to_vec()) };
    let t = vec("translation", &[0.0, 0.0, 0.0]);
    let q = vec("rotation", &[0.0, 0.0, 0.0, 1.0]);
    let s = vec("scale", &[1.0, 1.0, 1.0]);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let rot = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ];
    let mut r = identity();
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = rot[i][j] * s[j];
        }
        r[i][3] = t[i];
    }
    r
}

fn apply(m: &M4, p: V3) -> V3 {
    [m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + m[0][3], m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + m[1][3], m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + m[2][3]]
}

fn apply_dir(m: &M4, p: V3) -> V3 {
    norm([m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2], m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2], m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2]])
}

struct Doc<'a> {
    j: &'a Json,
    bin: Vec<Vec<u8>>,
}

impl Doc<'_> {
    fn arr(&self, k: &str) -> &[Json] {
        self.j.get(k).and_then(|a| a.as_array()).unwrap_or(&[])
    }

    /// Floats of an accessor (`comps` per element).
    fn floats(&self, acc: usize, comps: usize) -> Option<Vec<f32>> {
        let a = self.arr("accessors").get(acc)?;
        let view = self.arr("bufferViews").get(a.get("bufferView")?.as_u64()? as usize)?;
        let buf = self.bin.get(view.get("buffer")?.as_u64()? as usize)?;
        let count = a.get("count")?.as_u64()? as usize;
        if a.get("componentType")?.as_u64()? != 5126 {
            return None;
        }
        let stride = view.get("byteStride").and_then(|s| s.as_u64()).unwrap_or(0) as usize;
        let stride = if stride == 0 { comps * 4 } else { stride };
        let off = view.get("byteOffset").and_then(|s| s.as_u64()).unwrap_or(0) as usize + a.get("byteOffset").and_then(|s| s.as_u64()).unwrap_or(0) as usize;
        let mut out = Vec::with_capacity(count * comps);
        for i in 0..count {
            for c in 0..comps {
                let at = off + i * stride + c * 4;
                out.push(f32::from_le_bytes(buf.get(at..at + 4)?.try_into().ok()?));
            }
        }
        Some(out)
    }

    fn indices(&self, acc: usize) -> Option<Vec<u32>> {
        let a = self.arr("accessors").get(acc)?;
        let view = self.arr("bufferViews").get(a.get("bufferView")?.as_u64()? as usize)?;
        let buf = self.bin.get(view.get("buffer")?.as_u64()? as usize)?;
        let count = a.get("count")?.as_u64()? as usize;
        let ct = a.get("componentType")?.as_u64()?;
        let off = view.get("byteOffset").and_then(|s| s.as_u64()).unwrap_or(0) as usize + a.get("byteOffset").and_then(|s| s.as_u64()).unwrap_or(0) as usize;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            out.push(match ct {
                5121 => *buf.get(off + i)? as u32,
                5123 => u16::from_le_bytes(buf.get(off + i * 2..off + i * 2 + 2)?.try_into().ok()?) as u32,
                5125 => u32::from_le_bytes(buf.get(off + i * 4..off + i * 4 + 4)?.try_into().ok()?),
                _ => return None,
            });
        }
        Some(out)
    }

    fn image_uri(&self, tex: &Json) -> Option<String> {
        let t = self.arr("textures").get(tex.get("index")?.as_u64()? as usize)?;
        let img = self.arr("images").get(t.get("source")?.as_u64()? as usize)?;
        Some(img.get("uri")?.as_str()?.to_string())
    }
}

/// Read one model, add its materials to `lib` and return it with the base on y = 0 near the origin.
pub fn load(lib: &mut Lib, slug: &str, path: &Path) -> Result<Prop, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let j = lunaris_json::parse(&text).map_err(|e| e.to_string())?;
    let dir = path.parent().unwrap();
    let mut bin = Vec::new();
    for b in j.get("buffers").and_then(|b| b.as_array()).unwrap_or(&[]) {
        let uri = b.get("uri").and_then(|u| u.as_str()).ok_or("a buffer without a uri")?;
        bin.push(std::fs::read(dir.join(uri)).map_err(|e| format!("{uri}: {e}"))?);
    }
    let doc = Doc { j: &j, bin };
    // materials of this model, as library material numbers (made on first use)
    let mut mat_ids: HashMap<usize, usize> = HashMap::new();
    let mut get_mat = |lib: &mut Lib, mi: Option<usize>| -> usize {
        let key = mi.unwrap_or(usize::MAX);
        if let Some(id) = mat_ids.get(&key) {
            return *id;
        }
        let m = mi.and_then(|i| doc.arr("materials").get(i));
        let name = format!("p:{slug}:{}", m.and_then(|m| m.get("name")).and_then(|n| n.as_str()).unwrap_or("m"));
        let pbr = m.and_then(|m| m.get("pbrMetallicRoughness"));
        let rel = |u: String| format!("props/{slug}/{u}");
        let diff = pbr.and_then(|p| p.get("baseColorTexture")).and_then(|t| doc.image_uri(t)).map(rel);
        let arm = pbr.and_then(|p| p.get("metallicRoughnessTexture")).and_then(|t| doc.image_uri(t)).map(rel);
        let nor = m.and_then(|m| m.get("normalTexture")).and_then(|t| doc.image_uri(t)).map(rel);
        let factor = pbr.and_then(|p| p.get("baseColorFactor")).and_then(|f| f.as_array()).map(|f| [f[0].as_f64().unwrap_or(1.0) as f32, f[1].as_f64().unwrap_or(1.0) as f32, f[2].as_f64().unwrap_or(1.0) as f32]).unwrap_or([0.6; 3]);
        let cutoff = match m.and_then(|m| m.get("alphaMode")).and_then(|a| a.as_str()) {
            Some("MASK") => m.and_then(|m| m.get("alphaCutoff")).and_then(|c| c.as_f64()).unwrap_or(0.5) as f32,
            Some("BLEND") => 0.5,
            _ => 0.0,
        };
        let double = m.and_then(|m| m.get("doubleSided")).and_then(|d| d.as_bool()).unwrap_or(false) || cutoff > 0.0;
        let rough = pbr.and_then(|p| p.get("roughnessFactor")).and_then(|r| r.as_f64()).unwrap_or(0.8) as f32;
        let metal = pbr.and_then(|p| p.get("metallicFactor")).and_then(|r| r.as_f64()).unwrap_or(0.0) as f32;
        let id = lib.add_material(MatDef { name, diff: diff.clone(), nor, arm: arm.clone(), color: if diff.is_some() { [1.0; 3] } else { factor }, rough, metal, size: [1.0, 1.0], double_sided: double, alpha_cutoff: cutoff, avg: if diff.is_some() { [0.4; 3] } else { factor } });
        mat_ids.insert(key, id);
        id
    };
    // walk the scene
    let mut parts: Vec<PropPart> = Vec::new();
    let scene = j.get("scene").and_then(|s| s.as_u64()).unwrap_or(0) as usize;
    let roots: Vec<usize> = doc.arr("scenes").get(scene).and_then(|s| s.get("nodes")).and_then(|n| n.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().map(|v| v as usize)).collect()).unwrap_or_default();
    let mut stack: Vec<(usize, M4)> = roots.into_iter().map(|r| (r, identity())).collect();
    while let Some((ni, parent)) = stack.pop() {
        let Some(node) = doc.arr("nodes").get(ni) else { continue };
        let m = mul(&parent, &node_matrix(node));
        if let Some(kids) = node.get("children").and_then(|c| c.as_array()) {
            for k in kids {
                if let Some(k) = k.as_u64() {
                    stack.push((k as usize, m));
                }
            }
        }
        let Some(mesh) = node.get("mesh").and_then(|x| x.as_u64()).and_then(|x| doc.arr("meshes").get(x as usize)) else { continue };
        for prim in mesh.get("primitives").and_then(|p| p.as_array()).unwrap_or(&[]) {
            let attrs = prim.get("attributes").ok_or("a primitive without attributes")?;
            let Some(pos) = attrs.get("POSITION").and_then(|a| a.as_u64()).and_then(|a| doc.floats(a as usize, 3)) else { continue };
            let n = pos.len() / 3;
            let nrm = attrs.get("NORMAL").and_then(|a| a.as_u64()).and_then(|a| doc.floats(a as usize, 3));
            let uv = attrs.get("TEXCOORD_0").and_then(|a| a.as_u64()).and_then(|a| doc.floats(a as usize, 2));
            let idx = match prim.get("indices").and_then(|a| a.as_u64()) {
                Some(a) => doc.indices(a as usize).ok_or("unreadable indices")?,
                None => (0..n as u32).collect(),
            };
            let mat = get_mat(lib, prim.get("material").and_then(|x| x.as_u64()).map(|x| x as usize));
            let mut part = PropPart { mat, cut: lib.mats[mat].alpha_cutoff > 0.0, pos: Vec::with_capacity(n), nrm: Vec::with_capacity(n), uv: Vec::with_capacity(n), idx };
            for i in 0..n {
                part.pos.push(apply(&m, [pos[i * 3], pos[i * 3 + 1], pos[i * 3 + 2]]));
                part.nrm.push(match &nrm {
                    Some(v) => apply_dir(&m, [v[i * 3], v[i * 3 + 1], v[i * 3 + 2]]),
                    None => [0.0, 1.0, 0.0],
                });
                // glTF has v = 0 at the top of the picture and so does this writer's output
                part.uv.push(match &uv {
                    Some(v) => [v[i * 2], v[i * 2 + 1]],
                    None => [0.0, 0.0],
                });
            }
            parts.push(part);
        }
    }
    if parts.is_empty() {
        return Err("no triangles".into());
    }
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &parts {
        for v in &p.pos {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
    }
    Ok(Prop { name: slug.to_string(), parts, lo, hi, crown: None })
}

#[cfg(test)]
mod thin_tests {
    use super::*;

    #[test]
    fn thinning_cards_keeps_about_the_budget_and_makes_the_cards_that_stay_bigger() {
        // 1000 separate quads (cards) of 0.1 m scattered on a line
        let mut part = PropPart { mat: 0, cut: true, pos: Vec::new(), nrm: Vec::new(), uv: Vec::new(), idx: Vec::new() };
        for i in 0..1000u32 {
            let x = i as f32 * 0.5;
            let b = part.pos.len() as u32;
            for (dx, dz) in [(0.0, 0.0), (0.1, 0.0), (0.1, 0.1), (0.0, 0.1)] {
                part.pos.push([x + dx, 1.0, dz]);
                part.nrm.push([0.0, 1.0, 0.0]);
                part.uv.push([dx * 10.0, dz * 10.0]);
            }
            part.idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        let thin = thin_cards(&part, 500);
        let tris = thin.idx.len() / 3;
        assert!((300..=700).contains(&tris), "{tris} triangles kept of 2000 for a budget of 500");
        // a kept card is about twice as wide as it was (a quarter of them kept: the area grows four times)
        let w = thin.pos[1][0] - thin.pos[0][0];
        assert!(w > 0.15 && w < 0.3, "{w}");
        assert_eq!(thin.pos.len() / 4, tris / 2);
        assert!(thin_cards(&part, 5000).idx.len() == part.idx.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_model_is_read_placed_and_keeps_its_material_and_size() {
        let dir = std::env::temp_dir().join("lunaris_citygen_prop_test").join("box_01");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // one triangle pair: a 2 m x 1 m quad standing in x and y
        let pos: [f32; 12] = [-1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, -1.0, 1.0, 0.0];
        let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let mut bin: Vec<u8> = pos.iter().flat_map(|f| f.to_le_bytes()).collect();
        let idx_at = bin.len();
        bin.extend(idx.iter().flat_map(|i| i.to_le_bytes()));
        std::fs::write(dir.join("box_01.bin"), &bin).unwrap();
        let gltf = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0,"translation":[0,0.5,0]}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1,"material":0}}]}}],
            "materials":[{{"name":"paint","pbrMetallicRoughness":{{"baseColorFactor":[0.2,0.4,0.6,1.0]}}}}],"buffers":[{{"uri":"box_01.bin","byteLength":{}}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":48}},{{"buffer":0,"byteOffset":{idx_at},"byteLength":12}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3"}},{{"bufferView":1,"componentType":5123,"count":6,"type":"SCALAR"}}]}}"#,
            bin.len()
        );
        std::fs::write(dir.join("box_01.gltf"), gltf).unwrap();
        let mut lib = Lib::from_json("");
        let before = lib.mats.len();
        let prop = load(&mut lib, "box_01", &dir.join("box_01.gltf")).unwrap();
        assert_eq!(lib.mats.len(), before + 1);
        assert_eq!(prop.triangles(), 2);
        assert_eq!(prop.decimated(100).triangles(), 2);
        assert!((prop.size()[0] - 2.0).abs() < 1e-5 && (prop.size()[1] - 1.0).abs() < 1e-5);
        assert!((prop.lo[1] - 0.5).abs() < 1e-5);
        let mut mb = MeshBuilder::new();
        prop.place(&mut mb, [10.0, 3.0, 5.0], 90.0, 2.0, [1.0; 3]);
        let (lo, hi) = mb.bounds().unwrap();
        // turned a quarter, doubled in size, its base on y = 3
        assert!((lo[1] - 3.0).abs() < 1e-4 && (hi[1] - 5.0).abs() < 1e-4, "{lo:?} {hi:?}");
        assert!((hi[2] - lo[2]).abs() > 3.9 && (hi[0] - lo[0]).abs() < 0.01, "{lo:?} {hi:?}");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    /// A folder `slug` with a model of `quads` separate flat quads standing in x and y, a metre apart.
    fn quads_folder(root: &Path, slug: &str, quads: usize, voxel: bool) {
        let dir = root.join(slug);
        std::fs::create_dir_all(&dir).unwrap();
        let mut pos: Vec<f32> = Vec::new();
        let mut idx: Vec<u16> = Vec::new();
        for q in 0..quads {
            let x = q as f32 * 2.0;
            pos.extend_from_slice(&[x, 0.0, 0.0, x + 1.0, 0.0, 0.0, x + 1.0, 1.0, 0.0, x, 1.0, 0.0]);
            let b = (q * 4) as u16;
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        let mut bin: Vec<u8> = pos.iter().flat_map(|f| f.to_le_bytes()).collect();
        let idx_at = bin.len();
        bin.extend(idx.iter().flat_map(|i| i.to_le_bytes()));
        std::fs::write(dir.join(format!("{slug}.bin")), &bin).unwrap();
        let gltf = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1,"material":0}}]}}],
            "materials":[{{"name":"paint","pbrMetallicRoughness":{{"baseColorFactor":[0.2,0.4,0.6,1.0]}}}}],"buffers":[{{"uri":"{slug}.bin","byteLength":{}}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":{}}},{{"buffer":0,"byteOffset":{idx_at},"byteLength":{}}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":{},"type":"VEC3"}},{{"bufferView":1,"componentType":5123,"count":{},"type":"SCALAR"}}]}}"#,
            bin.len(),
            pos.len() * 4,
            idx.len() * 2,
            pos.len() / 3,
            idx.len()
        );
        std::fs::write(dir.join(format!("{slug}.gltf")), gltf).unwrap();
        if voxel {
            std::fs::write(dir.join("voxel"), b"").unwrap();
        }
    }

    #[test]
    fn a_voxel_prop_is_loaded_as_it_is_and_the_others_are_brought_down_to_their_budget() {
        let root = std::env::temp_dir().join("lunaris_citygen_voxel_test");
        let _ = std::fs::remove_dir_all(&root);
        quads_folder(&root, "plain_crate", 40, false);
        quads_folder(&root, "voxel_crate", 40, true);
        let mut lib = Lib::from_json("");
        let props = Props::load_dir(&mut lib, &root, &|_| 20);
        assert_eq!(props.get("voxel_crate").unwrap().triangles(), 80, "the triangles it has");
        assert!(props.get("plain_crate").unwrap().triangles() < 80, "{}", props.get("plain_crate").unwrap().triangles());
        // a budget of 0 still leaves a voxel prop out
        let mut lib = Lib::from_json("");
        let none = Props::load_dir(&mut lib, &root, &|s| if s.starts_with("voxel") { 0 } else { 20 });
        assert!(none.get("voxel_crate").is_none());
        let _ = std::fs::remove_dir_all(root);
    }
}
