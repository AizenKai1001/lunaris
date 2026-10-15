//! The Lunaris GPU renderer on wgpu: Vulkan, DirectX 12 or Metal, headless (pictures for the `render` tool and tests) or into a window.
//!
//! It draws the `Scene` that `lunaris_render::gather_scene` builds, the same one the reference CPU renderer draws, in one of two modes:
//!
//! * **Classic** matches the reference renderer: the same gamma-2.0 working space, the same Blinn-Phong lighting, nearest-neighbour
//!   textures, no effects. The tests compare it with the CPU pictures pixel by pixel.
//! * **High** is the one made to look good: HDR rendering, physically based materials (`metallic`, `roughness`), a procedural sky that
//!   lights the scene and shows in reflections, cascaded shadow maps from the sun, screen-space ambient occlusion, bloom, distance
//!   fog, 4x multisampling, filmic tone mapping and true sRGB output. The `Environment` component steers it.
pub mod gpu;
pub mod pipelines;
pub mod renderer;
pub mod util;

pub use gpu::Gpu;
pub use renderer::{GpuRenderer, Mode, Quality};
