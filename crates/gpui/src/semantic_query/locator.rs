use super::{Query, QueryError};
use crate::{
    DevtoolsElementSnapshot, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollWheelEvent,
    TouchPhase, VisualTestContext, point, px,
};

/// Where [`Locator::drag_to`] ends a drag.
#[derive(Clone, Debug, PartialEq)]
pub enum DragTarget {
    /// The center of the element a query identifies.
    Element(Query),
    /// A window position.
    Point(Point<Pixels>),
}

impl From<Query> for DragTarget {
    fn from(query: Query) -> Self {
        DragTarget::Element(query)
    }
}

impl From<&str> for DragTarget {
    fn from(query: &str) -> Self {
        DragTarget::Element(query.into())
    }
}

impl From<Point<Pixels>> for DragTarget {
    fn from(point: Point<Pixels>) -> Self {
        DragTarget::Point(point)
    }
}

/// A [`Query`] bound to a test window, with actions that drive simulated input at the element it
/// identifies. Every call resolves the query again against the latest rendered frame, after the
/// executor parks and pending invalidations draw.
///
/// Create one with [`VisualTestContext::locator`].
pub struct Locator<'a> {
    cx: &'a mut VisualTestContext,
    query: Query,
}

impl<'a> Locator<'a> {
    pub(crate) fn new(cx: &'a mut VisualTestContext, query: Query) -> Self {
        Self { cx, query }
    }

    /// The query this locator resolves.
    pub fn query(&self) -> &Query {
        &self.query
    }

    /// Picks the zero-based match among all matches.
    pub fn nth(mut self, index: usize) -> Self {
        self.query = self.query.nth(index);
        self
    }

    /// The one element the query identifies.
    pub fn resolve(&mut self) -> Result<DevtoolsElementSnapshot, QueryError> {
        let snapshot = self.cx.settled_frame_snapshot();
        self.query.resolve(&snapshot).cloned()
    }

    /// Every element the query matches, in prepaint order, ignoring `nth`.
    pub fn all(&mut self) -> Vec<DevtoolsElementSnapshot> {
        let snapshot = self.cx.settled_frame_snapshot();
        self.query.matches(&snapshot).into_iter().cloned().collect()
    }

    /// How many elements the query matches.
    pub fn count(&mut self) -> usize {
        self.all().len()
    }

    /// Clicks the primary button at the element's center.
    pub fn click(&mut self) -> Result<DevtoolsElementSnapshot, QueryError> {
        let element = self.resolve()?;
        self.cx.simulate_click(center(&element), Modifiers::none());
        Ok(element)
    }

    /// Moves the mouse to the element's center with no button held.
    pub fn hover(&mut self) -> Result<DevtoolsElementSnapshot, QueryError> {
        let element = self.resolve()?;
        self.cx
            .simulate_mouse_move(center(&element), None, Modifiers::none());
        Ok(element)
    }

    /// Drags with the primary button from the element's center to `target`, through evenly
    /// spaced intermediate positions.
    pub fn drag_to(
        &mut self,
        target: impl Into<DragTarget>,
    ) -> Result<DevtoolsElementSnapshot, QueryError> {
        let element = self.resolve()?;
        let start = center(&element);
        let end = match target.into() {
            DragTarget::Point(point) => point,
            DragTarget::Element(query) => {
                let snapshot = self.cx.settled_frame_snapshot();
                center(query.resolve(&snapshot)?)
            }
        };
        const STEPS: usize = 4;
        let path = (0..=STEPS)
            .map(|step| {
                let t = step as f32 / STEPS as f32;
                point(
                    start.x + (end.x - start.x) * t,
                    start.y + (end.y - start.y) * t,
                )
            })
            .collect::<Vec<_>>();
        self.cx
            .simulate_drag(&path, MouseButton::Left, Modifiers::none());
        Ok(element)
    }

    /// Scrolls the wheel by `delta` pixels over the element's center.
    pub fn scroll(&mut self, delta: Point<Pixels>) -> Result<DevtoolsElementSnapshot, QueryError> {
        let element = self.resolve()?;
        self.cx.simulate_event(ScrollWheelEvent {
            position: center(&element),
            delta: ScrollDelta::Pixels(delta),
            modifiers: Modifiers::none(),
            touch_phase: TouchPhase::Moved,
        });
        Ok(element)
    }

    /// Focuses the element by clicking it, unless focus is already on it or inside it.
    pub fn focus(&mut self) -> Result<DevtoolsElementSnapshot, QueryError> {
        let snapshot = self.cx.settled_frame_snapshot();
        let element = self.query.resolve(&snapshot)?.clone();
        if !super::focus_is_within(&snapshot, &element) {
            self.cx.simulate_click(center(&element), Modifiers::none());
        }
        Ok(element)
    }

    /// Focuses the element and types `text`, one keystroke per character.
    pub fn type_text(&mut self, text: &str) -> Result<DevtoolsElementSnapshot, QueryError> {
        let element = self.focus()?;
        self.cx.simulate_input(text);
        Ok(element)
    }

    /// Focuses the element and presses space-separated keystrokes, such as `"ctrl-a backspace"`.
    pub fn press(&mut self, keystrokes: &str) -> Result<DevtoolsElementSnapshot, QueryError> {
        let element = self.focus()?;
        self.cx.simulate_keystrokes(keystrokes);
        Ok(element)
    }
}

/// The center of an element's window-local bounds.
pub fn center(element: &DevtoolsElementSnapshot) -> Point<Pixels> {
    let bounds = element.bounds;
    point(
        px(bounds.x + bounds.width / 2.),
        px(bounds.y + bounds.height / 2.),
    )
}
