use crate::{
    AnyElement, AnyEntity, AnyWeakEntity, App, AvailableSpace, Bounds, ContentMask, Context,
    Element, ElementId, ElementPaintContext, Entity, EntityId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, PaintIndex, Pixels, PrepaintStateIndex, Render, RenderOnce, Size, Style,
    StyleRefinement, TextStyle, WeakEntity,
};
use crate::{Empty, Window};
use anyhow::Result;
use collections::FxHashSet;
use refineable::Refineable;
use std::mem;
use std::{
    any::{TypeId, type_name},
    fmt,
    ops::Range,
};

/// A dynamically-typed view handle that can be downcast to a specific `Entity<V>`.
///
/// This is the type-erased counterpart to [`ViewElement`]: it holds an entity plus
/// a function pointer to its render, and is itself a [`View`], so embedding it as an
/// element goes through the same [`ViewElement`] machinery as any other view.
#[derive(Clone, Debug)]
pub struct AnyView {
    entity: AnyEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
}

impl<V: Render> From<Entity<V>> for AnyView {
    fn from(value: Entity<V>) -> Self {
        AnyView {
            entity: value.into_any(),
            render: any_view::render::<V>,
        }
    }
}

impl AnyView {
    /// Embed this view as a cached [`ViewElement`] laid out at `style`.
    ///
    /// The rendered subtree is recycled from the previous frame unless
    /// [Context::notify] was called on the backing entity since it was rendered
    /// (or [Window::refresh] is called, which ignores caching).
    pub fn cached(self, style: StyleRefinement) -> ViewElement<AnyView> {
        ViewElement::new(self).cached(style)
    }

    /// Convert this to a weak handle.
    pub fn downgrade(&self) -> AnyWeakView {
        AnyWeakView {
            entity: self.entity.downgrade(),
            render: self.render,
        }
    }

    /// Convert this to a [Entity] of a specific type.
    /// If this handle does not contain a view of the specified type, returns itself in an `Err` variant.
    pub fn downcast<T: 'static>(self) -> Result<Entity<T>, Self> {
        match self.entity.downcast() {
            Ok(entity) => Ok(entity),
            Err(entity) => Err(Self {
                entity,
                render: self.render,
            }),
        }
    }

    /// Gets the [TypeId] of the underlying view.
    pub fn entity_type(&self) -> TypeId {
        self.entity.entity_type
    }

    /// The [`EntityId`] of this view.
    pub fn entity_id(&self) -> EntityId {
        self.entity.entity_id()
    }
}

impl PartialEq for AnyView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl Eq for AnyView {}

/// `AnyView` is the type-erased [`View`]: its `render` is a function pointer rather
/// than a concrete type, but it participates in the reactive graph exactly like any
/// other view via [`ViewElement`].
impl View for AnyView {
    fn entity_id(&self) -> Option<EntityId> {
        Some(self.entity.entity_id())
    }

    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        (self.render)(&self, window, cx)
    }
}

impl<V: 'static + Render> IntoElement for Entity<V> {
    type Element = ViewElement<Entity<V>>;

    fn into_element(self) -> Self::Element {
        ViewElement::new(self)
    }

    #[inline(never)]
    fn into_any_element(self) -> AnyElement {
        self.into_element().into_any()
    }
}

impl IntoElement for AnyView {
    type Element = ViewElement<AnyView>;

    fn into_element(self) -> Self::Element {
        ViewElement::new(self)
    }
}

/// A weak, dynamically-typed view handle.
pub struct AnyWeakView {
    entity: AnyWeakEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
}

impl AnyWeakView {
    /// Upgrade to a strong `AnyView` handle, if the view is still alive.
    pub fn upgrade(&self) -> Option<AnyView> {
        let entity = self.entity.upgrade()?;
        Some(AnyView {
            entity,
            render: self.render,
        })
    }
}

impl<V: 'static + Render> From<WeakEntity<V>> for AnyWeakView {
    fn from(view: WeakEntity<V>) -> Self {
        AnyWeakView {
            entity: view.into(),
            render: any_view::render::<V>,
        }
    }
}

