---
agentx:
  kind: knowledge
  scope: repo:tasks/gpux-fixed-sticky-positioning
  status: current
  created: 2026-07-04T06:38:30+02:00
  updated: 2026-07-06T02:30:00+02:00
  topics:
    - gpux-fixed-sticky-positioning
    - implementation-plan
    - positioning
  supersedes: []
  superseded_by: []
---

# Implementation Plan: GPUI Fixed And Sticky Positioning

Do not spawn sub-agents unless the user explicitly asks for sub-agents, delegation, or parallel agent work. Where a group says it is parallelizable, use `multi_agent_v1.spawn_agent` with `agent_type: worker` only for the named owned scopes.

## Sprint 1: Position API And Layout Contracts

### Group 1: Position Enum And Style Methods

Dependency: none.

Parallelization: sequential.

- [x] Add `Fixed` and `Sticky` to `Position` in `crates/gpux/crates/gpui/src/style.rs`, update default/exhaustive matches, and document CSS-like semantics. Owned scope: gpux style position enum. Validation: focused compile/tests for `Position` matches.
- [x] Map `Position::Fixed` to Taffy absolute and `Position::Sticky` to Taffy relative. Owned scope: `style.rs` and any `taffy.rs` conversion surface if needed. Validation: unit test or focused assertion for position-to-Taffy mapping.
- [x] Add generated `.fixed()` and `.sticky()` methods to `crates/gpux/crates/gpui_macros/src/styles.rs` and confirm they appear on the styled surface through `styled.rs`. Owned scope: gpux style macro. Validation: compile test or focused unit using `div().fixed()` and `div().sticky()`.
- [x] Update every current exhaustive match on `Position` to handle the new variants without compatibility aliases. Owned scope: gpux/RayX code that matches `Position`. Validation: `cargo check` for affected crates.

### Group 2: API Behavior Tests

Dependency: Group 1.

Parallelization: sequential.

- [x] Add focused tests proving fixed is out-of-flow at the Taffy mapping layer and sticky remains in-flow at the Taffy mapping layer. Owned scope: gpux style/layout tests. Validation: targeted gpux test command.
- [x] Add compile-style tests for using existing inset APIs with `.fixed()` and `.sticky()`. Owned scope: gpux tests or doctests near styled API. Validation: targeted gpux test/check.
- [x] Add regression tests ensuring `.relative()` and `.absolute()` still set the same variants and map to the same layout behavior. Owned scope: gpux style tests. Validation: targeted gpux test command.

## Sprint 2: Window Positioning Context Foundation

### Group 1: Scroll Context Stack

Dependency: Sprint 1.

Parallelization: sequential.

- [x] Add internal frame-local scroll container context support to `Window`. Owned scope: `crates/gpux/crates/gpui/src/window.rs` or a focused gpux positioning module. Validation: unit tests for push/pop stack behavior and nearest-context lookup.
- [x] Define scroll context data with scrollport bounds, content bounds or content size, current scroll offset, and active overflow axes. Owned scope: gpux internal positioning types. Validation: compile tests and unit tests for axis matching.
- [x] Add temporary debug assertions that context push/pop is balanced during prepaint and paint. Owned scope: gpux window lifecycle internals. Validation: focused tests pass without assertion failures.

### Group 2: Scrollable Div Context Integration

Dependency: Group 1.

Parallelization: sequential.

- [x] Push a scroll container context around scrollable `Div` children in `Div::prepaint` using the existing measured bounds, content size, overflow, and scroll offset. Owned scope: `crates/gpux/crates/gpui/src/elements/div.rs`. Validation: test proving a child can see the nearest scroll context.
- [x] Mirror the same scroll context behavior for `Div::paint` so paint uses the same coordinate assumptions as prepaint. Owned scope: `div.rs` and `window.rs`. Validation: targeted div paint/prepaint test.
- [x] Confirm `ScrollHandleState.child_bounds`, max offsets, and scroll clamping are unchanged when no sticky/fixed descendants exist. Owned scope: `div.rs` scroll tracking. Validation: existing scroll tests plus a no-position regression test.

