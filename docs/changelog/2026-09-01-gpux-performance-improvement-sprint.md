# GPUX Performance Improvement Sprint Report

The sprint ran while gpux lived in RayX under `crates/gpux`. Commit hashes refer to RayX history. The experiment records moved with gpux to `.agentx/experiments/2026-08-30-gpux-maximum-performance/` on 2026-10-05.

## Executive summary

The sprint ran one baseline followed by exactly 150 AgentX experiments between August 30–31, 2026. Of the 150 optimization iterations:

### Curated integration commits

| Commit | Purpose |
|---|---|
| `fd97adf865f351caa5f5f68e486ddf114bbe0e59` | `perf(gpux): optimize layout rendering and frame hot paths` |
| `0c5d242ee52998eee26ae7d858b3507ef2040997` | `chore(agentx): archive gpux performance experiments` |
| `fda4058239ad9b3034c1995e840c6b25cb959f41` | `docs(gpux): document performance results and dependency policy` |

- 111 experiments succeeded and produced accepted implementation, diagnostic, validation, or research evidence.
- 39 experiments failed their predefined gate and were reverted.
- Acceptance rate: 74%.
- No failed product experiment remains in the source tree.
- Final product checkpoint: `c8abe9de8fa297c6c9af7a84aac4246938138ae6`.
- Original improvement-loop closure HEAD: `f1e74a124a0c740d9498612fb3607dd16b90e976`.

The sprint produced substantial reductions in specific hot-path operations:

| Area | Proven reduction |
|---|---:|
| Bounds-tree search visits | 25,720 → 4,440, `-82.74%` |
| Duplicate bounds propagation | At least 429 unions and max comparisons removed/frame |
| Taffy node creation after warm-up | 1,123/frame → 0 |
| Taffy child-buffer creation after warm-up | 782/frame → 0 |
| Measured-context map mutations | 458/frame → 0 |
| Repeated line-layout full-key hashes | 921 → 320, `-65.26%` |
| Text bytes hashed per warm frame | 17,178 → 6,006, `-65.04%` |
| Decoration capacity | 10,240 → 320 slots, `-96.875%` |
| Font scratch synchronization | 640 mutex operations/frame removed |
| Unused line-wrapper synchronization | 640 mutex operations/frame removed |
| Repeated browser WASM constructor traversals | Many export wrappers → one call/module |
| Static prepaint style computation | Up to 729 computations reused/frame |
| Request-to-prepaint style computation | 716 computations reused/frame |
| Element-state finish misses | 1,344 → 0 |
| Taffy vendor footprint | 67 files / 23,970 lines → 0 |
| Final focused test result | 543/543 passed |

The largest independently measured local gains were:

- Explicit WASM constructor initialization: approximately `+20.73%/+20.61%` throughput and `11.77%/11.29%` better paced p95 at DPR 1/2.
- Registry Taffy 0.14 upgrade: `+20.34%/+21.25%` throughput and `0.43%/4.60%` better p95 versus the immediate Taffy 0.12.2 checkpoint.
- Replay-only element-state transfer: `+6.14%/+7.77%` throughput and `26.25%/25.57%` better p95.
- Thread-local font scratch: `+1.78%/+5.37%` throughput and `3.81%/4.02%` better p95.
- Input-sized decoration runs: `+4.93%/+3.30%` throughput and `2.46%/0.76%` better p95.
- Cached line-layout hashes: `+2.56%/+0.41%` throughput, with a small mixed p95 result.
- Style handoff reduced sampled style-refinement self time by `65.64%/53.04%` and `compute_style_internal` by `57.04%/41.00%`.

These percentages are comparisons against each experiment’s immediate, contemporaneous checkpoint. They must not be added together.

The authoritative loop state is `.agentx/experiments/2026-08-30-gpux-maximum-performance/_state.json`.

---

## 1. Scope and methodology

### Target

The sprint exclusively optimized gpux as RayX’s bottom rendering and UI runtime layer:

- GPUI layout and lifecycle
- Taffy integration
- scene construction and batching
- bounds-tree operations
- WGPU uploads, passes, pipelines, shaders, and atlases
- text measurement, layout, and painting
- frame storage and cleanup
- browser/WASM scheduling
- virtualization-related hot paths
- stable element identity and retained state

Application-hosted performance and runtime validation used only `apps/test_suite`, as required by the repository’s diagnostic boundary. Lab was not used.

### Representative workload

The primary workload was a wide virtualized DataTable:

- 100,000 rows
- 48 columns
- four fixed columns
- headed Chromium
- WebGPU enabled
- DPR 1 and DPR 2
- exact final scroll position verification
- zero-terminal-error requirement

The initial baseline also covered:

- a 100,001-row list and an 89,983-visible-row tree
- TextArea wheel-to-published-frame latency
- native text-layout Criterion measurements

### Atomic experiment discipline

Every optimization experiment had:

- one declared mechanism
- one implementation attempt
- an exact checkpoint
- fixed correctness and performance gates
- mandatory commit on success
- immediate revert on failure
- no fix-forward inside a failed experiment

