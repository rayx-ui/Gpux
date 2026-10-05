# Update From Zed GPUI Closure

Use this reference when updating gpux directly from upstream Zed while keeping only the GPUI crate closure.

Run from the Gpux repository root. Upstream paths are from `zed-industries/zed`; Zed `crates/<name>/` maps to the same relative path `crates/<name>/` in Gpux.

## Guardrails

- Start from a clean worktree or separate branch.
- Use `https://github.com/zed-industries/zed` as the source of truth for upstream GPUI closure imports.
- Fetch or shallow-clone upstream Zed into a temporary/cache location. A local checkout such as `../.forks/zed` is optional cache only, not a required source.
- Do not import the full Zed application tree.
- Preserve gpux-local files (`crates/gpux_color`, `crates/gpux_fonts`, `crates/gpui_mobile`, `tooling/perf`, `vendor/wgpu`, `docs/`, `.claude/`, and root metadata) unless the update explicitly requires changing them.
- Preserve the local main package/import crate name `gpux` (even though the upstream source path remains `crates/gpui`) and the `gpux-*` package names of the other crates.
- Preserve or replay RayX/gpux patches from gpux's own history after importing upstream Zed closure state.
- Reconcile with a semantic three-way merge from the last represented Zed commit. Audit both the upstream delta and the local delta from that base; never treat either current tree as a safe wholesale winner.
- Keep one import/replay step per commit so provenance stays reviewable.
- Record provenance against upstream Zed commits, not a local checkout.
- Use `docs/history/upstream-provenance.md`, its ledgers, and the archived pre-migration history (annotated tag `gpux-pre-migration-2026-06-17`) as the self-contained pre-migration source. Do not depend on a predecessor RayX checkout.

## Current Closure Path Mapping

Treat this list as the current gpux closure shape. Re-check the root `Cargo.toml` workspace members before each update because the closure can grow or shrink. Each Zed `crates/<name>` path maps to the same `crates/<name>` path here:

- `crates/collections`
- `crates/gpui`
- `crates/gpui_linux`
- `crates/gpui_macos`
- `crates/gpui_macros`
- `crates/gpui_platform`
- `crates/gpui_shared_string`
- `crates/gpui_util`
- `crates/gpui_web`
- `crates/gpui_wgpu`
- `crates/gpui_windows`
- `crates/http_client`
- `crates/http_client_tls`
- `crates/media`
- `crates/path`
- `crates/refineable`
- `crates/reqwest_client`
- `crates/scheduler`
- `crates/sum_tree`
- `crates/util`
- `crates/util_macros`

The one path that moves is `assets/fonts` -> `crates/gpux_fonts/assets/fonts`; see the asset rule under Manifest And Packaging Shape.

Do not import `zlog`, `ztracing` or `ztracing_macro`. gpux removed them because RayX never builds with `--cfg ztracing`, where they only expand to no-ops. Strip `#[ztracing::instrument(...)]`/`#[instrument(...)]` attributes, `use ztracing::...` imports and `zlog::init_test()` test loggers from imported closure sources; use the `profiling` crate when instrumentation is needed.

Preserve `tooling/perf` as gpux-local tooling unless the user deliberately asks to refresh it from another source.

## Manifest And Packaging Shape

Every gpux crate publishes to crates.io, so imported manifests and sources must keep gpux's publishing shape. The [README crate table](../../../../README.md#crates) is the authoritative directory/package/Rust-name mapping.