### Group 3: Effective Positioned Bounds Helper

Dependency: Groups 1 and 2.

Parallelization: sequential.

- [x] Add an internal helper that resolves a style/layout bounds pair into layout bounds, visual bounds, and visual delta for relative/no-op positioning. Owned scope: gpux positioning helper. Validation: unit tests for relative and absolute no-op behavior.
- [x] Extend the helper with fixed viewport-origin resolution using existing inset values and current viewport bounds. Owned scope: gpux positioning helper/window viewport data. Validation: unit tests for top/left/right/bottom fixed origins.
- [x] Extend the helper with sticky threshold calculation for top and left insets against the nearest scroll context. Owned scope: gpux positioning helper. Validation: unit tests for unstuck and stuck top/left cases.

## Sprint 3: Normal Visual Tree Fixed And Sticky

### Group 1: Div Prepaint/Paint Position Delta

Dependency: Sprint 2.

Parallelization: sequential.

- [x] Apply resolved visual bounds to `Interactivity::prepaint` for fixed/sticky divs while preserving normal behavior for relative/absolute divs. Owned scope: `crates/gpux/crates/gpui/src/elements/div.rs`. Validation: hitbox/bounds tests for fixed and sticky visual position.
- [x] Apply the same visual delta to child prepaint via `window.with_element_offset(...)` so child content moves with the fixed/sticky parent. Owned scope: `div.rs`. Validation: nested child bounds test.
- [x] Store effective positioning state from prepaint and reuse it during `Div::paint`. Owned scope: `div.rs` prepaint state. Validation: paint/prepaint consistency test.

### Group 2: Fixed Content Mask And Containing Block

Dependency: Group 1.

Parallelization: sequential.

- [x] Resolve fixed positioned elements against the active content-mask containing block without changing default `with_content_mask` or deferred drawing behavior. Owned scope: `crates/gpux/crates/gpui/src/window.rs`. Validation: unit/integration test proving ordinary clipped children still clip.
- [x] Preserve the active content mask when prepainting and painting fixed divs inside clipped/scrollable ancestors. Owned scope: `div.rs` and `window.rs`. Validation: test proving fixed child anchors inside the clipped parent scrollport.
- [x] Verify fixed hitboxes align with visual bounds and remain clipped by the active parent mask. Owned scope: gpux interaction/hitbox tests. Validation: pointer/hitbox test.

### Group 3: Sticky Axis Completion And Clamping

Dependency: Group 1.

Parallelization: sequential.

- [x] Complete sticky resolution for bottom and right insets, including behavior when both opposing insets are present. Owned scope: gpux positioning helper. Validation: unit tests for top, bottom, left, right, and mixed inset cases.
- [x] Add containing block/content-boundary clamping so sticky elements release before escaping their scroll content. Owned scope: gpux positioning helper and div scroll context. Validation: scroll test showing sticky release at section/content end.
- [x] Add nested scroll container tests proving sticky selects the nearest relevant scroll ancestor. Owned scope: gpux div/window tests. Validation: targeted gpux test command.

## Sprint 4: gpux Low-Level Collection Support

### Group 1: UniformList Scroll Context And Candidates

Dependency: Sprint 3.

Parallelization: sequential.

- [x] Push scroll context around rendered `uniform_list` items in self-scroll mode using existing bounds, content size, and scroll offset. Owned scope: `crates/gpux/crates/gpui/src/elements/uniform_list.rs`. Validation: test proving rendered sticky row sticks while visible.
- [x] Add optional sticky candidate index support to `UniformList` and include active candidates in the rendered item set. Owned scope: `uniform_list.rs` public/internal API. Validation: test proving an off-visible sticky candidate remains rendered while active.
- [x] Preserve existing `visible_range` callbacks/events so extra sticky candidate rendering does not falsely expand the logical visible range unless an explicit diagnostics field is added. Owned scope: `uniform_list.rs`. Validation: existing visible range tests plus sticky candidate regression.

