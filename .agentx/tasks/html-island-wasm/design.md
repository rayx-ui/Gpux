---
agentx:
  kind: knowledge
  scope: repo:tasks/html-island-wasm
  status: current
  created: 2026-07-01T00:00:00+02:00
  updated: 2026-07-01T03:40:00+02:00
  topics:
    - html-island
    - wasm
    - gpux
  supersedes: []
  superseded_by: []
---

# Design: HTML Island WASM Canvas Integration

## Design Summary

Add `html_island` as an experimental GPUI element backed by a web-only DOM registry and dynamic WebGPU texture rendering path. The feature should live primarily in `crates/gpux` because it needs access to the GPUI element lifecycle, `Scene` primitive ordering, `gpui_web`'s real browser canvas, and `gpui_wgpu`'s renderer.

RayX application code should see a clean Rust element API. It should not need to know about browser DOM roots, raw `web_sys` handles, WebGPU upload details, or Playwright flags.

## Source Evidence

The current `gpui_web::WebWindow::new` creates a `web_sys::HtmlCanvasElement`, initializes DPR-aware backing size, applies full-size CSS, appends it to `document.body`, creates a hidden input element, and constructs `WgpuRenderer::new_from_canvas`.

The current event path registers pointer and wheel listeners on the canvas and keyboard/composition/focus listeners on the hidden input. Pointer coordinates come from `offsetX` and `offsetY`, which are relative to the event target and will be wrong when the event target is a canvas child DOM node.

The current `gpux::Scene` contains paint operations and primitive arrays for shadows, quads, paths, underlines, monochrome sprites, subpixel sprites, polychrome sprites, and surfaces. There is no HTML island primitive or dynamic external DOM texture primitive.

The current `gpui_wgpu::WgpuRenderer` draws scene batches in `encode_scene_to_view`. `PrimitiveBatch::Surfaces` is treated as macOS-only video playback and is a no-op on web.

The current gpux workspace pins Zed's `wgpu` v29 branch. Repo source has no current references to `copyElementImageToTexture`, `ElementImage`, `drawElementImage`, or `layoutsubtree`, so the renderer upload path must be proven during implementation.

The current Playwright infrastructure already has a dedicated Canary project:

```ts
{
  name: "chrome-canary-html-island",
  testMatch: ["rayx-html-island.spec.ts"],
  use: {
    ...devices["Desktop Chrome"],
    channel: "chrome-canary",
    headless: false,
    launchOptions: {
      args: htmlInCanvasArgs,
    },
  },
}
```

`htmlInCanvasArgs` includes `CanvasDrawElement` together with the WebGPU/SharedArrayBuffer flags. `tools-node/package.json` exposes `test:wasm:html-island`.

## External Browser Model

The HTML-in-canvas proposal is expected to provide these pieces:

- `layoutsubtree` on the canvas so direct children participate in layout, hit-testing, and accessibility.
- Canvas paint invalidation through a `paint` event and `requestPaint()`.
- Snapshot/capture objects such as `ElementImage`.
- Canvas/WebGL/WebGPU draw/upload functions, with WebGPU centered on `GPUQueue.copyElementImageToTexture`.

Because this is experimental, all browser API calls must be feature-detected and diagnostics-facing.

## Public API Shape

Initial author-facing shape:

```rust
html_island("settings-form")
    .html(trusted_html)
    .input_mode(HtmlIslandInputMode::DomOwned)
    .fallback(fallback_element)
```

Required public concepts:

- `html_island`
- `HtmlIsland`
- `HtmlIslandInputMode`
- `HtmlIslandSupport`

The exact builder method names can follow local GPUI style. The constructor and element naming are fixed. V1 should avoid exposing raw `web_sys::Element` to normal app code; a future DOM builder callback can be added only after the primitive, registry, and texture path are proven.

## Architecture

### GPUI Core

Add:

- `crates/gpux/crates/gpui/src/elements/html_island.rs`
- exports from `crates/gpux/crates/gpui/src/elements/mod.rs`
- `HtmlIslandPrimitive` in `crates/gpux/crates/gpui/src/scene.rs`
- `Primitive::HtmlIsland`
- `PrimitiveKind::HtmlIsland`
- `PrimitiveBatch::HtmlIslands`
- `Window::paint_html_island(...)` or equivalent internal helper.

The element participates in layout using normal GPUI `Style`. During paint it inserts an HTML island primitive with scaled bounds, content mask, stable id, input mode, content revision, and trusted HTML payload or registry key.

Native targets render fallback content or an explicit unsupported state. They do not host DOM.

### Web Platform

Add:

- `crates/gpux/crates/gpui_web/src/html_in_canvas.rs`
- `crates/gpux/crates/gpui_web/src/html_island.rs`
- web-sys/js-sys features needed for detection, custom event access, DOM root updates, and element target inspection.

`WebWindowInner` should own:

- HTML-in-canvas support state,
- optional `WebHtmlIslandRegistry`,
- paint event listener,
- diagnostics counters,
- dirty island set.

Before drawing a scene, `WebWindow` should:

1. Extract HTML island primitives from the scene.
2. Sync/create direct canvas child DOM roots.
3. Update trusted HTML on revision change.
4. Sync bounds and input-mode CSS.
5. Request initial/dirty paint where required.
6. Prune roots absent from the scene.
7. Provide dirty/snapshot/upload sources to the renderer.

The web layer owns DOM nodes. The renderer must not parse HTML.

### Renderer

Add:

- `crates/gpux/crates/gpui_wgpu/src/html_island_texture.rs`
- web-only upload helpers in or near `wgpu_renderer.rs`
- dynamic texture cache keyed by stable island id and physical size.

Preferred upload path:

