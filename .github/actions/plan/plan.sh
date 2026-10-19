#!/usr/bin/env bash
# Chooses what a workflow run executes. Reads EVENT_NAME, DISPATCH_JOBS, DISPATCH_PLATFORMS,
# DISPATCH_GREP, PR_LABELS (a JSON array of label names), BASE_SHA and HEAD_SHA, and writes
# `jobs`, `os`, `lane_c` and `grep` to $GITHUB_OUTPUT (stdout when unset).
#
#   manual run   the inputs choose; `all` is everything
#   pull request labels choose: `ci:<job>` limits the jobs (several add up), `ci:all` runs
#                everything, `ci-os:<ubuntu|windows|macos>` limits the platforms;
#                without a `ci:` label a change that touches nothing but documentation, memory,
#                licences or CHANGELOG.md runs only `check`; a release pull request (it changes
#                nothing but own version values in Cargo.toml files and Cargo.lock, plus
#                CHANGELOG.md) runs `check` on ubuntu and `package` (on its own macos-15
#                runner); `ci-os:` labels move `check`; any other change runs every job
#   schedule     everything, lane C in full mode
#
# Lane C (the offscreen pixels, docs/ci.md) is the slow lane: on WARP, Windows' software DirectX
# 12, it took hours. A pull request runs it on lavapipe and Metal; WARP joins on the
# `ci-os:windows` label or `ci:all`, in a manual run that asks for the platform, and nightly.
# Lane C runs its scroll-layer suites sampled, except in a nightly run and a manual run with the
# `full` input (docs/testing.md).
set -euo pipefail

# Whether a pull request without `ci-os:windows` or `ci:all` still runs lane C on WARP.
LANE_C_WARP_ON_PULL_REQUEST=false
# The `--jobs` lane C passes to the testkit on each platform (concurrent test processes). WARP runs
# three: three complete runs on windows-2025 passed without a hang or a lost device and were 7% to 41%
# faster than the 1h46m of one process (1h03m, 1h38m, 1h12m). lavapipe (2 processes: 22 min against 14) and Metal (2: no gain) stay
# at 1. docs/measurements/lane-c-timings.md has the numbers.
lane_c_jobs() { # a manual run's DISPATCH_LANE_C_JOBS overrides the platform's value
  if [ -n "${DISPATCH_LANE_C_JOBS:-}" ]; then echo "$DISPATCH_LANE_C_JOBS"; return; fi
  case "$1" in ubuntu) echo 1 ;; windows) echo 3 ;; macos) echo 1 ;; esac
}

ALL_JOBS=(check test lane-a lane-c lane-d package lane-b lane-e lane-f)
OS_NAMES=(ubuntu windows macos)
image() { case "$1" in ubuntu) echo ubuntu-24.04 ;; windows) echo windows-2025 ;; macos) echo macos-15 ;; esac; }
backend() { case "$1" in ubuntu) echo lavapipe ;; windows) echo warp ;; macos) echo metal ;; esac; }
lines_to_array() { # name, then lines on stdin
  local name=$1 line; eval "$name=()"
  while IFS= read -r line; do [ -n "$line" ] && eval "$name+=(\"\$line\")"; done
  return 0
}

csv_to_lines() { tr ',' '\n' <<<"$1" | sed 's/^ *//;s/ *$//' | grep -v '^$' || true; }

# A manifest or lockfile with its own version values replaced by `*`: the `version = "..."` that is
# the only key on its line in `[package]`, `[workspace.package]` and lockfile `[[package]]`
# sections, and the `version` of a dependency entry whose `path` does not lead into `vendor/`.
# A registry or git bump also changes `checksum`/`source` lines, a fork's version sits on a
# `vendor/` path line and a crates.io requirement has no `path`, so none of them is masked.
mask_own_versions() {
  awk '
    /^[[:space:]]*\[/ { section = $0; gsub(/[[:space:]]/, "", section) }
    (section == "[package]" || section == "[workspace.package]" || section == "[[package]]") &&
      /^[[:space:]]*version[[:space:]]*=[[:space:]]*"[^"]*"[[:space:]]*$/ {
      sub(/"[^"]*"/, "\"*\""); print; next
    }
    /path[[:space:]]*=[[:space:]]*"/ && !/path[[:space:]]*=[[:space:]]*"[^"]*vendor\// {
      gsub(/version[[:space:]]*=[[:space:]]*"[^"]*"/, "version = \"*\""); print; next
    }
    { print }
  '
}

# Whether the change list ($1, one path per line) is a release pull request's: only
# `CHANGELOG.md`, `Cargo.lock` and `Cargo.toml` files outside `vendor/`, every manifest and
# lockfile present on both sides and equal to the base once own version values are masked.
is_release_change() {
  local file head=${HEAD_SHA:-HEAD}
  [ -n "$1" ] || return 1
  while IFS= read -r file; do
    case "$file" in
      vendor/*) return 1 ;;
      CHANGELOG.md) continue ;;
      Cargo.lock | Cargo.toml | */Cargo.toml) ;;
      *) return 1 ;;
    esac
    git cat-file -e "$BASE_SHA:$file" 2>/dev/null && git cat-file -e "$head:$file" 2>/dev/null || return 1
    [ "$(git show "$BASE_SHA:$file" | mask_own_versions)" = "$(git show "$head:$file" | mask_own_versions)" ] || return 1
  done <<<"$1"
}