### Group 2: gpux List Scroll Context And Candidates

Dependency: Sprint 3.

Parallelization: sequential.

- [x] Push scroll context around gpux `list` rendered items using existing list bounds, content height, and scroll offset. Owned scope: `crates/gpux/crates/gpui/src/elements/list.rs`. Validation: test proving a rendered sticky item sticks.
- [x] Add sticky candidate support compatible with `list` variable-height state and existing focused/offscreen item handling. Owned scope: `list.rs` state and prepaint item selection. Validation: variable-height candidate test.
- [x] Preserve scroll-to-item, focus, and visible item behavior when sticky candidates are present. Owned scope: `list.rs`. Validation: existing list tests plus sticky/focus regression.

### Group 3: gpux Collection Runtime Proof

Dependency: Groups 1 and 2.

Parallelization: sequential.

- [x] Add or extend gpux examples/tests to exercise sticky rows in `uniform_list` and `list`. Owned scope: gpux examples/tests only. Validation: targeted gpux tests.
- [x] Run focused gpux collection tests and fix root-cause regressions. Owned scope: validation and fixes in gpux collection files. Validation: focused command succeeds.

## Sprint 5: RayX VirtualList, ListView, And TreeView Support

### Group 1: RayX VirtualList Candidate Rendering

Dependency: Sprint 3.

Parallelization: sequential.

- [x] Push scroll context around RayX `VirtualList` rendered items for self-scroll and synchronized external-scroll modes. Owned scope: `crates/rayx/rayx_components/rayx_components_core/src/virtual_list/mod.rs`. Validation: focused VirtualList test for rendered sticky item.
- [x] Integrate ParentViewport mode with the parent scroll context and parent viewport metrics without applying the external scroll offset twice. Owned scope: `virtual_list/mod.rs`. Validation: ParentViewport test with page scroll offset.
- [x] Add optional sticky candidate index/range support and render the union of visible range plus active candidates. Owned scope: `virtual_list/mod.rs`. Validation: test proving active candidate outside visible range remains rendered.

### Group 2: ListView Sticky Keys

Dependency: Group 1.

Parallelization: parallelizable with sub-agents because ListView and TreeView state APIs own separate modules after VirtualList candidate rendering exists.

- [x] Add key-based sticky candidate APIs to `ListViewState` and map keys to current visible/projection indices through insert/remove/move/source updates. Owned scope: `crates/rayx/rayx_components/rayx_components_core/src/list_view/state.rs`. Validation: ListView state tests for key preservation and candidate index mapping.
- [x] Wire `ListViewState` sticky candidates into the low-level `VirtualList` candidate path during render. Owned scope: `list_view/state.rs` render closure and `list_view/element.rs` only if wrapper API is needed. Validation: rendered ListView sticky candidate test.
- [x] Preserve visible range events, scroll anchors, follow-tail behavior, and ParentViewport scroll writes while sticky candidates are active. Owned scope: ListView state tests. Validation: existing ListView virtualization tests plus new sticky regressions.

### Group 3: TreeView Sticky Keys

Dependency: Group 1.

Parallelization: parallelizable with sub-agents because TreeView state/API work is disjoint from ListView after VirtualList candidate rendering exists.

- [x] Add key-based sticky candidate APIs to `TreeViewState` and map keys through visible projection, expansion, collapse, insert, remove, and move operations. Owned scope: `crates/rayx/rayx_components/rayx_components_core/src/tree_view/state.rs`. Validation: TreeView state tests for projection-aware candidate mapping.
- [x] Wire `TreeViewState` sticky candidates into the low-level `VirtualList` candidate path during render. Owned scope: `tree_view/state.rs` render closure and `tree_view/element.rs` only if wrapper API is needed. Validation: rendered TreeView sticky candidate test.
- [x] Ensure collapsed/hidden sticky keys do not render as sticky candidates until visible again. Owned scope: TreeView state tests. Validation: expansion/collapse sticky regression tests.

