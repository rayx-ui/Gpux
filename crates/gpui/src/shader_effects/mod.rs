use crate::{Bounds, Hsla, Pixels, Point, SharedString, point, px};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A visual effect emitted by a styled element and consumed by renderer backends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum ElementEffect {
    /// CSS-like backdrop blur sampling content already painted behind the element.
    BackdropBlur(BackdropBlurEffect),
    /// Liquid glass composed from backdrop blur, tint, highlight, and edge refraction.
    Glass(GlassEffect),
    /// A halo around the element bounds.
    Glow(GlowEffect),
}

/// Renderer quality preference for shader-backed effects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum EffectQuality {
    /// Cost-bounded default for mobile and WebGPU.
    #[default]
    Fast,
    /// Higher-radius sampling for backends that can afford it.
    High,
}

/// CSS-like backdrop blur parameters for a styled element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackdropBlurEffect {
    radius: Pixels,
    saturation: f32,
    brightness: f32,
    tint: Option<Hsla>,
    quality: EffectQuality,
}

impl BackdropBlurEffect {
    /// Creates a backdrop blur effect. WGPU is the first backend with shader support;
    /// unsupported renderers report capabilities rather than drawing fake blur.
    pub fn new(radius: Pixels) -> Self {
        Self {
            radius: clamp_pixels(radius, px(0.0), px(256.0)),
            saturation: 1.0,
            brightness: 1.0,
            tint: None,
            quality: EffectQuality::Fast,
        }
    }

    /// Sets the blur radius.
    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = clamp_pixels(radius, px(0.0), px(256.0));
        self
    }

    /// Sets the saturation multiplier.
    pub fn saturation(mut self, saturation: f32) -> Self {
        self.saturation = clamp_f32(saturation, 0.0, 4.0);
        self
    }

    /// Sets the brightness multiplier.
    pub fn brightness(mut self, brightness: f32) -> Self {
        self.brightness = clamp_f32(brightness, 0.0, 4.0);
        self
    }

    /// Sets an optional tint composited over the sampled backdrop.
    pub fn tint(mut self, tint: impl Into<Hsla>) -> Self {
        self.tint = Some(tint.into());
        self
    }

    /// Sets the renderer quality preference.
    pub fn quality(mut self, quality: EffectQuality) -> Self {
        self.quality = quality;
        self
    }

    /// Selects the higher-radius quality tier.
    pub fn high_quality(mut self) -> Self {
        self.quality = EffectQuality::High;
        self
    }

    /// Returns the blur radius.
    pub fn radius_value(&self) -> Pixels {
        self.radius
    }

    /// Returns the saturation multiplier.
    pub fn saturation_value(&self) -> f32 {
        self.saturation
    }

    /// Returns the brightness multiplier.
    pub fn brightness_value(&self) -> f32 {
        self.brightness
    }

    /// Returns the optional tint.
    pub fn tint_color(&self) -> Option<Hsla> {
        self.tint
    }

    /// Returns the renderer quality preference.
    pub fn quality_value(&self) -> EffectQuality {
        self.quality
    }
}

impl From<BackdropBlurEffect> for ElementEffect {
    fn from(effect: BackdropBlurEffect) -> Self {
        Self::BackdropBlur(effect)
    }
}

/// Liquid glass parameters for a styled element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GlassEffect {
    blur_radius: Pixels,
    saturation: f32,
    brightness: f32,
    tint: Hsla,
    highlight: Hsla,
    refraction: f32,
    distortion: f32,
    quality: EffectQuality,
}

impl Default for GlassEffect {
    fn default() -> Self {
        Self {
            blur_radius: px(18.0),
            saturation: 1.18,
            brightness: 1.08,
            tint: Hsla {
                h: 0.58,
                s: 0.18,
                l: 0.88,
                a: 0.22,
            },
            highlight: Hsla {
                h: 0.58,
                s: 0.1,
                l: 1.0,
                a: 0.32,
            },
            refraction: 0.035,
            distortion: 0.02,
            quality: EffectQuality::Fast,
        }
    }
}

impl GlassEffect {
    /// Creates a glass effect. WGPU is the first backend with shader support;
    /// unsupported renderers report capabilities rather than drawing fake glass.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the backdrop blur radius used by the glass.
    pub fn blur(mut self, radius: Pixels) -> Self {
        self.blur_radius = clamp_pixels(radius, px(0.0), px(256.0));
        self
    }

    /// Sets the saturation multiplier.
    pub fn saturation(mut self, saturation: f32) -> Self {
        self.saturation = clamp_f32(saturation, 0.0, 4.0);
        self
    }

    /// Sets the brightness multiplier.
    pub fn brightness(mut self, brightness: f32) -> Self {
        self.brightness = clamp_f32(brightness, 0.0, 4.0);
        self
    }

    /// Sets the glass tint.
    pub fn tint(mut self, tint: impl Into<Hsla>) -> Self {
        self.tint = tint.into();
        self
    }

    /// Sets the edge highlight.
    pub fn highlight(mut self, highlight: impl Into<Hsla>) -> Self {
        self.highlight = highlight.into();
        self
    }

    /// Sets the refraction strength in normalized screen-space units.
    pub fn refraction(mut self, refraction: f32) -> Self {
        self.refraction = clamp_f32(refraction, 0.0, 1.0);
        self
    }

    /// Sets the distortion/noise strength in normalized screen-space units.
    pub fn distortion(mut self, distortion: f32) -> Self {
        self.distortion = clamp_f32(distortion, 0.0, 1.0);
        self
    }

