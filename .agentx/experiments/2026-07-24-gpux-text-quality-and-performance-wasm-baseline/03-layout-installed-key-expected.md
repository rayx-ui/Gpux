# Expected outcome

Unchanged layout updates return `TextLayoutUpdate::default()` before whole-text and all-row work.
Changed source style runs or projection continue through validation and full layout preparation.
The focused WASM fixture passes at both configured DPRs with a nonempty, visibly changed scroll
result and no browser/runtime crash.
