//! A border's four quads, painted in one clip told to the scene.
//!
//! `Style::paint` paints a visible border as four quads of the element's
//! bounds (top, right, bottom, left), each in the content mask of one side's
//! strip, so that each side is drawn once and the corners are not blended
//! twice. Painted as upstream does, each strip is a clip pushed through
//! `Window::with_content_mask`, and the scene is told each (see
//! `crate::fast::edge_shadows`): four clips, eight paint operations, four
//! records and four sets of entries of one entry each for every bordered
//! element, which the eighth review of the edge shadows measured at a
//! fifth of the time to build a page of bordered rows.
//!
//! Here the scene is told one clip, [`ClipOp::Border`], for all four, with the
//! strips as the masks of its quads. The window's own content masks are
//! what they were: each quad's mask is its strip intersected with what is
//! around it, and so are the bounds the scene's tree holds, the draw orders
//! and the batches, and the pixels. What the scene knows of the strips is
//! that a quad of the border is painted in one of them, which is all
//! `edge_shadows` asks of a clip it was told of: a quad's bounds are taken
//! within its strip, as with the strip a clip of its own, when the clips
//! around are lifted.
//!
//! gpux's own: upstream and gpui-fast have no told clips.

use crate::{Bounds, ContentMask, PaintQuad, Pixels, Window};

/// Paints the four quads of a border: `quad` in each of the `strips`, in the
/// order they are given, inside one clip of `bounds` told to the scene.
pub(crate) fn paint(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    strips: [Bounds<Pixels>; 4],
    quad: PaintQuad,
) {
    let painting = window.is_paint_phase();
    if painting {
        crate::fast::edge_shadows::border(window, bounds, &strips);
    }
    for strip in strips {
        // The scene was told the strips with the clip.
        window.with_untold_content_mask(Some(ContentMask::rectangular(strip)), |window| {
            window.paint_quad(quad.clone());
        });
    }
    if painting {
        crate::fast::edge_shadows::pop_clip(window);
    }
}
