//! The defer queue is drained the way sml.cpp drains it
//! (stateforward/sml.rs#17): after a step changes the state, each deferred
//! event is retried once in queue order; an event deferred again keeps its
//! place, and the pass starts over from the front only when a retried event
//! changes the state. An event deferred again, by an internal row or by an
//! external self-transition, therefore waits for the next state change
//! instead of being retried forever.
//!
//! Each case runs on a thread of its own so a spinning drain fails the test
//! instead of hanging the suite.

use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use sml::utility::QueueFull;
use sml::{sml, JumpTable, Policies, Queue, QueuePolicy};

/// How long a case may take before the test calls it a spin.
const SPIN: Duration = Duration::from_secs(5);

/// Runs `body` on a thread of its own, returning its result, or `None` when it
/// did not finish within `SPIN`.
fn within_spin<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(body());
    });
    finished.recv_timeout(SPIN).ok()
}

/// Builds and blocks on a future on a thread of its own, like `within_spin`.
fn block_within_spin<T: Send + 'static, F: core::future::Future<Output = T>>(
    body: impl FnOnce() -> F + Send + 'static,
) -> Option<T> {
    within_spin(move || smol::block_on(body()))
}

async fn yield_ok() -> Result<(), ()> {
    smol::future::yield_now().await;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Ask;
#[derive(Debug, Clone)]
pub struct Other;
#[derive(Debug, Clone)]
pub struct Go;
#[derive(Debug, Clone)]
pub struct Hop;
#[derive(Debug, Clone)]
pub struct Done;
#[derive(Debug, Clone)]
pub struct Fin;

/// Which row defers the ask again: an internal row (reached with `Go`) or an
/// external self-transition (reached with `Hop`).
#[derive(Clone, Copy)]
enum Row {
    Internal,
    SelfTransition,
}

// ---------------------------------------------------------------------------
// Flat, synchronous.
// ---------------------------------------------------------------------------

sml! {
    Deferring[states_attr: #[derive(Debug, Clone, Copy, Eq)]] {
        *Writing + event<Ask> / defer,
         Writing + event<Other> / defer,
         Writing + event<Go> = Listening,
         Writing + event<Hop> = Waiting,
         Writing + event<Done> = Idle,

         Listening + event<Ask> / defer,
         Listening + event<Other> / defer,
         Listening + event<Go> = Idle,
         Listening + event<Done>,

         Waiting + event<Ask> / defer = Waiting,
         Waiting + event<Hop> = Idle,

         Idle + event<Ask> = Busy,
         Idle + event<Other>,
         Idle + event<Go> = Writing,
         Idle + event<Done>,

         Busy + event<Other> / defer,
         Busy + event<Ask>,
         Busy + event<Go>,
         Busy + event<Done> = Idle,
    }
}

struct Plain;

impl DeferringStateMachineContext for Plain {}

const STATES: [DeferringStates; 5] = [
    DeferringStates::Idle,
    DeferringStates::Busy,
    DeferringStates::Listening,
    DeferringStates::Waiting,
    DeferringStates::Writing,
];

/// Runs `steps` on a fresh machine, returning the state it ended in.
fn run(steps: fn(&mut DeferringStateMachine<Plain>)) -> Option<DeferringStates> {
    within_spin(move || {
        let mut machine = DeferringStateMachine::new(Plain);
        steps(&mut machine);
        STATES.iter().copied().find(|state| machine.is(state))
    })
    .flatten()
}

/// An event deferred in one state and again by an internal row of the state a
/// transition leads to stays deferred, and the step returns.
#[test]
fn deferred_again_after_a_transition_waits() {
    let state = run(|machine| {
        machine.process_event(Ask).expect("deferred");
        machine
            .process_event(Go)
            .expect("to listening, ask deferred again");
        assert!(machine.is(&DeferringStates::Listening));
        machine.process_event(Go).expect("to idle, taking the ask");
    });
    assert_eq!(
        state,
        Some(DeferringStates::Busy),
        "the deferred ask was taken once idle"
    );
}

/// An event deferred again by an external self-transition stays deferred: the
/// self-transition does not change the state, so the pass does not start
/// over.
#[test]
fn deferred_again_by_a_self_transition_waits() {
    let state = run(|machine| {
        machine.process_event(Ask).expect("deferred");
        machine
            .process_event(Hop)
            .expect("to waiting, ask deferred again");
        assert!(machine.is(&DeferringStates::Waiting));
        machine.process_event(Hop).expect("to idle, taking the ask");
    });
    assert_eq!(state, Some(DeferringStates::Busy));
}

/// A drained event whose transition leads into a state that defers the next
/// one queued leaves it deferred, and the step returns.
#[test]
fn deferred_again_after_a_drained_state_change_waits() {
    let state = run(|machine| {
        machine.process_event(Ask).expect("deferred");
        machine.process_event(Other).expect("deferred");
        // Idle takes the ask (to busy), and busy defers the other again.
        machine.process_event(Done).expect("to idle");
        assert!(machine.is(&DeferringStates::Busy), "the ask was taken");
        machine
            .process_event(Done)
            .expect("to idle, the other drained and ignored");
    });
    assert_eq!(state, Some(DeferringStates::Idle), "every step returned");
}

/// Events deferred again keep their order for the next state change.
#[test]
fn deferred_again_keeps_order() {
    let state = run(|machine| {
        machine.process_event(Other).expect("deferred");
        machine.process_event(Ask).expect("deferred");
        machine
            .process_event(Go)
            .expect("to listening, both deferred again");
        // Idle ignores the other, then takes the ask.
        machine.process_event(Go).expect("to idle, then busy");
    });
    assert_eq!(state, Some(DeferringStates::Busy));
}

// ---------------------------------------------------------------------------
// Retry order, errors, and bounded queues (flat, synchronous).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct A;
#[derive(Debug, Clone)]
pub struct B;
#[derive(Debug, Clone)]
pub struct C;
#[derive(Debug, Clone)]
pub struct Item(pub u8);
#[derive(Debug, Clone)]
pub struct Stop;

sml! {
    Retrying {
        *Hold + event<A> / defer,
         Hold + event<B> / defer,
         Hold + event<C> / defer,
         Hold + event<Go> = First,

         First + event<A> / defer,
         First + event<B> / log_b = Second,
         First + event<C> / defer,

         Second + event<A> / log_a,
         Second + event<C> / log_c,
    }
}

/// A shared record of the actions taken, in order.
#[derive(Default, Clone)]
struct Log(Arc<Mutex<Vec<&'static str>>>);

impl Log {
    fn taken(&self) -> Vec<&'static str> {
        self.0.lock().expect("unpoisoned").clone()
    }
}

impl RetryingStateMachineContext for Log {
    fn log_a(&mut self, _: &A) -> Result<(), ()> {
        self.0.lock().expect("unpoisoned").push("a");
        Ok(())
    }
    fn log_b(&mut self, _: &B) -> Result<(), ()> {
        self.0.lock().expect("unpoisoned").push("b");
        Ok(())
    }
    fn log_c(&mut self, _: &C) -> Result<(), ()> {
        self.0.lock().expect("unpoisoned").push("c");
        Ok(())
    }
}

/// A retained event is retried again, in the same step, once a later drained
/// event changes the state, and ahead of the events not yet retried: the pass
/// starts over from the front, as in sml.cpp.
#[test]
fn a_retained_event_is_retried_first_after_a_drained_state_change() {
    let log = within_spin(|| {
        let log = Log::default();
        let mut machine = RetryingStateMachine::new(log.clone());
        machine.process_event(A).expect("deferred");
        machine.process_event(B).expect("deferred");
        machine.process_event(C).expect("deferred");
        // First retains a, b leads to second, then a and c are taken there.
        machine.process_event(Go).expect("to first, then second");
        assert!(machine.is(&RetryingStates::Second));
        log.taken()
    });
    assert_eq!(log, Some(vec!["b", "a", "c"]));
}

sml! {
    Faulting {
        *Hold + event<Item> / defer,
         Hold + event<Go> = Run,

         Run + event<Item> [is_odd] / defer,
         Run + event<Item> / take,
         Run + event<Stop> = Finished,

         Finished + event<Item> / record,
    }
}

/// A shared record of the items taken, in order.
#[derive(Default, Clone)]
struct Ledger(Arc<Mutex<Vec<u8>>>);

impl FaultingStateMachineContext for Ledger {
    fn is_odd(&self, item: &Item) -> Result<bool, ()> {
        Ok(item.0 % 2 == 1)
    }
    fn take(&mut self, item: &Item) -> Result<(), ()> {
        if item.0 == 0 {
            Err(())
        } else {
            self.0.lock().expect("unpoisoned").push(item.0);
            Ok(())
        }
    }
    fn record(&mut self, item: &Item) -> Result<(), ()> {
        self.0.lock().expect("unpoisoned").push(item.0);
        Ok(())
    }
}

/// When a drained event fails with a propagated error, the step returns it and
/// nothing else deferred is lost: events retained earlier in the pass stay
/// ahead of those not yet retried.
#[test]
fn a_propagated_error_keeps_the_rest_deferred_in_order() {
    let ledger = within_spin(|| {
        let ledger = Ledger::default();
        let mut machine = FaultingStateMachine::new(ledger.clone());
        for id in [1, 0, 2] {
            machine.process_event(Item(id)).expect("deferred");
        }
        // Run retains 1, then taking 0 fails.
        assert!(matches!(
            machine.process_event(Go),
            Err(FaultingError::ActionFailed(()))
        ));
        assert!(machine.is(&FaultingStates::Run));
        // Finished records what is left, in order.
        machine.process_event(Stop).expect("to finished");
        let taken = ledger.0.lock().expect("unpoisoned").clone();
        taken
    });
    assert_eq!(ledger, Some(vec![1, 2]));
}

#[derive(Default)]
struct TinyQueues;

impl QueuePolicy for TinyQueues {
    type Queue<E> = sml::utility::EventQueue<E, 2>;

    fn new_queue<E>(&self) -> Self::Queue<E> {
        sml::utility::EventQueue::new()
    }
}

type TinyPolicies = Policies<(), (), JumpTable, (), (), TinyQueues, TinyQueues>;

sml! {
    Crowded {
        *Hold + event<A> / defer,
         Hold + event<B> / defer,
         Hold + event<Go> = One,

         One + event<A> / defer = Two,
         One + event<B> / defer,

         Two + event<A> / log_a,
         Two + event<B> / log_b,
    }
}

impl CrowdedStateMachineContext for Log {
    fn log_a(&mut self, _: &A) -> Result<(), ()> {
        self.0.lock().expect("unpoisoned").push("a");
        Ok(())
    }
    fn log_b(&mut self, _: &B) -> Result<(), ()> {
        self.0.lock().expect("unpoisoned").push("b");
        Ok(())
    }
}

/// A full bounded queue drains without overflowing, even when a retained event
/// changes the state and the pass starts over.
#[test]
fn a_full_bounded_queue_starts_over_without_overflow() {
    let log = within_spin(|| {
        let log = Log::default();
        let policy = TinyPolicies::with_dispatch(JumpTable, (), (), (), (), TinyQueues, TinyQueues);
        let mut machine = CrowdedStateMachine::new_with_policy(log.clone(), policy);
        machine.process_event(A).expect("deferred");
        machine.process_event(B).expect("deferred, queue full");
        // One retains a and leads to two; there a, then b, are taken.
        machine.process_event(Go).expect("to one, then two");
        assert!(machine.is(&CrowdedStates::Two));
        log.taken()
    });
    assert_eq!(log, Some(vec!["a", "b"]));
}

/// A queue whose capacity comes from its policy factory; `Queue::new` makes
/// one that holds nothing, so a drain that builds queues itself fails.
pub struct Configured<E> {
    capacity: usize,
    events: VecDeque<E>,
}

impl<E> Queue<E> for Configured<E> {
    fn new() -> Self {
        Self {
            capacity: 0,
            events: VecDeque::new(),
        }
    }

    fn defer(&mut self, event: E) -> Result<(), QueueFull> {
        if self.events.len() >= self.capacity {
            return Err(QueueFull);
        }
        self.events.push_back(event);
        Ok(())
    }

    fn process(&mut self, event: E) -> Result<(), QueueFull> {
        if self.events.len() >= self.capacity {
            return Err(QueueFull);
        }
        self.events.push_front(event);
        Ok(())
    }

    fn pop(&mut self) -> Option<E> {
        self.events.pop_front()
    }
}

struct ConfiguredQueues(usize);

impl QueuePolicy for ConfiguredQueues {
    type Queue<E> = Configured<E>;

    fn new_queue<E>(&self) -> Self::Queue<E> {
        Configured {
            capacity: self.0,
            events: VecDeque::new(),
        }
    }
}

type ConfiguredPolicies = Policies<(), (), JumpTable, (), (), ConfiguredQueues, ConfiguredQueues>;

/// The drain keeps the queue the policy factory built.
#[test]
fn the_drain_keeps_the_policy_built_queue() {
    let busy = within_spin(|| {
        let policy = ConfiguredPolicies::with_dispatch(
            JumpTable,
            (),
            (),
            (),
            (),
            ConfiguredQueues(4),
            ConfiguredQueues(4),
        );
        let mut machine = DeferringStateMachine::new_with_policy(Plain, policy);
        machine.process_event(Ask).expect("deferred");
        machine
            .process_event(Go)
            .expect("to listening, ask deferred again");
        machine.process_event(Go).expect("to idle, taking the ask");
        machine.is(&DeferringStates::Busy)
    });
    assert_eq!(busy, Some(true));
}

// ---------------------------------------------------------------------------
// Flat, asynchronous.
// ---------------------------------------------------------------------------

sml! {
    AsyncDeferring {
        *Writing + event<Ask> / defer,
         Writing + event<Go> / async advance = Listening,
         Writing + event<Hop> / async advance_hop = Waiting,

         Listening + event<Ask> / defer,
         Listening + event<Go> = Idle,

         Waiting + event<Ask> / defer = Waiting,
         Waiting + event<Hop> = Idle,

         Idle + event<Ask> = Busy,
         Busy + event<Fin> = X,
    }
}

struct AsyncPlain;

impl AsyncDeferringStateMachineContext for AsyncPlain {
    async fn advance(&mut self, _: &Go) -> Result<(), ()> {
        yield_ok().await
    }
    async fn advance_hop(&mut self, _: &Hop) -> Result<(), ()> {
        yield_ok().await
    }
}

fn async_flat_run(row: Row) -> Option<bool> {
    block_within_spin(move || async move {
        let mut machine = AsyncDeferringStateMachine::new(AsyncPlain);
        machine.process_event(Ask).await.expect("deferred");
        match row {
            Row::Internal => {
                machine.process_event(Go).await.expect("to listening");
                machine.process_event(Go).await.expect("to idle");
            }
            Row::SelfTransition => {
                machine.process_event(Hop).await.expect("to waiting");
                machine.process_event(Hop).await.expect("to idle");
            }
        }
        machine.process_event(Fin).await.expect("busy to X");
        machine.is(&AsyncDeferringStates::X)
    })
}

#[test]
fn async_deferred_again_after_a_transition_waits() {
    assert_eq!(async_flat_run(Row::Internal), Some(true));
}

#[test]
fn async_deferred_again_by_a_self_transition_waits() {
    assert_eq!(async_flat_run(Row::SelfTransition), Some(true));
}

sml! {
    AsyncRetrying {
        *Hold + event<A> / defer,
         Hold + event<B> / defer,
         Hold + event<C> / defer,
         Hold + event<Go> / async advance = First,

         First + event<A> / defer,
         First + event<B> / log_b = Second,
         First + event<C> / defer,

         Second + event<A> / log_a,
         Second + event<C> / log_c,
    }
}

impl AsyncRetryingStateMachineContext for Log {
    async fn advance(&mut self, _: &Go) -> Result<(), ()> {
        yield_ok().await
    }
    fn log_a(&mut self, _: &A) -> Result<(), ()> {
        self.0.lock().expect("unpoisoned").push("a");
        Ok(())
    }
    fn log_b(&mut self, _: &B) -> Result<(), ()> {
        self.0.lock().expect("unpoisoned").push("b");
        Ok(())
    }
    fn log_c(&mut self, _: &C) -> Result<(), ()> {
        self.0.lock().expect("unpoisoned").push("c");
        Ok(())
    }
}

/// The async drain retries in queue order and starts over the same way.
#[test]
fn async_a_retained_event_is_retried_first_after_a_drained_state_change() {
    let log = block_within_spin(|| async {
        let log = Log::default();
        let mut machine = AsyncRetryingStateMachine::new(log.clone());
        machine.process_event(A).await.expect("deferred");
        machine.process_event(B).await.expect("deferred");
        machine.process_event(C).await.expect("deferred");
        machine
            .process_event(Go)
            .await
            .expect("to first, then second");
        assert!(machine.is(&AsyncRetryingStates::Second));
        log.taken()
    });
    assert_eq!(log, Some(vec!["b", "a", "c"]));
}

// ---------------------------------------------------------------------------
// Composite (one child), synchronous and asynchronous.
// ---------------------------------------------------------------------------

sml! {
    DrainChild {
        *Writing + event<Ask> / defer,
         Writing + event<Go> = Listening,
         Writing + event<Hop> = Waiting,

         Listening + event<Ask> / defer,
         Listening + event<Go> = Idle,

         Waiting + event<Ask> / defer = Waiting,
         Waiting + event<Hop> = Idle,

         Idle + event<Ask> = Busy,
         Busy + event<Fin> = X,
    }

    DrainParent {
        *state<DrainChild> + completion<_> = X,
    }
}

impl DrainParentStateMachineContext for Plain {}

fn composite_run(row: Row) -> Option<bool> {
    within_spin(move || {
        let mut machine = DrainParentStateMachine::new(Plain);
        machine.process_event(Ask).expect("deferred");
        match row {
            Row::Internal => {
                machine.process_event(Go).expect("to listening");
                machine.process_event(Go).expect("to idle");
            }
            Row::SelfTransition => {
                machine.process_event(Hop).expect("to waiting");
                machine.process_event(Hop).expect("to idle");
            }
        }
        machine.process_event(Fin).expect("busy to X");
        machine.is(&DrainParentStates::X)
    })
}

#[test]
fn composite_deferred_again_after_a_transition_waits() {
    assert_eq!(composite_run(Row::Internal), Some(true));
}

#[test]
fn composite_deferred_again_by_a_self_transition_waits() {
    assert_eq!(composite_run(Row::SelfTransition), Some(true));
}

sml! {
    AsyncDrainChild {
        *Writing + event<Ask> / defer,
         Writing + event<Go> / async advance = Listening,
         Writing + event<Hop> / async advance_hop = Waiting,

         Listening + event<Ask> / defer,
         Listening + event<Go> = Idle,

         Waiting + event<Ask> / defer = Waiting,
         Waiting + event<Hop> = Idle,

         Idle + event<Ask> = Busy,
         Busy + event<Fin> = X,
    }

    AsyncDrainParent {
        *state<AsyncDrainChild> + completion<_> = X,
    }
}

impl AsyncDrainParentStateMachineContext for AsyncPlain {
    async fn advance(&mut self, _: &Go) -> Result<(), ()> {
        yield_ok().await
    }
    async fn advance_hop(&mut self, _: &Hop) -> Result<(), ()> {
        yield_ok().await
    }
}

fn async_composite_run(row: Row) -> Option<bool> {
    block_within_spin(move || async move {
        let mut machine = AsyncDrainParentStateMachine::new(AsyncPlain);
        machine.process_event(Ask).await.expect("deferred");
        match row {
            Row::Internal => {
                machine.process_event(Go).await.expect("to listening");
                machine.process_event(Go).await.expect("to idle");
            }
            Row::SelfTransition => {
                machine.process_event(Hop).await.expect("to waiting");
                machine.process_event(Hop).await.expect("to idle");
            }
        }
        machine.process_event(Fin).await.expect("busy to X");
        machine.is(&AsyncDrainParentStates::X)
    })
}

#[test]
fn async_composite_deferred_again_after_a_transition_waits() {
    assert_eq!(async_composite_run(Row::Internal), Some(true));
}

#[test]
fn async_composite_deferred_again_by_a_self_transition_waits() {
    assert_eq!(async_composite_run(Row::SelfTransition), Some(true));
}

// ---------------------------------------------------------------------------
// Composite (two children), synchronous and asynchronous.
// ---------------------------------------------------------------------------

sml! {
    MultiChild {
        *Writing + event<Ask> / defer,
         Writing + event<Go> = Listening,
         Writing + event<Hop> = Waiting,

         Listening + event<Ask> / defer,
         Listening + event<Go> = Idle,

         Waiting + event<Ask> / defer = Waiting,
         Waiting + event<Hop> = Idle,

         Idle + event<Ask> = Busy,
         Busy + event<Fin> = X,
    }

    MultiSpare {
        *Resting + event<Done> = X,
    }

    MultiParent {
        *state<MultiChild> + completion<_> = X,
         state<MultiChild> + event<Other> = state<MultiSpare>,
    }
}

impl MultiParentStateMachineContext for Plain {}

fn multi_run(row: Row) -> Option<bool> {
    within_spin(move || {
        let mut machine = MultiParentStateMachine::new(Plain);
        machine.process_event(Ask).expect("deferred");
        match row {
            Row::Internal => {
                machine.process_event(Go).expect("to listening");
                machine.process_event(Go).expect("to idle");
            }
            Row::SelfTransition => {
                machine.process_event(Hop).expect("to waiting");
                machine.process_event(Hop).expect("to idle");
            }
        }
        machine.process_event(Fin).expect("busy to X");
        machine.is(&MultiParentStates::X)
    })
}

#[test]
fn multi_composite_deferred_again_after_a_transition_waits() {
    assert_eq!(multi_run(Row::Internal), Some(true));
}

#[test]
fn multi_composite_deferred_again_by_a_self_transition_waits() {
    assert_eq!(multi_run(Row::SelfTransition), Some(true));
}

sml! {
    AsyncMultiChild {
        *Writing + event<Ask> / defer,
         Writing + event<Go> / async advance = Listening,
         Writing + event<Hop> / async advance_hop = Waiting,

         Listening + event<Ask> / defer,
         Listening + event<Go> = Idle,

         Waiting + event<Ask> / defer = Waiting,
         Waiting + event<Hop> = Idle,

         Idle + event<Ask> = Busy,
         Busy + event<Fin> = X,
    }

    AsyncMultiSpare {
        *Resting + event<Done> = X,
    }

    AsyncMultiParent {
        *state<AsyncMultiChild> + completion<_> = X,
         state<AsyncMultiChild> + event<Other> = state<AsyncMultiSpare>,
    }
}

impl AsyncMultiParentStateMachineContext for AsyncPlain {
    async fn advance(&mut self, _: &Go) -> Result<(), ()> {
        yield_ok().await
    }
    async fn advance_hop(&mut self, _: &Hop) -> Result<(), ()> {
        yield_ok().await
    }
}

fn async_multi_run(row: Row) -> Option<bool> {
    block_within_spin(move || async move {
        let mut machine = AsyncMultiParentStateMachine::new(AsyncPlain);
        machine.process_event(Ask).await.expect("deferred");
        match row {
            Row::Internal => {
                machine.process_event(Go).await.expect("to listening");
                machine.process_event(Go).await.expect("to idle");
            }
            Row::SelfTransition => {
                machine.process_event(Hop).await.expect("to waiting");
                machine.process_event(Hop).await.expect("to idle");
            }
        }
        machine.process_event(Fin).await.expect("busy to X");
        machine.is(&AsyncMultiParentStates::X)
    })
}

#[test]
fn async_multi_composite_deferred_again_after_a_transition_waits() {
    assert_eq!(async_multi_run(Row::Internal), Some(true));
}

#[test]
fn async_multi_composite_deferred_again_by_a_self_transition_waits() {
    assert_eq!(async_multi_run(Row::SelfTransition), Some(true));
}

// ---------------------------------------------------------------------------
// Orthogonal, synchronous and asynchronous.
// ---------------------------------------------------------------------------

sml! {
    DrainRegions {
        *Writing + event<Ask> / defer,
         Writing + event<Go> = Listening,
         Writing + event<Hop> = Waiting,

         Listening + event<Ask> / defer,
         Listening + event<Go> = Idle,

         Waiting + event<Ask> / defer = Waiting,
         Waiting + event<Hop> = Idle,

         Idle + event<Ask> = Busy,
         Busy + event<Fin> = X,

        *Watch + event<Fin> = X,
    }
}

impl DrainRegionsStateMachineContext for Plain {}

fn orthogonal_run(row: Row) -> Option<bool> {
    within_spin(move || {
        let mut machine = DrainRegionsStateMachine::new(Plain);
        machine.process_event(Ask).expect("deferred");
        match row {
            Row::Internal => {
                machine.process_event(Go).expect("to listening");
                machine.process_event(Go).expect("to idle");
            }
            Row::SelfTransition => {
                machine.process_event(Hop).expect("to waiting");
                machine.process_event(Hop).expect("to idle");
            }
        }
        machine.process_event(Fin).expect("busy to X");
        machine.is(&[DrainRegionsStates::X, DrainRegionsStates::X])
    })
}

#[test]
fn orthogonal_deferred_again_after_a_transition_waits() {
    assert_eq!(orthogonal_run(Row::Internal), Some(true));
}

#[test]
fn orthogonal_deferred_again_by_a_self_transition_waits() {
    assert_eq!(orthogonal_run(Row::SelfTransition), Some(true));
}

sml! {
    AsyncDrainRegions {
        *Writing + event<Ask> / defer,
         Writing + event<Go> / async advance = Listening,
         Writing + event<Hop> / async advance_hop = Waiting,

         Listening + event<Ask> / defer,
         Listening + event<Go> = Idle,

         Waiting + event<Ask> / defer = Waiting,
         Waiting + event<Hop> = Idle,

         Idle + event<Ask> = Busy,
         Busy + event<Fin> = X,

        *Watch + event<Fin> = X,
    }
}

impl AsyncDrainRegionsStateMachineContext for AsyncPlain {
    async fn advance(&mut self, _: &Go) -> Result<(), ()> {
        yield_ok().await
    }
    async fn advance_hop(&mut self, _: &Hop) -> Result<(), ()> {
        yield_ok().await
    }
}

fn async_orthogonal_run(row: Row) -> Option<bool> {
    block_within_spin(move || async move {
        let mut machine = AsyncDrainRegionsStateMachine::new(AsyncPlain);
        machine.process_event(Ask).await.expect("deferred");
        match row {
            Row::Internal => {
                machine.process_event(Go).await.expect("to listening");
                machine.process_event(Go).await.expect("to idle");
            }
            Row::SelfTransition => {
                machine.process_event(Hop).await.expect("to waiting");
                machine.process_event(Hop).await.expect("to idle");
            }
        }
        machine.process_event(Fin).await.expect("busy to X");
        machine.is(&[AsyncDrainRegionsStates::X, AsyncDrainRegionsStates::X])
    })
}

#[test]
fn async_orthogonal_deferred_again_after_a_transition_waits() {
    assert_eq!(async_orthogonal_run(Row::Internal), Some(true));
}

#[test]
fn async_orthogonal_deferred_again_by_a_self_transition_waits() {
    assert_eq!(async_orthogonal_run(Row::SelfTransition), Some(true));
}

// ---------------------------------------------------------------------------
// Composite with an orthogonal root.
// ---------------------------------------------------------------------------

sml! {
    RootChild {
        *Writing + event<Ask> / defer,
         Writing + event<Go> = Listening,
         Writing + event<Hop> = Waiting,

         Listening + event<Ask> / defer,
         Listening + event<Go> = Idle,

         Waiting + event<Ask> / defer = Waiting,
         Waiting + event<Hop> = Idle,

         Idle + event<Ask> = Busy,
         Busy + event<Fin> = X,
    }

    RootRegions {
        *state<RootChild> + completion<_> = X,
        *Watch + event<Fin> = X,
    }
}

impl RootRegionsStateMachineContext for Plain {}

fn orthogonal_root_run(row: Row) -> Option<bool> {
    within_spin(move || {
        let mut machine = RootRegionsStateMachine::new(Plain);
        machine.process_event(Ask).expect("deferred");
        match row {
            Row::Internal => {
                machine.process_event(Go).expect("to listening");
                machine.process_event(Go).expect("to idle");
            }
            Row::SelfTransition => {
                machine.process_event(Hop).expect("to waiting");
                machine.process_event(Hop).expect("to idle");
            }
        }
        machine.process_event(Fin).expect("both regions to X");
        machine.is(&[RootRegionsStates::X, RootRegionsStates::X])
    })
}

#[test]
fn orthogonal_root_composite_deferred_again_after_a_transition_waits() {
    assert_eq!(orthogonal_root_run(Row::Internal), Some(true));
}

#[test]
fn orthogonal_root_composite_deferred_again_by_a_self_transition_waits() {
    assert_eq!(orthogonal_root_run(Row::SelfTransition), Some(true));
}

// ---------------------------------------------------------------------------
// Nested `process(...)` dispatches (review of stateforward/sml.rs#18).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Back;
#[derive(Debug, Clone)]
pub struct Fail;

/// Shared by the bouncing fixtures: `enable` opens the gate for the ask, and
/// each action taken is recorded in order.
fn gate_open(log: &Log) -> bool {
    log.0.lock().expect("unpoisoned").contains(&"enable")
}

fn record(log: &Log, entry: &'static str) {
    log.0.lock().expect("unpoisoned").push(entry);
}

sml! {
    Bouncing {
        *Hold + event<Ask> [enabled] / take,
         Hold + event<Ask> / defer,
         Hold + event<Go> / process(Back {}) = Ready,

         Ready + event<Back> / enable = Hold,
    }
}

impl BouncingStateMachineContext for Log {
    fn enabled(&self, _: &Ask) -> Result<bool, ()> {
        Ok(gate_open(self))
    }
    fn take(&mut self, _: &Ask) -> Result<(), ()> {
        record(self, "ask");
        Ok(())
    }
    fn enable(&mut self, _: &Back) -> Result<(), ()> {
        record(self, "enable");
        Ok(())
    }
}

/// A step whose nested `process(...)` returns to the starting state still
/// changed the state on the way, so the deferred events are retried.
#[test]
fn a_nested_round_trip_retries_deferred_events() {
    let log = within_spin(|| {
        let log = Log::default();
        let mut machine = BouncingStateMachine::new(log.clone());
        machine.process_event(Ask).expect("deferred");
        machine
            .process_event(Go)
            .expect("to ready and back to hold");
        assert!(machine.is(&BouncingStates::Hold));
        log.taken()
    });
    assert_eq!(log, Some(vec!["enable", "ask"]));
}

sml! {
    BouncingChild {
        *Hold + event<Ask> [enabled] / take,
         Hold + event<Ask> / defer,
         Hold + event<Go> / process(Back {}) = Ready,

         Ready + event<Back> / enable = Hold,
         Hold + event<Fin> = X,
    }

    BouncingParent {
        *state<BouncingChild> + completion<_> = X,
    }
}

impl BouncingParentStateMachineContext for Log {
    fn enabled(&self, _: &Ask) -> Result<bool, ()> {
        Ok(gate_open(self))
    }
    fn take(&mut self, _: &Ask) -> Result<(), ()> {
        record(self, "ask");
        Ok(())
    }
    fn enable(&mut self, _: &Back) -> Result<(), ()> {
        record(self, "enable");
        Ok(())
    }
}

#[test]
fn a_nested_round_trip_retries_deferred_events_in_a_composite() {
    let log = within_spin(|| {
        let log = Log::default();
        let mut machine = BouncingParentStateMachine::new(log.clone());
        machine.process_event(Ask).expect("deferred");
        machine
            .process_event(Go)
            .expect("to ready and back to hold");
        log.taken()
    });
    assert_eq!(log, Some(vec!["enable", "ask"]));
}

sml! {
    BouncingMultiChild {
        *Hold + event<Ask> [enabled] / take,
         Hold + event<Ask> / defer,
         Hold + event<Go> / process(Back {}) = Ready,

         Ready + event<Back> / enable = Hold,
         Hold + event<Fin> = X,
    }

    BouncingMultiSpare {
        *Resting + event<Done> = X,
    }

    BouncingMultiParent {
        *state<BouncingMultiChild> + completion<_> = X,
         state<BouncingMultiChild> + event<Other> = state<BouncingMultiSpare>,
    }
}

impl BouncingMultiParentStateMachineContext for Log {
    fn enabled(&self, _: &Ask) -> Result<bool, ()> {
        Ok(gate_open(self))
    }
    fn take(&mut self, _: &Ask) -> Result<(), ()> {
        record(self, "ask");
        Ok(())
    }
    fn enable(&mut self, _: &Back) -> Result<(), ()> {
        record(self, "enable");
        Ok(())
    }
}

#[test]
fn a_nested_round_trip_retries_deferred_events_in_a_multi_child_composite() {
    let log = within_spin(|| {
        let log = Log::default();
        let mut machine = BouncingMultiParentStateMachine::new(log.clone());
        machine.process_event(Ask).expect("deferred");
        machine
            .process_event(Go)
            .expect("to ready and back to hold");
        log.taken()
    });
    assert_eq!(log, Some(vec!["enable", "ask"]));
}

sml! {
    BouncingRegions {
        *Hold + event<Ask> [enabled] / take,
         Hold + event<Ask> / defer,
         Hold + event<Go> / process(Back {}) = Ready,

         Ready + event<Back> / enable = Hold,

        *Watch + event<Fin> = X,
    }
}

impl BouncingRegionsStateMachineContext for Log {
    fn enabled(&self, _: &Ask) -> Result<bool, ()> {
        Ok(gate_open(self))
    }
    fn take(&mut self, _: &Ask) -> Result<(), ()> {
        record(self, "ask");
        Ok(())
    }
    fn enable(&mut self, _: &Back) -> Result<(), ()> {
        record(self, "enable");
        Ok(())
    }
}

#[test]
fn a_nested_round_trip_retries_deferred_events_in_regions() {
    let log = within_spin(|| {
        let log = Log::default();
        let mut machine = BouncingRegionsStateMachine::new(log.clone());
        machine.process_event(Ask).expect("deferred");
        machine
            .process_event(Go)
            .expect("to ready and back to hold");
        log.taken()
    });
    assert_eq!(log, Some(vec!["enable", "ask"]));
}

sml! {
    Interleaving {
        *Hold + event<A> / defer,
         Hold + event<B> / defer,
         Hold + event<Go> = First,

         First + event<A> / (process(C {}), process(Fail {})) = Second,
         First + event<B> / defer,

         Second + event<B> / defer,
         Second + event<C> / defer,
         Second + event<Fail> / fail,
         Second + event<Go> = Third,

         Third + event<B> / log_b,
         Third + event<C> / log_c,
    }
}

impl InterleavingStateMachineContext for Log {
    fn fail(&mut self, _: &Fail) -> Result<(), ()> {
        Err(())
    }
    fn log_b(&mut self, _: &B) -> Result<(), ()> {
        record(self, "b");
        Ok(())
    }
    fn log_c(&mut self, _: &C) -> Result<(), ()> {
        record(self, "c");
        Ok(())
    }
}

/// When a drained event fails, the events it deferred through nested
/// `process(...)` calls stay behind the deferred events not yet retried.
#[test]
fn a_propagated_error_keeps_new_deferrals_behind_untouched_events() {
    let log = within_spin(|| {
        let log = Log::default();
        let mut machine = InterleavingStateMachine::new(log.clone());
        machine.process_event(A).expect("deferred");
        machine.process_event(B).expect("deferred");
        // A leads to second, defers c there, and failing aborts the step.
        assert!(matches!(
            machine.process_event(Go),
            Err(InterleavingError::ActionFailed(()))
        ));
        assert!(machine.is(&InterleavingStates::Second));
        machine.process_event(Go).expect("to third");
        log.taken()
    });
    assert_eq!(log, Some(vec!["b", "c"]));
}
