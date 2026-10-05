## Registrar-gated DataTable structured selection frame

Rejected and reverted. The DataTable fixture passed 70/70 headed WASM/WebGPU runs and its screenshot remained correct, but the candidate did not meet the p95 gate against the retained baseline: DPR 1 was 118.19 ms vs 113.772 ms and DPR 2 was 121.60 ms vs 113.164 ms. The source was returned to its checkpoint before this result was recorded.
