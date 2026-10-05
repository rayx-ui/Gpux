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
    - gpux
  supersedes: []
  superseded_by: []
---

# Design: HTML Element DOM Overlay

## Design Summary

Add `html_element` as a web-only DOM overlay primitive integrated into GPUI layout and paint. GPUI/RayX remains responsible for layout, scroll, clipping, and frame metadata. `gpui_web` remains responsible for browser DOM creation and synchronization through `web_sys`.

The key design split is:

- `html_island`: canvas-child DOM captured into WebGPU texture, direct canvas child, Chrome HTML-in-canvas path.
- `html_element`: ordinary real browser DOM overlay above canvas, sibling overlay root, no texture upload, intended for iframes and provider-owned content.

The DOM overlay must be frame-synchronized from GPUI primitives. It must not create an independent browser scroll state for normal use.

## Current-State Summary

`gpui_web::WebWindow::new` creates one real `HtmlCanvasElement`, sizes it for DPR, appends it to `document.body`, creates a hidden input, and passes the canvas to `WgpuRenderer::new_from_canvas`.

`WebWindow::draw(scene)` currently handles pending canvas backing-size updates, syncs `html_island` roots when required, uploads dirty island textures, calls `renderer.draw(scene)`, and writes HTML-in-canvas diagnostics.

`html_island` already demonstrates the useful local pattern:

- public GPUI element in `crates/gpux/crates/gpui/src/elements/html_island.rs`
- scene primitive in `crates/gpux/crates/gpui/src/scene.rs`
- paint helper in `crates/gpux/crates/gpui/src/window.rs`
- web registry in `crates/gpux/crates/gpui_web/src/html_island.rs`
- draw-time sync in `crates/gpux/crates/gpui_web/src/window.rs`
- Playwright coverage under `tools-node/tests/rayx-html-island.spec.ts`

Scroll containers in GPUI already own scroll state. `Div::track_scroll` stores viewport bounds, content size, child bounds, and scroll offset in `ScrollHandle`. `VirtualListExternalScrollBehavior::ParentViewport` already handles the parent-page-scroll model by keeping virtualized components in normal flow, deriving visibility from the parent content mask, and avoiding a duplicated scroll offset.

## Proposed Architecture

### Core Principle

`html_element` emits browser-DOM synchronization metadata during GPUI paint. It does not draw pixels into the canvas. The web backend consumes that metadata and keeps real DOM overlay nodes aligned with the canvas.

```text
RayX render
  -> GPUI layout/prepaint/paint
    -> html_element(...) calls Window::paint_html_element(...)
      -> Scene stores HtmlElementPrimitive
        -> gpui_web::WebWindow::draw(scene)
          -> WebHtmlElementRegistry::sync_roots(...)
            -> web_sys creates/updates overlay DOM
          -> WGPU renders GPUI canvas
```

### Public API

Initial API:

```rust
html_element("sports-stream")
    .trusted_html(r#"
        <iframe
          src="https://provider.example/stream"
          title="Live stream"
          allow="autoplay; fullscreen"
          style="width:100%;height:100%;border:0;display:block"></iframe>
    "#)
    .input_mode(HtmlElementInputMode::DomOwned)
    .keep_alive(true)
    .fallback(div().child("Stream unavailable"));
```

Public concepts:

```rust
pub fn html_element(id: impl Into<ElementId>) -> HtmlElement;

pub enum HtmlElementInputMode {
    DomOwned,
    GpuiOwned,
}

pub enum HtmlElementLifetime {
    PruneWhenAbsent,
    KeepAliveHidden,
}
```

`keep_alive(true)` can be sugar over `HtmlElementLifetime::KeepAliveHidden`.

Future structured API:

```rust
html_iframe("deposit-provider")
    .src(payment_url)
    .title("Add funds")
    .sandbox("allow-forms allow-scripts allow-popups allow-same-origin")
    .allow("payment; clipboard-write")
    .referrer_policy("strict-origin-when-cross-origin")
    .keep_alive(true);
```

