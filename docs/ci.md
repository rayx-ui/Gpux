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
| Any other pull request, or the nightly schedule | Everything, as before (lanes B, E and F keep their own path filters). |

Jobs: `check`, `test`, `lane-a`, `lane-c`, `lane-d`, `package` (workflow *Gpux*), `lane-b`
(*Gpux lane B*), `lane-e` (*Gpux lane E*), `lane-f` (*Gpux lane F*).

Labels are read when the plan runs, so after changing them use *Re-run all jobs* (or push); a
label change alone starts nothing. Examples:

- iterating on the scroll-layer shaders: label the pull request `ci:lane-c` and `ci-os:macos`;
- a Windows-only fix: `ci-os:windows` (every job, on Windows only);
- one failing lane-E check: run *Gpux lane E* manually with `grep` = `scroll-layers-android`.

`grep` is the testkit's own selector (`gpux-testkit lane <x> --grep <pattern>`; lane B passes it to
Playwright), applies to lanes A, B, C, D, E and F, and takes no spaces. The `test` job runs the
workspace's cargo tests and has no selector: select it by job and platform.

A job a run skips shows as *skipped*, which counts as passing for branch protection; if a branch
protection rule requires a check by name, require the `plan` job and the jobs that must always run.
Run everything (`ci:all`, or no labels on a code change) before merging.
