//! iOS platform view implementation.
//!
//! Embeds native iOS `UIView` instances in the GPUI render tree through
//! window composition: `platform_view_element` attaches the view to the
//! container of a composition native surface, which GPUI sizes and places
//! between its base content and its overlay.

use crate::platform_view::{
    PlatformView, PlatformViewBounds, PlatformViewFactory, PlatformViewId, PlatformViewParams,
};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "ios")]
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

#[cfg(target_os = "ios")]
use super::cg_types::ObjcCGRect;

/// iOS implementation of a platform view.
///
/// Wraps a `UIView` instance, which fills the composition container it is
/// attached to.
pub struct IosPlatformView {
    id: PlatformViewId,
    view_type: String,
    /// Pointer to the Objective-C UIView instance.
    /// Null if disposed.
    #[cfg(target_os = "ios")]
    native_view: std::sync::Mutex<*mut AnyObject>,
    disposed: AtomicBool,
}

// Safety: UIView operations are dispatched to the main thread.
unsafe impl Send for IosPlatformView {}
unsafe impl Sync for IosPlatformView {}

impl IosPlatformView {
    /// Create a new iOS platform view.
    ///
    /// The UIView is allocated and initialized with the given bounds; it
    /// shows once attached to a composition container.
    #[cfg(target_os = "ios")]
    pub fn new(view_type: &str, params: &PlatformViewParams) -> Result<Self, String> {
        let id = PlatformViewId::next();

        let native_view = Self::create_native_view(view_type, &params.bounds, params)?;

        Ok(Self {
            id,
            view_type: view_type.to_string(),
            native_view: std::sync::Mutex::new(native_view),
            disposed: AtomicBool::new(false),
        })
    }

    /// Create the native UIView via Objective-C runtime.
    ///
    /// Dispatches to type-specific creation for known view types:
    /// - "video_player": Creates UIView with AVPlayerLayer (requires player_id in params)
    /// - "webview": Creates WKWebView (uses url/html from params)
    /// - "camera_preview": Creates UIView with AVCaptureVideoPreviewLayer (requires session_id in params)
    /// - Default: Creates a generic UIView container
    #[cfg(target_os = "ios")]
    fn create_native_view(
        view_type: &str,
        bounds: &PlatformViewBounds,
        params: &PlatformViewParams,
    ) -> Result<*mut AnyObject, String> {
        unsafe {
            let frame = ObjcCGRect::new(
                bounds.x as f64,
                bounds.y as f64,
                bounds.width as f64,
                bounds.height as f64,
            );

            let view: *mut AnyObject = match view_type {
                "video_player" => Self::create_video_player_view(frame, params)?,
                "webview" => Self::create_webview_view(frame, params)?,
                "camera_preview" => Self::create_camera_preview_view(frame, params)?,
                _ => Self::create_generic_view(frame)?,
            };

            if view.is_null() {
                return Err(format!("Failed to create UIView for type '{}'", view_type));
            }

            // Clip to bounds
            let _: () = msg_send![view, setClipsToBounds: true];

            log::info!(
                "IosPlatformView: created native UIView for type '{}' at ({}, {}, {}, {})",
                view_type,
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
            );

            Ok(view)
        }
    }

    /// Create a generic transparent UIView container.
    #[cfg(target_os = "ios")]
    unsafe fn create_generic_view(frame: ObjcCGRect) -> Result<*mut AnyObject, String> {
        let uiview_class = class!(UIView);
        let view: *mut AnyObject = msg_send![uiview_class, alloc];
        let view: *mut AnyObject = msg_send![view, initWithFrame: frame];
        if view.is_null() {
            return Err("Failed to create UIView".into());
        }
        let clear_color: *mut AnyObject = msg_send![class!(UIColor), clearColor];
        let _: () = msg_send![view, setBackgroundColor: clear_color];
        Ok(view)
    }