    /// Sets the renderer quality preference.
    pub fn quality(mut self, quality: EffectQuality) -> Self {
        self.quality = quality;
        self
    }

    /// Selects the higher-radius quality tier.
    pub fn high_quality(mut self) -> Self {
        self.quality = EffectQuality::High;
        self
    }

    /// Returns the blur radius.
    pub fn blur_radius(&self) -> Pixels {
        self.blur_radius
    }

    /// Returns the saturation multiplier.
    pub fn saturation_value(&self) -> f32 {
        self.saturation
    }

    /// Returns the brightness multiplier.
    pub fn brightness_value(&self) -> f32 {
        self.brightness
    }

    /// Returns the glass tint.
    pub fn tint_color(&self) -> Hsla {
        self.tint
    }

    /// Returns the edge highlight color.
    pub fn highlight_color(&self) -> Hsla {
        self.highlight
    }

    /// Returns the refraction strength.
    pub fn refraction_value(&self) -> f32 {
        self.refraction
    }

    /// Returns the distortion strength.
    pub fn distortion_value(&self) -> f32 {
        self.distortion
    }

    /// Returns the renderer quality preference.
    pub fn quality_value(&self) -> EffectQuality {
        self.quality
    }
}

impl From<GlassEffect> for ElementEffect {
    fn from(effect: GlassEffect) -> Self {
        Self::Glass(effect)
    }
}

/// Glow parameters for a styled element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GlowEffect {
    color: Hsla,
    radius: Pixels,
    spread: Pixels,
    intensity: f32,
}

impl GlowEffect {
    /// Creates a glow effect. WGPU is the first backend with shader support;
    /// unsupported renderers report capabilities rather than drawing fake glow.
    pub fn new(color: Hsla) -> Self {
        Self {
            color,
            radius: px(24.0),
            spread: px(0.0),
            intensity: 1.0,
        }
    }

    /// Sets the glow blur radius.
    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = clamp_pixels(radius, px(0.0), px(256.0));
        self
    }

    /// Sets the glow spread before blur.
    pub fn spread(mut self, spread: Pixels) -> Self {
        self.spread = clamp_pixels(spread, px(0.0), px(256.0));
        self
    }

    /// Sets the intensity multiplier.
    pub fn intensity(mut self, intensity: f32) -> Self {
        self.intensity = clamp_f32(intensity, 0.0, 2.0);
        self
    }

    /// Returns the glow color.
    pub fn color(&self) -> Hsla {
        self.color
    }

    /// Returns the glow blur radius.
    pub fn radius_value(&self) -> Pixels {
        self.radius
    }

    /// Returns the glow spread.
    pub fn spread_value(&self) -> Pixels {
        self.spread
    }

    /// Returns the intensity multiplier.
    pub fn intensity_value(&self) -> f32 {
        self.intensity
    }
}

impl From<GlowEffect> for ElementEffect {
    fn from(effect: GlowEffect) -> Self {
        Self::Glow(effect)
    }
}

/// Converts one or more element-effect builders into ordered element effects.
pub trait IntoElementEffects {
    /// Converts this input into ordered element effects.
    fn into_element_effects(self) -> Vec<ElementEffect>;
}

impl<T> IntoElementEffects for T
where
    T: Into<ElementEffect>,
{
    fn into_element_effects(self) -> Vec<ElementEffect> {
        vec![self.into()]
    }
}

impl<T, const N: usize> IntoElementEffects for [T; N]
where
    T: Into<ElementEffect>,
{
    fn into_element_effects(self) -> Vec<ElementEffect> {
        self.into_iter().map(Into::into).collect()
    }
}

impl IntoElementEffects for Vec<ElementEffect> {
    fn into_element_effects(self) -> Vec<ElementEffect> {
        self
    }
}

macro_rules! impl_into_element_effects_tuple {
    ($($name:ident),+ $(,)?) => {
        impl<$($name),+> IntoElementEffects for ($($name,)+)
        where
            $($name: Into<ElementEffect>),+
        {
            #[allow(non_snake_case)]
            fn into_element_effects(self) -> Vec<ElementEffect> {
                let ($($name,)+) = self;
                vec![$($name.into()),+]
            }
        }
    };
}

impl_into_element_effects_tuple!(A, B);
impl_into_element_effects_tuple!(A, B, C);
impl_into_element_effects_tuple!(A, B, C, D);
impl_into_element_effects_tuple!(A, B, C, D, E);
impl_into_element_effects_tuple!(A, B, C, D, E, F);

/// A window or region-level post-process effect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ScreenEffect {
    /// Animated ripple distortion.
    Ripple(RippleEffect),
    /// Transition between a retained snapshot and the newly rendered scene.
    SnapshotTransition(SnapshotTransitionEffect),
}

impl ScreenEffect {
    /// Returns the effect target.
    pub fn target(&self) -> &ScreenEffectTarget {
        match self {
            Self::Ripple(effect) => effect.target_ref(),
            Self::SnapshotTransition(effect) => effect.target_ref(),
        }
    }

    /// Returns the configured duration in milliseconds.
    pub fn duration_ms_value(&self) -> u64 {
        match self {
            Self::Ripple(effect) => effect.duration_ms_value(),
            Self::SnapshotTransition(effect) => effect.duration_ms_value(),
        }
    }

    /// Returns the retained snapshot id referenced by this effect, when this is
    /// a snapshot transition.
    pub fn snapshot_id(&self) -> Option<ScreenEffectSnapshotId> {
        match self {
            Self::Ripple(_) => None,
            Self::SnapshotTransition(effect) => Some(effect.snapshot_id()),
        }
    }
}

