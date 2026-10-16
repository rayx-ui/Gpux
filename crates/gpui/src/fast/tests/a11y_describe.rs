//! The accessibility tree in fast mode's frame description: `describe_rendered_frame` lists it
//! (nodes in tree order with their properties, focus, focusable nodes, action handlers and the id
//! map), the oracle compares it, so a difference in a carried tree fails the oracle, and shadow
//! verification runs while the tree is built.

use super::oracle_support::{Change, OracleWindow, oracle_app};
use crate::{
    AppContext as _, Context, FastMode, FocusHandle, IntoElement, Render, Role, Window, div,
    prelude::*, px, size,
};

const VIEWS: FastMode = FastMode {
    layout_retention: true,
    view_retention: true,
    ..FastMode::OFF
};

struct Probe {
    focus: FocusHandle,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .id("save")
                .role(Role::Button)
                .track_focus(&self.focus)
                .on_a11y_action(accesskit::Action::Increment, |_, _, _| {})
                .w(px(60.))
                .h(px(20.))
                .child("Save"),
        )
    }
}

#[test]
fn the_frame_description_lists_the_tree() {
    let mut cx = oracle_app(0);
    let window = cx.add_window(|window, cx| {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Probe { focus }
    });
    cx.simulate_window_resize(window.into(), size(px(300.), px(200.)));
    cx.run_until_parked();
    let describe = |cx: &mut crate::TestAppContext| {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            window.describe_rendered_frame()
        })
        .unwrap()
    };
    let lines = describe(&mut cx);
    assert!(
        lines.iter().any(|line| line == "a11y none"),
        "no tree without a source"
    );

    let _guard = cx
        .update_window(window.into(), |_, window, _| window.begin_a11y_inspection())
        .unwrap();
    let lines = describe(&mut cx);
    let a11y: Vec<&String> = lines
        .iter()
        .filter(|line| line.starts_with("a11y "))
        .collect();
    let has = |prefix: &str, needle: &str| {
        a11y.iter()
            .any(|line| line.starts_with(prefix) && line.contains(needle))
    };
    assert!(has("a11y focus", "#"), "{a11y:#?}");
    assert!(has("a11y 0 #0", "Window"), "{a11y:#?}");
    assert!(has("a11y ", "role: Button"), "{a11y:#?}");
    assert!(has("a11y ", "\"Save\""), "the text leaf: {a11y:#?}");
    assert!(has("a11y focusable", "#"), "{a11y:#?}");
    assert!(has("a11y listener", "Increment"), "{a11y:#?}");
    assert!(has("a11y element", "save"), "{a11y:#?}");
}

/// Two windows of the oracle, building their trees for inspection, drawn after `change`.
fn pair_after(
    change: Change,
    tamper: Option<fn(&mut crate::window::a11y::A11yEvent)>,
) -> Option<String> {
    let mut incremental = OracleWindow::open_with(0, VIEWS, true);
    let mut reference = OracleWindow::open_with(0, FastMode::OFF, true);
    for (step, change) in [Change::Hover { x: 4000., y: 4000. }, change]
        .into_iter()
        .enumerate()
    {
        incremental.apply(&change);
        reference.apply(&change);
        let (expected, _) = reference.draw(true);
        let (actual, stats) = incremental.draw_after(false, |window| {
            if step == 1
                && let Some(tamper) = tamper
            {
                // Seed a difference into what the cached panels carry.
                for event in &mut window.a11y.rendered_events {
                    if let crate::window::a11y::A11yEvent::Push {
                        node: Some(node), ..
                    } = event
                        && node.label().is_some_and(|label| label.starts_with("panel"))
                    {
                        tamper(event);
                    }
                }
            }
        });
        if step == 1 {
            assert!(
                stats.views_reused > 0,
                "the panels are drawn from last frame: {stats:?}"
            );
        }
        if let Some(difference) = crate::fast::verify::first_frame_difference(&actual, &expected) {
            return Some(difference);
        }
    }
    None
}

#[test]
fn a_seeded_difference_in_a_carried_tree_fails_the_oracle() {
    assert_eq!(
        pair_after(Change::Redraw, None),
        None,
        "the untouched pair matches"
    );
    let difference = pair_after(
        Change::Redraw,
        Some(|event| {
            if let crate::window::a11y::A11yEvent::Push {
                node: Some(node), ..
            } = event
            {
                node.set_label("tampered");
            }
        }),
    )
    .expect("the tampered tree is noticed");
    assert!(difference.contains("tampered"), "{difference}");
}

#[test]
fn a_seeded_difference_in_carried_bounds_fails_the_oracle() {
    let difference = pair_after(
        Change::Redraw,
        Some(|event| {
            if let crate::window::a11y::A11yEvent::Push {
                bounds: Some(bounds),
                ..
            } = event
            {
                bounds.origin.x += crate::px(1000.);
            }
        }),
    )
    .expect("the moved node is noticed");
    assert!(difference.contains("a11y"), "{difference}");
}

#[cfg(feature = "diagnostics")]
mod verification {
    use std::{cell::RefCell, rc::Rc};

    use super::super::oracle_support::oracle_app;
    use crate::{
        AppContext as _, Context, Entity, IntoElement, Render, RetentionMismatch, Role, Window,
        div, prelude::*, px, size,
    };

    struct Untracked {
        count: Rc<RefCell<usize>>,
    }

    impl Render for Untracked {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("count")
                .role(Role::Status)
                .w(px(80.))
                .h(px(20.))
                .child(format!("count {}", self.count.borrow()))
        }
    }

    struct Host {
        child: Entity<Untracked>,
        tick: usize,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(self.child.clone())
                .child(format!("tick {}", self.tick))
        }
    }

    /// Shadow verification runs while the accessibility tree is built: a view reading untracked
    /// state is reported, and a window whose views report what they read reports nothing.
    #[test]
    fn shadow_verification_runs_with_accessibility_on() {
        let count = Rc::new(RefCell::new(0));
        let mut cx = oracle_app(0);
        let window = cx.add_window({
            let count = count.clone();
            move |_, cx| Host {
                child: cx.new(|_| Untracked { count }),
                tick: 0,
            }
        });
        cx.simulate_window_resize(window.into(), size(px(300.), px(200.)));
        cx.set_draw_on_flush(false);
        let reports: Rc<RefCell<Vec<RetentionMismatch>>> = Rc::default();
        let _guard = cx
            .update_window(window.into(), |_, window, _| {
                window.set_fast_mode(super::VIEWS);
                window.set_retention_verify(true);
                let reports = reports.clone();
                window.on_retention_mismatch(move |m| reports.borrow_mut().push(m.clone()));
                window.begin_a11y_inspection()
            })
            .unwrap();
        let draw = |cx: &mut crate::TestAppContext, tick: bool| {
            if tick {
                window
                    .update(cx, |host, _, cx| {
                        host.tick += 1;
                        cx.notify();
                    })
                    .unwrap();
            }
            cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
        };
        for _ in 0..3 {
            draw(&mut cx, true);
        }
        assert!(reports.borrow().is_empty(), "{:?}", reports.borrow());
        *count.borrow_mut() = 7;
        draw(&mut cx, true);
        let reports = reports.borrow();
        assert_eq!(reports.len(), 1, "{reports:?}");
        assert!(reports[0].view_type.contains("Untracked"), "{reports:?}");
    }
}
