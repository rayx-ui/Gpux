//! What a reusable subtree read while it was drawn, so it is drawn again only
//! while what it read is unchanged.
//!
//! Adapted from gpui-fast's `fast/dependencies.rs` and the hover reads of
//! `fast/retained.rs` (`note_hover_read`, `hovers_unchanged`).
//!
//! Hovers come first: gpux's cached views (`AnyView::cached`) read them
//! through hover styles, and a cached view painted while hovered must not be
//! replayed once the pointer has left it, whether a mouse move, a wheel
//! event, or a press moved the pointer, nor while an overlay this frame adds
//! may cover it, nor at all once it read the hover of a hitbox it did not
//! insert.
//!
//! The rest records, for view retention, everything else a subtree reads
//! while it is built: the entities it accesses, the globals it reads (or
//! only asks the presence of), the versions of the scroll and list state its
//! elements use, and the window's ambient input (the pointer, the modifier
//! keys, the pressed button). Changes are stamped as they happen, so a
//! recording can tell later whether anything it read has changed since.

use std::{
    any::TypeId,
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
};

use collections::{FxHashMap, FxHashSet, TypeIdHashMap};
use smallvec::SmallVec;

use crate::{App, Bounds, Context, Entity, EntityId, HitboxId, Pixels, Window};

/// The hovers read while reusable subtrees were being drawn.
#[derive(Default)]
pub(crate) struct HoverReads {
    /// How many recordings are open, innermost last.
    depth: Cell<usize>,
    /// Every hover read since the outermost recording opened, in order:
    /// a nested recording's reads stay for the recordings around it.
    reads: RefCell<Vec<(HitboxId, bool)>>,
}

/// Notes that whatever is being drawn asked whether `hitbox` is hovered and
/// got `hovered`.
#[inline(always)]
pub(crate) fn note_hover_read(window: &Window, hitbox: HitboxId, hovered: bool) {
    let hovers = &window.fast_hover_reads;
    if hovers.depth.get() > 0 {
        hovers.reads.borrow_mut().push((hitbox, hovered));
    }
}

/// Starts recording the hovers read from here on; returns where they start.
pub(crate) fn begin_recording(window: &Window) -> usize {
    let hovers = &window.fast_hover_reads;
    hovers.depth.set(hovers.depth.get() + 1);
    hovers.reads.borrow().len()
}

/// Ends the recording [`begin_recording`] started at `start` and returns the
/// hovers read since. They stay recorded for the recordings around it.
pub(crate) fn finish_recording(window: &Window, start: usize) -> Vec<(HitboxId, bool)> {
    let hovers = &window.fast_hover_reads;
    let depth = hovers.depth.get().saturating_sub(1);
    hovers.depth.set(depth);
    let mut reads = hovers.reads.borrow_mut();
    let recorded = reads.get(start..).map(<[_]>::to_vec).unwrap_or_default();
    if depth == 0 {
        reads.clear();
    }
    recorded
}

/// Adds the hovers a subtree drawn again from its last frame was drawn by to
/// the recordings around it, which depend on them too.
pub(crate) fn note_replayed_hovers(window: &Window, reads: &[(HitboxId, bool)]) {
    let hovers = &window.fast_hover_reads;
    if hovers.depth.get() > 0 && !reads.is_empty() {
        hovers.reads.borrow_mut().extend_from_slice(reads);
    }
}

/// Whether every hover in `reads` is still as it was read, against the
/// window's current hit test.
pub(crate) fn hovers_unchanged(window: &Window, reads: &[(HitboxId, bool)]) -> bool {
    reads
        .iter()
        .all(|(hitbox, hovered)| hitbox.hovered_unrecorded(window) == *hovered)
}

/// Where a cached view's hovers can change, worked out when it is built.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum HoverArea {
    /// Every hover it read is of a hitbox it inserted itself, and those can
    /// be hit only within these bounds: the union of the masked bounds of
    /// the hitboxes it read.
    Own(Option<Bounds<Pixels>>),
    /// It read the hover of a hitbox it did not insert, such as an
    /// ancestor's group (`group_hover`). That hitbox is inserted again, under
    /// a new id and perhaps at new bounds, whenever its owner is drawn, so
    /// nothing the view recorded says where its hovers can change.
    #[default]
    Foreign,
}

