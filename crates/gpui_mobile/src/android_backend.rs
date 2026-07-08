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
