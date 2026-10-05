## Revision-gated editable SelectionArea hit map

Accepted. The editable adapter now rebuilds row hits only after text or geometry changes, retaining the exact prior map on unchanged in-area paint frames. A dedicated 2,000-line TextArea-in-SelectionArea fixture passed 70/70 headed WASM/WebGPU runs and its screenshot retained the expected scrolled content.

Against the same checkpoint fixture, DPR 1 p50/p95 improved from 126.57/137.49 ms to 91.81/127.03 ms; DPR 2 improved from 134.01/135.71 ms to 79.51/89.48 ms.
