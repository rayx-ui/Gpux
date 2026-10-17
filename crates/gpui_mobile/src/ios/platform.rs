//! iOS Platform implementation.
//!
//! This implements the Platform trait for iOS using UIKit.
//! Key differences from macOS:
//! - Uses UIApplication instead of NSApplication
//! - No menu bar (iOS apps don't have traditional menus)
//! - No windowed mode (iOS apps are always fullscreen on their display)
//! - Touch-based input instead of mouse
//! - System keyboard handling differs significantly

use super::{IosDispatcher, IosDisplay, IosWindow};
use anyhow::anyhow;
use futures::channel::oneshot;
use gpui::{
    Action, ActivityGuard, AnyWindowHandle, BackgroundExecutor, ClipboardItem, CursorStyle,
    DummyKeyboardMapper, ForegroundExecutor, GestureTuning, Keymap, Menu, MenuItem,
    PathPromptOptions, Platform, PlatformDisplay, PlatformGestures, PlatformKeyboardLayout,
    PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, Result, ScrollPhysics, Task,
    ThermalState, WindowAppearance, WindowParams,
};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use parking_lot::Mutex;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

pub struct IosPlatform(Mutex<IosPlatformState>);

/// Touch gesture tuning that matches UIKit's scroll deceleration.
struct IosGestures;

impl PlatformGestures for IosGestures {
    fn tuning(&self) -> GestureTuning {
        GestureTuning {
            scroll_physics: ScrollPhysics::ios(),
            ..GestureTuning::default()
        }
    }
}

pub(crate) struct IosPlatformState {
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
    text_system: Arc<dyn PlatformTextSystem>,
    finish_launching: Option<Box<dyn FnOnce()>>,
    quit_callback: Option<Box<dyn FnMut() -> bool>>,
    open_urls_callback: Option<Box<dyn FnMut(Vec<String>)>>,
    thermal_state_callback: Option<Box<dyn FnMut()>>,
}

impl Default for IosPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl IosPlatform {
    pub fn new() -> Self {
        let dispatcher = Arc::new(IosDispatcher);

        let text_system: Arc<dyn PlatformTextSystem> = Arc::new(super::IosTextSystem::new());

        Self(Mutex::new(IosPlatformState {
            background_executor: BackgroundExecutor::new(dispatcher.clone()),
            foreground_executor: ForegroundExecutor::new(dispatcher),
            text_system,
            finish_launching: None,
            quit_callback: None,
            open_urls_callback: None,
            thermal_state_callback: None,
        }))
    }
}

/// A simple iOS keyboard layout.
struct IosKeyboardLayout;

impl PlatformKeyboardLayout for IosKeyboardLayout {
    fn id(&self) -> &str {
        "ios-default"
    }

    fn name(&self) -> &str {
        "iOS Default"
    }
}

