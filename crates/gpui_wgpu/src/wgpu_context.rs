#[cfg(not(target_family = "wasm"))]
use anyhow::Context as _;
#[cfg(not(target_family = "wasm"))]
use gpui_util::ResultExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use wgpu::TextureFormat;

#[derive(Clone)]
pub struct WgpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    backend: WgpuBackend,
    dual_source_blending: bool,
    color_texture_format: wgpu::TextureFormat,
    device_lost: Arc<AtomicBool>,
    /// The device-wide error state of a headless context, whose cached device several
    /// renderers share. Windowed contexts leave errors to their renderers.
    #[cfg(not(target_family = "wasm"))]
    shared_errors: Option<Arc<DeviceErrorState>>,
}

/// GPU errors reported by a device's single uncaptured-error callback, shared by every
/// renderer on it (Zed #64718/#64842). wgpu keeps one callback per device, so the state is
/// installed once and each renderer observes it from the point it joined.
#[cfg(not(target_family = "wasm"))]
#[derive(Default)]
pub struct DeviceErrorState {
    last_error: parking_lot::Mutex<(u64, Option<String>)>,
}

#[cfg(not(target_family = "wasm"))]
impl DeviceErrorState {
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    fn install(device: &wgpu::Device) -> Arc<Self> {
        let errors = Arc::new(Self::default());
        device.on_uncaptured_error(Arc::new({
            let errors = Arc::clone(&errors);
            move |error| errors.record(error.to_string())
        }));
        errors
    }

    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    fn record(&self, error: String) {
        let mut last_error = self.last_error.lock();
        last_error.0 = last_error.0.wrapping_add(1);
        last_error.1 = Some(error);
    }

    /// The generation a renderer joining the device starts from, so it observes only
    /// errors raised after it joined, including any while it builds its pipelines.
    pub fn current_generation(&self) -> u64 {
        self.last_error.lock().0
    }

    /// The latest error raised since `generation`, once per observer; it advances
    /// `generation`. Several errors between observations coalesce into the latest one.
    pub fn observe_error(&self, generation: &mut u64) -> Option<String> {
        let last_error = self.last_error.lock();
        if *generation == last_error.0 {
            return None;
        }
        *generation = last_error.0;
        last_error.1.clone()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WgpuBackend {
    BrowserWebGpu,
    Gl,
    Native(wgpu::Backend),
}

#[cfg(target_family = "wasm")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WebBackendPreference {
    #[default]
    Auto,
    WebGpu,
    WebGl,
}

#[cfg(target_family = "wasm")]
pub struct PreparedWebGraphics {
    pub context: WgpuContext,
    pub surface: wgpu::Surface<'static>,
}

/// wgpu-core requires a display handle before it will construct a WebGL surface.
/// The browser backend does not dereference this unit handle; it only uses it to
/// establish that the instance is associated with the web display.
#[cfg(target_family = "wasm")]
#[derive(Debug)]
struct WebDisplaySource;

#[cfg(target_family = "wasm")]
impl raw_window_handle::HasDisplayHandle for WebDisplaySource {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(raw_window_handle::DisplayHandle::web())
    }
}

#[cfg(all(not(target_family = "wasm"), target_os = "windows"))]
pub fn native_backend_policy() -> wgpu::Backends {
    wgpu::Backends::DX12
}

#[cfg(all(not(target_family = "wasm"), target_os = "windows"))]
pub fn native_backend_policy_label() -> &'static str {
    "DX12"
}

#[cfg(all(not(target_family = "wasm"), not(target_os = "windows")))]
pub fn native_backend_policy() -> wgpu::Backends {
    wgpu::Backends::VULKAN | wgpu::Backends::GL
}

#[cfg(all(not(target_family = "wasm"), not(target_os = "windows")))]
pub fn native_backend_policy_label() -> &'static str {
    "VULKAN|GL"
}

#[derive(Clone, Copy)]
pub struct CompositorGpuHint {
    pub vendor_id: u32,
    pub device_id: u32,
}

