//! Lanes and the commands that run them.

use crate::cli::{Selection, UsageError};
use gpux_conformance::{CATALOG, Lane, SceneSpec, platform_checks::PLATFORM_CHECKS};
use std::fmt;

/// A test lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LaneId {
    /// Behaviour on the test platform.
    A,
    /// A real browser.
    B,
    /// Offscreen pixels through the headless renderer.
    C,
    /// Real desktop windows.
    D,
    /// Android.
    E,
    /// iOS.
    F,
    /// The local real GPU.
    G,
}

impl LaneId {
    /// Parses a lane letter, either case.
    pub fn parse(value: &str) -> Result<Self, UsageError> {
        Ok(match value.to_ascii_lowercase().as_str() {
            "a" => Self::A,
            "b" => Self::B,
            "c" => Self::C,
            "d" => Self::D,
            "e" => Self::E,
            "f" => Self::F,
            "g" => Self::G,
            _ => {
                return Err(UsageError(format!(
                    "unknown lane `{value}`; lanes are a (behaviour), b (browser), c (offscreen \
                     pixels), d (desktop windows), e (Android), f (iOS) and g (local GPU)"
                )));
            }
        })
    }

    /// The lane letter.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
            Self::C => "c",
            Self::D => "d",
            Self::E => "e",
            Self::F => "f",
            Self::G => "g",
        }
    }

    /// The catalog lane, for lanes that run conformance scenes.
    pub fn catalog_lane(self) -> Option<Lane> {
        match self {
            Self::A => Some(Lane::A),
            Self::B => Some(Lane::B),
            Self::C => Some(Lane::C),
            Self::D => Some(Lane::D),
            Self::E => Some(Lane::E),
            Self::F => Some(Lane::F),
            Self::G => None,
        }
    }
}

impl fmt::Display for LaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How the output of an invocation reports results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputKind {
    /// Rust's libtest harness (`cargo test`).
    Libtest,
    /// `gpux-conformance:` result lines from a conformance host.
    ResultLines,
    /// libtest's `--list` output: test names, not results.
    Listing,
    /// Criterion's benchmark report: one result per benchmark.
    Criterion,
}

/// One process a lane runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    /// A short name, used for its log file.
    pub label: String,
    /// The program, resolved on `PATH` before running.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
    /// How its output reports results.
    pub output: OutputKind,
}

impl Invocation {
    /// The command line, for listings and logs.
    pub fn command_line(&self) -> String {
        let mut line: Vec<String> = self
            .env
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        line.push(self.program.clone());
        line.extend(self.args.iter().cloned());
        line.join(" ")
    }
}

/// What a lane runs for a selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanePlan {
    /// The lane.
    pub lane: LaneId,
    /// The processes to run, in order.
    pub invocations: Vec<Invocation>,
    /// The test selection as planned, one name per test.
    pub tests: Vec<String>,
    /// For libtest lanes, one `--list` invocation per test target: `--list` asks libtest for the
    /// exact tests the run executes rather than trusting the plan.
    pub listings: Vec<Listing>,
    /// For a plan whose invocations run side by side, the build that runs once before them so
    /// that they do not contend on the build directory.
    pub prebuild: Option<Invocation>,
}

/// Lists the tests of one libtest target without running them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    /// The test target, prefixed to every listed name.
    pub target: String,
    /// `cargo test ... -- --list` with the run's filters.
    pub invocation: Invocation,
}

fn listing(target: &str, filters: &[String]) -> Listing {
    let mut invocation = cargo(
        &format!("list-{target}"),
        &[
            "test",
            "-p",
            "gpux-conformance",
            "--test",
            target,
            "--",
            "--list",
        ],
        OutputKind::Listing,
    );
    if !filters.is_empty() {
        invocation.args.push("--exact".into());
        invocation.args.extend(filters.iter().cloned());
    }
    Listing {
        target: target.to_string(),
        invocation,
    }
}

/// A lane that cannot run the selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanError(pub String);

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PlanError {}

/// The test function name of a scene in the lane suites.
pub fn test_name(scene: &SceneSpec) -> String {
    scene.id.replace('-', "_")
}

/// The lane-A groups besides the scenes: the catalog, image comparison and the host contract,
/// none of which needs a renderer.
const LANE_A_GROUPS: [&str; 8] = [
    "catalog_coverage",
    "image_compare",
    "golden_policy",
    "executor_clock",
    "host_capabilities",
    "rgb565_tolerance",
    "screen_shows",
    "lane_a_fast_parity",
];

