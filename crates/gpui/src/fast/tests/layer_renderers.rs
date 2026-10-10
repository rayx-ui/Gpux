//! What the renderers' scroll-layer tiles take from the core: the sprites
//! that composite tiles say their textures are premultiplied, and no layer
//! keeps content that names sprite-atlas tiles the renderer may hold
//! something else in: after a frame drawn from scratch, and after the window
//! removed an entry from its atlas.
//!
//! ```text
//! cargo test -p gpux --features test-support --lib fast::tests::layer_renderers::
//! ```

use crate::fast::layers::policy::Decision;
use crate::fast::tests::layer_support::*;
use crate::{
    AnyWindowHandle, LAYER_TILE_PREMULTIPLIED, PolychromeSprite, RenderImage, TestAppContext,
    decode_layer_tile,
};
use std::sync::Arc;

fn record_generation(cx: &mut TestAppContext, window: AnyWindowHandle) -> Option<u64> {
    let id = scroller_id(cx, window);
    with_window(cx, window, |window, _| {
        window
            .fast_layers
            .layers
            .get(&id)
            .and_then(|layer| layer.record.as_ref())
            .map(|record| record.generation)
    })
}

fn tile_sprites(cx: &mut TestAppContext, window: AnyWindowHandle) -> Vec<PolychromeSprite> {
    with_window(cx, window, |window, _| {
        window
            .rendered_frame
            .scene
            .polychrome_sprites
            .iter()
            .filter(|sprite| {
                decode_layer_tile(sprite.tile.texture_id, sprite.tile.tile_id).is_some()
            })
            .copied()
            .collect()
    })
}

/// A tile's texture holds what was drawn into it, premultiplied; the
/// polychrome shaders multiply an atlas texture's colour by its alpha
/// unless the sprite says otherwise.
#[crate::test]
fn tile_sprites_say_their_textures_are_premultiplied(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let window = page(cx, false).into();
    promote(cx, window);
    assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    let sprites = tile_sprites(cx, window);
    assert!(!sprites.is_empty(), "the frame composites tiles");
    for sprite in sprites {
        assert_eq!(sprite.pad, LAYER_TILE_PREMULTIPLIED);
        assert!(!sprite.grayscale);
        assert_eq!(sprite.opacity, 1.);
    }
}

/// A window is drawn from scratch for what no dependency records: a theme
/// read past the bookkeeping, or a renderer that lost its sprite atlas, whose
/// tiles the content a layer kept would still name. No layer keeps content
/// painted before such a frame, and the content it paints next has a
/// generation the renderer holds no tile of.
#[crate::test]
fn a_frame_drawn_from_scratch_leaves_no_layer_content(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let window = page(cx, false).into();
    promote(cx, window);
    assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    let before = record_generation(cx, window).expect("the layer holds content");

    frame_after(cx, window, |cx| {
        with_window(cx, window, |window, _| window.refresh());
    });
    let stats = with_window(cx, window, |window, _| window.fast_frame_stats());
    assert_eq!(
        (stats.layers_composited, stats.layers_repainted),
        (0, 0),
        "a frame drawn from scratch paints the content into the frame"
    );
    assert_eq!(record_generation(cx, window), None);
    assert!(tile_sprites(cx, window).is_empty());
    let id = scroller_id(cx, window);
    assert!(
        with_window(cx, window, |window, _| window
            .fast_layers
            .layers
            .contains_key(&id)),
        "the container keeps its standing"
    );

    // Scrolled again, it is painted into its layer again, afresh.
    let mut decisions = Vec::new();
    for _ in 0..4 {
        decisions.push(scroll(cx, window, -4.));
        if record_generation(cx, window).is_some() {
            break;
        }
    }
    assert_eq!(
        decisions.last(),
        Some(&Some(Decision::Repaint)),
        "{decisions:?}"
    );
    let after = record_generation(cx, window).expect("the layer holds content again");
    assert!(after > before, "generation {after} after {before}");
    assert_eq!(scroll(cx, window, -4.), Some(Decision::Composite));
}

/// The atlas hands a removed entry's slot out again, so an image that
/// replaces another of its size can come back as the very tile the layer's
/// content named the first by: the tiles' hashes would not tell them apart
/// and the renderer would keep the tiles it rasterized. Removing an entry
/// leaves no layer content painted before it; what is painted next has a
/// new generation and every tile dirty.
#[crate::test]
fn removing_an_atlas_entry_leaves_no_layer_content(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    for svg in [false, true] {
        let window = page(cx, false).into();
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        let before = record_generation(cx, window).expect("the layer holds content");

        with_window(cx, window, |window, _| {
            if svg {
                window
                    .drop_svg("icon.svg".into(), crate::size(8.into(), 8.into()))
                    .unwrap();
            } else {
                let image = Arc::new(RenderImage::new(vec![image::Frame::new(
                    image::RgbaImage::new(4, 4),
                )]));
                window.drop_image(image).unwrap();
            }
        });
        assert_eq!(record_generation(cx, window), None);

        let mut decisions = Vec::new();
        for _ in 0..4 {
            decisions.push(scroll(cx, window, -4.));
            if record_generation(cx, window).is_some() {
                break;
            }
        }
        assert_eq!(
            decisions.last(),
            Some(&Some(Decision::Repaint)),
            "{decisions:?}"
        );
        let after = record_generation(cx, window).expect("the layer holds content again");
        assert!(after > before, "generation {after} after {before}");
        let id = scroller_id(cx, window);
        with_window(cx, window, |window, _| {
            let record = window.fast_layers.layers[&id].record.as_ref().unwrap();
            assert_eq!(
                record.dirty_tiles.len(),
                record.tile_hashes.len(),
                "every tile is dirty"
            );
        });
    }
}