The structured iframe builder is useful but not required for V1.

### GPUI Element

Add:

- `crates/gpux/crates/gpui/src/elements/html_element.rs`
- export from `crates/gpux/crates/gpui/src/elements/mod.rs`

The element should be patterned after `HtmlIsland`, but it should call a new `Window::paint_html_element(...)` helper instead of `paint_html_island(...)`.

Sketch:

```rust
pub struct HtmlElement {
    id: ElementId,
    trusted_html: SharedString,
    input_mode: HtmlElementInputMode,
    lifetime: HtmlElementLifetime,
    fallback: Option<AnyElement>,
    style: StyleRefinement,
}

impl Element for HtmlElement {
    type RequestLayoutState = Style;
    type PrepaintState = ();

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Style,
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(fallback) = self.fallback.as_mut() {
            fallback.paint(window, cx);
            return;
        }

        let id = self.id.clone();
        let input_mode = self.input_mode;
        let lifetime = self.lifetime;
        let content_revision = trusted_html_revision(&self.trusted_html);
        let trusted_html = self.trusted_html.clone();

        style.paint(bounds, window, cx, move |window, _cx| {
            window.paint_html_element(
                id,
                bounds,
                input_mode,
                lifetime,
                content_revision,
                trusted_html,
            );
        });
    }
}
```

### Scene Primitive

