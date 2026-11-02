//! Context with members example
//!
//! An example of using the context structure with members for counting the number of transitions.
//!
//! As in `sml.cpp`, the caller owns the dependencies: the machine holds a
//! shared reference to the context, and the count lives in a `Cell` so the
//! caller can read it while the machine is alive.

#![deny(missing_docs)]

use sml::sml;

sml! {
    _ {
        *State1 + Event1 / count_transition1 = State2,
        State2 + Event2 / count_transition2 = State1,
    }
}

/// Context with member
pub struct Context {
    /// Number of transitions
    pub num_transitions: core::cell::Cell<usize>,
}

impl StateMachineContext for &Context {
    fn count_transition1(&mut self) -> Result<(), ()> {
        self.num_transitions.set(self.num_transitions.get() + 1);
        Ok(())
    }

    fn count_transition2(&mut self) -> Result<(), ()> {
        self.num_transitions.set(self.num_transitions.get() + 1);
        Ok(())
    }
}

fn main() {
    let context = Context {
        num_transitions: core::cell::Cell::new(0),
    };
    let mut sm = StateMachine::new(&context);

    sm.process_event(Events::Event1).ok(); // ++
    sm.process_event(Events::Event1).ok(); // Will fail
    sm.process_event(Events::Event2).ok(); // ++

    assert_eq!(context.num_transitions.get(), 2);

    // ...
}
