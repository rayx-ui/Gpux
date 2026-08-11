use crate::{
    Bounds, Capslock, Context, Empty, IntoElement, Keystroke, Modifiers, Pixels, Point, Render,
    Window, point, seal::Sealed,
};
use scheduler::Instant;
use smallvec::SmallVec;
use std::{any::Any, fmt::Debug, ops::Deref, path::PathBuf};

/// An event from a platform input source.
pub trait InputEvent: Sealed + 'static {
    /// Convert this event into the platform input enum.
    fn to_platform_input(self) -> PlatformInput;
}

/// A key event from the platform.
pub trait KeyEvent: InputEvent {}

/// A gesture event from the platform.
pub trait GestureEvent: InputEvent {}

/// A located pointer event from the platform.
pub trait PointerEventLike: InputEvent {}

/// The key down event equivalent for the platform.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyDownEvent {
    /// The keystroke that was generated.
    pub keystroke: Keystroke,

    /// Whether the key is currently held down.
    pub is_held: bool,

    /// Whether to prefer character input over keybindings for this keystroke.
    /// In some cases, like AltGr on Windows, modifiers are significant for character input.
    pub prefer_character_input: bool,
}

impl Sealed for KeyDownEvent {}
impl InputEvent for KeyDownEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::KeyDown(self)
    }
}
impl KeyEvent for KeyDownEvent {}

/// The key up event equivalent for the platform.
#[derive(Clone, Debug)]
pub struct KeyUpEvent {
    /// The keystroke that was released.
    pub keystroke: Keystroke,
}

impl Sealed for KeyUpEvent {}
impl InputEvent for KeyUpEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::KeyUp(self)
    }
}
impl KeyEvent for KeyUpEvent {}

/// The modifiers changed event equivalent for the platform.
#[derive(Clone, Debug, Default)]
pub struct ModifiersChangedEvent {
    /// The new state of the modifier keys
    pub modifiers: Modifiers,
    /// The new state of the capslock key
    pub capslock: Capslock,
}

impl Sealed for ModifiersChangedEvent {}
impl InputEvent for ModifiersChangedEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::ModifiersChanged(self)
    }
}
impl KeyEvent for ModifiersChangedEvent {}

impl Deref for ModifiersChangedEvent {
    type Target = Modifiers;

    fn deref(&self) -> &Self::Target {
        &self.modifiers
    }
}

/// The phase of a touch motion event.
/// Based on the winit enum of the same name.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TouchPhase {
    /// The touch started.
    Started,
    /// The touch event is moving.
    #[default]
    Moved,
    /// The touch phase has ended.
    Ended,
    /// The platform cancelled the touch sequence.
    Cancelled,
}

/// An identity that remains stable for one active pointer sequence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct PointerId(u64);

impl PointerId {
    /// Creates a pointer identity from the platform's active-sequence identifier.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the platform identifier for this active sequence.
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// The factual device family that produced a pointer event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PointerKind {
    /// A mouse or mouse-compatible pointing device.
    Mouse,
    /// A direct-touch contact.
    Touch,
    /// A stylus or pen.
    Pen,
    /// The platform did not identify the device family.
    #[default]
    Unknown,
}

/// The lifecycle state of a pointer event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PointerPhase {
    /// A pointer entered or moved without contact.
    Hover,
    /// A pointer contact began.
    Down,
    /// A pointer changed position or button state.
    #[default]
    Move,
    /// A pointer contact ended.
    Up,
    /// The platform cancelled the pointer sequence.
    Cancel,
}

/// Optional facts reported by pressure- or contact-aware platforms.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerContact {
    /// Normalized contact pressure when the platform reports it.
    pub pressure: Option<f32>,
    /// Contact width and height in logical pixels when reported by the platform.
    pub size: Option<Point<Pixels>>,
    /// Pen tilt from the horizontal axis in degrees when reported by the platform.
    pub tilt_x: Option<f32>,
    /// Pen tilt from the vertical axis in degrees when reported by the platform.
    pub tilt_y: Option<f32>,
    /// Pen barrel rotation in degrees when reported by the platform.
    pub twist: Option<f32>,
}

