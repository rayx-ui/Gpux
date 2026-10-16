# gpux tools

What drives or hosts gpux's tests. The library crates under test live in `crates/`, beside the
two verification crates no consumer depends on:
[`crates/gpux_tests_conformance`](../crates/gpux_tests_conformance) (UI tests: the scene catalog,
lane targets, goldens and app hosts) and
[`crates/gpux_tests_benchmark`](../crates/gpux_tests_benchmark/README.md) (the improvement-loop
benchmarks). [docs/testing.md](../docs/testing.md) describes every lane.

| Folder | What it is | How it runs |
|---|---|---|
| [`testkit`](testkit) | `gpux-testkit`, the single entry point for every test lane | `cargo run -p gpux-testkit -- lane <lane>... [--scene <id>] [--grep <pattern>]` |
| [`perf`](perf) | `gpux-perf`, Zed's `#[perf]` test timer | `cargo perf-test -p <package>` and `cargo perf-compare` (aliases in `.cargo/config.toml`) |
| [`browser`](browser) | Lane B's Playwright project (specs, fixtures, profiles) | `cargo run -p gpux-testkit -- lane b` or `web e2e`; set up once with `pnpm install` and `pnpm exec playwright install chromium` here |
| [`android`](android) | Lane E's Gradle-free APK shell (the manifest) | `cargo run -p gpux-testkit -- lane e` (`android build`, `android run`) |
| [`ios`](ios) | Lane F's XcodeGen UIKit shell (`project.yml`, sources) | `cargo run -p gpux-testkit -- lane f` on a Mac (`ios build --simulator`, `ios run --simulator`) |

The benchmarks run through `python3 crates/gpux_tests_benchmark/run.py --target <target>`; see
their README.

Checkouts from before this layout can keep git-ignored build output (a generated Xcode
project, `node_modules`, the benchmark's `target` folders and lock file) in the old top-level
folders that held these tools and the benchmark; delete those folders. Nothing tracked remains
there.