/// Which adapter a headless renderer uses.
///
/// Only headless rendering (tests, benchmarks, offscreen captures) honours this; windowed
/// renderers keep their own adapter selection.
#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum HeadlessAdapterSelection {
    /// The best adapter of the native backend policy: discrete, then integrated, then others,
    /// with software adapters last.
    #[default]
    Default,
    /// A software adapter: WARP on Windows, lavapipe or llvmpipe on Linux.
    Software,
    /// Any adapter that is not a software rasterizer.
    Hardware,
    /// The first adapter whose name contains this text, ignoring case.
    Name(String),
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
impl HeadlessAdapterSelection {
    /// The environment variable that selects the headless adapter.
    pub const ENV_VAR: &'static str = "GPUX_WGPU_ADAPTER";

    /// Reads [`Self::ENV_VAR`]: `software`, `hardware`, or a name substring. Unset or empty
    /// selects [`Self::Default`].
    pub fn from_env() -> Self {
        std::env::var(Self::ENV_VAR)
            .map(|value| Self::parse(&value))
            .unwrap_or_default()
    }

    /// Parses an override value as [`Self::from_env`] does.
    pub fn parse(value: &str) -> Self {
        let value = value.trim();
        if value.is_empty() {
            Self::Default
        } else if value.eq_ignore_ascii_case("software") {
            Self::Software
        } else if value.eq_ignore_ascii_case("hardware") {
            Self::Hardware
        } else {
            Self::Name(value.to_string())
        }
    }

    fn accepts(&self, info: &wgpu::AdapterInfo) -> bool {
        match self {
            Self::Default => true,
            Self::Software => info.device_type == wgpu::DeviceType::Cpu,
            Self::Hardware => info.device_type != wgpu::DeviceType::Cpu,
            Self::Name(name) => info.name.to_lowercase().contains(&name.to_lowercase()),
        }
    }
}

