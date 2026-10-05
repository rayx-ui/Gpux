## Measurement plan

1. Confirm the renderer exposes its negotiated dual-source capability and the Windows platform query is the only unconditional `true` path.
2. Change only that query to the renderer capability snapshot.
3. Run the isolated `rayx-text-quality-performance.spec.ts` page through headed WASM WebGPU at DPR 1 and 2.
4. Inspect the emitted DPR 2 screenshot and retain its raw metrics.
5. Compare the focused test output with the frozen WASM text-quality baseline. This experiment has a no-regression expectation; it does not claim a WASM speedup.

## Acceptance

- The Windows implementation has no unconditional subpixel-support return.
- WASM remains grayscale and the focused quality page passes at both DPRs.
- No measured text-quality timing, image gate, or crash regression appears.
