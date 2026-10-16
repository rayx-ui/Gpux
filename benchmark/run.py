#!/usr/bin/env python3
"""Builds and runs the gpux rendering benchmarks, locally and on demand only.

    python3 benchmark/run.py --target macos [--target ios-sim ...] [options]

Targets (pick one or more with --target, or --target all-local for every target this host runs):

    macos        native app, a real Metal window (macOS hosts)
    linux        native app, a real window (Linux hosts)
    windows      native app, a real DirectX 12 window (Windows hosts)
    ios-sim      the iOS app on an iOS Simulator (macOS hosts with Xcode and XcodeGen)
    web-webgpu   the WASM app in Playwright's Chromium on WebGPU
    web-webgl2   the WASM app in Playwright's Chromium on WebGL2

Configurations (--configs, default both):

    off   this checkout with every fast-mode mechanism off (Window::set_fast_mode(OFF))
    on    this checkout with every fast-mode mechanism on (Window::set_fast_mode(ALL))

Both run the same build; only the window's mode differs. Every run is one process (one page in
the browser) that opens one window, draws the scenario's warm-up frames, measures --frames
frames and writes a JSON report. Runs are interleaved: each repetition goes through every
scenario and, within a scenario, both configurations in a rotating order, so drift (thermal
state, background load) spreads over both alike.

Results land in benchmark/results/<name>/: raw/<target>/<scenario>.<config>.r<n>.json, meta.json
and report.md (written by report.py). Nothing here runs in CI.
"""

import argparse
import datetime
import json
import os
import platform
import shutil
import subprocess
import sys
import time
from pathlib import Path

BENCH = Path(__file__).resolve().parent
ROOT = BENCH.parent
NIGHTLY = os.environ.get("GPUX_WEB_TOOLCHAIN", "nightly-2026-06-23")
WASM_RUSTFLAGS = " ".join(
    [
        "-C target-feature=+atomics,+bulk-memory,+mutable-globals",
        "-C link-arg=--shared-memory",
        "-C link-arg=--import-memory",
        "-C link-arg=--max-memory=2147483648",
        "-C link-arg=--export=__heap_base",
        "-C link-arg=--export=__wasm_init_tls",
        "-C link-arg=--export=__tls_size",
        "-C link-arg=--export=__tls_align",
        "-C link-arg=--export=__tls_base",
    ]
)
DESKTOP = {"macos": "Darwin", "linux": "Linux", "windows": "Windows"}
TARGETS = list(DESKTOP) + ["ios-sim", "web-webgpu", "web-webgl2"]
PLANNED = {"android": "the Android target is planned: it needs an APK shell like lane E's"}
CONFIGS = ["off", "on"]
BUNDLE = "dev.gpux.benchmark"


def log(message):
    print(f"[bench {datetime.datetime.now():%H:%M:%S}] {message}", flush=True)


def sh(command, cwd=None, env=None, check=True, capture=False, timeout=None):
    log(" ".join(str(part) for part in command) + (f"  (in {cwd})" if cwd else ""))
    return subprocess.run(
        [str(part) for part in command],
        cwd=cwd,
        env={**os.environ, **(env or {})},
        check=check,
        text=True,
        capture_output=capture,
        timeout=timeout,
    )


def git(*args, cwd=ROOT):
    return subprocess.run(["git", *args], cwd=cwd, check=True, text=True, capture_output=True).stdout.strip()


class Tree:
    """The gpux checkout the benchmark builds against."""

    def __init__(self):
        self.root = ROOT
        self.bench = BENCH

    def commit(self):
        return git("rev-parse", "HEAD", cwd=self.root)

    def dirty(self):
        return bool(git("status", "--porcelain", "--", "crates", "vendor", "Cargo.toml", "Cargo.lock", cwd=self.root))


# ---------------------------------------------------------------------------------------------
# Builds: one artifact per target.


def build_desktop(tree, _target):
    sh(["cargo", "build", "--release", "--bin", "gpux-bench"], cwd=tree.bench)
    exe = "gpux-bench.exe" if platform.system() == "Windows" else "gpux-bench"
    return tree.bench / "target" / "release" / exe