impl From<RippleEffect> for ScreenEffect {
    fn from(effect: RippleEffect) -> Self {
        Self::Ripple(effect)
    }
}

impl From<SnapshotTransitionEffect> for ScreenEffect {
    fn from(effect: SnapshotTransitionEffect) -> Self {
        Self::SnapshotTransition(effect)
    }
}

/// A named visual region that can receive screen effects.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ScreenEffectRegionId(SharedString);

impl ScreenEffectRegionId {
    /// Creates a region id.
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self(id.into())
    }
}

impl AsRef<str> for ScreenEffectRegionId {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl From<&str> for ScreenEffectRegionId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ScreenEffectRegionId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<SharedString> for ScreenEffectRegionId {
    fn from(value: SharedString) -> Self {
        Self::new(value)
    }
}

/// The visual target for a screen effect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ScreenEffectTarget {
    /// Target the whole window.
    Window,
    /// Target explicit window-relative bounds.
    Bounds(Bounds<Pixels>),
    /// Target a named region registered by a painted element.
    Region(ScreenEffectRegionId),
}

impl ScreenEffectTarget {
    /// Targets the whole window.
    pub fn window() -> Self {
        Self::Window
    }

    /// Targets explicit window-relative bounds.
    pub fn bounds(bounds: Bounds<Pixels>) -> Self {
        Self::Bounds(bounds)
    }

    /// Targets a named screen-effect region.
    pub fn region(id: impl Into<ScreenEffectRegionId>) -> Self {
        Self::Region(id.into())
    }
}

/// Animated ripple distortion parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RippleEffect {
    origin: Point<Pixels>,
    target: ScreenEffectTarget,
    duration_ms: u64,
    radius: Pixels,
    amplitude: Pixels,
    wavelength: Pixels,
    damping: f32,
    chromatic_aberration: f32,
    tint: Option<Hsla>,
}

impl RippleEffect {
    /// Creates a ripple at a window-relative origin. WGPU is the first backend
    /// with shader support; unsupported renderers report capabilities rather
    /// than drawing a fake transition.
    pub fn new(origin: Point<Pixels>) -> Self {
        Self {
            origin,
            target: ScreenEffectTarget::Window,
            duration_ms: 650,
            radius: px(420.0),
            amplitude: px(12.0),
            wavelength: px(42.0),
            damping: 4.0,
            chromatic_aberration: 0.0,
            tint: None,
        }
    }

    /// Sets the target for this ripple.
    pub fn target(mut self, target: ScreenEffectTarget) -> Self {
        self.target = target;
        self
    }

    /// Sets the duration in milliseconds.
    pub fn duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = duration_ms.max(16);
        self
    }

    /// Sets the ripple radius.
    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = clamp_pixels(radius, px(0.0), px(4096.0));
        self
    }

    /// Sets the ripple amplitude.
    pub fn amplitude(mut self, amplitude: Pixels) -> Self {
        self.amplitude = clamp_pixels(amplitude, px(0.0), px(512.0));
        self
    }

    /// Sets the ripple wavelength.
    pub fn wavelength(mut self, wavelength: Pixels) -> Self {
        self.wavelength = clamp_pixels(wavelength, px(1.0), px(4096.0));
        self
    }

    /// Sets the exponential damping strength.
    pub fn damping(mut self, damping: f32) -> Self {
        self.damping = clamp_f32(damping, 0.0, 32.0);
        self
    }

    /// Sets optional chromatic aberration strength.
    pub fn chromatic_aberration(mut self, chromatic_aberration: f32) -> Self {
        self.chromatic_aberration = clamp_f32(chromatic_aberration, 0.0, 1.0);
        self
    }

    /// Sets an optional tint for the ripple.
    pub fn tint(mut self, tint: impl Into<Hsla>) -> Self {
        self.tint = Some(tint.into());
        self
    }

    /// Returns the origin.
    pub fn origin(&self) -> Point<Pixels> {
        self.origin
    }

    /// Returns the target.
    pub fn target_ref(&self) -> &ScreenEffectTarget {
        &self.target
    }

    /// Returns the duration in milliseconds.
    pub fn duration_ms_value(&self) -> u64 {
        self.duration_ms
    }

    /// Returns the ripple radius.
    pub fn radius_value(&self) -> Pixels {
        self.radius
    }

    /// Returns the ripple amplitude.
    pub fn amplitude_value(&self) -> Pixels {
        self.amplitude
    }

    /// Returns the ripple wavelength.
    pub fn wavelength_value(&self) -> Pixels {
        self.wavelength
    }

    /// Returns the damping strength.
    pub fn damping_value(&self) -> f32 {
        self.damping
    }

    /// Returns the chromatic aberration strength.
    pub fn chromatic_aberration_value(&self) -> f32 {
        self.chromatic_aberration
    }

    /// Returns the optional tint.
    pub fn tint_color(&self) -> Option<Hsla> {
        self.tint
    }
}

/// Snapshot transition shader variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SnapshotTransitionKind {
    /// Crossfade old snapshot content into the newly rendered scene.
    Crossfade,
    /// Crossfade from a blurred snapshot into the newly rendered scene.
    BlurCrossfade,
    /// Reveal the new scene radially over the old snapshot.
    RadialReveal,
    /// Reveal the new scene with a ripple-shaped leading edge over the old
    /// snapshot.
    RippleReveal,
}

