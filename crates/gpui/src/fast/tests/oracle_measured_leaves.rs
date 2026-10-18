//! The fast-mode oracle over measured leaves other than text in retained
//! views: a measured custom element as a scroll container's content and as a
//! flex child, and a `uniform_list` sized by `ListSizingBehavior::Infer`, each
//! in a child view of its own, beside a plain view. Random histories of
//! notifications elsewhere, resizes and wheel turns draw each frame
//! incrementally and from scratch, and the whole frame descriptions must be
//! equal.
//!
//! ```text
//! cargo test -p gpux --features test-support --lib fast::tests::oracle_measured_leaves
//! ```

use crate::fast::tests::oracle_support::{oracle_app, seeds_and_steps};
use crate::{
    App, AppContext as _, AvailableSpace, Bounds, Context, Element, ElementId, Entity, FastMode,
    GlobalElementId, InspectorElementId, InteractiveElement as _, IntoElement, LayoutId,
    ListSizingBehavior, ParentElement as _, Pixels, PlatformInput, Render, ScrollDelta,
    ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement as _, Style, Styled as _,
    TestAppContext, TouchPhase, Window, WindowHandle, div, fill, hsla, point, px, size,
};
use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

/// A measured element as tall as `per_width` gives for the width it gets.
struct Measured {
    per_width: f32,
}

impl IntoElement for Measured {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Measured {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, ()) {
        let per_width = self.per_width;
        let layout =
            window.request_measured_layout(Style::default(), move |known, available, _, _| {
                let width = known.width.unwrap_or(match available.width {
                    AvailableSpace::Definite(width) => width,
                    _ => px(120.),
                });
                size(
                    width,
                    known.height.unwrap_or(px(per_width / width.0.max(1.))),
                )
            });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let mut top = bounds.top();
        let mut band = 0;
        while top < bounds.bottom() {
            window.paint_quad(fill(
                Bounds::new(point(bounds.left(), top), size(bounds.size.width, px(50.))),
                hsla((band % 5) as f32 / 5., 0.5, 0.5, 1.),
            ));
            top += px(50.);
            band += 1;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Measured,
    InferredList,
    Plain,
}

struct Child {
    kind: Kind,
    /// Changed by a notification of this view alone.
    count: usize,
}

impl Render for Child {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        match self.kind {
            Kind::Measured => Measured {
                per_width: 400_000. + self.count as f32 * 20_000.,
            }
            .into_any_element(),
            Kind::InferredList => {
                crate::uniform_list("inferred", 200 + self.count, |range, _, _| {
                    range
                        .map(|row| {
                            div().w_full().h(px(16.)).bg(hsla(
                                (row % 11) as f32 / 11.,
                                0.5,
                                0.5,
                                1.,
                            ))
                        })
                        .collect::<Vec<_>>()
                })
                .with_sizing_behavior(ListSizingBehavior::Infer)
                .w_full()
                .into_any_element()
            }
            Kind::Plain => div()
                .w(px(40.))
                .h(px(20. + (self.count % 3) as f32 * 10.))
                .bg(crate::black())
                .into_any_element(),
        }
    }
}

struct Page {
    children: Vec<Entity<Child>>,
    scrolls: Vec<ScrollHandle>,
    beside: usize,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let parent = |index: usize, page: &Page| {
            div()
                .id(("parent", index))
                .w_full()
                .h(px(120.))
                .flex_none()
                .overflow_y_scroll()
                .track_scroll(&page.scrolls[index])
        };
        div()
            .size_full()
            .bg(crate::white())
            .flex()
            .flex_col()
            // The measured element as a scroll container's content.
            .child(parent(0, self).child(self.children[0].clone()))
            // The inferred-height list as a scroll container's content.
            .child(parent(1, self).child(self.children[1].clone()))
            // The measured element as a flex child.
            .child(
                parent(2, self)
                    .flex()
                    .flex_col()
                    .child(self.children[2].clone()),
            )
            .child(
                div()
                    .flex()
                    .child(self.children[3].clone())
                    .child(div().w(px(10. + (self.beside % 4) as f32 * 7.)).h(px(10.))),
            )
    }
}

#[derive(Clone, Copy, Debug)]
enum Change {
    Notify(usize),
    Beside,
    Resize(f32, f32),
    Wheel(usize, f32),
}

impl Change {
    fn random(rng: &mut StdRng) -> Change {
        match rng.random_range(0..10) {
            0..=2 => Change::Notify(rng.random_range(0..4)),
            3 | 4 => Change::Beside,
            5 => Change::Resize(
                rng.random_range(160..400) as f32,
                rng.random_range(420..600) as f32,
            ),
            _ => Change::Wheel(
                rng.random_range(0..3),
                [-30., -75., 40., -10.][rng.random_range(0..4)],
            ),
        }
    }
}