jobs=("${ALL_JOBS[@]}")
platforms=("${OS_NAMES[@]}")
grep_selector=""
full=false
warp=$LANE_C_WARP_ON_PULL_REQUEST

case "$EVENT_NAME" in
  workflow_dispatch)
    if [ "${DISPATCH_JOBS:-all}" != all ]; then
      lines_to_array jobs < <(csv_to_lines "$DISPATCH_JOBS")
    fi
    if [ "${DISPATCH_PLATFORMS:-all}" != all ]; then
      lines_to_array platforms < <(csv_to_lines "$DISPATCH_PLATFORMS")
    fi
    grep_selector="${DISPATCH_GREP:-}"
    [ "${DISPATCH_FULL:-false}" = true ] && full=true
    warp=true
    ;;
  schedule)
    full=true
    warp=true
    ;;
  pull_request)
    labels="${PR_LABELS:-[]}"
    lines_to_array chosen < <(jq -r '.[] | select(startswith("ci:")) | ltrimstr("ci:")' <<<"$labels")
    lines_to_array chosen_os < <(jq -r '.[] | select(startswith("ci-os:")) | ltrimstr("ci-os:")' <<<"$labels")
    if [ "${#chosen[@]}" -gt 0 ]; then
      if ! printf '%s\n' ${chosen[@]+"${chosen[@]}"} | grep -qx all; then
        jobs=(${chosen[@]+"${chosen[@]}"})
      else
        warp=true
      fi
    elif [ -n "${BASE_SHA:-}" ]; then
      # Documentation, memory and licences cannot change what a test proves. The file list is
      # read whole first: piped into `grep -q`, a long `git diff` dies of SIGPIPE when grep stops
      # at the first code file, and `pipefail` then reads the pull request as documentation only.
      changed=$(git diff --name-only "$BASE_SHA" "${HEAD_SHA:-HEAD}")
      if [ -z "$changed" ] \
        || ! grep -qv -E '^(docs/|\.agentx/|\.claude/|README\.md$|CLAUDE\.md$|CHANGELOG\.md$|LICENSE)' <<<"$changed"; then
        jobs=(check)
      elif is_release_change "$changed"; then
        # A release pull request (docs/releasing.md) changes only version values and the
        # changelog: formatting, compilation and packaging prove it; the nightly covers the rest.
        jobs=(check package)
        platforms=(ubuntu)
      fi
    fi
    if [ "${#chosen_os[@]}" -gt 0 ]; then
      platforms=("${chosen_os[@]}")
      printf '%s\n' "${chosen_os[@]}" | grep -qx windows && warp=true
    fi
    ;;
esac

for name in "${jobs[@]}"; do
  printf '%s\n' "${ALL_JOBS[@]}" | grep -qx "$name" || { echo "unknown job '$name'; known: ${ALL_JOBS[*]}" >&2; exit 1; }
done
for name in "${platforms[@]}"; do
  [ -n "$(image "$name")" ] || { echo "unknown platform '$name'; known: ${OS_NAMES[*]}" >&2; exit 1; }
done

jobs_json=$(printf '%s\n' "${jobs[@]}" | jq -R . | jq -sc .)
os_json=$(for name in "${platforms[@]}"; do image "$name"; done | jq -R . | jq -sc .)
# Lane C's platforms: WARP only where the rules above let it run.
lane_c_platforms=()
skipped=""
for name in "${platforms[@]}"; do
  if [ "$name" = windows ] && [ "$warp" != true ]; then
    printf '%s\n' "${jobs[@]}" | grep -qx lane-c && skipped="lane C skipped on $(backend "$name") ($(image "$name")): add the ci-os:windows label or ci:all to run it; the nightly run covers it"
  else
    lane_c_platforms+=("$name")
  fi
done
lane_c_json=$(for name in ${lane_c_platforms[@]+"${lane_c_platforms[@]}"}; do
  jq -nc --arg os "$(image "$name")" --arg backend "$(backend "$name")" --argjson jobs "$(lane_c_jobs "$name")" \
    '{os: $os, backend: $backend, jobs: $jobs}'
done | jq -sc .)

out="${GITHUB_OUTPUT:-/dev/stdout}"
{
  echo "jobs=$jobs_json"
  echo "os=$os_json"
  echo "lane_c=$lane_c_json"
  echo "grep=$grep_selector"
  echo "full=$full"
  echo "skipped=$skipped"
} >>"$out"
echo "plan: jobs=$jobs_json os=$os_json lane_c=$lane_c_json full=$full grep='$grep_selector'" >&2
[ -z "$skipped" ] || echo "plan: $skipped" >&2
