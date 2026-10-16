//! TalkBack's adapter: an `accesskit_android` `InjectingAdapter` on the view that shows the GPUI
//! surface. The adapter is on jni 0.21 and gpux on 0.22, so it meets gpux's JNI only through the
//! raw VM and Activity handles.
//!
//! Every GPUI window of the app shows on the Activity's one surface and a view takes one
//! accessibility delegate, so the app's windows share one adapter on that view, which describes
//! the front window ([`crate::accessibility_registry`]). The adapter belongs to its Activity: a
//! window opened under a recreated Activity gets a new adapter on the new view, and the adapter
//! goes when the embedded host's context is cleared.

use accesskit_android::InjectingAdapter;
use gpui::accesskit;
use jni_021::{
    JNIEnv, JavaVM,
    objects::{JClass, JObject, JValue},
};

use crate::accessibility_registry::{AdapterHandle, Registration, Registry};

/// `android.R.id.content`.
const CONTENT_ID: i32 = 0x0102_0002;

/// The adapter on the Activity's view.
pub(crate) struct AndroidAdapter(InjectingAdapter);

impl AdapterHandle for AndroidAdapter {
    fn update_if_active(&mut self, update: accesskit::TreeUpdate) {
        // The adapter posts its Java work to the UI thread itself.
        self.0.update_if_active(|| update);
    }
}

static REGISTRY: Registry<AndroidAdapter> = Registry::new();

struct ActivationHandler;

impl accesskit::ActivationHandler for ActivationHandler {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        REGISTRY.activate()
    }
}

struct ActionHandler;

impl accesskit::ActionHandler for ActionHandler {
    fn do_action(&mut self, request: accesskit::ActionRequest) {
        REGISTRY.act(request);
    }
}

/// Registers a window opened in front, under the current Activity. `None` without a VM or an
/// Activity (a headless platform).
pub(crate) fn register(callbacks: gpui::A11yCallbacks) -> Option<Registration<AndroidAdapter>> {
    let activity = super::jni::activity_as_ptr();
    if activity.is_null() {
        return None;
    }
    REGISTRY.register(activity as usize, callbacks, create)
}

/// Drops the adapter: the embedded host cleared its Activity.
pub(crate) fn release() {
    REGISTRY.release();
}

/// The adapter on the current Activity's view, or `None` when the view cannot be read or
/// already has an accessibility delegate (an app's own, which the adapter does not replace).
fn create() -> Option<AndroidAdapter> {
    let vm = super::jni::java_vm();
    let activity = super::jni::activity_as_ptr();
    if vm.is_null() || activity.is_null() {
        return None;
    }
    // SAFETY: the pointers are the process's JavaVM and a global reference to the Activity,
    // valid while it is the current one.
    let vm = unsafe { JavaVM::from_raw(vm.cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let activity = unsafe { JObject::from_raw(activity.cast()) };
    let host = match host_view(&mut env, &activity).and_then(|host| {
        let delegate = env
            .call_method(
                &host,
                "getAccessibilityDelegate",
                "()Landroid/view/View$AccessibilityDelegate;",
                &[],
            )?
            .l()?;
        Ok((host, delegate.is_null()))
    }) {
        Ok((host, true)) => host,
        Ok((_, false)) => {
            log::warn!(
                "accessibility: the GPUI surface's view already has an accessibility delegate; \
                 TalkBack is not given the GPUI tree"
            );
            return None;
        }
        Err(error) => {
            log::warn!("accessibility: could not find the view to expose: {error}");
            env.exception_clear().ok();
            return None;
        }
    };
    Some(AndroidAdapter(InjectingAdapter::new(
        &mut env,
        &host,
        ActivationHandler,
        ActionHandler,
    )))
}

/// The view whose coordinates are the GPUI surface's: the view an embedding host declared with
/// `Composition.setHostView`, else the first `SurfaceView` under the content view (an embedded
/// host), else the decor view, whose window surface a NativeActivity's GPUI draws. The view
/// tree is only read here; the adapter installs its delegate on the UI thread.
fn host_view<'local>(
    env: &mut JNIEnv<'local>,
    activity: &JObject,
) -> jni_021::errors::Result<JObject<'local>> {
    if let Some(declared) = declared_host_view(env, activity) {
        return Ok(declared);
    }
    let window = env
        .call_method(activity, "getWindow", "()Landroid/view/Window;", &[])?
        .l()?;
    let decor = env
        .call_method(&window, "getDecorView", "()Landroid/view/View;", &[])?
        .l()?;
    let content = env
        .call_method(
            &decor,
            "findViewById",
            "(I)Landroid/view/View;",
            &[JValue::Int(CONTENT_ID)],
        )?
        .l()?;
    if !content.is_null()
        && let Some(surface) = find_surface_view(env, &content)?
    {
        return Ok(surface);
    }
    Ok(decor)
}

/// The view `dev.gpux.mobile.Composition.setHostView` declared, if the app has the class and
/// declared one.
fn declared_host_view<'local>(
    env: &mut JNIEnv<'local>,
    activity: &JObject,
) -> Option<JObject<'local>> {
    let found = (|| -> jni_021::errors::Result<Option<JObject<'local>>> {
        // App classes load through the Activity's class loader, not the native thread's.
        let loader = env
            .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
            .l()?;
        let name = env.new_string("dev.gpux.mobile.Composition")?;
        let class = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)],
            )?
            .l()?;
        let class = JClass::from(class);
        let weak = env
            .get_static_field(&class, "hostView", "Ljava/lang/ref/WeakReference;")?
            .l()?;
        if weak.is_null() {
            return Ok(None);
        }
        let view = env
            .call_method(&weak, "get", "()Ljava/lang/Object;", &[])?
            .l()?;
        Ok((!view.is_null()).then_some(view))
    })();
    match found {
        Ok(view) => view,
        Err(_) => {
            // No Composition class in this app, or none declared.
            env.exception_clear().ok();
            None
        }
    }
}

/// The first `SurfaceView` in `view`'s subtree, depth first.
fn find_surface_view<'local>(
    env: &mut JNIEnv<'local>,
    view: &JObject,
) -> jni_021::errors::Result<Option<JObject<'local>>> {
    if env.is_instance_of(view, "android/view/SurfaceView")? {
        return Ok(Some(env.new_local_ref(view)?));
    }
    if !env.is_instance_of(view, "android/view/ViewGroup")? {
        return Ok(None);
    }
    let count = env.call_method(view, "getChildCount", "()I", &[])?.i()?;
    for index in 0..count {
        let child = env
            .call_method(
                view,
                "getChildAt",
                "(I)Landroid/view/View;",
                &[JValue::Int(index)],
            )?
            .l()?;
        if let Some(surface) = find_surface_view(env, &child)? {
            return Ok(Some(surface));
        }
    }
    Ok(None)
}
