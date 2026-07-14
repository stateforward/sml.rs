//! Allocation-free utilities for runtime dispatch and groups of state machines.

use crate::{Machine, Terminated};

/// Result of hierarchical child-first event routing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HierarchicalDispatch {
    /// The active child handled the event and remains active.
    Child,
    /// The child handled the event and reached its terminal state.
    ChildTerminated,
    /// The parent handled the event.
    Parent,
    /// Neither active machine handled the event.
    Unhandled,
}

/// Parent and child state machines with child-first event routing and retained
/// shallow history.
///
/// Deactivating a child does not reset it. Reactivating it therefore resumes
/// its previous state; `reset_child` explicitly replaces that history.
pub struct Hierarchical<P, C> {
    parent: P,
    child: C,
    child_active: bool,
}

impl<P, C> Hierarchical<P, C> {
    /// Creates a hierarchy with an inactive child.
    pub const fn new(parent: P, child: C) -> Self {
        Self {
            parent,
            child,
            child_active: false,
        }
    }

    /// Creates a hierarchy whose child starts active.
    pub const fn new_active(parent: P, child: C) -> Self {
        Self {
            parent,
            child,
            child_active: true,
        }
    }

    /// Activates the child, preserving its previous state.
    pub fn activate_child(&mut self) {
        self.child_active = true;
    }

    /// Deactivates the child while preserving shallow history.
    pub fn deactivate_child(&mut self) {
        self.child_active = false;
    }

    /// Replaces the child and activates its new initial state.
    pub fn reset_child(&mut self, child: C) {
        self.child = child;
        self.child_active = true;
    }

    /// Returns true when the child is active.
    pub const fn child_is_active(&self) -> bool {
        self.child_active
    }

    /// Returns the parent machine.
    pub fn parent(&self) -> &P {
        &self.parent
    }

    /// Returns the parent machine mutably.
    pub fn parent_mut(&mut self) -> &mut P {
        &mut self.parent
    }

    /// Returns the child machine.
    pub fn child(&self) -> &C {
        &self.child
    }

    /// Returns the child machine mutably.
    pub fn child_mut(&mut self) -> &mut C {
        &mut self.child
    }

    /// Routes an event to the active child first and bubbles unhandled events
    /// to the parent.
    pub fn process_event<E, FC, FP>(
        &mut self,
        event: E,
        mut child_dispatch: FC,
        mut parent_dispatch: FP,
    ) -> HierarchicalDispatch
    where
        E: Clone,
        C: Terminated,
        FC: FnMut(&mut C, E) -> bool,
        FP: FnMut(&mut P, E) -> bool,
    {
        if self.child_active && child_dispatch(&mut self.child, event.clone()) {
            return if self.child.is_terminated() {
                HierarchicalDispatch::ChildTerminated
            } else {
                HierarchicalDispatch::Child
            };
        }

        if parent_dispatch(&mut self.parent, event) {
            HierarchicalDispatch::Parent
        } else {
            HierarchicalDispatch::Unhandled
        }
    }

    /// Routes an event like `process_event` and propagates child termination
    /// to a parent completion callback.
    ///
    /// When the callback handles completion, the child is deactivated while
    /// retaining its state as shallow history.
    pub fn process_event_with_completion<E, FC, FP, FT>(
        &mut self,
        event: E,
        mut child_dispatch: FC,
        mut parent_dispatch: FP,
        mut child_completion: FT,
    ) -> HierarchicalDispatch
    where
        E: Clone,
        C: Terminated,
        FC: FnMut(&mut C, E) -> bool,
        FP: FnMut(&mut P, E) -> bool,
        FT: FnMut(&mut P) -> bool,
    {
        if self.child_active && child_dispatch(&mut self.child, event.clone()) {
            if self.child.is_terminated() {
                if child_completion(&mut self.parent) {
                    self.child_active = false;
                    return HierarchicalDispatch::Parent;
                }
                return HierarchicalDispatch::ChildTerminated;
            }
            return HierarchicalDispatch::Child;
        }

        if parent_dispatch(&mut self.parent, event) {
            HierarchicalDispatch::Parent
        } else {
            HierarchicalDispatch::Unhandled
        }
    }
}

/// Error returned when a bounded event queue has no remaining capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueFull;

/// Allocation-free FIFO used for deferred and explicitly processed events.
pub struct EventQueue<E, const N: usize> {
    events: [Option<E>; N],
    head: usize,
    len: usize,
}

impl<E, const N: usize> EventQueue<E, N> {
    /// Creates an empty queue.
    pub const fn new() -> Self {
        Self {
            events: [const { None }; N],
            head: 0,
            len: 0,
        }
    }

