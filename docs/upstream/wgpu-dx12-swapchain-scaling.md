# Upstream: DX12 swap chain scaling

The `gpux-wgpu` fork in `vendor/wgpu` exists only because wgpu creates DX12 window swap chains
with `DXGI_SCALING_STRETCH`. This file tracks the upstream change that makes the fork unnecessary.

| | |
|---|---|
| Upstream issue | [gfx-rs/wgpu#10196](https://github.com/gfx-rs/wgpu/issues/10196) (swap chain scaling control) |
| Upstream change | `Add SurfaceConfiguration::scaling_mode`, commit `6ee057f81` |
| Branch | [`rayx-ui/wgpu` `refactor/surface-scaling-mode`](https://github.com/rayx-ui/wgpu/tree/refactor/surface-scaling-mode), with trunk `d7220bf2f` merged in (`dcb2b7e26`, not pushed yet) |
| Pull request | Not opened yet |

The change follows the design a wgpu maintainer proposed on the issue: a scaling mode on
`SurfaceConfiguration`, the supported modes in `SurfaceCapabilities`, validation in wgpu-core, and
an `Auto` default that chooses stretching when the surface supports it and otherwise keeps the
platform's behavior. The issue itself reports Vulkan on Intel Arc, which this change does not
implement, so the pull request is "part of" the issue rather than a fix for it.

When a wgpu release ships `SurfaceConfiguration::scaling_mode`: delete `vendor/wgpu`, depend on the
crates.io `wgpu`, and in the surface configuration of `WgpuRenderer`
(`crates/gpui_wgpu/src/wgpu_renderer.rs`) request `wgpu::SurfaceScalingMode::OneToOne` when the
surface's `SurfaceCapabilities::scaling_modes` contains it, and `Auto` otherwise. Only DX12 window
surfaces report `OneToOne`, which matches what the fork does today.

## wgpu rules that apply

- **Hand-written description.** `CONTRIBUTING.md` ("LLMs (AI)") and the PR template ask that PR
  descriptions be written by the author, not pasted from an LLM. The notes below are the facts to
  write from, not text to paste.
- **LLM disclosure.** The template's "LLM Use?" section must say how LLMs were used, and its hidden
  comment asks agent-created PRs to add two checklist lines (below) and fill them in accurately.
- **No AI authorship in commits.** The `ai-commit-check` action rejects commits authored by an AI
  or carrying AI trailers (`Co-Authored-By`, ...). `6ee057f81` is authored by `dXnSix` with no
  trailers; keep every commit on the branch that way.
- **Changelog.** Entries need the PR number. CI runs `cargo xtask changelog origin/trunk`.
- **Merging.** Mergify squash-merges by default; the `merge: rebase` label keeps commits as they
  are. Open the PR as ready for review: reviewers are assigned to non-draft PRs at the weekly
  maintainers meeting. After addressing review comments, re-request review explicitly.

## Before opening the pull request

1. Rebase the branch onto `trunk` so the PR is the single commit, then force-push the fork branch
   (`git push --force-with-lease rayx refactor/surface-scaling-mode`). The merge commit
   `dcb2b7e26` would vanish in a squash merge anyway, but one commit is easier to review. The only
   conflict is the `CHANGELOG.md` "Major changes" list.
2. Open the PR against `gfx-rs/wgpu` `trunk` from `rayx-ui:refactor/surface-scaling-mode`.
3. Replace both `#XXXXX` placeholders in `CHANGELOG.md` (the "Major changes" entry and the DX12
   "Added/New Features" entry) with the PR number, check the `@dXnSix` handle, amend, and push.
4. Write the description from the notes below in your own words, and fill in the LLM section and
   checklist truthfully.

## Pull request notes

Title: `Add SurfaceConfiguration::scaling_mode`

### Connections

- Part of #10196 (not a fix: the report is Vulkan on Intel Arc; this PR implements DX12 only).
- Implements the design cwfitzgerald proposed in the issue: the setting on `SurfaceConfiguration`,
  the supported list in `SurfaceCapabilities`, validation in wgpu-core, an `Auto` default that
  stretches when it can, and documented fallback behavior.
- Possible follow-ups: Vulkan through `VK_EXT_swapchain_maintenance1`
  (`VkSwapchainPresentScalingCreateInfoEXT`, which would cover the issue's case); Metal through
  the layer's `contentsGravity`.

### Description

Problem:

- wgpu-hal creates every DX12 swap chain with `DXGI_SCALING_STRETCH`
  (`wgpu-hal/src/dx12/mod.rs`, `Surface::configure`).
- During a live resize, the window changes size before the app reconfigures the surface, so DXGI
  stretches the previous frame to the new window size. Apps that redraw at the new size every
  frame (UI toolkits) wobble and smear while the window is dragged.
- There is no way to opt out. Backends also differ (the issue: newer Intel Vulkan drivers
  letterbox instead of stretching).

API (`wgpu-types`, re-exported from `wgpu`):

- `SurfaceScalingMode { Auto = 0, Stretch = 1, OneToOne = 2 }`: `#[repr(C)]`, `Auto` is the
  default (`ConstDefault`), serde behind the `serde` feature.
- `SurfaceConfiguration::scaling_mode` (new field).
- `SurfaceCapabilities::scaling_modes` (new field, always contains `Auto`; `Default` gives
  `[Auto]`).
- `Surface::get_default_config` sets `Auto`.
- The `width`/`height` docs on `SurfaceConfiguration` now point at `scaling_mode`.

wgpu-core (`device/surface_config.rs`, `present.rs`, `instance.rs`):

- `Auto` resolves to `Stretch` when the surface lists `Stretch`; otherwise it stays `Auto` and the
  backend keeps its platform behavior. Logged through `api_log!`, like present and alpha `Auto`.
- A mode the surface doesn't list fails with `ConfigureSurfaceError::UnsupportedScalingMode
  { requested, available }`, which is a validation error.

wgpu-hal:

- `hal::SurfaceCapabilities::scaling_modes` and `hal::SurfaceConfiguration::scaling_mode`.
- DX12, `SurfaceTarget::WndHandle`: `[Auto, Stretch, OneToOne]`; `OneToOne` maps to
  `DXGI_SCALING_NONE` (frame drawn unscaled at the top-left; the uncovered part shows the window
  background).
- DX12, composition targets (`Visual`, `VisualFromWndHandle`, `SurfaceHandle`, `SwapChainPanel`):
  `[Auto, Stretch]`. DXGI composition swap chains only support stretch scaling.
- DX12 reconfigure: `ResizeBuffers` cannot change `Scaling`, so a configure with a different
  `scaling_mode` releases the old swap chain and creates a new one. The same mode still takes the
  `ResizeBuffers` path. `SwapChain` stores its `scaling_mode` to tell the two apart.
- Vulkan, Metal, GLES: `[Auto]`, behavior unchanged.

WebGPU / Deno:

- The `webgpu` backend reports `[Auto]` (page CSS decides canvas scaling); `deno_webgpu` passes
  `Auto`. No WebGPU spec equivalent, and no behavior change for WebGPU implementations.

Breaking change:

- The new field breaks struct literals of `SurfaceConfiguration`, hence the "Major changes"
  CHANGELOG entry with a migration diff. Adding `scaling_mode: SurfaceScalingMode::Auto` keeps
  the old behavior exactly (DX12 `Auto` resolves to `Stretch`, which is what it always used).
- Examples and hal examples are updated (`framework.rs`, `02_hello_window`, `03_hdr_surface`,
  the bug repros, `halmark`, `ray-traced-triangle`).

### Testing

Run on Windows 11 on 2026-10-08, on the branch with trunk `d7220bf2f` merged in:

- `cargo test -p wgpu-core --lib surface_configuration_tests`: 12 passed, including four new
  tests (`auto_scaling_resolves_to_stretch`, `auto_scaling_without_stretch_stays_auto`,
  `explicit_one_to_one_scaling_is_honored`, `explicit_unsupported_scaling_mode_errors`).
- `cargo fmt --all --check`.
- CI's clippy commands with `-D warnings`: `--tests --features glsl,spirv` on
  `x86_64-pc-windows-msvc` and `wasm32-unknown-unknown`; `-p wgpu -p wgpu-hal
  --no-default-features`; `-p wgpu-hal --all-features`; the `no_std` set (`wgpu-types`,
  `wgpu-hal`, `wgpu-core`, `wgpu --no-default-features`) on `wasm32v1-none`; `wgpu-types` with
  `serde`.
- `cargo check --workspace --all-targets` except `deno_webgpu` and `cts_runner` (their V8 build
  script fails on this machine).
- `RUSTDOCFLAGS="-D warnings" cargo doc -p wgpu-types -p wgpu-hal -p wgpu-core -p wgpu --no-deps`.
- `cargo xtask changelog origin/trunk`.

Manual DX12 run (from the original change, on Windows 11): a temporary build of `02_hello_window`
alternated `OneToOne` and `Auto` across resizes and read the swap chain back with
`IDXGISwapChain1::GetDesc1`. `OneToOne` produced `DXGI_SCALING_NONE`, `Auto` produced
`DXGI_SCALING_STRETCH`, mode changes recreated the swap chain without errors, and the window
closed cleanly. Mention this only if you can stand behind it, ideally after re-running it.

Downstream: gpux (a GPUI fork) presents its Windows windows through `DXGI_SCALING_NONE` window
swap chains with a patched wgpu 30.0.1 that hardcodes the setting. That shows the DXGI mode works;
it does not exercise this API.

Not tested: composition-surface targets at runtime, `cargo xtask test` (GPU tests), the
`deno_webgpu`/`cts_runner` builds, and any non-Windows platform at runtime (those backends only
gained a capability entry).

### Squash or Rebase?

One commit after the rebase; either merge method works.

### LLM Use?

Say in your own words how LLMs were involved (code, tests, CHANGELOG entry, these notes) and
that you reviewed and own the change.

### Checklist

- [ ] I self-reviewed and fully understand this PR.
- [x] `CHANGELOG.md` entries for the user-facing effects of this change are present.
- [x] The PR is minimal, and doesn't make sense to land as multiple PRs.
- [x] Commits are logically scoped and individually reviewable. (after the rebase)
- [ ] The PR description has enough context to understand the motivation and solution implemented.
- [ ] (If applicable) WebGPU implementations built with `wgpu` may be affected behaviorally. (not
  applicable: WebGPU and Deno report only `Auto`)
- [x] (If applicable) Validation and feature gates are in place to confine behavioral changes.
- [x] (If applicable) Tests demonstrate the validation and altered logic works.
- [ ] This PR was automatically created by an Agent.
- [ ] The description was automatically written by an Agent.

Check the first, fifth, and the two Agent lines yourself, according to what is true.
