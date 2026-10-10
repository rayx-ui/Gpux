//! Scroll-only detection, carried outputs and input under scroll layers.
//! Adapted from gpui-fast's `fast/tests/layers.rs`.

mod invalidation {
    use crate::fast::layers::invalidate::{ScrollSource, render_read_offset, scrolled};
    use crate::{
        AnyWindowHandle, App, AppContext as _, Context, Entity, GlobalElementId,
        InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollDelta,
        ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _,
        TestAppContext, TouchPhase, Window, div, point, px, rgb,
    };
    use std::{cell::Cell, rc::Rc};

    /// A page scrolled by a wheel: a 100 px tall scroll container of forty
    /// 20 px rows at the top left of the window.
    struct Page {
        handle: ScrollHandle,
        read_offset_in_render: bool,
        reader: Option<Entity<Reader>>,
    }

    impl Render for Page {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            if self.read_offset_in_render {
                let _ = self.handle.offset();
            }
            div()
                .size_full()
                .child(
                    div()
                        .id("scroller")
                        .overflow_y_scroll()
                        .track_scroll(&self.handle)
                        .w(px(200.))
                        .h(px(100.))
                        .children(
                            (0..40).map(|row| div().h(px(20.)).bg(rgb(0x100000 + row * 0x10))),
                        ),
                )
                .children(self.reader.clone())
        }
    }

    /// A view outside the scroll container that shows where it is scrolled.
    struct Reader {
        handle: ScrollHandle,
        renders: Rc<Cell<usize>>,
    }

    impl Render for Reader {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let offset = self.handle.offset();
            div().w(px(10.)).h(px(10.) - offset.y / 100.)
        }
    }

    fn page(handle: ScrollHandle, read_offset_in_render: bool) -> Page {
        Page {
            handle,
            read_offset_in_render,
            reader: None,
        }
    }

    pub(super) fn with_window<R>(
        cx: &mut TestAppContext,
        window: AnyWindowHandle,
        f: impl FnOnce(&mut Window, &mut App) -> R,
    ) -> R {
        cx.update_window(window, |_, window, cx| f(window, cx))
            .unwrap()
    }

    pub(super) fn draw(cx: &mut TestAppContext, window: AnyWindowHandle) {
        with_window(cx, window, |window, cx| window.draw(cx).clear(cx));
    }

    /// Scrolls the page by `dy` with the wheel, returning the scroll
    /// containers that count as scrolled before the frame that follows is
    /// drawn.
    pub(super) fn wheel(
        cx: &mut TestAppContext,
        window: AnyWindowHandle,
        dy: f32,
    ) -> Vec<GlobalElementId> {
        with_window(cx, window, |window, cx| {
            window.dispatch_event(
                crate::PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position: point(px(20.), px(20.)),
                    delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                    modifiers: Default::default(),
                    touch_phase: TouchPhase::Moved,
                }),
                cx,
            );
            // Before the update ends: the frame the scroll asks for is drawn
            // as its effects are flushed, and takes the scroll in.
            window
                .fast_layers
                .scrolls
                .containers()
                .filter(|id| scrolled(window, id))
                .cloned()
                .collect()
        })
    }

    /// The page's scroll container, as last painted.
    fn scroller(cx: &mut TestAppContext, window: AnyWindowHandle) -> GlobalElementId {
        with_window(cx, window, |window, _| {
            window
                .fast_layers
                .scrolls
                .containers()
                .find(|id| id.last() == Some(&"scroller".into()))
                .cloned()
                .expect("the scroll container was painted")
        })
    }

    fn is_scrolled(cx: &mut TestAppContext, window: AnyWindowHandle, id: &GlobalElementId) -> bool {
        with_window(cx, window, |window, _| scrolled(window, id))
    }

    #[crate::test]
    fn a_wheel_scroll_is_noted_for_its_container(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window: AnyWindowHandle = crate::fast::tests::layer_support::add_window(cx, |_, _| {
            page(ScrollHandle::new(), false)
        })
        .into();
        draw(cx, window);
        draw(cx, window);
        let scroller = scroller(cx, window);
        assert!(!is_scrolled(cx, window, &scroller));

        assert_eq!(wheel(cx, window, -30.), vec![scroller.clone()]);
        draw(cx, window);
        assert!(
            !is_scrolled(cx, window, &scroller),
            "a frame takes in the scrolls before it"
        );
    }

    #[crate::test]
    fn programmatic_scrolls_are_noted(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = ScrollHandle::new();
        let window: AnyWindowHandle = crate::fast::tests::layer_support::add_window(cx, {
            let handle = handle.clone();
            move |_, _| page(handle, false)
        })
        .into();
        draw(cx, window);
        let scroller = scroller(cx, window);
        assert!(!is_scrolled(cx, window, &scroller));
        crate::fast::tests::layer_support::set_offset(&handle, point(px(0.), px(-40.)));
        assert!(is_scrolled(cx, window, &scroller));
        draw(cx, window);
        assert!(!is_scrolled(cx, window, &scroller));
        handle.scroll_to_item(30);
        assert!(is_scrolled(cx, window, &scroller));
        draw(cx, window);
        assert!(!is_scrolled(cx, window, &scroller));
        handle.scroll_to_bottom();
        assert!(is_scrolled(cx, window, &scroller));
    }

    /// Whether the root view's own reading, last frame, included the offset
    /// of its scroll container.
    fn root_view_read_offset(cx: &mut TestAppContext, window: AnyWindowHandle) -> bool {
        let scroller = scroller(cx, window);
        with_window(cx, window, |window, _| {
            let source = window
                .fast_layers
                .scrolls
                .source(&scroller)
                .expect("the scroll container was painted");
            assert!(matches!(source, ScrollSource::Handle(_)));
            let record = window
                .rendered_frame
                .fast_retained
                .records
                .first()
                .expect("the root view is retained");
            let read = record
                .render_dependencies
                .as_ref()
                .unwrap_or(&record.own_dependencies);
            render_read_offset(read, &source)
        })
    }

    #[crate::test]
    fn a_render_that_reads_the_offset_depends_on_it(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window: AnyWindowHandle = crate::fast::tests::layer_support::add_window(cx, |_, _| {
            page(ScrollHandle::new(), true)
        })
        .into();
        draw(cx, window);
        assert!(root_view_read_offset(cx, window));
    }

    #[crate::test]
    fn one_that_does_not_does_not(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window: AnyWindowHandle = crate::fast::tests::layer_support::add_window(cx, |_, _| {
            page(ScrollHandle::new(), false)
        })
        .into();
        draw(cx, window);
        assert!(!root_view_read_offset(cx, window));
    }

    #[crate::test]
    fn a_view_that_read_the_offset_is_built_again_when_it_scrolls(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let renders = Rc::new(Cell::new(0));
        let window: AnyWindowHandle = crate::fast::tests::layer_support::add_window(cx, {
            let renders = renders.clone();
            move |_, cx| {
                let handle = ScrollHandle::new();
                let reader = cx.new(|_| Reader {
                    handle: handle.clone(),
                    renders,
                });
                Page {
                    handle,
                    read_offset_in_render: false,
                    reader: Some(reader),
                }
            }
        })
        .into();
        draw(cx, window);
        draw(cx, window);
        let before = renders.get();
        wheel(cx, window, -30.);
        draw(cx, window);
        assert_eq!(renders.get(), before + 1, "the reader shows the new offset");
        draw(cx, window);
        assert_eq!(renders.get(), before + 1, "and is reused once it has");
    }
}

