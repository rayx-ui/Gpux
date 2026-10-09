//! Tests of what fast mode adds, kept out of the test modules of the files
//! it hooks into. Each group is selected on its own with
//! `cargo test -p gpux --features test-support --lib fast::tests::<group>::`.

mod bounds_grid;
mod oracle;
mod oracle_support;
mod scoped_invalidation;
mod stats;
mod switches;
