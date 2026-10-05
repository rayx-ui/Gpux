# Multithreaded WASM Apps

Use this reference when creating, fixing, or validating gpux/RayX browser apps. RayX browser apps are multithreaded by default.

The commands here build and validate RayX browser apps: run them from `../RayX` (the RayX checkout beside Gpux) unless a project-specific command says otherwise.

## Default Architecture

- Keep `gpui_web` default features enabled. Its default feature set enables `multithreaded`, which pulls `wasm_thread`.
- Do not write `gpui_web = { ..., default-features = false }` for RayX browser apps unless the task explicitly requires a single-threaded build. Inside Gpux, `cargo check -p gpux-web --target wasm32-unknown-unknown --no-default-features` is only a stable-toolchain compile check; the multithreaded default build needs nightly `-Z build-std`, which RayX's xtask provides.
- Keep app source imports on `gpux::{...}` and dependencies pointed at the gpux crates (RayX uses the sibling `../Gpux` checkout; other projects can use the crates.io `gpux-*` releases).
- Start RayX WASM directly from Rust orchestration such as `cargo run -p xtask -- app apps/test_suite build wasm` or `cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu`.

GOOD (RayX root `Cargo.toml`):

```toml
[workspace.dependencies]
gpui_web = { package = "gpux-web", path = "../Gpux/crates/gpui_web" }
```

BAD:

```toml
[workspace.dependencies]
gpui_web = { package = "gpux-web", path = "../Gpux/crates/gpui_web", default-features = false }
```

## Toolchain

Install `wasm-bindgen-cli` at the same version used by the active Rust dependency graph. gpux and RayX currently use `0.2.126`:

```bash
cargo install wasm-bindgen-cli --version 0.2.126 --locked
wasm-bindgen --version
```

Multithreaded `wasm_thread` currently needs nightly Rust for `stdarch_wasm_atomic_wait`. Use nightly for the browser build only, with `rust-src` installed:

```bash
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
rustup target add wasm32-unknown-unknown --toolchain nightly
```

## Cargo Build Flags

Build with atomics, shared memory, imported memory, and the TLS exports required by `wasm-bindgen` plus Rust's threaded wasm runtime. The manifest below is the web wrapper crate that `cargo run -p xtask -- app apps/test_suite build wasm` generates in RayX:

```bash
RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals \
  -C link-arg=--shared-memory \
  -C link-arg=--import-memory \
  -C link-arg=--max-memory=2147483648 \
  -C link-arg=--export=__heap_base \
  -C link-arg=--export=__wasm_init_tls \
  -C link-arg=--export=__tls_size \
  -C link-arg=--export=__tls_align \
  -C link-arg=--export=__tls_base" \
cargo +nightly build \
  --manifest-path artifacts-temp/apps/RayXTestSuite/wasm/rust/Cargo.toml \
  --target wasm32-unknown-unknown \
  --target-dir target \
  -Z build-std=std,panic_abort
```

Then run `wasm-bindgen` against the generated module:

```bash
wasm-bindgen target/wasm32-unknown-unknown/debug/rayx_test_suite_web.wasm \
  --target web \
  --out-dir artifacts/wasm
```

Prefer keeping these flags in Rust orchestration (`xtask`) so local builds, CI, and Playwright validation all use the same path.

## Browser Server Requirements

SharedArrayBuffer and wasm threads require cross-origin isolation. Serve the generated app with these headers on every HTML, JavaScript, and WASM response:

```text
Cross-Origin-Embedder-Policy: require-corp
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Resource-Policy: same-origin
```

The browser host page should expose simple diagnostics for Playwright:

- `globalThis.crossOriginIsolated === true`
- `typeof globalThis.SharedArrayBuffer === "function"`
- `typeof globalThis.Atomics === "object"`
- `typeof globalThis.Atomics.waitAsync === "function"`
- `navigator.gpu` is present when WebGPU is required.

Use an explicit viewport meta tag in the generated HTML host:

```html
<meta name="viewport" content="width=device-width, initial-scale=1.0" />
```

For canvas-backed browser apps, `gpui_web` should use the visible browser viewport as the default logical window bounds, not the native desktop 75% centered default window size. Initialize the canvas backing store before the first frame to `logical_size * devicePixelRatio`, clamped by the adapter's max texture dimension, then let `ResizeObserver` handle later viewport or DPR changes. In the resize path, derive physical backing-store size from `ResizeObserverEntry.contentRect` CSS size multiplied by `window.devicePixelRatio`; do not rely on `devicePixelContentBoxSize` alone, because Chromium under Playwright-emulated high DPI can report values that reset the backing store to 1x.

