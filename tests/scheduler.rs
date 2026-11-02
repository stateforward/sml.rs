//! Scheduler policies: shared machines whose events are queued at call time
//! and run to completion one at a time.
#![cfg(feature = "std")]

use sml::{sml, sml_policies, Emit, FifoScheduler, InlineScheduler, Step, StepHook};
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

/// Journals each step with the outputs it emitted.
#[derive(Default)]
pub struct Journal {
    steps: Journaled,
    emitted: Vec<u32>,
}

impl Emit<u32> for Recorder {
    fn drain_outputs(&mut self, sink: impl FnMut(u32)) {
        self.seen.drain(..).for_each(sink);
    }
}

impl<L> StepHook<L, CounterEvents> for Journal {
    type Output = u32;

    fn output(&mut self, _logger: &mut L, output: u32) {
        self.emitted.push(output);
    }

    fn after_step(&mut self, _logger: &mut L, step: Step) {
        let emitted = core::mem::take(&mut self.emitted);
        lock(&self.steps).push((step, emitted));
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
    assert_eq!(ready(machine.process_event_async(Push(5))), Some(true));
    assert_eq!(ready(machine.process_event_async(Stop)), Some(true));
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
    use sml::{sml, sml_policies, TokioScheduler, TokioScope};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    sml_policies!(Tokio {
        scheduler: TokioScheduler,
        step_hook: super::Journal
    });

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_time()
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
            (
                first.await,
                joined.await.unwrap_or(false),
                rejected.await,
                machine.process_event_async(Stop).await,
                machine.process_event_async(Push(4)).await,
            )
        });
        assert_eq!(accepted, (true, true, false, true, false));
        let emitted: Vec<_> = lock(&steps).iter().map(|(_, seen)| seen.clone()).collect();
        assert_eq!(emitted, vec![vec![1], vec![2], vec![3], vec![], vec![]]);
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

    pub struct Work;
    pub struct Halt;

    sml! {
        Worker {
            *Idle + event<Work> / work = Idle,
            Idle + event<Halt> = X,
        }
    }

    /// Counts how many workers run a step at once.
    #[derive(Default)]
    pub struct Gauge {
        running: AtomicUsize,
        most: AtomicUsize,
    }

    /// A worker whose step takes a while, or until the test opens `gate`.
    pub struct Busy(Arc<Gauge>, Option<std::sync::mpsc::Receiver<()>>);

    impl WorkerStateMachineContext for Busy {
        fn work(&mut self, _event: &Work) -> Result<(), ()> {
            let running = self.0.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.0.most.fetch_max(running, Ordering::SeqCst);
            match &self.1 {
                Some(gate) => gate.recv().ok().unwrap_or_default(),
                None => std::thread::sleep(Duration::from_millis(5)),
            }
            self.0.running.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    }

    sml_policies!(Scoped {
        scheduler: TokioScheduler
    });

    fn worker(scheduler: TokioScheduler, gauge: &Arc<Gauge>) -> WorkerStateMachine<Busy, Scoped> {
        WorkerStateMachine::new_with_policy(
            Busy(Arc::clone(gauge), None),
            Scoped::default().with_scheduler(scheduler),
        )
    }

    #[test]
    fn a_scope_caps_how_many_of_its_machines_step_at_once() {
        let runtime = runtime();
        let gauge = Arc::new(Gauge::default());
        let all = runtime.block_on(async {
            let scope = TokioScope::new(1);
            let machines: Vec<_> = (0..3).map(|_| worker(scope.scheduler(), &gauge)).collect();
            let completions: Vec<_> = machines
                .iter()
                .flat_map(|machine| (0..3).map(move |_| machine.process_event_async(Work)))
                .collect();
            let mut all = true;
            for completion in completions {
                all &= completion.await;
            }
            all
        });
        assert!(all);
        assert_eq!(gauge.most.load(Ordering::SeqCst), 1);
    }

    async fn ended(machine: &WorkerStateMachine<Busy, Scoped>) -> bool {
        // Ending reaches each nested scope through its owner's task.
        for _ in 0..200 {
            if !machine.process_event_async(Work).await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        false
    }

    #[test]
    fn an_owner_reaching_x_ends_its_scope_and_every_nested_scope() {
        let runtime = runtime();
        let gauge = Arc::new(Gauge::default());
        let outcome = runtime.block_on(async {
            let hosted = TokioScope::new(4);
            let project_scope = TokioScope::new(2);
            let host = worker(TokioScheduler::default().owning(hosted.clone()), &gauge);
            let project = worker(hosted.scheduler().owning(project_scope.clone()), &gauge);
            let child = worker(project_scope.scheduler(), &gauge);
            let before = (
                project.process_event_async(Work).await,
                child.process_event_async(Work).await,
            );
            let halted = host.process_event_async(Halt).await;
            let after = (
                host.process_event_async(Work).await,
                ended(&project).await,
                ended(&child).await,
            );
            // A machine placed in an ended scope never starts.
            let late = worker(project_scope.scheduler(), &gauge);
            (before, halted, after, late.process_event_async(Work).await)
        });
        assert_eq!(outcome, ((true, true), true, (false, true, true), false));
    }

    #[test]
    fn ending_a_scope_skips_machines_whose_handles_are_gone() {
        let runtime = runtime();
        let gauge = Arc::new(Gauge::default());
        let accepted = runtime.block_on(async {
            let scope = TokioScope::new(2);
            drop(worker(scope.scheduler(), &gauge));
            let kept = worker(scope.scheduler(), &gauge);
            let first = kept.process_event_async(Work).await;
            scope.end();
            scope.end();
            (first, ended(&kept).await)
        });
        assert_eq!(accepted, (true, true));
        assert!(format!("{:?}", TokioScope::new(0)).starts_with("TokioScope"));
    }

    #[test]
    fn ending_a_scope_stops_machines_that_have_not_started_or_wait_for_a_slot() {
        let gauge = Arc::new(Gauge::default());
        // On a current-thread runtime a spawned machine has not started until
        // the test yields, so ending the scope first means it never starts.
        let current = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap_or_else(|error| panic!("tokio runtime: {}", error));
        let unstarted = current.block_on(async {
            let scope = TokioScope::new(1);
            let owned = TokioScope::new(1);
            let machine = worker(scope.scheduler().owning(owned.clone()), &gauge);
            let queued = machine.process_event_async(Work);
            scope.end();
            let late = worker(owned.scheduler(), &gauge);
            (queued.await, late.process_event_async(Work).await)
        });
        assert_eq!(unstarted, (false, false));

        // With one slot held by a step the test controls, the second
        // machine's step waits for a slot; ending the scope stops it instead.
        let waiting = runtime().block_on(async {
            let scope = TokioScope::new(1);
            let held = Arc::new(Gauge::default());
            let (open, gate) = std::sync::mpsc::channel();
            let busy = WorkerStateMachine::new_with_policy(
                Busy(Arc::clone(&held), Some(gate)),
                Scoped::default().with_scheduler(scope.scheduler()),
            );
            let blocked = worker(scope.scheduler(), &gauge);
            let running = busy.process_event_async(Work);
            while held.running.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            let parked = blocked.process_event_async(Work);
            scope.end();
            open.send(()).ok();
            (running.await, parked.await)
        });
        assert_eq!(waiting, (true, false));
    }
}