impl WgpuContext {
    #[cfg(not(target_family = "wasm"))]
    pub fn new(
        instance: wgpu::Instance,
        surface: &wgpu::Surface<'_>,
        compositor_gpu: Option<CompositorGpuHint>,
    ) -> anyhow::Result<Self> {
        Self::new_with_backend_policy(instance, surface, compositor_gpu, native_backend_policy())
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn new_with_backend_policy(
        instance: wgpu::Instance,
        surface: &wgpu::Surface<'_>,
        compositor_gpu: Option<CompositorGpuHint>,
        backend_policy: wgpu::Backends,
    ) -> anyhow::Result<Self> {
        Self::new_with_options(instance, surface, compositor_gpu, false, backend_policy)
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn new_rejecting_software(
        instance: wgpu::Instance,
        surface: &wgpu::Surface<'_>,
        compositor_gpu: Option<CompositorGpuHint>,
    ) -> anyhow::Result<Self> {
        Self::new_rejecting_software_with_backend_policy(
            instance,
            surface,
            compositor_gpu,
            native_backend_policy(),
        )
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn new_rejecting_software_with_backend_policy(
        instance: wgpu::Instance,
        surface: &wgpu::Surface<'_>,
        compositor_gpu: Option<CompositorGpuHint>,
        backend_policy: wgpu::Backends,
    ) -> anyhow::Result<Self> {
        Self::new_with_options(instance, surface, compositor_gpu, true, backend_policy)
    }

    #[cfg(not(target_family = "wasm"))]
    fn new_with_options(
        instance: wgpu::Instance,
        surface: &wgpu::Surface<'_>,
        compositor_gpu: Option<CompositorGpuHint>,
        reject_software: bool,
        backend_policy: wgpu::Backends,
    ) -> anyhow::Result<Self> {
        let device_id_filter = device_id_filter();

        // Select an adapter by actually testing surface configuration with the real device.
        // This is the only reliable way to determine compatibility on hybrid GPU systems.
        let (adapter, device, queue, dual_source_blending, color_texture_format) =
            gpux::block_on(Self::select_adapter_and_device(
                &instance,
                device_id_filter,
                surface,
                compositor_gpu.as_ref(),
                reject_software,
                backend_policy,
            ))?;

        let device_lost = install_device_lost_callback(&device);

        log::info!(
            "Selected GPU adapter: {:?} ({:?})",
            adapter.get_info().name,
            adapter.get_info().backend
        );
        let backend = WgpuBackend::Native(adapter.get_info().backend);

        Ok(Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            backend,
            dual_source_blending,
            color_texture_format,
            device_lost,
            shared_errors: None,
        })
    }

    /// A context with no surface for headless rendering, on the adapter `selection` names.
    ///
    /// Creating a device takes tens of milliseconds and tests and benchmarks create a headless
    /// renderer per window, so each selection shares one device per process until it is lost.
    /// The cache is a static rather than a thread-local and is never dropped: wgpu's queue
    /// reads its own thread-locals on drop and would panic during thread-local teardown
    /// (Zed #64842).
    #[cfg(all(
        not(target_family = "wasm"),
        any(test, feature = "bench-support", feature = "test-support")
    ))]
    pub fn new_headless(
        selection: &HeadlessAdapterSelection,
    ) -> anyhow::Result<(Self, wgpu::TextureFormat)> {
        type Cache = Vec<(HeadlessAdapterSelection, WgpuContext, wgpu::TextureFormat)>;
        static HEADLESS: parking_lot::Mutex<Cache> = parking_lot::Mutex::new(Vec::new());
        let mut headless = HEADLESS.lock();
        if let Some((_, context, format)) = headless
            .iter()
            .find(|(cached, context, _)| cached == selection && !context.device_lost())
        {
            return Ok((context.clone(), *format));
        }
        let created = Self::create_headless(selection)?;
        headless.retain(|(cached, _, _)| cached != selection);
        headless.push((selection.clone(), created.0.clone(), created.1));
        Ok(created)
    }

    #[cfg(all(
        not(target_family = "wasm"),
        any(test, feature = "bench-support", feature = "test-support")
    ))]
    fn create_headless(
        selection: &HeadlessAdapterSelection,
    ) -> anyhow::Result<(Self, wgpu::TextureFormat)> {
        let backend_policy = native_backend_policy();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: backend_policy,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        let device_id_filter = device_id_filter();
        let (adapter, device, queue, dual_source_blending, color_texture_format, target_format) =
            gpux::block_on(async {
                let mut adapters = instance.enumerate_adapters(backend_policy).await;
                sort_adapters(&mut adapters, device_id_filter, None);
                let available = adapters
                    .iter()
                    .map(|adapter| {
                        let info = adapter.get_info();
                        format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type)
                    })
                    .collect::<Vec<_>>();

                for adapter in adapters {
                    let info = adapter.get_info();
                    if !selection.accepts(&info) {
                        continue;
                    }
                    let Some(target_format) = Self::headless_target_format(&adapter) else {
                        log::warn!(
                            "Adapter {:?} has no supported headless render target format",
                            info.name
                        );
                        continue;
                    };
                    match Self::create_device(&adapter).await {
                        Ok((device, queue, dual_source_blending, color_texture_format)) => {
                            #[cfg(feature = "bench-support")]
                            if info.device_type == wgpu::DeviceType::Cpu {
                                log::error!(
                                    "Headless renderer selected software adapter {:?}; benchmark \
                                     results measure CPU software rendering, not hardware GPU \
                                     rendering",
                                    info.name
                                );
                            }
                            return Ok((
                                adapter,
                                device,
                                queue,
                                dual_source_blending,
                                color_texture_format,
                                target_format,
                            ));
                        }
                        Err(error) => log::warn!(
                            "Failed to create a headless device for adapter {:?}: {error:#}",
                            info.name
                        ),
                    }
                }

                anyhow::bail!(
                    "No usable headless GPU adapter for {selection:?} with backend policy \
                     {backend_policy:?} ({}={:?}). Available adapters: [{}]. Windows always \
                     offers WARP; on Linux without a GPU, install Mesa's Vulkan drivers \
                     (lavapipe) and point VK_DRIVER_FILES at its ICD.",
                    HeadlessAdapterSelection::ENV_VAR,
                    std::env::var(HeadlessAdapterSelection::ENV_VAR).unwrap_or_default(),
                    available.join(", ")
                )
            })?;

        let device_lost = install_device_lost_callback(&device);
        let shared_errors = Some(DeviceErrorState::install(&device));
        let info = adapter.get_info();
        log::info!(
            "Selected headless GPU adapter: {:?} ({:?}, {:?})",
            info.name,
            info.backend,
            info.device_type
        );
        let backend = WgpuBackend::Native(info.backend);
        Ok((
            Self {
                instance,
                adapter,
                device: Arc::new(device),
                queue: Arc::new(queue),
                backend,
                dual_source_blending,
                color_texture_format,
                device_lost,
                shared_errors,
            },
            target_format,
        ))
    }

    /// The headless render target format: a non-sRGB 8-bit format, BGRA first like the
    /// desktop surfaces, that can be rendered to, copied out and multisampled at the fixed
    /// headless path sample count. Readback copies 4 bytes per pixel and only swizzles BGRA,
    /// so no other format may be added without updating it.
    #[cfg(all(
        not(target_family = "wasm"),
        any(test, feature = "bench-support", feature = "test-support")
    ))]
    fn headless_target_format(adapter: &wgpu::Adapter) -> Option<wgpu::TextureFormat> {
        let required_usages =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
        [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ]
        .into_iter()
        .find(|format| {
            let features = adapter.get_texture_format_features(*format);
            features.allowed_usages.contains(required_usages)
                && features
                    .flags
                    .sample_count_supported(HEADLESS_PATH_SAMPLE_COUNT)
        })
    }

    #[cfg(target_family = "wasm")]
    pub async fn new_web(
        canvas: &web_sys::HtmlCanvasElement,
        preference: WebBackendPreference,
    ) -> anyhow::Result<PreparedWebGraphics> {
        let backends = match preference {
            WebBackendPreference::Auto => wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            WebBackendPreference::WebGpu => wgpu::Backends::BROWSER_WEBGPU,
            WebBackendPreference::WebGl => wgpu::Backends::GL,
        };
        let descriptor = wgpu::InstanceDescriptor {
            backends,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: Some(Box::new(WebDisplaySource)),
        };
        let instance = if preference == WebBackendPreference::Auto {
            wgpu::util::new_instance_with_webgpu_detection(descriptor).await
        } else {
            wgpu::Instance::new(descriptor)
        };
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|error| {
                anyhow::anyhow!("Failed to create browser graphics surface: {error}")
            })?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "Failed to request a {preference:?} adapter compatible with the canvas: {error}"
                )
            })?;

        log::info!(
            "Selected GPU adapter: {:?} ({:?})",
            adapter.get_info().name,
            adapter.get_info().backend
        );

        let (device, queue, dual_source_blending, color_texture_format) =
            Self::create_device(&adapter).await?;
        let device_lost = install_device_lost_callback(&device);
        let backend = match adapter.get_info().backend {
            wgpu::Backend::BrowserWebGpu => WgpuBackend::BrowserWebGpu,
            wgpu::Backend::Gl => WgpuBackend::Gl,
            backend => anyhow::bail!("unexpected browser graphics backend {backend:?}"),
        };

        let context = Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            backend,
            dual_source_blending,
            color_texture_format,
            device_lost,
        };
        Ok(PreparedWebGraphics { context, surface })
    }

    async fn create_device(
        adapter: &wgpu::Adapter,
    ) -> anyhow::Result<(wgpu::Device, wgpu::Queue, bool, TextureFormat)> {
        let dual_source_blending = adapter
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING);

        let mut required_features = wgpu::Features::empty();
        if dual_source_blending {
            required_features |= wgpu::Features::DUAL_SOURCE_BLENDING;
        } else {
            log::warn!(
                "Dual-source blending not available on this GPU. \
                Subpixel text antialiasing will be disabled."
            );
        }

        let color_atlas_texture_format = Self::select_color_texture_format(adapter)?;

        let required_limits = required_limits(adapter.get_info().backend, adapter.limits());

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("gpui_device"),
                required_features,
                required_limits,
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create wgpu device: {e}"))?;

        Ok((
            device,
            queue,
            dual_source_blending,
            color_atlas_texture_format,
        ))
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn instance(display: Box<dyn wgpu::wgt::WgpuHasDisplayHandle>) -> wgpu::Instance {
        Self::instance_with_backend_policy(display, native_backend_policy())
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn instance_with_backend_policy(
        display: Box<dyn wgpu::wgt::WgpuHasDisplayHandle>,
        backend_policy: wgpu::Backends,
    ) -> wgpu::Instance {
        wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: backend_policy,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: Some(display),
        })
    }

    pub fn check_compatible_with_surface(&self, surface: &wgpu::Surface<'_>) -> anyhow::Result<()> {
        let caps = surface.get_capabilities(&self.adapter);
        if caps.formats.is_empty() {
            let info = self.adapter.get_info();
            anyhow::bail!(
                "Adapter {:?} (backend={:?}, device={:#06x}) is not compatible with the \
                 display surface for this window.",
                info.name,
                info.backend,
                info.device,
            );
        }
        Ok(())
    }

    /// Select an adapter and create a device, testing that the surface can actually be configured.
    /// This is the only reliable way to determine compatibility on hybrid GPU systems, where
    /// adapters may report surface compatibility via get_capabilities() but fail when actually
    /// configuring (e.g., NVIDIA reporting Vulkan Wayland support but failing because the
    /// Wayland compositor runs on the Intel GPU).
    #[cfg(not(target_family = "wasm"))]
    async fn select_adapter_and_device(
        instance: &wgpu::Instance,
        device_id_filter: Option<u32>,
        surface: &wgpu::Surface<'_>,
        compositor_gpu: Option<&CompositorGpuHint>,
        reject_software: bool,
        backend_policy: wgpu::Backends,
    ) -> anyhow::Result<(
        wgpu::Adapter,
        wgpu::Device,
        wgpu::Queue,
        bool,
        TextureFormat,
    )> {
        let mut adapters: Vec<_> = instance.enumerate_adapters(backend_policy).await;

        if adapters.is_empty() {
            anyhow::bail!("No GPU adapters found for backend policy {backend_policy:?}");
        }

        if let Some(device_id) = device_id_filter {
            log::info!("ZED_DEVICE_ID filter: {:#06x}", device_id);
        }

        sort_adapters(&mut adapters, device_id_filter, compositor_gpu);

        // Log all available adapters (in sorted order)
        log::info!(
            "Found {} GPU adapter(s) for backend policy {:?}:",
            adapters.len(),
            backend_policy
        );
        for adapter in &adapters {
            let info = adapter.get_info();
            log::info!(
                "  - {} (vendor={:#06x}, device={:#06x}, backend={:?}, type={:?})",
                info.name,
                info.vendor,
                info.device,
                info.backend,
                info.device_type,
            );
        }

        // Test each adapter by creating a device and configuring the surface
        for adapter in adapters {
            let info = adapter.get_info();

            if reject_software && info.device_type == wgpu::DeviceType::Cpu {
                log::info!(
                    "Skipping software renderer: {} ({:?})",
                    info.name,
                    info.backend
                );
                continue;
            }

            #[cfg(target_os = "android")]
            if info.backend == wgpu::Backend::Vulkan && info.device_type == wgpu::DeviceType::Cpu {
                log::info!(
                    "Skipping Android software Vulkan adapter: {} ({:?})",
                    info.name,
                    info.backend
                );
                continue;
            }

            log::info!("Testing adapter: {} ({:?})...", info.name, info.backend);

            match Self::try_adapter_with_surface(&adapter, surface).await {
                Ok((device, queue, dual_source_blending, color_atlas_texture_format)) => {
                    log::info!(
                        "Selected GPU (passed configuration test): {} ({:?})",
                        info.name,
                        info.backend
                    );
                    return Ok((
                        adapter,
                        device,
                        queue,
                        dual_source_blending,
                        color_atlas_texture_format,
                    ));
                }
                Err(e) => {
                    log::info!(
                        "  Adapter {} ({:?}) failed: {}, trying next...",
                        info.name,
                        info.backend,
                        e
                    );
                }
            }
        }

        anyhow::bail!(
            "No GPU adapter found that can configure the display surface for backend policy {backend_policy:?}"
        )
    }

    /// Try to use an adapter with a surface by creating a device and testing configuration.
    /// Returns the device and queue if successful, allowing them to be reused.
    #[cfg(not(target_family = "wasm"))]
    async fn try_adapter_with_surface(
        adapter: &wgpu::Adapter,
        surface: &wgpu::Surface<'_>,
    ) -> anyhow::Result<(wgpu::Device, wgpu::Queue, bool, TextureFormat)> {
        let caps = surface.get_capabilities(adapter);
        if caps.formats.is_empty() {
            anyhow::bail!("no compatible surface formats");
        }
        if caps.alpha_modes.is_empty() {
            anyhow::bail!("no compatible alpha modes");
        }

        let (device, queue, dual_source_blending, color_atlas_texture_format) =
            Self::create_device(adapter).await?;
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let test_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: caps.formats[0],
            width: 64,
            height: 64,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };

        surface.configure(&device, &test_config);

        let error = error_scope.pop().await;
        if let Some(e) = error {
            anyhow::bail!("surface configuration failed: {e}");
        }

        Ok((
            device,
            queue,
            dual_source_blending,
            color_atlas_texture_format,
        ))
    }

    fn select_color_texture_format(adapter: &wgpu::Adapter) -> anyhow::Result<wgpu::TextureFormat> {
        let required_usages = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let usable = |format| {
            adapter
                .get_texture_format_features(format)
                .allowed_usages
                .contains(required_usages)
        };
        if let Some(format) = preferred_color_atlas_format(adapter.get_info().backend, usable) {
            return Ok(format);
        }
        let bgra_features = adapter.get_texture_format_features(wgpu::TextureFormat::Bgra8Unorm);

        let rgba_features = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba8Unorm);
        if rgba_features.allowed_usages.contains(required_usages) {
            let info = adapter.get_info();
            log::warn!(
                "Adapter {} ({:?}) does not support Bgra8Unorm atlas textures with usages {:?}; \
                 falling back to Rgba8Unorm atlas textures.",
                info.name,
                info.backend,
                required_usages,
            );
            return Ok(wgpu::TextureFormat::Rgba8Unorm);
        }

        let info = adapter.get_info();
        Err(anyhow::anyhow!(
            "Adapter {} ({:?}, device={:#06x}) does not support a usable color atlas texture \
             format with usages {:?}. Bgra8Unorm allowed usages: {:?}; \
             Rgba8Unorm allowed usages: {:?}.",
            info.name,
            info.backend,
            info.device,
            required_usages,
            bgra_features.allowed_usages,
            rgba_features.allowed_usages,
        ))
    }
    pub fn supports_dual_source_blending(&self) -> bool {
        self.dual_source_blending
    }

    pub fn backend(&self) -> WgpuBackend {
        self.backend
    }

    /// Whether instance data goes through textures instead of storage buffers: on WebGL2, and on
    /// native devices whose vertex shaders cannot read storage buffers (GLES devices such as
    /// Android emulators).
    pub fn uses_webgl_instance_data(&self) -> bool {
        texture_instance_data(
            self.backend,
            self.adapter.get_downlevel_capabilities().flags,
        )
    }

    pub fn color_texture_format(&self) -> wgpu::TextureFormat {
        self.color_texture_format
    }

    /// Returns true if the GPU device was lost (e.g., due to driver crash, suspend/resume).
    /// When this returns true, the context should be recreated.
    pub fn device_lost(&self) -> bool {
        self.device_lost.load(Ordering::Relaxed)
    }

    /// The device-wide error state of a headless context; `None` for windowed contexts,
    /// whose renderers each install their own error callback.
    #[cfg(not(target_family = "wasm"))]
    pub fn shared_errors(&self) -> Option<&Arc<DeviceErrorState>> {
        self.shared_errors.as_ref()
    }

    /// Returns a clone of the device_lost flag for sharing with renderers.
    pub(crate) fn device_lost_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.device_lost)
    }
}