When diagnosing a browser app that looks zoomed, separate canvas/DPR scale from font metrics. RayX story-style browser apps should keep the bundled proportional Latin UI font (`IBM Plex Sans`) as the primary `Theme::font_family`; load CJK fonts such as Noto Sans SC for fallback coverage, but do not make a CJK font the global Latin UI face unless the app is intentionally CJK-first.

## Browser Clipboard

Invoke browser clipboard writes while the trusted keyboard or menu gesture is still on the stack. `navigator.clipboard.writeText(...)` can reject when `clipboard-write` permission is denied even though the user initiated Copy, so keep `gpui_web`'s synchronous one-shot `copy` event fallback alongside the modern promise. Preserve the document selection, select a focus-neutral offscreen rendered range so Chromium emits `copy`, supply the exact plain-text payload through `ClipboardEvent.clipboardData`, call `preventDefault()`, and restore the prior selection. Do not focus a temporary input: its focus events can re-enter GPUI action dispatch, while default rendered-selection serialization can normalize or trim multiline/trailing-newline content.

Prove context-menu Copy in headed Test Suite WASM with write permission denied, multiple selected lines including a trailing newline, and an actual `navigator.clipboard.readText()` comparison. Granting `clipboard-write` masks the failure mode.

## Binary Validation

After `wasm-bindgen`, verify the generated `*_bg.wasm` before launching the browser. The module must have:

- an imported or declared shared memory with wasm memory flags containing `0x02`
- an exported `wasm_thread_entry_point`
- at least one atomic instruction prefix byte `0xfe`

Keep `gpui_platform::web_init()` as the browser entry path and call the linker-synthesized `__wasm_call_ctors` from there. Without an explicit call reachable from an export, `wasm-ld` can classify the module as command-style and inject the complete constructor traversal at the start of every exported function. This is especially expensive for GPUI's inventory-backed action registrations, even though inventory guards repeated submissions for correctness. After linking, disassemble the final `*_bg.wasm`: `__wasm_call_ctors` should be called from `gpui_platform::web_init`, while `run`, `wasm_thread_entry_point`, `__wbindgen_malloc`, `__wbindgen_free`, and other exports should not have linker-generated command wrappers. Preserve the `SAFETY:` explanation at the call site and continue to invoke `web_init` once before application use.

In RayX, `cargo run -p xtask -- app apps/test_suite build wasm` performs this check for Test Suite. If creating another app, use that app directory with the same `app <dir> build wasm` shape for developer builds, but keep agent-driven runtime validation and diagnostics on Test Suite.

## Playwright Validation

New Codex worktrees automatically prepare `tools-node` and reuse the operating-system Playwright Chromium cache. For an older or interrupted worktree, run `cargo run -p xtask -- setup-wasm` before validation; never set `PLAYWRIGHT_BROWSERS_PATH=0`, copy or junction `node_modules`, or create a worktree-local browser cache.

Launch headed Chromium or Edge with WebGPU enabled:

```bash
cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu
```

Use the smallest focused headed browser spec that proves the changed behavior.
WebGPU remains the preferred backend, but RayX automatically retries with WebGL2
when WebGPU initialization or adapter selection fails. Force either path for
diagnosis with `?backend=webgl` or `?backend=webgpu`; use
`../RayX/tools-node/test_suite/rayx-webgl-fallback.spec.ts` when backend selection or the
renderer transport changes.

The Playwright project should pass browser flags similar to:

```text
--enable-unsafe-webgpu
--ignore-gpu-blocklist
--enable-features=Vulkan,WebGPU,SharedArrayBuffer
```

Capture traces, screenshots, video, and JSON diagnostics when startup fails. Check the JSON first for:

- `crossOriginIsolated`
- `sharedArrayBufferAvailable`
- `atomicsWaitAsyncAvailable`
- `wasmThreadsAvailable`
- `webgpuAvailable`
- viewport meta content (`width=device-width, initial-scale=1.0`)
- `visualViewport.scale`
- CSS canvas bounds compared with `window.innerWidth`/`window.innerHeight`
- canvas backing-store-to-CSS ratio compared with `devicePixelRatio`
- console errors and page errors

If threads are unavailable, inspect server headers before changing Rust code. If WebGPU is unavailable, inspect browser flags, adapter availability, GPU blocklist state, and platform driver support, then confirm the automatic WebGL2 retry. A forced `?backend=webgpu` request intentionally disables fallback.

