# 03: WGPU dashed borders rendered solid

Branches: Gpux `fix/wgpu-dashed-border-gaps` (`c6b6512`, `e7149e7`) and RayX
`test/dashed-border-gaps` (`f1a9be7b`). Closed PRs:
[rayx-ui/Gpux#2](https://github.com/rayx-ui/Gpux/pull/2) and
[rayx-ui/RayX#1](https://github.com/rayx-ui/RayX/pull/1).

**This fix is complete and runtime-proven. Only merging is left.** It does not need a Mac, but its
macOS CI job cannot be green until file 01 (and 02) land on `main`.

## Bug

In `crates/gpui_wgpu/src/shaders.wgsl`, `fs_quad` faded dashed borders with
`border_color.a *= dash_alpha(...)` at two sites (in the `max_t >= 1.0` branch and the short
straight-border branch). Since cb4130b, `border_color` is **premultiplied** encoded sRGB. Scaling
only alpha left the premultiplied RGB in each gap, and `over()`, which is premultiplied source-over,
added it on top of the fill or backdrop. With a transparent quad background, a gap output
`(border.rgb, 0)`, which premultiplied blending adds straight onto the framebuffer. So dashed
borders rendered nearly solid on WebGPU, WebGL2 and native WGPU. That shader is shared:
`shaders.wgsl` is concatenated into the storage-buffer, WebGL and subpixel variants. RayX's chart
tooltips (`crates/rayx_components/rayx_components_core/src/charts/plot/tooltip.rs`) use dashed
borders, so production UI was affected.

## Fix

- Both sites use `border_color *= dash_alpha(...)`. `border_straight_color` (the input to
  `border_edge_coverage`) is still computed before the fade.
- The regression test `dashed_border_gaps_fade_the_whole_premultiplied_color` lives in
  `wgpu_renderer.rs` tests. It is a shader-source assertion, because upstream Zed's straight-alpha
  shader fades alpha alone and an import could bring that back.
- Memory (`e7149e7`): `renderer/wgpu/rules.md` gains "Fade or mask premultiplied paint by scaling
  all four channels"; `knowledge.md` records the regression and the proof.
- The Metal port in file 01 already fades the whole color, so both renderers agree.

## Proof (all done)

- RayX Test Suite page `/rendering/dashed-borders`
  (`apps/test_suite/src/pages/rendering_dashed_borders.rs`): unfilled white, filled accent, and
  rounded white 4 px dashed borders on `#0a0a0a`.
- Spec `tools-node/test_suite/rayx-dashed-borders.spec.ts`. It samples each top border's centre
  line on WebGPU and on forced WebGL2 (`?backend=webgl`), at DPR 1 and 2. Gaps must match the
  backdrop or fill, dashes the border colour, and no pixel may be brighter than both. It also does a
  console and page-error crash smoke.
- Against Gpux `main`, the spec failed all 4 runs (gap fraction 0; the white boxes rendered as
  solid rectangles). With the fix all 4 pass, and so do `rayx-border-antialiasing` and
  `rayx-webgl-fallback` (10/10).
- `cargo test -p gpux-wgpu-renderer --lib`: 58 passed, including naga validation of every shader
  variant. clippy `-D warnings`, the `gpux-web` wasm check and fmt pass.
- Not run separately: native Windows DX12. It uses the same storage-buffer WGSL.

## What Is Left

1. CI on PR #2: `check (macos-latest)` failed only because `main` lacks files 01 and 02.
   `test (windows)` failed once on an unrelated flaky test (file 04). A re-run was started and its
   result is unknown.
2. After 01 and 02 merge into `main`, merge `main` into `fix/wgpu-dashed-border-gaps` (merge, do not
   rebase or force-push), push, reopen or recreate the Gpux PR, and wait for green CI.
3. Merge the Gpux PR and RayX `test/dashed-border-gaps` together. The RayX spec fails against a
   Gpux checkout without `c6b6512`, because RayX builds against the sibling `../Gpux` checkout by
   path.
