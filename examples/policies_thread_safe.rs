//! Wrap event processing in an owner-supplied no-std lock policy.

use sml::{sml, sml_policies, RawMutex, ThreadSafe};

pub struct Tick;

sml! {
    ThreadSafeMachine {
        *Idle + event<Tick> = X,
    }
}

struct Context;
impl ThreadSafeMachineStateMachineContext for Context {}

struct Lock;
struct Guard;

impl RawMutex for Lock {
    type Guard<'a>
        = Guard
    where
        Self: 'a;

    fn lock(&self) -> Self::Guard<'_> {
        Guard
    }
}

sml_policies!(ThreadSafePolicies {
    thread_safe: ThreadSafe<Lock>,
});

fn main() {
    let policy = ThreadSafePolicies::with_dispatch(
        sml::JumpTable,
        (),
        (),
        ThreadSafe::new(Lock),
        (),
        sml::DefaultQueuePolicy,
        sml::DefaultQueuePolicy,
    );
    let mut machine = ThreadSafeMachineStateMachine::new_with_policy(Context, policy);
    machine.process_event(Tick).unwrap();
}