/// Sorts adapters into a single priority order. Tiers (from highest to lowest):
///
/// 1. ZED_DEVICE_ID match — explicit user override
/// 2. Compositor GPU match — the GPU the display server is rendering on
/// 3. Device type (Discrete > Integrated > Other > Virtual > Cpu).
///    "Other" ranks above "Virtual" because OpenGL seems to count as "Other".
/// 4. Backend — prefer primary native backends over GL/etc. on policies that allow both.
#[cfg(not(target_family = "wasm"))]
fn sort_adapters(
    adapters: &mut [wgpu::Adapter],
    device_id_filter: Option<u32>,
    compositor_gpu: Option<&CompositorGpuHint>,
) {
    adapters.sort_by_key(|adapter| {
        let info = adapter.get_info();

        // Backends like OpenGL report device=0 for all adapters, so
        // device-based matching is only meaningful when non-zero.
        let device_known = info.device != 0;

        let user_override: u8 = match device_id_filter {
            Some(id) if device_known && info.device == id => 0,
            _ => 1,
        };

        let compositor_match: u8 = match compositor_gpu {
            Some(hint)
                if device_known
                    && info.vendor == hint.vendor_id
                    && info.device == hint.device_id =>
            {
                0
            }
            _ => 1,
        };

        let type_priority: u8 = if info.device_type == wgpu::DeviceType::Cpu {
            4
        } else {
            match info.device_type {
                wgpu::DeviceType::DiscreteGpu => 0,
                wgpu::DeviceType::IntegratedGpu => 1,
                wgpu::DeviceType::Other => 2,
                wgpu::DeviceType::VirtualGpu => 3,
                wgpu::DeviceType::Cpu => 4,
            }
        };

        let backend_priority: u8 = match info.backend {
            wgpu::Backend::Vulkan | wgpu::Backend::Metal | wgpu::Backend::Dx12 => 0,
            _ => 1,
        };

        (
            user_override,
            compositor_match,
            type_priority,
            backend_priority,
        )
    });
}