/// The canonical cross-platform located pointer event.
#[derive(Clone, Debug, Default)]
pub struct PointerEvent {
    /// The identity for this active pointer sequence.
    pub id: PointerId,
    /// The factual source device family.
    pub kind: PointerKind,
    /// The lifecycle phase.
    pub phase: PointerPhase,
    /// Whether this is the platform's primary pointer for its device family.
    pub is_primary: bool,
    /// The pressed buttons after this event.
    pub buttons: SmallVec<[MouseButton; 1]>,
    /// The button that changed for this event, or the platform default when none changed.
    pub button: MouseButton,
    /// The primary pressed button when the platform reports one.
    pub pressed_button: Option<MouseButton>,
    /// The platform click or tap count when available.
    pub click_count: usize,
    /// Whether this is the first mouse click that should focus its target.
    pub first_mouse: bool,
    /// The position in window coordinates.
    pub position: Point<Pixels>,
    /// A target-local position when a platform adapter can determine it.
    pub local_position: Option<Point<Pixels>>,
    /// The modifier state associated with this event.
    pub modifiers: Modifiers,
    /// Optional pressure, contact-size, tilt, and twist facts.
    pub contact: PointerContact,
    /// The platform timestamp when available.
    pub timestamp: Option<Instant>,
}

impl PointerEvent {
    /// Builds a pointer event with no button or contact facts.
    pub fn new(
        id: PointerId,
        kind: PointerKind,
        phase: PointerPhase,
        position: Point<Pixels>,
        modifiers: Modifiers,
    ) -> Self {
        Self {
            id,
            kind,
            phase,
            is_primary: true,
            buttons: SmallVec::new(),
            button: MouseButton::default(),
            pressed_button: None,
            click_count: 0,
            first_mouse: false,
            position,
            local_position: None,
            modifiers,
            contact: PointerContact::default(),
            timestamp: None,
        }
    }

    /// Builds a mouse hover or drag-update event.
    pub fn mouse_move(
        position: Point<Pixels>,
        pressed_button: Option<MouseButton>,
        modifiers: Modifiers,
    ) -> Self {
        Self {
            pressed_button,
            phase: if pressed_button.is_some() {
                PointerPhase::Move
            } else {
                PointerPhase::Hover
            },
            ..Self::new(
                PointerId::new(0),
                PointerKind::Mouse,
                PointerPhase::Hover,
                position,
                modifiers,
            )
        }
    }

    /// Builds a mouse button-down event.
    pub fn mouse_down(
        position: Point<Pixels>,
        button: MouseButton,
        modifiers: Modifiers,
        click_count: usize,
        first_mouse: bool,
    ) -> Self {
        Self {
            button,
            pressed_button: Some(button),
            click_count,
            first_mouse,
            buttons: SmallVec::from_slice(&[button]),
            ..Self::new(
                PointerId::new(0),
                PointerKind::Mouse,
                PointerPhase::Down,
                position,
                modifiers,
            )
        }
    }

    /// Builds a mouse button-up event.
    pub fn mouse_up(
        position: Point<Pixels>,
        button: MouseButton,
        modifiers: Modifiers,
        click_count: usize,
    ) -> Self {
        Self {
            button,
            click_count,
            ..Self::new(
                PointerId::new(0),
                PointerKind::Mouse,
                PointerPhase::Up,
                position,
                modifiers,
            )
        }
    }

    /// Builds a direct-touch pointer event without fabricating mouse button facts.
    pub fn touch(
        id: PointerId,
        phase: PointerPhase,
        position: Point<Pixels>,
        modifiers: Modifiers,
    ) -> Self {
        Self {
            is_primary: id == PointerId::new(0),
            ..Self::new(id, PointerKind::Touch, phase, position, modifiers)
        }
    }

