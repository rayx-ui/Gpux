//! Tests of what fast mode adds, kept out of the test modules of the files
//! it hooks into. Each group is selected on its own with
//! `cargo test -p gpux --features test-support --lib fast::tests::<group>::`.

mod bounds_grid;
mod composition;
mod composition_overlay;
mod dependencies;
mod dependency_nesting;
mod draw_order;
mod edge_shadows;
mod glyph_runs;
mod hover_dependencies;
mod layer_carry;
mod layer_extent;
mod layer_inputs;
mod layer_policy;
mod layer_renderers;
mod layer_support;
mod layer_touch;
mod layout_keys;
mod list_layers;
mod oracle;
mod oracle_layers;
mod oracle_layout;
mod oracle_splice;
mod oracle_support;
mod oracle_views;
mod own_dirty;
mod retained_records;
mod reuse_paths;
mod scoped_invalidation;
mod splice;
mod stats;
mod switches;
mod text_carry;
#[cfg(feature = "diagnostics")]
mod verify_mode;
mod view_invalidation;
mod view_support;
mod write_elision;
