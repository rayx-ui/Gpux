# Hypothesis

The ordered row metadata can locate buffer offsets and visual rows with `partition_point` rather
than scanning every logical row. This removes linear lookup cost from caret, selection, hit test,
and geometry requests while preserving exact newline and final-row ownership rules.

## Scope lock

This experiment changes only `row_for_buffer_offset` and `row_for_visual_row` in `layout_map.rs`.
It does not change layout rows, cache invalidation, paint enumeration, or selection semantics.
