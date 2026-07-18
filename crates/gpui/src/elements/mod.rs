mod anchored;
mod animated_frame;
mod animation;
mod canvas;
mod deferred;
mod div;
mod editable_text;
mod effect_frame;
mod frame;
mod frame_style;
mod image_cache;
mod img;
mod list;
mod surface;
mod svg;
mod text;
mod transition_frame;
mod uniform_list;

pub use anchored::*;
pub use animated_frame::*;
pub use animation::*;
pub use canvas::*;
pub use deferred::*;
pub use div::*;
pub use editable_text::*;
pub use effect_frame::*;
pub use frame::*;
pub use image_cache::*;
pub use img::*;
pub use list::*;
pub use surface::*;
pub use svg::*;
pub use text::*;
pub use transition_frame::*;
pub use uniform_list::*;

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use crate::{
        Context, Corners, Element, ElementEffect, ElementId, IntoElement, Overflow,
        ParentElement as _, Position, Render, Styled as _, TestAppContext, TransformationMatrix,
        Window, backdrop_blur, div, glow, hsla, point, px, red, relative, size,
    };

    use super::AnimatedFrameChild;
    use super::frame_style::FrameStyle;

    #[test]
    fn frame_style_defaults_to_lightweight_geometry_only() {
        let style = FrameStyle::default();

        assert!(!style.clip);
        assert!(style.opacity.is_none());
        assert!(style.rotate.is_none());
        assert!(style.scale.is_none());
        assert!(style.translate.is_none());
        assert!(style.skew.is_none());
    }

    #[test]
    fn frame_style_size_helpers_update_layout_style() {
        let style = FrameStyle::default()
            .w(px(24.0))
            .h(px(32.0))
            .min_w(px(12.0))
            .max_h(px(64.0));
        let layout = style.layout_style();

        assert_eq!(layout.size.width, px(24.0).into());
        assert_eq!(layout.size.height, px(32.0).into());
        assert_eq!(layout.min_size.width, px(12.0).into());
        assert_eq!(layout.max_size.height, px(64.0).into());

        let full = FrameStyle::default().size_full();
        let layout = full.layout_style();

        assert_eq!(layout.size.width, relative(1.0).into());
        assert_eq!(layout.size.height, relative(1.0).into());
    }

    #[test]
    fn frame_style_position_helpers_update_layout_style() {
        let absolute = FrameStyle::default().absolute().layout_style();
        assert_eq!(absolute.position, Position::Absolute);

        let relative = FrameStyle::default().absolute().relative().layout_style();
        assert_eq!(relative.position, Position::Relative);
    }

    #[test]
    fn frame_style_inset_helpers_update_layout_style() {
        let style = FrameStyle::default()
            .inset_0()
            .left(px(4.0))
            .top(px(8.0))
            .right(px(12.0))
            .bottom(px(16.0));
        let layout = style.layout_style();

        assert_eq!(layout.inset.left, px(4.0).into());
        assert_eq!(layout.inset.top, px(8.0).into());
        assert_eq!(layout.inset.right, px(12.0).into());
        assert_eq!(layout.inset.bottom, px(16.0).into());
    }

    #[test]
    fn frame_style_transform_helpers_update_paint_transform_fields() {
        let style = FrameStyle::default()
            .rotate(0.25)
            .scale_xy(1.25, 0.75)
            .translate(point(px(4.0), px(8.0)))
            .skew_x(0.1)
            .transform_origin(point(0.0, 1.0));

        let layout = style.layout_style();
        assert_eq!(layout.rotate, Some(0.25));
        assert_eq!(layout.scale, Some(point(1.25, 0.75)));
        assert_eq!(layout.translate, Some(point(px(4.0), px(8.0))));
        assert_eq!(layout.skew, Some(point(0.1, 0.0)));
        assert_eq!(layout.transform_origin, Some(point(0.0, 1.0)));
    }

    #[test]
    fn frame_style_opacity_helper_updates_layout_style() {
        let style = FrameStyle::default().opacity(0.42);

        assert_eq!(style.opacity, Some(0.42));
        assert_eq!(style.layout_style().opacity, Some(0.42));
    }

    #[test]
    fn frame_style_radius_helper_updates_layout_style() {
        let layout = FrameStyle::default().rounded(px(7.0)).layout_style();

        assert_eq!(layout.corner_radii.top_left, px(7.0).into());
        assert_eq!(layout.corner_radii.top_right, px(7.0).into());
        assert_eq!(layout.corner_radii.bottom_right, px(7.0).into());
        assert_eq!(layout.corner_radii.bottom_left, px(7.0).into());
    }

    #[test]
    fn frame_style_clip_helper_updates_layout_overflow() {
        let style = FrameStyle::default().clip();
        let layout = style.layout_style();

        assert!(style.clip);
        assert_eq!(layout.overflow.x, Overflow::Hidden);
        assert_eq!(layout.overflow.y, Overflow::Hidden);
    }

    #[test]
    fn frame_type_is_available_from_elements_module() {
        let _ = std::mem::size_of::<super::Frame>();
    }

    #[test]
    fn frame_constructor_returns_frame_element() {
        let element: super::Frame = crate::frame();

        element.into_element();
    }

    #[test]
    fn effect_frame_constructor_returns_effect_frame_element() {
        let element: super::EffectFrame = crate::effect_frame();

        element.into_element();
    }

    #[test]
    fn animated_frame_constructor_returns_animated_frame_element() {
        let element: super::AnimatedFrame = crate::animated_frame("profile-card");

        assert_eq!(element.id(), Some(ElementId::from("profile-card")));
        element.into_element();
    }

    #[test]
    fn transition_frame_constructor_returns_transition_frame_element() {
        let element: super::TransitionFrame = crate::transition_frame("profile-card");

        assert_eq!(element.id(), Some(ElementId::from("profile-card")));
        assert_eq!(
            element.screen_effect_region.as_ref(),
            "transition-frame-profile-card"
        );
        element.into_element();
    }

    #[test]
    fn animated_frame_child_sets_the_single_child_slot() {
        let frame = crate::animated_frame("profile-card").child("contents");

        assert!(matches!(frame.child, AnimatedFrameChild::Static(_)));
    }

    #[test]
    #[should_panic(expected = "AnimatedFrame accepts exactly one child")]
    fn animated_frame_child_rejects_repeated_child_calls_in_debug_builds() {
        let _ = crate::animated_frame("profile-card")
            .child("first")
            .child("second");
    }

    #[test]
    #[should_panic(expected = "AnimatedFrame accepts exactly one child")]
    fn animated_frame_progress_child_rejects_static_child_in_debug_builds() {
        let _ = crate::animated_frame("profile-card")
            .child("first")
            .child_with_progress(|_, _, _| "second");
    }

    #[test]
    #[should_panic(expected = "TransitionFrame accepts exactly one child")]
    fn transition_frame_child_rejects_repeated_child_calls_in_debug_builds() {
        let _ = crate::transition_frame("profile-card")
            .child("first")
            .child("second");
    }

    #[test]
    fn frame_child_sets_the_single_child_slot() {
        let frame = crate::frame().child("contents");

        assert!(frame.child.is_some());
    }

    #[test]
    #[should_panic(expected = "Frame accepts exactly one child")]
    fn frame_child_rejects_repeated_child_calls_in_debug_builds() {
        let _ = crate::frame().child("first").child("second");
    }

    #[test]
    fn effect_frame_helpers_update_effect_state() {
        let effects = crate::effect_frame()
            .effect(backdrop_blur(px(12.0)))
            .effects([glow(hsla(0.5, 0.8, 0.7, 0.4))])
            .screen_effect_region("spotlight");

        assert_eq!(effects.effects.len(), 2);
        assert!(matches!(effects.effects[0], ElementEffect::BackdropBlur(_)));
        assert!(matches!(effects.effects[1], ElementEffect::Glow(_)));
        assert_eq!(
            effects.screen_effect_region.as_ref().unwrap().as_ref(),
            "spotlight"
        );

        let cleared = effects.clear_effects();
        assert!(cleared.effects.is_empty());
    }

    #[test]
    #[should_panic(expected = "EffectFrame accepts exactly one child")]
    fn effect_frame_child_rejects_repeated_child_calls_in_debug_builds() {
        let _ = crate::effect_frame().child("first").child("second");
    }

    #[test]
    fn frame_public_geometry_helpers_update_frame_style() {
        let frame = crate::frame()
            .w(px(24.0))
            .h(px(32.0))
            .min_w(px(12.0))
            .max_h(px(64.0))
            .absolute()
            .inset_0()
            .left(px(4.0))
            .top(px(8.0))
            .rotate(0.25)
            .scale_xy(1.25, 0.75)
            .translate(point(px(2.0), px(3.0)))
            .skew_x(0.1)
            .transform_origin(point(0.0, 1.0))
            .opacity(0.42)
            .rounded(px(7.0))
            .clip();
        let layout = frame.style.layout_style();

        assert_eq!(layout.size.width, px(24.0).into());
        assert_eq!(layout.size.height, px(32.0).into());
        assert_eq!(layout.min_size.width, px(12.0).into());
        assert_eq!(layout.max_size.height, px(64.0).into());
        assert_eq!(layout.position, Position::Absolute);
        assert_eq!(layout.inset.left, px(4.0).into());
        assert_eq!(layout.inset.top, px(8.0).into());
        assert_eq!(layout.rotate, Some(0.25));
        assert_eq!(layout.scale, Some(point(1.25, 0.75)));
        assert_eq!(layout.translate, Some(point(px(2.0), px(3.0))));
        assert_eq!(layout.skew, Some(point(0.1, 0.0)));
        assert_eq!(layout.transform_origin, Some(point(0.0, 1.0)));
        assert_eq!(layout.opacity, Some(0.42));
        assert_eq!(layout.corner_radii.top_left, px(7.0).into());
        assert_eq!(layout.overflow.x, Overflow::Hidden);
        assert_eq!(layout.overflow.y, Overflow::Hidden);
    }

    struct EmptyFrameRoot;

    impl Render for EmptyFrameRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            super::Frame {
                style: FrameStyle::default().w(px(48.0)).h(px(32.0)),
                child: None,
                source_location: None,
            }
        }
    }

    #[gpux::test]
    fn frame_element_participates_in_render_lifecycle(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(120.0), px(90.0)), |_, _| EmptyFrameRoot);
        cx.run_until_parked();

        let quad_count = window
            .update(cx, |_, window, _| window.rendered_frame.scene.quads.len())
            .unwrap();
        assert_eq!(quad_count, 0);
    }

    struct RoundedFrameClipRoot;

    impl Render for RoundedFrameClipRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            crate::frame()
                .w(px(64.0))
                .h(px(64.0))
                .rounded(px(12.0))
                .clip()
                .child(div().w(px(80.0)).h(px(80.0)).bg(red()))
        }
    }

    #[gpux::test]
    fn frame_clip_radii_flow_to_child_scene_primitives(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(120.0), px(90.0)), |_, _| RoundedFrameClipRoot);
        cx.run_until_parked();

        let (corner_radii, scale_factor) = window
            .update(cx, |_, window, _| {
                (
                    window.rendered_frame.scene.quads[0]
                        .content_mask
                        .corner_radii,
                    window.scale_factor(),
                )
            })
            .unwrap();
        assert_eq!(corner_radii, Corners::all(px(12.0).scale(scale_factor)));
    }

    struct TransformedOpacityFrameClipRoot;

    impl Render for TransformedOpacityFrameClipRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            crate::frame()
                .w(px(64.0))
                .h(px(64.0))
                .translate(point(px(10.0), px(6.0)))
                .opacity(0.5)
                .rounded(px(8.0))
                .clip()
                .child(div().w(px(80.0)).h(px(80.0)).bg(red()))
        }
    }

    #[gpux::test]
    fn frame_clip_opacity_and_transform_compose_on_child_scene_primitives(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(120.0), px(90.0)), |_, _| {
            TransformedOpacityFrameClipRoot
        });
        cx.run_until_parked();

        let (content_mask, transformation, alpha, scale_factor) = window
            .update(cx, |_, window, _| {
                let quad = &window.rendered_frame.scene.quads[0];
                (
                    quad.content_mask.clone(),
                    quad.transformation,
                    quad.background.solid_color().a,
                    window.scale_factor(),
                )
            })
            .unwrap();

        let expected_translation = point(px(10.0).scale(scale_factor), px(6.0).scale(scale_factor));
        assert_eq!(
            content_mask.bounds.origin, expected_translation,
            "clip bounds should be transformed with the frame"
        );
        assert_eq!(
            content_mask.bounds.size,
            size(px(64.0).scale(scale_factor), px(64.0).scale(scale_factor))
        );
        assert_eq!(
            content_mask.corner_radii,
            Corners::all(px(8.0).scale(scale_factor))
        );
        assert_eq!(
            transformation,
            TransformationMatrix::unit().translate(expected_translation)
        );
        assert!((alpha - 0.5).abs() < f32::EPSILON);
    }

    struct EffectFrameElementRoot;

    impl Render for EffectFrameElementRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            crate::effect_frame()
                .w(px(64.0))
                .h(px(64.0))
                .rounded(px(12.0))
                .clip()
                .effect(backdrop_blur(px(18.0)))
                .effects([glow(hsla(0.5, 0.8, 0.7, 0.4))])
                .screen_effect_region("spotlight")
                .child(div().w(px(80.0)).h(px(80.0)).bg(red()))
        }
    }

    #[gpux::test]
    fn effect_frame_element_emits_effect_region_and_clips_child(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(120.0), px(90.0)), |_, _| EffectFrameElementRoot);
        cx.run_until_parked();

        let (effect_count, effect_radii, region_id, region_radii, child_mask_radii, scale_factor) =
            window
                .update(cx, |_, window, _| {
                    let scene = &window.rendered_frame.scene;
                    (
                        scene.visual_effects[0].effects.len(),
                        scene.visual_effects[0].corner_radii,
                        scene.screen_effect_regions[0].id.as_ref().to_string(),
                        scene.screen_effect_regions[0].corner_radii,
                        scene.quads[0].content_mask.corner_radii,
                        window.scale_factor(),
                    )
                })
                .unwrap();

        let expected_radii = Corners::all(px(12.0).scale(scale_factor));
        assert_eq!(effect_count, 2);
        assert_eq!(effect_radii, expected_radii);
        assert_eq!(region_id, "spotlight");
        assert_eq!(region_radii, expected_radii);
        assert_eq!(child_mask_radii, expected_radii);
    }

    struct AnimatedFrameElementRoot;

    impl Render for AnimatedFrameElementRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            crate::animated_frame("profile-card")
                .w(px(64.0))
                .h(px(48.0))
                .rounded(px(10.0))
                .clip()
                .child(div().w(px(80.0)).h(px(80.0)).bg(red()))
        }
    }

    #[gpux::test]
    fn animated_frame_element_participates_in_render_lifecycle(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(120.0), px(90.0)), |_, _| AnimatedFrameElementRoot);
        cx.run_until_parked();

        let (child_mask_radii, scale_factor) = window
            .update(cx, |_, window, _| {
                (
                    window.rendered_frame.scene.quads[0]
                        .content_mask
                        .corner_radii,
                    window.scale_factor(),
                )
            })
            .unwrap();

        assert_eq!(child_mask_radii, Corners::all(px(10.0).scale(scale_factor)));
    }

    struct AnimatedFrameProgressChildRoot {
        progress_values: Rc<RefCell<Vec<f32>>>,
    }

    impl Render for AnimatedFrameProgressChildRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let progress_values = self.progress_values.clone();
            crate::animated_frame("profile-card")
                .w(px(64.0))
                .h(px(48.0))
                .child_with_progress(move |progress, _, _| {
                    progress_values.borrow_mut().push(progress);
                    div().w(px(20.0)).h(px(20.0)).bg(red())
                })
        }
    }

    #[gpux::test]
    fn animated_frame_progress_child_participates_in_render_lifecycle(cx: &mut TestAppContext) {
        let progress_values = Rc::new(RefCell::new(Vec::new()));
        let window = cx.open_window(size(px(120.0), px(90.0)), |_, _| {
            AnimatedFrameProgressChildRoot {
                progress_values: progress_values.clone(),
            }
        });
        cx.run_until_parked();

        let quad_count = window
            .update(cx, |_, window, _| window.rendered_frame.scene.quads.len())
            .unwrap();
        assert_eq!(quad_count, 1);
        assert_eq!(progress_values.borrow().as_slice(), &[1.0]);
    }

    struct TransitionFrameElementRoot;

    impl Render for TransitionFrameElementRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            crate::transition_frame("profile-card")
                .target_key("compact")
                .w(px(64.0))
                .h(px(48.0))
                .rounded(px(10.0))
                .clip()
                .child(div().w(px(80.0)).h(px(80.0)).bg(red()))
        }
    }

    #[gpux::test]
    fn transition_frame_element_registers_region_and_paints_child(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(120.0), px(90.0)), |_, _| TransitionFrameElementRoot);
        cx.run_until_parked();

        let (region_id, child_mask_radii, snapshot_count, active_count, scale_factor) = window
            .update(cx, |_, window, _| {
                let scene = &window.rendered_frame.scene;
                (
                    scene.screen_effect_regions[0].id.as_ref().to_string(),
                    scene.quads[0].content_mask.corner_radii,
                    scene.active_screen_effect_snapshots.len(),
                    scene.active_screen_effects.len(),
                    window.scale_factor(),
                )
            })
            .unwrap();

        assert_eq!(region_id, "transition-frame-profile-card");
        assert_eq!(child_mask_radii, Corners::all(px(10.0).scale(scale_factor)));
        assert_eq!(snapshot_count, 0);
        assert_eq!(active_count, 0);
    }

    struct TransitionFrameOverrideRegionRoot;

    impl Render for TransitionFrameOverrideRegionRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            crate::transition_frame("profile-card")
                .target_key("compact")
                .screen_effect_region("profile-card-transition")
                .w(px(64.0))
                .h(px(48.0))
                .child(div().w(px(20.0)).h(px(20.0)).bg(red()))
        }
    }

    #[gpux::test]
    fn transition_frame_allows_explicit_region_override(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(120.0), px(90.0)), |_, _| {
            TransitionFrameOverrideRegionRoot
        });
        cx.run_until_parked();

        let region_id = window
            .update(cx, |_, window, _| {
                window.rendered_frame.scene.screen_effect_regions[0]
                    .id
                    .as_ref()
                    .to_string()
            })
            .unwrap();

        assert_eq!(region_id, "profile-card-transition");
    }

    struct TransitionFrameTargetKeyRoot {
        expanded: Rc<Cell<bool>>,
    }

    impl Render for TransitionFrameTargetKeyRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let expanded = self.expanded.get();
            let key = if expanded { "expanded" } else { "compact" };
            let size_px = if expanded { 56.0 } else { 24.0 };

            crate::transition_frame("profile-card")
                .target_key(key)
                .transition(crate::FrameTransition::ripple_reveal())
                .w(px(64.0))
                .h(px(64.0))
                .child(div().w(px(size_px)).h(px(size_px)).bg(red()))
        }
    }

    #[gpux::test]
    fn transition_frame_key_change_degrades_without_snapshot_capability(cx: &mut TestAppContext) {
        let expanded = Rc::new(Cell::new(false));
        let window = cx.open_window(size(px(120.0), px(90.0)), |_, _| {
            TransitionFrameTargetKeyRoot {
                expanded: expanded.clone(),
            }
        });
        cx.run_until_parked();

        expanded.set(true);
        cx.refresh().unwrap();
        cx.run_until_parked();

        let (snapshot_count, active_count, quad_count) = window
            .update(cx, |_, window, _| {
                (
                    window
                        .rendered_frame
                        .scene
                        .active_screen_effect_snapshots
                        .len(),
                    window.rendered_frame.scene.active_screen_effects.len(),
                    window.rendered_frame.scene.quads.len(),
                )
            })
            .unwrap();

        assert_eq!(snapshot_count, 0);
        assert_eq!(active_count, 0);
        assert_eq!(quad_count, 1);
    }
}
