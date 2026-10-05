# Measurement plan

Run the isolated headed WASM/WebGPU TextArea wheel fixture at DPR 1 and 2 with five warmups and
thirty measured samples, inspect the DPR 2 screenshot, and retain raw timing artifacts. Source
inspection must prove steady prepaint uses a bounded range rather than `0..logical_row_count`.
