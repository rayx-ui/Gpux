//! The windows one screen-reader adapter describes, when several GPUI windows share one host
//! view (Android: every window shows on the Activity's one surface, and a view takes one
//! accessibility delegate).
//!
//! The adapter belongs to its host (an Activity): a window opening under another host drops the
//! old adapter and puts a new one on the new host's view, and the adapter goes when the host is
//! released. It stays while its host does, with no window open too: an adapter's removal from
//! its view is queued on the UI thread, so a new one put on the same view at once would find the
//! old delegate still there. It describes the front (last opened) window: its
//! handlers reach that window's callbacks, only that window's tree updates reach the adapter,
//! and when the front window changes the new one is asked for its tree, which the adapter then
//! gets in full.
//!
//! The windows and the adapter sit behind separate locks: an adapter calls its handlers (which
//! lock the windows) while it holds its own lock, so the registry never holds both.

use std::sync::{Arc, Mutex};

use gpui::accesskit;

/// The part of an adapter the registry drives.
pub(crate) trait AdapterHandle {
    /// Hands the adapter `update` if a screen reader activated it.
    fn update_if_active(&mut self, update: accesskit::TreeUpdate);
}

type Activation = Box<dyn Fn() -> Option<accesskit::TreeUpdate> + Send + 'static>;
type Action = Box<dyn Fn(accesskit::ActionRequest) + Send + 'static>;

struct Registered {
    key: u64,
    activation: Mutex<Activation>,
    action: Mutex<Action>,
}

struct Windows {
    /// Open windows, the front one last.
    open: Vec<Arc<Registered>>,
    /// Whether a screen reader activated the current adapter.
    activated: bool,
    next_key: u64,
}

struct Installed<A> {
    /// The host the adapter is on.
    host: usize,
    adapter: A,
}

/// The windows sharing a host view and the adapter on it.
pub(crate) struct Registry<A> {
    windows: Mutex<Windows>,
    adapter: Mutex<Option<Installed<A>>>,
}

impl<A: AdapterHandle> Registry<A> {
    pub(crate) const fn new() -> Self {
        Self {
            windows: Mutex::new(Windows {
                open: Vec::new(),
                activated: false,
                next_key: 0,
            }),
            adapter: Mutex::new(None),
        }
    }

    fn front(&self) -> Option<Arc<Registered>> {
        self.windows.lock().ok()?.open.last().cloned()
    }

    /// What the adapter's activation handler returns: the front window's tree. Marks the
    /// adapter activated.
    pub(crate) fn activate(&self) -> Option<accesskit::TreeUpdate> {
        if let Ok(mut windows) = self.windows.lock() {
            windows.activated = true;
        }
        let window = self.front()?;
        let activation = window.activation.lock().ok()?;
        activation()
    }

    /// What the adapter's action handler does: hands the request to the front window.
    pub(crate) fn act(&self, request: accesskit::ActionRequest) {
        if let Some(window) = self.front()
            && let Ok(action) = window.action.lock()
        {
            action(request);
        }
    }

    /// Asks the front window for its tree, as an adapter does when it activates, and hands the
    /// adapter what it returns; the frame the window then draws brings the full tree.
    fn activate_front(&self) {
        let Some(window) = self.front() else {
            return;
        };
        let initial = window
            .activation
            .lock()
            .ok()
            .and_then(|activation| activation());
        if let Some(initial) = initial
            && let Ok(mut adapter) = self.adapter.lock()
            && let Some(installed) = adapter.as_mut()
        {
            installed.adapter.update_if_active(initial);
        }
    }

