#!/usr/bin/env bash
# Chooses what a workflow run executes. Reads EVENT_NAME, DISPATCH_JOBS, DISPATCH_PLATFORMS,
# DISPATCH_GREP, PR_LABELS (a JSON array of label names), BASE_SHA and HEAD_SHA, and writes
# `jobs`, `os`, `lane_c` and `grep` to $GITHUB_OUTPUT (stdout when unset).
#
#   manual run   the inputs choose; `all` is everything
#   pull request labels choose: `ci:<job>` limits the jobs (several add up), `ci:all` runs
#                everything, `ci-os:<ubuntu|windows|macos>` limits the platforms;
#                without a `ci:` label a change that touches nothing but documentation, memory
#                or licences runs only `check`, and any other change runs every job
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
# three: three complete runs on windows-2025 passed without a hang or a lost device and were 8% to 41%
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
      # Documentation, memory and licences cannot change what a test proves.
      if ! git diff --name-only "$BASE_SHA" "${HEAD_SHA:-HEAD}" \
        | grep -qv -E '^(docs/|\.agentx/|\.claude/|README\.md$|CLAUDE\.md$|LINUX-WSL\.md$|LICENSE)'; then
        jobs=(check)
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
