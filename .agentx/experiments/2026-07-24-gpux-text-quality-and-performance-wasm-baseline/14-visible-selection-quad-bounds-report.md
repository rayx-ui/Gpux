## Visible selection-quad bounds

No production change is accepted. Inspection confirms `editable_text_selection_quads` already derives `first_visible_row` and `visible_row_end`, intersects them with the selected display range, and enumerates only that bounded range.

The new dedicated drag- and keyboard-selection Test Suite pages passed focused headed WASM/WebGPU smoke validation. The DPR 2 keyboard screenshot visibly contains the selected range. Keyboard selection recorded 243.7300 ms at DPR 1 and 272.0270 ms at DPR 2 in the focused samples.

Decision: retain the existing simple bounded implementation; another cache or index would not remove demonstrated work.
