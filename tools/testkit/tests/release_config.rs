//! The release configuration of the whole workspace: one version for every own package (major 1
//! under the 1.x policy) with the vendored forks on their own `-gpux.N` versions, `release-plz.toml`
//! (one version group whose only tag, GitHub Release and changelog come from `gpux`), the
//! `release-plz.yml` workflow, and the root `CHANGELOG.md`. These invariants hold after every
//! release pull request and every Zed import; docs/releasing.md explains them.

use std::path::{Path, PathBuf};

use toml::{Table, Value};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn parse(path: &str) -> Table {
    read(path)
        .parse::<Table>()
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn table<'a>(table: &'a Table, key: &str) -> &'a Table {
    table
        .get(key)
        .and_then(Value::as_table)
        .unwrap_or_else(|| panic!("missing table `{key}`"))
}

fn string<'a>(table: &'a Table, key: &str) -> &'a str {
    table
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing string `{key}`"))
}

/// A workspace member: its package name, version, manifest directory and whether it is a fork.
#[derive(Debug)]
struct Member {
    name: String,
    version: String,
    dir: String,
    fork: bool,
}

fn workspace_version() -> String {
    let manifest = parse("Cargo.toml");
    string(table(table(&manifest, "workspace"), "package"), "version").to_string()
}

fn members() -> Vec<Member> {
    let manifest = parse("Cargo.toml");
    let workspace_version = workspace_version();
    table(&manifest, "workspace")["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| {
            let dir = member.as_str().unwrap().to_string();
            let package = parse(&format!("{dir}/Cargo.toml"));
            let package = table(&package, "package");
            let version = match &package["version"] {
                Value::String(version) => version.clone(),
                Value::Table(inherited) => {
                    assert_eq!(inherited.get("workspace"), Some(&Value::Boolean(true)));
                    workspace_version.clone()
                }
                other => panic!("{dir}: unexpected version {other}"),
            };
            Member {
                name: string(package, "name").to_string(),
                version,
                fork: dir.starts_with("vendor/"),
                dir,
            }
        })
        .collect()
}

fn own_names() -> Vec<String> {
    let mut names: Vec<String> = members()
        .into_iter()
        .filter(|member| !member.fork)
        .map(|member| member.name)
        .collect();
    names.sort();
    names
}

/// Every dependency table of a manifest: `[dependencies]`, `[dev-dependencies]`,
/// `[build-dependencies]` and their `[target.'...'.*]` forms.
fn dependency_tables(manifest: &Table) -> Vec<&Table> {
    const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut tables: Vec<&Table> = KINDS
        .iter()
        .filter_map(|kind| manifest.get(*kind).and_then(Value::as_table))
        .collect();
    if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
        for target in targets.values().filter_map(Value::as_table) {
            tables.extend(
                KINDS
                    .iter()
                    .filter_map(|kind| target.get(*kind).and_then(Value::as_table)),
            );
        }
    }
    tables
}

/// The `(manifest, key, path, version)` of every requirement on an own package: a dependency with
/// a `path` that does not lead into `vendor/`, together with its `version` if it has one.
fn own_requirements() -> Vec<(String, String, String, Option<String>)> {
    let mut requirements = Vec::new();
    let mut collect = |manifest: &str, base: &Path, entries: &Table| {
        for (key, entry) in entries {
            let Some(entry) = entry.as_table() else {
                continue;
            };
            let Some(path) = entry.get("path").and_then(Value::as_str) else {
                continue;
            };
            let target = base.join(path);
            let target = target.canonicalize().unwrap_or(target);
            if target.starts_with(root().join("vendor")) {
                continue;
            }
            requirements.push((
                manifest.to_string(),
                key.clone(),
                path.to_string(),
                entry
                    .get("version")
                    .and_then(Value::as_str)
                    .map(String::from),
            ));
        }
    };
    let manifest = parse("Cargo.toml");
    collect(
        "Cargo.toml",
        &root(),
        table(table(&manifest, "workspace"), "dependencies"),
    );
    for member in members().iter().filter(|member| !member.fork) {
        let path = format!("{}/Cargo.toml", member.dir);
        let manifest = parse(&path);
        for entries in dependency_tables(&manifest) {
            collect(&path, &root().join(&member.dir), entries);
        }
    }
    requirements
}