This is why 39 apparently promising ideas were deliberately discarded.

---

## 2. Baseline

Baseline commit: `4f4335e5b46f41bfc796700312480f2612ac3a0b`.

The complete baseline record is `.agentx/experiments/2026-08-30-gpux-maximum-performance/2026-08-30-01-baseline-measurement.json`.

### Wide DataTable

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Median acknowledged steps/s | 24.7618 | 25.8916 |
| Paced p95 catch-up latency | 27.7312 ms | 27.5449 ms |
| Paced p95 thumb lag | 36.8156 px | 34.9402 px |
| Runs per DPR | 5 | 5 |
| Terminal error | 0 | 0 |

### Sliver list/tree

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Median acknowledged steps/s | 33.0829 | 33.3222 |
| Paced p95 catch-up latency | 13.9438 ms | 13.7173 ms |
| Terminal error | 0 | 0 |

### TextArea

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| p50 wheel-to-published-frame | 66.1529 ms | 76.7219 ms |
| p95 | 92.2634 ms | 86.2303 ms |
| Retained samples | 30 | 30 |

### Initial CPU attribution

| Hot path | DPR 1 self share | DPR 2 self share |
|---|---:|---:|
| Taffy flex | 4.38% | 4.62% |
| WASM malloc | 2.34% | 2.39% |
| Text-line paint | 1.34% | 1.75% |
| Taffy block | 1.34% | 1.56% |
| Bounds-tree insertion | 1.24% | 1.33% |
| Global element path | 0.97% | 0.92% |
| Monochrome sprite sorting | 0.78% | 0.55% |

The baseline established that gpux was primarily CPU-bound in layout, allocation, text, scene construction, and cleanup—not in GPU submission count alone.

---

## 3. Final architecture

The optimized hot-frame path is:

```text
App invalidation
  → Window::draw
  → pooled Taffy layout tree
  → prepaint and deferred traversal
  → retained style handoffs
  → paint
  → Scene::finish
  → family batch merge
  → WgpuRenderer
  → one command submission
  → presentation
```

### Layout and Taffy

The final Taffy integration:

- uses exact crates.io Taffy `0.14.0`
- disables default features
- enables only `std`, `taffy_tree`, `flexbox`, `grid`, and `block_layout`
- reuses a window-owned pool of Taffy nodes
- retains child topology across frames
- skips unchanged `set_style` operations
- skips unchanged `set_children` operations
- retains empty measured-context slots without retaining frame closures
- tracks active containers rather than probing every node
- remains bounded by the largest tree observed by that window

See [taffy.rs](../../crates/gpui/src/taffy.rs) and the [Taffy manifest](../../crates/gpui/Cargo.toml).

### Scene and renderer

The final ordinary warmed frame records:

- nine batches
- 12 queue-buffer writes
- 10 bind groups
- two render passes
- one submission
- zero buffer restarts
- zero atlas writes

Pipelines, geometric buffers, bind-group layouts, atlases, and textures already have appropriate renderer-owned lifetimes. The sprint found no evidence justifying a second cache layer.

The remaining second render pass exists because the retained-scene snapshot/effects architecture renders through an intermediate texture and then passes it to the target. It cannot be safely removed as a local optimization.

### Text

The final text path:

- preserves the local size cache
- preserves current/previous wrapped-layout caches
- stores a cached hash in each owned line-layout key
- maintains collision-safe full equality
- sizes decoration storage from actual input cardinality
- uses thread-local font-run scratch
- acquires a line wrapper only when truncation needs it
- preserves cache generation and eviction behavior

See [line_layout.rs](../../crates/gpui/src/text_system/line_layout.rs).

### Frame state

The final frame lifecycle:

- records arena destructor work only for values that need destruction
- lazily allocates DevTools semantics for annotated nodes
- retains full element-state access history for replay and rollback
- separately tracks only replay-pending element-state transfers
- never scans direct-access keys during frame finish
- clears frame-owned collections while retaining useful capacities

See [arena.rs](../../crates/gpui/src/arena.rs) and [window.rs](../../crates/gpui/src/window.rs).

---

## 4. Detailed gains

### 4.1 Bounds-tree work

#### Intersection pruning

The slow bounds search previously pushed non-intersecting branches and rejected them later.

The accepted change rejects:

- a non-intersecting root before stack traversal
- non-intersecting children before stack insertion

Measured warmed-frame result:

| Metric | Before | After | Change |
|---|---:|---:|---:|
| Search-node visits | 25,720 | 4,440 | `-82.74%` |
| Insertions | 504 | 504 | unchanged |
| Child evaluations | 14,351 | 14,351 | unchanged |
| Retained nodes | 576 | 576 | unchanged |
| Output | exact | exact | unchanged |

#### Duplicate propagation removal

For non-full terminal insertions, a bounds union and maximum-order update were performed immediately and then repeated by the shared propagation loop.

The accepted change removed at least:

