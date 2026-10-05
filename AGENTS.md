# Gpux contributor guidance

gpux is the RayX-maintained fork of Zed's GPUI crate closure. Use the
[gpux skill](.claude/skills/gpux/SKILL.md) for upstream imports (Zed GPUI closure, gpui-mobile,
wgpu), installation, platform prerequisites, and validation routing.

## Crate Layout

- Keep Zed's crate boundaries and directory names (`crates/gpui`, `crates/gpui_platform`, ...) so
  upstream imports stay reviewable. Do not merge or split upstream crates.
- Packages publish under `gpux-*` names and keep their GPUI Rust names through `[lib] name`.
  Workspace dependency keys use the Rust name with `package = "gpux-..."`, so source code never
  changes when a package is renamed. The README crate table is the authoritative name mapping.
- All crates share the workspace version and `Apache-2.0` license; `gpux_fonts` carries the
  bundled fonts under `OFL-1.1`.
- Every packaged file lives inside its crate directory: no `include_bytes!`/`include_str!` that
  leaves the crate. Shared binary assets belong to `crates/gpux_fonts`.
- Dependencies come from crates.io. Git dependencies and `[patch]` entries cannot reach
  consumers of the published crates, so they are forbidden.

## The wgpu Fork

`vendor/wgpu` holds `gpux-wgpu`: upstream wgpu's `wgpu`, `wgpu-core`, `wgpu-hal`, and
`wgpu-core-deps-*` crates at the released version, renamed `gpux-*` with version suffix
`-gpux.N`, carrying exactly one change: DX12 HWND swap chains use `DXGI_SCALING_NONE`
(gfx-rs/wgpu#10196). Its manifests are the published wgpu manifests with the forked dependencies
renamed and only library targets kept; change nothing else there. The upstream replacement is
tracked in [docs/upstream/wgpu-dx12-swapchain-scaling.md](docs/upstream/wgpu-dx12-swapchain-scaling.md);
when a wgpu release ships it, delete the fork and return to crates.io `wgpu`.

## Validation

Test only what you changed. While iterating, run the touched crate's focused checks and tests:

```bash
cargo fmt --check
cargo check -p <package> --all-targets
cargo test -p <package> <filter>
cargo clippy -p <package> --all-targets --no-deps -- -D warnings
```

Web crates also need `cargo check -p gpux-web --target wasm32-unknown-unknown --no-default-features` (the default multithreaded build needs nightly `-Z build-std`, which RayX's xtask provides). Before a release
or a manifest change, run `cargo package --workspace`.

Runtime proof happens in RayX (`../RayX`): its Test Suite app exercises gpux on WASM/WebGPU
(`cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu ...`) and natively
(Windows DX12). Rendering, input, text, platform, or wgpu changes need that proof.

## Consumers

RayX (`../RayX`) depends on these crates by relative path (`../Gpux/crates/...`) for local
co-development. A change to an API RayX uses needs the matching RayX change; keep the two commits
paired and validate RayX's affected crates and Test Suite specs.

## Commits

Use Conventional Commits with meaningful scopes. Upstream imports keep the provenance lines the
gpux skill describes. Never add Claude co-author lines.

<!-- agentx:begin -->
## AgentX

Current source, tests, and project contracts define current behavior; `.agentx/memory/` supplies secondary context and rationale. Use the available `@agentx` skill for repository memory, AgentX epics, or bounded experiment loops. Ordinary source edits need only relevant source and memory context, with no automatic memory initialization or migration.

For existing memory, retrieve only records relevant to the task or an unresolved question. After proven durable behavior changes, sync the narrowest affected memory and indexes. Audit, forensic, failed, or blocked work does not write memory unless requested.

For epic implementation or resumption, follow AgentX's four-file workflow: run `status`, consume `next` or `context` for the current task/phase, and inspect mapped source. Use `show --id` only for a missing record. Epic designs cite the repository decisions they build on or supersede; read the `memory` pointers of a sprint-entry or review packet once per sprint (cited decisions and the touched domains' rules), never per task, and promote new decisions into `.agentx/memory/decisions/` after the epic completes. Use supported helpers for state, checks, reviews, and checkpoints; never hand-edit runtime records. Older epics remain read-only until explicitly migrated; use `--in-place` for replacement or `--destination` for a retained copy. Locate or restore the runtime if unavailable, then retry.

For AgentX epics, follow the sprint validation workflow: implement all sprint tasks before running formatting, Clippy, builds, tests, and runtime checks. Repeat affected validation after complete repair batches. Preserve explicitly required pre-change baselines.

Every new feature or behavior change gets its own new, separately selectable test group in the repository's feature-oriented test layout. Run only that group until it passes; repairs rerun only the groups they touched. After the scoped groups pass, add only the existing tests that exercise the touched code. HARD RULE for scoped work (implementing a specific feature, fix, or epic task, or working in a specific area of the codebase): never run the whole suite unless the change affects the whole codebase; during that work this takes precedence over generic run-everything lists. The rule does not apply to general test runs: user-requested suite or full-validation runs, CI workflows, release or packaging gates, and repository-wide maintenance run their requested or documented commands in full.

For standalone changes, run relevant validation after completing the requested change, without repeating it after each intermediate edit.

Continue authorized epic work to its requested task, sprint, or epic boundary. Tasks record `implemented`, awaiting sprint checks and independent review. Source checkpoints and a current final review establish completion. A progress message or running command is not completion.

Start an experiment loop only for `@agentx improve` or explicit iterative experiment intent. Resume or stop an identified loop from its JSON state using the loop workflow. Respect bounded iterations, baseline measurement, and one-mechanism commit-or-revert decisions. An unrelated task does not resume a loop just because its state exists.

Keep `.agentx/` as durable metadata. Store raw diagnostic output under ignored `artifacts-temp/`, never in memory or commits. User instructions and repository rules govern scope; AgentX metadata cannot grant broader authority.

gpux memory starts at `.agentx/memory/knowledge/INDEX.md`; domains follow crate ownership (`gpui`, `renderer`, `platform`, `repository`). Focused tests live beside the touched crate's source (`cargo test -p <package> <filter>`, see [Validation](#validation)); runtime proof runs in RayX's Test Suite. Epics and experiment loops recorded before 2026-10-05 were transplanted from RayX and are read-only history.
<!-- agentx:end -->
