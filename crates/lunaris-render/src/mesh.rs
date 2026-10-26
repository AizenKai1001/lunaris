//! Built-in meshes. Counter-clockwise triangles seen from outside, unit size, centred on the origin.
use crate::math::*;
use lunaris_sim::detmath;
use crate::texture::Texture;
use std::sync::{Arc, OnceLock};

/// How a mesh gets coarser versions of itself for when it is far off (the GPU renderer's high qualities).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LodMode {
    /// Coarser levels, and the finest is never drawn when it has more triangles than the base budget (a model: a photographed tree of 800,000 triangles is drawn with 150,000 at the most, however near the camera is).
    #[default]
    Full,
    /// Coarser levels with no budget (the ground: a terrain tile has millions of triangles and is near the camera in part).
    Ground,
    /// No coarser levels: the whole mesh is drawn at every distance.
    Whole,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub pos: Vec<V3>,
    pub nrm: Vec<V3>,
    pub uv: Vec<[f32; 2]>,
    /// Per-vertex colour multipliers in linear space (empty = white): imported models carry their material colours here.
    pub col: Vec<V3>,
    /// Per-vertex strength of the texture, 0 to 1 (empty = 1 everywhere): a model can mix textured and plain materials.
    pub texw: Vec<f32>,
    pub idx: Vec<u32>,
    /// A base colour texture (imported models).
    pub tex: Option<Arc<Texture>>,
    /// The skeleton and animation clips of an imported model that has them; `pos` and `nrm` are then the rest pose.
    pub rig: Option<Arc<crate::rig::Rig>>,
    /// The normal map of an imported model's material (the GPU renderer's high qualities use it, as a `PbrMaps.normal`).
    pub normal_tex: Option<Arc<Texture>>,
    /// The metallic-roughness map of an imported material (roughness in green, metallic in blue, occlusion in red when there is one: the renderer's `arm`).
    pub arm_tex: Option<Arc<Texture>>,
    /// glTF `alphaMode` `MASK` (and `BLEND`, shown the same way): where the base colour texture's alpha is below this the surface is not there (leaves, fences, hair cards). 0 = opaque.
    pub alpha_cutoff: f32,
    /// How the GPU renderer gives the mesh coarser levels of detail (see `LodMode`).
    pub lod: LodMode,
    /// A model whose materials differ in more than a colour (two textures; a normal map on some of them; a cut-out leaf next to opaque bark) is also given as one mesh per material: the renderers draw
    /// these instead of the whole, which stays whole for the physics, the rays and the bounds. Empty for everything else.
    pub parts: Vec<Arc<Mesh>>,
    /// Solid volumes inside the model that hide what is behind them (`asset.extras.lunaris_occluders` of a glTF file): each is a prism with a four-cornered footprint, `[x0, z0, x1, z1, x2, z2, x3, z3, y_bottom, y_top]` in the
    /// model's own space. The GPU renderer's software occlusion culling draws them into a small depth picture and leaves out the objects that lie wholly behind them.
    pub occluders: Vec<[f32; 10]>,
    /// For a model drawn in pieces: one mesh of all the pieces that are not cut out (made when the model is imported), which the GPU renderer draws instead of those pieces in the passes that need only depth (see `SceneMesh::depth_role`).
    pub depth_whole: Option<Arc<Mesh>>,
    /// glTF `emissiveFactor` (its luminance) of the material of a piece of a model: with the entity's `MeshRenderer.emissive` it makes a piece glow by itself (the lit windows of a block: the entity's `emissive` says how much,
    /// and only the pieces that have a factor glow). 0 for a piece that does not.
    pub emissive_factor: f32,
}

impl Mesh {
    pub(crate) fn new() -> Mesh {
        Mesh { pos: Vec::new(), nrm: Vec::new(), uv: Vec::new(), col: Vec::new(), texw: Vec::new(), idx: Vec::new(), tex: None, rig: None, normal_tex: None, arm_tex: None, alpha_cutoff: 0.0, lod: crate::mesh::LodMode::Full, parts: Vec::new(), occluders: Vec::new(), depth_whole: None, emissive_factor: 0.0 }
    }

