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
`wgpu-core/platform-deps/*` at the released version, renamed `gpux-*` with version suffix
`-gpux.N`, carrying exactly one change: DX12 HWND swap chains use `DXGI_SCALING_NONE`
(gfx-rs/wgpu#10196). Change nothing else there. Delete the fork and return to crates.io `wgpu`
once upstream ships the fix.

## Validation

Test only what you changed. While iterating, run the touched crate's focused checks and tests:

```bash
cargo fmt --check
cargo check -p <package> --all-targets
cargo test -p <package> <filter>
cargo clippy -p <package> --all-targets -- -D warnings
```

Web crates also need `cargo check -p gpux-web --target wasm32-unknown-unknown`. Before a release
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
