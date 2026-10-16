//! TalkBack's adapter: an `accesskit_android` `InjectingAdapter` on the view that shows the GPUI
//! surface. The adapter is on jni 0.21 and gpux on 0.22, so it meets gpux's JNI only through the
//! raw VM and Activity handles.
//!
//! Every GPUI window of the app shows on the Activity's one surface, the last opened in front,
//! and a view takes one accessibility delegate. So the app has one adapter, on that view, and it
//! describes the front window: each window registers its callbacks when it opens, the adapter's
//! handlers reach the front one, only its tree updates reach the adapter, and when the front
//! window changes (one opens, or the front one closes) the new front window is activated, which
//! sends the adapter its full tree.

use std::sync::{Arc, Mutex};

use accesskit_android::InjectingAdapter;
use jni_021::{
    JNIEnv, JavaVM,
    objects::{JObject, JValue},
};

/// `android.R.id.content`.
const CONTENT_ID: i32 = 0x0102_0002;

type Activation = Box<dyn Fn() -> Option<accesskit::TreeUpdate> + Send + 'static>;
type Action = Box<dyn Fn(accesskit::ActionRequest) + Send + 'static>;

/// A window's callbacks, which the adapter's handlers call from the UI thread.
struct Registered {
    key: u64,
    activation: Mutex<Activation>,
    action: Mutex<Action>,
}

impl Registered {
    fn activate(&self) -> Option<accesskit::TreeUpdate> {
        (self.activation.lock().ok()?)()
    }

    fn act(&self, request: accesskit::ActionRequest) {
        if let Ok(action) = self.action.lock() {
            action(request);
        }
    }
}

#[derive(Default)]
struct Windows {
    /// Open windows, the front one last.
    open: Vec<Arc<Registered>>,
    /// Whether an accessibility client activated the adapter.
    activated: bool,
    next_key: u64,
}

static WINDOWS: Mutex<Windows> = Mutex::new(Windows {
    open: Vec::new(),
    activated: false,
    next_key: 0,
});

/// The adapter, apart from [`WINDOWS`]: its handlers lock that while the adapter holds its own
/// lock, so the two are never held together here.
static ADAPTER: Mutex<Option<InjectingAdapter>> = Mutex::new(None);

fn front() -> Option<Arc<Registered>> {
    WINDOWS.lock().ok()?.open.last().cloned()
}

/// Asks the front window for its tree, as the adapter does when it activates, and hands the
/// adapter what it returns; the frame the window then draws brings the full tree.
fn activate_front() {
    let Some(window) = front() else {
        return;
    };
    if let Some(initial) = window.activate()
        && let Some(adapter) = ADAPTER.lock().ok().as_mut().and_then(|a| a.as_mut())
    {
        adapter.update_if_active(|| initial);
    }
}

struct ActivationHandler;

impl accesskit::ActivationHandler for ActivationHandler {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        if let Ok(mut windows) = WINDOWS.lock() {
            windows.activated = true;
        }
        front()?.activate()
    }
}

struct ActionHandler;

impl accesskit::ActionHandler for ActionHandler {
    fn do_action(&mut self, request: accesskit::ActionRequest) {
        if let Some(window) = front() {
            window.act(request);
        }
    }
}

/// A window's place among the windows the adapter describes; dropping it takes the window out.
pub(crate) struct Registration {
    key: u64,
}

/// Registers a window opened in front, putting the adapter on the Activity's view first if
/// none is there yet. `None` without a VM or an Activity (a headless platform).
pub(crate) fn register(callbacks: gpui::A11yCallbacks) -> Option<Registration> {
    {
        let mut adapter = ADAPTER.lock().ok()?;
        if adapter.is_none() {
            *adapter = Some(create()?);
        }
    }
    let activated = {
        let mut windows = WINDOWS.lock().ok()?;
        let key = windows.next_key;
        windows.next_key += 1;
        windows.open.push(Arc::new(Registered {
            key,
            activation: Mutex::new(callbacks.activation),
            action: Mutex::new(callbacks.action),
        }));
        (windows.activated, key)
    };
    if activated.0 {
        activate_front();
    }
    Some(Registration { key: activated.1 })
}

impl Registration {
    /// Hands the adapter `tree_update` if this window is the one in front.
    pub(crate) fn update(&self, tree_update: accesskit::TreeUpdate) {
        if front().is_none_or(|front| front.key != self.key) {
            return;
        }
        if let Some(adapter) = ADAPTER.lock().ok().as_mut().and_then(|a| a.as_mut()) {
            // The adapter posts its Java work to the UI thread itself.
            adapter.update_if_active(|| tree_update);
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let front_changed = WINDOWS.lock().is_ok_and(|mut windows| {
            let was_front = windows.open.last().is_some_and(|w| w.key == self.key);
            windows.open.retain(|window| window.key != self.key);
            was_front && windows.activated && !windows.open.is_empty()
        });
        if front_changed {
            activate_front();
        }
    }
}

/// The adapter on the Activity's view, or `None` when the view hierarchy cannot be read.
fn create() -> Option<InjectingAdapter> {
    let vm = super::jni::java_vm();
    let activity = super::jni::activity_as_ptr();
    if vm.is_null() || activity.is_null() {
        return None;
    }
    // SAFETY: the pointers are the process's JavaVM and a global reference to the Activity,
    // valid for the app's lifetime.
    let vm = unsafe { JavaVM::from_raw(vm.cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let activity = unsafe { JObject::from_raw(activity.cast()) };
    let host = match host_view(&mut env, &activity) {
        Ok(host) => host,
        Err(error) => {
            log::warn!("accessibility: could not find the view to expose: {error}");
            env.exception_clear().ok();
            return None;
        }
    };
    Some(InjectingAdapter::new(
        &mut env,
        &host,
        ActivationHandler,
        ActionHandler,
    ))
}

/// The view whose coordinates are the GPUI surface's: an embedded host's `SurfaceView`, or a
/// NativeActivity's decor view, whose window surface GPUI draws.
fn host_view<'local>(
    env: &mut JNIEnv<'local>,
    activity: &JObject,
) -> jni_021::errors::Result<JObject<'local>> {
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