    /// Returns whether this event terminates its active pointer sequence.
    pub fn is_terminal(&self) -> bool {
        matches!(self.phase, PointerPhase::Up | PointerPhase::Cancel)
    }

    /// Returns whether the primary button is held during this pointer event.
    pub fn dragging(&self) -> bool {
        self.pressed_button == Some(MouseButton::Left)
    }
}

impl Sealed for PointerEvent {}
impl InputEvent for PointerEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::Pointer(self)
    }
}
impl PointerEventLike for PointerEvent {}

/// The platform source that produced a scroll wheel event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ScrollWheelSource {
    /// The platform did not expose a more specific source.
    #[default]
    Unknown,
    /// A discrete mouse wheel or line-based desktop wheel source.
    MouseWheel,
    /// A high-resolution wheel or trackpad-style source.
    PrecisionWheel,
    /// Windows DirectManipulation generated the scroll event.
    DirectManipulation,
    /// A direct touch pan generated the scroll event.
    Touch,
}

/// The factual phase of a platform scroll sequence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ScrollWheelPhase {
    /// The scroll sequence started.
    Started,
    /// The scroll sequence moved.
    #[default]
    Moved,
    /// The scroll sequence ended.
    Ended,
    /// The platform cancelled the scroll sequence.
    Cancelled,
}

/// The native platform momentum phase associated with a scroll event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ScrollWheelMomentumPhase {
    /// Native momentum started.
    Started,
    /// Native momentum moved.
    #[default]
    Moved,
    /// Native momentum ended.
    Ended,
    /// Native momentum was cancelled.
    Cancelled,
}

/// A click event, generated when a pointer contact is pressed and released.
#[derive(Clone, Debug, Default)]
pub struct PointerClickEvent {
    /// The pointer event when the contact was pressed.
    pub down: PointerEvent,

    /// The pointer event when the contact was released.
    pub up: PointerEvent,
}

/// A click event that was generated by a keyboard button being pressed and released.
#[derive(Clone, Debug, Default)]
pub struct KeyboardClickEvent {
    /// The keyboard button that was pressed to trigger the click.
    pub button: KeyboardButton,

    /// The bounds of the element that was clicked.
    pub bounds: Bounds<Pixels>,
}

/// A click event, generated when a pointer contact or keyboard button is pressed and released.
#[derive(Clone, Debug)]
pub enum ClickEvent {
    /// A click event triggered by a pointer contact being pressed and released.
    Pointer(PointerClickEvent),
    /// A click event trigger by a keyboard button being pressed and released.
    Keyboard(KeyboardClickEvent),
}

impl Default for ClickEvent {
    fn default() -> Self {
        ClickEvent::Keyboard(KeyboardClickEvent::default())
    }
}

impl ClickEvent {
    /// Returns the modifiers that were held during the click event
    ///
    /// `Keyboard`: The keyboard click events never have modifiers.
    /// `Mouse`: Modifiers that were held during the mouse key up event.
    pub fn modifiers(&self) -> Modifiers {
        match self {
            // Click events are only generated from keyboard events _without any modifiers_, so we know the modifiers are always Default
            ClickEvent::Keyboard(_) => Modifiers::default(),
            // Click events on the web only reflect the modifiers for the keyup event,
            // tested via observing the behavior of the `ClickEvent.shiftKey` field in Chrome 138
            // under various combinations of modifiers and keyUp / keyDown events.
            ClickEvent::Pointer(event) => event.up.modifiers,
        }
    }

    /// Returns the position of the click event
    ///
    /// `Keyboard`: The bottom left corner of the clicked hitbox
    /// `Mouse`: The position of the mouse when the button was released.
    pub fn position(&self) -> Point<Pixels> {
        match self {
            ClickEvent::Keyboard(event) => event.bounds.bottom_left(),
            ClickEvent::Pointer(event) => event.up.position,
        }
    }