#[test]
fn the_workspace_version_is_on_major_1() {
    // The 1.x policy holds while the owner is the only consumer; this assertion changes when
    // the policy ends (docs/releasing.md).
    let version = workspace_version();
    assert!(
        version.starts_with("1.") && version.split('.').count() == 3,
        "workspace version {version} is not 1.x"
    );
}

#[test]
fn every_own_package_has_the_workspace_version() {
    let workspace_version = workspace_version();
    for member in members().iter().filter(|member| !member.fork) {
        assert_eq!(member.version, workspace_version, "{}", member.dir);
        let manifest = parse(&format!("{}/Cargo.toml", member.dir));
        assert_eq!(
            table(&manifest, "package")["version"].to_string(),
            "{ workspace = true }",
            "{} must keep `version.workspace = true`",
            member.dir
        );
    }
}

#[test]
fn every_own_requirement_names_the_workspace_version() {
    let workspace_version = workspace_version();
    let requirements = own_requirements();
    let root_versioned = requirements
        .iter()
        .filter(|(manifest, ..)| manifest == "Cargo.toml")
        .filter(|(.., version)| version.is_some())
        .count();
    // Every own package except gpux-testkit, which nothing depends on.
    assert_eq!(root_versioned, own_names().len() - 1);
    for (manifest, key, path, version) in &requirements {
        if let Some(version) = version {
            assert_eq!(
                version, &workspace_version,
                "{manifest}: `{key}` (path {path}) must require the workspace version"
            );
        }
    }
    assert!(
        requirements.iter().any(|(manifest, key, _, version)| {
            manifest == "crates/gpui_mobile/Cargo.toml" && key == "gpui" && version.is_some()
        }),
        "gpui_mobile's inline gpux requirement is versioned"
    );
}

#[test]
fn the_forks_keep_their_own_versions() {
    let workspace_version = workspace_version();
    let forks: Vec<Member> = members().into_iter().filter(|member| member.fork).collect();
    assert_eq!(forks.len(), 8, "{forks:?}");
    for fork in forks {
        assert!(fork.version.contains("-gpux."), "{fork:?}");
        assert_ne!(fork.version, workspace_version, "{fork:?}");
    }
}

fn release_plz() -> Table {
    parse("release-plz.toml")
}

fn packages(config: &Table) -> Vec<&Table> {
    config["package"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| package.as_table().unwrap())
        .collect()
}

#[test]
fn release_plz_releases_the_workspace_as_one_git_only_version() {
    let config = release_plz();
    let workspace = table(&config, "workspace");
    let expect = [
        ("git_only", Value::Boolean(true)),
        ("publish", Value::Boolean(false)),
        ("git_tag_name", Value::String("v{{ version }}".into())),
        ("semver_check", Value::Boolean(false)),
        ("git_tag_enable", Value::Boolean(false)),
        ("git_release_enable", Value::Boolean(false)),
        ("changelog_update", Value::Boolean(false)),
        ("release_commits", Value::String("^(feat|fix|perf)".into())),
    ];
    for (key, value) in &expect {
        assert_eq!(workspace.get(*key), Some(value), "[workspace] {key}");
    }
    assert_eq!(workspace.len(), expect.len(), "{workspace:?}");
}

#[test]
fn only_gpux_tags_releases_and_writes_the_changelog() {
    let config = release_plz();
    let packages = packages(&config);
    let gpux = packages
        .iter()
        .find(|package| string(package, "name") == "gpux")
        .unwrap();
    let expect = [
        ("version_group", Value::String("gpux".into())),
        ("git_tag_enable", Value::Boolean(true)),
        ("git_release_enable", Value::Boolean(true)),
        // The spike's finding: the default Release name is `gpux-vX.Y.Z`.
        ("git_release_name", Value::String("v{{ version }}".into())),
        ("changelog_update", Value::Boolean(true)),
        ("changelog_path", Value::String("CHANGELOG.md".into())),
    ];
    for (key, value) in &expect {
        assert_eq!(gpux.get(*key), Some(value), "gpux {key}");
    }
    let mut included: Vec<String> = gpux["changelog_include"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap().to_string())
        .collect();
    included.sort();
    let others: Vec<String> = own_names().into_iter().filter(|n| n != "gpux").collect();
    assert_eq!(
        included, others,
        "changelog_include lists every other own package"
    );
    assert_eq!(gpux.len(), expect.len() + 2, "{gpux:?}");
}

