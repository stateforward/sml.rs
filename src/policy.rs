//! Policy extension points for generated state machines.

use crate::utility::{EventQueue, QueueFull};

/// Supplies a stable name for a generated event payload or variant.
pub trait EventName {
    /// Returns the event name.
    fn name(&self) -> &'static str;
}

/// Supplies a stable name for a generated state value.
pub trait StateName {
    /// Returns the state name.
    fn name(&self) -> &'static str;
}

/// Receives process, state-change, action, and guard callbacks.
///
/// Hooks use static names rather than serialized state values. This keeps the
/// policy layer allocation-free and usable for generated event enums carrying
/// borrowed data. An owner can correlate callbacks with its own event envelope
/// without exposing policy access through an event or message.
pub trait Logger {
    /// Called when an event enters processing.
    fn log_process_event(&mut self, _event: &'static str) {}
    /// Called after a state change.
    fn log_state_change(&mut self, _from: &'static str, _to: &'static str) {}
    /// Called after an action succeeds.
    fn log_action(&mut self, _action: &'static str, _event: &'static str) {}
    /// Called after a guard returns.
    fn log_guard(&mut self, _guard: &'static str, _event: &'static str, _result: bool) {}
}

impl Logger for () {}

/// Receives the same semantic callbacks as [`Logger`].
///
/// This is intentionally generic: an owner can implement an OpenTelemetry,
/// metrics, tracing, or application observer adapter without adding a
/// telemetry dependency to this crate.
pub trait Observer {
    /// Called when an event enters processing.
    fn observe_process_event(&mut self, _event: &'static str) {}
    /// Called after a state change.
    fn observe_state_change(&mut self, _from: &'static str, _to: &'static str) {}
    /// Called after an action succeeds.
    fn observe_action(&mut self, _action: &'static str, _event: &'static str) {}
    /// Called after a guard returns.
    fn observe_guard(&mut self, _guard: &'static str, _event: &'static str, _result: bool) {}
}

impl Observer for () {}

/// Selects the generated event-routing path.
///
/// The generated semantic engine remains responsible for guard, action,
/// queue, hierarchy, and completion behavior. Returning `false` rejects the
/// event before that engine is entered; the built-in strategies always return
/// `true`. A custom policy may additionally choose individual same-event
/// candidates with [`Dispatch::dispatch_candidate`].
pub trait Dispatch {
    /// Decides whether the generated engine should receive `event`.
    fn dispatch(&self, event: &'static str) -> bool;

    /// Selects a generated candidate for `event` from `state`.
    ///
    /// Candidates are visited in transition-table order. Returning `false`
    /// skips that candidate and allows the generated engine to try the next
    /// one. The default accepts every candidate, preserving the zero-cost
    /// behavior of the built-in strategies and simple custom gates.
    #[inline(always)]
    fn dispatch_candidate(
        &self,
        state: &'static str,
        event: &'static str,
        _candidate: usize,
    ) -> bool {
        let _ = (state, event);
        true
    }
}

/// Default generated dispatch strategy.
#[derive(Default)]
pub struct JumpTable;

/// Branch-chain dispatch strategy.
#[derive(Default)]
pub struct BranchStm;

/// Switch-style dispatch strategy. Rust's match lowering may make this
/// identical to [`JumpTable`] after optimization.
#[derive(Default)]
pub struct SwitchStm;

impl Dispatch for JumpTable {
    #[inline(always)]
    fn dispatch(&self, _event: &'static str) -> bool {
        true
    }
}

impl Dispatch for BranchStm {
    #[inline(always)]
    fn dispatch(&self, _event: &'static str) -> bool {
        true
    }
}

impl Dispatch for SwitchStm {
    #[inline(always)]
    fn dispatch(&self, _event: &'static str) -> bool {
        true
    }
}

/// A lock policy used around event processing.
///
/// The generated machine holds the returned guard while it mutably processes
/// the machine. Implementations must therefore use `self` only as the lock
/// state and must not return a guard that aliases, retains, or exposes the
/// policy bundle or any of its other policy slots. This is the invariant that
/// permits generated machines to lock one policy slot while mutating the
/// machine and the remaining policy slots.
/// # Safety
///
/// An implementation must ensure that the guard borrows only the lock state
/// represented by `Self`. It must not retain or expose aliases to the policy
/// bundle, the machine, or any other policy slot. Generated machines use this
/// guarantee when they hold the guard while mutably processing the machine.
pub unsafe trait ThreadSafety {
    /// The guard held during processing.
    type Guard<'a>
    where
        Self: 'a;

