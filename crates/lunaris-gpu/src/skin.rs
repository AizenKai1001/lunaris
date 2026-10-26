//! Skinning on the card. A skinned model (a character with a skeleton) used to be posed on the processor for every picture: its vertices went through their joints and the whole mesh was uploaded again, about a megabyte
//! for an armed mannequin. Now the processor only works out the joint matrices of the pose (`MeshRef::Skinned`, a few kilobytes), and a compute pass moves the vertices (`shaders/skin.wgsl`) into the entity's own vertex buffer,
//! which the main, shadow and depth passes then draw as they draw every other mesh. What a model keeps on the card for good: its rest vertices and its joints and weights (once for every model, whoever uses it).
//!
//! The box round a posed model (for the culling of the view and of the shadows) is no longer found from its vertices: a vertex lies within the model's own distance `radius[j]` of the joint `j` it is moved by
//! (in the rest pose, the farthest vertex that joint has any weight on), so the box round the balls of that size round the posed joints holds every vertex that is skinned, and the vertices that no joint moves stay where they were.
use lunaris_render::math::{M4, V3};
use lunaris_render::mesh::Mesh;
use std::collections::HashMap;
use std::sync::Arc;

/// What is known of a model for the box round a pose: where each joint is in the rest pose, how far from it the vertices it moves are, and the box round the vertices that no joint moves.
pub struct SkinBounds {
    joints: usize,
    /// Each joint's rest pose in the model space (the inverse of its `inverse_bind`): the pose's matrix for the joint times this is where the joint is, and how it is scaled, in the pose.
    bind: Vec<M4>,
    /// How far from its joint, in the joint's own space, the vertices it moves are.
    radius: Vec<f32>,
    still: Option<(V3, V3)>,
}

/// How many vertices of a model the rig moves (the rest are never skinned).
fn skinned_vertices(base: &Mesh) -> usize {
    let Some(rig) = base.rig.as_ref() else { return 0 };
    rig.bind_pos.len().min(rig.bind_nrm.len()).min(rig.vj.len()).min(rig.vw.len()).min(base.pos.len())
}

impl SkinBounds {
    pub fn new(base: &Mesh) -> Option<SkinBounds> {
        let rig = base.rig.as_ref()?;
        let nj = rig.joints.len();
        let skinned_n = skinned_vertices(base);
        let mut radius = vec![0.0f32; nj];
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for i in 0..base.pos.len() {
            let p = if i < skinned_n { rig.bind_pos[i] } else { base.pos[i] };
            let mut moved = false;
            if i < skinned_n {
                for k in 0..4 {
                    let j = rig.vj[i][k] as usize;
                    if rig.vw[i][k] > 0.0 && j < nj {
                        moved = true;
                        let local = rig.joints[j].inverse_bind.transform_point(p);
                        radius[j] = radius[j].max((local[0] * local[0] + local[1] * local[1] + local[2] * local[2]).sqrt());
                    }
                }
            }
            if !moved {
                for a in 0..3 {
                    lo[a] = lo[a].min(p[a]);
                    hi[a] = hi[a].max(p[a]);
                }
            }
        }
        let bind: Vec<M4> = rig.joints.iter().map(|j| j.inverse_bind.inverse_affine()).collect();
        Some(SkinBounds { joints: nj, bind, radius, still: (lo[0] <= hi[0]).then_some((lo, hi)) })
    }

