## Registrar-gated editable adapter synchronization

`EditableTextSelectionElement::paint` now synchronizes its participant only when the active traversal exposes a SelectionArea registrar. A standalone editable element remains local without reading the model, copying text, or rebuilding its row hit map.

The focused headed WASM/WebGPU TextArea run passed 70 repetitions across DPR 1 and DPR 2. In the 30 measured samples per DPR, input-to-published-frame p50/p95 improved from 126.2468/135.2297 ms to 63.41/89.48 ms at DPR 1, and from 133.9514/136.3979 ms to 86.43/93.75 ms at DPR 2. The dedicated SelectionArea fixture also passed at both DPRs, and the retained TextArea screenshot shows the expected scrolled 2,000-line content.

Keep this narrow registrar gate; it reuses the existing scoped context and adds no cache or additional lifecycle state.
