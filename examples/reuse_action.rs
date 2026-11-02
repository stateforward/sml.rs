//! Reuse the same aciton more than once
//!
//! This example shows how to use the same action in multiple transitions.

#![deny(missing_docs)]

use sml::sml;

sml! {
    _ {
        *State1 + Event1 / action = State2,
        State1 + Event2 / action = State3,
        State2 + Event2 = State1,
    }
}

/// Action will increment our context. The machine holds a shared
/// reference, so the caller can read the count between events.
pub struct Context(core::cell::Cell<usize>);

impl StateMachineContext for &Context {
    fn action(&mut self) -> Result<(), ()> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
}

fn main() {
    let context = Context(core::cell::Cell::new(0));
    let mut sm = StateMachine::new(&context);
    assert!(sm.is(&States::State1));
    assert_eq!(context.0.get(), 0);

    // triggers action
    let r = sm.process_event(Events::Event1);
    assert!(matches!(r, Ok(&States::State2)));
    assert_eq!(context.0.get(), 1);

    let r = sm.process_event(Events::Event2);
    assert!(matches!(r, Ok(&States::State1)));
    assert_eq!(context.0.get(), 1);

    // triggers the same action
    let r = sm.process_event(Events::Event2);
    assert!(matches!(r, Ok(&States::State3)));
    assert_eq!(context.0.get(), 2);
}
