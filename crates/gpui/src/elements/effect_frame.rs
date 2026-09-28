use crate::{
    AbsoluteLength, AnyElement, App, Bounds, Corners, Element, ElementEffect, ElementId,
    GlobalElementId, InspectorElementId, IntoElement, IntoElementEffects, LayoutId, Length, Pixels,
    Point, ScreenEffectRegionId, Size, Window,
};

use super::frame_style::FrameStyle;

/// Creates a single-child frame for shader-backed effects.
#[track_caller]
pub fn effect_frame() -> EffectFrame {
    EffectFrame::default()
}

/// A lightweight single-child frame that emits shader-backed effects.
pub struct EffectFrame {
    pub(crate) style: FrameStyle,
    pub(crate) child: Option<AnyElement>,
    pub(crate) effects: Vec<ElementEffect>,
    pub(crate) screen_effect_region: Option<ScreenEffectRegionId>,
    pub(crate) source_location: Option<&'static core::panic::Location<'static>>,
}

impl EffectFrame {
    /// Sets the element's single child.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        debug_assert!(
            self.child.is_none(),
            "EffectFrame accepts exactly one child"
        );
        self.child = Some(child.into_any_element());
        self
    }

    /// Appends a shader-backed effect.
    pub fn effect(mut self, effect: impl Into<ElementEffect>) -> Self {
        self.effects.push(effect.into());
        self
    }

    /// Appends multiple shader-backed effects in order.
    pub fn effects(mut self, effects: impl IntoElementEffects) -> Self {
        self.effects.extend(effects.into_element_effects());
        self
    }

    /// Removes all shader-backed effects from this element.
    pub fn clear_effects(mut self) -> Self {
        self.effects.clear();
        self
    }

    /// Registers this element's bounds as a screen effect target region.
    pub fn screen_effect_region(mut self, id: impl Into<ScreenEffectRegionId>) -> Self {
        self.screen_effect_region = Some(id.into());
        self
    }

    /// Sets both layout dimensions.
    pub fn size(mut self, size: impl Into<Size<Length>>) -> Self {
        self.style = self.style.size(size);
        self
    }

    /// Sets the layout width.
    pub fn w(mut self, width: impl Into<Length>) -> Self {
        self.style = self.style.w(width);
        self
    }

    /// Sets the layout height.
    pub fn h(mut self, height: impl Into<Length>) -> Self {
        self.style = self.style.h(height);
        self
    }

    /// Sets the layout width to fill the parent.
    pub fn w_full(mut self) -> Self {
        self.style = self.style.w_full();
        self
    }

    /// Sets the layout height to fill the parent.
    pub fn h_full(mut self) -> Self {
        self.style = self.style.h_full();
        self
    }

    /// Sets both layout dimensions to fill the parent.
    pub fn size_full(mut self) -> Self {
        self.style = self.style.size_full();
        self
    }

    /// Sets the minimum layout width.
    pub fn min_w(mut self, width: impl Into<Length>) -> Self {
        self.style = self.style.min_w(width);
        self
    }

    /// Sets the minimum layout height.
    pub fn min_h(mut self, height: impl Into<Length>) -> Self {
        self.style = self.style.min_h(height);
        self
    }

    /// Sets the maximum layout width.
    pub fn max_w(mut self, width: impl Into<Length>) -> Self {
        self.style = self.style.max_w(width);
        self
    }

    /// Sets the maximum layout height.
    pub fn max_h(mut self, height: impl Into<Length>) -> Self {
        self.style = self.style.max_h(height);
        self
    }

    /// Uses absolute positioning for this element.
    pub fn absolute(mut self) -> Self {
        self.style = self.style.absolute();
        self
    }

    /// Uses relative positioning for this element.
    pub fn relative(mut self) -> Self {
        self.style = self.style.relative();
        self
    }

    /// Sets all inset edges to the same value.
    pub fn inset(mut self, inset: impl Into<Length>) -> Self {
        self.style = self.style.inset(inset);
        self
    }

    /// Sets all inset edges to zero.
    pub fn inset_0(mut self) -> Self {
        self.style = self.style.inset_0();
        self
    }

    /// Sets the left inset.
    pub fn left(mut self, left: impl Into<Length>) -> Self {
        self.style = self.style.left(left);
        self
    }

    /// Sets the right inset.
    pub fn right(mut self, right: impl Into<Length>) -> Self {
        self.style = self.style.right(right);
        self
    }

    /// Sets the top inset.
    pub fn top(mut self, top: impl Into<Length>) -> Self {
        self.style = self.style.top(top);
        self
    }

    /// Sets the bottom inset.
    pub fn bottom(mut self, bottom: impl Into<Length>) -> Self {
        self.style = self.style.bottom(bottom);
        self
    }

    /// Rotates the element in radians.
    pub fn rotate(mut self, radians: f32) -> Self {
        self.style = self.style.rotate(radians);
        self
    }

    /// Scales the element uniformly.
    pub fn scale(mut self, scale: f32) -> Self {
        self.style = self.style.scale(scale);
        self
    }

    /// Scales the element independently on each axis.
    pub fn scale_xy(mut self, x: f32, y: f32) -> Self {
        self.style = self.style.scale_xy(x, y);
        self
    }

    /// Translates the element during painting.
    pub fn translate(mut self, translation: Point<Pixels>) -> Self {
        self.style = self.style.translate(translation);
        self
    }

    /// Sets the transform origin as normalized x/y coordinates.
    pub fn transform_origin(mut self, origin: Point<f32>) -> Self {
        self.style = self.style.transform_origin(origin);
        self
    }

    /// Skews the element independently on each axis in radians.
    pub fn skew(mut self, x_radians: f32, y_radians: f32) -> Self {
        self.style = self.style.skew(x_radians, y_radians);
        self
    }

    /// Skews the element on the x axis in radians.
    pub fn skew_x(mut self, radians: f32) -> Self {
        self.style = self.style.skew_x(radians);
        self
    }

    /// Skews the element on the y axis in radians.
    pub fn skew_y(mut self, radians: f32) -> Self {
        self.style = self.style.skew_y(radians);
        self
    }

    /// Sets the opacity used while painting effects and child content.
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.style = self.style.opacity(opacity);
        self
    }

    /// Sets the same corner radius on every corner.
    pub fn rounded(mut self, radius: impl Into<AbsoluteLength>) -> Self {
        self.style = self.style.rounded(radius);
        self
    }

    /// Sets individual corner radii.
    pub fn corner_radii(mut self, corner_radii: impl Into<Corners<AbsoluteLength>>) -> Self {
        self.style = self.style.corner_radii(corner_radii);
        self
    }

    /// Clips descendant content to the element bounds.
    pub fn clip(mut self) -> Self {
        self.style = self.style.clip();
        self
    }

    /// Disables descendant content clipping.
    pub fn unclip(mut self) -> Self {
        self.style = self.style.unclip();
        self
    }
}