def build_ios(tree, _target):
    sh(
        [
            "cargo", "rustc", "--lib", "--release", "--target", "aarch64-apple-ios-sim",
            "--crate-type", "staticlib",
        ],
        cwd=tree.bench,
    )
    lib = tree.bench / "target" / "aarch64-apple-ios-sim" / "release" / "libgpux_benchmark.a"
    sh(["xcodegen", "generate", "--spec", "project.yml", "--quiet"], cwd=tree.bench / "ios")
    products = tree.bench / "target" / "ios" / "products"
    sh(
        [
            "xcodebuild", "-quiet", "-project", "GpuxBench.xcodeproj", "-target", "GpuxBench",
            "-configuration", "Release", "-sdk", "iphonesimulator",
            f"SYMROOT={products}", f"OBJROOT={tree.bench / 'target' / 'ios' / 'obj'}",
            "CODE_SIGNING_ALLOWED=NO", f"GPUX_BENCH_LIB={lib}",
            "build",
        ],
        cwd=tree.bench / "ios",
    )
    return products / "Release-iphonesimulator" / "GpuxBench.app"


def build_web(tree, _target):
    env = {
        "RUSTFLAGS": WASM_RUSTFLAGS,
        "CFLAGS_wasm32_unknown_unknown": "-matomics -mbulk-memory -mmutable-globals",
    }
    sh(
        [
            "cargo", f"+{NIGHTLY}", "rustc", "--lib", "--release", "--crate-type", "cdylib",
            "--target", "wasm32-unknown-unknown", "-Z", "build-std=std,panic_abort",
            "--target-dir", "target-web",
        ],
        cwd=tree.bench,
        env=env,
    )
    dist = tree.bench / "target-web" / "dist"
    shutil.rmtree(dist, ignore_errors=True)
    wasm = tree.bench / "target-web" / "wasm32-unknown-unknown" / "release" / "gpux_benchmark.wasm"
    sh(["wasm-bindgen", wasm, "--target", "web", "--out-dir", dist])
    shutil.copy(BENCH / "web" / "index.html", dist / "index.html")
    return dist


def artifact(tree, target):
    """Where an earlier build of `target` left its artifact."""
    if target in DESKTOP:
        exe = "gpux-bench.exe" if platform.system() == "Windows" else "gpux-bench"
        return tree.bench / "target" / "release" / exe
    if target == "ios-sim":
        return tree.bench / "target" / "ios" / "products" / "Release-iphonesimulator" / "GpuxBench.app"
    return tree.bench / "target-web" / "dist"


def builder(target):
    if target in DESKTOP:
        return build_desktop
    if target == "ios-sim":
        return build_ios
    return build_web


# ---------------------------------------------------------------------------------------------
# Runs.


def fast_arg(config):
    return config


