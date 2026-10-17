#![deny(unsafe_op_in_unsafe_fn)]

use std::{
    cell::{Cell, RefCell},
    num::NonZeroIsize,
    path::PathBuf,
    rc::{Rc, Weak},
    str::FromStr,
    sync::{Arc, Once, atomic::AtomicBool},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use futures::channel::oneshot::{self, Receiver};
use gpui_util::ResultExt;
use raw_window_handle as rwh;
use smallvec::SmallVec;
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Dwm::*,
        Graphics::Gdi::*,
        System::{
            Com::*, Diagnostics::Debug::MessageBeep, LibraryLoader::*, Ole::*, SystemServices::*,
        },
        UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
    },
    core::*,
};

use crate::direct_manipulation::DirectManipulationHandler;
use crate::*;
use gpui_wgpu::{WgpuRenderer, WgpuSurfaceConfig, native_backend_policy_label};
use gpux::*;

pub(crate) struct WindowsWindow(pub Rc<WindowsWindowInner>);

#[derive(Clone, Copy, Debug)]
pub(crate) struct WgpuWin32Window {
    hwnd: NonZeroIsize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowsSurfaceResize {
    Resize(Size<DevicePixels>),
    Suspend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowsWgpuDeviceRecoveryAction {
    Continue,
    RecoverRenderer,
}

impl WgpuWin32Window {
    pub(crate) fn new(hwnd: HWND) -> Self {
        Self {
            hwnd: NonZeroIsize::new(hwnd.0 as isize)
                .expect("WGPU Win32 window handle must be non-null"),
        }
    }
}

impl rwh::HasWindowHandle for WgpuWin32Window {
    fn window_handle(&self) -> std::result::Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let raw = rwh::Win32WindowHandle::new(self.hwnd).into();
        Ok(unsafe { rwh::WindowHandle::borrow_raw(raw) })
    }
}

impl rwh::HasDisplayHandle for WgpuWin32Window {
    fn display_handle(&self) -> std::result::Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(rwh::DisplayHandle::windows())
    }
}

#[cfg(not(target_family = "wasm"))]
#[allow(dead_code)]
const _: fn(HWND) -> WgpuWin32Window = WgpuWin32Window::new;

#[cfg(not(target_family = "wasm"))]
#[allow(dead_code)]
const _: fn(
    gpui_wgpu::GpuContext,
    &WgpuWin32Window,
    gpui_wgpu::WgpuSurfaceConfig,
    Option<gpui_wgpu::CompositorGpuHint>,
) -> anyhow::Result<gpui_wgpu::WgpuRenderer> = gpui_wgpu::WgpuRenderer::new::<WgpuWin32Window>;

impl std::ops::Deref for WindowsWindow {
    type Target = WindowsWindowInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct WindowsWindowState {
    pub origin: Cell<Point<Pixels>>,
    pub logical_size: Cell<Size<Pixels>>,
    pub min_size: Option<Size<Pixels>>,
    pub fullscreen_restore_bounds: Cell<Bounds<Pixels>>,
    pub border_offset: WindowBorderOffset,
    pub appearance: Cell<WindowAppearance>,
    pub background_appearance: Cell<WindowBackgroundAppearance>,
    pub scale_factor: Cell<f32>,
    pub restore_from_minimized: Cell<Option<Box<dyn FnMut(RequestFrameOptions)>>>,

    pub callbacks: Callbacks,
    pub frame_signal: Arc<PlatformFrameSignal>,
    pub input_handler: Cell<Option<PlatformInputHandler>>,
    // Windows interaction smoothness depends on keeping all user-driven redraws off the
    // invalidate/WM_PAINT path. Mouse moves are coalesced at the source, and all input sources
    // share one high-priority update message. Do not remove these fields during upstream merges
    // unless the backend gets an equivalent replacement.
    pub pending_mouse_move: RefCell<Option<MouseMoveEvent>>,
    pub input_update_posted: Rc<Cell<bool>>,
    pub ime_enabled: Cell<bool>,
    pub pending_surrogate: Cell<Option<u16>>,
    pub last_reported_modifiers: Cell<Option<Modifiers>>,
    pub last_reported_capslock: Cell<Option<Capslock>>,
    pub hovered: Cell<bool>,
    pub last_visibility: Cell<Option<WindowVisibility>>,
    pub direct_manipulation: DirectManipulationHandler,

    pub renderer: RefCell<WgpuRenderer>,
    pub fast_composition: RefCell<crate::fast::composition::Composition>,
    pub subpixel_rendering_supported: bool,
    /// Coalesce live-resize surface changes until the frame that draws with the
    /// matching GPUI viewport. This avoids presenting with a surface size from
    /// one resize message and scene geometry from another.
    pub pending_drawable_size: Cell<Option<Size<DevicePixels>>>,
    /// Set after a GPU device-lost recovery so the next `draw_window` call is
    /// treated as a forced render. This guarantees the next frame both
    /// bypasses the GPUI view cache and rebuilds the scene against fresh atlas
    /// resources.
    pub force_render_after_recovery: Cell<bool>,
    /// True while Windows is running the modal user resize/move loop.
    pub in_size_move_loop: Cell<bool>,
    /// Coalesces retry draws requested by WGPU surface acquire/reconfigure.
    pub wgpu_force_update_posted: Cell<bool>,

    pub click_state: ClickState,
    pub current_cursor: Cell<Option<HCURSOR>>,
    /// Shared with [`WindowsPlatformState::cursor_visible`].
    pub cursor_visible: Arc<AtomicBool>,
    pub nc_button_pressed: Cell<Option<u32>>,

    pub display: Cell<WindowsDisplay>,
    fullscreen: Cell<Option<StyleAndBounds>>,
    initial_placement: Cell<Option<WindowOpenStatus>>,
    hwnd: HWND,
    #[cfg(feature = "accessibility")]
    pub(crate) a11y: RefCell<Option<A11yState>>,
    pub(crate) destroyed: Cell<bool>,
    pub(crate) evaluating_close: Cell<bool>,
    pub(crate) drag_drop_registered: Cell<bool>,
}

pub(crate) struct WindowsWindowInner {
    pub(crate) hwnd: HWND,
    pub(crate) dialog_owner: Rc<crate::dialog::DialogOwner>,
    drop_target_helper: IDropTargetHelper,
    pub(crate) state: WindowsWindowState,
    system_settings: WindowsSystemSettings,
    pub(crate) handle: AnyWindowHandle,
    pub(crate) hide_title_bar: bool,
    pub(crate) is_movable: bool,
    pub(crate) executor: ForegroundExecutor,
    pub(crate) validation_number: usize,
    pub(crate) main_receiver: PriorityQueueReceiver<RunnableVariant>,
    pub(crate) platform_window_handle: HWND,
    pub(crate) parent_hwnd: Option<HWND>,
}

impl WindowsWindowState {
    fn new(
        hwnd: HWND,
        gpu_context: gpui_wgpu::GpuContext,
        window_params: &CREATESTRUCTW,
        current_cursor: Option<HCURSOR>,
        cursor_visible: Arc<AtomicBool>,
        display: WindowsDisplay,
        min_size: Option<Size<Pixels>>,
        appearance: WindowAppearance,
    ) -> Result<Self> {
        let scale_factor = {
            let monitor_dpi = unsafe { GetDpiForWindow(hwnd) } as f32;
            monitor_dpi / USER_DEFAULT_SCREEN_DPI as f32
        };
        let origin = logical_point(window_params.x as f32, window_params.y as f32, scale_factor);
        let logical_size = {
            let physical_size = size(
                DevicePixels(window_params.cx),
                DevicePixels(window_params.cy),
            );
            physical_size.to_pixels(scale_factor)
        };
        let fullscreen_restore_bounds = Bounds {
            origin,
            size: logical_size,
        };
        let border_offset = WindowBorderOffset::default();
        let restore_from_minimized = None;
        let device_size = size(
            DevicePixels(window_params.cx.max(1)),
            DevicePixels(window_params.cy.max(1)),
        );
        log::info!(
            "Creating Windows WGPU renderer: backend_policy={}, size={}x{}, present_mode=Fifo, transparent=false",
            native_backend_policy_label(),
            device_size.width.0,
            device_size.height.0
        );
        let renderer = WgpuRenderer::new(
            gpu_context,
            &WgpuWin32Window::new(hwnd),
            windows_wgpu_surface_config(device_size),
            None,
        )
        .context("Creating Windows WGPU renderer")?;
        let subpixel_rendering_supported = renderer.supports_dual_source_blending();
        let gpu_specs = renderer.gpu_specs();
        log::info!(
            "Created Windows WGPU renderer: device='{}', driver='{}', driver_info='{}', software={}",
            gpu_specs.device_name,
            gpu_specs.driver_name,
            gpu_specs.driver_info,
            gpu_specs.is_software_emulated
        );
        let callbacks = Callbacks::default();
        let input_handler = None;
        let pending_mouse_move = RefCell::new(None);
        let input_update_posted = Rc::new(Cell::new(false));
        let pending_surrogate = None;
        let last_reported_modifiers = None;
        let last_reported_capslock = None;
        let hovered = false;
        let click_state = ClickState::new();
        let nc_button_pressed = None;
        let fullscreen = None;
        let initial_placement = None;

        let direct_manipulation =
            DirectManipulationHandler::new(hwnd, scale_factor, Rc::clone(&input_update_posted))
                .context("initializing Direct Manipulation")?;

        Ok(Self {
            origin: Cell::new(origin),
            logical_size: Cell::new(logical_size),
            fullscreen_restore_bounds: Cell::new(fullscreen_restore_bounds),
            border_offset,
            appearance: Cell::new(appearance),
            background_appearance: Cell::new(WindowBackgroundAppearance::Opaque),
            scale_factor: Cell::new(scale_factor),
            restore_from_minimized: Cell::new(restore_from_minimized),
            min_size,
            callbacks,
            frame_signal: Arc::new(PlatformFrameSignal::new()),
            input_handler: Cell::new(input_handler),
            pending_mouse_move,
            input_update_posted,
            ime_enabled: Cell::new(true),
            pending_surrogate: Cell::new(pending_surrogate),
            last_reported_modifiers: Cell::new(last_reported_modifiers),
            last_reported_capslock: Cell::new(last_reported_capslock),
            hovered: Cell::new(hovered),
            last_visibility: Cell::new(None),
            renderer: RefCell::new(renderer),
            fast_composition: RefCell::default(),
            subpixel_rendering_supported,
            pending_drawable_size: Cell::new(None),
            force_render_after_recovery: Cell::new(false),
            in_size_move_loop: Cell::new(false),
            wgpu_force_update_posted: Cell::new(false),
            click_state,
            current_cursor: Cell::new(current_cursor),
            cursor_visible,
            nc_button_pressed: Cell::new(nc_button_pressed),
            display: Cell::new(display),
            fullscreen: Cell::new(fullscreen),
            initial_placement: Cell::new(initial_placement),
            hwnd,
            direct_manipulation,
            #[cfg(feature = "accessibility")]
            a11y: RefCell::new(None),
            destroyed: Cell::new(false),
            evaluating_close: Cell::new(false),
            drag_drop_registered: Cell::new(false),
        })
    }

