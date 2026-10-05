## Hypothesis

Windows currently reports subpixel text support unconditionally even when its WGPU renderer did not negotiate `DUAL_SOURCE_BLENDING`. Capturing that negotiated renderer capability in `WindowsWindowState` and returning it from `is_subpixel_rendering_supported` will prevent an unsupported LCD request from selecting an RGB glyph path.

## Single mechanism

Snapshot `WgpuRenderer::supports_dual_source_blending()` when constructing the Windows window state, then use that snapshot for the platform capability query. Do not change glyph cache keys, raster parameters, shader code, sampling, transfer, or layout.

## Scope and limitation

The requested validation matrix is WASM-only. The change is therefore checked by the equivalent WASM grayscale path and a focused source-level capability invariant, not by a Windows runtime claim.
