//! Lane D's window-composition checks on Windows (`composition-windows-*`):
//! a native surface placed through `native_surface` between GPUI's base
//! content and a GPUI menu deferred onto the overlay. The checks draw the
//! native surface's content themselves, solid red through a wgpu device of
//! their own on the surface's DirectComposition visual, and read what DWM
//! composed through a GDI screenshot of the window. On other platforms they
//! report skipped.

use crate::checks::Check;
use gpux::AsyncApp;

#[cfg(not(target_os = "windows"))]
mod unsupported {
    use super::*;

    fn skipped() -> anyhow::Result<Check> {
        Ok(Check::Skipped(
            "Windows window composition checks run on Windows".into(),
        ))
    }

    pub async fn order(_: &mut AsyncApp) -> anyhow::Result<Check> {
        skipped()
    }
    pub async fn clip(_: &mut AsyncApp) -> anyhow::Result<Check> {
        skipped()
    }
    pub async fn device_loss(_: &mut AsyncApp) -> anyhow::Result<Check> {
        skipped()
    }
    pub async fn accelerator(_: &mut AsyncApp) -> anyhow::Result<Check> {
        skipped()
    }
    pub async fn resize(_: &mut AsyncApp) -> anyhow::Result<Check> {
        skipped()
    }
}

#[cfg(not(target_os = "windows"))]
pub use unsupported::*;

#[cfg(target_os = "windows")]
pub use windows_checks::*;