    #[inline]
    pub(crate) fn is_fullscreen(&self) -> bool {
        self.fullscreen.get().is_some()
    }

    pub(crate) fn is_maximized(&self) -> bool {
        !self.is_fullscreen() && unsafe { IsZoomed(self.hwnd) }.as_bool()
    }

    fn bounds(&self) -> Bounds<Pixels> {
        Bounds {
            origin: self.origin.get(),
            size: self.logical_size.get(),
        }
    }

    // Calculate the bounds used for saving and whether the window is maximized.
    fn calculate_window_bounds(&self) -> (Bounds<Pixels>, bool) {
        let placement = unsafe {
            let mut placement = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            GetWindowPlacement(self.hwnd, &mut placement)
                .context("failed to get window placement")
                .log_err();
            placement
        };
        (
            calculate_client_rect(
                placement.rcNormalPosition,
                &self.border_offset,
                self.scale_factor.get(),
            ),
            placement.showCmd == SW_SHOWMAXIMIZED.0 as u32,
        )
    }

    fn window_bounds(&self) -> WindowBounds {
        let (bounds, maximized) = self.calculate_window_bounds();

        if self.is_fullscreen() {
            WindowBounds::Fullscreen(self.fullscreen_restore_bounds.get())
        } else if maximized {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        }
    }

    /// get the logical size of the app's drawable area.
    ///
    /// Currently, GPUI uses the logical size of the app to handle mouse interactions (such as
    /// whether the mouse collides with other elements of GPUI).
    fn content_size(&self) -> Size<Pixels> {
        self.logical_size.get()
    }
}

fn windows_wgpu_surface_config(device_size: Size<DevicePixels>) -> WgpuSurfaceConfig {
    WgpuSurfaceConfig {
        size: device_size,
        transparent: false,
        preferred_present_mode: None,
    }
}

pub(crate) fn windows_surface_resize_for_client_size(
    width: i32,
    height: i32,
    minimized: bool,
) -> WindowsSurfaceResize {
    if minimized || width <= 0 || height <= 0 {
        WindowsSurfaceResize::Suspend
    } else {
        WindowsSurfaceResize::Resize(size(DevicePixels(width), DevicePixels(height)))
    }
}

fn windows_wgpu_device_recovery_action(device_lost: bool) -> WindowsWgpuDeviceRecoveryAction {
    if device_lost {
        WindowsWgpuDeviceRecoveryAction::RecoverRenderer
    } else {
        WindowsWgpuDeviceRecoveryAction::Continue
    }
}

pub(crate) fn windows_wgpu_frame_request_options(
    force_render: bool,
    require_presentation: bool,
    resize_frame_pending: bool,
    recovery_frame_pending: bool,
) -> RequestFrameOptions {
    RequestFrameOptions {
        require_presentation: require_presentation
            || resize_frame_pending
            || recovery_frame_pending,
        force_render: force_render || resize_frame_pending || recovery_frame_pending,
        ..Default::default()
    }
}

pub(crate) fn windows_wgpu_should_post_retry_frame(
    needs_redraw: bool,
    already_posted: bool,
    in_size_move_loop: bool,
) -> bool {
    needs_redraw && !already_posted && !in_size_move_loop
}

fn windows_wgpu_transparency_for_background_appearance(
    background_appearance: WindowBackgroundAppearance,
) -> bool {
    background_appearance != WindowBackgroundAppearance::Opaque
}

fn windows_effect_capabilities_from_renderer(
    renderer: Option<&WgpuRenderer>,
) -> EffectCapabilities {
    renderer
        .map(WgpuRenderer::effect_capabilities)
        .unwrap_or_default()
}

impl WindowsWindowInner {
    /// Whether the window is being presented: shown and not minimized. Windows
    /// has no notification for a window fully covered by other windows, so
    /// that case reports `Visible`.
    pub(crate) fn visibility(&self) -> WindowVisibility {
        let is_visible =
            unsafe { IsWindowVisible(self.hwnd).as_bool() && !IsIconic(self.hwnd).as_bool() };
        if is_visible {
            WindowVisibility::Visible
        } else {
            WindowVisibility::Hidden
        }
    }

    fn new(context: &mut WindowCreateContext, hwnd: HWND, cs: &CREATESTRUCTW) -> Result<Rc<Self>> {
        let state = WindowsWindowState::new(
            hwnd,
            context.gpu_context.clone(),
            cs,
            context.current_cursor,
            context.cursor_visible.clone(),
            context.display,
            context.min_size,
            context.appearance,
        )?;

        Ok(Rc::new(Self {
            hwnd,
            dialog_owner: crate::dialog::DialogOwner::new(hwnd),
            drop_target_helper: context.drop_target_helper.clone(),
            state,
            handle: context.handle,
            hide_title_bar: context.hide_title_bar,
            is_movable: context.is_movable,
            executor: context.executor.clone(),
            validation_number: context.validation_number,
            main_receiver: context.main_receiver.clone(),
            platform_window_handle: context.platform_window_handle,
            system_settings: WindowsSystemSettings::new(),
            parent_hwnd: context.parent_hwnd,
        }))
    }

