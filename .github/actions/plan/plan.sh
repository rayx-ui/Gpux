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
#   schedule     everything
set -euo pipefail

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

case "$EVENT_NAME" in
  workflow_dispatch)
    if [ "${DISPATCH_JOBS:-all}" != all ]; then
      lines_to_array jobs < <(csv_to_lines "$DISPATCH_JOBS")
    fi
    if [ "${DISPATCH_PLATFORMS:-all}" != all ]; then
      lines_to_array platforms < <(csv_to_lines "$DISPATCH_PLATFORMS")
    fi
    grep_selector="${DISPATCH_GREP:-}"
    ;;
  pull_request)
    labels="${PR_LABELS:-[]}"
    lines_to_array chosen < <(jq -r '.[] | select(startswith("ci:")) | ltrimstr("ci:")' <<<"$labels")
    lines_to_array chosen_os < <(jq -r '.[] | select(startswith("ci-os:")) | ltrimstr("ci-os:")' <<<"$labels")
    if [ "${#chosen[@]}" -gt 0 ]; then
      if ! printf '%s\n' ${chosen[@]+"${chosen[@]}"} | grep -qx all; then
        jobs=(${chosen[@]+"${chosen[@]}"})
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
lane_c_json=$(for name in "${platforms[@]}"; do
  jq -nc --arg os "$(image "$name")" --arg backend "$(backend "$name")" '{os: $os, backend: $backend}'
done | jq -sc .)

out="${GITHUB_OUTPUT:-/dev/stdout}"
{
  echo "jobs=$jobs_json"
  echo "os=$os_json"
  echo "lane_c=$lane_c_json"
  echo "grep=$grep_selector"
} >>"$out"
echo "plan: jobs=$jobs_json os=$os_json grep='$grep_selector'" >&2
