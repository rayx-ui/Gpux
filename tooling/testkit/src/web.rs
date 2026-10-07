//! Lane B's browser commands: `web build` (the threaded `web_host` WASM app and its page),
//! `web serve` (cross-origin isolated static serving) and `web e2e` (Playwright in `tools-node`).

use crate::{
    Environment,
    cli::Selection,
    lanes::{Invocation, OutputKind},
    results::{self, Tally},
    run::{self, Runner},
    serve, wasm_verify,
};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Rust flags for a threaded browser build: atomics, shared imported memory, and the TLS exports
/// `wasm_thread` needs.
pub const WASM_RUSTFLAGS: &str = "-C target-feature=+atomics,+bulk-memory,+mutable-globals \
-C link-arg=--shared-memory -C link-arg=--import-memory -C link-arg=--max-memory=2147483648 \
-C link-arg=--export=__heap_base -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size \
-C link-arg=--export=__tls_align -C link-arg=--export=__tls_base";

/// C flags for dependencies that compile C for the threaded browser build.
pub const WASM_CFLAGS: &str = "-matomics -mbulk-memory -mmutable-globals";

/// The environment variable that overrides the nightly toolchain (`nightly` by default).
pub const TOOLCHAIN_ENV_VAR: &str = "GPUX_WEB_TOOLCHAIN";

/// Where the browser build lives under the workspace root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebPaths {
    /// The cargo target directory of the browser build.
    pub target_dir: PathBuf,
    /// The served directory: the wasm-bindgen output and `index.html`.
    pub dist: PathBuf,
    /// The Playwright project.
    pub tools_node: PathBuf,
}

impl WebPaths {
    /// The paths under `root`.
    pub fn new(root: &Path) -> Self {
        let target_dir = root.join("target").join("gpux-web");
        Self {
            dist: target_dir.join("dist"),
            target_dir,
            tools_node: root.join("tools-node"),
        }
    }

    /// The linked module cargo builds.
    pub fn module(&self) -> PathBuf {
        self.target_dir
            .join("wasm32-unknown-unknown")
            .join("release")
            .join("examples")
            .join("web_host.wasm")
    }
}

/// The `wasm-bindgen` version `Cargo.lock` pins; the CLI must match it.
pub fn locked_wasm_bindgen_version(lock: &str) -> Option<String> {
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "name = \"wasm-bindgen\"" {
            return lines
                .next()?
                .trim()
                .strip_prefix("version = \"")?
                .strip_suffix('"')
                .map(str::to_string);
        }
    }
    None
}

/// The version `wasm-bindgen --version` reports.
pub fn parse_wasm_bindgen_version(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("wasm-bindgen "))
        .map(|version| {
            version
                .split_whitespace()
                .next()
                .unwrap_or(version)
                .to_string()
        })
}

/// The cargo invocation that builds `web_host` as a threaded WASM module.
pub fn cargo_build(toolchain: &str, paths: &WebPaths) -> Invocation {
    Invocation {
        label: "web-build".into(),
        program: "cargo".into(),
        args: vec![
            format!("+{toolchain}"),
            "build".into(),
            "-p".into(),
            "gpux-conformance".into(),
            "--no-default-features".into(),
            "--features".into(),
            "app-host".into(),
            "--example".into(),
            "web_host".into(),
            "--target".into(),
            "wasm32-unknown-unknown".into(),
            "--release".into(),
            "--target-dir".into(),
            paths.target_dir.display().to_string(),
            "-Z".into(),
            "build-std=std,panic_abort".into(),
        ],
        env: vec![
            ("RUSTFLAGS".into(), WASM_RUSTFLAGS.into()),
            ("CFLAGS_wasm32_unknown_unknown".into(), WASM_CFLAGS.into()),
            (
                "CARGO_FUTURE_INCOMPAT_REPORT_FREQUENCY".into(),
                "never".into(),
            ),
        ],
        output: OutputKind::Libtest,
    }
}