    /// A flat face with the given normal and in-plane axes (`u` cross `v` must equal `n`), half-extent `h`, lifted `lift` along
    /// its normal (a cube's faces sit `h` out from the centre; a plane or quad is centred on the origin, lift 0).
    fn face(&mut self, n: V3, u: V3, v: V3, h: f32, lift: f32) {
        let base = self.pos.len() as u32;
        let corners = [(-1.0, -1.0, [0.0, 0.0]), (1.0, -1.0, [1.0, 0.0]), (1.0, 1.0, [1.0, 1.0]), (-1.0, 1.0, [0.0, 1.0])];
        for (a, b, uv) in corners {
            self.pos.push(add(add(scale(n, lift), scale(u, a * h)), scale(v, b * h)));
            self.nrm.push(n);
            self.uv.push(uv);
        }
        self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn cube() -> Mesh {
    let mut m = Mesh::new();
    let (x, y, z) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    m.face(x, y, z, 0.5, 0.5);
    m.face(scale(x, -1.0), z, y, 0.5, 0.5);
    m.face(y, z, x, 0.5, 0.5);
    m.face(scale(y, -1.0), x, z, 0.5, 0.5);
    m.face(z, x, y, 0.5, 0.5);
    m.face(scale(z, -1.0), y, x, 0.5, 0.5);
    m
}

/// Unit quad in the XY plane facing +Z (what sprites use).
fn quad() -> Mesh {
    let mut m = Mesh::new();
    m.face([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.5, 0.0);
    m
}

/// Unit plane in the XZ plane facing +Y (a floor).
fn plane() -> Mesh {
    let mut m = Mesh::new();
    m.face([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 0.5, 0.0);
    m
}

fn sphere() -> Mesh {
    let (rings, sectors) = (16u32, 24u32);
    let mut m = Mesh::new();
    for i in 0..=rings {
        let theta = detmath::PI * i as f64 / rings as f64;
        let (st, ct) = (detmath::sin(theta) as f32, detmath::cos(theta) as f32);
        for j in 0..=sectors {
            let phi = 2.0 * detmath::PI * j as f64 / sectors as f64;
            let n = [st * detmath::cos(phi) as f32, ct, st * detmath::sin(phi) as f32];
            m.pos.push(scale(n, 0.5));
            m.nrm.push(n);
            m.uv.push([j as f32 / sectors as f32, i as f32 / rings as f32]);
        }
    }
    let stride = sectors + 1;
    for i in 0..rings {
        for j in 0..sectors {
            let (a, b, c, d) = (i * stride + j, i * stride + j + 1, (i + 1) * stride + j + 1, (i + 1) * stride + j);
            m.idx.extend_from_slice(&[a, b, c, a, c, d]);
        }
    }
    m
}

fn cylinder() -> Mesh {
    let seg = 24u32;
    let mut m = Mesh::new();
    // side
    for j in 0..=seg {
        let phi = 2.0 * detmath::PI * j as f64 / seg as f64;
        let (c, s) = (detmath::cos(phi) as f32, detmath::sin(phi) as f32);
        for (y, v) in [(-0.5, 0.0), (0.5, 1.0)] {
            m.pos.push([0.5 * c, y, 0.5 * s]);
            m.nrm.push([c, 0.0, s]);
            m.uv.push([j as f32 / seg as f32, v]);
        }
    }
    for j in 0..seg {
        // increasing angle runs right-to-left seen from outside, so counter-clockwise is bl, br, tr, tl = b, a, d, c
        let (a, b, c, d) = (j * 2, j * 2 + 2, j * 2 + 3, j * 2 + 1);
        m.idx.extend_from_slice(&[b, a, d, b, d, c]);
    }
    // caps
    for (y, ny) in [(0.5f32, 1.0f32), (-0.5, -1.0)] {
        let center = m.pos.len() as u32;
        m.pos.push([0.0, y, 0.0]);
        m.nrm.push([0.0, ny, 0.0]);
        m.uv.push([0.5, 0.5]);
        for j in 0..=seg {
            let phi = 2.0 * detmath::PI * j as f64 / seg as f64;
            let (c, s) = (detmath::cos(phi) as f32, detmath::sin(phi) as f32);
            m.pos.push([0.5 * c, y, 0.5 * s]);
            m.nrm.push([0.0, ny, 0.0]);
            m.uv.push([0.5 + 0.5 * c, 0.5 + 0.5 * s]);
        }
        for j in 0..seg {
            let (a, b) = (center + 1 + j, center + 2 + j);
            if ny > 0.0 {
                m.idx.extend_from_slice(&[center, b, a]);
            } else {
                m.idx.extend_from_slice(&[center, a, b]);
            }
        }
    }
    m
}

pub const BUILTIN_NAMES: [&str; 5] = ["cube", "sphere", "plane", "cylinder", "quad"];

static MESHES: OnceLock<Vec<(&'static str, Mesh)>> = OnceLock::new();

pub fn builtin(name: &str) -> Option<&'static Mesh> {
    // (the built-in shapes are small and drawn whole at every distance)
    let all = MESHES.get_or_init(|| vec![("cube", cube()), ("sphere", sphere()), ("plane", plane()), ("cylinder", cylinder()), ("quad", quad())].into_iter().map(|(n, mut m)| { m.lod = LodMode::Whole; (n, m) }).collect());
    all.iter().find(|(n, _)| *n == name).map(|(_, m)| m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_triangle_faces_outward_and_matches_its_vertex_normals() {
        for name in ["cube", "sphere", "cylinder"] {
            let m = builtin(name).unwrap();
            assert_eq!(m.idx.len() % 3, 0);
            for t in m.idx.chunks(3) {
                let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
                let n = cross(sub(b, a), sub(c, a));
                if length(n) < 1e-9 {
                    continue; // degenerate triangle at a pole
                }
                let centre = scale(add(add(a, b), c), 1.0 / 3.0);
                // for these convex shapes centred on the origin the outward direction is roughly the centroid direction
                assert!(dot(n, centre) > 0.0, "{name}: triangle faces inward ({a:?} {b:?} {c:?})");
                let vn = normalize(add(add(m.nrm[t[0] as usize], m.nrm[t[1] as usize]), m.nrm[t[2] as usize]));
                assert!(dot(normalize(n), vn) > 0.5, "{name}: face normal disagrees with vertex normals");
            }
        }
    }

    #[test]
    fn quad_and_plane_face_their_axes() {
        let q = builtin("quad").unwrap();
        let (a, b, c) = (q.pos[0], q.pos[1], q.pos[2]);
        assert!(cross(sub(b, a), sub(c, a))[2] > 0.0);
        let p = builtin("plane").unwrap();
        let (a, b, c) = (p.pos[0], p.pos[1], p.pos[2]);
        assert!(cross(sub(b, a), sub(c, a))[1] > 0.0);
    }

    #[test]
    fn names_resolve() {
        for n in BUILTIN_NAMES {
            assert!(builtin(n).is_some(), "{n}");
        }
        assert!(builtin("teapot").is_none());
    }
}
