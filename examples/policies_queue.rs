//! Substitute a bounded owner-provided queue factory.
//!
//! A step hook records each step's summary, including whether the machine
//! reached its terminal state.

use std::cell::RefCell;
use std::rc::Rc;

use sml::{sml, sml_policies, QueuePolicy, Step};

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

#[derive(Default)]
struct Steps(Rc<RefCell<Vec<Step>>>);

impl<L> sml::StepHook<L, QueueMachineEvents> for Steps {
    type Output = ();

    fn after_step(&mut self, _logger: &mut L, step: Step) {
        self.0.borrow_mut().push(step);
    }
}

sml_policies!(QueuePolicies {
    defer_queue: SmallQueues,
    process_queue: SmallQueues,
    step_hook: Steps,
});

fn main() {
    let steps = Steps::default();
    let seen = Rc::clone(&steps.0);
    let policy: QueuePolicies =
        sml::PolicyBundle::new((), (), (), (), SmallQueues, SmallQueues).with_step_hook(steps);
    let mut machine = QueueMachineStateMachine::new_with_policy(Context, policy);
    machine.process_event(Defer).unwrap();
    machine.process_event(Release).unwrap();
    // `Release` moves to `Ready`, which replays the deferred `Defer` into `X`.
    let last = seen.borrow().last().copied();
    assert!(last.is_some_and(|step| step.terminated));
}
