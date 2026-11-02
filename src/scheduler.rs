//! Scheduler policies for generated state machines.
//!
//! The scheduler slot decides where a generated machine's state lives and who
//! runs its events. The default `()` scheduler keeps today's owned machine:
//! the caller holds it and processes events through `&mut self`. A queued
//! scheduler moves the machine behind a shared cell, so the generated machine
//! becomes a cloneable handle whose `process_event_async(&self, event)` queues
//! the event at call time and resolves to its acceptance once it has run to
//! completion.
//!
//! Every queued scheduler satisfies the `sml.cpp` strict-ordering contract:
//! events run in FIFO order, one at a time, each to completion before the next
//! starts. [`StrictOrdering`] states the contract at the type level.

use crate::{StepHook, Terminated};

/// The scheduler policy slot.
///
/// `Cell<M>` is the storage a generated machine uses for its private core `M`.
/// The default `()` scheduler stores the core by value, preserving the owned,
/// `&mut self` machine.
pub trait Scheduler {
    /// Storage for a generated machine core.
    type Cell<M>;
}

impl Scheduler for () {
    type Cell<M> = M;
}

/// Places a generated machine core into a scheduler's storage.
pub trait Schedule<M: Terminated>: Scheduler {
    /// Moves `machine` into this scheduler's storage. A queued scheduler also
    /// runs the machine's initial entry here, as `sml.cpp` does on
    /// construction.
    fn cell(self, machine: M) -> Self::Cell<M>;
}

impl<M: Terminated> Schedule<M> for () {
    #[inline]
    fn cell(self, machine: M) -> M {
        machine
    }
}

/// A scheduler that queues events for a shared machine.
///
/// [`Queued::submit`] queues the event before it returns. Dropping the
/// returned completion does not cancel the event: it still runs, and only its
/// result is discarded. Once the machine reaches `X`, queued and later events
/// resolve to `false` without running.
pub trait Queued<M: SyncMachineStep>: Schedule<M> + StrictOrdering {
    /// Resolves to the event's acceptance after it has run to completion.
    type Completion: core::future::Future<Output = bool>;

    /// Queues `event`, waiting for room when the queue is bounded and full.
    fn submit(cell: &Self::Cell<M>, event: M::Event) -> Self::Completion;

    /// Queues `event` or reports that the queue has no room.
    ///
    /// # Errors
    ///
    /// Returns [`QueueFull`](crate::utility::QueueFull) when a bounded queue
    /// has no remaining capacity. The event is not queued.
    fn try_submit(
        cell: &Self::Cell<M>,
        event: M::Event,
    ) -> Result<Self::Completion, crate::utility::QueueFull>;
}

/// The `sml.cpp` `strict_ordering_scheduler_contract`.
///
/// A queued scheduler must guarantee FIFO order, a single consumer, and
/// run-to-completion. The constants exist so a generic owner can assert the
/// contract; every scheduler in this crate sets all three to `true`.
pub trait StrictOrdering {
    /// Events run in the order they were queued.
    const GUARANTEES_FIFO: bool;
    /// Only one event runs at a time.
    const SINGLE_CONSUMER: bool;
    /// Each event, including the machine's own `process`/`defer` queues and
    /// completion transitions, finishes before the next one starts.
    const RUN_TO_COMPLETION: bool;
}

/// A generated machine core whose callbacks are synchronous, as a queued
/// scheduler drives it.
///
/// Generated machines implement this for their private core. It is public so
/// owners can write their own [`Scheduler`].
pub trait SyncMachineStep: Terminated {
    /// The generated event enum.
    type Event;

    /// Runs the initial entry and anonymous-transition stabilization.
    fn start(&mut self) -> bool;

    /// Runs one event to completion, including the per-step hook, and
    /// reports whether it was accepted.
    fn step(&mut self, event: Self::Event) -> bool;
}

/// Summary handed to [`StepHook::after_step`] after each event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// The processed event's name.
    pub event: &'static str,
    /// Whether the machine accepted the event.
    pub accepted: bool,
    /// Whether the machine is now in its terminal `X` state.
    pub terminated: bool,
}

impl<L, E> StepHook<L, E> for () {
    type Output = ();
    const ACTIVE: bool = false;
}

#[cfg(feature = "std")]
mod shared {
    extern crate std;