- 429 bounds unions/frame
- 429 maximum comparisons/frame

The randomized bounds-tree oracle and headed WebGPU runtime preserved exact results.

#### Rejected bounds alternatives

The following were tested and rejected:

- recorded descent positions
- eight-child branching
- sixteen-child branching
- paired sixteen-child branching
- direct child-loop replacement
- deferred intersection
- cached area calculations

Several reduced one local operation but made total execution worse due to larger trees, stack traffic, code generation, or changed cache behavior.

### 4.2 Taffy and layout

#### Taffy 0.14 registry upgrade

Against the immediate registry Taffy 0.12.2 checkpoint:

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Throughput improvement | `+20.34%` | `+21.25%` |
| Paced p95 improvement | `+0.43%` | `+4.60%` |
| Transitive packages removed | 1 | 1 |

The upgrade also slightly exceeded the performance of the temporary two-child inline vendor patch, removing the justification for carrying that patch.

#### Vendor removal

The temporary vendor surface contained:

- 67 files
- 23,970 lines

Final state:

- zero tracked Taffy vendor files
- zero Taffy vendor files on disk
- no path patch
- no Git patch
- exact registry checksum in both lockfiles

The checksum is:

```text
639627c87f43b9181c811f40a6296409e093a17bc761214cba3c15df74f86b99
```

The gpux dependency policy now prefers:

1. crates.io
2. an exact-revision maintained fork for a small essential unreleased patch
3. full vendoring only for offline builds, upstream instability, or deep modifications

See [gpux skill policy](../../.claude/skills/gpux/SKILL.md).

#### Node pooling

Before warm pooling, each frame created:

- 1,123 Taffy nodes
- 782 container child buffers

After reaching the window’s observed maximum tree size:

- zero new Taffy nodes/frame
- zero new child-buffer allocations/frame

Measured immediate result:

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Throughput | `+0.10%` | `-0.38%` |
| Paced p95 improvement | `+2.83%` | `+6.51%` |

The main benefit is deterministic removal of structural allocation churn and materially better tail latency.

#### Measured-context lifetime

The old clear path could retain 229 additional boxed measurement closures per warmed frame.

The accepted lifecycle:

- removes or takes captured closures before invalidation
- retains no frame-local closure after clear
- later reuses 229 empty bounded context slots
- eliminates 229 removals plus 229 insertions/frame

#### Active container detachment

The final clear path records active containers directly.

This removes:

- 341 leaf `child_count` probes/frame
- detach-condition branches for all 1,123 nodes

#### Style and topology write elision

Every warmed frame contains:

- 1,123 reused Taffy nodes
- 1,123 unchanged styles
- 1,123 unchanged topologies
- 1,041 unique child edges
- zero duplicate active-child insertions

The final implementation compares exact values and avoids writes only when equality is proven.

#### Feature minimization

Disabling unused defaults improved:

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Throughput | `+0.28%` | `+0.62%` |
| Paced p95 | `+1.72%` | `+3.48%` |
| Repeated WASM prefix | `-1,265` bytes | `-1,265` bytes |

Removing unused `content_size` added:

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Throughput | `+0.15%` | `+0.38%` |
| Paced p95 | `+1.21%` | `+1.58%` |
| Repeated WASM prefix | `-256` bytes | `-256` bytes |

A follow-up profile measured total Taffy self time approximately 8.1% lower at both DPRs after removing `content_size`.

### 4.3 Text and cache work

#### Cached line-layout key hash

The previous two-generation wrapped cache rehashed the same full key during lookup and promotion.

| Metric | Before | After | Change |
|---|---:|---:|---:|
| Full hashes/frame | 921 | 320 | `-65.26%` |
| Text bytes hashed | 17,178 | 6,006 | `-65.04%` |
| Run items hashed | 921 | 320 | `-65.26%` |
| Equality probes | 322 | 324 | `+0.62%` |

Immediate measured performance:

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Throughput | `+2.56%` | `+0.41%` |
| Paced p95 | `-0.61%` | `-0.81%` |

The p95 movement was slightly negative, while throughput and structural work improved. Collision-safe full equality remains authoritative.

#### Decoration run allocation

Telemetry showed every one of the 320 warmed lines contained exactly one decoration run, while each vector reserved 32 slots.

| Metric | Before | After | Change |
|---|---:|---:|---:|
| Retained slots/frame | 10,240 | 320 | `-96.875%` |
| Live runs | 320 | 320 | unchanged |

Immediate performance:

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Throughput | `+4.93%` | `+3.30%` |
| Paced p95 improvement | `+2.46%` | `+0.76%` |

Capacity is now seeded from input run count, capped at 32, with normal growth preserved for complex text.

#### Thread-local font scratch

The shared font-run pool had:

- 320 takes
- 320 returns
- 100% reuse
- zero allocation misses
- 640 mutex acquisitions/frame

The accepted thread-local pool preserved the 100% hit rate and removed all 640 mutex acquisitions.

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Throughput | `+1.78%` | `+5.37%` |
| Paced p95 improvement | `+3.81%` | `+4.02%` |

