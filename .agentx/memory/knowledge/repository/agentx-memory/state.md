---
agentx:
  kind: state
  scope: knowledge/repository/agentx-memory
  status: current
  created: 2026-10-05T23:00:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - agentx-memory
    - taxonomy
    - transplant
    - epics
    - experiments
  supersedes: []
  superseded_by: []
---
# AgentX Memory State

## Purpose And Usage

Read before maintaining this memory tree or resuming a transplanted epic or experiment loop.

## Current Technical Shape

The taxonomy has four domains derived from the repository: `gpui` (the framework crate), `renderer` (`gpui_wgpu`, `gpux_color`), `platform` (web, desktop, mobile crates), and `repository` (upstream sync, packaging, validation, this governance subdomain). Decisions mirror the same domain/subdomain folders.

The tree was transplanted from RayX on 2026-10-05, when gpux had just moved to its own repository. RayX's `runtime/gpux-foundation` memory was redistributed here and compacted (experiment narration became knowledge facts; stale claims were corrected against source), and 44 of its 45 decisions moved here under new names. RayX keeps a slim `runtime/gpux-foundation` subdomain for its consumer view of gpux, the "gpux remains the GPUI base" decision (AD-002), and its `gpux_color` ownership decision, which RayX epics cite.

Transplanted epics in `.agentx/tasks/` (gpux fixed/sticky positioning, WGPU platform renderers, text rendering quality/performance, high-quality WASM text, HTML element overlay, HTML islands, keyed sticky stacking, shader effects, frame/effects navigation transitions, frame-family component transitions) and experiment loops in `.agentx/experiments/` (GPUX text quality and performance, its WASM baseline, WASM text rendering quality, GPUX maximum performance) are historical records written in RayX. `gpux-wgpu-platform-renderers` (in progress) and `high-quality-wasm-text-rendering` (blocked) are unfinished. RayX kept its DataTable, SelectionArea, sliver, and MediaQuery loops even where gpux code changed, because their subjects are RayX components.

## Backlinks

- [Experiment index](../../../../experiments/INDEX.json)
- 2026-10-05 current [Experiment instructions](../../../../experiments/EXPERIMENT_INSTRUCTIONS.md)
- 2026-10-05 current [Taxonomy and transplant decision](../../../decisions/repository/agentx-memory/2026-10-05-repository-agentx-memory-taxonomy-transplant.md)
- 2026-10-05 current [RayX gpux integration memory](../../../../../../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/state.md)
