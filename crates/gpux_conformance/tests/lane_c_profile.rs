//! The test build is optimized (`[profile.test]` sets `opt-level = 1` so lane C's pixel suites
//! finish in seconds) but keeps its safety checks: a lane that passes only because assertions
//! were compiled out would prove nothing.
//!
//! ```text
//! cargo test -p gpux-conformance --test lane_c_profile
//! ```

#[test]
fn debug_assertions_are_on_in_the_lane_c_test_build() {
    assert!(
        cfg!(debug_assertions),
        "the test profile must keep `debug-assertions = true`"
    );
}

#[test]
#[should_panic(expected = "overflow")]
fn integer_overflow_panics_in_the_lane_c_test_build() {
    let largest = std::hint::black_box(u8::MAX);
    let one = std::hint::black_box(1u8);
    // With `overflow-checks = false` this wraps to 0 and the test fails for not panicking.
    let sum = largest + one;
    std::hint::black_box(sum);
}
