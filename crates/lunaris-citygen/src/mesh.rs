//! The geometry kernel: vectors, a frame to build things in, and a mesh builder with one part per material.
//!
//! Faces are given as four corners and the direction they must face; the builder turns the winding the right way, so no code has to get counter-clockwise right by hand.

pub type V3 = [f32; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
pub fn norm(a: V3) -> V3 {
    let l = len(a);
    if l < 1e-12 {
        [0.0, 1.0, 0.0]
    } else {
        scale(a, 1.0 / l)
    }
}
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
pub fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// An orthonormal right-handed frame (`a` x `b` = `c`) with an origin: points are `o + a s + b h + c d`. A facade uses a = along the wall, b = up, c = out of the wall.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub o: V3,
    pub a: V3,
    pub b: V3,
    pub c: V3,
}

impl Frame {
    pub fn at(&self, s: f32, h: f32, d: f32) -> V3 {
        [self.o[0] + self.a[0] * s + self.b[0] * h + self.c[0] * d, self.o[1] + self.a[1] * s + self.b[1] * h + self.c[1] * d, self.o[2] + self.a[2] * s + self.b[2] * h + self.c[2] * d]
    }

    /// The frame of a wall that stands on the ground at `o`, faces `out` (a horizontal unit vector) and runs to the right of someone looking at it from outside.
    pub fn wall(o: V3, out: V3) -> Frame {
        let b = [0.0, 1.0, 0.0];
        let a = norm(cross(b, out));
        Frame { o, a, b, c: out }
    }

    /// The same frame moved by (s, h, d).
    pub fn shifted(&self, s: f32, h: f32, d: f32) -> Frame {
        Frame { o: self.at(s, h, d), ..*self }
    }
}

/// Everything one material is drawn with: a part of the model.
#[derive(Clone, Debug, Default)]
pub struct Part {
    pub pos: Vec<V3>,
    pub nrm: Vec<V3>,
    pub uv: Vec<[f32; 2]>,
    pub col: Vec<V3>,
    pub idx: Vec<u32>,
}

/// A mesh being built: parts by material number.
#[derive(Clone, Debug, Default)]
pub struct MeshBuilder {
    pub parts: Vec<Part>,
    /// Solid volumes of the model that hide what is behind them (`asset.extras.lunaris_occluders`): `[x0, z0, x1, z1, x2, z2, x3, z3, y_bottom, y_top]`, the footprint's four corners in order round it.
    pub occluders: Vec<[f32; 10]>,
    /// Furniture that can be searched (`interior::container_kind`): `[x, y, z, yaw_degrees, size_x, size_y, size_z, kind]`, the middle of its base, its footprint and height in its own axes.
    pub containers: Vec<[f32; 8]>,
    /// The doors inside a building, as the game hangs them (`runtime::DoorEnt`): `[hinge x, y, z, along x, z (the closed leaf, unit), out x, z (the side it swings away from, unit), width, height, how open it starts, 0 to 1]`.
    pub doors: Vec<[f32; 10]>,
}

/// How the pictures of a material are laid on a surface: `size` metres per repeat (the picture's real size) and an offset in metres, so that two walls of the same material do not show the same bricks.
#[derive(Clone, Copy, Debug)]
pub struct Tex {
    pub size: [f32; 2],
    pub off: [f32; 2],
}

impl Tex {
    pub fn uv(&self, along: f32, up: f32) -> [f32; 2] {
        [(along + self.off[0]) / self.size[0], -(up + self.off[1]) / self.size[1]]
    }
}

impl MeshBuilder {
    pub fn new() -> MeshBuilder {
        MeshBuilder::default()
    }

    fn part(&mut self, mat: usize) -> &mut Part {
        if self.parts.len() <= mat {
            self.parts.resize_with(mat + 1, Part::default);
        }
        &mut self.parts[mat]
    }

    pub fn triangles(&self) -> usize {
        self.parts.iter().map(|p| p.idx.len() / 3).sum()
    }

    /// A flat four-cornered face that faces `want` (the corners may be given in either order). Returns false when it has no area.
    pub fn quad(&mut self, mat: usize, p: [V3; 4], uv: [[f32; 2]; 4], col: [V3; 4], want: V3) -> bool {
        let mut n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        if len(n) < 1e-9 {
            n = cross(sub(p[2], p[0]), sub(p[3], p[0]));
        }
        if len(n) < 1e-9 {
            return false;
        }
        let order: [usize; 4] = if dot(n, want) < 0.0 { [0, 3, 2, 1] } else { [0, 1, 2, 3] };
        let n = norm(if dot(n, want) < 0.0 { scale(n, -1.0) } else { n });
        let part = self.part(mat);
        let base = part.pos.len() as u32;
        for &i in &order {
            part.pos.push(p[i]);
            part.nrm.push(n);
            part.uv.push(uv[i]);
            part.col.push(col[i]);
        }
        part.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        true
    }

