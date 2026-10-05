---
agentx:
  kind: decision
  scope: decisions/gpui/layout-style
  status: superseded
  created: 2026-08-31T05:55:49+02:00
  updated: 2026-08-31T06:13:59+02:00
  topics:
    - gpux
    - taffy
    - layout
    - performance
    - dependencies
  supersedes: []
  superseded_by:
    - 2026-08-31-gpui-layout-style-disable-taffy-content-size.md
---

## Decision: Declare the minimum Taffy feature set explicitly

### Context: Taffy 0.14's default bundle enabled balanced flex wrapping, float layout, calc expressions, and detailed layout records even though gpux's style conversion and layout engine do not use those capabilities.

### Alternatives considered: Keep every Taffy default; disable all optional semantic features including `content_size`; retain the gpux-required layout algorithms and content sizing while disabling only capabilities with no caller.

### Reasoning: Gpux needs the standard library, `TaffyTree`, flexbox, grid, block layout, and content-size semantics. Disabling only the four unused defaults compiled without source adaptation and passed all 411 gpux unit tests, every gpux integration suite, and all 52 serialized WGPU tests. Six headed Test Suite WebGPU cases preserved exact layout, renderer, identity, and final-scroll signatures. Versus the accepted FxHasher build, three-process median acknowledged throughput improved 0.28%/0.62% at DPR 1/2 and paced p95 improved 1.72%/3.48%; the repeated multithreaded WASM build's atomic prefix shrank by 1,265 bytes.

### Trade-offs accepted: Balanced flex wrapping, CSS floats, Taffy calc expressions, and detailed layout records are unavailable to gpux until a concrete API requirement deliberately enables them and repeats compatibility/performance proof. `content_size` remains enabled because it affects layout semantics even without a direct gpux field read.

### Supersedes: None.
