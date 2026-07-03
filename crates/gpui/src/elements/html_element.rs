use refineable::Refineable as _;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use crate::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, HtmlElementContentFit,
    HtmlElementInputMode, HtmlElementLifetime, HtmlElementWheelBehavior, InspectorElementId,
    IntoElement, LayoutId, Pixels, SharedString, Style, StyleRefinement, Styled, Window,
};

/// Construct a trusted browser DOM overlay element with a stable id.
///
/// `html_element` is separate from [`crate::html_island`]. HTML islands are an
/// experimental HTML-in-canvas path that uploads direct canvas-child DOM into
/// WebGPU textures. HTML elements keep browser-owned DOM as a real overlay
/// above the GPUI canvas on supported WASM/browser targets, which is the right
/// model for iframe, payment, video, game, and other provider-owned content.
///
/// The id is used to correlate GPUI frame metadata with browser DOM roots. Use
/// a stable application id and stable iframe `src` values when you want browser
/// iframe/session state to survive normal GPUI re-renders.
///
/// The HTML content is trusted application markup. This API does not sanitize,
/// sandbox, or fetch remote content; sanitize untrusted content before passing
/// it to [`HtmlElement::trusted_html`].
pub fn html_element(id: impl Into<ElementId>) -> HtmlElement {
    HtmlElement {
        id: id.into(),
        trusted_html: SharedString::default(),
        input_mode: HtmlElementInputMode::GpuiOwned,
        lifetime: HtmlElementLifetime::PruneWhenAbsent,
        content_fit: HtmlElementContentFit::Natural,
        wheel_behavior: HtmlElementWheelBehavior::Browser,
        extra_styles: SharedString::default(),
        fallback: None,
        style: StyleRefinement::default(),
    }
}

/// Browser DOM overlay element for trusted application HTML.
///
/// On supported WASM/browser targets, `HtmlElement` emits frame metadata that
/// `gpui_web` uses to keep a real DOM subtree aligned above the GPUI canvas.
/// Native and unsupported targets can render explicit fallback content.
///
/// Do not use this as a sanitizer or iframe policy bypass. Cross-origin iframe
/// internals remain browser/provider-owned, and GPUI cannot draw above iframe
/// content because the DOM overlay sits above the canvas.
///
/// V1 alignment supports rectangular CSS-pixel bounds and rectangular content
/// masks. Rounded clips, non-rectangular masks, rotation, skew, perspective,
/// and other complex transforms do not clip the browser DOM subtree like they
/// clip canvas-rendered GPUI content.
///
/// ```rust,ignore
/// html_element("payment-provider")
///     .trusted_html(r#"
///         <iframe
///           src="https://provider.example/checkout"
///           title="Checkout provider"
///           allow="payment; clipboard-write"
///           sandbox="allow-forms allow-scripts allow-popups allow-same-origin"
///           style="width:100%;height:100%;border:0;display:block"></iframe>
///     "#)
///     .input_mode(HtmlElementInputMode::DomOwned)
///     .keep_alive(true);
/// ```
///
/// ```rust,ignore
/// html_element("decorative-provider-preview")
///     .trusted_html("<iframe title=\"Preview\" srcdoc=\"ready\"></iframe>")
///     .input_mode(HtmlElementInputMode::GpuiOwned);
/// ```
pub struct HtmlElement {
    id: ElementId,
    trusted_html: SharedString,
    input_mode: HtmlElementInputMode,
    lifetime: HtmlElementLifetime,
    content_fit: HtmlElementContentFit,
    wheel_behavior: HtmlElementWheelBehavior,
    extra_styles: SharedString,
    fallback: Option<AnyElement>,
    style: StyleRefinement,
}

impl HtmlElement {
    /// Set trusted application HTML for the browser DOM overlay root.
    ///
    /// Content is replaced in the browser only when the derived content
    /// revision changes. Keep iframe `src` values stable to avoid provider
    /// reloads.
    pub fn trusted_html(mut self, trusted_html: impl Into<SharedString>) -> Self {
        self.trusted_html = trusted_html.into();
        self
    }

    /// Set whether browser DOM or GPUI owns pointer and focus interaction.
    pub fn input_mode(mut self, input_mode: HtmlElementInputMode) -> Self {
        self.input_mode = input_mode;
        self
    }

    /// Set the lifecycle policy for absent or fully clipped overlay roots.
    pub fn lifetime(mut self, lifetime: HtmlElementLifetime) -> Self {
        self.lifetime = lifetime;
        self
    }

    /// Preserve the browser DOM root while absent or offscreen.
    ///
    /// Use stable provider/session ids with keep-alive roots. Do not use row
    /// indices, random render-time ids, or otherwise unstable ids for provider
    /// iframes that must retain browsing context.
    pub fn keep_alive(mut self, keep_alive: bool) -> Self {
        self.lifetime = if keep_alive {
            HtmlElementLifetime::KeepAliveHidden
        } else {
            HtmlElementLifetime::PruneWhenAbsent
        };
        self
    }

