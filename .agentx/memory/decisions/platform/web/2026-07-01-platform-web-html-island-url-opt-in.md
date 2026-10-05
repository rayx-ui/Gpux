---
agentx:
  kind: decision
  scope: decisions/platform/web
  status: current
  created: 2026-07-01T00:00:00+02:00
  updated: 2026-07-01T00:00:00+02:00
  topics:
    - gpux
    - html-island
    - wasm
    - diagnostics
  supersedes: []
  superseded_by: []
---

## Decision: Gate HTML island canvas layout subtree through an explicit browser opt-in

### Context: Sprint 2 of `html-island-wasm` needed `gpui_web::WebWindow::new` to set the real canvas into HTML-in-canvas layout subtree mode only when the experimental path is enabled and browser capability detection says it is safe.

### Alternatives considered: Always enable when the browser exposes the API; use a Rust compile-time Cargo feature; use only a JavaScript global set manually by tests; use a URL query parameter that the generated host converts into a global before WASM starts.

### Reasoning: A URL query parameter keeps normal WASM validation behavior unchanged while making the dedicated Canary Playwright project deterministic. The generated host already runs before WASM initialization, so it can translate `?rayx-html-island=1` into `globalThis.__rayxHtmlIslandEnabled` for Rust to read without adding an app API or a TypeScript runtime bridge.

### Trade-offs accepted: The first implementation has a development/test-facing opt-in rather than an app-author-facing configuration surface. A later public `html_island` API or feature flag can replace or supplement this once the element, registry, and renderer paths exist.

### Supersedes: None
