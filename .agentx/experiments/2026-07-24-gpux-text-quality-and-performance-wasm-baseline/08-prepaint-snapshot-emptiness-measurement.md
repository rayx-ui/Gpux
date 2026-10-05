# Measurement plan

Run the existing page-isolated headed WASM/WebGPU TextArea wheel fixture at DPR 1 and 2 with five
warmups and thirty measured samples each. Inspect its DPR 2 screenshot and retain raw timing JSON.
Source inspection must prove prepaint no longer calls `snapshot.text()`.
