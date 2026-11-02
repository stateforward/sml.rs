use core::cell::{Cell, RefCell};
use core::sync::atomic::{AtomicUsize, Ordering};
use std::rc::Rc;

use sml::{
    sml, BranchStm, DefaultQueuePolicy, Dispatch, EventName, JumpTable, Logger, NoPolicy, Observer,
    Policies, QueuePolicy, RawMutex, StateName, TestingPolicy, ThreadSafe,
};

/// Returns whether root region `$region` is in a state matching `$pattern`.
macro_rules! region_is {
    ($machine:expr, $region:expr, $pattern:pat) => {{
        let mut index = 0;
        let mut matched = false;
        $machine.visit_current_states(|state| {
            if index == $region {
                matched = matches!(state, $pattern);
            }
            index += 1;
        });
        matched
    }};
}

sml::sml_policies!(TestingPolicies {
    testing: TestingPolicy,
});

pub struct Go;
pub struct Stop;
pub struct Fail;
pub struct FirstCandidate;
pub struct SecondCandidate;
pub struct AsyncGo;

sml! {
    PolicyFlat {
        *Idle + event<Go> [allow] / act = Ready,
        Ready + event<Stop> = X,
        Ready + event<Fail> / fail = X,
    }
}

sml! {
    PolicyCandidate {
        *CandidateIdle + event<FirstCandidate> [first] = CandidateOne,
        CandidateIdle + event<FirstCandidate> [second] = CandidateTwo,
    }
}

sml! {
    PolicyAsync {
        *AsyncIdle + event<AsyncGo> / async async_act = AsyncReady,
    }
}

#[derive(Default)]
struct Context {
    actions: Cell<usize>,
    process_events: Cell<usize>,
}

impl PolicyFlatStateMachineContext for &Context {
    fn allow(&self, _event: &Go) -> Result<bool, ()> {
        Ok(true)
    }

    fn act(&mut self, _event: &Go) -> Result<(), ()> {
        self.actions.set(self.actions.get() + 1);
        Ok(())
    }

    fn fail(&mut self, _event: &Fail) -> Result<(), ()> {
        Err(())
    }

    fn log_process_event(&self, _: &PolicyFlatStates, _: &PolicyFlatEvents) {
        self.process_events.set(self.process_events.get() + 1);
    }
}

#[derive(Default)]
struct CandidateContext;

impl PolicyCandidateStateMachineContext for CandidateContext {
    fn first(&self, _: &FirstCandidate) -> Result<bool, ()> {
        Ok(true)
    }

    fn second(&self, _: &FirstCandidate) -> Result<bool, ()> {
        Ok(true)
    }
}

#[cfg(feature = "std")]
struct AsyncContext;

#[cfg(feature = "std")]
impl PolicyAsyncStateMachineContext for AsyncContext {
    async fn async_act(&mut self, _: &AsyncGo) -> Result<(), ()> {
        core::future::poll_fn(|_| core::task::Poll::Ready(())).await;
        Ok(())
    }
}

#[cfg(feature = "std")]
#[test]
fn standard_mutex_guard_supports_async_processing_without_unsafe_borrows() {
    type StandardMutexPolicies = Policies<(), (), JumpTable, ThreadSafe<std::sync::Mutex<()>>>;
    let policy = StandardMutexPolicies::with_dispatch(
        JumpTable,
        (),
        (),
        ThreadSafe::new(std::sync::Mutex::new(())),
        (),
        DefaultQueuePolicy,
        DefaultQueuePolicy,
    );
    let mut machine = PolicyAsyncStateMachine::new_with_policy(AsyncContext, policy);

    smol::block_on(machine.process_event(AsyncGo)).unwrap();
    assert!(machine.is(&PolicyAsyncStates::AsyncReady));
}

struct CandidateDispatch;

impl Dispatch for CandidateDispatch {
    fn dispatch(&self, _: &'static str) -> bool {
        true
    }

    fn dispatch_candidate(&self, _: &'static str, event: &'static str, candidate: usize) -> bool {
        assert_eq!(event, "FirstCandidate");
        candidate != 0
    }
}

type CandidatePolicies = Policies<(), (), CandidateDispatch>;

#[test]
fn custom_dispatch_can_select_a_later_guarded_candidate() {
    let mut machine = PolicyCandidateStateMachine::new_with_policy(
        CandidateContext,
        CandidatePolicies::with_dispatch(
            CandidateDispatch,
            (),
            (),
            (),
            (),
            DefaultQueuePolicy,
            DefaultQueuePolicy,
        ),
    );
    machine.process_event(FirstCandidate).unwrap();
    assert!(machine.is(&PolicyCandidateStates::CandidateTwo));
}

