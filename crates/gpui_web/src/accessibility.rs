//! The web platform's screen-reader adapter: a DOM mirror of the window's AccessKit tree.
//!
//! AccessKit has no web adapter, and a canvas is one opaque element to a screen reader, so each
//! window keeps, beside its canvas, a tree of DOM elements built from its AccessKit tree, as
//! canvas UIs such as Flutter's do. A browser cannot report a screen reader, so the mirror starts
//! when the user activates a visually hidden "Enable accessibility" button, first in the window's
//! DOM, or from the window's first frame when the application asked for [`WebScreenReader::Always`].
//! Until then the window builds no tree for it and the mirror has no elements.
//!
//! Each node other than a generic container gets one element: its ARIA role (from gpux's one role
//! table, [`gpux::aria_role`]), name, description, value and range, states, relations (over DOM
//! ids derived from node ids), list and table positions and an absolutely positioned box at the
//! node's bounds. Elements are transparent and never take pointer input; the mirror's root has a
//! transform, so it is the containing block of every element's fixed box, which keeps each box
//! in window coordinates whatever its DOM parent. A generic container's children go to its
//! nearest ancestor with an element. Updates go through `accesskit_consumer`'s change handler, so
//! an update touches only the elements of the nodes it changed (and reorders the children of
//! their parents).
//!
//! Screen readers operate the mirror through DOM events: `click` becomes AccessKit `Click`, DOM
//! focus becomes `Focus` (and `ScrollIntoView` for a node outside a container that clips it), and
//! the ARIA keyboard interactions of sliders, spin buttons and disclosures become
//! `Increment`/`Decrement`/`Expand`/`Collapse`. Other keys reach GPUI as they do from the IME
//! mirror. When GPUI's focus moves, the mirror moves DOM focus to the focused node's element
//! behind a guard, so the browser's `focus` event is not sent back; a focused text field is the
//! IME mirror's textarea, labelled with the node's name, so typing and IMEs keep their one path.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::{Rc, Weak},
};

use accesskit::{
    Action, ActionData, ActionRequest, Invalid, Live, NodeId, Orientation, Role, Toggled, TreeId,
    TreeUpdate,
};
use accesskit_consumer::{NodeRef, Tree, TreeChangeHandler as ChangeHandler};
use gpux::A11yCallbacks;
use wasm_bindgen::{JsCast, closure::Closure};

use crate::{events::EventListenerHandle, window::WebWindowInner};

/// How web windows offer the screen-reader mirror. Set it with [`set_web_screen_reader`] before
/// opening a window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WebScreenReader {
    /// A visually hidden, focusable "Enable accessibility" button comes first in each window's
    /// DOM; activating it turns the mirror on. Browsers do not tell a page that a screen reader
    /// runs, so this is the default.
    #[default]
    OnRequest,
    /// Every window builds the mirror from its first frame, with no button.
    Always,
}

/// The label of the enable button when the application set none.
pub const DEFAULT_SCREEN_READER_LABEL: &str = "Enable accessibility";

thread_local! {
    static SETTINGS: RefCell<(WebScreenReader, Option<String>)> = const { RefCell::new((WebScreenReader::OnRequest, None)) };
}

/// Sets how windows opened from now on offer the screen-reader mirror.
pub fn set_web_screen_reader(mode: WebScreenReader) {
    SETTINGS.with(|settings| settings.borrow_mut().0 = mode);
}

/// Sets the label of the "Enable accessibility" button of windows opened from now on, for an
/// application to localize it.
pub fn set_web_screen_reader_label(label: impl Into<String>) {
    SETTINGS.with(|settings| settings.borrow_mut().1 = Some(label.into()));
}

fn settings() -> (WebScreenReader, String) {
    SETTINGS.with(|settings| {
        let settings = settings.borrow();
        (
            settings.0,
            settings
                .1
                .clone()
                .unwrap_or_else(|| DEFAULT_SCREEN_READER_LABEL.to_string()),
        )
    })
}

/// Hides the enable button visually and leaves it focusable and readable.
const BUTTON_STYLE: &str = "position:fixed;left:0;top:0;width:1px;height:1px;margin:-1px;padding:0;\
     border:0;overflow:hidden;clip:rect(0 0 0 0);clip-path:inset(50%);white-space:nowrap";

/// The mirror's root: a transformed box over the canvas, the containing block of every element.
const ROOT_STYLE: &str = "position:fixed;left:0;top:0;margin:0;padding:0;border:0;\
     pointer-events:none;overflow:visible;z-index:0";

