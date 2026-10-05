# Hypothesis

Iterating unprojected `TextBufferSnapshot` logical-line ranges directly removes the whole
`Rope::to_string()` materialization and the temporary logical-line vector. Projected display text
continues to use its already-owned projection string, but also streams `split('\n')` instead of
collecting rows first.

## Scope lock

This experiment changes snapshot line-range access and layout-map line iteration only. It does not
attempt retained-row splicing, cache invalidation, row indexes, or paint behavior.
