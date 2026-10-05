use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::anyhow;
use calloop::{LoopHandle, ping::Ping};
use gpux::{RunnableVariant, profiler};

/// How long one idle pass runs foreground tasks before the event loop dispatches the input and
/// window events that arrived meanwhile.
const IDLE_PASS_BUDGET: Duration = Duration::from_millis(10);

/// Foreground tasks waiting for the event loop's idle phase.
///
/// Tasks run from a calloop idle callback, after the loop has dispatched input and window events,
/// for at most [`IDLE_PASS_BUDGET`] per pass; when tasks remain, the pass schedules the next one.
/// Scheduling a pass wakes the loop, so it never waits for an event while tasks are queued, and
/// events that arrived meanwhile are dispatched before the next pass.
pub(crate) struct ForegroundTasks<Data: 'static, Task: 'static> {
    handle: LoopHandle<'static, Data>,
    run: Box<dyn Fn(Task)>,
    queue: RefCell<VecDeque<Task>>,
    pass_scheduled: Cell<bool>,
    wake: Ping,
}

impl<Data, Task> ForegroundTasks<Data, Task> {
    /// Creates the queue on `handle`'s event loop; `run` runs one task.
    pub(crate) fn new(
        handle: LoopHandle<'static, Data>,
        run: impl Fn(Task) + 'static,
    ) -> anyhow::Result<Rc<Self>> {
        let (wake, wake_source) = calloop::ping::make_ping()?;
        // The wake-up only ends the loop's wait; the idle pass scheduled with it runs the tasks.
        handle
            .insert_source(wake_source, |_, _, _| {})
            .map_err(|error| {
                anyhow!(
                    "failed to register the foreground task wake-up: {}",
                    error.error
                )
            })?;
        Ok(Rc::new(Self {
            handle,
            run: Box::new(run),
            queue: RefCell::default(),
            pass_scheduled: Cell::new(false),
            wake,
        }))
    }

    /// Queues `task` behind the tasks already waiting.
    pub(crate) fn push(self: &Rc<Self>, task: Task) {
        self.queue.borrow_mut().push_back(task);
        self.schedule_pass();
    }

    fn schedule_pass(self: &Rc<Self>) {
        if !self.pass_scheduled.replace(true) {
            let tasks = Rc::clone(self);
            self.handle.insert_idle(move |_| tasks.run_pass());
            // Idle callbacks run after the loop's next wait, which must not block.
            self.wake.ping();
        }
    }

    fn run_pass(self: &Rc<Self>) {
        self.pass_scheduled.set(false);
        let start = Instant::now();
        while start.elapsed() < IDLE_PASS_BUDGET {
            let Some(task) = self.queue.borrow_mut().pop_front() else {
                return;
            };
            (self.run)(task);
        }
        if !self.queue.borrow().is_empty() {
            self.schedule_pass();
        }
    }
}

/// Runs a foreground runnable and records its timing for the profiler.
pub(crate) fn run_runnable(runnable: RunnableVariant) {
    let location = runnable.metadata().location;
    let spawned = runnable.metadata().spawned;
    profiler::update_running_task(spawned, location);
    runnable.run();
    profiler::save_task_timing();
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, thread, time::Duration, time::Instant};

    use calloop::{EventLoop, channel};

    use super::{ForegroundTasks, IDLE_PASS_BUDGET};

    type Task = Box<dyn FnOnce()>;
    type Log = Rc<RefCell<Vec<u32>>>;

    fn run(task: Task) {
        task();
    }

    /// Dispatches once with a long timeout, which queued tasks must not wait for.
    fn dispatch(event_loop: &mut EventLoop<'static, ()>) {
        let start = Instant::now();
        event_loop
            .dispatch(Duration::from_secs(10), &mut ())
            .unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the loop waited for an event while tasks were queued"
        );
    }

    fn logging(log: &Log, id: u32, duration: Duration) -> Task {
        let log = log.clone();
        Box::new(move || {
            thread::sleep(duration);
            log.borrow_mut().push(id);
        })
    }

    #[test]
    fn tasks_run_in_order_in_the_idle_phase() {
        let mut event_loop = EventLoop::<()>::try_new().unwrap();
        let tasks = ForegroundTasks::new(event_loop.handle(), run as fn(Task)).unwrap();
        let log = Log::default();
        for id in 1..=3 {
            tasks.push(logging(&log, id, Duration::ZERO));
        }
        assert!(log.borrow().is_empty());

        dispatch(&mut event_loop);

        assert_eq!(*log.borrow(), [1, 2, 3]);
    }

    /// A pass that uses up its budget leaves the rest of the tasks for later passes.
    #[test]
    fn a_pass_over_budget_leaves_the_rest_for_later_passes() {
        let mut event_loop = EventLoop::<()>::try_new().unwrap();
        let tasks = ForegroundTasks::new(event_loop.handle(), run as fn(Task)).unwrap();
        let log = Log::default();
        for id in 1..=3 {
            tasks.push(logging(&log, id, IDLE_PASS_BUDGET));
        }

        for expected in [vec![1], vec![1, 2], vec![1, 2, 3]] {
            dispatch(&mut event_loop);
            assert_eq!(*log.borrow(), expected);
        }
    }

    /// An event that arrives while tasks are queued is dispatched before the remaining tasks.
    #[test]
    fn events_are_dispatched_between_passes() {
        let mut event_loop = EventLoop::<()>::try_new().unwrap();
        let tasks = ForegroundTasks::new(event_loop.handle(), run as fn(Task)).unwrap();
        let log = Log::default();
        let (sender, channel) = channel::channel::<()>();
        event_loop
            .handle()
            .insert_source(channel, {
                let log = log.clone();
                move |event, _, _| {
                    if let channel::Event::Msg(()) = event {
                        log.borrow_mut().push(100);
                    }
                }
            })
            .unwrap();

        tasks.push({
            let log = log.clone();
            Box::new(move || {
                sender.send(()).unwrap();
                thread::sleep(IDLE_PASS_BUDGET);
                log.borrow_mut().push(1);
            })
        });
        for id in 2..=3 {
            tasks.push(logging(&log, id, IDLE_PASS_BUDGET));
        }

        while log.borrow().len() < 4 {
            dispatch(&mut event_loop);
        }

        assert_eq!(*log.borrow(), [1, 100, 2, 3]);
    }
}