#[cfg(target_os = "windows")]
mod windows_checks {
    use super::*;
    use crate::checks::{close_window, open};
    use anyhow::{Context as _, bail};
    use gpui_wgpu::wgpu;
    use gpux::{
        AnyWindowHandle, Context, IntoElement, Render, Window, WindowCompositionSurface, anchored,
        deferred, div, native_surface, point, prelude::*, px, rgb,
    };
    use gpux_tests_conformance::app_host::wait_frames;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{cell::RefCell, rc::Rc};
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
            Graphics::{
                DirectComposition::{
                    DCompositionCreateDevice2, IDCompositionDesktopDevice, IDCompositionTarget,
                    IDCompositionVisual2,
                },
                Dwm::DwmFlush,
                Gdi::{
                    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, ClientToScreen,
                    CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
                    DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
                },
            },
            System::LibraryLoader::GetModuleHandleW,
            UI::WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowRect,
                GetWindowTextW, HWND_TOP, PostMessageW, RegisterClassW, SW_SHOWNOACTIVATE,
                SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SetWindowPos, ShowWindow,
                WINDOW_EX_STYLE, WM_KEYDOWN, WM_USER, WNDCLASSW, WS_CHILD, WS_OVERLAPPEDWINDOW,
                WS_VISIBLE,
            },
        },
        core::{IUnknown, Interface as _, w},
    };

    /// Where the native surface is placed, in logical pixels.
    const NATIVE: (f32, f32, f32, f32) = (40., 50., 160., 100.);
    /// Where the menu opens over it.
    const MENU: (f32, f32, f32, f32) = (110., 80., 80., 40.);
    /// The row of the red line the resize check follows, in logical pixels.
    const LINE_Y: f32 = 30.;
    /// gpux's DirectX device-lost message (`WM_GPUI_GPU_DEVICE_LOST`).
    const WM_GPUI_GPU_DEVICE_LOST: u32 = WM_USER + 7;

    const RED: [u8; 3] = [255, 0, 0];
    const WHITE: [u8; 3] = [255, 255, 255];
    const MENU_GREY: [u8; 3] = [0xdd, 0xdd, 0xdd];

    struct Composed {
        surface: Option<WindowCompositionSurface>,
        menu_open: bool,
        line: bool,
        tick: usize,
    }

    impl Render for Composed {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .relative()
                .bg(rgb(0xffffff))
                .when(self.line, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .top(px(LINE_Y))
                            .w_full()
                            .h(px(4.))
                            .bg(rgb(0xff0000)),
                    )
                })
                .children(self.surface.clone().map(|surface| {
                    native_surface(surface)
                        .absolute()
                        .left(px(NATIVE.0))
                        .top(px(NATIVE.1))
                        .w(px(NATIVE.2))
                        .h(px(NATIVE.3))
                }))
                .when(self.menu_open, |this| {
                    this.child(deferred(
                        anchored().position(point(px(MENU.0), px(MENU.1))).child(
                            div()
                                .id("menu")
                                .w(px(MENU.2))
                                .h(px(MENU.3))
                                .bg(rgb(0xdddddd))
                                .shadow_lg(),
                        ),
                    ))
                })
                .child(format!("{}", self.tick % 2))
        }
    }

    /// Solid colour presented on a DirectComposition visual through a wgpu
    /// device of the check's own; kept alive while the content must show.
    struct HostContent {
        _instance: wgpu::Instance,
        _device: wgpu::Device,
        _surface: wgpu::Surface<'static>,
    }

    impl HostContent {
        /// Presents `size` device pixels of red on `visual`, or of white with
        /// a red line at row `line` when given.
        fn new(visual: &IUnknown, size: (u32, u32), line: Option<u32>) -> anyhow::Result<Self> {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::DX12,
                flags: wgpu::InstanceFlags::default(),
                backend_options: wgpu::BackendOptions::default(),
                memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
                display: None,
            });
            // SAFETY: the visual outlives the surface: the check keeps it.
            let surface = unsafe {
                instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CompositionVisual(
                    visual.as_raw(),
                ))
            }?;
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::default(),
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                    apply_limit_buckets: false,
                }))?;
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    label: Some("composition check"),
                    required_features: wgpu::Features::empty(),
                    required_limits:
                        wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                    memory_hints: wgpu::MemoryHints::default(),
                    trace: wgpu::Trace::Off,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                }))?;
            let caps = surface.get_capabilities(&adapter);
            let format = caps.formats.first().copied().context("no surface format")?;
            // The line is copied into the frame, which then needs COPY_DST.
            let usage = if line.is_some() {
                anyhow::ensure!(
                    caps.usages.contains(wgpu::TextureUsages::COPY_DST),
                    "the surface takes no copies"
                );
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST
            } else {
                wgpu::TextureUsages::RENDER_ATTACHMENT
            };
            surface.configure(
                &device,
                &wgpu::SurfaceConfiguration {
                    usage,
                    format,
                    width: size.0.max(1),
                    height: size.1.max(1),
                    present_mode: wgpu::PresentMode::Fifo,
                    desired_maximum_frame_latency: 2,
                    alpha_mode: caps.alpha_modes.first().copied().unwrap_or_default(),
                    view_formats: Vec::new(),
                    color_space: wgpu::SurfaceColorSpace::Auto,
                },
            );
            let frame = match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                _ => bail!("acquiring the native surface's texture failed"),
            };
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let clear = |encoder: &mut wgpu::CommandEncoder, color| {
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(color),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    ..Default::default()
                });
            };
            match line {
                None => clear(&mut encoder, wgpu::Color::RED),
                Some(line) => {
                    clear(&mut encoder, wgpu::Color::WHITE);
                    // A 4-pixel red row at `line`, copied from a texture cleared red.
                    let red = device.create_texture(&wgpu::TextureDescriptor {
                        label: None,
                        size: wgpu::Extent3d {
                            width: size.0.max(1),
                            height: 4,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::COPY_SRC,
                        view_formats: &[],
                    });
                    let red_view = red.create_view(&wgpu::TextureViewDescriptor::default());
                    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &red_view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                                store: wgpu::StoreOp::Store,
                            },
                            depth_slice: None,
                        })],
                        depth_stencil_attachment: None,
                        ..Default::default()
                    });
                    encoder.copy_texture_to_texture(
                        red.as_image_copy(),
                        wgpu::TexelCopyTextureInfo {
                            texture: &frame.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: 0,
                                y: line,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::Extent3d {
                            width: size.0.max(1),
                            height: 4,
                            depth_or_array_layers: 1,
                        },
                    );
                }
            }
            queue.submit(std::iter::once(encoder.finish()));
            queue.present(frame);
            Ok(Self {
                _instance: instance,
                _device: device,
                _surface: surface,
            })
        }
    }

    fn hwnd(window: &Window) -> anyhow::Result<HWND> {
        match HasWindowHandle::window_handle(window)
            .map_err(|error| anyhow::anyhow!("the window has no handle: {error}"))?
            .as_raw()
        {
            RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut _)),
            _ => bail!("not a Win32 window"),
        }
    }

    fn visual(surface: &WindowCompositionSurface) -> anyhow::Result<IUnknown> {
        surface
            .platform_surface()?
            .platform_handle()?
            .downcast::<IUnknown>()
            .map(|unknown| *unknown)
            .map_err(|_| anyhow::anyhow!("the native surface is not a DirectComposition visual"))
    }

    /// The window's client area as DWM composed it, in RGB rows.
    struct Screenshot {
        width: i32,
        height: i32,
        pixels: Vec<u8>,
    }

    impl Screenshot {
        fn of(hwnd: HWND) -> anyhow::Result<Self> {
            unsafe {
                DwmFlush().ok();
                let mut client = RECT::default();
                GetClientRect(hwnd, &mut client)?;
                let mut origin = POINT::default();
                ClientToScreen(hwnd, &mut origin).ok()?;
                let (width, height) = (client.right - client.left, client.bottom - client.top);
                let screen = GetDC(None);
                let memory = CreateCompatibleDC(Some(screen));
                let bitmap = CreateCompatibleBitmap(screen, width, height);
                let previous = SelectObject(memory, bitmap.into());
                let copied = BitBlt(
                    memory,
                    0,
                    0,
                    width,
                    height,
                    Some(screen),
                    origin.x,
                    origin.y,
                    SRCCOPY | CAPTUREBLT,
                );
                let mut info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: width,
                        biHeight: -height,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bgra = vec![0u8; (width * height * 4) as usize];
                let rows = GetDIBits(
                    memory,
                    bitmap,
                    0,
                    height as u32,
                    Some(bgra.as_mut_ptr().cast()),
                    &mut info,
                    DIB_RGB_COLORS,
                );
                SelectObject(memory, previous);
                let _ = DeleteObject(bitmap.into());
                let _ = DeleteDC(memory);
                ReleaseDC(None, screen);
                copied?;
                if rows == 0 {
                    bail!("GetDIBits read no rows");
                }
                let pixels = bgra
                    .chunks_exact(4)
                    .flat_map(|pixel| [pixel[2], pixel[1], pixel[0]])
                    .collect();
                Ok(Self {
                    width,
                    height,
                    pixels,
                })
            }
        }

        /// The pixel at `(x, y)` logical pixels.
        fn at(&self, (x, y): (f32, f32), scale: f32) -> anyhow::Result<[u8; 3]> {
            let (x, y) = ((x * scale).round() as i32, (y * scale).round() as i32);
            if x < 0 || y < 0 || x >= self.width || y >= self.height {
                bail!(
                    "({x}, {y}) is outside the {}x{} screenshot",
                    self.width,
                    self.height
                );
            }
            let index = ((y * self.width + x) * 3) as usize;
            Ok([
                self.pixels[index],
                self.pixels[index + 1],
                self.pixels[index + 2],
            ])
        }

        /// The device rows, top to bottom, where column `x` is red.
        fn red_rows(&self, x: i32) -> Vec<i32> {
            (0..self.height)
                .filter(|y| {
                    let index = ((y * self.width + x) * 3) as usize;
                    close(
                        [
                            self.pixels[index],
                            self.pixels[index + 1],
                            self.pixels[index + 2],
                        ],
                        RED,
                    )
                })
                .collect()
        }
    }

    fn close(actual: [u8; 3], expected: [u8; 3]) -> bool {
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.abs_diff(expected) <= 24)
    }

    fn expect(
        screenshot: &Screenshot,
        at: (f32, f32),
        scale: f32,
        color: [u8; 3],
        what: &str,
    ) -> anyhow::Result<()> {
        let actual = screenshot.at(at, scale)?;
        if !close(actual, color) {
            bail!("{what} at {at:?} reads {actual:?}, expected {color:?}");
        }
        Ok(())
    }

    /// A composed window whose native surface shows red over the whole
    /// window's size from its origin, so its clip shows at its bounds.
    struct Setup {
        window: AnyWindowHandle,
        hwnd: HWND,
        surface: WindowCompositionSurface,
        content: Rc<RefCell<Option<HostContent>>>,
        scale: f32,
    }

    async fn setup(cx: &mut AsyncApp, title: &str, line: bool) -> anyhow::Result<Setup> {
        let handle = open(cx, title, (320., 220.), move |_, _| Composed {
            surface: None,
            menu_open: false,
            line,
            tick: 0,
        })?;
        let setup = handle.update(cx, |root, window, cx| {
            let hwnd = hwnd(window)?;
            crate::os::bring_to_front(window).map_err(anyhow::Error::msg)?;
            let composition = window.enable_window_composition()?;
            let surface = composition.create_native_surface()?;
            let scale = window.scale_factor();
            let size = window.viewport_size();
            let device = (
                (f32::from(size.width) * scale).round() as u32,
                (f32::from(size.height) * scale).round() as u32,
            );
            let content = Rc::new(RefCell::new(Some(HostContent::new(
                &visual(&surface)?,
                device,
                None,
            )?)));
            let root = root
                .downcast::<Composed>()
                .map_err(|_| anyhow::anyhow!("not the composed view"))?;
            let placed = surface.clone();
            root.update(cx, |composed, cx| {
                composed.surface = Some(placed);
                cx.notify();
            });
            anyhow::Ok(Setup {
                window: handle,
                hwnd,
                surface,
                content,
                scale,
            })
        })??;
        wait_frames(cx, handle, 3).await?;
        Ok(setup)
    }

    fn update(cx: &mut AsyncApp, window: AnyWindowHandle, change: impl FnOnce(&mut Composed)) {
        window
            .update(cx, |root, _, cx| {
                if let Ok(root) = root.downcast::<Composed>() {
                    root.update(cx, |composed, cx| {
                        change(composed);
                        composed.tick += 1;
                        cx.notify();
                    });
                }
            })
            .ok();
    }

    /// The native surface shows above the base and below the overlay: red
    /// over its area, the menu's grey where the menu covers it, white base
    /// content around it.
    pub async fn order(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let setup = setup(cx, "composition-windows-order", false).await?;
        let result = async {
            update(cx, setup.window, |composed| composed.menu_open = true);
            wait_frames(cx, setup.window, 3).await?;
            let shot = Screenshot::of(setup.hwnd)?;
            let scale = setup.scale;
            expect(
                &shot,
                (NATIVE.0 + 10., NATIVE.1 + 10.),
                scale,
                RED,
                "the native surface",
            )?;
            expect(
                &shot,
                (MENU.0 + 20., MENU.1 + 20.),
                scale,
                MENU_GREY,
                "the menu over it",
            )?;
            expect(&shot, (10., 10.), scale, WHITE, "the base content")?;
            Ok(Check::Passed(
                "base, then the native surface, then the menu".into(),
            ))
        }
        .await;
        close_window(cx, setup.window);
        drop(setup.content);
        result
    }

    /// The native surface's content, presented larger than its bounds, is
    /// clipped to them.
    pub async fn clip(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let setup = setup(cx, "composition-windows-clip", false).await?;
        let result = (|| {
            let shot = Screenshot::of(setup.hwnd)?;
            let scale = setup.scale;
            let (right, bottom) = (NATIVE.0 + NATIVE.2, NATIVE.1 + NATIVE.3);
            expect(
                &shot,
                (right - 3., NATIVE.1 + 10.),
                scale,
                RED,
                "inside the right edge",
            )?;
            expect(
                &shot,
                (right + 3., NATIVE.1 + 10.),
                scale,
                WHITE,
                "outside the right edge",
            )?;
            expect(
                &shot,
                (NATIVE.0 + 10., bottom - 3.),
                scale,
                RED,
                "inside the bottom edge",
            )?;
            expect(
                &shot,
                (NATIVE.0 + 10., bottom + 3.),
                scale,
                WHITE,
                "outside the bottom edge",
            )?;
            Ok(Check::Passed(format!(
                "clipped to {NATIVE:?} at scale {scale}"
            )))
        })();
        close_window(cx, setup.window);
        drop(setup.content);
        result
    }

    /// The device-lost message rebuilds the tree on a new device: the native
    /// surface's owner is told and given a new visual, and its content shows
    /// again on it.
    pub async fn device_loss(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let setup = setup(cx, "composition-windows-device-loss", false).await?;
        let result = async {
            let recreated = Rc::new(RefCell::new(0usize));
            let size = {
                let shot = Screenshot::of(setup.hwnd)?;
                (shot.width as u32, shot.height as u32)
            };
            {
                let (recreated, content) = (recreated.clone(), setup.content.clone());
                setup
                    .surface
                    .platform_surface()?
                    .set_compositor_recreated_callback(Rc::new(move |handle| {
                        let visual = handle
                            .downcast::<IUnknown>()
                            .map_err(|_| anyhow::anyhow!("the new handle is not a visual"))?;
                        *content.borrow_mut() = Some(HostContent::new(&visual, size, None)?);
                        *recreated.borrow_mut() += 1;
                        Ok(())
                    }))?;
            }
            unsafe {
                PostMessageW(
                    Some(setup.hwnd),
                    WM_GPUI_GPU_DEVICE_LOST,
                    WPARAM(0),
                    LPARAM(0),
                )?;
            }
            update(cx, setup.window, |_| {});
            wait_frames(cx, setup.window, 5).await?;
            let count = *recreated.borrow();
            if count != 1 {
                bail!("the native surface's owner was told {count} times, expected once");
            }
            let shot = Screenshot::of(setup.hwnd)?;
            expect(
                &shot,
                (NATIVE.0 + 10., NATIVE.1 + 10.),
                setup.scale,
                RED,
                "the rebuilt native surface",
            )?;
            Ok(Check::Passed(
                "tree rebuilt, owner told once, content shown again".into(),
            ))
        }
        .await;
        close_window(cx, setup.window);
        drop(setup.content);
        result
    }

    /// A key down for a child window that is not GPUI's reaches its own
    /// window procedure, translated: an edit control types the letter.
    pub async fn accelerator(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let setup = setup(cx, "composition-windows-accelerator", false).await?;
        let result = async {
            let edit = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("EDIT"),
                    w!(""),
                    WS_CHILD | WS_VISIBLE,
                    220,
                    10,
                    80,
                    24,
                    Some(setup.hwnd),
                    None,
                    Some(GetModuleHandleW(None)?.into()),
                    None,
                )
            }?;
            let typed = async {
                // VK 'A', scan code 0x1E, repeat count 1.
                unsafe {
                    PostMessageW(
                        Some(edit),
                        WM_KEYDOWN,
                        WPARAM(0x41),
                        LPARAM(1 | (0x1E << 16)),
                    )?;
                }
                update(cx, setup.window, |_| {});
                wait_frames(cx, setup.window, 3).await?;
                let mut text = [0u16; 16];
                let length = unsafe { GetWindowTextW(edit, &mut text) };
                anyhow::Ok(String::from_utf16_lossy(&text[..length.max(0) as usize]))
            }
            .await;
            unsafe { DestroyWindow(edit) }.ok();
            let text = typed?;
            if text != "a" {
                bail!(
                    "the edit control reads {text:?} after a posted 'A' key down, expected \"a\""
                );
            }
            Ok(Check::Passed("the edit control typed \"a\"".into()))
        }
        .await;
        close_window(cx, setup.window);
        drop(setup.content);
        result
    }

    /// Fast height-only live resize with composition on: the base content
    /// stays where it was drawn (`DXGI_SCALING_NONE`, not stretched) until
    /// the next frame. For comparison, the same resize of a window whose
    /// content is a composition-visual swap chain (the alternative design,
    /// option (b)) is measured and reported.
    pub async fn resize(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let setup = setup(cx, "composition-windows-resize", true).await?;
        let result = async {
            let scale = setup.scale;
            let column = ((NATIVE.0 + NATIVE.2 + 20.) * scale).round() as i32;
            let before_shot = Screenshot::of(setup.hwnd)?;
            let before = before_shot.red_rows(column);
            if before.is_empty() {
                bail!("the base's red line is not on screen before the resize");
            }
            let grown = grow(setup.hwnd, 240)?;
            let after = Screenshot::of(setup.hwnd)?;
            let after_rows = after.red_rows(column);
            let exposed = after.at((20., before_shot.height as f32 / scale + 100.), scale)?;
            let base = format!(
                "base rows {:?}..{:?} -> {:?}..{:?} after growing {grown} px, newly exposed rows \
                 read {exposed:?}",
                before.first(),
                before.last(),
                after_rows.first(),
                after_rows.last()
            );
            if after_rows.first() != before.first() {
                bail!("the base's line moved during the resize, stretched: {base}");
            }
            let visual =
                option_b_resize().unwrap_or_else(|error| format!("not measured: {error:#}"));
            Ok(Check::Passed(format!(
                "(a) {base}; (b) visual-hosted {visual}"
            )))
        }
        .await;
        close_window(cx, setup.window);
        drop(setup.content);
        result
    }

    /// Grows the window's height by `by` pixels at once and returns it.
    fn grow(hwnd: HWND, by: i32) -> anyhow::Result<i32> {
        unsafe {
            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect)?;
            SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top + by,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )?;
        }
        Ok(by)
    }

    extern "system" fn spike_window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    /// Option (b)'s live resize: a plain window whose content is a swap
    /// chain on a DirectComposition visual, drawn once with a red line and
    /// grown without drawing again. Reports where the line shows and what
    /// the newly exposed rows show.
    fn option_b_resize() -> anyhow::Result<String> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = w!("GpuxCompositionResizeSpike");
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(spike_window_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            });
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("composition resize spike"),
                WS_OVERLAPPEDWINDOW,
                420,
                80,
                320,
                220,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            let measured = (|| {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let device: IDCompositionDesktopDevice = DCompositionCreateDevice2(None)?;
                let target: IDCompositionTarget = device.CreateTargetForHwnd(hwnd, true)?;
                let visual: IDCompositionVisual2 = device.CreateVisual()?;
                target.SetRoot(&visual)?;
                let mut client = RECT::default();
                GetClientRect(hwnd, &mut client)?;
                let size = (
                    (client.right - client.left) as u32,
                    (client.bottom - client.top) as u32,
                );
                let content = HostContent::new(&visual.cast::<IUnknown>()?, size, Some(30))?;
                device.Commit()?;
                device.WaitForCommitCompletion()?;
                let before = Screenshot::of(hwnd)?.red_rows(20);
                grow(hwnd, 240)?;
                let after = Screenshot::of(hwnd)?;
                let rows = after.red_rows(20);
                let exposed = after.at((20., (size.1 + 100) as f32), 1.)?;
                drop(content);
                anyhow::Ok(format!(
                    "line rows {:?}..{:?} -> {:?}..{:?}, newly exposed rows read {exposed:?}",
                    before.first(),
                    before.last(),
                    rows.first(),
                    rows.last()
                ))
            })();
            let _ = DestroyWindow(hwnd);
            measured
        }
    }
}
