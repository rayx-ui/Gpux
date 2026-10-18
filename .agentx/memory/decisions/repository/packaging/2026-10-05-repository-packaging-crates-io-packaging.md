---
agentx:
  kind: decision
  scope: decisions/repository/packaging
  status: superseded
  created: 2026-10-05T18:00:00+02:00
  updated: 2026-10-19T01:30:00+02:00
  topics:
    - gpux
    - crates-io
    - publishing
    - dependencies
    - sibling-repositories
  supersedes:
    - 2026-08-30-repository-packaging-upstream-dependency-sources.md
  superseded_by:
    - 2026-10-18-repository-packaging-git-tag-releases.md
---

## Decision: gpux publishes to crates.io from its sibling repository as `gpux-*` packages with GPUI Rust names

### Context: gpux moved from the nested `crates/gpux` workspace into the sibling `../Gpux` repository (history kept with `git filter-repo`) so it can be published to crates.io. Its crates had generic or taken names (`util`, `collections`, `gpui_mobile`), git dependencies and `[patch]` entries, files included from outside crate directories (fonts), dangling license pointers, and a triple-licensed mobile crate.

### Alternatives considered: Publish under the existing names; rename the Rust crates and rewrite imports; merge the GPUI closure crates into fewer crates; rename only the packages and keep the Rust names through `[lib] name`.

### Reasoning: Package names `gpux-*` (the README crate table is the mapping, e.g. `gpux-platform`, `gpux-wgpu-renderer`, `gpux-zed-util`) are free on crates.io, while `[lib] name` keeps every GPUI Rust name, so neither gpux nor RayX source changes and upstream Zed imports stay diffable; workspace dependency keys keep the Rust name with `package = "gpux-..."`. Keeping Zed's crate boundaries preserves the upstream-import workflow. Published crates cannot carry git dependencies or `[patch]`, so dependencies come from crates.io only (Zed's published `zed-*` forks, proptest 1.11, async-task 4.7.1, calloop 0.14.5); an essential patch ships as a published fork crate. Bundled fonts live in `gpux_fonts` (OFL-1.1) because packaged files must sit inside their crate; every other crate is Apache-2.0 with a real license copy, and gpui_mobile is distributed under its Apache-2.0 option. Source guards that RayX tests used to run against gpux files now live in Gpux (or became type-level checks), and RayX tooling resolves the gpux checkout from the root manifest.

### Trade-offs accepted: The crates.io calloop release lacks Zed's fork change that bounds idle callbacks to about 10 ms per loop iteration; gpui_linux reproduces it with its own budgeted foreground task queue (`ForegroundTasks`), so no calloop fork is needed. Dev-dependencies on publish cycles (gpui_macros -> gpux, gpux -> gpui_platform/gpui_web) are path-only, so the published crates' own tests do not build from the registry tarball. Upstream imports must re-apply the publishing manifest shape. Publishing and the GitHub repository remain manual steps.

### Supersedes: [Prefer upstream dependency sources over full vendoring](2026-08-30-repository-packaging-upstream-dependency-sources.md): git forks and full vendoring are no longer options for published gpux crates.
