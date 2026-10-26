//! The body model: what the physics needs to know about the creature's body, as the trainer exports it (`spidergpu export`): the floats of `struct Model` of `creature_core.h` in their order.
//! The spider's own model is built into the crate (`Model::spider()`), the same bytes that the trainer used.
use crate::vecmath::V3;

pub const N_LEGS: usize = 8;
/// Bodies of a leg, in order: coxa, femur, patella, tibia, metatarsus, tarsus.
pub const LEGB: usize = 6;
/// Joints of a leg that the brain drives: swing, lift, femur-patella, patella-tibia, tibia-metatarsus.
pub const LEGJ: usize = 5;
pub const NJ: usize = N_LEGS * LEGJ;
/// The prosoma, the abdomen and the segments of the legs.
pub const NB: usize = 2 + LEGB * N_LEGS;
/// Floats of a model.
pub const MODEL_FLOATS: usize = NB * 42 + 6;

/// The unit of the body: everything that does not change while the creature lives.
#[derive(Clone, Debug)]
pub struct Model {
    pub inv_mass: [f32; NB],
    /// Inverse inertia about the body's own axes.
    pub inv_i: [[f32; 3]; NB],
    /// Half sizes of the box of each body.
    pub half: [[f32; 3]; NB],
    pub rest_pos: [[f32; 3]; NB],
    pub rest_q: [[f32; 4]; NB],
    /// Index of the parent body (-1 for the root).
    pub parent: [f32; NB],
    /// The hinge's anchor in the parent's frame and in the child's own frame, its axis in both frames, the child's rest rotation in the parent's frame.
    pub rp: [[f32; 3]; NB],
    pub rc: [[f32; 3]; NB],
    pub axp: [[f32; 3]; NB],
    pub axc: [[f32; 3]; NB],
    pub qrel: [[f32; 4]; NB],
    /// Joint limits (lo < hi: on).
    pub lim: [[f32; 2]; NB],
    /// The strongest muscle torque of a joint (0: no motor).
    pub tmax: [f32; NB],
    pub ctrl: [f32; NB],
    pub damp: [f32; NB],
    pub spring: [f32; NB],
    /// The hydraulics of a joint: the strongest torque (signed: the direction that extends the joint), the stiffness, how far full pressure extends it and how far no pressure lets it flex.
    pub hyd: [f32; NB],
    pub hk: [f32; NB],
    pub hx: [f32; NB],
    pub hf: [f32; NB],
    /// A factor on the friction of every part, and the grip of each part on the ground.
    pub mu: f32,
    pub mub: [f32; NB],
    /// How much of the last step's impulses is applied at the start of this one, how fast position errors are corrected, how many small steps a tick is, how many rounds without correction follow each.
    pub ws: f32,
    pub beta: f32,
    pub sub: f32,
    pub relax: f32,
    /// How much the bodies of different creatures differ.
    pub indiv: f32,
}

struct Reader<'a> {
    v: &'a [f32],
    at: usize,
}

impl Reader<'_> {
    fn one(&mut self) -> f32 {
        let x = self.v[self.at];
        self.at += 1;
        x
    }
    fn arr<const N: usize>(&mut self) -> [f32; N] {
        let mut a = [0.0f32; N];
        a.copy_from_slice(&self.v[self.at..self.at + N]);
        self.at += N;
        a
    }
    fn nb(&mut self) -> [f32; NB] {
        self.arr::<NB>()
    }
    fn nb3(&mut self) -> [[f32; 3]; NB] {
        let mut a = [[0.0f32; 3]; NB];
        for x in a.iter_mut() {
            *x = self.arr::<3>();
        }
        a
    }
    fn nb4(&mut self) -> [[f32; 4]; NB] {
        let mut a = [[0.0f32; 4]; NB];
        for x in a.iter_mut() {
            *x = self.arr::<4>();
        }
        a
    }
    fn nb2(&mut self) -> [[f32; 2]; NB] {
        let mut a = [[0.0f32; 2]; NB];
        for x in a.iter_mut() {
            *x = self.arr::<2>();
        }
        a
    }
}

impl Model {
    /// A model from its floats (`MODEL_FLOATS` of them, in the order of `struct Model` in `creature_core.h`).
    pub fn from_floats(v: &[f32]) -> Result<Model, String> {
        if v.len() != MODEL_FLOATS {
            return Err(format!("a model has {MODEL_FLOATS} floats, this has {}", v.len()));
        }
        let mut r = Reader { v, at: 0 };
        let m = Model {
            inv_mass: r.nb(),
            inv_i: r.nb3(),
            half: r.nb3(),
            rest_pos: r.nb3(),
            rest_q: r.nb4(),
            parent: r.nb(),
            rp: r.nb3(),
            rc: r.nb3(),
            axp: r.nb3(),
            axc: r.nb3(),
            qrel: r.nb4(),
            lim: r.nb2(),
            tmax: r.nb(),
            ctrl: r.nb(),
            damp: r.nb(),
            spring: r.nb(),
            hyd: r.nb(),
            hk: r.nb(),
            hx: r.nb(),
            hf: r.nb(),
            mu: r.one(),
            mub: r.nb(),
            ws: r.one(),
            beta: r.one(),
            sub: r.one(),
            relax: r.one(),
            indiv: r.one(),
        };
        debug_assert_eq!(r.at, MODEL_FLOATS);
        Ok(m)
    }

    /// A model from the bytes of a model file (little-endian f32).
    pub fn from_bytes(b: &[u8]) -> Result<Model, String> {
        if b.len() % 4 != 0 {
            return Err("a model file is a whole number of floats".to_string());
        }
        let v: Vec<f32> = b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        Model::from_floats(&v)
    }

    /// The spider (the body that `spidergpu` trains), as exported by `spidergpu export`.
    pub fn spider() -> Model {
        Model::from_bytes(include_bytes!("../data/spider_model.f32")).expect("the built-in spider model is well formed")
    }

    /// Where the centre of body `i` starts (the rest pose) as a vector.
    pub fn rest_position(&self, i: usize) -> V3 {
        crate::vecmath::ld3(&self.rest_pos[i])
    }
}
