use std::{
    collections::VecDeque,
    fmt,
    iter::FusedIterator,
    sync::{Arc, atomic::AtomicUsize},
};

use rand::{Rng, SeedableRng, rngs::SmallRng};

use crate::Priority;

struct PriorityQueues<T> {
    high_priority: VecDeque<T>,
    medium_priority: VecDeque<T>,
    low_priority: VecDeque<T>,
}

impl<T> PriorityQueues<T> {
    fn is_empty(&self) -> bool {
        self.high_priority.is_empty()
            && self.medium_priority.is_empty()
            && self.low_priority.is_empty()
    }
}

struct PriorityQueueState<T> {
    queues: parking_lot::Mutex<PriorityQueues<T>>,
    #[cfg(not(target_family = "wasm"))]
    condvar: parking_lot::Condvar,
    #[cfg(target_family = "wasm")]
    sequence: std::sync::atomic::AtomicI32,
    receiver_count: AtomicUsize,
    sender_count: AtomicUsize,
}

impl<T> PriorityQueueState<T> {
    fn spin_lock(&self) -> parking_lot::MutexGuard<'_, PriorityQueues<T>> {
        loop {
            if let Some(guard) = self.queues.try_lock() {
                return guard;
            }
            std::hint::spin_loop();
        }
    }

    fn lock(&self) -> parking_lot::MutexGuard<'_, PriorityQueues<T>> {
        // No WASM caller may park, including diagnostics on the browser main thread.
        #[cfg(target_family = "wasm")]
        {
            self.spin_lock()
        }
        #[cfg(not(target_family = "wasm"))]
        {
            self.queues.lock()
        }
    }

    #[cfg(target_family = "wasm")]
    fn sequence_view(&self) -> js_sys::Int32Array {
        let memory = js_sys::WebAssembly::Memory::from(wasm_bindgen::memory());
        js_sys::Int32Array::new_with_byte_offset_and_length(
            &memory.buffer(),
            self.sequence.as_ptr() as u32,
            1,
        )
    }

    fn notify_one(&self) {
        #[cfg(not(target_family = "wasm"))]
        self.condvar.notify_one();
        #[cfg(target_family = "wasm")]
        {
            self.sequence
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            // Unlike Condvar notification, Atomics.notify never parks the sender.
            js_sys::Atomics::notify_with_count(&self.sequence_view(), 0, 1)
                .expect("queue sequence is an aligned live Int32Array");
        }
    }

    fn send(&self, priority: Priority, item: T) -> Result<(), SendError<T>> {
        if self
            .receiver_count
            .load(std::sync::atomic::Ordering::Relaxed)
            == 0
        {
            return Err(SendError(item));
        }

        let mut queues = self.lock();
        Self::push(&mut queues, priority, item);
        self.notify_one();
        Ok(())
    }

    fn spin_send(&self, priority: Priority, item: T) -> Result<(), SendError<T>> {
        if self
            .receiver_count
            .load(std::sync::atomic::Ordering::Relaxed)
            == 0
        {
            return Err(SendError(item));
        }

        let mut queues = self.spin_lock();
        Self::push(&mut queues, priority, item);
        self.notify_one();
        Ok(())
    }

    fn push(queues: &mut PriorityQueues<T>, priority: Priority, item: T) {
        match priority {
            Priority::RealtimeAudio => unreachable!(
                "Realtime audio priority runs on a dedicated thread and is never queued"
            ),
            Priority::High => queues.high_priority.push_back(item),
            Priority::Medium => queues.medium_priority.push_back(item),
            Priority::Low => queues.low_priority.push_back(item),
        };
    }

    fn recv<'a>(&'a self) -> Result<parking_lot::MutexGuard<'a, PriorityQueues<T>>, RecvError> {
        let mut queues = self.lock();

        let sender_count = self.sender_count.load(std::sync::atomic::Ordering::Relaxed);
        if queues.is_empty() && sender_count == 0 {
            return Err(crate::queue::RecvError);
        }

        while queues.is_empty() {
            #[cfg(not(target_family = "wasm"))]
            self.condvar.wait(&mut queues);
            #[cfg(target_family = "wasm")]
            {
                if self.sender_count.load(std::sync::atomic::Ordering::Acquire) == 0 {
                    return Err(RecvError);
                }
                // Capture under the queue lock. A send between unlocking and waiting
                // changes this value, so Atomics.wait returns without losing the wake.
                let observed = self.sequence.load(std::sync::atomic::Ordering::Acquire);
                drop(queues);
                // Blocking pop is worker-only; main-thread callers use try_pop.
                js_sys::Atomics::wait(&self.sequence_view(), 0, observed)
                    .expect("blocking queue receive requires a worker and shared memory");
                queues = self.lock();
            }
        }

        Ok(queues)
    }

    fn try_recv<'a>(
        &'a self,
    ) -> Result<Option<parking_lot::MutexGuard<'a, PriorityQueues<T>>>, RecvError> {
        let mut queues = self.lock();

        let sender_count = self.sender_count.load(std::sync::atomic::Ordering::Relaxed);
        if queues.is_empty() && sender_count == 0 {
            return Err(crate::queue::RecvError);
        }

        if queues.is_empty() {
            Ok(None)
        } else {
            Ok(Some(queues))
        }
    }

    fn spin_try_recv<'a>(
        &'a self,
    ) -> Result<Option<parking_lot::MutexGuard<'a, PriorityQueues<T>>>, RecvError> {
        let queues = self.spin_lock();

        let sender_count = self.sender_count.load(std::sync::atomic::Ordering::Relaxed);
        if queues.is_empty() && sender_count == 0 {
            return Err(crate::queue::RecvError);
        }

        if queues.is_empty() {
            Ok(None)
        } else {
            Ok(Some(queues))
        }
    }
}

