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

History mapping files:

- [gpux-commit-map.tsv](gpux-commit-map.tsv) maps the original standalone gpux hashes to the filtered hashes used by the predecessor repository.
- [post-embed-filter-map.tsv](post-embed-filter-map.tsv) maps the final predecessor commits to their gpux-only archive commits.

## Update contract

1. Read this file, `zed-imports.tsv`, the archived gpux history, this repository's history, and this repository's AgentX memory for the patch surfaces the import touches (start at `.agentx/memory/knowledge/repository/upstream-sync/`).
2. Treat the latest `zed_commit` in `zed-imports.tsv` as the three-way merge base.
3. Fetch the chosen target from `zed-industries/zed` and import only the mapped GPUI closure.
4. Preserve or replay archived and current-repository gpux patches, including the reduced workspace and `gpux` crate rename.
5. After validation, append the new import to `zed-imports.tsv`, update the represented commit above and AgentX state, and record import/replay provenance in commit bodies.

The archive branch and tag are Git refs rather than files on the active branch. Push both refs when publishing this repository so fresh clones receive the forensic graph.
