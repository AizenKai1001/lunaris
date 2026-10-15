//! The GPU device: an adapter chosen for performance, a device with the default (portable) limits, and a handler that turns wgpu's
//! asynchronous validation errors into messages the renderer can return instead of panics.
use std::sync::{Arc, Mutex};

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
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
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: surface,
            ..Default::default()
        }))
        .map_err(|e| format!("no graphics adapter is available ({e})"))?;
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("lunaris"),
            required_features: wgpu::Features::empty(),
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
        Ok(Gpu { instance, adapter, device, queue, info, errors })
    }

    /// Take the errors reported since the last call.
    pub fn take_errors(&self) -> Vec<String> {
        std::mem::take(&mut *self.errors.lock().unwrap_or_else(|p| p.into_inner()))
    }

    pub fn name(&self) -> String {
        format!("{} ({:?}, {:?})", self.info.name, self.info.backend, self.info.device_type)
    }
}
