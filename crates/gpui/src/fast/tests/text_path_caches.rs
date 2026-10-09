//! The standalone text and path optimizations: numbers put together from
//! cached glyphs, kept path tessellations, and glyph work done once a run.
//! Each must give exactly what the work it saves gives.

use std::{borrow::Cow, sync::atomic::AtomicUsize};

use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

use crate::{
    Bounds, DevicePixels, Font, FontId, FontMetrics, FontRun, GlyphId, LineLayout, Pixels,
    PlatformTextSystem, RenderGlyphParams, Result, ShapedGlyph, ShapedRun, Size, TextRenderingMode,
    fast::number_shaping::NumberShaping, point, px, size,
};

/// A platform text system with kerning between some digit pairs, a
/// ligature for `--`, and digits that shape wider after a `%` anywhere in
/// the line: what a font can do to numbers beyond setting glyphs side by
/// side. Positions are summed in `f32`, glyph by glyph, as shaping engines
/// report them.
struct KerningTextSystem {
    /// Lines the platform was asked to shape.
    shaped: AtomicUsize,
    /// Whether a `%` widens the digits of its line.
    contextual: bool,
}

impl KerningTextSystem {
    fn new(contextual: bool) -> Self {
        Self {
            shaped: AtomicUsize::new(0),
            contextual,
        }
    }

    fn advance(ch: char, font_size: Pixels, widened: bool) -> f32 {
        let base = match ch {
            '0'..='9' => 7.0 + (ch as u32 - '0' as u32) as f32 % 3. * 0.5,
            '.' | ',' => 3.25,
            '-' | '+' => 4.5,
            '%' | '$' => 8.0,
            _ => 6.0,
        };
        base * font_size.0 / 14. + if widened { 0.375 } else { 0. }
    }

    fn kerning(left: char, right: char, font_size: Pixels) -> f32 {
        let pixels = match (left, right) {
            ('1', '1') => -0.75,
            ('7', '.') | ('7', ',') => -1.25,
            ('4', '1') => -0.5,
            _ => 0.,
        };
        pixels * font_size.0 / 14.
    }
}

impl PlatformTextSystem for KerningTextSystem {
    fn add_fonts(&self, _: Vec<Cow<'static, [u8]>>) -> Result<()> {
        Ok(())
    }

    fn all_font_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn font_id(&self, _: &Font) -> Result<FontId> {
        Ok(FontId(1))
    }

    fn font_metrics(&self, _: FontId) -> FontMetrics {
        crate::NoopTextSystem.font_metrics(FontId(1))
    }

    fn typographic_bounds(&self, _: FontId, _: GlyphId) -> Result<Bounds<f32>> {
        Ok(Bounds::default())
    }

    fn advance(&self, _: FontId, _: GlyphId) -> Result<Size<f32>> {
        Ok(size(0., 0.))
    }

    fn glyph_for_char(&self, _: FontId, ch: char) -> Option<GlyphId> {
        Some(GlyphId(ch as u32))
    }

    fn glyph_raster_bounds(&self, _: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        Ok(Bounds::default())
    }

    fn rasterize_glyph(
        &self,
        _: &RenderGlyphParams,
        bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        Ok((bounds.size, Vec::new()))
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
        self.shaped
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let widened = self.contextual && text.contains('%');
        let font_id = runs.first().map_or(FontId(1), |run| run.font_id);
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut glyphs = Vec::new();
        let mut x = 0f32;
        let mut ix = 0;
        while ix < chars.len() {
            let (index, ch) = chars[ix];
            // `--` is one glyph.
            let (id, len, advance) =
                if ch == '-' && chars.get(ix + 1).is_some_and(|(_, next)| *next == '-') {
                    (0x2014, 2, Self::advance('-', font_size, widened) * 1.5)
                } else {
                    (ch as u32, 1, Self::advance(ch, font_size, widened))
                };
            glyphs.push(ShapedGlyph {
                id: GlyphId(id),
                position: point(px(x), px(0.)),
                index,
                is_emoji: false,
            });
            let last = chars[ix + len - 1].1;
            let kerning = chars
                .get(ix + len)
                .map_or(0., |(_, next)| Self::kerning(last, *next, font_size));
            x += advance + kerning;
            ix += len;
        }
        let width = x;
        LineLayout {
            font_size,
            letter_spacing: px(0.),
            width: px(width),
            ascent: px(font_size.0 * 0.8),
            descent: px(font_size.0 * 0.2),
            runs: vec![ShapedRun { font_id, glyphs }],
            len: text.len(),
        }
    }

    fn recommended_rendering_mode(&self, _: FontId, _: Pixels) -> TextRenderingMode {
        TextRenderingMode::Grayscale
    }
}

fn runs(text: &str) -> Vec<FontRun> {
    vec![FontRun {
        len: text.len(),
        font_id: FontId(1),
    }]
}