    /// Returns the mouse position of the click event
    ///
    /// `Keyboard`: None
    /// `Mouse`: The position of the mouse when the button was released.
    pub fn mouse_position(&self) -> Option<Point<Pixels>> {
        match self {
            ClickEvent::Keyboard(_) => None,
            ClickEvent::Pointer(event) => {
                (event.up.kind == PointerKind::Mouse).then_some(event.up.position)
            }
        }
    }

    /// Returns if this was a right click
    ///
    /// `Keyboard`: false
    /// `Mouse`: Whether the right button was pressed and released
    pub fn is_right_click(&self) -> bool {
        match self {
            ClickEvent::Keyboard(_) => false,
            ClickEvent::Pointer(event) => {
                event.down.button == MouseButton::Right && event.up.button == MouseButton::Right
            }
        }
    }

    /// Returns if this was a middle click
    ///
    /// `Keyboard`: false
    /// `Mouse`: Whether the middle button was pressed and released
    pub fn is_middle_click(&self) -> bool {
        match self {
            ClickEvent::Keyboard(_) => false,
            ClickEvent::Pointer(event) => {
                event.down.button == MouseButton::Middle && event.up.button == MouseButton::Middle
            }
        }
    }

    /// Returns whether the click was a standard click
    ///
    /// `Keyboard`: Always true
    /// `Mouse`: Left button pressed and released
    pub fn standard_click(&self) -> bool {
        match self {
            ClickEvent::Keyboard(_) => true,
            ClickEvent::Pointer(event) => {
                event.down.button == MouseButton::Left && event.up.button == MouseButton::Left
            }
        }
    }

    /// Returns whether the click focused the element
    ///
    /// `Keyboard`: false, keyboard clicks only work if an element is already focused
    /// `Mouse`: Whether this was the first focusing click
    pub fn first_focus(&self) -> bool {
        match self {
            ClickEvent::Keyboard(_) => false,
            ClickEvent::Pointer(event) => event.down.first_mouse,
        }
    }

    /// Returns the click count of the click event
    ///
    /// `Keyboard`: Always 1
    /// `Pointer`: Count reported by the terminal pointer event.
    pub fn click_count(&self) -> usize {
        match self {
            ClickEvent::Keyboard(_) => 1,
            ClickEvent::Pointer(event) => event.up.click_count,
        }
    }

    /// Returns whether the click event is generated by a keyboard event
    pub fn is_keyboard(&self) -> bool {
        match self {
            ClickEvent::Pointer(_) => false,
            ClickEvent::Keyboard(_) => true,
        }
    }
}

/// An enum representing the keyboard button that was pressed for a click event.
#[derive(Hash, PartialEq, Eq, Copy, Clone, Debug, Default)]
pub enum KeyboardButton {
    /// Enter key was clicked
    #[default]
    Enter,
    /// Space key was clicked
    Space,
}

/// An enum representing the mouse button that was pressed.
#[derive(Hash, Default, PartialEq, Eq, Copy, Clone, Debug)]
pub enum MouseButton {
    /// The left mouse button.
    #[default]
    Left,

    /// The right mouse button.
    Right,

    /// The middle mouse button.
    Middle,

    /// A navigation button, such as back or forward.
    Navigate(NavigationDirection),

    /// A platform mouse button that does not have a standard semantic mapping.
    Other(u16),
}

impl MouseButton {
    /// Get all the mouse buttons in a list.
    pub fn all() -> Vec<Self> {
        vec![
            MouseButton::Left,
            MouseButton::Right,
            MouseButton::Middle,
            MouseButton::Navigate(NavigationDirection::Back),
            MouseButton::Navigate(NavigationDirection::Forward),
        ]
    }
}

/// A navigation direction, such as back or forward.
#[derive(Hash, Default, PartialEq, Eq, Copy, Clone, Debug)]
pub enum NavigationDirection {
    /// The back button.
    #[default]
    Back,

    /// The forward button.
    Forward,
}

/// A mouse wheel event from the platform.
#[derive(Clone, Debug, Default)]
pub struct ScrollWheelEvent {
    /// The position of the mouse on the window.
    pub position: Point<Pixels>,

