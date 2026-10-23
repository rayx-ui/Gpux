# Changelog

All notable changes to gpux are documented in this file. The whole workspace is released as one
version under one `vX.Y.Z` tag. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). It is generated from
Conventional Commits by release-plz.

## [1.1.0] - 2026-10-23

### Features
- *(conformance)* measure 1 px border beading with a ring-evenness step
- *(conformance)* add paint-contract scenes that hold every renderer to the contract

### Fixes
- *(conformance)* keep the pattern probes in their cells at every host scale

### Maintenance
- delete ToFix and drop it from the layout guard

### Tests
- *(util)* allow the darwin process test helper's raw spawn
- *(testkit)* run the ring-evenness and paint-contract scale groups in lanes A and C

## [1.0.0] - 2026-10-18

The first release of gpux under one workspace version: every own crate is 1.0.0 and the whole
workspace is released under the tag `v1.0.0`. It is written by hand; later sections are generated
from Conventional Commits.

### What gpux is at 1.0.0

- **GPUI from Zed:** Zed's GPUI crate closure at Zed commit `d544470d1d` (imported 2026-10-05),
  with Zed's Linux headless rendering (zed-industries/zed#64718) and shared headless device
  (zed-industries/zed#64842) ported on 2026-10-07, keeping Zed's crate boundaries and GPUI Rust
  names under `gpux-*` package names.
- **Platforms:** macOS (Metal), Windows (DirectX 12 through wgpu), Linux (Wayland and X11), the
  browser (multithreaded WASM on WebGPU with a WebGL2 fallback), and Android and iOS from
  gpui-mobile.
- **Rendering:** the wgpu renderer on Windows, Linux, the web and mobile, through the `gpux-wgpu`
  fork of wgpu 30.0.1 (DirectX 12 swap chains without stretching), and the `gpux-accesskit-macos`
  fork of `accesskit_macos` 0.27.1 for VoiceOver's tables, expanded states, range values and
  dimmed controls.
- **Fast mode:** layout retention, view retention, splice and scroll layers on by default, opt-in
  native window composition (`GPUX_FAST_MODE=0` turns them off).
- **Text and accessibility:** `EditableText`, a plain text field that becomes a rich text editor;
  an AccessKit accessibility tree that screen readers read on every platform and agents inspect
  and drive through gpux-devtools and its MCP sidecar.
- **Effects and layout:** frame and shader effects, fixed and sticky positioning, programmatic
  scrolling and virtual lists.
- **Tests:** seven lanes run by `gpux-testkit`, from the test platform (A) through a real browser
  (B), offscreen pixels (C), desktop windows (D), Android (E) and iOS (F) to the local GPU (G).

### Distribution

- gpux is consumed by release tag:
  `gpux = { git = "https://github.com/rayx-ui/Gpux", tag = "v1.0.0" }`, the other packages with
  their Rust-name key and `package = "gpux-..."`. Nothing is published to a registry; the
  manifests stay publishable.
- gpux declares what rayx needs: `gpux-fonts` is a rayx asset package, `gpux-mobile` names its
  Android Java sources (`android_java_dirs`), and the workspace pins the Android SDK, build tools
  and NDK (`[workspace.metadata.rayx]`). Building apps on gpux 1.0.0 with rayx needs rayx 1.4.0
  or later.
