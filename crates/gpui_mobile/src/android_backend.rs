/// Which GPU back-end to use for an Android wgpu surface.
///
/// Vulkan is the default preference because it is the native high-performance
/// Android GPU path for wgpu. The default policy still includes GL-ES as a
/// fallback for Android images where Vulkan cannot initialize, such as emulator
/// profiles that expose GLES but report `vulkan_renderengine: false`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum AndroidBackend {
    #[default]
    Vulkan,
    Gles,
}

impl AndroidBackend {
    pub fn wgpu_backends(self) -> gpui_wgpu::wgpu::Backends {
        match self {
            Self::Vulkan => gpui_wgpu::wgpu::Backends::VULKAN | gpui_wgpu::wgpu::Backends::GL,
            Self::Gles => gpui_wgpu::wgpu::Backends::GL,
        }
    }

    /// The backends a window tries, one surface each, in order. Android's Vulkan loader connects
    /// the native window when it creates a Vulkan surface (`vkCreateAndroidSurfaceKHR`), and only
    /// destroying that surface releases it, so a GL surface on the same window fails ("already
    /// connected") while a Vulkan surface exists. A surface shared by both backends therefore
    /// never falls back to GL when Vulkan is present but unusable, such as an emulator's software
    /// SwiftShader Vulkan, which gpux skips.
    pub fn surface_attempts(self) -> &'static [gpui_wgpu::wgpu::Backends] {
        match self {
            Self::Vulkan => &[
                gpui_wgpu::wgpu::Backends::VULKAN,
                gpui_wgpu::wgpu::Backends::GL,
            ],
            Self::Gles => &[gpui_wgpu::wgpu::Backends::GL],
        }
    }

    pub fn wgpu_backend_policy_label(self) -> &'static str {
        match self {
            Self::Vulkan => "VULKAN|GL",
            Self::Gles => "GL",
        }
    }
}

impl std::fmt::Display for AndroidBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vulkan => write!(f, "Vulkan"),
            Self::Gles => write!(f, "OpenGL ES"),
        }
    }
}

#[cfg(test)]
mod surface_attempt_tests {
    use super::AndroidBackend;
    use gpui_wgpu::wgpu::Backends;

    #[test]
    fn vulkan_and_gl_get_a_surface_each_vulkan_first() {
        assert_eq!(
            AndroidBackend::Vulkan.surface_attempts(),
            [Backends::VULKAN, Backends::GL]
        );
        assert_eq!(AndroidBackend::Gles.surface_attempts(), [Backends::GL]);
    }

    #[test]
    fn the_attempts_cover_exactly_the_backend_policy() {
        for backend in [AndroidBackend::Vulkan, AndroidBackend::Gles] {
            let attempts = backend.surface_attempts();
            assert_eq!(
                attempts
                    .iter()
                    .fold(Backends::empty(), |all, attempt| all | *attempt),
                backend.wgpu_backends(),
                "{backend}"
            );
            assert!(
                attempts
                    .iter()
                    .all(|attempt| attempt.bits().count_ones() == 1),
                "{backend}: one backend per surface"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AndroidBackend;

    #[test]
    fn android_backend_maps_to_wgpu_backend_policy() {
        assert_eq!(
            AndroidBackend::Vulkan.wgpu_backends(),
            gpui_wgpu::wgpu::Backends::VULKAN | gpui_wgpu::wgpu::Backends::GL
        );
        assert_eq!(
            AndroidBackend::Gles.wgpu_backends(),
            gpui_wgpu::wgpu::Backends::GL
        );
    }

    #[test]
    fn android_backend_policy_labels_match_wgpu_masks() {
        assert_eq!(
            AndroidBackend::Vulkan.wgpu_backend_policy_label(),
            "VULKAN|GL"
        );
        assert_eq!(AndroidBackend::Gles.wgpu_backend_policy_label(), "GL");
    }
}
