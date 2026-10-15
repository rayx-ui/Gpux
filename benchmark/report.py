#!/usr/bin/env python3
"""Turns a results directory's raw JSON reports into report.md tables.

    python3 benchmark/report.py benchmark/results/<name>

Each run's report holds per-frame statistics over its measured frames. A cell here is the median
of that statistic over the run's repetitions. A change is shown against the configuration it is
compared with and marked `~` when it is within the repetitions' own spread (the larger of the two
configurations' relative ranges) or under 3%: such a change is noise, not a result.
"""

import json
import math
import statistics
import sys
from pathlib import Path

# The scenarios in the order src/scenarios/mod.rs lists them, with their groups.
SCENARIOS = [
    ("dashboard-clock", "retention"),
    ("dashboard-one-panel", "retention"),
    ("dashboard-all-panels", "retention"),
    ("table-stream", "retention"),
    ("text-typing", "text"),
    ("text-log-stream", "text"),
    ("text-relabel", "text"),
    ("scroll-uniform-list", "scroll"),
    ("scroll-variable-list", "scroll"),
    ("scroll-div-cards", "scroll"),
    ("scroll-data-table", "scroll"),
    ("hover-grid", "input"),
    ("tree-toggle", "input"),
    ("animated-dots", "animation"),
    ("effects-cards", "animation"),
]

TARGET_NAMES = {
    "macos": "macOS native (Metal)",
    "linux": "Linux native",
    "windows": "Windows native (DirectX 12)",
    "ios-sim": "iOS Simulator (Metal)",
    "web-webgpu": "WASM in Chromium, WebGPU",
    "web-webgl2": "WASM in Chromium, WebGL2",
}

NOISE_FLOOR = 0.03


def get(report, path):
    value = report
    for key in path.split("."):
        if not isinstance(value, dict) or value.get(key) is None:
            return None
        value = value[key]
    return value


METRICS = {
    "cpu": "main_cpu_ms.mean",
    "draw50": "draw_ms.p50",
    "draw95": "draw_ms.p95",
    "draw_mean": "draw_ms.mean",
    "latency50": "frame_ms.p50",
    "fps": "fps",
    "janky": "janky_frames",
    "proc_cpu_frame": "process_cpu_ms_per_frame",
    "proc_cpu_pct": "process_cpu_pct",
    "allocs": "allocations_per_frame",
    "alloc_kb": "allocated_kb_per_frame",
    "heap_live": "heap_live_mb",
    "heap_peak": "heap_peak_mb",
    "fp_end": "footprint_mb.end",
    "fp_peak": "footprint_mb.peak",
    "fp_growth": None,
    "open_ms": "open_to_first_present_ms",
    "first_draw": "first_draw_ms",
    "js_heap": "js_heap_mb",
}


class Cell:
    """One statistic of one (target, scenario, config) over its repetitions."""

    def __init__(self, values):
        self.values = [value for value in values if value is not None]

    @property
    def median(self):
        return statistics.median(self.values) if self.values else None

    @property
    def spread(self):
        """The repetitions' range relative to their median."""
        if len(self.values) < 2 or not self.median:
            return 0.0
        return (max(self.values) - min(self.values)) / abs(self.median)


def load(out_dir):
    """{target: {scenario: {config: [report, ...]}}}"""
    data = {}
    for path in sorted((out_dir / "raw").glob("*/*.r*.json")):
        target = path.parent.name
        scenario, config, _ = path.name.split(".", 2)
        report = json.loads(path.read_text())
        data.setdefault(target, {}).setdefault(scenario, {}).setdefault(config, []).append(report)
    return data


def cell(reports, metric):
    if metric == "fp_growth":
        return Cell(
            [
                get(report, "footprint_mb.end") - get(report, "footprint_mb.after_warmup")
                for report in reports
                if get(report, "footprint_mb.end") is not None
            ]
        )
    return Cell([get(report, METRICS[metric]) for report in reports])


def fmt(value, digits=2):
    if value is None:
        return "–"
    if abs(value) >= 100:
        return f"{value:.0f}"
    if abs(value) >= 10:
        return f"{value:.1f}"
    return f"{value:.{digits}f}"


def delta(new, old, lower_is_better=True, absolute=False):
    """The change from `old` to `new` as text, with ~ when within the noise."""
    if new.median is None or old.median is None:
        return "–", None
    if absolute:
        # For values near zero (growth), a difference says more than a ratio. Noise is the
        # larger of the two configurations' ranges.
        change = new.median - old.median
        noise = max(
            max(new.values) - min(new.values) if new.values else 0,
            max(old.values) - min(old.values) if old.values else 0,
        )
        text = f"{change:+.2f}"
        if abs(change) <= noise:
            return f"~{text}", 0
        better = (change < 0) == lower_is_better
        return (f"**{text}**" if not better else text), (1 if better else -1)
    if old.median == 0:
        return "–", None
    change = new.median / old.median - 1
    noise = max(NOISE_FLOOR, new.spread, old.spread)
    text = f"{change:+.0%}" if abs(change) >= 0.095 else f"{change:+.1%}"
    if abs(change) < noise:
        return f"~{text}", 0
    better = (change < 0) == lower_is_better
    return (f"**{text}**" if not better else text), (1 if better else -1)