#[test]
fn every_workspace_package_has_exactly_one_entry() {
    let config = release_plz();
    let members = members();
    let packages = packages(&config);
    let mut names: Vec<&str> = packages
        .iter()
        .map(|package| string(package, "name"))
        .collect();
    names.sort();
    let mut expected: Vec<&str> = members.iter().map(|member| member.name.as_str()).collect();
    expected.sort();
    assert_eq!(
        names, expected,
        "release-plz.toml has one [[package]] per workspace package (a new crate joins the \
         `gpux` version group and gpux's changelog_include)"
    );
    for member in &members {
        let package = packages
            .iter()
            .find(|package| string(package, "name") == member.name)
            .unwrap();
        if member.fork {
            assert_eq!(package.len(), 2, "{package:?}");
            assert_eq!(package.get("release"), Some(&Value::Boolean(false)));
        } else if member.name != "gpux" {
            assert_eq!(package.len(), 2, "{package:?}");
            assert_eq!(string(package, "version_group"), "gpux");
        }
    }
}

#[test]
fn release_commits_start_releases_only_for_feat_fix_and_perf() {
    let config = release_plz();
    let pattern =
        regex::Regex::new(string(table(&config, "workspace"), "release_commits")).unwrap();
    for subject in [
        "feat: x",
        "fix(gpui): x",
        "perf: x",
        "feat(gpux): import Zed GPUI closure 0123abc",
    ] {
        assert!(pattern.is_match(subject), "{subject}");
    }
    for subject in [
        "docs: x",
        "test: x",
        "ci: x",
        "chore: x",
        "chore(agentx): x",
        "refactor(gpui): x",
        "build: x",
        "style: x",
    ] {
        assert!(!pattern.is_match(subject), "{subject}");
    }
}