## Sprint 6: Lab Stories And Runtime Diagnostics

### Group 1: Normal Tree Stories

Dependency: Sprint 3.

Parallelization: sequential.

- [x] Add a Lab story for normal scroll containers with sticky headers, nested scroll containers, and a fixed toolbar inside a clipped/scrolled parent. Owned scope: `apps/lab/src/stories` and story registration only. Validation: `cargo check --manifest-path apps/lab/Cargo.toml`.
- [x] Add runtime-readable diagnostics or DevKit-visible semantics for the story that expose expected vs actual fixed/sticky bounds during validation. Owned scope: Lab story diagnostics and existing diagnostics APIs only. Validation: headed runtime can read diagnostics.
- [x] Use temporary traces around sticky threshold and fixed containing-block/mask behavior while validating, then remove trace-only code before completing the sprint. Owned scope: gpux positioning internals. Validation: final diff has no auto-enabled trace-only helpers.

### Group 2: Collection Stories

Dependency: Sprints 4 and 5.

Parallelization: sequential.

- [x] Add Lab demos for gpux `uniform_list` and gpux `list` sticky rows. Owned scope: Lab story files and any public example wrappers. Validation: Lab cargo check.
- [x] Add Lab demos for RayX `VirtualList`, `ListView`, and `TreeView` sticky candidates in both self-scroll and ParentViewport/page-scroll modes where applicable. Owned scope: Lab story files and RayX component examples. Validation: Lab cargo check and focused component tests.
- [x] Expose diagnostics for active sticky candidate indices/keys and visible ranges so WASM validation can prove candidates are rendered without corrupting visible range semantics. Owned scope: Lab story diagnostics and component event hooks. Validation: diagnostics snapshot/manual check.

### Group 3: WASM And Native Runtime Proof

Dependency: Groups 1 and 2.

Parallelization: sequential.

- [x] Run `cargo run -p xtask -- app apps/lab test wasm --headed --webgpu` for fixed/sticky normal tree behavior. Owned scope: validation only. Validation: WebGPU headed test passes, no page errors, sticky/fixed screenshots or diagnostics match expectations.
- [x] Extend the WASM validation script under `tools-node/` if existing coverage cannot scroll and inspect fixed/sticky bounds. Owned scope: `tools-node` validation script/config only. Validation: headed WebGPU test asserts bounds/diagnostics.
- [x] Run `cargo run -p xtask -- app apps/lab run host` for native runtime crash and interaction proof. Owned scope: validation only. Validation: no runtime crash and DevKit/diagnostic inspection confirms positions, or exact DevKit blocker is documented.

## Sprint 7: Cross-Target Validation, Documentation, And Memory

### Group 1: Broad Validation Matrix

Dependency: Sprint 6.

Parallelization: sequential.

- [x] Run `cargo fmt --all`. Owned scope: validation only. Validation: command succeeds.
- [x] Run focused gpux tests for style, div, window positioning, uniform_list, and list. Owned scope: validation only. Validation: command succeeds or root-cause fixes are made.
- [x] Run focused RayX component tests for VirtualList, ListView, and TreeView sticky candidates. Owned scope: validation only. Validation: command succeeds or root-cause fixes are made.
- [x] Run `cargo check --manifest-path apps/lab/Cargo.toml` and `cargo check --manifest-path apps/lab/platform/web/Cargo.toml`. Owned scope: validation only. Validation: commands succeed.
- [x] Run `cargo clippy --workspace --all-features -- -D warnings` when feasible, or document the exact blocker. Owned scope: validation only. Validation: command succeeds or blocker is explicit.

### Group 2: Android And Platform-Specific Proof

Dependency: Sprint 6.

Parallelization: sequential.

