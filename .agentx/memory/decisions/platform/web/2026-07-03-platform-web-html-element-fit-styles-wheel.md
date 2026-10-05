---
agentx:
  kind: decision
  scope: decisions/platform/web
  status: current
  created: 2026-07-03T22:36:41+02:00
  updated: 2026-07-03T22:36:41+02:00
  topics:
    - runtime
    - gpux-foundation
    - html-element
    - wasm
    - iframe
    - wheel
  supersedes: []
  superseded_by: []
---

## Decision: Extend `html_element` with content fit, root extra styles, class-based static CSS, and explicit wheel modes

### Context: Real provider iframes need responsive sizing and decoration without reloading trusted HTML, and many overlay roots made the DOM noisy because common root/content styles were repeated inline. Browser wheel behavior also needed an explicit API because DOM-owned iframes can prevent GPUI parent scrolling under the pointer.

### Alternatives considered: Add duplicate width/height/aspect APIs to `html_element`; mutate trusted iframe markup for every fit case; keep all root CSS inline; expose only a two-state wheel enum; silently try to forward cross-origin iframe wheel events from the parent document.

### Reasoning: `HtmlElement` already implements GPUI `Styled`, so layout belongs to existing width, max-width, flex, and `aspect_ratio` APIs. `HtmlElementContentFit::Fill` is a browser-side content policy that makes top-level trusted DOM children fill the GPUI layout box without changing the HTML payload. `extra_styles(...)` applies trusted decoration CSS to the root clipping element and is independent from `content_revision`, so borders/radius/background do not reload iframes. Static overlay/root/content/capture CSS belongs in one per-window stylesheet, leaving inline styles for dynamic geometry/state and caller extra declarations. Wheel forwarding needs three modes because parent documents cannot observe wheel events inside cross-origin iframe browsing contexts; `CaptureForward` is the explicit trade-off when parent scrolling must win over iframe pointer interaction.

### Trade-offs accepted: `content_fit(Fill)` is intentionally simple and affects top-level trusted DOM children rather than measuring cross-origin intrinsic iframe size. `extra_styles` is trusted CSS and reserved geometry/state properties are overwritten by runtime sync. `CaptureForward` works over iframes by intercepting pointer hit testing with a transparent layer, so it should not be the default for fully interactive players.

### Supersedes: None