    /// The change in scroll wheel position for this event.
    pub delta: ScrollDelta,

    /// The modifiers that were held down when the mouse was moved.
    pub modifiers: Modifiers,

    /// The phase of the touch event.
    pub touch_phase: TouchPhase,

    /// The platform source that produced this scroll event.
    pub scroll_source: ScrollWheelSource,

    /// The factual platform scroll phase for this event.
    pub scroll_phase: ScrollWheelPhase,

    /// Whether the platform identified the delta as a precise pixel delta.
    pub precise_delta: bool,

    /// The native platform momentum phase, when the platform exposes one.
    pub native_momentum_phase: Option<ScrollWheelMomentumPhase>,

    /// The direct touch-pan release velocity in logical px/s, when reported by the platform adapter.
    pub release_velocity: Option<Point<Pixels>>,

    /// The platform event timestamp, when the adapter can provide one.
    pub timestamp: Option<Instant>,
}

impl Sealed for ScrollWheelEvent {}
impl InputEvent for ScrollWheelEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::ScrollWheel(self)
    }
}

impl Deref for ScrollWheelEvent {
    type Target = Modifiers;

    fn deref(&self) -> &Self::Target {
        &self.modifiers
    }
}

/// The scroll delta for a scroll wheel event.
#[derive(Clone, Copy, Debug)]
pub enum ScrollDelta {
    /// An exact scroll delta in pixels.
    Pixels(Point<Pixels>),
    /// An inexact scroll delta in lines.
    Lines(Point<f32>),
}

impl Default for ScrollDelta {
    fn default() -> Self {
        Self::Lines(Default::default())
    }
}

/// A pinch gesture event from the platform, generated when the user performs
/// a pinch-to-zoom gesture (typically on a trackpad).
///
#[derive(Clone, Debug, Default)]
pub struct PinchEvent {
    /// The position of the pinch center on the window.
    pub position: Point<Pixels>,

    /// The zoom delta for this event.
    /// Positive values indicate zooming in, negative values indicate zooming out.
    /// For example, 0.1 represents a 10% zoom increase.
    pub delta: f32,

    /// The modifiers that were held down during the pinch gesture.
    pub modifiers: Modifiers,

    /// The phase of the pinch gesture.
    pub phase: TouchPhase,
}

impl Sealed for PinchEvent {}
impl InputEvent for PinchEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::Pinch(self)
    }
}
impl GestureEvent for PinchEvent {}

impl Deref for PinchEvent {
    type Target = Modifiers;

    fn deref(&self) -> &Self::Target {
        &self.modifiers
    }
}

/// A rotate gesture event from the platform.
#[derive(Clone, Debug, Default)]
pub struct RotateEvent {
    /// The position of the rotation center on the window.
    pub position: Point<Pixels>,

    /// The rotation delta in degrees since the previous event.
    /// Positive values indicate clockwise rotation.
    pub delta_degrees: f32,

    /// The modifiers that were held down during the rotate gesture.
    pub modifiers: Modifiers,

    /// The phase of the rotate gesture.
    pub phase: TouchPhase,
}

impl Sealed for RotateEvent {}
impl InputEvent for RotateEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::Rotate(self)
    }
}
impl GestureEvent for RotateEvent {}

impl Deref for RotateEvent {
    type Target = Modifiers;

    fn deref(&self) -> &Self::Target {
        &self.modifiers
    }
}

impl ScrollDelta {
    /// Returns true if this is a precise scroll delta in pixels.
    pub fn precise(&self) -> bool {
        match self {
            ScrollDelta::Pixels(_) => true,
            ScrollDelta::Lines(_) => false,
        }
    }

    /// Converts this scroll event into exact pixels.
    pub fn pixel_delta(&self, line_height: Pixels) -> Point<Pixels> {
        match self {
            ScrollDelta::Pixels(delta) => *delta,
            ScrollDelta::Lines(delta) => point(line_height * delta.x, line_height * delta.y),
        }
    }