/// Where the hovers in `reads` can change, for a view that inserted the
/// hitboxes at `hitboxes` in the frame being drawn.
pub(crate) fn hover_area(
    window: &Window,
    reads: &[(HitboxId, bool)],
    hitboxes: Range<usize>,
) -> HoverArea {
    if reads.is_empty() {
        return HoverArea::Own(None);
    }
    let own = &window.next_frame.hitboxes[hitboxes];
    let ids: FxHashSet<HitboxId> = own.iter().map(|hitbox| hitbox.id).collect();
    if reads.iter().any(|(hitbox, _)| !ids.contains(hitbox)) {
        return HoverArea::Foreign;
    }
    // Only where a hitbox whose hover was read can be hit can that hover
    // change.
    let read: FxHashSet<HitboxId> = reads.iter().map(|(hitbox, _)| *hitbox).collect();
    HoverArea::Own(
        own.iter()
            .filter(|hitbox| read.contains(&hitbox.id))
            .map(|hitbox| hitbox.bounds.intersect(&hitbox.content_mask.bounds))
            .filter(|bounds| !bounds.is_empty())
            .reduce(|area, bounds| area.union(&bounds)),
    )
}

/// Whether a cached view drawn by `reads`, whose hovers can change in
/// `area`, may be replayed as far as its hovers go.
///
/// Its hovers must be as they were against the hit test prepaint sees, which
/// is last frame's hitboxes. This frame's hitboxes, known only after
/// prepaint, can cover or uncover the view's, but only where the pointer is:
/// with the pointer away from all of them, and none holding the pointer
/// captured, every hover it read is false whatever this frame adds. So a view
/// that read hovers is drawn again while the pointer is over its hitboxes,
/// and its replays always show the hovers a frame drawn from scratch shows.
/// A view that read the hover of a hitbox it did not insert is never
/// replayed: that hitbox's id and bounds belong to whoever inserted it.
pub(crate) fn may_replay_hovers(
    window: &Window,
    reads: &[(HitboxId, bool)],
    area: HoverArea,
) -> bool {
    if reads.is_empty() {
        return true;
    }
    let HoverArea::Own(area) = area else {
        return false;
    };
    let pointer_over = area.is_some_and(|area| area.contains(&window.mouse_position));
    let captured = window
        .captured_hitbox
        .is_some_and(|captured| reads.iter().any(|(hitbox, _)| *hitbox == captured));
    !pointer_over && !captured && hovers_unchanged(window, reads)
}

/// The app's side of recording what subtrees read: when each global last
/// changed, and what was read while a recording is open.
#[derive(Default)]
pub(crate) struct AppDependencies {
    /// Counts the changes to globals, each stamped into `global_changed_at`,
    /// so a recording can tell whether a global it read changed since.
    global_generation: u64,
    global_changed_at: TypeIdHashMap<u64>,
    /// Every global read while a recording is open.
    global_read_log: Rc<RefCell<Vec<TypeId>>>,
    /// Every [`StateVersion`] read while a recording is open, with the
    /// version it was at.
    state_read_log: RefCell<Vec<(StateVersion, u64)>>,
}

impl AppDependencies {
    /// Stamps a change to the global of type `global_type`.
    fn global_changed(&mut self, global_type: TypeId) {
        // Stamped on every change, not only the first one an effect is
        // queued for: a subtree built in between has seen only the first.
        self.global_generation += 1;
        self.global_changed_at
            .insert(global_type, self.global_generation);
    }
}

/// Stands for whether a global of type `G` is set, which a subtree that only
/// asked [`App::has_global`] depends on instead of the global.
struct GlobalPresence<G>(std::marker::PhantomData<G>);

/// Records, for any recording that is open, that the global of type
/// `global` was read.
#[inline(always)]
pub(crate) fn note_global_read(cx: &App, global: TypeId) {
    if cx.entities.fast_access_log.is_recording() {
        cx.fast_dependencies
            .global_read_log
            .borrow_mut()
            .push(global);
    }
}

/// Records, for any recording that is open, that whether a global of type
/// `G` is set was read, as [`App::has_global`] does. Only setting it where it
/// was not, or removing it, changes that; writing to it does not.
#[inline(always)]
pub(crate) fn note_global_presence_read<G: 'static>(cx: &App) {
    note_global_read(cx, TypeId::of::<GlobalPresence<G>>());
}

/// Stamps a change to whether a global of type `G` is set, if it is about to
/// be set where it was not.
pub(crate) fn note_global_inserted<G: 'static>(cx: &mut App) {
    if !cx.globals_by_type.contains_key(&TypeId::of::<G>()) {
        cx.fast_dependencies
            .global_changed(TypeId::of::<GlobalPresence<G>>());
    }
}

