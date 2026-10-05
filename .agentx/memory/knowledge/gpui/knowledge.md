---
agentx:
  kind: knowledge
  scope: knowledge/gpui
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - gpui
    - wide-datatable-benchmark
    - cpu-profile
    - performance-loop
  supersedes: []
  superseded_by: []
---
# gpui Knowledge

- The reference performance workload is RayX's Test Suite wide DataTable fixture, measured warmed in headed WASM/WebGPU at DPR 1 and DPR 2 with three independent processes per change. Its per-frame shape anchors most layout, frame, and text decisions in this domain; see [validation](../repository/validation/state.md).
- The GPUX Maximum Performance loop (2026-08-30 to 2026-08-31) ran one baseline plus 150 experiments: 111 accepted, 39 reverted. Its final profile still ranks Taffy flex layout first, then gpux request-node integration, broad `Window::draw` orchestration, bounds insertion, line painting, global-element-id allocation, style conversion, and scene insertion. The loop records are in [experiments](../../../experiments/2026-08-30-gpux-maximum-performance/_state.json) and the [sprint changelog](../../../../docs/changelog/2026-09-01-gpux-performance-improvement-sprint.md).
- A probe-free two-DPR profile measured Taffy flexbox at 618.8/697.8 ms, pooled `request_taffy_node` refresh 277.3/293.8 ms, `Window::draw` 218.2/181.2 ms, bounds insertion 149.1/138.8 ms, `Style::to_taffy` 122.1/106.9 ms, global-element-id arc allocation 96.7/131.6 ms, previous-frame element-state removal 96.8/88.8 ms, and monochrome scene insertion 93.2/74.8 ms. Treat Taffy and broad draw frames as attribution envelopes and investigate the source-confirmed integration sites independently. Element-state removal was later eliminated (see [frame-pipeline](frame-pipeline/knowledge.md)).
