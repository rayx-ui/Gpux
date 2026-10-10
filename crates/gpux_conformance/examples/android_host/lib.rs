//! Lane E's Android host: a NativeActivity app that runs the lane-E conformance scenes and the
//! Android platform checks in the activity's window, logging one `gpux-conformance:` result line
//! each to logcat.
//!
//! `gpux-testkit android build` packages it into an APK and `gpux-testkit android run` installs,
//! launches and follows it. Checks that need the operating system ask the testkit for it in a
//! `gpux-conformance-host:` request line and wait for the outcome:
//!
//! - `tap <x> <y>` and `swipe <x1> <y1> <x2> <y2> <ms>`: `adb shell input` in physical pixels;
//! - `background`: the home key, then the activity relaunched with `am start`;
//! - `screenshot <n>`: `adb screencap`, written to the app's `files/screens/<n>.png`.
//!
//! Pixel steps read the screen through `screenshot`: the GL drivers of Android emulators read
//! rendered textures back empty, and the screen is what the user sees anyway.
//!
//! The run ends with `gpux-conformance-host: done`. Failing pixel steps save their frame under
//! the app's files directory (`frames/`), which the testkit pulls. The `debug.gpux.scene` and
//! `debug.gpux.grep` system properties narrow the run like `--scene` and `--grep` elsewhere.

#[cfg(target_os = "android")]
mod checks;
#[cfg(target_os = "android")]
mod composition;
mod platform_views;

#[cfg(target_os = "android")]
mod android {
    use crate::checks;
    use gpui_mobile::android::jni;
    use gpux::{
        AnyWindowHandle, AppContext as _, AsyncApp, Context, IntoElement, Render, Styled as _,
        Window, WindowOptions, div, point, px, rgb,
    };
    use gpux_conformance::{
        CATALOG, Lane,
        app_host::{self, AppHost, FrameSource},
        report::{Outcome, ResultLine},
    };
    use std::{
        cell::Cell,
        path::{Path, PathBuf},
        rc::Rc,
        time::{Duration, Instant},
    };

    /// Logs a result line where the testkit reads it.
    pub fn report(line: &ResultLine) {
        log::info!("{line}");
    }

    /// A system property, or `None` when it is unset or empty.
    fn property(name: &str) -> Option<String> {
        let name = std::ffi::CString::new(name).ok()?;
        let mut value = [0 as std::ffi::c_char; 92];
        // SAFETY: `value` has room for PROP_VALUE_MAX (92) bytes, as the call requires.
        let length = unsafe { libc::__system_property_get(name.as_ptr(), value.as_mut_ptr()) };
        if length <= 0 {
            return None;
        }
        // SAFETY: the call wrote a NUL-terminated string into `value`.
        let value = unsafe { std::ffi::CStr::from_ptr(value.as_ptr()) };
        Some(value.to_string_lossy().into_owned()).filter(|value| !value.is_empty())
    }

    /// The selection the testkit set, as `(scene, grep)`.
    pub fn selection() -> (Option<String>, Option<String>) {
        (property("debug.gpux.scene"), property("debug.gpux.grep"))
    }

    /// Whether the test `id` is in the selection.
    pub fn selected(id: &str) -> bool {
        let (scene, grep) = selection();
        gpux_conformance::selects(id, scene.as_deref(), grep.as_deref())
    }

    /// Asks the testkit for an operating-system action.
    pub fn request(request: &str) {
        log::info!("gpux-conformance-host: {request}");
    }

    /// How long the host waits for an answer before repeating a request.
    const REQUEST_REPEAT: Duration = Duration::from_secs(5);

    /// Screenshots the testkit takes on request, numbered in order.
    pub struct Screens {
        dir: PathBuf,
        taken: Cell<u32>,
    }

    impl Screens {
        fn new(dir: PathBuf) -> Rc<Self> {
            std::fs::create_dir_all(&dir).ok();
            Rc::new(Self {
                dir,
                taken: Cell::new(0),
            })
        }

        /// The screen once `window`'s latest frame is presented, in physical pixels.
        pub async fn capture(
            &self,
            cx: &mut AsyncApp,
            window: AnyWindowHandle,
        ) -> anyhow::Result<image::RgbaImage> {
            app_host::wait_frames(cx, window, 2).await?;
            let number = self.taken.get() + 1;
            self.taken.set(number);
            let path = self.dir.join(format!("{number}.png"));
            let start = Instant::now();
            let mut asked = None::<Instant>;
            while !path.is_file() {
                anyhow::ensure!(
                    start.elapsed() < Duration::from_secs(60),
                    "the testkit sent no screenshot {number}"
                );
                // `adb logcat` can drop lines while adb transfers a screenshot, so the request is
                // repeated until the file arrives; the testkit takes each number once.
                if asked.is_none_or(|asked| asked.elapsed() >= REQUEST_REPEAT) {
                    request(&format!("screenshot {number}"));
                    asked = Some(Instant::now());
                }
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
            }
            let bytes = std::fs::read(&path)?;
            std::fs::remove_file(&path).ok();
            Ok(image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)?.to_rgba8())
        }

        /// Pixel steps' frames, from the screen.
        fn frame_source(self: &Rc<Self>) -> FrameSource {
            let screens = Rc::clone(self);
            Rc::new(move |mut cx, window| {
                let screens = Rc::clone(&screens);
                Box::pin(async move { screens.capture(&mut cx, window).await })
            })
        }
    }

    /// The host's own window: open for the whole run, so closing a scene's window never
    /// closes the app's last one (which quits the app).
    struct Anchor;

    impl Render for Anchor {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(0xffffff))
        }
    }

    async fn run(cx: &mut AsyncApp, data: &Path) {
        let screens = Screens::new(data.join("screens"));
        if let Err(error) = cx.update(app_host::register_bundled_fonts) {
            log::error!("android_host: could not register the bundled fonts: {error:#}");
        }
        let _anchor = match cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Anchor)) {
            Ok(anchor) => anchor,
            Err(error) => {
                report(&ResultLine::new(
                    Outcome::Fail,
                    "android-startup".to_string(),
                    format!("could not open the host window: {error:#}"),
                ));
                request("done");
                return;
            }
        };
        if selected("android-startup") {
            report(&checks::startup(cx, &screens).await);
        }
        let host = AppHost::new(Lane::E, point(px(0.), px(0.)))
            .with_failure_frames(data.join("frames"))
            .with_frame_source(screens.frame_source());
        for scene in CATALOG
            .iter()
            .filter(|scene| scene.runs_in(Lane::E) && selected(scene.id))
        {
            report(&host.run_scene(cx, scene).await);
        }
        for line in checks::os_checks(cx, &screens).await {
            report(&line);
        }
        request("done");
    }

    /// The activity's entry point, called by `android-activity` on its native thread.
    #[unsafe(no_mangle)]
    fn android_main(app: android_activity::AndroidApp) {
        jni::install_panic_hook();
        let data = app
            .internal_data_path()
            .unwrap_or_else(|| PathBuf::from("."));
        jni::init_platform(&app);
        let Some(platform) = jni::shared_platform() else {
            report(&ResultLine::new(
                Outcome::Fail,
                "android-startup".to_string(),
                "the GPUI platform did not initialize".to_string(),
            ));
            request("done");
            return;
        };
        gpux::Application::with_platform(platform.into_rc()).run(move |mut cx| async move {
            run(&mut cx, &data).await;
        });
    }
}
