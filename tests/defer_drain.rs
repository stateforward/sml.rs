//! The defer queue drains each event once per state change, as in sml.cpp:
//! an event the new state defers again waits for the next state change
//! instead of being popped again forever (stateforward/sml.rs#17). Each case
//! runs on a thread of its own so a spinning drain fails the test instead of
//! hanging it.

use std::sync::mpsc;
use std::time::Duration;

use sml::sml;

/// How long a case may take before the test calls it a spin.
const SPIN: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct Ask;
#[derive(Debug)]
pub struct Other;
#[derive(Debug)]
pub struct Go;
#[derive(Debug)]
pub struct Done;

sml! {
    Deferring[states_attr: #[derive(Debug, Clone, Copy, Eq)]] {
        *Writing + event<Ask> / defer,
         Writing + event<Other> / defer,
         Writing + event<Go> = Listening,
         Writing + event<Done> = Idle,

         Listening + event<Ask> / defer,
         Listening + event<Other> / defer,
         Listening + event<Go> = Idle,
         Listening + event<Done>,

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

const STATES: [DeferringStates; 4] = [
    DeferringStates::Idle,
    DeferringStates::Busy,
    DeferringStates::Listening,
    DeferringStates::Writing,
];

/// Runs `body` on a thread of its own, returning its result, or `None` when
/// it did not finish within `SPIN`.
fn within_spin<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(body());
    });
    finished.recv_timeout(SPIN).ok()
}

/// Runs `steps` on a fresh machine, returning the state it ended in.
fn run(steps: fn(&mut DeferringStateMachine<Plain>)) -> Option<DeferringStates> {
    within_spin(move || {
        let mut machine = DeferringStateMachine::new(Plain);
        steps(&mut machine);
        STATES.iter().copied().find(|state| machine.is(state))
    })
    .flatten()
}

/// An event deferred in one state and again in the state a transition leads
/// to stays deferred, and the step returns.
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

/// A drained event whose transition leads into a state that defers the next
/// one queued (a nested drain) leaves it deferred, and the step returns.
#[test]
fn deferred_again_in_a_nested_drain_waits() {
    let state = run(|machine| {
        machine.process_event(Ask).expect("deferred");
        machine.process_event(Other).expect("deferred");
        // Idle takes the ask (to busy, a nested drain), and busy defers the
        // other again.
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
    assert_eq!(
        state,
        Some(DeferringStates::Busy),
        "the ask came after the other"
    );
}

sml! {
    AsyncDeferring[states_attr: #[derive(Debug, Clone, Copy, Eq)]] {
        *Writing + event<Ask> / defer,
         Writing + event<Go> / async go = Listening,

         Listening + event<Ask> / defer,
         Listening + event<Go> = Idle,

         Idle + event<Ask> = Busy,
    }
}

struct AsyncPlain;

impl AsyncDeferringStateMachineContext for AsyncPlain {
    async fn go(&mut self, _: &Go) -> Result<(), ()> {
        smol::future::yield_now().await;
        Ok(())
    }
}

/// The async machine's drain hands each deferred event over once too.
#[test]
fn async_deferred_again_after_a_transition_waits() {
    let state = within_spin(|| {
        smol::block_on(async {
            let mut machine = AsyncDeferringStateMachine::new(AsyncPlain);
            machine.process_event(Ask).await.expect("deferred");
            machine.process_event(Go).await.expect("to listening");
            assert!(machine.is(&AsyncDeferringStates::Listening));
            machine
                .process_event(Go)
                .await
                .expect("to idle, taking the ask");
            [
                AsyncDeferringStates::Idle,
                AsyncDeferringStates::Busy,
                AsyncDeferringStates::Listening,
                AsyncDeferringStates::Writing,
            ]
            .iter()
            .copied()
            .find(|state| machine.is(state))
        })
    })
    .flatten();
    assert_eq!(state, Some(AsyncDeferringStates::Busy));
}