def scenario_rows(data, target):
    scenarios = data.get(target, {})
    return [(scenario, group, scenarios[scenario]) for scenario, group in SCENARIOS if scenario in scenarios]


def geomean(ratios):
    ratios = [ratio for ratio in ratios if ratio and ratio > 0]
    if not ratios:
        return None
    return math.exp(sum(math.log(ratio) for ratio in ratios) / len(ratios))


def comparison_table(rows, metric, configs, title, unit, lower_is_better=True, digits=2, absolute=False):
    lines = [f"**{title}** ({unit})", ""]
    header = ["Scenario"] + [config for config in configs]
    if "pre" in configs and "off" in configs:
        header.append("off vs pre")
    if "off" in configs and "on" in configs:
        header.append("on vs off")
    if "pre" in configs and "on" in configs:
        header.append("on vs pre")
    lines.append("| " + " | ".join(header) + " |")
    lines.append("|" + "|".join(["---"] + ["---:"] * (len(header) - 1)) + "|")
    for scenario, _, by_config in rows:
        cells = {config: cell(by_config.get(config, []), metric) for config in configs}
        row = [f"`{scenario}`"] + [fmt(cells[config].median, digits) for config in configs]
        if "pre" in configs and "off" in configs:
            row.append(delta(cells["off"], cells["pre"], lower_is_better, absolute)[0])
        if "off" in configs and "on" in configs:
            row.append(delta(cells["on"], cells["off"], lower_is_better, absolute)[0])
        if "pre" in configs and "on" in configs:
            row.append(delta(cells["on"], cells["pre"], lower_is_better, absolute)[0])
        lines.append("| " + " | ".join(row) + " |")
    return lines + [""]


def summary(data, configs):
    """Per target: geometric-mean ratios and win/loss counts for CPU and draw time."""
    lines = [
        "| Target | Metric | off / pre | regressions (off vs pre) | on / off | on wins / losses (vs off) |",
        "|---|---|---:|---:|---:|---:|",
    ]
    for target in data:
        rows = scenario_rows(data, target)
        for metric, label in [("cpu", "CPU ms per frame"), ("draw50", "draw p50")]:
            off_pre, on_off = [], []
            regressions, wins, losses = 0, 0, 0
            for _, _, by_config in rows:
                cells = {config: cell(by_config.get(config, []), metric) for config in configs}
                if "pre" in cells and "off" in cells and cells["pre"].median and cells["off"].median:
                    off_pre.append(cells["off"].median / cells["pre"].median)
                    regressions += delta(cells["off"], cells["pre"])[1] == -1
                if "off" in cells and "on" in cells and cells["off"].median and cells["on"].median:
                    on_off.append(cells["on"].median / cells["off"].median)
                    verdict = delta(cells["on"], cells["off"])[1]
                    wins += verdict == 1
                    losses += verdict == -1
            ratio = lambda value: f"{value:.3f} ({value - 1:+.1%})" if value else "–"
            lines.append(
                f"| {TARGET_NAMES.get(target, target)} | {label} | {ratio(geomean(off_pre))} | "
                f"{regressions} of {len(off_pre)} | {ratio(geomean(on_off))} | {wins} / {losses} of {len(on_off)} |"
            )
    return lines + [""]