struct Run {
    cx: TestAppContext,
    window: WindowHandle<Page>,
}

impl Run {
    fn open(seed: u64, mode: FastMode) -> Self {
        let mut cx = oracle_app(seed);
        let window = cx.add_window(|_, cx| Page {
            children: [
                Kind::Measured,
                Kind::InferredList,
                Kind::Measured,
                Kind::Plain,
            ]
            .into_iter()
            .map(|kind| cx.new(|_| Child { kind, count: 0 }))
            .collect(),
            scrolls: (0..3).map(|_| ScrollHandle::new()).collect(),
            beside: 0,
        });
        cx.simulate_window_resize(window.into(), size(px(320.), px(480.)));
        cx.update_window(window.into(), |_, window, _| window.set_fast_mode(mode))
            .unwrap();
        cx.set_draw_on_flush(false);
        Self { cx, window }
    }

    fn apply(&mut self, change: Change) {
        match change {
            Change::Notify(child) => {
                let child = self
                    .window
                    .read_with(&self.cx, |page, _| page.children[child].clone())
                    .unwrap();
                child.update(&mut self.cx, |child, cx| {
                    child.count += 1;
                    cx.notify();
                });
            }
            Change::Beside => self
                .window
                .update(&mut self.cx, |page, _, cx| {
                    page.beside += 1;
                    cx.notify();
                })
                .unwrap(),
            Change::Resize(width, height) => self
                .cx
                .simulate_window_resize(self.window.into(), size(px(width), px(height))),
            Change::Wheel(parent, dy) => {
                let position = point(px(20.), px(40. + parent as f32 * 120.));
                self.cx
                    .update_window(self.window.into(), |_, window, cx| {
                        window.dispatch_event(
                            PlatformInput::ScrollWheel(ScrollWheelEvent {
                                position,
                                delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                                modifiers: Default::default(),
                                touch_phase: TouchPhase::Moved,
                            }),
                            cx,
                        );
                    })
                    .unwrap();
            }
        }
    }

    fn draw(&mut self, from_scratch: bool) -> (Vec<String>, crate::FastStats) {
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                if from_scratch {
                    window.forget_retained_state();
                }
                window.draw(cx).clear(cx);
                (
                    crate::fast::verify::describe_frame(window),
                    window.fast_frame_stats(),
                )
            })
            .unwrap()
    }
}

/// Drives an incremental window in `mode` and a window drawing from scratch
/// through one random history, comparing every frame; returns how many views
/// the incremental window built for a measured leaf.
fn lockstep(seed: u64, steps: usize, mode: FastMode) -> u64 {
    let mut incremental = Run::open(seed, mode);
    let mut reference = Run::open(seed, FastMode::OFF);
    let mut rng = StdRng::seed_from_u64(seed);
    let mut history = Vec::new();
    let mut built = 0;
    for step in 0..steps {
        let changes: Vec<Change> = if step == 0 {
            Vec::new()
        } else {
            (0..rng.random_range(1..=2))
                .map(|_| Change::random(&mut rng))
                .collect()
        };
        for change in &changes {
            incremental.apply(*change);
            reference.apply(*change);
        }
        history.push(changes);
        let (expected, _) = reference.draw(true);
        let (actual, stats) = incremental.draw(false);
        built += stats.views_built_for_measured_leaves;
        if let Some(difference) = crate::fast::verify::first_frame_difference(&actual, &expected) {
            panic!(
                "seed {seed}, step {step}, {mode:?}: the incremental frame differs from the \
                 frame drawn from scratch: {difference}\nchanges: {history:?}"
            );
        }
    }
    built
}

#[test]
fn retained_views_with_measured_leaves_match_frames_drawn_from_scratch() {
    let (seeds, steps) = seeds_and_steps(12, 30);
    let mut built = 0;
    for seed in seeds {
        built += lockstep(seed, steps, FastMode::ALL);
    }
    assert!(
        built > 0,
        "the histories built views for their measured leaves"
    );
}

#[test]
fn retained_views_with_measured_leaves_match_without_splice_or_layers() {
    let (seeds, steps) = seeds_and_steps(8, 30);
    let mode = FastMode {
        splice: false,
        scroll_layers: false,
        ..FastMode::ALL
    };
    for seed in seeds {
        lockstep(seed, steps, mode);
    }
}