#[doc(hidden)]
pub struct PriorityQueueSender<T> {
    state: Arc<PriorityQueueState<T>>,
}

impl<T> PriorityQueueSender<T> {
    fn new(state: Arc<PriorityQueueState<T>>) -> Self {
        Self { state }
    }

    pub fn send(&self, priority: Priority, item: T) -> Result<(), SendError<T>> {
        self.state.send(priority, item)?;
        Ok(())
    }

    pub fn spin_send(&self, priority: Priority, item: T) -> Result<(), SendError<T>> {
        self.state.spin_send(priority, item)?;
        Ok(())
    }
}

impl<T> Drop for PriorityQueueSender<T> {
    fn drop(&mut self) {
        let previous = self
            .state
            .sender_count
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        #[cfg(target_family = "wasm")]
        if previous == 1 {
            // Synchronize with the empty check and sequence snapshot in recv.
            let _queues = self.state.lock();
            self.state
                .sequence
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            js_sys::Atomics::notify(&self.state.sequence_view(), 0)
                .expect("queue sequence is an aligned live Int32Array");
        }
        #[cfg(not(target_family = "wasm"))]
        let _ = previous;
    }
}

#[doc(hidden)]
pub struct PriorityQueueReceiver<T> {
    state: Arc<PriorityQueueState<T>>,
    rand: SmallRng,
    disconnected: bool,
}

impl<T> Clone for PriorityQueueReceiver<T> {
    fn clone(&self) -> Self {
        self.state
            .receiver_count
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Self {
            state: Arc::clone(&self.state),
            rand: SmallRng::seed_from_u64(0),
            disconnected: self.disconnected,
        }
    }
}

#[doc(hidden)]
pub struct SendError<T>(pub T);

impl<T: fmt::Debug> fmt::Debug for SendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SendError").field(&self.0).finish()
    }
}

#[derive(Debug)]
#[doc(hidden)]
pub struct RecvError;