- Package names stay `gpux-*` (the main crate is `gpux`) with the GPUI Rust name in `[lib] name`, whatever package name the upstream manifest uses, so imported source keeps writing `gpui_platform::`, `collections::`, `util::`, and the other upstream crate paths.
- Manifests keep `version.workspace`, `license.workspace`, `repository.workspace`, `homepage.workspace`, and `rust-version.workspace`, a `description`, and no `publish = false`.
- Workspace dependency keys keep the Rust name with `package = "gpux-..."`, for example `collections = { package = "gpux-collections", path = "crates/collections", version = "0.3.0" }`.
- Dev-dependencies on the cycle edges `gpui_macros` -> `gpux` and `gpux` -> `gpui_platform`/`gpui_web` stay path-only (no `version`), so publishing strips them.
- Dependencies come from crates.io only: no git dependencies and no `[patch]`. Map Zed's git forks to their crates.io releases `zed-reqwest 0.12.15-zed`, `zed-scap 0.0.8-zed`, `zed-font-kit 0.14.1-zed`, and `zed-xim 0.4.0-zed` (declared with `package = "zed-..."`). `proptest 1.11`, `async-task 4.7.1`, and `calloop 0.14.5` come from crates.io. Zed's calloop fork bounds idle callbacks to about 10 ms per loop iteration; gpux gets that behavior from `ForegroundTasks` (`crates/gpui_linux/src/linux/foreground_tasks.rs`) instead, so when importing Zed's X11 or Wayland client, keep routing main-thread runnables through `ForegroundTasks::push` rather than Zed's per-runnable `insert_idle`.
- Never include files from outside a crate directory. Zed's `include_bytes!` of `assets/fonts/...` become `gpux_fonts` constants (`gpux_fonts::IBM_PLEX_SANS_REGULAR`, `gpux_fonts::LILEX_REGULAR`, `gpux_fonts::ALL`, ...), with `gpux_fonts` added as a (dev-)dependency where needed. `crates/gpux_fonts/assets/` is also the asset root that holds `fonts/...` for the SVG renderer; a new upstream font file goes there with a matching constant.
- Every gpux crate is Apache-2.0 (`gpux-fonts` is OFL-1.1). Never re-import Zed's `LICENSE-GPL` or `LICENSE-AGPL` files or GPL/AGPL license fields.

## Workflow

1. Read current provenance:
   - `docs/history/upstream-provenance.md`
   - `docs/history/zed-imports.tsv`
   - `docs/history/gpux-commit-map.tsv`
   - `docs/history/post-embed-filter-map.tsv`
   - `git log --reverse --format='%H %s%n%b' gpux-pre-migration-2026-06-17`
   - `../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/state.md`
   - `../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/rules.md`
   - `../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/knowledge.md`
   - `../RayX/.agentx/memory/decisions/INDEX.md`
   - Relevant `runtime/gpux-foundation`, `tooling/devkit-diagnostics`, and browser/WASM decisions for HTML DOM, fixed/sticky, shader/frame effects, Windows WGPU, and asset-loading behavior.
   - `../RayX/.agentx/memory/knowledge/tooling/validation/state.md`
   - `git log --format='%h%n%B' --grep='Source Zed'`
   - latest import commits with subjects like `chore(gpux): import zed gpui closure ...`

2. Prepare upstream Zed source:
   - Fetch or shallow-clone `https://github.com/zed-industries/zed` into a temporary/cache location.
   - Use an explicit upstream Zed commit SHA for the import target.
   - If reusing an existing local checkout, verify its `origin` or selected remote points at `https://github.com/zed-industries/zed` and fetch before selecting the commit.

3. Identify the source range:
   - Find the last upstream Zed commit already represented in `docs/history/zed-imports.tsv`; confirm it against `Source Zed commit:`, `Source Zed merge commit:`, or older `Imported source parent:` provenance in the archived and current histories.
   - Identify newer upstream Zed commits that touch closure paths.
   - Use upstream Zed commits as closure import points.
   - Audit the touched upstream paths for the whole closure, including `crates/gpui/src/window.rs`, `crates/gpui/src/scene.rs`, `crates/gpui/src/style.rs`, `crates/gpui/src/taffy.rs`, `crates/gpui/src/elements`, `crates/gpui/src/shader_effects`, `crates/gpui_windows`, `crates/gpui_web`, `crates/gpui_wgpu`, `crates/gpui_macos`, shader files, helper crates, and assets.

4. Import upstream closure state:
   - Extract only the closure paths from the chosen upstream Zed source commit.
   - Replace the mapped paths, including deletions inside the closure.
   - Reconcile gpux root metadata manually: `Cargo.toml`, `Cargo.lock`, `.cargo/config.toml`, `clippy.toml`, `rust-toolchain.toml`, repo docs (including the README crate table), and local tooling should stay reduced-workspace aware.
   - Reapply the local main-crate rename from upstream `gpui` to `gpux` in manifests, Rust imports, macro-generated paths, docs, and examples.
   - Reapply the Manifest And Packaging Shape to every imported manifest and include: `gpux-*` package names, `[lib] name`, workspace metadata, crates.io-only dependencies, path-only cycle dev-dependencies, `gpux_fonts` constants, and Apache-2.0 licensing.
   - Commit with a subject like `chore(gpux): import zed gpui closure <short-sha>`.
   - Include commit body lines:

```text
Source Zed commit: <zed-upstream-sha>
Original subject: <original source subject>
```

5. Preserve or replay RayX/gpux GPUI patches:
   - Use gpux's own history and RayX's AgentX patch-surface memories to identify local RayX/gpux patches that must survive the import.
   - Preserve patches already present after reconciliation or replay local gpux commits when upstream replacement overwrote them.
   - Keep each local patch as a separate gpux commit when practical.
   - Adapt only what is needed for the reduced workspace.
   - Include commit body lines:

```text
Preserved gpux patch commit: <gpux-local-sha>
Original subject: <original local subject>
```

6. Reconcile the reduced workspace:
   - Regenerate or update `Cargo.lock` only after workspace manifests are settled.
   - Keep dependencies limited to what the GPUI closure needs, all from crates.io.
   - Add focused follow-up commits for gpux-only metadata or validation fixes when needed.

7. Validate:
   - Run `cargo metadata --no-deps`.
   - Run `cargo fmt --check`.
   - Run at least `cargo check -p gpux --all-targets`.
   - Prefer targeted package checks for `gpux-platform`, `gpux-windows`, `gpux-wgpu-renderer`, `gpux-web` (`--target wasm32-unknown-unknown --no-default-features`), `gpux-util`, and `gpux-shared-string` when the update touches those surfaces.
   - Prefer `cargo check --workspace --all-targets` when time and platform dependencies allow.
   - Run `cargo package --workspace`, because imports change manifests, dependencies, and included files.
   - Run targeted tests/examples for changed framework or backend behavior.
   - When browser or shared renderer behavior changes, run the specs that exercise it from `../RayX`: `cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu --no-default-features --features rayx_diagnostics --playwright-test test_suite/<spec>`.
   - When Windows platform, WGPU surface, resize, text, or shader-effect capability behavior changes, run the Test Suite app on the Windows host from `../RayX` and verify DevKit/runtime evidence for the DX12 WGPU path.
   - When shader/frame/screen-effect behavior changes, add or reuse a Test Suite page in `../RayX/apps/test_suite` that exercises `effect_frame()`, `animated_frame(id)`, `transition_frame(id)`, and active screen effects on every affected target that can run locally.

## RayX Patch Surface To Preserve

The RayX memories identify these gpux-owned patches as especially easy to miss during imports. Treat this as a categorized audit list, not a substitute for reading the current diff and gpux history.

### Workspace And Provenance

- Preserve the reduced GPUI-closure workspace shape, the public package/import crate name `gpux`, and the `gpux-*` package names, even though upstream Zed paths still use `crates/gpui`.
- Preserve gpux-local metadata, local tooling, package docs, and upstream provenance markers; do not pull in unrelated Zed app crates or make the root workspace look like full Zed.
- Keep the workspace `wgpu` dependency on the `gpux-wgpu` fork (`wgpu = { package = "gpux-wgpu", path = "vendor/wgpu/wgpu", version = "=<fork version>" }`); never bring back Zed's wgpu git dependency or add a `[patch]` section.
- Preserve public examples and docs on `gpux::{...}` while keeping source provenance tied to the upstream Zed commit and local replay commits.

### Native Rendering And Windows

- Preserve `Window::render_to_image()` and backend renderer readback paths.
- Preserve the Windows `gpui_wgpu::WgpuRenderer` path, shared `GpuContext`, Win32 WGPU surface wrapper, `CosmicTextSystem::new("Segoe UI")`, renderer-reported GPU specs, and shader-effect capabilities.
- Preserve the Windows native backend policy as WGPU on DX12. Do not reintroduce the removed DirectX renderer/device/text stack or Vulkan/GL fallback for normal Windows rendering.
- Preserve Windows interactive redraw priority, mouse-move coalescing, and live-resize coalescing into scheduled frames. Pending resize frames must present and force a fresh GPUI draw so scene geometry and surface size come from the same resize sample.
- Preserve native WGPU waiting before `Surface::configure`; browser/WASM resize remains nonblocking.
- Preserve target-specific `WgpuRenderer::gpu_specs()` backend-label helpers so WASM does not read native-only backend policy fields.