    /// A face with one colour and uv from a texture laid in the plane of the face by the given in-plane metre coordinates of each corner.
    pub fn quad_tex(&mut self, mat: usize, p: [V3; 4], metres: [[f32; 2]; 4], tex: &Tex, col: V3, want: V3) -> bool {
        let uv = [tex.uv(metres[0][0], metres[0][1]), tex.uv(metres[1][0], metres[1][1]), tex.uv(metres[2][0], metres[2][1]), tex.uv(metres[3][0], metres[3][1])];
        self.quad(mat, p, uv, [col; 4], want)
    }

    /// A rectangle of a wall frame at depth `d` between (s0, h0) and (s1, h1), laid with the texture in wall metres; the colour at the bottom and the top may differ (dirt, shade).
    #[allow(clippy::too_many_arguments)]
    pub fn wall_rect(&mut self, mat: usize, f: &Frame, s0: f32, s1: f32, h0: f32, h1: f32, d: f32, tex: &Tex, col_bottom: V3, col_top: V3) -> bool {
        if s1 - s0 < 1e-4 || h1 - h0 < 1e-4 {
            return false;
        }
        let p = [f.at(s0, h0, d), f.at(s1, h0, d), f.at(s1, h1, d), f.at(s0, h1, d)];
        let uv = [tex.uv(s0, h0), tex.uv(s1, h0), tex.uv(s1, h1), tex.uv(s0, h1)];
        self.quad(mat, p, uv, [col_bottom, col_bottom, col_top, col_top], f.c)
    }

    /// The same as `wall_rect` but facing `want` (the inner side of a wall faces into the building).
    #[allow(clippy::too_many_arguments)]
    pub fn wall_rect_facing(&mut self, mat: usize, f: &Frame, s0: f32, s1: f32, h0: f32, h1: f32, d: f32, tex: &Tex, col_bottom: V3, col_top: V3, want: V3) -> bool {
        if s1 - s0 < 1e-4 || h1 - h0 < 1e-4 {
            return false;
        }
        let p = [f.at(s0, h0, d), f.at(s1, h0, d), f.at(s1, h1, d), f.at(s0, h1, d)];
        let uv = [tex.uv(s0, h0), tex.uv(s1, h0), tex.uv(s1, h1), tex.uv(s0, h1)];
        self.quad(mat, p, uv, [col_bottom, col_bottom, col_top, col_top], want)
    }

    /// An axis-aligned box in a frame, between (s0, h0, d0) and (s1, h1, d1), with the faces named by `faces` (bit 0 +s, 1 -s, 2 +h, 3 -h, 4 +d, 5 -d). The pictures are laid at their real size.
    #[allow(clippy::too_many_arguments)]
    pub fn cuboid(&mut self, mat: usize, f: &Frame, lo: V3, hi: V3, faces: u8, tex: &Tex, col: V3) {
        let (s0, h0, d0) = (lo[0], lo[1], lo[2]);
        let (s1, h1, d1) = (hi[0], hi[1], hi[2]);
        let at = |s: f32, h: f32, d: f32| f.at(s, h, d);
        if faces & 1 != 0 {
            self.quad_tex(mat, [at(s1, h0, d0), at(s1, h0, d1), at(s1, h1, d1), at(s1, h1, d0)], [[d0, h0], [d1, h0], [d1, h1], [d0, h1]], tex, col, f.a);
        }
        if faces & 2 != 0 {
            self.quad_tex(mat, [at(s0, h0, d0), at(s0, h0, d1), at(s0, h1, d1), at(s0, h1, d0)], [[d0, h0], [d1, h0], [d1, h1], [d0, h1]], tex, col, scale(f.a, -1.0));
        }
        if faces & 4 != 0 {
            self.quad_tex(mat, [at(s0, h1, d0), at(s1, h1, d0), at(s1, h1, d1), at(s0, h1, d1)], [[s0, d0], [s1, d0], [s1, d1], [s0, d1]], tex, col, f.b);
        }
        if faces & 8 != 0 {
            self.quad_tex(mat, [at(s0, h0, d0), at(s1, h0, d0), at(s1, h0, d1), at(s0, h0, d1)], [[s0, d0], [s1, d0], [s1, d1], [s0, d1]], tex, col, scale(f.b, -1.0));
        }
        if faces & 16 != 0 {
            self.quad_tex(mat, [at(s0, h0, d1), at(s1, h0, d1), at(s1, h1, d1), at(s0, h1, d1)], [[s0, h0], [s1, h0], [s1, h1], [s0, h1]], tex, col, f.c);
        }
        if faces & 32 != 0 {
            self.quad_tex(mat, [at(s0, h0, d0), at(s1, h0, d0), at(s1, h1, d0), at(s0, h1, d0)], [[s0, h0], [s1, h0], [s1, h1], [s0, h1]], tex, col, scale(f.c, -1.0));
        }
    }