#### Lazy line-wrapper acquisition

The warmed workload made 320 full layouts and zero truncation requests, but acquired and returned a wrapper for every layout.

The accepted path removed another 640 mutex operations/frame.

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Throughput | `+6.09%` | `+0.06%` |
| Paced p95 | 1.55% worse | `+0.89%` better |

The path remains lazy rather than removing the wrapper mechanism, so truncation behavior is preserved.

#### Cache behavior learned

The warmed frame had:

- 882 text measurement callbacks
- 562 local size-cache reuses
- 320 full text-layout requests
- 19 current-frame wrapped hits
- 301 previous-frame wrapped hits
- zero warmed wrapped misses
- zero warmed platform shaping

Across the complete benchmark, however:

- DPR 1: 46,074 wrapped misses across 484,348 requests
- DPR 2: 48,006 misses across 456,481 requests
- all 94,080 nested unwrapped requests missed

Two apparently attractive attempts were rejected:

- bypassing nested unwrapped-cache population removed 100% of those requests but regressed throughput by 11.66%/8.64%
- sharing a cloned wrapped-key hash prefix regressed throughput by 16.66%/14.82%

This proved that removing an operation count does not necessarily remove useful cache work.

### 4.4 Browser/WASM startup

The generated WASM module had linker-generated constructor traversal in multiple command-export wrappers.

The accepted initialization moved constructor execution to one explicit call in `gpui_platform::web_init`.

| Metric | Before | After |
|---|---|---|
| Constructor owners | repeated export wrappers | one module initialization |
| Remaining constructor call sites | many | 1 |
| Command wrapper calls | repeated | 0 |
| Constructor/inventory profile samples | substantial | 0 |
| Repeated WASM prefix | 99,160 bytes | 99,112 bytes |

Immediate performance:

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Throughput | `+20.73%` | `+20.61%` |
| Paced p95 improvement | `+11.77%` | `+11.29%` |

This was the strongest clean local win in the sprint.

See [gpui_platform.rs](../../crates/gpui_platform/src/gpui_platform.rs).

### 4.5 Style computation and handoff

#### Conditional prepaint recomputation

Gpux previously recomputed style after sticky-state resolution even when no listener or stuck-state refinement could alter it.

The accepted path recomputes only when:

- a sticky-state listener exists, or
- the element is stuck and has a `when_stuck` refinement

Profile change:

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| `compute_style_internal` self time | `-29.41%` | `-10.63%` |
| Paced p95 improvement | `+0.95%` | `+0.62%` |
| Throughput | `-0.61%` | `+1.51%` |

#### Prepaint-to-paint style retention

Telemetry found that 729 of 845 warmed paint-style computations—86.27%—were safe under the conservative reuse predicate.

The accepted frame-owned style store:

- retains only structurally static styles
- excludes focus, hover, active, group, and drag-sensitive refinements
- consumes state within the same draw
- clears ownership each draw
- retains bounded vector capacity

Measured profile reductions:

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| `compute_style_internal` normalized self share | `-47.33%` | `-47.31%` |
| `Style::refine` | `-57.16%` | `-63.82%` |
| `StyleRefinement` drop | `-38.56%` | `-46.52%` |
| Paced p95 improvement | `+1.21%` | `+1.61%` |

#### Request-to-prepaint handoff

The first implementation assumed all 729 candidates crossed a gpux-owned request path. Runtime proof found:

- 716 gpux-owned candidates
- 13 external or custom candidates

That attempt was reverted. A later exact ownership partition preserved the external API and reused only the 716 gpux-owned candidates.

Follow-up profile:

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| `Style::refine` self time | `-65.64%` | `-53.04%` |
| `compute_style_internal` | `-57.04%` | `-41.00%` |
| Interactivity prepaint | `-30.57%` | `-34.00%` |
| Flexbox sampled self time | `-11.06%` | `-7.62%` |

End-to-end timing stayed within host variance:

- throughput `-0.13%/+0.25%`
- p95 `+3.24%/+1.61%` slower

The change was accepted because the exact computation reduction was proven and the timing movement did not cross its material-regression gate.

### 4.6 Scene, batching, sprites, and identity

#### BatchIterator

The renderer has ten primitive-family heads but consumes only the smallest and second-smallest head.

The old implementation fully sorted all ten entries per emitted batch. The new implementation performs one two-minimum scan, bounded at 17 key comparisons.

Behavior remained exact, but browser process variance was too large to assign an end-to-end speedup.

#### Monochrome sprite sorting

The representative scene was not nearly sorted:

- 684 of 1,514 adjacent pairs disordered at DPR 1
- 666 of 1,512 disordered at DPR 2

An already-sorted fast path was rejected. Stable cached-key sorting was accepted for the measured monochrome family.

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Sort-path self time | `-29.37%` | `-54.25%` |
| Throughput | `+0.04%` | `+0.04%` |
| Paced p95 improvement | `+2.84%` | `+1.50%` |