    /// Acquires the processing lock.
    fn lock(&self) -> Self::Guard<'_>;
}

unsafe impl ThreadSafety for () {
    type Guard<'a>
        = ()
    where
        Self: 'a;

    #[inline(always)]
    fn lock(&self) {}
}

/// A no-std mutex interface suitable for [`ThreadSafe`].
///
/// A [`RawMutex`] guard must borrow only the mutex represented by `Self` and
/// must not retain aliases to its containing policy bundle or to the machine.
pub trait RawMutex {
    /// The borrowed lock guard.
    type Guard<'a>
    where
        Self: 'a;

    /// Acquires the lock.
    fn lock(&self) -> Self::Guard<'_>;
}

/// Wraps a raw mutex as a [`ThreadSafety`] policy.
pub struct ThreadSafe<L: RawMutex>(pub L);

impl<L: RawMutex> ThreadSafe<L> {
    /// Creates a thread-safety policy from a raw mutex.
    pub const fn new(lock: L) -> Self {
        Self(lock)
    }
}

unsafe impl<L: RawMutex> ThreadSafety for ThreadSafe<L> {
    type Guard<'a>
        = L::Guard<'a>
    where
        Self: 'a;

    #[inline]
    fn lock(&self) -> Self::Guard<'_> {
        self.0.lock()
    }
}

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
impl<T> RawMutex for std::sync::Mutex<T> {
    type Guard<'a>
        = std::sync::MutexGuard<'a, T>
    where
        Self: 'a;

    fn lock(&self) -> Self::Guard<'_> {
        std::sync::Mutex::lock(self).expect("sml policy mutex poisoned")
    }
}

#[cfg(feature = "spin")]
impl<T> RawMutex for spin::Mutex<T> {
    type Guard<'a>
        = spin::MutexGuard<'a, T>
    where
        Self: 'a;

    fn lock(&self) -> Self::Guard<'_> {
        spin::Mutex::lock(self)
    }
}

/// Controls whether state override helpers are available.
pub trait Testing {
    /// Whether this policy opts into state-control helpers.
    const ENABLED: bool;
}

impl Testing for () {
    const ENABLED: bool = false;
}

/// Enables focused state setup and restoration helpers.
#[derive(Default)]
pub struct TestingPolicy;

impl Testing for TestingPolicy {
    const ENABLED: bool = true;
}

/// Grants access to state setup helpers on a generated machine.
pub trait TestingAccess {}

impl TestingAccess for TestingPolicy {}

/// A queue implementation used by generated defer/process storage.
pub trait Queue<E>: Sized {
    /// Creates an empty queue.
    fn new() -> Self;
    /// Appends an event.
    fn defer(&mut self, event: E) -> Result<(), QueueFull>;
    /// Inserts an event ahead of deferred events.
    fn process(&mut self, event: E) -> Result<(), QueueFull>;
    /// Removes the next event.
    fn pop(&mut self) -> Option<E>;
}

impl<E, const N: usize> Queue<E> for EventQueue<E, N> {
    fn new() -> Self {
        EventQueue::new()
    }

    fn defer(&mut self, event: E) -> Result<(), QueueFull> {
        EventQueue::defer(self, event)
    }

    fn process(&mut self, event: E) -> Result<(), QueueFull> {
        EventQueue::process(self, event)
    }

