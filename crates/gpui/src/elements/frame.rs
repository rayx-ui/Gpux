use crate::{
    AbsoluteLength, AnyElement, App, Bounds, Corners, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Length, Pixels, Point, Size, Window,
};

use super::frame_style::FrameStyle;

/// Creates a lightweight single-child geometry container.
#[track_caller]
pub fn frame() -> Frame {
    Frame::default()
}

/// A lightweight single-child geometry container.
pub struct Frame {
    pub(crate) style: FrameStyle,
    pub(crate) child: Option<AnyElement>,
    child_offset: Point<Pixels>,
    pub(crate) source_location: Option<&'static core::panic::Location<'static>>,
}

impl Frame {
    /// Sets the frame's single child element.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        debug_assert!(self.child.is_none(), "Frame accepts exactly one child");
        self.child = Some(child.into_any_element());
        self
    }

    /// Offsets the child visually without contributing to this frame's layout.
    ///
    /// Unlike a paint-only transform, this offset is established during
    /// prepaint so descendant hitboxes and authored semantics use the same
    /// visual position as descendant painting.
    pub fn child_offset(mut self, offset: Point<Pixels>) -> Self {
        self.child_offset = offset;
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

    /// Uses absolute positioning for this frame.
    pub fn absolute(mut self) -> Self {
        self.style = self.style.absolute();
        self
    }

    /// Uses relative positioning for this frame.
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

    /// Rotates the frame in radians.
    pub fn rotate(mut self, radians: f32) -> Self {
        self.style = self.style.rotate(radians);
        self
    }

    /// Scales the frame uniformly.
    pub fn scale(mut self, scale: f32) -> Self {
        self.style = self.style.scale(scale);
        self
    }

    /// Scales the frame independently on each axis.
    pub fn scale_xy(mut self, x: f32, y: f32) -> Self {
        self.style = self.style.scale_xy(x, y);
        self
    }

    /// Translates the frame during painting.
    pub fn translate(mut self, translation: Point<Pixels>) -> Self {
        self.style = self.style.translate(translation);
        self
    }

    /// Sets the transform origin as normalized x/y coordinates.
    pub fn transform_origin(mut self, origin: Point<f32>) -> Self {
        self.style = self.style.transform_origin(origin);
        self
    }

    /// Skews the frame independently on each axis in radians.
    pub fn skew(mut self, x_radians: f32, y_radians: f32) -> Self {
        self.style = self.style.skew(x_radians, y_radians);
        self
    }

    /// Skews the frame on the x axis in radians.
    pub fn skew_x(mut self, radians: f32) -> Self {
        self.style = self.style.skew_x(radians);
        self
    }

    /// Skews the frame on the y axis in radians.
    pub fn skew_y(mut self, radians: f32) -> Self {
        self.style = self.style.skew_y(radians);
        self
    }

    /// Sets the opacity used while painting this frame and its child.
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

    /// Clips descendant content to the frame bounds.
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

impl Default for Frame {
    #[track_caller]
    fn default() -> Self {
        Self {
            style: FrameStyle::default(),
            child: None,
            child_offset: Point::default(),
            source_location: Some(core::panic::Location::caller()),
        }
    }
}

impl Element for Frame {
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
        // Painting applies the opacity and transform to the child, so cached
        // views below must see them while prepainting.
        let transformation = style.transformation(bounds, window.scale_factor());
        window.with_element_paint_context(style.opacity, transformation, |window| {
            window.with_content_mask(style.overflow_mask(bounds, window.rem_size()), |window| {
                if let Some(child) = self.child.as_mut() {
                    window.with_element_offset(self.child_offset, |window| {
                        child.prepaint(window, cx);
                    });
                }
            });
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
                window.with_content_mask(
                    style.overflow_mask(bounds, window.rem_size()),
                    |window| {
                        if let Some(child) = self.child.as_mut() {
                            child.paint(window, cx);
                        }
                    },
                );
            });
        });
    }
}

impl IntoElement for Frame {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
