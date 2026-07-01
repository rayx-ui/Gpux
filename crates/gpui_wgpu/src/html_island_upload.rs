use gpux::{DevicePixels, Size};
use wasm_bindgen::JsCast;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlIslandUploadPath {
    CopyElementImageToTexture,
    IntermediateCanvas,
}

impl HtmlIslandUploadPath {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CopyElementImageToTexture => "copyElementImageToTexture",
            Self::IntermediateCanvas => "intermediate-canvas",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtmlIslandUploadResult {
    pub path: HtmlIslandUploadPath,
    pub size: Size<DevicePixels>,
    pub texture_reallocated: bool,
    pub source_sample: [u8; 4],
}

pub fn upload_html_island_element_to_texture(
    queue: &wgpu::Queue,
    element: &web_sys::HtmlElement,
    texture: &wgpu::Texture,
    size: Size<DevicePixels>,
) -> anyhow::Result<HtmlIslandUploadResult> {
    let width = u32::try_from(size.width.0.max(0))
        .map_err(|_| anyhow::anyhow!("HTML island width exceeds u32"))?;
    let height = u32::try_from(size.height.0.max(0))
        .map_err(|_| anyhow::anyhow!("HTML island height exceeds u32"))?;

    anyhow::ensure!(
        width > 0 && height > 0,
        "HTML island upload requires non-zero dimensions"
    );

    let canvas = element
        .parent_element()
        .ok_or_else(|| anyhow::anyhow!("HTML island upload element has no parent canvas"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|error| anyhow::anyhow!("HTML island upload parent is not a canvas: {error:?}"))?;
    let context = canvas
        .get_context("2d")
        .map_err(|error| anyhow::anyhow!("Failed to query 2D context: {error:?}"))?
        .ok_or_else(|| anyhow::anyhow!("2D context unavailable for HTML island upload"))?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .map_err(|error| anyhow::anyhow!("Context is not CanvasRenderingContext2D: {error:?}"))?;
    let data = context
        .get_image_data(0., 0., width as f64, height as f64)
        .map_err(|error| anyhow::anyhow!("Failed to read HTML island upload canvas: {error:?}"))?
        .data()
        .0;
    let source_sample = sample_center_pixel(&data, width, height);
    let bytes_per_row = width * 4;
    let padded_bytes_per_row = bytes_per_row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let upload_data = if padded_bytes_per_row == bytes_per_row {
        data
    } else {
        let mut padded = vec![0; padded_bytes_per_row as usize * height as usize];
        for row in 0..height as usize {
            let src_offset = row * bytes_per_row as usize;
            let dst_offset = row * padded_bytes_per_row as usize;
            padded[dst_offset..dst_offset + bytes_per_row as usize]
                .copy_from_slice(&data[src_offset..src_offset + bytes_per_row as usize]);
        }
        padded
    };

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &upload_data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(padded_bytes_per_row),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    Ok(HtmlIslandUploadResult {
        path: HtmlIslandUploadPath::IntermediateCanvas,
        size,
        texture_reallocated: false,
        source_sample,
    })
}

fn sample_center_pixel(data: &[u8], width: u32, height: u32) -> [u8; 4] {
    let x = width.saturating_sub(1) / 2;
    let y = height.saturating_sub(1) / 2;
    let offset = ((y * width + x) * 4) as usize;
    if offset + 4 <= data.len() {
        [
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]
    } else {
        [0, 0, 0, 0]
    }
}
