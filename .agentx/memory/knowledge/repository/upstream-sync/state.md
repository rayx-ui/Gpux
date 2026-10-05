---
agentx:
  kind: state
  scope: knowledge/repository/upstream-sync
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - upstream-sync
    - zed
    - gpui-mobile
    - provenance
    - patch-surface
  supersedes: []
  superseded_by: []
---
# Upstream Sync State

## Purpose And Usage

Read before any upstream import. The step-by-step workflows are the gpux skill references; this file records where provenance lives and what the current base is.

## Current Technical Shape

`docs/history/` is the complete local source of truth: `upstream-provenance.md` names the represented Zed commit (`c24358d96cdb4ce14ecbc088462295353b0103f0` as of the last validated import), `zed-imports.tsv` records every recovered closure import, and the commit maps connect predecessor hashes to the filtered gpux-only archive. The annotated tag `gpux-pre-migration-2026-06-17` (and RayX's `archive/gpux-pre-migration` branch) preserves 72 gpux-only commits through `acdeadd279bc67334cb625e233b85c264391d8bc`. RayX history from `9aff311f` carried every later change under `crates/gpux` until the 2026-10-05 `git filter-repo --subdirectory-filter crates/gpux` extraction; Gpux root commit `e2f8568` corresponds to RayX `9aff311f`.

Imports are semantic three-way reconciliations from the prior represented commit, so upstream changes and gpux-local patches stay independently auditable. The skill's [Zed closure guide](../../../../../.claude/skills/gpux/references/update-from-zed-gpui-closure.md) and [gpui-mobile guide](../../../../../.claude/skills/gpux/references/update-from-gpui-mobile.md) carry the accumulated patch-surface audit lists (browser/WASM DOM, DevKit semantics, fixed/sticky, frame and shader effects, Windows WGPU/DX12, mobile JNI/lifecycle/backend policy, iOS Metal/text). Single Zed PRs use the [Zed PR guide](../../../../../.claude/skills/gpux/references/update-from-zed-pr.md); wgpu releases use the [wgpu guide](../../../../../.claude/skills/gpux/references/update-from-wgpu.md) with `scripts/fork_wgpu.py`.

The 2026-08-07 import kept the gpux crate identity, renderer and platform patches, DevKit and diagnostics surfaces, DOM/HTML islands, effects, gestures, layered keymaps, async startup, and the multi-pointer drag model; follow-up reconciliation completed native AccessKit adapters, Wayland `xdg_popup`, platform emoji recognition, and the full-ABI WebGL transport. It left Wayland `client.rs`/`window.rs` and X11 `window.rs` partially imported until their 2026-10-05 re-import.

## Backlinks

- [Upstream provenance](../../../../../docs/history/upstream-provenance.md)
- [Zed import ledger](../../../../../docs/history/zed-imports.tsv)
- [gpux skill](../../../../../.claude/skills/gpux/SKILL.md)
- 2026-08-07 current [Self-contained upstream provenance](../../../decisions/repository/upstream-sync/2026-08-07-repository-upstream-sync-self-contained-upstream-provenance.md)
