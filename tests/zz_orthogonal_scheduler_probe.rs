//! Probe: orthogonal machines under a scheduler policy and the testing API.
#![cfg(feature = "std")]

use sml::{sml, sml_policies, FifoScheduler, Terminated};
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

pub struct Go;
pub struct Stop;

sml! {
    Dual {
        *Idle + event<Go> / record = Busy,
         Busy + event<Stop> = X,

        *Idle2 + event<Go> = Busy2,
         Busy2 + event<Stop> = X,
    }
}

#[derive(Default)]
pub struct Recorder {
    seen: usize,
}

impl DualStateMachineContext for Recorder {
    fn record(&mut self, _event: &Go) -> Result<(), ()> {
        self.seen += 1;
        Ok(())
    }
}

sml_policies!(Fifo { scheduler: FifoScheduler<4> });
sml_policies!(Testing {
    testing: sml::TestingPolicy
});

fn ready<F: Future>(future: F) -> Option<F::Output> {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => Some(output),
        Poll::Pending => None,
    }
}

#[test]
fn scheduled_orthogonal_machine_runs_queued_events() {
    let machine = DualStateMachine::new_with_policy(Recorder::default(), Fifo::default());
    let handle = machine.clone();
    assert!(!Terminated::is_terminated(&machine));
    assert_eq!(ready(machine.process_event_async(Go)), Some(true));
    assert_eq!(ready(machine.process_event_async(Go)), Some(false));
    let stop = handle.try_process_event(Stop).expect("queue has room");
    assert_eq!(ready(stop), Some(true));
    assert!(Terminated::is_terminated(&machine));
    assert!(Terminated::is_terminated(&handle));
}

#[test]
fn unscheduled_orthogonal_machine_keeps_testing_accessors() {
    let mut machine = DualStateMachine::new_with_policy(Recorder::default(), Testing::default());
    machine.initialize().expect("initialize");
    assert!(machine.is(&[DualStates::Idle, DualStates::Idle2]));
    assert_eq!(machine.process_event(Go), Ok(true));
    assert_eq!(machine.process_event(Go), Ok(false));
    assert!(machine.state(0) == Some(&DualStates::Busy));
    assert!(machine.is_region(1, &DualStates::Busy2));
    assert_eq!(machine.context().seen, 1);
    machine.context_mut().seen = 7;
    assert_eq!(machine.states().len(), 2);
    assert_eq!(machine.process_event(Stop), Ok(true));
    assert!(machine.is_terminated());
    assert!(Terminated::is_terminated(&machine));
    assert_eq!(machine.context().seen, 7);

    let mut plain = DualStateMachine::new(Recorder::default());
    plain.initialize().expect("initialize");
    assert_eq!(plain.process_event(Go), Ok(true));
    assert!(plain.is(&[DualStates::Busy, DualStates::Busy2]));
}
