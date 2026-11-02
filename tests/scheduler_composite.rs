//! Composite machines (single-child and multi-child generators) with a queued
//! scheduler policy. A scheduled handle answers `is` and
//! `visit_current_states` under the machine's lock.
#![cfg(feature = "std")]

use sml::{sml, sml_policies, FifoScheduler, StateName};
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Start;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Other;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop;

// Single-child composite: the `generate_code` path.
sml! {
    ProbeWork {
        *Planning + event<Step> = Done,
    }

    ProbeJob {
        *Idle + event<Start> = state<ProbeWork>,
         state<ProbeWork> + event<Stop> = X,
    }
}

// Two children: the `generate_multi_code` path.
sml! {
    ProbeLeft {
        *LeftA + event<Step> = LeftB,
    }

    ProbeRight {
        *RightA + event<Other> = RightB,
    }

    ProbeTree {
        *Idle + event<Start> = state<ProbeLeft>,
         state<ProbeLeft> + event<Other> = state<ProbeRight>,
         state<ProbeRight> + event<Stop> = X,
    }
}

#[derive(Default)]
pub struct Ctx;

impl ProbeJobStateMachineContext for Ctx {}
impl ProbeTreeStateMachineContext for Ctx {}

sml_policies!(Fifo {
    scheduler: FifoScheduler<4>
});

fn ready<F: Future<Output = bool>>(future: F) -> bool {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(accepted) => accepted,
        Poll::Pending => panic!("a FIFO machine that is not busy drains inline"),
    }
}

fn names<S: StateName>(visit: impl FnOnce(&mut dyn FnMut(&S))) -> Vec<&'static str> {
    let mut seen = Vec::new();
    visit(&mut |state: &S| seen.push(state.name()));
    seen
}

#[test]
fn single_child_composite_runs_on_a_fifo_scheduler() {
    let handle: ProbeJobStateMachine<Ctx, Fifo> =
        ProbeJobStateMachine::new_with_policy(Ctx, Fifo::default());
    let clone = handle.clone();
    assert!(handle.is(&ProbeJobStates::Idle));
    assert!(ready(clone.try_process_event(Start).expect("room")));
    assert!(handle.is(&ProbeJobStates::ProbeWork));
    assert!(ready(handle.process_event_async(Step)));
    assert!(!ready(handle.process_event_async(Start)));
    assert_eq!(
        names(|visitor| handle.visit_current_states(visitor)),
        vec!["ProbeWork"]
    );
    assert!(ready(handle.process_event_async(Stop)));
    assert!(clone.is(&ProbeJobStates::X));
    assert!(!ready(clone.process_event_async(Stop)));
    assert!(!ready(clone.process_event_async(Start)));
}

#[test]
fn multi_child_composite_runs_on_a_fifo_scheduler() {
    let handle: ProbeTreeStateMachine<Ctx, Fifo> =
        ProbeTreeStateMachine::new_with_policy(Ctx, Fifo::default());
    let clone = handle.clone();
    assert!(ready(clone.try_process_event(Start).expect("room")));
    assert!(handle.is(&ProbeTreeStates::ProbeLeft));
    assert!(ready(handle.process_event_async(Step)));
    assert!(ready(handle.process_event_async(Other)));
    assert!(handle.is(&ProbeTreeStates::ProbeRight));
    assert!(ready(handle.process_event_async(Other)));
    assert!(ready(handle.process_event_async(Stop)));
    assert_eq!(
        names(|visitor| clone.visit_current_states(visitor)),
        vec!["X"]
    );
    assert!(!ready(clone.process_event_async(Stop)));
    assert!(!ready(clone.process_event_async(Start)));
}

#[test]
fn unscheduled_composites_keep_the_owned_api() {
    let mut job = ProbeJobStateMachine::new(Ctx);
    job.initialize().expect("initialize");
    assert!(job.process_event(Start).is_ok());
    assert!(job.process_event(Start).is_err());
    assert!(job.is(&ProbeJobStates::ProbeWork));
    assert!(sml::Machine::process_event(
        &mut job,
        ProbeJobEvents::from(Stop)
    ));
    assert!(job.is(&ProbeJobStates::X));

    let mut tree = ProbeTreeStateMachine::new(Ctx);
    tree.initialize().expect("initialize");
    assert!(tree.process_event(Start).is_ok());
    assert!(tree.is(&ProbeTreeStates::ProbeLeft));
}

// An orthogonal root: no `Machine<Events>`, but still schedulable.
pub struct Broadcast;

sml! {
    ProbeRegionChild {
        *"waiting"_s + event<Broadcast> = X,
    }

    ProbeOrthoRoot {
        *state<ProbeRegionChild> = X,
        *"parallel"_s + event<Broadcast> = X,
    }
}

impl ProbeOrthoRootStateMachineContext for Ctx {}

#[test]
fn orthogonal_root_composite_runs_on_a_fifo_scheduler() {
    let handle: ProbeOrthoRootStateMachine<Ctx, Fifo> =
        ProbeOrthoRootStateMachine::new_with_policy(Ctx, Fifo::default());
    assert_eq!(
        names(|visitor| handle.visit_current_states(visitor)).len(),
        2
    );
    assert!(ready(handle.process_event_async(Broadcast)));
    assert!(!ready(handle.process_event_async(Broadcast)));
}