/// Each element: a transparent box at its node's bounds that never takes pointer input.
const ELEMENT_STYLE: &str = "position:fixed;margin:0;padding:0;border:0;box-sizing:border-box;\
     pointer-events:none;color:transparent;background:transparent;overflow:hidden;\
     white-space:pre;outline:none;font-size:1px;line-height:1px";

static NEXT_WINDOW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A web window's screen-reader adapter.
pub(crate) struct WebAccessibility {
    window: Weak<WebWindowInner>,
    callbacks: A11yCallbacks,
    document: web_sys::Document,
    /// The enable button, until the mirror starts.
    button: RefCell<Option<web_sys::HtmlElement>>,
    mirror: RefCell<Option<Mirror>>,
    /// Set while the mirror moves DOM focus itself, so the `focus` event that follows is not
    /// sent back to GPUI.
    moving_focus: Cell<bool>,
    /// The DOM id prefix of this window's elements.
    prefix: String,
    listeners: RefCell<Vec<EventListenerHandle>>,
}

struct Mirror {
    root: web_sys::HtmlElement,
    tree: Tree,
    /// The element of every node that has one; the window's root node is the mirror's root.
    elements: HashMap<NodeId, web_sys::HtmlElement>,
    /// Nodes whose `accessibility_id` may name an `html_element` overlay, owned by `aria-owns`.
    owners: HashMap<NodeId, String>,
    /// The number of action requests the mirror sent, published on the root for tests.
    actions: u32,
}

/// What an update changed, collected by the change handler and applied after it.
#[derive(Default)]
struct Changes {
    written: Vec<NodeId>,
    removed: Vec<NodeId>,
    focus: Option<Option<NodeId>>,
}

impl ChangeHandler for Changes {
    fn node_added(&mut self, node: &NodeRef) {
        self.written.push(node.locate().0);
    }

    fn node_updated(&mut self, _old: &NodeRef, new: &NodeRef) {
        self.written.push(new.locate().0);
    }

    fn focus_moved(&mut self, _old: Option<&NodeRef>, new: Option<&NodeRef>) {
        self.focus = Some(new.map(|node| node.locate().0));
    }

    fn node_removed(&mut self, node: &NodeRef) {
        self.removed.push(node.locate().0);
    }
}

/// Whether `node` gets an element of its own.
fn has_element(node: &NodeRef) -> bool {
    !matches!(node.role(), Role::GenericContainer | Role::Unknown)
}

fn style(element: &web_sys::HtmlElement, css: &str) {
    element.set_attribute("style", css).ok();
}

fn attribute(element: &web_sys::Element, name: &str, value: Option<&str>) {
    match value {
        Some(value) => element.set_attribute(name, value).ok(),
        None => element.remove_attribute(name).ok(),
    };
}