#[allow(dead_code)]
impl<T> PriorityQueueReceiver<T> {
    pub fn new() -> (PriorityQueueSender<T>, Self) {
        let state = PriorityQueueState {
            queues: parking_lot::Mutex::new(PriorityQueues {
                high_priority: VecDeque::new(),
                medium_priority: VecDeque::new(),
                low_priority: VecDeque::new(),
            }),
            #[cfg(not(target_family = "wasm"))]
            condvar: parking_lot::Condvar::new(),
            #[cfg(target_family = "wasm")]
            sequence: std::sync::atomic::AtomicI32::new(0),
            receiver_count: AtomicUsize::new(1),
            sender_count: AtomicUsize::new(1),
        };
        let state = Arc::new(state);

        let sender = PriorityQueueSender::new(Arc::clone(&state));

        let receiver = PriorityQueueReceiver {
            state,
            rand: SmallRng::seed_from_u64(0),
            disconnected: false,
        };

        (sender, receiver)
    }

    /// Returns whether the queue currently contains no elements.
    pub fn is_empty(&self) -> bool {
        self.state.lock().is_empty()
    }

    /// Returns the number of queued elements across all priorities.
    pub(crate) fn len(&self) -> usize {
        let queues = self.state.lock();
        queues.high_priority.len() + queues.medium_priority.len() + queues.low_priority.len()
    }

    /// Tries to pop one element from the priority queue without blocking.
    ///
    /// This will early return if there are no elements in the queue.
    ///
    /// This method is best suited if you only intend to pop one element, for better performance
    /// on large queues see [`Self::try_iter`]
    ///
    /// # Errors
    ///
    /// If the sender was dropped
    pub fn try_pop(&mut self) -> Result<Option<T>, RecvError> {
        self.pop_inner(false)
    }

    pub fn spin_try_pop(&mut self) -> Result<Option<T>, RecvError> {
        use Priority as P;

        let Some(mut queues) = self.state.spin_try_recv()? else {
            return Ok(None);
        };

        let high = P::High.weight() * !queues.high_priority.is_empty() as u32;
        let medium = P::Medium.weight() * !queues.medium_priority.is_empty() as u32;
        let low = P::Low.weight() * !queues.low_priority.is_empty() as u32;
        let mut mass = high + medium + low;

        if !queues.high_priority.is_empty() {
            let flip = self.rand.random_ratio(P::High.weight(), mass);
            if flip {
                return Ok(queues.high_priority.pop_front());
            }
            mass -= P::High.weight();
        }

        if !queues.medium_priority.is_empty() {
            let flip = self.rand.random_ratio(P::Medium.weight(), mass);
            if flip {
                return Ok(queues.medium_priority.pop_front());
            }
            mass -= P::Medium.weight();
        }

        if !queues.low_priority.is_empty() {
            let flip = self.rand.random_ratio(P::Low.weight(), mass);
            if flip {
                return Ok(queues.low_priority.pop_front());
            }
        }

        Ok(None)
    }

    /// Pops an element from the priority queue blocking if necessary.
    ///
    /// This method is best suited if you only intend to pop one element, for better performance
    /// on large queues see [`Self::iter``]
    ///
    /// # Errors
    ///
    /// If the sender was dropped
    pub fn pop(&mut self) -> Result<T, RecvError> {
        self.pop_inner(true).map(|e| e.unwrap())
    }

    /// Returns an iterator over the elements of the queue
    /// this iterator will end when all elements have been consumed and will not wait for new ones.
    pub fn try_iter(self) -> TryIter<T> {
        TryIter {
            receiver: self,
            ended: false,
        }
    }

    /// Returns an iterator over the elements of the queue
    /// this iterator will wait for new elements if the queue is empty.
    pub fn iter(self) -> Iter<T> {
        Iter(self)
    }