/// Old-to-new screen transition backed by a retained renderer snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotTransitionEffect {
    snapshot_id: ScreenEffectSnapshotId,
    kind: SnapshotTransitionKind,
    origin: Point<Pixels>,
    target: ScreenEffectTarget,
    duration_ms: u64,
    radius: Pixels,
    amplitude: Pixels,
    wavelength: Pixels,
    damping: f32,
    blur_radius: Pixels,
    tint: Option<Hsla>,
}

/// Snapshot transition variant used by [`crate::transition_frame`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum FrameTransitionKind {
    /// Crossfade old snapshot content into the newly rendered frame child.
    Crossfade,
    /// Crossfade from a blurred snapshot into the newly rendered frame child.
    BlurCrossfade,
    /// Reveal the new frame child radially over the old snapshot.
    RadialReveal,
    /// Reveal the new frame child with a ripple-shaped leading edge.
    RippleReveal,
}

/// Frame-scoped replacement transition configuration.
///
/// This is a GPUI element-level configuration that resolves to existing
/// retained-snapshot screen effects. It is intentionally independent of RayX
/// navigation transition types.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameTransition {
    kind: FrameTransitionKind,
    duration_ms: u64,
    origin: Option<Point<Pixels>>,
    radius: Pixels,
    amplitude: Pixels,
    wavelength: Pixels,
    damping: f32,
    blur_radius: Pixels,
    tint: Option<Hsla>,
}

impl Default for FrameTransition {
    fn default() -> Self {
        Self {
            kind: FrameTransitionKind::Crossfade,
            duration_ms: 520,
            origin: None,
            radius: px(520.0),
            amplitude: px(18.0),
            wavelength: px(48.0),
            damping: 4.0,
            blur_radius: px(10.0),
            tint: None,
        }
    }
}

impl FrameTransition {
    /// Creates a crossfade replacement transition.
    pub fn crossfade() -> Self {
        Self {
            kind: FrameTransitionKind::Crossfade,
            ..Self::default()
        }
    }

    /// Creates a blurred crossfade replacement transition.
    pub fn blur_crossfade() -> Self {
        Self {
            kind: FrameTransitionKind::BlurCrossfade,
            ..Self::default()
        }
    }

    /// Creates a radial reveal replacement transition.
    pub fn radial_reveal() -> Self {
        Self {
            kind: FrameTransitionKind::RadialReveal,
            ..Self::default()
        }
    }

    /// Creates a ripple reveal replacement transition.
    pub fn ripple_reveal() -> Self {
        Self {
            kind: FrameTransitionKind::RippleReveal,
            ..Self::default()
        }
    }

    /// Sets the transition kind.
    pub fn kind(mut self, kind: FrameTransitionKind) -> Self {
        self.kind = kind;
        self
    }

    /// Sets the transition duration in milliseconds.
    pub fn duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = duration_ms.max(16);
        self
    }

    /// Sets the explicit window-relative reveal origin.
    pub fn origin(mut self, origin: Point<Pixels>) -> Self {
        self.origin = Some(origin);
        self
    }

    /// Clears the explicit reveal origin so the frame center is used.
    pub fn clear_origin(mut self) -> Self {
        self.origin = None;
        self
    }

    /// Sets the reveal radius.
    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = clamp_pixels(radius, px(0.0), px(4096.0));
        self
    }

    /// Sets the ripple edge amplitude.
    pub fn amplitude(mut self, amplitude: Pixels) -> Self {
        self.amplitude = clamp_pixels(amplitude, px(0.0), px(512.0));
        self
    }

    /// Sets the ripple edge wavelength.
    pub fn wavelength(mut self, wavelength: Pixels) -> Self {
        self.wavelength = clamp_pixels(wavelength, px(1.0), px(4096.0));
        self
    }

    /// Sets the ripple edge damping strength.
    pub fn damping(mut self, damping: f32) -> Self {
        self.damping = clamp_f32(damping, 0.0, 32.0);
        self
    }

    /// Sets the blur radius used by blur crossfade.
    pub fn blur_radius(mut self, blur_radius: Pixels) -> Self {
        self.blur_radius = clamp_pixels(blur_radius, px(0.0), px(128.0));
        self
    }

    /// Sets an optional tint over the retained snapshot.
    pub fn tint(mut self, tint: impl Into<Hsla>) -> Self {
        self.tint = Some(tint.into());
        self
    }

    /// Clears the optional retained-snapshot tint.
    pub fn clear_tint(mut self) -> Self {
        self.tint = None;
        self
    }

    /// Returns the configured transition kind.
    pub fn kind_value(&self) -> FrameTransitionKind {
        self.kind
    }

    /// Returns the configured transition duration in milliseconds.
    pub fn duration_ms_value(&self) -> u64 {
        self.duration_ms
    }

    /// Returns the optional explicit reveal origin.
    pub fn origin_value(&self) -> Option<Point<Pixels>> {
        self.origin
    }

    /// Resolves this frame transition to a retained-snapshot screen effect.
    pub fn resolve_screen_effect(
        &self,
        snapshot_id: ScreenEffectSnapshotId,
        target: ScreenEffectTarget,
        default_origin: Point<Pixels>,
    ) -> ScreenEffect {
        let origin = self.origin.unwrap_or(default_origin);
        let mut transition = match self.kind {
            FrameTransitionKind::Crossfade => screen::crossfade(snapshot_id),
            FrameTransitionKind::BlurCrossfade => screen::blur_crossfade(snapshot_id),
            FrameTransitionKind::RadialReveal => screen::radial_reveal(snapshot_id, origin),
            FrameTransitionKind::RippleReveal => screen::ripple_reveal(snapshot_id, origin),
        }
        .target(target)
        .duration_ms(self.duration_ms)
        .radius(self.radius)
        .amplitude(self.amplitude)
        .wavelength(self.wavelength)
        .damping(self.damping)
        .blur_radius(self.blur_radius);

        if let Some(tint) = self.tint {
            transition = transition.tint(tint);
        }

        ScreenEffect::from(transition)
    }
}