    fn toggle_fullscreen(self: &Rc<Self>) {
        let this = self.clone();
        self.executor
            .spawn(async move {
                let StyleAndBounds {
                    style,
                    x,
                    y,
                    cx,
                    cy,
                } = match this.state.fullscreen.take() {
                    Some(state) => state,
                    None => {
                        let (window_bounds, _) = this.state.calculate_window_bounds();
                        this.state.fullscreen_restore_bounds.set(window_bounds);

                        let style =
                            WINDOW_STYLE(unsafe { get_window_long(this.hwnd, GWL_STYLE) } as _);
                        let mut rc = RECT::default();
                        unsafe { GetWindowRect(this.hwnd, &mut rc) }
                            .context("failed to get window rect")
                            .log_err();
                        let _ = this.state.fullscreen.set(Some(StyleAndBounds {
                            style,
                            x: rc.left,
                            y: rc.top,
                            cx: rc.right - rc.left,
                            cy: rc.bottom - rc.top,
                        }));
                        let style = style
                            & !(WS_THICKFRAME
                                | WS_SYSMENU
                                | WS_MAXIMIZEBOX
                                | WS_MINIMIZEBOX
                                | WS_CAPTION);
                        let physical_bounds = this.state.display.get().physical_bounds();
                        StyleAndBounds {
                            style,
                            x: physical_bounds.left().0,
                            y: physical_bounds.top().0,
                            cx: physical_bounds.size.width.0,
                            cy: physical_bounds.size.height.0,
                        }
                    }
                };
                set_non_rude_hwnd(this.hwnd, !this.state.is_fullscreen());
                unsafe { set_window_long(this.hwnd, GWL_STYLE, style.0 as isize) };
                unsafe {
                    SetWindowPos(
                        this.hwnd,
                        None,
                        x,
                        y,
                        cx,
                        cy,
                        SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER,
                    )
                }
                .log_err();
            })
            .detach();
    }

    fn set_window_placement(self: &Rc<Self>) -> Result<()> {
        let Some(open_status) = self.state.initial_placement.take() else {
            return Ok(());
        };
        match open_status.state {
            WindowOpenState::Maximized => unsafe {
                SetWindowPlacement(self.hwnd, &open_status.placement)
                    .context("failed to set window placement")?;
                ShowWindowAsync(self.hwnd, SW_MAXIMIZE).ok()?;
            },
            WindowOpenState::Fullscreen => {
                unsafe {
                    SetWindowPlacement(self.hwnd, &open_status.placement)
                        .context("failed to set window placement")?
                };
                self.toggle_fullscreen();
            }
            WindowOpenState::Windowed => unsafe {
                SetWindowPlacement(self.hwnd, &open_status.placement)
                    .context("failed to set window placement")?;
            },
        }
        Ok(())
    }

    pub(crate) fn system_settings(&self) -> &WindowsSystemSettings {
        &self.system_settings
    }
}

#[derive(Default)]
pub(crate) struct Callbacks {
    pub(crate) request_frame: Cell<Option<Box<dyn FnMut(RequestFrameOptions)>>>,
    pub(crate) input: Cell<Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>>,
    pub(crate) active_status_change: Cell<Option<Box<dyn FnMut(bool)>>>,
    pub(crate) minimized_status_change: Cell<Option<Box<dyn FnMut(bool)>>>,
    pub(crate) visibility_change: Cell<Option<Box<dyn FnMut(WindowVisibility)>>>,
    pub(crate) hovered_status_change: Cell<Option<Box<dyn FnMut(bool)>>>,
    pub(crate) resize: Cell<Option<Box<dyn FnMut(Size<Pixels>, f32)>>>,
    pub(crate) moved: Cell<Option<Box<dyn FnMut()>>>,
    pub(crate) should_close: Cell<Option<Box<dyn FnMut() -> bool>>>,
    pub(crate) close: Cell<Option<Box<dyn FnOnce()>>>,
    pub(crate) hit_test_window_control: Cell<Option<Box<dyn FnMut() -> Option<WindowControlArea>>>>,
    pub(crate) appearance_changed: Cell<Option<Box<dyn FnMut()>>>,
}

struct WindowCreateContext {
    inner: Option<Result<Rc<WindowsWindowInner>>>,
    handle: AnyWindowHandle,
    hide_title_bar: bool,
    display: WindowsDisplay,
    is_movable: bool,
    min_size: Option<Size<Pixels>>,
    executor: ForegroundExecutor,
    current_cursor: Option<HCURSOR>,
    cursor_visible: Arc<AtomicBool>,
    drop_target_helper: IDropTargetHelper,
    validation_number: usize,
    main_receiver: PriorityQueueReceiver<RunnableVariant>,
    platform_window_handle: HWND,
    appearance: WindowAppearance,
    gpu_context: gpui_wgpu::GpuContext,
    parent_hwnd: Option<HWND>,
}

impl WindowsWindow {
    pub(crate) fn new(
        handle: AnyWindowHandle,
        params: WindowParams,
        creation_info: WindowCreationInfo,
    ) -> Result<Self> {
        let WindowCreationInfo {
            icon,
            executor,
            current_cursor,
            cursor_visible,
            drop_target_helper,
            validation_number,
            main_receiver,
            platform_window_handle,
            gpu_context,
        } = creation_info;
        register_window_class(icon);
        let parent_hwnd = if params.kind == WindowKind::Dialog {
            let parent_window = unsafe { GetActiveWindow() };
            if parent_window.is_invalid() {
                None
            } else {
                // Disable the parent window to make this dialog modal
                unsafe {
                    EnableWindow(parent_window, false).as_bool();
                };
                Some(parent_window)
            }
        } else {
            None
        };
        let hide_title_bar = params
            .titlebar
            .as_ref()
            .map(|titlebar| titlebar.appears_transparent)
            .unwrap_or(true);
        let window_name = HSTRING::from(
            params
                .titlebar
                .as_ref()
                .and_then(|titlebar| titlebar.title.as_ref())
                .map(|title| title.as_ref())
                .unwrap_or(""),
        );

        let (dwexstyle, dwstyle) = if params.kind == WindowKind::PopUp {
            (WS_EX_TOOLWINDOW, WINDOW_STYLE(0x0))
        } else {
            let mut dwstyle = WS_SYSMENU;

            if params.is_resizable {
                dwstyle |= WS_THICKFRAME | WS_MAXIMIZEBOX;
            }

            if params.is_minimizable {
                dwstyle |= WS_MINIMIZEBOX;
            }
            let dwexstyle = if params.kind == WindowKind::Dialog {
                dwstyle |= WS_POPUP | WS_CAPTION;
                WS_EX_DLGMODALFRAME
            } else {
                WS_EX_APPWINDOW
            };

            (dwexstyle, dwstyle)
        };
        let hinstance = get_module_handle();
        let display = if let Some(display_id) = params.display_id {
            WindowsDisplay::new(display_id)
        } else {
            None
        }
        .or_else(WindowsDisplay::primary_monitor)
        .context("failed to find any monitor")?;
        let appearance = system_appearance().unwrap_or_default();
        let mut context = WindowCreateContext {
            inner: None,
            handle,
            hide_title_bar,
            display,
            is_movable: params.is_movable,
            min_size: params.window_min_size,
            executor,
            current_cursor,
            cursor_visible,
            drop_target_helper,
            validation_number,
            main_receiver,
            platform_window_handle,
            appearance,
            gpu_context,
            parent_hwnd,
        };
        let creation_result = unsafe {
            CreateWindowExW(
                dwexstyle,
                WINDOW_CLASS_NAME,
                &window_name,
                dwstyle,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                parent_hwnd,
                None,
                Some(hinstance.into()),
                Some(&context as *const _ as *const _),
            )
        };

        // Failure to create a `WindowsWindowState` can cause window creation to fail,
        // so check the inner result first.
        let this = context.inner.take().transpose()?;
        let hwnd = creation_result?;
        let this = this.unwrap();

        register_drag_drop(&this)?;
        set_non_rude_hwnd(hwnd, true);
        configure_dwm_dark_mode(hwnd, appearance);
        this.state.border_offset.update(hwnd)?;
        let placement =
            retrieve_window_placement(hwnd, display, params.bounds, &this.state.border_offset)?;
        if params.show {
            unsafe { SetWindowPlacement(hwnd, &placement)? };
        } else {
            this.state.initial_placement.set(Some(WindowOpenStatus {
                placement,
                state: WindowOpenState::Windowed,
            }));
        }

        Ok(Self(this))
    }
}

impl rwh::HasWindowHandle for WindowsWindow {
    fn window_handle(&self) -> std::result::Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let raw = rwh::Win32WindowHandle::new(unsafe {
            NonZeroIsize::new_unchecked(self.0.hwnd.0 as isize)
        })
        .into();
        Ok(unsafe { rwh::WindowHandle::borrow_raw(raw) })
    }
}

