---
agentx:
  kind: knowledge
  scope: repo:tasks/html-element-dom-overlay
  status: current
  created: 2026-07-03T18:30:00+02:00
  updated: 2026-07-03T18:30:00+02:00
  topics:
    - html-element
    - dom-overlay
    - wasm
    - iframe
  supersedes: []
  superseded_by: []
---

# Epic: HTML Element DOM Overlay

## Scope

Implement a new GPUI/gpux element named `html_element` for RayX WASM browser targets. Unlike `html_island`, which captures live canvas-child DOM into WebGPU textures, `html_element` keeps browser-owned DOM as a real overlay above the GPUI canvas. This is the intended path for cross-origin iframes, payment providers, sports stream players, casino/game frames, YouTube embeds, and other embedded content that cannot or should not be copied into a canvas texture.

The feature belongs primarily in `crates/gpux` because it depends on GPUI layout, paint-time content masks, scene-frame metadata, `gpui_web` window ownership, and browser DOM synchronization through `web_sys`. RayX/Lab should consume it as a normal Rust element and story surface.

## Problem Statement

RayX browser apps render GPUI through a WebGPU canvas. The existing `html_island` feature can composite trusted DOM into that canvas when experimental HTML-in-canvas support is available, but Chrome intentionally does not paint cross-origin iframe contents into canvas for privacy and security reasons. That makes `html_island` unsuitable for major iframe-heavy product cases such as banking/payment flows, sportsbook streaming providers, YouTube/video embeds, casino games, and third-party interactive widgets.

RayX needs a second, explicit browser-DOM feature that can host real DOM above the canvas while still following GPUI layout, scroll, clipping, resizing, and lifecycle semantics closely enough for product use.

## Outcomes

- RayX authors can write `html_element(...)` in Rust UI code.
- On WASM/browser targets, `gpui_web` creates real DOM nodes with `web_sys`.
- DOM roots are keyed by stable GPUI ids and are not destroyed on ordinary RayX/GPUI re-renders.
- HTML content is replaced only when `content_revision` changes.
- Geometry, clipping, visibility, opacity, pointer mode, and size are synchronized every rendered frame when changed.
- Parent scroll containers remain GPUI-owned; DOM overlay roots mirror GPUI bounds and content masks instead of creating a duplicate DOM scrolling state.
- `keep_alive(true)` parks an absent/offscreen DOM root without destroying iframe browsing context, preserving video/game/payment/session state.
- Native and unsupported targets render explicit fallback content or no-op safely.
- Lab includes real examples for iframe, video/player, keep-alive, parent scroll, resize, clipping, and lifecycle behavior.
- Playwright and RayX diagnostics prove real browser DOM follows GPUI scroll/geometry without relying on Chrome Canary HTML-in-canvas flags.

## In Scope

- Add public GPUI API:
  - `html_element(id)`
  - `HtmlElement`
  - `HtmlElementInputMode`
  - `HtmlElementLifetime` or equivalent `keep_alive(true)` behavior
  - `trusted_html(...)`
  - `input_mode(...)`
  - `keep_alive(...)`
  - `fallback(...)`
- Add a GPUI scene/frame primitive for DOM overlay synchronization.
- Add `Window::paint_html_element(...)` or equivalent paint helper.
- Add `gpui_web::WebHtmlElementRegistry` backed by `web_sys`.
- Create one `data-rayx-html-element-overlay` sibling overlay root above the real GPUI canvas.
- Sync DOM overlay roots from `WebWindow::draw(scene)` using scene primitives and the canvas bounding client rect.
- Separate content lifecycle from geometry lifecycle:
  - `content_revision` controls `set_inner_html`.
  - bounds, content mask, canvas rect, opacity, pointer mode, and visibility sync independently and must not reload iframes.
- Preserve iframe browsing contexts with `keep_alive(true)` by parking hidden roots offscreen without `display: none` and without clearing content.
- Use GPUI `ScrollHandle` and paint content masks as source of truth for scrolling; do not create duplicate DOM scroll containers for normal use.
- Support rectangular clipping through CSS wrapper roots.
- Add diagnostics, tests, and Lab story coverage for the browser overlay path.
- Keep `html_island` behavior and direct canvas-child texture upload unchanged.

## Out Of Scope

- Capturing iframe pixels into the WebGPU canvas.
- Making GPUI elements render visually above DOM overlay iframe content.
- Cross-origin iframe DOM inspection, intrinsic content measurement, or direct mutation.
- Replacing `html_island`.
- Replacing GPUI text/input/image/rendering systems with DOM.
- Browser-native duplicate scrolling as the default implementation.
- Sanitizing untrusted HTML inside `html_element`; the API accepts trusted app markup only.
- Native desktop/mobile WebView hosting in this epic.
- Guaranteeing provider iframe embedding when the provider blocks framing through CSP, `frame-ancestors`, X-Frame-Options, COEP/CORP, or authentication/session policy.

## Actors