impl SnapshotTransitionEffect {
    /// Creates a transition from a previously captured snapshot. WGPU is the
    /// first backend with shader support; unsupported renderers report
    /// capabilities rather than drawing a fake transition.
    pub fn new(snapshot_id: ScreenEffectSnapshotId, kind: SnapshotTransitionKind) -> Self {
        Self {
            snapshot_id,
            kind,
            origin: point(px(0.0), px(0.0)),
            target: ScreenEffectTarget::Window,
            duration_ms: 520,
            radius: px(520.0),
            amplitude: px(18.0),
            wavelength: px(48.0),
            damping: 4.0,
            blur_radius: px(10.0),
            tint: None,
        }
    }

    /// Sets the target for this transition.
    pub fn target(mut self, target: ScreenEffectTarget) -> Self {
        self.target = target;
        self
    }

    /// Sets the origin for radial and ripple reveal variants.
    pub fn origin(mut self, origin: Point<Pixels>) -> Self {
        self.origin = origin;
        self
    }

    /// Sets the duration in milliseconds.
    pub fn duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = duration_ms.max(16);
        self
    }

    /// Sets the reveal radius.
    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = clamp_pixels(radius, px(0.0), px(4096.0));
        self
    }

    /// Sets the ripple edge amplitude for ripple reveal.
    pub fn amplitude(mut self, amplitude: Pixels) -> Self {
        self.amplitude = clamp_pixels(amplitude, px(0.0), px(512.0));
        self
    }

    /// Sets the ripple edge wavelength for ripple reveal.
    pub fn wavelength(mut self, wavelength: Pixels) -> Self {
        self.wavelength = clamp_pixels(wavelength, px(1.0), px(4096.0));
        self
    }

    /// Sets the ripple edge damping strength.
    pub fn damping(mut self, damping: f32) -> Self {
        self.damping = clamp_f32(damping, 0.0, 32.0);
        self
    }

    /// Sets the blur radius for blur crossfade.
    pub fn blur_radius(mut self, blur_radius: Pixels) -> Self {
        self.blur_radius = clamp_pixels(blur_radius, px(0.0), px(128.0));
        self
    }

    /// Sets an optional tint over the retained snapshot.
    pub fn tint(mut self, tint: impl Into<Hsla>) -> Self {
        self.tint = Some(tint.into());
        self
    }

    /// Returns the snapshot id.
    pub fn snapshot_id(&self) -> ScreenEffectSnapshotId {
        self.snapshot_id
    }

    /// Returns the transition variant.
    pub fn kind(&self) -> SnapshotTransitionKind {
        self.kind
    }

    /// Returns the origin.
    pub fn origin_value(&self) -> Point<Pixels> {
        self.origin
    }

    /// Returns the target.
    pub fn target_ref(&self) -> &ScreenEffectTarget {
        &self.target
    }

    /// Returns the duration in milliseconds.
    pub fn duration_ms_value(&self) -> u64 {
        self.duration_ms
    }

    /// Returns the reveal radius.
    pub fn radius_value(&self) -> Pixels {
        self.radius
    }

    /// Returns the ripple edge amplitude.
    pub fn amplitude_value(&self) -> Pixels {
        self.amplitude
    }

    /// Returns the ripple edge wavelength.
    pub fn wavelength_value(&self) -> Pixels {
        self.wavelength
    }

    /// Returns the ripple edge damping strength.
    pub fn damping_value(&self) -> f32 {
        self.damping
    }

    /// Returns the blur radius.
    pub fn blur_radius_value(&self) -> Pixels {
        self.blur_radius
    }

    /// Returns the optional tint.
    pub fn tint_color(&self) -> Option<Hsla> {
        self.tint
    }
}

/// Creates a backdrop blur element effect.
///
/// WGPU is the first backend with shader support; unsupported renderers report
/// capabilities rather than drawing a fake blur.
pub fn backdrop_blur(radius: Pixels) -> BackdropBlurEffect {
    BackdropBlurEffect::new(radius)
}

/// Creates a liquid glass element effect.
///
/// WGPU is the first backend with shader support; unsupported renderers report
/// capabilities rather than drawing fake glass.
pub fn glass() -> GlassEffect {
    GlassEffect::new()
}

/// Creates a glow element effect.
///
/// WGPU is the first backend with shader support; unsupported renderers report
/// capabilities rather than drawing fake glow.
pub fn glow(color: impl Into<Hsla>) -> GlowEffect {
    GlowEffect::new(color.into())
}

/// Screen effect constructors.
pub mod screen {
    use super::{RippleEffect, SnapshotTransitionEffect, SnapshotTransitionKind};
    use crate::{Pixels, Point};

    /// Creates an animated ripple distortion at a window-relative origin.
    ///
    /// WGPU is the first backend with shader support; unsupported renderers
    /// report capabilities rather than drawing a fake transition.
    pub fn ripple(origin: Point<Pixels>) -> RippleEffect {
        RippleEffect::new(origin)
    }