impl PartialEq for AnyWeakView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl std::fmt::Debug for AnyWeakView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnyWeakView")
            .field("entity_id", &self.entity.entity_id)
            .finish_non_exhaustive()
    }
}

mod any_view {
    use crate::{AnyElement, AnyView, App, IntoElement, Render, Window};

    pub(crate) fn render<V: 'static + Render>(
        view: &AnyView,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let view = view.clone().downcast::<V>().unwrap();
        // Record the view's Render type name so the accessibility debug dump can
        // attribute nodes to the view that produced them.
        #[cfg(debug_assertions)]
        window
            .a11y
            .view_type_names
            .insert(view.entity_id(), std::any::type_name::<V>());
        #[cfg(feature = "diagnostics")]
        crate::fast::verify::retention::note_view_type(
            window,
            view.entity_id(),
            std::any::type_name::<V>(),
        );
        crate::fast::dependencies::render_next(cx, view.entity_id());
        view.update(cx, |view, cx| view.render(window, cx).into_any_element())
    }
}

/// A renderable that participates in GPUI's reactive graph — the unifying model
/// behind [`Render`] and [`RenderOnce`].
///
/// When `entity_id()` returns `Some`, that id becomes the view's identity: it gets
/// a unique element-id space (so internal `use_state` / `.id(..)` never collide
/// across siblings) and `cx.notify()` on that entity re-renders only this view's
/// subtree. `None` behaves like a stateless component.
///
/// You rarely implement `View` directly. `Entity<T: Render>` and any `T: RenderOnce`
/// get a blanket impl below; implement it by hand only when a component needs both
/// parent-supplied props *and* a backing entity for identity.
pub trait View: 'static + Sized {
    /// This view's identity, if it has one. A view typically holds the backing
    /// entity as a field and returns its [`EntityId`] here.
    ///
    /// The id becomes this view's [`ElementId`], so two views keyed on the same
    /// entity must not be rendered at the same position in the element tree
    /// (e.g. as siblings under the same parent): their internal element state
    /// (`use_state`, scroll offsets, etc.) would silently collide. Nesting is
    /// fine — the id is scoped by the parent path.
    fn entity_id(&self) -> Option<EntityId>;

    /// Render this view into an element tree, consuming `self`.
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement;
}

/// A stateless component (`RenderOnce`) is a `View` with no identity.
impl<T: RenderOnce> View for T {
    fn entity_id(&self) -> Option<EntityId> {
        None
    }

    #[inline]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        RenderOnce::render(self, window, cx)
    }
}

/// An entity that renders itself (`Render`) is a `View` keyed on its own id.
impl<T: Render> View for Entity<T> {
    fn entity_id(&self) -> Option<EntityId> {
        Some(Entity::entity_id(self))
    }

    #[inline]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        #[cfg(feature = "diagnostics")]
        crate::fast::verify::retention::note_view_type(
            window,
            self.entity_id(),
            std::any::type_name::<T>(),
        );
        crate::fast::dependencies::render_next(cx, self.entity_id());
        self.update(cx, |this, cx| {
            Render::render(this, window, cx).into_any_element()
        })
    }
}

impl<T: Render> Entity<T> {
    /// Embed this entity as a cached [`ViewElement`] laid out at `style`.
    ///
    /// The rendered subtree is reused until the entity is notified (or the
    /// cached bounds / text style change). Caching requires a definite size:
    /// a cached view is laid out from `style` and is *not* measured from its
    /// contents. Use [`ViewElement::new`] (or `.child(entity)`) for the
    /// uncached case.
    #[track_caller]
    pub fn cached(self, style: StyleRefinement) -> ViewElement<Entity<T>> {
        ViewElement::new(self).cached(style)
    }
}

/// The element type for [`View`] implementations. Wraps a `View` and hooks it
/// into layout, prepaint, and paint. Constructed via [`ViewElement::new`].
#[doc(hidden)]
pub struct ViewElement<V: View> {
    view: Option<V>,
    entity_id: Option<EntityId>,
    cached_style: Option<StyleRefinement>,
    #[cfg(debug_assertions)]
    source: &'static core::panic::Location<'static>,
    /// How the view was drawn with view retention on.
    fast_retention: crate::fast::retained::ViewRetention,
}