- RayX application author: places `html_element` inside normal GPUI layout.
- `gpui` core: calculates layout, paint bounds, content masks, opacity, and scene order metadata.
- `gpui_web`: owns the browser canvas, overlay root, web DOM registry, diagnostics, and event behavior.
- Browser iframe/provider: owns cross-origin frame contents, focus, input, video playback, payment session, or game session.
- Test/diagnostics tooling: verifies geometry, lifecycle, and runtime behavior through RayX diagnostics and Playwright.

## Functional Requirements

### Public API

- `html_element(id)` must require a stable application-supplied id.
- The id must be used to correlate GPUI frame primitives with browser DOM roots.
- `trusted_html(...)` must accept trusted app markup and must not sanitize, sandbox, or fetch remote content itself.
- `fallback(...)` must render on native and unsupported paths.
- `input_mode(HtmlElementInputMode::DomOwned)` must let browser DOM own pointer, focus, keyboard, composition, media controls, iframe interaction, and accessibility within the DOM root.
- `input_mode(HtmlElementInputMode::GpuiOwned)` must leave the overlay non-interactive so GPUI receives pointer input through the canvas.
- `keep_alive(true)` must keep a DOM root alive when absent from a frame or offscreen according to the registry policy.
- API docs must explicitly distinguish `html_element` from `html_island`.
- API docs must warn that stable ids and stable iframe `src` values are required to avoid unintended reloads.

### Scene And Paint Model

- GPUI paint must insert an `HtmlElementPrimitive` into the current frame/scene.
- The primitive must include:
  - draw/order metadata
  - stable id
  - scaled bounds
  - logical CSS-pixel bounds
  - logical CSS-pixel content mask
  - input mode
  - opacity
  - content revision
  - trusted HTML payload or future host-slot key
  - keep-alive/lifetime policy
- The primitive must not become a WebGPU draw batch.
- Native and unsupported renderers must ignore the primitive and rely on fallback behavior.

### Browser DOM Registry

- `gpui_web` must create exactly one sibling overlay root above the real GPUI canvas for DOM overlays.
- The overlay root must use `position: fixed`, `inset: 0`, `overflow: hidden`, and `pointer-events: none`.
- Each DOM root must be keyed by stable id and reused across GPUI re-renders.
- Each DOM root should be shaped as:
  - a clipping wrapper with `position: absolute`, `overflow: hidden`, and diagnostic data attributes
  - a content child with `position: absolute`, `transform-origin: 0 0`, and the trusted HTML content
- The registry must call `set_inner_html(...)` only when `content_revision` changes.
- The registry must sync geometry independently of content revision.
- The registry must prune absent roots by default.
- The registry must park absent roots when `keep_alive(true)` is active.
- Parked roots must preserve their iframe browsing context.
- Parking must avoid `display: none` and avoid clearing `src`/HTML.
- Parking should use hidden/offscreen CSS such as `visibility: hidden`, `opacity: 0`, `pointer-events: none`, and far-offscreen transform while preserving last nonzero dimensions.

### Geometry Synchronization

- `WebWindow::draw(scene)` must sync HTML element roots after canvas size updates and before/around renderer drawing for the same frame.
- Geometry sync must use `canvas.getBoundingClientRect()` plus primitive logical bounds and logical content mask.
- DOM clip wrapper geometry must be positioned in viewport CSS coordinates.
- DOM content child geometry must be positioned relative to the clip wrapper.
- Parent scroll changes must update overlay position/clip without replacing HTML.
- Parent viewport resize must update overlay position/clip without replacing HTML.
- `html_element` size changes must update wrapper/content width and height without replacing HTML.
- Canvas rect changes must update viewport positioning without replacing HTML.
- Empty clipped bounds must hide or park the root according to lifetime policy without reloading iframe content.
- Rectangular clipping is required for V1; non-rectangular masks, rotation, complex transforms, and shader effects are out of scope unless they can be represented safely with CSS later.

### Scroll Model

- GPUI scroll containers must remain the source of truth.
- Normal `html_element` usage must not create a duplicate browser scroll container.
- The DOM overlay must mirror current GPUI paint bounds and intersected content masks.
- Nested scroll containers should work through nested/intersected GPUI content masks.
- `VirtualListExternalScrollBehavior::ParentViewport` must remain compatible: virtualized content stays in normal GPUI flow, and the overlay follows visible rendered items.
- Wheel/trackpad events over a cross-origin iframe may be consumed by the iframe and may not scroll the GPUI parent; this limitation must be documented.

### Input And Focus

- Dom-owned roots must be interactive browser DOM.
- Empty overlay regions must not block the GPUI canvas.
- GPUI-owned roots must set pointer events so the canvas remains interactive.
- Focus inside iframe/provider DOM must not force hidden GPUI input focus.
- Browser accessibility should see the real DOM subtree when visible.
- Iframes must require meaningful `title` attributes in examples and docs.

### Security And Provider Compatibility

- The API must treat HTML strings as trusted application markup only.
- Examples must use safe iframe attributes such as `title`, `allow`, `sandbox`, `referrerpolicy`, and explicit sizing.
- Future `html_iframe(...)` builder can provide safer structured iframe construction, but raw trusted HTML is sufficient for V1.
- Cross-origin iframe internals must not be read or assumed.
- Intrinsic iframe content size cannot be measured cross-origin; app state, explicit layout, provider APIs, same-origin wrappers, or `postMessage` must provide size when dynamic sizing is needed.