    #[inline(always)]
    // algorithm is the loaded die from biased coin from
    // https://www.keithschwarz.com/darts-dice-coins/
    fn pop_inner(&mut self, block: bool) -> Result<Option<T>, RecvError> {
        use Priority as P;

        let mut queues = if !block {
            let Some(queues) = self.state.try_recv()? else {
                return Ok(None);
            };
            queues
        } else {
            self.state.recv()?
        };

        let high = P::High.weight() * !queues.high_priority.is_empty() as u32;
        let medium = P::Medium.weight() * !queues.medium_priority.is_empty() as u32;
        let low = P::Low.weight() * !queues.low_priority.is_empty() as u32;
        let mut mass = high + medium + low; //%

        if !queues.high_priority.is_empty() {
            let flip = self.rand.random_ratio(P::High.weight(), mass);
            if flip {
                return Ok(queues.high_priority.pop_front());
            }
            mass -= P::High.weight();
        }

        if !queues.medium_priority.is_empty() {
            let flip = self.rand.random_ratio(P::Medium.weight(), mass);
            if flip {
                return Ok(queues.medium_priority.pop_front());
            }
            mass -= P::Medium.weight();
        }

        if !queues.low_priority.is_empty() {
            let flip = self.rand.random_ratio(P::Low.weight(), mass);
            if flip {
                return Ok(queues.low_priority.pop_front());
            }
        }

        Ok(None)
    }
}

impl<T> Drop for PriorityQueueReceiver<T> {
    fn drop(&mut self) {
        self.state
            .receiver_count
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

#[doc(hidden)]
pub struct Iter<T>(PriorityQueueReceiver<T>);
impl<T> Iterator for Iter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.pop().ok()
    }
}
impl<T> FusedIterator for Iter<T> {}

#[doc(hidden)]
pub struct TryIter<T> {
    receiver: PriorityQueueReceiver<T>,
    ended: bool,
}
impl<T> Iterator for TryIter<T> {
    type Item = Result<T, RecvError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.ended {
            return None;
        }

        let res = self.receiver.try_pop();
        self.ended = res.is_err();

        res.transpose()
    }
}
impl<T> FusedIterator for TryIter<T> {}

#[cfg(test)]
mod tests {
    use collections::HashSet;

    use super::*;

    #[test]
    fn all_tasks_get_yielded() {
        let (tx, mut rx) = PriorityQueueReceiver::new();
        tx.send(Priority::Medium, 20).unwrap();
        tx.send(Priority::High, 30).unwrap();
        tx.send(Priority::Low, 10).unwrap();
        tx.send(Priority::Medium, 21).unwrap();
        tx.send(Priority::High, 31).unwrap();

        drop(tx);

        assert_eq!(
            rx.iter().collect::<HashSet<_>>(),
            [30, 31, 20, 21, 10].into_iter().collect::<HashSet<_>>()
        )
    }

    #[test]
    fn new_high_prio_task_get_scheduled_quickly() {
        let (tx, mut rx) = PriorityQueueReceiver::new();
        for _ in 0..100 {
            tx.send(Priority::Low, 1).unwrap();
        }

        assert_eq!(rx.pop().unwrap(), 1);
        tx.send(Priority::High, 3).unwrap();
        assert_eq!(rx.pop().unwrap(), 3);
        assert_eq!(rx.pop().unwrap(), 1);
    }
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn concurrent_sends_preserve_each_producers_fifo_and_all_items() {
        let (tx, mut rx) = PriorityQueueReceiver::new();
        let tx = Arc::new(tx);
        std::thread::scope(|scope| {
            for producer in 0..4 {
                let tx = tx.clone();
                scope.spawn(move || {
                    for value in 0..1000 {
                        if value % 2 == 0 {
                            tx.send(Priority::High, (producer, value)).unwrap();
                        } else {
                            tx.spin_send(Priority::High, (producer, value)).unwrap();
                        }
                    }
                });
            }
            let mut expected = [0; 4];
            for _ in 0..4000 {
                let (producer, value) = rx.pop().unwrap();
                assert_eq!(value, expected[producer]);
                expected[producer] += 1;
            }
            assert_eq!(expected, [1000; 4]);
        });
        drop(tx);
        assert!(rx.try_pop().is_err());
    }
}
