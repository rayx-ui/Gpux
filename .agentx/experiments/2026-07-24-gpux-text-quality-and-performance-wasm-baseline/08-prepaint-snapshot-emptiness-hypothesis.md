# Hypothesis

Editable-text prepaint currently materializes the complete `TextBufferSnapshot` solely to decide
whether a placeholder should be drawn. Using the snapshot's constant-time `is_empty` query removes
that allocation from every prepaint without changing layout, paint, selection, or placeholder
semantics.

## Scope lock

This experiment changes only the placeholder emptiness check in editable-text prepaint. It does
not change row iteration, culling, cache retention, or editing behavior.
