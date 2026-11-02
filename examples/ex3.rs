//! Looping state machine
//!
//! An example of using guards and actions.
//! A picture depicting the state machine can be found in the README.
//!
//! The actions record that they ran as outputs; a step hook prints and
//! collects them after each event.

#![deny(missing_docs)]

use std::cell::RefCell;
use std::rc::Rc;

use sml::{sml, sml_policies};

sml! {
    _ {
        *State1 + Event1 [guard] / action1 = State2,
        State2 + Event2 [guard_fail] / action2 = State3,
    }
}

/// What the actions emit.
#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    /// `action1` ran.
    Action1,
    /// `action2` ran.
    Action2,
}

/// Context
#[derive(Default)]
pub struct Context {
    outputs: Vec<Output>,
}

impl StateMachineContext for Context {
    fn guard(&self) -> Result<bool, ()> {
        // Always ok
        Ok(true)
    }

    fn guard_fail(&self) -> Result<bool, ()> {
        // Always fail
        Ok(false)
    }

    fn action1(&mut self) -> Result<(), ()> {
        self.outputs.push(Output::Action1);
        Ok(())
    }

    fn action2(&mut self) -> Result<(), ()> {
        self.outputs.push(Output::Action2);
        Ok(())
    }
}

impl sml::Emit<Output> for Context {
    fn drain_outputs(&mut self, sink: impl FnMut(Output)) {
        self.outputs.drain(..).for_each(sink);
    }
}

/// Prints and collects every output the machine emits.
#[derive(Default)]
pub struct Collect(Rc<RefCell<Vec<Output>>>);

impl<L> sml::StepHook<L, Events> for Collect {
    type Output = Output;

    fn output(&mut self, _logger: &mut L, output: Output) {
        println!("{output:?} ran");
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

    // Go through the first guard and action
    let r = sm.process_event(Events::Event1);
    assert_eq!(r, Ok(true));
    assert_eq!(seen.take(), [Output::Action1]);

    // The action will never run as the guard will fail
    let r = sm.process_event(Events::Event2);
    assert_eq!(r, Ok(false));
    assert!(seen.take().is_empty());

    // Now we are stuck due to the guard never returning true
    let r = sm.process_event(Events::Event2);
    assert_eq!(r, Ok(false));
    assert!(seen.take().is_empty());
}
