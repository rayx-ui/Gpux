//! Reading results from the output of each lane's host: libtest, a conformance host's stdout,
//! Android logcat and Playwright's list reporter.

use gpux_testkit::results::{
    Tally, result_lines, tally_libtest, tally_playwright_list, tally_result_lines,
};
use gpux_tests_conformance::report::Outcome;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn libtest_summaries_add_up_across_test_binaries() {
    let tally = tally_libtest(&fixture("libtest_lane_a.txt"));
    assert_eq!((tally.passed, tally.failed, tally.skipped), (5, 1, 1));
    assert!(
        tally.failures.contains(&"list_variable".to_string()),
        "{tally:?}"
    );
    // The failed scene's own result line names the failing step.
    assert!(
        tally
            .failures
            .iter()
            .any(|failure| failure.starts_with("list-variable step 5")),
        "{tally:?}"
    );
}

#[test]
fn conformance_host_stdout_counts_each_scene_once() {
    let tally = tally_result_lines(&fixture("desktop_host_stdout.txt"));
    assert_eq!((tally.passed, tally.failed, tally.skipped), (3, 1, 1));
    assert_eq!(tally.failures.len(), 1);
    assert!(
        tally.failures[0].starts_with("paint-borders step 6"),
        "{tally:?}"
    );
}

#[test]
fn logcat_prefixes_do_not_hide_result_lines() {
    let output = fixture("android_logcat.txt");
    let lines = result_lines(&output);
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[3].outcome, Outcome::Fail);
    assert_eq!(lines[3].scene, "scroll-touch");
    let tally = tally_result_lines(&output);
    assert_eq!((tally.passed, tally.failed, tally.skipped), (2, 1, 1));
}

#[test]
fn playwright_list_reporter_counts_come_from_its_closing_lines() {
    let tally = tally_playwright_list(&fixture("playwright_list.txt"));
    assert_eq!((tally.passed, tally.failed, tally.skipped), (2, 1, 1));
    assert_eq!(tally.failures.len(), 1);
    assert!(
        tally.failures[0].contains("@pr clicks a located button"),
        "{tally:?}"
    );
}

#[test]
fn interrupted_playwright_runs_count_each_test_by_its_last_attempt() {
    // No closing counts; the retried test passed on its second attempt.
    let tally = tally_playwright_list(&fixture("playwright_list_interrupted.txt"));
    assert_eq!(
        (tally.passed, tally.failed, tally.skipped),
        (2, 0, 0),
        "{tally:?}"
    );
}

#[test]
fn tallies_add_and_count_failures() {
    let mut tally = Tally {
        passed: 2,
        ..Tally::default()
    };
    tally.add(Tally::failure("lane b: not available"));
    assert_eq!(tally.total(), 3);
    assert_eq!(tally.failures, vec!["lane b: not available".to_string()]);
    assert_eq!(tally_result_lines("no results here").total(), 0);
}
