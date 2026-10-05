## Decision

Use this WASM-only baseline for subsequent atomic experiments. Prioritize a single TextArea-scroll hypothesis first because it is decisively slower than the other semantic actions. Keep the current fixed fixtures and per-action visual-difference guard unchanged so later results remain comparable.

For long sample collections, run affected page fixtures in fresh narrow Playwright processes when a full combined process exhibits browser-context churn. Do not classify the Chromium closure as a product improvement or regression without a reproducible fixture-state failure.