    /// Returns the number of queued events.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns true when no events are queued.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Defers an event until events already in the queue have been processed.
    pub fn defer(&mut self, event: E) -> Result<(), QueueFull> {
        if self.len == N || N == 0 {
            return Err(QueueFull);
        }
        let tail = (self.head + self.len) % N;
        self.events[tail] = Some(event);
        self.len += 1;
        Ok(())
    }

    /// Schedules an event ahead of currently deferred events.
    pub fn process(&mut self, event: E) -> Result<(), QueueFull> {
        if self.len == N || N == 0 {
            return Err(QueueFull);
        }
        self.head = (self.head + N - 1) % N;
        self.events[self.head] = Some(event);
        self.len += 1;
        Ok(())
    }

    /// Removes the next event.
    pub fn pop(&mut self) -> Option<E> {
        if self.len == 0 {
            return None;
        }
        let event = self.events[self.head].take();
        self.head = (self.head + 1) % N;
        self.len -= 1;
        event
    }

    /// Drops all queued events.
    pub fn clear(&mut self) {
        while self.pop().is_some() {}
    }
}

impl<E, const N: usize> Default for EventQueue<E, N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Result reported by a queued dispatch callback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DispatchStatus {
    /// The event matched a transition or handler.
    pub handled: bool,
    /// Processing changed the active state.
    pub transitioned: bool,
}

/// Aggregate result of an external dispatch and the queue work it triggered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DispatchSummary {
    /// Total number of events offered to the machine.
    pub dispatched: usize,
    /// Number reported as handled.
    pub handled: usize,
    /// Number that changed active state.
    pub transitioned: usize,
}

impl DispatchSummary {
    fn record(&mut self, status: DispatchStatus) {
        self.dispatched += 1;
        self.handled += usize::from(status.handled);
        self.transitioned += usize::from(status.transitioned);
    }
}

/// Separate bounded queues for deferred events and events requested by actions.
pub struct EventQueues<E, const DEFERRED: usize, const PROCESSED: usize> {
    deferred: EventQueue<E, DEFERRED>,
    processed: EventQueue<E, PROCESSED>,
}

impl<E, const DEFERRED: usize, const PROCESSED: usize> EventQueues<E, DEFERRED, PROCESSED> {
    /// Creates empty queues.
    pub const fn new() -> Self {
        Self {
            deferred: EventQueue::new(),
            processed: EventQueue::new(),
        }
    }

    /// Defers an event until a dispatch changes state.
    pub fn defer(&mut self, event: E) -> Result<(), QueueFull> {
        self.deferred.defer(event)
    }

    /// Schedules an event for immediate processing after the current action.
    pub fn process(&mut self, event: E) -> Result<(), QueueFull> {
        self.processed.defer(event)
    }

    /// Returns the number of deferred events.
    pub const fn deferred_len(&self) -> usize {
        self.deferred.len()
    }

    /// Returns the number of immediately processed events.
    pub const fn processed_len(&self) -> usize {
        self.processed.len()
    }

    /// Dispatches an external event, drains action-requested events, and gives
    /// each previously deferred event one retry after a state change.
    pub fn dispatch<M, F>(&mut self, machine: &mut M, event: E, mut dispatch: F) -> DispatchSummary
    where
        F: FnMut(&mut M, &mut Self, E) -> DispatchStatus,
    {
        let mut summary = DispatchSummary::default();
        let initial = dispatch(machine, self, event);
        summary.record(initial);
        self.drain_processed(machine, &mut dispatch, &mut summary);

        if summary.transitioned > 0 {
            let retries = self.deferred.len();
            for _ in 0..retries {
                if let Some(event) = self.deferred.pop() {
                    let status = dispatch(machine, self, event);
                    summary.record(status);
                    self.drain_processed(machine, &mut dispatch, &mut summary);
                }
            }
        }

        summary
    }

    fn drain_processed<M, F>(
        &mut self,
        machine: &mut M,
        dispatch: &mut F,
        summary: &mut DispatchSummary,
    ) where
        F: FnMut(&mut M, &mut Self, E) -> DispatchStatus,
    {
        while let Some(event) = self.processed.pop() {
            summary.record(dispatch(machine, self, event));
        }
    }
}

impl<E, const DEFERRED: usize, const PROCESSED: usize> Default
    for EventQueues<E, DEFERRED, PROCESSED>
{
    fn default() -> Self {
        Self::new()
    }
}

/// Routes a runtime event ID to one of a contiguous set of typed handlers.
///
/// Each handler is responsible for translating the raw event into the
/// generated event enum expected by its state machine.
pub struct DispatchTable<'a, M, Raw, R> {
    machine: &'a mut M,
    first_id: usize,
    handlers: &'a [fn(&mut M, &Raw) -> R],
}

impl<'a, M, Raw, R> DispatchTable<'a, M, Raw, R> {
    /// Creates a table whose first handler corresponds to `first_id`.
    pub const fn new(
        machine: &'a mut M,
        first_id: usize,
        handlers: &'a [fn(&mut M, &Raw) -> R],
    ) -> Self {
        Self {
            machine,
            first_id,
            handlers,
        }
    }

