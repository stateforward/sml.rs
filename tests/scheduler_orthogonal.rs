//! Orthogonal machines under a scheduler policy.
#![cfg(feature = "std")]

use sml::{sml, sml_policies, FifoScheduler, StateName};
use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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

/// Counts `record` calls through a shared counter the test also holds.
#[derive(Default)]
pub struct Recorder {
    seen: Arc<AtomicUsize>,
}

impl DualStateMachineContext for Recorder {
    fn record(&mut self, _event: &Go) -> Result<(), ()> {
        self.seen.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

sml_policies!(Fifo { scheduler: FifoScheduler<4> });

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
    let recorder = Recorder::default();
    let seen = Arc::clone(&recorder.seen);
    let machine = DualStateMachine::new_with_policy(recorder, Fifo::default());
    let handle = machine.clone();
    assert!(machine.is(&[DualStates::Idle, DualStates::Idle2]));
    assert_eq!(ready(machine.process_event_async(Go)), Some(true));
    assert_eq!(ready(machine.process_event_async(Go)), Some(false));
    let mut regions = Vec::new();
    handle.visit_current_states(|state| regions.push(state.name()));
    assert_eq!(regions, vec!["Busy", "Busy2"]);
    let stop = handle.try_process_event(Stop).expect("queue has room");
    assert_eq!(ready(stop), Some(true));
    assert!(handle.is(&[DualStates::X, DualStates::X]));
    // Both regions are terminated: neither handle accepts further events.
    assert_eq!(ready(machine.process_event_async(Go)), Some(false));
    assert_eq!(ready(handle.process_event_async(Stop)), Some(false));
    assert_eq!(seen.load(Ordering::Relaxed), 1);
}

#[test]
fn unscheduled_orthogonal_machine_keeps_the_owned_api() {
    let mut plain = DualStateMachine::new(Recorder::default());
    plain.initialize().expect("initialize");
    assert!(plain.is(&[DualStates::Idle, DualStates::Idle2]));
    assert!(plain.process_event(Go).is_ok());
    assert!(plain.process_event(Go).is_err());
    assert!(plain.process_event(Stop).is_ok());
    assert!(plain.process_event(Stop).is_err());
}
