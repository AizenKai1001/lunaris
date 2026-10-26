//! The GPU device: an adapter chosen for performance, a device with the default (portable) limits, and a handler that turns wgpu's
//! asynchronous validation errors into messages the renderer can return instead of panics.
use std::sync::{Arc, Mutex};

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    /// Whether the card can time passes (`Features::TIMESTAMP_QUERY`, asked for when the adapter has it).
    pub timestamps: bool,
    /// Whether it can also stamp the time between two draws of one pass (`Features::TIMESTAMP_QUERY_INSIDE_PASSES`: the profiler's `mark`s).
    pub timestamps_inside: bool,
    /// Whether it reads block-compressed textures (BC1 to BC7: every desktop card does; asked for when the adapter has it).
    pub bc: bool,
    /// Errors wgpu reported outside of an error scope, oldest first.
    pub errors: Arc<Mutex<Vec<String>>>,
}

impl Gpu {
    /// A device for drawing into images (no window).
    pub fn headless() -> Result<Gpu, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        Gpu::with_instance(instance, None)
    }

    /// A device that can draw into `surface` (it must have been created from `instance`).
    pub fn with_instance(instance: wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<Gpu, String> {
        // `LUNARIS_GPU=1070` picks the adapter whose name contains that text (on a machine with two cards: the one that is not busy, or the weaker one for a speed test); with no match, or without it, the
        // adapter chosen for performance is used
        let wanted = std::env::var("LUNARIS_GPU").ok().map(|s| s.to_lowercase()).filter(|s| !s.is_empty());
        let named = wanted.and_then(|w| {
            pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
                .into_iter()
                .find(|a| a.get_info().name.to_lowercase().contains(&w) && surface.is_none_or(|s| a.is_surface_supported(s)))
        });
        let adapter = match named {
            Some(a) => a,
            None => pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: surface,
                ..Default::default()
            }))
            .map_err(|e| format!("no graphics adapter is available ({e})"))?,
        };
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("lunaris"),
            required_features: adapter.features() & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES | wgpu::Features::TEXTURE_COMPRESSION_BC),
            required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("cannot open the graphics device ({e})"))?;
        let errors: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = errors.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            let mut v = sink.lock().unwrap_or_else(|p| p.into_inner());
            if v.len() < 32 {
                v.push(e.to_string());
            }
        }));
        let timestamps = device.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let timestamps_inside = timestamps && device.features().contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);
        let bc = device.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
        Ok(Gpu { instance, adapter, device, queue, info, timestamps, timestamps_inside, bc, errors })
    }

    /// Take the errors reported since the last call.
    pub fn take_errors(&self) -> Vec<String> {
        std::mem::take(&mut *self.errors.lock().unwrap_or_else(|p| p.into_inner()))
    }

    pub fn name(&self) -> String {
        format!("{} ({:?}, {:?})", self.info.name, self.info.backend, self.info.device_type)
    }

    /// Whether the adapter is a software one (Mesa's llvmpipe, Windows' WARP): Vulkan or Direct3D run on the processor.
    pub fn is_software(&self) -> bool {
        self.info.device_type == wgpu::DeviceType::Cpu
    }
}
