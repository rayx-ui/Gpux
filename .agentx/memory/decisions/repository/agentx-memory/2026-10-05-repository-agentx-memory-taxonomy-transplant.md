---
agentx:
  kind: decision
  scope: decisions/repository/agentx-memory
  status: current
  created: 2026-10-05T23:00:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - agentx-memory
    - taxonomy
    - transplant
    - sibling-repositories
  supersedes: []
  superseded_by: []
---

## Decision: Give Gpux its own AgentX memory with a crate-ownership taxonomy, transplanted from RayX

### Context: gpux moved from RayX `crates/gpux` into this repository on 2026-10-05, but its memory, decisions, epics, and experiment loops stayed in RayX's `.agentx/` under `runtime/gpux-foundation`. Agents working in Gpux had to read another repository's memory, and that subdomain had grown into a 68 KB state file mixing current behavior with experiment narration.

### Alternatives considered: Keep gpux memory in RayX and point at it; copy RayX's `runtime/gpux-foundation` folder verbatim into a single Gpux domain; split by RayX's taxonomy (runtime/ui/platform/tooling); or derive a new taxonomy from Gpux's own crate boundaries and redistribute the content.

### Reasoning: Memory belongs with the source it describes. Gpux's crate boundaries are stable and inherited from Zed, so domains follow them: `gpui` (the framework crate, split by layout-style, frame-pipeline, positioning-scroll, input, text, and frame-effects), `renderer` (`gpui_wgpu`, `gpux_color`), `platform` (web, desktop, mobile), and `repository` (upstream-sync, packaging, validation, agentx-memory). RayX's runtime/ui/tooling split describes an application stack and would scatter one crate across domains. Redistributing allowed compaction: experiment measurements became knowledge facts backing rules, and three stale claims were corrected against source (`PaintColor` is encoded sRGB under `ClipSrgb`; surfaces use empty `view_formats` on a non-sRGB format; touch scrolling has momentum). Epics and loops whose subject is gpux moved here; RayX kept loops and epics whose subject is a RayX component even when they changed gpux code.

### Trade-offs accepted: Decision filenames changed to the new domain/subdomain prefixes, so RayX links were retargeted and historical text references in RayX epics keep the old names (map below). Transplanted epic and experiment JSON was not edited: hashes and `crates/gpux/` paths refer to RayX, and the two unfinished epics need `migrate` or a new plan before resuming. RayX keeps a slim `runtime/gpux-foundation` subdomain, AD-002, and its `gpux_color` ownership decision, so a reader sometimes follows a cross-repository link. Excluded categories: no `examples`, `ui`, or `tooling` domains, because Gpux has no apps, components, or build tooling of its own beyond `tooling/perf`.

### Supersedes: None. RayX decision index entries for the moved records now point here.

### Moved decisions

