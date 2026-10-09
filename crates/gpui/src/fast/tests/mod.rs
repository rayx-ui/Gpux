//! Tests of what fast mode adds, kept out of the test modules of the files
//! it hooks into. Each group is selected on its own with
//! `cargo test -p gpux --features test-support --lib fast::tests::<group>::`.

mod bounds_grid;
mod dependencies;
mod dependency_nesting;
mod draw_order;
mod glyph_runs;
mod hover_dependencies;
mod layout_keys;
mod oracle;
mod oracle_layout;
mod oracle_support;
mod scoped_invalidation;
mod stats;
mod switches;
mod text_carry;
mod write_elision;
