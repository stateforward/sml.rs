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

    /// Reports whether the machine in `cell` reached its terminal `X` state.
    fn is_terminated(cell: &Self::Cell<M>) -> bool;
}

impl<M: Terminated> Schedule<M> for () {
    #[inline]
    fn cell(self, machine: M) -> M {
        machine
    }

    #[inline]
    fn is_terminated(cell: &M) -> bool {
        cell.is_terminated()
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

impl<C, L, E> StepHook<C, L, E> for () {}

#[cfg(feature = "std")]
mod shared {
    extern crate std;

    use super::{Queued, Schedule, Scheduler, StrictOrdering, SyncMachineStep};
    use crate::utility::{EventQueue, QueueFull};
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

        fn is_terminated(cell: &InlineCell<M>) -> bool {
            lock(&cell.machine).is_terminated()
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

        fn is_terminated(cell: &FifoCell<M, N>) -> bool {
            lock(&cell.shared.queue).terminated
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
            if queue.draining || !queue.events.is_empty() {
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
            drop(queue);
            Offer::Done(self.drain(event))
        }

        // Runs `first`, then every event queued meanwhile, and leaves the
        // machine idle. Only the caller that set `draining` gets here, so the
        // machine lock is never contended.
        fn drain(&self, first: M::Event) -> bool {
            let (accepted, mut terminated) = self.run(move |machine: &mut M| machine.step(first));
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
                let result = if terminated {
                    false
                } else {
                    let (result, now_terminated) = self.run(job);
                    terminated = now_terminated;
                    result
                };
                ticket.resolve(result);
            }
        }

        fn run(&self, job: impl FnOnce(&mut M) -> bool) -> (bool, bool) {
            let mut machine = lock(&self.shared.machine);
            let accepted = job(&mut machine);
            (accepted, machine.is_terminated())
        }
    }

    /// Completion of an event submitted to a [`FifoScheduler`].
    pub struct FifoCompletion<M: SyncMachineStep, const N: usize> {
        state: FifoWait<M, N>,
    }

    enum FifoWait<M: SyncMachineStep, const N: usize> {
        Done(bool),
        Queued(Arc<Ticket>),
        // The queue was full: the event is queued once there is room.
        Room(FifoCell<M, N>, Option<M::Event>),
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
            loop {
                match &mut this.state {
                    FifoWait::Done(accepted) => return Poll::Ready(*accepted),
                    FifoWait::Queued(ticket) => return ticket.poll(cx),
                    FifoWait::Room(cell, event) => {
                        let Some(event) = event.take() else {
                            return Poll::Ready(false);
                        };
                        let cell = cell.clone();
                        this.state = match cell.offer(event, Some(cx.waker())) {
                            Offer::Done(accepted) => FifoWait::Done(accepted),
                            Offer::Queued(ticket) => FifoWait::Queued(ticket),
                            Offer::Full(event) => {
                                this.state = FifoWait::Room(cell, Some(event));
                                return Poll::Pending;
                            }
                        };
                    }
                }
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
                Offer::Full(event) => FifoWait::Room(cell.clone(), Some(event)),
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
}

#[cfg(feature = "std")]
pub use shared::{FifoCell, FifoCompletion, FifoScheduler, InlineCell, InlineScheduler};

#[cfg(feature = "tokio")]
mod task {
    extern crate std;

    use super::{Queued, Schedule, Scheduler, StrictOrdering, SyncMachineStep};
    use crate::utility::QueueFull;
    use core::future::Future;
    use core::pin::Pin;
    use core::sync::atomic::{AtomicBool, Ordering};
    use core::task::{Context, Poll};
    use std::boxed::Box;
    use std::sync::Arc;
    use tokio::sync::{mpsc, oneshot};

    /// Runs the machine on a dedicated Tokio task.
    ///
    /// Constructing the machine spawns one task that owns it, runs its
    /// initial entry, and then runs queued events one at a time. Callers only
    /// queue: `process_event_async` sends the event to the task before it
    /// returns and never holds a lock across `.await`. The queue is
    /// unbounded, so every event is queued at call time.
    ///
    /// The default scheduler spawns on the runtime current at construction,
    /// like `tokio::spawn`; [`TokioScheduler::new`] names a runtime instead.
    /// A step runs synchronously on a runtime worker, so a long step occupies
    /// that worker for its duration.
    #[derive(Clone, Debug, Default)]
    pub struct TokioScheduler {
        handle: Option<tokio::runtime::Handle>,
    }

    impl TokioScheduler {
        /// Spawns machines on `handle`'s runtime.
        #[must_use]
        pub const fn new(handle: tokio::runtime::Handle) -> Self {
            Self {
                handle: Some(handle),
            }
        }
    }

    // A queued event, type-erased so the cell does not name the event type.
    type Request<M> = (
        Box<dyn FnOnce(&mut M) -> bool + Send>,
        oneshot::Sender<bool>,
    );

    /// Shared storage for a [`TokioScheduler`] machine: the queue into its
    /// task and the task's terminal flag.
    pub struct TokioCell<M> {
        events: mpsc::UnboundedSender<Request<M>>,
        terminated: Arc<AtomicBool>,
    }

    impl<M> Clone for TokioCell<M> {
        fn clone(&self) -> Self {
            Self {
                events: self.events.clone(),
                terminated: Arc::clone(&self.terminated),
            }
        }
    }

    impl Scheduler for TokioScheduler {
        type Cell<M> = TokioCell<M>;
    }

    impl StrictOrdering for TokioScheduler {
        const GUARANTEES_FIFO: bool = true;
        const SINGLE_CONSUMER: bool = true;
        const RUN_TO_COMPLETION: bool = true;
    }

    async fn run<M: SyncMachineStep>(
        mut machine: M,
        mut events: mpsc::UnboundedReceiver<Request<M>>,
        terminated: Arc<AtomicBool>,
    ) {
        let _ = machine.start();
        let mut stopped = machine.is_terminated();
        terminated.store(stopped, Ordering::Release);
        if stopped {
            events.close();
        }
        // The task ends once every handle is gone and the queue is empty.
        while let Some((job, reply)) = events.recv().await {
            let accepted = !stopped && job(&mut machine);
            if !stopped && machine.is_terminated() {
                stopped = true;
                terminated.store(true, Ordering::Release);
                // Already-queued events still drain, each resolving `false`.
                events.close();
            }
            // A caller that dropped its completion no longer wants the result.
            reply.send(accepted).ok();
        }
    }

    impl<M> Schedule<M> for TokioScheduler
    where
        M: SyncMachineStep + Send + 'static,
        M::Event: Send + 'static,
    {
        fn cell(self, machine: M) -> TokioCell<M> {
            let (sender, receiver) = mpsc::unbounded_channel();
            let terminated = Arc::new(AtomicBool::new(false));
            let task = run(machine, receiver, Arc::clone(&terminated));
            // The task is detached: it owns the machine and stops on its own.
            drop(match self.handle {
                Some(handle) => handle.spawn(task),
                None => tokio::spawn(task),
            });
            TokioCell {
                events: sender,
                terminated,
            }
        }

        fn is_terminated(cell: &TokioCell<M>) -> bool {
            cell.terminated.load(Ordering::Acquire)
        }
    }

    /// Completion of an event submitted to a [`TokioScheduler`].
    pub struct TokioCompletion {
        reply: Option<oneshot::Receiver<bool>>,
    }

    impl Future for TokioCompletion {
        type Output = bool;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<bool> {
            let this = self.get_mut();
            let Some(reply) = this.reply.as_mut() else {
                return Poll::Ready(false);
            };
            // A closed reply means the task stopped before running the event.
            Pin::new(reply)
                .poll(cx)
                .map(|accepted| accepted.unwrap_or(false))
        }
    }

    impl<M> Queued<M> for TokioScheduler
    where
        M: SyncMachineStep + Send + 'static,
        M::Event: Send + 'static,
    {
        type Completion = TokioCompletion;

        fn submit(cell: &TokioCell<M>, event: M::Event) -> TokioCompletion {
            let (reply, receiver) = oneshot::channel();
            let job: Box<dyn FnOnce(&mut M) -> bool + Send> =
                Box::new(move |machine: &mut M| machine.step(event));
            let queued = cell.events.send((job, reply)).is_ok();
            TokioCompletion {
                reply: queued.then_some(receiver),
            }
        }

        fn try_submit(cell: &TokioCell<M>, event: M::Event) -> Result<TokioCompletion, QueueFull> {
            Ok(Self::submit(cell, event))
        }
    }
}

#[cfg(feature = "tokio")]
pub use task::{TokioCell, TokioCompletion, TokioScheduler};
