//! Context with members example
//!
//! An example of using the context structure with members for counting the
//! number of transitions. The context reports each new count as an output; a
//! step hook collects the outputs after each event.

#![deny(missing_docs)]

use std::cell::RefCell;
use std::rc::Rc;

use sml::{sml, sml_policies};

sml! {
    _ {
        *State1 + Event1 / count_transition1 = State2,
        State2 + Event2 / count_transition2 = State1,
    }
}

/// What the actions emit.
#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    /// The transition counter now holds this value.
    Transitions(usize),
}

/// Context with member
#[derive(Default)]
pub struct Context {
    /// Number of transitions
    num_transitions: usize,
    outputs: Vec<Output>,
}

impl Context {
    fn count(&mut self) {
        self.num_transitions += 1;
        self.outputs.push(Output::Transitions(self.num_transitions));
    }
}

impl StateMachineContext for Context {
    fn count_transition1(&mut self) -> Result<(), ()> {
        self.count();
        Ok(())
    }

    fn count_transition2(&mut self) -> Result<(), ()> {
        self.count();
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

    assert_eq!(sm.process_event(Events::Event1), Ok(true)); // ++
    assert_eq!(seen.take(), [Output::Transitions(1)]);

    assert_eq!(sm.process_event(Events::Event1), Ok(false)); // Not handled in State2
    assert!(seen.take().is_empty());

    assert_eq!(sm.process_event(Events::Event2), Ok(true)); // ++
    assert_eq!(seen.take(), [Output::Transitions(2)]);

    // ...
}
