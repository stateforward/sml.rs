//! State data example
//!
//! An example of using state data together with an action.

#![deny(missing_docs)]

use sml::sml;

/// State data
#[derive(PartialEq, Eq)]
pub struct MyStateData(pub u32);

sml! {
    _ {
        State2 + Event2 / action = State1,
        *State1(MyStateData) + Event1 = State2,
        // ...
    }
}

/// Context
pub struct Context;

impl StateMachineContext for Context {
    fn action(&mut self) -> Result<MyStateData, ()> {
        Ok(MyStateData(42))
    }
}

fn main() {
    let mut sm = StateMachine::new(Context, MyStateData(42));
    assert_eq!(sm.process_event(Events::Event1), Ok(true));
    let result = sm.process_event(Events::Event2);

    assert_eq!(result, Ok(true));
    assert!(sm.visit_current_state(|state| matches!(state, States::State1(MyStateData(42)))));
}