A reusable key vector was slower and reverted.

#### GlobalElementId hashing

The workload creates:

- 485 global IDs/frame
- 8,668 path elements
- average depth 17.87
- maximum depth 23

This ruled out small inline storage.

Replacing SipHash-style cached identity hashing with `FxHasher` produced:

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Hash self-time reduction | `-90.68%` | `-86.81%` |
| Throughput | `-0.29%` | `-0.20%` |
| Paced p95 improvement | `+0.17%` | `+0.38%` |
| WASM repeated prefix | `-7,208` bytes | `-7,208` bytes |

Full path equality remains collision-safe and authoritative.

Map-based and ordinal-vector Arc reuse were both tested and rejected because they reduced allocation samples but regressed product performance.

### 4.7 Paint and arena work

#### Identity opacity

Default inherited opacity is exactly `1.0`. The old path rebuilt immutable color or background values and their lazy preparation cache.

The accepted path reuses the original cached value for exact identity opacity.

| Metric | DPR 1 | DPR 2 |
|---|---:|---:|
| Throughput | `+0.75%` | `+0.50%` |
| Paced p95 improvement | `+1.55%` | `+0.07%` |

Non-identity opacity retains the original behavior.

#### Arena destructor records

The arena previously registered a type-erased destructor record even for trivially droppable allocations.

The accepted path uses `std::mem::needs_drop::<T>()`.

Effects:

- no destructor bookkeeping for trivial allocations
- exact destructor behavior for nontrivial values
- repeated WASM prefix reduced by 908 bytes
- sampled `Arena::force_clear` self time decreased at both DPRs

End-to-end cadence was approximately neutral, as expected for a small cleanup optimization.

### 4.8 DevTools semantics and element state

#### Lazy DevTools semantics

Every frame node previously carried concrete default semantics even when the node had no annotations.

The final representation uses optional semantics and constructs the payload only when authored.

Preserved behavior:

- public snapshots remain concrete
- authored semantic merging remains intact
- writable properties remain intact
- no skipped-node mode
- no public API change
- no pool

The follow-up profile was mixed because authored semantic payloads still need normal teardown, so no broad semantics pooling was added.

#### Replay-only element-state finish

Telemetry proved the warmed frame performed:

- 1,344 finish scans
- zero successful previous-frame transfers
- 1,344 misses

Direct state access had already moved every required state. Only replayed subtrees needed deferred finish transfer.

The final representation keeps:

- complete access history for replay and transaction rollback
- a separate pending-transfer list populated only by replay

Results:

| Metric | Before | After |
|---|---:|---:|
| Finish scans | 1,344 | 0 |
| Successful transfers | 0 | 0 |
| Finish misses | 1,344 | 0 |
| DPR 1 throughput | baseline | `+6.14%` |
| DPR 2 throughput | baseline | `+7.77%` |
| DPR 1 paced p95 | baseline | `+26.25%` |
| DPR 2 paced p95 | baseline | `+25.57%` |
| Removal self time | baseline | `-10.68%/-15.44%` |

This was one of the strongest final product improvements.

---

## 5. Renderer and shader findings

The research did not find evidence for indiscriminately adding WGPU caches or shader variants.

### Preserved architecture

The final renderer already has:

- persistent render pipelines
- persistent geometric buffers
- persistent texture atlases
- stable bind-group layouts
- keyed atlas entries
- lazy atlas upload queues
- one command encoder and submission for an ordinary frame
- family-specific shader pipelines
- an axis-aligned/simple-quad shader fast path

### Rejected WGPU family prepacking

Prepacking primitive families reduced:

- buffer writes from 12 to five
- bind groups from 10 to six

But DPR 2 throughput regressed in every measured candidate process. The experiment was reverted.

This is important evidence: WebGPU host-call count was not the limiting variable for this workload.

### Shader branching

The audit found that most invariant work is already separated by pipeline.

The remaining monochrome-sprite glyph/image mask switch is genuinely per-instance. Splitting it would:

- add a batch boundary
- add a pipeline
- increase pipeline changes

No change was made without GPU timing proving that the saved branch outweighs those costs.

### Two-pass presentation

The ordinary frame uses:

1. main rendering into an intermediate retained-scene texture
2. a pass-through render to the presentation target

Direct-to-surface rendering could remove the second pass for effect-free frames. However, it requires a new retained-scene snapshot contract; presented swapchain textures cannot simply replace the retained intermediate texture. This remains the highest-value GPU-side architectural opportunity.

---

## 6. End-to-end interpretation

A single cumulative sprint speedup cannot be stated responsibly.

### Baseline versus final absolute observations

| DataTable metric | Initial baseline | Final profile |
|---|---:|---:|
| DPR 1 throughput | 24.7618 steps/s | 24.7379 steps/s |
| DPR 2 throughput | 25.8916 steps/s | 24.7449 steps/s |
| DPR 1 paced p95 | 27.7312 ms | 26.9453 ms |
| DPR 2 paced p95 | 27.5449 ms | 27.5493 ms |

