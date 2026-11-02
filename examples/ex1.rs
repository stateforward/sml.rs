//! Linear state machine
//!
//! A simple example of a state machine which will get stuck in the final state.
//! A picture depicting the state machine can be found in the README.
//!
//! `process_event` reports whether a transition handled the event: `Ok(true)`
//! moves the machine along, `Ok(false)` means the current state ignored it.

#![deny(missing_docs)]

use sml::sml;

sml! {
    _ {
        *State1 + Event1 = State2,
        State2 + Event2 = State3,
    }
}

/// Context
pub struct Context;

impl StateMachineContext for Context {}

fn main() {
    let mut sm = StateMachine::new(Context);

    // State1 -> State2
    let r = sm.process_event(Events::Event1);
    assert_eq!(r, Ok(true));

    // State2 -> State3
    let r = sm.process_event(Events::Event2);
    assert_eq!(r, Ok(true));

    // Now all events will not give any change of state: State3 handles none
    let r = sm.process_event(Events::Event1);
    assert_eq!(r, Ok(false));

    let r = sm.process_event(Events::Event2);
    assert_eq!(r, Ok(false));
}