### Diagnostics

- `gpui_web` must expose diagnostics for the overlay path, separate from `__rayxHtmlIslandDiagnostics`.
- Diagnostics must include:
  - source marker
  - overlay root presence
  - active root count
  - parked root count
  - pruned root count
  - content update count
  - geometry update count
  - skipped geometry update count
  - last root geometry
  - input mode counts
  - keep-alive counts
  - last error, if any
- Diagnostics must be readable from Playwright and useful during RayX runtime diagnosis.
- Temporary trace logs may be used during development but must be removed before final implementation commit.

### Lab Examples

- Lab must include a dedicated `html_element` story or a clearly separated section from `html_island`.
- Examples must include:
  - basic trusted HTML overlay
  - iframe embed
  - video/player iframe
  - keep-alive iframe that preserves session/playback through re-render, tab switch, or scroll absence
  - parent scroll clipping
  - nested/parent-scroll virtualization case
  - size change controls
  - GPUI-owned pointer passthrough case
  - native/unsupported fallback
- The story must explain the limitations: above-canvas layer, no GPUI-over-iframe drawing, iframe wheel behavior, trusted markup, and cross-origin measurement limits.

## Non-Functional Requirements

### Performance

- Geometry sync must batch DOM writes and skip unchanged geometry.
- HTML content replacement must be avoided unless content revision changes.
- Registry pruning/parking must be O(active roots + known roots) per frame.
- Scrolling should update transform/position/size only; it must not reload iframe content.
- Keep-alive roots must be bounded by app behavior and diagnostics so long-lived iframes do not leak silently.

### Reliability

- Unsupported targets must compile and render fallback.
- Browser DOM failures must be diagnostics-visible and must not crash the GPUI render loop.
- Route changes and window destruction must remove overlay roots cleanly.
- Canvas resize and DPR changes must preserve coordinate correctness.

### Accessibility

- DOM-owned content must remain real DOM when visible.
- Examples must include iframe `title`.
- Hidden/parked content should not remain accidentally reachable to assistive technology when inactive.

### Security And Privacy

- Raw HTML is trusted-only.
- Cross-origin iframe internals are opaque.
- Provider embedding restrictions must be respected.
- Documentation must avoid implying canvas capture, DOM inspection, or CORS workarounds.

### Maintainability

- Keep `html_element` and `html_island` separate in API, registry, diagnostics, and tests.
- Use `web_sys` for browser DOM creation and updates.
- Do not introduce a TypeScript bridge, managed bridge, proxy DOM, cloned DOM, scratch canvas, or compatibility shim.
- Keep code organized as vertical slices.

## Acceptance Criteria

- `html_element` API compiles on native and WASM targets.
- Native target renders fallback without `web_sys`.
- WASM target creates one overlay root sibling above the real GPUI canvas.
- A stable-id iframe root survives ordinary RayX re-render without reload.
- `content_revision` changes replace content exactly once and intentionally reload iframe content.
- Parent scroll moves and clips DOM overlay roots without replacing content.
- Size changes update DOM width/height without replacing content.
- `keep_alive(true)` parks a root when absent and restores it without recreating the iframe element.
- Default absent roots are pruned.
- Dom-owned iframe controls are clickable/focusable.
- GPUI-owned overlay regions pass pointer interaction through to GPUI canvas.
- Playwright verifies geometry through DOM rects and screenshots in headed WebGPU browser.
- RayX diagnostics report active/parked/pruned/content/geometry counters.
- Normal WASM validation remains green.
- Existing `html_island` Canary validation remains green.

## Assumptions

- The new feature name is `html_element`.
- `web_sys` is the correct runtime DOM API layer.
- `html_element` is a gpux framework feature, not a RayX app-local story helper.
- `html_element` should not depend on Chrome's experimental HTML-in-canvas APIs.
- Stable ids are app-author responsibility.
- Provider-specific dynamic sizing will use explicit GPUI layout or provider cooperation such as `postMessage`.

## Risks And Edge Cases

- GPUI cannot render visually above a DOM overlay iframe because the overlay is above the canvas.
- Cross-origin iframes may consume wheel events and prevent parent GPUI scroll under the pointer.
- Some providers may pause or reset hidden iframes when parked offscreen.
- Overusing `keep_alive(true)` can leak CPU, memory, network, or active media playback.
- CSS clipping may not match complex GPUI transforms or rounded/non-rect masks in V1.
- Browser focus inside iframes may interact differently across providers.
- COOP/COEP requirements for threaded WASM can conflict with third-party iframe embedding unless providers support required headers or credentialless mode is acceptable.

## Follow-Up Items

- Consider a structured `html_iframe(...)` builder after raw trusted HTML V1 lands.
- Consider a provider-size `postMessage` helper for same-origin wrappers or cooperating providers.
- Consider DOM overlay occlusion hiding when GPUI modals/menus overlap an active `html_element`.
- Consider an explicit keep-alive resource budget or max parked roots policy.