/// The lane-C groups besides the scenes, which need the renderer: lane G's probes-only mode,
/// fast-mode parity, and scroll layers drawn from the renderer's tiles (at whole scale
/// factors, at fractional ones, and under a pointer that hovers a list's rows), the checks
/// that the test build keeps debug assertions and overflow checks on, the frame comparison
/// the scroll-layer groups share, and the sampled and full modes of those groups.
const LANE_C_GROUPS: [&str; 8] = [
    "lane_g",
    "lane_c_fast_parity",
    "lane_c_scroll_layers",
    "lane_c_layer_scales",
    "lane_c_list_hover",
    "lane_c_profile",
    "lane_c_compare",
    "lane_c_sampling",
];

/// The variable that makes lane C's scroll-layer suites run frame for frame.
pub const FULL_VARIABLE: &str = "GPUX_LANE_C_FULL";

/// What a lane C run says about the scroll-layer suites, when it runs them.
pub fn mode_line(full: bool) -> &'static str {
    if full {
        "lane c: full scroll-layer suites"
    } else {
        "lane c: sampled scroll-layer suites (GPUX_LANE_C_FULL=1 or --full runs every frame)"
    }
}

/// Whether an environment value of `GPUX_LANE_C_FULL` asks for the full mode.
pub fn full_requested(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

/// Makes lane C's test processes run the scroll-layer suites in full.
pub fn set_full(plan: &mut LanePlan) {
    for invocation in &mut plan.invocations {
        invocation.env.push((FULL_VARIABLE.into(), "1".into()));
    }
}

/// Plans `lane` for `selection`, run by one process.
pub fn plan(lane: LaneId, selection: &Selection) -> Result<LanePlan, PlanError> {
    plan_with_jobs(lane, selection, 1)
}

/// Plans `lane` for `selection`, run by up to `jobs` concurrent processes where the lane can
/// split (lane C's groups; every other lane is one process).
pub fn plan_with_jobs(
    lane: LaneId,
    selection: &Selection,
    jobs: usize,
) -> Result<LanePlan, PlanError> {
    match lane {
        LaneId::A => plan_a(selection),
        LaneId::C => plan_c(selection, jobs),
        LaneId::D => plan_d(selection),
        LaneId::B => Err(PlanError(
            "lane b runs Playwright specs, not cargo targets: `gpux-testkit lane b` runs them \
             through `web e2e`"
                .into(),
        )),
        LaneId::E => Err(PlanError(
            "lane e runs on an Android device, not through cargo targets: `gpux-testkit lane e` \
             runs it through `android run`"
                .into(),
        )),
        LaneId::F => Err(PlanError(
            "lane f runs on the iOS Simulator, not through cargo targets: `gpux-testkit lane f` \
             runs it through `ios run --simulator`"
                .into(),
        )),
        LaneId::G => Err(PlanError(
            "lane g runs lanes b, c and d and the benchmarks on the local GPU, not one cargo \
             plan: `gpux-testkit lane g` runs them"
                .into(),
        )),
    }
}

/// The catalog scenes of `lane` that the selection keeps.
fn scenes(lane: Lane, selection: &Selection) -> Result<Vec<&'static SceneSpec>, PlanError> {
    if let Some(id) = &selection.scene {
        let scene = gpux_conformance::scene(id).ok_or_else(|| {
            let ids = || CATALOG.iter().map(|scene| scene.id);
            let mut similar: Vec<&str> = ids()
                .filter(|candidate| candidate.contains(id.as_str()) || id.contains(*candidate))
                .collect();
            if similar.is_empty() {
                let first_word = id.split('-').next().unwrap_or_default();
                similar = ids()
                    .filter(|candidate| candidate.split('-').next() == Some(first_word))
                    .collect();
            }
            PlanError(if similar.is_empty() {
                format!("no conformance scene `{id}`; `--list` shows the lane's scenes")
            } else {
                format!(
                    "no conformance scene `{id}`; similar scenes: {}",
                    similar.join(", ")
                )
            })
        })?;
        if !scene.runs_in(lane) {
            return Err(PlanError(format!(
                "scene `{id}` does not run in lane {}",
                lane.as_str()
            )));
        }
    }
    Ok(CATALOG
        .iter()
        .filter(|scene| scene.runs_in(lane))
        .filter(|scene| {
            gpux_conformance::selects(
                scene.id,
                selection.scene.as_deref(),
                selection.grep.as_deref(),
            )
        })
        .collect())
}

fn cargo(label: &str, args: &[&str], output: OutputKind) -> Invocation {
    Invocation {
        label: label.to_string(),
        program: "cargo".to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        env: Vec::new(),
        output,
    }
}