    /// The box round the model in the pose these matrices make: its centre, the radius of the sphere round it, and half its size.
    pub fn of(&self, matrices: &[M4]) -> (V3, f32, V3) {
        let (mut lo, mut hi) = self.still.unwrap_or(([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]));
        for j in 0..self.joints.min(matrices.len()) {
            if self.radius[j] <= 0.0 {
                continue;
            }
            // (the joint in the pose: the radius is in the joint's own space, which a model made in centimetres has a hundred times too big: the scale of the joint's matrix says by how much)
            let w = matrices[j].mul(&self.bind[j]).0;
            let p = [w[12], w[13], w[14]];
            let col = |c: usize| (w[c * 4] * w[c * 4] + w[c * 4 + 1] * w[c * 4 + 1] + w[c * 4 + 2] * w[c * 4 + 2]).sqrt();
            let r = self.radius[j] * col(0).max(col(1)).max(col(2)) * 1.02 + 0.01;
            for a in 0..3 {
                lo[a] = lo[a].min(p[a] - r);
                hi[a] = hi[a].max(p[a] + r);
            }
        }
        if !(0..3).all(|a| lo[a].is_finite() && hi[a].is_finite() && lo[a] <= hi[a]) {
            return ([0.0; 3], 0.0, [0.0; 3]);
        }
        let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
        let half = [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5, (hi[2] - lo[2]) * 0.5];
        (center, (half[0] * half[0] + half[1] * half[1] + half[2] * half[2]).sqrt(), half)
    }
}

/// What a model keeps on the card for skinning, and what is known of it for the box round a pose.
pub struct SkinRest {
    rest: wgpu::Buffer,
    jw: wgpu::Buffer,
    /// The vertices of the model.
    pub count: u32,
    bounds: SkinBounds,
    /// Keeps the model's address (the key) from being used for another model while this is cached.
    _keep: std::sync::Weak<Mesh>,
    pub last_used: u64,
}

fn bytes_of_u32(v: &[u32]) -> &[u8] {
    bytemuck::cast_slice(v)
}

fn bytes_of_f32(v: &[f32]) -> &[u8] {
    bytemuck::cast_slice(v)
}

fn storage(device: &wgpu::Device, label: &str, len: usize, extra: wgpu::BufferUsages) -> wgpu::Buffer {
    let size = (len.max(4) as u64 + 3) & !3;
    device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | extra, mapped_at_creation: false })
}

impl SkinRest {
    /// Put a model's rest pose, joints and weights on the card. `None` for a model without a skeleton.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, base: &Arc<Mesh>, frame: u64) -> Option<SkinRest> {
        let rig = base.rig.as_ref()?;
        let bounds = SkinBounds::new(base)?;
        let n = base.pos.len();
        // (the rig's own copy of the rest pose is what `Rig::skin` starts from; the vertices beyond it are never moved)
        let skinned_n = skinned_vertices(base);
        let mut v: Vec<f32> = Vec::with_capacity(n * 12);
        let mut jw: Vec<u32> = Vec::with_capacity(n * 6);
        for i in 0..n {
            let (p, nr) = if i < skinned_n { (rig.bind_pos[i], rig.bind_nrm[i]) } else { (base.pos[i], base.nrm[i]) };
            v.extend_from_slice(&p);
            v.extend_from_slice(&nr);
            v.extend_from_slice(&base.uv[i]);
            v.extend_from_slice(&base.col.get(i).copied().unwrap_or([1.0, 1.0, 1.0]));
            v.push(base.texw.get(i).copied().unwrap_or(1.0));
            let (j, w) = if i < skinned_n { (rig.vj[i], rig.vw[i]) } else { ([0; 4], [0.0; 4]) };
            jw.push(j[0] as u32 | (j[1] as u32) << 16);
            jw.push(j[2] as u32 | (j[3] as u32) << 16);
            for k in 0..4 {
                jw.push(w[k].to_bits());
            }
        }
        let rest = storage(device, "skin-rest", v.len() * 4, wgpu::BufferUsages::empty());
        let jwb = storage(device, "skin-joints", jw.len() * 4, wgpu::BufferUsages::empty());
        if !v.is_empty() {
            queue.write_buffer(&rest, 0, bytes_of_f32(&v));
            queue.write_buffer(&jwb, 0, bytes_of_u32(&jw));
        }
        Some(SkinRest { rest, jw: jwb, count: n as u32, bounds, _keep: Arc::downgrade(base), last_used: frame })
    }

    /// The box round the model in the pose these matrices make: its centre, the radius of the sphere round it, and half its size.
    pub fn bounds(&self, matrices: &[M4]) -> (V3, f32, V3) {
        self.bounds.of(matrices)
    }
}

