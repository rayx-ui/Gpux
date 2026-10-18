//! A `VirtualList` built without `track_scroll` or `track_external_scroll` keeps one scroll
//! handle across renders, with fast mode off and on.
//!
//! The list used to make a fresh handle on every render. Tracking a new handle in its base
//! `div` published new metrics and notified the owner on every frame, so the first draw of a
//! window holding the list never returned. The holder view below bounds its renders and panics
//! with "drew again" instead of hanging.
//!
//! `cargo test -p gpux --features test-support --test virtual_list_default_handle`

use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
};

use gpux::{
    Context, Entity, FastMode, IntoElement, ParentElement as _, Render, Styled as _, TestApp,
    TestAppWindow, VirtualListScrollHandle, Window, div, point, px, size, v_virtual_list,
};

/// More renders than any test here asks of the holder.
const MAX_RENDERS: usize = 40;

const ROWS: usize = 1000;
const ROW: f32 = 20.;
const W: f32 = 300.;
const H: f32 = 200.;

struct Holder {
    renders: Rc<Cell<usize>>,
    ranges: Rc<RefCell<Vec<Range<usize>>>>,
    tracked: Option<VirtualListScrollHandle>,
}

impl Render for Holder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let renders = self.renders.get() + 1;
        self.renders.set(renders);
        assert!(
            renders <= MAX_RENDERS,
            "the list's view drew again: {renders} renders, more than the {MAX_RENDERS} \
             this test asks for"
        );
        let ranges = self.ranges.clone();
        let list = v_virtual_list(
            cx.entity(),
            "default-handle",
            Rc::new(vec![size(px(W), px(ROW)); ROWS]),
            |_, range, _, _| range.map(|_| div().w_full().h(px(ROW))).collect::<Vec<_>>(),
        )
        .on_rendered_range(move |range, _, _| ranges.borrow_mut().push(range))
        .w(px(W))
        .h(px(H));
        match &self.tracked {
            Some(handle) => list.track_scroll(handle),
            None => list,
        }
    }
}

/// Shows the holder only once the test has chosen the window's fast mode.
struct Root {
    holder: Option<Entity<Holder>>,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(gpux::white())
            .children(self.holder.clone())
    }
}

struct Fixture {
    app: TestApp,
    window: TestAppWindow<Root>,
    holder: Entity<Holder>,
    renders: Rc<Cell<usize>>,
    ranges: Rc<RefCell<Vec<Range<usize>>>>,
}

impl Fixture {
    fn open(mode: FastMode) -> Self {
        let mut app = TestApp::new();
        let mut window = app.open_window(|_, _| Root { holder: None });
        window.update(|_, window, _| {
            window.set_fast_mode(mode);
            window.activate_window();
        });
        window.draw();
        let renders = Rc::new(Cell::new(0));
        let ranges = Rc::new(RefCell::new(Vec::new()));
        let holder = app.new_entity({
            let (renders, ranges) = (renders.clone(), ranges.clone());
            move |_| Holder {
                renders,
                ranges,
                tracked: None,
            }
        });
        window.update({
            let holder = holder.clone();
            move |root, _, cx| {
                root.holder = Some(holder);
                cx.notify();
            }
        });
        window.simulate_frame();
        app.run_until_parked();
        Self {
            app,
            window,
            holder,
            renders,
            ranges,
        }
    }

    /// Delivers `count` frames; each draws only when something asked for it.
    fn frames(&mut self, count: usize) {
        for _ in 0..count {
            self.window.simulate_frame();
            self.app.run_until_parked();
        }
    }

    fn last_range(&self) -> Range<usize> {
        self.ranges
            .borrow()
            .last()
            .cloned()
            .expect("the list rendered a range")
    }

    fn wheel(&mut self, dy: f32) {
        self.window
            .simulate_scroll(point(px(W / 2.), px(H / 2.)), point(px(0.), px(dy)));
        self.frames(2);
    }
}

fn modes() -> [(&'static str, FastMode); 2] {
    [
        ("fast mode off", FastMode::OFF),
        ("fast mode on", FastMode::ALL),
    ]
}

#[test]
fn a_handle_less_list_settles_after_its_first_draw() {
    for (name, mode) in modes() {
        let mut fixture = Fixture::open(mode);
        fixture.frames(3);
        let settled = fixture.renders.get();
        assert!(settled >= 1, "{name}: the list's view was never drawn");
        assert_eq!(fixture.last_range(), 0..12, "{name}: the first rows show");
        fixture.frames(5);
        assert_eq!(
            fixture.renders.get(),
            settled,
            "{name}: the list's view drew again with nothing changed"
        );
    }
}

#[test]
fn a_handle_less_list_scrolls_and_keeps_its_offset_across_renders() {
    for (name, mode) in modes() {
        let mut fixture = Fixture::open(mode);
        fixture.frames(2);
        fixture.wheel(-400.);
        assert_eq!(
            fixture.last_range(),
            20..32,
            "{name}: the wheel scrolled the list"
        );
        fixture.wheel(-100.);
        assert_eq!(
            fixture.last_range(),
            25..37,
            "{name}: a second wheel turn adds"
        );

        let before = fixture.renders.get();
        let holder = fixture.holder.clone();
        fixture.app.update_entity(&holder, |_, cx| cx.notify());
        fixture.frames(2);
        assert!(
            fixture.renders.get() > before,
            "{name}: the notified view rendered again"
        );
        assert_eq!(
            fixture.last_range(),
            25..37,
            "{name}: the offset survived the render"
        );
    }
}

#[test]
fn track_scroll_replaces_the_default_handle() {
    for (name, mode) in modes() {
        let mut fixture = Fixture::open(mode);
        fixture.frames(2);
        fixture.wheel(-400.);
        assert_eq!(
            fixture.last_range(),
            20..32,
            "{name}: the default handle scrolled"
        );

        let handle = VirtualListScrollHandle::new();
        let holder = fixture.holder.clone();
        fixture.app.update_entity(&holder, {
            let handle = handle.clone();
            move |holder, cx| {
                holder.tracked = Some(handle);
                cx.notify();
            }
        });
        fixture.frames(2);
        assert_eq!(
            handle.offset().y,
            px(0.),
            "{name}: the tracked handle starts at 0"
        );
        assert_eq!(
            fixture.last_range(),
            0..12,
            "{name}: the tracked handle's offset shows"
        );

        fixture.wheel(-200.);
        assert_eq!(
            handle.offset().y,
            px(-200.),
            "{name}: the wheel moves the tracked handle"
        );
        assert_eq!(fixture.last_range(), 10..22, "{name}");
    }
}
