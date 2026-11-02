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
        use super::{lock, FifoScheduler, InlineScheduler, Queued, Schedule, StrictOrdering};
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

            assert!(cell.drain(9));
            assert_eq!(poll(first.as_mut()), Poll::Ready(true));
            assert_eq!(poll(waiting.as_mut()), Poll::Ready(true));
            assert_eq!(lock(&cell.shared.machine).seen, [1, 9, 2, 3]);

            let stop = <FifoScheduler<2> as Queued<Recorder>>::submit(&cell, 0);
            assert_eq!(poll(stop), Poll::Ready(true));
            assert!(lock(&cell.shared.queue).terminated);
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

#[cfg(feature = "tokio")]
mod task {
    extern crate std;

    use super::{Queued, Schedule, Scheduler, StrictOrdering, SyncMachineStep};
    use crate::utility::QueueFull;
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Context, Poll};
    use std::boxed::Box;
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
    use std::vec::Vec;
    use tokio::sync::{mpsc, oneshot, Semaphore};

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

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
    /// [`TokioScope::scheduler`] places the machine in a scope, and
    /// [`TokioScheduler::owning`] makes it the scope's owner. A step runs
    /// synchronously on a runtime worker, so a long step occupies that worker
    /// for its duration.
    #[derive(Clone, Debug, Default)]
    pub struct TokioScheduler {
        handle: Option<tokio::runtime::Handle>,
        scope: Option<TokioScope>,
        owns: Option<TokioScope>,
    }

    impl TokioScheduler {
        /// Spawns machines on `handle`'s runtime.
        #[must_use]
        pub const fn new(handle: tokio::runtime::Handle) -> Self {
            Self {
                handle: Some(handle),
                scope: None,
                owns: None,
            }
        }

        /// Makes the machine the owner of `scope`: once the machine reaches
        /// `X`, or its own scope ends it, `scope` ends every machine in it.
        #[must_use]
        pub fn owning(mut self, scope: TokioScope) -> Self {
            self.owns = Some(scope);
            self
        }
    }

    /// A group of machines that one owner bounds and ends together.
    ///
    /// Each machine in a scope keeps its own task and its own FIFO queue, so
    /// one machine's slow step never delays a sibling's queue. The scope caps
    /// how many of its machines run a step at once; a step waits for a slot in
    /// FIFO order, so machines take turns and a busy machine cannot starve
    /// the others. Ending the scope ends every machine in it: each stops
    /// taking events, its queued and later callers resolve to `false`, and a
    /// machine that owns a scope of its own ends that scope too. Scopes nest
    /// through owners: a host machine owns the scope its project machines run
    /// in, and each project machine owns the scope of its own children.
    #[derive(Clone)]
    pub struct TokioScope {
        inner: Arc<ScopeInner>,
    }

    impl core::fmt::Debug for TokioScope {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("TokioScope").finish_non_exhaustive()
        }
    }

    struct ScopeInner {
        running: Semaphore,
        members: Mutex<Members>,
    }

    struct Members {
        ended: bool,
        machines: Vec<Box<dyn Member>>,
    }

    // A machine in a scope, held weakly so a machine whose handles are all
    // dropped still stops.
    trait Member: Send {
        fn alive(&self) -> bool;
        fn end(&self);
    }

    impl<M> Member for mpsc::WeakUnboundedSender<Message<M>> {
        fn alive(&self) -> bool {
            self.strong_count() > 0
        }

        fn end(&self) {
            if let Some(events) = self.upgrade() {
                // A task that already stopped no longer needs to be told.
                events.send(Message::End).ok();
            }
        }
    }

    impl TokioScope {
        /// Creates a scope in which at most `max_running` machines run a step
        /// at once (at least one).
        #[must_use]
        pub fn new(max_running: usize) -> Self {
            Self {
                inner: Arc::new(ScopeInner {
                    running: Semaphore::new(max_running.clamp(1, Semaphore::MAX_PERMITS)),
                    members: Mutex::new(Members {
                        ended: false,
                        machines: Vec::new(),
                    }),
                }),
            }
        }

        /// A scheduler that runs its machine in this scope, spawned on the
        /// runtime current at construction.
        #[must_use]
        pub fn scheduler(&self) -> TokioScheduler {
            TokioScheduler {
                handle: None,
                scope: Some(self.clone()),
                owns: None,
            }
        }

        /// Ends every machine in this scope. Machines placed in it afterwards
        /// never start.
        pub fn end(&self) {
            let machines = {
                let mut members = lock(&self.inner.members);
                if members.ended {
                    return;
                }
                members.ended = true;
                core::mem::take(&mut members.machines)
            };
            // Wakes steps waiting for a slot; they stop instead of running.
            self.inner.running.close();
            for machine in machines {
                machine.end();
            }
        }

        // Registers a machine; false once the scope has ended.
        fn join(&self, machine: Box<dyn Member>) -> bool {
            let mut members = lock(&self.inner.members);
            if members.ended {
                return false;
            }
            members.machines.retain(|member| member.alive());
            members.machines.push(machine);
            true
        }
    }

    // A queued event, type-erased so the cell does not name the event type.
    type Job<M> = Box<dyn FnOnce(&mut M) -> bool + Send>;

    enum Message<M> {
        Run(Job<M>, oneshot::Sender<bool>),
        End,
    }

    /// Shared storage for a [`TokioScheduler`] machine: the queue into its
    /// task.
    pub struct TokioCell<M> {
        events: mpsc::UnboundedSender<Message<M>>,
    }

    impl<M> Clone for TokioCell<M> {
        fn clone(&self) -> Self {
            Self {
                events: self.events.clone(),
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

    // Runs one step under the scope's cap; `None` once the scope has ended.
    async fn step<M>(
        scope: Option<&TokioScope>,
        machine: &mut M,
        job: impl FnOnce(&mut M) -> bool,
    ) -> Option<bool> {
        let slot = match scope {
            Some(scope) => Some(scope.inner.running.acquire().await.ok()?),
            None => None,
        };
        let accepted = job(machine);
        drop(slot);
        Some(accepted)
    }

    async fn run<M: SyncMachineStep>(
        mut machine: M,
        mut events: mpsc::UnboundedReceiver<Message<M>>,
        scope: Option<TokioScope>,
        owns: Option<TokioScope>,
    ) {
        let started = step(scope.as_ref(), &mut machine, |machine| {
            machine.start();
            !machine.is_terminated()
        })
        .await;
        let mut running = started == Some(true);
        if !running {
            events.close();
            end_owned(owns.as_ref());
        }
        // The task ends once every handle is gone and the queue is empty, or
        // once the machine stopped and its queue drained.
        while let Some(message) = events.recv().await {
            match message {
                Message::End => {
                    running = false;
                    events.close();
                }
                Message::Run(job, reply) => {
                    let stepped = if running {
                        step(scope.as_ref(), &mut machine, job).await
                    } else {
                        Some(false)
                    };
                    // `None`: the scope ended while the step waited for a slot.
                    let accepted = stepped.unwrap_or_else(|| {
                        running = false;
                        events.close();
                        false
                    });
                    if running && machine.is_terminated() {
                        running = false;
                        // Already-queued events still drain, each resolving `false`.
                        events.close();
                    }
                    if !running {
                        end_owned(owns.as_ref());
                    }
                    // A caller that dropped its completion no longer wants the result.
                    reply.send(accepted).ok();
                }
            }
            if !running {
                end_owned(owns.as_ref());
            }
        }
        end_owned(owns.as_ref());
    }

    fn end_owned(owns: Option<&TokioScope>) {
        if let Some(owned) = owns {
            owned.end();
        }
    }

    impl<M> Schedule<M> for TokioScheduler
    where
        M: SyncMachineStep + Send + 'static,
        M::Event: Send + 'static,
    {
        fn cell(self, machine: M) -> TokioCell<M> {
            let (sender, receiver) = mpsc::unbounded_channel();
            if let Some(scope) = &self.scope {
                if !scope.join(Box::new(sender.downgrade())) {
                    // The scope already ended: the machine never starts and
                    // every event resolves `false`.
                    end_owned(self.owns.as_ref());
                    drop(receiver);
                    return TokioCell { events: sender };
                }
            }
            let task = run(machine, receiver, self.scope, self.owns);
            // The task is detached: it owns the machine and stops on its own.
            drop(match self.handle {
                Some(handle) => handle.spawn(task),
                None => tokio::spawn(task),
            });
            TokioCell { events: sender }
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
            let job: Job<M> = Box::new(move |machine: &mut M| machine.step(event));
            let queued = cell.events.send(Message::Run(job, reply)).is_ok();
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
pub use task::{TokioCell, TokioCompletion, TokioScheduler, TokioScope};

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
