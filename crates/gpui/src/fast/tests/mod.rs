//! Tests of what fast mode adds, kept out of the test modules of the files
//! it hooks into. Each group is selected on its own with
//! `cargo test -p gpux --features test-support --lib fast::tests::<group>::`.

mod oracle;
mod oracle_support;
mod stats;
mod switches;