impl Default for EffectFrame {
    #[track_caller]
    fn default() -> Self {
        Self {
            style: FrameStyle::default(),
            child: None,
            effects: Vec::new(),
            screen_effect_region: None,
            source_location: Some(core::panic::Location::caller()),
        }
    }
}

impl Element for EffectFrame {
    type RequestLayoutState = Option<LayoutId>;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        self.source_location
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let child_layout_id = self
            .child
            .as_mut()
            .map(|child| child.request_layout(window, cx));
        let style = self.style.layout_style();
        let layout_id = window.request_layout(style, child_layout_id.iter().copied(), cx);
        (layout_id, child_layout_id)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let style = self.style.layout_style();
        window.with_content_mask(style.overflow_mask(bounds, window.rem_size()), |window| {
            if let Some(child) = self.child.as_mut() {
                child.prepaint(window, cx);
            }
        });
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let style = self.style.layout_style();
        window.with_element_opacity(style.opacity, |window| {
            style.paint(bounds, window, cx, |window, cx| {
                let rem_size = window.rem_size();
                let corner_radii = style
                    .corner_radii
                    .to_pixels(rem_size)
                    .clamp_radii_for_quad_size(bounds.size);

                let layer_effects = self.effects.iter().any(ElementEffect::paints_after_content);
                if layer_effects {
                    let backdrop = self
                        .effects
                        .iter()
                        .filter(|effect| !effect.paints_after_content())
                        .cloned()
                        .collect::<Vec<_>>();
                    window.paint_visual_effects(bounds, corner_radii, &backdrop);
                } else {
                    window.paint_visual_effects(bounds, corner_radii, &self.effects);
                }
                if let Some(region_id) = self.screen_effect_region.clone() {
                    window.paint_screen_effect_region(region_id, bounds, corner_radii);
                }

                window.with_content_mask(
                    style.overflow_mask(bounds, window.rem_size()),
                    |window| {
                        if let Some(child) = self.child.as_mut() {
                            child.paint(window, cx);
                        }
                    },
                );

                // Layer effects blur what the child painted, reaching past the bounds.
                if layer_effects {
                    for effect in self
                        .effects
                        .iter()
                        .filter(|effect| effect.paints_after_content())
                    {
                        let outset = effect.outset();
                        let radii = Corners {
                            top_left: corner_radii.top_left + outset,
                            top_right: corner_radii.top_right + outset,
                            bottom_right: corner_radii.bottom_right + outset,
                            bottom_left: corner_radii.bottom_left + outset,
                        };
                        window.paint_visual_effects(
                            bounds.dilate(outset),
                            radii,
                            std::slice::from_ref(effect),
                        );
                    }
                }
            });
        });
    }
}

impl IntoElement for EffectFrame {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