    /// Combines two scroll deltas into one.
    /// If the signs of the deltas are the same (both positive or both negative),
    /// the deltas are added together. If the signs are opposite, the second delta
    /// (other) is used, effectively overriding the first delta.
    pub fn coalesce(self, other: ScrollDelta) -> ScrollDelta {
        match (self, other) {
            (ScrollDelta::Pixels(a), ScrollDelta::Pixels(b)) => {
                let x = if a.x.signum() == b.x.signum() {
                    a.x + b.x
                } else {
                    b.x
                };

                let y = if a.y.signum() == b.y.signum() {
                    a.y + b.y
                } else {
                    b.y
                };

                ScrollDelta::Pixels(point(x, y))
            }

            (ScrollDelta::Lines(a), ScrollDelta::Lines(b)) => {
                let x = if a.x.signum() == b.x.signum() {
                    a.x + b.x
                } else {
                    b.x
                };

                let y = if a.y.signum() == b.y.signum() {
                    a.y + b.y
                } else {
                    b.y
                };

                ScrollDelta::Lines(point(x, y))
            }

            _ => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::px;

    #[test]
    fn scroll_wheel_event_exposes_platform_metadata() {
        let timestamp = Instant::now();
        let event = ScrollWheelEvent {
            position: point(px(10.0), px(20.0)),
            delta: ScrollDelta::Pixels(point(px(1.0), px(2.0))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Started,
            scroll_source: ScrollWheelSource::DirectManipulation,
            scroll_phase: ScrollWheelPhase::Started,
            precise_delta: true,
            native_momentum_phase: Some(ScrollWheelMomentumPhase::Moved),
            release_velocity: None,
            timestamp: Some(timestamp),
        };

        assert_eq!(event.scroll_source, ScrollWheelSource::DirectManipulation);
        assert_eq!(event.scroll_phase, ScrollWheelPhase::Started);
        assert!(event.precise_delta);
        assert_eq!(
            event.native_momentum_phase,
            Some(ScrollWheelMomentumPhase::Moved)
        );
        assert_eq!(event.timestamp, Some(timestamp));
    }
}

/// A collection of paths from the platform, such as from a file drop.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct ExternalPaths(pub SmallVec<[PathBuf; 2]>);

impl ExternalPaths {
    /// Convert this collection of paths into a slice.
    pub fn paths(&self) -> &[PathBuf] {
        &self.0
    }
}

/// Data offered to the platform when an internal drag leaves the window.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ExternalDragPayload {
    /// Real on-disk paths promoted to a native file drag.
    Files(FileDragPaths),
}

/// Paths supplied to a native file drag with precomputed directory metadata.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct FileDragPaths(SmallVec<[(PathBuf, bool); 2]>);

impl FileDragPaths {
    /// Creates a native file-drag payload.
    pub fn new(entries: impl IntoIterator<Item = (PathBuf, bool)>) -> Self {
        Self(entries.into_iter().collect())
    }

    /// Returns each path paired with whether it is a directory.
    pub fn entries(&self) -> &[(PathBuf, bool)] {
        &self.0
    }
}

impl Render for ExternalPaths {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // the platform will render icons for the dragged files
        Empty
    }
}

/// A file drop event from the platform, generated when files are dragged and dropped onto the window.
#[derive(Debug, Clone)]
pub enum FileDropEvent {
    /// The files have entered the window.
    Entered {
        /// The position of the mouse relative to the window.
        position: Point<Pixels>,
        /// The paths of the files that are being dragged.
        paths: ExternalPaths,
    },
    /// The files are being dragged over the window
    Pending {
        /// The position of the mouse relative to the window.
        position: Point<Pixels>,
    },
    /// The files have been dropped onto the window.
    Submit {
        /// The position of the mouse relative to the window.
        position: Point<Pixels>,
    },
    /// The user has stopped dragging the files over the window.
    Exited,
}

impl Sealed for FileDropEvent {}
impl InputEvent for FileDropEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::FileDrop(self)
    }
}