impl rwh::HasDisplayHandle for WindowsWindow {
    fn display_handle(&self) -> std::result::Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(rwh::DisplayHandle::windows())
    }
}

impl Drop for WindowsWindow {
    fn drop(&mut self) {
        self.0.dialog_owner.close();
        if self.state.destroyed.get() {
            return;
        }
        unsafe { ShowWindowAsync(self.0.hwnd, SW_HIDE).ok().log_err() };
        // `DestroyWindow` below sends `WM_SHOWWINDOW`; without a callback the
        // resulting visibility report has nothing to notify.
        self.0.state.callbacks.visibility_change.take();
        // clone this `Rc` to prevent early release of the pointer
        let this = self.0.clone();
        self.0
            .executor
            .spawn(async move {
                this.dialog_owner.when_idle().await;
                if !this.state.destroyed.get() {
                    // SAFETY: the retained window is owned by this foreground
                    // thread and has not entered WM_DESTROY. That message owns
                    // resource cleanup, including the drag-drop registration.
                    unsafe { DestroyWindow(this.hwnd).log_err() };
                }
            })
            .detach();
    }
}

impl PlatformWindow for WindowsWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.state.bounds()
    }

    fn is_maximized(&self) -> bool {
        self.state.is_maximized()
    }

    fn window_bounds(&self) -> WindowBounds {
        self.state.window_bounds()
    }

    /// get the logical size of the app's drawable area.
    ///
    /// Currently, GPUI uses the logical size of the app to handle mouse interactions (such as
    /// whether the mouse collides with other elements of GPUI).
    fn content_size(&self) -> Size<Pixels> {
        self.state.content_size()
    }

    fn resize(&mut self, size: Size<Pixels>) {
        let hwnd = self.0.hwnd;
        let bounds = gpux::bounds(self.bounds().origin, size).to_device_pixels(self.scale_factor());
        let rect = calculate_window_rect(bounds, &self.state.border_offset);

        self.0
            .executor
            .spawn(async move {
                unsafe {
                    SetWindowPos(
                        hwnd,
                        None,
                        bounds.origin.x.0,
                        bounds.origin.y.0,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        SWP_NOMOVE,
                    )
                    .context("unable to set window content size")
                    .log_err();
                }
            })
            .detach();
    }

    fn scale_factor(&self) -> f32 {
        self.state.scale_factor.get()
    }

    fn appearance(&self) -> WindowAppearance {
        self.state.appearance.get()
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(self.state.display.get()))
    }

    fn mouse_position(&self) -> Point<Pixels> {
        let scale_factor = self.scale_factor();
        let point = unsafe {
            let mut point: POINT = std::mem::zeroed();
            GetCursorPos(&mut point)
                .context("unable to get cursor position")
                .log_err();
            ScreenToClient(self.0.hwnd, &mut point).ok().log_err();
            point
        };
        logical_point(point.x as f32, point.y as f32, scale_factor)
    }

    fn modifiers(&self) -> Modifiers {
        current_modifiers()
    }

    fn capslock(&self) -> Capslock {
        current_capslock()
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.state.input_handler.set(Some(input_handler));
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.state.input_handler.take()
    }

    fn prompt(
        &self,
        level: PromptLevel,
        msg: &str,
        detail: Option<&str>,
        answers: &[PromptButton],
    ) -> Option<Receiver<usize>> {
        let (mut done_tx, done_rx) = oneshot::channel();
        let msg = msg.to_string();
        let detail_string = detail.map(|detail| detail.to_string());
        let answers = answers.to_vec();
        let dialog = crate::dialog::show_dialog(
            Some(self.0.dialog_owner.clone()),
            &self.0.executor,
            move |handle| {
                unsafe {
                    let mut config = TASKDIALOGCONFIG::default();
                    config.cbSize = std::mem::size_of::<TASKDIALOGCONFIG>() as _;
                    config.hwndParent = handle;
                    config.dwFlags = TDF_ALLOW_DIALOG_CANCELLATION;
                    let title;
                    let main_icon;
                    match level {
                        PromptLevel::Info => {
                            title = windows::core::w!("Info");
                            main_icon = TD_INFORMATION_ICON;
                        }
                        PromptLevel::Warning => {
                            title = windows::core::w!("Warning");
                            main_icon = TD_WARNING_ICON;
                        }
                        PromptLevel::Critical => {
                            title = windows::core::w!("Critical");
                            main_icon = TD_ERROR_ICON;
                        }
                    };
                    config.pszWindowTitle = title;
                    config.Anonymous1.pszMainIcon = main_icon;
                    let instruction = HSTRING::from(msg);
                    config.pszMainInstruction = PCWSTR::from_raw(instruction.as_ptr());
                    let hints_encoded;
                    if let Some(ref hints) = detail_string {
                        hints_encoded = HSTRING::from(hints);
                        config.pszContent = PCWSTR::from_raw(hints_encoded.as_ptr());
                    };
                    let mut button_id_map = Vec::with_capacity(answers.len());
                    let mut buttons = Vec::new();
                    let mut btn_encoded = Vec::new();
                    for (index, btn) in answers.iter().enumerate() {
                        let encoded = HSTRING::from(btn.label().as_ref());
                        let button_id = match btn {
                            PromptButton::Ok(_) => IDOK.0,
                            PromptButton::Cancel(_) => IDCANCEL.0,
                            // the first few low integer values are reserved for known buttons
                            // so for simplicity we just go backwards from -1
                            PromptButton::Other(_) => -(index as i32) - 1,
                        };
                        button_id_map.push(button_id);
                        buttons.push(TASKDIALOG_BUTTON {
                            nButtonID: button_id,
                            pszButtonText: PCWSTR::from_raw(encoded.as_ptr()),
                        });
                        btn_encoded.push(encoded);
                    }
                    config.cButtons = buttons.len() as _;
                    config.pButtons = buttons.as_ptr();

                    config.pfCallback = Some(crate::dialog::task_dialog_callback);
                    config.lpCallbackData = button_id_map.contains(&IDCANCEL.0) as isize;
                    let mut res = std::mem::zeroed();
                    TaskDialogIndirect(&config, Some(&mut res), None, None)
                        .context("unable to create task dialog")?;
                    Ok(button_id_map.iter().position(|&button_id| button_id == res))
                }
            },
        );
        self.0
            .executor
            .spawn(async move {
                if let futures::future::Either::Left((result, _)) =
                    futures::future::select(dialog, done_tx.cancellation()).await
                    && let Some(Some(clicked)) = result
                        .context("native dialog thread stopped")
                        .and_then(|result| result)
                        .log_err()
                {
                    done_tx.send(clicked).ok();
                }
            })
            .detach();

        Some(done_rx)
    }

    fn activate(&self) {
        let hwnd = self.0.hwnd;
        let this = self.0.clone();
        self.0
            .executor
            .spawn(async move {
                this.set_window_placement().log_err();

                unsafe {
                    // If the window is minimized, restore it.
                    if IsIconic(hwnd).as_bool() {
                        ShowWindowAsync(hwnd, SW_RESTORE).ok().log_err();
                    }

                    SetActiveWindow(hwnd).ok();
                    SetFocus(Some(hwnd)).ok();
                }

                // premium ragebait by windows, this is needed because the window
                // must have received an input event to be able to set itself to foreground
                // so let's just simulate user input as that seems to be the most reliable way
                // some more info: https://gist.github.com/Aetopia/1581b40f00cc0cadc93a0e8ccb65dc8c
                // bonus: this bug also doesn't manifest if you have vs attached to the process
                let inputs = [
                    INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VK_MENU,
                                dwFlags: KEYBD_EVENT_FLAGS(0),
                                ..Default::default()
                            },
                        },
                    },
                    INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VK_MENU,
                                dwFlags: KEYEVENTF_KEYUP,
                                ..Default::default()
                            },
                        },
                    },
                ];
                unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };

                // todo(windows)
                // crate `windows 0.56` reports true as Err
                unsafe { SetForegroundWindow(hwnd).as_bool() };
            })
            .detach();
    }

    fn request_attention(&self) {
        if self.is_active() {
            return;
        }

        let hwnd = self.0.hwnd;
        self.0
            .executor
            .spawn(async move {
                let info = FLASHWINFO {
                    cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                    hwnd,
                    dwFlags: FLASHW_ALL,
                    uCount: 1,
                    dwTimeout: 0,
                };
                unsafe { FlashWindowEx(&info).ok().log_err() };
            })
            .detach();
    }

    fn is_active(&self) -> bool {
        self.0.hwnd == unsafe { GetActiveWindow() }
    }

    fn visibility(&self) -> WindowVisibility {
        self.0.visibility()
    }

    fn is_hovered(&self) -> bool {
        self.state.hovered.get()
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        self.state.background_appearance.get()
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        self.state.subpixel_rendering_supported
    }

    fn set_title(&mut self, title: &str) {
        unsafe { SetWindowTextW(self.0.hwnd, &HSTRING::from(title)) }
            .inspect_err(|e| log::error!("Set title failed: {e}"))
            .ok();
    }

    fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
        self.state.background_appearance.set(background_appearance);
        let hwnd = self.0.hwnd;

        // using Dwm APIs for Mica and MicaAlt backdrops.
        // others follow the set_window_composition_attribute approach
        match background_appearance {
            WindowBackgroundAppearance::Opaque => {
                set_window_composition_attribute(hwnd, None, 0);
            }
            WindowBackgroundAppearance::Transparent => {
                set_window_composition_attribute(hwnd, None, 2);
            }
            WindowBackgroundAppearance::Blurred => {
                set_window_composition_attribute(hwnd, Some((0, 0, 0, 0)), 4);
            }
            WindowBackgroundAppearance::MicaBackdrop => {
                // DWMSBT_MAINWINDOW => MicaBase
                dwm_set_window_composition_attribute(hwnd, 2);
            }
            WindowBackgroundAppearance::MicaAltBackdrop => {
                // DWMSBT_TABBEDWINDOW => MicaAlt
                dwm_set_window_composition_attribute(hwnd, 4);
            }
        }
        self.state.renderer.borrow_mut().update_transparency(
            windows_wgpu_transparency_for_background_appearance(background_appearance),
        );
    }

    fn minimize(&self) {
        unsafe { ShowWindowAsync(self.0.hwnd, SW_MINIMIZE).ok().log_err() };
    }

    fn zoom(&self) {
        unsafe {
            if IsWindowVisible(self.0.hwnd).as_bool() {
                ShowWindowAsync(self.0.hwnd, SW_MAXIMIZE).ok().log_err();
            } else if let Some(mut status) = self.state.initial_placement.take() {
                status.state = WindowOpenState::Maximized;
                self.state.initial_placement.set(Some(status));
            }
        }
    }

    fn toggle_fullscreen(&self) {
        if unsafe { IsWindowVisible(self.0.hwnd).as_bool() } {
            self.0.toggle_fullscreen();
        } else if let Some(mut status) = self.state.initial_placement.take() {
            status.state = WindowOpenState::Fullscreen;
            self.state.initial_placement.set(Some(status));
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.state.is_fullscreen()
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.state.callbacks.request_frame.set(Some(callback));
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.state.callbacks.input.set(Some(callback));
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0
            .state
            .callbacks
            .active_status_change
            .set(Some(callback));
    }

    fn on_minimized_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0
            .state
            .callbacks
            .minimized_status_change
            .set(Some(callback));
    }

    fn on_visibility_change(&self, callback: Box<dyn FnMut(WindowVisibility)>) {
        self.0.state.last_visibility.set(Some(self.0.visibility()));
        self.0.state.callbacks.visibility_change.set(Some(callback));
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0
            .state
            .callbacks
            .hovered_status_change
            .set(Some(callback));
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.state.callbacks.resize.set(Some(callback));
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.state.callbacks.moved.set(Some(callback));
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.state.callbacks.should_close.set(Some(callback));
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.state.callbacks.close.set(Some(callback));
    }

    fn on_hit_test_window_control(&self, callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
        self.0
            .state
            .callbacks
            .hit_test_window_control
            .set(Some(callback));
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0
            .state
            .callbacks
            .appearance_changed
            .set(Some(callback));
    }

    fn draw(&self, scene: &Scene) {
        let mut renderer = self.state.renderer.borrow_mut();

        if let Some(size) = self.state.pending_drawable_size.take() {
            renderer.update_drawable_size(size);
        }

        if windows_wgpu_device_recovery_action(renderer.device_lost())
            == WindowsWgpuDeviceRecoveryAction::RecoverRenderer
        {
            log::warn!("Windows WGPU device loss detected; attempting renderer recovery");

            match renderer.recover(&WgpuWin32Window::new(self.0.hwnd)) {
                Ok(()) => {
                    log::info!("Windows WGPU recovery completed; forcing a fresh render");
                    crate::fast::composition::device_lost(&self.0);
                }
                Err(err) => {
                    log::warn!(
                        "Windows WGPU recovery failed; forcing a fresh render and retrying on the next frame: {err:#}"
                    );
                }
            }

            self.state.force_render_after_recovery.set(true);
            return;
        }

        renderer.draw(scene);

        if renderer.needs_redraw() {
            self.0.post_wgpu_force_update_window();
        }
    }

    fn draw_composed(&self, scene: gpux::ComposedScene<'_>) {
        crate::fast::composition::draw_composed(self, scene)
    }

    fn rasterizes_layer_tiles(&self) -> bool {
        // The wgpu renderer does (`gpui_wgpu`'s `fast::layers`).
        true
    }

    fn enable_window_composition(&self) -> anyhow::Result<()> {
        crate::fast::composition::enable_window_composition(self)
    }

    fn create_native_surface(&self) -> anyhow::Result<Rc<dyn gpux::PlatformSurfaceAttachment>> {
        crate::fast::composition::create_native_surface(self)
    }

    fn set_composition_order(
        &self,
        fast_surfaces: &[gpux::PlatformCompositionSurface],
    ) -> anyhow::Result<()> {
        crate::fast::composition::set_composition_order(self, fast_surfaces)
    }

    fn render_to_image(&self, scene: &Scene) -> anyhow::Result<image::RgbaImage> {
        let mut renderer = self.state.renderer.borrow_mut();
        if let Some(size) = self.state.pending_drawable_size.take() {
            renderer.update_drawable_size(size);
        }
        renderer.render_to_image(scene)
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.state.renderer.borrow().sprite_atlas().clone()
    }

    fn get_raw_handle(&self) -> HWND {
        self.0.hwnd
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        Some(self.state.renderer.borrow().gpu_specs())
    }

    fn effect_capabilities(&self) -> EffectCapabilities {
        let renderer = self.state.renderer.borrow();
        windows_effect_capabilities_from_renderer(Some(&renderer))
    }

    fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        let scale_factor = self.state.scale_factor.get();
        let caret_position = POINT {
            x: (bounds.origin.x.as_f32() * scale_factor) as i32,
            y: (bounds.origin.y.as_f32() * scale_factor) as i32
                + ((bounds.size.height.as_f32() * scale_factor) as i32 / 2),
        };

        self.0.update_ime_position(self.0.hwnd, caret_position);
    }

    fn play_system_bell(&self) {
        // MB_OK: The sound specified as the Windows Default Beep sound.
        let _ = unsafe { MessageBeep(MB_OK) };
    }

    #[cfg(feature = "accessibility")]
    fn a11y_init(&self, callbacks: gpux::A11yCallbacks) {
        if self.state.destroyed.get() {
            return;
        }
        let action_handler = A11yActionHandler(callbacks.action);
        let is_focused = unsafe { GetForegroundWindow() } == self.0.hwnd;
        let adapter = accesskit_windows::Adapter::new(
            accesskit_windows::HWND(self.0.hwnd.0),
            is_focused,
            action_handler,
        );
        let activation_handler = A11yActivationHandler {
            callback: callbacks.activation,
        };

        *self.state.a11y.borrow_mut() = Some(A11yState {
            adapter,
            activation_handler,
        });
    }

    #[cfg(feature = "accessibility")]
    fn a11y_tree_update(&self, tree_update: accesskit::TreeUpdate) {
        let events = {
            let mut a11y = self.state.a11y.borrow_mut();
            a11y.as_mut()
                .and_then(|a11y| a11y.adapter.update_if_active(|| tree_update))
        };
        // The borrow must be dropped before raising events, because
        // `events.raise()` calls `UiaRaiseAutomationPropertyChangedEvent`
        // which may send a nested `WM_GETOBJECT` back into this window
        // procedure, re-entering `handle_wm_getobject` which also borrows
        // `self.state.a11y`.
        if let Some(events) = events {
            events.raise();
        }
    }

    fn a11y_update_window_bounds(&self) {
        // Windows UIA handles window bounds tracking automatically.
    }
}

