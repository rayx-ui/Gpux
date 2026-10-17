#!/usr/bin/env bash
# A benchmark run whose window is hidden ends with status 2 and says why, instead of waiting for
# frames the platform never requests (macOS). A run covered after its first frame must end that
# way; a run opened while the screens are covered must not wait either (AppKit may present a
# window ordered in under another app's window: then it completes). Run from the benchmark
# directory after `cargo build --release`.
set -u
cd "$(dirname "$0")/.."
bench=${GPUX_BENCH:-target/release/gpux-bench}
cover=$(mktemp -d)/cover
swiftc -O tools/cover.swift -o "$cover" || exit 1
args=(--scenario dashboard-clock --fast on --frames 600 --warmup 120 --target macos)
failed=0
passed=0

# `expect_hidden <name> <pid> <log> [completing]`: the run ends with status 2 and the reason, or,
# with `completing`, may also complete with status 0.
expect_hidden() {
    local name=$1 pid=$2 log=$3 completing=${4:-}
    for _ in $(seq 1 40); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
    if kill -0 "$pid" 2>/dev/null; then
        kill "$pid"; echo "FAIL $name: the run was still waiting for frames after 40 s"; failed=1; return
    fi
    wait "$pid"; local status=$?
    if [ "$status" -eq 2 ] && grep -q "gpux-bench: the window" "$log"; then
        echo "ok $name: $(grep 'gpux-bench:' "$log")"; passed=$((passed + 1))
    elif [ "$status" -eq 0 ] && [ -n "$completing" ]; then
        echo "ok $name: the window was presented and the run completed"; passed=$((passed + 1))
    else
        echo "FAIL $name: status $status"; cat "$log"; failed=1
    fi
}

log=$(mktemp)
"$bench" "${args[@]}" >"$log" 2>&1 &
pid=$!
sleep 3
"$cover" 4 >/dev/null
expect_hidden "covered after the first frame" "$pid" "$log"

log=$(mktemp)
"$cover" 14 >/dev/null &
cover_pid=$!
sleep 1
"$bench" "${args[@]}" >"$log" 2>&1 &
pid=$!
expect_hidden "opened while the screens are covered" "$pid" "$log" completing
wait "$cover_pid"

echo "hidden-window: $passed passed"
exit $failed
