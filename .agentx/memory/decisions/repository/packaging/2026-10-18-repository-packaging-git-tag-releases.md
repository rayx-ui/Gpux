---
agentx:
  kind: decision
  scope: decisions/repository/packaging
  status: current
  created: 2026-10-19T01:30:00+02:00
  updated: 2026-10-19T01:30:00+02:00
  promoted_from: gpux-releases
  topics:
    - gpux
    - releases
    - release-plz
    - git-tags
    - packaging
    - versioning
  supersedes:
    - 2026-10-05-repository-packaging-crates-io-packaging.md
  superseded_by: []
---

## Decision: gpux is released as one workspace version under one `vX.Y.Z` git tag and consumed by tag, while every crate stays crates.io-ready

### Context: RayX depended on gpux by relative path (`../Gpux/crates/...`), and the 2026-10-05 packaging decision planned crates.io publishing. On 2026-10-18 the owner chose git tags over a registry for gpux and the other RayX libraries, with release-plz cutting releases from Conventional Commits, a clean 1.0.0 start, and 1.x only while RayX is the only consumer.

### Alternatives considered: crates.io publishing (the previous plan); per-crate versions and tags with release-plz's defaults; cargo-release + git-cliff (recorded fallback); a local-override mechanism (`[patch]` file, `rayx dev link`) for simultaneous gpux + RayX edits.

### Reasoning: One version for the whole workspace keeps Zed's many crates moving together and gives consumers one tag (`{ git = "https://github.com/rayx-ui/Gpux", tag = "vX.Y.Z" }`; the `Cargo.lock` records the commit and the "Release tags" ruleset keeps tags from moving). release-plz in git-only mode does it with one version group where only `gpux` tags, creates the GitHub Release and writes the root `CHANGELOG.md`, and the forks are `release = false`; this relies on undocumented behaviour, so it was proven in a scratch clone with a local bare remote (release-plz 0.3.172, pinned in `release-plz.yml`; [docs/releasing.md](../../../../../docs/releasing.md#how-the-configuration-was-proven)). Under the 1.x policy no commit carries `!` or a `BREAKING CHANGE:` footer; breaking work and Zed imports are `feat` with the break in the body and RayX notes in a `Migration:` footer, and `semver_check = false`. A release pull request (content: only own version values plus `CHANGELOG.md`) runs only `check` and `package`. RayX consumes gpux only by tag: a change spanning both releases gpux first, then RayX bumps its tag by hand; there is no local override. gpux declares what rayx needs (`[package.metadata.rayx]`, `[workspace.metadata.rayx]`, `rayx.assets.toml`) and never depends on rayx.

The packaging rules of the superseded decision stay: `gpux-*` package names with the GPUI Rust names in `[lib] name` (the README crate table is the mapping), Rust-name workspace keys with `package = "gpux-..."`, workspace metadata and a `description` everywhere, no `publish = false`, path-only cycle-edge dev-dependencies, crates.io-only dependencies (no git dependencies, no `[patch]`), every packaged file inside its crate (fonts in `gpux_fonts`), Apache-2.0 (`gpux-fonts` OFL-1.1), and `cargo package --workspace` after manifest changes and before a release. Publishing to crates.io stays a later option.

### Trade-offs accepted: Commits go straight to `main` without pull-request CI; the nightly covers them and a release pull request is merged only after a green nightly. A shared fine-grained PAT (`RELEASE_PLZ_TOKEN`) must include every library repository and expires silently. A new crate must join `release-plz.toml` by hand (`release_config.rs` fails until it does). Building apps on gpux 1.0.0 needs rayx 1.4.0 or later. v1.0.0 was released on 2026-10-18 (Release-plz run 39174990232, tag at 045ca98).

### Supersedes: [gpux publishes to crates.io](2026-10-05-repository-packaging-crates-io-packaging.md) in its distribution channel ("publishes to crates.io", "RayX tooling resolves the gpux checkout"); its packaging rules are restated above.