    fn pop(&mut self) -> Option<E> {
        EventQueue::pop(self)
    }
}

/// Queue policy for deferred events.
pub trait DeferQueue<E>: Queue<E> {}

impl<E, T: Queue<E>> DeferQueue<E> for T {}

/// Queue policy for action-requested events.
pub trait ProcessQueue<E>: Queue<E> {}

impl<E, T: Queue<E>> ProcessQueue<E> for T {}

/// A factory for generated queue instances.
pub trait QueuePolicy {
    /// The queue selected for event type `E`.
    type Queue<E>: Queue<E>;

    /// Creates an empty queue.
    fn new_queue<E>(&self) -> Self::Queue<E>;
}

/// The allocation-free 16-event queue used by default.
#[derive(Default)]
pub struct DefaultQueuePolicy;

impl QueuePolicy for DefaultQueuePolicy {
    type Queue<E> = EventQueue<E, 16>;

    fn new_queue<E>(&self) -> Self::Queue<E> {
        EventQueue::new()
    }
}

/// A composable policy bundle.
pub struct PolicyBundle<
    L = (),
    O = (),
    D = JumpTable,
    S = (),
    T = (),
    DQ = DefaultQueuePolicy,
    PQ = DefaultQueuePolicy,
> {
    /// Logger instance.
    pub logger: L,
    /// Observer instance.
    pub observer: O,
    /// Thread-safety instance.
    pub thread_safe: S,
    /// Testing marker instance.
    pub testing: T,
    /// Defer queue factory.
    pub defer_queue: DQ,
    /// Process queue factory.
    pub process_queue: PQ,
    /// Dispatch strategy instance.
    pub dispatch: D,
}

impl<L, O, D, S, T, DQ, PQ> PolicyBundle<L, O, D, S, T, DQ, PQ> {
    /// Creates a policy bundle.
    pub fn new(
        logger: L,
        observer: O,
        thread_safe: S,
        testing: T,
        defer_queue: DQ,
        process_queue: PQ,
    ) -> Self
    where
        D: Default,
    {
        Self {
            logger,
            observer,
            thread_safe,
            testing,
            defer_queue,
            process_queue,
            dispatch: D::default(),
        }
    }

    /// Creates a policy bundle with an explicitly configured dispatch policy.
    pub fn with_dispatch(
        dispatch: D,
        logger: L,
        observer: O,
        thread_safe: S,
        testing: T,
        defer_queue: DQ,
        process_queue: PQ,
    ) -> Self {
        Self {
            logger,
            observer,
            thread_safe,
            testing,
            defer_queue,
            process_queue,
            dispatch,
        }
    }
}

impl<L, O, D, S, T, DQ, PQ> Default for PolicyBundle<L, O, D, S, T, DQ, PQ>
where
    L: Default,
    O: Default,
    D: Default,
    S: Default,
    T: Default,
    DQ: Default,
    PQ: Default,
{
    fn default() -> Self {
        Self::new(
            L::default(),
            O::default(),
            S::default(),
            T::default(),
            DQ::default(),
            PQ::default(),
        )
    }
}

/// Describes the policy slots a generated machine uses.
pub trait Policies {
    /// Logger slot.
    type Logger: Logger;
    /// Observer slot.
    type Observer: Observer;
    /// Dispatch slot.
    type Dispatch: Dispatch;
    /// Thread-safety slot.
    type ThreadSafe: ThreadSafety;
    /// Testing slot.
    type Testing: Testing;
    /// Deferred queue slot.
    type DeferQueue<E>: Queue<E>;
    /// Process queue slot.
    type ProcessQueue<E>: Queue<E>;