#[cfg(not(target_family = "wasm"))]
fn device_id_filter() -> Option<u32> {
    match std::env::var("ZED_DEVICE_ID") {
        Ok(val) => parse_pci_id(&val)
            .context("Failed to parse device ID from `ZED_DEVICE_ID` environment variable")
            .log_err(),
        Err(std::env::VarError::NotPresent) => None,
        err => {
            err.context("Failed to read value of `ZED_DEVICE_ID` environment variable")
                .log_err();
            None
        }
    }
}

/// Installs the device-lost callback and returns the flag it raises.
fn install_device_lost_callback(device: &wgpu::Device) -> Arc<AtomicBool> {
    let device_lost = Arc::new(AtomicBool::new(false));
    device.set_device_lost_callback({
        let device_lost = Arc::clone(&device_lost);
        move |reason, message| {
            log::error!("wgpu device lost: reason={reason:?}, message={message}");
            if reason != wgpu::DeviceLostReason::Destroyed {
                device_lost.store(true, Ordering::Relaxed);
            }
        }
    });
    device_lost
}

/// The fixed path sample count of headless rendering, so captured images do not depend on
/// which multisample counts an adapter offers.
#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
pub(crate) const HEADLESS_PATH_SAMPLE_COUNT: u32 = 4;

#[cfg(not(target_family = "wasm"))]
fn parse_pci_id(id: &str) -> anyhow::Result<u32> {
    let mut id = id.trim();

    if id.starts_with("0x") || id.starts_with("0X") {
        id = &id[2..];
    }
    let is_hex_string = id.chars().all(|c| c.is_ascii_hexdigit());
    let is_4_chars = id.len() == 4;
    anyhow::ensure!(
        is_4_chars && is_hex_string,
        "Expected a 4 digit PCI ID in hexadecimal format"
    );

    u32::from_str_radix(id, 16).context("parsing PCI ID as hex")
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use super::parse_pci_id;

    #[test]
    fn test_parse_device_id() {
        assert!(parse_pci_id("0xABCD").is_ok());
        assert!(parse_pci_id("ABCD").is_ok());
        assert!(parse_pci_id("abcd").is_ok());
        assert!(parse_pci_id("1234").is_ok());
        assert!(parse_pci_id("123").is_err());
        assert_eq!(
            parse_pci_id(&format!("{:x}", 0x1234)).unwrap(),
            parse_pci_id(&format!("{:X}", 0x1234)).unwrap(),
        );

        assert_eq!(
            parse_pci_id(&format!("{:#x}", 0x1234)).unwrap(),
            parse_pci_id(&format!("{:#X}", 0x1234)).unwrap(),
        );
    }
}