- [x] Determine whether fixed/sticky changes affect gpui_mobile/Android behavior through shared gpux code paths. Owned scope: source inspection and validation notes. Validation: platform impact note in closeout.
- [x] Run `cargo run -p xtask -- app apps/lab run android` when mobile runtime is affected and an Android target is available. Owned scope: validation only. Validation: no runtime crash and sticky/fixed demo behaves correctly, or exact environment blocker is documented.
- [x] Document any backend/platform limitations such as transform-created fixed containing block simplifications. Owned scope: task closeout/docs only. Validation: limitation wording matches current behavior.

### Group 3: Docs, Skill Guidance, And AgentX Memory

Dependency: Sprints 1 through 6.

Parallelization: sequential.

- [x] Add gpux/RayX docs or examples for `.fixed()` and `.sticky()` usage, including virtualized sticky candidate requirements. Owned scope: relevant docs/examples only. Validation: doc examples compile where possible.
- [x] Update the narrowest relevant RayX/gpux skill reference only if implementation produces reusable guidance beyond this plan. Owned scope: `.codex/skills/rayx` or `.codex/skills/gpux` only when justified. Validation: skill guidance includes concrete DO/DON'T examples if changed.
- [x] Update AgentX memory for current implemented behavior and durable rules after source changes land. Owned scope: `.agentx/memory/knowledge/runtime/gpux-foundation` and `.agentx/memory/knowledge/ui/virtualization` only as needed. Validation: active AgentX audit script reports no new relevant errors.
- [x] Perform a final requirements audit against `requirements.md` and record unresolved follow-ups. Owned scope: task closeout/audit artifact if created and `state.json` only during implementation. Validation: every requirement is implemented, deferred with reason, or marked not applicable with evidence.

## Final Audit Notes (2026-07-04)

- Requirements implemented: gpux `Position::Fixed` and `Position::Sticky`, fluent `.fixed()` and `.sticky()`, active content-mask fixed containing blocks, nearest-scroll sticky resolution, axis/inset clamping, fixed content-mask preservation, hitbox/child visual-bound alignment, scroll context integration for div/list/uniform_list/VirtualList/ListView/TreeView, explicit virtualized sticky candidates, Lab demos, docs, tests, and AgentX memory updates.
- WASM proof: `cargo run -p xtask -- app apps/lab build wasm` succeeded; headed Playwright with WebGPU enabled ran against the xtask-built artifact and passed the fixed/sticky focused test in Chromium and Chromium DPR2, then passed the full suite at 16/16. The direct xtask test server path was bypassed after stale/orphan port state; the custom static server used the same built artifact with COOP/COEP/CORP and SPA fallback.
- Native proof: `cargo run -p xtask -- app apps/lab build host` succeeded; `target/release/RayX-story-main.exe` stayed alive for a 12 second smoke test. A devkit-enabled native build started and exposed metadata for `RayX-story`; DevKit diagnostics were empty except for the synthetic click event used during inspection. DevKit route-body inspection did not expose the fixed/sticky story body text, so route-specific runtime position proof remains the headed WASM/WebGPU Playwright evidence.
- Android proof: the feature touches shared gpux/RayX rendering paths that Android consumes, but Android runtime validation was blocked because `adb` is not on PATH in this environment. Installed Rust targets include Android ABIs, but no Android device/emulator bridge was available.
- Platform web check: `apps/lab/platform/web/Cargo.toml` does not exist in the current source tree. The current xtask path generates the WASM entry crate under `artifacts-temp/apps/RayX-story/wasm/rust`, so validation used `cargo run -p xtask -- app apps/lab build wasm` plus headed WebGPU Playwright instead of the stale manifest path.
- Limitations documented: fixed positioning resolves against the active GPUI content-mask containing block; transformed-ancestor fixed containing blocks remain out of scope. Virtualized sticky support requires explicit candidate metadata because offscreen closures are not instantiated.
- Skill guidance: no `.codex/skills/rayx` or `.codex/skills/gpux` reference file needed a durable update beyond the repository docs and AgentX memory added in this task.
