## Full editable-layout boundary parity

The 2,000-line same-length edit differential test now compares every valid UTF-8 byte boundary, rather than five selected offsets, against a complete layout rebuild. Focused headed WASM/WebGPU TextArea editing passed at DPR 1 and DPR 2; source guard and whitespace validation passed.

Accept. This strengthens the retained-layout oracle without altering production behavior.