    /// Append another builder's parts.
    pub fn append(&mut self, other: &MeshBuilder) {
        self.occluders.extend_from_slice(&other.occluders);
        self.containers.extend_from_slice(&other.containers);
        self.doors.extend_from_slice(&other.doors);
        for (m, p) in other.parts.iter().enumerate() {
            if p.idx.is_empty() {
                continue;
            }
            let dst = self.part(m);
            let base = dst.pos.len() as u32;
            dst.pos.extend_from_slice(&p.pos);
            dst.nrm.extend_from_slice(&p.nrm);
            dst.uv.extend_from_slice(&p.uv);
            dst.col.extend_from_slice(&p.col);
            dst.idx.extend(p.idx.iter().map(|i| i + base));
        }
    }

    /// Move every vertex by `d` (a model made in the world's frame, written in the frame of the place it is put at, so that far from the origin its numbers stay small).
    pub fn translate(&mut self, d: V3) {
        for o in &mut self.doors {
            o[0] += d[0];
            o[1] += d[1];
            o[2] += d[2];
        }
        for c in &mut self.containers {
            c[0] += d[0];
            c[1] += d[1];
            c[2] += d[2];
        }
        for o in &mut self.occluders {
            for k in 0..4 {
                o[2 * k] += d[0];
                o[2 * k + 1] += d[2];
            }
            o[8] += d[1];
            o[9] += d[1];
        }
        for p in &mut self.parts {
            for v in &mut p.pos {
                *v = [v[0] + d[0], v[1] + d[1], v[2] + d[2]];
            }
        }
    }

    /// Turn everything about the vertical axis and move it: a model made in a frame of its own whose x axis is `ux` and z axis `uz` in the world (a turn: `ux = (c, -s)`, `uz = (s, c)`), then put at `t`. Only the surfaces are moved: the occluders, containers and doors are not (a polar park has none).
    pub fn turn_and_move(&mut self, ux: [f32; 2], uz: [f32; 2], t: V3) {
        for p in &mut self.parts {
            for v in &mut p.pos {
                *v = [t[0] + ux[0] * v[0] + uz[0] * v[2], v[1] + t[1], t[2] + ux[1] * v[0] + uz[1] * v[2]];
            }
            for n in &mut p.nrm {
                *n = [ux[0] * n[0] + uz[0] * n[2], n[1], ux[1] * n[0] + uz[1] * n[2]];
            }
        }
    }

    /// The lowest and highest corner of everything.
    pub fn bounds(&self) -> Option<(V3, V3)> {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        let mut any = false;
        for p in &self.parts {
            for v in &p.pos {
                any = true;
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        any.then_some((lo, hi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quad_faces_the_way_it_is_told_to_whatever_the_corner_order() {
        let mut m = MeshBuilder::new();
        let p = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
        let uv = [[0.0; 2]; 4];
        assert!(m.quad(0, p, uv, [[1.0; 3]; 4], [0.0, 0.0, 1.0]));
        assert!(m.quad(0, p, uv, [[1.0; 3]; 4], [0.0, 0.0, -1.0]));
        let part = &m.parts[0];
        assert_eq!(part.nrm[0], [0.0, 0.0, 1.0]);
        assert_eq!(part.nrm[4], [0.0, 0.0, -1.0]);
        // the winding agrees with the normal: (p1 - p0) x (p2 - p0) points the same way
        for q in 0..2 {
            let i = &part.idx[q * 6..q * 6 + 3];
            let n = cross(sub(part.pos[i[1] as usize], part.pos[i[0] as usize]), sub(part.pos[i[2] as usize], part.pos[i[0] as usize]));
            assert!(dot(n, part.nrm[i[0] as usize]) > 0.0);
        }
    }

    #[test]
    fn a_wall_frame_runs_right_of_someone_facing_it() {
        // a wall facing +z (towards the viewer, who stands at +z looking at -z): to their right is +x
        let f = Frame::wall([0.0; 3], [0.0, 0.0, 1.0]);
        assert!((f.a[0] - 1.0).abs() < 1e-6, "{:?}", f.a);
        assert!((dot(cross(f.a, f.b), f.c) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_cuboid_with_all_faces_is_twelve_triangles_facing_outwards() {
        let mut m = MeshBuilder::new();
        let f = Frame::wall([0.0; 3], [0.0, 0.0, 1.0]);
        let t = Tex { size: [1.0, 1.0], off: [0.0, 0.0] };
        m.cuboid(0, &f, [0.0, 0.0, 0.0], [2.0, 3.0, 4.0], 63, &t, [1.0; 3]);
        assert_eq!(m.triangles(), 12);
        let centre = [1.0, 1.5, 2.0];
        let part = &m.parts[0];
        for q in 0..6 {
            let i = part.idx[q * 6] as usize;
            let out = sub(part.pos[i], centre);
            assert!(dot(out, part.nrm[i]) > 0.0, "face {q} faces inwards");
        }
    }
}
