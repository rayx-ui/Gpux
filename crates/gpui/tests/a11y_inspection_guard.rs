//! Inspection guards: while a guard from `Window::begin_a11y_inspection` lives, the window builds
//! its accessibility tree; the first guard forces a full redraw, dropping the last one requests a
//! frame, and `Window::a11y_tree` returns the last frame's tree only while a source is on.

use gpux::{
    AnyView, AnyWindowHandle, Context, Entity, IntoElement, Render, Role, StyleRefinement,
    TestAppContext, VisualTestContext, Window, div, prelude::*, px,
};
use std::{cell::Cell, rc::Rc};

/// A cached child view: it renders again only when notified or when the window redraws fully.
struct Panel {
    renders: Rc<Cell<usize>>,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        div()
            .id("save")
            .role(Role::Button)
            .aria_label("Save")
            .w(px(80.))
            .h(px(24.))
    }
}

struct Root {
    panel: Entity<Panel>,
    /// Counts the window's frames: the root view renders on every draw.
    frames: Rc<Cell<usize>>,
    /// Set by the test: the next render starts an inspection guard in the middle of its frame.
    begin_while_rendering: bool,
    guard_from_render: Option<gpux::A11yInspection>,
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.frames.set(self.frames.get() + 1);
        if std::mem::take(&mut self.begin_while_rendering) {
            self.guard_from_render = Some(window.begin_a11y_inspection());
        }
        div()
            .size_full()
            .child(AnyView::from(self.panel.clone()).cached(StyleRefinement::default()))
    }
}

struct Opened {
    vcx: VisualTestContext,
    window: AnyWindowHandle,
    root: Entity<Root>,
    panel_renders: Rc<Cell<usize>>,
    frames: Rc<Cell<usize>>,
}

fn open(cx: &mut TestAppContext) -> Opened {
    let panel_renders = Rc::new(Cell::new(0));
    let frames = Rc::new(Cell::new(0));
    let (root, vcx) = cx.add_window_view({
        let panel_renders = panel_renders.clone();
        let frames = frames.clone();
        move |_, cx| Root {
            panel: cx.new(|_| Panel {
                renders: panel_renders,
            }),
            frames,
            begin_while_rendering: false,
            guard_from_render: None,
        }
    });
    let vcx = vcx.clone();
    let window = vcx.window_handle();
    vcx.run_until_parked();
    Opened {
        vcx,
        window,
        root,
        panel_renders,
        frames,
    }
}

fn tree_has_save(vcx: &mut VisualTestContext) -> bool {
    vcx.update(|window, _| {
        window.a11y_tree().is_some_and(|tree| {
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Save"))
        })
    })
}

#[gpux::test]
fn the_first_guard_forces_a_full_redraw(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let renders = opened.panel_renders.get();
    assert!(opened.vcx.update(|window, _| window.a11y_tree().is_none()));

    let guard = opened
        .vcx
        .update(|window, _| window.begin_a11y_inspection());
    opened.vcx.run_until_parked();
    assert!(
        opened.panel_renders.get() > renders,
        "the cached panel renders again, so its node is in the tree"
    );
    assert!(tree_has_save(&mut opened.vcx));

    // A second guard is not a first one: it forces nothing.
    let renders = opened.panel_renders.get();
    let frames = opened.frames.get();
    let second = opened
        .vcx
        .update(|window, _| window.begin_a11y_inspection());
    opened.vcx.run_until_parked();
    assert_eq!(opened.panel_renders.get(), renders);
    assert_eq!(opened.frames.get(), frames);
    drop((guard, second));
}

#[gpux::test]
fn the_source_stays_on_until_the_last_guard_is_dropped(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let first = opened
        .vcx
        .update(|window, _| window.begin_a11y_inspection());
    let second = first.clone();
    opened.vcx.run_until_parked();
    assert!(tree_has_save(&mut opened.vcx));

    drop(first);
    opened.vcx.update(|window, _| window.refresh());
    opened.vcx.run_until_parked();
    assert!(tree_has_save(&mut opened.vcx), "one guard still lives");

    // Dropped in an app update, as app code (and the devtools) drop guards.
    opened.vcx.update(|window, _| {
        drop(second);
        assert!(
            window.a11y_tree().is_none(),
            "no source is on once the last guard is gone"
        );
        // `is_a11y_active` describes the frame drawn last, which still built the tree.
        assert!(window.is_a11y_active());
    });
    opened.vcx.run_until_parked();
    assert!(
        !opened.vcx.update(|window, _| window.is_a11y_active()),
        "dropping the last guard requested a frame, which built no tree"
    );
    assert!(opened.vcx.update(|window, _| window.a11y_tree().is_none()));
    assert!(
        opened
            .vcx
            .take_accessibility_updates(opened.window)
            .is_empty()
    );
}

#[gpux::test]
fn a_tree_from_an_earlier_inspection_is_never_read_as_current(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let guard = opened
        .vcx
        .update(|window, _| window.begin_a11y_inspection());
    opened.vcx.run_until_parked();
    assert!(tree_has_save(&mut opened.vcx));
    drop(guard);
    opened.vcx.run_until_parked();

    // A new inspection reads nothing until its first frame is drawn.
    let guard = opened.vcx.update(|window, _| {
        let guard = window.begin_a11y_inspection();
        assert!(window.a11y_tree().is_none());
        guard
    });
    opened.vcx.run_until_parked();
    assert!(tree_has_save(&mut opened.vcx));
    drop(guard);
}

#[gpux::test]
fn a_guard_started_while_drawing_takes_effect_after_the_draw(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    opened.vcx.update(|_, cx| {
        opened.root.update(cx, |root, cx| {
            root.begin_while_rendering = true;
            cx.notify();
        })
    });
    opened.vcx.run_until_parked();
    assert!(
        opened
            .vcx
            .update(|_, cx| opened.root.read(cx).guard_from_render.is_some())
    );
    assert!(
        tree_has_save(&mut opened.vcx),
        "the guard's redraw ran after the frame that started it"
    );
}

#[gpux::test]
fn an_inaccessible_application_gives_a_disabled_guard(cx: &mut TestAppContext) {
    cx.disable_accessibility();
    let mut opened = open(cx);
    let guard = opened
        .vcx
        .update(|window, _| window.begin_a11y_inspection());
    assert!(guard.accessibility_disabled());
    opened.vcx.run_until_parked();
    assert!(opened.vcx.update(|window, _| window.a11y_tree().is_none()));
}
