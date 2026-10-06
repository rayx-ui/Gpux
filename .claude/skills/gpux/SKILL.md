---
name: gpux
description: Maintain, install, and use gpux, the RayX-maintained GPUI fork in this repository (Gpux). Use when a task needs to update gpux directly from upstream Zed (the GPUI closure or a single Zed PR), gpui-mobile, or a new wgpu release (the temporary `gpux-wgpu` fork in `vendor/wgpu`), preserve or replay gpux/RayX patches from local history, preserve source provenance markers, maintain crate boundaries and the crates.io packaging of the `gpux-*` crates, set up gpux as a Rust dependency, create or validate multithreaded WASM/browser gpux apps, install GPUI/gpux prerequisites on Windows/macOS/Linux/WASM/Android, fix local build tooling such as fxc, Clang, VC++ redistributables, wasm-bindgen, or run gpux validation.
---

# gpux

## Overview

Use this skill for gpux maintenance, installation, and usage tasks that depend on the repository's GPUI-only workspace, crate naming, packaging shape, upstream provenance rules, and platform prerequisites.

Run from the Gpux repository root; Cargo commands use the root workspace `Cargo.toml` directly, without `--manifest-path`, unless a referenced workflow explicitly changes directories. RayX (`../RayX`, checked out beside Gpux) is the main consumer and the runtime-proof host: Test Suite runs, xtask builds, and RayX memory updates happen from `../RayX`.