These figures are not an apples-to-apples before/after comparison:

- the baseline serialized projects with one worker
- later profiles used evolved diagnostics and execution orchestration
- the benchmark fixture and telemetry contract expanded during the sprint
- host and browser performance drifted materially over the 27.5-hour loop
- later paired experiments used their own fresh checkpoint distributions for this reason

At intermediate clean checkpoints, the same workload frequently measured around 29–31 steps/s. Later clean runs dropped into the 24–28 range without corresponding source regressions, demonstrating substantial process and host sensitivity.

Therefore:

- local paired experiment gains are valid
- deterministic operation-count reductions are valid
- profile self-time reductions are valid
- a cumulative percentage derived from the initial and final absolute rows would not be valid

---

## 7. Final profile and remaining bottlenecks

The final profile covered approximately 16.2 seconds and more than 10,000 samples per DPR.

| Sampled self-time leader | DPR 1 | DPR 2 |
|---|---:|---:|
| Taffy flexbox | 650.963 ms | 712.181 ms |
| `request_taffy_node` | 260.972 ms | 256.033 ms |
| `Window::draw` | 195.155 ms | 192.091 ms |
| Taffy block | 162.616 ms | 139.807 ms |
| Line painting | 139.636 ms | 132.566 ms |
| Global ID Arc allocation | 138.146 ms | 129.685 ms |
| Bounds insertion | 129.569 ms | 149.105 ms |
| Style-to-Taffy conversion | 101.050 ms | 106.092 ms |
| Monochrome scene insertion | 90.282 ms | 85.507 ms |
| Ordinary element-state removal | 65.701 ms | 91.270 ms |

Ranked remaining opportunities:

1. Taffy flex layout remains the dominant cost.
2. Gpux still converts and equality-checks 1,123 styles and topologies per warm frame even though they are unchanged.
3. Global ID `Arc<[ElementId]>` allocation remains measurable, but every tested interner or reuse scheme regressed.
4. Bounds insertion remains material after the 82.74% traversal reduction.
5. Line painting remains a significant owner.
6. Warm glyph paint performs both a TextSystem raster-bounds cache lookup and an atlas lookup.
7. Effect-free WGPU frames still use the retained intermediate texture and pass-through pass.
8. Broad allocator activity remains visible, but generic pooling repeatedly underperformed direct allocation.

---

## 8. Validation

Final validation passed:

- `cargo fmt`
- gpux Clippy with `-D warnings`
- gpux Clippy with `rayx_diagnostics` and `-D warnings`
- 491 gpux tests
- 52 gpui_wgpu tests
- headed Test Suite WASM/WebGPU at DPR 1
- headed Test Suite WASM/WebGPU at DPR 2
- exact final offset `-351402.6875`
- zero terminal runtime errors
- zero buffer restarts
- zero atlas writes in the warmed final frame
- one WGPU submission
- loop-state validation
- AgentX memory audit

The final validation record is `.agentx/experiments/2026-08-30-gpux-maximum-performance/2026-08-31-149-final-gpux-crate-validation.json`.

---

# Appendix A: Complete iteration ledger

Legend:

- `Accepted` means the experiment’s evidence was accepted. It may represent a production optimization, diagnostic, profile, research conclusion, or validation.
- `Reverted` means the attempted mechanism failed its gate and no product change remains.

## Iterations 1–25

| Iteration | Outcome | Experiment |
|---:|---|---|
| 1 | Accepted | WGPU Frame Telemetry |
| 2 | Reverted | WebGPU Family Instance Writes |
| 3 | Accepted | Batch Iterator Two Minimum Selection |
| 4 | Accepted | Sprite Sort Disorder Telemetry |
| 5 | Accepted | Bounds Tree Traversal Telemetry |
| 6 | Accepted | Bounds Tree Intersection Push Pruning |
| 7 | Accepted | Bounds Tree Single Propagation Update |
| 8 | Reverted | Bounds Tree Recorded Descent Positions |
| 9 | Accepted | Global Element ID Path Telemetry |
| 10 | Accepted | Taffy Layout Tree Telemetry |
| 11 | Accepted, later superseded | Taffy Two Child Inline Storage |
| 12 | Reverted | Taffy 0.14.0 Upgrade using superseded vendor mechanism |
| 13 | Accepted | Remove Taffy Vendor Surface |
| 14 | Accepted | Registry Taffy 0.14 Upgrade |
| 15 | Reverted | Taffy SecondaryMap Layout Caches |
| 16 | Reverted | Combined Absolute Layout Cache |
| 17 | Reverted | Layout Bounds Cache Telemetry |
| 18 | Reverted | Complete Layout Bounds Cache Telemetry |
| 19 | Reverted | Shader Semantic Test Normalization |
| 20 | Reverted | gpui_wgpu Suite Hang Isolation |
| 21 | Accepted | Shader Test Whitespace Normalization |
| 22 | Reverted | Layout Bounds Cache Telemetry formatting attempt |
| 23 | Accepted | Layout Bounds Cache Telemetry with Formatted Oracle |
| 24 | Reverted | Direct Cached Parent Origin Lookup |
| 25 | Reverted | Direct Cached Parent Origin with Formatted Branch |

