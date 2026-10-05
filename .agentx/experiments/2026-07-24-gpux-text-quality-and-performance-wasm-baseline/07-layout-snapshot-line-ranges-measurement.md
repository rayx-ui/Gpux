# Measurement plan

Run the focused headed WASM/WebGPU TextArea fixture at both DPRs, inspect the screenshot, and
record the action timing. Source inspection must prove the unprojected branch reaches the shaper
without `snapshot.text()` or a logical-line vector, while the projection branch preserves display
to buffer mapping.
