# Hypothesis

`EditableTextState::update_layout` materializes `snapshot.text()` before calling the display
transform, even when the default identity transform necessarily returns no projection. Bypassing
that materialization on the identity branch removes a whole-buffer allocation from the ordinary
TextArea layout path while preserving transformed display text behavior.

## Scope lock

This experiment changes only identity-transform projection selection. It does not alter
incremental row splicing, cache behavior, prepaint, or transformed projection mapping.
