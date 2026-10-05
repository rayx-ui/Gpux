## Single-line editable layout row splice

The same-length, single-logical-line edit path reshapes and replaces only the changed row when the installed layout is unprojected, unstyled, and geometry-preserving. Every other edit takes the existing full layout path.

### Focused WASM validation

- `cargo run -p xtask -- check-source-guard` passed.
- `cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu --playwright-test test_suite/rayx-text-area-edit-performance.spec.ts` passed at DPR 1 and DPR 2.
- The dedicated Test Suite page is `/performance/text-area-edit`; its action selects and replaces one visible character in a 2,000-line `TextArea`.
- The DPR 2 screenshot shows the edited character and retained caret with no text or scroll corruption.

### Measurement protocol

The headed Chromium WebGPU run used one worker, both configured DPR projects, 35 repeats per project, and discarded the first five samples per project. It produced 30 measured samples at each DPR and no failures.

| DPR | p50 input-to-published-frame | p95 | changed pixels |
| --- | ---: | ---: | ---: |
| 1 | 185.2906 ms | 207.9167 ms | 2,857 |
| 2 | 176.5264 ms | 203.7505 ms | 10,784 |

Raw Playwright artifacts remain in `11-single-line-layout-row-splice-output/`; they are intentionally untracked.

### Decision

Accept. The targeted path keeps UTF-8 row boundaries, existing visual-row metadata, and unchanged shaped layouts intact while strictly falling back for newlines, style runs, projections, length changes, duplicate old rows, and a possible maximum-width reduction. The focused source test checks that a 2,000-row same-length edit reports one shaped row and 1,999 reused rows while matching full-layout coordinate geometry.
