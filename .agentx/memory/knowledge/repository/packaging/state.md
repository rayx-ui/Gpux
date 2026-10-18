---
agentx:
  kind: state
  scope: knowledge/repository/packaging
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-19T09:00:00+02:00
  topics:
    - crates-io
    - packaging
    - dependencies
    - wgpu-fork
    - licenses
    - taffy
    - releases
  supersedes: []
  superseded_by: []
---
# Packaging State

## Purpose And Usage

Read before changing a manifest, dependency, license, included file, or the wgpu fork.

## Current Technical Shape

Every crate is named `gpux` or `gpux-*` and keeps its GPUI Rust name through `[lib] name`; workspace dependency keys use the Rust name with `package = "gpux-..."`, so source never changes when a package is renamed. The [README crate table](../../../../../README.md#crates) is the authoritative mapping. All workspace crates share the workspace version and Apache-2.0 (gpui_mobile under its Apache-2.0 option); the standalone `crates/gpux_tests_benchmark` workspace (version `0.0.0`, `publish = false`) is exempt and never published; `gpux_fonts` ships the bundled fonts under OFL-1.1, and every packaged file lives inside its crate directory.

`gpux-tests-conformance` excludes `tests/goldens/**` from its package; a policy test caps each golden PNG at 32 KiB (written as compact RGB) and all goldens at 2 MiB, and rejects misnamed or orphaned goldens. Its `test-host` feature (default) pulls `gpui_platform` and, on Linux and Windows, `gpui_wgpu`; without it the catalog compiles for `wasm32-unknown-unknown` on stable, because `gpui_platform` always enables the threaded `gpux-web`.

Dependencies come only from crates.io: no git dependencies and no `[patch]` tables. Zed's own forks come from their published `zed-*` crates (`zed-reqwest`, `zed-scap`, `zed-font-kit`, `zed-xim`). Taffy is the registry `=0.14.0` with a minimal feature set. `zlog`, `ztracing`, and `ztracing_macro` are not carried. Dev-dependencies on the `gpui_macros` → `gpux` and `gpux` → `gpui_platform`/`gpui_web` cycle edges are path-only.

The workspace is released as one version (1.0.0 onward, 1.x while the owner is the only consumer) under one `vX.Y.Z` tag by release-plz in git-only mode ([release-plz.toml](../../../../../release-plz.toml), [release-plz.yml](../../../../../.github/workflows/release-plz.yml)): every own package sits in the `gpux` version group, only `gpux` tags, creates the GitHub Release and writes the root `CHANGELOG.md`, and the forks are `release = false`. Every own requirement (the root `[workspace.dependencies]` and `crates/gpui_mobile`'s inline `gpux` one) carries the workspace version; `tools/testkit/tests/release_config.rs` guards it. Nothing is published to a registry; the manifests stay publishable.

gpux declares what rayx (from its library-agnostic release) needs, inert for Cargo: `crates/gpux_fonts/rayx.assets.toml` makes the fonts a rayx asset package (`fonts/...` logical paths), `crates/gpui_mobile` lists `android_java_dirs = ["java"]`, and the root `[workspace.metadata.rayx]` pins lane E's Android platform, build tools and NDK for apps built in this workspace; `tools/testkit/tests/rayx_declarations.rs` keeps them equal to lane E.

`vendor/wgpu` holds `gpux-wgpu`: upstream wgpu's `wgpu`, `wgpu-core`, `wgpu-hal`, and `wgpu-core-deps-*` at the released version (currently `30.0.1`, published as `=30.0.1-gpux.1`), carrying exactly one change: DX12 HWND swap chains use `DXGI_SCALING_NONE` (gfx-rs/wgpu#10196). Its manifests are the published wgpu manifests with forked dependencies renamed and only library targets kept. HTML-island element upload is gpux code in `gpui_wgpu`, not a wgpu patch. The upstream replacement is tracked in [wgpu-dx12-swapchain-scaling](../../../../../docs/upstream/wgpu-dx12-swapchain-scaling.md).

## Backlinks

- [Workspace manifest](../../../../../Cargo.toml)
- [release-plz configuration](../../../../../release-plz.toml)
- [wgpu fork](../../../../../vendor/wgpu)
- [Fork script](../../../../../.claude/skills/gpux/scripts/fork_wgpu.py)
- 2026-10-18 current [git-tag releases](../../../decisions/repository/packaging/2026-10-18-repository-packaging-git-tag-releases.md)
- 2026-10-05 superseded [crates.io packaging](../../../decisions/repository/packaging/2026-10-05-repository-packaging-crates-io-packaging.md)
- 2026-10-05 current [gpux-wgpu fork](../../../decisions/repository/packaging/2026-10-05-repository-packaging-gpux-wgpu-fork.md)
- 2026-10-05 current [Drop zlog/ztracing](../../../decisions/repository/packaging/2026-10-05-repository-packaging-drop-z-crates.md)
- 2026-08-30 current [Registry Taffy 0.14.0](../../../decisions/gpui/layout-style/2026-08-30-gpui-layout-style-registry-taffy-0-14.md)