    /// Registers a window opened in front on `host`, putting an adapter (`create`) on the host
    /// when it has none: a window under another host replaces the adapter of the old one.
    pub(crate) fn register(
        &'static self,
        host: usize,
        callbacks: gpui::A11yCallbacks,
        create: impl FnOnce() -> Option<A>,
    ) -> Option<Registration<A>> {
        let replaced = {
            let mut adapter = self.adapter.lock().ok()?;
            let stale = adapter
                .as_ref()
                .is_some_and(|installed| installed.host != host);
            if stale {
                // The old host is gone (an Activity recreated): its adapter goes with it.
                *adapter = None;
            }
            if adapter.is_none() {
                *adapter = create().map(|adapter| Installed { host, adapter });
            }
            stale
        };
        let (activated, key) = {
            let mut windows = self.windows.lock().ok()?;
            if replaced {
                windows.activated = false;
            }
            let key = windows.next_key;
            windows.next_key += 1;
            windows.open.push(Arc::new(Registered {
                key,
                activation: Mutex::new(callbacks.activation),
                action: Mutex::new(callbacks.action),
            }));
            (windows.activated, key)
        };
        if activated {
            self.activate_front();
        }
        Some(Registration {
            registry: self,
            key,
        })
    }

    /// Drops the adapter: its host is released (an embedded Activity's context cleared).
    pub(crate) fn release(&self) {
        if let Ok(mut adapter) = self.adapter.lock() {
            *adapter = None;
        }
        if let Ok(mut windows) = self.windows.lock() {
            windows.activated = false;
        }
    }

    /// Whether an adapter is installed, on which host.
    #[cfg(test)]
    fn host(&self) -> Option<usize> {
        self.adapter
            .lock()
            .ok()?
            .as_ref()
            .map(|installed| installed.host)
    }

    #[cfg(test)]
    fn with_adapter<R>(&self, f: impl FnOnce(&mut A) -> R) -> Option<R> {
        self.adapter
            .lock()
            .ok()?
            .as_mut()
            .map(|installed| f(&mut installed.adapter))
    }
}

/// A window's place among the windows the adapter describes; dropping it takes the window out.
pub(crate) struct Registration<A: AdapterHandle + 'static> {
    registry: &'static Registry<A>,
    key: u64,
}

impl<A: AdapterHandle> Registration<A> {
    /// Hands the adapter `tree_update` if this window is the one in front.
    pub(crate) fn update(&self, tree_update: accesskit::TreeUpdate) {
        if self
            .registry
            .front()
            .is_none_or(|front| front.key != self.key)
        {
            return;
        }
        if let Ok(mut adapter) = self.registry.adapter.lock()
            && let Some(installed) = adapter.as_mut()
        {
            installed.adapter.update_if_active(tree_update);
        }
    }
}

