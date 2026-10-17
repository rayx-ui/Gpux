//! Effect diagnostics: `Window::effect_diagnostics` counts the last rendered frame's active screen
//! effects, their snapshots and regions, and the region-targeted effects and snapshot transitions
//! whose region or snapshot the frame lacks.

use gpux::{
    Context, EffectCapabilities, EffectDiagnostics, IntoElement, Render, ScreenEffectSnapshotId,
    ScreenEffectTarget, TestAppContext, Window, WindowHandle, canvas, div, point, prelude::*, px,
    screen, size,
};

/// Long enough that no effect expires while a test runs.
const DURATION_MS: u64 = 60_000;

/// A window that paints the screen-effect region `region`, when it names one.
struct Regions {
    region: Option<&'static str>,
}

impl Render for Regions {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let region = self.region;
        div().size_full().child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    if let Some(region) = region {
                        window.paint_screen_effect_region(region, bounds, Default::default());
                    }
                },
            )
            .size(px(40.)),
        )
    }
}

fn open(cx: &mut TestAppContext, region: Option<&'static str>) -> WindowHandle<Regions> {
    let window = cx.open_window(size(px(100.), px(100.)), move |_, _| Regions { region });
    cx.run_until_parked();
    window
}

/// Draws a frame and returns its diagnostics.
fn diagnostics(cx: &mut TestAppContext, window: WindowHandle<Regions>) -> EffectDiagnostics {
    cx.refresh().unwrap();
    cx.run_until_parked();
    window
        .update(cx, |_, window, _| window.effect_diagnostics())
        .unwrap()
}

#[gpux::test]
fn an_active_screen_effect_is_counted(cx: &mut TestAppContext) {
    let window = open(cx, None);
    let quiet = diagnostics(cx, window);
    assert_eq!(
        quiet,
        EffectDiagnostics {
            active_screen_effect_count: 0,
            screen_effect_snapshot_count: 0,
            visual_effect_count: 0,
            screen_effect_region_count: 0,
            missing_screen_effect_region_count: 0,
            missing_screen_effect_snapshot_count: 0,
            capabilities: EffectCapabilities::default(),
        }
    );

    window
        .update(cx, |_, window, _| {
            window.play_screen_effect(
                screen::ripple(point(px(10.), px(10.))).duration_ms(DURATION_MS),
            );
            window.play_screen_effect(
                screen::ripple(point(px(20.), px(20.))).duration_ms(DURATION_MS),
            );
        })
        .unwrap();
    let playing = diagnostics(cx, window);
    assert_eq!(playing.active_screen_effect_count, 2);
    // A window-targeted effect needs no region and no snapshot.
    assert_eq!(playing.missing_screen_effect_region_count, 0);
    assert_eq!(playing.missing_screen_effect_snapshot_count, 0);
}

#[gpux::test]
fn a_region_targeted_effect_counts_its_region_missing_until_painted(cx: &mut TestAppContext) {
    let play = |cx: &mut TestAppContext, window: WindowHandle<Regions>| {
        window
            .update(cx, |_, window, _| {
                window.play_screen_effect(
                    screen::ripple(point(px(0.), px(0.)))
                        .target(ScreenEffectTarget::region("panel"))
                        .duration_ms(DURATION_MS),
                );
            })
            .unwrap();
    };

    let without_region = open(cx, None);
    play(cx, without_region);
    let missing = diagnostics(cx, without_region);
    assert_eq!(missing.active_screen_effect_count, 1);
    assert_eq!(missing.screen_effect_region_count, 0);
    assert_eq!(missing.missing_screen_effect_region_count, 1);

    let with_region = open(cx, Some("panel"));
    play(cx, with_region);
    let present = diagnostics(cx, with_region);
    assert_eq!(present.active_screen_effect_count, 1);
    assert_eq!(present.screen_effect_region_count, 1);
    assert_eq!(present.missing_screen_effect_region_count, 0);

    // Another region does not stand in for the one targeted.
    let other_region = open(cx, Some("sidebar"));
    play(cx, other_region);
    let other = diagnostics(cx, other_region);
    assert_eq!(other.screen_effect_region_count, 1);
    assert_eq!(other.missing_screen_effect_region_count, 1);
}

#[gpux::test]
fn a_snapshot_transition_counts_its_snapshot_missing_until_captured(cx: &mut TestAppContext) {
    let window = open(cx, None);
    window
        .update(cx, |_, window, _| {
            window.play_screen_effect(
                screen::crossfade(ScreenEffectSnapshotId::new(404)).duration_ms(DURATION_MS),
            );
        })
        .unwrap();
    let missing = diagnostics(cx, window);
    assert_eq!(missing.active_screen_effect_count, 1);
    assert_eq!(missing.screen_effect_snapshot_count, 0);
    assert_eq!(missing.missing_screen_effect_snapshot_count, 1);

    let captured = open(cx, None);
    captured
        .update(cx, |_, window, _| {
            let snapshot = window.capture_screen_effect_snapshot(ScreenEffectTarget::window());
            window.play_screen_effect(screen::crossfade(snapshot).duration_ms(DURATION_MS));
        })
        .unwrap();
    let present = diagnostics(cx, captured);
    assert_eq!(present.active_screen_effect_count, 1);
    assert_eq!(present.screen_effect_snapshot_count, 1);
    assert_eq!(present.missing_screen_effect_snapshot_count, 0);
}