WebGPU supplies packed scene records through storage buffers. WebGL2 cannot, so
gpui_wgpu stores the same 32-bit words in `Rgba32Uint` textures and reconstructs
records in WGSL. Preserve the Rust byte-size/stride assertions for every record
family and the WebGL binding aliases used by shaders that dynamically select an
auxiliary record source.

For WebGPU renderer changes that add new storage-buffer instance structs, verify Rust `Pod` layout against WGSL storage layout before trusting draw counters. Avoid `vec3<f32>` as a padding member unless the Rust struct intentionally includes the same 16-byte alignment; scalar `f32` padding is safer when the Rust struct is expected to stay tightly packed. Dawn validation messages such as `bound with size 48 ... requires ... at least 64 bytes` mean the command buffer is invalid even if higher-level diagnostics counted a submitted draw.

For the gpux paint ABI specifically, Rust `PaintColor` is four tightly packed `f32` fields with byte size 16 and alignment 4. Model it in WGSL as a scalar-field struct and convert it to `vec4<f32>` at use sites; a `vec4<f32>` alias raises storage alignment to 16 and changes enclosing sprite strides. Keep the paint values premultiplied encoded sRGB, use premultiplied-alpha blending, and render to the non-sRGB surface format so translucent colors blend in encoded sRGB like Figma and CSS; only blur taps decode to linear light for averaging. Headed Playwright paint tests should collect console errors as failures so Dawn layout errors cannot coexist with a nominally submitted frame.

`gpux_color` is the sole standards color owner and gpux exposes its persistent
value as `gpux::Color`. Persistent style/component state stores `Color`; scene
construction obtains the cached `PaintColor` once at the renderer boundary.
Do not introduce a RayX color adapter, a second canonical color type, or warm
per-frame conversions alongside this path.

## Editable Text Diagnostics

`EditableTextStyleRuns` is revision-bound. Every buffer mutation and history restore must reset stored runs to `EditableTextStyleRuns::empty(current_revision)` unless the owner installs fresh runs for that exact revision. Do not carry runs across revisions: a stale style-run revision can make layout reject the update and paint an empty frame even though the buffer still contains text.

Prove text fixes in headed WASM/WebGPU with stable diagnostics for buffer, layout, and style-run revisions plus shaped-row, cache-hit, and invalidated-row counters. Assert the three revisions are equal after real input and delayed redraws. Keep temporary traces only long enough to identify the mismatched transition, then remove them.

Run narrow performance specs serially and measure action-to-published-frame latency in browser code. Report cold startup and environment metadata separately so process startup, Playwright transport, and polling backoff do not contaminate warm renderer measurements.

When considering a sprite-sort fast path, measure pre-sort adjacent `(order, tile_id)` disorder in `../RayX/apps/test_suite` first. The wide virtualized DataTable is not usually sorted: its warmed monochrome stream measured roughly 44-45% descending adjacent pairs at DPR 1 and 2, so skipping the stable sort or optimizing only the already-sorted case is not an evidence-backed optimization for that workload.

When optimizing `BoundsTree`, measure max-leaf hits, slow searches, visited nodes, and insertion child evaluations separately. In the wide virtualized DataTable, only 7 of 504 warmed insertions use the max-leaf path. Keep intersection pruning at the stack-push boundary: rejecting non-intersecting roots and children before stacking reduced slow-search node visits from 25,720 to 4,440 (82.74%) at both DPRs. Keep terminal-node bounds/max updates solely in the shared reverse-propagation loop; updating a non-full terminal before that loop duplicates the same union and comparison. Preserve the randomized order oracle and headed WebGPU p95 for both mechanisms.

Measure `GlobalElementId` path depth before replacing its `Arc<[ElementId]>` storage with an inline collection. The wide virtualized DataTable creates 485 paths containing 8,668 elements per warmed frame at both DPRs (average 17.87, maximum 23); only 36.70% fit within 16 elements. Small inline capacities therefore miss the dominant workload while making cheap `Arc` clones larger or more expensive, so retain the Arc representation unless a different fixture and clone-pressure measurement provide contrary evidence.

Measure Taffy child fanout before changing its per-node child storage. The wide virtualized DataTable rebuilds 782 non-leaf vectors and 1,041 child edges per warmed frame; 92.84% of containers have at most two children, 96.55% at most four, and 99.74% at most eight. Capacity two is the smallest inline candidate that covers a dominant share, but validate the larger outer slot-map entry and overflow path against allocation counts and headed p95 before accepting a local Taffy patch.