def target_section(data, target, meta, configs):
    rows = scenario_rows(data, target)
    first = next(
        (reports[0] for _, _, by_config in rows for reports in by_config.values() if reports), {}
    )
    info = meta.get("targets", {}).get(target, {})
    lines = [f"## {TARGET_NAMES.get(target, target)}", ""]
    details = []
    if first.get("viewport"):
        details.append(f"viewport {first['viewport'][0]:.0f}×{first['viewport'][1]:.0f} @{first.get('scale_factor', 1):g}x")
    if first.get("vsync_ms"):
        details.append(f"display interval ≈{first['vsync_ms']:.1f} ms")
    if first.get("device"):
        details.append(first["device"])
    if first.get("browser"):
        details.append(f"Chromium {first['browser']}, backend {first.get('backend')}")
    if info:
        details.append(f"{info.get('reports')} of {info.get('runs')} runs reported, {info.get('minutes')} min")
    lines += ["; ".join(details), ""]

    web = target.startswith("web")
    cpu_unit = "ms of Chrome main-thread task time per frame" if web else "ms of main-thread CPU per frame, mean"
    lines += comparison_table(rows, "cpu", configs, "Frame cost: CPU", cpu_unit)
    lines += comparison_table(rows, "draw50", configs, "Window::draw, median", "ms per frame")
    lines += comparison_table(rows, "draw95", configs, "Window::draw, 95th percentile", "ms per frame")
    lines += comparison_table(rows, "fps", configs, "Frame rate", "frames per second", lower_is_better=False, digits=1)
    lines += comparison_table(rows, "janky", configs, "Missed frames", "intervals over 1.5× the display's, of the measured frames; changes in frames", digits=0, absolute=True)
    if not web:
        lines += comparison_table(rows, "proc_cpu_pct", configs, "Process CPU", "% of one core, all threads", digits=1)
    lines += comparison_table(rows, "fp_end", configs, "Memory footprint at the end" if not web else "WASM linear memory at the end", "MB")
    lines += comparison_table(rows, "fp_growth", configs, "Memory growth over the measured frames", "MB; changes in MB", digits=2, absolute=True)
    lines += comparison_table(rows, "heap_peak", configs, "Peak live Rust heap", "MB")
    lines += comparison_table(rows, "allocs", configs, "Heap allocations", "per frame", digits=0)
    lines += comparison_table(rows, "alloc_kb", configs, "Bytes allocated", "KB per frame", digits=0)
    lines += comparison_table(rows, "open_ms", configs, "Startup: window opened to first frame presented", "ms", digits=0)
    lines += comparison_table(rows, "first_draw", configs, "Startup: the first frame's draw", "ms", digits=1)
    return lines


COUNTERS = [
    "views_built",
    "views_reused",
    "views_spliced",
    "cached_views_replayed",
    "style_writes",
    "nodes_claimed",
    "lines_shaped",
    "primitives_painted",
    "primitives_copied",
    "layers_composited",
]


def counters_section(out_dir):
    """Fast mode's work counters per frame, from the `--stats` runs."""
    lines = []
    for target_dir in sorted((out_dir / "stats").glob("*")):
        rows = []
        for scenario, _ in SCENARIOS:
            for config in ["off", "on"]:
                path = target_dir / f"{scenario}.{config}.json"
                if not path.exists():
                    continue
                report = json.loads(path.read_text())
                fields = dict(
                    item.split("=", 1) for item in (report.get("fast_stats") or "").split()[2:] if "=" in item
                )
                frames = float(fields.get("frames", 0)) or 1
                rows.append(
                    [f"`{scenario}`", config]
                    + [fmt(float(fields.get(name, 0)) / frames, 1) for name in COUNTERS]
                )
        if not rows:
            continue
        lines += [
            f"## Fast-mode work counters: {TARGET_NAMES.get(target_dir.name, target_dir.name)}",
            "",
            "One untimed run per scenario in a build with gpux's diagnostics counters (`--stats`): "
            "the work per measured frame.",
            "",
            "| Scenario | mode | " + " | ".join(name.replace("_", " ") for name in COUNTERS) + " |",
            "|---|---|" + "---:|" * len(COUNTERS),
        ]
        lines += ["| " + " | ".join(row) + " |" for row in rows]
        lines.append("")
    return lines


def write_report(out_dir):
    out_dir = Path(out_dir)
    meta_file = out_dir / "meta.json"
    meta = json.loads(meta_file.read_text()) if meta_file.exists() else {}
    data = load(out_dir)
    configs = [config for config in meta.get("configs", ["pre", "off", "on"])]
    machine = meta.get("machine", {})
    lines = [
        f"# gpux benchmark run `{meta.get('name', out_dir.name)}`",
        "",
        f"- Machine: {machine.get('cpu', machine.get('machine'))}, {machine.get('memory_gb', '?')} GB, "
        f"macOS {machine.get('macos', '')} ({machine.get('os')})",
        f"- Commits: " + ", ".join(f"{tree} `{commit[:9]}`" for tree, commit in meta.get("commits", {}).items()),
        f"- Every run: {meta.get('warmup')} warm-up frames, then {meta.get('frames')} measured frames; "
        f"{meta.get('reps')} repetitions, interleaved",
        "- `pre` is gpux before PR #3; `off` and `on` are this checkout with every fast-mode mechanism "
        "off or on (`Window::set_fast_mode`).",
        "- A cell is the median over the repetitions. A change marked `~` is within the repetitions' "
        "spread or under 3% (noise); **bold** marks a change for the worse.",
        "",
        "## Summary",
        "",
        "Geometric means over the scenarios of the ratio of medians.",
        "",
    ]
    lines += summary(data, configs)
    for target in data:
        lines += target_section(data, target, meta, configs)
    lines += counters_section(out_dir)
    report = out_dir / "report.md"
    report.write_text("\n".join(lines))
    return report


if __name__ == "__main__":
    print(write_report(sys.argv[1] if len(sys.argv) > 1 else sorted((Path(__file__).parent / "results").iterdir())[-1]))