/// Stamps a change to whether a global of type `G` is set, as it is about to
/// be removed.
pub(crate) fn note_global_removed<G: 'static>(cx: &mut App) {
    cx.fast_dependencies
        .global_changed(TypeId::of::<GlobalPresence<G>>());
}

/// Stamps a change to the global of type `global_type`, as its observers
/// are about to be notified: every write to a global goes through that.
#[inline(always)]
pub(crate) fn global_changed(cx: &mut App, global_type: TypeId) {
    cx.fast_dependencies.global_changed(global_type);
}

/// Records, for any recording that is open, that the state `version`
/// belongs to was read as it is now.
#[inline(always)]
pub(crate) fn note_state_read(cx: &App, version: &StateVersion) {
    if cx.entities.fast_access_log.is_recording() {
        cx.fast_dependencies
            .state_read_log
            .borrow_mut()
            .push((version.clone(), version.get()));
    }
}

/// A counter that state kept outside of entities (a scroll handle, a list
/// state) increments when it really changes, so a subtree that read it is
/// built again, as it would be for an entity that changed.
#[derive(Clone, Default, Debug)]
pub(crate) struct StateVersion(Rc<Cell<u64>>);

impl StateVersion {
    pub(crate) fn get(&self) -> u64 {
        self.0.get()
    }

    /// Marks the state as changed.
    pub(crate) fn bump(&self) {
        self.0.set(self.0.get().wrapping_add(1));
    }

    /// Marks the state as changed if `changed`, for a change that may leave
    /// it as it was.
    #[inline(always)]
    pub(crate) fn bump_if(&self, changed: bool) {
        if changed {
            self.bump();
        }
    }

    #[allow(dead_code, reason = "read by view retention, which lands next")]
    fn same_state(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Bumps the version of a piece of state, when it is dropped, if a mutator
/// it spans changed what the state shows: a request to scroll where the
/// state already is changes nothing, and moves no version.
pub(crate) struct ChangeGuard<'a, T, M: PartialEq> {
    cell: &'a RefCell<T>,
    before: Option<M>,
    marker: fn(&T) -> M,
    version: fn(&T) -> &StateVersion,
}

impl<'a, T, M: PartialEq> ChangeGuard<'a, T, M> {
    pub(crate) fn new(
        cell: &'a RefCell<T>,
        marker: fn(&T) -> M,
        version: fn(&T) -> &StateVersion,
    ) -> Self {
        let before = cell.try_borrow().ok().map(|state| marker(&state));
        Self {
            cell,
            before,
            marker,
            version,
        }
    }
}

impl<T, M: PartialEq> Drop for ChangeGuard<'_, T, M> {
    fn drop(&mut self) {
        if let Some(before) = self.before.take()
            && let Ok(state) = self.cell.try_borrow()
            && (self.marker)(&state) != before
        {
            (self.version)(&state).bump();
        }
    }
}

/// Parts of a window's state a view can read while it is drawn without
/// reading an entity or a global: each is recorded as a global of its own
/// type, and stamped changed when an input event changes it.
pub(crate) mod ambient {
    /// Where the pointer is: [`crate::Window::mouse_position`].
    pub(crate) struct Pointer;
    /// The modifier keys and caps lock: [`crate::Window::modifiers`] and
    /// [`crate::Window::capslock`].
    pub(crate) struct Keys;
    /// The pressed mouse button: [`crate::Window::pressed_mouse_button`].
    pub(crate) struct Button;
}

/// A window's handle on the app's recording, so that reading the window's
/// own input state while a view is drawn is recorded as reading a global.
#[derive(Clone, Default)]
pub(crate) struct AmbientReads {
    globals: Rc<RefCell<Vec<TypeId>>>,
    recordings: Rc<Cell<usize>>,
}

impl AmbientReads {
    /// Records, for any recording that is open, that the ambient state `T`
    /// was read.
    #[inline(always)]
    pub(crate) fn note<T: 'static>(&self) {
        if self.recordings.get() > 0 {
            self.globals.borrow_mut().push(TypeId::of::<T>());
        }
    }
}

/// The window's input state before it handled an event, to tell afterwards
/// what the event changed.
pub(crate) struct AmbientInput {
    position: crate::Point<Pixels>,
    modifiers: crate::Modifiers,
    capslock: crate::Capslock,
    button: Option<crate::MouseButton>,
}

