# 01: Metal renderer on the `PaintColor` paint contract

Branch: Gpux `fix/metal-paint-contract` (commits `3ad5fca`, `5321a25`), closed PR
[rayx-ui/Gpux#1](https://github.com/rayx-ui/Gpux/pull/1).

## The Original Failure

CI run [37500248162](https://github.com/rayx-ui/Gpux/actions/runs/37500248162), job
`check (macos-latest)`, step `cargo check --workspace --all-targets`. The `gpux-apple` build script
(`crates/gpui_apple/build.rs`) failed while compiling the Metal shaders:

```
scene.h:33:18: error: typedef redefinition with different types ('uint32_t' vs 'MonochromeSpriteKind')
scene.h:470:3: error: unknown type name 'PaintColor'      (also 489:3 and 553:3)
shaders.metal:499:5: error: type 'const constant Shadow *' is not valid for attribute 'buffer'
(the same error for Underline and MonochromeSprite)
```

## Root Cause

The upstream import 25dbe34 only *moved* `shaders.metal` from `gpui_macos` to `gpui_apple`,
unchanged. The real break is **cb4130b** (2026-07-13, "perf(gpux): move paint pipeline to linear
colors"). It moved every scene color from `Hsla` to `PaintColor` (premultiplied, gamma-encoded sRGB
since d5823fe) and ported only the WGPU renderer. macOS has been broken since then; CI simply had no
macOS job before. The break had three layers, each hidden behind the previous one:

1. **Generated header (`scene.h`).**
   - cbindgen parsed only `crates/gpui/src/{scene,geometry,color,window,platform}.rs` and
     `metal_renderer.rs`. `PaintColor` lives in `crates/gpux_color/src/color/paint.rs`, so it was
     never defined.
   - `MonochromeSpriteKind` (gpux-only, added in 063ce0c) was `#[repr(u32)]`. For sized enums in C
     mode, cbindgen 0.28 emits `enum X {...}; typedef uint32_t X;`
     (`cbindgen-0.28.0/src/bindgen/ir/enumeration.rs`, `write_tag_enum`). That is legal C, but Metal
     is C++-based and rejects it as a typedef redefinition. `#[repr(C)]` enums get
     `typedef enum X {...} X;`, which Metal accepts; `AtlasTextureKind` and `BorderStyle` already
     prove that.
2. **Shaders.** Five `hsla_to_rgba(...)` calls now received `PaintColor`. The output was
   straight-alpha while the paint is premultiplied.
3. **Rust (`metal_renderer.rs`).** Five type errors: `pack_background` took `&Background` (now
   `PreparedBackground`), `PackedBackground.solid` was `Hsla`, and gradient stops were re-prepared
   from authored colors.

## What The Branch Changes

- `crates/gpui_apple/build.rs`: also parses `gpux_color`'s `src/color/paint.rs`, found through
  `gpux_color::GPUX_COLOR_MANIFEST_DIR` (a new `#[doc(hidden)]` static in
  `crates/gpux_color/src/lib.rs`, mirroring `gpux::GPUX_MANIFEST_DIR`). It exports `PaintColor`
  instead of `Hsla`. `gpux-apple` gains `gpux_color` as an Apple-only build-dependency.
  `Cargo.lock` changed by one line.
- `crates/gpui/src/scene.rs`: `MonochromeSpriteKind` is `#[repr(C)]` (with a comment explaining
  why). Layout and discriminants are unchanged: 4 bytes on every gpux target, and the WGPU
  compile-time asserts on `MonochromeSprite` offsets still pass.
- `crates/gpui_apple/src/shaders.metal`, mirroring WGSL in `crates/gpui_wgpu/src/shaders.wgsl`:
  - `paint_color_value(PaintColor)` replaces `hsla_to_rgba`.
  - Every fragment returns premultiplied color, and `over()` is premultiplied source-over.
  - Gradient dither scales with alpha (no alpha dither).
  - Pattern and checkerboard backgrounds scale the whole color.
  - OKLab stops go through `premultiplied_oklab_to_srgb` (unpremultiply, convert, clamp, then
    premultiply).
  - `border_edge_coverage` takes the straight border color.
  - Dashed borders fade the whole premultiplied color (see file 03 for the matching WGPU bug).
  - Polychrome sprites treat the atlas sample as straight alpha and premultiply it, as WGSL does.
  - Shadows honor `inset`, `element_bounds` and `element_corner_radii`, as WGSL does. Metal ignored
    inset shadows before.
- `crates/gpui_apple/src/metal_renderer.rs`:
  - `PackedBackground.solid: PaintColor`.
  - `pack_background(&PreparedBackground)`; `pack_gradient_stop(&PreparedLinearColorStop)` copies
    `stop.color`.
  - `build_pipeline_state` blends premultiplied: RGB `One`/`OneMinusSourceAlpha`, alpha
    `One`/`OneMinusSourceAlpha`. That matches WGPU's `PREMULTIPLIED_ALPHA_BLENDING`; it was
    `SourceAlpha` for RGB and `One` for destination alpha. The path-sprite and path-rasterization
    pipelines were already premultiplied and are unchanged.
  - New `#[cfg(test)]` group `paint_packing_*`, two tests.
- `3ad5fca` removes two `.clone()` calls on `ContentMask`, which is `Copy`; clippy `clone_on_copy`
  fails them on Apple targets.

Expected visual result: solid fills, text, images, borders and shadows are mathematically identical
to the pre-July Metal renderer. Premultiplying in the shader and blending with `One` equals the old
straight-alpha `SourceAlpha` blend. Intended differences, all matching WGPU:

- translucent gradients interpolate premultiplied, as CSS specifies;
- inset shadows render;
- transparent windows get standard source-over alpha instead of accumulating to 1.

## What Is Already Proven

- Real macOS CI run [37520997553](https://github.com/rayx-ui/Gpux/actions/runs/37520997553)
  (PR #1): the `gpux-apple` build script, including the real `xcrun metal` and `metallib` compile,
  **succeeded**, and `gpux-macos`/`gpux-platform` started checking. The job then failed only in
  `gpux-zed-util` (file 02).
- From Windows:
  - the cbindgen config reproduced CI's header errors at the same line and column, and the new
    header passes `clang++ -std=c++14`;
  - `shaders.metal` type-checked against a stub Metal prelude;
  - `cargo check` and `clippy -D warnings` pass for `gpux-apple` on `aarch64-apple-darwin` and
    `aarch64-apple-ios` (with a placeholder metallib, because the build script is host-gated);
  - `cargo check` passes for `gpux-macos` (all targets) and every macOS workspace library except
    those needing a C cross-compiler;
  - Windows check and clippy pass, the Linux check passes in WSL, and so does
    `cargo package --workspace`.

## What Still Needs A Mac

1. Get `cargo check --workspace --all-targets` green. It is blocked by file 02 and maybe more
   crates behind it.
2. `cargo test -p gpux-apple paint_packing` (the new tests).
   `cargo clippy -p gpux-apple --all-targets --no-deps -- -D warnings`.
3. Runtime proof, since this is a rendering change. Run RayX's Test Suite natively on macOS
   (`cargo run -p xtask -- app apps/test_suite run host --no-default-features --features rayx_diagnostics,devkit`
   from `../RayX`, with the `rayx-devkit-mcp` sidecar per RayX `diagnostics.md`). Inspect at least:
   - `/rendering/srgb-compositing` (translucent colors over backdrops must match the WGPU and Figma
     values that `rayx-srgb-compositing.spec.ts` encodes);
   - `/rendering/border-antialiasing` (memory rule: do not call the Metal border-antialiasing port
     done until this passes on macOS);
   - `/rendering/dashed-borders` (exists only on RayX branch `test/dashed-border-gaps`; gaps must
     show the backdrop or fill);
   - pages with gradients (OKLab and sRGB), inset and drop shadows, text, SVG icons, images and
     emoji;
   - a transparent window, if Test Suite has one.

   Compare against the same pages on WebGPU (`cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu ...`).
4. Optional: `cargo test -p gpux-platform visual_test -- --ignored --test-threads=1` exercises
   `MetalHeadlessRenderer` (see `crates/gpui_platform/src/gpui_platform.rs` tests).

## Things To Double-Check On Real Hardware

- **Polychrome atlas alpha.** The shader assumes atlas images and emoji are straight alpha, which
  is what WGSL assumes. If emoji or images look dark-fringed or too bright at edges, check what
  `gpui_macos`'s text system and image upload store (premultiplied versus straight).
- **Monochrome sprites** ignore `MonochromeSpriteKind`. Metal applies no gamma correction to glyphs,
  as upstream Zed doesn't; WGSL corrects only `Glyph`. That is fine unless macOS text looks off.
- **Border edge coverage.** Metal hard-codes the WGPU defaults (`BORDER_GAMMA_RATIOS`, contrast
  1.0) in `quad_fragment`. This port has never been seen on screen.
- **The `runtime_shaders` feature** stitches `scene.h` and `shaders.metal` together at build time.
  Check it once:
  `cargo check -p gpux-apple --features runtime_shaders`, then run something.
- **iOS shares `gpux-apple`.** If an iOS target is available, try
  `cargo check -p gpux-apple --target aarch64-apple-ios` (the build script handles the
  iphoneos/iphonesimulator SDKs).

## Follow-Ups After It Is Proven

- **Gpux memory.** Update `.agentx/memory/knowledge/platform/desktop/state.md`,
  `knowledge.md` and `hypotheses.md` (the Metal border port and the paint contract are now
  compiled and run; drop "not compiled or run"). Add a knowledge item: shared enums in
  Metal-exported records must be `#[repr(C)]`, because cbindgen's sized-enum typedef is a C++
  redefinition. Also update the decision record
  `.agentx/memory/decisions/renderer/wgpu/2026-09-30-renderer-wgpu-srgb-compositing.md`, which
  says the Metal port is not yet compiled or proven. Run
  `python ~/.claude/skills/agentx/scripts/audit_agentx_memory.py` afterwards.
- **Import guide.** `.claude/skills/gpux/references/update-from-zed-gpui-closure.md`, "RayX Patch
  Surface To Preserve" → "Native Rendering". Add a bullet: the Metal renderer consumes
  premultiplied encoded-sRGB `PaintColor`; cbindgen parses `gpux_color`'s `paint.rs`;
  Metal-exported enums stay `repr(C)`; and upstream Zed's `shaders.metal` still uses `Hsla`, so do
  not take it wholesale.