/// An enum corresponding to all kinds of platform input events.
#[derive(Clone, Debug)]
pub enum PlatformInput {
    /// A key was pressed.
    KeyDown(KeyDownEvent),
    /// A key was released.
    KeyUp(KeyUpEvent),
    /// The keyboard modifiers were changed.
    ModifiersChanged(ModifiersChangedEvent),
    /// A normalized located pointer event.
    Pointer(PointerEvent),
    /// The scroll wheel was used.
    ScrollWheel(ScrollWheelEvent),
    /// A pinch gesture was performed.
    Pinch(PinchEvent),
    /// A rotate gesture was performed.
    Rotate(RotateEvent),
    /// Files were dragged and dropped onto the window.
    FileDrop(FileDropEvent),
}

impl PlatformInput {
    pub(crate) fn input_event(&self) -> Option<&dyn Any> {
        match self {
            PlatformInput::KeyDown { .. } => None,
            PlatformInput::KeyUp { .. } => None,
            PlatformInput::ModifiersChanged { .. } => None,
            PlatformInput::Pointer(event) => Some(event),
            PlatformInput::ScrollWheel(event) => Some(event),
            PlatformInput::Pinch(event) => Some(event),
            PlatformInput::Rotate(event) => Some(event),
            PlatformInput::FileDrop(event) => Some(event),
        }
    }

    pub(crate) fn keyboard_event(&self) -> Option<&dyn Any> {
        match self {
            PlatformInput::KeyDown(event) => Some(event),
            PlatformInput::KeyUp(event) => Some(event),
            PlatformInput::ModifiersChanged(event) => Some(event),
            PlatformInput::Pointer(_) => None,
            PlatformInput::ScrollWheel(_) => None,
            PlatformInput::Pinch(_) => None,
            PlatformInput::Rotate(_) => None,
            PlatformInput::FileDrop(_) => None,
        }
    }
}

#[cfg(test)]
mod test {

    use crate::{
        self as gpux, AppContext as _, Context, FocusHandle, InputLayerId, InteractiveElement,
        IntoElement, KeyBinding, KeymapLayer, Keystroke, ParentElement, Render, TestAppContext,
        Window, div,
    };

    struct TestView {
        saw_key_down: bool,
        saw_action: bool,
        focus_handle: FocusHandle,
    }

    actions!(test_only, [TestAction]);

    impl Render for TestView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().id("testview").child(
                div()
                    .key_context("parent")
                    .on_key_down(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.saw_key_down = true
                    }))
                    .on_action(cx.listener(|this: &mut TestView, _: &TestAction, _, _| {
                        this.saw_action = true
                    }))
                    .child(
                        div()
                            .key_context("nested")
                            .track_focus(&self.focus_handle)
                            .into_element(),
                    ),
            )
        }
    }

    #[gpux::test]
    fn test_on_events(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| TestView {
                    saw_key_down: false,
                    saw_action: false,
                    focus_handle: cx.focus_handle(),
                })
            })
            .unwrap()
        });

        cx.update(|cx| {
            cx.set_keymap_layer(KeymapLayer::new(
                InputLayerId::new("test.interactive"),
                0,
                [KeyBinding::new("ctrl-g", TestAction, Some("parent"))],
            ))
            .unwrap();
        });

        window
            .update(cx, |test_view, window, cx| {
                window.focus(&test_view.focus_handle, cx)
            })
            .unwrap();

        cx.dispatch_keystroke(*window, Keystroke::parse("a").unwrap());
        cx.dispatch_keystroke(*window, Keystroke::parse("ctrl-g").unwrap());

        window
            .update(cx, |test_view, _, _| {
                assert!(test_view.saw_key_down || test_view.saw_action);
                assert!(test_view.saw_key_down);
                assert!(test_view.saw_action);
            })
            .unwrap();
    }
}