/// One model to skin in this picture.
pub struct SkinTask {
    /// The entity's key (the bind group it has).
    pub key: u64,
    /// The model's address (its `SkinRest`).
    pub rest: usize,
    /// The buffer the skinned vertices go to.
    pub out: wgpu::Buffer,
    pub mat_base: u32,
    pub joints: u32,
}

struct SkinGroup {
    bg: wgpu::BindGroup,
    rest: usize,
    /// The generation of the buffers it was made with (`SkinState::gen`).
    gen: u64,
}

/// The numbers of one dispatch (`SkinU` of `skin.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SkinU {
    count: u32,
    mat_base: u32,
    joints: u32,
    pad: u32,
}

pub struct SkinState {
    pipe: wgpu::ComputePipeline,
    pub rests: HashMap<usize, SkinRest>,
    /// The joint matrices of this picture's models, four columns a matrix, and the table of the dispatches' numbers.
    mats: Option<wgpu::Buffer>,
    params: Option<wgpu::Buffer>,
    /// Counts up whenever one of those buffers is made again: the bind groups that name them are then made again too.
    gen: u64,
    groups: HashMap<u64, SkinGroup>,
    /// What this picture has staged for the card: the matrices (16 floats each) and the models.
    pub staged: Vec<f32>,
    pub tasks: Vec<SkinTask>,
}

impl SkinState {
    pub fn new(pipe: wgpu::ComputePipeline) -> SkinState {
        SkinState { pipe, rests: HashMap::new(), mats: None, params: None, gen: 0, groups: HashMap::new(), staged: Vec::new(), tasks: Vec::new() }
    }

    /// Forget what is no longer used: the entities that have no buffers any more, the models nobody has skinned for a long time.
    pub fn retain(&mut self, live: impl Fn(u64) -> bool, frame: u64) {
        self.groups.retain(|k, _| live(*k));
        let used: std::collections::HashSet<usize> = self.groups.values().map(|g| g.rest).collect();
        self.rests.retain(|k, r| frame.saturating_sub(r.last_used) <= 256 || used.contains(k));
    }

    /// The entity's buffers are new: its bind group is out of date.
    pub fn forget(&mut self, key: u64) {
        self.groups.remove(&key);
    }