fn random_number(rng: &mut StdRng) -> String {
    let characters = b"0123456789.,+-%$KMBT";
    (0..rng.random_range(1..16))
        .map(|_| characters[rng.random_range(0..characters.len())] as char)
        .collect()
}

fn same(ours: &LineLayout, theirs: &LineLayout) -> bool {
    ours.len == theirs.len
        && ours.width == theirs.width
        && ours.ascent == theirs.ascent
        && ours.descent == theirs.descent
        && ours.runs.len() == theirs.runs.len()
        && ours.runs.iter().zip(&theirs.runs).all(|(ours, theirs)| {
            ours.font_id == theirs.font_id
                && ours.glyphs.len() == theirs.glyphs.len()
                && ours
                    .glyphs
                    .iter()
                    .zip(&theirs.glyphs)
                    .all(|(a, b)| a.id == b.id && a.index == b.index && a.position == b.position)
        })
}

/// Numbers put together from cached glyphs and kerning have exactly the
/// glyphs and positions the platform shapes them with, in every size, and
/// the platform is asked far less once the characters are measured.
#[test]
fn numbers_put_together_equal_the_platform_shaping_them() {
    let platform = KerningTextSystem::new(false);
    let reference = KerningTextSystem::new(false);
    let mut numbers = NumberShaping::default();
    let mut rng = StdRng::seed_from_u64(1);
    let mut put_together = 0;
    for _ in 0..2000 {
        let text = random_number(&mut rng);
        let font_size = px([14., 17.5, 21.][rng.random_range(0..3)]);
        let expected = reference.layout_line(&text, font_size, &runs(&text));
        match numbers.shape(&platform, 0, &text, font_size, &runs(&text)) {
            Some(layout) => {
                assert!(same(&layout, &expected), "{text:?} at {font_size:?}");
                put_together += 1;
            }
            None => {}
        }
    }
    assert!(
        put_together > 1000,
        "only {put_together} numbers were put together"
    );
    let asked = platform.shaped.load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        asked < 2000,
        "the platform was asked {asked} times for 2000 numbers"
    );
}

/// What numbers are not: text with other characters, several runs, and
/// pairs the platform shapes as something else than two glyphs (a ligature)
/// are left to the platform.
#[test]
fn what_is_not_a_plain_number_is_left_to_the_platform() {
    let platform = KerningTextSystem::new(false);
    let mut numbers = NumberShaping::default();
    for text in ["12 apples", "x1", "", "١٢٣"] {
        assert!(
            numbers
                .shape(&platform, 0, text, px(14.), &runs(text))
                .is_none(),
            "{text:?}"
        );
    }
    let two_runs = [
        FontRun {
            len: 1,
            font_id: FontId(1),
        },
        FontRun {
            len: 1,
            font_id: FontId(2),
        },
    ];
    assert!(
        numbers
            .shape(&platform, 0, "12", px(14.), &two_runs)
            .is_none()
    );
    // `--` is a ligature: lines holding it are not put together, and what
    // they would give is the platform's.
    let line = numbers.shape(&platform, 0, "1--2", px(14.), &runs("1--2"));
    assert!(line.is_none());
}

/// A font whose numbers shape differently in context is found out by the
/// checks of the first lines, and left to the platform from then on: every
/// line returned is the platform's.
#[test]
fn a_font_that_shapes_numbers_in_context_is_left_to_the_platform() {
    let platform = KerningTextSystem::new(true);
    let reference = KerningTextSystem::new(true);
    let mut numbers = NumberShaping::default();
    let mut rng = StdRng::seed_from_u64(2);
    for _ in 0..500 {
        let text = random_number(&mut rng);
        let expected = reference.layout_line(&text, px(14.), &runs(&text));
        if let Some(layout) = numbers.shape(&platform, 0, &text, px(14.), &runs(&text)) {
            assert!(same(&layout, &expected), "{text:?}");
        }
    }
    // Once found out, nothing more is put together.
    assert!(
        numbers
            .shape(&platform, 0, "123", px(14.), &runs("123"))
            .is_none()
    );
    // A new font generation measures the font again.
    let line = numbers.shape(&platform, 1, "123", px(14.), &runs("123"));
    assert!(
        line.is_some_and(|line| same(&line, &reference.layout_line("123", px(14.), &runs("123"))))
    );
}

mod glyphs {
    use crate::{
        AppContext as _, Color, Context, FontId, GlyphId, Render, TransformationMatrix, Window,
        canvas, div, fast::glyphs::LineGlyphPainter, hsla, point, prelude::*, px,
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
                                                let origin =
                                                    point(px(x), px(line as f32 * 20.3 + 12.));
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
        let mut cx = super::super::oracle_support::oracle_app(0);
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
        let run = super::super::oracle_support::run_lockstep(31, 30, crate::FastMode::OFF);
        assert!(run.incremental.cached_views_replayed > 0);
    }
}