Packages publish to crates.io under `gpux-*` names but keep their GPUI Rust names through `[lib] name`, so source code still writes `gpui_platform::`, `collections::`, and so on. `cargo -p` selectors take package names (`-p gpux-platform`, `-p gpux-wgpu-renderer`, `-p gpux-collections`); the [README crate table](../../../README.md#crates) is the authoritative directory/package/Rust-name mapping.

gpux memory lives in this repository's `.agentx/memory/` (moved from RayX on 2026-10-05). For repository maintenance tasks, start at `.agentx/memory/knowledge/INDEX.md` and `.agentx/memory/decisions/INDEX.md`, then read only the matching domain (`gpui`, `renderer`, `platform`, `repository`); `repository/upstream-sync` records the represented upstream commits.

RayX memory records only RayX's consumer view. For RayX-side work, read the narrowest matching RayX memory:

- `../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/state.md` for how RayX consumes gpux.
- `../RayX/.agentx/memory/knowledge/tooling/validation/state.md` for build/test orchestration.
- `../RayX/.agentx/memory/knowledge/ui/theming-components/state.md` for RayX component source copied from RayX.
- `../RayX/.agentx/memory/knowledge/ui/virtualization/state.md` for RayX virtualization integration.

## Task Routing

- For installing prerequisites, configuring a machine, adding `gpux` to a Rust project, running examples, or validating a local setup, read [references/installation-and-usage.md](references/installation-and-usage.md). It includes Rust, `wasm-bindgen`, WebGPU browser flags, XcodeGen, Android SDK/NDK, and cargo-ndk routing.
- For Windows setup, including Windows ARM64, `fxc.exe`, Visual Studio Build Tools, Clang, VC++ redistributables, WebGPU browser flags, or automatic installs, also read [references/windows-prerequisites.md](references/windows-prerequisites.md). Use [scripts/install-windows-prereqs.ps1](scripts/install-windows-prereqs.ps1) for check-first or automatic Windows setup.
- For macOS, Linux, or WASM/browser setup, also read [references/macos-linux-wasm-prerequisites.md](references/macos-linux-wasm-prerequisites.md).
- For creating, fixing, or validating browser gpux apps, especially RayX WASM apps, read [references/multithreaded-wasm-apps.md](references/multithreaded-wasm-apps.md). RayX browser apps must use multithreaded `gpui_web` by default.
- For fixed/sticky positioning, keyed sticky stacks, `StickyStacking`, ordinary `div()` candidates, or low-level List/UniformList candidates, read [references/sticky-positioning.md](references/sticky-positioning.md).
- For `ScrollActivity`, `ScrollBehavior`, programmatic Div/List/UniformList scrolling, retargeting, or direct-input cancellation, read [references/programmatic-scroll.md](references/programmatic-scroll.md).
- For upstream updates from `zed-industries/zed`, read [references/update-from-zed-gpui-closure.md](references/update-from-zed-gpui-closure.md).
- For importing a specific upstream Zed pull request into gpux, read [references/update-from-zed-pr.md](references/update-from-zed-pr.md) first, then use the GPUI closure update reference for the full import mechanics.
- For upstream `gpui_mobile` updates from `itsbalamurali/gpui-mobile`, read [references/update-from-gpui-mobile.md](references/update-from-gpui-mobile.md).
- For moving to a new wgpu release, maintaining the temporary `gpux-wgpu` fork in `vendor/wgpu/`, or retiring that fork, read [references/update-from-wgpu.md](references/update-from-wgpu.md).
- For crates.io packaging, crate naming, or manifest changes, use the README crate table and the manifest shape in [references/update-from-zed-gpui-closure.md](references/update-from-zed-gpui-closure.md#manifest-and-packaging-shape), then run `cargo package --workspace`.
- For ordinary code changes, use gpux's own memory domains and expand only to the narrow matching subdomain; consult RayX memory only for RayX consumer behavior.

## Validation

Test only what you changed. From the Gpux repository root:

```bash
cargo fmt --check
cargo check -p <package> --all-targets
cargo test -p <package> <filter>
cargo clippy -p <package> --all-targets --no-deps -- -D warnings
cargo check -p gpux-web --target wasm32-unknown-unknown --no-default-features
cargo ndk -t arm64-v8a check -p gpux-mobile    # with ANDROID_NDK_HOME set
cargo package --workspace                      # manifest, dependency, or included-file changes
```

The web check disables default features only because the default multithreaded build needs nightly `-Z build-std`, which RayX's xtask provides; RayX browser apps stay multithreaded. Runtime proof (headed WASM/WebGPU, native Windows DX12, mobile) runs in RayX's Test Suite from `../RayX`, for example `cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu --no-default-features --features rayx_diagnostics --playwright-test test_suite/<spec>`, plus the native and mobile xtask routes (`app apps/test_suite run windows`, `app apps/test_suite build android --debug`). RayX forbids `apps/lab` as a testing or diagnostics target.

## Core Rules

### UI-thread input snapshots

`KeymapSnapshot` retains boxed actions and is deliberately UI-thread state. Retain it with `Rc`,
capture one immutable snapshot plus map revision at input-sequence start, and pass that same pair
through completion. Do not change it to `Arc` unless every contained action becomes `Send + Sync`;
doing so triggers unsound cross-thread assumptions.

### Input model

gpux uses upstream GPUI's input model unchanged: `MouseDown`/`MouseUp`/`MouseMove`/`ScrollWheel`
events, raw `Touch` events recognized by `TouchGestureRecognizer` (tap, pan with fling momentum,
long press, touch drag), `ClickEvent::Touch`, and a single active drag with platform drag-out
handoff. Platforms emit raw touches and choose fling physics through `Platform::gestures`. Do not
reintroduce a gpux pointer layer (`PointerEvent`, per-pointer ownership, gesture maps, touch
momentum in `ScrollActivity`, or extra `ScrollWheelEvent` metadata).

- Keep gpux scoped to this repository's copy of Zed's GPUI crate closure unless the user explicitly changes repository scope.
- Preserve Zed GPUI closure paths (Zed `crates/<name>/` is Gpux `crates/<name>/`) and support crate topology, but keep the main framework package/import crate named `gpux` and the other packages named `gpux-*` with their GPUI Rust names in `[lib] name`.
- Fetch or shallow-clone `https://github.com/zed-industries/zed` into a temporary/cache location for upstream updates; do not require a local Zed fork checkout.
- Import only GPUI and GPUI-dependent crates from upstream Zed; do not import the full Zed application tree.
- Fetch or shallow-clone `https://github.com/itsbalamurali/gpui-mobile.git` into a temporary/cache location for `gpui_mobile` updates, then merge its repository root into `crates/gpui_mobile/`.
- For a new wgpu release, recreate the `gpux-wgpu` fork in `vendor/wgpu/` from the published crates.io release with [scripts/fork_wgpu.py](scripts/fork_wgpu.py) `<version> <fork-suffix>` (the suffix is the fork number `N` in `<version>-gpux.<N>`), which also reapplies the DX12 change and bumps the workspace pin; reapply the DX12 change by hand if the script reports that upstream changed the swap-chain description. Then port gpux to the new API. There is no vendored Zed wgpu repository.
- Preserve or replay RayX/gpux GPUI patches from gpux's own history on top of imported upstream closure state.
- For Zed GPUI closure imports, use `references/update-from-zed-gpui-closure.md` as the accumulated RayX patch-surface checklist, including browser/WASM DOM surfaces, DevKit semantics, fixed/sticky positioning, shader/frame effects, and Windows WGPU/DX12 behavior.
- For `gpui_mobile` imports, use `references/update-from-gpui-mobile.md` as the accumulated mobile patch-surface checklist, including Android Java/JNI naming, native activity/SurfaceView lifecycle, backend policy, safe areas, frame scheduling, input metadata, package helpers, and iOS Metal/TextSystem behavior.
- Keep the `gpux-wgpu` fork to exactly one code change: DX12 swap chains for `SurfaceTarget::WndHandle` use `DXGI_SCALING_NONE` (`vendor/wgpu/wgpu-hal/src/dx12/mod.rs`). HTML island element upload is gpux code in `crates/gpui_wgpu/src/html_island_upload.rs`, not a wgpu patch. Retire the fork and return to crates.io `wgpu` once upstream ships `SurfaceConfiguration::scaling_mode`, tracked in [docs/upstream/wgpu-dx12-swapchain-scaling.md](../../../docs/upstream/wgpu-dx12-swapchain-scaling.md).
- Take every dependency from crates.io. Git dependencies and `[patch]` entries cannot reach consumers of the published crates, so they are forbidden; Zed's own forks come from their crates.io releases (`zed-reqwest`, `zed-scap`, `zed-font-kit`, `zed-xim`).
- Record fork provenance compactly in the fork manifests' header comment and description and in `docs/upstream/`. Add a dedicated update reference only when the update/replay process is genuinely complex or recurring, not merely because one crate is patched.
- Prefer upstreaming gpux dependency patches and returning to crates.io over permanently maintaining a fork or vendored tree.
- Keep every crate publishable: `gpux-*` package names with GPUI Rust names in `[lib] name`, workspace metadata, a `description`, no `publish = false`, workspace dependency keys that keep the Rust name with `package = "gpux-..."`, path-only dev-dependencies on the `gpui_macros` -> `gpux` and `gpux` -> `gpui_platform`/`gpui_web` cycle edges, and no file included from outside its crate (shared fonts come from `gpux_fonts`). Validate manifest changes with `cargo package --workspace`.
- Every gpux crate is Apache-2.0 (`gpux-fonts` is OFL-1.1; the `gpux-wgpu` fork keeps wgpu's `MIT OR Apache-2.0`). Never re-import GPL/AGPL license files or license fields from Zed or gpui-mobile.
- Keep gpux's registry Taffy dependency on the measured minimum feature set: `std`, `taffy_tree`, `flexbox`, `grid`, and `block_layout`. Gpux derives scroll content extents from child layout bounds and does not consume Taffy's feature-gated scrollable-overflow rectangle, so keep `content_size` disabled along with `flexbox_balance`, `float_layout`, `calc`, and `detailed_layout_info` unless a concrete gpux requirement passes fresh layout/runtime proof.
- Keep gpux's pooled Taffy context slots empty between frames: take tracked measurement closures during clear, mark those nodes dirty, and refill retained slots on measured reuse. The empty slots are bounded by the per-window node pool; never retain frame-local closures or captured application state.
- Treat stable measured-leaf results as an optimization lead, not an implicit cache contract. The warmed wide DataTable repeats all 882 measured calls with identical inputs and snapped outputs at both tested DPRs, and diagnostics attribute every call to text rather than list, uniform-list, or custom measurement. Keep any optimization text-owned unless another workload proves a generic need, and retain closure execution until a caller-visible invalidation key or generation proves cached intrinsic content is current.
- Preserve `TextLayout`'s existing local-size reuse path: it serves 562 of the warmed wide DataTable's 882 text callbacks at both tested DPRs. Before adding a content/style key that every callback must build, measure cache-tier hits and actual platform shaping inside the remaining 320 full text-layout executions.
- Do not target glyph shaping for the warmed wide DataTable: all 320 full text layouts hit the wrapped-layout cache at both tested DPRs (19 current-frame, 301 previous-frame, zero misses), so unwrapped line layout and platform shaping are both zero. Measure previous-frame lookup/promotion or sound upper-layer invalidation instead.
- Preserve the ordinary line-layout cache's prehashed-key contract: compute the deterministic full-field Fx hash once for each borrowed request, retain it in owned keys, and hash only that scalar during current lookup, previous removal, and promotion. Equality must still compare every identity field after the hash check, and rollover plus unused-entry eviction must remain exact. This cut warmed full-key hashing from 921 to 320 calls without adding a crate, raw-entry API, map layer, or unbounded state.
- Do not add a multi-width `TextLayout` local cache for the warmed wide DataTable: the 320 full layouts partition into 229 empty first-use states (one per measured leaf) and 91 first wrap-width transitions, with no truncation invalidation or observed transition back. Investigate cross-frame ephemeral-state reconstruction and wrapped-result materialization instead.
- Size `TextSystem::shape_text` decoration-run vectors from `min(input_text_run_count, 32)`, retaining ordinary growth for complex lines. The warmed workload uses exactly 320 slots for 320 live runs after this change instead of 10,240 reserved slots; do not restore a fixed 32-slot allocation.
- Keep font-run scratch in the shared thread-local pool used by all four text-system callers. Borrow the pool only during pop/push so nested calls remain safe; retained vectors per thread are bounded by maximum concurrent outstanding calls and keep their maximum observed capacity. Do not restore the removed `TextSystem` mutex, which cost 640 warmed acquisitions per frame.
- Acquire a pooled `LineWrapperHandle` only after `TextLayout` has established that truncation is active. The warmed non-truncating path executes 320 full layouts and otherwise pays 320 unused wrapper checkouts plus 320 returns, each locking the shared wrapper pool; keep all existing truncation algorithms and wrapper reuse unchanged.
- Preserve gpux's per-window Taffy layout-node pool and exact ordered child topology across frames. Compare retained child IDs during reuse and call `set_children` only on change; track current-frame child membership in a capacity-retaining set so computed roots detach stale prior parents after topology shrinkage. Bound both the node pool and child-membership set by the window's maximum observed tree and drop them with the window.
- For pooled Taffy nodes, skip `set_style` only when the freshly converted current style exactly equals the retained Taffy style; do not infer equality from pool position or element identity.
- Taffy node-request diagnostics show a completely warm wide DataTable at both DPRs: 1,123/1,123 pooled nodes, unchanged converted styles, and unchanged ordered topologies; 229 context refills need zero extra dirty marks; all 1,041 active-child inserts are unique. Do not target new-node allocation, duplicate child filtering, or redundant dirty calls for that workload. Investigate a sound style/layout generation before bypassing conversion or equality, and do not repeat the regressive parallel topology shadow.
- Keep request-to-prepaint style reuse private to gpux-owned Div, Img, Svg, and UniformList implementations. Their borrowed layout adapters retain the already-computed request `Style` in the window-owned draw-local slot, consume it during prepaint, and replace the same slot with the final style for paint. Preserve the public consuming-callback path for external custom `Interactivity` users, plus every focus/hover/active/group gate, sticky/listener recomputation, inspector behavior, drag-time paint path, and draw-boundary clear. The warmed wide DataTable reuses exactly 761 gpux-owned request styles (each Test Suite sidebar navigation item adds three) while all 13 eligible external consumers still recompute; do not broaden this into a public or cross-frame cache.
- In renderer paint paths, prepare the original `Color` or `Background` directly when inherited element opacity is exactly `1.0`. Preserve the established authored-opacity path for every non-identity factor; do not rebuild an immutable color and its lazy caches for an identity transform.
- In GPUI arenas, register a type-erased drop record only when `std::mem::needs_drop::<T>()` is true. Trivial allocations still live in arena chunks and participate in generation invalidation, but clear must not traverse no-op destructor thunks.
- Keep equal-key sprite order stable. For the hot, densely disordered 128-byte monochrome sprite family, use stable cached-key sorting on `(DrawOrder, TileId)` so sorting operates on compact keys before permuting records. Do not apply the extra allocation/code path to cold or empty sprite families without separate measurements.
- Keep `GlobalElementId` on direct `Arc<[ElementId]>` path ownership and compute its cached identity with the existing deterministic `collections::FxHasher`; full path equality remains the collision-safe identity contract. Keep reuse diagnostics hash-only and behind `rayx_diagnostics`, and do not add retained production interning state without a separately measured design that improves both throughput and tail latency.
- Keep upstream Zed commit provenance in import commit bodies and local gpux patch provenance in replay/preservation commit bodies.
- Audit platform/rendering sibling crates and shader files during imports; RayX patches are not limited to `crates/gpui`.
- On macOS Metal shader fixes, preserve packed Rust buffer ABIs and convert cbindgen-exported array fields to Metal vector types in shader helpers at use sites.
- Keep platform text capability answers tied to negotiated renderer features. Windows may expose subpixel text only when `WgpuRenderer` negotiated dual-source blending; snapshot that immutable capability during window construction instead of borrowing the renderer during paint.
- Update gpux memory (`.agentx/memory/knowledge/repository/upstream-sync/state.md` and the affected domain) when source hashes, latest imported upstream Zed or `gpui_mobile` commit, the forked wgpu release, multithreaded WASM rules, or replay rules change.
- Keep public usage examples on `gpux::{...}` and the package name `gpux`, while preserving the repository path `crates/gpui`.
- Keep RayX browser/WASM apps multithreaded by default. Do not disable `gpui_web` default features unless a task explicitly proves single-threaded output is required.
- Bridge browser paste from the trusted DOM `paste` event into the active GPUI
  input handler and cache the same plain-text payload for platform clipboard
  reads; do not depend on an async browser clipboard read or keydown alone.
- Build and validate multithreaded browser behavior through Test Suite orchestration from `../RayX`, such as `cargo run -p xtask -- app apps/test_suite build wasm` or `cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu`; do not route RayX WASM startup through obsolete .NET/browser bridge flows or automate Lab.
- For gpux `div().fixed()`, resolve insets against the current content-mask containing block and preserve that mask through hitboxes, paint, and children. Use top-layer APIs or `Deferred::unclipped()` for intentional viewport/global overlays that must escape parent clipping.
- Do not import unrelated RayX solution/browser bridge tooling into gpux setup docs or install flows.

## Shader Effect Quality

For WGPU visual-effect work, keep blur and refraction behavior physically plausible enough that demos do not look like low-resolution post-process artifacts.

DO:

```rust
// Keep blur sources full-resolution, or use an explicitly filtered downsample path.
blur_color: Self::create_target(device, key, "effect_blur_color"),
```

```rust
// Keep glass-blur and Liquid Glass as separate effect responsibilities.
effect_frame()
    .effect(backdrop_blur(px(118.0)).high_quality())
    .child(content);

effect_frame()
    .effect(glass().blur(px(0.0)).refraction(0.08))
    .child(content);
```

```rust
// Render glow in a separate additive color pass and preserve destination alpha.
blend: Some(visual_effect_glow_blend_state()),
```

```wgsl
// Model Liquid Glass from a squircle SDF and scale sample coordinates inward.
let d = liquid_glass_sd_squircle(p, 3.0, 1.0);
let dist = -d;
let scale = pow(max(liquid_glass_refract(max(dist, 0.0)), 0.001), 1.779);
let sample_local = center + (local_position - center) * scale;
```

```wgsl
// The source-replacement pass should ignore glow; the additive pass returns only glow energy.
case VISUAL_EFFECT_KIND_GLOW: {}
return vec4<f32>(glow_color, glow_alpha);
```

```wgsl
// Dawn rejects compound assignment into a swizzle; rebuild the vector instead.
glass_color = vec4<f32>(glass_color.rgb + grain_rgb, glass_color.a);
```

DON'T:

```rust
// A nearest-sampled half-size copy makes high-contrast backdrops look like mosaics.
blur_color: Self::create_sized_target(
    device,
    WgpuEffectTargetKey::new(key.width / 2, key.height / 2, key.format),
    "effect_blur_color",
),
```

```wgsl
// Edge-only bevel displacement creates hard rectangular bands on card-like glass.
let edge_alpha = 1.0 - smoothstep(0.0, edge_width, inward_distance);
let refracted_position = input.texture_position - edge_normal * edge_alpha * strength;
```

```wgsl
// Do not composite glow by returning sampled source color from an expanded glow quad.
return vec4<f32>(source.rgb + glow.rgb * alpha, max(source.a, alpha));
```

```wgsl
// This parses in some mental models, but not in browser WebGPU.
glass_color.rgb += grain_rgb;
```

```rust
// Do not wire the circular glass-blur demo through Liquid Glass.
effect_frame()
    .rounded(px(999.))
    .effect(glass().blur(px(118.0)))
    .child(content);
```

## Frame Family Elements

Use `frame()` for lightweight geometry, transforms, opacity, and inherited rounded clipping. Use `effect_frame()` for shader effects and `screen_effect_region(...)`. Use `animated_frame(id)` when frame-owned properties should interpolate across target changes. Use `transition_frame(id)` when a stable region should replace one child state with another through a retained screen-effect snapshot. Frame-family elements are intentionally single-child; wrap a `div()` inside them when you need flex/grid, text styling, event handling, or multiple children.

DO:

```rust
frame()
    .rounded(px(24.0))
    .clip()
    .child(content);

effect_frame()
    .rounded(px(24.0))
    .clip()
    .effect(backdrop_blur(px(32.0)))
    .screen_effect_region("story-content-transition")
    .child(content);

animated_frame("profile-card")
    .target_key(if expanded { "expanded" } else { "compact" })
    .duration(Duration::from_millis(220))
    .easing(ease_in_out)
    .h(if expanded { px(148.0) } else { px(72.0) })
    .rounded(if expanded { px(22.0) } else { px(12.0) })
    .child_with_progress(move |progress, _, cx| {
        render_profile_card(expanded, progress, cx)
    });

transition_frame("details-card")
    .target_key(details_state.id())
    .transition(FrameTransition::ripple_reveal().duration_ms(420))
    .screen_effect_region("details-card")
    .rounded(px(24.0))
    .clip()
    .child(render_details(details_state));
```

DON'T:

```rust
div()
    .rounded(px(24.0))
    .effect(backdrop_blur(px(32.0)))
    .screen_effect_region("story-content-transition");

animated_frame(format!("profile-card-{}", cx.frame_count()))
    .child(content);

transition_frame(row_index)
    .target_key(row_index)
    .child(content);
```

`Styled`/`div()` do not own shader effects. `animated_frame(id)` and `transition_frame(id)` need stable semantic ids, not per-render, positional, or row-index keys. `transition_frame()` is image-space replacement, not automatic tree diffing or shared-element morphing. Keep rounded clipping at the content-mask/renderer level so child subtrees and effects inherit the same clip.

## Installation Automation Rules

- Detect the host OS, architecture, package managers, Rust toolchain, and missing commands before installing anything.
- If the user asks to install or fix prerequisites, run a check-only pass first, then run the smallest automatic install step that addresses the missing toolchain pieces.
- If the user asks only for instructions, give commands without executing system-level installers.
- Prefer idempotent package manager commands and PATH updates; report when a restart, terminal restart, browser restart, or elevated installer prompt is expected.
- On Windows ARM64, account for both the `ring` Clang requirement and both ARM64 and x64 VC++ redistributables.
- For .NET Android or mobile validation failures such as `XA5300`, recommend Microsoft Build of OpenJDK 21 LTS, verify `java`, `javac`, and `JAVA_HOME`, and pass `-p:JavaSdkDirectory="$env:JAVA_HOME"` if .NET still cannot discover the JDK.