impl<V: View> ViewElement<V> {
    /// Wrap a [`View`] as an element.
    #[track_caller]
    pub fn new(view: V) -> Self {
        let entity_id = view.entity_id();
        ViewElement {
            entity_id,
            cached_style: None,
            view: Some(view),
            #[cfg(debug_assertions)]
            source: core::panic::Location::caller(),
            fast_retention: Default::default(),
        }
    }

    /// Enable caching of this view's rendered subtree, laid out at `style`.
    /// The composer supplies the layout style because caching skips rendering
    /// the contents to measure them.
    ///
    /// Crate-private on purpose: caching is only sound for entity-backed views,
    /// where [`Context::notify`] is the contract that busts the cache. A stateless
    /// view has no such contract, so a frozen subtree could never be invalidated.
    /// Reach this through [`Entity::cached`] or [`AnyView::cached`], which are
    /// entity-backed by construction.
    pub(crate) fn cached(mut self, style: StyleRefinement) -> Self {
        self.cached_style = Some(style);
        self
    }
}

impl<V: View> IntoElement for ViewElement<V> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub(crate) struct ViewElementState {
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
    cache_key: ViewElementCacheKey,
    accessed_entities: FxHashSet<EntityId>,
    /// The hovers the view was drawn by, nested views included: it is
    /// replayed only while each is as it was.
    hover_reads: Vec<(crate::HitboxId, bool)>,
    /// Where the view's hovers can change: a view with hovers is not
    /// replayed while the pointer is there.
    hover_area: crate::fast::dependencies::HoverArea,
}

struct ViewElementCacheKey {
    bounds: Bounds<Pixels>,
    content_mask: ContentMask<Pixels>,
    text_style: TextStyle,
    /// The opacity and transform the cached primitives were painted with.
    paint_context: ElementPaintContext,
}

impl<V: View> Element for ViewElement<V> {
    type RequestLayoutState = Option<AnyElement>;
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        self.entity_id.map(ElementId::View)
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        #[cfg(debug_assertions)]
        return Some(self.source);

        #[cfg(not(debug_assertions))]
        return None;
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        if let Some(entity_id) = self.entity_id
            && crate::fast::retained::active(window)
        {
            let view = &mut self.view;
            let layout_id = crate::fast::retained::request_view_layout(
                &mut self.fast_retention,
                entity_id,
                self.cached_style.as_ref(),
                id,
                window,
                cx,
                &mut |window, cx| view.take().unwrap().render(window, cx).into_any_element(),
            );
            (layout_id, None)
        } else if let Some(entity_id) = self.entity_id {
            // Stateful path: create a reactive boundary.
            let view = &mut self.view;
            request_layout_view(
                entity_id,
                self.cached_style.as_ref(),
                window,
                cx,
                &mut |window, cx| view.take().unwrap().render(window, cx).into_any_element(),
            )
        } else {
            // Stateless path: isolate subtree via type name (no entity identity).
            request_layout_component(type_name::<V>(), window, cx, &mut |window, cx| {
                self.view
                    .take()
                    .unwrap()
                    .render(window, cx)
                    .into_any_element()
            })
        }
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        if let Some(entity_id) = self.entity_id
            && crate::fast::retained::prepaint_view(
                &mut self.fast_retention,
                entity_id,
                global_id,
                bounds,
                window,
                cx,
                &mut |window, cx| {
                    self.view
                        .take()
                        .unwrap()
                        .render(window, cx)
                        .into_any_element()
                },
            )
        {
            None
        } else if let Some(entity_id) = self.entity_id {
            // Stateful path.
            prepaint_view(
                entity_id,
                global_id,
                bounds,
                element,
                window,
                cx,
                &mut |window, cx| {
                    self.view
                        .take()
                        .unwrap()
                        .render(window, cx)
                        .into_any_element()
                },
            )
        } else {
            // Stateless path: just prepaint the element.
            prepaint_component(type_name::<V>(), element, window, cx)
        }
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        element: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(entity_id) = self.entity_id
            && crate::fast::retained::paint_view(&mut self.fast_retention, entity_id, window, cx)
        {
        } else if let Some(entity_id) = self.entity_id {
            // Stateful path.
            paint_view(
                entity_id,
                self.cached_style.is_some(),
                global_id,
                element,
                window,
                cx,
            );
        } else {
            // Stateless path: just paint the element.
            paint_component(std::any::type_name::<V>(), element, window, cx);
        }
    }
}

