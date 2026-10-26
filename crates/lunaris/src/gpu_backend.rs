//! Connects the GPU renderer (`lunaris-gpu`) to the engine's `render` tool: the engine starts it the first time a picture asks for a
//! GPU quality, so commands that never draw do not open a graphics card, and a machine without one quietly keeps the CPU renderer.
use lunaris_api::{Engine, GpuBackend};
use lunaris_ecs::World;
use lunaris_gpu::{GpuRenderer, Quality};
use lunaris_render::{Frame, RenderOptions, TextureCache};

struct Backend {
    renderer: GpuRenderer,
    /// The program was started with `--quality NAME`: a software adapter is then wanted as it is.
    explicit_quality: bool,
}

impl GpuBackend for Backend {
    fn name(&self) -> String {
        self.renderer.adapter_name()
    }

    fn is_software(&self) -> bool {
        self.renderer.adapter_is_software() && !self.explicit_quality
    }

    fn render(&mut self, world: &mut World, textures: &mut TextureCache, opts: &RenderOptions, quality: &str) -> Result<Frame, String> {
        let q = Quality::from_name(quality).ok_or_else(|| format!("unknown quality `{quality}`"))?;
        self.renderer.render_to_image(world, textures, opts, &q)
    }

    fn bake_probes(&mut self, world: &mut World, textures: &mut TextureCache, uid: Option<u64>) -> Result<Vec<u64>, String> {
        self.renderer.bake_probes(world, textures, uid)
    }
}

/// Offer the GPU renderer to `engine` (unless `--no-gpu` or `LUNARIS_NO_GPU=1` says otherwise). `quality` is what `render` draws with
/// when a call names none.
pub fn attach(engine: &mut Engine, disabled: bool, quality: Option<&str>) {
    if disabled || std::env::var_os("LUNARIS_NO_GPU").is_some_and(|v| v == "1") {
        return;
    }
    let explicit_quality = quality.is_some();
    engine.enable_gpu(Box::new(move || GpuRenderer::headless().map(|renderer| Box::new(Backend { renderer, explicit_quality }) as Box<dyn GpuBackend>)), quality.unwrap_or("high"));
}
