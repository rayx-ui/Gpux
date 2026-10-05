# Measurement plan

Run the focused Test Suite TextArea page through headed Chromium with WebGPU at DPR 1 and DPR 2.
The test performs a visible wheel-scroll action, verifies the published frame and screenshot, and
records input-to-published-frame time. Inspect the screenshot and retain the measurement JSON.

Compare against the frozen baseline TextArea action median/p95: DPR 1 534.002/593.299 ms and DPR
2 497.957/561.084 ms. The retained map must preserve rendering and interaction while source
inspection confirms the exact-key return precedes `snapshot.text()`, `logical_lines`, and
`reusable_rows`.
