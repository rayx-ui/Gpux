//! A view drawn with view retention beside the same view drawn from scratch,
//! for the view-retention groups: both get the same changes and input, and
//! every frame of the retained window is compared with the reference's.

use super::oracle_support::oracle_app;
use crate::{
    App, AppContext as _, Context, FastMode, FastStats, InputEvent as _, Modifiers, MouseButton,
    PlatformInput, Point, Render, TestAppContext, Window, WindowHandle, point, px, size,
};

/// Views mode: layout and view retention on, splice off.
pub(crate) const VIEWS: FastMode = FastMode {
    layout_retention: true,
    view_retention: true,
    ..FastMode::OFF
};

/// The window size the pairs open at.
const SIZE: (f32, f32) = (400., 300.);

/// A window drawing `V` with view retention and a window drawing it from
/// scratch, in applications of their own.
pub(crate) struct ViewPair<V: 'static> {
    retained: (TestAppContext, WindowHandle<V>),
    reference: (TestAppContext, WindowHandle<V>),
}

impl<V: Render + 'static> ViewPair<V> {
    /// Opens both windows, building the root view in each with `build`, and
    /// draws their first frame.
    pub(crate) fn open(
        build: impl Fn(&mut Window, &mut Context<V>) -> V + Clone + 'static,
    ) -> Self {
        Self::open_in(VIEWS, build)
    }

    /// [`Self::open`] with the retained window in `mode`.
    pub(crate) fn open_in(
        mode: FastMode,
        build: impl Fn(&mut Window, &mut Context<V>) -> V + Clone + 'static,
    ) -> Self {
        let open = |mode: FastMode| {
            let mut cx = oracle_app(0);
            let window = cx.add_window(build.clone());
            cx.simulate_window_resize(window.into(), size(px(SIZE.0), px(SIZE.1)));
            cx.update_window(window.into(), |_, window, _| window.set_fast_mode(mode))
                .unwrap();
            cx.set_draw_on_flush(false);
            (cx, window)
        };
        let mut pair = Self {
            retained: open(mode),
            reference: open(FastMode::OFF),
        };
        pair.draw("first frame");
        pair
    }

    /// Runs `change` on the root view of each window.
    pub(crate) fn update(&mut self, change: impl Fn(&mut V, &mut Window, &mut Context<V>)) {
        for (cx, window) in [&mut self.retained, &mut self.reference] {
            window
                .update(cx, |view, window, cx| change(view, window, cx))
                .unwrap();
        }
    }

    /// Runs `change` on each application, given its window's root view.
    pub(crate) fn update_app(&mut self, change: impl Fn(&crate::Entity<V>, &mut App)) {
        for (cx, window) in [&mut self.retained, &mut self.reference] {
            let root = window.root(cx).unwrap();
            cx.update(|cx| change(&root, cx));
        }
    }

    /// Advances both applications' clocks by `duration`, running what comes
    /// due.
    pub(crate) fn advance_clock(&mut self, duration: std::time::Duration) {
        for (cx, _) in [&mut self.retained, &mut self.reference] {
            cx.executor().advance_clock(duration);
            cx.run_until_parked();
        }
    }

    /// Runs `f` on each application with its window.
    pub(crate) fn each(&mut self, f: impl Fn(&mut TestAppContext, crate::AnyWindowHandle)) {
        for (cx, window) in [&mut self.retained, &mut self.reference] {
            f(cx, (*window).into());
        }
    }

    /// Dispatches `event` to both windows.
    pub(crate) fn dispatch(&mut self, event: PlatformInput) {
        for (cx, window) in [&mut self.retained, &mut self.reference] {
            cx.update_window((*window).into(), |_, window, cx| {
                window.dispatch_event(event.clone(), cx);
            })
            .unwrap();
        }
    }

    /// Moves the pointer to `position` in both windows.
    pub(crate) fn move_to(&mut self, position: Point<crate::Pixels>) {
        self.dispatch(
            crate::MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
        );
    }

    /// Clicks at `position` in both windows.
    pub(crate) fn click(&mut self, position: Point<crate::Pixels>) {
        self.move_to(position);
        self.dispatch(
            crate::MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
        );
        self.dispatch(
            crate::MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
            }
            .to_platform_input(),
        );
    }

    /// Moves the pointer outside both windows, where no view reads a hover.
    pub(crate) fn pointer_away(&mut self) {
        self.move_to(point(px(SIZE.0 * 4.), px(SIZE.1 * 4.)));
    }

    /// Draws a frame in each window, the reference from scratch, panics with
    /// `label` and the first difference if they differ, and returns the
    /// retained window's counters for the frame.
    pub(crate) fn draw(&mut self, label: &str) -> FastStats {
        let draw = |(cx, window): &mut (TestAppContext, WindowHandle<V>), from_scratch: bool| {
            cx.update_window((*window).into(), |_, window, cx| {
                let callbacks = window.next_frame_callbacks.take();
                for callback in callbacks {
                    callback(window, cx);
                }
            })
            .unwrap();
            cx.update_window((*window).into(), |_, window, cx| {
                if from_scratch {
                    window.forget_retained_state();
                }
                window.draw(cx).clear(cx);
                (window.describe_rendered_frame(), window.fast_frame_stats())
            })
            .unwrap()
        };
        let (expected, _) = draw(&mut self.reference, true);
        let (actual, stats) = draw(&mut self.retained, false);
        if let Some(difference) = crate::fast::verify::first_frame_difference(&actual, &expected) {
            panic!(
                "{label}: the retained frame differs from the one drawn from scratch: {difference}"
            );
        }
        stats
    }

    /// Runs `f` with the retained window and its root view, which it may
    /// read but not update: an update would change it for the next frame.
    pub(crate) fn retained<R>(
        &mut self,
        f: impl FnOnce(&crate::Entity<V>, &mut Window, &mut App) -> R,
    ) -> R {
        let (cx, window) = &mut self.retained;
        cx.update_window((*window).into(), |root, window, cx| {
            f(&root.downcast::<V>().unwrap(), window, cx)
        })
        .unwrap()
    }
}