/// The limits to request from an adapter. GL adapters (WebGL2, and GLES devices without Vulkan,
/// such as Android emulators) may have no compute stage, so they get the WebGL2 baseline, which
/// asks for none; other backends get the downlevel baseline. Both take the adapter's own
/// resolution and alignment limits.
pub(crate) fn required_limits(backend: wgpu::Backend, adapter: wgpu::Limits) -> wgpu::Limits {
    let baseline = if backend == wgpu::Backend::Gl {
        wgpu::Limits::downlevel_webgl2_defaults()
    } else {
        wgpu::Limits::downlevel_defaults()
    };
    baseline
        .using_resolution(adapter.clone())
        .using_alignment(adapter)
}

#[cfg(test)]
mod required_limits_tests {
    use super::required_limits;

    #[test]
    fn gl_adapters_without_compute_get_limits_they_can_meet() {
        // A GLES adapter without compute reports zero compute limits.
        let adapter = wgpu::Limits {
            max_compute_workgroups_per_dimension: 0,
            max_compute_invocations_per_workgroup: 0,
            ..wgpu::Limits::downlevel_webgl2_defaults()
        };
        let limits = required_limits(wgpu::Backend::Gl, adapter.clone());
        assert!(limits.check_limits(&adapter), "{limits:?}");
        assert_eq!(limits.max_compute_workgroups_per_dimension, 0);
    }