fn number(value: f64) -> String {
    if value.fract() == 0. && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

impl WebAccessibility {
    /// Sets up `window`'s adapter: the enable button, or the mirror at once under
    /// [`WebScreenReader::Always`].
    pub(crate) fn init(window: &Rc<WebWindowInner>, callbacks: A11yCallbacks) {
        let Some(document) = window.browser_window.document() else {
            return;
        };
        let (mode, label) = settings();
        let prefix = format!(
            "gpux-a11y-{}-",
            NEXT_WINDOW.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let this = Rc::new(Self {
            window: Rc::downgrade(window),
            callbacks,
            document,
            button: RefCell::new(None),
            mirror: RefCell::new(None),
            moving_focus: Cell::new(false),
            prefix,
            listeners: RefCell::new(Vec::new()),
        });
        match mode {
            WebScreenReader::OnRequest => {
                if let Some(button) = this.create_button(window, &label) {
                    *this.button.borrow_mut() = Some(button);
                }
            }
            WebScreenReader::Always => {
                // After the window's setup returns, as an adapter's activation would arrive.
                let weak = Rc::downgrade(&this);
                let start = Closure::once_into_js(move || {
                    if let Some(this) = weak.upgrade() {
                        this.start();
                    }
                });
                window
                    .browser_window
                    .set_timeout_with_callback(start.unchecked_ref())
                    .ok();
            }
        }
        *window.accessibility.borrow_mut() = Some(this);
    }

    fn create_button(
        self: &Rc<Self>,
        window: &WebWindowInner,
        label: &str,
    ) -> Option<web_sys::HtmlElement> {
        let button: web_sys::HtmlElement = self
            .document
            .create_element("button")
            .ok()?
            .dyn_into()
            .ok()?;
        button.set_attribute("type", "button").ok();
        button.set_attribute("data-gpux-a11y-enable", "").ok();
        button.set_text_content(Some(label));
        style(&button, BUTTON_STYLE);
        let canvas: &web_sys::Node = window.canvas.as_ref();
        let parent = canvas.parent_node()?;
        parent.insert_before(&button, Some(canvas)).ok()?;
        let weak = Rc::downgrade(self);
        let handle = EventListenerHandle::add(button.as_ref(), "click", move |_| {
            if let Some(this) = weak.upgrade() {
                this.start();
            }
        });
        self.listeners.borrow_mut().push(handle);
        Some(button)
    }

    /// Turns the window's platform source on and builds the mirror, from the root-only tree the
    /// activation returns; the frame it requests sends the full tree.
    fn start(self: &Rc<Self>) {
        if self.mirror.borrow().is_some() {
            return;
        }
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let Some(initial) = (self.callbacks.activation)() else {
            return;
        };
        let Ok(root) = self
            .document
            .create_element("div")
            .and_then(|root| root.dyn_into::<web_sys::HtmlElement>().map_err(Into::into))
        else {
            return;
        };
        root.set_attribute("data-gpux-a11y-mirror", "").ok();
        root.set_id(&format!("{}root", self.prefix));
        style(&root, ROOT_STYLE);
        let canvas: &web_sys::Element = window.canvas.as_ref();
        canvas.after_with_node_1(&root).ok();
        let had_focus =
            self.button.borrow().as_ref().is_some_and(|button| {
                self.document.active_element().as_ref() == Some(button.as_ref())
            });
        if let Some(button) = self.button.borrow_mut().take() {
            let button: &web_sys::Element = button.as_ref();
            button.remove();
        }
        self.listen(&root);

        let tree = Tree::new(initial, true);
        let mut elements = HashMap::new();
        elements.insert(tree.state().root().locate().0, root.clone());
        *self.mirror.borrow_mut() = Some(Mirror {
            root,
            tree,
            elements,
            owners: HashMap::new(),
            actions: 0,
        });
        let all = self.with_mirror(|mirror| {
            let mut ids = Vec::new();
            let mut stack = vec![mirror.tree.state().root()];
            while let Some(node) = stack.pop() {
                ids.push(node.locate().0);
                stack.extend(node.children());
            }
            ids
        });
        self.apply(Changes {
            written: all.unwrap_or_default(),
            ..Changes::default()
        });
        if had_focus {
            window.focus_ime_mirror();
        }
    }

    fn with_mirror<R>(&self, f: impl FnOnce(&mut Mirror) -> R) -> Option<R> {
        self.mirror.borrow_mut().as_mut().map(f)
    }

    /// Applies a tree update the window sends while its platform source is on.
    pub(crate) fn update(self: &Rc<Self>, update: TreeUpdate) {
        let mut changes = Changes::default();
        if self
            .with_mirror(|mirror| mirror.tree.update_and_process_changes(update, &mut changes))
            .is_none()
        {
            return;
        }
        self.apply(changes);
    }

    fn apply(self: &Rc<Self>, changes: Changes) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let scale = window.state.borrow().scale_factor.max(f32::EPSILON) as f64;
        let focus = {
            let mut guard = self.mirror.borrow_mut();
            let Some(mirror) = guard.as_mut() else {
                return;
            };
            mirror.place_root(&window);
            for id in &changes.removed {
                if let Some(element) = mirror.elements.remove(id) {
                    let element: &web_sys::Element = element.as_ref();
                    element.remove();
                }
                mirror.owners.remove(id);
            }
            // Write every changed node, then put the children of each parent they touched in
            // tree order.
            let mut parents: Vec<NodeId> = Vec::new();
            let mut seen = HashSet::new();
            let mut touch = |id: NodeId, parents: &mut Vec<NodeId>| {
                if seen.insert(id) {
                    parents.push(id);
                }
            };
            let state = mirror.tree.state();
            let root_id = state.root().locate().0;
            let mut created = Vec::new();
            let mut dropped = Vec::new();
            for id in &changes.written {
                let Some(node) = state.node_by_tree_local_id(*id, TreeId::ROOT) else {
                    continue;
                };
                if *id == root_id {
                    touch(*id, &mut parents);
                    continue;
                }
                if has_element(&node) {
                    let element = match mirror.elements.get(id) {
                        Some(element) => element.clone(),
                        None => {
                            let Some(element) = self.create_element(*id) else {
                                continue;
                            };
                            created.push((*id, element.clone()));
                            element
                        }
                    };
                    write(&element, &node, &self.prefix, scale);
                    touch(*id, &mut parents);
                } else if mirror.elements.contains_key(id) {
                    dropped.push(*id);
                }
                if let Some(parent) = element_parent(&node, &mirror.elements, &created) {
                    touch(parent, &mut parents);
                }
                // A node that lost its element hands its children to its own element parent;
                // one that gained one takes them back. Either way the children's element
                // parent is this node's element parent or the node itself, both touched.
                match node.data().author_id() {
                    Some(author) => {
                        mirror.owners.insert(*id, author.to_string());
                    }
                    None => {
                        mirror.owners.remove(id);
                    }
                }
            }
            mirror.elements.extend(created);
            for id in dropped {
                if let Some(element) = mirror.elements.remove(&id) {
                    // Its children move to its element parent when that is reordered.
                    let element: &web_sys::Element = element.as_ref();
                    while let Some(child) = element.first_element_child() {
                        mirror.root.append_child(&child).ok();
                    }
                    element.remove();
                }
            }
            for parent in parents {
                mirror.reorder(parent);
            }
            mirror.refresh_owners(&self.document);
            changes.focus
        };
        if let Some(focus) = focus {
            self.move_focus(&window, focus);
        }
    }

    fn create_element(&self, id: NodeId) -> Option<web_sys::HtmlElement> {
        let element: web_sys::HtmlElement =
            self.document.create_element("div").ok()?.dyn_into().ok()?;
        element.set_id(&format!("{}{}", self.prefix, id.0));
        element
            .set_attribute("data-gpux-node", &id.0.to_string())
            .ok();
        Some(element)
    }

    /// Re-resolves `aria-owns` after the window's `html_element` overlays changed.
    pub(crate) fn refresh_html_owners(&self) {
        let document = self.document.clone();
        self.with_mirror(|mirror| mirror.refresh_owners(&document));
    }

    /// Moves DOM focus to the element of the node GPUI focused.
    fn move_focus(&self, window: &Rc<WebWindowInner>, focus: Option<NodeId>) {
        let target = self.with_mirror(|mirror| {
            let state = mirror.tree.state();
            let node = focus.and_then(|id| state.node_by_tree_local_id(id, TreeId::ROOT))?;
            if node.is_text_input() {
                return Some(Err(node.label()));
            }
            mirror
                .elements
                .get(&node.locate().0)
                .filter(|element| *element != &mirror.root)
                .cloned()
                .map(Ok)
        });
        match target.flatten() {
            Some(Ok(element)) => {
                self.moving_focus.set(true);
                let options = web_sys::FocusOptions::new();
                options.set_prevent_scroll(true);
                element.focus_with_options(&options).ok();
                self.moving_focus.set(false);
            }
            Some(Err(name)) => {
                // A text field is the IME mirror's textarea, which keeps text input on one path.
                window.ime_mirror.set_accessible_name(name.as_deref());
                if !window.ime_mirror.is_focused() {
                    self.moving_focus.set(true);
                    window.focus_ime_mirror();
                    self.moving_focus.set(false);
                }
            }
            None => {
                // Focus left the tree's elements: keyboard input returns to the IME mirror.
                window.ime_mirror.set_accessible_name(None);
                let in_mirror = self.with_mirror(|mirror| {
                    self.document.active_element().is_some_and(|active| {
                        let root: &web_sys::Node = mirror.root.as_ref();
                        root.contains(Some(active.as_ref()))
                    })
                });
                if in_mirror == Some(true) {
                    self.moving_focus.set(true);
                    window.focus_ime_mirror();
                    self.moving_focus.set(false);
                }
            }
        }
    }

    fn listen(self: &Rc<Self>, root: &web_sys::HtmlElement) {
        let target: &web_sys::EventTarget = root.as_ref();
        let mut listeners = self.listeners.borrow_mut();
        let weak = Rc::downgrade(self);
        listeners.push(EventListenerHandle::add(target, "click", move |event| {
            if let Some(this) = weak.upgrade() {
                let event: web_sys::Event = event.unchecked_into();
                if let Some(id) = node_of(&event) {
                    event.prevent_default();
                    this.send(id, Action::Click, None);
                }
            }
        }));
        let weak = Rc::downgrade(self);
        listeners.push(EventListenerHandle::add(target, "focusin", move |event| {
            if let Some(this) = weak.upgrade() {
                let event: web_sys::Event = event.unchecked_into();
                if this.moving_focus.get() {
                    return;
                }
                if let Some(id) = node_of(&event) {
                    this.send(id, Action::Focus, None);
                    if this.with_mirror(|mirror| mirror.clipped(id)) == Some(true) {
                        this.send(id, Action::ScrollIntoView, None);
                    }
                }
            }
        }));
        let weak = Rc::downgrade(self);
        listeners.push(EventListenerHandle::add(target, "keydown", move |event| {
            if let Some(this) = weak.upgrade() {
                let event: web_sys::KeyboardEvent = event.unchecked_into();
                if let Some(id) = node_of(&event)
                    && let Some(action) = this.keyboard_action(id, &event.key())
                {
                    event.prevent_default();
                    event.stop_propagation();
                    this.send(id, action, None);
                    return;
                }
                if let Some(window) = this.window.upgrade() {
                    window.handle_key_down(&event);
                }
            }
        }));
        let weak = Rc::downgrade(self);
        listeners.push(EventListenerHandle::add(target, "keyup", move |event| {
            if let Some(this) = weak.upgrade()
                && let Some(window) = this.window.upgrade()
            {
                window.handle_key_up(&event.unchecked_into());
            }
        }));
    }

    /// The ARIA keyboard interaction of the widget `id` for `key`, as an AccessKit action.
    fn keyboard_action(&self, id: NodeId, key: &str) -> Option<Action> {
        self.with_mirror(|mirror| {
            let node = mirror
                .tree
                .state()
                .node_by_tree_local_id(id, TreeId::ROOT)?;
            let data = node.data();
            let ranged = matches!(
                node.role(),
                Role::Slider | Role::SpinButton | Role::ScrollBar
            );
            if ranged {
                return match key {
                    "ArrowUp" | "ArrowRight" if data.supports_action(Action::Increment) => {
                        Some(Action::Increment)
                    }
                    "ArrowDown" | "ArrowLeft" if data.supports_action(Action::Decrement) => {
                        Some(Action::Decrement)
                    }
                    _ => None,
                };
            }
            let expanded = data.is_expanded()?;
            match (key, expanded) {
                ("Enter" | " " | "ArrowRight", false) if data.supports_action(Action::Expand) => {
                    Some(Action::Expand)
                }
                ("Enter" | " " | "ArrowLeft", true) if data.supports_action(Action::Collapse) => {
                    Some(Action::Collapse)
                }
                _ => None,
            }
        })
        .flatten()
    }

    fn send(&self, target: NodeId, action: Action, data: Option<ActionData>) {
        self.with_mirror(|mirror| {
            mirror.actions += 1;
            mirror
                .root
                .set_attribute("data-gpux-actions", &mirror.actions.to_string())
                .ok();
            mirror
                .root
                .set_attribute("data-gpux-last-action", &format!("{action:?} {}", target.0))
                .ok();
        });
        (self.callbacks.action)(ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: target,
            data,
        });
    }

    /// Removes the mirror and the enable button with their window.
    pub(crate) fn remove(&self) {
        self.listeners.borrow_mut().clear();
        if let Some(button) = self.button.borrow_mut().take() {
            let button: &web_sys::Element = button.as_ref();
            button.remove();
        }
        if let Some(mirror) = self.mirror.borrow_mut().take() {
            let root: &web_sys::Element = mirror.root.as_ref();
            root.remove();
        }
    }
}