mod decisions {
    use crate::fast::layers::policy::Decision;
    use crate::fast::tests::layer_support::*;
    use crate::{
        AnyWindowHandle, AppContext as _, Bounds, Context, Entity, Hsla, InteractiveElement as _,
        IntoElement, MouseMoveEvent, ParentElement as _, Pixels, Render, ScrollDelta, ScrollHandle,
        ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _, TestAppContext, TouchPhase,
        Window, WindowHandle, div, point, px, rgb,
    };
    use std::{cell::Cell, rc::Rc};
    /// A background no tile can be cleared with keeps the container on
    /// today's path for a while, rather than painting a layer every
    /// scrolled frame that it can never composite.
    #[crate::test]
    fn a_background_that_cannot_be_baked_stops_repainting(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        handle
            .update(cx, |page, _, cx| {
                page.translucent = true;
                cx.notify();
            })
            .unwrap();
        draw(cx, window);
        promote(cx, window);
        for _ in 0..20 {
            assert_eq!(scroll(cx, window, -5.), Some(Decision::Bypass));
        }
    }

    #[crate::test]
    fn a_wheel_scroll_composites(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = page(cx, false).into();
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        assert_eq!(scroll(cx, window, 20.), Some(Decision::Composite));
    }

    /// A scrollbar being dragged publishes the offset itself, which notifies
    /// the view holding the container as a wheel listener would: that
    /// notification is the scroll's, and the layer composites.
    #[crate::test]
    fn a_scrollbar_drag_composites(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        promote(cx, window);
        for step in 0..6 {
            frame_after(cx, window, |cx| {
                handle
                    .update(cx, |page, _, cx| {
                        let offset = page.handle.offset();
                        let owner = cx.entity_id();
                        assert!(page.handle.publish_scrollbar_offset(
                            point(offset.x, offset.y - px(10.)),
                            owner,
                            cx,
                        ));
                    })
                    .unwrap();
            });
            assert_eq!(
                decision(cx, window),
                Some(Decision::Composite),
                "step {step}"
            );
        }
    }

    /// A scroll an application asks for is applied when the container is
    /// next prepainted, in a frame the application asks for by notifying the
    /// view: nothing tells that notification from one for a change of what
    /// the view renders, so the frame paints the layer afresh. The
    /// notification the published offset sends is the scroll's, and the
    /// frame it asks for composites.
    #[crate::test]
    fn a_requested_scroll_the_owner_is_notified_for_repaints(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        promote(cx, window);
        frame_after(cx, window, |cx| {
            handle
                .update(cx, |page, _, cx| {
                    set_offset(&page.handle, point(px(0.), px(-70.)));
                    cx.notify();
                })
                .unwrap();
        });
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
        assert_eq!(
            handle
                .update(cx, |page, _, _| page.handle.offset())
                .unwrap(),
            point(px(0.), px(-70.))
        );
        cx.run_until_parked();
        draw(cx, window);
        assert_eq!(decision(cx, window), Some(Decision::Composite));
    }

    #[crate::test]
    fn a_child_view_page_composites(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = page(cx, true).into();
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    }