    /// Dispatches `raw` through the handler associated with `id`.
    ///
    /// Returns `None` when the ID is outside the table's contiguous range.
    #[inline]
    pub fn dispatch(&mut self, raw: &Raw, id: usize) -> Option<R> {
        let index = id.checked_sub(self.first_id)?;
        let handler = *self.handlers.get(index)?;
        Some(handler(self.machine, raw))
    }

    /// Returns a shared reference to the underlying machine.
    pub fn machine(&self) -> &M {
        self.machine
    }

    /// Returns a mutable reference to the underlying machine.
    pub fn machine_mut(&mut self) -> &mut M {
        self.machine
    }
}

/// Associates an event with the index of the machine that should receive it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedEvent<E> {
    /// Index in the pool's storage.
    pub index: usize,
    /// Event to dispatch.
    pub event: E,
}

/// Creates an indexed event for heterogeneous batch dispatch.
pub const fn with_id<E>(index: usize, event: E) -> IndexedEvent<E> {
    IndexedEvent { index, event }
}

/// A storage-generic pool of state machines.
///
/// Arrays, mutable slices, and allocation-backed containers can all be used as
/// storage. Dispatch is supplied as a closure because each generated sml.rs
/// machine has its own concrete event and error types.
pub struct SmPool<S> {
    storage: S,
}

/// A set of simultaneously active state-machine regions.
///
/// Each event is offered to every region, matching orthogonal-region broadcast
/// semantics. Storage remains caller-selected and allocation-free.
pub struct OrthogonalRegions<S> {
    regions: S,
}

impl<S> OrthogonalRegions<S> {
    /// Wraps the active region machines.
    pub const fn new(regions: S) -> Self {
        Self { regions }
    }

    /// Returns the region storage.
    pub fn regions(&self) -> &S {
        &self.regions
    }

    /// Returns the region storage mutably.
    pub fn regions_mut(&mut self) -> &mut S {
        &mut self.regions
    }

    /// Broadcasts an event to every region and returns the number that handled
    /// it successfully.
    pub fn process_event<M, E>(&mut self, event: E) -> usize
    where
        S: AsMut<[M]>,
        M: Machine<E>,
        E: Clone,
    {
        self.regions.as_mut().iter_mut().fold(0, |handled, region| {
            handled + usize::from(Machine::process_event(region, event.clone()))
        })
    }
}

impl<S> SmPool<S> {
    /// Wraps caller-provided storage.
    pub const fn new(storage: S) -> Self {
        Self { storage }
    }

    /// Returns the underlying storage.
    pub fn storage(&self) -> &S {
        &self.storage
    }

    /// Returns the underlying storage mutably.
    pub fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    /// Replaces every machine using a caller-provided initializer.
    pub fn reset<M, F>(&mut self, mut initialize: F)
    where
        S: AsMut<[M]>,
        F: FnMut(usize) -> M,
    {
        for (index, machine) in self.storage.as_mut().iter_mut().enumerate() {
            *machine = initialize(index);
        }
    }

    /// Dispatches one event to one machine.
    #[inline]
    pub fn process_indexed<M, E, R, F>(&mut self, index: usize, event: E, dispatch: F) -> Option<R>
    where
        S: AsMut<[M]>,
        F: FnOnce(&mut M, E) -> R,
    {
        self.storage
            .as_mut()
            .get_mut(index)
            .map(|machine| dispatch(machine, event))
    }

    /// Dispatches the same clonable event to a batch of machine indices.
    ///
    /// Returns the number of valid indices that were processed.
    pub fn process_indexed_batch<M, E, I, F>(
        &mut self,
        indices: I,
        event: E,
        mut dispatch: F,
    ) -> usize
    where
        S: AsMut<[M]>,
        E: Clone,
        I: IntoIterator<Item = usize>,
        F: FnMut(&mut M, E),
    {
        let machines = self.storage.as_mut();
        let mut handled = 0;
        for index in indices {
            if let Some(machine) = machines.get_mut(index) {
                dispatch(machine, event.clone());
                handled += 1;
            }
        }
        handled
    }

    /// Dispatches a batch of indexed events.
    ///
    /// Returns the number of valid indices that were processed.
    pub fn process_event_batch<M, E, I, F>(&mut self, events: I, mut dispatch: F) -> usize
    where
        S: AsMut<[M]>,
        I: IntoIterator<Item = IndexedEvent<E>>,
        F: FnMut(&mut M, E),
    {
        let machines = self.storage.as_mut();
        let mut handled = 0;
        for IndexedEvent { index, event } in events {
            if let Some(machine) = machines.get_mut(index) {
                dispatch(machine, event);
                handled += 1;
            }
        }
        handled
    }
}