/// The node of the mirror element an event targets.
fn node_of(event: &web_sys::Event) -> Option<NodeId> {
    let target: web_sys::Element = event.target()?.dyn_into().ok()?;
    let element = target.closest("[data-gpux-node]").ok()??;
    element
        .get_attribute("data-gpux-node")?
        .parse()
        .ok()
        .map(NodeId)
}

/// The node whose element holds `node`'s element: its nearest ancestor with one.
fn element_parent(
    node: &NodeRef,
    elements: &HashMap<NodeId, web_sys::HtmlElement>,
    created: &[(NodeId, web_sys::HtmlElement)],
) -> Option<NodeId> {
    let mut parent = node.parent();
    while let Some(candidate) = parent {
        let id = candidate.locate().0;
        if candidate.is_root()
            || elements.contains_key(&id)
            || created.iter().any(|(created, _)| *created == id)
        {
            return Some(id);
        }
        parent = candidate.parent();
    }
    None
}

impl Mirror {
    /// Puts the root over the canvas, sized to it.
    fn place_root(&self, window: &WebWindowInner) {
        let canvas: &web_sys::Element = window.canvas.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let style = self.root.style();
        style
            .set_property(
                "transform",
                &format!("translate({}px,{}px)", rect.left(), rect.top()),
            )
            .ok();
        style
            .set_property("width", &format!("{}px", rect.width()))
            .ok();
        style
            .set_property("height", &format!("{}px", rect.height()))
            .ok();
    }

