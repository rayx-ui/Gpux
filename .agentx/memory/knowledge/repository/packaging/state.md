---
agentx:
  kind: state
  scope: knowledge/repository/packaging
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - crates-io
    - packaging
    - dependencies
    - wgpu-fork
    - licenses
    - taffy
  supersedes: []
  superseded_by: []
---
# Packaging State

## Purpose And Usage

Read before changing a manifest, dependency, license, included file, or the wgpu fork.

## Current Technical Shape

Every crate publishes as `gpux` or `gpux-*` and keeps its GPUI Rust name through `[lib] name`; workspace dependency keys use the Rust name with `package = "gpux-..."`, so source never changes when a package is renamed. The [README crate table](../../../../../README.md#crates) is the authoritative mapping. All crates share the workspace version and Apache-2.0 (gpui_mobile under its Apache-2.0 option); `gpux_fonts` ships the bundled fonts under OFL-1.1, and every packaged file lives inside its crate directory.

Dependencies come only from crates.io: no git dependencies and no `[patch]` tables. Zed's own forks come from their published `zed-*` crates (`zed-reqwest`, `zed-scap`, `zed-font-kit`, `zed-xim`). Taffy is the registry `=0.14.0` with a minimal feature set. `zlog`, `ztracing`, and `ztracing_macro` are not carried. Dev-dependencies on the `gpui_macros` → `gpux` and `gpux` → `gpui_platform`/`gpui_web` cycle edges are path-only.

`vendor/wgpu` holds `gpux-wgpu`: upstream wgpu's `wgpu`, `wgpu-core`, `wgpu-hal`, and `wgpu-core-deps-*` at the released version (currently `30.0.1`, published as `=30.0.1-gpux.1`), carrying exactly one change: DX12 HWND swap chains use `DXGI_SCALING_NONE` (gfx-rs/wgpu#10196). Its manifests are the published wgpu manifests with forked dependencies renamed and only library targets kept. HTML-island element upload is gpux code in `gpui_wgpu`, not a wgpu patch. The upstream replacement is tracked in [wgpu-dx12-swapchain-scaling](../../../../../docs/upstream/wgpu-dx12-swapchain-scaling.md).

## Backlinks

- [Workspace manifest](../../../../../Cargo.toml)
- [wgpu fork](../../../../../vendor/wgpu)
- [Fork script](../../../../../.claude/skills/gpux/scripts/fork_wgpu.py)
- 2026-10-05 current [crates.io packaging](../../../decisions/repository/packaging/2026-10-05-repository-packaging-crates-io-packaging.md)
- 2026-10-05 current [gpux-wgpu fork](../../../decisions/repository/packaging/2026-10-05-repository-packaging-gpux-wgpu-fork.md)
- 2026-10-05 current [Drop zlog/ztracing](../../../decisions/repository/packaging/2026-10-05-repository-packaging-drop-z-crates.md)
- 2026-08-30 current [Registry Taffy 0.14.0](../../../decisions/gpui/layout-style/2026-08-30-gpui-layout-style-registry-taffy-0-14.md)
