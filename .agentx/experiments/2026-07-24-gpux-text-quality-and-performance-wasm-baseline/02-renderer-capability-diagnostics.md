## Diagnostics

- `WgpuRenderer::supports_dual_source_blending()` exposes the negotiated device feature.
- `WindowsWindowState` snapshots that capability at renderer construction.
- `WindowsWindow::is_subpixel_rendering_supported()` now returns the snapshot instead of an unconditional `true`.
- The equivalent WASM platform still returns `false`, preserving its grayscale route.
- The focused WASM fixture reported WebGPU and completed at DPR 1 and DPR 2. Its final viewport capture remains readable in light and dark specimens.

No Windows runtime diagnostic was run, by explicit target constraint.