#[test]
fn the_changelog_template_names_gpux_and_prints_migration_footers() {
    let config = release_plz();
    let changelog = table(&config, "changelog");
    let header = string(changelog, "header");
    assert!(header.starts_with("# Changelog\n"), "{header}");
    assert!(header.contains("gpux"), "{header}");
    assert!(
        header.contains("whole workspace is released as one"),
        "{header}"
    );
    let body = string(changelog, "body");
    assert!(
        body.contains(r#"## [{{ version }}] - {{ timestamp | date(format="%Y-%m-%d") }}"#),
        "{body}"
    );
    assert!(body.contains("for footer in commit.footers"), "{body}");
    assert!(body.contains(r#"footer.token == "Migration""#), "{body}");
    assert!(body.contains("- Migration: {{ footer.value }}"), "{body}");
    assert_eq!(string(changelog, "tag_pattern"), "v[0-9].*");
    let parsers: Vec<&Table> = changelog["commit_parsers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|parser| parser.as_table().unwrap())
        .collect();
    let skipped: Vec<&str> = parsers
        .iter()
        .filter(|parser| parser.get("skip") == Some(&Value::Boolean(true)))
        .map(|parser| string(parser, "message"))
        .collect();
    assert_eq!(
        skipped,
        [
            "^Merge ",
            r"^chore\(agentx\)",
            r"^chore\(release\)",
            r"^chore\(deps\)"
        ]
    );
    let groups: Vec<(&str, &str)> = parsers
        .iter()
        .filter_map(|parser| {
            Some((
                string(parser, "message"),
                parser.get("group")?.as_str().unwrap(),
            ))
        })
        .collect();
    assert_eq!(
        groups,
        [
            ("^feat", "Features"),
            ("^fix", "Fixes"),
            ("^perf", "Performance"),
            ("^refactor", "Refactoring"),
            ("^docs", "Documentation"),
            ("^(ci|build)", "Build and CI"),
            ("^test", "Tests"),
            ("^chore", "Maintenance"),
        ]
    );
}

#[test]
fn the_changelog_has_the_configured_header_and_a_section_for_the_workspace_version() {
    let config = release_plz();
    let header = string(table(&config, "changelog"), "header");
    let changelog = read("CHANGELOG.md");
    // release-plz keeps the header only while it is identical to the configured one.
    assert!(
        changelog.starts_with(header),
        "CHANGELOG.md starts with the configured header"
    );
    let heading = format!("## [{}] - ", workspace_version());
    let line = changelog
        .lines()
        .find(|line| line.starts_with(&heading))
        .unwrap_or_else(|| panic!("CHANGELOG.md has no `{heading}YYYY-MM-DD` section"));
    let date = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap();
    assert!(date.is_match(&line[heading.len()..]), "{line}");
}

/// The lines of a workflow's top-level `key:` block, trimmed, without blanks and comments.
fn block<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    let start = format!("{key}:");
    text.lines()
        .skip_while(|line| *line != start)
        .skip(1)
        .take_while(|line| line.is_empty() || line.starts_with(' ') || line.starts_with('#'))
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// The lines of the job `name` in a workflow's `jobs:` block, trimmed and without a step's
/// leading `- `.
fn job<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let start = format!("  {name}:");
    text.lines()
        .skip_while(|line| *line != start)
        .skip(1)
        .take_while(|line| line.is_empty() || line.starts_with("    "))
        .map(|line| line.trim().trim_start_matches("- "))
        .filter(|line| !line.is_empty())
        .collect()
}

#[test]
fn the_release_workflow_runs_release_then_release_pr_with_the_pinned_version() {
    let text = read(".github/workflows/release-plz.yml");
    assert_eq!(
        block(&text, "on"),
        ["push:", "branches: [main]", "workflow_dispatch:"]
    );
    for (name, command) in [("release", "release"), ("release-pr", "release-pr")] {
        let lines = job(&text, name);
        for expected in [
            "if: github.repository == 'rayx-ui/Gpux'",
            "runs-on: ubuntu-24.04",
            "contents: write",
            "cancel-in-progress: false",
            "fetch-depth: 0",
            "token: ${{ secrets.RELEASE_PLZ_TOKEN }}",
            "uses: actions-rust-lang/setup-rust-toolchain@v1",
            "cache: false",
            "uses: release-plz/action@v0.5",
            &format!("command: {command}"),
            // The release-plz version release-plz.toml was proven with (docs/releasing.md).
            "version: \"0.3.172\"",
            "GITHUB_TOKEN: ${{ secrets.RELEASE_PLZ_TOKEN }}",
        ] {
            assert!(
                lines.contains(&expected),
                "job {name}: missing `{expected}`"
            );
        }
        assert_eq!(
            lines.contains(&"needs: release"),
            name == "release-pr",
            "only release-pr waits for release"
        );
        assert_eq!(
            lines.contains(&"pull-requests: write"),
            name == "release-pr",
            "only release-pr writes pull requests"
        );
    }
    let jobs: Vec<&str> = text
        .lines()
        .skip_while(|line| *line != "jobs:")
        .skip(1)
        .filter(|line| line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
        .collect();
    assert_eq!(jobs, ["  release:", "  release-pr:"]);
    let secrets: Vec<&str> = text
        .match_indices("secrets.")
        .map(|(at, _)| &text[at..])
        .collect();
    assert!(!secrets.is_empty());
    for secret in secrets {
        assert!(
            secret.starts_with("secrets.RELEASE_PLZ_TOKEN }}"),
            "{secret}"
        );
    }
    for forbidden in ["run:", "cargo ", "gpux-testkit", "lane"] {
        assert!(
            !text.contains(forbidden),
            "release-plz.yml runs no `{forbidden}`"
        );
    }
}

#[test]
fn only_the_release_workflow_runs_on_pushes() {
    let dir = root().join(".github/workflows");
    let mut workflows: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".yml") || name.ends_with(".yaml"))
        .collect();
    workflows.sort();
    assert!(workflows.len() >= 5, "{workflows:?}");
    for name in workflows {
        let text = read(&format!(".github/workflows/{name}"));
        let on = block(&text, "on");
        assert!(!on.is_empty(), "{name} has an `on:` block");
        let push = on
            .iter()
            .any(|line| *line == "push:" || line.starts_with("push:"));
        assert_eq!(
            push,
            name == "release-plz.yml",
            "{name}: only release-plz.yml has a `push` trigger"
        );
    }
}
