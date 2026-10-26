//! The hook through which the `render` tool can draw with a GPU renderer. The API crate itself knows nothing about graphics cards:
//! the program that hosts the engine (the `lunaris` binary) hands over a factory, and the engine starts the renderer the first time a
//! picture asks for it, so commands that never draw do not pay for opening a GPU, and a machine without one falls back quietly.
use lunaris_ecs::World;
use lunaris_render::{Frame, RenderOptions, TextureCache};

/// A renderer that draws the world faster, and better looking, than the reference CPU renderer.
pub trait GpuBackend: Send {
    /// What it draws on, for messages ("NVIDIA GeForce RTX 4060 (Vulkan, DiscreteGpu)").
    fn name(&self) -> String;
    /// Whether it is a software adapter (the processor under a graphics API), which draws a picture several times slower than the CPU reference renderer does: a call that names no quality then uses the reference renderer.
    fn is_software(&self) -> bool {
        false
    }
    /// Draw `world` with a quality named `classic`, `low`, `medium`, `high` or `ultra`. The frame has the picture and the entity at
    /// each pixel (`ids`); it may leave `depth` at infinity.
    fn render(&mut self, world: &mut World, textures: &mut TextureCache, opts: &RenderOptions, quality: &str) -> Result<Frame, String>;
    /// Capture the world's reflection probes (`ReflectionProbe`) again now (`uid`: only that probe, `None`: all of them) and say which were captured. A renderer without probes says so.
    fn bake_probes(&mut self, _world: &mut World, _textures: &mut TextureCache, _uid: Option<u64>) -> Result<Vec<u64>, String> {
        Err("this renderer has no reflection probes".to_string())
    }
}

pub type GpuFactory = Box<dyn FnOnce() -> Result<Box<dyn GpuBackend>, String> + Send>;

/// Where the engine stands with its GPU renderer.
pub(crate) enum GpuState {
    /// None was offered: only the reference renderer exists.
    Off,
    /// Offered, not started yet.
    Offered(GpuFactory),
    Ready(Box<dyn GpuBackend>),
    /// Tried and failed (the reason is kept for the `renderer` note).
    Failed(String),
}

/// The quality names the `render` tool understands. `reference` is the deterministic CPU renderer; the rest need a GPU.
pub const QUALITIES: [&str; 6] = ["reference", "classic", "low", "medium", "high", "ultra"];