#[cfg(feature = "accessibility")]
pub(crate) struct A11yState {
    pub(crate) adapter: accesskit_windows::Adapter,
    pub(crate) activation_handler: A11yActivationHandler,
}

#[cfg(feature = "accessibility")]
pub(crate) struct A11yActivationHandler {
    callback: Box<dyn Fn() -> Option<accesskit::TreeUpdate> + Send + 'static>,
}

#[cfg(feature = "accessibility")]
impl accesskit::ActivationHandler for A11yActivationHandler {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        (self.callback)()
    }
}

#[cfg(feature = "accessibility")]
struct A11yActionHandler(Box<dyn Fn(accesskit::ActionRequest) + Send + 'static>);

#[cfg(feature = "accessibility")]
impl accesskit::ActionHandler for A11yActionHandler {
    fn do_action(&mut self, request: accesskit::ActionRequest) {
        (self.0)(request);
    }
}

impl WindowsWindowInner {
    fn post_wgpu_force_update_window(&self) {
        if !windows_wgpu_should_post_retry_frame(
            true,
            self.state.wgpu_force_update_posted.get(),
            self.state.in_size_move_loop.get(),
        ) {
            return;
        }

        self.state.wgpu_force_update_posted.set(true);
        let post_result = unsafe {
            PostMessageW(
                Some(self.hwnd),
                WM_GPUI_FORCE_UPDATE_WINDOW,
                WPARAM::default(),
                LPARAM::default(),
            )
        };
        if let Err(err) = post_result {
            self.state.wgpu_force_update_posted.set(false);
            log::error!("failed to post Windows WGPU retry frame: {err}");
        }
    }

    pub(crate) fn unconfigure_wgpu_surface(&self) {
        self.state.pending_drawable_size.set(None);
        self.state.renderer.borrow_mut().unconfigure_surface();
        self.state.force_render_after_recovery.set(true);
    }

    pub(crate) fn destroy_wgpu_surface(&self) {
        self.state.pending_drawable_size.set(None);
        self.state.renderer.borrow_mut().destroy();
    }
}

#[implement(IDropTarget)]
struct WindowsDragDropHandler(pub Rc<WindowsWindowInner>);

impl WindowsDragDropHandler {
    fn handle_drag_drop(&self, input: PlatformInput) {
        if let Some(mut func) = self.0.state.callbacks.input.take() {
            func(input);
            self.0.state.callbacks.input.set(Some(func));
        }
    }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for WindowsDragDropHandler_Impl {
    fn DragEnter(
        &self,
        pdataobj: windows::core::Ref<IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe {
            let idata_obj = pdataobj.ok()?;
            let config = FORMATETC {
                cfFormat: CF_HDROP.0,
                ptd: std::ptr::null_mut() as _,
                dwAspect: DVASPECT_CONTENT.0,
                lindex: -1,
                tymed: TYMED_HGLOBAL.0 as _,
            };
            let cursor_position = POINT { x: pt.x, y: pt.y };
            if idata_obj.QueryGetData(&config as _) == S_OK {
                *pdweffect = DROPEFFECT_COPY;
                let Some(mut idata) = idata_obj.GetData(&config as _).log_err() else {
                    return Ok(());
                };
                if idata.u.hGlobal.is_invalid() {
                    return Ok(());
                }
                let hdrop = HDROP(idata.u.hGlobal.0);
                let mut paths = SmallVec::<[PathBuf; 2]>::new();
                with_file_names(hdrop, |file_name| {
                    if let Some(path) = PathBuf::from_str(&file_name).log_err() {
                        paths.push(path);
                    }
                });
                ReleaseStgMedium(&mut idata);
                let mut cursor_position = cursor_position;
                ScreenToClient(self.0.hwnd, &mut cursor_position)
                    .ok()
                    .log_err();
                let scale_factor = self.0.state.scale_factor.get();
                let input = PlatformInput::FileDrop(FileDropEvent::Entered {
                    position: logical_point(
                        cursor_position.x as f32,
                        cursor_position.y as f32,
                        scale_factor,
                    ),
                    paths: ExternalPaths(paths),
                });
                self.handle_drag_drop(input);
            } else {
                *pdweffect = DROPEFFECT_NONE;
            }
            self.0
                .drop_target_helper
                .DragEnter(self.0.hwnd, idata_obj, &cursor_position, *pdweffect)
                .log_err();
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let mut cursor_position = POINT { x: pt.x, y: pt.y };
        unsafe {
            *pdweffect = DROPEFFECT_COPY;
            self.0
                .drop_target_helper
                .DragOver(&cursor_position, *pdweffect)
                .log_err();
            ScreenToClient(self.0.hwnd, &mut cursor_position)
                .ok()
                .log_err();
        }
        let scale_factor = self.0.state.scale_factor.get();
        let input = PlatformInput::FileDrop(FileDropEvent::Pending {
            position: logical_point(
                cursor_position.x as f32,
                cursor_position.y as f32,
                scale_factor,
            ),
        });
        self.handle_drag_drop(input);

        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        unsafe {
            self.0.drop_target_helper.DragLeave().log_err();
        }
        let input = PlatformInput::FileDrop(FileDropEvent::Exited);
        self.handle_drag_drop(input);

        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: windows::core::Ref<IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let idata_obj = pdataobj.ok()?;
        let mut cursor_position = POINT { x: pt.x, y: pt.y };
        unsafe {
            *pdweffect = DROPEFFECT_COPY;
            self.0
                .drop_target_helper
                .Drop(idata_obj, &cursor_position, *pdweffect)
                .log_err();
            ScreenToClient(self.0.hwnd, &mut cursor_position)
                .ok()
                .log_err();
        }
        let scale_factor = self.0.state.scale_factor.get();
        let input = PlatformInput::FileDrop(FileDropEvent::Submit {
            position: logical_point(
                cursor_position.x as f32,
                cursor_position.y as f32,
                scale_factor,
            ),
        });
        self.handle_drag_drop(input);

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ClickState {
    button: Cell<MouseButton>,
    last_click: Cell<Instant>,
    last_position: Cell<Point<DevicePixels>>,
    double_click_spatial_tolerance_width: Cell<i32>,
    double_click_spatial_tolerance_height: Cell<i32>,
    double_click_interval: Cell<Duration>,
    pub(crate) current_count: Cell<usize>,
}

impl ClickState {
    pub fn new() -> Self {
        let double_click_spatial_tolerance_width = unsafe { GetSystemMetrics(SM_CXDOUBLECLK) };
        let double_click_spatial_tolerance_height = unsafe { GetSystemMetrics(SM_CYDOUBLECLK) };
        let double_click_interval = Duration::from_millis(unsafe { GetDoubleClickTime() } as u64);

        ClickState {
            button: Cell::new(MouseButton::Left),
            last_click: Cell::new(Instant::now()),
            last_position: Cell::new(Point::default()),
            double_click_spatial_tolerance_width: Cell::new(double_click_spatial_tolerance_width),
            double_click_spatial_tolerance_height: Cell::new(double_click_spatial_tolerance_height),
            double_click_interval: Cell::new(double_click_interval),
            current_count: Cell::new(0),
        }
    }

    /// update self and return the needed click count
    pub fn update(&self, button: MouseButton, new_position: Point<DevicePixels>) -> usize {
        if self.button.get() == button && self.is_double_click(new_position) {
            self.current_count.update(|it| it + 1);
        } else {
            self.current_count.set(1);
        }
        self.last_click.set(Instant::now());
        self.last_position.set(new_position);
        self.button.set(button);

        self.current_count.get()
    }

    pub fn system_update(&self, wparam: usize) {
        match wparam {
            // SPI_SETDOUBLECLKWIDTH
            29 => self
                .double_click_spatial_tolerance_width
                .set(unsafe { GetSystemMetrics(SM_CXDOUBLECLK) }),
            // SPI_SETDOUBLECLKHEIGHT
            30 => self
                .double_click_spatial_tolerance_height
                .set(unsafe { GetSystemMetrics(SM_CYDOUBLECLK) }),
            // SPI_SETDOUBLECLICKTIME
            32 => self
                .double_click_interval
                .set(Duration::from_millis(unsafe { GetDoubleClickTime() } as u64)),
            _ => {}
        }
    }

    #[inline]
    fn is_double_click(&self, new_position: Point<DevicePixels>) -> bool {
        let diff = self.last_position.get() - new_position;

        self.last_click.get().elapsed() < self.double_click_interval.get()
            && diff.x.0.abs() <= self.double_click_spatial_tolerance_width.get()
            && diff.y.0.abs() <= self.double_click_spatial_tolerance_height.get()
    }
}

#[derive(Copy, Clone)]
struct StyleAndBounds {
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    cx: i32,
    cy: i32,
}

#[repr(C)]
struct WINDOWCOMPOSITIONATTRIBDATA {
    attrib: u32,
    pv_data: *mut std::ffi::c_void,
    cb_data: usize,
}

#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32,
    animation_id: u32,
}

type Color = (u8, u8, u8, u8);

#[derive(Debug, Default, Clone)]
pub(crate) struct WindowBorderOffset {
    pub(crate) width_offset: Cell<i32>,
    pub(crate) height_offset: Cell<i32>,
}

impl WindowBorderOffset {
    pub(crate) fn update(&self, hwnd: HWND) -> anyhow::Result<()> {
        let window_rect = unsafe {
            let mut rect = std::mem::zeroed();
            GetWindowRect(hwnd, &mut rect)?;
            rect
        };
        let client_rect = unsafe {
            let mut rect = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect)?;
            rect
        };
        self.width_offset
            .set((window_rect.right - window_rect.left) - (client_rect.right - client_rect.left));
        self.height_offset
            .set((window_rect.bottom - window_rect.top) - (client_rect.bottom - client_rect.top));
        Ok(())
    }
}

#[derive(Clone)]
struct WindowOpenStatus {
    placement: WINDOWPLACEMENT,
    state: WindowOpenState,
}

#[derive(Clone, Copy)]
enum WindowOpenState {
    Maximized,
    Fullscreen,
    Windowed,
}

pub(crate) const WINDOW_CLASS_NAME: PCWSTR = w!("Zed::Window");

fn register_window_class(icon_handle: HICON) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_procedure),
            hIcon: icon_handle,
            lpszClassName: PCWSTR(WINDOW_CLASS_NAME.as_ptr()),
            style: CS_HREDRAW | CS_VREDRAW,
            hInstance: get_module_handle().into(),
            hbrBackground: unsafe { CreateSolidBrush(COLORREF(0x00000000)) },
            ..Default::default()
        };
        unsafe { RegisterClassW(&wc) };
    });
}

