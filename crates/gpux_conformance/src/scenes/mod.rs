//! The scene catalog. Each module groups scenes by area; [`CATALOG`] lists every scene once.

pub mod common;
pub mod editing;
pub mod effects;
pub mod focus;
pub mod interaction;
pub mod layout;
pub mod lists;
pub mod paint;
pub mod scrolling;
pub mod semantics;
pub mod text;

use crate::scene::SceneSpec;

/// Every conformance scene, in a stable order.
pub static CATALOG: &[SceneSpec] = &[
    layout::FLEX,
    layout::GRID,
    layout::ABSOLUTE,
    layout::OVERFLOW,
    layout::FIXED_STICKY,
    text::SHAPING,
    text::RUNS,
    text::DECORATIONS,
    text::WRAPPING,
    text::TRUNCATION,
    text::INTERACTIVE,
    editing::EDITABLE,
    editing::IME,
    lists::UNIFORM_LIST,
    lists::LIST,
    scrolling::WHEEL,
    scrolling::NESTED,
    scrolling::SCROLLBAR,
    scrolling::PROGRAMMATIC,
    scrolling::TOUCH,
    focus::TAB_ORDER,
    focus::ACTIONS,
    focus::HOVER_ACTIVE,
    interaction::DRAG_DROP,
    interaction::TOOLTIP,
    interaction::ANCHORED,
    interaction::DEFERRED,
    paint::IMAGE,
    paint::SVG,
    paint::CANVAS,
    paint::BORDERS,
    paint::SHADOWS_GRADIENTS,
    paint::OPACITY_CLIPPING,
    paint::SRGB,
    effects::SHADER_EFFECTS,
    effects::ANIMATED_FRAME,
    effects::TRANSITION_FRAME,
    semantics::ACCESSIBILITY,
    semantics::CONTROLS,
];
