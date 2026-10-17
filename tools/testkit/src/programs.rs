//! Finding programs on `PATH`, including Windows `.cmd` shims such as `pnpm.cmd` and `npx.cmd`,
//! which `std::process::Command` does not find from a bare name.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The extensions Windows tries when `PATHEXT` is unset.
pub const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// Resolves `name` against the directories of `path`.
///
/// On Windows (`windows`), a name without an extension tries each `pathext` extension in order
/// (so `pnpm` finds `pnpm.cmd`) and never the bare file, which Windows cannot execute; a name with
/// an extension is looked up as given. Elsewhere the name is looked up as given. A name holding a
/// path separator is checked directly instead of searched.
pub fn resolve(name: &str, path: &OsStr, pathext: Option<&str>, windows: bool) -> Option<PathBuf> {
    let candidates = candidates(name, pathext, windows);
    if name.contains('/') || name.contains('\\') {
        return candidates
            .into_iter()
            .map(PathBuf::from)
            .find(|candidate| candidate.is_file());
    }
    std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .find_map(|dir| {
            candidates
                .iter()
                .map(|candidate| dir.join(candidate))
                .find(|candidate| candidate.is_file())
        })
}

fn candidates(name: &str, pathext: Option<&str>, windows: bool) -> Vec<String> {
    if !windows || Path::new(name).extension().is_some() {
        return vec![name.to_string()];
    }
    pathext
        .unwrap_or(DEFAULT_PATHEXT)
        .split(';')
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(|extension| format!("{name}{}", extension.to_ascii_lowercase()))
        .collect()
}
