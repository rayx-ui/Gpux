# Hypothesis

For an identity-projection edit that replaces bytes within one logical row without changing that
row's byte length or introducing a newline, the layout map can retain every unaffected `LayoutRow`
and only reshape/splice the changed row. Any style, projection, line-count, or geometry condition
outside that narrow invariant falls back to the existing full update.

## Scope lock

This experiment only carries a precise `TextEdit` from editable state to layout and splices the
single safe row. It does not generalize insertion/deletion, multi-line changes, projections, or
font/style invalidation.
