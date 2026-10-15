//! Built-in material textures made from maths, so that a scene can have a grassy ground, a brick wall or a wooden floor without a single
//! image file. Every one tiles seamlessly (use `uv_scale` on a `MeshRenderer` to repeat it) and is the same on every machine: only
//! integer hashing and IEEE `+ - * /` go into the pixels, no `sin` or `exp` from the platform's maths library.
use crate::texture::Texture;

/// The names of the material textures, besides the little pictures `circle`, `ring`, `checker` and `gradient`.
pub const MATERIALS: [&str; 9] = ["grass", "dirt", "stone", "bricks", "wood", "sand", "concrete", "tiles", "metal"];

const SIZE: u32 = 128;

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / 16_777_216.0
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Value noise on a lattice of `px` x `py` cells that wraps around, so that it tiles.
fn vnoise(u: f32, v: f32, px: i32, py: i32, seed: u32) -> f32 {
    let (x, y) = (u * px as f32, v * py as f32);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (smooth(x - x0), smooth(y - y0));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let h = |dx: i32, dy: i32| hash((ix + dx).rem_euclid(px), (iy + dy).rem_euclid(py), seed);
    lerp(lerp(h(0, 0), h(1, 0), fx), lerp(h(0, 1), h(1, 1), fx), fy)
}

/// Several layers of value noise, each twice as fine and half as strong; 0..1.
fn fbm(u: f32, v: f32, period: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut p) = (0.0, 0.5, 0.0, period);
    for o in 0..octaves {
        sum += amp * vnoise(u, v, p, p, seed.wrapping_add(o * 977));
        norm += amp;
        amp *= 0.5;
        p *= 2;
    }
    sum / norm
}

/// Cellular noise on an `n` x `n` wrapping grid: the distance to the nearest and the second nearest point, and a number identifying
/// the nearest cell (0..1).
fn worley(u: f32, v: f32, n: i32, seed: u32) -> (f32, f32, f32) {
    let (x, y) = (u * n as f32, v * n as f32);
    let (cx, cy) = (x.floor() as i32, y.floor() as i32);
    let (mut f1, mut f2, mut id) = (f32::MAX, f32::MAX, 0.0);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (gx, gy) = (cx + dx, cy + dy);
            let (wx, wy) = (gx.rem_euclid(n), gy.rem_euclid(n));
            let px = gx as f32 + 0.15 + 0.7 * hash(wx, wy, seed);
            let py = gy as f32 + 0.15 + 0.7 * hash(wx, wy, seed.wrapping_add(31));
            let d = ((px - x) * (px - x) + (py - y) * (py - y)).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
                id = hash(wx, wy, seed.wrapping_add(77));
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2, id)
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)]
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    smooth(((x - e0) / (e1 - e0)).clamp(0.0, 1.0))
}

fn shade(c: [f32; 3], k: f32) -> [f32; 3] {
    [(c[0] * k).clamp(0.0, 1.0), (c[1] * k).clamp(0.0, 1.0), (c[2] * k).clamp(0.0, 1.0)]
}