## Iterations 26–50

| Iteration | Outcome | Experiment |
|---:|---|---|
| 26 | Reverted | Deferred Draw Traversal Telemetry |
| 27 | Accepted | Wide DataTable CPU Profile |
| 28 | Accepted | Taffy Measured Context Cleanup |
| 29 | Accepted | Taffy Layout Node Reuse |
| 30 | Accepted | Taffy Active Container Detach List |
| 31 | Accepted | Taffy Unchanged Style Write Elision |
| 32 | Accepted | Taffy Measured Context Slot Reuse |
| 33 | Accepted | Post-Pool Wide DataTable CPU Profile |
| 34 | Accepted | Taffy Unchanged Topology Write Elision |
| 35 | Accepted | Post-Topology Wide DataTable CPU Profile |
| 36 | Reverted | Pooled Taffy Topology Shadow |
| 37 | Accepted | Identity Element Opacity Elision |
| 38 | Accepted | Post-Opacity Wide DataTable CPU Profile |
| 39 | Accepted | Residual Color Caller Trace |
| 40 | Accepted | Arena Trivial Drop Record Elision |
| 41 | Accepted | Post-Arena Wide DataTable CPU Profile |
| 42 | Accepted | Monochrome Sprite Cached Key Sort |
| 43 | Accepted | Post-Cached-Key Sprite Sort CPU Profile |
| 44 | Reverted | Reusable Monochrome Sort Keys |
| 45 | Accepted | Global Element ID Frame Reuse Telemetry |
| 46 | Reverted | Bounded Global Element ID Interner |
| 47 | Accepted | Global Element ID Cost Profile |
| 48 | Accepted | Fx Global Element ID Hash |
| 49 | Accepted | Post-Fx Element ID Profile |
| 50 | Accepted | Minimal Taffy Feature Set |

## Iterations 51–75

| Iteration | Outcome | Experiment |
|---:|---|---|
| 51 | Accepted | Post-Minimal Taffy Profile |
| 52 | Accepted | Disable Taffy Content Size |
| 53 | Accepted | Post-Content-Size Taffy Profile |
| 54 | Accepted | Taffy Measured Leaf Stability Telemetry |
| 55 | Accepted | Taffy Measurement Origin Telemetry |
| 56 | Accepted | Text Measurement Path Telemetry |
| 57 | Accepted | Line Layout Cache Tier Telemetry |
| 58 | Accepted | Line Layout Cache Key Cost Telemetry |
| 59 | Accepted | Cached Line Layout Key Hash |
| 60 | Accepted | Text Local Cache Miss Reasons |
| 61 | Accepted | Text Decoration Run Cardinality |
| 62 | Accepted | Input Sized Decoration Runs |
| 63 | Accepted | Font Run Pool Telemetry |
| 64 | Accepted | Thread Local Font Run Scratch |
| 65 | Accepted | Lazy Line Wrapper Acquisition |
| 66 | Accepted | Post Text Pool CPU Profile |
| 67 | Accepted | Explicit WASM Constructors |
| 68 | Accepted | Conditional Prepaint Style Recompute |
| 69 | Accepted | Style Phase Reuse Eligibility Telemetry |
| 70 | Reverted | Conservative Prepaint Style Handoff |
| 71 | Accepted | Frame-Bounded Prepaint Style Store |
| 72 | Reverted | Taffy Node Request Cost Telemetry |
| 73 | Accepted | Split Taffy Node Request Telemetry |
| 74 | Reverted | Borrowed Taffy Child Iterator Comparison |
| 75 | Accepted | Post Style Handoff CPU Profile |

## Iterations 76–100

| Iteration | Outcome | Experiment |
|---:|---|---|
| 76 | Reverted | Request-to-Prepaint Style Eligibility Telemetry |
| 77 | Accepted | Crate-Visible Style Eligibility Telemetry |
| 78 | Reverted | Frame-Bounded Request Style Handoff |
| 79 | Accepted | Request Style Ownership Telemetry |
| 80 | Accepted | Gpux-Owned Request Style Handoff |
| 81 | Accepted | Post Request Handoff CPU Profile |
| 82 | Accepted | Current Taffy Flex Allocation Research |
| 83 | Accepted | Interactivity Default Base Style Telemetry |
| 84 | Accepted | Post Handoff Allocator Stack Attribution |
| 85 | Accepted | Full Run Line Layout Cache Aggregation |
| 86 | Reverted | Direct Wrapped Miss Shaping |
| 87 | Reverted | Shared Wrapped Key Hash Prefix |
| 88 | Accepted | Interactivity Representation Research |
| 89 | Accepted | Window Frame Clear Drop Attribution |
| 90 | Reverted | DevTools Semantics Runtime Cardinality |
| 91 | Accepted | Lazy DevTools Frame Semantics |
| 92 | Accepted | Post Lazy Semantics CPU Profile |
| 93 | Accepted | Taffy Measured Dirty Propagation Research |
| 94 | Accepted | Element State Cleanup Attribution |
| 95 | Accepted | Dispatch Tree Cleanup Attribution |
| 96 | Accepted | Scene Cleanup Attribution |
| 97 | Accepted | Compact Scene Replay Locator Research |
| 98 | Reverted | Compact Scene Replay Operations |
| 99 | Reverted | Formatted Compact Scene Replay Operations |
| 100 | Reverted | Corrected Compact Scene Replay Operations |

