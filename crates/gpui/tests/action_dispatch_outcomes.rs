use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use gpux::{
    ActionDispatchEvent, ActionDispatchOutcome, ActionDispatchReport, ActionInvocation,
    ActionInvocationSource, Context, FocusHandle, IntoElement, Render, TestApp, Window, actions,
    div, prelude::*,
};

actions!(
    test,
    [
        DispatchAction,
        ExplicitDispatchAction,
        NestedDispatchAction,
        UnhandledDispatchAction
    ]
);

struct ActionHarness {
    focus: FocusHandle,
    handled: Rc<Cell<usize>>,
    explicit_outcome: Option<ActionDispatchOutcome>,
    dispatch_nested: bool,
}

impl Render for ActionHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let handled = self.handled.clone();
        let element = div()
            .track_focus(&self.focus)
            .on_action(move |_: &DispatchAction, _, _| {
                handled.set(handled.get() + 1);
            });

        if let Some(outcome) = self.explicit_outcome {
            let dispatch_nested = self.dispatch_nested;
            element.on_action_with_outcome(move |_: &ExplicitDispatchAction, window, cx| {
                if dispatch_nested {
                    window.dispatch_action(Box::new(NestedDispatchAction), cx);
                }
                outcome
            })
        } else {
            element
        }
    }
}

#[test]
fn dispatch_report_preserves_invocation_metadata_and_terminal_outcome() {
    let invocation = ActionInvocation::new(
        7,
        "test::Dispatch",
        ActionInvocationSource::Programmatic,
        None,
        None,
        None,
        Vec::new(),
    );
    let report = ActionDispatchReport::new(
        invocation.clone(),
        ActionDispatchOutcome::Handled,
        1,
        Duration::from_millis(2),
    );

    assert_eq!(report.invocation(), &invocation);
    assert_eq!(report.outcome(), ActionDispatchOutcome::Handled);
    assert_eq!(report.handled_count(), 1);
    assert_eq!(report.duration(), Duration::from_millis(2));
    assert_eq!(invocation.window_id(), None);
    assert!(invocation.started_at() <= std::time::Instant::now());
}

#[test]
fn terminal_outcomes_are_distinct() {
    assert_ne!(
        ActionDispatchOutcome::Handled,
        ActionDispatchOutcome::Unhandled
    );
    assert_ne!(
        ActionDispatchOutcome::Failed,
        ActionDispatchOutcome::Cancelled
    );
}

#[test]
fn handled_dispatch_emits_one_begin_and_one_terminal_report() {
    let mut app = TestApp::new();
    let handled = Rc::new(Cell::new(0));
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut window = app.open_window({
        let handled = handled.clone();
        move |_, cx| ActionHarness {
            focus: cx.focus_handle(),
            handled,
            explicit_outcome: None,
            dispatch_nested: false,
        }
    });
    window.draw();
    window.update(|harness, window, cx| window.focus(&harness.focus, cx));
    window.draw();

    let _subscription = window.update({
        let events = events.clone();
        move |_, window, _| {
            window
                .observe_action_dispatch(move |event, _, _| events.borrow_mut().push(event.clone()))
        }
    });
    let report =
        window.update(|_, window, cx| window.dispatch_action(Box::new(DispatchAction), cx));

    assert_eq!(handled.get(), 1);
    assert_eq!(report.outcome(), ActionDispatchOutcome::Handled);
    assert_eq!(report.handled_count(), 1);
    assert_eq!(
        events.borrow().as_slice(),
        [
            ActionDispatchEvent::Began(report.invocation().clone()),
            ActionDispatchEvent::Ended(report.clone()),
        ]
    );
}

#[test]
fn explicit_handler_outcomes_are_reported_without_propagating() {
    let mut app = TestApp::new();
    let mut window = app.open_window(|_, cx| ActionHarness {
        focus: cx.focus_handle(),
        handled: Rc::new(Cell::new(0)),
        explicit_outcome: Some(ActionDispatchOutcome::Failed),
        dispatch_nested: false,
    });
    window.draw();
    window.update(|harness, window, cx| window.focus(&harness.focus, cx));
    window.draw();

    let report =
        window.update(|_, window, cx| window.dispatch_action(Box::new(ExplicitDispatchAction), cx));

    assert_eq!(report.outcome(), ActionDispatchOutcome::Failed);
    assert_eq!(report.handled_count(), 0);
}