/// The display colour and the height (0..1) of one texel of material `name` at (u, v), both 0..1 with v counted downwards.
fn texel(name: &str, u: f32, v: f32) -> Option<([f32; 3], f32)> {
    Some(match name {
        "grass" => {
            let broad = fbm(u, v, 4, 4, 11);
            let blades = vnoise(u, v, 48, 48, 12);
            let t = (broad * 0.65 + blades * 0.45).clamp(0.0, 1.0);
            let c = shade(mix3([0.10, 0.25, 0.06], [0.34, 0.56, 0.15], t), 0.9 + 0.2 * vnoise(u, v, 96, 96, 13));
            (c, 0.3 + 0.5 * blades + 0.2 * broad)
        }
        "dirt" => {
            let t = fbm(u, v, 4, 5, 21);
            let pebble = smoothstep(0.78, 0.9, vnoise(u, v, 40, 40, 22));
            (shade(mix3([0.27, 0.18, 0.10], [0.46, 0.34, 0.21], t), 1.0 - 0.35 * pebble), 0.3 + 0.4 * t + 0.5 * pebble)
        }
        "stone" => {
            // cobblestones: cells of slightly different grey with dark gaps
            let (f1, f2, id) = worley(u, v, 6, 31);
            let gap = smoothstep(0.0, 0.09, f2 - f1);
            let base = lerp(0.42, 0.66, id) + 0.1 * (fbm(u, v, 8, 3, 32) - 0.5);
            let c = [base, base * 0.98, base * 0.94];
            // the stones are domed: higher in the middle of a cell
            (shade(c, 0.35 + 0.65 * gap), gap * (0.55 + 0.45 * (1.0 - (f1 * 1.6).min(1.0))) + 0.05 * vnoise(u, v, 48, 48, 33))
        }
        "bricks" => {
            let rows = 8.0;
            let ry = v * rows;
            let row = ry.floor();
            let offset = if (row as i32) % 2 == 0 { 0.0 } else { 0.5 };
            let bx = (u * 4.0 + offset).rem_euclid(1.0);
            let by = ry - row;
            let brick_id = hash(((u * 4.0 + offset).floor() as i32).rem_euclid(4), row as i32, 41);
            let edge = (bx.min(1.0 - bx) * 2.0).min(by.min(1.0 - by) * 3.0);
            let mortar = 1.0 - smoothstep(0.03, 0.09, edge);
            let grain = fbm(u, v, 16, 3, 42);
            let brick = shade(mix3([0.55, 0.22, 0.14], [0.72, 0.34, 0.22], brick_id), 0.85 + 0.3 * grain);
            (mix3(brick, [0.62, 0.60, 0.56], mortar), (1.0 - mortar) * (0.8 + 0.2 * grain))
        }
        "wood" => {
            // horizontal planks with grain running along them
            let planks = 4.0;
            let py = v * planks;
            let plank = py.floor();
            let local = py - plank;
            let id = hash(plank as i32, 0, 51);
            let warp = fbm(u, v, 4, 3, 52);
            let ring = (vnoise(u + id, local * 0.15 + warp * 0.3, 2, 40, 53) * 0.7 + vnoise(u, v, 8, 64, 54) * 0.3).clamp(0.0, 1.0);
            let base = mix3([0.40, 0.25, 0.12], [0.66, 0.46, 0.25], ring);
            let gap = 1.0 - smoothstep(0.0, 0.04, local.min(1.0 - local));
            (mix3(shade(base, 0.9 + 0.2 * id), [0.12, 0.08, 0.05], gap), (1.0 - gap) * (0.75 + 0.25 * ring))
        }
        "sand" => {
            let t = fbm(u, v, 6, 4, 61);
            let speck = vnoise(u, v, 64, 64, 62);
            (shade(mix3([0.70, 0.60, 0.40], [0.82, 0.73, 0.52], t), 0.92 + 0.16 * speck), 0.4 + 0.4 * t + 0.2 * speck)
        }
        "concrete" => {
            let t = fbm(u, v, 3, 5, 71);
            let speck = vnoise(u, v, 80, 80, 72);
            let g = 0.46 + 0.16 * t + 0.06 * (speck - 0.5);
            ([g, g, g * 1.02], 0.3 + 0.5 * t + 0.2 * speck)
        }
        "tiles" => {
            let n = 4.0;
            let (tx, ty) = (u * n, v * n);
            let (cx, cy) = (tx.floor(), ty.floor());
            let (fx, fy) = (tx - cx, ty - cy);
            let edge = fx.min(1.0 - fx).min(fy.min(1.0 - fy));
            let grout = 1.0 - smoothstep(0.02, 0.06, edge);
            let tint = hash(cx as i32, cy as i32, 81);
            let tile = shade([0.80, 0.82, 0.85], 0.92 + 0.12 * tint);
            (mix3(tile, [0.38, 0.38, 0.40], grout), 1.0 - grout)
        }
        "metal" => {
            // brushed: noise stretched along x
            let streak = vnoise(u, v, 2, 128, 91) * 0.6 + vnoise(u, v, 4, 64, 92) * 0.4;
            let g = 0.58 + 0.14 * streak;
            ([g, g, g * 1.03], 0.4 + 0.6 * streak)
        }
        _ => return None,
    })
}

/// A material texture by name, or `None` when `name` is not one of `MATERIALS`.
pub fn generate(name: &str) -> Option<Texture> {
    if !MATERIALS.contains(&name) {
        return None;
    }
    let mut texels = Vec::with_capacity((SIZE * SIZE) as usize);
    let mut heights = Vec::with_capacity((SIZE * SIZE) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (u, v) = ((x as f32 + 0.5) / SIZE as f32, (y as f32 + 0.5) / SIZE as f32);
            let (c, h) = texel(name, u, v)?;
            // stored squared, in the renderer's working space, like every other texture
            texels.push([c[0] * c[0], c[1] * c[1], c[2] * c[2], 1.0]);
            heights.push(h.clamp(0.0, 1.0));
        }
    }
    Some(Texture { width: SIZE, height: SIZE, texels, heights: Some(heights) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_material_exists_is_opaque_and_varies() {
        for name in MATERIALS {
            let t = generate(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(t.texels.len(), (SIZE * SIZE) as usize);
            assert!(t.texels.iter().all(|p| p[3] == 1.0 && p[..3].iter().all(|c| (0.0..=1.0).contains(c))), "{name}");
            let (lo, hi) = t.texels.iter().fold((1.0f32, 0.0f32), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
            assert!(hi - lo > 0.01, "{name} is flat");
        }
        assert!(generate("circle").is_none());
    }

    #[test]
    fn materials_tile_seamlessly() {
        // the last column continues into the first one about as smoothly as neighbouring columns follow each other
        for name in MATERIALS {
            let t = generate(name).unwrap();
            let w = SIZE as usize;
            let (mut seam, mut inner) = (0.0f32, 0.0f32);
            for y in 0..w {
                seam += (t.texels[y * w + w - 1][0] - t.texels[y * w][0]).abs();
                inner += (t.texels[y * w + w / 2][0] - t.texels[y * w + w / 2 + 1][0]).abs();
            }
            assert!(seam <= inner * 4.0 + 0.5, "{name}: the seam ({seam}) is much rougher than the inside ({inner})");
        }
    }

    #[test]
    fn they_are_deterministic() {
        let a = generate("stone").unwrap();
        let b = generate("stone").unwrap();
        assert_eq!(a.texels, b.texels);
    }
}
