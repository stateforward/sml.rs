//! Scheduler policies: shared machines whose events are queued at call time
//! and run to completion one at a time.
#![cfg(feature = "std")]

use sml::{sml, sml_policies, FifoScheduler, InlineScheduler, Step, StepHook, Terminated};
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::task::{Context, Poll, Waker};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Push(u32);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Again;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unknown;

sml! {
    Counter {
        *Idle + event<Push> / record = Idle,
        Idle + event<Again> / process(Push(99)) = Idle,
        Idle + event<Stop> = X,
        Idle + event<Unknown> [never] = Idle,
    }
}

type Handle = CounterStateMachine<Recorder, Fifo>;
type Waiting = sml::FifoCompletion<__SmlCounterStateMachineCore<Recorder, Fifo>, 4>;

/// What a re-entrant action did, shared with the test.
#[derive(Default)]
pub struct Reentry {
    this: OnceLock<Handle>,
    tried: Mutex<Vec<bool>>,
    waiting: Mutex<Option<Waiting>>,
}

#[derive(Default)]
pub struct Recorder {
    seen: Vec<u32>,
    reentry: Arc<Reentry>,
}

impl CounterStateMachineContext for Recorder {
    fn never(&self, _event: &Unknown) -> Result<bool, ()> {
        Ok(false)
    }

    fn record(&mut self, event: &Push) -> Result<(), ()> {
        self.seen.push(event.0);
        if event.0 == 1 {
            if let Some(this) = self.reentry.this.get() {
                // Re-entering a busy FIFO machine only queues the event; the
                // draining caller runs each after this one completes.
                let tried = (10..15)
                    .map(|value| this.try_process_event(Push(value)).is_ok())
                    .collect();
                *lock(&self.reentry.tried) = tried;
                *lock(&self.reentry.waiting) = Some(this.process_event_async(Push(20)));
            }
        }
        Ok(())
    }
}

type Journaled = Arc<Mutex<Vec<(Step, Vec<u32>)>>>;

/// Journals each step and drains the context's output.
#[derive(Default)]
pub struct Journal {
    steps: Journaled,
}

impl<L> StepHook<Recorder, L, CounterEvents> for Journal {
    fn after_step(&mut self, context: &mut Recorder, _logger: &mut L, step: Step) {
        let drained = core::mem::take(&mut context.seen);
        lock(&self.steps).push((step, drained));
    }
}

sml_policies!(Fifo { scheduler: FifoScheduler<4>, step_hook: Journal });
sml_policies!(Inline {
    scheduler: InlineScheduler,
    step_hook: Journal
});

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn ready<F: Future>(future: F) -> Option<F::Output> {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => Some(value),
        Poll::Pending => None,
    }
}

fn journaled<P>(build: impl FnOnce(Journal) -> P) -> (P, Journaled) {
    let journal = Journal::default();
    let steps = Arc::clone(&journal.steps);
    (build(journal), steps)
}

const fn step(event: &'static str, accepted: bool, terminated: bool) -> Step {
    Step {
        event,
        accepted,
        terminated,
    }
}

#[test]
fn fifo_runs_an_idle_machines_event_before_returning() {
    let (policy, steps) = journaled(|journal| Fifo::default().with_step_hook(journal));
    let machine = CounterStateMachine::new_with_policy(Recorder::default(), policy);
    let handle = machine.clone();

    let completion = handle.process_event_async(Push(7));
    // Dropping a completion never cancels its event.
    drop(machine.process_event_async(Again));
    assert_eq!(ready(completion), Some(true));
    assert_eq!(ready(handle.process_event_async(Unknown)), Some(false));
    assert_eq!(
        *lock(&steps),
        vec![
            (step("Push", true, false), vec![7]),
            (step("Again", true, false), vec![99]),
            (step("Unknown", false, false), vec![]),
        ]
    );
}