    #[test]
    fn other_backends_get_the_downlevel_baseline() {
        let adapter = wgpu::Limits::default();
        let limits = required_limits(wgpu::Backend::Vulkan, adapter);
        assert_eq!(
            limits.max_compute_workgroups_per_dimension,
            wgpu::Limits::downlevel_defaults().max_compute_workgroups_per_dimension
        );
    }
}

/// Whether a backend reads instance data from textures instead of storage buffers: WebGL2
/// always, and native devices whose vertex shaders cannot read storage buffers (GLES devices such
/// as Android emulators).
pub(crate) fn texture_instance_data(backend: WgpuBackend, flags: wgpu::DownlevelFlags) -> bool {
    match backend {
        WgpuBackend::Gl => true,
        WgpuBackend::BrowserWebGpu | WgpuBackend::Native(_) => {
            !flags.contains(wgpu::DownlevelFlags::VERTEX_STORAGE)
        }
    }
}

/// The color atlas format a backend prefers among the `usable` ones, or `None` when only the
/// logged Rgba8Unorm fallback is left. On GL, BGRA textures come from an ES extension that some drivers report but
/// cannot sample (Android emulators'); RGBA is core, so GL uses it, as WebGL must. Elsewhere BGRA
/// matches the platforms' native image layout.
pub(crate) fn preferred_color_atlas_format(
    backend: wgpu::Backend,
    usable: impl Fn(wgpu::TextureFormat) -> bool,
) -> Option<wgpu::TextureFormat> {
    if backend == wgpu::Backend::Gl && usable(wgpu::TextureFormat::Rgba8Unorm) {
        return Some(wgpu::TextureFormat::Rgba8Unorm);
    }
    usable(wgpu::TextureFormat::Bgra8Unorm).then_some(wgpu::TextureFormat::Bgra8Unorm)
}

