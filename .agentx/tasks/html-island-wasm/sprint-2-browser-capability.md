---
agentx:
  kind: knowledge
  scope: repo:tasks/html-island-wasm/sprint-2
  status: current
  created: 2026-07-01T00:00:00+02:00
  updated: 2026-07-01T00:00:00+02:00
  topics:
    - html-island
    - diagnostics
    - wasm
    - gpui-web
  supersedes: []
  superseded_by: []
---

# Sprint 2 Browser Capability Notes

## 2026-07-01 Runtime Capability Wiring

`gpui_web` now owns the browser-side `globalThis.__rayxHtmlIslandDiagnostics` object after `WebWindow::new` creates the real canvas. The generated WASM host still installs a pre-init bootstrap object so Playwright can inspect unsupported mode before Rust starts, then `gpui_web` overwrites it with `source: "gpui_web"`.

The dedicated Canary diagnostics URL uses `?rayx-html-island=1`. The generated host converts that query into `globalThis.__rayxHtmlIslandEnabled` before WASM initialization. `gpui_web` only sets the real canvas `layoutsubtree` boolean attribute when that opt-in is present and runtime probes report layout subtree, paint event, and `requestPaint()` support.

The current diagnostics include:

- support/probe flags for layout subtree, paint/requestPaint, capture/draw APIs, and WebGPU element/external-image upload methods;
- canvas layout subtree state, direct child count, `requestPaintCount`, `paintEventCount`, and `changedElementCount`;
- initial zero-valued island counters;
- renderer path `unsupported` until the later renderer sprint.

Validation run during Sprint 2:

- `cargo fmt --all --check`
- `cargo +nightly check --manifest-path apps/lab/platform/web/Cargo.toml --target wasm32-unknown-unknown -Z build-std=std,panic_abort`
- `cargo test -p xtask`
- `pnpm exec tsc --noEmit --pretty false`
- `pnpm run test:wasm:html-island` against served Lab WASM on Chrome Canary
- `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`

Artifacts were written under ignored `artifacts-temp/`, `artifacts/`, and `tools-node/test-results/` paths.