#[derive(Default)]
struct LogCounts {
    process: usize,
    guards: usize,
    actions: usize,
    changes: usize,
    last_change: Option<(&'static str, &'static str)>,
}

/// A logger that records into a sink the test also holds, since the machine
/// keeps its policies private.
#[derive(Clone, Default)]
struct Log(Rc<RefCell<LogCounts>>);

impl Log {
    fn counts(&self) -> std::cell::Ref<'_, LogCounts> {
        self.0.borrow()
    }
}

impl Logger for Log {
    fn log_process_event(&mut self, event: &'static str) {
        assert!(!event.is_empty());
        self.0.borrow_mut().process += 1;
    }

    fn log_state_change(&mut self, from: &'static str, to: &'static str) {
        let mut counts = self.0.borrow_mut();
        counts.last_change = Some((from, to));
        counts.changes += 1;
    }

    fn log_action(&mut self, action: &'static str, event: &'static str) {
        assert_eq!(action, "act");
        assert_eq!(event, "Go");
        self.0.borrow_mut().actions += 1;
    }

    fn log_guard(&mut self, guard: &'static str, event: &'static str, result: bool) {
        assert_eq!(guard, "allow");
        assert_eq!(event, "Go");
        assert!(result);
        self.0.borrow_mut().guards += 1;
    }
}

struct Gate {
    rejected: AtomicUsize,
}

impl Dispatch for Gate {
    fn dispatch(&self, event: &'static str) -> bool {
        if event == "Stop" {
            self.rejected.fetch_add(1, Ordering::Relaxed);
            false
        } else {
            true
        }
    }
}

#[test]
fn flat_logger_and_custom_dispatch_are_owner_policies() {
    let gate = Gate {
        rejected: AtomicUsize::new(0),
    };
    let log = Log::default();
    let observe = Observe::default();
    let policy = Policies::<Log, Observe, Gate>::with_dispatch(
        gate,
        log.clone(),
        observe.clone(),
        (),
        (),
        DefaultQueuePolicy,
        DefaultQueuePolicy,
    );
    let context = Context::default();
    let mut machine = PolicyFlatStateMachine::new_with_policy(&context, policy);

    assert_eq!(PolicyFlatEvents::EVENT_NAMES, &["Fail", "Go", "Stop"]);
    assert_eq!(PolicyFlatStates::name(&PolicyFlatStates::Idle), "Idle");
    assert_eq!(PolicyFlatEvents::name(&PolicyFlatEvents::Go(Go)), "Go");
    machine.process_event(Go).unwrap();
    assert!(machine.process_event(Stop).is_err());
    assert!(machine.process_event(Fail).is_err());
    assert_eq!(log.counts().process, 3);
    assert_eq!(log.counts().guards, 1);
    assert_eq!(log.counts().actions, 1);
    assert_eq!(log.counts().changes, 1);
    assert_eq!(log.counts().last_change, Some(("Idle", "Ready")));
    assert_eq!(context.actions.get(), 1);
    assert_eq!(context.process_events.get(), 3);
    assert_eq!(observe.0.get(), 3);
}

/// An observer that counts into a cell the test also holds.
#[derive(Clone, Default)]
struct Observe(Rc<Cell<usize>>);

impl Observer for Observe {
    fn observe_process_event(&mut self, _: &'static str) {
        self.0.set(self.0.get() + 1);
    }
}

sml::sml_policies!(ReversePolicies {
    testing: TestingPolicy,
    logger: Log,
});

pub struct Enter;
pub struct ChildWork;
pub struct ChildFail;

sml! {
    PolicyChild {
        *ChildIdle + event<ChildWork> = X,
        ChildIdle + event<ChildFail> / child_fail = X,
    }

    PolicyParent {
        *Outside + event<Enter> = state<PolicyChild>,
        state<PolicyChild> + event<Stop> = Outside,
    }
}

#[derive(Default)]
struct CompositeContext {
    process_events: Cell<usize>,
}

impl PolicyParentStateMachineContext for &CompositeContext {
    fn log_process_event(&self, _: &PolicyParentStates, _: &PolicyParentEvents) {
        self.process_events.set(self.process_events.get() + 1);
    }

    fn child_fail(&mut self, _: &ChildFail) -> Result<(), ()> {
        Err(())
    }
}

#[test]
fn composite_policy_is_shared_by_parent_and_child() {
    let log = Log::default();
    let policy = Policies::<Log, (), JumpTable>::with_dispatch(
        JumpTable,
        log.clone(),
        (),
        (),
        (),
        DefaultQueuePolicy,
        DefaultQueuePolicy,
    );
    let context = CompositeContext::default();
    let mut machine = PolicyParentStateMachine::new_with_policy(&context, policy);
    machine.process_event(Enter).unwrap();
    assert_eq!(log.counts().process, 1);
    assert_eq!(context.process_events.get(), 1);
    assert!(machine.is(&PolicyParentStates::PolicyChild));
    assert!(machine.process_event(ChildFail).is_err());
    assert_eq!(log.counts().process, 2);
    assert_eq!(log.counts().actions, 0);
    assert_eq!(context.process_events.get(), 2);
    assert_eq!(
        PolicyParentEvents::name(&PolicyParentEvents::Enter(Enter)),
        "Enter"
    );
}