#[cfg(test)]
mod backend_choice_tests {
    use super::{WgpuBackend, preferred_color_atlas_format, texture_instance_data};
    use wgpu::{Backend, DownlevelFlags, TextureFormat};

    #[test]
    fn devices_without_vertex_storage_read_instances_from_textures() {
        let without = DownlevelFlags::empty();
        let with = DownlevelFlags::VERTEX_STORAGE;
        assert!(texture_instance_data(WgpuBackend::Gl, with));
        assert!(texture_instance_data(
            WgpuBackend::Native(Backend::Gl),
            without
        ));
        assert!(!texture_instance_data(
            WgpuBackend::Native(Backend::Gl),
            with
        ));
        assert!(!texture_instance_data(
            WgpuBackend::Native(Backend::Vulkan),
            with
        ));
        assert!(!texture_instance_data(WgpuBackend::BrowserWebGpu, with));
    }

    #[test]
    fn gl_prefers_rgba_atlases_and_other_backends_bgra() {
        let both = |_| true;
        assert_eq!(
            preferred_color_atlas_format(Backend::Gl, both),
            Some(TextureFormat::Rgba8Unorm)
        );
        assert_eq!(
            preferred_color_atlas_format(Backend::Vulkan, both),
            Some(TextureFormat::Bgra8Unorm)
        );
        let only_bgra = |format| format == TextureFormat::Bgra8Unorm;
        assert_eq!(
            preferred_color_atlas_format(Backend::Gl, only_bgra),
            Some(TextureFormat::Bgra8Unorm)
        );
        let only_rgba = |format| format == TextureFormat::Rgba8Unorm;
        assert_eq!(
            preferred_color_atlas_format(Backend::Metal, only_rgba),
            None
        );
    }
}