    /// Skin the models staged for this picture, into the commands `enc`. Nothing is recorded when there are none.
    pub fn encode(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, enc: &mut wgpu::CommandEncoder) {
        if self.tasks.is_empty() {
            self.staged.clear();
            return;
        }
        // the matrices and the numbers go to the card (the buffers grow by doubling)
        let mat_bytes = (self.staged.len() * 4) as u64;
        if self.mats.as_ref().is_none_or(|b| b.size() < mat_bytes) {
            self.mats = Some(storage(device, "skin-matrices", (mat_bytes.next_power_of_two().max(4096)) as usize, wgpu::BufferUsages::empty()));
            self.gen += 1;
        }
        let stride = (device.limits().min_uniform_buffer_offset_alignment as usize).max(std::mem::size_of::<SkinU>());
        let table = self.tasks.len() * stride;
        if self.params.as_ref().is_none_or(|b| (b.size() as usize) < table) {
            let size = (table.next_power_of_two().max(4096)) as u64;
            self.params = Some(device.create_buffer(&wgpu::BufferDescriptor { label: Some("skin-numbers"), size, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }));
            self.gen += 1;
        }
        let (mats, params) = (self.mats.as_ref().expect("made"), self.params.as_ref().expect("made"));
        queue.write_buffer(mats, 0, bytes_of_f32(&self.staged));
        let mut numbers = vec![0u8; table];
        for (i, t) in self.tasks.iter().enumerate() {
            let Some(r) = self.rests.get(&t.rest) else { continue };
            let u = SkinU { count: r.count, mat_base: t.mat_base, joints: t.joints, pad: 0 };
            numbers[i * stride..i * stride + std::mem::size_of::<SkinU>()].copy_from_slice(bytemuck::bytes_of(&u));
        }
        queue.write_buffer(params, 0, &numbers);
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("skin"), timestamp_writes: None });
        pass.set_pipeline(&self.pipe);
        for (i, t) in self.tasks.iter().enumerate() {
            let Some(r) = self.rests.get(&t.rest) else { continue };
            let fresh = self.groups.get(&t.key).is_some_and(|g| g.gen == self.gen && g.rest == t.rest);
            if !fresh {
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("skin"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: r.rest.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: r.jw.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: mats.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: t.out.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: params, offset: 0, size: std::num::NonZeroU64::new(std::mem::size_of::<SkinU>() as u64) }) },
                    ],
                });
                self.groups.insert(t.key, SkinGroup { bg, rest: t.rest, gen: self.gen });
            }
            let g = &self.groups[&t.key];
            pass.set_bind_group(0, &g.bg, &[(i * stride) as u32]);
            let groups = r.count.div_ceil(64);
            // (a card allows 65535 groups in a direction: a model of more than four million vertices goes in rows)
            pass.dispatch_workgroups(groups.min(65535), groups.div_ceil(65535).max(1), 1);
        }
        drop(pass);
        self.tasks.clear();
        self.staged.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_render::TextureCache;

    /// Every vertex of the model, posed on the processor at several moments of every clip, lies inside the box `SkinBounds` gives for the pose's matrices; and the box is not far bigger than the vertices' own.
    fn check_bounds(root: &std::path::Path, name: &str) {
        let mut cache = TextureCache::new(Some(root.to_path_buf()));
        let base = cache.model(name).unwrap_or_else(|| panic!("{name} loads"));
        let rig = base.rig.as_ref().expect("a model with a skeleton");
        let bounds = SkinBounds::new(&base).expect("bounds");
        let (mut worst_ratio, mut checked) = (0.0f32, 0);
        for (ci, clip) in rig.clips.iter().enumerate() {
            for step in 0..=8 {
                let matrices = rig.skin_matrices(Some(ci), clip.duration * step as f32 / 8.0, true);
                let posed = rig.skin(&base, &matrices);
                let (center, radius, half) = bounds.of(&matrices);
                let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
                for p in &posed.pos {
                    for a in 0..3 {
                        assert!((p[a] - center[a]).abs() <= half[a] + 1e-4, "{name}: clip {} step {step}: a vertex at {:?} is outside the box (centre {:?}, half {:?})", clip.name, p, center, half);
                        lo[a] = lo[a].min(p[a]);
                        hi[a] = hi[a].max(p[a]);
                    }
                    let d = ((p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2) + (p[2] - center[2]).powi(2)).sqrt();
                    assert!(d <= radius + 1e-4, "{name}: a vertex is outside the sphere");
                }
                let size = (0..3).map(|a| hi[a] - lo[a]).fold(0.0f32, f32::max).max(1e-3);
                let ratio = (0..3).map(|a| half[a] * 2.0).fold(0.0f32, f32::max) / size;
                if ratio > worst_ratio {
                    println!("{name}: clip {} step {step}: box {:.3} m, vertices {:.3} m (centre {:?}, vertices from {:?} to {:?}, still {:?})", clip.name, half[0] * 2.0, size, center, lo, hi, bounds.still);
                }
                worst_ratio = worst_ratio.max(ratio);
                checked += 1;
            }
        }
        println!("{name}: {} poses, the box is at most {worst_ratio:.2} times the longest side of the vertices'", checked);
        assert!(worst_ratio < 3.0, "{name}: the box is too loose ({worst_ratio:.2})");
    }

    #[test]
    fn the_box_round_a_pose_holds_every_skinned_vertex() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        check_bounds(&root, "examples/models/swayer.glb");
        // (the game's armed mannequin, when it is at hand: PUPPET_ROOT is the game folder)
        if let Some(game) = std::env::var_os("PUPPET_ROOT").map(std::path::PathBuf::from) {
            for name in ["assets/models/player_rifle.glb", "assets/models/player_pistol.glb"] {
                if game.join(name).exists() {
                    check_bounds(&game, name);
                }
            }
        }
    }
}