1. Ask browser for an island element image/snapshot or use the direct DOM element if the API permits.
2. Upload through `GPUQueue.copyElementImageToTexture`.
3. Draw the resulting texture as a primitive batch in scene order.

Fallback/prototype upload path if direct bindings are blocked:

1. `captureElementImage` for the direct canvas child.
2. `drawElementImage` into an intermediate canvas or offscreen canvas.
3. Existing `wgpu::Queue::copy_external_image_to_texture` from that canvas.

The fallback must be measured and diagnostics-tagged. It should be kept replaceable by the direct path.

### Playwright And Xtask

Keep normal validation independent:

```powershell
cargo run -p xtask -- app apps/lab test wasm --headed --webgpu
```

Use the existing dedicated HTML island project for required-mode feature work:

```powershell
cd tools-node
pnpm run test:wasm:html-island
```

`tools-node/tests/rayx-html-island.spec.ts` must be bound to `chrome-canary-html-island` through the existing project `testMatch`. It should collect JSON diagnostics, screenshots, traces, console logs, page errors, request failures, WebGPU adapter info, and pixel evidence.

If the implementation later adds xtask orchestration for the dedicated project, it should preserve the normal WASM validation path and add a narrow opt-in such as an environment variable or `--html-island` flag.

## Diagnostics Design

Expose a browser-readable object such as:

```js
globalThis.__rayxHtmlIslandDiagnostics = {
  available: true,
  support: {
    layoutSubtree: true,
    paintEvent: true,
    requestPaint: true,
    captureElementImage: true,
    copyElementImageToTexture: true,
    fallbackExternalImageCopy: false
  },
  canvas: {
    layoutSubtreeEnabled: true,
    childCount: 2
  },
  islands: {
    count: 2,
    dirtyCount: 0,
    uploadCount: 3,
    textureReallocationCount: 1
  },
  renderer: {
    path: "copyElementImageToTexture",
    lastError: null
  }
}
```

Field names can change, but the diagnostics must remain structured. Tests should not scrape visible text.

Implementation debugging should use the RayX diagnostics workflow:

- add temporary traces at layout, prepaint, paint, DOM registry, input target, dirty tracking, upload, and draw boundaries;
- run headed WASM/Canary Playwright validation with traces and counters;
- use DevKit/rendered-frame diagnostics where they can prove GPUI bounds and hierarchy;
- remove temporary traces after root cause proof.

## Data Models

### `HtmlIslandId`

Stable id supplied by app code. Prefer a small newtype over `SharedString` if it improves type clarity without adding needless abstraction.

### `HtmlIslandInputMode`

```rust
pub enum HtmlIslandInputMode {
    DomOwned,
    GpuiOwned,
}
```

### `HtmlIslandPrimitive`

Fields:

- stable id,
- bounds in scaled pixels,
- logical bounds for DOM sync,
- content mask,
- input mode,
- content revision/hash,
- trusted HTML payload or registry key,
- optional accessible label/fallback metadata.

If large HTML payloads in scene history become expensive, move payload storage into a per-window content registry keyed by id/revision and keep primitives small.

### `WebHtmlIslandRegistry`

Fields:

- canvas handle,
- support flags,
- map of id to DOM root,
- dirty set,
- last bounds/revision/input mode per island,
- diagnostics counters,
- optional snapshot handles requiring cleanup.

## Event And Focus Design

Replace the current `offsetX/offsetY` helper with a canvas-relative calculation:

```rust
let rect = canvas.get_bounding_client_rect();
let x = event.client_x() as f64 - rect.left();
let y = event.client_y() as f64 - rect.top();
```

The resulting value should remain in GPUI logical CSS pixels unless a call site explicitly expects physical pixels.

For `DomOwned` islands:

- classify event target by walking ancestors to a registered island root;
- avoid forcing hidden input focus when the pointer down targets a DOM-owned island;
- avoid unconditional `preventDefault()` on events that the DOM island should own;
- let browser focus, keyboard, composition, and accessibility work inside the island.

For `GpuiOwned` islands:

- set `pointer-events: none` on the island root;
- keep current GPUI hidden input and pointer event behavior.

## Renderer Ordering

`Scene::batches()` must include `PrimitiveBatch::HtmlIslands`. Ordering should follow the same `order` and primitive kind sorting model as existing primitives. Tests should include:

- quad below, island, quad above;
- island below sprite/text-equivalent content;
- clipped island;
- zero-sized island skipped;
- removed island pruned.

## Security Model

`html_island` is trusted-markup only. The element is not an HTML sanitizer, iframe host, cross-origin page renderer, or browser privacy bypass. Documentation and examples must say this plainly.

## Rollout

- Keep the API experimental and opt-in.
- Keep normal CI and normal WASM validation independent from Chrome Canary.
- Run Canary required-mode tests locally and in any environment where Chrome Canary plus the feature flag are available.
- Promote the AgentX gpux-foundation hypothesis to a rule only after repeated implementation success across browser versions.

## Rejected Alternatives

- Overlay DOM or WebView: rejected because it cannot prove scene order, clipping, or canvas compositing.
- App-level DOM portal: rejected because app code does not own the real `gpui_web` canvas or renderer.
- TypeScript runtime bridge: rejected by Rust-only RayX architecture.
- Intermediate canvas as the only design: acceptable as measured prototype/fallback, but not the preferred end state.

## Open Questions

- Does Chrome Canary currently expose the direct WebGPU upload path through API names compatible with Rust binding access?
- Can the pinned `wgpu` branch expose enough raw handles for a narrow JS reflection call without an architectural leak?
- Should `xtask` grow a dedicated HTML island Playwright project selector, or should the Canary script remain a `tools-node` script used against a manually served WASM host?
- How much transform and rounded-clip support is required for the first useful Lab story?
