//! Lunaris reference renderer: deterministic CPU rasteriser for 2D sprites and 3D meshes, with entity-id and depth buffers.
//! It exists so every machine (and every AI agent, headless) can render the same pixels; a GPU renderer is validated against it.
pub mod font;
pub mod fonts;
pub mod frame;
pub mod gltf;
pub mod math;
pub mod mesh;
pub mod particles;
pub mod procedural;
pub mod query;
pub mod render;
pub mod rig;
mod l10n;
pub mod terrain;
pub mod texture;
mod ui;
pub mod views;

pub use frame::{encode_png, Frame, Visible};
pub use query::{raycast, raycast_masked, raycast_targets, raycast_with, RayHit, RayTargets};
pub use render::{is_model_name, render, ExplicitCamera, RenderOptions, View};
pub use texture::{is_procedural_texture, TextureCache};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
#[cfg(test)]
mod ik_tests;
#[cfg(test)]
mod meshcol_tests;
#[cfg(test)]
mod particle_tests;
#[cfg(test)]
mod views_tests;
