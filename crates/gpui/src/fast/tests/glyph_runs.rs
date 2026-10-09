//! The standalone text optimization kept: glyph work done once a run, which
//! must give exactly what painting each glyph on its own gives.
//!
//! Number shaping and the path tessellation cache were measured and reverted
//! (see `docs/fast-mode.md`).

use crate::{
    AppContext as _, Color, Context, FontId, GlyphId, Render, TransformationMatrix, Window, canvas,
    div, fast::glyphs::LineGlyphPainter, hsla, point, prelude::*, px,
};
use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

/// Paints a seeded sequence of glyph runs, per glyph through
/// `Window::paint_glyph` or a run at a time through `LineGlyphPainter`,
/// under an opacity, a transform and a clip that change between lines.
struct Glyphs {
    per_run: bool,
    seed: u64,
}

impl Render for Glyphs {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (per_run, seed) = (self.per_run, self.seed);
        div().size_full().child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let mut rng = StdRng::seed_from_u64(seed);
                    // Colors equal in value but separate, so a run is
                    // told by its colour's value.
                    let colors = |rng: &mut StdRng| -> Color {
                        [
                            hsla(0., 0., 0.1, 1.),
                            hsla(0.6, 0.7, 0.5, 1.),
                            hsla(0.1, 0.9, 0.5, 0.5),
                        ][rng.random_range(0..3)]
                        .into()
                    };
                    for line in 0..12 {
                        let opacity = [1.0, 0.5, 0.25][line % 3];
                        let transform = (line % 4 == 3)
                            .then(|| TransformationMatrix::unit().rotate(crate::radians(0.1)));
                        let mask = crate::ContentMask {
                            bounds: crate::Bounds::new(
                                bounds.origin,
                                crate::size(px(100. + line as f32 * 40.), px(400.)),
                            ),
                            ..Default::default()
                        };
                        window.with_content_mask(Some(mask), |window| {
                            window.with_element_opacity(Some(opacity), |window| {
                                window.with_element_transform(transform, |window| {
                                    let mut painter = LineGlyphPainter::new(window);
                                    let mut x = 0.;
                                    for _run in 0..rng.random_range(1..4) {
                                        let font_id = FontId(rng.random_range(0..2));
                                        let font_size = px([12., 14.5][rng.random_range(0..2)]);
                                        let color = colors(&mut rng);
                                        for _ in 0..rng.random_range(1..12) {
                                            let origin = point(px(x), px(line as f32 * 20.3 + 12.));
                                            let glyph_id = GlyphId(rng.random_range(33..90));
                                            if per_run {
                                                painter
                                                    .paint_glyph(
                                                        window, origin, font_id, glyph_id,
                                                        font_size, &color,
                                                    )
                                                    .unwrap();
                                            } else {
                                                window
                                                    .paint_glyph(
                                                        origin,
                                                        font_id,
                                                        glyph_id,
                                                        font_size,
                                                        color.clone(),
                                                    )
                                                    .unwrap();
                                            }
                                            x += 7.37;
                                        }
                                    }
                                })
                            })
                        });
                    }
                },
            )
            .size_full(),
        )
    }
}

fn sprites(per_run: bool, seed: u64, frames: usize) -> Vec<String> {
    let mut cx = super::oracle_support::oracle_app(0);
    let window = cx.add_window(move |_, _| Glyphs { per_run, seed });
    let mut lines = Vec::new();
    for _ in 0..frames {
        lines = cx
            .update_window(window.into(), |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
                let scene = &window.rendered_frame.scene;
                scene
                    .monochrome_sprites
                    .iter()
                    .map(|sprite| format!("{sprite:?}"))
                    .chain(
                        scene
                            .subpixel_sprites
                            .iter()
                            .map(|sprite| format!("{sprite:?}")),
                    )
                    .collect()
            })
            .unwrap();
    }
    lines
}

/// Glyphs painted a run at a time are exactly the sprites painting each
/// on its own gives, tiles included, on the first frame and on frames
/// that find their raster bounds kept.
#[test]
fn glyph_runs_paint_what_painting_each_glyph_paints() {
    for seed in 0..12 {
        for frames in [1, 3] {
            let per_glyph = sprites(false, seed, frames);
            assert!(!per_glyph.is_empty());
            assert_eq!(
                sprites(true, seed, frames),
                per_glyph,
                "seed {seed}, {frames} frames"
            );
        }
    }
}

/// Text painted by lines draws what it drew before the per-run painter,
/// whatever the window kept: the oracle's cached views agree with frames
/// drawn from scratch, which forget every kept glyph.
#[test]
fn lines_painted_by_runs_match_frames_drawn_from_scratch() {
    let run = super::oracle_support::run_lockstep(31, 30, crate::FastMode::OFF);
    assert!(run.incremental.cached_views_replayed > 0);
}