    /// Create a UIView with an AVPlayerLayer for video playback.
    #[cfg(target_os = "ios")]
    unsafe fn create_video_player_view(
        frame: ObjcCGRect,
        params: &PlatformViewParams,
    ) -> Result<*mut AnyObject, String> {
        // Create a container UIView
        let uiview_class = class!(UIView);
        let view: *mut AnyObject = msg_send![uiview_class, alloc];
        let view: *mut AnyObject = msg_send![view, initWithFrame: frame];
        if view.is_null() {
            return Err("Failed to create UIView for video_player".into());
        }

        let black_color: *mut AnyObject = msg_send![class!(UIColor), blackColor];
        let _: () = msg_send![view, setBackgroundColor: black_color];

        // If a player_id is provided, try to get the AVPlayer and create AVPlayerLayer
        if let Some(player_id_str) = params.creation_params.get("player_id") {
            if let Ok(player_id) = player_id_str.parse::<u32>() {
                #[cfg(feature = "video_player")]
                {
                    // Get AVPlayer from the video_player package's PLAYERS map
                    if let Some(player_ptr) =
                        crate::packages::video_player::ios_get_player(player_id)
                    {
                        let player_layer: *mut AnyObject =
                            msg_send![class!(AVPlayerLayer), playerLayerWithPlayer: player_ptr];
                        if !player_layer.is_null() {
                            let _: () = msg_send![player_layer, setFrame: frame];
                            // Set video gravity to aspect fit
                            // SAFETY: runs on the main thread, inside the autorelease pool UIKit
                            // provides; the string is autoreleased, so it is not released here.
                            let gravity =
                                unsafe { Self::make_nsstring("AVLayerVideoGravityResizeAspect") };
                            let _: () = msg_send![player_layer, setVideoGravity: gravity];
                            let view_layer: *mut AnyObject = msg_send![view, layer];
                            let _: () = msg_send![view_layer, addSublayer: player_layer];
                        }
                    }
                }

                #[cfg(not(feature = "video_player"))]
                {
                    let _ = player_id;
                    return Err("video_player package feature is not enabled".into());
                }
            }
        }

        Ok(view)
    }

    /// Create a WKWebView.
    #[cfg(target_os = "ios")]
    unsafe fn create_webview_view(
        frame: ObjcCGRect,
        params: &PlatformViewParams,
    ) -> Result<*mut AnyObject, String> {
        let config: *mut AnyObject = msg_send![class!(WKWebViewConfiguration), alloc];
        let config: *mut AnyObject = msg_send![config, init];
        if config.is_null() {
            return Err("Failed to create WKWebViewConfiguration".into());
        }

        let js_enabled = params
            .creation_params
            .get("javascript_enabled")
            .map(|v| v == "true")
            .unwrap_or(true);

        let prefs: *mut AnyObject = msg_send![config, preferences];
        if !prefs.is_null() {
            let _: () = msg_send![prefs, setJavaScriptEnabled: js_enabled];
        }

        let webview: *mut AnyObject = msg_send![class!(WKWebView), alloc];
        let webview: *mut AnyObject =
            msg_send![webview, initWithFrame: frame, configuration: config];
        if webview.is_null() {
            return Err("Failed to create WKWebView".into());
        }

        // Load URL or HTML if provided
        if let Some(url) = params.creation_params.get("url") {
            if !url.is_empty() {
                // SAFETY: runs on the main thread, inside the autorelease pool UIKit provides.
                let ns_url_str = unsafe { Self::make_nsstring(url) };
                let nsurl: *mut AnyObject = msg_send![class!(NSURL), URLWithString: ns_url_str];
                if !nsurl.is_null() {
                    let request: *mut AnyObject =
                        msg_send![class!(NSURLRequest), requestWithURL: nsurl];
                    let _: *mut AnyObject = msg_send![webview, loadRequest: request];
                }
            }
        } else if let Some(html) = params.creation_params.get("html") {
            if !html.is_empty() {
                // SAFETY: runs on the main thread, inside the autorelease pool UIKit provides.
                let ns_html = unsafe { Self::make_nsstring(html) };
                let base_url: *mut AnyObject = std::ptr::null_mut();
                let _: *mut AnyObject =
                    msg_send![webview, loadHTMLString: ns_html, baseURL: base_url];
            }
        }

        Ok(webview)
    }

    /// Create a UIView with AVCaptureVideoPreviewLayer for camera preview.
    #[cfg(target_os = "ios")]
    unsafe fn create_camera_preview_view(
        frame: ObjcCGRect,
        params: &PlatformViewParams,
    ) -> Result<*mut AnyObject, String> {
        let uiview_class = class!(UIView);
        let view: *mut AnyObject = msg_send![uiview_class, alloc];
        let view: *mut AnyObject = msg_send![view, initWithFrame: frame];
        if view.is_null() {
            return Err("Failed to create UIView for camera_preview".into());
        }

        let black_color: *mut AnyObject = msg_send![class!(UIColor), blackColor];
        let _: () = msg_send![view, setBackgroundColor: black_color];

        // If a session_id is provided, try to get the AVCaptureSession and create preview layer
        if let Some(session_id_str) = params.creation_params.get("session_id") {
            if let Ok(session_id) = session_id_str.parse::<usize>() {
                #[cfg(feature = "camera")]
                {
                    if let Some(session_ptr) = crate::packages::camera::ios_get_session(session_id)
                    {
                        let layer: *mut AnyObject =
                            msg_send![class!(AVCaptureVideoPreviewLayer), alloc];
                        let layer: *mut AnyObject = msg_send![layer, initWithSession: session_ptr];
                        if !layer.is_null() {
                            let _: () = msg_send![layer, setFrame: frame];
                            // SAFETY: runs on the main thread, inside the autorelease pool UIKit
                            // provides.
                            let gravity = unsafe {
                                Self::make_nsstring("AVLayerVideoGravityResizeAspectFill")
                            };
                            let _: () = msg_send![layer, setVideoGravity: gravity];
                            let view_layer: *mut AnyObject = msg_send![view, layer];
                            let _: () = msg_send![view_layer, addSublayer: layer];
                        }
                    }
                }

                #[cfg(not(feature = "camera"))]
                {
                    let _ = session_id;
                    return Err("camera package feature is not enabled".into());
                }
            }
        }

        Ok(view)
    }

