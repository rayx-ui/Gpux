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

## Before and after in CI

Lane C's `Lane C` step, per backend. All runs are `feature/faster-test-lanes` on GitHub-hosted
runners (`ubuntu-24.04` lavapipe, `windows-2025` WARP, `macos-15` Metal).

| Backend | Before | Sampled, 1 process (the pull request default) | Sampled, 2 processes | Full, 2 processes | Sampled, 3 processes |
|---|---:|---:|---:|---:|---:|
| lavapipe | 39 m | 14 m 07 s | 22 m 06 s | 28 m 31 s | n/a |
| Metal | 25 m | 11 m 41 s | 11 m 33 s | 8 m 58 s | n/a |
| WARP | 2 h 58 m | 1 h 46 m 37 s | n/a | n/a | 1 h 03 m, 1 h 38 m, 1 h 12 m (three runs) |

- *Before*: CI run [38474614288](https://github.com/rayx-ui/Gpux/actions/runs/38474614288) of
  `gpux-fast-mode` (head `f8add89`): `opt-level = 0`, every frame, one process.
- *Sampled, 1 process*: the pull request's run
  [38625501156](https://github.com/rayx-ui/Gpux/actions/runs/38625501156) (lavapipe, Metal) and the
  manual run [38606788745](https://github.com/rayx-ui/Gpux/actions/runs/38606788745) (WARP,
  `lane-c-jobs` 1), both with `[profile.test] opt-level = 1`.
- *2 processes*: manual run [38606794033](https://github.com/rayx-ui/Gpux/actions/runs/38606794033)
  (sampled) and [38654808091](https://github.com/rayx-ui/Gpux/actions/runs/38654808091) (`full`).
- *WARP, 3 processes*: manual runs
  [38625496039](https://github.com/rayx-ui/Gpux/actions/runs/38625496039),
  [38634338461](https://github.com/rayx-ui/Gpux/actions/runs/38634338461) and
  [38647041173](https://github.com/rayx-ui/Gpux/actions/runs/38647041173).
- A whole pull request run took about 30 minutes (the `test (windows-2025)` job, 30 minutes, was
  the longest) where lane C on WARP alone took 3 hours before; the lane C jobs of a pull request
  now finish in 12 to 15 minutes.
- Build time is part of every figure and is not small on a cold runner: Metal's 11 m 41 s holds
  only 3 m 43 s of tests (223 s), lavapipe's 14 m 07 s holds 8 m 42 s (522 s), WARP's
  1 h 46 m holds 1 h 34 m (5622 s). The `[profile.test]` build costs the rest.

### Where the time is now

Per-test durations are the differences between the log's completion timestamps, as
`timings.md` of the testkit reports them (logs in `artifacts-temp/ci-logs/`):

| Backend | Tests, total | The three scroll-layer suites (32 tests) | `lane_c_layer_scales` | Slowest test |
|---|---:|---:|---:|---|
| lavapipe | 522 s | 467 s (89%) | 307 s | `shadowed_content_scrolls_from_tiles_at_fractional_scales`, 88 s |
| Metal | 223 s | 184 s (83%) | 129 s | the same, 36 s |
| WARP | 5622 s | 5377 s (96%) | 3625 s | the same, 980 s |

(Before, on WARP, the 20 slowest tests held 10035 s of 10289 s, 98%; the slowest took 1986 s.)

Per-frame cost of that slowest test, 9 contents at 3 scales: before, 7344 frames (every step of
both windows) and 270, 56 and 31 ms per frame on WARP, lavapipe and Metal. Now, sampled, 5238
frames (the layered window draws all 136 frames of a script, the reference 58 of them) and
187 ms (WARP), 17 ms (lavapipe) and 6.9 ms (Metal). Per frame the optimized test build removed
31% of WARP's time, 70% of lavapipe's and 78% of Metal's: WARP's own rasterization and
readback is most of its frame, and the build and the sampling cannot reach it.

### What each optimization bought

- **`opt-level = 1`** (first section): Metal, 10.6 s to 1.1 s for the measured test; in CI the
  per-frame figures above. It is the largest gain on lavapipe and Metal, the smallest on WARP.
- **Byte-first comparison**: not separable in CI. On the Mac the comparison is a small share of a
  frame once the build is optimized; the change is kept because it costs nothing and reports the
  same message.
- **Sampling**: saves the reference window's draw at 78 of 135 steps of the biggest scripts and
  nothing else, because the layered window must draw every frame. On Metal the three suites
  take 36 s sampled against 42 s full (the Mac, `opt-level = 1`); on CI, lavapipe 8 m 42 s of
  tests sampled and the full run's 28 m 31 s with a build and two processes are not comparable
  step for step. It is a small gain; the full run exists for what it does not compare.
- **WARP off the pull request path**: the largest gain for a pull request: 1 h 46 m is not
  waited for unless `ci-os:windows` or `ci:all` asks for it.
- **Processes**: see the next section.

### Concurrent processes (`--jobs`)

Three complete lane C runs on `windows-2025` with `--jobs 3` passed all 205 tests, each ending
`gpux-testkit: 205 passed, 0 failed, 0 skipped`, with no hang, no device-lost error and no timeout
in their logs (a search for `device lost`, `hung` and `timed out` finds nothing). They took
1 h 03 m, 1 h 38 m and 1 h 12 m against 1 h 46 m for one process: 7% to 41% faster; the spread is
the hosted runner's, on 4 vCPUs shared by three WARP rasterizers. Windows therefore defaults to
three processes (`lane_c_jobs` in `.github/actions/plan/plan.sh`).

Two processes did not pay on the other platforms: lavapipe took 22 m against 14 m (its
rasterizer is multithreaded, so processes compete for the 4 vCPUs), Metal 11 m 33 s against
11 m 41 s. Both stay at one. The layer-scales group alone is 57% of the tests, so no split of
whole groups goes below its time; splitting a group's tests over processes needs per-test
selection and is not done.

### What did not pay off

- Package-level optimization overrides (`image`, the conformance crate, `gpux`, the renderer,
  every dependency): 8.4 to 9.4 s against 10.6 s on the Mac (first section).
- Two processes on lavapipe and Metal (above).
- Sampling as a cost saver on the Mac (14% on Metal): it is kept for the reference draws it
  saves where frames are expensive, and costs nothing in coverage that the nightly run does not
  recover.

### Not measured

- WARP in `full` mode (the nightly run's WARP leg): a manual run was cancelled before it finished,
  so the nightly duration on WARP is not recorded here. The first nightly run supplies it.
- The share of WARP's time that `opt-level = 1` alone removed, apart from sampling and processes,
  is known only per frame (270 to 187 ms for the slowest test).
