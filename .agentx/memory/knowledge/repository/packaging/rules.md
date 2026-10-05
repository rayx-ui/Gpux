---
agentx:
  kind: rules
  scope: knowledge/repository/packaging
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - crates-io
    - packaging
    - dependencies
    - wgpu-fork
    - licenses
  supersedes: []
  superseded_by: []
---
# Packaging Rules

- Take every dependency from crates.io; git dependencies and `[patch]` entries cannot reach consumers of the published crates. Ship an essential unreleased patch as a published fork crate and record its provenance in the fork manifest and `docs/upstream/`.
- Prefer upstreaming patches and returning to crates.io releases over maintaining forks or vendored trees.
- Keep every crate publishable: `gpux-*` package name with the GPUI Rust name in `[lib] name`, workspace metadata, a `description`, no `publish = false`, Rust-name workspace dependency keys with `package = "gpux-..."`, path-only cycle-edge dev-dependencies, and no file included from outside its crate.
- Keep gpux Apache-2.0 (`gpux-fonts` OFL-1.1, the wgpu fork `MIT OR Apache-2.0`); never re-import GPL/AGPL license files or fields from Zed or gpui-mobile.
- Keep the `gpux-wgpu` fork to its single DX12 scaling change and change nothing else in its manifests; recreate it per wgpu release with `scripts/fork_wgpu.py`, and delete it once a wgpu release ships `SurfaceConfiguration::scaling_mode`.
- Run `cargo package --workspace` for manifest, dependency, or included-file changes and before a release.