unsafe extern "system" fn window_procedure(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let window_params = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        let window_creation_context = window_params.lpCreateParams as *mut WindowCreateContext;
        let window_creation_context = unsafe { &mut *window_creation_context };
        return match WindowsWindowInner::new(window_creation_context, hwnd, window_params) {
            Ok(window_state) => {
                let weak = Box::new(Rc::downgrade(&window_state));
                unsafe { set_window_long(hwnd, GWLP_USERDATA, Box::into_raw(weak) as isize) };
                window_creation_context.inner = Some(Ok(window_state));
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
            Err(error) => {
                window_creation_context.inner = Some(Err(error));
                LRESULT(0)
            }
        };
    }

    let ptr = unsafe { get_window_long(hwnd, GWLP_USERDATA) } as *mut Weak<WindowsWindowInner>;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let inner = unsafe { &*ptr };
    let result = if let Some(inner) = inner.upgrade() {
        if msg == WM_NCDESTROY {
            inner.dialog_owner.close();
        }
        inner.handle_msg(hwnd, msg, wparam, lparam)
    } else {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    };

    if msg == WM_NCDESTROY {
        unsafe { set_window_long(hwnd, GWLP_USERDATA, 0) };
        unsafe { drop(Box::from_raw(ptr)) };
    }

    result
}

