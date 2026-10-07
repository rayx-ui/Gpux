//! `gpux::bench_main!` runs benchmarks on a thread with `BENCH_STACK_BYTES` of stack, the 8 MiB
//! Linux and macOS give their main thread: Windows gives its main thread 1 MiB, which deep element
//! trees (the `tall` family of `gpux-platform`'s `randomized_element_tree`) overflowed.
#![cfg(feature = "bench-support")]

use gpux::{BENCH_STACK_BYTES, run_with_bench_stack};

/// Recurses until at least `bytes` of stack lie between `top` and the deepest frame, whatever
/// the build's frame sizes, and returns how deep it went.
#[inline(never)]
fn use_stack(top: usize, bytes: usize) -> usize {
    let frame = std::hint::black_box([0u8; 1024]);
    if top.abs_diff(frame.as_ptr() as usize) >= bytes {
        return 1;
    }
    1 + use_stack(top, bytes) + usize::from(frame[0])
}

#[test]
fn benchmarks_run_with_eight_mib_of_stack() {
    assert_eq!(BENCH_STACK_BYTES, 8 * 1024 * 1024);
    // 5 MiB: more than a test thread's 2 MiB or a Windows main thread's 1 MiB.
    run_with_bench_stack(|| {
        let top = std::hint::black_box(0u8);
        let depth = use_stack(&top as *const u8 as usize, 5 * 1024 * 1024);
        assert!(depth > 1, "{depth}");
    });
}

#[test]
#[should_panic(expected = "the benchmark failed")]
fn a_panicking_benchmark_fails_the_run() {
    run_with_bench_stack(|| panic!("the benchmark failed"));
}
