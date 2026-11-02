//! Scheduler policies: shared machines whose events are queued at call time
//! and run to completion one at a time.
#![cfg(feature = "std")]

use sml::{sml, sml_policies, FifoScheduler, InlineScheduler};
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
type Waiting = std::pin::Pin<Box<dyn Future<Output = bool> + Send>>;
type Seen = Arc<Mutex<Vec<u32>>>;

/// What a re-entrant action did, shared with the test.
#[derive(Default)]
pub struct Reentry {
    this: OnceLock<Handle>,
    tried: Mutex<Vec<bool>>,
    waiting: Mutex<Option<Waiting>>,
}

/// The context owns only shared handles, so the test observes what the
/// actions did without borrowing the machine.
#[derive(Default)]
pub struct Recorder {
    seen: Seen,
    reentry: Arc<Reentry>,
}

impl CounterStateMachineContext for Recorder {
    fn never(&self, _event: &Unknown) -> Result<bool, ()> {
        Ok(false)
    }

    fn record(&mut self, event: &Push) -> Result<(), ()> {
        lock(&self.seen).push(event.0);
        if event.0 == 1 {
            if let Some(this) = self.reentry.this.get() {
                // Re-entering a busy FIFO machine only queues the event; the
                // draining caller runs each after this one completes.
                let tried = (10..15)
                    .map(|value| this.try_process_event(Push(value)).is_ok())
                    .collect();
                *lock(&self.reentry.tried) = tried;
                *lock(&self.reentry.waiting) = Some(Box::pin(this.process_event_async(Push(20))));
            }
        }
        Ok(())
    }
}

sml_policies!(Fifo { scheduler: FifoScheduler<4> });
sml_policies!(Inline {
    scheduler: InlineScheduler
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

fn recorder() -> (Recorder, Seen) {
    let recorder = Recorder::default();
    let seen = Arc::clone(&recorder.seen);
    (recorder, seen)
}

#[test]
fn fifo_runs_an_idle_machines_event_before_returning() {
    let (context, seen) = recorder();
    let machine = CounterStateMachine::new_with_policy(context, Fifo::default());
    let handle = machine.clone();

    let completion = handle.process_event_async(Push(7));
    // Dropping a completion never cancels its event.
    drop(machine.process_event_async(Again));
    assert_eq!(ready(completion), Some(true));
    assert_eq!(ready(handle.process_event_async(Unknown)), Some(false));
    assert_eq!(*lock(&seen), vec![7, 99]);
    assert!(machine.is(&CounterStates::Idle));
    let mut visited = Vec::new();
    handle.visit_current_states(|state| visited.push(sml::StateName::name(state)));
    assert_eq!(visited, vec!["Idle"]);
}

#[test]
fn fifo_queues_reentrant_events_bounded_and_waits_for_room() {
    let (context, seen) = recorder();
    let reentry = Arc::clone(&context.reentry);
    let machine = CounterStateMachine::new_with_policy(context, Fifo::default());
    assert!(reentry.this.set(machine.clone()).is_ok());

    assert_eq!(ready(machine.process_event_async(Push(1))), Some(true));
    // Four events fit the queue; the fifth is refused without being queued.
    assert_eq!(*lock(&reentry.tried), vec![true, true, true, true, false]);
    assert_eq!(*lock(&seen), vec![1, 10, 11, 12, 13]);

    // The async entry point waited for room instead; the machine is idle now,
    // so polling it runs the event.
    let waiting = lock(&reentry.waiting).take();
    assert_eq!(waiting.and_then(ready), Some(true));
    assert_eq!(lock(&seen).last(), Some(&20));
}

#[test]
fn fifo_resolves_pending_events_false_once_terminated() {
    let (context, seen) = recorder();
    let machine = CounterStateMachine::new_with_policy(context, Fifo::default());
    assert_eq!(ready(machine.process_event_async(Push(5))), Some(true));
    assert_eq!(ready(machine.process_event_async(Stop)), Some(true));
    assert!(machine.is(&CounterStates::X));
    assert_eq!(ready(machine.process_event_async(Push(1))), Some(false));
    assert_eq!(
        machine.try_process_event(Push(1)).ok().and_then(ready),
        Some(false)
    );
    assert_eq!(*lock(&seen), vec![5]);
}

#[test]
fn inline_runs_each_event_under_the_shared_lock() {
    let (context, seen) = recorder();
    let machine = CounterStateMachine::new_with_policy(context, Inline::default());
    let handle = machine.clone();
    let worker = std::thread::spawn(move || ready(handle.process_event_async(Push(3))));
    assert_eq!(worker.join().ok().flatten(), Some(true));
    assert_eq!(ready(machine.process_event_async(Stop)), Some(true));
    assert_eq!(
        machine.try_process_event(Push(4)).ok().and_then(ready),
        Some(false)
    );
    assert_eq!(*lock(&seen), vec![3]);
    assert!(machine.is(&CounterStates::X));
}

sml! {
    Greeter {
        *Idle + on_entry<_> / greet,
        Idle + event<Push> / record = Idle,
    }
}

/// Records `0` when it enters `Idle` and each pushed value afterwards.
#[derive(Default)]
pub struct Greetings(Seen);

impl GreeterStateMachineContext for Greetings {
    fn greet(&mut self) -> Result<(), ()> {
        lock(&self.0).push(0);
        Ok(())
    }

    fn record(&mut self, event: &Push) -> Result<(), ()> {
        lock(&self.0).push(event.0);
        Ok(())
    }
}

#[test]
fn a_queued_machine_initializes_when_constructed() {
    let seen = Seen::default();
    let machine =
        GreeterStateMachine::new_with_policy(Greetings(Arc::clone(&seen)), Fifo::default());
    assert_eq!(*lock(&seen), vec![0]);
    assert_eq!(ready(machine.process_event_async(Push(6))), Some(true));
    // The self-transition re-enters `Idle`, so `greet` runs again.
    assert_eq!(*lock(&seen), vec![0, 6, 0]);
}

/// A machine defined in a module and handed out by value, as an owner would.
mod defined_elsewhere {
    use sml::{sml, sml_policies, FifoScheduler};

    pub struct Go;

    sml! {
        Remote {
            *Idle + event<Go> / went = Idle,
        }
    }

    pub struct Quiet;

    impl RemoteStateMachineContext for Quiet {
        fn went(&mut self, _event: &Go) -> Result<(), ()> {
            Ok(())
        }
    }

    sml_policies!(Queued { scheduler: FifoScheduler<4> });

    pub fn make() -> RemoteStateMachine<Quiet, Queued> {
        RemoteStateMachine::new_with_policy(Quiet, Queued::default())
    }
}

const fn is_send<T: Send>(_: &T) {}

#[test]
fn a_scheduled_machine_is_usable_outside_its_defining_module() {
    let machine = defined_elsewhere::make();
    assert_eq!(
        ready(machine.process_event_async(defined_elsewhere::Go)),
        Some(true)
    );
    let queued = machine.try_process_event(defined_elsewhere::Go);
    assert_eq!(queued.ok().and_then(ready), Some(true));
    is_send(&machine.process_event_async(defined_elsewhere::Go));
    assert!(machine.is(&defined_elsewhere::RemoteStates::Idle));
}

#[test]
fn the_owned_machine_is_unchanged_by_the_default_scheduler() {
    let (context, seen) = recorder();
    let mut machine = CounterStateMachine::new(context);
    assert!(machine.initialize().is_ok());
    assert!(machine.process_event(Push(2)).is_ok());
    assert!(machine.is(&CounterStates::Idle));
    assert_eq!(*lock(&seen), vec![2]);
}