pub struct Left;
pub struct Right;
pub struct OrthogonalFail;

sml! {
    PolicyOrthogonal {
        *A + event<Left> = X,
        A + event<OrthogonalFail> / region_fail = X,
        *B + event<Right> = X,
    }
}

#[derive(Default)]
struct OrthogonalContext {
    process_events: Cell<usize>,
}

impl PolicyOrthogonalStateMachineContext for &OrthogonalContext {
    fn log_process_event(&self, _: &[PolicyOrthogonalStates; 2], _: &PolicyOrthogonalEvents) {
        self.process_events.set(self.process_events.get() + 1);
    }

    fn region_fail(&mut self, _: &OrthogonalFail) -> Result<(), ()> {
        Err(())
    }
}

#[test]
fn orthogonal_policy_uses_the_same_dispatch_and_logger_slots() {
    let log = Log::default();
    let policy = Policies::<Log, (), BranchStm>::with_dispatch(
        BranchStm,
        log.clone(),
        (),
        (),
        (),
        DefaultQueuePolicy,
        DefaultQueuePolicy,
    );
    let context = OrthogonalContext::default();
    let mut machine = PolicyOrthogonalStateMachine::new_with_policy(&context, policy);
    assert!(machine.process_event(OrthogonalFail).is_err());
    assert_eq!(log.counts().actions, 0);
    machine.process_event(Left).unwrap();
    assert_eq!(log.counts().process, 2);
    assert_eq!(context.process_events.get(), 2);
    assert_eq!(
        PolicyOrthogonalStates::name(&PolicyOrthogonalStates::A),
        "A"
    );
}

#[derive(Default)]
struct TinyQueues;

impl QueuePolicy for TinyQueues {
    type Queue<E> = sml::utility::EventQueue<E, 2>;

    fn new_queue<E>(&self) -> Self::Queue<E> {
        sml::utility::EventQueue::new()
    }
}

pub struct Deferred;
pub struct Unlock;

sml! {
    PolicyQueue {
        *Idle + event<Deferred> / defer,
        Idle + event<Unlock> = Ready,
        Ready + event<Deferred> = X,
    }
}

struct QueueContext;
impl PolicyQueueStateMachineContext for QueueContext {}

type QueuePolicies = Policies<(), (), JumpTable, (), (), TinyQueues, TinyQueues>;

#[test]
fn custom_queue_factories_replace_the_default_queue() {
    let policy = QueuePolicies::with_dispatch(JumpTable, (), (), (), (), TinyQueues, TinyQueues);
    let mut machine = PolicyQueueStateMachine::new_with_policy(QueueContext, policy);
    machine.process_event(Deferred).unwrap();
    machine.process_event(Unlock).unwrap();
    assert!(machine.is(&PolicyQueueStates::X));
}

static LOCK_CALLS: AtomicUsize = AtomicUsize::new(0);
static LOCK_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct LockProbe;

struct LockGuard;

impl Drop for LockGuard {
    fn drop(&mut self) {}
}

impl RawMutex for LockProbe {
    type Guard<'a>
        = LockGuard
    where
        Self: 'a;

    fn lock(&self) -> Self::Guard<'_> {
        LOCK_CALLS.fetch_add(1, Ordering::Relaxed);
        LockGuard
    }
}

type LockedQueuePolicies =
    Policies<(), (), JumpTable, ThreadSafe<LockProbe>, (), TinyQueues, TinyQueues>;

#[test]
fn deferred_processing_does_not_reenter_the_thread_lock() {
    let _test_guard = LOCK_TEST.lock().expect("lock-policy tests must serialize");
    LOCK_CALLS.store(0, Ordering::Relaxed);
    let policy = LockedQueuePolicies::with_dispatch(
        JumpTable,
        (),
        (),
        ThreadSafe::new(LockProbe),
        (),
        TinyQueues,
        TinyQueues,
    );
    let mut machine = PolicyQueueStateMachine::new_with_policy(QueueContext, policy);
    machine.process_event(Deferred).unwrap();
    machine.process_event(Unlock).unwrap();
    assert!(machine.is(&PolicyQueueStates::X));
    assert_eq!(LOCK_CALLS.load(Ordering::Relaxed), 2);
}