/// A view that renders nothing
pub struct EmptyView;

impl Render for EmptyView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[inline(never)]
fn request_layout_view(
    entity_id: EntityId,
    cached_style: Option<&StyleRefinement>,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> (LayoutId, Option<AnyElement>) {
    window.with_rendered_view(entity_id, |window| {
        let caching_disabled = window.is_inspector_picking(cx);
        match cached_style {
            Some(style) if !caching_disabled => {
                let mut root_style = Style::default();
                root_style.refine(style);
                let layout_id = window.request_layout(root_style, None, cx);
                (layout_id, None)
            }
            _ => {
                crate::fast::fast_stat!(window.fast_stats.frame, views_built += 1);
                let mut element = render(window, cx);
                let layout_id = element.request_layout(window, cx);
                (layout_id, Some(element))
            }
        }
    })
}

#[inline(never)]
fn request_layout_component(
    name: &'static str,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> (LayoutId, Option<AnyElement>) {
    window.with_id(ElementId::from(name), |window| {
        let mut element = render(window, cx);
        let layout_id = element.request_layout(window, cx);
        (layout_id, Some(element))
    })
}

#[inline(never)]
fn prepaint_view(
    entity_id: EntityId,
    global_id: Option<&GlobalElementId>,
    bounds: Bounds<Pixels>,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> Option<AnyElement> {
    window.set_view_id(entity_id);
    window.with_rendered_view(entity_id, |window| {
        if let Some(mut element) = element.take() {
            element.prepaint(window, cx);
            return Some(element);
        }

        window.with_element_state::<ViewElementState, _>(
            global_id.unwrap(),
            |element_state, window| {
                let content_mask = window.content_mask();
                let text_style = window.text_style();
                let paint_context = window.element_paint_context();

                if let Some(mut element_state) = element_state
                    && element_state.cache_key.bounds == bounds
                    && element_state.cache_key.content_mask == content_mask
                    && element_state.cache_key.text_style == text_style
                    && element_state.cache_key.paint_context == paint_context
                    && crate::fast::dependencies::may_replay_hovers(
                        window,
                        &element_state.hover_reads,
                        element_state.hover_area,
                    )
                    && !window.dirty_views.contains(&entity_id)
                    && !window.refreshing
                    // Accessibility nodes and action listeners are rebuilt
                    // each frame; cached visual replay does not retain them.
                    && !window.is_a11y_active()
                {
                    let prepaint_start = window.prepaint_index();
                    crate::fast::fast_stat!(window.fast_stats.frame, cached_views_replayed += 1);
                    window.reuse_prepaint(element_state.prepaint_range.clone());
                    cx.entities
                        .extend_accessed(&element_state.accessed_entities);
                    let prepaint_end = window.prepaint_index();
                    element_state.prepaint_range = prepaint_start..prepaint_end;

                    return (None, element_state);
                }

                let refreshing = mem::replace(&mut window.refreshing, true);
                let prepaint_start = window.prepaint_index();
                crate::fast::fast_stat!(window.fast_stats.frame, views_built += 1);
                let hovers_start = crate::fast::dependencies::begin_recording(window);
                let (element, accessed_entities) = cx.detect_accessed_entities(|cx| {
                    let mut element = render(window, cx);
                    element.layout_as_root(Size::<AvailableSpace>::from(bounds.size), window, cx);
                    element.prepaint_at(bounds.origin, window, cx);
                    element
                });
                let hover_reads = crate::fast::dependencies::finish_recording(window, hovers_start);

                let prepaint_end = window.prepaint_index();
                let hover_area = crate::fast::dependencies::hover_area(
                    window,
                    &hover_reads,
                    &[prepaint_start.hitboxes_index..prepaint_end.hitboxes_index],
                );
                window.refreshing = refreshing;

                (
                    Some(element),
                    ViewElementState {
                        accessed_entities,
                        hover_reads,
                        hover_area,
                        prepaint_range: prepaint_start..prepaint_end,
                        paint_range: PaintIndex::default()..PaintIndex::default(),
                        cache_key: ViewElementCacheKey {
                            bounds,
                            content_mask,
                            text_style,
                            paint_context,
                        },
                    },
                )
            },
        )
    })
}

#[inline(never)]
fn prepaint_component(
    name: &'static str,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    window.with_id(ElementId::from(name), |window| {
        element.as_mut().unwrap().prepaint(window, cx);
    });
    Some(element.take().unwrap())
}

