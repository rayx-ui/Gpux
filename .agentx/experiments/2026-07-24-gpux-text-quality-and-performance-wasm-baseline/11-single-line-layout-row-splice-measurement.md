# Measurement plan

Add a deterministic differential test that applies a same-length single-line edit to a 2,000-line
buffer and compares offset/position/visual-row parity with the full update. Run the focused headed
WASM/WebGPU TextArea editing fixture at both DPRs and inspect its screenshot; retain raw timing and
work-counter artifacts.
