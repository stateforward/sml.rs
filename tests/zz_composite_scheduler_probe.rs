//! Probe: composite machines (single-child and multi-child generators) with
//! a queued scheduler policy and the testing-only accessor surface.
#![cfg(feature = "std")]

use sml::{sml, sml_policies, FifoScheduler, Terminated};
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
sml_policies!(Testing {
    testing: sml::TestingPolicy
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

#[test]
fn single_child_composite_runs_on_a_fifo_scheduler() {
    let handle: ProbeJobStateMachine<Ctx, Fifo> =
        ProbeJobStateMachine::new_with_policy(Ctx, Fifo::default());
    let clone = handle.clone();
    assert!(ready(clone.try_process_event(Start).expect("room")));
    assert!(ready(handle.process_event_async(Step)));
    assert!(!ready(handle.process_event_async(Start)));
    assert!(!Terminated::is_terminated(&handle));
    assert!(ready(handle.process_event_async(Stop)));
    assert!(Terminated::is_terminated(&clone));
}

#[test]
fn multi_child_composite_runs_on_a_fifo_scheduler() {
    let handle: ProbeTreeStateMachine<Ctx, Fifo> =
        ProbeTreeStateMachine::new_with_policy(Ctx, Fifo::default());
    let clone = handle.clone();
    assert!(ready(clone.try_process_event(Start).expect("room")));
    assert!(ready(handle.process_event_async(Step)));
    assert!(ready(handle.process_event_async(Other)));
    assert!(ready(handle.process_event_async(Other)));
    assert!(ready(handle.process_event_async(Stop)));
    assert!(Terminated::is_terminated(&clone));
}

#[test]
fn unscheduled_composites_keep_the_testing_accessors() {
    let mut job: ProbeJobStateMachine<Ctx, Testing> =
        ProbeJobStateMachine::new_with_policy(Ctx, Testing::default());
    job.initialize().expect("initialize");
    assert_eq!(job.process_event(Start), Ok(true));
    assert_eq!(job.process_event(Start), Ok(false));
    assert!(job.state() == &ProbeJobStates::ProbeWork);
    assert!(job.child_is_active());
    let _: &Ctx = job.context();
    assert!(sml::Machine::process_event(
        &mut job,
        ProbeJobEvents::from(Stop)
    ));
    assert!(job.is_terminated());

    let mut tree: ProbeTreeStateMachine<Ctx, Testing> =
        ProbeTreeStateMachine::new_with_policy(Ctx, Testing::default());
    tree.initialize().expect("initialize");
    assert_eq!(tree.process_event(Start), Ok(true));
    assert!(tree.state() == &ProbeTreeStates::ProbeLeft);
    let _: &Ctx = tree.context();

    let mut plain = ProbeJobStateMachine::new(Ctx);
    plain.initialize().expect("initialize");
    assert_eq!(plain.process_event(Start), Ok(true));
}

// Async and temporary-context composites keep the unscheduled API.
pub struct AsyncGo;
pub struct AsyncChildGo;

sml! {
    ProbeAsyncSub {
        *"idle"_s + event<AsyncChildGo> [async child_ready] / async child_ran = X,
    }

    ProbeAsyncParent {
        *"idle"_s + event<AsyncGo> = state<ProbeAsyncSub>,
         state<ProbeAsyncSub> + event<Stop> = X,
    }
}

impl ProbeAsyncParentStateMachineContext for Ctx {
    async fn child_ready(&self, _event: &AsyncChildGo) -> Result<bool, ()> {
        core::future::ready(()).await;
        Ok(true)
    }

    async fn child_ran(&mut self, _event: &AsyncChildGo) -> Result<(), ()> {
        core::future::ready(()).await;
        Ok(())
    }
}

pub struct TempEnter;
pub struct TempAdvance;

sml! {
    ProbeTempChild {
        *"idle"_s + event<TempAdvance> / temp_child = X,
    }

    ProbeTempParent[temporary_context: &mut usize] {
        *"outside"_s + event<TempEnter> = state<ProbeTempChild>,
         state<ProbeTempChild> + completion<_> = X,
    }
}

impl ProbeTempParentStateMachineContext for Ctx {
    fn temp_child(&mut self, temporary: &mut usize, _: &TempAdvance) -> Result<(), ()> {
        *temporary += 1;
        Ok(())
    }
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
fn async_and_temporary_context_composites_process_events() {
    smol::block_on(async {
        let mut machine = ProbeAsyncParentStateMachine::new(Ctx);
        machine.initialize().await.expect("initialize");
        assert_eq!(machine.process_event(AsyncGo).await, Ok(true));
        assert_eq!(machine.process_event(AsyncChildGo).await, Ok(true));
    });

    let mut machine = ProbeTempParentStateMachine::new(Ctx);
    let mut temporary = 0;
    machine.initialize(&mut temporary).expect("initialize");
    assert_eq!(machine.process_event(&mut temporary, TempEnter), Ok(true));
    assert_eq!(machine.process_event(&mut temporary, TempAdvance), Ok(true));
    assert_eq!(temporary, 1);
    assert!(machine.is_terminated());
}

#[test]
fn orthogonal_root_composite_runs_on_a_fifo_scheduler() {
    let handle: ProbeOrthoRootStateMachine<Ctx, Fifo> =
        ProbeOrthoRootStateMachine::new_with_policy(Ctx, Fifo::default());
    assert!(ready(handle.process_event_async(Broadcast)));
    assert!(Terminated::is_terminated(&handle));
}