impl Platform for IosPlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        self.0.lock().background_executor.clone()
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.0.lock().foreground_executor.clone()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.0.lock().text_system.clone()
    }

    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        // Store the callback for later invocation via FFI.
        // The callback will be invoked when gpui_ios_did_finish_launching() is called
        // from the iOS app delegate's applicationDidFinishLaunchingWithOptions:.
        self.0.lock().finish_launching = Some(on_finish_launching);

        // On iOS, the app lifecycle is managed by UIApplicationMain which must be
        // called from main() before any Rust code runs. The Application::run() method
        // is called during app initialization, before UIApplicationMain starts its
        // event loop.
        //
        // The finish_launching callback is stored and will be invoked when the iOS
        // app delegate calls gpui_ios_did_finish_launching() via FFI.
        //
        // Unlike macOS where we call NSApplication.run() here, on iOS we don't need
        // to start the run loop - UIApplicationMain handles that.
        //
        // The callback is forwarded to the FFI layer so it can be invoked from Obj-C.
        if let Some(callback) = self.0.lock().finish_launching.take() {
            super::ffi::set_finish_launching_callback(callback);
        }

        log::info!("GPUI iOS: Platform::run() completed, waiting for app delegate callback");
    }

    fn quit(&self) {
        // iOS apps cannot programmatically quit - they can only be terminated by the user
        // or the system. We can suspend to background though.
        log::warn!("iOS apps cannot programmatically quit");
    }

    fn restart(&self, _binary_path: Option<PathBuf>, _arguments: Vec<OsString>) {
        // iOS apps cannot restart themselves
        log::warn!("iOS apps cannot restart themselves");
    }

    fn activate(&self, _ignoring_other_apps: bool) {
        // iOS handles app activation automatically
    }

    fn hide(&self) {
        // iOS apps cannot hide themselves
    }

    fn hide_other_apps(&self) {
        // Not applicable on iOS
    }

    fn unhide_other_apps(&self) {
        // Not applicable on iOS
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        IosDisplay::all()
            .map(|display| Rc::new(display) as Rc<dyn PlatformDisplay>)
            .collect()
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(IosDisplay::main()))
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        // iOS typically has one active window
        // This would need to track the current key window
        None
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        let window = Box::new(IosWindow::new(handle, options)?);
        // Register the window with FFI layer so Objective-C can access it for rendering
        window.register_with_ffi();
        Ok(window)
    }

    fn window_appearance(&self) -> WindowAppearance {
        unsafe {
            let style: i64 = {
                let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
                let key_window: *mut AnyObject = msg_send![app, keyWindow];
                if key_window.is_null() {
                    return WindowAppearance::Light;
                }
                let trait_collection: *mut AnyObject = msg_send![key_window, traitCollection];
                msg_send![trait_collection, userInterfaceStyle]
            };

            // UIUserInterfaceStyle: 0 = unspecified, 1 = light, 2 = dark
            match style {
                2 => WindowAppearance::Dark,
                _ => WindowAppearance::Light,
            }
        }
    }

    fn open_url(&self, url: &str) {
        unsafe {
            let url_string: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: url.as_ptr()];
            let url: *mut AnyObject = msg_send![class!(NSURL), URLWithString: url_string];
            let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
            let _: () = msg_send![app, openURL: url, options: std::ptr::null::<AnyObject>(), completionHandler: std::ptr::null::<AnyObject>()];
        }
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.0.lock().open_urls_callback = Some(callback);
    }

    fn register_url_scheme(&self, _url: &str) -> Task<Result<()>> {
        // URL schemes on iOS are registered in Info.plist, not programmatically
        Task::ready(Ok(()))
    }

    fn prompt_for_paths(
        &self,
        _options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        let (tx, rx) = oneshot::channel();
        // iOS uses UIDocumentPickerViewController for file selection
        // This would need to be implemented with proper UIKit integration
        let _ = tx.send(Err(anyhow!("File picker not yet implemented for iOS")));
        rx
    }

    fn prompt_for_new_path(
        &self,
        _directory: &Path,
        _suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(Err(anyhow!("Save dialog not yet implemented for iOS")));
        rx
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        false
    }

    fn reveal_path(&self, _path: &Path) {
        // iOS doesn't have a file manager like Finder
    }

    fn open_with_system(&self, _path: &Path) {
        // Would use UIDocumentInteractionController or UIActivityViewController
    }

    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.lock().quit_callback = Some(callback);
    }

    // UIKit delivers no system sleep or wake notifications to apps.
    fn on_system_sleep(&self, _callback: Box<dyn FnMut()>) {}

    fn on_system_wake(&self, _callback: Box<dyn FnMut()>) {}

    fn prevent_idle_sleep(&self, reason: &str) -> Task<Result<ActivityGuard>> {
        Task::ready(Err(anyhow!(
            "Idle sleep prevention for {reason:?} is not supported on iOS"
        )))
    }

    fn gestures(&self) -> Option<Rc<dyn PlatformGestures>> {
        Some(Rc::new(IosGestures))
    }

    fn on_reopen(&self, _callback: Box<dyn FnMut()>) {
        // iOS handles app reopening through scene lifecycle
    }

    fn set_menus(&self, _menus: Vec<Menu>, _keymap: &Keymap) {
        // iOS doesn't have a menu bar
        // Could potentially integrate with UIMenuBuilder for context menus
    }

    fn set_dock_menu(&self, _menu: Vec<MenuItem>, _keymap: &Keymap) {
        // iOS doesn't have a dock menu
    }

    fn on_app_menu_action(&self, _callback: Box<dyn FnMut(&dyn Action)>) {
        // Not applicable on iOS
    }

    fn on_will_open_app_menu(&self, _callback: Box<dyn FnMut()>) {
        // Not applicable on iOS
    }

    fn on_validate_app_menu_command(&self, _callback: Box<dyn FnMut(&dyn Action) -> bool>) {
        // Not applicable on iOS
    }

    fn app_path(&self) -> Result<PathBuf> {
        unsafe {
            let bundle: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
            let path: *mut AnyObject = msg_send![bundle, bundlePath];
            let utf8: *const i8 = msg_send![path, UTF8String];
            if utf8.is_null() {
                return Err(anyhow!("Failed to get bundle path"));
            }
            let path_str = std::ffi::CStr::from_ptr(utf8).to_str()?;
            Ok(PathBuf::from(path_str))
        }
    }

    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        let app_path = self.app_path()?;
        Ok(app_path.join(name))
    }

    fn set_cursor_style(&self, _style: CursorStyle) {
        // iOS doesn't have visible cursors (except for Apple Pencil hover on iPad)
    }

    fn hide_cursor_until_mouse_moves(&self) {
        // No-op: iOS does not expose a desktop-style mouse cursor.
    }

    fn is_cursor_visible(&self) -> bool {
        false
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        true // iOS always auto-hides scrollbars
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        unsafe {
            let pasteboard: *mut AnyObject = msg_send![class!(UIPasteboard), generalPasteboard];
            let Some(text) = item.text() else {
                return;
            };
            // The string is made from the text's bytes and length: a Rust string has no
            // terminating NUL for `stringWithUTF8String:` to stop at.
            let ns_text = clipboard_ns_string(&text);
            match item.html() {
                // Text and HTML as one item, so other apps paste either representation.
                Some(html) => {
                    let keys = [
                        clipboard_ns_string("public.utf8-plain-text"),
                        clipboard_ns_string("public.html"),
                    ];
                    let values = [ns_text, clipboard_ns_string(html)];
                    let dictionary: *mut AnyObject = msg_send![
                        class!(NSDictionary),
                        dictionaryWithObjects: values.as_ptr(),
                        forKeys: keys.as_ptr(),
                        count: 2usize
                    ];
                    let items: *mut AnyObject =
                        msg_send![class!(NSArray), arrayWithObject: dictionary];
                    let _: () = msg_send![pasteboard, setItems: items];
                }
                None => {
                    let _: () = msg_send![pasteboard, setString: ns_text];
                }
            }
        }
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        unsafe {
            let pasteboard: *mut AnyObject = msg_send![class!(UIPasteboard), generalPasteboard];
            let string: *mut AnyObject = msg_send![pasteboard, string];
            if string.is_null() {
                return None;
            }
            let mut item = ClipboardItem::new_string(clipboard_string_from(string)?);
            let html_type = clipboard_ns_string("public.html");
            let html: *mut AnyObject = msg_send![pasteboard, valueForPasteboardType: html_type];
            if !html.is_null() {
                let is_string: bool = msg_send![html, isKindOfClass: class!(NSString)];
                let html = if is_string {
                    clipboard_string_from(html)
                } else {
                    let length: usize = msg_send![html, length];
                    let bytes: *const std::ffi::c_void = msg_send![html, bytes];
                    (!bytes.is_null()).then(|| {
                        String::from_utf8_lossy(std::slice::from_raw_parts(
                            bytes.cast::<u8>(),
                            length,
                        ))
                        .into_owned()
                    })
                };
                if let Some(html) = html {
                    item.entries.push(gpui::ClipboardEntry::Html(html));
                }
            }
            Some(item)
        }
    }

    fn write_credentials(&self, _url: &str, _username: &str, _password: &[u8]) -> Task<Result<()>> {
        // Would use iOS Keychain Services
        Task::ready(Err(anyhow!("Keychain not yet implemented for iOS")))
    }

    fn read_credentials(&self, _url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Err(anyhow!("Keychain not yet implemented for iOS")))
    }

    fn delete_credentials(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow!("Keychain not yet implemented for iOS")))
    }

    fn on_keyboard_layout_change(&self, _callback: Box<dyn FnMut()>) {
        // iOS handles keyboard layout changes differently
    }

    fn thermal_state(&self) -> ThermalState {
        // iOS provides thermal state via ProcessInfo
        // For now, return nominal
        ThermalState::Nominal
    }

    fn on_thermal_state_change(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().thermal_state_callback = Some(callback);
        // In a full implementation, we would register for
        // NSProcessInfoThermalStateDidChangeNotification
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(IosKeyboardLayout)
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(DummyKeyboardMapper)
    }
}

/// An autoreleased `NSString` with `text`'s UTF-8 bytes, which need no terminating NUL.
unsafe fn clipboard_ns_string(text: &str) -> *mut AnyObject {
    unsafe {
        let string: *mut AnyObject = msg_send![class!(NSString), alloc];
        let string: *mut AnyObject = msg_send![
            string,
            initWithBytes: text.as_ptr() as *const std::ffi::c_void,
            length: text.len(),
            encoding: 4u64
        ];
        msg_send![string, autorelease]
    }
}

/// The UTF-8 text of an `NSString`, read by length rather than to a NUL.
unsafe fn clipboard_string_from(string: *mut AnyObject) -> Option<String> {
    unsafe {
        let data: *mut AnyObject = msg_send![string, dataUsingEncoding: 4u64];
        if data.is_null() {
            return None;
        }
        let length: usize = msg_send![data, length];
        let bytes: *const std::ffi::c_void = msg_send![data, bytes];
        if bytes.is_null() {
            return Some(String::new());
        }
        Some(
            String::from_utf8_lossy(std::slice::from_raw_parts(bytes.cast::<u8>(), length))
                .into_owned(),
        )
    }
}
