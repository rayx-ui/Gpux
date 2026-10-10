use crate::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window,
};

/// Builds a `Deferred` element, which delays the layout and paint of its child.
pub fn deferred(child: impl IntoElement) -> Deferred {
    Deferred {
        child: Some(child.into_any_element()),
        id: None,
        priority: 0,
        content_mask_mode: DeferredContentMaskMode::Current,
    }
}

/// An element which delays the painting of its child until after all of
/// its ancestors, while keeping its layout as part of the current element tree.
pub struct Deferred {
    child: Option<AnyElement>,
    id: Option<ElementId>,
    priority: usize,
    content_mask_mode: DeferredContentMaskMode,
}

#[derive(Clone, Copy)]
enum DeferredContentMaskMode {
    Current,
    Unclipped,
}

impl Deferred {
    /// Sets the id for this element.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the `priority` value of the `deferred` element, which
    /// determines the drawing order relative to other deferred elements,
    /// with higher values being drawn on top.
    pub fn with_priority(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }

    /// Paints this deferred element without inheriting the current content mask.
    ///
    /// This is intended for top-level overlays such as menus, popovers, tooltips,
    /// and date-picker calendars that must escape scroll or dialog clipping.
    pub fn unclipped(mut self) -> Self {
        self.content_mask_mode = DeferredContentMaskMode::Unclipped;
        self
    }
}

impl Element for Deferred {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        self.id.clone()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let layout_id = self.child.as_mut().unwrap().request_layout(window, cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let child = self.child.take().unwrap();
        let element_offset = window.element_offset();
        let content_mask = match self.content_mask_mode {
            DeferredContentMaskMode::Current => {
                Some(crate::fast::layers::paint::unlayered_content_mask(window))
            }
            DeferredContentMaskMode::Unclipped => None,
        };
        window.defer_draw(child, element_offset, self.priority, content_mask)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }
}

impl IntoElement for Deferred {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Deferred {
    /// Sets a priority for the element. A higher priority conceptually means painting the element
    /// on top of deferred draws with a lower priority (i.e. closer to the viewer).
    pub fn priority(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Color, Context, Hsla, IntoElement, Render, ScaledPixels, TestAppContext, Window, deferred,
        div, hsla, point, prelude::*, px, size,
    };

    struct DeferredMaskTestView {
        unclipped: bool,
    }

    impl Render for DeferredMaskTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let overlay = div()
                .absolute()
                .left(px(0.))
                .top(px(0.))
                .w(px(120.))
                .h(px(120.))
                .bg(hsla(1., 0., 0.5, 1.));

            let overlay = if self.unclipped {
                deferred(overlay).unclipped()
            } else {
                deferred(overlay)
            };

            div().size_full().child(
                div()
                    .absolute()
                    .left(px(20.))
                    .top(px(20.))
                    .w(px(40.))
                    .h(px(40.))
                    .overflow_hidden()
                    .child(overlay),
            )
        }
    }

    fn sp(value: f32) -> ScaledPixels {
        ScaledPixels::from(value)
    }

    fn red_quad_content_mask(
        unclipped: bool,
        cx: &mut TestAppContext,
    ) -> crate::Bounds<crate::ScaledPixels> {
        let window = cx.open_window(size(px(200.), px(200.)), move |_, _| DeferredMaskTestView {
            unclipped,
        });

        cx.run_until_parked();

        window
            .update(cx, |_, window, _| {
                window
                    .rendered_frame
                    .scene
                    .quads
                    .iter()
                    .find(|quad| {
                        quad.background.solid
                            == Color::from(Hsla {
                                h: 1.,
                                s: 0.,
                                l: 0.5,
                                a: 1.,
                            })
                            .paint_color()
                    })
                    .map(|quad| quad.content_mask.bounds)
            })
            .unwrap()
            .expect("red deferred quad should be painted")
    }

    #[gpux::test]
    fn deferred_inherits_current_content_mask_by_default(cx: &mut TestAppContext) {
        let mask = red_quad_content_mask(false, cx);

        assert_eq!(mask.origin, point(sp(40.), sp(40.)));
        assert_eq!(mask.size, size(sp(80.), sp(80.)));
    }

    #[gpux::test]
    fn unclipped_deferred_escapes_current_content_mask(cx: &mut TestAppContext) {
        let mask = red_quad_content_mask(true, cx);

        assert_eq!(mask.origin, point(sp(0.), sp(0.)));
        assert_eq!(mask.size, size(sp(400.), sp(400.)));
    }
}
#[cfg(test)]
mod nested_deferred_tests {
    use crate::{
        Context, Entity, StyleRefinement, TestAppContext, Window, anchored, deferred, div, point,
        prelude::*, px, size,
    };

    /// A stand-in for a dock panel hosting a popover (deferred draw) whose
    /// content opens another popover (a deferred draw created while
    /// prepainting the first one's content).
    struct PanelView;

    impl Render for PanelView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().key_context("Panel").size_full().child(
                deferred(
                    anchored().position(point(px(10.), px(10.))).child(
                        div().key_context("Popover").w(px(200.)).h(px(200.)).child(
                            deferred(
                                anchored().position(point(px(30.), px(30.))).child(
                                    div()
                                        .key_context("NestedMenu")
                                        .debug_selector(|| "NESTED_MENU".into())
                                        .w(px(50.))
                                        .h(px(50.)),
                                ),
                            )
                            .with_priority(2),
                        ),
                    ),
                )
                .with_priority(1),
            )
        }
    }

    struct RootView {
        panel: Entity<PanelView>,
    }

    impl Render for RootView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().key_context("Root").size_full().child(
                self.panel
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
        }
    }

    /// Regression test for a crash with nested deferred draws (e.g. a popover
    /// menu inside a popover hosted by a cached dock panel). Prepaint indices
    /// recorded during the deferred draw rounds must index the same
    /// `deferred_draws` vector that `reuse_prepaint` slices on the next frame;
    /// previously they were measured against a transient per-round vector, so
    /// reusing the panel's subtree grafted the wrong deferred draws and
    /// panicked in the dispatch tree.
    #[gpux::test]
    fn test_nested_deferred_draws_with_reused_views(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(800.), px(600.)), |_, cx| {
            let panel = cx.new(|_| PanelView);
            RootView { panel }
        });
        cx.run_until_parked();

        let menu_bounds = window
            .update(cx, |_, window, _| {
                window
                    .rendered_frame
                    .debug_bounds
                    .get("NESTED_MENU")
                    .copied()
            })
            .unwrap()
            .expect("NESTED_MENU debug bounds not found");
        assert_eq!(menu_bounds.size, size(px(50.), px(50.)));

        // Re-render only the root view; the panel is cached, so its subtree -
        // including both deferred draw records - is reused from the previous
        // frame.
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.run_until_parked();

        // Reuse the subtree a second time, exercising ranges that were
        // themselves recorded during a reused frame.
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.run_until_parked();

        // Re-render the panel itself again to prove the popovers still draw.
        window
            .update(cx, |root, _, cx| {
                root.panel.update(cx, |_, cx| cx.notify());
            })
            .unwrap();
        cx.run_until_parked();

        window
            .update(cx, |_, window, _| {
                assert_eq!(window.rendered_frame.deferred_draws.len(), 2);
                assert!(
                    window
                        .rendered_frame
                        .debug_bounds
                        .contains_key("NESTED_MENU")
                );
            })
            .unwrap();
    }
}