    /// Creates a transition that crossfades from a retained snapshot to the
    /// newly rendered scene.
    pub fn crossfade(snapshot_id: super::ScreenEffectSnapshotId) -> SnapshotTransitionEffect {
        SnapshotTransitionEffect::new(snapshot_id, SnapshotTransitionKind::Crossfade)
    }

    /// Creates a transition that crossfades from a softly blurred retained
    /// snapshot to the newly rendered scene.
    pub fn blur_crossfade(snapshot_id: super::ScreenEffectSnapshotId) -> SnapshotTransitionEffect {
        SnapshotTransitionEffect::new(snapshot_id, SnapshotTransitionKind::BlurCrossfade)
    }

    /// Creates a transition that reveals the new scene radially from an origin.
    pub fn radial_reveal(
        snapshot_id: super::ScreenEffectSnapshotId,
        origin: Point<Pixels>,
    ) -> SnapshotTransitionEffect {
        SnapshotTransitionEffect::new(snapshot_id, SnapshotTransitionKind::RadialReveal)
            .origin(origin)
    }

    /// Creates a transition that reveals the new scene with a ripple-shaped
    /// leading edge from an origin.
    pub fn ripple_reveal(
        snapshot_id: super::ScreenEffectSnapshotId,
        origin: Point<Pixels>,
    ) -> SnapshotTransitionEffect {
        SnapshotTransitionEffect::new(snapshot_id, SnapshotTransitionKind::RippleReveal)
            .origin(origin)
    }
}

/// Maximum number of active screen effects retained per window.
pub const MAX_ACTIVE_SCREEN_EFFECTS: usize = 16;

/// Maximum number of retained screen-effect snapshots per window.
pub const MAX_SCREEN_EFFECT_SNAPSHOTS: usize = 8;

/// Stable id assigned to a playing screen effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ScreenEffectId(u64);

impl ScreenEffectId {
    /// Creates an effect id from a monotonic raw value.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the raw monotonic id.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// Stable id assigned to a retained screen-effect snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ScreenEffectSnapshotId(u64);

impl ScreenEffectSnapshotId {
    /// Creates a snapshot id from a monotonic raw value.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the raw monotonic id.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// Renderer-facing retained screen-effect snapshot request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActiveScreenEffectSnapshot {
    id: ScreenEffectSnapshotId,
    target: ScreenEffectTarget,
}

impl ActiveScreenEffectSnapshot {
    /// Creates a retained snapshot descriptor.
    pub fn new(id: ScreenEffectSnapshotId, target: ScreenEffectTarget) -> Self {
        Self { id, target }
    }

    /// Returns the stable snapshot id.
    pub fn id(&self) -> ScreenEffectSnapshotId {
        self.id
    }

    /// Returns the snapshot target.
    pub fn target(&self) -> &ScreenEffectTarget {
        &self.target
    }
}

/// Per-frame snapshot of an active screen effect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActiveScreenEffect {
    id: ScreenEffectId,
    effect: ScreenEffect,
    elapsed_ms: u64,
}

impl ActiveScreenEffect {
    /// Creates an active-effect snapshot.
    pub fn new(id: ScreenEffectId, effect: ScreenEffect, elapsed_ms: u64) -> Self {
        Self {
            id,
            effect,
            elapsed_ms,
        }
    }

    /// Returns the stable active-effect id.
    pub fn id(&self) -> ScreenEffectId {
        self.id
    }

    /// Returns the effect payload.
    pub fn effect(&self) -> &ScreenEffect {
        &self.effect
    }

    /// Returns elapsed playback time in milliseconds.
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    /// Returns normalized progress in the inclusive range 0..=1.
    pub fn progress(&self) -> f32 {
        let duration_ms = self.effect.duration_ms_value().max(1);
        (self.elapsed_ms as f32 / duration_ms as f32).clamp(0.0, 1.0)
    }

    /// Returns whether this snapshot has reached the end of its duration.
    pub fn is_complete(&self) -> bool {
        self.elapsed_ms >= self.effect.duration_ms_value()
    }
}

/// Renderer support for shader-backed effects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EffectCapabilities {
    /// Element-level effect support.
    pub element: ElementEffectCapabilities,
    /// Screen-level effect support.
    pub screen: ScreenEffectCapabilities,
}

impl EffectCapabilities {
    /// Returns whether this renderer supports the supplied element effect.
    pub fn supports_element_effect(&self, effect: &ElementEffect) -> bool {
        match effect {
            ElementEffect::BackdropBlur(_) => self.element.backdrop_blur,
            ElementEffect::Glass(_) => self.element.glass,
            ElementEffect::Glow(_) => self.element.glow,
        }
    }

    /// Returns whether this renderer supports the supplied screen effect.
    pub fn supports_screen_effect(&self, effect: &ScreenEffect) -> bool {
        match effect {
            ScreenEffect::Ripple(_) => self.screen.ripple,
            ScreenEffect::SnapshotTransition(_) => self.screen.snapshot_transitions,
        }
    }
}

/// Renderer support for element-level effects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ElementEffectCapabilities {
    /// Backdrop blur support.
    pub backdrop_blur: bool,
    /// Glass effect support.
    pub glass: bool,
    /// Glow effect support.
    pub glow: bool,
}

/// Renderer support for screen-level effects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScreenEffectCapabilities {
    /// Ripple transition support.
    pub ripple: bool,
    /// Retained-snapshot transition support.
    pub snapshot_transitions: bool,
}