impl AmbientInput {
    pub(crate) fn of(window: &Window) -> Self {
        AmbientInput {
            position: window.mouse_position,
            modifiers: window.modifiers,
            capslock: window.capslock,
            button: window.pressed_mouse_button,
        }
    }

    /// Stamps what the event changed as changed, for the views that read it
    /// while they were drawn.
    pub(crate) fn stamp_changes(self, window: &Window, cx: &mut App) {
        if window.mouse_position != self.position {
            cx.fast_dependencies
                .global_changed(TypeId::of::<ambient::Pointer>());
        }
        if window.modifiers != self.modifiers || window.capslock != self.capslock {
            cx.fast_dependencies
                .global_changed(TypeId::of::<ambient::Keys>());
        }
        if window.pressed_mouse_button != self.button {
            cx.fast_dependencies
                .global_changed(TypeId::of::<ambient::Button>());
        }
    }
}

/// The entity map's side of recording what subtrees read.
#[derive(Default)]
pub(crate) struct EntityAccessLog {
    /// Every entity accessed while a recording is open, in order.
    access_log: RefCell<Vec<EntityId>>,
    /// Where the last recording began or ended: an access repeating the one
    /// just before it is left out only after this.
    boundary: Cell<usize>,
    /// How many recordings are open.
    recordings: Rc<Cell<usize>>,
    /// Counts the entities updated or changed while no recording is open,
    /// each stamped into `updated_at` or `changed_at`.
    update_generation: u64,
    /// When each entity was last updated while no recording was open.
    updated_at: FxHashMap<EntityId, u64>,
    /// Counts the entities updated while a recording is open (written while
    /// the window draws), each stamped into `written_at`.
    write_generation: u64,
    /// When each entity was last written while the window drew.
    written_at: FxHashMap<EntityId, u64>,
    /// The entity the framework is about to lease to render it, which is
    /// drawing it, not writing to it. See [`render_next`].
    rendering: Option<EntityId>,
    /// The entities updated and not notified since.
    updated_unnotified: FxHashSet<EntityId>,
    /// When each entity was last changed: notified after being updated, or
    /// notified while drawing or while asked something.
    changed_at: FxHashMap<EntityId, u64>,
    /// The entity being asked something through [`query`], whose update is
    /// not a change unless it notifies.
    queried: Option<EntityId>,
}

impl EntityAccessLog {
    /// Whether any recording is open.
    #[inline(always)]
    pub(crate) fn is_recording(&self) -> bool {
        self.recordings.get() > 0
    }

    /// Whether any of `entities` was changed after `generation`, or, with
    /// `updates`, updated.
    #[allow(dead_code, reason = "read by view retention, which lands next")]
    fn changed_since(&self, entities: &[EntityId], generation: u64, updates: bool) -> bool {
        let after = |stamps: &FxHashMap<EntityId, u64>, entity| {
            stamps.get(entity).is_some_and(|at| *at > generation)
        };
        generation != self.update_generation
            && entities.iter().any(|entity| {
                after(&self.changed_at, entity) || (updates && after(&self.updated_at, entity))
            })
    }

    /// Whether any of `entities` was written while the window drew, after
    /// `writes` began and other than by the subtree `writes` belongs to.
    #[allow(dead_code, reason = "read by view retention, which lands next")]
    fn written_since(&self, entities: &[EntityId], writes: &Writes) -> bool {
        self.write_generation != writes.to
            && entities.iter().any(|entity| {
                self.written_at
                    .get(entity)
                    .is_some_and(|written_at| writes.is_foreign(*written_at))
            })
    }

    fn stamp_changed(&mut self, entity_id: EntityId) {
        self.update_generation += 1;
        self.changed_at.insert(entity_id, self.update_generation);
    }

    /// Forgets a released entity's stamps.
    pub(crate) fn forget(&mut self, entity_id: EntityId) {
        self.updated_at.remove(&entity_id);
        self.written_at.remove(&entity_id);
        self.updated_unnotified.remove(&entity_id);
        self.changed_at.remove(&entity_id);
    }
}

/// Records, for any recording that is open, that `entity_id` was accessed.
#[inline(always)]
pub(crate) fn note_access(log: &EntityAccessLog, entity_id: EntityId) {
    if log.is_recording() {
        let mut accesses = log.access_log.borrow_mut();
        // A view reads the same entity many times in a row as it renders;
        // one mention is all a recording needs.
        if accesses.len() > log.boundary.get() && accesses.last() == Some(&entity_id) {
            return;
        }
        accesses.push(entity_id);
    }
}