pub(crate) fn window_from_hwnd(hwnd: HWND) -> Option<Rc<WindowsWindowInner>> {
    if hwnd.is_invalid() {
        return None;
    }

    let ptr = unsafe { get_window_long(hwnd, GWLP_USERDATA) } as *mut Weak<WindowsWindowInner>;
    if !ptr.is_null() {
        let inner = unsafe { &*ptr };
        inner.upgrade()
    } else {
        None
    }
}

fn get_module_handle() -> HMODULE {
    unsafe {
        let mut h_module = std::mem::zeroed();
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            windows::core::w!("ZedModule"),
            &mut h_module,
        )
        .expect("Unable to get module handle"); // this should never fail

        h_module
    }
}

fn register_drag_drop(window: &Rc<WindowsWindowInner>) -> Result<()> {
    let window_handle = window.hwnd;
    let handler = WindowsDragDropHandler(window.clone());
    // The lifetime of `IDropTarget` is handled by Windows, it won't release until
    // we call `RevokeDragDrop`.
    // So, it's safe to drop it here.
    let drag_drop_handler: IDropTarget = handler.into();
    unsafe {
        RegisterDragDrop(window_handle, &drag_drop_handler)
            .context("unable to register drag-drop event")?;
    }
    window.state.drag_drop_registered.set(true);
    Ok(())
}

fn calculate_window_rect(bounds: Bounds<DevicePixels>, border_offset: &WindowBorderOffset) -> RECT {
    // NOTE:
    // The reason we're not using `AdjustWindowRectEx()` here is
    // that the size reported by this function is incorrect.
    // You can test it, and there are similar discussions online.
    // See: https://stackoverflow.com/questions/12423584/how-to-set-exact-client-size-for-overlapped-window-winapi
    //
    // So we manually calculate these values here.
    let mut rect = RECT {
        left: bounds.left().0,
        top: bounds.top().0,
        right: bounds.right().0,
        bottom: bounds.bottom().0,
    };
    let left_offset = border_offset.width_offset.get() / 2;
    let top_offset = border_offset.height_offset.get() / 2;
    let right_offset = border_offset.width_offset.get() - left_offset;
    let bottom_offset = border_offset.height_offset.get() - top_offset;
    rect.left -= left_offset;
    rect.top -= top_offset;
    rect.right += right_offset;
    rect.bottom += bottom_offset;
    rect
}

fn calculate_client_rect(
    rect: RECT,
    border_offset: &WindowBorderOffset,
    scale_factor: f32,
) -> Bounds<Pixels> {
    let left_offset = border_offset.width_offset.get() / 2;
    let top_offset = border_offset.height_offset.get() / 2;
    let right_offset = border_offset.width_offset.get() - left_offset;
    let bottom_offset = border_offset.height_offset.get() - top_offset;
    let left = rect.left + left_offset;
    let top = rect.top + top_offset;
    let right = rect.right - right_offset;
    let bottom = rect.bottom - bottom_offset;
    let physical_size = size(DevicePixels(right - left), DevicePixels(bottom - top));
    Bounds {
        origin: logical_point(left as f32, top as f32, scale_factor),
        size: physical_size.to_pixels(scale_factor),
    }
}

fn retrieve_window_placement(
    hwnd: HWND,
    display: WindowsDisplay,
    initial_bounds: Bounds<Pixels>,
    border_offset: &WindowBorderOffset,
) -> Result<WINDOWPLACEMENT> {
    let mut placement = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    unsafe { GetWindowPlacement(hwnd, &mut placement)? };
    // the bounds may be not inside the display
    let bounds = if display.check_given_bounds(initial_bounds) {
        initial_bounds
    } else {
        display.default_bounds()
    };
    // `bounds` is expressed in logical pixels for `display`, so it must be converted
    // to device pixels using that display's own scale factor. The window's current
    // scale factor can't be used here: `CreateWindowExW` was called with
    // `CW_USEDEFAULT`, so at this point the window may still be sitting on whichever
    // monitor Windows picked by default, which can have a different DPI than `display`
    // and would otherwise throw off the physical position (e.g. leaving the window
    // partially off-screen when moved to a monitor with a different scale factor).
    let bounds = bounds.to_device_pixels(display.scale_factor());
    placement.rcNormalPosition = calculate_window_rect(bounds, border_offset);
    Ok(placement)
}

