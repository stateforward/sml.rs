//! Reuse the same action more than once
//!
//! This example shows how to use the same action in multiple transitions.
//! The action reports each run as an output; a step hook collects the outputs
//! after each event.

#![deny(missing_docs)]

use std::cell::RefCell;
use std::rc::Rc;

use sml::{sml, sml_policies};

sml! {
    _ {
        *State1 + Event1 / action = State2,
        State1 + Event2 / action = State3,
        State2 + Event2 = State1,
    }
}

/// What the action emits.
#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    /// `action` ran for the n-th time.
    Action(usize),
}

/// Action will increment our context
#[derive(Default)]
pub struct Context {
    runs: usize,
    outputs: Vec<Output>,
}

impl StateMachineContext for Context {
    fn action(&mut self) -> Result<(), ()> {
        self.runs += 1;
        self.outputs.push(Output::Action(self.runs));
        Ok(())
    }
}

impl sml::Emit<Output> for Context {
    fn drain_outputs(&mut self, sink: impl FnMut(Output)) {
        self.outputs.drain(..).for_each(sink);
    }
}

/// Collects every output the machine emits.
#[derive(Default)]
pub struct Collect(Rc<RefCell<Vec<Output>>>);

impl<L> sml::StepHook<L, Events> for Collect {
    type Output = Output;

    fn output(&mut self, _logger: &mut L, output: Output) {
        self.0.borrow_mut().push(output);
    }
}

sml_policies!(Observed { step_hook: Collect });

fn main() {
    let collect = Collect::default();
    let seen = Rc::clone(&collect.0);
    let mut sm = StateMachine::new_with_policy(
        Context::default(),
        Observed::default().with_step_hook(collect),
    );

    // State1 -> State2 triggers action
    let r = sm.process_event(Events::Event1);
    assert_eq!(r, Ok(true));
    assert_eq!(seen.take(), [Output::Action(1)]);

    // State2 -> State1 runs no action
    let r = sm.process_event(Events::Event2);
    assert_eq!(r, Ok(true));
    assert!(seen.take().is_empty());

    // State1 -> State3 triggers the same action
    let r = sm.process_event(Events::Event2);
    assert_eq!(r, Ok(true));
    assert_eq!(seen.take(), [Output::Action(2)]);

    // State3 handles nothing
    let r = sm.process_event(Events::Event1);
    assert_eq!(r, Ok(false));
    assert!(seen.take().is_empty());
}