/// Records that `entity_id` is notified. A notification after an update
/// marks the entity changed. So does one while the window draws (a view
/// changing a model it read as it renders) or while the entity is asked
/// something ([`query`]). One alone, outside drawing, changes nothing a view
/// could have read.
#[inline(always)]
pub(crate) fn note_notify(log: &mut EntityAccessLog, entity_id: EntityId) {
    if log.updated_unnotified.remove(&entity_id) || log.is_recording() || log.queried.is_some() {
        log.stamp_changed(entity_id);
    }
}

/// Records that `entity_id` is being updated (leased), as [`note_access`]
/// does for an access.
///
/// An entity updated outside of drawing (by a task, a listener, an action)
/// may have changed without being notified, as when a view changes a model
/// it renders and notifies only itself: a subtree that read it counts it as
/// changed only inside a notified view. An entity updated while the window
/// draws is written, and a subtree that read it is built again, unless it
/// wrote it itself while it was built. The lease that renders a view is
/// neither.
#[inline]
pub(crate) fn note_update(log: &mut EntityAccessLog, entity_id: EntityId) {
    note_access(log, entity_id);
    if log.rendering == Some(entity_id) {
        log.rendering = None;
        if log.is_recording() {
            return;
        }
    }
    if log.queried == Some(entity_id) {
        return;
    }
    if !log.is_recording() {
        log.update_generation += 1;
        log.updated_at.insert(entity_id, log.update_generation);
        log.updated_unnotified.insert(entity_id);
    } else {
        log.write_generation += 1;
        log.written_at.insert(entity_id, log.write_generation);
    }
}

/// Marks the next lease of `entity_id` as the framework rendering it, not a
/// write to it.
#[inline(always)]
pub(crate) fn render_next(cx: &mut App, entity_id: EntityId) {
    cx.entities.fast_access_log.rendering = Some(entity_id);
}

/// Updates `entity` to ask it something, as the platform asks a text input
/// for its selection every frame: unlike [`Entity::update`], this does not
/// count as changing what the entity holds, unless it notifies while asked.
#[inline(always)]
pub(crate) fn query<T: 'static, R>(
    entity: &Entity<T>,
    cx: &mut App,
    query: impl FnOnce(&mut T, &mut Context<T>) -> R,
) -> R {
    let outer = cx
        .entities
        .fast_access_log
        .queried
        .replace(entity.entity_id());
    let result = entity.update(cx, query);
    cx.entities.fast_access_log.queried = outer;
    result
}

/// Where a recording started by [`App::begin_recording_dependencies`]
/// begins.
#[allow(dead_code, reason = "read by view retention, which lands next")]
#[derive(Clone, Copy)]
pub(crate) struct DependencyRecording {
    entities: usize,
    globals: usize,
    states: usize,
    generation: u64,
    updates: u64,
    writes: u64,
}

/// Where in the write generation a subtree was built: writes after `from`
/// change what it read, except those it made itself, within one of the
/// `own` stretches.
#[allow(dead_code, reason = "read by view retention, which lands next")]
#[derive(Clone, Default, Debug)]
pub(crate) struct Writes {
    from: u64,
    to: u64,
    own: SmallVec<[(u64, u64); 2]>,
}

impl Writes {
    /// Whether a write at `written_at` came from outside the subtree after it
    /// began.
    #[allow(dead_code, reason = "read by view retention, which lands next")]
    fn is_foreign(&self, written_at: u64) -> bool {
        written_at > self.from
            && !self
                .own
                .iter()
                .any(|(began, finished)| written_at > *began && written_at <= *finished)
    }
}

/// What a subtree read while it was built, as of the stamps then: while none
/// of it has changed, building it again would build the same thing.
#[allow(dead_code, reason = "read by view retention, which lands next")]
#[derive(Clone, Default, Debug)]
pub(crate) struct RenderDependencies {
    pub(crate) entities: Rc<[EntityId]>,
    pub(crate) globals: Rc<[TypeId]>,
    /// State kept outside of entities, with the version each was read at.
    pub(crate) states: Rc<[(StateVersion, u64)]>,
    pub(crate) generation: u64,
    /// The entity update generation the recording began at.
    pub(crate) updates: u64,
    pub(crate) writes: Writes,
}

/// Sorts `items` and removes repeats.
#[allow(dead_code, reason = "read by view retention, which lands next")]
fn sort_unique<T: Ord>(items: &mut Vec<T>) {
    items.sort_unstable();
    items.dedup();
}

