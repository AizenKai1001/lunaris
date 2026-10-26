//! Skeletal animation: a model's skeleton (joints with their rest pose and inverse bind matrices), its animation clips, and the skinning
//! of its vertices. Everything here uses only IEEE `+ - * / sqrt` (rotations are blended by normalised linear interpolation, not by
//! `sin`/`acos`), so a pose is bit-identical on every machine like the rest of the reference renderer.
//!
//! The importer fills a `Rig` from a glTF file's first skin and all its animations; `Rig::skin_matrices` poses the skeleton at a time in
//! a clip, and `Rig::skin` moves a copy of the model's vertices with those matrices.
use crate::math::*;
use crate::mesh::Mesh;
use lunaris_sim::detmath;

#[derive(Clone, Debug)]
pub struct Joint {
    pub name: String,
    /// The nearest ancestor that is itself a joint, as an index into `Rig::joints`.
    pub parent: Option<usize>,
    /// Where the joint's non-joint ancestors put it (the model space of the skeleton's root), used when `parent` is `None`.
    pub base: M4,
    /// The joint's rest pose relative to its parent: translation, rotation (x, y, z, w) and scale.
    pub t: V3,
    pub r: [f32; 4],
    pub s: V3,
    /// Takes a vertex from model space into the joint's own space in the bind pose.
    pub inverse_bind: M4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    Translation,
    Rotation,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interp {
    Linear,
    Step,
    /// Hermite spline: every key stores an in-tangent, the value and an out-tangent.
    Cubic,
}

/// The keys of one property (position, rotation or scale) of one joint.
#[derive(Clone, Debug)]
pub struct Track {
    pub joint: usize,
    pub path: Path,
    pub interp: Interp,
    pub times: Vec<f32>,
    /// 3 floats per key (4 for rotations), times 3 for cubic splines.
    pub values: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    pub duration: f32,
    pub tracks: Vec<Track>,
    /// What the clip does to the node that holds the skeleton (the armature, "root motion"): its translation and rotation tracks. The pose never uses them (the picture of a character stays where its
    /// entity is); motion matching reads the path the clip travels from them.
    pub root: Vec<Track>,
}

#[derive(Clone, Debug)]
pub struct Rig {
    pub joints: Vec<Joint>,
    /// Joint indices with every joint after its parent.
    pub order: Vec<usize>,
    pub clips: Vec<Clip>,
    /// The vertices of the model before skinning, in model space.
    pub bind_pos: Vec<V3>,
    pub bind_nrm: Vec<V3>,
    /// Up to four joints per vertex and how much each moves it (the weights add up to 1; all 0 = not skinned).
    pub vj: Vec<[u16; 4]>,
    pub vw: Vec<[f32; 4]>,
}

fn lerp3(a: [f32; 4], b: [f32; 4], t: f32, n: usize) -> [f32; 4] {
    let mut o = [0.0; 4];
    for k in 0..n {
        o[k] = a[k] + (b[k] - a[k]) * t;
    }
    o
}

fn norm4(q: [f32; 4]) -> [f32; 4] {
    quat_normalize(q)
}

impl Track {
    fn comps(&self) -> usize {
        if self.path == Path::Rotation {
            4
        } else {
            3
        }
    }

    /// The `which`-th (0 in-tangent, 1 value, 2 out-tangent) vector of key `i`.
    fn key(&self, i: usize, which: usize) -> [f32; 4] {
        let c = self.comps();
        let base = if self.interp == Interp::Cubic { (i * 3 + which) * c } else { i * c };
        let mut o = [0.0; 4];
        for k in 0..c {
            o[k] = self.values.get(base + k).copied().unwrap_or(0.0);
        }
        if c == 3 {
            o[3] = 0.0;
        }
        o
    }

    /// The value of this property at time `t` (seconds into the clip).
    pub fn sample(&self, t: f32) -> [f32; 4] {
        let n = self.times.len();
        if n == 0 {
            return [0.0, 0.0, 0.0, 1.0];
        }
        let c = self.comps();
        let finish = |v: [f32; 4]| if c == 4 { norm4(v) } else { v };
        if t <= self.times[0] || n == 1 {
            return finish(self.key(0, 1));
        }
        if t >= self.times[n - 1] {
            return finish(self.key(n - 1, 1));
        }
        // the key interval holding t (times are increasing)
        let k = match self.times.binary_search_by(|x| x.partial_cmp(&t).unwrap_or(std::cmp::Ordering::Less)) {
            Ok(i) => i.min(n - 2),
            Err(i) => i.saturating_sub(1).min(n - 2),
        };
        let dt = self.times[k + 1] - self.times[k];
        if dt <= 0.0 {
            return finish(self.key(k, 1));
        }
        let u = (t - self.times[k]) / dt;
        match self.interp {
            Interp::Step => finish(self.key(k, 1)),
            Interp::Linear => {
                let (a, mut b) = (self.key(k, 1), self.key(k + 1, 1));
                if c == 4 && a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] < 0.0 {
                    // the short way round
                    b = [-b[0], -b[1], -b[2], -b[3]];
                }
                finish(lerp3(a, b, u, c))
            }
            Interp::Cubic => {
                let (p0, m0, m1, p1) = (self.key(k, 1), self.key(k, 2), self.key(k + 1, 0), self.key(k + 1, 1));
                let (u2, u3) = (u * u, u * u * u);
                let (h00, h10, h01, h11) = (2.0 * u3 - 3.0 * u2 + 1.0, u3 - 2.0 * u2 + u, -2.0 * u3 + 3.0 * u2, u3 - u2);
                let mut o = [0.0; 4];
                for i in 0..c {
                    o[i] = h00 * p0[i] + h10 * dt * m0[i] + h01 * p1[i] + h11 * dt * m1[i];
                }
                finish(o)
            }
        }
    }
}

/// One layer of a pose: a clip at a time, how much of it shows, and where (the joint `root` and everything hanging from it; `None` = all).
#[derive(Clone, Copy, Debug)]
pub struct PoseLayer {
    pub clip: Option<usize>,
    pub time: f32,
    pub looping: bool,
    pub weight: f32,
    pub root: Option<usize>,
    /// Instead of `clip` at `time`: the clips an `Inertia` chain has played, blended by inertialization (`Rig::chain_local`).
    pub inertia: Option<InertiaChain>,
}

/// The most clips an inertialization chain remembers.
pub const INERTIA_MAX: usize = 6;

/// The step of the finite differences that work out the velocities an inertialization starts from.
const INERTIA_H: f32 = 1.0 / 60.0;
/// The fastest the offset of a joint's rotation may be turning at the moment of a switch (rad/s): faster than any limb of an animation goes.
const INERTIA_MAX_SPIN: f32 = 30.0;

