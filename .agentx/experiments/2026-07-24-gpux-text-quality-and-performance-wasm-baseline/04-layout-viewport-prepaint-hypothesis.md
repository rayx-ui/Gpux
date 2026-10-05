# Hypothesis

Selecting the logical rows that intersect the viewport with the already ordered visual-row
metadata will replace the current all-row prepaint scan. It will preserve the same visible rows
while bounding ordinary scroll work to the visible rows instead of all 2,000 lines.

## Scope lock

This experiment changes only the prepaint row enumeration in `editable_text/element.rs` and the
layout-map query that supplies the exact range. It does not change layout rebuilding, row caching,
selection geometry, or style processing.