    /// Borrows the logger.
    fn logger(&self) -> &Self::Logger;
    /// Mutably borrows the logger.
    fn logger_mut(&mut self) -> &mut Self::Logger;
    /// Borrows the observer.
    fn observer(&self) -> &Self::Observer;
    /// Mutably borrows the observer.
    fn observer_mut(&mut self) -> &mut Self::Observer;
    /// Borrows the dispatch strategy.
    fn dispatch(&self) -> &Self::Dispatch;
    /// Borrows the thread-safety policy.
    fn thread_safe(&self) -> &Self::ThreadSafe;
    /// Borrows the testing policy.
    fn testing(&self) -> &Self::Testing;
    /// Creates the generated deferred queue.
    fn new_defer_queue<E>(&self) -> Self::DeferQueue<E>;
    /// Creates the generated process queue.
    fn new_process_queue<E>(&self) -> Self::ProcessQueue<E>;
}

impl<L, O, D, S, T, DQ, PQ> Policies for PolicyBundle<L, O, D, S, T, DQ, PQ>
where
    L: Logger,
    O: Observer,
    D: Dispatch,
    S: ThreadSafety,
    T: Testing,
    DQ: QueuePolicy,
    PQ: QueuePolicy,
{
    type Logger = L;
    type Observer = O;
    type Dispatch = D;
    type ThreadSafe = S;
    type Testing = T;
    type DeferQueue<E> = DQ::Queue<E>;
    type ProcessQueue<E> = PQ::Queue<E>;

    fn logger(&self) -> &Self::Logger {
        &self.logger
    }

    fn logger_mut(&mut self) -> &mut Self::Logger {
        &mut self.logger
    }

    fn observer(&self) -> &Self::Observer {
        &self.observer
    }

    fn observer_mut(&mut self) -> &mut Self::Observer {
        &mut self.observer
    }

    fn dispatch(&self) -> &Self::Dispatch {
        &self.dispatch
    }

    fn thread_safe(&self) -> &Self::ThreadSafe {
        &self.thread_safe
    }

    fn testing(&self) -> &Self::Testing {
        &self.testing
    }

    fn new_defer_queue<E>(&self) -> Self::DeferQueue<E> {
        self.defer_queue.new_queue()
    }

    fn new_process_queue<E>(&self) -> Self::ProcessQueue<E> {
        self.process_queue.new_queue()
    }
}

/// The default policy bundle. Its fields are all zero-sized.
pub struct NoPolicy {
    logger: (),
    observer: (),
    thread_safe: (),
    testing: (),
    dispatch: JumpTable,
}

impl Policies for NoPolicy {
    type Logger = ();
    type Observer = ();
    type Dispatch = JumpTable;
    type ThreadSafe = ();
    type Testing = ();
    type DeferQueue<E> = EventQueue<E, 16>;
    type ProcessQueue<E> = EventQueue<E, 16>;

    fn logger(&self) -> &Self::Logger {
        &self.logger
    }

    fn logger_mut(&mut self) -> &mut Self::Logger {
        &mut self.logger
    }

    fn observer(&self) -> &Self::Observer {
        &self.observer
    }

    fn observer_mut(&mut self) -> &mut Self::Observer {
        &mut self.observer
    }

    fn dispatch(&self) -> &Self::Dispatch {
        &self.dispatch
    }

    fn thread_safe(&self) -> &Self::ThreadSafe {
        &self.thread_safe
    }

    fn testing(&self) -> &Self::Testing {
        &self.testing
    }

    fn new_defer_queue<E>(&self) -> Self::DeferQueue<E> {
        EventQueue::new()
    }

    fn new_process_queue<E>(&self) -> Self::ProcessQueue<E> {
        EventQueue::new()
    }
}

impl Default for NoPolicy {
    fn default() -> Self {
        Self {
            logger: (),
            observer: (),
            thread_safe: (),
            testing: (),
            dispatch: JumpTable,
        }
    }
}

