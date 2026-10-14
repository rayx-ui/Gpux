# Lane C timings

Where lane C's time goes and what the optimizations of the `gpux-faster-test-lanes` epic bought.
Raw logs stay in `artifacts-temp/` (git-ignored); the file name follows each figure.

## Optimized test build (`[profile.test]`)

Machine: Apple M5 (10 cores), macOS 26.6.2, rustc 1.95.0, Metal, commit `0181ed7` plus the manifest
change below. The test is `containers_laid_over_one_another_scroll_from_tiles` of
`lane_c_scroll_layers`, run alone with `--test-threads=1`, the slowest kind of lane C test: it
renders a 420 by 360 window hundreds of times and compares every frame. Each scope below used a
fresh `CARGO_TARGET_DIR` (so a clean build) and the settings were passed with `cargo --config`;
`artifacts-temp/profile-measure/summary.txt` has every line. Test time is the harness's own
`finished in` figure of two consecutive runs (both are listed), build time is wall clock.

| Scope | Test, run 1 / run 2 (s) | Clean build (s) | Rebuild after touching `gpui.rs` (s) |
|---|---:|---:|---:|
| `opt-level = 0` (before) | 10.68 / 10.63 | 46 | 6 |
| `[profile.dev] opt-level = 1` | 1.62 / 1.59 | 89 | 6 |
| **`[profile.test] opt-level = 1`** | **1.17 / 1.08** | 83 | 5 |
| a dedicated profile (`--profile fastlane`, inherits `dev`, level 1) | 1.15 / 1.24 | 82 | n/a |
| `image` and `gpux-conformance` at 1 | 9.33 / 9.40 | 50 | n/a |
| plus `gpux` and `gpux-wgpu-renderer` at 1 | 8.95 / 8.44 | 76 | n/a |
| every non-workspace dependency (`"*"`) and `gpux-conformance` at 1 | 9.33 / 9.30 | 84 | n/a |

The level-0 row's clean build and test runs were taken before the manifest changed; its rebuild was
taken with `--config profile.test.opt-level=0` over the new manifest (`baseline0.core.log` ends
`unoptimized + debuginfo`). Each rebuild touched `crates/gpui/src/gpui.rs` and was checked to recompile `gpux`
(`core_incremental_build_s` lines, `gpux_recompiled=1`). Debug assertions and overflow checks
stay on in every row (`debug-assertions = true` and `overflow-checks = true` are explicit in the
chosen profile; the `lane_c_profile` test group fails if either is off).

### Why this scope

- Overriding only packages does not help: every package-level scope that leaves some workspace
  crate at level 0 stays within 21% of the unoptimized build (8.44 to 9.40 s against 10.63 s),
  while the three whole-profile scopes reach 1.1 to 1.6 s. Where the remaining time sits (the
  workspace crates' own code, or standard-library code instantiated inside them) was not
  profiled; the measurement shows only that the cheapest scope within about 25% of the
  whole-profile time is a whole-profile one.
- `[profile.test]` over `[profile.dev]`: it is 0.4 to 0.5 s faster in both runs (1.17 and 1.08 s
  against 1.62 and 1.59 s, a difference this measurement does not explain) and applies only to
  `cargo test`, which every lane and the testkit use. `cargo run`, `cargo build` and debugging
  examples keep `[profile.dev]` at level 0. A contributor who builds both profiles compiles the
  dependencies a second time (83 s clean next to 46 s).
- A dedicated profile performs the same (1.15 and 1.24 s, 82 s clean) but a plain `cargo test`
  does not select it: every caller would have to pass `--profile`, which the testkit could do
  but a contributor's own `cargo test -p gpux-conformance` would not.
- The clean build of the test profile costs 37 seconds more on this Mac (46 to 83); a rebuild
  after a change in the core crate costs 5 seconds against 6 at level 0. A slower runner pays more of the
  clean build; the CI measurement belongs to the epic's last sprint, which records it in this
  file.
- RayX sets its own profiles and does not read these (a dependency's manifest profiles are ignored
  for path dependencies), so it is unaffected.
