use crate::{
    AbsoluteLength, Corners, Edges, Length, Overflow, Pixels, Point, Position, Size, Style, point,
    relative,
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FrameStyle {
    pub position: Position,
    pub inset: Edges<Length>,
    pub size: Size<Length>,
    pub min_size: Size<Length>,
    pub max_size: Size<Length>,
    pub opacity: Option<f32>,
    pub rotate: Option<f32>,
    pub scale: Option<Point<f32>>,
    pub translate: Option<Point<Pixels>>,
    pub skew: Option<Point<f32>>,
    pub transform_origin: Option<Point<f32>>,
    pub corner_radii: Corners<AbsoluteLength>,
    pub clip: bool,
}

impl Default for FrameStyle {
    fn default() -> Self {
        Self {
            position: Position::Relative,
            inset: Edges::auto(),
            size: Size::auto(),
            min_size: Size::auto(),
            max_size: Size::auto(),
            opacity: None,
            rotate: None,
            scale: None,
            translate: None,
            skew: None,
            transform_origin: None,
            corner_radii: Corners::default(),
            clip: false,
        }
    }
}

impl FrameStyle {
    pub fn size(mut self, size: impl Into<Size<Length>>) -> Self {
        self.size = size.into();
        self
    }

    pub fn w(mut self, width: impl Into<Length>) -> Self {
        self.size.width = width.into();
        self
    }

    pub fn h(mut self, height: impl Into<Length>) -> Self {
        self.size.height = height.into();
        self
    }

    pub fn w_full(self) -> Self {
        self.w(relative(1.0))
    }

    pub fn h_full(self) -> Self {
        self.h(relative(1.0))
    }

    pub fn size_full(self) -> Self {
        self.w_full().h_full()
    }

    pub fn min_w(mut self, width: impl Into<Length>) -> Self {
        self.min_size.width = width.into();
        self
    }

    pub fn min_h(mut self, height: impl Into<Length>) -> Self {
        self.min_size.height = height.into();
        self
    }

    pub fn max_w(mut self, width: impl Into<Length>) -> Self {
        self.max_size.width = width.into();
        self
    }

    pub fn max_h(mut self, height: impl Into<Length>) -> Self {
        self.max_size.height = height.into();
        self
    }

    pub fn absolute(mut self) -> Self {
        self.position = Position::Absolute;
        self
    }

    pub fn relative(mut self) -> Self {
        self.position = Position::Relative;
        self
    }

    pub fn inset(mut self, inset: impl Into<Length>) -> Self {
        self.inset = Edges::all(inset.into());
        self
    }

    pub fn inset_0(self) -> Self {
        self.inset(Length::default())
    }

    pub fn left(mut self, left: impl Into<Length>) -> Self {
        self.inset.left = left.into();
        self
    }

    pub fn right(mut self, right: impl Into<Length>) -> Self {
        self.inset.right = right.into();
        self
    }

    pub fn top(mut self, top: impl Into<Length>) -> Self {
        self.inset.top = top.into();
        self
    }

    pub fn bottom(mut self, bottom: impl Into<Length>) -> Self {
        self.inset.bottom = bottom.into();
        self
    }

    pub fn rotate(mut self, radians: f32) -> Self {
        assert!(radians.is_finite(), "rotation must be finite");
        self.rotate = Some(radians);
        self
    }

    pub fn scale(mut self, scale: f32) -> Self {
        assert!(
            scale.is_finite() && scale > 0.0,
            "scale must be finite and positive"
        );
        self.scale = Some(point(scale, scale));
        self
    }

    pub fn scale_xy(mut self, x: f32, y: f32) -> Self {
        assert!(
            x.is_finite() && x > 0.0,
            "x scale must be finite and positive"
        );
        assert!(
            y.is_finite() && y > 0.0,
            "y scale must be finite and positive"
        );
        self.scale = Some(point(x, y));
        self
    }

    pub fn translate(mut self, translation: Point<Pixels>) -> Self {
        self.translate = Some(translation);
        self
    }

    pub fn transform_origin(mut self, origin: Point<f32>) -> Self {
        assert!(
            origin.x.is_finite() && origin.y.is_finite(),
            "transform origin must be finite"
        );
        self.transform_origin = Some(origin);
        self
    }

    pub fn skew(mut self, x_radians: f32, y_radians: f32) -> Self {
        assert!(
            x_radians.is_finite() && y_radians.is_finite(),
            "skew must be finite"
        );
        self.skew = Some(point(x_radians, y_radians));
        self
    }

    pub fn skew_x(mut self, radians: f32) -> Self {
        assert!(radians.is_finite(), "skew must be finite");
        let mut skew = self.skew.unwrap_or_default();
        skew.x = radians;
        self.skew = Some(skew);
        self
    }

    pub fn skew_y(mut self, radians: f32) -> Self {
        assert!(radians.is_finite(), "skew must be finite");
        let mut skew = self.skew.unwrap_or_default();
        skew.y = radians;
        self.skew = Some(skew);
        self
    }

    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = Some(opacity);
        self
    }

    pub fn rounded(mut self, radius: impl Into<AbsoluteLength>) -> Self {
        let radius = radius.into();
        self.corner_radii = Corners {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        };
        self
    }

    pub fn corner_radii(mut self, corner_radii: impl Into<Corners<AbsoluteLength>>) -> Self {
        self.corner_radii = corner_radii.into();
        self
    }

    pub fn clip(mut self) -> Self {
        self.clip = true;
        self
    }

    pub fn unclip(mut self) -> Self {
        self.clip = false;
        self
    }

    pub fn layout_style(&self) -> Style {
        let mut style = Style::default();
        style.position = self.position;
        style.inset = self.inset;
        style.size = self.size;
        style.min_size = self.min_size;
        style.max_size = self.max_size;
        style.opacity = self.opacity;
        style.rotate = self.rotate;
        style.scale = self.scale;
        style.translate = self.translate;
        style.skew = self.skew;
        style.transform_origin = self.transform_origin;
        style.corner_radii = self.corner_radii;
        if self.clip {
            style.overflow = point(Overflow::Hidden, Overflow::Hidden);
        }
        style
    }
}
