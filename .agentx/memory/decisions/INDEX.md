---
agentx:
  kind: index
  scope: decisions
  status: current
  created: 2026-10-05T23:00:00+02:00
  updated: 2026-10-09T17:00:00+02:00
  topics:
    - decisions
    - gpux
  supersedes: []
  superseded_by: []
---

# Gpux Decision Index

Durable architecture choices for gpux. Current source remains the first evidence to inspect. Decisions before 2026-10-05 were written while gpux lived in RayX; see the [transplant decision](repository/agentx-memory/2026-10-05-repository-agentx-memory-taxonomy-transplant.md). RayX keeps [AD-002 gpux remains the GPUI base](../../../../RayX/.agentx/memory/decisions/runtime/gpux-foundation/2026-06-24-runtime-gpux-foundation-gpux-base.md) and [gpux_color as the canonical Color owner](../../../../RayX/.agentx/memory/decisions/runtime/workspace/2026-07-13-runtime-workspace-gpux-color-owner.md).

## Domains

- gpui
  - [Use registry Taffy 0.14.0 for gpux layout](gpui/layout-style/2026-08-30-gpui-layout-style-registry-taffy-0-14.md)
  - [Vendor Taffy with two inline child slots](gpui/layout-style/2026-08-30-gpui-layout-style-taffy-inline-children.md) (superseded)
  - [Reuse pre-resolution styles unless sticky publication can change them](gpui/layout-style/2026-08-31-gpui-layout-style-conditional-prepaint-style-recompute.md)
  - [Let gpux own scroll extents without Taffy content-size tracking](gpui/layout-style/2026-08-31-gpui-layout-style-disable-taffy-content-size.md)
  - [Handoff static styles through a window-owned draw-local store](gpui/layout-style/2026-08-31-gpui-layout-style-frame-bounded-prepaint-style-handoff.md)
  - [Require explicit invalidation before reusing measured-layout output](gpui/layout-style/2026-08-31-gpui-layout-style-measured-layout-invalidation.md)
  - [Declare the minimum Taffy feature set explicitly](gpui/layout-style/2026-08-31-gpui-layout-style-minimum-taffy-features.md) (superseded)
  - [Handoff request styles only through gpux-owned element paths](gpui/layout-style/2026-08-31-gpui-layout-style-request-style-handoff.md)
  - [Keep measured-layout optimization text-owned for the wide DataTable](gpui/layout-style/2026-08-31-gpui-layout-style-text-measurement-scope.md)
  - [Add explicit unclipped deferred overlays](gpui/frame-pipeline/2026-06-29-gpui-frame-pipeline-unclipped-deferred-overlays.md)
  - [Cache thermal state for frame throttling](gpui/frame-pipeline/2026-07-24-gpui-frame-pipeline-cached-thermal-frame-throttling.md)
  - [Request upstream's follow-up frame from frame demand without App re-entry](gpui/frame-pipeline/2026-10-05-gpui-frame-pipeline-frame-demand-follow-up.md)
  - [Order fixed and sticky divs through shared positioned paint and hit-test lanes](gpui/positioning-scroll/2026-07-04-gpui-positioning-scroll-fixed-sticky-positioned-scene-order.md)
  - [Implement fixed and sticky positioning in GPUI element lifecycle](gpui/positioning-scroll/2026-07-04-gpui-positioning-scroll-fixed-sticky-positioning.md)
  - [Clamp and consume nested scroll wheel input in the active scrollport](gpui/positioning-scroll/2026-07-04-gpui-positioning-scroll-nested-scroll-wheel-propagation.md)
  - [Resolve fixed divs against the active content-mask containing block](gpui/positioning-scroll/2026-07-06-gpui-positioning-scroll-fixed-containing-block.md)
  - [AD-041 share ScrollActivity between touch and programmatic scrolling](gpui/positioning-scroll/2026-07-29-gpui-positioning-scroll-shared-scroll-activity.md) (superseded)
  - [Use one canonical relation for ordinary and virtualized sticky stacking](gpui/positioning-scroll/2026-07-31-gpui-positioning-scroll-keyed-sticky-stacking.md) (superseded)
  - [Capture wheel policy belongs on the opaque hit-test owner](gpui/positioning-scroll/2026-07-31-gpui-positioning-scroll-opaque-scroll-capture-owner.md)
  - [Order active sticky hierarchies through element traversal](gpui/positioning-scroll/2026-07-31-gpui-positioning-scroll-sticky-hierarchy-traversal-order.md)
  - [Apply one push-out replacement model to every start-edge sticky relation](gpui/positioning-scroll/2026-07-31-gpui-positioning-scroll-universal-sticky-push-out.md)
  - [Publish concrete ScrollHandle thumb offsets in the input turn](gpui/positioning-scroll/2026-08-21-gpui-positioning-scroll-immediate-scrollbar-publication.md)
  - [Keep pointer ownership per PointerId and refresh capture after reconciliation](gpui/input/2026-07-30-gpui-input-pointer-capture-reconciliation.md) (superseded)
  - [Use Zed GPUI's input model unchanged and delete the gpux pointer layer](gpui/input/2026-10-05-gpui-input-upstream-input-model.md)
  - [Split lightweight frame geometry from shader effects](gpui/frame-effects/2026-07-06-gpui-frame-effects-frame-effects-elements.md)
  - [Use frame-family naming for child-operating lightweight elements](gpui/frame-effects/2026-07-07-gpui-frame-effects-frame-family-naming.md)

- renderer
  - [WGPU owns first shader-effect support and non-WGPU backends stay capability-false](renderer/wgpu/2026-07-04-renderer-wgpu-shader-effects-backend-capabilities.md)
  - [Configure alternate WGPU surface views only when the adapter supports them](renderer/wgpu/2026-07-16-renderer-wgpu-downlevel-surface-formats.md) (superseded)
  - [Prefer WebGPU and fall back to a full-ABI WebGL2 transport](renderer/wgpu/2026-08-07-renderer-wgpu-browser-backend-fallback.md)
  - [Keep RayX browser rendering WebGPU-only](renderer/wgpu/2026-08-07-renderer-wgpu-webgpu-only-browser-renderer.md) (superseded)
  - [Composite WGPU paint in encoded sRGB, like Figma, CSS and the Metal renderer](renderer/wgpu/2026-09-30-renderer-wgpu-srgb-compositing.md)

- platform
  - [RayX gpux browser apps are multithreaded by default](platform/web/2026-06-24-platform-web-multithreaded-wasm.md)
  - [Load browser SVG assets through local WASM asset futures](platform/web/2026-06-29-platform-web-wasm-svg-asset-loading.md)
  - [Gate HTML island canvas layout subtree through an explicit browser opt-in](platform/web/2026-07-01-platform-web-html-island-url-opt-in.md)
  - [Keep HTML island canonical roots under the real GPUI canvas](platform/web/2026-07-02-platform-web-html-island-canonical-roots.md)
  - [Vendor wgpu for direct HTML island element-to-texture upload](platform/web/2026-07-02-platform-web-html-island-wgpu-direct-upload.md) (superseded)
  - [Add `html_element` as a real DOM overlay system, separate from `html_island`](platform/web/2026-07-03-platform-web-html-element-dom-overlay.md)
  - [Extend `html_element` with content fit, root extra styles, class-based static CSS, and explicit wheel modes](platform/web/2026-07-03-platform-web-html-element-fit-styles-wheel.md)
  - [Initialize WASM constructors explicitly in gpui_platform](platform/web/2026-08-31-platform-web-explicit-wasm-constructors.md)
  - [Lane B drives the threaded WASM conformance host in Chromium through Playwright and asserts the backend](platform/web/2026-10-07-platform-web-browser-lane-b.md)
  - [Lane E runs a Gradle-free NativeActivity conformance host over adb](platform/mobile/2026-10-07-platform-mobile-android-lane-e.md)
  - [Lane F runs a static-library conformance host behind a minimal XcodeGen UIKit shell on the Simulator](platform/mobile/2026-10-07-platform-mobile-ios-lane-f.md)
  - [Use WGPU DX12 as the Windows native backend policy](platform/desktop/2026-07-08-platform-desktop-windows-wgpu-dx12-policy.md)
  - [Use non-stretch DXGI scaling for Windows WGPU HWND surfaces](platform/desktop/2026-07-08-platform-desktop-windows-wgpu-dxgi-scaling.md)

- repository
  - [Keep gpux upstream provenance self-contained in RayX](repository/upstream-sync/2026-08-07-repository-upstream-sync-self-contained-upstream-provenance.md) (written while gpux lived in RayX; the ledgers moved here with it)
  - [Prefer upstream dependency sources over full vendoring](repository/packaging/2026-08-30-repository-packaging-upstream-dependency-sources.md) (superseded)
  - [gpux publishes to crates.io from its sibling repository as `gpux-*` packages with GPUI Rust names](repository/packaging/2026-10-05-repository-packaging-crates-io-packaging.md)
  - [gpux does not carry `zlog`, `ztracing` or `ztracing_macro`](repository/packaging/2026-10-05-repository-packaging-drop-z-crates.md)
  - [wgpu 30 from crates.io through a minimal published `gpux-wgpu` fork, with the element-image upload owned by gpux](repository/packaging/2026-10-05-repository-packaging-gpux-wgpu-fork.md)
  - [Give Gpux its own AgentX memory with a crate-ownership taxonomy, transplanted from RayX](repository/agentx-memory/2026-10-05-repository-agentx-memory-taxonomy-transplant.md)
  - [Seven test lanes named by where they can run](repository/validation/2026-10-07-repository-validation-test-lanes.md)
  - [gpux owns its runtime proof; RayX is only a consumer](repository/validation/2026-10-07-repository-validation-gpux-owns-runtime-proof.md)
  - [`gpux-testkit` is the single entry point for every lane](repository/validation/2026-10-07-repository-validation-gpux-testkit.md)
  - [Real-GPU and timing proof run locally in lane G](repository/validation/2026-10-07-repository-validation-local-gpu-lane-g.md)
