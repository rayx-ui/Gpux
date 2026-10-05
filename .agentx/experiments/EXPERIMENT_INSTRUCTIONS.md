## Evidence hygiene

Keep `.agentx/experiments/` limited to durable loop state and concise experiment JSON or Markdown records. Write Playwright output, screenshots, traces, scanner output, profiles, logs, process IDs, and other temporary diagnostics under ignored `artifacts-temp/agentx-experiments/{loop}/{experiment}/` (in Gpux, or in `../RayX` when the Test Suite produced them); remove them when the experiment is concluded. Summarize the measurements and conclusions in the durable experiment JSON instead of linking transient files from `.agentx/`.

## Measurement

gpux runtime measurements run through RayX's Test Suite (`../RayX`), headed WASM/WebGPU at DPR 1 and DPR 2, with three independent processes per change. Accept a change only when output and every established telemetry counter stay exact.

## Transplanted loops

The four loops here were run while gpux lived in RayX under `crates/gpux` and were moved on 2026-10-05 without edits. Their commit hashes refer to RayX history, a `main` repository at path `.` meant the RayX root, and `crates/gpux/<path>` is `<path>` in this repository. See `.agentx/memory/knowledge/repository/agentx-memory/knowledge.md` for mapping commits.
