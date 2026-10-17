---
agentx:
  kind: decision
  scope: decisions/gpui/accessibility
  status: current
  created: 2026-10-17T12:00:00+02:00
  updated: 2026-10-17T12:00:00+02:00
  promoted_from: gpux-a11y-devtools
  topics:
    - gpux
    - accessibility
    - accesskit
    - devtools
    - fast-mode
  supersedes: []
  superseded_by: []
---

# AccessKit Is gpux's One Semantic Tree, Built on Demand

## Decision: Screen readers, tests and agents read one AccessKit tree, built only while a source asks for it

### Context: gpux carried two semantic layers: AccessKit for screen readers and per-element DevKit nodes (the `devtools` feature) for tests and agents. The DevKit nodes cost 16% of `dashboard-clock`'s CPU with fast mode on, and RayX annotated most components twice (epic gpux-a11y-devtools, 2026-10-16).

### Alternatives considered: Keep both layers; keep the DevKit layer behind its feature; map DevKit semantics into AccessKit; make `accesskit` itself optional in `gpux`.

### Reasoning: One annotation surface (`role`, `aria_*`, `on_a11y_action`) keeps what screen readers, `semantic_query` and gpux-devtools read the same. The tree is built only while a source asks for it, so frames pay nothing otherwise:
- the platform adapter activating it (a screen reader);
- an `A11yInspection` guard (tests, agents);
- `AccessibilityMode::Always` / `GPUX_ACCESSIBILITY`.

Only the platform source reaches the adapter. The platform adapters, and the web mirror, sit behind each platform crate's `accessibility` feature. `accesskit`'s data types stay in `gpux`, because the `Element` trait Zed defines uses them.

What the tree holds:
- text drawn without an id becomes `Label` leaves numbered under their owner, so names are computed as screen readers compute them;
- while inspecting, elements with an id but no role become generic containers, which every adapter's filter drops, with a node-to-`GlobalElementId` map;
- relations name targets by `accessibility_id`;
- nodes advertise actions exactly as they handle them;
- siblings with a duplicate element id each get a node of their own, numbered in draw order (`Window::a11y_node_id`).

Fast mode replays retained views' recorded accessibility events instead of rebuilding them.

### Trade-offs accepted:
- What a component does not expose through AccessKit is invisible to agents, as it is to screen readers.
- Duplicate-id nodes hold their ids only while the duplicates keep their order.
- The web mirror, the macOS VoiceOver fork (`vendor/accesskit_macos`) and the mobile adapters are gpux's to maintain until AccessKit ships equivalents.

### Supersedes: None in gpux. In RayX it supersedes the DevKit inspection decisions (`2026-10-16-tooling-devkit-diagnostics-gpux-devtools-extension.md`) and the WASM-unsupported AccessKit contract (`2026-10-17-ui-accessibility-gpux-screen-readers-everywhere.md`).
