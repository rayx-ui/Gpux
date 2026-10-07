//! Backend ids: short names for the adapter a capture came from, used in golden file names and
//! result details.

use gpux::GpuSpecs;

/// The adapter family a capture came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendId {
    /// Mesa's Vulkan software rasterizer.
    Lavapipe,
    /// Mesa's OpenGL software rasterizer.
    Llvmpipe,
    /// Windows' DX12 software rasterizer (Microsoft Basic Render Driver).
    Warp,
    /// Apple Metal.
    Metal,
    /// Chromium's SwiftShader behind WebGPU.
    SwiftShaderWebGpu,
    /// A hardware adapter: never compared against committed goldens.
    Hardware(String),
}

impl BackendId {
    /// Classifies a GPU from its specs.
    pub fn from_specs(specs: &GpuSpecs) -> Self {
        let name = specs.device_name.to_ascii_lowercase();
        let backend = specs.backend.to_ascii_lowercase();
        if name.contains("llvmpipe") || name.contains("lavapipe") {
            if backend.contains("vulkan") {
                Self::Lavapipe
            } else {
                Self::Llvmpipe
            }
        } else if name.contains("microsoft basic render driver") {
            Self::Warp
        } else if name.contains("swiftshader") {
            Self::SwiftShaderWebGpu
        } else if backend.contains("metal") {
            Self::Metal
        } else {
            Self::Hardware(specs.device_name.clone())
        }
    }

    /// The id used in golden names, or `None` for hardware adapters.
    pub fn golden_id(&self) -> Option<&'static str> {
        match self {
            Self::Lavapipe => Some("lavapipe"),
            Self::Llvmpipe => Some("llvmpipe"),
            Self::Warp => Some("warp"),
            Self::Metal => Some("metal"),
            Self::SwiftShaderWebGpu => Some("swiftshader-webgpu"),
            Self::Hardware(_) => None,
        }
    }

    /// A short label for logs and result lines.
    pub fn label(&self) -> String {
        match self {
            Self::Hardware(name) => format!("hardware ({name})"),
            other => other.golden_id().unwrap_or_default().to_string(),
        }
    }
}
