//! Android platform view implementation.
//!
//! Embeds native Android `View` instances in the GPUI render tree through
//! window composition: gpux's Java helper (`dev.gpux.mobile.PlatformViews`,
//! under `crates/gpui_mobile/java`) creates the view for its type, and
//! `platform_view_element` puts it in the container of a composition native
//! surface, which GPUI sizes and places between its base content and its
//! overlay.

use super::jni as android_jni;
use crate::platform_view::{
    PlatformView, PlatformViewBounds, PlatformViewFactory, PlatformViewId, PlatformViewParams,
};
use jni::objects::{JClass, JObject, JValue};
use jni::refs::Global;
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};

/// The helper class, in dot notation for the activity's class loader.
const HELPER: &str = "dev.gpux.mobile.PlatformViews";

/// Android implementation of a platform view: a native `View` the helper
/// made, which fills the composition container it is attached to.
pub struct AndroidPlatformView {
    id: PlatformViewId,
    view_type: String,
    disposed: AtomicBool,
}

/// Runs `f` with the helper class; `name` labels errors.
fn call_helper<T>(
    name: &str,
    f: impl FnOnce(&mut jni::Env, &JClass) -> jni::errors::Result<T>,
) -> Result<T, String> {
    android_jni::with_env(|env| {
        let helper = android_jni::find_app_class(env, HELPER)?;
        f(env, &helper).map_err(|error| {
            env.exception_describe();
            env.exception_clear();
            format!("{HELPER}.{name}: {error}")
        })
    })
}

impl AndroidPlatformView {
    /// Creates the view through the helper; it shows once attached.
    pub fn new(view_type: &str, params: &PlatformViewParams) -> Result<Self, String> {
        let id = PlatformViewId::next();
        // Serialize creation params as "key1=value1|key2=value2".
        let creation_params = params
            .creation_params
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("|");
        let created = call_helper("createView", |env, helper| {
            let activity =
                android_jni::activity(env).map_err(|_| jni::errors::Error::NullPtr("activity"))?;
            let view_type = env.new_string(view_type)?;
            let params = env.new_string(&creation_params)?;
            env.call_static_method(
                helper,
                jni::jni_str!("createView"),
                jni::jni_sig!("(Landroid/app/Activity;Ljava/lang/String;JLjava/lang/String;)Z"),
                &[
                    JValue::Object(&activity),
                    JValue::Object(&view_type),
                    JValue::Long(id.0 as i64),
                    JValue::Object(&params),
                ],
            )?
            .z()
        })?;
        if !created {
            return Err(format!("{HELPER} has no factory for `{view_type}`"));
        }
        log::info!("AndroidPlatformView: created {id} of type '{view_type}'");
        Ok(Self {
            id,
            view_type: view_type.to_string(),
            disposed: AtomicBool::new(false),
        })
    }
}

impl PlatformView for AndroidPlatformView {
    fn id(&self) -> PlatformViewId {
        self.id
    }

    fn view_type(&self) -> &str {
        &self.view_type
    }

    /// Composition places the view's container; `set_frame` the view in it.
    fn set_bounds(&self, _: PlatformViewBounds) {}

    fn set_visible(&self, visible: bool) {
        if self.disposed.load(Ordering::Relaxed) {
            return;
        }
        let id = self.id.0 as i64;
        call_helper("setVisible", |env, helper| {
            env.call_static_method(
                helper,
                jni::jni_str!("setVisible"),
                jni::jni_sig!("(JZ)V"),
                &[JValue::Long(id), JValue::Bool(visible)],
            )
            .map(|_| ())
        })
        .map_err(|error| log::error!("{error}"))
        .ok();
    }

    /// The view's frame in its container, handed to the helper in device
    /// pixels.
    fn set_frame(&self, frame: PlatformViewBounds, scale_factor: f32) {
        if self.disposed.load(Ordering::Relaxed) {
            return;
        }
        let id = self.id.0 as i64;
        let pixels = |value: f32| JValue::Int((value * scale_factor).round() as i32);
        call_helper("setFrame", |env, helper| {
            env.call_static_method(
                helper,
                jni::jni_str!("setFrame"),
                jni::jni_sig!("(JIIII)V"),
                &[
                    JValue::Long(id),
                    pixels(frame.x),
                    pixels(frame.y),
                    pixels(frame.width),
                    pixels(frame.height),
                ],
            )
            .map(|_| ())
        })
        .map_err(|error| log::error!("{error}"))
        .ok();
    }

    /// Composition orders native surfaces; the view's own z-index is moot.
    fn set_z_index(&self, _: i32) {}

    fn attach(&self, container: &dyn Any) -> Result<(), String> {
        let container = container
            .downcast_ref::<Global<JObject<'static>>>()
            .ok_or("the container is not a composition native surface's")?;
        let id = self.id.0 as i64;
        call_helper("attach", |env, helper| {
            env.call_static_method(
                helper,
                jni::jni_str!("attach"),
                jni::jni_sig!("(JLandroid/widget/FrameLayout;)V"),
                &[JValue::Long(id), JValue::Object(container.as_obj())],
            )
            .map(|_| ())
        })
    }

    fn dispose(&self) {
        if self.disposed.swap(true, Ordering::Relaxed) {
            return;
        }
        let id = self.id.0 as i64;
        call_helper("disposeView", |env, helper| {
            env.call_static_method(
                helper,
                jni::jni_str!("disposeView"),
                jni::jni_sig!("(J)V"),
                &[JValue::Long(id)],
            )
            .map(|_| ())
        })
        .map_err(|error| log::error!("{error}"))
        .ok();
        log::info!("AndroidPlatformView: disposed view {}", self.id);
    }

    fn is_disposed(&self) -> bool {
        self.disposed.load(Ordering::Relaxed)
    }
}

/// Generic Android platform view factory: the helper creates the `View` for
/// the type, from the factories it has (built in, or registered by the app).
pub struct AndroidPlatformViewFactory {
    view_type: String,
}

impl AndroidPlatformViewFactory {
    pub fn new(view_type: &str) -> Self {
        Self {
            view_type: view_type.to_string(),
        }
    }
}

impl PlatformViewFactory for AndroidPlatformViewFactory {
    fn create(&self, params: &PlatformViewParams) -> Result<Box<dyn PlatformView>, String> {
        let view = AndroidPlatformView::new(&self.view_type, params)?;
        Ok(Box::new(view))
    }

    fn view_type(&self) -> &str {
        &self.view_type
    }
}
