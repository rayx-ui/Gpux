### Measurement plan

Measure the existing 2,000-line TextArea fixture with headed WASM/WebGPU at DPR 1 and DPR 2 using 35 repetitions per project, discarding the five configured warmups. Compare p50 and p95 input-to-published-frame durations before and after the gate.

Run the focused TextArea and SelectionArea fixtures after the change. Inspect the DPR 2 TextArea screenshot and retain its JSON/image artifacts. The static gate is that adapter synchronization is unreachable when no registrar is present.
