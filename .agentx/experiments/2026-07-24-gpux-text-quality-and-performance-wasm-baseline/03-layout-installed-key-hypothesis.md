# Hypothesis

When the editable buffer revision, normalized style runs, display projection, and layout style
match the installed `TextLayoutMap`, returning the installed map before text materialization will
remove the whole-document `Rope::to_string`, logical-line vector, row-cache clone, UTF-8 boundary
scan, and shaping work from ordinary prepaint updates.

## Scope lock

This experiment changes only the exact installed-key fast path in `layout_map.rs` and its cache
state. It does not attempt row splicing, direct lookup indexes, or viewport painting; those are
separate hypotheses.