    /// Puts the elements under `parent`'s element in tree order: its children's, and through
    /// generic containers, their descendants'.
    fn reorder(&self, parent: NodeId) {
        let state = self.tree.state();
        let Some(node) = state.node_by_tree_local_id(parent, TreeId::ROOT) else {
            return;
        };
        let Some(element) = self.elements.get(&parent) else {
            return;
        };
        let mut wanted = Vec::new();
        let mut stack: Vec<NodeRef> = node.children().rev().collect();
        while let Some(child) = stack.pop() {
            match self.elements.get(&child.locate().0) {
                Some(child_element) => wanted.push(child_element.clone()),
                None => stack.extend(child.children().rev()),
            }
        }
        let element: &web_sys::Element = element.as_ref();
        let mut cursor = element.first_element_child();
        for child in wanted {
            let child: &web_sys::Element = child.as_ref();
            if cursor.as_ref() == Some(child) {
                cursor = child.next_element_sibling();
            } else {
                let reference: Option<&web_sys::Node> =
                    cursor.as_ref().map(|cursor| cursor.as_ref());
                element.insert_before(child, reference).ok();
            }
        }
    }

    /// Sets `aria-owns` on the elements whose `accessibility_id` names an `html_element`
    /// overlay present in the document, so its DOM is read where the node is.
    fn refresh_owners(&self, document: &web_sys::Document) {
        for (id, author) in &self.owners {
            let Some(element) = self.elements.get(id) else {
                continue;
            };
            let owned = crate::html_element::overlay_dom_id(author);
            let present = document.get_element_by_id(&owned).is_some();
            attribute(element, "aria-owns", present.then_some(owned.as_str()));
        }
    }

