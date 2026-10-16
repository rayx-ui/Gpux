# gpux upstream provenance

This directory and the repository's archived Git refs are the complete local source of truth for gpux upstream updates. Future updates must not depend on the retired `RayX.NET/RayX` checkout or another RayX predecessor repository.

## Current represented upstream

The latest Zed GPUI closure represented is:

```text
d544470d1d3770f0024651dbf121525564a2657c
```

Use `https://github.com/zed-industries/zed` as the authoritative upstream. [zed-imports.tsv](zed-imports.tsv) records every recovered closure import point and its original fork-merge provenance.

## Preserved local patch history

The annotated tag `gpux-pre-migration-2026-06-17` (also present in RayX, where the filtered branch `archive/gpux-pre-migration` carried the same history) preserves 72 gpux-only commits through `acdeadd279bc67334cb625e233b85c264391d8bc`. The archived tree is rooted directly at the gpux workspace and contains no managed RayX source or unrelated predecessor history.

Inspect it with:

```text
git log --reverse --format="%H %s%n%b" gpux-pre-migration-2026-06-17
git -C <zed-cache> diff 481854f7541636c63f79abec9785697ed74f4005..<target-zed-commit> -- <closure-paths>
git show gpux-pre-migration-2026-06-17:crates/gpui/src/window.rs
```

RayX history from `9aff311fa2692fd4d390839c8feb5b5ccebc7b20` onward carried every later gpux change under `crates/gpux`. On 2026-10-05 that history was extracted into this repository with `git filter-repo --subdirectory-filter crates/gpux`; its root commit `e2f8568cf7b1bae4a09dbd3d8035d9d515493c20` corresponds to RayX `9aff311f`, and commit hashes recorded elsewhere in this folder refer to RayX history.

The 2026-08-07 import preserved RayX's `gpux` crate identity, vendored WGPU 29
patches, Windows DX12 backend, mobile runtime, DevKit and diagnostics surfaces,
DOM/HTML islands, effects, gestures, layered keymaps, async startup, and
multi-pointer drag model. Follow-up reconciliation completed the imported native
AccessKit adapters and Wayland `xdg_popup` lifecycle, restored platform emoji
font recognition, and adapted Zed's WebGL transport to RayX's complete extended
renderer ABI. Browser startup now prefers WebGPU and falls back to WebGL2; the
WebGL path transports every packed scene record through exact `Rgba32Uint`
textures rather than silently omitting RayX effects or HTML-island data.

That import left `crates/gpui_linux/src/linux/wayland/client.rs` and `window.rs`
at the previous closure state, and the follow-up hand-ported popups onto them, so
the Wayland backend no longer compiled. On 2026-10-05 both files were re-imported
from `c24358d96cdb4ce14ecbc088462295353b0103f0` by three-way merge from
`481854f7541636c63f79abec9785697ed74f4005`, with the gpux Linux patches replayed.
`x11/window.rs`, which had the same partial import but still compiled, was
re-imported the same way.

The 2026-10-05 import of `d544470d1d3770f0024651dbf121525564a2657c` (Zed
"Reduce the dependency footprint (#65176)") was a three-way merge from
`c24358d96cdb4ce14ecbc088462295353b0103f0`. It adopted Zed's input model
unchanged (mouse events, raw touches through `TouchGestureRecognizer`, one active
drag with platform drag-out handoff) and deleted the gpux pointer layer:
`PointerEvent` and per-pointer ownership, the gesture map and arena, touch
momentum in `ScrollActivity`, rotate events, and the multi-drag extras. It
imported `gpux-bench-metrics` and `gpui_apple`, dropped `media`, and kept
gpux's cached thermal state by requesting Zed's follow-up frame from frame
demand without re-entering the app on idle frames.

The 2026-10-05 import replaced Zed's wgpu headless renderer (#64718 "gpui_wgpu: Add
Linux headless rendering", merge `e91b82c106817f2419207ebf81f1da766698ac95`, and the
shared headless device from #64842 "gpui: Add randomized element tree benchmarks and
fix Linux benchmark leaks", merge `3135cdf8dc8e6800d2b7ad89d62ea4994d892fb6`) with an
`Ok(None)` stub, because gpux's renderer had diverged (element and screen effects,
gradient storage, HTML islands, the encoded-sRGB paint contract, DX12 on Windows). On
2026-10-07 the gpux-cross-platform-test-lanes epic ported both onto gpux's renderer.
The adaptation keeps one renderer type with an optional surface instead of Zed's
`WgpuRendererCore` split, renders opaque non-sRGB targets with 4x path sampling, adds
the `GPUX_WGPU_ADAPTER` adapter override, and extends the port to Windows (DX12, WARP
without a GPU). `zed-imports.tsv` records the port with `port:` rows; they leave the
represented upstream commit and the merge base unchanged.

On 2026-10-16 the gpux-a11y-devtools epic moved gpux to the AccessKit 0.25 family
(`accesskit` 0.25.1, `accesskit_consumer` 0.39.1, `accesskit_macos` 0.27.1,
`accesskit_windows` 0.35.1, `accesskit_unix` 0.24.0), ahead of Zed's 0.24 pins. It is a
gpux-local delta: an import keeps gpux's pins unless Zed's are newer
([accesskit-0.25](../upstream/accesskit-0.25.md)).

Other upstreams gpux adapts code from, each with its own page:

- [AccessKit 0.25](../upstream/accesskit-0.25.md): the pins gpux keeps ahead of Zed.
- [gpui-fast](../upstream/gpui-fast.md) (longbridge/gpui-fast, v0.1.6 `b8f0c02`): fast mode, adapted file by file into `fast/` modules.

History mapping files:

- [gpux-commit-map.tsv](gpux-commit-map.tsv) maps the original standalone gpux hashes to the filtered hashes used by the predecessor repository.
- [post-embed-filter-map.tsv](post-embed-filter-map.tsv) maps the final predecessor commits to their gpux-only archive commits.

## Update contract

1. Read this file, `zed-imports.tsv`, the archived gpux history, this repository's history, and this repository's AgentX memory for the patch surfaces the import touches (start at `.agentx/memory/knowledge/repository/upstream-sync/`).
2. Treat the latest closure-import `zed_commit` in `zed-imports.tsv` as the three-way merge base. Rows whose subject starts with `port:` record single upstream pull requests ported onto gpux after an import; they never move the merge base.
3. Fetch the chosen target from `zed-industries/zed` and import only the mapped GPUI closure.
4. Preserve or replay archived and current-repository gpux patches, including the reduced workspace and `gpux` crate rename.
5. After validation, append the new import to `zed-imports.tsv`, update the represented commit above and AgentX state, and record import/replay provenance in commit bodies.

The archive branch and tag are Git refs rather than files on the active branch. Push both refs when publishing this repository so fresh clones receive the forensic graph.