    #[crate::test]
    fn a_notified_content_view_repaints(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, true);
        let window = handle.into();
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        let rows = handle
            .read_with(cx, |page, _| page.rows.clone().unwrap())
            .unwrap();
        // Notified in the frame a scroll draws, so that the page is built
        // again for the scroll and the rows' view is found changed.
        // Notified without being updated first: the view may have changed
        // what it renders in a way nothing it read shows.
        assert_eq!(
            scroll_and(cx, window, -20., |cx| cx.notify(rows.entity_id())),
            Some(Decision::Repaint)
        );
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        assert_eq!(
            scroll_and(cx, window, -20., |cx| rows.update(cx, |rows, cx| {
                rows.tint = 0x200000;
                cx.notify();
            })),
            Some(Decision::Repaint)
        );
        // Repeated broad repaints now fall back rather than keeping the
        // expensive overscan alive.
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
    }

    #[crate::test]
    fn an_owner_notified_for_another_reason_while_scrolling_repaints(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        // What the owner renders changes through state nothing records a
        // read of, which only its notification tells.
        let height = Rc::new(Cell::new(10.));
        handle
            .update(cx, |page, _, cx| {
                let height = height.clone();
                page.extra = Some(Rc::new(move || {
                    div().h(px(height.get())).into_any_element()
                }));
                cx.notify();
            })
            .unwrap();
        draw(cx, window);
        let owner = handle.update(cx, |_, _, cx| cx.entity_id()).unwrap();
        promote(cx, window);
        let d = scroll(cx, window, -20.);
        assert_eq!(d, Some(Decision::Composite));
        // Notified before the scroll.
        assert_eq!(
            scroll_and(cx, window, -20., |cx| {
                height.set(30.);
                cx.notify(owner);
            }),
            Some(Decision::Repaint)
        );
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        // Notified after it.
        frame_after(cx, window, |cx| {
            with_window(cx, window, |window, cx| {
                window.dispatch_event(
                    crate::PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position: point(px(20.), px(20.)),
                        delta: ScrollDelta::Pixels(point(px(0.), px(-20.))),
                        modifiers: Default::default(),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
                height.set(10.);
                cx.notify(owner);
            })
        });
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
        // Repeated broad repaints now fall back rather than keeping the
        // expensive overscan alive.
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
    }

    #[crate::test]
    fn content_wider_than_a_container_scrolling_on_y_composites(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = crate::fast::tests::layer_support::add_window(cx, |_, cx| LayerPage {
            wide: true,
            ..new_page(false, cx)
        });
        let window = handle.into();
        draw(cx, window);
        draw(cx, window);
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    }

    /// A scrollbar beside the container reads the offset while it is
    /// prepainted and painted, outside the content: the content is
    /// composited, and the scrollbar drawn afresh around it.
    #[crate::test]
    fn an_owner_that_reads_the_offset_outside_the_content_composites(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        handle
            .update(cx, |page, _, cx| {
                page.scrollbar = true;
                cx.notify();
            })
            .unwrap();
        draw(cx, window);
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    }

    #[crate::test]
    fn an_owner_that_reads_the_offset_in_render_repaints(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        handle
            .update(cx, |page, _, cx| {
                page.read_offset_in_render = true;
                cx.notify();
            })
            .unwrap();
        draw(cx, window);
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Repaint));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Repaint));
    }

    /// Moves the pointer to `position`, drawing the frame that follows.
    fn move_to(cx: &mut TestAppContext, window: AnyWindowHandle, position: crate::Point<Pixels>) {
        frame_after(cx, window, |cx| {
            with_window(cx, window, |window, cx| {
                window.dispatch_event(
                    crate::PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        pressed_button: None,
                        modifiers: Default::default(),
                    }),
                    cx,
                );
            })
        });
    }

    /// Scrolls the page by `dy` through its scrollbar, which leaves the
    /// pointer where it is, returning what the container decided.
    fn drag_scrollbar(
        cx: &mut TestAppContext,
        handle: WindowHandle<LayerPage>,
        dy: f32,
    ) -> Option<Decision> {
        let window = handle.into();
        frame_after(cx, window, |cx| {
            handle
                .update(cx, |page, _, cx| {
                    let offset = page.handle.offset();
                    let owner = cx.entity_id();
                    page.handle.publish_scrollbar_offset(
                        point(offset.x, offset.y + px(dy)),
                        owner,
                        cx,
                    );
                })
                .unwrap();
        });
        decision(cx, window)
    }

    /// A page whose rows have a hover style, promoted by wheel scrolls from
    /// beside the rows.
    fn hover_page(cx: &mut TestAppContext) -> WindowHandle<LayerPage> {
        let handle = page(cx, false);
        let window = handle.into();
        handle
            .update(cx, |page, _, cx| {
                page.hover = true;
                cx.notify();
            })
            .unwrap();
        draw(cx, window);
        assert_eq!(
            scroll_at(cx, window, beside_rows(), -20.),
            Some(Decision::Bypass)
        );
        assert_eq!(
            scroll_at(cx, window, beside_rows(), -20.),
            Some(Decision::Repaint)
        );
        handle
    }

    /// Prepaint sees the last frame's hit test, which a scroll leaves
    /// behind: content that read hovers composites only where none of them
    /// can be true, with the pointer over none of the hitboxes it read.
    #[crate::test]
    fn content_that_read_hovers_composites_with_the_pointer_off_them(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = hover_page(cx);
        let window = handle.into();
        for _ in 0..3 {
            assert_eq!(
                scroll_at(cx, window, beside_rows(), -20.),
                Some(Decision::Composite)
            );
        }
        // The wheel over a row: the scroll moves a row under the pointer,
        // which a frame painting the content afresh paints hovered.
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Repaint));
    }

    #[crate::test]
    fn a_hover_change_in_the_content_repaints(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = hover_page(cx);
        let window = handle.into();
        assert_eq!(
            scroll_at(cx, window, beside_rows(), -20.),
            Some(Decision::Composite)
        );
        move_to(cx, window, point(px(20.), px(70.)));
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
        // The row under the pointer was painted hovered: a scroll moves
        // another one there, and the layer is not composited as it was.
        assert_ne!(drag_scrollbar(cx, handle, -20.), Some(Decision::Composite));
    }

    /// A page beside a scroll container of the view around it, whose
    /// handle the page is a dependent view of.
    struct Sided {
        page: Entity<LayerPage>,
        side: ScrollHandle,
    }

    impl Render for Sided {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .child(
                    div()
                        .w(viewport().size.width)
                        .h(viewport().size.height)
                        .child(self.page.clone()),
                )
                .child(
                    div()
                        .id("side")
                        .overflow_y_scroll()
                        .track_scroll(&self.side)
                        .w(px(100.))
                        .h(viewport().size.height)
                        .children((0..ROWS).map(|index| {
                            div()
                                .h(px(ROW_HEIGHT))
                                .bg(rgb(0x001000 + index as u32 * 0x10))
                        })),
                )
        }
    }

    /// gpux notifies a scroll handle's dependent views with its owner when
    /// its offset is published. Such a notification of the view holding a
    /// layer is a scroll's, not a change of what the view renders: the layer
    /// composites, where any other notification paints it afresh.
    #[crate::test]
    fn a_notification_as_a_scroll_dependent_composites(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = add_window(cx, |_, cx| {
            let side = ScrollHandle::new();
            let page = cx.new(|cx| new_page(false, cx));
            side.register_dependent_view(page.entity_id());
            Sided { page, side }
        });
        let window = handle.into();
        draw(cx, window);
        draw(cx, window);
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        let beside = point(viewport().size.width + px(50.), px(20.));
        let side = handle.update(cx, |sided, _, _| sided.side.clone()).unwrap();
        for _ in 0..3 {
            let before = side.offset();
            // The page is built again, as the handle's dependent, and its
            // container decides: nothing of its own scrolled or changed.
            assert_eq!(
                scroll_at(cx, window, beside, -20.),
                Some(Decision::Composite)
            );
            assert_ne!(
                side.offset(),
                before,
                "the wheel scrolled the side container"
            );
        }
        let page = handle.update(cx, |sided, _, _| sided.page.clone()).unwrap();
        frame_after(cx, window, |cx| page.update(cx, |_, cx| cx.notify()));
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
    }

    /// A view around the page that sets the text colour the page inherits.
    struct Themed {
        color: Hsla,
        page: Entity<LayerPage>,
    }

    impl Render for Themed {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .text_color(self.color)
                .child(self.page.clone())
        }
    }

    #[crate::test]
    fn a_style_change_of_the_scroll_div_repaints(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        // The scroll div's own style is set by the view that holds it, whose
        // notification repaints the layer anyway; what it inherits changes
        // without it, as the text colour of a view around it does.
        let handle = crate::fast::tests::layer_support::add_window(cx, |_, cx| Themed {
            color: crate::black(),
            page: cx.new(|cx| new_page(false, cx)),
        });
        let window: AnyWindowHandle = handle.into();
        draw(cx, window);
        draw(cx, window);
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        frame_after(cx, window, |cx| {
            handle
                .update(cx, |themed, _, cx| {
                    themed.color = crate::white();
                    cx.notify();
                })
                .unwrap();
        });
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    }

    #[crate::test]
    fn exposing_past_the_margin_repaints(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = page(cx, false).into();
        promote(cx, window);
        // Painted at the offset it was promoted at, -40 px, two viewports
        // (200 px) beyond each edge as far as the content goes. The margin is
        // a quarter of a viewport: the ninth 20 px scroll leaves less than
        // 25 px painted below the viewport.
        for _ in 0..8 {
            assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        }
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Repaint));
        let id = scroller_id(cx, window);
        let painted_at = with_window(cx, window, |window, _| {
            let record = window.fast_layers.layers[&id].record.as_ref().unwrap();
            (record.scroll_offset, record.painted_region)
        });
        assert_eq!(
            painted_at,
            (
                point(px(0.), px(-220.)),
                Bounds::from_corners(point(px(0.), px(-200.)), point(px(200.), px(300.)))
            ),
            "re-centred on the viewport"
        );
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    }

    /// Puts a 200 px square container scrolling on x, holding an 800 px wide
    /// child, at the top of the page's scroll container, painted by the
    /// page's own view.
    fn with_inner_scroller(
        cx: &mut TestAppContext,
        handle: WindowHandle<LayerPage>,
    ) -> ScrollHandle {
        let inner = ScrollHandle::new();
        let tracked = inner.clone();
        handle
            .update(cx, |page, _, cx| {
                page.extra = Some(Rc::new(move || {
                    div()
                        .id("inner")
                        .overflow_x_scroll()
                        .track_scroll(&tracked)
                        .w(px(200.))
                        .h(px(200.))
                        .child(div().w(px(800.)).h(px(200.)).bg(rgb(0x123456)))
                        .into_any_element()
                }));
                cx.notify();
            })
            .unwrap();
        draw(cx, handle.into());
        inner
    }

    /// gpux chains wheel scrolls: the innermost container that can scroll
    /// takes the wheel, and the ones around it see it only once that one is
    /// at its edge. The inner container is painted by the view holding the
    /// outer one, which its scroll notifies: a change of the outer layer's
    /// content, which is painted afresh, not composited as it was.
    #[crate::test]
    fn a_wheel_scroll_of_a_container_inside_the_content_repaints(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        let inner = with_inner_scroller(cx, handle);
        draw(cx, window);
        let before = (inner.offset(), outer_offset(cx, handle));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Repaint));
        assert_ne!(
            inner.offset(),
            before.0,
            "the wheel scrolled the inner container"
        );
        assert_eq!(outer_offset(cx, handle), before.1, "and not the outer one");
    }

    fn outer_offset(
        cx: &mut TestAppContext,
        handle: WindowHandle<LayerPage>,
    ) -> crate::Point<Pixels> {
        handle
            .update(cx, |page, _, _| page.handle.offset())
            .unwrap()
    }

    /// A container inside the content scrolled in the frame that scrolls
    /// the outer one: a change of the outer layer's content, not a scroll
    /// of it (spec §6.6).
    #[crate::test]
    fn a_scroll_of_a_container_inside_the_content_repaints(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        promote(cx, window);
        let inner = with_inner_scroller(cx, handle);
        draw(cx, window);
        cx.run_until_parked();
        draw(cx, window);
        // The scrollbar scrolls the outer container alone, which composites.
        assert_eq!(drag_scrollbar(cx, handle, -10.), Some(Decision::Composite));
        // With the inner one scrolled in the same frame, it does not.
        frame_after(cx, window, |cx| {
            handle
                .update(cx, |page, _, cx| {
                    let offset = page.handle.offset();
                    let owner = cx.entity_id();
                    page.handle.publish_scrollbar_offset(
                        point(offset.x, offset.y - px(10.)),
                        owner,
                        cx,
                    );
                    inner.publish_scrollbar_offset(point(px(-100.), px(0.)), owner, cx);
                })
                .unwrap();
        });
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
        assert_eq!(inner.offset(), point(px(-100.), px(0.)));
    }
}