    #[cfg(target_os = "ios")]
    unsafe fn make_nsstring(s: &str) -> *mut AnyObject {
        // SAFETY: the caller upholds `nsstring`'s contract, the same one this function states.
        unsafe { crate::ios::util::nsstring(s) }
    }

    /// Returns the raw pointer to the underlying `UIView`.
    ///
    /// Use this to insert the view into the iOS view hierarchy from
    /// the window/paint code. Returns null if the view has been disposed.
    #[cfg(target_os = "ios")]
    pub fn native_view_ptr(&self) -> *mut AnyObject {
        *self.native_view.lock().unwrap()
    }
}

impl PlatformView for IosPlatformView {
    fn id(&self) -> PlatformViewId {
        self.id
    }

    fn view_type(&self) -> &str {
        &self.view_type
    }

    /// The view fills its container, which composition places.
    fn set_bounds(&self, _: PlatformViewBounds) {}

    fn set_visible(&self, visible: bool) {
        if self.disposed.load(Ordering::Relaxed) {
            return;
        }
        #[cfg(target_os = "ios")]
        {
            let view = *self.native_view.lock().unwrap();
            if !view.is_null() {
                unsafe {
                    let _: () = msg_send![view, setHidden: !visible];
                }
            }
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = visible;
        }
    }

    /// The view's frame in its container, in points.
    fn set_frame(&self, frame: PlatformViewBounds, _scale_factor: f32) {
        if self.disposed.load(Ordering::Relaxed) {
            return;
        }
        #[cfg(target_os = "ios")]
        {
            let view = *self.native_view.lock().unwrap();
            if !view.is_null() {
                let frame = ObjcCGRect::new(
                    frame.x as f64,
                    frame.y as f64,
                    frame.width as f64,
                    frame.height as f64,
                );
                unsafe {
                    let _: () = msg_send![view, setFrame: frame];
                }
            }
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = frame;
        }
    }

    /// Composition orders native surfaces; the view's own z-index is moot.
    fn set_z_index(&self, _: i32) {}

    /// Adds the view to `container`, a composition native surface's
    /// `UIView*` (as a `usize`), which clips it.
    fn attach(&self, container: &dyn std::any::Any) -> Result<(), String> {
        let container = *container
            .downcast_ref::<usize>()
            .ok_or("the container is not a composition native surface's")?;
        #[cfg(target_os = "ios")]
        {
            let view = *self.native_view.lock().unwrap();
            if view.is_null() {
                return Err(format!("{} is disposed", self.id));
            }
            let container = container as *mut AnyObject;
            unsafe {
                // `set_frame` places the view; until then it fills the container.
                let bounds: ObjcCGRect = msg_send![container, bounds];
                let _: () = msg_send![view, setFrame: bounds];
                let _: () = msg_send![container, addSubview: view];
            }
            log::info!("IosPlatformView: attached view {}", self.id);
            Ok(())
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = container;
            Err("iOS platform views are only available on iOS".to_string())
        }
    }

    fn dispose(&self) {
        if self.disposed.swap(true, Ordering::Relaxed) {
            return;
        }
        #[cfg(target_os = "ios")]
        {
            let view = *self.native_view.lock().unwrap();
            if !view.is_null() {
                unsafe {
                    // Remove from superview if it was added to one.
                    let _: () = msg_send![view, removeFromSuperview];
                }
            }
        }
        log::info!("IosPlatformView: disposed view {}", self.id);
    }

    fn is_disposed(&self) -> bool {
        self.disposed.load(Ordering::Relaxed)
    }
}

/// iOS platform view factory.
pub struct IosPlatformViewFactory {
    view_type: String,
}

impl IosPlatformViewFactory {
    pub fn new(view_type: &str) -> Self {
        Self {
            view_type: view_type.to_string(),
        }
    }
}

impl PlatformViewFactory for IosPlatformViewFactory {
    fn create(&self, params: &PlatformViewParams) -> Result<Box<dyn PlatformView>, String> {
        #[cfg(target_os = "ios")]
        {
            let view = IosPlatformView::new(&self.view_type, params)?;
            Ok(Box::new(view))
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = params;
            Err("iOS platform views are only available on iOS".to_string())
        }
    }

    fn view_type(&self) -> &str {
        &self.view_type
    }
}
