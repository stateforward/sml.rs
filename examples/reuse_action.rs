//! Reuse the same aciton more than once
//!
//! This example shows how to use the same action in multiple transitions.

#![deny(missing_docs)]

use sml::{sml, sml_policies};

sml! {
    _ {
        *State1 + Event1 / action = State2,
        State1 + Event2 / action = State3,
        State2 + Event2 = State1,
    }
}

/// Action will increment our context
pub struct Context(usize);

impl StateMachineContext for Context {
    fn action(&mut self) -> Result<(), ()> {
        self.0 += 1;
        Ok(())
    }
}

// Reading the action counter back out of the context is testing-only, so
// opt into `sml::TestingPolicy`.
sml_policies!(InspectContext {
    testing: sml::TestingPolicy,
});

fn main() {
    let mut sm = StateMachine::new_with_policy(Context(0), InspectContext::default());
    assert!(sm.is(&States::State1));
    assert_eq!(sm.context().0, 0);

    // triggers action
    let r = sm.process_event(Events::Event1);
    assert_eq!(r, Ok(true));
    assert!(sm.is(&States::State2));
    assert_eq!(sm.context().0, 1);

    let r = sm.process_event(Events::Event2);
    assert_eq!(r, Ok(true));
    assert!(sm.is(&States::State1));
    assert_eq!(sm.context().0, 1);

    // triggers the same action
    let r = sm.process_event(Events::Event2);
    assert_eq!(r, Ok(true));
    assert!(sm.is(&States::State3));
    assert_eq!(sm.context().0, 2);
}