/// The page that loads the build. URL opt-ins become globals before the module starts, and the
/// page records whether it is cross-origin isolated, which WASM threads need.
pub fn index_html() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <base href="/" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover" />
  <link rel="icon" href="data:," />
  <title>gpux conformance</title>
  <style>html, body { margin: 0; height: 100%; overflow: hidden; background: #ffffff; }</style>
</head>
<body>
  <script type="module">
    const params = new URLSearchParams(location.search);
    const flag = (name) => params.get(name) === '1' || params.get(name) === 'true';
    if (flag('gpux-html-island') || flag('rayx-html-island')) {
      globalThis.__gpuxHtmlIslandEnabled = true;
    }
    globalThis.__gpuxThreadDiagnostics = {
      crossOriginIsolated: globalThis.crossOriginIsolated === true,
      sharedArrayBuffer: typeof globalThis.SharedArrayBuffer === 'function',
      atomicsWaitAsync: typeof globalThis.Atomics?.waitAsync === 'function',
    };
    const { default: init, run } = await import('./web_host.js');
    await init();
    run();
  </script>
</body>
</html>
"#
    .to_string()
}

/// The SVG the `web-svg-asset` scene fetches through `Svg::external_path`: a full square, so
/// its tint fills the element.
pub const CONFORMANCE_SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" viewBox=\"0 0 24 24\"><rect width=\"24\" height=\"24\"/></svg>\n";

/// The lane-B scenes, for specs that enumerate them: `[{"id": "...", "scales": [1, 2],
/// "areas": ["LayoutFlex"], "clock": false}, ...]`. `clock` marks scenes whose scripts advance
/// time: on the browser's real clock their assertions depend on how fast frames render, so
/// software profiles skip them.
pub fn catalog_json() -> String {
    let scenes: Vec<String> = gpux_conformance::CATALOG
        .iter()
        .filter(|scene| scene.runs_in(gpux_conformance::Lane::B))
        .map(|scene| {
            let scales: Vec<String> = scene.scales.iter().map(|scale| scale.to_string()).collect();
            let areas: Vec<String> = scene
                .areas
                .iter()
                .map(|area| format!("\"{area:?}\""))
                .collect();
            let clock = gpux_conformance::scene::flatten(&(scene.script)())
                .iter()
                .any(|step| matches!(step, gpux_conformance::Step::Advance(_)));
            format!(
                "  {{\"id\": \"{}\", \"scales\": [{}], \"areas\": [{}], \"clock\": {clock}}}",
                scene.id,
                scales.join(", "),
                areas.join(", ")
            )
        })
        .collect();
    format!("[\n{}\n]\n", scenes.join(",\n"))
}

fn run_step(
    invocation: &Invocation,
    workdir: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    log: &Path,
) -> Result<run::RunOutput, String> {
    let program = environment
        .find_program(&invocation.program)
        .ok_or_else(|| format!("`{}` is not on PATH", invocation.program))?;
    let result = runner
        .run(invocation, &program, workdir, log)
        .map_err(|error| format!("{error:#}"))?;
    if !result.success {
        return Err(format!(
            "`{}` failed; see {}",
            invocation.command_line(),
            log.display()
        ));
    }
    Ok(result)
}

