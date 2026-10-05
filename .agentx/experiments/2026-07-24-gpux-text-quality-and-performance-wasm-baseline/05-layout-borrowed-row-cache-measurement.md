# Measurement plan

Run the focused headed WASM/WebGPU TextArea fixture at both DPRs and inspect its screenshot. Use
source inspection to prove no `HashMap::clone` remains in the reusable-row path, while the
installation remains after successful shaping so an error cannot destroy the prior cache.