    use super::{Queued, Schedule, Scheduler, StrictOrdering, SyncMachineStep};
    use crate::utility::{EventQueue, QueueFull};
    use crate::Terminated;
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Context, Poll, Waker};
    use std::boxed::Box;
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
    use std::vec::Vec;

    // A panic inside a callback poisons the lock. The scheduler still hands
    // the data back: the poisoned machine stays where it was, and every later
    // caller observes the same state the panicking caller left.
    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs each event in the calling task, under the machine's lock.
    ///
    /// This is the `sml.cpp` `inline_scheduler` for a shared machine: the
    /// event runs before `process_event_async` returns, and the completion is
    /// already resolved. A callback that calls back into its own machine
    /// deadlocks; use [`FifoScheduler`] for re-entrant machines.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct InlineScheduler;

    /// Shared storage for an [`InlineScheduler`] machine.
    pub struct InlineCell<M> {
        machine: Arc<Mutex<M>>,
    }

    impl<M> Clone for InlineCell<M> {
        fn clone(&self) -> Self {
            Self {
                machine: Arc::clone(&self.machine),
            }
        }
    }

    impl Scheduler for InlineScheduler {
        type Cell<M> = InlineCell<M>;
    }

    impl StrictOrdering for InlineScheduler {
        const GUARANTEES_FIFO: bool = true;
        const SINGLE_CONSUMER: bool = true;
        const RUN_TO_COMPLETION: bool = true;
    }

    impl<M: SyncMachineStep> Schedule<M> for InlineScheduler {
        fn cell(self, mut machine: M) -> InlineCell<M> {
            let _ = machine.start();
            InlineCell {
                machine: Arc::new(Mutex::new(machine)),
            }
        }
    }

    impl<M: SyncMachineStep> Queued<M> for InlineScheduler {
        type Completion = core::future::Ready<bool>;

        fn submit(cell: &InlineCell<M>, event: M::Event) -> Self::Completion {
            let accepted = {
                let mut machine = lock(&cell.machine);
                !machine.is_terminated() && machine.step(event)
            };
            core::future::ready(accepted)
        }

        fn try_submit(
            cell: &InlineCell<M>,
            event: M::Event,
        ) -> Result<Self::Completion, QueueFull> {
            Ok(Self::submit(cell, event))
        }
    }

    /// A bounded FIFO scheduler with the `sml.cpp` draining model.
    ///
    /// A caller that finds the machine idle runs its event at once and then
    /// drains whatever other callers queued meanwhile. A caller that finds
    /// the machine busy, including an action re-entering its own machine,
    /// only queues its event; the draining caller runs it. `N` bounds the
    /// queue and must be a power of two greater than one.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct FifoScheduler<const N: usize = 1024>;

    /// Shared storage for a [`FifoScheduler`] machine.
    pub struct FifoCell<M, const N: usize> {
        shared: Arc<FifoShared<M, N>>,
    }

    impl<M, const N: usize> Clone for FifoCell<M, N> {
        fn clone(&self) -> Self {
            Self {
                shared: Arc::clone(&self.shared),
            }
        }
    }

    // A queued event, type-erased like `sml.cpp`'s fifo task slots so the
    // cell does not name the machine's event type.
    type Job<M> = Box<dyn FnOnce(&mut M) -> bool + Send>;

    struct FifoShared<M, const N: usize> {
        // Locked only by the draining caller, so it is never contended.
        machine: Mutex<M>,
        queue: Mutex<FifoQueue<M, N>>,
    }

    struct FifoQueue<M, const N: usize> {
        events: EventQueue<(Job<M>, Arc<Ticket>), N>,
        draining: bool,
        terminated: bool,
        room: Vec<Waker>,
    }

    #[derive(Default)]
    struct Ticket {
        state: Mutex<TicketState>,
    }

    #[derive(Default)]
    struct TicketState {
        accepted: Option<bool>,
        waker: Option<Waker>,
    }

    impl Ticket {
        fn resolve(&self, accepted: bool) {
            let waker = {
                let mut state = lock(&self.state);
                state.accepted = Some(accepted);
                state.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        }

        fn poll(&self, cx: &Context<'_>) -> Poll<bool> {
            let mut state = lock(&self.state);
            if let Some(accepted) = state.accepted {
                return Poll::Ready(accepted);
            }
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    // Resolves its event's caller to `false` unless the step completed, so a
    // panicking step never leaves a caller waiting.
    struct Pending(Option<Arc<Ticket>>);

    impl Pending {
        // Runs `job` unless the machine already terminated; returns its
        // acceptance and whether the machine is now terminated.
        fn run<M: SyncMachineStep, const N: usize>(
            mut self,
            cell: &FifoCell<M, N>,
            job: impl FnOnce(&mut M) -> bool,
            terminated: bool,
        ) -> (bool, bool)
        where
            M::Event: Send + 'static,
        {
            let outcome = if terminated {
                (false, true)
            } else {
                cell.run(job)
            };
            if let Some(ticket) = self.0.take() {
                ticket.resolve(outcome.0);
            }
            outcome
        }
    }

    impl Drop for Pending {
        fn drop(&mut self) {
            if let Some(ticket) = self.0.take() {
                ticket.resolve(false);
            }
        }
    }

    // When a step panics, hands draining to the next caller: wakes callers
    // waiting for room, records whether the panicking step reached `X`, and
    // if it did, resolves every event still queued to `false`.
    struct Unwinding<'a, M: Terminated, const N: usize>(&'a FifoShared<M, N>);

    impl<M: Terminated, const N: usize> Drop for Unwinding<'_, M, N> {
        fn drop(&mut self) {
            if !std::thread::panicking() {
                return;
            }
            let terminated = lock(&self.0.machine).is_terminated();
            let (room, stranded) = {
                let mut queue = lock(&self.0.queue);
                queue.draining = false;
                queue.terminated |= terminated;
                let mut stranded = Vec::new();
                if queue.terminated {
                    while let Some((_, ticket)) = queue.events.pop() {
                        stranded.push(ticket);
                    }
                }
                (core::mem::take(&mut queue.room), stranded)
            };
            for ticket in stranded {
                ticket.resolve(false);
            }
            for waker in room {
                waker.wake();
            }
        }
    }

    impl<const N: usize> FifoScheduler<N> {
        const VALID_CAPACITY: () = assert!(
            N > 1 && N.is_power_of_two(),
            "FifoScheduler capacity must be a power of two greater than one"
        );
    }

    impl<const N: usize> Scheduler for FifoScheduler<N> {
        type Cell<M> = FifoCell<M, N>;
    }

    impl<const N: usize> StrictOrdering for FifoScheduler<N> {
        const GUARANTEES_FIFO: bool = true;
        const SINGLE_CONSUMER: bool = true;
        const RUN_TO_COMPLETION: bool = true;
    }

    impl<M: SyncMachineStep, const N: usize> Schedule<M> for FifoScheduler<N> {
        fn cell(self, mut machine: M) -> FifoCell<M, N> {
            let () = Self::VALID_CAPACITY;
            let _ = machine.start();
            let terminated = machine.is_terminated();
            FifoCell {
                shared: Arc::new(FifoShared {
                    machine: Mutex::new(machine),
                    queue: Mutex::new(FifoQueue {
                        events: EventQueue::new(),
                        draining: false,
                        terminated,
                        room: Vec::new(),
                    }),
                }),
            }
        }
    }

    /// Outcome of offering an event to a [`FifoCell`].
    enum Offer<E> {
        Done(bool),
        Queued(Arc<Ticket>),
        Full(E),
    }

    impl<M: SyncMachineStep, const N: usize> FifoCell<M, N>
    where
        M::Event: Send + 'static,
    {
        fn offer(&self, event: M::Event, waker: Option<&Waker>) -> Offer<M::Event> {
            let mut queue = lock(&self.shared.queue);
            if queue.terminated {
                return Offer::Done(false);
            }
            if queue.draining {
                if queue.events.len() == N {
                    if let Some(waker) = waker {
                        queue.room.push(waker.clone());
                    }
                    return Offer::Full(event);
                }
                let ticket = Arc::new(Ticket::default());
                let job: Job<M> = Box::new(move |machine: &mut M| machine.step(event));
                if queue.events.defer((job, Arc::clone(&ticket))).is_err() {
                    // `len() < N` was checked under the same lock.
                    return Offer::Done(false);
                }
                return Offer::Queued(ticket);
            }
            queue.draining = true;
            if let Some((older, older_ticket)) = queue.events.pop() {
                let job: Job<M> = Box::new(move |machine: &mut M| machine.step(event));
                // A drainer unwound from a panicking step and left events
                // queued; they are older than this one, so it queues behind
                // them in the slot the pop freed.
                let ticket = Arc::new(Ticket::default());
                if queue.events.defer((job, Arc::clone(&ticket))).is_err() {
                    ticket.resolve(false);
                }
                drop(queue);
                self.drain(older, Some(older_ticket));
                return Offer::Queued(ticket);
            }
            drop(queue);
            Offer::Done(self.drain(move |machine: &mut M| machine.step(event), None))
        }

        // Runs `first`, then every event queued meanwhile, and leaves the
        // machine idle. Only the caller that set `draining` gets here, so the
        // machine lock is never contended. A panicking step unwinds through
        // the caller draining at that moment; the guards resolve the
        // panicking event's caller to `false` and hand draining to the next
        // caller, which runs what is still queued first.
        fn drain(&self, first: impl FnOnce(&mut M) -> bool, ticket: Option<Arc<Ticket>>) -> bool {
            let _unwinding = Unwinding(&self.shared);
            let (accepted, mut terminated) = Pending(ticket).run(self, first, false);
            loop {
                let (next, room) = {
                    let mut queue = lock(&self.shared.queue);
                    queue.terminated = terminated;
                    let next = queue.events.pop();
                    if next.is_none() {
                        queue.draining = false;
                    }
                    (next, core::mem::take(&mut queue.room))
                };
                for waker in room {
                    waker.wake();
                }
                let Some((job, ticket)) = next else {
                    return accepted;
                };
                terminated = Pending(Some(ticket)).run(self, job, terminated).1;
            }
        }

        fn run(&self, job: impl FnOnce(&mut M) -> bool) -> (bool, bool) {
            let mut machine = lock(&self.shared.machine);
            let accepted = job(&mut machine);
            (accepted, machine.is_terminated())
        }
    }

    /// Completion of an event submitted to a [`FifoScheduler`].
    ///
    /// When the bounded queue was full at submission, the completion waits
    /// for room and queues the event when polled with room available; until
    /// then the event is not queued, and dropping the completion discards it.
    /// `try_process_event` reports a full queue instead.
    pub struct FifoCompletion<M: SyncMachineStep, const N: usize> {
        state: FifoWait<M, N>,
    }

    enum FifoWait<M: SyncMachineStep, const N: usize> {
        Done(bool),
        Queued(Arc<Ticket>),
        // The queue was full: the event is queued once there is room.
        Room(FifoCell<M, N>, M::Event),
    }

    // The completion never pins its fields; the waiting event is moved out
    // by value when room appears.
    impl<M: SyncMachineStep, const N: usize> Unpin for FifoCompletion<M, N> {}

    impl<M: SyncMachineStep, const N: usize> Future for FifoCompletion<M, N>
    where
        M::Event: Send + 'static,
    {
        type Output = bool;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<bool> {
            let this = self.get_mut();
            // Take the state out so a waiting event can move into the queue.
            this.state = match core::mem::replace(&mut this.state, FifoWait::Done(false)) {
                FifoWait::Room(cell, event) => match cell.offer(event, Some(cx.waker())) {
                    Offer::Done(accepted) => FifoWait::Done(accepted),
                    Offer::Queued(ticket) => FifoWait::Queued(ticket),
                    Offer::Full(event) => FifoWait::Room(cell, event),
                },
                state => state,
            };
            match &this.state {
                FifoWait::Done(accepted) => Poll::Ready(*accepted),
                FifoWait::Queued(ticket) => ticket.poll(cx),
                FifoWait::Room(..) => Poll::Pending,
            }
        }
    }

    impl<M: SyncMachineStep, const N: usize> Queued<M> for FifoScheduler<N>
    where
        M::Event: Send + 'static,
    {
        type Completion = FifoCompletion<M, N>;

        fn submit(cell: &FifoCell<M, N>, event: M::Event) -> Self::Completion {
            let state = match cell.offer(event, None) {
                Offer::Done(accepted) => FifoWait::Done(accepted),
                Offer::Queued(ticket) => FifoWait::Queued(ticket),
                Offer::Full(event) => FifoWait::Room(cell.clone(), event),
            };
            FifoCompletion { state }
        }

        fn try_submit(
            cell: &FifoCell<M, N>,
            event: M::Event,
        ) -> Result<Self::Completion, QueueFull> {
            match cell.offer(event, None) {
                Offer::Done(accepted) => Ok(FifoCompletion {
                    state: FifoWait::Done(accepted),
                }),
                Offer::Queued(ticket) => Ok(FifoCompletion {
                    state: FifoWait::Queued(ticket),
                }),
                Offer::Full(_) => Err(QueueFull),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::tests::{poll, Recorder};
        use super::{
            lock, FifoScheduler, InlineScheduler, Queued, Schedule, StrictOrdering, SyncMachineStep,
        };
        use core::task::Poll;
        extern crate std;
        use std::boxed::Box;

        #[test]
        fn a_busy_fifo_queues_bounded_and_resolves_after_the_drain() {
            const { assert!(<FifoScheduler<2> as StrictOrdering>::GUARANTEES_FIFO) };
            let cell =
                <FifoScheduler<2> as Schedule<Recorder>>::cell(FifoScheduler, Recorder::default());
            assert!(lock(&cell.shared.machine).started);
            // Another caller is draining, so these only queue.
            lock(&cell.shared.queue).draining = true;
            let mut first = Box::pin(<FifoScheduler<2> as Queued<Recorder>>::submit(&cell, 1));
            assert!(<FifoScheduler<2> as Queued<Recorder>>::try_submit(&cell, 2).is_ok());
            assert!(<FifoScheduler<2> as Queued<Recorder>>::try_submit(&cell, 3).is_err());
            let mut waiting = Box::pin(<FifoScheduler<2> as Queued<Recorder>>::submit(&cell, 3));
            assert_eq!(poll(first.as_mut()), Poll::Pending);
            assert_eq!(poll(waiting.as_mut()), Poll::Pending);
            // Room appears while the machine is still busy: the waiting event
            // queues instead of running.
            let popped = lock(&cell.shared.queue).events.pop();
            assert_eq!(poll(waiting.as_mut()), Poll::Pending);
            let ran = popped.map(|(job, ticket)| {
                let accepted = job(&mut lock(&cell.shared.machine));
                ticket.resolve(accepted);
                accepted
            });
            assert_eq!(ran, Some(true));

            assert!(cell.drain(
                Box::new(|machine: &mut Recorder| SyncMachineStep::step(machine, 9)),
                None
            ));
            assert_eq!(poll(first.as_mut()), Poll::Ready(true));
            assert_eq!(poll(waiting.as_mut()), Poll::Ready(true));
            assert_eq!(lock(&cell.shared.machine).seen, [1, 9, 2, 3]);

            let stop = <FifoScheduler<2> as Queued<Recorder>>::submit(&cell, 0);
            assert_eq!(poll(stop), Poll::Ready(true));
            assert!(lock(&cell.shared.queue).terminated);
        }

        #[test]
        fn a_panicking_step_hands_draining_to_the_next_caller() {
            let cell =
                <FifoScheduler<2> as Schedule<Recorder>>::cell(FifoScheduler, Recorder::default());
            lock(&cell.shared.queue).draining = true;
            let mut panicked = Box::pin(<FifoScheduler<2> as Queued<Recorder>>::submit(&cell, 7));
            let mut later = Box::pin(<FifoScheduler<2> as Queued<Recorder>>::submit(&cell, 8));
            let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cell.drain(
                    Box::new(|machine: &mut Recorder| SyncMachineStep::step(machine, 1)),
                    None,
                )
            }));
            assert!(unwound.is_err());
            assert!(!lock(&cell.shared.queue).draining);
            assert_eq!(poll(panicked.as_mut()), Poll::Ready(false));
            assert_eq!(poll(later.as_mut()), Poll::Pending);

            // The next caller drains: the event left queued runs before its own.
            let next = <FifoScheduler<2> as Queued<Recorder>>::submit(&cell, 2);
            assert_eq!(poll(next), Poll::Ready(true));
            assert_eq!(poll(later.as_mut()), Poll::Ready(true));
            assert_eq!(lock(&cell.shared.machine).seen, [1, 8, 2]);
        }

        #[test]
        fn a_step_panicking_into_x_wakes_waiters_and_strands_nothing() {
            let cell =
                <FifoScheduler<2> as Schedule<Recorder>>::cell(FifoScheduler, Recorder::default());
            lock(&cell.shared.queue).draining = true;
            let mut stopping = Box::pin(<FifoScheduler<2> as Queued<Recorder>>::submit(&cell, 6));
            let mut queued = Box::pin(<FifoScheduler<2> as Queued<Recorder>>::submit(&cell, 8));
            let mut waiting = Box::pin(<FifoScheduler<2> as Queued<Recorder>>::submit(&cell, 9));
            assert_eq!(poll(waiting.as_mut()), Poll::Pending);
            assert!(!lock(&cell.shared.queue).room.is_empty());
            let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cell.drain(
                    Box::new(|machine: &mut Recorder| SyncMachineStep::step(machine, 1)),
                    None,
                )
            }));
            assert!(unwound.is_err());
            assert!(lock(&cell.shared.queue).terminated);
            assert!(!lock(&cell.shared.queue).draining);
            assert!(lock(&cell.shared.queue).room.is_empty());
            assert_eq!(poll(stopping.as_mut()), Poll::Ready(false));
            assert_eq!(poll(queued.as_mut()), Poll::Ready(false));
            assert_eq!(poll(waiting.as_mut()), Poll::Ready(false));
            assert_eq!(lock(&cell.shared.machine).seen, [1]);
        }

        #[test]
        fn an_inline_machine_starts_and_stops() {
            const { assert!(<InlineScheduler as StrictOrdering>::SINGLE_CONSUMER) };
            let cell =
                <InlineScheduler as Schedule<Recorder>>::cell(InlineScheduler, Recorder::default());
            let clone = cell.clone();
            assert!(lock(&clone.machine).started);
            let stop = <InlineScheduler as Queued<Recorder>>::submit(&cell, 0);
            assert_eq!(poll(stop), Poll::Ready(true));
            assert!(crate::Terminated::is_terminated(&*lock(&cell.machine)));
            let refused = <InlineScheduler as Queued<Recorder>>::try_submit(&cell, 1);
            assert_eq!(refused.ok().map(poll), Some(Poll::Ready(false)));
        }
    }
}