## Iterations 101–125

| Iteration | Outcome | Experiment |
|---:|---|---|
| 101 | Accepted | Post Scene Rejection CPU Profile |
| 102 | Accepted | Previous Frame Global ID Arc Reuse Research |
| 103 | Reverted | Window Frame Global ID Arc Reuse |
| 104 | Reverted | Warning-Clean Window Frame Global ID Reuse |
| 105 | Accepted | Global ID Ordinal Reuse Locator |
| 106 | Accepted | Global ID Ordinal Stability Telemetry |
| 107 | Reverted | Global ID Ordinal Vector Arc Reuse |
| 108 | Reverted | Crate-Visible Global ID Ordinal Vector Reuse |
| 109 | Accepted | Retire Global ID Ordinal Probe |
| 110 | Accepted | Accepted Render Hotspot Reselection |
| 111 | Reverted | Eight-Child Bounds Tree |
| 112 | Reverted | Sixteen-Child Bounds Tree |
| 113 | Accepted | Current Twelve-Child Bounds Tree Profile |
| 114 | Reverted | Paired Sixteen-Child Bounds Tree |
| 115 | Accepted | Bounds Tree Area Cost Research |
| 116 | Reverted | Direct Bounds Search Child Loop |
| 117 | Accepted | Deferred Bounds Intersection Research |
| 118 | Accepted | Bounds Post-Intersection Prune Telemetry |
| 119 | Reverted | Deferred Bounds Intersection |
| 120 | Reverted | Retire Bounds Prune Probe |
| 121 | Accepted | Current Bounds Probe Baseline |
| 122 | Reverted | Paired Bounds Probe Retirement |
| 123 | Accepted | Test Suite Asset Lock Verification |
| 124 | Accepted | Recovered Bounds Probe Retirement |
| 125 | Accepted | Current Clean CPU Hotpath Profile |

## Iterations 126–150

| Iteration | Outcome | Experiment |
|---:|---|---|
| 126 | Accepted | Taffy Node Refresh API Research |
| 127 | Reverted | Taffy Child Iterator Topology Check |
| 128 | Accepted | Global Element ID Ownership Research |
| 129 | Accepted | Element State Finish Scan Research |
| 130 | Accepted | Element State Finish Telemetry |
| 131 | Accepted | Replay-Only Element State Finish |
| 132 | Accepted | Retire Element State Finish Probe |
| 133 | Accepted | Scene Insertion Storage Research |
| 134 | Accepted | WGPU Buffer Upload Research |
| 135 | Accepted | Shader Pipeline Lifetime Research |
| 136 | Accepted | Atlas Upload Cache Research |
| 137 | Accepted | Bind Group Lifetime Research |
| 138 | Accepted | Style-to-Taffy Reuse Research |
| 139 | Accepted | Window Draw Orchestration Research |
| 140 | Accepted | WASM Frame Scheduling Research |
| 141 | Accepted | Current Taffy Release Research |
| 142 | Accepted | Text Raster Bounds Research |
| 143 | Accepted | WGSL Shader Branch Audit |
| 144 | Accepted | WGPU Submission and Pass Audit |
| 145 | Accepted | Frame Arena Lifetime Audit |
| 146 | Accepted | Render Lock and Synchronization Audit |
| 147 | Accepted | Final Clean CPU Hotpath Profile |
| 148 | Accepted | Final Gpux Crate Validation |
| 149 | Accepted | Taffy Provenance and Vendor Closure Audit |
| 150 | Accepted | Iteration Cap and Architecture Synthesis |

---

## Final assessment

The sprint materially improved gpux by removing repeated work, allocation churn, synchronization, hashing, style recomputation, frame-finish scans, and repository bloat. It also established strong negative evidence against many more complex caches, pools, compact representations, branching-factor changes, upload coalescing schemes, and interning mechanisms.

The most important result is not a single benchmark percentage. It is that gpux’s accepted hot paths now do substantially less deterministic work while retaining:

- exact rendering
- stable ordering
- replay semantics
- collision-safe identity
- bounded cache ownership
- correct invalidation
- shader ABI correctness
- warning-clean Rust
- focused runtime and unit-test coverage

The sprint leaves gpux faster in several independently proven paths, simpler than many rejected alternatives, and with a much more precise map of where the next meaningful gains can be found.