#[test]
fn thread_safe_policy_wraps_event_processing() {
    let _test_guard = LOCK_TEST.lock().expect("lock-policy tests must serialize");
    LOCK_CALLS.store(0, Ordering::Relaxed);
    let policy = Policies::<(), (), JumpTable, ThreadSafe<LockProbe>>::with_dispatch(
        JumpTable,
        (),
        (),
        ThreadSafe::new(LockProbe),
        (),
        DefaultQueuePolicy,
        DefaultQueuePolicy,
    );
    let context = Context::default();
    let mut machine = PolicyFlatStateMachine::new_with_policy(&context, policy);
    machine.process_event(Go).unwrap();
    assert_eq!(LOCK_CALLS.load(Ordering::Relaxed), 1);
}

#[cfg(feature = "std")]
#[test]
fn standard_mutex_guard_is_borrowed_disjointly_from_policy_slots() {
    type StandardMutexPolicies = Policies<(), (), JumpTable, ThreadSafe<std::sync::Mutex<()>>>;
    let policy = StandardMutexPolicies::with_dispatch(
        JumpTable,
        (),
        (),
        ThreadSafe::new(std::sync::Mutex::new(())),
        (),
        DefaultQueuePolicy,
        DefaultQueuePolicy,
    );
    let context = Context::default();
    let mut machine = PolicyFlatStateMachine::new_with_policy(&context, policy);

    machine.process_event(Go).unwrap();
    assert!(machine.is(&PolicyFlatStates::Ready));
}

#[cfg(feature = "std")]
#[test]
fn standard_mutex_guard_is_disjoint_for_composite_and_orthogonal_machines() {
    type StandardMutexPolicies = Policies<(), (), JumpTable, ThreadSafe<std::sync::Mutex<()>>>;

    let composite_policy = StandardMutexPolicies::with_dispatch(
        JumpTable,
        (),
        (),
        ThreadSafe::new(std::sync::Mutex::new(())),
        (),
        DefaultQueuePolicy,
        DefaultQueuePolicy,
    );
    let composite_context = CompositeContext::default();
    let mut composite =
        PolicyParentStateMachine::new_with_policy(&composite_context, composite_policy);
    composite.process_event(Enter).unwrap();
    composite.process_event(ChildWork).unwrap();

    let orthogonal_policy = StandardMutexPolicies::with_dispatch(
        JumpTable,
        (),
        (),
        ThreadSafe::new(std::sync::Mutex::new(())),
        (),
        DefaultQueuePolicy,
        DefaultQueuePolicy,
    );
    let orthogonal_context = OrthogonalContext::default();
    let mut orthogonal =
        PolicyOrthogonalStateMachine::new_with_policy(&orthogonal_context, orthogonal_policy);
    orthogonal.process_event(Left).unwrap();
    assert!(region_is!(orthogonal, 0, PolicyOrthogonalStates::X));
}

#[test]
fn no_policy_is_zero_sized() {
    assert_eq!(core::mem::size_of::<NoPolicy>(), 0);
    assert_eq!(core::mem::size_of::<JumpTable>(), 0);
    assert_eq!(
        core::mem::size_of::<PolicyFlatStateMachine<&'static Context, NoPolicy>>(),
        core::mem::size_of::<PolicyFlatStateMachine<&'static Context>>()
    );
}

#[test]
fn policy_macro_allows_reordered_slots() {
    assert_eq!(
        core::mem::size_of::<ReversePolicies>(),
        core::mem::size_of::<Log>()
    );
}

#[test]
fn testing_policy_gates_state_setup_for_all_generators() {
    let flat_context = Context::default();
    let mut flat: PolicyFlatStateMachine<&Context, TestingPolicies> =
        PolicyFlatStateMachine::new_with_policy(&flat_context, TestingPolicies::default());
    flat.set_current_states(PolicyFlatStates::Ready);
    assert!(flat.is(&PolicyFlatStates::Ready));
    flat.set_current_states(PolicyFlatStates::Idle);

    let composite_context = CompositeContext::default();
    let mut composite: PolicyParentStateMachine<&CompositeContext, TestingPolicies> =
        PolicyParentStateMachine::new_with_policy(&composite_context, TestingPolicies::default());
    composite.set_current_states(PolicyParentStates::Outside);
    composite.set_child_state(PolicyParentPolicyChildStates::ChildIdle);

    let orthogonal_context = OrthogonalContext::default();
    let mut orthogonal: PolicyOrthogonalStateMachine<&OrthogonalContext, TestingPolicies> =
        PolicyOrthogonalStateMachine::new_with_policy(
            &orthogonal_context,
            TestingPolicies::default(),
        );
    orthogonal.set_current_states([PolicyOrthogonalStates::X, PolicyOrthogonalStates::B]);
    assert!(region_is!(orthogonal, 0, PolicyOrthogonalStates::X));
}
