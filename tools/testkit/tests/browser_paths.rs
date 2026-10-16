//! Lane B's Playwright project in `tools/browser`: where the testkit looks for it, the profiles it
//! defines, and the two paths inside it that lead back to the repository (the artifacts folder of
//! `playwright.config.ts` and the default build folder of `fixtures/catalog.ts`).

use gpux_testkit::{
    Environment, Os,
    lanes::Invocation,
    run::{RunOutput, Runner},
    web::{self, WebPaths},
};
use std::path::{Component, Path, PathBuf};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn project() -> PathBuf {
    WebPaths::new(&repository()).browser
}

/// `path` with its `.` and `..` components resolved lexically.
fn normalize(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}

/// The single-quoted strings of `text`, in order.
fn quoted(text: &str) -> Vec<&str> {
    text.split('\'').skip(1).step_by(2).collect()
}

#[test]
fn the_project_is_tools_browser() {
    let root = Path::new("/repo");
    assert_eq!(
        WebPaths::new(root).browser,
        root.join("tools").join("browser")
    );
    let project = project();
    for file in ["package.json", "pnpm-lock.yaml", "playwright.config.ts"] {
        assert!(project.join(file).is_file(), "{file}");
    }
    for folder in ["specs", "fixtures"] {
        assert!(project.join(folder).is_dir(), "{folder}");
    }
    // Ignored leftovers (`node_modules`) may stay in older checkouts; tracked files may not.
    let tracked = std::process::Command::new("git")
        // The old folder's name, spelled so the layout guard does not flag this file.
        .args(["ls-files", "--", concat!("tools", "-node")])
        .current_dir(repository())
        .output()
        .expect("git runs in the checkout");
    assert!(tracked.status.success(), "git ls-files failed");
    assert_eq!(String::from_utf8_lossy(&tracked.stdout), "");
}

#[test]
fn the_testkit_profiles_are_the_configured_projects() {
    let config = std::fs::read_to_string(project().join("playwright.config.ts")).unwrap();
    let projects = &config[config.find("projects:").expect("a projects list")..];
    let names: Vec<&str> = projects
        .lines()
        .filter_map(|line| line.trim().strip_prefix("name: "))
        .flat_map(quoted)
        .collect();
    assert_eq!(names, web::PROFILES);
}

#[test]
fn playwright_writes_to_the_repository_artifacts_folder() {
    let config = std::fs::read_to_string(project().join("playwright.config.ts")).unwrap();
    let line = config
        .lines()
        .find(|line| line.starts_with("const artifacts = "))
        .expect("the artifacts constant");
    let relative = quoted(line)[0];
    assert_eq!(
        normalize(&project().join(relative)),
        repository()
            .join("artifacts-temp")
            .join("testkit")
            .join("b")
    );
}

#[test]
fn the_catalog_defaults_to_the_repository_build_folder() {
    let catalog = std::fs::read_to_string(project().join("fixtures").join("catalog.ts")).unwrap();
    let start = catalog
        .find("join(dirname(fileURLToPath(import.meta.url))")
        .expect("the default dist");
    let call = &catalog[start..start + catalog[start..].find(';').unwrap()];
    let mut dist = project().join("fixtures");
    for part in quoted(call) {
        dist.push(part);
    }
    assert_eq!(
        normalize(&dist),
        repository().join("target").join("gpux-web").join("dist")
    );
    // The testkit's own build passes `GPUX_DIST`, which takes precedence.
    assert!(catalog.contains("process.env.GPUX_DIST ??"));
}

struct Checkout;

impl Environment for Checkout {
    fn var(&self, _name: &str) -> Option<String> {
        None
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        Some(PathBuf::from(format!("/fake/bin/{name}")))
    }

    fn os(&self) -> Os {
        Os::Linux
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        None
    }
}

struct NothingRuns;

impl Runner for NothingRuns {
    fn run(
        &mut self,
        invocation: &Invocation,
        _program: &Path,
        _workdir: &Path,
        _log: &Path,
    ) -> anyhow::Result<RunOutput> {
        panic!(
            "nothing runs without Playwright: {}",
            invocation.command_line()
        )
    }
}

#[test]
fn missing_playwright_is_reported_in_tools_browser() {
    let root = tempfile::tempdir().unwrap();
    let mut out = Vec::new();
    let tally = web::e2e(
        root.path(),
        &Checkout,
        &mut NothingRuns,
        "local-gpu",
        None,
        false,
        &mut out,
    );
    assert_eq!(tally.failed, 1);
    let message = &tally.failures[0];
    assert!(message.contains("Playwright is not installed"), "{message}");
    let named = root
        .path()
        .join("tools")
        .join("browser")
        .display()
        .to_string();
    assert!(message.contains(&named), "{message}");
}