fn clamp_pixels(value: Pixels, min: Pixels, max: Pixels) -> Pixels {
    if value.0.is_nan() {
        return min;
    }

    px(value.0.clamp(min.0, max.0))
}

fn clamp_f32(value: f32, min: f32, max: f32) -> f32 {
    if value.is_nan() {
        return min;
    }

    value.clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bounds, hsla, point, px, size};

    #[test]
    fn element_effect_constructors_apply_safe_defaults_and_clamping() {
        let blur = backdrop_blur(px(-12.0)).saturation(-2.0).brightness(8.0);
        assert_eq!(blur.radius_value(), px(0.0));
        assert_eq!(blur.saturation_value(), 0.0);
        assert_eq!(blur.brightness_value(), 4.0);

        let glass = glass()
            .blur(px(22.0))
            .refraction(-0.5)
            .distortion(2.0)
            .tint(hsla(0.6, 0.3, 0.9, 0.25));
        assert_eq!(glass.blur_radius(), px(22.0));
        assert_eq!(glass.refraction_value(), 0.0);
        assert_eq!(glass.distortion_value(), 1.0);
        assert_eq!(glass.quality_value(), EffectQuality::Fast);

        let glow = glow(hsla(0.1, 0.9, 0.6, 0.5))
            .radius(px(-5.0))
            .spread(px(12.0))
            .intensity(3.0);
        assert_eq!(glow.radius_value(), px(0.0));
        assert_eq!(glow.spread_value(), px(12.0));
        assert_eq!(glow.intensity_value(), 2.0);
    }

    #[test]
    fn blur_and_glass_quality_defaults_to_fast_and_can_opt_into_high() {
        assert_eq!(backdrop_blur(px(12.0)).quality_value(), EffectQuality::Fast);
        assert_eq!(
            backdrop_blur(px(12.0)).high_quality().quality_value(),
            EffectQuality::High
        );
        assert_eq!(
            glass().quality(EffectQuality::High).quality_value(),
            EffectQuality::High
        );
    }

    #[test]
    fn screen_ripple_defaults_are_browser_safe_and_bounded() {
        let ripple = screen::ripple(point(px(10.0), px(12.0)))
            .duration_ms(0)
            .amplitude(px(-4.0))
            .radius(px(-1.0))
            .wavelength(px(0.0))
            .damping(-10.0);

        assert_eq!(ripple.origin(), point(px(10.0), px(12.0)));
        assert_eq!(ripple.duration_ms_value(), 16);
        assert_eq!(ripple.amplitude_value(), px(0.0));
        assert_eq!(ripple.radius_value(), px(0.0));
        assert_eq!(ripple.wavelength_value(), px(1.0));
        assert_eq!(ripple.damping_value(), 0.0);
    }

    #[test]
    fn snapshot_transition_defaults_are_browser_safe_and_bounded() {
        let snapshot_id = ScreenEffectSnapshotId::new(42);
        let transition = screen::ripple_reveal(snapshot_id, point(px(10.0), px(12.0)))
            .duration_ms(0)
            .amplitude(px(-4.0))
            .radius(px(-1.0))
            .wavelength(px(0.0))
            .damping(-10.0)
            .blur_radius(px(900.0));

        assert_eq!(transition.snapshot_id(), snapshot_id);
        assert_eq!(transition.kind(), SnapshotTransitionKind::RippleReveal);
        assert_eq!(transition.origin_value(), point(px(10.0), px(12.0)));
        assert_eq!(transition.duration_ms_value(), 16);
        assert_eq!(transition.amplitude_value(), px(0.0));
        assert_eq!(transition.radius_value(), px(0.0));
        assert_eq!(transition.wavelength_value(), px(1.0));
        assert_eq!(transition.damping_value(), 0.0);
        assert_eq!(transition.blur_radius_value(), px(128.0));

        let effect = ScreenEffect::from(screen::crossfade(snapshot_id));
        assert_eq!(effect.snapshot_id(), Some(snapshot_id));
    }

    #[test]
    fn frame_transition_defaults_clamp_and_resolve_snapshot_variants() {
        let snapshot_id = ScreenEffectSnapshotId::new(7);
        let target = ScreenEffectTarget::region("panel");
        let default_origin = point(px(10.0), px(12.0));
        let explicit_origin = point(px(24.0), px(32.0));
        let transition = FrameTransition::ripple_reveal()
            .duration_ms(0)
            .origin(explicit_origin)
            .radius(px(-10.0))
            .amplitude(px(-1.0))
            .wavelength(px(0.0))
            .damping(-2.0)
            .blur_radius(px(900.0))
            .tint(hsla(0.2, 0.8, 0.6, 0.25));

        assert_eq!(transition.kind_value(), FrameTransitionKind::RippleReveal);
        assert_eq!(transition.duration_ms_value(), 16);
        assert_eq!(transition.origin_value(), Some(explicit_origin));

        let ScreenEffect::SnapshotTransition(effect) =
            transition.resolve_screen_effect(snapshot_id, target.clone(), default_origin)
        else {
            panic!("frame transitions should resolve to snapshot transitions");
        };

        assert_eq!(effect.snapshot_id(), snapshot_id);
        assert_eq!(effect.kind(), SnapshotTransitionKind::RippleReveal);
        assert_eq!(effect.target_ref(), &target);
        assert_eq!(effect.origin_value(), explicit_origin);
        assert_eq!(effect.radius_value(), px(0.0));
        assert_eq!(effect.amplitude_value(), px(0.0));
        assert_eq!(effect.wavelength_value(), px(1.0));
        assert_eq!(effect.damping_value(), 0.0);
        assert_eq!(effect.blur_radius_value(), px(128.0));
        assert_eq!(effect.tint_color(), Some(hsla(0.2, 0.8, 0.6, 0.25)));

        for (transition, expected) in [
            (
                FrameTransition::crossfade(),
                SnapshotTransitionKind::Crossfade,
            ),
            (
                FrameTransition::blur_crossfade(),
                SnapshotTransitionKind::BlurCrossfade,
            ),
            (
                FrameTransition::radial_reveal(),
                SnapshotTransitionKind::RadialReveal,
            ),
            (
                FrameTransition::ripple_reveal(),
                SnapshotTransitionKind::RippleReveal,
            ),
        ] {
            let ScreenEffect::SnapshotTransition(effect) =
                transition.resolve_screen_effect(snapshot_id, target.clone(), default_origin)
            else {
                panic!("frame transitions should resolve to snapshot transitions");
            };
            assert_eq!(effect.kind(), expected);
            if matches!(
                expected,
                SnapshotTransitionKind::RadialReveal | SnapshotTransitionKind::RippleReveal
            ) {
                assert_eq!(effect.origin_value(), default_origin);
            }
        }
    }

    #[test]
    fn screen_effect_targets_are_clean_public_values() {
        let bounds = Bounds::new(point(px(1.0), px(2.0)), size(px(3.0), px(4.0)));
        let region = ScreenEffectRegionId::new("story-content");

        assert_eq!(ScreenEffectTarget::Window, ScreenEffectTarget::window());
        assert_eq!(
            ScreenEffectTarget::Bounds(bounds),
            ScreenEffectTarget::bounds(bounds)
        );
        assert_eq!(
            ScreenEffectTarget::Region(region.clone()),
            ScreenEffectTarget::region(region.as_ref())
        );
        assert_eq!(region.as_ref(), "story-content");
    }

    #[test]
    fn into_element_effects_preserves_single_tuple_and_array_order() {
        let single = backdrop_blur(px(4.0)).into_element_effects();
        assert!(matches!(
            single.as_slice(),
            [ElementEffect::BackdropBlur(_)]
        ));

        let tuple = (
            backdrop_blur(px(8.0)),
            glass().refraction(0.25),
            glow(hsla(0.4, 0.8, 0.7, 0.6)),
        )
            .into_element_effects();
        assert!(matches!(
            tuple.as_slice(),
            [
                ElementEffect::BackdropBlur(_),
                ElementEffect::Glass(_),
                ElementEffect::Glow(_)
            ]
        ));

        let array = [
            ElementEffect::from(backdrop_blur(px(1.0))),
            ElementEffect::from(glow(hsla(0.1, 0.7, 0.5, 0.5))),
        ]
        .into_element_effects();
        assert!(matches!(
            array.as_slice(),
            [ElementEffect::BackdropBlur(_), ElementEffect::Glow(_)]
        ));
    }

    #[test]
    fn default_effect_capabilities_report_unsupported_effects() {
        let capabilities = EffectCapabilities::default();

        assert!(!capabilities.element.backdrop_blur);
        assert!(!capabilities.element.glass);
        assert!(!capabilities.element.glow);
        assert!(!capabilities.screen.ripple);
        assert!(!capabilities.screen.snapshot_transitions);
        assert!(!capabilities.supports_element_effect(&ElementEffect::from(backdrop_blur(px(8.)))));
        assert!(
            !capabilities
                .supports_element_effect(&ElementEffect::from(glow(hsla(0.55, 0.8, 0.7, 0.5))))
        );
        assert!(
            !capabilities.supports_screen_effect(&ScreenEffect::from(screen::ripple(point(
                px(1.0),
                px(2.0)
            ))))
        );
        assert!(
            !capabilities.supports_screen_effect(&ScreenEffect::from(screen::crossfade(
                ScreenEffectSnapshotId::new(1),
            )))
        );
    }
}