impl<A: AdapterHandle + 'static> Drop for Registration<A> {
    fn drop(&mut self) {
        let front_changed = self
            .registry
            .windows
            .lock()
            .map(|mut windows| {
                let was_front = windows.open.last().is_some_and(|w| w.key == self.key);
                windows.open.retain(|window| window.key != self.key);
                was_front && windows.activated && !windows.open.is_empty()
            })
            .unwrap_or(false);
        if front_changed {
            self.registry.activate_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Fake {
        updates: Vec<accesskit::TreeUpdate>,
    }

    impl AdapterHandle for Fake {
        fn update_if_active(&mut self, update: accesskit::TreeUpdate) {
            self.updates.push(update);
        }
    }

    fn tree(label: &str) -> accesskit::TreeUpdate {
        let mut root = accesskit::Node::new(accesskit::Role::Window);
        root.set_label(label);
        accesskit::TreeUpdate {
            nodes: vec![(accesskit::NodeId(0), root)],
            tree: Some(accesskit::TreeInfo::new(accesskit::NodeId(0))),
            tree_id: accesskit::TreeId::ROOT,
            focus: accesskit::NodeId(0),
        }
    }

    fn callbacks(label: &'static str, activations: Arc<AtomicUsize>) -> gpui::A11yCallbacks {
        gpui::A11yCallbacks {
            activation: Box::new(move || {
                activations.fetch_add(1, Ordering::SeqCst);
                Some(tree(label))
            }),
            action: Box::new(|_| {}),
            deactivation: Box::new(|| {}),
        }
    }

    fn registry() -> &'static Registry<Fake> {
        Box::leak(Box::new(Registry::new()))
    }

    #[test]
    fn accessibility_registry_keeps_one_adapter_per_host_and_rebuilds_it_for_a_new_host() {
        let registry = registry();
        let created = Arc::new(AtomicUsize::new(0));
        let create = || {
            created.fetch_add(1, Ordering::SeqCst);
            Some(Fake::default())
        };
        let count = Arc::new(AtomicUsize::new(0));
        let first = registry.register(1, callbacks("a", count.clone()), create);
        let second = registry.register(1, callbacks("b", count.clone()), create);
        assert_eq!(
            created.load(Ordering::SeqCst),
            1,
            "windows on one host share it"
        );
        assert_eq!(registry.host(), Some(1));
        drop((first, second));
        assert_eq!(
            registry.host(),
            Some(1),
            "the adapter stays on its host with no window open"
        );
        // A window opened again on the same host reuses it, and a screen reader that activated
        // it is given the new window's tree.
        registry.activate();
        let again = registry.register(1, callbacks("c", count.clone()), create);
        assert_eq!(created.load(Ordering::SeqCst), 1);
        let last = registry.with_adapter(|adapter| {
            adapter
                .updates
                .last()
                .and_then(|update| update.nodes[0].1.label().map(str::to_string))
        });
        assert_eq!(last, Some(Some("c".to_string())));
        drop(again);

        // The host goes away (an Activity recreated): the next window under the new host gets
        // an adapter of its own, not the dead view's.
        let old = registry.register(1, callbacks("a", count.clone()), create);
        registry.activate();
        let new = registry.register(2, callbacks("b", count), create);
        assert_eq!(registry.host(), Some(2));
        assert_eq!(created.load(Ordering::SeqCst), 2);
        // The new adapter starts unactivated: the front window is not asked for its tree
        // until a screen reader activates it.
        assert_eq!(registry.with_adapter(|a| a.updates.len()), Some(0));
        drop((old, new));
    }

    #[test]
    fn accessibility_registry_serves_the_front_window() {
        let registry = registry();
        let count = Arc::new(AtomicUsize::new(0));
        let back = registry
            .register(
                1,
                callbacks("back", count.clone()),
                || Some(Fake::default()),
            )
            .unwrap();
        assert_eq!(
            registry.activate().unwrap().nodes[0].1.label(),
            Some("back")
        );
        let front = registry
            .register(1, callbacks("front", count), || Some(Fake::default()))
            .unwrap();
        // The new front window is asked for its tree, which the adapter gets.
        let labels = || {
            registry
                .with_adapter(|adapter| {
                    adapter
                        .updates
                        .iter()
                        .map(|update| update.nodes[0].1.label().unwrap_or_default().to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap()
        };
        assert_eq!(labels(), ["front"]);
        // Only the front window's updates reach the adapter.
        back.update(tree("back again"));
        front.update(tree("front again"));
        assert_eq!(labels(), ["front", "front again"]);
        drop(front);
        back.update(tree("back now"));
        assert_eq!(labels().last().map(String::as_str), Some("back now"));
    }

    #[test]
    fn accessibility_registry_hands_actions_to_the_front_window() {
        let registry = registry();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let window = |name: &'static str| {
            let seen = seen.clone();
            gpui::A11yCallbacks {
                activation: Box::new(|| None),
                action: Box::new(move |_| seen.lock().unwrap().push(name)),
                deactivation: Box::new(|| {}),
            }
        };
        let request = || accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(1),
            data: None,
        };
        let back = registry.register(1, window("back"), || Some(Fake::default()));
        let front = registry.register(1, window("front"), || Some(Fake::default()));
        registry.act(request());
        drop(front);
        registry.act(request());
        assert_eq!(*seen.lock().unwrap(), ["front", "back"]);
        drop(back);
    }

    #[test]
    fn accessibility_registry_release_drops_the_adapter() {
        let registry = registry();
        let count = Arc::new(AtomicUsize::new(0));
        let window = registry.register(7, callbacks("a", count), || Some(Fake::default()));
        registry.release();
        assert_eq!(registry.host(), None);
        drop(window);
    }
}
