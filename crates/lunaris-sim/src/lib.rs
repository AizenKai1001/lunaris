//! Lunaris simulation: fixed-step deterministic loop, commands with undo, snapshots by clone, recording and replay.
pub mod character;
pub mod command;
pub mod components;
pub mod detmath;
pub mod dialogue;
pub mod expr;
pub mod fieldio;
pub mod gameplay;
pub mod graph;
pub mod hull;
pub mod language;
mod hull_contact;
pub mod layers;
pub mod meshcol;
pub mod meshcol_gltf;
pub mod meshcol_shapes;
pub mod nav;
pub mod noise;
pub mod physics;
mod physics_rot;
pub mod plugin;
pub mod png;
pub mod prefab;
pub mod rng;
pub mod scenes;
pub mod sim;
pub mod steer;
pub mod lookctl;
pub mod particle_modules;
pub mod spriteanim;
pub mod systems;
pub mod terrain;
pub mod ui_kit;
pub mod ui_kit3;
mod ui_drag;
mod ui_select;
mod ui_tip;
pub mod vehicle;
mod vehicle_ray;
pub mod water;
mod vm;

pub use command::{apply, apply_batch, Applied, Command};
pub use graph::{ScriptSet, ScriptSystem};
pub use rng::{hash_rand, Rng};
pub use sim::{default_threads, register_standard_components, Checkpoint, Event, RecordedRead, Recording, Sim, SimCtx, SystemEntry, MAX_EVENTS};

#[cfg(test)]
mod character_tests;
#[cfg(test)]
mod dialogue_tests;
#[cfg(test)]
mod graph_tests;
#[cfg(test)]
mod hierarchy_tests;
#[cfg(test)]
mod hull_contact_tests;
#[cfg(test)]
mod hull_physics_tests;
#[cfg(test)]
mod hull_tests;
#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod gameplay_tests;
#[cfg(test)]
mod joint_tests;
#[cfg(test)]
mod layers_tests;
#[cfg(test)]
mod language_tests;
#[cfg(test)]
mod mesh_physics_tests;
#[cfg(test)]
mod meshcol_tests;
#[cfg(test)]
mod nav_tests;
#[cfg(test)]
mod particles_tests;
#[cfg(test)]
mod physics_tests;
#[cfg(test)]
mod physics_rot_tests;
#[cfg(test)]
mod terrain_tests;
#[cfg(test)]
mod water_tests;
#[cfg(test)]
mod ui_kit_tests;
#[cfg(test)]
mod ui_kit3_tests;
#[cfg(test)]
mod ui_select_tests;
#[cfg(test)]
mod ui_drag_tests;
#[cfg(test)]
mod ui_tip_tests;
#[cfg(test)]
mod prefab_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vehicle_tests;
#[cfg(test)]
mod vm_tests;