#[inline(never)]
fn paint_view(
    entity_id: EntityId,
    cached: bool,
    global_id: Option<&GlobalElementId>,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) {
    window.with_rendered_view(entity_id, |window| {
        let caching_disabled = window.is_inspector_picking(cx);
        if cached && !caching_disabled {
            window.with_element_state::<ViewElementState, _>(
                global_id.unwrap(),
                |element_state, window| {
                    let mut element_state = element_state.unwrap();

                    let paint_start = window.paint_index();
                    let paint_context = window.element_paint_context();

                    if let Some(element) = element {
                        let refreshing = mem::replace(&mut window.refreshing, true);
                        let hovers_start = crate::fast::dependencies::begin_recording(window);
                        element.paint(window, cx);
                        let painted_hovers =
                            crate::fast::dependencies::finish_recording(window, hovers_start);
                        if !painted_hovers.is_empty() {
                            element_state.hover_reads.extend(painted_hovers);
                            let prepaint = &element_state.prepaint_range;
                            element_state.hover_area = crate::fast::dependencies::hover_area(
                                window,
                                &element_state.hover_reads,
                                &[prepaint.start.hitboxes_index..prepaint.end.hitboxes_index],
                            );
                        }
                        window.refreshing = refreshing;
                        // An ancestor that applies opacity or a transform only while
                        // painting hides it from prepaint. Keying on the painted context
                        // makes the view render again under such an ancestor.
                        element_state.cache_key.paint_context = paint_context;
                    } else {
                        window.reuse_paint(element_state.paint_range.clone());
                        crate::fast::dependencies::note_replayed_hovers(
                            window,
                            &element_state.hover_reads,
                        );
                        // Prepaint replayed it only with the pointer away from its
                        // hitboxes, where nothing this frame adds can hover them.
                        debug_assert!(
                            crate::fast::dependencies::hovers_unchanged(
                                window,
                                &element_state.hover_reads,
                            ),
                            "a replayed cached view's hovers changed after prepaint"
                        );
                        // An ancestor that applies opacity or a transform only while
                        // painting can change after prepaint replayed this view, so
                        // this frame shows stale primitives: render it next frame.
                        if element_state.cache_key.paint_context != paint_context {
                            window.on_next_frame(move |_, cx| cx.notify(entity_id));
                        }
                    }

                    let paint_end = window.paint_index();
                    element_state.paint_range = paint_start..paint_end;

                    ((), element_state)
                },
            )
        } else {
            element.as_mut().unwrap().paint(window, cx);
        }
    });
}

#[inline(never)]
fn paint_component(
    name: &'static str,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) {
    window.with_id(ElementId::Name(name.into()), |window| {
        element.as_mut().unwrap().paint(window, cx);
    });
}

#[cfg(test)]
mod cached_view_replay_tests {
    use crate::{
        AnyElement, App, Bounds, Context, Element, Entity, GlobalElementId, InspectorElementId,
        LayoutId, Modifiers, Pixels, Quad, Style, StyleRefinement, TestAppContext,
        TransformationMatrix, VisualTestContext, Window, div, point, prelude::*, px, red, size,
    };
    use std::{cell::Cell, rc::Rc};

    struct HitTestTop {
        clicks: Rc<Cell<usize>>,
        renders: Rc<Cell<usize>>,
    }

