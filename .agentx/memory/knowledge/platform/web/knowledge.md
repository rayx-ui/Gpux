---
agentx:
  kind: knowledge
  scope: knowledge/platform/web
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-07T23:55:00+02:00
  topics:
    - wasm
    - multithreading
    - browser-window
    - dpr
    - html-island
    - html-element
  supersedes: []
  superseded_by: []
---
# Web Knowledge

- Threaded WASM needs nightly `-Z build-std=std,panic_abort`, `+atomics,+bulk-memory,+mutable-globals`, shared/imported memory linker flags, and TLS/heap exports before `wasm-bindgen`. `gpux-testkit web build` makes that build, and `gpux-testkit web serve` sends the COOP/COEP/CORP headers. A threaded proof shows shared WASM memory, `wasm_thread_entry_point`, atomic instruction prefixes, `SharedArrayBuffer`, `Atomics.waitAsync`, a threaded-dispatcher log, and worker scopes.
- Before explicit constructor initialization, wasm-ld wrapped browser exports with the 760-byte `__wasm_call_ctors` routine and profiles showed about 0.5 s in repeated `inventory::Registry::submit`; calling it once in `web_init` improved acknowledged throughput about 20% and paced p95 about 11%.
- Browser apps should emit `<meta name="viewport" content="width=device-width, initial-scale=1.0">`; zoom/DPI investigations should check `visualViewport.scale`, CSS canvas bounds, and backing-store-to-CSS ratio against `devicePixelRatio`.
- Derive backing-store size from `ResizeObserverEntry.contentRect` × `devicePixelRatio`; Chromium under Playwright-emulated high DPI can report `devicePixelContentBoxSize` values that break the expected ratio.
- A queued `requestAnimationFrame` or `ResizeObserver` callback can outlive a closed window handle; without cancel/disconnect Chromium reports "closure invoked recursively or after being dropped".
- An apparently zoomed browser app may be a viewport/DPR bug or a font-metric change; keep proportional Latin UI fonts primary and large CJK faces as fallback unless the app is CJK-first.
- Chrome Canary's HTML-in-canvas requires the source DOM to stay a direct child of the canvas in the layout subtree; scratch canvases and cloned DOM are invalid. Its `copyElementImageToTexture` needs a `destination` member (the standard `copyExternalImageToTexture` dictionary does not fit), requires `COPY_DST` and `RENDER_ATTACHMENT` on the destination, and throws "No cached paint record for element" before a layout-subtree paint has produced a record. Canary layout subtree hit testing needs the transform; `left`/`top` alone mislocates islands.
- `html_element(...)` is scene metadata for `gpui_web`, not a WebGPU batch or HTML-in-canvas texture. Cross-origin iframe interiors do not bubble wheel events to the parent, and in cross-origin-isolated threaded pages third-party iframes without compatible COEP/CORP need `credentialless`.
