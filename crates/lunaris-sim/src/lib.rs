//! Lunaris simulation: fixed-step deterministic loop, commands with undo, snapshots by clone, recording and replay.
pub mod character;
pub mod command;
pub mod components;
pub mod contact_memory;
pub mod detmath;
pub mod dialogue;
pub mod expr;
pub mod fieldio;
pub mod gameplay;
pub mod graph;
pub mod hull;
pub mod inventory;
pub mod inertia;
pub mod motionmatch;
pub mod footik;
pub mod traversal;
pub mod language;
mod hull_contact;
pub mod layers;
pub mod meshcol;
pub mod meshcol_gltf;
pub mod meshcol_shapes;
pub mod motion;
pub mod probe;
pub mod nav;
pub mod noise;
pub mod physics;
mod physics_rot;
pub mod plugin;
pub mod pool;
pub mod png;
pub mod prefab;
pub mod rng;
pub mod scenes;
pub mod sim;
pub mod steer;
pub mod weapons;
pub mod perception;
pub mod survival;
pub mod building;
pub mod flyagent;
pub mod stealth;
pub mod interact;
pub mod seats;
pub mod sight;
pub mod loadout;
pub mod armor;
pub mod gear;
pub mod rebind;
pub mod blast;
pub mod lock;
pub mod depth;
pub mod helicopter;
pub mod respawn;
pub mod party;
pub mod director;
pub mod escalation;
pub mod stomp;
pub mod walker;
pub mod hacking;
pub mod events;
pub mod loot;
pub mod lookctl;
pub mod particle_modules;
pub mod fx;
pub mod spriteanim;
pub mod systems;
pub mod terrain;
pub mod ui_kit;
pub mod ui_kit3;
mod ui_drag;
mod ui_plot;
mod ui_select;
mod ui_tip;
pub mod vehicle;
pub mod vehicle_ray;
pub mod water;
pub mod creatures;
pub mod floaters;
mod vm;

pub use command::{apply, apply_batch, Applied, Command};
pub use graph::{ScriptSet, ScriptSystem};
pub use rng::{hash_rand, Rng};
pub use sim::{default_threads, register_standard_components, Checkpoint, Event, RecordedRead, Recording, Sim, SimCtx, SystemEntry, MAX_EVENTS};

#[cfg(test)]
mod fx_tests;
#[cfg(test)]
mod character_tests;
#[cfg(test)]
mod ray_gather_bench_tests;
#[cfg(test)]
mod motion_tests;
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
mod inventory_tests;
#[cfg(test)]
mod weapons_tests;
#[cfg(test)]
mod perception_tests;
#[cfg(test)]
mod survival_tests;
#[cfg(test)]
mod director_tests;
#[cfg(test)]
mod escalation_tests;
#[cfg(test)]
mod stomp_tests;
#[cfg(test)]
mod walker_tests;
#[cfg(test)]
mod events_tests;
#[cfg(test)]
mod hacking_tests;
#[cfg(test)]
mod sentry_tests;
#[cfg(test)]
mod loot_tests;
#[cfg(test)]
mod building_tests;
#[cfg(test)]
mod helicopter_tests;
#[cfg(test)]
mod flyagent_tests;
#[cfg(test)]
mod stealth_tests;
#[cfg(test)]
mod interact_tests;
#[cfg(test)]
mod seats_tests;
#[cfg(test)]
mod loadout_tests;
#[cfg(test)]
mod armor_tests;
#[cfg(test)]
mod gear_tests;
#[cfg(test)]
mod rebind_tests;
#[cfg(test)]
mod blast_tests;
#[cfg(test)]
mod lock_tests;
#[cfg(test)]
mod depth_tests;
#[cfg(test)]
mod tier_tests;
#[cfg(test)]
mod respawn_tests;
#[cfg(test)]
mod party_tests;
#[cfg(test)]
mod inertia_tests;
#[cfg(test)]
mod motionmatch_tests;
#[cfg(test)]
mod footik_tests;
#[cfg(test)]
mod traversal_tests;
#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod gameplay_tests;
#[cfg(test)]
mod camera_collide_tests;
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
mod physics_fast_tests;
#[cfg(test)]
mod physics_ccd_tests;
#[cfg(test)]
mod physics_sleep_tests;
#[cfg(test)]
mod terrain_tests;
#[cfg(test)]
mod water_tests;
#[cfg(test)]
mod floaters_tests;
#[cfg(test)]
mod creatures_tests;
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
mod ui_plot_tests;
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
mod prefab_tests;
#[cfg(test)]
mod prefab_bench_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vehicle_tests;
#[cfg(test)]
mod vm_tests;
pub mod procgen;