/// Expands a policy declaration while carrying the seven slot types through
/// the parser. This is public only because exported `macro_rules!` macros
/// expand at the call site.
#[doc(hidden)]
#[macro_export]
macro_rules! __sml_policy_parse {
    (@finish [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty]
    ) => {
        #[doc = "Generated policy bundle."]
        pub type $name = $crate::PolicyBundle<
            $logger,
            $observer,
            $dispatch,
            $thread_safe,
            $testing,
            $defer_queue,
            $process_queue,
        >;
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
    ) => {
        $crate::__sml_policy_parse!(@finish [$name]
            [$logger] [$observer] [$dispatch] [$thread_safe]
            [$testing] [$defer_queue] [$process_queue]);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        logger: $value:ty, $($rest:tt)*) => {
        $crate::__sml_policy_parse!(@parse [$name]
            [$value] [$observer] [$dispatch] [$thread_safe]
            [$testing] [$defer_queue] [$process_queue]; $($rest)*);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        observer: $value:ty, $($rest:tt)*) => {
        $crate::__sml_policy_parse!(@parse [$name]
            [$logger] [$value] [$dispatch] [$thread_safe]
            [$testing] [$defer_queue] [$process_queue]; $($rest)*);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        dispatch: $value:ty, $($rest:tt)*) => {
        $crate::__sml_policy_parse!(@parse [$name]
            [$logger] [$observer] [$value] [$thread_safe]
            [$testing] [$defer_queue] [$process_queue]; $($rest)*);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        thread_safe: $value:ty, $($rest:tt)*) => {
        $crate::__sml_policy_parse!(@parse [$name]
            [$logger] [$observer] [$dispatch] [$value]
            [$testing] [$defer_queue] [$process_queue]; $($rest)*);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        testing: $value:ty, $($rest:tt)*) => {
        $crate::__sml_policy_parse!(@parse [$name]
            [$logger] [$observer] [$dispatch] [$thread_safe]
            [$value] [$defer_queue] [$process_queue]; $($rest)*);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        defer_queue: $value:ty, $($rest:tt)*) => {
        $crate::__sml_policy_parse!(@parse [$name]
            [$logger] [$observer] [$dispatch] [$thread_safe]
            [$testing] [$value] [$process_queue]; $($rest)*);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        process_queue: $value:ty, $($rest:tt)*) => {
        $crate::__sml_policy_parse!(@parse [$name]
            [$logger] [$observer] [$dispatch] [$thread_safe]
            [$testing] [$defer_queue] [$value]; $($rest)*);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        logger: $value:ty
    ) => {
        $crate::__sml_policy_parse!(@finish [$name]
            [$value] [$observer] [$dispatch] [$thread_safe]
            [$testing] [$defer_queue] [$process_queue]);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        observer: $value:ty
    ) => {
        $crate::__sml_policy_parse!(@finish [$name]
            [$logger] [$value] [$dispatch] [$thread_safe]
            [$testing] [$defer_queue] [$process_queue]);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        dispatch: $value:ty
    ) => {
        $crate::__sml_policy_parse!(@finish [$name]
            [$logger] [$observer] [$value] [$thread_safe]
            [$testing] [$defer_queue] [$process_queue]);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        thread_safe: $value:ty
    ) => {
        $crate::__sml_policy_parse!(@finish [$name]
            [$logger] [$observer] [$dispatch] [$value]
            [$testing] [$defer_queue] [$process_queue]);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        testing: $value:ty
    ) => {
        $crate::__sml_policy_parse!(@finish [$name]
            [$logger] [$observer] [$dispatch] [$thread_safe]
            [$value] [$defer_queue] [$process_queue]);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        defer_queue: $value:ty
    ) => {
        $crate::__sml_policy_parse!(@finish [$name]
            [$logger] [$observer] [$dispatch] [$thread_safe]
            [$testing] [$value] [$process_queue]);
    };
    (@parse [$name:ident]
        [$logger:ty] [$observer:ty] [$dispatch:ty] [$thread_safe:ty]
        [$testing:ty] [$defer_queue:ty] [$process_queue:ty];
        process_queue: $value:ty
    ) => {
        $crate::__sml_policy_parse!(@finish [$name]
            [$logger] [$observer] [$dispatch] [$thread_safe]
            [$testing] [$defer_queue] [$value]);
    };
}

