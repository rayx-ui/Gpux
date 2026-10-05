---
agentx:
  kind: decision
  scope: decisions/gpui/layout-style
  status: current
  created: 2026-08-31T12:32:12+02:00
  updated: 2026-08-31T12:32:12+02:00
  topics:
    - gpux
    - prepaint
    - paint
    - performance
    - memory
  supersedes: []
  superseded_by: []
---

## Decision: Handoff static styles through a window-owned draw-local store

### Context: Diagnostics proved that 729 of 845 warmed paint style computations exactly repeat the final prepaint result. Carrying `Style` inline would enlarge every `Interactivity` substantially, while putting it in public element prepaint state would force a RayX-wide API migration.

### Alternatives considered: Keep recomputing during paint; add `Style` directly to `Interactivity`; change every public element prepaint/paint state; allocate a box per eligible element; retain styles across frames.

### Reasoning: One capacity-retaining vector on `Window` keeps ownership local to the draw lifecycle and adds only an optional index to eligible elements. Structural gates reject every focus, hover, active, and group refinement, while the active-drag gate preserves hitbox-dependent drag-over styles, predicates, and cursor behavior. Clearing owned values at each draw prevents cross-frame invalidation state. The warmed workload reused all 729 eligible styles, improved paced p95 at both DPRs, and reduced normalized style construction/refinement profile work by roughly 47–64% without changing public APIs or runtime invariants.

### Trade-offs accepted: The warmed window retains roughly 519 KiB of vector capacity at its observed maximum, and acknowledged throughput at DPR 1 moved -0.69% within process spread. Allocator profile samples were mixed, so the accepted performance claim is narrower: repeated style construction/refinement and drop work are removed, with better measured tail latency.

### Supersedes: None.