    /// Set how the trusted HTML subtree fits the GPUI layout box.
    ///
    /// `Fill` adds a browser-side class that makes top-level trusted HTML
    /// children fill the overlay root. Use normal `Styled` width, height, max
    /// width, flex, and `aspect_ratio(...)` APIs to control the RayX/GPUI
    /// layout box itself.
    pub fn content_fit(mut self, content_fit: HtmlElementContentFit) -> Self {
        self.content_fit = content_fit;
        self
    }

    /// Set how wheel events over this DOM overlay interact with GPUI scroll.
    ///
    /// `ForwardIfObservable` can forward same-document DOM wheel events.
    /// `CaptureForward` also works over iframes by adding a transparent capture
    /// layer, but that layer owns pointer hit testing while active.
    pub fn wheel_behavior(mut self, wheel_behavior: HtmlElementWheelBehavior) -> Self {
        self.wheel_behavior = wheel_behavior;
        self
    }

    /// Apply additional trusted CSS declarations to the root clipping element.
    ///
    /// The declarations are intended for decoration such as borders,
    /// border-radius, background, and shadows. Runtime geometry properties such
    /// as position, width, height, transform, visibility, opacity, and pointer
    /// behavior remain controlled by GPUI/web synchronization and are re-applied
    /// after these extra declarations.
    pub fn extra_styles(mut self, extra_styles: impl Into<SharedString>) -> Self {
        self.extra_styles = extra_styles.into();
        self
    }

    /// Set the element rendered on native and unsupported paths.
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

impl Element for HtmlElement {
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
        let lifetime = self.lifetime;
        let content_fit = self.content_fit;
        let wheel_behavior = self.wheel_behavior;
        let content_revision = trusted_html_revision(&self.trusted_html);
        let trusted_html = self.trusted_html.clone();
        let extra_styles = self.extra_styles.clone();
        style.paint(bounds, window, cx, move |window, _cx| {
            window.paint_html_element(
                id,
                bounds,
                input_mode,
                lifetime,
                content_fit,
                wheel_behavior,
                content_revision,
                trusted_html,
                extra_styles,
            );
        });
    }
}

impl IntoElement for HtmlElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for HtmlElement {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Context, Hsla, HtmlElementContentFit, HtmlElementInputMode, HtmlElementLifetime,
        HtmlElementWheelBehavior, IntoElement, ParentElement as _, Render, ScaledPixels,
        Styled as _, TestAppContext, Window, div, hsla, html_element, point, px, size,
    };

    struct FallbackView;
    struct PrimitiveView;

    impl Render for FallbackView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            html_element("fallback-element")
                .fallback(div().w(px(40.)).h(px(40.)).bg(hsla(0.58, 0.7, 0.5, 1.0)))
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
                        html_element("scene-element")
                            .trusted_html("<iframe title=\"Provider\"></iframe>")
                            .input_mode(HtmlElementInputMode::DomOwned)
                            .content_fit(HtmlElementContentFit::Fill)
                            .wheel_behavior(HtmlElementWheelBehavior::CaptureForward)
                            .extra_styles("border: 1px solid #fff; border-radius: 8px;")
                            .keep_alive(true)
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
    fn html_element_paints_fallback_on_native_targets(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| FallbackView);
        cx.run_until_parked();

        let painted_fallback = window
            .update(cx, |_, window, _| {
                window.rendered_frame.scene.quads.iter().any(|quad| {
                    quad.background.solid
                        == Hsla {
                            h: 0.58,
                            s: 0.7,
                            l: 0.5,
                            a: 1.0,
                        }
                })
            })
            .unwrap();

        assert!(painted_fallback);
    }

    #[gpux::test]
    fn html_element_paints_scene_primitive_with_logical_bounds_mask_and_lifetime(
        cx: &mut TestAppContext,
    ) {
        let window = cx.open_window(size(px(200.), px(200.)), |_, _| PrimitiveView);
        cx.run_until_parked();

        let primitive = window
            .update(cx, |_, window, _| {
                window.rendered_frame.scene.html_elements.first().cloned()
            })
            .unwrap()
            .expect("html element primitive should be painted");

        assert_eq!(primitive.id, "scene-element".into());
        assert_eq!(primitive.input_mode, HtmlElementInputMode::DomOwned);
        assert_eq!(primitive.lifetime, HtmlElementLifetime::KeepAliveHidden);
        assert_eq!(primitive.content_fit, HtmlElementContentFit::Fill);
        assert_eq!(
            primitive.wheel_behavior,
            HtmlElementWheelBehavior::CaptureForward
        );
        assert_eq!(
            primitive.extra_styles.as_ref(),
            "border: 1px solid #fff; border-radius: 8px;"
        );
        assert_eq!(
            primitive.trusted_html.as_ref(),
            "<iframe title=\"Provider\"></iframe>"
        );
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
            primitive.logical_content_mask.bounds,
            crate::Bounds::new(point(px(20.), px(20.)), size(px(40.), px(40.)))
        );
    }
}
