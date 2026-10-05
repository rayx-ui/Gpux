# Update From wgpu

Use this reference when moving gpux to a new wgpu release, maintaining the temporary `gpux-wgpu` fork in `vendor/wgpu/`, or retiring that fork once upstream wgpu ships DX12 swap-chain scaling control.

Run from the Gpux repository root. The upstream source is the published crates.io wgpu release (repository `https://github.com/gfx-rs/wgpu`); destination paths are under `vendor/wgpu/`. There is no vendored Zed wgpu repository and no `[patch]` anywhere: RayX's root manifest and xtask's generated web manifests no longer patch wgpu, and every consumer reaches the fork through the gpux crates.

## Guardrails

- Start from a clean worktree or separate branch.
- Fork only a published crates.io wgpu release, pinned with `=<version>`. Never fork a git branch, Zed's wgpu fork, or a local checkout.
- Fork exactly seven crates: `wgpu-hal` and the six crates that depend on it (`wgpu`, `wgpu-core`, `wgpu-core-deps-apple`, `wgpu-core-deps-emscripten`, `wgpu-core-deps-wasm`, `wgpu-core-deps-windows-linux-android`). `wgpu-types`, `naga`, `naga-types`, and `wgpu-naga-bridge` stay on crates.io because they do not depend on `wgpu-hal`.
- Keep exactly one code change: DX12 window swap-chain scaling in `vendor/wgpu/wgpu-hal/src/dx12/mod.rs`. Every other difference from the published source is the mechanical manifest transformation below; change nothing else in the fork.
- Keep HTML island element upload in gpux (`crates/gpui_wgpu/src/html_island_upload.rs`); never move it back into wgpu.
- Keep the fork refresh and the gpux port as separate commits so provenance stays reviewable.
- Retire the fork instead of refreshing it once a wgpu release ships `SurfaceConfiguration::scaling_mode` (see Retiring The Fork).

## Current Fork

- Forked release: wgpu 30.0.1, fork version `30.0.1-gpux.1`.
- Each fork crate lives in a flat directory `vendor/wgpu/<crate>`: `vendor/wgpu/wgpu`, `vendor/wgpu/wgpu-core`, `vendor/wgpu/wgpu-hal`, and `vendor/wgpu/wgpu-core-deps-{apple,emscripten,wasm,windows-linux-android}`.
- The seven fork crates are gpux workspace members and publish with gpux. The root `Cargo.toml` depends on `wgpu = { package = "gpux-wgpu", path = "vendor/wgpu/wgpu", version = "=30.0.1-gpux.1" }`, so gpux source keeps writing `wgpu::`.

Each fork crate is the published crate source, obtained with `cargo vendor --versioned-dirs` in a scratch project that depends on `wgpu = "=<version>"` with the `webgl` feature. Its normalized `Cargo.toml` gets exactly this transformation:

- a header comment explaining the fork;
- package name `gpux-<name>` (the `[lib] name` keeps the upstream Rust name, such as `wgpu_hal`);
- version `<version>-gpux.<N>`;
- a description prefixed with `Temporary gpux fork of <name> <version> (DX12 window swap chains use DXGI_SCALING_NONE, gfx-rs/wgpu#10196).`;
- repository `https://github.com/rayx-ui/Gpux`;
- forked dependencies rewritten to `package = "gpux-<dep>"`, `version = "=<fork version>"`, `path = "../<dep>"`;
- examples, tests, benches, and all dev-dependencies removed.

`.cargo-checksum.json`, `.cargo_vcs_info.json`, `Cargo.toml.orig`, and `Cargo.lock` are dropped. Everything else stays as published, including wgpu's `MIT OR Apache-2.0` license and license files.

The only code change, in `vendor/wgpu/wgpu-hal/src/dx12/mod.rs`, sets the swap-chain scaling: `SurfaceTarget::WndHandle` swap chains use `DXGI_SCALING_NONE`, and every other target keeps `DXGI_SCALING_STRETCH`.

```rust
// gpux: window swap chains present unscaled, so resizing never stretches
// the previous frame (https://github.com/gfx-rs/wgpu/issues/10196).
Scaling: if matches!(&self.target, SurfaceTarget::WndHandle(_)) {
    Dxgi::DXGI_SCALING_NONE
} else {
    Dxgi::DXGI_SCALING_STRETCH
},
```

## Workflow