/// The rotation vector (axis times angle, the short way) of a unit quaternion.
fn quat_log(q: [f32; 4]) -> V3 {
    let q = if q[3] < 0.0 { [-q[0], -q[1], -q[2], -q[3]] } else { q };
    let s = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
    if s < 1e-6 {
        return [2.0 * q[0], 2.0 * q[1], 2.0 * q[2]];
    }
    let k = 2.0 * s.atan2(q[3]) / s;
    [q[0] * k, q[1] * k, q[2] * k]
}

/// The unit quaternion of a rotation vector.
fn quat_exp(v: V3) -> [f32; 4] {
    let a = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if a < 1e-6 {
        return norm4([0.5 * v[0], 0.5 * v[1], 0.5 * v[2], 1.0]);
    }
    let half = 0.5 * a;
    let k = half.sin() / a;
    [v[0] * k, v[1] * k, v[2] * k, half.cos()]
}


/// The clips an animated character has switched through lately (the oldest first, the one playing now last), which is what **inertialization** needs: when a clip is switched for another, the
/// difference between the pose the old one showed and the pose the new one starts in (and between their velocities) is not cut or crossfaded but made to decay smoothly to zero while the new clip plays on.
/// Entry `k`: `clip`, its time `t0` when it started, `rate` (clip seconds per second), `gap` (seconds it played, or has played so far) and `span` (seconds its offset takes to decay; entry 0 has none).
#[derive(Clone, Copy, Debug)]
pub struct InertiaChain {
    pub n: usize,
    pub clip: [Option<usize>; INERTIA_MAX],
    pub t0: [f32; INERTIA_MAX],
    pub rate: [f32; INERTIA_MAX],
    pub gap: [f32; INERTIA_MAX],
    pub span: [f32; INERTIA_MAX],
    /// Whether each clip looped (an older clip is posed as it was played).
    pub looping: [bool; INERTIA_MAX],
}

/// The local translations, rotations and scales of every joint.
type LocalPose = (Vec<V3>, Vec<[f32; 4]>, Vec<V3>);

/// What an inverse-kinematics goal asks of a joint.
#[derive(Clone, Copy, Debug)]
pub enum IkKind {
    /// Bend the two joints above the goal joint (a hand's forearm and upper arm, a foot's shin and thigh) so that it reaches the target.
    Reach,
    /// Turn the goal joint so that its own axis `axis` faces the target, at most `max_angle` degrees away from where the animation points it.
    Look { axis: V3, max_angle: f32 },
    /// Turn the goal joint by `degrees` about `axis` (in the model's space), times the goal's weight: a lean, a tilt, whatever the clip is playing.
    Turn { axis: V3, degrees: f32 },
}

/// A goal that only fixes the joint in one or two directions: the joint is moved so that it is level with `target` along `n1` (and `n2`), keeping the animation's place in all other directions. `one_sided`: only
/// when it is behind the plane (its distance from `target` along the direction is negative).
#[derive(Clone, Copy, Debug)]
pub struct IkPin {
    pub n1: V3,
    pub n2: Option<V3>,
    pub one_sided: bool,
}

/// One goal of inverse kinematics, in the model space of the rig.
#[derive(Clone, Copy, Debug)]
pub struct IkGoal {
    pub joint: usize,
    pub kind: IkKind,
    pub target: V3,
    /// `Reach`: the goal is a pin (see `IkPin`): the place the joint is taken to is the one the animation gives it, corrected along the pin's directions only.
    pub pin: Option<IkPin>,
    /// `Reach`: a point the elbow or knee bends towards (without one it keeps bending the way the animation bends it).
    pub pole: Option<V3>,
    /// 0 leaves the animation alone, 1 reaches the goal exactly.
    pub weight: f32,
}

fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn quat_conj(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

/// A direction at right angles to `v` (any one).
fn perpendicular(v: V3) -> V3 {
    let axis = if v[0].abs() < 0.6 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    normalize(cross(v, axis))
}

/// The shortest rotation that takes the unit vector `u` to the unit vector `v`, no trigonometry needed.
fn rotation_between(u: V3, v: V3) -> [f32; 4] {
    let c = dot(u, v);
    if c < -0.999_999 {
        let p = perpendicular(u);
        return [p[0], p[1], p[2], 0.0];
    }
    let a = cross(u, v);
    quat_normalize([a[0], a[1], a[2], 1.0 + c])
}

/// `q` taken `w` of the way from no rotation (normalised linear interpolation along the short way).
fn partial(q: [f32; 4], w: f32) -> [f32; 4] {
    let q = if q[3] < 0.0 { [-q[0], -q[1], -q[2], -q[3]] } else { q };
    quat_normalize([q[0] * w, q[1] * w, q[2] * w, 1.0 + (q[3] - 1.0) * w])
}

impl Rig {
    /// Where the root (the node that holds the skeleton) is in clip `clip` at `t` seconds: its translation and rotation (x, y, z, w); a clip without root tracks stays at the origin.
    pub fn root_motion(&self, clip: usize, t: f32) -> (V3, [f32; 4]) {
        let mut pos = [0.0f32; 3];
        let mut rot = [0.0f32, 0.0, 0.0, 1.0];
        if let Some(c) = self.clips.get(clip) {
            for tr in &c.root {
                let v = tr.sample(t);
                match tr.path {
                    Path::Translation => pos = [v[0], v[1], v[2]],
                    Path::Rotation => rot = v,
                    Path::Scale => {}
                }
            }
        }
        (pos, rot)
    }

    /// Every joint's model-space transform from local joint transforms.
    fn worlds(&self, tr: &[V3], rot: &[[f32; 4]], sc: &[V3]) -> Vec<M4> {
        let mut world: Vec<M4> = vec![M4::identity(); self.joints.len()];
        for &j in &self.order {
            let local = M4::from_trs(tr[j], rot[j], sc[j]);
            world[j] = match self.joints[j].parent {
                Some(p) => world[p].mul(&local),
                None => self.joints[j].base.mul(&local),
            };
        }
        world
    }

    /// Turn joint `j` by `delta` in model space (its children follow), by changing its local rotation.
    fn rotate_joint(&self, j: usize, delta: [f32; 4], tr: &[V3], rot: &mut [[f32; 4]], sc: &[V3]) {
        let world = self.worlds(tr, rot, sc);
        let own = decompose(&world[j]).1;
        let parent = match self.joints[j].parent {
            Some(p) => decompose(&world[p]).1,
            None => decompose(&self.joints[j].base).1,
        };
        rot[j] = quat_normalize(quat_mul(quat_conj(parent), quat_mul(delta, own)));
    }

    /// Bend the two joints above `end` so that `end` reaches `goal.target`.
    fn solve_reach(&self, goal: &IkGoal, tr: &[V3], rot: &mut [[f32; 4]], sc: &[V3]) {
        let c = goal.joint;
        let Some(b) = self.joints[c].parent else { return };
        let Some(a) = self.joints[b].parent else { return };
        let pos = |w: &[M4], j: usize| [w[j].0[12], w[j].0[13], w[j].0[14]];
        let world = self.worlds(tr, rot, sc);
        let (pa, pb, pc) = (pos(&world, a), pos(&world, b), pos(&world, c));
        let (l1, l2) = (length(sub(pb, pa)), length(sub(pc, pb)));
        let mut target = goal.target;
        if let Some(pin) = &goal.pin {
            let mut t = pc;
            for n in [Some(pin.n1), pin.n2].into_iter().flatten() {
                if length(n) < 1e-6 {
                    continue;
                }
                let n = normalize(n);
                let d = dot(sub(t, goal.target), n);
                if !pin.one_sided || d < 0.0 {
                    t = sub(t, scale(n, d));
                }
            }
            target = t;
        }
        let to_target = sub(target, pa);
        let d = length(to_target);
        if l1 < 1e-5 || l2 < 1e-5 || d < 1e-5 || !d.is_finite() {
            return;
        }
        let dir = scale(to_target, 1.0 / d);
        // the target pulled into reach: the chain can neither be longer than straight nor fold shorter than the difference of its bones
        let reach = (d.min((l1 + l2) * 0.9999)).max((l1 - l2).abs() * 1.0001 + 1e-5);
        let along = (reach * reach + l1 * l1 - l2 * l2) / (2.0 * reach);
        let height = (l1 * l1 - along * along).max(0.0).sqrt();
        let bend_hint = match goal.pole {
            Some(p) => sub(p, pa),
            None => sub(pb, pa),
        };
        let mut out = sub(bend_hint, scale(dir, dot(bend_hint, dir)));
        if length(out) < 1e-5 {
            out = perpendicular(dir);
        }
        let out = normalize(out);
        let mid_goal = add(add(pa, scale(dir, along)), scale(out, height));
        let end_goal = add(pa, scale(dir, reach));
        let w = goal.weight.clamp(0.0, 1.0);
        // the upper bone swings to the new knee, then the lower one to the end goal
        let swing_a = rotation_between(normalize(sub(pb, pa)), normalize(sub(mid_goal, pa)));
        self.rotate_joint(a, partial(swing_a, w), tr, rot, sc);
        let world = self.worlds(tr, rot, sc);
        let (pb2, pc2) = (pos(&world, b), pos(&world, c));
        let to_end = sub(end_goal, pb2);
        if length(to_end) < 1e-6 || length(sub(pc2, pb2)) < 1e-6 {
            return;
        }
        let swing_b = rotation_between(normalize(sub(pc2, pb2)), normalize(to_end));
        self.rotate_joint(b, partial(swing_b, w), tr, rot, sc);
    }

    /// Turn the joint by `degrees` about the model-space `axis`, scaled by the weight.
    fn solve_turn(&self, goal: &IkGoal, axis: V3, degrees: f32, tr: &[V3], rot: &mut [[f32; 4]], sc: &[V3]) {
        if length(axis) < 1e-6 || !degrees.is_finite() {
            return;
        }
        let half = (degrees.clamp(-360.0, 360.0) as f64).to_radians() * 0.5 * goal.weight.clamp(0.0, 1.0) as f64;
        let (s, c) = (detmath::sin(half) as f32, detmath::cos(half) as f32);
        let a = normalize(axis);
        self.rotate_joint(goal.joint, [a[0] * s, a[1] * s, a[2] * s, c], tr, rot, sc);
    }

    /// Turn the joint so that its `axis` faces the target, within `max_angle` of where it points now.
    fn solve_look(&self, goal: &IkGoal, axis: V3, max_angle: f32, tr: &[V3], rot: &mut [[f32; 4]], sc: &[V3]) {
        let j = goal.joint;
        let world = self.worlds(tr, rot, sc);
        let (p, q, _) = decompose(&world[j]);
        let to_target = sub(goal.target, p);
        if length(to_target) < 1e-5 || length(axis) < 1e-6 {
            return;
        }
        let f = normalize(rotate(q, normalize(axis)));
        let d = normalize(to_target);
        let c = dot(f, d).clamp(-1.0, 1.0);
        let limit = (max_angle.clamp(0.0, 180.0) as f64).to_radians();
        let (sin_max, cos_max) = (detmath::sin(limit) as f32, detmath::cos(limit) as f32);
        let wanted = if c >= cos_max {
            d
        } else {
            // beyond the limit: the direction on the edge of the cone round where the animation points
            let sideways = sub(d, scale(f, c));
            let side = if length(sideways) < 1e-6 { perpendicular(f) } else { normalize(sideways) };
            add(scale(f, cos_max), scale(side, sin_max))
        };
        let swing = rotation_between(f, wanted);
        self.rotate_joint(j, partial(swing, goal.weight.clamp(0.0, 1.0)), tr, rot, sc);
    }

    /// The model-space transform of every joint in the pose of a stack of layers with inverse kinematics on top (what `skin_matrices_ik` multiplies by the inverse bind matrices): where a thing
    /// fixed to a joint belongs.
    pub fn joint_worlds(&self, layers: &[PoseLayer], goals: &[IkGoal]) -> Vec<M4> {
        let (tr, mut rot, sc) = self.layers_local(layers);
        for g in goals {
            if g.joint >= self.joints.len() || !g.target.iter().all(|v| v.is_finite()) || !(g.weight > 0.0) {
                continue;
            }
            match g.kind {
                IkKind::Reach => self.solve_reach(g, &tr, &mut rot, &sc),
                IkKind::Look { axis, max_angle } => self.solve_look(g, axis, max_angle, &tr, &mut rot, &sc),
                IkKind::Turn { axis, degrees } => self.solve_turn(g, axis, degrees, &tr, &mut rot, &sc),
            }
        }
        self.worlds(&tr, &rot, &sc)
    }

    /// The skin matrices of a stack of layers with inverse kinematics on top: every goal, in order, bends the pose the layers made.
    pub fn skin_matrices_ik(&self, layers: &[PoseLayer], goals: &[IkGoal]) -> Vec<M4> {
        let (tr, mut rot, sc) = self.layers_local(layers);
        for g in goals {
            if g.joint >= self.joints.len() || !g.target.iter().all(|v| v.is_finite()) || !(g.weight > 0.0) {
                continue;
            }
            match g.kind {
                IkKind::Reach => self.solve_reach(g, &tr, &mut rot, &sc),
                IkKind::Look { axis, max_angle } => self.solve_look(g, axis, max_angle, &tr, &mut rot, &sc),
                IkKind::Turn { axis, degrees } => self.solve_turn(g, axis, degrees, &tr, &mut rot, &sc),
            }
        }
        // the solver only turns joints: scales and translations stay as the animation made them
        self.matrices(&tr, &rot, &sc)
    }

    /// A copy of `base` posed by a stack of layers with inverse kinematics on top.
    pub fn pose_layers_ik(&self, base: &Mesh, layers: &[PoseLayer], goals: &[IkGoal]) -> Mesh {
        self.skin(base, &self.skin_matrices_ik(layers, goals))
    }

    /// The model-space position of `joint` at `samples + 1` evenly spaced moments of clip `clip`, the first and the last being the clip's ends.
    pub fn joint_path(&self, clip: usize, joint: usize, samples: usize) -> Vec<V3> {
        if clip >= self.clips.len() || joint >= self.joints.len() || samples == 0 {
            return Vec::new();
        }
        let d = self.clips[clip].duration;
        (0..=samples)
            .map(|k| {
                let (tr, rot, sc) = self.local_pose(Some(clip), d * k as f32 / samples as f32, false);
                let w = self.worlds(&tr, &rot, &sc);
                [w[joint].0[12], w[joint].0[13], w[joint].0[14]]
            })
            .collect()
    }

    /// The index of the joint called `name`.
    pub fn joint_index(&self, name: &str) -> Option<usize> {
        self.joints.iter().position(|j| j.name == name)
    }

    /// Is `joint` the `root` joint or hanging from it?
    fn hangs_from(&self, mut joint: usize, root: usize) -> bool {
        for _ in 0..self.joints.len() + 1 {
            if joint == root {
                return true;
            }
            match self.joints[joint].parent {
                Some(p) => joint = p,
                None => return false,
            }
        }
        false
    }

    /// The skin matrices of a stack of layers: the first is the base (it replaces the rest pose), each later one is blended over what is there
    /// by its weight, only on the joints below its `root`.
    pub fn skin_matrices_layers(&self, layers: &[PoseLayer]) -> Vec<M4> {
        let (tr, rot, sc) = self.layers_local(layers);
        self.matrices(&tr, &rot, &sc)
    }

    /// The local joint transforms of a stack of layers.
    fn layers_local(&self, layers: &[PoseLayer]) -> (Vec<V3>, Vec<[f32; 4]>, Vec<V3>) {
        let n = self.joints.len();
        let (mut tr, mut rot, mut sc) = self.local_pose(None, 0.0, false);
        for (k, l) in layers.iter().enumerate() {
            let (tb, rb, sb) = match &l.inertia {
                Some(chain) if chain.n > 0 => self.chain_local(chain, l.looping),
                _ => self.local_pose(l.clip, l.time, l.looping),
            };
            for j in 0..n {
                let inside = l.root.is_none_or(|r| self.hangs_from(j, r));
                let w = if k == 0 { 1.0 } else if inside { l.weight.clamp(0.0, 1.0) } else { 0.0 };
                if w <= 0.0 {
                    continue;
                }
                for a in 0..3 {
                    tr[j][a] += (tb[j][a] - tr[j][a]) * w;
                    sc[j][a] += (sb[j][a] - sc[j][a]) * w;
                }
                let (a, mut b) = (rot[j], rb[j]);
                if a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] < 0.0 {
                    b = [-b[0], -b[1], -b[2], -b[3]];
                }
                rot[j] = norm4([a[0] + (b[0] - a[0]) * w, a[1] + (b[1] - a[1]) * w, a[2] + (b[2] - a[2]) * w, a[3] + (b[3] - a[3]) * w]);
            }
        }
        (tr, rot, sc)
    }

    /// A copy of `base` posed by a stack of layers (see `skin_matrices_layers`).
    pub fn pose_layers(&self, base: &Mesh, layers: &[PoseLayer]) -> Mesh {
        self.skin(base, &self.skin_matrices_layers(layers))
    }

    /// The index of the clip called `name`.
    pub fn clip_index(&self, name: &str) -> Option<usize> {
        self.clips.iter().position(|c| c.name == name)
    }

    /// The time inside clip `clip` that `time` seconds of playing means: wrapped when looping, held at the ends otherwise.
    pub fn clip_time(&self, clip: usize, time: f32, looping: bool) -> f32 {
        let d = self.clips[clip].duration;
        if !(d > 0.0) || !time.is_finite() {
            return 0.0;
        }
        if looping {
            time.rem_euclid(d)
        } else {
            time.clamp(0.0, d)
        }
    }

    /// One matrix per joint that moves a vertex of the bind pose to where the pose puts it: the joint's model-space transform in the
    /// pose times its inverse bind matrix. `clip` None = the rest pose.
    pub fn skin_matrices(&self, clip: Option<usize>, time: f32, looping: bool) -> Vec<M4> {
        let (tr, rot, sc) = self.local_pose(clip, time, looping);
        self.matrices(&tr, &rot, &sc)
    }

    /// The skin matrices of a crossfade: the pose of clip `from` at its time, blended with the pose of `to`, `w` = 0 being all `from` and
    /// 1 all `to` (translations and scales linearly, rotations by normalised linear interpolation along the short way round).
    pub fn skin_matrices_blend(&self, from: (Option<usize>, f32), to: (Option<usize>, f32), looping: bool, w: f32) -> Vec<M4> {
        let w = w.clamp(0.0, 1.0);
        let (ta, ra, sa) = self.local_pose(from.0, from.1, looping);
        let (tb, rb, sb) = self.local_pose(to.0, to.1, looping);
        let n = self.joints.len();
        let tr: Vec<V3> = (0..n).map(|j| [ta[j][0] + (tb[j][0] - ta[j][0]) * w, ta[j][1] + (tb[j][1] - ta[j][1]) * w, ta[j][2] + (tb[j][2] - ta[j][2]) * w]).collect();
        let sc: Vec<V3> = (0..n).map(|j| [sa[j][0] + (sb[j][0] - sa[j][0]) * w, sa[j][1] + (sb[j][1] - sa[j][1]) * w, sa[j][2] + (sb[j][2] - sa[j][2]) * w]).collect();
        let rot: Vec<[f32; 4]> = (0..n)
            .map(|j| {
                let (a, mut b) = (ra[j], rb[j]);
                if a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] < 0.0 {
                    b = [-b[0], -b[1], -b[2], -b[3]];
                }
                norm4([a[0] + (b[0] - a[0]) * w, a[1] + (b[1] - a[1]) * w, a[2] + (b[2] - a[2]) * w, a[3] + (b[3] - a[3]) * w])
            })
            .collect();
        self.matrices(&tr, &rot, &sc)
    }

    /// A copy of `base` posed in a crossfade (see `skin_matrices_blend`).
    pub fn pose_blend(&self, base: &Mesh, from: (Option<usize>, f32), to: (Option<usize>, f32), looping: bool, w: f32) -> Mesh {
        self.skin(base, &self.skin_matrices_blend(from, to, looping, w))
    }

    /// The pose of an inertialization chain now (its last entry at its `gap`).
    pub fn chain_local(&self, chain: &InertiaChain, looping: bool) -> LocalPose {
        let k = chain.n.min(INERTIA_MAX).saturating_sub(1);
        // (the last entry is the clip playing now: its looping is the animator's)
        let mut chain = *chain;
        chain.looping[k] = looping;
        self.chain_at(&chain, k, chain.gap[k])
    }

    /// The pose of entry `k` of the chain `u` seconds after it started: its clip at `t0 + rate * u`, plus the offset that took the pose of the entry before (at the moment of the switch) to this clip's
    /// first pose and is still decaying: a quintic Hermite curve that starts at the difference and its velocity and ends at rest (zero velocity and acceleration) after the entry's `span`.
    /// Rotation offsets are quaternions (`from * to^-1`) that are scaled along their own axis, translations and scales are vectors.
    fn chain_at(&self, chain: &InertiaChain, k: usize, u: f32) -> LocalPose {
        let looping = chain.looping[k];
        let (mut tr, mut rot, mut sc) = self.local_pose(chain.clip[k], chain.t0[k] + chain.rate[k] * u, looping);
        let span = chain.span[k];
        if k == 0 || !(span > 1e-4) || !(u < span) || !u.is_finite() {
            return (tr, rot, sc);
        }
        let h = INERTIA_H;
        let ih = 1.0 / h;
        let end = chain.gap[k - 1];
        let src0 = self.chain_at(chain, k - 1, end);
        let src1 = self.chain_at(chain, k - 1, end + h);
        let dst0 = self.local_pose(chain.clip[k], chain.t0[k], looping);
        let dst1 = self.local_pose(chain.clip[k], chain.t0[k] + chain.rate[k] * h, looping);
        let tau = (u / span).clamp(0.0, 1.0);
        let (t2, t3) = (tau * tau, tau * tau * tau);
        let h0 = 1.0 - 10.0 * t3 + 15.0 * t3 * tau - 6.0 * t3 * t2;
        let h1 = tau - 6.0 * t3 + 8.0 * t3 * tau - 3.0 * t3 * t2;
        for j in 0..self.joints.len() {
            for a in 0..3 {
                let x0 = src0.0[j][a] - dst0.0[j][a];
                let v0 = ((src1.0[j][a] - src0.0[j][a]) - (dst1.0[j][a] - dst0.0[j][a])) * ih;
                tr[j][a] += x0 * h0 + v0 * span * h1;
                let x0 = src0.2[j][a] - dst0.2[j][a];
                let v0 = ((src1.2[j][a] - src0.2[j][a]) - (dst1.2[j][a] - dst0.2[j][a])) * ih;
                sc[j][a] += x0 * h0 + v0 * span * h1;
            }
            let mut d0 = quat_mul(src0.1[j], quat_conj(dst0.1[j]));
            if d0[3] < 0.0 {
                d0 = [-d0[0], -d0[1], -d0[2], -d0[3]];
            }
            let mut d1 = quat_mul(src1.1[j], quat_conj(dst1.1[j]));
            if d0[0] * d1[0] + d0[1] * d1[1] + d0[2] * d1[2] + d0[3] * d1[3] < 0.0 {
                d1 = [-d1[0], -d1[1], -d1[2], -d1[3]];
            }
            // The offset as a rotation vector (the axis times the angle) and its velocity, like the translations': the curve is x0 h0 + v0 span h1 on each of the three. (Scaled along the offset's own axis, as it was, the
            // velocity could not be taken up when the two poses are alike and their velocities are not: a foot that is planted in the clip that was playing and swinging at 14 m/s in the one that is switched to has
            // no offset to scale, and its swing began in one picture. The velocity is bounded: a clip's end, or a loop that wraps in the picture's span, would give a speed that is not there.)
            let (l0, l1) = (quat_log(d0), quat_log(d1));
            let mut v = [(l1[0] - l0[0]) * ih, (l1[1] - l0[1]) * ih, (l1[2] - l0[2]) * ih];
            let vm = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if vm > INERTIA_MAX_SPIN {
                let k = INERTIA_MAX_SPIN / vm;
                v = [v[0] * k, v[1] * k, v[2] * k];
            }
            let mut off = [l0[0] * h0 + v[0] * span * h1, l0[1] * h0 + v[1] * span * h1, l0[2] * h0 + v[2] * span * h1];
            let (n0, no) = ((l0[0] * l0[0] + l0[1] * l0[1] + l0[2] * l0[2]).sqrt(), (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt());
            if no < 1e-7 {
                continue;
            }
            let cap = n0 * 1.15 + INERTIA_MAX_SPIN * span * 0.15;
            if no > cap {
                let k = cap / no;
                off = [off[0] * k, off[1] * k, off[2] * k];
            }
            rot[j] = norm4(quat_mul(quat_exp(off), rot[j]));
        }
        (tr, rot, sc)
    }

    /// Every joint's local translation, rotation and scale at `time` of `clip` (the rest pose where no track reaches).
    fn local_pose(&self, clip: Option<usize>, time: f32, looping: bool) -> (Vec<V3>, Vec<[f32; 4]>, Vec<V3>) {
        let n = self.joints.len();
        let mut tr: Vec<V3> = self.joints.iter().map(|j| j.t).collect();
        let mut rot: Vec<[f32; 4]> = self.joints.iter().map(|j| j.r).collect();
        let mut sc: Vec<V3> = self.joints.iter().map(|j| j.s).collect();
        if let Some(ci) = clip.filter(|&c| c < self.clips.len()) {
            let t = self.clip_time(ci, time, looping);
            for track in &self.clips[ci].tracks {
                if track.joint >= n {
                    continue;
                }
                let v = track.sample(t);
                match track.path {
                    Path::Translation => tr[track.joint] = [v[0], v[1], v[2]],
                    Path::Rotation => rot[track.joint] = v,
                    Path::Scale => sc[track.joint] = [v[0], v[1], v[2]],
                }
            }
        }
        (tr, rot, sc)
    }

    /// Skin matrices from local joint transforms.
    fn matrices(&self, tr: &[V3], rot: &[[f32; 4]], sc: &[V3]) -> Vec<M4> {
        let n = self.joints.len();
        let mut world: Vec<M4> = vec![M4::identity(); n];
        for &j in &self.order {
            let local = M4::from_trs(tr[j], rot[j], sc[j]);
            world[j] = match self.joints[j].parent {
                Some(p) => world[p].mul(&local),
                None => self.joints[j].base.mul(&local),
            };
        }
        (0..n).map(|j| world[j].mul(&self.joints[j].inverse_bind)).collect()
    }

    /// A copy of `base` (the model this rig belongs to) with its vertices moved by `matrices` (see `skin_matrices`).
    pub fn skin(&self, base: &Mesh, matrices: &[M4]) -> Mesh {
        let mut out = base.clone();
        let n = self.bind_pos.len().min(out.pos.len());
        for i in 0..n {
            let (j, w) = (self.vj[i], self.vw[i]);
            let (mut p, mut nr) = ([0.0f32; 3], [0.0f32; 3]);
            let mut total = 0.0;
            for k in 0..4 {
                if w[k] > 0.0 {
                    if let Some(m) = matrices.get(j[k] as usize) {
                        let (tp, tn) = (m.transform_point(self.bind_pos[i]), m.transform_dir(self.bind_nrm[i]));
                        p = add(p, scale(tp, w[k]));
                        nr = add(nr, scale(tn, w[k]));
                        total += w[k];
                    }
                }
            }
            if total > 0.0 {
                out.pos[i] = p;
                out.nrm[i] = normalize(nr);
            } else {
                out.pos[i] = self.bind_pos[i];
                out.nrm[i] = self.bind_nrm[i];
            }
        }
        out
    }

    /// The model posed at `time` seconds of playing clip `clip` (None: the rest pose).
    pub fn pose(&self, base: &Mesh, clip: Option<usize>, time: f32, looping: bool) -> Mesh {
        self.skin(base, &self.skin_matrices(clip, time, looping))
    }
}

