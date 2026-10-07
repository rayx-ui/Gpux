# Multithreaded WASM Apps

Use this reference when creating, fixing, or validating gpux browser apps. Browser apps, RayX's included, are multithreaded by default.

Run the commands here from the Gpux repository root. gpux's own threaded browser app is the `web_host` example of `gpux-conformance` (`crates/gpux_conformance/examples/web_host`): `gpux-testkit web build` builds it, and lane B's Playwright specs in `tools-node/specs/` drive it ([docs/testing.md](../../../../docs/testing.md#lane-b-a-real-browser)).

## Default Architecture

- Keep `gpui_web` default features enabled. Its default feature set enables `multithreaded`, which pulls `wasm_thread`.
- Do not write `gpui_web = { ..., default-features = false }` for browser apps (RayX's included) unless the task explicitly requires a single-threaded build. Inside Gpux, `cargo check -p gpux-web --target wasm32-unknown-unknown --no-default-features` is only a stable-toolchain compile check; the multithreaded default build needs nightly `-Z build-std`, which `gpux-testkit web build` provides.
- Keep app source imports on `gpux::{...}` and dependencies pointed at the gpux crates (RayX uses the sibling `../Gpux` checkout; other projects can use the crates.io `gpux-*` releases).
- Build gpux's threaded host through Rust orchestration: `cargo run -p gpux-testkit -- web build [--verify]` writes it to `target/gpux-web/dist`, and `cargo run -p gpux-testkit -- web serve [--port 8000]` serves it cross-origin isolated (open `/?scene=<id>`).

GOOD (consumer guidance, RayX root `Cargo.toml`):

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

Install `wasm-bindgen-cli` at the same version used by the active Rust dependency graph. gpux's `Cargo.lock` pins `0.2.129`, and `gpux-testkit web build` refuses any other CLI version:

```bash
cargo install wasm-bindgen-cli --version 0.2.129 --locked
wasm-bindgen --version
```

Multithreaded `wasm_thread` currently needs nightly Rust for `stdarch_wasm_atomic_wait`. Use nightly for the browser build only, with `rust-src` installed (`GPUX_WEB_TOOLCHAIN` names another nightly for the testkit):

```bash
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
rustup target add wasm32-unknown-unknown --toolchain nightly
```

## Cargo Build Flags

Build with atomics, shared memory, imported memory, and the TLS exports required by `wasm-bindgen` plus Rust's threaded wasm runtime. This is the reference build `gpux-testkit web build` runs for the `web_host` example (`tooling/testkit/src/web.rs`); another app uses the same flags with its own package:

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
CFLAGS_wasm32_unknown_unknown="-matomics -mbulk-memory -mmutable-globals" \
cargo +nightly build -p gpux-conformance \
  --no-default-features --features app-host \
  --example web_host \
  --target wasm32-unknown-unknown \
  --release \
  --target-dir target/gpux-web \
  -Z build-std=std,panic_abort
```

Then it runs `wasm-bindgen` against the linked module and writes `index.html`, `catalog.json` (the lane-B scenes the specs enumerate), and the scene assets beside the bindings:

```bash
wasm-bindgen target/gpux-web/wasm32-unknown-unknown/release/examples/web_host.wasm \
  --target web \
  --out-dir target/gpux-web/dist
```

Prefer keeping these flags in Rust orchestration (gpux keeps them in `gpux-testkit`) so local builds, CI, and Playwright validation all use the same path.

## Browser Server Requirements

SharedArrayBuffer and wasm threads require cross-origin isolation. Serve the generated app with these headers on every HTML, JavaScript, and WASM response (`gpux-testkit web serve` and `web e2e` do):

```text
Cross-Origin-Embedder-Policy: require-corp
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Resource-Policy: same-origin
```

The browser host page should expose simple diagnostics for Playwright (gpux's host page records cross-origin isolation, `SharedArrayBuffer`, and `Atomics.waitAsync` in `globalThis.__gpuxThreadDiagnostics`):

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

Prove clipboard changes in lane B with `tools-node/specs/clipboard.spec.ts` (`cargo run -p gpux-testkit -- lane b --grep clipboard`); its fallback cases remove the Clipboard API and read the system clipboard back. A context-menu Copy change also needs a case with write permission denied, multiple selected lines including a trailing newline, and an actual `navigator.clipboard.readText()` comparison. Granting `clipboard-write` masks the failure mode.

## Binary Validation

`gpux-testkit web build --verify` checks the generated `web_host_bg.wasm` before any browser starts (`web e2e` and `lane b` always verify; `tooling/testkit/src/wasm_verify.rs` is the reference check for another app's `*_bg.wasm`). The module must have:

- an imported or declared shared memory with wasm memory flags containing `0x02`
- an exported `wasm_thread_entry_point`
- at least one atomic instruction prefix byte `0xfe`
- exactly one call site of `__wasm_call_ctors`, the explicit one from `gpui_platform::web_init`, and no `<export>.command_export` wrappers; keep the name section (do not strip it), which the check reads

Keep `gpui_platform::web_init()` as the browser entry path and call the linker-synthesized `__wasm_call_ctors` from there. Without an explicit call reachable from an export, `wasm-ld` can classify the module as command-style and inject the complete constructor traversal at the start of every exported function. This is especially expensive for GPUI's inventory-backed action registrations, even though inventory guards repeated submissions for correctness. In the final `*_bg.wasm`, `__wasm_call_ctors` should be called only from `gpui_platform::web_init` (in release builds `web_init` usually inlines into the app's entry function, which then holds the single call), while `run`, `wasm_thread_entry_point`, `__wbindgen_malloc`, `__wbindgen_free`, and other exports should not have linker-generated command wrappers. Preserve the `SAFETY:` explanation at the call site and continue to invoke `web_init` once before application use.

## Playwright Validation

Playwright lives in gpux's `tools-node/`. Set it up once per checkout, with Node.js 22 or later and `pnpm`:

```bash
cd tools-node
pnpm install
pnpm exec playwright install chromium
```

Playwright reuses the operating-system Chromium cache; never set `PLAYWRIGHT_BROWSERS_PATH=0`, copy or junction `node_modules`, or create a worktree-local browser cache.

Run lane B narrowed to the area that changed:

```bash
cargo run -p gpux-testkit -- lane b --grep <pattern>
cargo run -p gpux-testkit -- lane b --scene <id>
cargo run -p gpux-testkit -- web e2e [--profile local-gpu] [--grep <pattern>] [--headed]
```

`lane b` runs the `@canary` and `@pr` specs in the profile `GPUX_WEB_PROFILE` names (`local-gpu` by default: new headless Chromium on the machine's GPU); `--grep` (which Playwright matches against the spec file name and test titles, so `--grep clipboard` selects `clipboard.spec.ts`) and `--scene` narrow it, and `web e2e` runs any selection, headed with `--headed`. Both build, verify, and serve first. The specs in `tools-node/specs/`: `scenes.spec.ts` runs every lane-B conformance scene with trusted input; one spec per `gpui_web` area (`backend`, `viewport`, `window`, `pointer`, `wheel`, `keyboard`, `touch`, `ime`, `clipboard`, `html-element`, `html-island`, `text-fallback`, `threads`, `assets`, `cursor`, `lifecycle`, `safe-area`); `dawn.spec.ts` fails on any WebGPU validation error; and the `@timing` `timing.spec.ts` runs only in lane G. Use the smallest selection that proves the changed behavior. New browser coverage goes into gpux: a conformance scene in `crates/gpux_conformance/src/scenes/` that names lane B, or a spec in `tools-node/specs/`.

WebGPU remains the preferred backend, but `gpui_web` automatically retries with WebGL2
when WebGPU initialization or adapter selection fails. Force either path for
diagnosis with `?backend=webgl` or `?backend=webgpu`; run
`tools-node/specs/backend.spec.ts` (`lane b --grep backend`) when backend selection or the
renderer transport changes.

The Playwright profiles in `tools-node/playwright.config.ts` pass the browser flags: `--enable-unsafe-webgpu` and `--enable-blink-features=CanvasDrawElement` on every profile, plus `--enable-features=Vulkan,CDPScreenshotNewSurface --use-angle=swiftshader --use-vulkan=swiftshader` for `ci-linux-swiftshader` (no GPU) and `--use-angle=d3d11-warp` for `windows-software`. [docs/testing.md](../../../../docs/testing.md#chromium-flags-and-troubleshooting) explains each flag and the failure messages.

Traces and failure screenshots land in `artifacts-temp/testkit/b/`. When startup fails, check first:

- `crossOriginIsolated`
- `SharedArrayBuffer` availability
- `Atomics.waitAsync` availability
- wasm thread availability
- WebGPU availability
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

For the gpux paint ABI specifically, Rust `PaintColor` is four tightly packed `f32` fields with byte size 16 and alignment 4. Model it in WGSL as a scalar-field struct and convert it to `vec4<f32>` at use sites; a `vec4<f32>` alias raises storage alignment to 16 and changes enclosing sprite strides. Keep the paint values premultiplied encoded sRGB, use premultiplied-alpha blending, and render to the non-sRGB surface format so translucent colors blend in encoded sRGB like Figma and CSS; only blur taps decode to linear light for averaging. Lane B's specs fail on console errors, and `dawn.spec.ts` renders every scene and fails on any WebGPU validation error, so Dawn layout errors cannot coexist with a nominally submitted frame; keep new paint specs on that fixture.

`gpux_color` is the sole standards color owner and gpux exposes its persistent
value as `gpux::Color`. Persistent style/component state stores `Color`; scene
construction obtains the cached `PaintColor` once at the renderer boundary.
Do not introduce a RayX color adapter, a second canonical color type, or warm
per-frame conversions alongside this path.

## Editable Text Diagnostics

`EditableTextStyleRuns` is revision-bound. Every buffer mutation and history restore must reset stored runs to `EditableTextStyleRuns::empty(current_revision)` unless the owner installs fresh runs for that exact revision. Do not carry runs across revisions: a stale style-run revision can make layout reject the update and paint an empty frame even though the buffer still contains text.

Prove text fixes in lane A and in WASM/WebGPU through lane B (a conformance scene or a spec in `tools-node/specs/`), with stable diagnostics for buffer, layout, and style-run revisions plus shaped-row, cache-hit, and invalidated-row counters. Assert the three revisions are equal after real input and delayed redraws. Keep temporary traces only long enough to identify the mismatched transition, then remove them.

Run narrow performance specs serially and measure action-to-published-frame latency in browser code. Report cold startup and environment metadata separately so process startup, Playwright transport, and polling backoff do not contaminate warm renderer measurements. Frame budgets belong in `@timing` specs, which only lane G runs (`cargo run -p gpux-testkit -- lane g [--headed]` on a machine with a GPU; summary in `artifacts-temp/testkit/g/timing.md`).

The measurements below were taken on the wide virtualized DataTable workload. Take new ones on gpux's benchmarks (`cargo bench -p gpux-platform --features bench-support --bench randomized_element_tree --bench inspector_render`, which lane G runs on the GPU; `BENCH_MEASUREMENT` picks wall time, instructions, or allocations) or a lane B spec, with the `diagnostics` cargo feature for the counters.

When considering a sprite-sort fast path, measure pre-sort adjacent `(order, tile_id)` disorder first. The wide virtualized DataTable workload is not usually sorted: its warmed monochrome stream measured roughly 44-45% descending adjacent pairs at DPR 1 and 2, so skipping the stable sort or optimizing only the already-sorted case is not an evidence-backed optimization for that workload.

When optimizing `BoundsTree`, measure max-leaf hits, slow searches, visited nodes, and insertion child evaluations separately. On the wide virtualized DataTable workload, only 7 of 504 warmed insertions use the max-leaf path. Keep intersection pruning at the stack-push boundary: rejecting non-intersecting roots and children before stacking reduced slow-search node visits from 25,720 to 4,440 (82.74%) at both DPRs. Keep terminal-node bounds/max updates solely in the shared reverse-propagation loop; updating a non-full terminal before that loop duplicates the same union and comparison. Preserve the randomized order oracle, and recheck lane G's frame-work p95 for both mechanisms.

Measure `GlobalElementId` path depth before replacing its `Arc<[ElementId]>` storage with an inline collection. The wide virtualized DataTable workload creates 485 paths containing 8,668 elements per warmed frame at both DPRs (average 17.87, maximum 23); only 36.70% fit within 16 elements. Small inline capacities therefore miss the dominant workload while making cheap `Arc` clones larger or more expensive, so retain the Arc representation unless a different fixture and clone-pressure measurement provide contrary evidence.

Measure Taffy child fanout before changing its per-node child storage. The wide virtualized DataTable workload rebuilds 782 non-leaf vectors and 1,041 child edges per warmed frame; 92.84% of containers have at most two children, 96.55% at most four, and 99.74% at most eight. Capacity two is the smallest inline candidate that covers a dominant share, but validate the larger outer slot-map entry and overflow path against allocation counts and lane G's frame-work p95 before accepting a local Taffy patch.