### Browser And WASM

- Preserve multithreaded `gpui_web` as the default RayX browser path, including the `gpui_platform::application()` mapping, shared-memory build flags, COOP/COEP/CORP serving assumptions, and headed WebGPU validation route.
- Preserve automatic browser backend selection: WebGPU is attempted first and WebGL2 is the fallback. The WebGL transport must cover RayX's complete renderer ABI—including effects, HTML islands, paths, sprites, and every auxiliary instance family—through exact word-addressable `Rgba32Uint` textures; never import a narrower upstream transport that silently drops local scene fields.
- Keep the focused headed Test Suite backend proof at `../RayX/tools-node/test_suite/rayx-webgl-fallback.spec.ts`. It must force `?backend=webgl`, assert a visible nonzero canvas without page errors, and separately prove that automatic selection still prefers Browser WebGPU on a capable browser.
- Construct `FetchHttpClient` from the owning `WebPlatform` dispatcher and attach it while building the web `Application`; it is not a standalone `Default` client.
- Preserve `gpui_web` ownership of visible viewport bounds, `devicePixelRatio`-backed canvas/backing-store sizing, `ResizeObserver` resize coalescing, and pending physical-size redraw/present behavior.
- Preserve browser asset behavior: WASM `Asset::load` futures are local, `App::fetch_asset` foreground-spawns browser asset futures, and `Svg::external_path(...)` fetches browser-served SVG bytes.
- Preserve browser WebGPU renderer reentrancy and WGPU shader parity.
- Candidate, not confirmed current gpux behavior: browser WebGPU adapter retry with softer power-preference hints from RayX memory.

### HTML DOM Surfaces

- Preserve `html_island(...)` as the HTML-in-canvas texture path: stable ids, trusted app HTML, `DomOwned`/`GpuiOwned`, fallback content, scene primitives with scaled/logical bounds, content masks, opacity, revision, and live direct-canvas-child DOM roots.
- Preserve the no-scratch/no-clone/no-proxy architecture for HTML island upload. Dirty roots are gated by browser paint readiness and use gpux's own `GPUQueue.copyElementImageToTexture` binding in `crates/gpui_wgpu/src/html_island_upload.rs` (on the `wgpu::Queue::as_webgpu()` and `wgpu::Texture::as_webgpu()` handles) with the experimental Canary dictionary shape; it is not a wgpu patch.
- Preserve HTML island texture usage (`COPY_DST | TEXTURE_BINDING | RENDER_ATTACHMENT`), storage-buffer/WGSL layout assertions, root transforms, pruning, diagnostics, and `No cached paint record for element` retry-deferral behavior.
- Preserve `html_element(...)` as live browser DOM overlay metadata, not a WebGPU batch: body-level overlay root, stable ids, keep-alive parking, revision-gated `innerHTML`, geometry diffing, content-fit, wheel behavior, extra styles, fallback rendering, diagnostics, and overlay cleanup on drop.
- Preserve DOM-owned event bypass for `html_island` and `html_element` pointer, wheel, context-menu, drag, and drop events. Preserve `credentialless` guidance for third-party iframes in threaded WASM pages.

### Elements, Layout, And Input

