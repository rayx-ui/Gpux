//! Host prerequisites per lane, reported as actionable messages instead of failing mid-run.

use crate::{Environment, Os, lanes::LaneId};

/// The missing prerequisites of `lane` on this host, one actionable message each.
pub fn check(lane: LaneId, environment: &dyn Environment) -> Vec<String> {
    let mut problems = Vec::new();
    if environment.find_program("cargo").is_none() {
        problems.push(
            "`cargo` is not on PATH: install Rust with rustup (https://rustup.rs) and open a new shell"
                .to_string(),
        );
    }
    if lane == LaneId::D && environment.os() == Os::Linux {
        let x11 = environment
            .var("DISPLAY")
            .is_some_and(|value| !value.is_empty());
        let wayland = environment
            .var("WAYLAND_DISPLAY")
            .is_some_and(|value| !value.is_empty());
        if !x11 && !wayland {
            problems.push(
                "lane d opens real windows but neither DISPLAY nor WAYLAND_DISPLAY is set: run it in \
                 a desktop session, or under X11 with `xvfb-run -a gpux-testkit lane d`"
                    .to_string(),
            );
        }
        if x11 && !wayland && environment.find_program("xdotool").is_none() {
            problems.push(
                "lane d sends OS-level input through XTest on X11 and needs `xdotool`: install it \
                 (for example `sudo apt-get install xdotool`)"
                    .to_string(),
            );
        }
    }
    problems
}
