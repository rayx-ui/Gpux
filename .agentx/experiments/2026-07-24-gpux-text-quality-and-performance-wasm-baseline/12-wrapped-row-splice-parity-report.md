## Wrapped-row splice parity

The differential map test now covers a same-byte-length edit that reduces the edited logical row from three wrapped visual rows to two. It compares every valid UTF-8 byte boundary with a full layout update, including the row after the changed row.

Focused headed WASM/WebGPU validation of `/performance/text-area-edit` passed at DPR 1 and DPR 2 with the edited character and caret rendered correctly. `check-source-guard` and `git diff --check` also passed.

Accept: the existing splice implementation already updates following `visual_start` metadata by the visual-row delta, and the new parity coverage proves the required coordinate behavior without expanding the fast-path eligibility.