    impl Render for HitTestTop {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let clicks = self.clicks.clone();
            div()
                .id("top")
                .size_full()
                .occlude()
                .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
        }
    }

    struct HitTestRoot {
        top: Entity<HitTestTop>,
        below_clicks: Rc<Cell<usize>>,
    }

    impl Render for HitTestRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let below_clicks = self.below_clicks.clone();
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("below")
                        .absolute()
                        .size_full()
                        .occlude()
                        .on_click(move |_, _, _| below_clicks.set(below_clicks.get() + 1)),
                )
                .child(
                    self.top
                        .clone()
                        .cached(StyleRefinement::default().absolute().size_full()),
                )
        }
    }

    /// A cached view replayed above a sibling that re-rendered this frame keeps
    /// its hitboxes from the previous frame, so it must still win the hit test.
    #[gpux::test]
    fn reused_cached_view_stays_above_earlier_rerendered_sibling(cx: &mut TestAppContext) {
        let top_clicks = Rc::new(Cell::new(0));
        let top_renders = Rc::new(Cell::new(0));
        let below_clicks = Rc::new(Cell::new(0));
        let (root, cx) = cx.add_window_view(|_, cx| HitTestRoot {
            top: cx.new(|_| HitTestTop {
                clicks: top_clicks.clone(),
                renders: top_renders.clone(),
            }),
            below_clicks: below_clicks.clone(),
        });
        let position = point(px(50.), px(50.));

        cx.simulate_click(position, Modifiers::none());
        assert_eq!((top_clicks.get(), below_clicks.get()), (1, 0));

        // Re-render only the root: the sibling below gets fresh hitboxes while
        // the cached view on top is replayed.
        let rendered = top_renders.get();
        root.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(top_renders.get(), rendered, "the top view must be replayed");

        cx.simulate_click(position, Modifiers::none());
        assert_eq!((top_clicks.get(), below_clicks.get()), (2, 0));
    }

    struct PaintContextChild {
        renders: Rc<Cell<usize>>,
    }

    impl Render for PaintContextChild {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div().size_full().bg(red())
        }
    }

    struct PaintContextRoot {
        child: Entity<PaintContextChild>,
        opacity: f32,
        scale: f32,
    }

    impl Render for PaintContextRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .opacity(self.opacity)
                .scale(self.scale)
                .child(
                    self.child
                        .clone()
                        .cached(StyleRefinement::default().size_full()),
                )
        }
    }

    fn open_paint_context_window(
        cx: &mut TestAppContext,
    ) -> (
        Entity<PaintContextRoot>,
        Rc<Cell<usize>>,
        &mut VisualTestContext,
    ) {
        let renders = Rc::new(Cell::new(0));
        let (root, cx) = cx.add_window_view(|_, cx| PaintContextRoot {
            child: cx.new(|_| PaintContextChild {
                renders: renders.clone(),
            }),
            opacity: 1.,
            scale: 1.,
        });
        (root, renders, cx)
    }

    fn painted_child_quad(cx: &mut VisualTestContext) -> Quad {
        cx.update(|window, _| {
            let quads = window.painted_quads();
            assert_eq!(quads.len(), 1, "only the cached child paints a quad");
            quads[0].clone()
        })
    }

    /// Paints the current frame again from scratch and returns its child quad.
    fn refreshed_child_quad(cx: &mut VisualTestContext) -> Quad {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        painted_child_quad(cx)
    }

    fn assert_same_paint_context(replayed: &Quad, refreshed: &Quad) {
        assert_eq!(
            format!("{:?}", replayed.background),
            format!("{:?}", refreshed.background),
            "replayed opacity must match a refreshed frame"
        );
        assert_eq!(
            replayed.transformation, refreshed.transformation,
            "replayed transform must match a refreshed frame"
        );
    }

    #[gpux::test]
    fn cached_view_replays_while_ancestor_paint_context_is_unchanged(cx: &mut TestAppContext) {
        let (root, renders, cx) = open_paint_context_window(cx);
        root.update(cx, |root, cx| {
            root.opacity = 0.5;
            root.scale = 2.;
            cx.notify();
        });
        cx.run_until_parked();
        let rendered = renders.get();

        root.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(renders.get(), rendered, "an unchanged context replays");
        let replayed = painted_child_quad(cx);
        assert_same_paint_context(&replayed, &refreshed_child_quad(cx));
    }

    #[gpux::test]
    fn cached_view_follows_ancestor_opacity(cx: &mut TestAppContext) {
        let (root, _, cx) = open_paint_context_window(cx);
        root.update(cx, |root, cx| {
            root.opacity = 0.5;
            cx.notify();
        });
        cx.run_until_parked();
        let replayed = painted_child_quad(cx);
        assert_same_paint_context(&replayed, &refreshed_child_quad(cx));
    }

    #[gpux::test]
    fn cached_view_follows_ancestor_transform(cx: &mut TestAppContext) {
        let (root, _, cx) = open_paint_context_window(cx);
        root.update(cx, |root, cx| {
            root.scale = 2.;
            cx.notify();
        });
        cx.run_until_parked();
        let replayed = painted_child_quad(cx);
        assert_same_paint_context(&replayed, &refreshed_child_quad(cx));
    }

    /// Transforms its child only while painting, like a zoomable viewer that
    /// leaves layout and hit testing alone.
    struct PaintOnlyTransform {
        transformation: Option<TransformationMatrix>,
        child: AnyElement,
    }

    impl IntoElement for PaintOnlyTransform {
        type Element = Self;

        fn into_element(self) -> Self {
            self
        }
    }

    impl Element for PaintOnlyTransform {
        type RequestLayoutState = ();
        type PrepaintState = ();

        fn id(&self) -> Option<crate::ElementId> {
            None
        }

        fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
            None
        }

        fn request_layout(
            &mut self,
            _: Option<&GlobalElementId>,
            _: Option<&InspectorElementId>,
            window: &mut Window,
            cx: &mut App,
        ) -> (LayoutId, ()) {
            let child = self.child.request_layout(window, cx);
            let mut style = Style::default();
            style.size = size(px(100.).into(), px(100.).into());
            (window.request_layout(style, [child], cx), ())
        }

        fn prepaint(
            &mut self,
            _: Option<&GlobalElementId>,
            _: Option<&InspectorElementId>,
            _: Bounds<Pixels>,
            _: &mut (),
            window: &mut Window,
            cx: &mut App,
        ) {
            self.child.prepaint(window, cx);
        }

        fn paint(
            &mut self,
            _: Option<&GlobalElementId>,
            _: Option<&InspectorElementId>,
            _: Bounds<Pixels>,
            _: &mut (),
            _: &mut (),
            window: &mut Window,
            cx: &mut App,
        ) {
            window
                .with_element_transform(self.transformation, |window| self.child.paint(window, cx));
        }
    }

    struct PaintOnlyTransformRoot {
        child: Entity<PaintContextChild>,
        transformation: Option<TransformationMatrix>,
    }

    impl Render for PaintOnlyTransformRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            PaintOnlyTransform {
                transformation: self.transformation,
                child: self
                    .child
                    .clone()
                    .cached(StyleRefinement::default().size_full())
                    .into_any_element(),
            }
        }
    }

    /// A transform applied only while painting is invisible to prepaint, so the
    /// frame that first applies it replays stale primitives; the cached view must
    /// then render on the next frame and never replay under that transform.
    #[gpux::test]
    fn cached_view_follows_paint_only_ancestor_transform(cx: &mut TestAppContext) {
        let renders = Rc::new(Cell::new(0));
        let (root, cx) = cx.add_window_view(|_, cx| PaintOnlyTransformRoot {
            child: cx.new(|_| PaintContextChild {
                renders: renders.clone(),
            }),
            transformation: None,
        });
        root.update(cx, |root, cx| {
            root.transformation = Some(TransformationMatrix::unit().scale(size(2., 2.)));
            cx.notify();
        });
        cx.run_until_parked();
        let scheduled = cx.update(|window, cx| window.simulate_next_frame(cx));
        assert!(scheduled > 0, "a stale replay schedules a render");
        cx.run_until_parked();
        let rendered = painted_child_quad(cx);
        assert_same_paint_context(&rendered, &refreshed_child_quad(cx));

        let rendered = renders.get();
        root.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(renders.get(), rendered + 1, "no replay under the transform");
        let rendered = painted_child_quad(cx);
        assert_same_paint_context(&rendered, &refreshed_child_quad(cx));
    }
}
