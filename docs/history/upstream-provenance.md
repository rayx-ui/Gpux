# gpux upstream provenance

This directory and the repository's archived Git refs are the complete local source of truth for gpux upstream updates. Future updates must not depend on the retired `RayX.NET/RayX` checkout or another RayX predecessor repository.

## Current represented upstream

The latest Zed GPUI closure represented is:

```text
c24358d96cdb4ce14ecbc088462295353b0103f0
```

Use `https://github.com/zed-industries/zed` as the authoritative upstream. [zed-imports.tsv](zed-imports.tsv) records every recovered closure import point and its original fork-merge provenance.

## Preserved local patch history

The filtered branch `archive/gpux-pre-migration` and annotated tag `gpux-pre-migration-2026-06-17` preserve 72 gpux-only commits through `acdeadd279bc67334cb625e233b85c264391d8bc`. The archived tree is rooted directly at the gpux workspace and contains no managed RayX source or unrelated predecessor history.

Inspect it with:

```text
git log --reverse --format="%H %s%n%b" archive/gpux-pre-migration
git -C <zed-cache> diff 481854f7541636c63f79abec9785697ed74f4005..<target-zed-commit> -- <closure-paths>
git show archive/gpux-pre-migration:crates/gpui/src/window.rs
```

The current repository history from `9aff311fa2692fd4d390839c8feb5b5ccebc7b20` onward preserves every later gpux/RayX change under `crates/gpux`.

The 2026-08-07 import preserved RayX's `gpux` crate identity, vendored WGPU 29
patches, Windows DX12 backend, mobile runtime, DevKit and diagnostics surfaces,
DOM/HTML islands, effects, gestures, layered keymaps, async startup, and
multi-pointer drag model. Follow-up reconciliation completed the imported native
AccessKit adapters and Wayland `xdg_popup` lifecycle, restored platform emoji
font recognition, and adapted Zed's WebGL transport to RayX's complete extended
renderer ABI. Browser startup now prefers WebGPU and falls back to WebGL2; the
WebGL path transports every packed scene record through exact `Rgba32Uint`
textures rather than silently omitting RayX effects or HTML-island data.

History mapping files:

- [gpux-commit-map.tsv](gpux-commit-map.tsv) maps the original standalone gpux hashes to the filtered hashes used by the predecessor repository.
- [post-embed-filter-map.tsv](post-embed-filter-map.tsv) maps the final predecessor commits to their gpux-only archive commits.

## Update contract

1. Read this file, `zed-imports.tsv`, the archived gpux history, current `crates/gpux` history, and the AgentX `runtime/gpux-foundation` patch-surface guidance.
2. Treat the latest `zed_commit` in `zed-imports.tsv` as the three-way merge base.
3. Fetch the chosen target from `zed-industries/zed` and import only the mapped GPUI closure.
4. Preserve or replay archived and current-repository gpux patches, including the reduced workspace and `gpux` crate rename.
5. After validation, append the new import to `zed-imports.tsv`, update the represented commit above and AgentX state, and record import/replay provenance in commit bodies.

The archive branch and tag are Git refs rather than files on the active branch. Push both refs when publishing this repository so fresh clones receive the forensic graph.
