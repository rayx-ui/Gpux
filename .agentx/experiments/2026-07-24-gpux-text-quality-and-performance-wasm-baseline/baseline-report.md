## WASM WebGPU semantic-action baseline

All five page-isolated Test Suite fixtures were measured headed at DPR 1 and DPR 2. Each cell has three fresh replicates, five warmups per replicate, and ninety retained measured samples. See `baseline-measurements.json` for the exact distributions.

The visual captures are valid: the light/dark text specimen is readable, the TextArea scroll moved through the fixed 2,000-line workload, and the shell interaction changed only the shell fixture. Every action expected to change pixels had a nonzero physical-pixel difference.

The dominant baseline is TextArea scrolling: p50 534.002 ms at DPR 1 and 497.957 ms at DPR 2. DataTable and SelectionArea are roughly 80–106 ms p50, while text-quality capture and shell isolation are below 30 ms p50.

Two full-suite replicate processes were invalidated by late Chromium browser-context closures after 348–349 otherwise passing tests. Their unaffected scenario/DPR samples are retained only where complete, and each affected DPR 2 cell was replaced with its own fresh 35-run, one-page process. All three replacements passed 35/35.
