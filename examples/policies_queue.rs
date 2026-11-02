//! Substitute a bounded owner-provided queue factory.

use sml::{sml, sml_policies, QueuePolicy};

pub struct Defer;
pub struct Release;

sml! {
    QueueMachine {
        *Idle + event<Defer> / defer,
        Idle + event<Release> = Ready,
        Ready + event<Defer> = X,
    }
}

struct Context;
impl QueueMachineStateMachineContext for Context {}

#[derive(Default)]
struct SmallQueues;

impl QueuePolicy for SmallQueues {
    type Queue<E> = sml::utility::EventQueue<E, 2>;

    fn new_queue<E>(&self) -> Self::Queue<E> {
        sml::utility::EventQueue::new()
    }
}

sml_policies!(QueuePolicies {
    defer_queue: SmallQueues,
    process_queue: SmallQueues,
});

fn main() {
    let policy = QueuePolicies::new((), (), (), (), SmallQueues, SmallQueues);
    let mut machine = QueueMachineStateMachine::new_with_policy(Context, policy);
    machine.process_event(Defer).unwrap();
    machine.process_event(Release).unwrap();
    assert!(machine.is_terminated());
}