1. Read current provenance:
   - `docs/upstream/wgpu-dx12-swapchain-scaling.md`; if a wgpu release ships `SurfaceConfiguration::scaling_mode`, retire the fork instead.
   - The header comment and `version` of `vendor/wgpu/wgpu/Cargo.toml` for the forked release and fork version.
   - `../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/state.md`
   - `../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/rules.md`
   - `../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/knowledge.md`
   - `../RayX/.agentx/memory/decisions/runtime/gpux-foundation/2026-07-02-runtime-gpux-foundation-html-island-wgpu-direct-upload.md`
   - `../RayX/.agentx/memory/decisions/runtime/gpux-foundation/2026-07-08-runtime-gpux-foundation-windows-wgpu-dx12-policy.md`
   - `../RayX/.agentx/memory/decisions/runtime/gpux-foundation/2026-07-08-runtime-gpux-foundation-windows-wgpu-dxgi-scaling.md`
   - `git log --format='%h%n%B' -- vendor/wgpu`

2. Choose the release:
   - Pick an exact published wgpu version and read its changelog for changes to instance and surface creation, `Queue`/`Texture` and their `as_webgpu()` handles, WGSL/Naga validation, limits, and the backend features gpux enables.
   - When a Zed GPUI closure import needs a newer wgpu API, move the fork to a crates.io release that provides it; never take Zed's wgpu git revision.
   - A new release starts at fork version `<version>-gpux.1`; bump `<N>` only when the fork itself changes without a new upstream release.

3. Recreate the fork:
   - From the Gpux root, run `python .claude/skills/gpux/scripts/fork_wgpu.py <version> <fork-suffix>` ([scripts/fork_wgpu.py](../scripts/fork_wgpu.py)), where the suffix is the fork number `N`: `30.0.1 1` produces `30.0.1-gpux.1`. The script vendors the release into a temporary directory, replaces `vendor/wgpu/` with the seven fork crates and the manifest transformation above (including deletions), reapplies the DX12 change, and updates the root `Cargo.toml` `wgpu` dependency to `version = "=<fork version>"`.
   - If the script stops because the DX12 swap-chain description changed upstream, reapply the scaling change in `vendor/wgpu/wgpu-hal/src/dx12/mod.rs` by hand. Either way, confirm it is the only code difference from the published source.
   - Commit with a subject like `build(wgpu): fork wgpu <version>`.
   - Include commit body lines:

```text
Source crate: wgpu <version> (crates.io)
Source repository: https://github.com/gfx-rs/wgpu
Fork version: <version>-gpux.<N>
```

4. Port gpux to the new API:
   - Update `crates/gpui_wgpu` and the platform crates that use wgpu directly (`gpui_windows`, `gpui_linux`, `gpui_web`, `gpui_mobile`), keeping every behavior under gpux Surface To Preserve.
   - Keep the `naga` dev-dependency in `crates/gpui_wgpu/Cargo.toml` on the release's `naga` version.
   - Commit the port separately from the fork refresh.

5. Reconcile dependencies and locks:
   - Regenerate `Cargo.lock` only after the manifests are settled.
   - Confirm the lockfile has no git sources and that `wgpu-types`, `naga`, `naga-types`, and `wgpu-naga-bridge` resolve from crates.io at the release's versions.

6. Validate:
   - Run `cargo metadata --no-deps`.
   - Run `cargo fmt --check`.
   - Run `cargo check -p gpux-wgpu-renderer --all-targets`.
   - Run targeted checks for affected packages such as `gpux-windows`, `gpux-platform`, and `gpux`, plus `cargo check -p gpux-web --target wasm32-unknown-unknown --no-default-features`.
   - Run `cargo package --workspace`, because the fork crates publish with gpux.
   - For WebGPU API changes, run the headed WASM/WebGPU specs that exercise them from `../RayX`: `cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu --no-default-features --features rayx_diagnostics --playwright-test test_suite/<spec>`.
   - For DX12 or surface changes, run the native Windows Test Suite app from `../RayX` and verify the renderer/backend diagnostics report DX12; manually test fast height-only live resizing when the change touches surface scaling or resize behavior.
   - For Android/iOS platform-dependency changes, run `cargo ndk -t arm64-v8a check -p gpux-mobile` with `ANDROID_NDK_HOME` set, and the target-specific xtask mobile build/check path from `../RayX` when the toolchain is available.

## gpux Surface To Preserve

RayX memories identify these wgpu-related behaviors as easy to lose when moving to a new release:

