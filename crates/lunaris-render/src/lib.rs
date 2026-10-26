//! Lunaris reference renderer: deterministic CPU rasteriser for 2D sprites and 3D meshes, with entity-id and depth buffers.
//! It exists so every machine (and every AI agent, headless) can render the same pixels; a GPU renderer is validated against it.
pub mod atmosphere;
pub mod font;
pub mod fonts;
pub mod frame;
pub mod fx_draw;
pub mod fx_sim;
pub mod gltf;
pub mod loader;
pub mod timing;
pub mod bg;
pub use timing::Timed;
pub mod math;
pub mod mesh;
pub mod motion_db;
pub mod particles;
pub mod procedural;
pub mod query;
pub mod render;
pub mod rig;
mod l10n;
pub mod terrain;
pub mod thumb;
pub mod terrain_layers;
pub mod texture;
mod ui;
pub mod ui_audit;
pub mod views;
pub mod water_body;

pub use frame::{encode_png, Frame, Visible};
pub use query::{raycast, raycast_masked, raycast_targets, raycast_with, RayHit, RayTargets};
pub use render::{is_model_name, render, ExplicitCamera, RenderOptions, View};
pub use texture::{is_procedural_texture, TextureCache};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
#[cfg(test)]
mod inertia_real_tests;
#[cfg(test)]
mod ik_tests;
#[cfg(test)]
mod puppet_cost_tests;
#[cfg(test)]
mod pbr_tests;
#[cfg(test)]
mod meshcol_tests;
#[cfg(test)]
mod fx_tests;
#[cfg(test)]
mod particle_tests;
#[cfg(test)]
mod views_tests;
#[cfg(test)]
mod draw_order_tests;
#[cfg(test)]
mod ui_scale_tests;
#[cfg(test)]
mod ui_scroll_tests;
#[cfg(test)]
mod ui_radial_tests;
#[cfg(test)]
mod ui_audit_tests;
#[cfg(test)]
mod terrain_layers_tests;