fn plan_a(selection: &Selection) -> Result<LanePlan, PlanError> {
    let scenes = scenes(Lane::A, selection)?;
    let mut tests: Vec<String> = scenes
        .iter()
        .map(|scene| format!("lane_a::{}", test_name(scene)))
        .collect();
    let mut args = vec!["test", "-p", "gpux-conformance", "--test", "lane_a"];
    if selection.is_everything() {
        for group in LANE_A_GROUPS {
            args.extend(["--test", group]);
            tests.push(format!("{group}::*"));
        }
        // A failing target must not keep the later ones from reporting.
        args.push("--no-fail-fast");
    }
    let names: Vec<String> = scenes.iter().map(|scene| test_name(scene)).collect();
    let mut invocation = cargo("lane-a", &args, OutputKind::Libtest);
    let mut listings = Vec::new();
    if selection.is_everything() {
        listings.push(listing("lane_a", &[]));
        listings.extend(LANE_A_GROUPS.iter().map(|group| listing(group, &[])));
    } else {
        invocation.args.push("--".into());
        invocation.args.push("--exact".into());
        invocation.args.extend(names.iter().cloned());
        if !names.is_empty() {
            listings.push(listing("lane_a", &names));
        }
    }
    let empty = tests.is_empty();
    Ok(LanePlan {
        lane: LaneId::A,
        invocations: if empty { Vec::new() } else { vec![invocation] },
        tests,
        listings,
        prebuild: None,
    })
}

/// The lane C units a run is made of, with their relative cost in milliseconds: the scenes of
/// `lane_c` and each group of [`LANE_C_GROUPS`]. Measured on an Apple M5 on Metal, in the sampled
/// mode, from the `timings.md` of `gpux-testkit lane c`; only the ratios matter, and a unit
/// missing here makes `lane_jobs` fail. Refresh them by hand from a new `timings.md`.
const LANE_C_WEIGHTS: [(&str, u64); 9] = [
    ("lane_c", 880),
    ("lane_g", 580),
    ("lane_c_fast_parity", 1_980),
    ("lane_c_scroll_layers", 7_470),
    ("lane_c_layer_scales", 20_460),
    ("lane_c_list_hover", 2_710),
    ("lane_c_profile", 10),
    ("lane_c_compare", 10),
    ("lane_c_sampling", 2_020),
];

/// The weight of lane C unit `unit`.
pub fn weight(unit: &str) -> Option<u64> {
    LANE_C_WEIGHTS
        .iter()
        .find(|(name, _)| *name == unit)
        .map(|(_, weight)| *weight)
}

/// Splits `units` over at most `workers` processes, heaviest first onto the least loaded one
/// (ties go to the lowest worker), so the slowest units never share a process while there are
/// processes to spare. Each worker keeps its units in their given order, workers come in the
/// order of their first unit, and no worker is empty.
pub fn partition(units: &[(String, u64)], workers: usize) -> Vec<Vec<String>> {
    let workers = workers.clamp(1, units.len().max(1));
    let mut by_weight: Vec<usize> = (0..units.len()).collect();
    by_weight.sort_by(|&a, &b| units[b].1.cmp(&units[a].1).then(a.cmp(&b)));
    let mut loads = vec![0u64; workers];
    let mut assigned: Vec<Vec<usize>> = vec![Vec::new(); workers];
    for unit in by_weight {
        let lightest = (0..workers)
            .min_by_key(|&worker| (loads[worker], worker))
            .unwrap();
        loads[lightest] += units[unit].1;
        assigned[lightest].push(unit);
    }
    let mut result: Vec<Vec<usize>> = assigned.into_iter().filter(|w| !w.is_empty()).collect();
    for worker in &mut result {
        worker.sort_unstable();
    }
    result.sort_by_key(|worker| worker[0]);
    result
        .into_iter()
        .map(|worker| {
            worker
                .into_iter()
                .map(|unit| units[unit].0.clone())
                .collect()
        })
        .collect()
}

