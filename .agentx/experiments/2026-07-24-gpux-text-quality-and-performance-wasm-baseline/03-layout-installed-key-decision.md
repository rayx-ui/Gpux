# Decision

Accept the exact installed-key fast path. It preserves the existing layout map whenever the full
layout input is unchanged and measurably improves the fixed WASM TextArea action without changing
projection, style validation, or changed-layout behavior. Continue with a separate experiment for
retained row data and incremental changed-row layout; do not combine it with the accepted fast
path change.