Add to `crates/gpux/crates/gpui/src/scene.rs`:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlElementInputMode {
    DomOwned,
    GpuiOwned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlElementLifetime {
    PruneWhenAbsent,
    KeepAliveHidden,
}

#[derive(Clone, Debug)]
pub struct HtmlElementPrimitive {
    pub order: DrawOrder,
    pub id: ElementId,
    pub bounds: Bounds<ScaledPixels>,
    pub logical_bounds: Bounds<Pixels>,
    pub logical_content_mask: ContentMask<Pixels>,
    pub input_mode: HtmlElementInputMode,
    pub lifetime: HtmlElementLifetime,
    pub opacity: f32,
    pub content_revision: u64,
    pub trusted_html: SharedString,
}
```

The scene should store `pub html_elements: Vec<HtmlElementPrimitive>`.

Unlike `HtmlIslandPrimitive`, `HtmlElementPrimitive` does not need to participate in `PrimitiveBatch` unless a future renderer wants ordered canvas/DOM occlusion diagnostics. V1 can keep it as scene side metadata only. If it does become a `Primitive`, renderers must skip drawing it.

### Window Paint Helper

Add `Window::paint_html_element(...)` near `paint_html_island(...)`:

```rust
pub fn paint_html_element(
    &mut self,
    id: ElementId,
    bounds: Bounds<Pixels>,
    input_mode: HtmlElementInputMode,
    lifetime: HtmlElementLifetime,
    content_revision: u64,
    trusted_html: SharedString,
) {
    self.invalidator.debug_assert_paint();

    let scale_factor = self.scale_factor();
    let transformation = self.current_element_transform();
    let scaled_bounds = bounds.scale(scale_factor);

    let bounds = if transformation == TransformationMatrix::unit() {
        self.snap_bounds(bounds)
    } else {
        self.transform_scaled_bounds(scaled_bounds)
    };

    let logical_bounds = bounds.map(|coordinate| px(coordinate.0 / scale_factor));
    let content_mask = if transformation == TransformationMatrix::unit() {
        self.snapped_content_mask()
    } else {
        self.paint_content_mask()
    };

    let logical_content_mask = ContentMask {
        bounds: content_mask
            .bounds
            .map(|coordinate| px(coordinate.0 / scale_factor)),
    };

    self.next_frame.scene.html_elements.push(HtmlElementPrimitive {
        order: 0,
        id,
        bounds,
        logical_bounds,
        logical_content_mask,
        input_mode,
        lifetime,
        opacity: self.element_opacity(),
        content_revision,
        trusted_html,
    });
}
```

The helper intentionally captures logical bounds and logical content mask so `gpui_web` can sync CSS pixel geometry directly.

### Web Overlay Root

Add `crates/gpux/crates/gpui_web/src/html_element.rs`.

Add fields to `WebWindowInner`:

```rust
pub(crate) html_element_registry: Rc<RefCell<WebHtmlElementRegistry>>,
pub(crate) html_element_overlay_root: web_sys::HtmlElement,
```

Create the overlay root in `WebWindow::new` after the canvas is created and appended:

```rust
let overlay_root: web_sys::HtmlElement = document
    .create_element("div")
    .map_err(|e| anyhow::anyhow!("Failed to create HTML element overlay root: {e:?}"))?
    .dyn_into()
    .map_err(|e| anyhow::anyhow!("Overlay root is not HtmlElement: {e:?}"))?;

overlay_root
    .set_attribute("data-rayx-html-element-overlay", "true")
    .ok();

let style = overlay_root.style();
style.set_property("position", "fixed").ok();
style.set_property("inset", "0").ok();
style.set_property("overflow", "hidden").ok();
style.set_property("pointer-events", "none").ok();
style.set_property("z-index", "2147483647").ok();

body.append_child(&overlay_root)
    .map_err(|e| anyhow::anyhow!("Failed to append HTML element overlay root: {e:?}"))?;
```

The root uses `pointer-events: none`; individual content roots opt back into pointer handling for `DomOwned`.

### Registry DOM Shape

Each root:

```html
<div data-rayx-html-element-id="sports-stream">
  <div data-rayx-html-element-content="true">
    <iframe ...></iframe>
  </div>
</div>
```

Rust shape:

```rust
struct WebHtmlElementRoot {
    clip_element: web_sys::HtmlElement,
    content_element: web_sys::HtmlElement,
    input_mode: HtmlElementInputMode,
    lifetime: HtmlElementLifetime,
    content_revision: u64,
    last_geometry: Option<HtmlElementGeometry>,
    last_nonzero_size: Size<Pixels>,
    seen_this_frame: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct HtmlElementGeometry {
    clip: Bounds<Pixels>,
    element: Bounds<Pixels>,
    canvas_left: Pixels,
    canvas_top: Pixels,
    visible: bool,
    opacity: f32,
    input_mode: HtmlElementInputMode,
}
```

### Draw-Time Sync

Add near the existing `html_island` sync in `WebWindow::draw(scene)`:

```rust
let canvas_rect = self.inner.canvas.get_bounding_client_rect();
self.inner
    .html_element_registry
    .borrow_mut()
    .sync_roots(&scene.html_elements, &canvas_rect);

self.inner.renderer.borrow_mut().draw(scene);
```

This should run after pending canvas size updates are applied. It can run before or after `renderer.draw(scene)` as long as it receives the same frame scene and final canvas rect; before draw is preferred for single-frame visual coherence.

### Content Sync

Content sync must be revision-gated:

```rust
if root.content_revision != primitive.content_revision {
    root.content_element.set_inner_html(&primitive.trusted_html);
    root.content_revision = primitive.content_revision;
    diagnostics.content_updates += 1;
}
```

No scroll, size, visibility, or opacity update may call `set_inner_html(...)`.

### Geometry Sync

Geometry sync is frame-driven and independent from content sync:

```rust
fn sync_geometry(
    &mut self,
    primitive: &HtmlElementPrimitive,
    canvas_rect: &web_sys::DomRect,
) {
    let clip = primitive
        .logical_bounds
        .intersect(&primitive.logical_content_mask.bounds);

    let geometry = HtmlElementGeometry {
        clip,
        element: primitive.logical_bounds,
        canvas_left: px(canvas_rect.left() as f32),
        canvas_top: px(canvas_rect.top() as f32),
        visible: !clip.is_empty(),
        opacity: primitive.opacity,
        input_mode: primitive.input_mode,
    };

    if self.last_geometry == Some(geometry) {
        return;
    }

    self.last_geometry = Some(geometry);

    if !geometry.visible {
        self.park_for_invisible_frame();
        return;
    }

    self.last_nonzero_size = primitive.logical_bounds.size;

    set_px(&self.clip_element, "left", geometry.canvas_left + clip.left());
    set_px(&self.clip_element, "top", geometry.canvas_top + clip.top());
    set_px(&self.clip_element, "width", clip.size.width);
    set_px(&self.clip_element, "height", clip.size.height);

    set_px(&self.content_element, "width", primitive.logical_bounds.size.width);
    set_px(&self.content_element, "height", primitive.logical_bounds.size.height);

    let local_x = primitive.logical_bounds.left() - clip.left();
    let local_y = primitive.logical_bounds.top() - clip.top();

    self.content_element
        .style()
        .set_property("transform", &format!("translate({local_x}px, {local_y}px)"))
        .ok();

    self.clip_element.style().set_property("visibility", "visible").ok();
    self.clip_element.style().set_property("opacity", &primitive.opacity.to_string()).ok();

    self.content_element
        .style()
        .set_property(
            "pointer-events",
            match primitive.input_mode {
                HtmlElementInputMode::DomOwned => "auto",
                HtmlElementInputMode::GpuiOwned => "none",
            },
        )
        .ok();
}
```

This covers:

- parent scroll
- nested scroll mask changes
- element size changes
- viewport resize
- canvas rect movement
- opacity changes
- input mode changes

### Lifecycle And Keep Alive

Registry frame algorithm:

```rust
for root in self.roots.values_mut() {
    root.seen_this_frame = false;
}

for primitive in elements {
    let root = self.root_for_or_create(primitive.id.clone());
    root.seen_this_frame = true;
    root.lifetime = primitive.lifetime;
    root.input_mode = primitive.input_mode;
    root.sync_content(primitive);
    root.sync_geometry(primitive, canvas_rect);
}

let stale_ids = self
    .roots
    .iter()
    .filter_map(|(id, root)| (!root.seen_this_frame).then(|| id.clone()))
    .collect::<Vec<_>>();

for id in stale_ids {
    let keep_alive = self
        .roots
        .get(&id)
        .is_some_and(|root| root.lifetime == HtmlElementLifetime::KeepAliveHidden);

    if keep_alive {
        self.roots.get_mut(&id).unwrap().park_hidden();
    } else if let Some(root) = self.roots.remove(&id) {
        root.remove();
    }
}
```

Parking CSS:

```rust
fn park_hidden(&self) {
    let style = self.clip_element.style();
    style.set_property("visibility", "hidden").ok();
    style.set_property("opacity", "0").ok();
    style.set_property("pointer-events", "none").ok();
    style.set_property("transform", "translate(-100000px, -100000px)").ok();
}
```

Do not use `display: none` for keep-alive provider iframes because providers may pause, unload, or reinitialize.

### Scroll Integration

No new DOM scroller is needed for V1. The GPUI side already performs scroll through `with_element_offset` and `with_content_mask`. By the time `HtmlElementPrimitive` is produced, it has current painted bounds and intersected content mask.

For parent-scroll virtualization, the same principle applies: virtualized items are painted at current GPUI positions and clipped by the parent content mask. The overlay registry follows that frame output.

### Input Behavior

For `DomOwned`:

- content child uses `pointer-events: auto`
- browser handles pointer, focus, keyboard, iframe controls, and accessibility
- canvas listeners do not receive events targeted to the overlay sibling

For `GpuiOwned`:

- content child uses `pointer-events: none`
- overlay root remains `pointer-events: none`
- canvas receives pointer events normally

If future code adds event listeners above the canvas or document-level interception, add a registry target lookup helper similar to `html_island` so dom-owned overlay targets are excluded from GPUI default-prevention paths.

### Diagnostics

Add `WebHtmlElementDiagnostics`:

```rust
pub(crate) struct WebHtmlElementDiagnostics {
    pub(crate) source: &'static str,
    pub(crate) overlay_root_present: bool,
    pub(crate) active_root_count: u32,
    pub(crate) parked_root_count: u32,
    pub(crate) created_root_count: u32,
    pub(crate) pruned_root_count: u32,
    pub(crate) content_update_count: u32,
    pub(crate) geometry_update_count: u32,
    pub(crate) geometry_skip_count: u32,
    pub(crate) dom_owned_count: u32,
    pub(crate) gpui_owned_count: u32,
    pub(crate) keep_alive_count: u32,
    pub(crate) last_geometry_sample: Option<HtmlElementGeometrySample>,
    pub(crate) last_error: Option<String>,
}
```

Expose as `globalThis.__rayxHtmlElementDiagnostics`.

Do not merge with `__rayxHtmlIslandDiagnostics`; the capabilities and failure modes are different.

### Lab Story

Add `HtmlElementStory` as a separate story from `HtmlIslandStory`. It should not depend on `CanvasDrawElement` or Canary.

Examples:

- Basic DOM overlay: simple styled HTML block.
- Iframe: `srcdoc` same-origin fixture for deterministic tests.
- External iframe: allowed provider-like external page with documented caveats.
- YouTube/video iframe: real-world media/player behavior.
- Keep alive: counter/timestamp plus iframe identity proof that root survives rerenders/offscreen absence.
- Parent scroll: item inside StoryContainer scroll area.
- Parent scroll virtualization: row/item in a `ParentViewport` virtualized list.
- Size sync: GPUI buttons change width/height, DOM iframe stays alive.
- GPUI-owned passthrough: overlay visual does not block GPUI button/click beneath.

### Testing Strategy

Unit/native:

- GPUI element fallback behavior.
- Primitive contains logical bounds and logical content mask.
- Scene clears/sorts/stores html element metadata.
- Revision hash changes when HTML changes.

Web registry:

- create/reuse by stable id
- content revision gates `set_inner_html`
- geometry sync updates CSS without content replacement
- keep-alive parks instead of removing
- default lifecycle prunes
- input mode toggles pointer events

Playwright:

- normal headed WebGPU Chromium project
- optional DPR2 project
- no Canary/CanvasDrawElement dependency
- assert overlay root exists as canvas sibling
- assert active/parked/pruned diagnostics
- assert iframe DOM node identity survives rerender
- assert size changes update DOM rect without iframe replacement
- assert scrolling updates DOM rect/clipping
- assert keep-alive root parks and restores
- assert GPUI-owned passthrough works
- assert normal `html_island` Canary tests still pass unchanged

### Validation Commands

Expected implementation validation:

- `cargo fmt --all`
- targeted GPUI/gpui_web/gpui tests
- `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu`
- `pnpm exec tsc --noEmit --pretty false` from `tools-node/`
- targeted Playwright HTML element overlay spec
- existing `pnpm run test:wasm:html-island` when `html_island` code or shared web event code is touched

### Alternatives Considered

#### Use `html_island` for iframes

Rejected. Current Chrome HTML-in-canvas intentionally does not paint cross-origin iframe contents into canvas, and payments/game/video iframes should remain provider-owned browser DOM.

#### Duplicate DOM scroll containers

Rejected as the default. It creates two independent scroll states, fights GPUI scroll behavior, and duplicates virtualization semantics. GPUI already owns scroll; the DOM overlay should mirror GPUI frame geometry.

#### Build DOM through a Rust HTML parser crate

Rejected for runtime DOM creation. `web_sys` is the correct WASM browser DOM layer. Parser/sanitizer crates may be useful before runtime, but they should not own browser element creation.

#### Make it a RayX app-local component

Rejected. The feature requires GPUI paint content masks, frame scene metadata, and `gpui_web` DOM ownership. It belongs in gpux with RayX stories/tests proving behavior.

#### Use WebView components

Rejected. Native WebView overlays solve different targets and do not integrate with GPUI WASM canvas layout/scroll geometry.

## Rollout

- Land hidden/experimental API first.
- Add Lab story and Playwright tests before treating behavior as usable.
- Document iframe/provider limitations prominently.
- Keep `html_island` and `html_element` stories/tests separate.
- Consider a later feature flag or app-builder option if applications need to disable overlay DOM globally.