def hold_clocks(count):
    """Starts `count` processes that spin on their own cores, and returns them.

    A frame that needs less work runs at a lower clock, so its thread takes more CPU time per
    unit of work: on an Apple M-series core the same main-thread work reads 2.9 ms with the
    clusters idle and 1.0 ms with them busy. A build that does less work then reads as slower in
    `main_cpu_ms`. Spinning cores keep the clocks up, so builds are compared by their work.
    """
    return [
        subprocess.Popen([sys.executable, "-c", "while True: pass"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(count)
    ]


def run_desktop(target, jobs, exe, options):
    spinners = hold_clocks(options.hold_clocks)
    try:
        run_desktop_jobs(target, jobs, exe, options)
    finally:
        for spinner in spinners:
            spinner.kill()


def run_desktop_jobs(target, jobs, exe, options):
    # The first launch of a fresh binary pays for the OS checking it; take it before measuring.
    sh([exe, "--scenario", "dashboard-clock", "--frames", "30", "--warmup", "10"], capture=True, check=False, timeout=300)
    for job in jobs:
        command = [
            exe, "--scenario", job["scenario"], "--fast", fast_arg(job["config"]),
            "--frames", options.frames, "--warmup", options.warmup, "--label", job["config"],
            "--target", target, "--out", job["out"],
        ]
        if platform.system() == "Darwin":
            # Keep the display awake: a sleeping display stops delivering frames.
            command = ["caffeinate", "-d", "-i", "-u", *command]
        try:
            sh(command, timeout=300)
        except subprocess.SubprocessError as error:
            log(f"FAILED {job['scenario']} {job['config']}: {error}")


def simulator(name):
    devices = json.loads(sh(["xcrun", "simctl", "list", "devices", "available", "-j"], capture=True).stdout)
    for runtime, entries in sorted(devices["devices"].items(), reverse=True):
        if "iOS" not in runtime:
            continue
        for device in entries:
            if name in (device["name"], device["udid"]):
                return device, runtime.rsplit(".", 1)[-1]
    sys.exit(f"no available simulator named {name}; see `xcrun simctl list devices available`")


def run_ios(target, jobs, app, options):
    device, runtime = simulator(options.device)
    udid = device["udid"]
    sh(["xcrun", "simctl", "boot", udid], check=False)
    sh(["xcrun", "simctl", "bootstatus", udid, "-b"], capture=True)
    sh(["open", "-a", "Simulator"], check=False)
    sh(["xcrun", "simctl", "install", udid, app])
    # The first launch of a freshly installed app is slow; take it before measuring.
    sh(
        ["xcrun", "simctl", "launch", "--console-pty", "--terminate-running-process", udid,
         BUNDLE, "--scenario", "dashboard-clock", "--frames", "30", "--warmup", "10"],
        capture=True, check=False, timeout=300,
    )
    for job in jobs:
        bundle = BUNDLE
        command = [
            "xcrun", "simctl", "launch", "--console-pty", "--terminate-running-process", udid, bundle,
            "--scenario", job["scenario"], "--fast", fast_arg(job["config"]),
            "--frames", options.frames, "--warmup", options.warmup, "--label", job["config"],
            "--target", target,
        ]
        try:
            result = sh(command, capture=True, timeout=300, check=False)
        except subprocess.TimeoutExpired:
            log(f"FAILED {job['scenario']} {job['config']}: timed out")
            sh(["xcrun", "simctl", "terminate", udid, bundle], check=False)
            continue
        line = next((line for line in result.stdout.splitlines() if line.startswith("gpux-bench-result: ")), None)
        if not line:
            log(f"FAILED {job['scenario']} {job['config']}:\n{result.stdout[-2000:]}{result.stderr[-2000:]}")
            continue
        report = json.loads(line.split(": ", 1)[1])
        report["device"] = f"{device['name']} ({runtime})"
        Path(job["out"]).write_text(json.dumps(report, indent=2))
        # Let the simulator settle between processes.
        time.sleep(1)


def node():
    for candidate in [os.environ.get("GPUX_BENCH_NODE"), "/opt/homebrew/opt/node@22/bin/node", shutil.which("node")]:
        if not candidate:
            continue
        version = subprocess.run([candidate, "--version"], capture_output=True, text=True).stdout.strip()
        if version.startswith("v") and int(version[1:].split(".")[0]) >= 18:
            return candidate
    sys.exit("Playwright needs Node.js 18 or newer; set GPUX_BENCH_NODE to one")


def run_web(target, jobs, dist, options):
    backend = "webgpu" if target == "web-webgpu" else "webgl"
    plan = {
        "dist": str(dist),
        "runs": [
            {
                "query": (
                    f"scenario={job['scenario']}&fast={fast_arg(job['config'])}&frames={options.frames}"
                    f"&warmup={options.warmup}&label={job['config']}&target={target}&backend={backend}"
                ),
                "out": job["out"],
            }
            for job in jobs
        ],
    }
    # Beside the build output, not the results: the plan holds this machine's paths.
    plan_file = BENCH / "target-web" / f"plan-{target}.json"
    plan_file.parent.mkdir(exist_ok=True)
    plan_file.write_text(json.dumps(plan, indent=2))
    sh([node(), BENCH / "web" / "run.mjs", "--plan", plan_file], check=False)


def runner(target):
    if target in DESKTOP:
        return run_desktop
    if target == "ios-sim":
        return run_ios
    return run_web


# ---------------------------------------------------------------------------------------------


def run_stats(target, tree, scenarios, out_dir, options):
    """Fast mode's work counters per scenario, from a build that compiles them in."""
    if not options.skip_build:
        sh(
            ["cargo", "build", "--release", "--bin", "gpux-bench", "--features", "fast-stats",
             "--target-dir", "target-stats"],
            cwd=tree.bench,
        )
    exe = tree.bench / "target-stats" / "release" / artifact(tree, target).name
    stats = out_dir / "stats" / target
    stats.mkdir(parents=True, exist_ok=True)
    jobs = [
        {"scenario": scenario, "config": config, "out": str(stats / f"{scenario}.{config}.json")}
        for scenario in scenarios
        for config in ["off", "on"]
    ]
    run_desktop(target, jobs, exe, options)


def machine():
    info = {"os": platform.platform(), "machine": platform.machine(), "python": platform.python_version()}
    if platform.system() == "Darwin":
        info["cpu"] = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True).stdout.strip()
        info["memory_gb"] = int(subprocess.run(["sysctl", "-n", "hw.memsize"], capture_output=True, text=True).stdout) // 2**30
        info["macos"] = subprocess.run(["sw_vers", "-productVersion"], capture_output=True, text=True).stdout.strip()
    return info


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--target", action="append", required=True, help="a target (repeatable) or all-local")
    parser.add_argument("--configs", default=",".join(CONFIGS), help="comma list of off,on")
    parser.add_argument("--scenarios", default="", help="comma list of scenario ids or groups (default: all)")
    parser.add_argument("--reps", type=int, default=3, help="repetitions of every run")
    parser.add_argument("--frames", type=int, default=600, help="measured frames per run")
    parser.add_argument("--warmup", type=int, default=120, help="warm-up frames per run")
    parser.add_argument("--device", default="iPhone 17 Pro", help="iOS Simulator name or UDID")
    parser.add_argument("--name", default=None, help="results directory name")
    parser.add_argument("--skip-build", action="store_true", help="reuse the last build")
    parser.add_argument(
        "--hold-clocks", type=int, default=0, metavar="N",
        help="run N busy processes beside the desktop runs so the CPU clocks stay up and builds are "
        "compared by their work, not by the clock a lighter frame idles at (3 is enough on an M-series Mac)",
    )
    parser.add_argument(
        "--stats",
        action="store_true",
        help="also run every scenario once with fast mode on and off in a build with gpux's "
        "diagnostics counters (desktop targets; not timed)",
    )
    options = parser.parse_args()

    targets = []
    for target in options.target:
        if target == "all-local":
            host = platform.system()
            targets += [name for name, system in DESKTOP.items() if system == host]
            if host == "Darwin":
                targets.append("ios-sim")
            targets += ["web-webgpu", "web-webgl2"]
        elif target in PLANNED:
            sys.exit(PLANNED[target])
        elif target not in TARGETS:
            sys.exit(f"unknown target {target}; choose from {', '.join(TARGETS)}")
        else:
            targets.append(target)
    for target in targets:
        if target in DESKTOP and DESKTOP[target] != platform.system():
            sys.exit(f"the {target} target runs on a {DESKTOP[target]} host; this is {platform.system()}")

    configs = [config for config in options.configs.split(",") if config]
    sys.path.insert(0, str(BENCH))
    from report import SCENARIOS, write_report  # noqa: E402

    wanted = [item for item in options.scenarios.split(",") if item]
    scenarios = [
        scenario for scenario, group in SCENARIOS if not wanted or scenario in wanted or group in wanted
    ]
    if not scenarios:
        sys.exit("no scenario matches --scenarios")

    name = options.name or datetime.datetime.now().strftime("%Y-%m-%d-%H%M")
    out_dir = BENCH / "results" / name
    tree = Tree()
    if tree.dirty():
        log("warning: gpux's sources have uncommitted changes; the report records the commit only")

    meta_file = out_dir / "meta.json"
    meta = json.loads(meta_file.read_text()) if meta_file.exists() else {"targets": {}}
    meta.update(
        {
            "name": name,
            "machine": machine(),
            "commit": tree.commit(),
            "frames": options.frames,
            "warmup": options.warmup,
            "reps": options.reps,
            "hold_clocks": options.hold_clocks,
            "configs": configs,
        }
    )

    for target in targets:
        log(f"target {target}")
        built = artifact(tree, target) if options.skip_build else builder(target)(tree, target)
        raw = out_dir / "raw" / target
        raw.mkdir(parents=True, exist_ok=True)
        jobs = []
        for rep in range(1, options.reps + 1):
            for index, scenario in enumerate(scenarios):
                # Rotate the configurations so none always runs first.
                shift = (index + rep) % len(configs)
                for config in configs[shift:] + configs[:shift]:
                    jobs.append(
                        {
                            "scenario": scenario,
                            "config": config,
                            "out": str(raw / f"{scenario}.{config}.r{rep}.json"),
                        }
                    )
        started = time.time()
        runner(target)(target, jobs, built, options)
        meta["targets"][target] = {
            "finished": datetime.datetime.now().isoformat(timespec="seconds"),
            "minutes": round((time.time() - started) / 60, 1),
            "runs": len(jobs),
            "reports": len(list(raw.glob("*.r*.json"))),
        }
        meta_file.write_text(json.dumps(meta, indent=2))

        if options.stats and target in DESKTOP:
            run_stats(target, tree, scenarios, out_dir, options)

    report = write_report(out_dir)
    log(f"report: {report}")


if __name__ == "__main__":
    main()