mod input {
    use crate::fast::layers::policy::Decision;
    use crate::fast::tests::layer_support::{decision, frame_after};
    use crate::fast::tests::layer_support::{draw, with_window};
    use crate::{
        AnyWindowHandle, AppContext as _, Bounds, Context, DispatchPhase, FocusHandle,
        HitboxBehavior, InteractiveElement as _, IntoElement, Modifiers, MouseButton,
        MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render,
        ScrollDelta, ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _,
        TestAppContext, TouchPhase, Window, WindowHandle, canvas, div, point, px, rgb,
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    const ROWS: usize = 40;
    const ROW_HEIGHT: f32 = 20.;

    /// What a listener inside a row saw: the row, the bounds of the hitbox
    /// it captured, and the event's position.
    type Seen = (usize, Bounds<Pixels>, Point<Pixels>);

    /// A 200 × 100 px scroll container of forty 20 px rows, 100 px wide, that
    /// can be clicked and dragged, at the top left of the window. Each row
    /// holds a canvas whose mouse-down listener notes the bounds of the
    /// hitbox it captured when painted. Row 2 can take focus and counts the
    /// keys it sees. The wheel scrolls it from beside the rows, where it
    /// hovers none of them. With `inner`, the rows are in a 200 px wide div
    /// that `inner` tracks, with a tooltip if `tooltip` is set.
    struct InputPage {
        handle: ScrollHandle,
        clicks: Rc<RefCell<Vec<usize>>>,
        keys: Rc<Cell<usize>>,
        focus: FocusHandle,
        seen: Rc<RefCell<Vec<Seen>>>,
        drags: Rc<RefCell<Vec<Seen>>>,
        inner: Option<ScrollHandle>,
        tooltip: bool,
        /// Paints the page's background translucent, which a layer's tiles
        /// cannot be cleared with.
        translucent: bool,
    }

    /// What a row of an [`InputPage`] is dragged as.
    struct DraggedRow;

    /// A canvas filling a row, whose mouse-down listener notes into `seen`
    /// the bounds of the hitbox it captured when painted.
    fn noting_canvas(index: usize, seen: Rc<RefCell<Vec<Seen>>>) -> impl IntoElement {
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |_, hitbox, window, _| {
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
                    if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                        seen.borrow_mut()
                            .push((index, hitbox.bounds, event.position));
                    }
                });
            },
        )
        .size_full()
    }

    impl Render for InputPage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let rows = (0..ROWS).map(|index| {
                let clicks = self.clicks.clone();
                let drags = self.drags.clone();
                let row = div()
                    .id(("row", index))
                    .w(px(100.))
                    .h(px(ROW_HEIGHT))
                    .bg(rgb(0x100000 + index as u32 * 0x10))
                    .on_click(move |_, _, _| clicks.borrow_mut().push(index))
                    .on_drag(DraggedRow, |_, _, _, cx| {
                        cx.new(|_| crate::fast::tests::layer_support::EmptyView)
                    })
                    .on_drag_move::<DraggedRow>(move |event, _, _| {
                        drags
                            .borrow_mut()
                            .push((index, event.bounds, event.event.position))
                    })
                    .child(noting_canvas(index, self.seen.clone()));
                if index == 2 {
                    let keys = self.keys.clone();
                    row.track_focus(&self.focus)
                        .on_key_down(move |_, _, _| keys.set(keys.get() + 1))
                } else {
                    row
                }
            });
            let scroller = div()
                .id("scroller")
                .overflow_y_scroll()
                .track_scroll(&self.handle)
                .w(px(200.))
                .h(px(100.));
            let scroller = match &self.inner {
                Some(inner) => {
                    let inner = div()
                        .id("inner")
                        .track_scroll(inner)
                        .w(px(200.))
                        .children(rows);
                    if self.tooltip {
                        scroller.child(inner.tooltip(|_, cx| {
                            cx.new(|_| crate::fast::tests::layer_support::EmptyView)
                                .into()
                        }))
                    } else {
                        scroller.child(inner)
                    }
                }
                None => scroller.children(rows),
            };
            let background = if self.translucent {
                crate::rgba(0xffffff80)
            } else {
                rgb(0xffffff)
            };
            div().size_full().bg(background).child(scroller)
        }
    }

    fn input_page(cx: &mut TestAppContext) -> WindowHandle<InputPage> {
        input_page_with(cx, None, false)
    }

    fn input_page_with(
        cx: &mut TestAppContext,
        inner: Option<ScrollHandle>,
        tooltip: bool,
    ) -> WindowHandle<InputPage> {
        let window = crate::fast::tests::layer_support::add_window(cx, move |_, cx| InputPage {
            handle: ScrollHandle::new(),
            clicks: Rc::default(),
            keys: Rc::default(),
            focus: cx.focus_handle(),
            seen: Rc::default(),
            drags: Rc::default(),
            inner,
            tooltip,
            translucent: false,
        });
        draw(cx, window.into());
        draw(cx, window.into());
        window
    }

    /// Scrolls by `dy` with the wheel beside the rows, returning what the
    /// scroll container decided in the frame that followed.
    fn scroll(cx: &mut TestAppContext, window: AnyWindowHandle, dy: f32) -> Option<Decision> {
        frame_after(cx, window, |cx| {
            with_window(cx, window, |window, cx| {
                window.dispatch_event(
                    crate::PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position: point(px(150.), px(20.)),
                        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                        modifiers: Default::default(),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
            })
        });
        decision(cx, window)
    }

    fn click(cx: &mut TestAppContext, window: AnyWindowHandle, position: Point<Pixels>) {
        dispatch(cx, window, [mouse_down(position), mouse_up(position)]);
    }

    fn mouse_down(position: Point<Pixels>) -> crate::PlatformInput {
        crate::PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        })
    }

    fn mouse_up(position: Point<Pixels>) -> crate::PlatformInput {
        crate::PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        })
    }

    fn mouse_move(position: Point<Pixels>, pressed: bool) -> crate::PlatformInput {
        crate::PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: pressed.then_some(MouseButton::Left),
            modifiers: Modifiers::default(),
        })
    }

    /// Dispatches `events` to `window` in one update, with no frame drawn
    /// between them unless dispatching one draws it.
    fn dispatch(
        cx: &mut TestAppContext,
        window: AnyWindowHandle,
        events: impl IntoIterator<Item = crate::PlatformInput>,
    ) {
        with_window(cx, window, |window, cx| {
            for event in events {
                window.dispatch_event(event, cx);
            }
        });
    }

    fn rebuilds(cx: &mut TestAppContext, window: AnyWindowHandle) -> u64 {
        with_window(cx, window, |window, _| window.fast_layers.input_rebuilds)
    }

    /// A page with layers, and one without, for the same history to be
    /// played in both.
    fn page_with_and_without_layers(
        cx: &mut TestAppContext,
    ) -> (WindowHandle<InputPage>, WindowHandle<InputPage>) {
        pages_with_and_without_layers(cx, None, None)
    }

    /// [`page_with_and_without_layers`], the rows of each tracked by the
    /// handle given for it.
    fn pages_with_and_without_layers(
        cx: &mut TestAppContext,
        with_inner: Option<ScrollHandle>,
        without_inner: Option<ScrollHandle>,
    ) -> (WindowHandle<InputPage>, WindowHandle<InputPage>) {
        let with = input_page_with(cx, with_inner, false);
        let without = input_page_with(cx, without_inner, false);
        without
            .update(cx, |_, window, _| {
                crate::fast::tests::layer_support::set_layers(window, false)
            })
            .unwrap();
        draw(cx, without.into());
        (with, without)
    }

    /// Scrolls the page five times by -20 px: promoted on the second
    /// scroll, painted at -40 px, and composited from the third on, at
    /// -100 px after the fifth. Returns the decisions of the last three.
    fn scroll_five_times(
        cx: &mut TestAppContext,
        window: AnyWindowHandle,
    ) -> Vec<Option<Decision>> {
        (0..5)
            .map(|_| scroll(cx, window, -20.))
            .collect::<Vec<_>>()
            .split_off(2)
    }

    fn clicks(cx: &mut TestAppContext, window: WindowHandle<InputPage>) -> Vec<usize> {
        window
            .update(cx, |page, _, _| page.clicks.borrow().clone())
            .unwrap()
    }

    /// Promotes the page's container, painting its layer at -40 px, and
    /// composites it twice, at -60 and -80 px.
    fn composite_twice(cx: &mut TestAppContext, window: AnyWindowHandle) {
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Repaint));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    }

    #[crate::test]
    fn composited_frames_keep_the_content_interactive(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = input_page(cx);
        let window = handle.into();
        composite_twice(cx, window);
        // Row 6 lies at 120 px in the content, at 40 px once scrolled by -80.
        let row_6 = with_window(cx, window, |window, _| {
            window.rendered_frame.hitboxes.iter().any(|hitbox| {
                hitbox.bounds.origin == point(px(0.), px(40.))
                    && hitbox.bounds.size.height == px(ROW_HEIGHT)
            })
        });
        assert!(row_6, "the row's hitbox is carried, moved by the scroll");
        click(cx, window, point(px(20.), px(50.)));
        assert_eq!(clicks(cx, handle), vec![6]);
    }

    #[crate::test]
    fn keyboard_focus_inside_survives_composite_frames(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = input_page(cx);
        let window: AnyWindowHandle = handle.into();
        handle
            .update(cx, |page, window, cx| window.focus(&page.focus, cx))
            .unwrap();
        draw(cx, window);
        composite_twice(cx, window);
        let focused = handle
            .update(cx, |page, window, _| {
                page.focus.is_focused(window)
                    && window
                        .rendered_frame
                        .dispatch_tree
                        .focusable_node_id(page.focus.id)
                        .is_some()
            })
            .unwrap();
        assert!(focused, "the focused row's dispatch node is carried");
        cx.simulate_keystrokes(window, "a");
        let keys = handle.update(cx, |page, _, _| page.keys.get()).unwrap();
        assert_eq!(keys, 1);
    }

    #[crate::test]
    fn click_after_scroll_hits_current_element(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (with, without) = page_with_and_without_layers(cx);
        assert_eq!(
            scroll_five_times(cx, with.into()),
            vec![Some(Decision::Composite); 3]
        );
        scroll_five_times(cx, without.into());
        // Row 7 lies at 140 px in the content, at 40 px once scrolled by -100.
        for window in [with, without] {
            click(cx, window.into(), point(px(20.), px(50.)));
            assert_eq!(clicks(cx, window), vec![7]);
        }
        let seen = |cx: &mut TestAppContext, window: WindowHandle<InputPage>| {
            window
                .update(cx, |page, _, _| page.seen.borrow().clone())
                .unwrap()
        };
        let seen_with = seen(cx, with);
        assert_eq!(
            seen_with,
            vec![(
                7,
                Bounds::new(point(px(0.), px(40.)), crate::size(px(100.), px(20.))),
                point(px(20.), px(50.))
            )]
        );
        assert_eq!(seen_with, seen(cx, without));
        assert_eq!(rebuilds(cx, with.into()), 1);
    }

    #[crate::test]
    fn drag_after_scroll_sees_current_bounds(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (with, without) = page_with_and_without_layers(cx);
        assert_eq!(
            scroll_five_times(cx, with.into()),
            vec![Some(Decision::Composite); 3]
        );
        scroll_five_times(cx, without.into());
        let drags = |cx: &mut TestAppContext, window: WindowHandle<InputPage>| {
            // The drag is the app's: it ends with the mouse up, before the
            // other window is dragged in.
            dispatch(
                cx,
                window.into(),
                [
                    mouse_down(point(px(20.), px(50.))),
                    mouse_move(point(px(20.), px(60.)), true),
                    mouse_move(point(px(20.), px(70.)), true),
                    mouse_up(point(px(20.), px(70.))),
                ],
            );
            window
                .update(cx, |page, _, _| page.drags.borrow().clone())
                .unwrap()
        };
        let drags_with = drags(cx, with);
        assert!(!drags_with.is_empty(), "the row was dragged");
        assert!(
            drags_with
                .iter()
                .all(|(index, bounds, _)| *index != 7 || bounds.origin.y == px(40.)),
            "{drags_with:?}"
        );
        assert_eq!(drags_with, drags(cx, without));
    }

    #[crate::test]
    fn wheel_events_do_not_rebuild(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = input_page(cx);
        let window = handle.into();
        scroll_five_times(cx, window);
        for _ in 0..10 {
            assert_eq!(scroll(cx, window, 5.), Some(Decision::Composite));
        }
        assert_eq!(rebuilds(cx, window), 0);
    }

    #[crate::test]
    fn a_mouse_move_after_scroll_rebuilds_once(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = input_page(cx);
        let window = handle.into();
        scroll_five_times(cx, window);
        dispatch(cx, window, [mouse_move(point(px(20.), px(50.)), false)]);
        assert_eq!(rebuilds(cx, window), 1);
        dispatch(cx, window, [mouse_move(point(px(30.), px(60.)), false)]);
        draw(cx, window);
        dispatch(cx, window, [mouse_move(point(px(40.), px(70.)), false)]);
        assert_eq!(rebuilds(cx, window), 1);
        // Outside the viewport, a move after a scroll does not rebuild. (The
        // first scroll moves the pointer off the rows, a hover change.)
        scroll(cx, window, -20.);
        scroll(cx, window, -20.);
        dispatch(cx, window, [mouse_move(point(px(300.), px(300.)), false)]);
        assert_eq!(rebuilds(cx, window), 1);
    }

    /// Whether a row's hitbox takes the pointer at `position`.
    fn row_hit_at(
        cx: &mut TestAppContext,
        window: AnyWindowHandle,
        position: Point<Pixels>,
    ) -> bool {
        with_window(cx, window, |window, _| {
            window.rendered_frame.hitboxes.iter().any(|hitbox| {
                hitbox.bounds.size.height == px(ROW_HEIGHT)
                    && hitbox
                        .bounds
                        .intersect(&hitbox.content_mask.bounds)
                        .contains(&position)
            })
        })
    }

    #[crate::test]
    fn hitboxes_in_overscan_do_not_hit(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = input_page(cx);
        let window = handle.into();
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Repaint));
        // Painted at -40 px, over the viewport and 100 px of overscan below
        // it: row 9 is painted at 140 px, below the container.
        assert!(!row_hit_at(cx, window, point(px(20.), px(150.))));
        assert!(row_hit_at(cx, window, point(px(20.), px(50.))));
        // Composited at -80 px, row 7, painted in the overscan at 100 px,
        // shows at 60 px, and takes the pointer there.
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        assert!(row_hit_at(cx, window, point(px(20.), px(70.))));
        assert!(!row_hit_at(cx, window, point(px(20.), px(110.))));
    }

    fn tooltip_requested(cx: &mut TestAppContext, window: AnyWindowHandle) -> bool {
        with_window(cx, window, |window, _| {
            window
                .rendered_frame
                .tooltip_requests
                .iter()
                .any(|request| request.is_some())
        })
    }

    #[crate::test]
    fn tooltips_requested_inside_are_dropped_on_composite(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let page = input_page_with(cx, Some(ScrollHandle::new()), true);
        let window: AnyWindowHandle = page.into();
        scroll_five_times(cx, window);
        // Hover the div around the rows until its tooltip shows.
        dispatch(cx, window, [mouse_move(point(px(150.), px(30.)), false)]);
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        draw(cx, window);
        draw(cx, window);
        assert!(tooltip_requested(cx, window));
        // Scrolled from outside, which keeps the tooltip up on today's path.
        let handle = page.update(cx, |page, _, _| page.handle.clone()).unwrap();
        let mut decisions = Vec::new();
        for offset in [-110., -120.] {
            crate::fast::tests::layer_support::set_offset(&handle, point(px(0.), px(offset)));
            draw(cx, window);
            decisions.push(crate::fast::tests::layer_support::decision(cx, window));
        }
        assert_eq!(decisions.last(), Some(&Some(Decision::Composite)));
        assert!(!tooltip_requested(cx, window));
    }

    #[crate::test]
    fn bounds_for_item_is_current_after_composited_scrolls(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (with_inner, without_inner) = (ScrollHandle::new(), ScrollHandle::new());
        let (with, without) = pages_with_and_without_layers(
            cx,
            Some(with_inner.clone()),
            Some(without_inner.clone()),
        );
        assert_eq!(
            scroll_five_times(cx, with.into()),
            vec![Some(Decision::Composite); 3]
        );
        scroll_five_times(cx, without.into());
        // Row 7 lies at 140 px in the content, at 40 px once scrolled by -100.
        assert_eq!(
            with_inner.bounds_for_item(7).map(|bounds| bounds.origin),
            Some(point(px(0.), px(40.)))
        );
        for row in 0..ROWS {
            assert_eq!(
                with_inner.bounds_for_item(row),
                without_inner.bounds_for_item(row)
            );
        }
        // The container's own handle holds its children's bounds before
        // scrolling, which prepainting the container keeps current.
        let outer = |cx: &mut TestAppContext, window: WindowHandle<InputPage>| {
            window
                .update(cx, |page, _, _| page.handle.bounds_for_item(0))
                .unwrap()
        };
        assert_eq!(outer(cx, with), outer(cx, without));
    }

    /// How often observers of the page's view were told it changed, from
    /// now on.
    fn count_observations(
        cx: &mut TestAppContext,
        window: WindowHandle<InputPage>,
    ) -> Rc<Cell<usize>> {
        let count = Rc::new(Cell::new(0));
        let entity = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        let counter = count.clone();
        cx.update(|cx| {
            cx.observe(&entity, move |_, _| counter.set(counter.get() + 1))
                .detach()
        });
        count
    }

    #[crate::test]
    fn a_rebuild_for_input_is_not_seen_by_observers(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (with, without) = page_with_and_without_layers(cx);
        let mut counts = Vec::new();
        for window in [with, without] {
            let count = count_observations(cx, window);
            scroll_five_times(cx, window.into());
            dispatch(
                cx,
                window.into(),
                [mouse_move(point(px(20.), px(50.)), false)],
            );
            cx.run_until_parked();
            counts.push(count.get());
        }
        assert_eq!(rebuilds(cx, with.into()), 1);
        assert_eq!(counts[0], counts[1]);
    }

    fn repaints(cx: &mut TestAppContext, window: AnyWindowHandle) -> u64 {
        with_window(cx, window, |window, _| window.fast_stats().layers_repainted)
    }

    /// A move that lands beside the content — on a scrollbar being dragged,
    /// say — does not rebuild a scrolled layer: only the content's own
    /// elements compare pointer positions with bounds they hold. A move onto
    /// the content does.
    #[crate::test]
    fn only_a_move_onto_the_content_rebuilds_it(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = input_page(cx);
        let window: AnyWindowHandle = handle.into();
        scroll_five_times(cx, window);
        for step in 0..6 {
            let y = px(50. + step as f32);
            dispatch(cx, window, [mouse_move(point(px(150.), y), true)]);
        }
        assert_eq!(rebuilds(cx, window), 0, "moves beside the rows");
        dispatch(cx, window, [mouse_move(point(px(20.), px(50.)), false)]);
        assert_eq!(rebuilds(cx, window), 1, "a move onto a row");
    }

    #[crate::test]
    fn moves_beside_the_content_neither_rebuild_nor_demote(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = input_page(cx);
        let window: AnyWindowHandle = handle.into();
        scroll_five_times(cx, window);
        // Scrolling with the pointer moving in the viewport between scrolls,
        // beside the rows, where it lands on none of the content: no move
        // rebuilds, and none changes the content.
        for step in 0..12 {
            assert_eq!(
                scroll(cx, window, 5.),
                Some(Decision::Composite),
                "step {step}"
            );
            let y = px(50. + (step % 2) as f32);
            let repainted = repaints(cx, window);
            dispatch(cx, window, [mouse_move(point(px(150.), y), false)]);
            assert_eq!(repaints(cx, window), repainted, "step {step}");
        }
        assert_eq!(rebuilds(cx, window), 0);
        let id = crate::fast::tests::layer_support::scroller_id(cx, window);
        let (demoted, changes) = with_window(cx, window, |window, _| {
            let layer = &window.fast_layers.layers[&id];
            (
                window.fast_layers.demotions,
                layer.policy.change_history.count_ones(),
            )
        });
        assert_eq!((demoted, changes), (0, 0));
    }

    #[crate::test]
    fn content_painted_without_tiles_is_not_rebuilt_for_input(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = input_page(cx);
        let window: AnyWindowHandle = handle.into();
        scroll_five_times(cx, window);
        // The layer's tiles cannot be cleared with a translucent background:
        // its content is painted straight into the frame, at the offset
        // shown.
        let repainted = repaints(cx, window);
        frame_after(cx, window, |cx| {
            handle
                .update(cx, |page, _, cx| {
                    page.translucent = true;
                    cx.notify();
                })
                .unwrap();
        });
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
        assert_eq!(repaints(cx, window), repainted, "painted without tiles");
        dispatch(cx, window, [mouse_move(point(px(20.), px(50.)), false)]);
        assert_eq!(rebuilds(cx, window), 0);
    }

    #[crate::test]
    fn bounds_for_item_matches_after_the_tracked_element_goes(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (with_inner, without_inner) = (ScrollHandle::new(), ScrollHandle::new());
        let (with, without) = pages_with_and_without_layers(
            cx,
            Some(with_inner.clone()),
            Some(without_inner.clone()),
        );
        assert_eq!(
            scroll_five_times(cx, with.into()),
            vec![Some(Decision::Composite); 3]
        );
        scroll_five_times(cx, without.into());
        // The rows' div stops being rendered; its handle, kept, holds the
        // bounds its children had when last shown, as without a layer.
        for window in [with, without] {
            window
                .update(cx, |page, _, cx| {
                    page.inner = None;
                    cx.notify();
                })
                .unwrap();
            draw(cx, window.into());
        }
        for _ in 0..3 {
            scroll(cx, with.into(), 5.);
            scroll(cx, without.into(), 5.);
        }
        for row in 0..ROWS {
            assert_eq!(
                with_inner.bounds_for_item(row),
                without_inner.bounds_for_item(row),
                "row {row}"
            );
        }
    }
}