/// Builds the browser app into [`WebPaths::dist`]; with `verify`, checks the binary too.
pub fn build(
    root: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    verify: bool,
    out: &mut dyn Write,
) -> Result<WebPaths, String> {
    let paths = WebPaths::new(root);
    let logs = run::artifacts_dir(root, "b");
    let toolchain = environment
        .var(TOOLCHAIN_ENV_VAR)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "nightly".into());

    let lock = std::fs::read_to_string(root.join("Cargo.lock"))
        .map_err(|error| format!("could not read Cargo.lock: {error}"))?;
    let locked =
        locked_wasm_bindgen_version(&lock).ok_or("Cargo.lock does not pin wasm-bindgen")?;
    let version = run_step(
        &Invocation {
            label: "wasm-bindgen-version".into(),
            program: "wasm-bindgen".into(),
            args: vec!["--version".into()],
            env: Vec::new(),
            output: OutputKind::Listing,
        },
        root,
        environment,
        runner,
        &logs.join("wasm-bindgen-version.log"),
    )
    .map_err(|error| {
        format!("{error}; install the CLI with `cargo install wasm-bindgen-cli --version {locked}`")
    })?;
    let installed = parse_wasm_bindgen_version(&version.output).unwrap_or_default();
    if installed != locked {
        return Err(format!(
            "wasm-bindgen {installed} is installed but Cargo.lock pins {locked}: run `cargo install wasm-bindgen-cli --version {locked}`"
        ));
    }

    let cargo = cargo_build(&toolchain, &paths);
    writeln!(out, "web: {}", cargo.command_line()).ok();
    run_step(&cargo, root, environment, runner, &logs.join("web-build.log")).map_err(|error| {
        format!(
            "{error}. The threaded build needs the `{toolchain}` toolchain with `rust-src` and the wasm32 target: `rustup toolchain install {toolchain} --component rust-src --target wasm32-unknown-unknown`"
        )
    })?;

    std::fs::create_dir_all(&paths.dist)
        .map_err(|error| format!("could not create {}: {error}", paths.dist.display()))?;
    let bindgen = Invocation {
        label: "wasm-bindgen".into(),
        program: "wasm-bindgen".into(),
        args: vec![
            paths.module().display().to_string(),
            "--target".into(),
            "web".into(),
            "--out-dir".into(),
            paths.dist.display().to_string(),
        ],
        env: Vec::new(),
        output: OutputKind::Listing,
    };
    writeln!(out, "web: {}", bindgen.command_line()).ok();
    run_step(
        &bindgen,
        root,
        environment,
        runner,
        &logs.join("wasm-bindgen.log"),
    )?;
    std::fs::write(paths.dist.join("index.html"), index_html())
        .map_err(|error| format!("could not write index.html: {error}"))?;
    std::fs::write(paths.dist.join("catalog.json"), catalog_json())
        .map_err(|error| format!("could not write catalog.json: {error}"))?;
    let assets = paths.dist.join("assets");
    std::fs::create_dir_all(&assets)
        .and_then(|()| std::fs::write(assets.join("conformance.svg"), CONFORMANCE_SVG))
        .map_err(|error| format!("could not write the scene assets: {error}"))?;

    if verify {
        let module = paths.dist.join("web_host_bg.wasm");
        let bytes = std::fs::read(&module)
            .map_err(|error| format!("could not read {}: {error}", module.display()))?;
        let verification = wasm_verify::verify_threaded(&bytes)?;
        writeln!(out, "web: threaded wasm verified: {verification}").ok();
    }
    writeln!(out, "web: built {}", paths.dist.display()).ok();
    Ok(paths)
}

/// The Playwright projects `tools-node/playwright.config.ts` defines.
pub const PROFILES: [&str; 3] = ["local-gpu", "ci-linux-swiftshader", "windows-software"];

/// The environment variable that picks the profile `gpux-testkit lane b` runs
/// (`local-gpu` by default).
pub const PROFILE_ENV_VAR: &str = "GPUX_WEB_PROFILE";

/// The specs `gpux-testkit lane b` runs unless the selection narrows them.
pub const LANE_GREP: &str = "@canary|@pr";

/// The profile `gpux-testkit lane b` runs.
pub fn lane_profile(environment: &dyn Environment) -> String {
    environment
        .var(PROFILE_ENV_VAR)
        .filter(|profile| !profile.is_empty())
        .unwrap_or_else(|| "local-gpu".into())
}

/// The Playwright `--grep` of a lane-B selection: a scene's own specs, the pattern given, or
/// the lane's `@canary` and `@pr` specs.
pub fn lane_grep(selection: &Selection) -> String {
    match (&selection.scene, &selection.grep) {
        // Scene specs are titled `<scene> passes its script ...`; scene ids need no escaping.
        (Some(scene), _) => format!("{scene} passes"),
        (None, Some(grep)) => grep.clone(),
        (None, None) => LANE_GREP.into(),
    }
}

/// The spec titles Playwright's `--list` printed (`[project] › file:line › title` lines).
pub fn playwright_list(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('[') && line.contains(" › "))
        .map(str::to_string)
        .collect()
}

fn playwright(profile: &str, grep: Option<&str>, extra: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = ["exec", "playwright", "test", "--project", profile]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    if let Some(grep) = grep {
        args.extend(["--grep".into(), grep.to_string()]);
    }
    args.extend(extra.iter().map(|arg| arg.to_string()));
    args
}