fn plan_c(selection: &Selection, jobs: usize) -> Result<LanePlan, PlanError> {
    let scenes = scenes(Lane::C, selection)?;
    let names: Vec<String> = scenes
        .iter()
        .flat_map(|scene| {
            let name = test_name(scene);
            [format!("at_1x::{name}"), format!("at_2x::{name}")]
        })
        .collect();
    let mut args = vec!["test", "-p", "gpux-conformance", "--test", "lane_c"];
    let mut tests: Vec<String> = names.iter().map(|name| format!("lane_c::{name}")).collect();
    let mut listings = Vec::new();
    if selection.is_everything() {
        for group in LANE_C_GROUPS {
            args.extend(["--test", group]);
            tests.push(format!("{group}::*"));
        }
        // A failing target must not keep the later ones from reporting.
        args.push("--no-fail-fast");
        listings.push(listing("lane_c", &[]));
        listings.extend(LANE_C_GROUPS.iter().map(|group| listing(group, &[])));
    } else if !names.is_empty() {
        listings.push(listing("lane_c", &names));
    }
    // Scenes share one cached device; rendering one at a time keeps DX12 stable.
    args.extend(["--", "--test-threads=1"]);
    let mut invocation = cargo("lane-c", &args, OutputKind::Libtest);
    if !selection.is_everything() {
        invocation.args.push("--exact".into());
        invocation.args.extend(names.iter().cloned());
    }
    if jobs > 1 && selection.is_everything() && !names.is_empty() {
        return Ok(plan_c_jobs(jobs, tests, listings));
    }
    Ok(LanePlan {
        lane: LaneId::C,
        invocations: if names.is_empty() {
            Vec::new()
        } else {
            vec![invocation]
        },
        tests,
        listings,
        prebuild: None,
    })
}

/// The whole lane C as up to `jobs` processes of whole test groups, each with its own device.
fn plan_c_jobs(jobs: usize, tests: Vec<String>, listings: Vec<Listing>) -> LanePlan {
    let units: Vec<(String, u64)> = std::iter::once("lane_c")
        .chain(LANE_C_GROUPS)
        .map(|unit| (unit.to_string(), weight(unit).unwrap_or(0)))
        .collect();
    let workers = partition(&units, jobs);
    let invocations = workers
        .iter()
        .enumerate()
        .map(|(index, groups)| {
            let mut args = vec!["test".to_string(), "-p".into(), "gpux-conformance".into()];
            for group in groups {
                args.extend(["--test".to_string(), group.clone()]);
            }
            // A failing target must not keep the later ones from reporting; scenes of a process
            // share its one cached device, so they render one at a time.
            args.extend([
                "--no-fail-fast".into(),
                "--".into(),
                "--test-threads=1".into(),
            ]);
            Invocation {
                label: format!("lane-c-job-{}", index + 1),
                program: "cargo".into(),
                args,
                env: Vec::new(),
                output: OutputKind::Libtest,
            }
        })
        .collect();
    let mut build = vec!["test".to_string(), "-p".into(), "gpux-conformance".into()];
    for (unit, _) in &units {
        build.extend(["--test".to_string(), unit.clone()]);
    }
    build.push("--no-run".into());
    LanePlan {
        lane: LaneId::C,
        invocations,
        tests,
        listings,
        prebuild: Some(Invocation {
            label: "lane-c-build".into(),
            program: "cargo".into(),
            args: build,
            env: Vec::new(),
            // Echoed like any lane run: a cold build takes minutes and a failure must show.
            output: OutputKind::Libtest,
        }),
    }
}

fn plan_d(selection: &Selection) -> Result<LanePlan, PlanError> {
    // The desktop host selects with the same function, so this is exactly what it runs.
    let checks: Vec<&str> = PLATFORM_CHECKS
        .iter()
        .map(|check| check.id)
        .filter(|id| {
            gpux_conformance::selects(id, selection.scene.as_deref(), selection.grep.as_deref())
        })
        .collect();
    let scenes = scenes(Lane::D, selection)?;
    let mut tests: Vec<String> = checks
        .iter()
        .map(|id| format!("desktop_host::{id}"))
        .collect();
    tests.extend(
        scenes
            .iter()
            .map(|scene| format!("desktop_host::{}", scene.id)),
    );
    let mut invocation = cargo(
        "lane-d",
        &[
            "run",
            "-p",
            "gpux-conformance",
            "--no-default-features",
            "--features",
            "app-host",
            "--example",
            "desktop_host",
            "--",
        ],
        OutputKind::ResultLines,
    );
    if let Some(scene) = &selection.scene {
        invocation.args.extend(["--scene".into(), scene.clone()]);
    }
    if let Some(pattern) = &selection.grep {
        invocation.args.extend(["--grep".into(), pattern.clone()]);
    }
    Ok(LanePlan {
        lane: LaneId::D,
        invocations: if tests.is_empty() {
            Vec::new()
        } else {
            vec![invocation]
        },
        tests,
        listings: Vec::new(),
        prebuild: None,
    })
}
