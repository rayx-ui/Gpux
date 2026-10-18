//! How the documentation tells consumers to depend on gpux: the README crate table lists exactly
//! the workspace's packages (directory, package and Rust name as the manifests say), and the
//! install documents show only the release-tag form (`git = ".../Gpux", tag = "vX.Y.Z"`), never a
//! crates.io version or a `../Gpux` path.

use std::path::{Path, PathBuf};

use toml::{Table, Value};

const GIT: &str = r#"git = "https://github.com/rayx-ui/Gpux", tag = "v"#;

/// The documents that show consumers a gpux dependency.
const INSTALL_DOCS: [&str; 4] = [
    "README.md",
    "docs/installation-and-usage.md",
    ".claude/skills/gpux/references/installation-and-usage.md",
    ".claude/skills/gpux/references/multithreaded-wasm-apps.md",
];

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

/// `(directory, package, Rust name)` of every workspace member, from the manifests.
fn members() -> Vec<(String, String, String)> {
    let manifest = parse("Cargo.toml");
    manifest["workspace"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| {
            let dir = member.as_str().unwrap().to_string();
            let manifest = parse(&format!("{dir}/Cargo.toml"));
            let package = manifest["package"]["name"].as_str().unwrap().to_string();
            let rust_name = manifest
                .get("lib")
                .and_then(|lib| lib.get("name"))
                .and_then(Value::as_str)
                .map_or_else(|| package.replace('-', "_"), String::from);
            (dir, package, rust_name)
        })
        .collect()
}

/// The README crate table's rows: `(directory, package, Rust name)` cells without backticks.
fn crate_table() -> Vec<(String, String, String)> {
    let readme = read("README.md");
    let rows: Vec<(String, String, String)> = readme
        .lines()
        .skip_while(|line| !line.starts_with("| Directory | Package | Rust name |"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<String> = line
                .split('|')
                .map(|cell| cell.trim().replace('`', ""))
                .collect();
            (cells[1].clone(), cells[2].clone(), cells[3].clone())
        })
        .collect();
    assert!(!rows.is_empty(), "README.md has a crate table");
    rows
}

#[test]
fn the_crate_table_lists_exactly_the_workspace_packages() {
    let table = crate_table();
    let mut documented: Vec<&(String, String, String)> = table
        .iter()
        .filter(|(dir, ..)| !dir.starts_with("vendor/"))
        .collect();
    documented.sort();
    let members = members();
    let mut own: Vec<&(String, String, String)> = members
        .iter()
        .filter(|(dir, ..)| !dir.starts_with("vendor/"))
        .collect();
    own.sort();
    assert_eq!(
        documented, own,
        "the README crate table's rows equal the own workspace members (directory, package, \
         Rust name)"
    );

    // The fork rows name every fork package, the wgpu core-deps crates through their wildcard.
    let fork_cells: Vec<&str> = table
        .iter()
        .filter(|(dir, ..)| dir.starts_with("vendor/"))
        .flat_map(|(_, packages, _)| packages.split(','))
        .map(str::trim)
        .collect();
    let forks: Vec<&str> = members
        .iter()
        .filter(|(dir, ..)| dir.starts_with("vendor/"))
        .map(|(_, package, _)| package.as_str())
        .collect();
    assert_eq!(forks.len(), 8, "{forks:?}");
    for fork in forks {
        assert!(
            fork_cells.iter().any(|cell| match cell.strip_suffix('*') {
                Some(prefix) => fork.starts_with(prefix),
                None => *cell == fork,
            }),
            "the README's fork rows name {fork}: {fork_cells:?}"
        );
    }
}

#[test]
fn the_install_documents_show_only_the_release_tag_form() {
    // A dependency keyed by a package name (`gpux-web = ...`) is the crates.io form: the git-tag
    // form keys every package but `gpux` by its Rust name with `package = "gpux-..."`. Features
    // are prefixed with that key too, never with a package name (`gpux-platform/font-kit`).
    let package_key = regex::Regex::new(r"^\s*gpux-[a-z0-9-]+\s*=").unwrap();
    let package_feature = regex::Regex::new(r"`gpux-[a-z0-9-]+/").unwrap();
    for doc in INSTALL_DOCS {
        let text = read(doc);
        assert!(
            text.contains(GIT),
            "{doc} shows the release-tag form `{GIT}X.Y.Z\"`"
        );
        for (number, line) in text.lines().enumerate() {
            let line_number = number + 1;
            for forbidden in [
                "../Gpux",
                r#"gpux = ""#,
                r#"version = "0.3"#,
                "gpux = { version",
            ] {
                assert!(
                    !line.contains(forbidden),
                    "{doc}:{line_number} shows a crates.io or path form (`{forbidden}`): {line}"
                );
            }
            assert!(
                !package_key.is_match(line),
                "{doc}:{line_number} keys a dependency by its package name (the crates.io form): \
                 {line}"
            );
            assert!(
                !package_feature.is_match(line),
                "{doc}:{line_number} prefixes a feature with a package name, not the dependency \
                 key: {line}"
            );
            // Every gpux dependency line uses the release tag.
            let dependency = line.starts_with("gpux = ")
                || line.starts_with("gpux_")
                || (line.starts_with("gpui_") && line.contains(r#"package = "gpux-"#));
            if dependency {
                assert!(line.contains(GIT), "{doc}:{line_number}: {line}");
            }
        }
    }
}

#[test]
fn the_installation_guides_show_test_support_dev_dependencies_by_tag() {
    let gpux = regex::Regex::new(
        r#"(?m)^gpux = \{ git = "https://github\.com/rayx-ui/Gpux", tag = "v[0-9.]+", features = \["test-support"\] \}$"#,
    )
    .unwrap();
    let platform = regex::Regex::new(
        r#"(?m)^gpui_platform = \{ package = "gpux-platform", git = "https://github\.com/rayx-ui/Gpux", tag = "v[0-9.]+", features = \["test-support"\] \}$"#,
    )
    .unwrap();
    for doc in [
        "docs/installation-and-usage.md",
        ".claude/skills/gpux/references/installation-and-usage.md",
    ] {
        let text = read(doc);
        let dev = text
            .split("[dev-dependencies]")
            .nth(1)
            .unwrap_or_else(|| panic!("{doc} shows [dev-dependencies]"));
        assert!(gpux.is_match(dev), "{doc}: gpux test-support by tag");
        assert!(
            platform.is_match(dev),
            "{doc}: gpui_platform test-support by tag"
        );
    }
}

#[test]
fn the_install_documents_name_the_rayx_release_apps_need() {
    for doc in [
        "README.md",
        "docs/installation-and-usage.md",
        ".claude/skills/gpux/references/installation-and-usage.md",
        ".claude/skills/gpux/references/multithreaded-wasm-apps.md",
    ] {
        let text = read(doc).split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            text.contains("rayx 1.4.0 or later"),
            "{doc} says building apps with rayx needs rayx 1.4.0 or later"
        );
    }
}