/// Checks what every Playwright run needs and returns `pnpm` and the project folder.
fn playwright_prerequisites(
    root: &Path,
    environment: &dyn Environment,
    profile: &str,
) -> Result<(PathBuf, PathBuf), Tally> {
    if !PROFILES.contains(&profile) {
        return Err(Tally::failure(format!(
            "unknown profile `{profile}`; profiles are {}",
            PROFILES.join(", ")
        )));
    }
    let tools_node = WebPaths::new(root).tools_node;
    if !tools_node
        .join("node_modules")
        .join("@playwright")
        .join("test")
        .is_dir()
    {
        return Err(Tally::failure(format!(
            "Playwright is not installed in {}: run `pnpm install` and `pnpm exec playwright install chromium` there",
            tools_node.display()
        )));
    }
    let Some(pnpm) = environment.find_program("pnpm") else {
        return Err(Tally::failure(
            "`pnpm` is not on PATH: install it (for example `npm install -g pnpm`)",
        ));
    };
    Ok((pnpm, tools_node))
}

/// Prints the specs a profile and pattern select without running them, and returns how many
/// there are. The specs enumerate scenes from the build, so this builds first.
pub fn list(
    root: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    profile: &str,
    grep: &str,
    out: &mut dyn Write,
) -> Result<usize, Tally> {
    let (pnpm, tools_node) = playwright_prerequisites(root, environment, profile)?;
    let paths = build(root, environment, runner, false, out)
        .map_err(|error| Tally::failure(format!("lane b build: {error}")))?;
    let listing = Invocation {
        label: "web-list".into(),
        program: "pnpm".into(),
        args: playwright(profile, Some(grep), &["--list"]),
        env: vec![("GPUX_DIST".into(), paths.dist.display().to_string())],
        output: OutputKind::Listing,
    };
    writeln!(out, "lane b: {}", listing.command_line()).ok();
    let log = run::artifacts_dir(root, "b").join("web-list.log");
    let result = runner
        .run(&listing, &pnpm, &tools_node, &log)
        .map_err(|error| Tally::failure(format!("could not run Playwright: {error:#}")))?;
    if !result.success {
        return Err(Tally::failure(format!(
            "listing `{}` failed; see {}",
            listing.command_line(),
            log.display()
        )));
    }
    let specs = playwright_list(&result.output);
    for spec in &specs {
        writeln!(out, "  {spec}").ok();
    }
    if specs.is_empty() {
        writeln!(out, "lane b: nothing matches the selection").ok();
    }
    Ok(specs.len())
}

/// Builds, serves and runs the Playwright specs of `profile`, filtered by `grep`.
pub fn e2e(
    root: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    profile: &str,
    grep: Option<&str>,
    headed: bool,
    out: &mut dyn Write,
) -> Tally {
    let (program, tools_node) = match playwright_prerequisites(root, environment, profile) {
        Ok(found) => found,
        Err(failure) => return failure,
    };
    let paths = match build(root, environment, runner, true, out) {
        Ok(paths) => paths,
        Err(error) => return Tally::failure(format!("lane b build: {error}")),
    };
    let (address, _server) = match serve::start(paths.dist.clone(), 0) {
        Ok(started) => started,
        Err(error) => {
            return Tally::failure(format!("could not serve {}: {error}", paths.dist.display()));
        }
    };
    let args = playwright(profile, grep, if headed { &["--headed"] } else { &[] });
    let playwright = Invocation {
        label: "web-e2e".into(),
        program: "pnpm".into(),
        args,
        env: vec![
            ("GPUX_BASE_URL".into(), format!("http://{address}/")),
            ("GPUX_DIST".into(), paths.dist.display().to_string()),
        ],
        output: OutputKind::Libtest,
    };
    writeln!(
        out,
        "web: {} (serving {})",
        playwright.command_line(),
        address
    )
    .ok();
    out.flush().ok();
    let log = run::artifacts_dir(root, "b").join("web-e2e.log");
    match runner.run(&playwright, &program, &tools_node, &log) {
        Ok(result) => {
            let tally = results::tally_playwright_list(&result.output);
            if !result.success && tally.failed == 0 {
                let mut tally = tally;
                tally.add(Tally::failure(format!(
                    "Playwright exited with {} without a failed test; see {}",
                    result
                        .code
                        .map_or("a signal".into(), |code| format!("code {code}")),
                    log.display()
                )));
                return tally;
            }
            tally
        }
        Err(error) => Tally::failure(format!("could not run Playwright: {error:#}")),
    }
}
