---
agentx:
  kind: decision
  scope: decisions/gpui/layout-style
  status: current
  created: 2026-08-31T06:52:44+02:00
  updated: 2026-08-31T06:52:44+02:00
  topics:
    - gpux
    - taffy
    - text
    - layout
    - performance
  supersedes: []
  superseded_by: []
---

## Decision: Keep measured-layout optimization text-owned for the wide DataTable

### Context: Stability telemetry found 882 repeated measured-layout calls per warmed frame. Owner classification was needed before choosing whether to expand generic layout APIs or optimize a concrete owner.

### Alternatives considered: Add a generic keyed measurement cache; add owner-specific cache paths for all current callers; constrain further work to text; abandon the opportunity.

### Reasoning: At both DPR 1 and DPR 2, all 882 calls are text measurements and list, uniform-list, and custom measurement each contribute zero. A generic cache would add API and invalidation complexity with no evidence from the non-text owners. `TextLayout` already owns content, run/style, wrapping, truncation, and text-system interaction, so it is the narrowest place capable of defining a sound key.

### Trade-offs accepted: Non-text measurement remains unoptimized until a workload demonstrates material cost. Text key construction must still be measured because hashing or cloning semantic inputs can cost more than the avoided cache probe.

### Supersedes: None. This refines [explicit invalidation before measured-layout reuse](2026-08-31-gpui-layout-style-measured-layout-invalidation.md).
