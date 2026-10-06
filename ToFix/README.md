# ToFix: open work from the 2026-10-06 macOS CI session

**These notes are planned as the AgentX epic `macos-only-metal-port-and-ci-repair`**
(`.agentx/tasks/macos-only-metal-port-and-ci-repair/`). Start implementation on the Mac with
`/agentx implement macos-only-metal-port-and-ci-repair`. The epic is the authority; these notes are
its source material.

These notes hand off unfinished work to an agent running on a **real macOS machine**
(Apple Silicon or Intel, Xcode installed, so `xcrun metal` works). Everything below was done from
Windows; anything that needs a Mac is still open. Read this index first, then the numbered files in
order.

| File | What | State |
|---|---|---|
| [01-macos-metal-renderer.md](01-macos-metal-renderer.md) | Metal renderer ported to the `PaintColor` contract (the original CI failure) | Done on branch `fix/metal-paint-contract`; real macOS now compiles the Metal shaders. Runtime proof and tests still need a Mac |
| [02-macos-zed-util-process-reaping.md](02-macos-zed-util-process-reaping.md) | `gpux-zed-util` macOS build fails: `adopt_raw_pid` only exists in Zed's git fork of `async-process` | **Next CI blocker.** Proposed fix written and type-checked; not committed; needs running on a Mac |
| [03-wgpu-dashed-borders.md](03-wgpu-dashed-borders.md) | WGPU dashed borders rendered solid (premultiplied color faded through alpha only) | Fixed and runtime-proven on branches; only merging is left (blocked by 01/02 for a green macOS job) |
| [04-flaky-windows-process-test.md](04-flaky-windows-process-test.md) | `gpux-zed-util` Windows test `test_drop_terminates_grandchildren` timed out once in CI | Probably a CI timing flake; decide whether to harden it |

## Current Git State

Nothing is merged. All pull requests were **closed (not merged)** at the user's request; the
branches are pushed and kept.

| Repository | Branch | Commits on top of base | Closed PR |
|---|---|---|---|
| Gpux (`rayx-ui/Gpux`, base `main` at `29702e4`) | `fix/metal-paint-contract` | `3ad5fca` refactor(apple): drop clone calls on Copy content masks; `5321a25` fix(apple): port the Metal renderer to the PaintColor paint contract | [#1](https://github.com/rayx-ui/Gpux/pull/1) |
| Gpux | `fix/wgpu-dashed-border-gaps` | `c6b6512` fix(wgpu): show the backdrop through dashed border gaps; `e7149e7` docs(memory): record that premultiplied paint fades every channel | [#2](https://github.com/rayx-ui/Gpux/pull/2) |
| RayX (`rayx-ui/RayX`, base `master` at `9f4d2a1d`) | `test/dashed-border-gaps` | `f1a9be7b` test(test-suite): prove dashed border gaps show what lies beneath | [RayX#1](https://github.com/rayx-ui/RayX/pull/1) |

The local Gpux checkout (`D:\Work\projects\RayX\Gpux`) was left on `fix/metal-paint-contract` with
the **uncommitted** proposal from file 02 reverted; its full source is in file 02. The local RayX
checkout is on `test/dashed-border-gaps`. RayX builds against whatever branch the sibling `../Gpux`
checkout has, so the dashed-border spec fails if Gpux is on `main`.

## Recommended Order On The Mac

1. Check out Gpux `fix/metal-paint-contract`.
2. Apply the `adopt_raw_pid` replacement from file 02 (or a better one), then run
   `cargo check --workspace --all-targets`. That is exactly CI's failing step. Fix whatever it
   reports next. The Windows cross-checks could not reach the crates that need a C toolchain, so
   more macOS-only compile errors may still be hiding.
3. Run the macOS tests listed in files 01 and 02, then the runtime proof in file 01.
4. Commit (Conventional Commits, no Claude co-author lines), push, and reopen or recreate the PR.
   When it is green, merge it, then merge `main` into `fix/wgpu-dashed-border-gaps`. That should turn
   its macOS job green too. Then merge that branch and RayX `test/dashed-border-gaps` together.
5. Do the memory and import-guide updates listed in file 01.

## Repository Rules That Matter Here

- Gpux `CLAUDE.md` and `.claude/skills/gpux/SKILL.md`. **Dependencies come from crates.io only**: no
  git dependencies and no `[patch]` sections, because consumers of the published `gpux-*` crates
  cannot see them. This rule is the root cause of file 02.
- Test only what you changed. Focused checks are `cargo fmt --check`,
  `cargo check -p <pkg> --all-targets`, `cargo test -p <pkg> <filter>`, and
  `cargo clippy -p <pkg> --all-targets --no-deps -- -D warnings`. Run `cargo package --workspace`
  after manifest changes.
- Runtime proof for rendering changes happens in RayX's Test Suite (`../RayX`, `apps/test_suite`,
  never `apps/lab`). On macOS that means a native run with DevKit/MCP; see RayX
  `.claude/skills/rayx/references/diagnostics.md`.
- Raw logs go to ignored `artifacts-temp/`, never into `.agentx/` or commits.
- CI (`.github/workflows/ci.yml`): `check` runs on Ubuntu, Windows and macOS
  (`cargo check --workspace --all-targets`); clippy and tests run on Windows only; `package` runs on
  Ubuntu. Workflows run on pull requests and pushes to `main` only.
