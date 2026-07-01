use refineable::Refineable as _;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use crate::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, HtmlIslandInputMode,
    InspectorElementId, IntoElement, LayoutId, Pixels, SharedString, Style, StyleRefinement,
    Styled, Window,
};

/// Construct an experimental HTML island element with a stable id.
///
/// The supplied id is used to correlate the Rust element with browser DOM and
/// renderer resources on supported WASM targets. The HTML content is trusted
/// application markup; sanitize untrusted input before passing it to this API.
///
/// This element is not a production browser-support guarantee, an HTML
/// sanitizer, an iframe/webview host, or a cross-origin rendering workaround.
/// Native and unsupported browser targets render the configured fallback when
/// one is present.
pub fn html_island(id: impl Into<ElementId>) -> HtmlIsland {
    HtmlIsland {
        id: id.into(),
        trusted_html: SharedString::default(),
        input_mode: HtmlIslandInputMode::GpuiOwned,
        fallback: None,
        style: StyleRefinement::default(),
    }
}

/// Experimental GPUI element for trusted HTML content on WASM.
///
/// Native and unsupported web targets paint the configured fallback element when
/// one is present. Browser DOM hosting and scene compositing are implemented in
/// later slices of the `html-island-wasm` epic.
///
/// Treat the HTML string as trusted application markup. Sanitize any user,
/// remote, or otherwise untrusted content before calling [`HtmlIsland::html`].
pub struct HtmlIsland {
    id: ElementId,
    trusted_html: SharedString,
    input_mode: HtmlIslandInputMode,
    fallback: Option<AnyElement>,
    style: StyleRefinement,
}

impl HtmlIsland {
    /// Set the trusted HTML markup for this island.
    ///
    /// This method does not sanitize, sandbox, or fetch remote content.
    pub fn html(mut self, trusted_html: impl Into<SharedString>) -> Self {
        self.trusted_html = trusted_html.into();
        self
    }

    /// Set whether browser DOM or GPUI owns interaction for this island.
    pub fn input_mode(mut self, input_mode: HtmlIslandInputMode) -> Self {
        self.input_mode = input_mode;
        self
    }

    /// Set the element rendered on native and unsupported web targets.
    pub fn fallback(mut self, fallback: impl IntoElement) -> Self {
        self.fallback = Some(fallback.into_any_element());
        self
    }
}

fn trusted_html_revision(trusted_html: &SharedString) -> u64 {
    let mut hasher = DefaultHasher::new();
    trusted_html.hash(&mut hasher);
    hasher.finish()
}

impl Element for HtmlIsland {
    type RequestLayoutState = Style;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
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
    ) -> (LayoutId, Self::RequestLayoutState) {
        if let Some(fallback) = self.fallback.as_mut() {
            return (fallback.request_layout(window, cx), Style::default());
        }

        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style.clone(), [], cx);
        (layout_id, style)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if let Some(fallback) = self.fallback.as_mut() {
            fallback.prepaint(window, cx);
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(fallback) = self.fallback.as_mut() {
            fallback.paint(window, cx);
            return;
        }

        let id = self.id.clone();
        let input_mode = self.input_mode;
        let content_revision = trusted_html_revision(&self.trusted_html);
        let trusted_html = self.trusted_html.clone();
        style.paint(bounds, window, cx, move |window, _cx| {
            window.paint_html_island(id, bounds, input_mode, content_revision, trusted_html);
        });
    }
}

impl IntoElement for HtmlIsland {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for HtmlIsland {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Context, Hsla, HtmlIslandInputMode, IntoElement, ParentElement as _, Render, ScaledPixels,
        Styled as _, TestAppContext, Window, div, hsla, html_island, point, px, size,
    };

    struct FallbackView;
    struct PrimitiveView;

    impl Render for FallbackView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            html_island("fallback-island")
                .fallback(div().w(px(40.)).h(px(40.)).bg(hsla(0.25, 0.75, 0.5, 1.0)))
        }
    }

    impl Render for PrimitiveView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .absolute()
                    .left(px(20.))
                    .top(px(20.))
                    .w(px(40.))
                    .h(px(40.))
                    .overflow_hidden()
                    .child(
                        html_island("scene-island")
                            .html("<section>Island</section>")
                            .input_mode(HtmlIslandInputMode::DomOwned)
                            .absolute()
                            .left(px(0.))
                            .top(px(0.))
                            .w(px(120.))
                            .h(px(120.)),
                    ),
            )
        }
    }

    #[gpux::test]
    fn html_island_paints_fallback_on_native_targets(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| FallbackView);
        cx.run_until_parked();

        let painted_fallback = window
            .update(cx, |_, window, _| {
                window.rendered_frame.scene.quads.iter().any(|quad| {
                    quad.background.solid
                        == Hsla {
                            h: 0.25,
                            s: 0.75,
                            l: 0.5,
                            a: 1.0,
                        }
                })
            })
            .unwrap();

        assert!(painted_fallback);
    }

    #[gpux::test]
    fn html_island_paints_scene_primitive_with_scaled_bounds_and_mask(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(200.), px(200.)), |_, _| PrimitiveView);
        cx.run_until_parked();

        let primitive = window
            .update(cx, |_, window, _| {
                window.rendered_frame.scene.html_islands.first().cloned()
            })
            .unwrap()
            .expect("html island primitive should be painted");

        assert_eq!(primitive.id, "scene-island".into());
        assert_eq!(primitive.input_mode, HtmlIslandInputMode::DomOwned);
        assert_eq!(primitive.trusted_html.as_ref(), "<section>Island</section>");
        assert_eq!(
            primitive.bounds,
            crate::Bounds::new(
                point(ScaledPixels(40.), ScaledPixels(40.)),
                size(ScaledPixels(240.), ScaledPixels(240.))
            )
        );
        assert_eq!(
            primitive.logical_bounds,
            crate::Bounds::new(point(px(20.), px(20.)), size(px(120.), px(120.)))
        );
        assert_eq!(
            primitive.content_mask.bounds,
            crate::Bounds::new(
                point(ScaledPixels(40.), ScaledPixels(40.)),
                size(ScaledPixels(80.), ScaledPixels(80.))
            )
        );
    }
}