    /// Whether `id`'s bounds leave a container around it that clips its children: a node a
    /// scroll container has scrolled out of view.
    fn clipped(&self, id: NodeId) -> bool {
        let state = self.tree.state();
        let Some(node) = state.node_by_tree_local_id(id, TreeId::ROOT) else {
            return false;
        };
        let Some(bounds) = node.bounding_box() else {
            return false;
        };
        let mut parent = node.parent();
        while let Some(ancestor) = parent {
            if ancestor.clips_children()
                && let Some(port) = ancestor.bounding_box()
                && (bounds.x0 < port.x0 - 0.5
                    || bounds.y0 < port.y0 - 0.5
                    || bounds.x1 > port.x1 + 0.5
                    || bounds.y1 > port.y1 + 0.5)
            {
                return true;
            }
            parent = ancestor.parent();
        }
        false
    }
}

/// Writes `node` onto its element: role, name, value, states, relations and box.
fn write(element: &web_sys::HtmlElement, node: &NodeRef, prefix: &str, scale: f64) {
    let data = node.data();
    let role = node.role();
    let dom = |ids: &[NodeId]| {
        (!ids.is_empty()).then(|| {
            ids.iter()
                .map(|id| format!("{prefix}{}", id.0))
                .collect::<Vec<_>>()
                .join(" ")
        })
    };
    let el: &web_sys::Element = element.as_ref();
    style(element, ELEMENT_STYLE);
    let css = element.style();
    let rect = node.bounding_box().unwrap_or_default();
    for (property, value) in [
        ("left", rect.x0 / scale),
        ("top", rect.y0 / scale),
        ("width", (rect.x1 - rect.x0) / scale),
        ("height", (rect.y1 - rect.y0) / scale),
    ] {
        css.set_property(property, &format!("{value}px")).ok();
    }

    attribute(el, "role", gpux::aria_role(role));
    let text_role = matches!(role, Role::Label | Role::TextRun);
    let text_input = node.is_text_input();
    let name = (!text_role).then(|| node.label()).flatten();
    attribute(el, "aria-label", name.as_deref());
    attribute(el, "aria-description", node.description().as_deref());
    // Text and text fields carry their text as content, with no element children of their own.
    if (text_role || text_input) && node.children().next().is_none() {
        let text = node
            .value()
            .or_else(|| text_role.then(|| node.label()).flatten());
        if element.text_content() != text {
            element.set_text_content(text.as_deref());
        }
    }
    let ranged = matches!(
        role,
        Role::Slider | Role::SpinButton | Role::ScrollBar | Role::ProgressIndicator | Role::Meter
    );
    let numeric = |value: Option<f64>| value.filter(|_| ranged).map(number);
    attribute(
        el,
        "aria-valuenow",
        numeric(data.numeric_value()).as_deref(),
    );
    attribute(
        el,
        "aria-valuemin",
        numeric(data.min_numeric_value()).as_deref(),
    );
    attribute(
        el,
        "aria-valuemax",
        numeric(data.max_numeric_value()).as_deref(),
    );
    attribute(el, "aria-valuetext", data.value().filter(|_| ranged));
    let toggled = data.toggled().map(|toggled| match toggled {
        Toggled::True => "true",
        Toggled::False => "false",
        Toggled::Mixed => "mixed",
    });
    let pressed = matches!(role, Role::Button | Role::DefaultButton);
    attribute(el, "aria-checked", toggled.filter(|_| !pressed));
    attribute(el, "aria-pressed", toggled.filter(|_| pressed));
    let flag = |value: bool| value.then_some("true");
    attribute(
        el,
        "aria-expanded",
        data.is_expanded().map(|e| if e { "true" } else { "false" }),
    );
    attribute(
        el,
        "aria-selected",
        data.is_selected().map(|s| if s { "true" } else { "false" }),
    );
    attribute(el, "aria-disabled", flag(data.is_disabled()));
    attribute(el, "aria-required", flag(data.is_required()));
    attribute(el, "aria-readonly", flag(data.is_read_only()));
    attribute(el, "aria-hidden", flag(node.is_hidden()));
    attribute(el, "aria-multiline", flag(role == Role::MultilineTextInput));
    attribute(
        el,
        "aria-invalid",
        data.invalid().map(|invalid| match invalid {
            Invalid::True => "true",
            Invalid::Grammar => "grammar",
            Invalid::Spelling => "spelling",
        }),
    );
    attribute(el, "aria-placeholder", data.placeholder());
    attribute(
        el,
        "aria-orientation",
        data.orientation().map(|orientation| match orientation {
            Orientation::Horizontal => "horizontal",
            Orientation::Vertical => "vertical",
        }),
    );
    let count = |value: Option<usize>| value.map(|value| value.to_string());
    // AccessKit's indices start at 0, ARIA's at 1.
    let index = |value: Option<usize>| value.map(|value| (value + 1).to_string());
    attribute(el, "aria-level", count(data.level()).as_deref());
    attribute(
        el,
        "aria-posinset",
        count(data.position_in_set()).as_deref(),
    );
    attribute(el, "aria-setsize", count(data.size_of_set()).as_deref());
    attribute(el, "aria-rowindex", index(data.row_index()).as_deref());
    attribute(el, "aria-colindex", index(data.column_index()).as_deref());
    attribute(el, "aria-rowcount", count(data.row_count()).as_deref());
    attribute(el, "aria-colcount", count(data.column_count()).as_deref());
    attribute(
        el,
        "aria-live",
        match node.live() {
            Live::Polite => Some("polite"),
            Live::Assertive => Some("assertive"),
            Live::Off => None,
        },
    );
    attribute(el, "aria-labelledby", dom(data.labelled_by()).as_deref());
    attribute(el, "aria-describedby", dom(data.described_by()).as_deref());
    attribute(el, "aria-controls", dom(data.controls()).as_deref());
    attribute(
        el,
        "aria-errormessage",
        data.error_message()
            .map(|id| format!("{prefix}{}", id.0))
            .as_deref(),
    );
    attribute(
        el,
        "aria-activedescendant",
        data.active_descendant()
            .map(|id| format!("{prefix}{}", id.0))
            .as_deref(),
    );
    attribute(
        el,
        "tabindex",
        data.supports_action(Action::Focus).then_some("-1"),
    );
}