#[test]
fn cancelled_handler_outcome_is_terminal() {
    let mut app = TestApp::new();
    let mut window = app.open_window(|_, cx| ActionHarness {
        focus: cx.focus_handle(),
        handled: Rc::new(Cell::new(0)),
        explicit_outcome: Some(ActionDispatchOutcome::Cancelled),
        dispatch_nested: false,
    });
    window.draw();
    window.update(|harness, window, cx| window.focus(&harness.focus, cx));
    window.draw();

    let report =
        window.update(|_, window, cx| window.dispatch_action(Box::new(ExplicitDispatchAction), cx));

    assert_eq!(report.outcome(), ActionDispatchOutcome::Cancelled);
    assert_eq!(report.handled_count(), 0);
}

#[test]
fn unhandled_dispatch_emits_a_terminal_report() {
    let mut app = TestApp::new();
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut window = app.open_window(|_, cx| ActionHarness {
        focus: cx.focus_handle(),
        handled: Rc::new(Cell::new(0)),
        explicit_outcome: None,
        dispatch_nested: false,
    });
    window.draw();
    window.update(|harness, window, cx| window.focus(&harness.focus, cx));
    window.draw();
    let _subscription = window.update({
        let events = events.clone();
        move |_, window, _| {
            window
                .observe_action_dispatch(move |event, _, _| events.borrow_mut().push(event.clone()))
        }
    });

    let report = window
        .update(|_, window, cx| window.dispatch_action(Box::new(UnhandledDispatchAction), cx));

    assert_eq!(report.outcome(), ActionDispatchOutcome::Unhandled);
    assert_eq!(report.handled_count(), 0);
    assert!(matches!(events.borrow().as_slice(), [
        ActionDispatchEvent::Began(_),
        ActionDispatchEvent::Ended(ended),
    ] if ended == &report));
}

#[test]
fn nested_dispatch_captures_the_parent_invocation() {
    let mut app = TestApp::new();
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut window = app.open_window(|_, cx| ActionHarness {
        focus: cx.focus_handle(),
        handled: Rc::new(Cell::new(0)),
        explicit_outcome: Some(ActionDispatchOutcome::Handled),
        dispatch_nested: true,
    });
    window.draw();
    window.update(|harness, window, cx| window.focus(&harness.focus, cx));
    window.draw();
    let _subscription = window.update({
        let events = events.clone();
        move |_, window, _| {
            window
                .observe_action_dispatch(move |event, _, _| events.borrow_mut().push(event.clone()))
        }
    });

    let report =
        window.update(|_, window, cx| window.dispatch_action(Box::new(ExplicitDispatchAction), cx));
    let events = events.borrow();
    let [
        ActionDispatchEvent::Began(outer),
        ActionDispatchEvent::Began(inner),
        ActionDispatchEvent::Ended(inner_report),
        ActionDispatchEvent::Ended(outer_report),
    ] = events.as_slice()
    else {
        panic!("expected one begin/end pair for each nested dispatch");
    };

    assert_eq!(inner.parent_id(), Some(outer.id()));
    assert_eq!(inner_report.invocation(), inner);
    assert_eq!(outer_report, &report);
}

#[test]
fn released_focus_target_is_reported_as_unhandled_without_panicking() {
    let mut app = TestApp::new();
    let released_focus = Rc::new(RefCell::new(None));
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut window = app.open_window({
        let released_focus = released_focus.clone();
        move |_, cx| {
            *released_focus.borrow_mut() = Some(cx.focus_handle());
            ActionHarness {
                focus: cx.focus_handle(),
                handled: Rc::new(Cell::new(0)),
                explicit_outcome: None,
                dispatch_nested: false,
            }
        }
    });
    window.draw();
    let released_focus = released_focus
        .borrow()
        .clone()
        .expect("the released handle must be captured during construction");
    let _subscription = window.update({
        let events = events.clone();
        move |_, window, _| {
            window
                .observe_action_dispatch(move |event, _, _| events.borrow_mut().push(event.clone()))
        }
    });

    let report = window.update(|_, window, cx| {
        released_focus.dispatch_action(&UnhandledDispatchAction, window, cx)
    });

    assert_eq!(report.outcome(), ActionDispatchOutcome::Unhandled);
    assert!(matches!(events.borrow().as_slice(), [
        ActionDispatchEvent::Began(_),
        ActionDispatchEvent::Ended(ended),
    ] if ended == &report));
}