- Preserve optional `id(...)` hooks on `Anchored`, `Deferred`, `ImageCacheElement`, and `Surface`.
- Preserve `Window`'s per-`PointerId` interaction-owner and captured-hitbox maps. After pointer interaction reconciliation, refresh the raw event's captured hitbox from the reconciled map before terminal Up/Cancel dispatch; do not retain a removed pre-reconciliation hitbox.
- Preserve `Style.direction`, `StyleRefinement.direction`, `Styled::direction(...)`, and Taffy direction mapping.
- Preserve `TextStyle.letter_spacing` through layout, wrapping, shaping, and platform text systems.
- Preserve `CursorStyle::None` across platform cursor conversion.
- Preserve multi-stop linear gradients, gradient-capable borders, Oklab shader parity, and bounded gradient stop lookup.
- Preserve `Deferred::unclipped()` while keeping default `deferred(...)` content-mask-preserving.
- Prepaint deferred draws in place so cached deferred ranges keep stable indices. Moving completed entries to the end invalidates reuse ranges even when the frame appears correct without caching.
- Preserve fixed/sticky as first-class `Position` variants, generated `.fixed()`/`.sticky()` APIs, Taffy mapping, scroll-container context, content-mask containing blocks, positioned scene layers, positioned hitboxes, top-layer scrollbars, and sticky state callbacks/refinements.
- Preserve nested scroll wheel routing that stops propagation only when the current scrollport's clamped offset changes.

### DevKit And Text Inspection

- Preserve `Window::devtools_frame_snapshot()` and the rendered occurrence id model separate from dispatch-node ids.
- Preserve paint-time DevKit semantics/property callback attachment on rendered occurrences, while retaining dispatch-node storage for property mutation callbacks.
- Preserve `Window::dispatch_text_input(...)` and `Window::text_input_value(...)` for literal text entry and textbox value diagnostics; named key presses still use key down/up dispatch.

### Shader, Screen, And Frame Effects

- Preserve framework-owned shader effects on `effect_frame()` and screen effects on `Window`, not broad `Styled`/`div()` effect APIs.
- Preserve the frame family as single-child elements: `frame()`, `effect_frame()`, `animated_frame(id)`, and `transition_frame(id)` with stable ids and retained-snapshot image-space replacement semantics.
- Preserve `ContentMask` bounds plus corner radii across scene primitives, renderer clip state, HTML islands/elements, screen-effect regions, and frame-family rounded `.clip()`.
- Preserve WGPU element effects and screen effects: full-resolution backdrop blur, separate `backdrop_blur(...)` versus Liquid Glass `glass(...)`, squircle SDF refraction, additive glow pass, retained previous-frame snapshot textures, uncapture-on-retry behavior, packed Rust/WGSL layout assertions, and Dawn-safe WGSL syntax.
- Preserve capability-driven support. Native non-WGPU backends must not fake shader effects or report supported element/screen/snapshot effects until their native shader paths render them.
- Preserve device-pixel effect lengths: `Window::paint_visual_effects` stores `effect.prepare().scale(scale_factor)`, so `PreparedElementEffect` blur/glass radii and glow radius/spread reach the renderer in device pixels like the scene bounds (unscaled radii halved every effect at DPR 2).
- Preserve `ElementEffect::LayerBlur` (`layer_blur(radius)`): it prepares to the backdrop blur pass, reports `paints_after_content()` and an `outset()` of 1.5 × radius, and `EffectFrame::paint` paints it after the child over the bounds and corner radii widened by that outset, so the frame's own content blurs and bleeds past its edges; pre-content effects keep painting before the child.

8. Update durable memory:
   - Append the validated import to `docs/history/zed-imports.tsv` and update the current represented commit in `docs/history/upstream-provenance.md`.
   - Update `../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/state.md` with the new imported upstream Zed commit and any changed replay rule.
   - Update `../RayX/.agentx/memory/knowledge/runtime/gpux-foundation/knowledge.md` only for durable facts that are not obvious from state.
   - Add a decision in RayX memory if the closure boundaries, import method, or provenance policy changes.
   - Run the AgentX audit script from `../RayX` after memory changes.

## Review Checklist

- No unrelated Zed app files were imported.
- Closure deletions were applied intentionally.
- Main framework Cargo package and Rust imports still use `gpux`, not upstream `gpui`.
- Every manifest keeps the gpux publishing shape, dependencies stay on crates.io, no file is included from outside its crate, no GPL/AGPL license file or field came back, and `cargo package --workspace` passes.
- Commit bodies preserve upstream Zed commit provenance.
- Root workspace metadata still reflects gpux, not full Zed.
- RayX AgentX upstream-sync memory (`../RayX/.agentx/memory/`) matches the new source hashes.
- The checked-in provenance ledger matches the new source hash, and the workflow has no predecessor-repository dependency.
- Validation results are recorded in the final response or PR notes.
