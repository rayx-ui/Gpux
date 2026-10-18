//! Tests of what fast mode adds, kept out of the test modules of the files
//! it hooks into. Each group is selected on its own with
//! `cargo test -p gpux --features test-support --lib fast::tests::<group>::`.

mod a11y_describe;
mod a11y_retention;
mod border_clips;
mod bounds_grid;
mod composition;
mod composition_overlay;
mod defaults;
mod dependencies;
mod dependency_nesting;
mod draw_order;
mod edge_shadows;
mod glyph_runs;
mod half_pixel_edges;
mod hover_dependencies;
mod layer_a11y_scroll_targets;
mod layer_carry;
mod layer_content_layout;
mod layer_extent;
mod layer_inputs;
mod layer_memory;
mod layer_owner_replay;
mod layer_policy;
mod layer_renderers;
mod layer_support;
mod layer_touch;
mod layout_keys;
mod list_hover;
mod list_layers;
mod oracle;
mod oracle_layers;
mod oracle_layout;
mod oracle_measured_leaves;
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
mod transformed_masks;
mod uniform_measured_item;
#[cfg(feature = "diagnostics")]
mod verify_mode;
mod view_invalidation;
mod view_support;
mod virtual_list_layers;
mod write_elision;
