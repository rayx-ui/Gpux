//! Finding programs on PATH, including Windows `.cmd` shims.

use gpux_testkit::programs::resolve;
use std::ffi::OsString;

fn path_of(dirs: &[&std::path::Path]) -> OsString {
    std::env::join_paths(dirs).unwrap()
}

#[test]
fn windows_names_resolve_to_their_cmd_shims() {
    let dir = tempfile::tempdir().unwrap();
    // npm-style installs put an extensionless shell script next to the `.cmd` shim; Windows can
    // only start the shim.
    std::fs::write(dir.path().join("pnpm"), "#!/bin/sh").unwrap();
    std::fs::write(dir.path().join("pnpm.cmd"), "@echo off").unwrap();
    let path = path_of(&[dir.path()]);

    assert_eq!(
        resolve("pnpm", &path, Some(".COM;.EXE;.BAT;.CMD"), true),
        Some(dir.path().join("pnpm.cmd"))
    );
    // An explicit extension is looked up as given.
    assert_eq!(
        resolve("pnpm.cmd", &path, None, true),
        Some(dir.path().join("pnpm.cmd"))
    );
    // Elsewhere the bare name is the program.
    assert_eq!(
        resolve("pnpm", &path, None, false),
        Some(dir.path().join("pnpm"))
    );
}

#[test]
fn pathext_order_and_path_order_decide_ties() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    std::fs::write(first.path().join("tool.bat"), "").unwrap();
    std::fs::write(second.path().join("tool.exe"), "").unwrap();
    std::fs::write(second.path().join("tool.cmd"), "").unwrap();
    let path = path_of(&[first.path(), second.path()]);

    // The first PATH directory with any match wins, then PATHEXT order within it.
    assert_eq!(
        resolve("tool", &path, Some(".EXE;.BAT;.CMD"), true),
        Some(first.path().join("tool.bat"))
    );
    let only_second = path_of(&[second.path()]);
    assert_eq!(
        resolve("tool", &only_second, Some(".CMD;.EXE"), true),
        Some(second.path().join("tool.cmd"))
    );
    assert_eq!(resolve("missing", &path, None, true), None);
}

#[test]
fn names_with_a_directory_are_checked_directly() {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("xdotool");
    std::fs::write(&program, "").unwrap();
    let name = program.to_str().unwrap();
    assert_eq!(
        resolve(name, &OsString::new(), None, false),
        Some(program.clone())
    );
    assert_eq!(
        resolve(&format!("{name}-missing"), &OsString::new(), None, false),
        None
    );
}