| RayX name (`decisions/runtime/gpux-foundation/`) | Gpux location |
|---|---|
| `2026-06-24-runtime-gpux-foundation-multithreaded-wasm.md` | [platform/web/2026-06-24-platform-web-multithreaded-wasm.md](../../platform/web/2026-06-24-platform-web-multithreaded-wasm.md) |
| `2026-06-29-runtime-gpux-foundation-unclipped-deferred-overlays.md` | [gpui/frame-pipeline/2026-06-29-gpui-frame-pipeline-unclipped-deferred-overlays.md](../../gpui/frame-pipeline/2026-06-29-gpui-frame-pipeline-unclipped-deferred-overlays.md) |
| `2026-06-29-runtime-gpux-foundation-wasm-svg-asset-loading.md` | [platform/web/2026-06-29-platform-web-wasm-svg-asset-loading.md](../../platform/web/2026-06-29-platform-web-wasm-svg-asset-loading.md) |
| `2026-07-01-runtime-gpux-foundation-html-island-url-opt-in.md` | [platform/web/2026-07-01-platform-web-html-island-url-opt-in.md](../../platform/web/2026-07-01-platform-web-html-island-url-opt-in.md) |
| `2026-07-02-runtime-gpux-foundation-html-island-canonical-roots.md` | [platform/web/2026-07-02-platform-web-html-island-canonical-roots.md](../../platform/web/2026-07-02-platform-web-html-island-canonical-roots.md) |
| `2026-07-02-runtime-gpux-foundation-html-island-wgpu-direct-upload.md` | [platform/web/2026-07-02-platform-web-html-island-wgpu-direct-upload.md](../../platform/web/2026-07-02-platform-web-html-island-wgpu-direct-upload.md) |
| `2026-07-03-runtime-gpux-foundation-html-element-dom-overlay.md` | [platform/web/2026-07-03-platform-web-html-element-dom-overlay.md](../../platform/web/2026-07-03-platform-web-html-element-dom-overlay.md) |
| `2026-07-03-runtime-gpux-foundation-html-element-fit-styles-wheel.md` | [platform/web/2026-07-03-platform-web-html-element-fit-styles-wheel.md](../../platform/web/2026-07-03-platform-web-html-element-fit-styles-wheel.md) |
| `2026-07-04-runtime-gpux-foundation-fixed-sticky-positioned-scene-order.md` | [gpui/positioning-scroll/2026-07-04-gpui-positioning-scroll-fixed-sticky-positioned-scene-order.md](../../gpui/positioning-scroll/2026-07-04-gpui-positioning-scroll-fixed-sticky-positioned-scene-order.md) |
| `2026-07-04-runtime-gpux-foundation-fixed-sticky-positioning.md` | [gpui/positioning-scroll/2026-07-04-gpui-positioning-scroll-fixed-sticky-positioning.md](../../gpui/positioning-scroll/2026-07-04-gpui-positioning-scroll-fixed-sticky-positioning.md) |
| `2026-07-04-runtime-gpux-foundation-nested-scroll-wheel-propagation.md` | [gpui/positioning-scroll/2026-07-04-gpui-positioning-scroll-nested-scroll-wheel-propagation.md](../../gpui/positioning-scroll/2026-07-04-gpui-positioning-scroll-nested-scroll-wheel-propagation.md) |
| `2026-07-04-runtime-gpux-foundation-shader-effects-backend-capabilities.md` | [renderer/wgpu/2026-07-04-renderer-wgpu-shader-effects-backend-capabilities.md](../../renderer/wgpu/2026-07-04-renderer-wgpu-shader-effects-backend-capabilities.md) |
| `2026-07-06-runtime-gpux-foundation-fixed-containing-block.md` | [gpui/positioning-scroll/2026-07-06-gpui-positioning-scroll-fixed-containing-block.md](../../gpui/positioning-scroll/2026-07-06-gpui-positioning-scroll-fixed-containing-block.md) |
| `2026-07-06-runtime-gpux-foundation-frame-effects-elements.md` | [gpui/frame-effects/2026-07-06-gpui-frame-effects-frame-effects-elements.md](../../gpui/frame-effects/2026-07-06-gpui-frame-effects-frame-effects-elements.md) |
| `2026-07-07-runtime-gpux-foundation-frame-family-naming.md` | [gpui/frame-effects/2026-07-07-gpui-frame-effects-frame-family-naming.md](../../gpui/frame-effects/2026-07-07-gpui-frame-effects-frame-family-naming.md) |
| `2026-07-08-runtime-gpux-foundation-windows-wgpu-dx12-policy.md` | [platform/desktop/2026-07-08-platform-desktop-windows-wgpu-dx12-policy.md](../../platform/desktop/2026-07-08-platform-desktop-windows-wgpu-dx12-policy.md) |
| `2026-07-08-runtime-gpux-foundation-windows-wgpu-dxgi-scaling.md` | [platform/desktop/2026-07-08-platform-desktop-windows-wgpu-dxgi-scaling.md](../../platform/desktop/2026-07-08-platform-desktop-windows-wgpu-dxgi-scaling.md) |
| `2026-07-16-runtime-gpux-foundation-downlevel-surface-formats.md` | [renderer/wgpu/2026-07-16-renderer-wgpu-downlevel-surface-formats.md](../../renderer/wgpu/2026-07-16-renderer-wgpu-downlevel-surface-formats.md) |
| `2026-07-24-runtime-gpux-foundation-cached-thermal-frame-throttling.md` | [gpui/frame-pipeline/2026-07-24-gpui-frame-pipeline-cached-thermal-frame-throttling.md](../../gpui/frame-pipeline/2026-07-24-gpui-frame-pipeline-cached-thermal-frame-throttling.md) |
| `2026-07-29-runtime-gpux-foundation-shared-scroll-activity.md` | [gpui/positioning-scroll/2026-07-29-gpui-positioning-scroll-shared-scroll-activity.md](../../gpui/positioning-scroll/2026-07-29-gpui-positioning-scroll-shared-scroll-activity.md) |
| `2026-07-30-runtime-gpux-foundation-pointer-capture-reconciliation.md` | [gpui/input/2026-07-30-gpui-input-pointer-capture-reconciliation.md](../../gpui/input/2026-07-30-gpui-input-pointer-capture-reconciliation.md) |
| `2026-07-31-runtime-gpux-foundation-keyed-sticky-stacking.md` | [gpui/positioning-scroll/2026-07-31-gpui-positioning-scroll-keyed-sticky-stacking.md](../../gpui/positioning-scroll/2026-07-31-gpui-positioning-scroll-keyed-sticky-stacking.md) |
| `2026-07-31-runtime-gpux-foundation-opaque-scroll-capture-owner.md` | [gpui/positioning-scroll/2026-07-31-gpui-positioning-scroll-opaque-scroll-capture-owner.md](../../gpui/positioning-scroll/2026-07-31-gpui-positioning-scroll-opaque-scroll-capture-owner.md) |
| `2026-07-31-runtime-gpux-foundation-sticky-hierarchy-traversal-order.md` | [gpui/positioning-scroll/2026-07-31-gpui-positioning-scroll-sticky-hierarchy-traversal-order.md](../../gpui/positioning-scroll/2026-07-31-gpui-positioning-scroll-sticky-hierarchy-traversal-order.md) |
| `2026-07-31-runtime-gpux-foundation-universal-sticky-push-out.md` | [gpui/positioning-scroll/2026-07-31-gpui-positioning-scroll-universal-sticky-push-out.md](../../gpui/positioning-scroll/2026-07-31-gpui-positioning-scroll-universal-sticky-push-out.md) |
| `2026-08-07-runtime-gpux-foundation-browser-backend-fallback.md` | [renderer/wgpu/2026-08-07-renderer-wgpu-browser-backend-fallback.md](../../renderer/wgpu/2026-08-07-renderer-wgpu-browser-backend-fallback.md) |
| `2026-08-07-runtime-gpux-foundation-self-contained-upstream-provenance.md` | [repository/upstream-sync/2026-08-07-repository-upstream-sync-self-contained-upstream-provenance.md](../../repository/upstream-sync/2026-08-07-repository-upstream-sync-self-contained-upstream-provenance.md) |
| `2026-08-07-runtime-gpux-foundation-webgpu-only-browser-renderer.md` | [renderer/wgpu/2026-08-07-renderer-wgpu-webgpu-only-browser-renderer.md](../../renderer/wgpu/2026-08-07-renderer-wgpu-webgpu-only-browser-renderer.md) |
| `2026-08-21-runtime-gpux-foundation-immediate-scrollbar-publication.md` | [gpui/positioning-scroll/2026-08-21-gpui-positioning-scroll-immediate-scrollbar-publication.md](../../gpui/positioning-scroll/2026-08-21-gpui-positioning-scroll-immediate-scrollbar-publication.md) |
| `2026-08-30-runtime-gpux-foundation-registry-taffy-0-14.md` | [gpui/layout-style/2026-08-30-gpui-layout-style-registry-taffy-0-14.md](../../gpui/layout-style/2026-08-30-gpui-layout-style-registry-taffy-0-14.md) |
| `2026-08-30-runtime-gpux-foundation-taffy-inline-children.md` | [gpui/layout-style/2026-08-30-gpui-layout-style-taffy-inline-children.md](../../gpui/layout-style/2026-08-30-gpui-layout-style-taffy-inline-children.md) |
| `2026-08-30-runtime-gpux-foundation-upstream-dependency-sources.md` | [repository/packaging/2026-08-30-repository-packaging-upstream-dependency-sources.md](../../repository/packaging/2026-08-30-repository-packaging-upstream-dependency-sources.md) |
| `2026-08-31-runtime-gpux-foundation-conditional-prepaint-style-recompute.md` | [gpui/layout-style/2026-08-31-gpui-layout-style-conditional-prepaint-style-recompute.md](../../gpui/layout-style/2026-08-31-gpui-layout-style-conditional-prepaint-style-recompute.md) |
| `2026-08-31-runtime-gpux-foundation-disable-taffy-content-size.md` | [gpui/layout-style/2026-08-31-gpui-layout-style-disable-taffy-content-size.md](../../gpui/layout-style/2026-08-31-gpui-layout-style-disable-taffy-content-size.md) |
| `2026-08-31-runtime-gpux-foundation-explicit-wasm-constructors.md` | [platform/web/2026-08-31-platform-web-explicit-wasm-constructors.md](../../platform/web/2026-08-31-platform-web-explicit-wasm-constructors.md) |
| `2026-08-31-runtime-gpux-foundation-frame-bounded-prepaint-style-handoff.md` | [gpui/layout-style/2026-08-31-gpui-layout-style-frame-bounded-prepaint-style-handoff.md](../../gpui/layout-style/2026-08-31-gpui-layout-style-frame-bounded-prepaint-style-handoff.md) |
| `2026-08-31-runtime-gpux-foundation-measured-layout-invalidation.md` | [gpui/layout-style/2026-08-31-gpui-layout-style-measured-layout-invalidation.md](../../gpui/layout-style/2026-08-31-gpui-layout-style-measured-layout-invalidation.md) |
| `2026-08-31-runtime-gpux-foundation-minimum-taffy-features.md` | [gpui/layout-style/2026-08-31-gpui-layout-style-minimum-taffy-features.md](../../gpui/layout-style/2026-08-31-gpui-layout-style-minimum-taffy-features.md) |
| `2026-08-31-runtime-gpux-foundation-request-style-handoff.md` | [gpui/layout-style/2026-08-31-gpui-layout-style-request-style-handoff.md](../../gpui/layout-style/2026-08-31-gpui-layout-style-request-style-handoff.md) |
| `2026-08-31-runtime-gpux-foundation-text-measurement-scope.md` | [gpui/layout-style/2026-08-31-gpui-layout-style-text-measurement-scope.md](../../gpui/layout-style/2026-08-31-gpui-layout-style-text-measurement-scope.md) |
| `2026-09-30-runtime-gpux-foundation-srgb-compositing.md` | [renderer/wgpu/2026-09-30-renderer-wgpu-srgb-compositing.md](../../renderer/wgpu/2026-09-30-renderer-wgpu-srgb-compositing.md) |
| `2026-10-05-runtime-gpux-foundation-crates-io-packaging.md` | [repository/packaging/2026-10-05-repository-packaging-crates-io-packaging.md](../../repository/packaging/2026-10-05-repository-packaging-crates-io-packaging.md) |
| `2026-10-05-runtime-gpux-foundation-drop-z-crates.md` | [repository/packaging/2026-10-05-repository-packaging-drop-z-crates.md](../../repository/packaging/2026-10-05-repository-packaging-drop-z-crates.md) |
| `2026-10-05-runtime-gpux-foundation-gpux-wgpu-fork.md` | [repository/packaging/2026-10-05-repository-packaging-gpux-wgpu-fork.md](../../repository/packaging/2026-10-05-repository-packaging-gpux-wgpu-fork.md) |
