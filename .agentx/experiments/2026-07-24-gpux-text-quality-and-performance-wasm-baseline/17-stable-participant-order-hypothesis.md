## Stable participant-order fast path

Avoid materializing participant IDs and membership sets when the current and next traversals have the same length and the same selectable IDs in order. Preserve the existing membership-set path whenever that direct comparison detects a change.