#[cfg(test)]
mod public_export_tests {
    use crate::{hsla, point, px};

    #[test]
    fn effect_constructors_are_available_from_crate_root_and_prelude() {
        let _: crate::shader_effects::ElementEffect = crate::backdrop_blur(px(12.0)).into();
        let _: crate::shader_effects::ElementEffect = crate::glass().into();
        let _: crate::shader_effects::ElementEffect = crate::glow(hsla(0.2, 0.7, 0.6, 0.4)).into();

        {
            use crate::prelude::*;

            let _: crate::shader_effects::ElementEffect = backdrop_blur(px(14.0)).into();
            let _: crate::shader_effects::ElementEffect = glass().into();
            let _: crate::shader_effects::ElementEffect = glow(hsla(0.5, 0.8, 0.7, 0.5)).into();
            let _ = screen::ripple(point(px(4.0), px(6.0)));
            let snapshot_id = crate::shader_effects::ScreenEffectSnapshotId::new(8);
            let _: crate::shader_effects::ScreenEffect = screen::crossfade(snapshot_id).into();
            let _: crate::shader_effects::ScreenEffect = screen::blur_crossfade(snapshot_id).into();
            let _: crate::shader_effects::ScreenEffect =
                screen::radial_reveal(snapshot_id, point(px(4.0), px(6.0))).into();
            let _: crate::shader_effects::ScreenEffect =
                screen::ripple_reveal(snapshot_id, point(px(4.0), px(6.0))).into();
        }
    }
}