/// Names the policy slots that differ from the defaults.
///
/// Slots may be listed in any order. Omitted slots inherit the allocation-free
/// defaults, so a policy declaration only names the behavior it changes.
#[macro_export]
macro_rules! sml_policies {
    ($name:ident { $($slots:tt)* }) => {
        $crate::__sml_policy_parse!(@parse [$name]
            [()] [()] [$crate::JumpTable] [()] [()]
            [$crate::DefaultQueuePolicy] [$crate::DefaultQueuePolicy];
            $($slots)*);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_strategies_and_testing_markers_have_their_defaults() {
        assert!(JumpTable.dispatch("event"));
        assert!(BranchStm.dispatch("event"));
        assert!(SwitchStm.dispatch("event"));
        const {
            assert!(!<() as Testing>::ENABLED);
            assert!(TestingPolicy::ENABLED);
        }
    }

    #[test]
    fn queue_and_policy_accessors_are_all_allocation_free() {
        let mut queue: EventQueue<u8, 2> = <EventQueue<u8, 2> as Queue<u8>>::new();
        <EventQueue<u8, 2> as Queue<u8>>::defer(&mut queue, 1).unwrap();
        <EventQueue<u8, 2> as Queue<u8>>::process(&mut queue, 2).unwrap();
        assert_eq!(<EventQueue<u8, 2> as Queue<u8>>::pop(&mut queue), Some(2));
        assert_eq!(<EventQueue<u8, 2> as Queue<u8>>::pop(&mut queue), Some(1));
        assert_eq!(<EventQueue<u8, 2> as Queue<u8>>::pop(&mut queue), None);

        let queue_policy = DefaultQueuePolicy;
        let mut default_queue: EventQueue<u8, 16> =
            <DefaultQueuePolicy as QueuePolicy>::new_queue(&queue_policy);
        <EventQueue<u8, 16> as Queue<u8>>::defer(&mut default_queue, 3).unwrap();
        assert_eq!(
            <EventQueue<u8, 16> as Queue<u8>>::pop(&mut default_queue),
            Some(3)
        );

        let mut bundle = PolicyBundle::<(), (), JumpTable>::new(
            (),
            (),
            (),
            (),
            DefaultQueuePolicy,
            DefaultQueuePolicy,
        );
        let _ = Policies::logger(&bundle);
        let _ = Policies::logger_mut(&mut bundle);
        let _ = Policies::observer(&bundle);
        let _ = Policies::observer_mut(&mut bundle);
        let _ = Policies::dispatch(&bundle);
        let _ = Policies::thread_safe(&bundle);
        let _ = Policies::testing(&bundle);
        let _: EventQueue<u8, 16> = Policies::new_defer_queue(&bundle);
        let _: EventQueue<u8, 16> = Policies::new_process_queue(&bundle);

        let mut no_policy = NoPolicy::default();
        let _ = Policies::logger(&no_policy);
        let _ = Policies::logger_mut(&mut no_policy);
        let _ = Policies::observer(&no_policy);
        let _ = Policies::observer_mut(&mut no_policy);
        let _ = Policies::dispatch(&no_policy);
        let _ = Policies::thread_safe(&no_policy);
        let _ = Policies::testing(&no_policy);
        let _: EventQueue<u8, 16> = Policies::new_defer_queue(&no_policy);
        let _: EventQueue<u8, 16> = Policies::new_process_queue(&no_policy);
    }

    #[cfg(feature = "std")]
    #[test]
    fn std_mutex_is_a_raw_mutex_adapter() {
        let mutex = std::sync::Mutex::new(());
        let guard = <std::sync::Mutex<()> as RawMutex>::lock(&mutex);
        drop(guard);
    }

    #[cfg(feature = "spin")]
    #[test]
    fn spin_mutex_is_a_raw_mutex_adapter() {
        let mutex = spin::Mutex::new(());
        let guard = <spin::Mutex<()> as RawMutex>::lock(&mutex);
        drop(guard);
    }
}
