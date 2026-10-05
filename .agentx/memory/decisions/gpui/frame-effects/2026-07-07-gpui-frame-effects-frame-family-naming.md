---
agentx:
  kind: decision
  scope: decisions/gpui/frame-effects
  status: current
  created: 2026-07-07T00:00:00+02:00
  updated: 2026-07-07T00:00:00+02:00
  topics:
    - runtime
    - gpux-foundation
    - frame-family
    - element-transitions
    - shader-effects
  supersedes: []
  superseded_by: []
---

## Decision: Use frame-family naming for child-operating lightweight elements

### Context: After implementing `frame()` and `effects()`, follow-up design discussion clarified that shader effects, explicit animation, and component replacement transitions should all read as frame-like boundaries that operate on a single child. The accepted follow-up epic is recorded in `.agentx/tasks/frame-family-component-transitions/`.

### Alternatives considered: Keep `effects()` as the public shader-effect element; rename only a future `effect()` API; use `transition_region()` for component replacement; put animated/component transitions under unrelated helper names; preserve `effects()` as an alias while adding `effect_frame()`.

### Reasoning: `frame()`, `effect_frame()`, `animated_frame()`, and `transition_frame()` form a consistent API family: each is a lightweight, single-child boundary that applies one kind of behavior to its child. This naming makes it harder to confuse these elements with rich `div()`/`Styled` layout, and it leaves room for future behavior frames without creating a mixed vocabulary. Because the branch favors clean end-state APIs, the implemented `effects()` element should be renamed to `effect_frame()` without a compatibility shim.

### Trade-offs accepted: Existing `effects()` call sites must be updated. Historical task docs and decisions may mention `effects()` as the implemented intermediate state, while new implementation work should target the frame-family names. `transition_frame()` starts as image-space replacement via retained snapshots; automatic shared-element morphing and arbitrary tree diffing remain future work.

### Supersedes: Constructor naming guidance from `.agentx/memory/decisions/runtime/gpux-foundation/2026-07-06-runtime-gpux-foundation-frame-effects-elements.md`; the single-child frame/effect ownership reasoning in that decision remains valid.