/// `states` once each, at the earliest version read, so a change in between
/// still counts.
#[allow(dead_code, reason = "read by view retention, which lands next")]
fn unique_states(states: &[(StateVersion, u64)]) -> Rc<[(StateVersion, u64)]> {
    let mut unique: Vec<(StateVersion, u64)> = Vec::with_capacity(states.len());
    for state in states {
        if !unique
            .iter()
            .any(|(version, _)| version.same_state(&state.0))
        {
            unique.push(state.clone());
        }
    }
    unique.into()
}

impl App {
    /// A handle for a window to record reads of its own input state with.
    pub(crate) fn fast_ambient_reads(&self) -> AmbientReads {
        AmbientReads {
            globals: self.fast_dependencies.global_read_log.clone(),
            recordings: self.entities.fast_access_log.recordings.clone(),
        }
    }

    /// Starts recording what is read from here on: entities accessed,
    /// globals and global presence read, state versions and ambient input.
    #[allow(dead_code, reason = "read by view retention, which lands next")]
    pub(crate) fn begin_recording_dependencies(&mut self) -> DependencyRecording {
        let log = &mut self.entities.fast_access_log;
        let entities = log.access_log.borrow().len();
        log.boundary.set(entities);
        log.recordings.set(log.recordings.get() + 1);
        DependencyRecording {
            entities,
            globals: self.fast_dependencies.global_read_log.borrow().len(),
            states: self.fast_dependencies.state_read_log.borrow().len(),
            generation: self.fast_dependencies.global_generation,
            updates: log.update_generation,
            writes: log.write_generation,
        }
    }

    /// Ends `recording`, returning everything read while it was open.
    #[allow(dead_code, reason = "read by view retention, which lands next")]
    pub(crate) fn finish_recording_dependencies(
        &mut self,
        recording: DependencyRecording,
    ) -> RenderDependencies {
        let log = &mut self.entities.fast_access_log;
        let entities = {
            let mut accesses = log.access_log.borrow()[recording.entities..].to_vec();
            sort_unique(&mut accesses);
            Rc::from(accesses)
        };
        let globals = {
            let mut reads =
                self.fast_dependencies.global_read_log.borrow()[recording.globals..].to_vec();
            sort_unique(&mut reads);
            Rc::from(reads)
        };
        let states =
            unique_states(&self.fast_dependencies.state_read_log.borrow()[recording.states..]);
        let open = log.recordings.get() - 1;
        log.recordings.set(open);
        if open == 0 {
            log.access_log.borrow_mut().clear();
            self.fast_dependencies.global_read_log.borrow_mut().clear();
            self.fast_dependencies.state_read_log.borrow_mut().clear();
        }
        log.boundary.set(log.access_log.borrow().len());
        let mut own = SmallVec::new();
        if log.write_generation > recording.writes {
            own.push((recording.writes, log.write_generation));
        }
        RenderDependencies {
            entities,
            globals,
            states,
            // As of when the recording began, so that a global written while
            // it was open, after being read, counts as changed.
            generation: recording.generation,
            updates: recording.updates,
            writes: Writes {
                from: recording.writes,
                to: log.write_generation,
                own,
            },
        }
    }

    /// Whether anything in `dependencies` may have changed since they were
    /// recorded: an entity changed (updated and notified, or notified while
    /// drawing), written while the window drew by another subtree, or, with
    /// `inside_notified`, updated at all; a global written, inserted or
    /// removed; a state version moved.
    ///
    /// An entity notified without being updated (a wheel, a dragged
    /// scrollbar or an animation notifying a view to draw again) holds what
    /// it held: the view notified is built again, a view that read it is
    /// not. An entity updated without being notified (as every subscriber of
    /// an entity is updated for each event it emits) counts as changed only
    /// for a view drawn inside a view notified since, which upstream builds
    /// again with everything under it.
    #[allow(dead_code, reason = "read by view retention, which lands next")]
    pub(crate) fn dependencies_changed(
        &self,
        dependencies: &RenderDependencies,
        inside_notified: bool,
    ) -> bool {
        let log = &self.entities.fast_access_log;
        log.changed_since(
            &dependencies.entities,
            dependencies.updates,
            inside_notified,
        ) || log.written_since(&dependencies.entities, &dependencies.writes)
            || dependencies.globals.iter().any(|global| {
                self.fast_dependencies
                    .global_changed_at
                    .get(global)
                    .is_some_and(|changed_at| *changed_at > dependencies.generation)
            })
            || dependencies
                .states
                .iter()
                .any(|(version, read_at)| version.get() != *read_at)
    }
}
