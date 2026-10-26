//! Evolved creatures for Lunaris games.
//!
//! A creature here is a body (a rigid-body skeleton with joints, muscles and, for the spider, the hydraulic leg extension of a real spider) and a brain (three programs of the Evolve-0 machine) that
//! were trained together on the GPU (`spidergpu`, in the Evolve-0 project) to walk to a target over any ground. This crate runs them in a game exactly as they were trained: the trainer's physics and
//! machine are ported line for line and use their own deterministic math, so the GPU, the trainer's CPU twin and this crate agree to the last bit (see `tests/parity.rs`).
//!
//! ```ignore
//! let model = Model::spider();
//! let brain = Brain::from_champions_json(&std::fs::read_to_string("spider_champions.json")?)?.remove(0);
//! let ground = Flat; // or a Grid, or your own `Ground`
//! let mut spider = Creature::new(&model, brain, 7, &ground);
//! loop {
//!     spider.step([target_x, target_z], &ground);        // 60 ticks a second
//!     let (position, rotation) = spider.pose(0);          // the prosoma; 50 bodies in all, see `NB`
//! }
//! ```
pub mod creature;
pub mod episode;
pub mod ground;
pub mod mathx;
pub mod model;
pub mod nn;
pub mod solid;
pub mod vecmath;
pub mod visuals;
pub mod vm;

pub use creature::{Creature, DT};
pub use ground::{Flat, Grid, Ground, WithSolids};
pub use solid::{SolidBox, Solids};
pub use model::{Model, LEGB, LEGJ, NB, NJ, N_LEGS};
pub use vm::Brain;
