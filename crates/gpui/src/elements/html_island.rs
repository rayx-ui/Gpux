use refineable::Refineable as _;

use crate::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, SharedString, Style, StyleRefinement, Styled, Window,
};

/// Construct an experimental HTML island element with a stable id.
///
/// The supplied id is used to correlate the Rust element with browser DOM and
/// renderer resources on supported WASM targets. The HTML content is trusted
/// application markup; sanitize untrusted input before passing it to this API.
pub fn html_island(id: impl Into<ElementId>) -> HtmlIsland {
    HtmlIsland {
        id: id.into(),
        trusted_html: SharedString::default(),
        input_mode: HtmlIslandInputMode::GpuiOwned,
        fallback: None,
        style: StyleRefinement::default(),
    }
}

/// Controls whether browser DOM or GPUI owns interaction for an HTML island.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlIslandInputMode {
    /// Browser DOM receives pointer, focus, keyboard, composition, and
    /// accessibility behavior first.
    DomOwned,
    /// DOM is rendered but GPUI keeps owning pointer and focus behavior.
    GpuiOwned,
}

/// Experimental GPUI element for trusted HTML content on WASM.
///
/// Native and unsupported web targets paint the configured fallback element when
/// one is present. Browser DOM hosting and scene compositing are implemented in
/// later slices of the `html-island-wasm` epic.
pub struct HtmlIsland {
    id: ElementId,
    trusted_html: SharedString,
    input_mode: HtmlIslandInputMode,
    fallback: Option<AnyElement>,
    style: StyleRefinement,
}

impl HtmlIsland {
    /// Set the trusted HTML markup for this island.
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

        let _ = (&self.trusted_html, self.input_mode);
        style.paint(bounds, window, cx, |_window, _cx| {});
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
        Context, Hsla, IntoElement, Render, Styled as _, TestAppContext, Window, div, hsla,
        html_island, px, size,
    };

    struct FallbackView;

    impl Render for FallbackView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            html_island("fallback-island")
                .fallback(div().w(px(40.)).h(px(40.)).bg(hsla(0.25, 0.75, 0.5, 1.0)))
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
}
