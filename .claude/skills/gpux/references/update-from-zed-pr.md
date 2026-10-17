# Update From Zed Pull Requests

Use this reference when the user gives a `zed-industries/zed` pull request URL or PR number and asks to bring that GPUI work into gpux. Run from the Gpux repository root.

This is a PR-specific entrypoint. After reading it, use [update-from-zed-gpui-closure.md](update-from-zed-gpui-closure.md) for the full closure path mapping, provenance format, replay workflow, and validation checklist.

## Core Workflow

Treat the PR as an upstream GPUI-closure import, not as an unfiltered patch copy:

1. Fetch the PR branch or exact commit SHA into a temporary Zed checkout.
2. Inspect which files changed and keep only the GPUI-related crate closure that gpux carries (Zed `crates/<name>/` maps to the same path here).
3. Port or cherry-pick those changes into the gpux topology, preserving local crate naming, the `gpux-*` manifest publishing shape, and RayX-specific patches.
4. Resolve conflicts against existing fork patches, especially renderer/readback, layout direction, `CursorStyle::None`, element identity hooks, gradient rendering, mobile/platform changes, and related patch surfaces.
5. Record upstream provenance so future imports know where the change came from.
6. Validate gpux here, including the `gpux-testkit` lanes that exercise the change, and pair any API change RayX uses with the matching RayX change.

## Source Selection

- Prefer an exact PR head commit SHA when the user provides one.
- If the user only provides a PR URL or PR number, fetch the PR head, resolve it to an exact commit SHA, and use that resolved SHA in provenance.
- If the PR is open, draft, recently force-pushed, or review feedback is active, call out that the imported source may change upstream later.
- If the PR has merged, prefer the merge commit or final squashed commit on upstream `main` when that preserves the intended GPUI state better than the old PR head.
- Do not require a permanent local Zed fork. Use a temporary/cache checkout from `https://github.com/zed-industries/zed`.

## Closure Filtering

- Inspect the PR's changed paths before importing anything.
- Import only files that belong to the current GPUI closure listed in the closure update reference.
- Include platform/rendering sibling crates, helper crates, shader files, examples, and assets when they are part of the GPUI feature.
- Exclude unrelated Zed app/editor files, product UI changes, workspace metadata that only applies to the full Zed repository, and non-GPUI feature code.
- Apply the closure reference's Manifest And Packaging Shape to every imported manifest and include: `gpux-*` package names, crates.io-only dependencies, `gpux_fonts` constants instead of out-of-crate `include_bytes!`, and Apache-2.0 licensing.
- If a PR mixes GPUI framework changes with app-layer Zed usage, port the framework capability and leave Zed app call sites out unless they are useful examples that can live cleanly under gpux.

## Conflict Surfaces To Audit

Always check whether the PR touches or overwrites these RayX-owned gpux patch surfaces:

- `Window::render_to_image()` and backend renderer readback.
- Layout direction plumbing on `Style`, `StyleRefinement`, `Styled`, and Taffy conversion.
- `CursorStyle::None`.
- Concrete element identity hooks and pointer-capture support.
- Gradient backgrounds, gradient-capable borders, Oklab shader parity, and bounded gradient stop lookup.
- Deferred draw content-mask propagation.
- Windows input responsiveness, redraw priority, and mouse-move coalescing.
- Web renderer reentrancy and browser/WGPU behavior.
- Android/iOS `gpui_mobile` lifecycle, surface, frame-pump, touch, and platform-view changes.
- macOS text-system feature selection, Metal shaders, and platform readback imports.

## Provenance

- Record the PR URL or number, resolved upstream commit SHA, and original subject in the import commit body.
- When importing from an unmerged PR head, include a note that the source was a PR head rather than upstream `main`.
- Keep replayed or preserved RayX/gpux patches as separate commits when practical, with `Preserved gpux patch commit:` trailers as described in the closure update reference.
- Update gpux upstream-sync memory (`.agentx/memory/knowledge/repository/upstream-sync/`) if the latest imported upstream source, patch replay rule, or closure boundary changes.
- A PR that the represented closure import already contains but gpux stubbed or replaced (for example Zed #64718/#64842, the wgpu headless renderer) is a port, not an import. Commit it with `Zed PR:`, `Zed merge commit:` and `Zed subject:` lines plus its gpux adaptations, then append one `port:` row per PR to `docs/history/zed-imports.tsv` (gpux commit, date, Zed merge commit, `-`, `port: <subject>`) and explain it in `upstream-provenance.md`. `port:` rows never move the represented upstream commit or the merge base: the base is the latest row that is not a `port:` row.

## Validation

At minimum, after a PR import:

- Run `cargo metadata --no-deps`.
- Run `cargo fmt --check`.
- Run `cargo check -p gpux --all-targets`.
- Add targeted package checks (`-p gpux-<...>`, see the README crate table) for every touched backend/support crate.
- Run `cargo package --workspace` when the PR changes manifests, dependencies, or included files.
- Run the lanes that exercise the change ([docs/testing.md](../../../../docs/testing.md)): `cargo run -p gpux-testkit -- lane a [--scene <id>] [--grep <pattern>]` plus the touched crate's `cargo test -p <package> <filter>` for behaviour; `lane b --grep <pattern>` for browser/WASM/WebGPU behavior; `lane c` for pixels and the renderer; `lane d` for real windows and OS input; `lane e` for Android.
- If the PR changes an API RayX uses, ship the matching RayX change and run RayX's affected checks from `../RayX`; they prove the consumer, not gpux.

If the PR changes browser, WebGPU, mobile, capture, input, layout, or renderer behavior, add the matching gpux coverage before calling the import complete: a conformance scene in `crates/gpux_tests_conformance/src/scenes/` (it names its lanes), a Playwright spec in `tools/browser/specs/`, or a focused test in the crate's `tests/`.
