//! GPU errors on the wgpu headless renderer's shared device.
//!
//! Renderers on one adapter selection share a device, and every renderer on it observes the
//! errors raised there. These tests raise errors on the software adapter's device on purpose,
//! so they run in a process of their own, apart from `headless_renderer`, whose renderers
//! would otherwise observe them. Keep every test that raises GPU errors here, and only one per
//! adapter selection, since tests in one process run concurrently.
#![cfg(any(target_os = "linux", target_os = "windows"))]

use gpui_wgpu::{HeadlessAdapterSelection, WgpuHeadlessRenderer};
use gpux::{DevicePixels, PlatformHeadlessRenderer, Scene, size};

/// Raises a validation error on `renderer`'s device: a buffer may not be mappable for both
/// reading and writing without a native-only feature.
fn raise_validation_error(renderer: &WgpuHeadlessRenderer) {
    let _ = renderer
        .device()
        .create_buffer(&gpui_wgpu::wgpu::BufferDescriptor {
            label: Some("invalid_for_test"),
            size: 4,
            usage: gpui_wgpu::wgpu::BufferUsages::MAP_READ
                | gpui_wgpu::wgpu::BufferUsages::MAP_WRITE,
            mapped_at_creation: false,
        });
}

#[test]
fn every_renderer_on_a_shared_device_observes_its_gpu_errors() {
    let render = |renderer: &mut WgpuHeadlessRenderer| {
        renderer.render_scene_to_image(&Scene::default(), size(DevicePixels(4), DevicePixels(4)))
    };
    let mut first =
        WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software).unwrap();
    let later = WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software).unwrap();
    assert!(first.shares_device_with(&later));
    // Dropping the newest renderer must not stop the earlier one from seeing errors.
    drop(later);

    raise_validation_error(&first);
    let error = render(&mut first).expect_err("the error fails the next render");
    assert!(
        format!("{error:#}").contains("GPU error during headless rendering"),
        "{error:#}"
    );
    // Each error is reported once; the renderer stays usable.
    render(&mut first).unwrap();

    // A renderer joining after an error does not report it, while existing ones do.
    raise_validation_error(&first);
    let mut joined =
        WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software).unwrap();
    render(&mut joined).unwrap();
    assert!(render(&mut first).is_err());

    // An error raised after both exist reaches both.
    raise_validation_error(&joined);
    assert!(render(&mut first).is_err());
    assert!(render(&mut joined).is_err());
}
