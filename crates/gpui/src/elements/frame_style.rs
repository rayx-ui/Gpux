use crate::{
    AbsoluteLength, Corners, DefiniteLength, Edges, Length, Overflow, Pixels, Point, Position,
    Rems, Size, Style, point, px, relative,
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

    pub(crate) fn interpolate(&self, target: &Self, progress: f32) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        let mut current = target.clone();

        current.inset = interpolate_length_edges(&self.inset, &target.inset, progress);
        current.size = interpolate_length_size(&self.size, &target.size, progress);
        current.min_size = interpolate_length_size(&self.min_size, &target.min_size, progress);
        current.max_size = interpolate_length_size(&self.max_size, &target.max_size, progress);
        current.opacity = interpolate_optional_f32(self.opacity, target.opacity, 1.0, progress);
        current.rotate = interpolate_optional_f32(self.rotate, target.rotate, 0.0, progress);
        current.scale =
            interpolate_optional_point_f32(self.scale, target.scale, point(1.0, 1.0), progress);
        current.translate = interpolate_optional_point_pixels(
            self.translate,
            target.translate,
            point(px(0.0), px(0.0)),
            progress,
        );
        current.skew =
            interpolate_optional_point_f32(self.skew, target.skew, point(0.0, 0.0), progress);
        current.transform_origin = interpolate_optional_point_f32(
            self.transform_origin,
            target.transform_origin,
            point(0.5, 0.5),
            progress,
        );
        current.corner_radii =
            interpolate_absolute_length_corners(&self.corner_radii, &target.corner_radii, progress);

        current
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

fn interpolate_length_size(
    from: &Size<Length>,
    target: &Size<Length>,
    progress: f32,
) -> Size<Length> {
    Size {
        width: interpolate_length(from.width, target.width, progress),
        height: interpolate_length(from.height, target.height, progress),
    }
}

fn interpolate_length_edges(
    from: &Edges<Length>,
    target: &Edges<Length>,
    progress: f32,
) -> Edges<Length> {
    Edges {
        top: interpolate_length(from.top, target.top, progress),
        right: interpolate_length(from.right, target.right, progress),
        bottom: interpolate_length(from.bottom, target.bottom, progress),
        left: interpolate_length(from.left, target.left, progress),
    }
}

fn interpolate_length(from: Length, target: Length, progress: f32) -> Length {
    match (from, target) {
        (Length::Definite(from), Length::Definite(target)) => {
            Length::Definite(interpolate_definite_length(from, target, progress))
        }
        _ => target,
    }
}

fn interpolate_definite_length(
    from: DefiniteLength,
    target: DefiniteLength,
    progress: f32,
) -> DefiniteLength {
    match (from, target) {
        (DefiniteLength::Absolute(from), DefiniteLength::Absolute(target)) => {
            DefiniteLength::Absolute(interpolate_absolute_length(from, target, progress))
        }
        _ => target,
    }
}

fn interpolate_absolute_length(
    from: AbsoluteLength,
    target: AbsoluteLength,
    progress: f32,
) -> AbsoluteLength {
    match (from, target) {
        (AbsoluteLength::Pixels(from), AbsoluteLength::Pixels(target)) => {
            AbsoluteLength::Pixels(px(lerp(f32::from(from), f32::from(target), progress)))
        }
        (AbsoluteLength::Rems(from), AbsoluteLength::Rems(target)) => {
            AbsoluteLength::Rems(Rems(lerp(from.0, target.0, progress)))
        }
        _ => target,
    }
}

fn interpolate_absolute_length_corners(
    from: &Corners<AbsoluteLength>,
    target: &Corners<AbsoluteLength>,
    progress: f32,
) -> Corners<AbsoluteLength> {
    Corners {
        top_left: interpolate_absolute_length(from.top_left, target.top_left, progress),
        top_right: interpolate_absolute_length(from.top_right, target.top_right, progress),
        bottom_right: interpolate_absolute_length(from.bottom_right, target.bottom_right, progress),
        bottom_left: interpolate_absolute_length(from.bottom_left, target.bottom_left, progress),
    }
}

fn interpolate_optional_f32(
    from: Option<f32>,
    target: Option<f32>,
    default: f32,
    progress: f32,
) -> Option<f32> {
    if from == target || progress >= 1.0 {
        return target;
    }

    Some(lerp(
        from.unwrap_or(default),
        target.unwrap_or(default),
        progress,
    ))
}

fn interpolate_optional_point_f32(
    from: Option<Point<f32>>,
    target: Option<Point<f32>>,
    default: Point<f32>,
    progress: f32,
) -> Option<Point<f32>> {
    if from == target || progress >= 1.0 {
        return target;
    }

    let from = from.unwrap_or(default);
    let target = target.unwrap_or(default);
    Some(point(
        lerp(from.x, target.x, progress),
        lerp(from.y, target.y, progress),
    ))
}

fn interpolate_optional_point_pixels(
    from: Option<Point<Pixels>>,
    target: Option<Point<Pixels>>,
    default: Point<Pixels>,
    progress: f32,
) -> Option<Point<Pixels>> {
    if from == target || progress >= 1.0 {
        return target;
    }

    let from = from.unwrap_or(default);
    let target = target.unwrap_or(default);
    Some(point(
        px(lerp(f32::from(from.x), f32::from(target.x), progress)),
        px(lerp(f32::from(from.y), f32::from(target.y), progress)),
    ))
}

fn lerp(from: f32, target: f32, progress: f32) -> f32 {
    from + (target - from) * progress
}

#[cfg(test)]
mod tests {
    use crate::{Length, auto, point, px, relative};

    use super::*;

    #[test]
    fn frame_style_interpolates_absolute_dimensions_insets_and_opacity() {
        let from = FrameStyle::default()
            .w(px(100.0))
            .h(px(60.0))
            .min_w(px(40.0))
            .max_h(px(160.0))
            .inset_0()
            .left(px(8.0))
            .top(px(12.0))
            .opacity(0.2);
        let target = FrameStyle::default()
            .w(px(220.0))
            .h(px(120.0))
            .min_w(px(80.0))
            .max_h(px(240.0))
            .inset_0()
            .left(px(20.0))
            .top(px(36.0))
            .opacity(0.8);

        let current = from.interpolate(&target, 0.5);

        assert_eq!(current.size.width, Length::from(px(160.0)));
        assert_eq!(current.size.height, Length::from(px(90.0)));
        assert_eq!(current.min_size.width, Length::from(px(60.0)));
        assert_eq!(current.max_size.height, Length::from(px(200.0)));
        assert_eq!(current.inset.left, Length::from(px(14.0)));
        assert_eq!(current.inset.top, Length::from(px(24.0)));
        assert_eq!(current.opacity, Some(0.5));
    }

    #[test]
    fn frame_style_interpolates_transforms_and_corner_radii() {
        let from = FrameStyle::default()
            .translate(point(px(10.0), px(20.0)))
            .scale_xy(1.0, 1.5)
            .rotate(0.0)
            .skew(0.0, 0.2)
            .transform_origin(point(0.0, 0.25))
            .rounded(px(4.0));
        let target = FrameStyle::default()
            .translate(point(px(30.0), px(60.0)))
            .scale_xy(2.0, 2.5)
            .rotate(1.0)
            .skew(0.4, 0.6)
            .transform_origin(point(1.0, 0.75))
            .rounded(px(20.0));

        let current = from.interpolate(&target, 0.25);

        assert_eq!(current.translate, Some(point(px(15.0), px(30.0))));
        assert_eq!(current.scale, Some(point(1.25, 1.75)));
        assert_eq!(current.rotate, Some(0.25));
        assert_eq!(current.skew, Some(point(0.1, 0.3)));
        assert_eq!(current.transform_origin, Some(point(0.25, 0.375)));
        assert_eq!(current.corner_radii.top_left, px(8.0).into());
        assert_eq!(current.corner_radii.bottom_right, px(8.0).into());
    }

    #[test]
    fn frame_style_snaps_incompatible_lengths_to_target() {
        let from = FrameStyle::default().w(auto()).h(px(40.0));
        let target = FrameStyle::default().w(relative(1.0)).h(relative(0.5));

        let current = from.interpolate(&target, 0.5);

        assert_eq!(current.size.width, target.size.width);
        assert_eq!(current.size.height, target.size.height);
    }
}