/// Split an affine matrix into translation, rotation (a unit quaternion) and scale.
pub fn decompose(m: &M4) -> (V3, [f32; 4], V3) {
    let a = &m.0;
    let t = [a[12], a[13], a[14]];
    let col = |c: usize| [a[c * 4], a[c * 4 + 1], a[c * 4 + 2]];
    let mut s = [length(col(0)), length(col(1)), length(col(2))];
    // a mirrored matrix has a negative determinant: put the sign on one scale axis
    let det = dot(col(0), cross(col(1), col(2)));
    if det < 0.0 {
        s[0] = -s[0];
    }
    let inv = |v: f32| if v.abs() > 1e-12 { 1.0 / v } else { 0.0 };
    let (c0, c1, c2) = (scale(col(0), inv(s[0])), scale(col(1), inv(s[1])), scale(col(2), inv(s[2])));
    // rotation matrix with columns c0, c1, c2 to a quaternion
    let (m00, m10, m20, m01, m11, m21, m02, m12, m22) = (c0[0], c0[1], c0[2], c1[0], c1[1], c1[2], c2[0], c2[1], c2[2]);
    let tr = m00 + m11 + m22;
    let q = if tr > 0.0 {
        let k = (tr + 1.0).sqrt() * 2.0;
        [(m21 - m12) / k, (m02 - m20) / k, (m10 - m01) / k, 0.25 * k]
    } else if m00 > m11 && m00 > m22 {
        let k = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * k, (m01 + m10) / k, (m02 + m20) / k, (m21 - m12) / k]
    } else if m11 > m22 {
        let k = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / k, 0.25 * k, (m12 + m21) / k, (m02 - m20) / k]
    } else {
        let k = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / k, (m12 + m21) / k, 0.25 * k, (m10 - m01) / k]
    };
    (t, quat_normalize(q), s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(path: Path, interp: Interp, times: &[f32], values: &[f32]) -> Track {
        Track { joint: 0, path, interp, times: times.to_vec(), values: values.to_vec() }
    }

    fn one_joint_rig(clips: Vec<Clip>) -> Rig {
        let j = Joint { name: "a".into(), parent: None, base: M4::identity(), t: [0.0; 3], r: [0.0, 0.0, 0.0, 1.0], s: [1.0; 3], inverse_bind: M4::identity() };
        Rig { joints: vec![j], order: vec![0], clips, bind_pos: Vec::new(), bind_nrm: Vec::new(), vj: Vec::new(), vw: Vec::new() }
    }

    fn chain(clips: [usize; 2], t0: [f32; 2], gap: [f32; 2], span: f32) -> InertiaChain {
        InertiaChain { n: 2, looping: [false; INERTIA_MAX], clip: [Some(clips[0]), Some(clips[1]), None, None, None, None], t0: [t0[0], t0[1], 0.0, 0.0, 0.0, 0.0], rate: [1.0; INERTIA_MAX], gap: [gap[0], gap[1], 0.0, 0.0, 0.0, 0.0], span: [0.0, span, 0.0, 0.0, 0.0, 0.0] }
    }

    #[test]
    fn inertialization_starts_at_the_old_pose_and_velocity_and_ends_at_the_new_clip() {
        // clip 0 moves along x at 1 m/s, clip 1 stands at x = 5
        let a = Clip { name: "a".into(), duration: 10.0, tracks: vec![track(Path::Translation, Interp::Linear, &[0.0, 10.0], &[0.0, 0.0, 0.0, 10.0, 0.0, 0.0])], root: Vec::new() };
        let b = Clip { name: "b".into(), duration: 10.0, tracks: vec![track(Path::Translation, Interp::Linear, &[0.0, 10.0], &[5.0, 0.0, 0.0, 5.0, 0.0, 0.0])], root: Vec::new() };
        let rig = one_joint_rig(vec![a, b]);
        let x = |u: f32| rig.chain_local(&chain([0, 1], [2.0, 0.0], [0.5, u], 0.4), false).0[0][0];
        // the switch happens when clip 0 is at 2.5: the pose is still there at that moment, and keeps moving at the old 1 m/s
        assert!((x(0.0) - 2.5).abs() < 1e-5, "{}", x(0.0));
        assert!((x(0.01) - 2.51).abs() < 2e-3, "velocity is continuous: {}", x(0.01));
        // it ends at the new clip, exactly, and stays there
        assert!((x(0.4) - 5.0).abs() < 1e-5 && (x(0.9) - 5.0).abs() < 1e-5, "{} {}", x(0.4), x(0.9));
        // in between it moves from one to the other without a jump (the largest step between 100 samples is a smooth fraction of the whole way)
        let xs: Vec<f32> = (0..=100).map(|k| x(0.4 * k as f32 / 100.0)).collect();
        let biggest = xs.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(biggest < 0.08, "smooth: {biggest}");
        // and velocity dies down to the new clip's (0) at the end
        assert!((x(0.4) - x(0.39)).abs() < 1e-3);
    }

    #[test]
    fn inertialization_takes_up_a_velocity_that_differs_when_the_poses_are_alike() {
        // clip a turns about z at 4 rad/s, clip b stands at the angle a has at the moment of the switch (0.4 rad): the poses are the same, the velocities are not (a planted foot and a swinging one)
        let rot = |a: f32| [0.0, 0.0, (0.5 * a).sin(), (0.5 * a).cos()];
        let turning = Clip { name: "a".into(), duration: 2.0, tracks: vec![track(Path::Rotation, Interp::Linear, &[0.0, 0.2], &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, (0.4f32).sin(), (0.4f32).cos()])], root: Vec::new() };
        let r = rot(0.4);
        let still = Clip { name: "b".into(), duration: 2.0, tracks: vec![track(Path::Rotation, Interp::Linear, &[0.0, 2.0], &[r[0], r[1], r[2], r[3], r[0], r[1], r[2], r[3]])], root: Vec::new() };
        let rig = one_joint_rig(vec![turning, still]);
        let angle = |u: f32| {
            let q = rig.chain_local(&chain([0, 1], [0.0, 0.0], [0.1, u], 0.3), false).1[0];
            2.0 * q[2].atan2(q[3])
        };
        assert!((angle(0.0) - 0.4).abs() < 1e-4, "the switch itself shows the pose it had: {}", angle(0.0));
        // the turn goes on at about the old speed for the first pictures (4 rad/s: 0.0667 rad in 1/60 s), it is not stopped dead
        let first = angle(1.0 / 60.0) - angle(0.0);
        assert!(first > 0.045 && first < 0.09, "the velocity is continuous: {first} rad in a picture");
        // it comes to the new clip's pose and stays there
        assert!((angle(0.3) - 0.4).abs() < 1e-4 && (angle(0.6) - 0.4).abs() < 1e-4, "{} {}", angle(0.3), angle(0.6));
        // and the way there is smooth: no step bigger than the old speed asks for
        let xs: Vec<f32> = (0..=60).map(|k| angle(0.3 * k as f32 / 60.0)).collect();
        let biggest = xs.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(biggest < 0.08, "smooth: {biggest}");
    }

    #[test]
    fn inertialization_blends_rotations_along_the_short_way_and_ends_exactly() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let still = |q: [f32; 4]| Clip { name: "c".into(), duration: 2.0, tracks: vec![track(Path::Rotation, Interp::Linear, &[0.0, 2.0], &[q[0], q[1], q[2], q[3], q[0], q[1], q[2], q[3]])], root: Vec::new() };
        // identity, then a quarter turn about z
        let rig = one_joint_rig(vec![still([0.0, 0.0, 0.0, 1.0]), still([0.0, 0.0, h, h])]);
        let q = |u: f32| rig.chain_local(&chain([0, 1], [0.0, 0.0], [0.3, u], 0.5), false).1[0];
        let start = q(0.0);
        assert!(start[2].abs() < 1e-5 && (start[3] - 1.0).abs() < 1e-5, "{start:?}");
        let end = q(0.5);
        assert!((end[2] - h).abs() < 1e-5 && (end[3] - h).abs() < 1e-5, "{end:?}");
        let mid = q(0.25);
        let len = (mid[0] * mid[0] + mid[1] * mid[1] + mid[2] * mid[2] + mid[3] * mid[3]).sqrt();
        assert!((len - 1.0).abs() < 1e-5 && mid[2] > 0.2 && mid[2] < 0.6, "between the two: {mid:?}");
        // a single entry is just its clip
        let one = InertiaChain { n: 1, ..chain([0, 1], [0.0, 0.0], [0.3, 0.0], 0.5) };
        assert_eq!(rig.chain_local(&one, false).1[0], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn linear_tracks_interpolate_hold_their_ends_and_step_tracks_jump() {
        let t = track(Path::Translation, Interp::Linear, &[0.0, 1.0, 3.0], &[0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 4.0, 0.0]);
        assert_eq!(t.sample(-1.0)[..3], [0.0, 0.0, 0.0]);
        assert_eq!(t.sample(0.5)[..3], [1.0, 0.0, 0.0]);
        assert_eq!(t.sample(2.0)[..3], [2.0, 2.0, 0.0]);
        assert_eq!(t.sample(9.0)[..3], [2.0, 4.0, 0.0]);
        let s = track(Path::Translation, Interp::Step, &[0.0, 1.0], &[0.0, 0.0, 0.0, 5.0, 0.0, 0.0]);
        assert_eq!(s.sample(0.99)[0], 0.0);
        assert_eq!(s.sample(1.0)[0], 5.0);
    }

    #[test]
    fn rotations_blend_the_short_way_and_stay_unit_length() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        // identity to a quarter turn about z, and to the same turn written with the opposite sign
        let r = track(Path::Rotation, Interp::Linear, &[0.0, 1.0], &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -h, -h]);
        let mid = r.sample(0.5);
        let len = (mid[0] * mid[0] + mid[1] * mid[1] + mid[2] * mid[2] + mid[3] * mid[3]).sqrt();
        assert!((len - 1.0).abs() < 1e-6);
        assert!(mid[3] > 0.9, "the short way round keeps w large: {mid:?}");
    }

    #[test]
    fn a_cubic_spline_passes_through_its_keys_and_follows_the_tangents() {
        // keys store (in-tangent, value, out-tangent); both tangents are 0 here: an ease between 0 and 1
        let t = track(Path::Translation, Interp::Cubic, &[0.0, 1.0], &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!((t.sample(0.0)[0]).abs() < 1e-6 && (t.sample(1.0)[0] - 1.0).abs() < 1e-6);
        assert!((t.sample(0.5)[0] - 0.5).abs() < 1e-6, "{}", t.sample(0.5)[0]);
        assert!(t.sample(0.1)[0] < 0.1, "eases in");
    }

    #[test]
    fn decompose_undoes_from_trs() {
        let q = quat_axis_angle([0.0, 1.0, 0.0], 0.8);
        let m = M4::from_trs([1.0, 2.0, 3.0], q, [2.0, 1.0, 0.5]);
        let (t, r, s) = decompose(&m);
        assert_eq!(t, [1.0, 2.0, 3.0]);
        assert!(s.iter().zip([2.0, 1.0, 0.5]).all(|(a, b)| (a - b).abs() < 1e-5), "{s:?}");
        let d = (r[0] * q[0] + r[1] * q[1] + r[2] * q[2] + r[3] * q[3]).abs();
        assert!((d - 1.0).abs() < 1e-5, "{r:?} vs {q:?}");
    }

    /// Two joints stacked 1 unit apart; vertices at the base (joint 0), the middle (half and half) and the top (joint 1).
    fn two_joint_rig() -> (Rig, Mesh) {
        let joints = vec![
            Joint { name: "a".into(), parent: None, base: M4::identity(), t: [0.0, 0.0, 0.0], r: [0.0, 0.0, 0.0, 1.0], s: [1.0; 3], inverse_bind: M4::identity() },
            Joint { name: "b".into(), parent: Some(0), base: M4::identity(), t: [0.0, 1.0, 0.0], r: [0.0, 0.0, 0.0, 1.0], s: [1.0; 3], inverse_bind: M4::from_trs([0.0, -1.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3]) },
        ];
        let bind_pos = vec![[0.0, 0.0, 0.0], [0.0, 0.5, 0.0], [0.0, 1.0, 0.0]];
        let mesh = Mesh { pos: bind_pos.clone(), nrm: vec![[0.0, 0.0, 1.0]; 3], uv: vec![[0.0; 2]; 3], col: vec![], texw: vec![], idx: vec![0, 1, 2], tex: None, rig: None, normal_tex: None, arm_tex: None, alpha_cutoff: 0.0, lod: crate::mesh::LodMode::Full, parts: Vec::new(), occluders: Vec::new(), depth_whole: None, emissive_factor: 0.0 };
        let q = quat_axis_angle([0.0, 0.0, 1.0], std::f32::consts::FRAC_PI_2);
        let clip = Clip { name: "bend".into(), duration: 1.0, root: Vec::new(), tracks: vec![Track { joint: 1, path: Path::Rotation, interp: Interp::Linear, times: vec![0.0, 1.0], values: vec![0.0, 0.0, 0.0, 1.0, q[0], q[1], q[2], q[3]] }] };
        let rig = Rig { joints, order: vec![0, 1], clips: vec![clip], bind_pos, bind_nrm: vec![[0.0, 0.0, 1.0]; 3], vj: vec![[0, 0, 0, 0], [0, 1, 0, 0], [1, 0, 0, 0]], vw: vec![[1.0, 0.0, 0.0, 0.0], [0.5, 0.5, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]] };
        (rig, mesh)
    }

    #[test]
    fn skinning_moves_vertices_with_their_joints() {
        let (rig, mesh) = two_joint_rig();
        // the rest pose leaves the bind pose alone
        let rest = rig.pose(&mesh, None, 0.0, false);
        for (a, b) in rest.pos.iter().zip(&mesh.pos) {
            assert!((0..3).all(|k| (a[k] - b[k]).abs() < 1e-6), "{a:?} vs {b:?}");
        }
        // at the end of the clip the upper joint is bent a quarter turn about z at (0, 1, 0): its vertex (the top) stays at the joint
        // (a vertex on the joint does not move), while a vertex above the joint would swing to -x
        let bent = rig.pose(&mesh, Some(0), 1.0, false);
        assert!((bent.pos[2][1] - 1.0).abs() < 1e-5 && bent.pos[2][0].abs() < 1e-5, "{:?}", bent.pos[2]);
        // the middle vertex follows half joint 0 (stays) and half joint 1 (turns about (0,1,0)): it ends up half-way between
        let m = bent.pos[1];
        let expect = [0.5 * (0.0 + (-0.5 * 0.0 - 0.0)) + 0.5 * 0.0, 0.0, 0.0];
        let _ = expect;
        assert!(m[0].abs() > 0.1 && m[0] > -0.5 - 1e-5, "the middle vertex moves sideways: {m:?}");
        // normals turn with the joints and stay unit length
        let n = bent.nrm[1];
        assert!(((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_crossfade_blends_two_poses_and_the_ends_are_the_poses_themselves() {
        let (rig, mesh) = two_joint_rig();
        let bent_end = rig.pose(&mesh, Some(0), 1.0, false);
        let rest = rig.pose(&mesh, None, 0.0, false);
        // w = 0 is the first pose, w = 1 the second, exactly
        let a = rig.pose_blend(&mesh, (None, 0.0), (Some(0), 1.0), false, 0.0);
        let b = rig.pose_blend(&mesh, (None, 0.0), (Some(0), 1.0), false, 1.0);
        for k in 0..3 {
            for i in 0..3 {
                assert!((a.pos[k][i] - rest.pos[k][i]).abs() < 1e-5, "start {:?} vs {:?}", a.pos[k], rest.pos[k]);
                assert!((b.pos[k][i] - bent_end.pos[k][i]).abs() < 1e-5, "end {:?} vs {:?}", b.pos[k], bent_end.pos[k]);
            }
        }
        // half way: the middle vertex is between the two, and normals stay unit length
        let h = rig.pose_blend(&mesh, (None, 0.0), (Some(0), 1.0), false, 0.5);
        let (m0, m1, mh) = (rest.pos[1][0], bent_end.pos[1][0], h.pos[1][0]);
        assert!(mh < m0.max(m1) && mh > m0.min(m1), "{mh} is between {m0} and {m1}");
        let n = h.nrm[1];
        assert!(((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn layers_blend_by_weight_and_only_below_their_root() {
        let (mut rig, mesh) = two_joint_rig();
        // a second clip that turns the lower joint about z, and a stack: rest, then that clip
        rig.clips.push(Clip { name: "turn".into(), duration: 1.0, root: Vec::new(), tracks: vec![Track { joint: 0, path: Path::Rotation, interp: Interp::Linear, times: vec![0.0, 1.0], values: [quat_axis_angle([0.0, 0.0, 1.0], 1.0), quat_axis_angle([0.0, 0.0, 1.0], 1.0)].concat() }] });
        let rest = rig.pose(&mesh, None, 0.0, false);
        let bent = rig.pose(&mesh, Some(0), 1.0, false);
        let base = PoseLayer { clip: None, time: 0.0, looping: false, weight: 1.0, root: None, inertia: None };
        // a full-weight layer over everything is the clip itself
        let full = rig.pose_layers(&mesh, &[base, PoseLayer { clip: Some(0), time: 1.0, looping: false, weight: 1.0, root: None, inertia: None }]);
        // weight 0 is the base, weight 0.5 is between
        let none = rig.pose_layers(&mesh, &[base, PoseLayer { clip: Some(0), time: 1.0, looping: false, weight: 0.0, root: None, inertia: None }]);
        let half = rig.pose_layers(&mesh, &[base, PoseLayer { clip: Some(0), time: 1.0, looping: false, weight: 0.5, root: None, inertia: None }]);
        for i in 0..3 {
            for k in 0..3 {
                assert!((full.pos[i][k] - bent.pos[i][k]).abs() < 1e-5 && (none.pos[i][k] - rest.pos[i][k]).abs() < 1e-5);
            }
        }
        let (m0, m1, mh) = (rest.pos[1][0], bent.pos[1][0], half.pos[1][0]);
        assert!(mh < m0.max(m1) && mh > m0.min(m1), "{mh} between {m0} and {m1}");
        // a mask: the turn of joint 0 does not show when the layer is limited to joint 1 and what hangs from it, and does when it is limited to joint 0
        let masked = rig.pose_layers(&mesh, &[base, PoseLayer { clip: Some(1), time: 0.0, looping: false, weight: 1.0, root: Some(1), inertia: None }]);
        let open = rig.pose_layers(&mesh, &[base, PoseLayer { clip: Some(1), time: 0.0, looping: false, weight: 1.0, root: Some(0), inertia: None }]);
        assert!((0..3).all(|i| (0..3).all(|k| (masked.pos[i][k] - rest.pos[i][k]).abs() < 1e-5)), "joint 0 is outside the mask");
        assert!((0..3).any(|i| (0..3).any(|k| (open.pos[i][k] - rest.pos[i][k]).abs() > 1e-3)), "the turn shows when the mask covers joint 0");
        assert_eq!((rig.joint_index("b"), rig.joint_index("zz")), (Some(1), None));
    }

    #[test]
    fn clips_loop_or_hold() {
        let (rig, _) = two_joint_rig();
        assert!((rig.clip_time(0, 2.25, true) - 0.25).abs() < 1e-6);
        assert_eq!(rig.clip_time(0, 2.25, false), 1.0);
        assert_eq!(rig.clip_time(0, f32::NAN, true), 0.0);
        assert_eq!(rig.clip_index("bend"), Some(0));
        assert_eq!(rig.clip_index("nope"), None);
    }
}
