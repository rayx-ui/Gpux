---
agentx:
  kind: decision
  scope: decisions/platform/web
  status: current
  created: 2026-10-09T17:00:00+02:00
  updated: 2026-10-09T17:00:00+02:00
  topics:
    - gpux
    - web
    - lane-b
    - playwright
    - webgpu
  supersedes: []
  superseded_by: []
---

## Decision: Lane B drives the threaded WASM conformance host in Chromium through Playwright and asserts the backend

### Context: `gpui_web` needs proof in a real browser. Experiments on 2026-10-06 showed headless Chromium renders real WebGPU on GPU-less Linux through SwiftShader with specific flags, while on Windows the headless shell has no WebGPU adapter unless ANGLE uses D3D11. gpui_web's WebGL2 fallback would let WebGPU specs pass silently.

### Alternatives considered: wasm-bindgen-test in a headless browser; headed CI runs only; RayX's Test Suite in Playwright.

### Reasoning: `web_host` is a multithreaded browser app initialized through `gpui_platform::web_init` that picks its scene from the URL (`?scene=<id>&backend=webgpu|webgl`) and exposes `globalThis.__gpuxConformance` (ready flag, active backend, semantic `locate`, in-app probe results). `gpux-testkit web build` uses the pinned nightly with `-Z build-std` and the wasm-bindgen CLI that `Cargo.lock` pins, and verifies the binary; `web serve` sends COOP/COEP/CORP headers. Playwright in `tools/browser/` (pnpm, committed lockfile) drives trusted CDP input at located bounds. Profiles: `ci-linux-swiftshader`, `local-gpu` and `windows-software` (ANGLE on D3D11 WARP); only one `--enable-features` switch is ever passed, because Chromium keeps the last. Every spec asserts the backend (WebGPU unless it forces WebGL2), `crossOriginIsolated`, and no console errors or panics. Tags: `@canary`, `@pr`, `@dpr2`, and `@timing` (lane G only). The browser keeps its own user agent, so platform conventions such as the shortcut modifier follow the host OS.

### Trade-offs accepted: Lane B needs a Node toolchain, which building and testing the Rust crates never does. Software profiles never assert timing.

### Supersedes: None. Builds on [multithreaded WASM](2026-06-24-platform-web-multithreaded-wasm.md), [explicit WASM constructors](2026-08-31-platform-web-explicit-wasm-constructors.md), [WASM SVG asset loading](2026-06-29-platform-web-wasm-svg-asset-loading.md) and the [browser backend fallback](../../renderer/wgpu/2026-08-07-renderer-wgpu-browser-backend-fallback.md).
