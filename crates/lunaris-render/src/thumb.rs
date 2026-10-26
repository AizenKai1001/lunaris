//! A small picture of a model, for the editor's Project panel: the mesh drawn from a fixed three-quarter view (from the front, a little to the right and above) with a depth buffer and one directional
//! light, in the material's colours and base texture, fitted into a square and smoothed by drawing at twice the size. It is a plain CPU rasteriser with no GPU and no scene behind it, so a worker
//! thread can make one for every model in a folder while the editor goes on drawing; a model of a million triangles takes a fraction of a second.
use crate::math::*;
use crate::mesh::Mesh;

/// RGBA, 8 bits a channel, straight (not premultiplied) alpha: opaque where the model is, clear elsewhere.
pub struct Thumb {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

/// How many samples a side make one pixel of the result.
const SS: usize = 2;
/// The empty margin round the model, in pixels of the result.
const MARGIN: f32 = 2.0;

/// The colour of a surface with no colour and no texture of its own (a pale grey, so that the light and shade of the shape show).
const PLAIN: V3 = [0.72, 0.72, 0.74];

fn display(x: f32) -> u8 {
    (x.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8
}

/// A picture of `mesh` in a square of `side` pixels, or `None` for a mesh with nothing to draw. A model that is made of several pieces by material (`Mesh::parts`) is drawn piece by piece, each
/// with its own texture; the whole mesh gives the bounds.
pub fn thumbnail(mesh: &Mesh, side: usize) -> Option<Thumb> {
    let side = side.max(8);
    if mesh.pos.is_empty() || mesh.idx.len() < 3 {
        return None;
    }
    // the view: the eye is out along `d` from the middle of the model, `right` and `up` span the picture
    let d = normalize([0.55, 0.42, 0.72]);
    let right = normalize(cross([0.0, 1.0, 0.0], d));
    let up = cross(d, right);
    let light = normalize(add(add(scale(d, 0.7), scale(up, 0.5)), scale(right, -0.5)));
    // the extent of the model in the picture plane
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in &mesh.pos {
        let q = [dot(*p, right), dot(*p, up)];
        if !(q[0].is_finite() && q[1].is_finite()) {
            continue;
        }
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    if lo[0] > hi[0] {
        return None;
    }
    let big = side * SS;
    let extent = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-6);
    let k = (side as f32 - 2.0 * MARGIN) * SS as f32 / extent;
    let mid = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
    let half = big as f32 * 0.5;
    let screen = |p: V3| -> [f32; 3] { [(dot(p, right) - mid[0]) * k + half, half - (dot(p, up) - mid[1]) * k, dot(p, d)] };

    let mut depth = vec![f32::MIN; big * big];
    let mut color = vec![[0u8; 3]; big * big];
    let mut covered = vec![false; big * big];
    let drawn: Vec<&Mesh> = if mesh.parts.is_empty() { vec![mesh] } else { mesh.parts.iter().map(|p| &**p).collect() };
    for m in drawn {
        let has_nrm = m.nrm.len() == m.pos.len();
        let has_col = m.col.len() == m.pos.len();
        let has_uv = m.uv.len() == m.pos.len();
        let has_w = m.texw.len() == m.pos.len();
        let tex = if has_uv { m.tex.as_deref() } else { None };
        let sp: Vec<[f32; 3]> = m.pos.iter().map(|&p| screen(p)).collect();
        for t in m.idx.chunks_exact(3) {
            let (i0, i1, i2) = (t[0] as usize, t[1] as usize, t[2] as usize);
            if i0 >= sp.len() || i1 >= sp.len() || i2 >= sp.len() {
                continue;
            }
            let (a, b, c) = (sp[i0], sp[i1], sp[i2]);
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if !area.is_finite() || area.abs() < 1e-9 {
                continue;
            }
            let x0 = (a[0].min(b[0]).min(c[0]).floor().max(0.0)) as usize;
            let y0 = (a[1].min(b[1]).min(c[1]).floor().max(0.0)) as usize;
            let x1 = (a[0].max(b[0]).max(c[0]).ceil().min(big as f32 - 1.0)).max(0.0) as usize;
            let y1 = (a[1].max(b[1]).max(c[1]).ceil().min(big as f32 - 1.0)).max(0.0) as usize;
            if a[0].max(b[0]).max(c[0]) < 0.0 || a[1].max(b[1]).max(c[1]) < 0.0 {
                continue;
            }
            let inv = 1.0 / area;
            // the face normal, for a mesh that came without normals
            let face = if has_nrm { [0.0; 3] } else { normalize(cross(sub(m.pos[i1], m.pos[i0]), sub(m.pos[i2], m.pos[i0]))) };
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b[0] - px) * (c[1] - py) - (c[0] - px) * (b[1] - py)) * inv;
                    let w1 = ((c[0] - px) * (a[1] - py) - (a[0] - px) * (c[1] - py)) * inv;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                        continue;
                    }
                    let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
                    let at = y * big + x;
                    if z <= depth[at] {
                        continue;
                    }
                    let mix3 = |p0: V3, p1: V3, p2: V3| -> V3 { [w0 * p0[0] + w1 * p1[0] + w2 * p2[0], w0 * p0[1] + w1 * p1[1] + w2 * p2[1], w0 * p0[2] + w1 * p1[2] + w2 * p2[2]] };
                    // the surface colour: vertex colour times the base texture (as much of it as the vertices say)
                    let mut base = if has_col { mix3(m.col[i0], m.col[i1], m.col[i2]) } else if tex.is_some() { [1.0; 3] } else { PLAIN };
                    if let Some(tx) = tex {
                        let u = w0 * m.uv[i0][0] + w1 * m.uv[i1][0] + w2 * m.uv[i2][0];
                        let v = w0 * m.uv[i0][1] + w1 * m.uv[i1][1] + w2 * m.uv[i2][1];
                        let s = tx.sample(u - u.floor(), v - v.floor());
                        if m.alpha_cutoff > 0.0 && s[3] < m.alpha_cutoff {
                            continue;
                        }
                        let w = if has_w { (w0 * m.texw[i0] + w1 * m.texw[i1] + w2 * m.texw[i2]).clamp(0.0, 1.0) } else { 1.0 };
                        for j in 0..3 {
                            base[j] *= 1.0 - w + w * s[j];
                        }
                    }
                    let mut n = if has_nrm { normalize(mix3(m.nrm[i0], m.nrm[i1], m.nrm[i2])) } else { face };
                    if dot(n, d) < 0.0 {
                        n = scale(n, -1.0);
                    }
                    let shade = (0.30 + 0.85 * dot(n, light).max(0.0)).min(1.15);
                    depth[at] = z;
                    covered[at] = true;
                    color[at] = [display(base[0] * shade), display(base[1] * shade), display(base[2] * shade)];
                }
            }
        }
    }
    // SS x SS samples make a pixel: the colour is the mean of the covered samples, the alpha the share that is covered
    let mut rgba = vec![0u8; side * side * 4];
    for y in 0..side {
        for x in 0..side {
            let (mut sum, mut n) = ([0u32; 3], 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let at = (y * SS + sy) * big + x * SS + sx;
                    if covered[at] {
                        for j in 0..3 {
                            sum[j] += color[at][j] as u32;
                        }
                        n += 1;
                    }
                }
            }
            if n > 0 {
                let o = (y * side + x) * 4;
                rgba[o] = (sum[0] / n) as u8;
                rgba[o + 1] = (sum[1] / n) as u8;
                rgba[o + 2] = (sum[2] / n) as u8;
                rgba[o + 3] = ((n * 255 + (SS * SS) as u32 / 2) / (SS * SS) as u32) as u8;
            }
        }
    }
    Some(Thumb { w: side, h: side, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::texture::{Texels, Texture};
    use std::sync::Arc;

    fn px(t: &Thumb, x: usize, y: usize) -> [u8; 4] {
        let o = (y * t.w + x) * 4;
        [t.rgba[o], t.rgba[o + 1], t.rgba[o + 2], t.rgba[o + 3]]
    }

    #[test]
    fn a_cube_is_drawn_in_the_middle_with_three_shades_and_clear_corners() {
        let cube = crate::mesh::builtin("cube").unwrap();
        let t = thumbnail(cube, 64).unwrap();
        assert_eq!((t.w, t.h, t.rgba.len()), (64, 64, 64 * 64 * 4));
        assert_eq!(px(&t, 32, 32)[3], 255, "the middle is the model");
        assert_eq!(px(&t, 0, 0)[3], 0, "the corner is empty");
        assert_eq!(px(&t, 63, 63)[3], 0);
        // three faces are seen, each lit differently
        let mut shades: Vec<[u8; 3]> = t.rgba.chunks(4).filter(|p| p[3] == 255).map(|p| [p[0], p[1], p[2]]).collect();
        shades.sort();
        shades.dedup();
        assert!(shades.len() >= 3, "{} shades", shades.len());
        // it fills the square: something opaque within 4 pixels of the top and of the bottom edge, or of the left and right edge
        let rows: Vec<usize> = (0..64).filter(|&y| (0..64).any(|x| px(&t, x, y)[3] > 0)).collect();
        let cols: Vec<usize> = (0..64).filter(|&x| (0..64).any(|y| px(&t, x, y)[3] > 0)).collect();
        let (r0, r1, c0, c1) = (rows[0], *rows.last().unwrap(), cols[0], *cols.last().unwrap());
        assert!((r1 - r0).max(c1 - c0) >= 58, "rows {r0}..{r1}, columns {c0}..{c1}");
        assert!(r0 <= 4 || c0 <= 4);
        // nothing to draw gives nothing
        assert!(thumbnail(&Mesh::new(), 64).is_none());
    }

    #[test]
    fn a_nearer_surface_hides_a_farther_one_and_the_texture_and_colour_show() {
        // two quads facing the eye (the direction (0.55, 0.42, 0.72)), the near one red from a texture, the far one green from vertex colours
        let d = normalize([0.55, 0.42, 0.72]);
        let right = normalize(cross([0.0, 1.0, 0.0], d));
        let up = cross(d, right);
        let quad = |m: &mut Mesh, at: V3, half: f32, colour: V3| {
            let base = m.pos.len() as u32;
            for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                m.pos.push(add(at, add(scale(right, a * half), scale(up, b * half))));
                m.nrm.push(d);
                m.uv.push([0.5, 0.5]);
                m.col.push(colour);
                m.texw.push(0.0);
            }
            m.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        };
        let mut m = Mesh::new();
        quad(&mut m, scale(d, -1.0), 1.0, [0.0, 1.0, 0.0]); // far, big, green
        let far_only = thumbnail(&m, 32).unwrap();
        let g = px(&far_only, 16, 16);
        assert!(g[1] > g[0] && g[1] > g[2] && g[3] == 255, "green: {g:?}");
        quad(&mut m, scale(d, 1.0), 0.4, [1.0, 1.0, 1.0]); // near, small, white colour...
        m.tex = Some(Arc::new(Texture { width: 1, height: 1, texels: Texels::Srgb8(vec![[255, 0, 0, 255]]), heights: None }));
        // ...with the texture on the near one only (texw 1 there)
        let n = m.texw.len();
        for w in &mut m.texw[n - 4..] {
            *w = 1.0;
        }
        let both = thumbnail(&m, 32).unwrap();
        let middle = px(&both, 16, 16);
        assert!(middle[0] > middle[1] * 3 && middle[0] > middle[2] * 3, "the near quad hides the far one and is red: {middle:?}");
        let edge = px(&both, 16, 5);
        assert!(edge[1] > edge[0], "the far quad shows round it: {edge:?}");
    }
}