#[cfg(feature = "std")]
pub use shared::{FifoCell, FifoCompletion, FifoScheduler, InlineCell, InlineScheduler};

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{Schedule, Step, SyncMachineStep};
    use crate::{
        DefaultQueuePolicy, JumpTable, NoPolicy, Policies, PolicyBundle, PolicyParts, StepHook,
        Terminated,
    };
    use std::vec::Vec;

    /// Records each event; event `0` stops the machine.
    #[derive(Default)]
    pub(super) struct Recorder {
        pub(super) started: bool,
        pub(super) seen: Vec<u8>,
        stopped: bool,
    }

    impl Terminated for Recorder {
        fn is_terminated(&self) -> bool {
            self.stopped
        }
    }

    impl SyncMachineStep for Recorder {
        type Event = u8;

        fn start(&mut self) -> bool {
            self.started = true;
            true
        }

        fn step(&mut self, event: u8) -> bool {
            if event == 6 {
                self.stopped = true;
            }
            if event == 6 || event == 7 {
                std::panic::resume_unwind(std::boxed::Box::new("a step panicked"));
            }
            self.seen.push(event);
            self.stopped = event == 0;
            true
        }
    }

    #[cfg(feature = "std")]
    pub(super) fn poll<F: core::future::Future>(future: F) -> core::task::Poll<F::Output> {
        let waker = core::task::Waker::noop();
        core::pin::pin!(future).poll(&mut core::task::Context::from_waker(waker))
    }

    #[test]
    fn the_unit_scheduler_and_hook_keep_the_owned_machine() {
        let machine = <() as Schedule<Recorder>>::cell((), Recorder::default());
        const { assert!(!<() as StepHook<(), u8>>::ACTIVE) };
        StepHook::<(), u8>::before_step(&mut (), &mut (), &1);
        StepHook::<(), u8>::output(&mut (), &mut (), ());
        let step = Step {
            event: "event",
            accepted: true,
            terminated: false,
        };
        StepHook::<(), u8>::after_step(&mut (), &mut (), step);
        let mut context = Recorder::default();
        crate::Emit::<()>::drain_outputs(&mut context, drop);
        assert!(!machine.is_terminated());
        assert!(machine.seen.is_empty());

        let (mut parts, (), ()) = Policies::into_parts(NoPolicy::default());
        let ((), ()) = {
            let (hook, logger) = PolicyParts::step_hook_mut(&mut parts);
            (*hook, *logger)
        };
        let bundle = PolicyBundle::<(), (), JumpTable>::new(
            (),
            (),
            (),
            (),
            DefaultQueuePolicy,
            DefaultQueuePolicy,
        );
        let (mut parts, (), ()) =
            Policies::into_parts(bundle.with_scheduler(()).with_step_hook(()));
        let ((), ()) = {
            let (hook, logger) = PolicyParts::step_hook_mut(&mut parts);
            (*hook, *logger)
        };
    }
}
