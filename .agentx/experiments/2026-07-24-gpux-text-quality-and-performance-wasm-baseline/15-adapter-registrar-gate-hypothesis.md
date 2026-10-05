## Registrar-gated editable adapter synchronization

Only synchronize `EditableTextSelectableAdapter` while the current traversal scope exposes a SelectionArea registrar. Outside a SelectionArea, retain local ownership without reading text, layout, or rebuilding the row hit map.

Allowed production scope: `selection_area/mod.rs`, `selection_area/state.rs`, and `selection_area/editable_text_selectable_adapter.rs`.

Preserve local editing, IME, scrolling, privacy, and every SelectionArea gesture. Do not change hit-map geometry, participant reconciliation, or selection rendering in this experiment.
