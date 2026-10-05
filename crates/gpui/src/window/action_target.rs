use crate::{Action, ActionDispatchReport, ActionInvocationSource, App, Pixels, Point, Window};

impl Window {
    /// Dispatch through the frontmost action-capable scope at a window-relative position.
    ///
    /// Unlike focused dispatch, this routes pointer controls through their own
    /// ancestors without moving keyboard focus. Occluding hitboxes block targets
    /// beneath them; an absent target is unhandled, never a fallback to focus.
    pub fn dispatch_action_at(
        &mut self,
        position: Point<Pixels>,
        source: ActionInvocationSource,
        action: &dyn Action,
        cx: &mut App,
    ) -> ActionDispatchReport {
        let hit_test = self.rendered_frame.hit_test(position);
        let mut target = None;
        let mut hit_target = None;
        for hitbox in hit_test.ids.iter().take(hit_test.hover_hitbox_count) {
            let Some(candidate) = self
                .rendered_frame
                .hitbox_dispatch_nodes
                .get(hitbox)
                .copied()
            else {
                continue;
            };
            hit_target.get_or_insert(candidate);
            if !self
                .rendered_frame
                .dispatch_tree
                .is_action_available(action, candidate)
            {
                continue;
            }
            // A traversal wrapper may register its ancestor hitbox after its
            // children. Prefer a descendant in that frontmost branch, never a
            // sibling branch behind it.
            if target.is_none_or(|selected| self.dispatch_target_contains(candidate, selected)) {
                target = Some(candidate);
            }
        }
        match target.or(hit_target) {
            Some(target) => self.dispatch_action_on_node_from(target, source, action, cx),
            None => self.dispatch_unhandled_action(source, action, cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        ActionDispatchOutcome, ActionInvocationSource, AppContext as _, Context, FocusHandle,
        InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render, Styled as _,
        TestAppContext, Window, div, point, prelude::FluentBuilder as _, px, size,
    };
    use std::{cell::Cell, rc::Rc};

    actions!(pointer_action_target, [Activate, GlobalActivate]);

    struct Fixture {
        focus: FocusHandle,
        left: Rc<Cell<usize>>,
        right: Rc<Cell<usize>>,
        overlay: Option<crate::HitboxBehavior>,
        nonblocking_hitbox: bool,
    }

    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let left = self.left.clone();
            let right = self.right.clone();
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("left")
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(px(100.))
                        .h(px(100.))
                        .on_mouse_down(MouseButton::Left, |_, _, _| {})
                        .on_action(move |_: &Activate, _, _| left.set(left.get() + 1)),
                )
                .child(
                    div()
                        .id("right")
                        .absolute()
                        .top_0()
                        .left(px(100.))
                        .w(px(100.))
                        .h(px(100.))
                        .track_focus(&self.focus)
                        .on_action(move |_: &Activate, _, _| right.set(right.get() + 1)),
                )
                .when(self.nonblocking_hitbox, |root| {
                    root.child(
                        crate::canvas(
                            |bounds, window, _| {
                                window.insert_hitbox(bounds, crate::HitboxBehavior::Normal);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size(px(100.)),
                    )
                })
                .when_some(self.overlay, |root, behavior| {
                    root.child(
                        div()
                            .id("modal")
                            .absolute()
                            .top_0()
                            .left_0()
                            .w(px(100.))
                            .h(px(100.))
                            .when(behavior == crate::HitboxBehavior::BlockMouse, |this| {
                                this.occlude()
                            })
                            .when(
                                behavior == crate::HitboxBehavior::BlockMouseExceptScroll,
                                |this| this.block_mouse_except_scroll(),
                            ),
                    )
                })
        }
    }

    #[gpux::test]
    fn pointer_target_preserves_unrelated_focus_and_invocation_source(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(200.), px(100.)), |_, cx| Fixture {
            focus: cx.focus_handle(),
            left: Rc::new(Cell::new(0)),
            right: Rc::new(Cell::new(0)),
            overlay: None,
            nonblocking_hitbox: false,
        });
        cx.run_until_parked();
        window
            .update(cx, |root, window, cx| root.focus.focus(window, cx))
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |root, window, cx| {
                let report = window.dispatch_action_at(
                    point(px(25.), px(25.)),
                    ActionInvocationSource::Pointer,
                    &Activate,
                    cx,
                );
                assert_eq!(report.outcome(), ActionDispatchOutcome::Handled);
                assert_eq!(
                    report.invocation().source(),
                    ActionInvocationSource::Pointer
                );
                assert!(root.focus.is_focused(window));
                assert_eq!((root.left.get(), root.right.get()), (1, 0));
            })
            .unwrap();
    }

    #[gpux::test]
    fn pointer_target_never_falls_through_modal_or_missing_hitbox(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(200.), px(100.)), |_, cx| Fixture {
            focus: cx.focus_handle(),
            left: Rc::new(Cell::new(0)),
            right: Rc::new(Cell::new(0)),
            overlay: Some(crate::HitboxBehavior::BlockMouse),
            nonblocking_hitbox: false,
        });
        cx.run_until_parked();
        window
            .update(cx, |root, window, cx| root.focus.focus(window, cx))
            .unwrap();
        cx.run_until_parked();
        for behavior in [
            crate::HitboxBehavior::BlockMouse,
            crate::HitboxBehavior::BlockMouseExceptScroll,
        ] {
            window
                .update(cx, |root, _, cx| {
                    root.overlay = Some(behavior);
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |root, window, cx| {
                    for position in [point(px(25.), px(25.)), point(px(500.), px(500.))] {
                        let report = window.dispatch_action_at(
                            position,
                            ActionInvocationSource::Pointer,
                            &Activate,
                            cx,
                        );
                        assert_eq!(report.outcome(), ActionDispatchOutcome::Unhandled);
                        assert_eq!((root.left.get(), root.right.get()), (0, 0));
                        assert!(root.focus.is_focused(window));
                    }
                })
                .unwrap();
        }
    }

    #[gpux::test]
    fn pointer_target_skips_nonblocking_hitbox_without_an_action_handler(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(200.), px(100.)), |_, cx| Fixture {
            focus: cx.focus_handle(),
            left: Rc::new(Cell::new(0)),
            right: Rc::new(Cell::new(0)),
            overlay: None,
            nonblocking_hitbox: true,
        });
        cx.run_until_parked();
        window
            .update(cx, |root, window, cx| {
                let report = window.dispatch_action_at(
                    point(px(25.), px(25.)),
                    ActionInvocationSource::Pointer,
                    &Activate,
                    cx,
                );
                assert_eq!(report.outcome(), ActionDispatchOutcome::Handled);
                assert_eq!((root.left.get(), root.right.get()), (1, 0));
            })
            .unwrap();
    }

    #[gpux::test]
    fn pointer_target_dispatches_global_only_actions(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(200.), px(100.)), |_, cx| Fixture {
            focus: cx.focus_handle(),
            left: Rc::new(Cell::new(0)),
            right: Rc::new(Cell::new(0)),
            overlay: None,
            nonblocking_hitbox: true,
        });
        cx.run_until_parked();
        window
            .update(cx, |root, window, cx| {
                let calls = Rc::new(Cell::new(0));
                let received = calls.clone();
                crate::App::on_action(cx, move |_: &GlobalActivate, _| {
                    received.set(received.get() + 1)
                });
                root.focus.focus(window, cx);
                let report = window.dispatch_action_at(
                    point(px(25.), px(25.)),
                    ActionInvocationSource::Pointer,
                    &GlobalActivate,
                    cx,
                );
                assert_eq!(report.outcome(), ActionDispatchOutcome::Handled);
                assert_eq!(calls.get(), 1);
                assert!(root.focus.is_focused(window));
                let report = window.dispatch_action_at(
                    point(px(500.), px(500.)),
                    ActionInvocationSource::Pointer,
                    &GlobalActivate,
                    cx,
                );
                assert_eq!(report.outcome(), ActionDispatchOutcome::Unhandled);
                assert_eq!(calls.get(), 1);
            })
            .unwrap();
    }

    struct CachedTarget {
        renders: Rc<Cell<usize>>,
        actions: Rc<Cell<usize>>,
    }

    impl Render for CachedTarget {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let actions = self.actions.clone();
            div()
                .id("cached-target")
                .size_full()
                .on_mouse_down(MouseButton::Left, |_, _, _| {})
                .on_action(move |_: &Activate, _, _| actions.set(actions.get() + 1))
        }
    }

    struct CachedHost {
        target: crate::Entity<CachedTarget>,
        prefix: usize,
    }

    impl Render for CachedHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("cached-host")
                .relative()
                .size_full()
                .children((0..self.prefix).map(|_| div().absolute()))
                .child(
                    self.target
                        .clone()
                        .cached(crate::StyleRefinement::default().absolute().size_full()),
                )
        }
    }

    #[gpux::test]
    fn pointer_target_remaps_cached_hitboxes_without_rendering_the_child(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, cx| CachedHost {
            target: cx.new(|_| CachedTarget {
                renders: Rc::new(Cell::new(0)),
                actions: Rc::new(Cell::new(0)),
            }),
            prefix: 0,
        });
        cx.run_until_parked();
        for iteration in 0..4 {
            window
                .update(cx, |root, _, cx| {
                    root.prefix = iteration + 1;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |root, window, cx| {
                    let report = window.dispatch_action_at(
                        point(px(25.), px(25.)),
                        ActionInvocationSource::Pointer,
                        &Activate,
                        cx,
                    );
                    assert_eq!(report.outcome(), ActionDispatchOutcome::Handled);
                    let target = root.target.read(cx);
                    assert_eq!(target.actions.get(), iteration + 1);
                    assert_eq!(target.renders.get(), 1, "the target must really be reused");
                })
                .unwrap();
        }
    }
}
