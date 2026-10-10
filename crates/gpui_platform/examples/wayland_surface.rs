//! A GPU surface of its own on Wayland: the example renders an animation
//! into a native surface (a desynchronized `wl_subsurface`) with a wgpu
//! device of its own, under a GPUI menu, popover and dialog (see
//! `webview_common`).
//!
//! ```text
//! cargo run -p gpux-platform --features webview-examples,wayland --example wayland_surface
//! ```

#[path = "webview_common/mod.rs"]
mod common;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn main() {
    common::run("wayland_surface", embed);
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main() {
    eprintln!("wayland_surface runs on Linux under Wayland");
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn embed(
    surface: &gpux::WindowCompositionSurface,
    window: &mut gpux::Window,
    cx: &mut gpux::App,
) -> anyhow::Result<common::Embedded> {
    use gpui_wgpu::wgpu;
    use raw_window_handle::{HasDisplayHandle as _, RawWindowHandle};
    use std::{cell::RefCell, rc::Rc, time::Duration};

    let platform_surface = surface.platform_surface()?;
    let window_handle = *platform_surface
        .platform_handle()?
        .downcast::<RawWindowHandle>()
        .map_err(|_| anyhow::anyhow!("the native surface has no raw window handle"))?;
    anyhow::ensure!(
        matches!(window_handle, RawWindowHandle::Wayland(_)),
        "wayland_surface needs a Wayland session"
    );
    let display_handle = window
        .display_handle()
        .map_err(|error| anyhow::anyhow!("the window has no display handle: {error}"))?
        .as_raw();

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
        flags: wgpu::InstanceFlags::default(),
        backend_options: wgpu::BackendOptions::default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        display: None,
    });
    // SAFETY: the subsurface outlives the wgpu surface: both live as long as
    // the example's window, and the surface is dropped with the content.
    let gpu_surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(display_handle),
            raw_window_handle: window_handle,
        })
    }?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: Some(&gpu_surface),
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("wayland_surface"),
        required_features: wgpu::Features::empty(),
        required_limits:
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))?;
    let caps = gpu_surface.get_capabilities(&adapter);
    let format = caps
        .formats
        .first()
        .copied()
        .ok_or_else(|| anyhow::anyhow!("no format"))?;

    struct Producer {
        _instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        alpha_mode: wgpu::CompositeAlphaMode,
        size: (u32, u32),
        start: std::time::Instant,
    }

    impl Producer {
        /// Presents one frame of a hue cycling with time, sized to `size`
        /// device pixels. A producer sizes its own buffers to the bounds.
        fn present(&mut self, size: (u32, u32)) {
            if size.0 == 0 || size.1 == 0 {
                return;
            }
            if size != self.size {
                self.surface.configure(
                    &self.device,
                    &wgpu::SurfaceConfiguration {
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        format: self.format,
                        width: size.0,
                        height: size.1,
                        present_mode: wgpu::PresentMode::Fifo,
                        desired_maximum_frame_latency: 2,
                        alpha_mode: self.alpha_mode,
                        view_formats: Vec::new(),
                        color_space: wgpu::SurfaceColorSpace::Auto,
                    },
                );
                self.size = size;
            }
            let frame = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                _ => return,
            };
            let t = self.start.elapsed().as_secs_f64();
            let color = wgpu::Color {
                r: 0.5 + 0.5 * (t * 1.3).sin(),
                g: 0.5 + 0.5 * (t * 0.7 + 2.).sin(),
                b: 0.5 + 0.5 * (t * 1.1 + 4.).sin(),
                a: 1.,
            };
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(color),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            self.queue.submit(std::iter::once(encoder.finish()));
            self.queue.present(frame);
        }
    }

    let producer = Rc::new(RefCell::new(Producer {
        _instance: instance,
        surface: gpu_surface,
        device,
        queue,
        format,
        alpha_mode: caps.alpha_modes.first().copied().unwrap_or_default(),
        size: (0, 0),
        start: std::time::Instant::now(),
    }));
    let surface = surface.clone();
    let task = cx.spawn({
        let producer = producer.clone();
        async move |cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let Ok(platform_surface) = surface.platform_surface() else {
                break;
            };
            let bounds = platform_surface.bounds();
            producer.borrow_mut().present((
                bounds.size.width.0.max(0) as u32,
                bounds.size.height.0.max(0) as u32,
            ));
        }
    });
    Ok(common::Embedded {
        owner: Box::new((producer, task)),
        pointer: None,
    })
}
