# Direct editable TextLayout row lookup

The focused headed WASM/WebGPU Test Suite TextArea page passed at DPR 1 and DPR 2 with the expected
nonzero scroll pixel deltas. `row_for_buffer_offset` and `row_for_visual_row` now use the ordered
row starts with `partition_point`, then retain the former membership predicates for LF ownership,
EOF, and visual-row end boundaries. The previous whole-row `find_map` and `find` scans are gone.
