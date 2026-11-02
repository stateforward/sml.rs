//! Context with members example
//!
//! An example of using the context structure with members for counting the number of transitions.

#![deny(missing_docs)]

use sml::{sml, sml_policies};

sml! {
    _ {
        *State1 + Event1 / count_transition1 = State2,
        State2 + Event2 / count_transition2 = State1,
    }
}

/// Context with member
pub struct Context {
    /// Number of transitions
    pub num_transitions: usize,
}

impl StateMachineContext for Context {
    fn count_transition1(&mut self) -> Result<(), ()> {
        self.num_transitions += 1;
        Ok(())
    }

    fn count_transition2(&mut self) -> Result<(), ()> {
        self.num_transitions += 1;
        Ok(())
    }
}

sml_policies!(InspectContext {
    testing: sml::TestingPolicy,
});

fn main() {
    // Reading the context back out of the machine is a testing-only
    // capability, so this example opts into `sml::TestingPolicy`.
    let mut sm =
        StateMachine::new_with_policy(Context { num_transitions: 0 }, InspectContext::default());

    assert_eq!(sm.process_event(Events::Event1), Ok(true)); // ++
    assert_eq!(sm.process_event(Events::Event1), Ok(false)); // Not handled in State2
    assert_eq!(sm.process_event(Events::Event2), Ok(true)); // ++

    assert_eq!(sm.context().num_transitions, 2);

    // ...
}
