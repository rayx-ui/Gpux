---
agentx:
  kind: state
  scope: knowledge/renderer/color
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - gpux-color
    - paint-color
    - color-standards
    - hsla-cache
  supersedes: []
  superseded_by: []
---
# Color State

## Purpose And Usage

Read before changing `gpux_color`, color conversion or output policies, `PaintColor`, or how GPUI converts authored colors.

## Current Technical Shape

`gpux_color` has no production dependencies. `Color` is an immutable `Arc`-backed canonical XYZ D65 handle with conversions to 15 encoded spaces, parsing, interpolation, explicit output policies, lazy conversion caches shared by clones, and normalized semantic equality and hashing. `Color::paint_color()` caches one `PaintColor`: a `#[repr(C)]` 16-byte value of four `f32` holding clipped, premultiplied, gamma-encoded sRGB under the sole `PaintGamutPolicy::ClipSrgb`.

GPUI persistent style, text, background, border, shadow, gradient, sprite, path, and effect state stores `Color`; scene construction prepares `PaintColor`, and WGPU consumes it directly. Repeated `Hsla` inputs go through a bounded (1,024-entry) thread-local memoization cache in `crates/gpui/src/color.rs`, so identical authored values share one immutable `Color` and its caches. Consumers (RayX themes and components) persist `gpux::Color` directly with no adapters.

## Backlinks

- [Color module](../../../../../crates/gpux_color/src/color/mod.rs)
- [PaintColor](../../../../../crates/gpux_color/src/color/paint.rs)
- [GPUI color conversion and Hsla cache](../../../../../crates/gpui/src/color.rs)
- 2026-07-13 current [RayX decision: gpux_color as the canonical Color owner](../../../../../../RayX/.agentx/memory/decisions/runtime/workspace/2026-07-13-runtime-workspace-gpux-color-owner.md)
- 2026-09-30 current [Encoded sRGB compositing](../../../decisions/renderer/wgpu/2026-09-30-renderer-wgpu-srgb-compositing.md)