fn dwm_set_window_composition_attribute(hwnd: HWND, backdrop_type: u32) {
    let mut version = unsafe { std::mem::zeroed() };
    let status = unsafe { windows::Wdk::System::SystemServices::RtlGetVersion(&mut version) };

    // DWMWA_SYSTEMBACKDROP_TYPE is available only on version 22621 or later
    // using SetWindowCompositionAttributeType as a fallback
    if !status.is_ok() || version.dwBuildNumber < 22621 {
        return;
    }

    unsafe {
        let result = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &backdrop_type as *const _ as *const _,
            std::mem::size_of_val(&backdrop_type) as u32,
        );

        if !result.is_ok() {
            return;
        }
    }
}

fn set_window_composition_attribute(hwnd: HWND, color: Option<Color>, state: u32) {
    let mut version = unsafe { std::mem::zeroed() };
    let status = unsafe { windows::Wdk::System::SystemServices::RtlGetVersion(&mut version) };

    if !status.is_ok() || version.dwBuildNumber < 17763 {
        return;
    }

    unsafe {
        type SetWindowCompositionAttributeType =
            unsafe extern "system" fn(HWND, *mut WINDOWCOMPOSITIONATTRIBDATA) -> BOOL;
        let module_name = PCSTR::from_raw(c"user32.dll".as_ptr() as *const u8);
        if let Some(user32) = GetModuleHandleA(module_name)
            .context("Unable to get user32.dll handle")
            .log_err()
        {
            let func_name = PCSTR::from_raw(c"SetWindowCompositionAttribute".as_ptr() as *const u8);
            let set_window_composition_attribute: SetWindowCompositionAttributeType =
                std::mem::transmute(GetProcAddress(user32, func_name));
            let mut color = color.unwrap_or_default();
            let is_acrylic = state == 4;
            if is_acrylic && color.3 == 0 {
                color.3 = 1;
            }
            let accent = AccentPolicy {
                accent_state: state,
                accent_flags: if is_acrylic { 0 } else { 2 },
                gradient_color: (color.0 as u32)
                    | ((color.1 as u32) << 8)
                    | ((color.2 as u32) << 16)
                    | ((color.3 as u32) << 24),
                animation_id: 0,
            };
            let mut data = WINDOWCOMPOSITIONATTRIBDATA {
                attrib: 0x13,
                pv_data: &accent as *const _ as *mut _,
                cb_data: std::mem::size_of::<AccentPolicy>(),
            };
            let _ = set_window_composition_attribute(hwnd, &mut data as *mut _ as _);
        }
    }
}

// When the platform title bar is hidden, Windows may think that our application is meant to appear 'fullscreen'
// and will stop the taskbar from appearing on top of our window. Prevent this.
// https://devblogs.microsoft.com/oldnewthing/20250522-00/?p=111211
fn set_non_rude_hwnd(hwnd: HWND, non_rude: bool) {
    if non_rude {
        unsafe { SetPropW(hwnd, w!("NonRudeHWND"), Some(HANDLE(1 as _))) }.log_err();
    } else {
        unsafe { RemovePropW(hwnd, w!("NonRudeHWND")) }.log_err();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ClickState, WindowsSurfaceResize, WindowsWgpuDeviceRecoveryAction,
        windows_effect_capabilities_from_renderer, windows_surface_resize_for_client_size,
        windows_wgpu_device_recovery_action, windows_wgpu_frame_request_options,
        windows_wgpu_should_post_retry_frame, windows_wgpu_surface_config,
        windows_wgpu_transparency_for_background_appearance,
    };
    use gpux::{
        DevicePixels, EffectCapabilities, MouseButton, WindowBackgroundAppearance, point, size,
    };
    use std::time::Duration;

    #[test]
    fn windows_wgpu_surface_config_uses_vsynced_present_mode() {
        let config = windows_wgpu_surface_config(size(DevicePixels(800), DevicePixels(600)));

        assert_eq!(config.size, size(DevicePixels(800), DevicePixels(600)));
        assert!(!config.transparent);
        assert_eq!(config.preferred_present_mode, None);
    }

    #[test]
    fn missing_windows_renderer_reports_unsupported_effect_capabilities() {
        assert_eq!(
            windows_effect_capabilities_from_renderer(None),
            EffectCapabilities::default()
        );
    }

    #[test]
    fn windows_surface_resize_suspends_for_minimized_or_zero_size() {
        assert_eq!(
            windows_surface_resize_for_client_size(800, 600, false),
            WindowsSurfaceResize::Resize(size(DevicePixels(800), DevicePixels(600)))
        );
        assert_eq!(
            windows_surface_resize_for_client_size(800, 600, true),
            WindowsSurfaceResize::Suspend
        );
        assert_eq!(
            windows_surface_resize_for_client_size(0, 600, false),
            WindowsSurfaceResize::Suspend
        );
        assert_eq!(
            windows_surface_resize_for_client_size(800, 0, false),
            WindowsSurfaceResize::Suspend
        );
    }

    #[test]
    fn windows_wgpu_transparency_tracks_background_appearance() {
        assert!(!windows_wgpu_transparency_for_background_appearance(
            WindowBackgroundAppearance::Opaque
        ));
        assert!(windows_wgpu_transparency_for_background_appearance(
            WindowBackgroundAppearance::Transparent
        ));
        assert!(windows_wgpu_transparency_for_background_appearance(
            WindowBackgroundAppearance::Blurred
        ));
        assert!(windows_wgpu_transparency_for_background_appearance(
            WindowBackgroundAppearance::MicaBackdrop
        ));
        assert!(windows_wgpu_transparency_for_background_appearance(
            WindowBackgroundAppearance::MicaAltBackdrop
        ));
    }

    #[test]
    fn windows_wgpu_device_recovery_action_follows_renderer_device_loss() {
        assert_eq!(
            windows_wgpu_device_recovery_action(false),
            WindowsWgpuDeviceRecoveryAction::Continue
        );
        assert_eq!(
            windows_wgpu_device_recovery_action(true),
            WindowsWgpuDeviceRecoveryAction::RecoverRenderer
        );
    }

    #[test]
    fn windows_wgpu_frame_request_forces_resize_and_recovery_frames() {
        let idle = windows_wgpu_frame_request_options(false, false, false, false);
        assert!(!idle.require_presentation);
        assert!(!idle.force_render);

        let resize = windows_wgpu_frame_request_options(false, false, true, false);
        assert!(resize.require_presentation);
        assert!(resize.force_render);

        let recovery = windows_wgpu_frame_request_options(false, false, false, true);
        assert!(recovery.require_presentation);
        assert!(recovery.force_render);

        let retry = windows_wgpu_frame_request_options(false, true, false, false);
        assert!(retry.require_presentation);
        assert!(!retry.force_render);

        let explicit = windows_wgpu_frame_request_options(true, false, false, false);
        assert!(!explicit.require_presentation);
        assert!(explicit.force_render);
    }

    #[test]
    fn windows_wgpu_retry_frame_posts_once_until_handled() {
        assert!(windows_wgpu_should_post_retry_frame(true, false, false));
        assert!(!windows_wgpu_should_post_retry_frame(false, false, false));
        assert!(!windows_wgpu_should_post_retry_frame(true, true, false));
        assert!(!windows_wgpu_should_post_retry_frame(true, false, true));
    }

    #[test]
    fn test_double_click_interval() {
        let state = ClickState::new();
        assert_eq!(
            state.update(MouseButton::Left, point(DevicePixels(0), DevicePixels(0))),
            1
        );
        assert_eq!(
            state.update(MouseButton::Right, point(DevicePixels(0), DevicePixels(0))),
            1
        );
        assert_eq!(
            state.update(MouseButton::Left, point(DevicePixels(0), DevicePixels(0))),
            1
        );
        assert_eq!(
            state.update(MouseButton::Left, point(DevicePixels(0), DevicePixels(0))),
            2
        );
        state
            .last_click
            .update(|it| it - Duration::from_millis(700));
        assert_eq!(
            state.update(MouseButton::Left, point(DevicePixels(0), DevicePixels(0))),
            1
        );
    }

    #[test]
    fn test_double_click_spatial_tolerance() {
        let state = ClickState::new();
        assert_eq!(
            state.update(MouseButton::Left, point(DevicePixels(-3), DevicePixels(0))),
            1
        );
        assert_eq!(
            state.update(MouseButton::Left, point(DevicePixels(0), DevicePixels(3))),
            2
        );
        assert_eq!(
            state.update(MouseButton::Right, point(DevicePixels(3), DevicePixels(2))),
            1
        );
        assert_eq!(
            state.update(MouseButton::Right, point(DevicePixels(10), DevicePixels(0))),
            1
        );
    }
}
