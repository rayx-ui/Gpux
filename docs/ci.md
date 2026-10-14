# Choosing what CI runs

Every workflow starts with a `plan` job (`.github/actions/plan`) that decides which jobs, platforms
and tests the run executes, so a change that touches one area does not rerun every lane on every
platform. The rules, in order:

| Trigger | What runs |
|---|---|
| Manual run (Actions → the workflow → *Run workflow*) | The inputs choose: `jobs`, `platforms`, `grep`. `all` is everything. |
| Pull request with a `ci:<job>` label | Only the labelled jobs (several labels add up). `ci:all` runs everything. |
| Pull request with a `ci-os:<platform>` label | Only on those platforms: `ci-os:ubuntu`, `ci-os:windows`, `ci-os:macos` (several add up). |
| Pull request without `ci:` labels that changes only `docs/`, `.agentx/`, `.claude/` or the top-level markdown and licence files | Only `check` (format, `cargo check`). |
| Any other pull request | Everything, as before (lanes B, E and F keep their own path filters), except lane C on WARP (below). |
| Nightly schedule (03:17 UTC) | Everything, on every platform, with lane C's scroll-layer suites in full mode. |

Lane C on WARP (Windows' software DirectX 12) is the slowest job, so a pull request leaves it out:
lane C runs on lavapipe (ubuntu) and Metal (macos) on every push, and on WARP only with the
`ci-os:windows` label or `ci:all`, in a manual run that includes `windows`, and in the nightly
run. The `plan` job prints the platform it left out (`skipped`). Lane C also runs its
scroll-layer suites *sampled* except in the nightly run and a manual run with `full` set; see
[testing.md](testing.md#lane-c-offscreen-pixels). Change `LANE_C_WARP_ON_PULL_REQUEST` in
`.github/actions/plan/plan.sh` to flip the default, and `lane_c_jobs` there to change a
platform's `--jobs`.

Jobs: `check`, `test`, `lane-a`, `lane-c`, `lane-d`, `package` (workflow *Gpux*), `lane-b`
(*Gpux lane B*), `lane-e` (*Gpux lane E*), `lane-f` (*Gpux lane F*).

Labels are read when the plan runs, so after changing them use *Re-run all jobs* (or push); a
label change alone starts nothing. Examples:

- iterating on the scroll-layer shaders: label the pull request `ci:lane-c` and `ci-os:macos`;
- a Windows-only fix: `ci-os:windows` (every job, on Windows only);
- one failing lane-E check: run *Gpux lane E* manually with `grep` = `scroll-layers-android`.

`full` (manual runs only) runs lane C's scroll-layer suites frame for frame, and `lane-c-jobs` sets
the number of concurrent test processes of lane C on every platform (each platform's default is
`lane_c_jobs` in `plan.sh`, 1 until a measurement shows more is stable and faster).

`grep` is the testkit's own selector (`gpux-testkit lane <x> --grep <pattern>`; lane B passes it to
Playwright), applies to lanes A, B, C, D, E and F, and takes no spaces. The `test` job runs the
workspace's cargo tests and has no selector: select it by job and platform.

A job a run skips shows as *skipped*, which counts as passing for branch protection; if a branch
protection rule requires a check by name, require the `plan` job and the jobs that must always run.
Run everything (`ci:all`, or no labels on a code change) before merging.
