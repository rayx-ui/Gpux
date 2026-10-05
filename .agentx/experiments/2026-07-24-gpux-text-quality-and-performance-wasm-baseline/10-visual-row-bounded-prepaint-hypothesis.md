# Hypothesis

The layout map's sorted logical rows carry `visual_start` and `visual_len`, so prepaint can find
the logical rows intersecting the viewport plus one visual-row overscan with two partition-point
searches. Iterating only that contiguous range removes the all-logical-row scan and per-row cull
from steady scrolling.

## Scope lock

This experiment adds only a visual-row-to-logical-row range query and uses it in prepaint. It does
not change layout creation, cache reuse, edit invalidation, selection geometry, or paint order.
