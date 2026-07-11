//! Guard and action syntax example
//!
//! An example of using guards and actions with state and event data.

#![deny(missing_docs)]

use sml::sml;

/// Custom guard errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardError {
    /// This is a custom guard error variant
    Custom,
}

/// Event data
#[derive(PartialEq)]
pub struct MyEventData(pub u32);

/// State data
#[derive(PartialEq)]
pub struct MyStateData(pub u32);

sml! {
    _[custom_error] {
        *State1 + Event1(MyEventData) [guard1] / action1 = State2,
        State2(MyStateData) + Event2  [guard2] / action2 = State3,
        // ...
    }
}

/// Context
pub struct Context;

impl StateMachineContext for Context {
    type Error = GuardError; // Guard1 has access to the data from Event1
    fn guard1(&self, _event_data: &MyEventData) -> Result<bool, GuardError> {
        Err(GuardError::Custom)
    }

    // Action1 has access to the data from Event1, and need to return the state data for State2
    fn action1(&mut self, event_data: MyEventData) -> Result<MyStateData, Self::Error> {
        Ok(MyStateData(event_data.0))
    }

    // Guard2 has access to the data from State2
    fn guard2(&self, state_data: &MyStateData) -> Result<bool, GuardError> {
        Ok(state_data.0 > 0)
    }

    // Action2 has access to the data from State2
    fn action2(&mut self, _state_data: &MyStateData) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn main() {
    let mut sm = StateMachine::new(Context {});

    let r = sm.process_event(Events::Event1(MyEventData(1)));

    assert!(matches!(r, Err(Error::GuardFailed(GuardError::Custom))));
}
