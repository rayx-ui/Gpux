# Measurement plan

Run the page-isolated Test Suite TextArea wheel-scroll fixture headed with WebGPU at DPR 1 and DPR
2 for 35 runs per scale, discarding five warmups. Verify physical-pixel change and inspect the DPR
2 screenshot. Compare p50/p95 with the frozen baseline and the accepted installed-key result.

Source inspection will prove the prepaint loop receives only the binary-search-selected row range
and no longer starts from `0..logical_row_count()`.
