---
agentx:
  kind: decision
  scope: decisions/gpui/positioning-scroll
  status: current
  created: 2026-08-21T19:45:00+02:00
  updated: 2026-08-21T19:45:00+02:00
  topics:
    - gpux
    - scrollbar
    - direct-input
    - invalidation
  supersedes: []
  superseded_by: []
---

## Decision: Publish concrete ScrollHandle thumb offsets in the input turn

### Context: Direct thumb input queued an offset and invalidated the current view. SliverView previewed that pending offset during prepaint, tracked-scroll publication accepted it later in the same frame, and publication then deferred another owner notification so render-time metrics could catch up. A headed WASM CPU profile identified flex layout as the largest named active cost, and the stable Sliver benchmark exposed the extra acknowledgment frame as material thumb latency.

### Alternatives considered: Keep the queued preview path; raise the Sliver scrollbar sample rate; suppress the post-prepaint owner notification; publish every scrollbar-handle implementation immediately; publish only concrete `ScrollHandle` requests immediately while retaining queued defaults for other handles.

### Reasoning: A concrete `ScrollHandle` already owns accepted viewport bounds and maximum offset during pointer dispatch. Publishing against that geometry before render makes the accepted metrics observable in the single invalidated owner frame and safely notifies unique dependents in the same turn. Removing notification after queued publication would leave render-time metric consumers stale, while forcing immediate publication on list-specific handles would cross their own geometry/ownership contracts. The both-DPR benchmark improved acknowledged throughput by 56.8% and 58.5%, catch-up latency by 21.0% and 27.7%, and thumb lag by 29.9% and 50.8%, with exact endpoints.

### Trade-offs accepted: Direct `ScrollHandle` thumb input clamps against the last accepted viewport geometry rather than waiting for the next prepaint. Other handle implementations retain the queued hook default, and layout, programmatic, touch, and momentum mutations remain deferred.

### Supersedes: None.