#[test]
fn fifo_queues_reentrant_events_bounded_and_waits_for_room() {
    let (policy, steps) = journaled(|journal| Fifo::default().with_step_hook(journal));
    let reentry = Arc::new(Reentry::default());
    let machine = CounterStateMachine::new_with_policy(
        Recorder {
            seen: Vec::new(),
            reentry: Arc::clone(&reentry),
        },
        policy,
    );
    assert!(reentry.this.set(machine.clone()).is_ok());

    assert_eq!(ready(machine.process_event_async(Push(1))), Some(true));
    // Four events fit the queue; the fifth is refused without being queued.
    assert_eq!(*lock(&reentry.tried), vec![true, true, true, true, false]);
    let seen: Vec<_> = lock(&steps).iter().map(|(_, seen)| seen.clone()).collect();
    assert_eq!(seen, vec![vec![1], vec![10], vec![11], vec![12], vec![13]]);

    // The async entry point waited for room instead; the machine is idle now,
    // so polling it runs the event.
    let waiting = lock(&reentry.waiting).take();
    assert_eq!(waiting.and_then(ready), Some(true));
    assert_eq!(
        lock(&steps).last().map(|(_, seen)| seen.clone()),
        Some(vec![20])
    );
}

#[test]
fn fifo_resolves_pending_events_false_once_terminated() {
    let machine = CounterStateMachine::new_with_policy(Recorder::default(), Fifo::default());
    assert!(!Terminated::is_terminated(&machine));
    assert_eq!(ready(machine.process_event_async(Stop)), Some(true));
    assert!(Terminated::is_terminated(&machine));
    assert_eq!(ready(machine.process_event_async(Push(1))), Some(false));
    assert_eq!(
        machine.try_process_event(Push(1)).ok().and_then(ready),
        Some(false)
    );
}

#[test]
fn inline_runs_each_event_under_the_shared_lock() {
    let (policy, steps) = journaled(|journal| Inline::default().with_step_hook(journal));
    let machine = CounterStateMachine::new_with_policy(Recorder::default(), policy);
    let handle = machine.clone();
    let worker = std::thread::spawn(move || ready(handle.process_event_async(Push(3))));
    assert_eq!(worker.join().ok().flatten(), Some(true));
    assert_eq!(ready(machine.process_event_async(Stop)), Some(true));
    assert_eq!(
        machine.try_process_event(Push(4)).ok().and_then(ready),
        Some(false)
    );
    assert!(Terminated::is_terminated(&machine));
    assert_eq!(
        *lock(&steps),
        vec![
            (step("Push", true, false), vec![3]),
            (step("Stop", true, true), vec![]),
        ]
    );
}

#[cfg(feature = "tokio")]
mod task {
    use super::{journaled, lock, step, CounterStateMachine, Push, Recorder, Stop, Unknown};
    use sml::{sml_policies, Terminated, TokioScheduler};

    sml_policies!(Tokio { scheduler: TokioScheduler, step_hook: super::Journal });

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .unwrap_or_else(|error| panic!("tokio runtime: {}", error))
    }

    #[test]
    fn a_dedicated_task_runs_events_queued_from_many_tasks_in_order() {
        let runtime = runtime();
        let (policy, steps) = journaled(|journal| Tokio::default().with_step_hook(journal));
        let accepted = runtime.block_on(async move {
            let machine = CounterStateMachine::new_with_policy(Recorder::default(), policy);
            // Queued at call time: the order of these calls is the run order,
            // whichever task awaits first.
            let first = machine.process_event_async(Push(1));
            let second = machine.clone().process_event_async(Push(2));
            drop(machine.process_event_async(Push(3)));
            let rejected = machine.process_event_async(Unknown);
            let joined = tokio::spawn(second);
            let accepted = (
                first.await,
                joined.await.unwrap_or(false),
                rejected.await,
                machine.process_event_async(Stop).await,
                machine.process_event_async(Push(4)).await,
                Terminated::is_terminated(&machine),
            );
            accepted
        });
        assert_eq!(accepted, (true, true, false, true, false, true));
        let seen: Vec<_> = lock(&steps).iter().map(|(_, seen)| seen.clone()).collect();
        assert_eq!(seen, vec![vec![1], vec![2], vec![3], vec![], vec![]]);
        assert_eq!(
            lock(&steps).last().map(|(step, _)| *step),
            Some(step("Stop", true, true))
        );
    }

    #[test]
    fn an_explicit_runtime_handle_hosts_the_machine_task() {
        let runtime = runtime();
        let policy = Tokio::default().with_scheduler(TokioScheduler::new(runtime.handle().clone()));
        let machine = CounterStateMachine::new_with_policy(Recorder::default(), policy);
        let completion = machine.try_process_event(Push(5));
        assert_eq!(
            runtime.block_on(async move { completion.ok()?.await.then_some(()) }),
            Some(())
        );
    }
}