- The single fork change: `SurfaceTarget::WndHandle` swap chains use `DXGI_SCALING_NONE`; composition, visual, and `SwapChainPanel` targets keep `DXGI_SCALING_STRETCH` unless a new decision changes that.
- The fork's manifest transformation, including the `=<fork version>` pins between fork crates and in the root `Cargo.toml`.
- No `[patch]` and no git sources: not in gpux, not in RayX's root manifest, and not in xtask's generated web manifests.
- HTML island element upload in `crates/gpui_wgpu/src/html_island_upload.rs`: a wasm-bindgen structural binding to Chrome's experimental `GPUQueue.copyElementImageToTexture`, called on the handles from `wgpu::Queue::as_webgpu()` and `wgpu::Texture::as_webgpu()`.
- The element-copy dictionary shape required by Chrome Canary: source `{ source }`, destination `{ destination: { texture, mipLevel, origin, aspect }, colorSpace, premultipliedAlpha }`, and size `{ width, height, depthOrArrayLayers }`.
- The explicit error for element-image copies outside the WebGPU backend, so unsupported backends fail explicitly.
- Windows backend policy: native Windows WGPU validation proves DX12, not Vulkan or GL.

GOOD:

```text
Fork the published wgpu <version> crates into vendor/wgpu/ with scripts/fork_wgpu.py, confirm the DX12 WndHandle scaling change is the only code change, then port gpux to the new API in a separate commit.
```

BAD:

```text
Point wgpu at a git branch or add a [patch] for the new release, or copy only wgpu-hal/src/dx12/mod.rs into the old fork and leave the other fork crates and version pins on the previous release.
```

Check:

- `git diff -- vendor/wgpu`
- `rg -n "DXGI_SCALING_NONE|copyElementImageToTexture|as_webgpu|gpux-wgpu" vendor/wgpu/wgpu-hal/src/dx12/mod.rs crates/gpui_wgpu/src/html_island_upload.rs Cargo.toml`
- `rg -n "^\[patch|git = " -g Cargo.toml` finds nothing.
- `cargo metadata --no-deps`
- `cargo package --workspace`
- Target-specific WASM WebGPU, Windows DX12, and mobile checks when the touched paths affect those targets.

## Retiring The Fork

The fork exists only until upstream wgpu ships `SurfaceConfiguration::scaling_mode` (`SurfaceScalingMode::OneToOne` maps DX12 window swap chains to `DXGI_SCALING_NONE`), tracked in [docs/upstream/wgpu-dx12-swapchain-scaling.md](../../../../docs/upstream/wgpu-dx12-swapchain-scaling.md) (gfx-rs/wgpu#10196). When a release ships it:

- Delete `vendor/wgpu/`, remove the seven fork crates from the workspace members, and depend on the crates.io release (`wgpu = "<version>"`).
- In the surface configuration of `WgpuRenderer` (`crates/gpui_wgpu/src/wgpu_renderer.rs`), request `scaling_mode: wgpu::SurfaceScalingMode::OneToOne` when the surface's `SurfaceCapabilities::scaling_modes` contains it, and `Auto` otherwise; requesting an unlisted mode fails validation. gpui_windows renders through `gpui_wgpu`, and only DX12 window surfaces list `OneToOne`.
- Remove the fork rows and notes from `README.md`, `CLAUDE.md`, and `AGENTS.md`, the fork sections of this reference and `SKILL.md`, and `scripts/fork_wgpu.py`.
- Validate as above, including the native Windows fast height-only live-resize check.

## Durable Memory

After a completed fork refresh, port, or retirement:

- Update `../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/state.md` with the forked wgpu release, fork version, and any changed current renderer behavior.
- Update `../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/knowledge.md` only for durable facts that are not obvious from state.
- Update `../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/rules.md` only if the fork or port rule changed.
- Add a decision in RayX memory if the fork scope, its source, or its retirement condition changes.
- Run the AgentX audit script from `../RayX` after memory changes.

## Review Checklist

- The fork came from the published crates.io wgpu release and was recreated with `scripts/fork_wgpu.py`.
- `vendor/wgpu/` holds exactly the seven fork crates with the manifest transformation, and the DX12 `WndHandle` scaling change is the only code change.
- The root `wgpu` dependency and every fork-internal dependency pin `=<fork version>`; no `[patch]` or git source exists.
- HTML island direct element-to-texture upload still builds and validates on headed WASM/WebGPU.
- Windows WGPU uses DX12 and preserves non-stretch HWND scaling unless a new decision supersedes that rule.
- `cargo package --workspace` passes.
- Commit bodies record the forked release and fork version.
- Validation results are recorded in the final response or PR notes.
