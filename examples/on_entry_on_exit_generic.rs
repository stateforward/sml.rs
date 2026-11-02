//! An example of using state data to propagate events (See issue-17)

#![deny(missing_docs)]

use sml::sml;

sml! {
    OnEntryExample {
        *D0 + ToD1 = D1,
        D0 + ToD3 = D3,
        D1  + ToD2 = D2,
        D2 + ToD1 = D1,
        D1 + ToD0 = D0,
    },
}

/// Context
pub struct Context {
    exited_d0: core::cell::Cell<i32>,
    entered_d1: core::cell::Cell<i32>,
}

impl OnEntryExampleStateMachineContext for &Context {
    fn on_exit_d0(&mut self) {
        self.exited_d0.set(self.exited_d0.get() + 1);
    }
    fn on_entry_d1(&mut self) {
        self.entered_d1.set(self.entered_d1.get() + 1);
    }
}

fn main() {
    let context = Context {
        exited_d0: core::cell::Cell::new(0),
        entered_d1: core::cell::Cell::new(0),
    };
    let mut sm = OnEntryExampleStateMachine::new(&context);

    // first event starts the dominos
    let _ = sm.process_event(OnEntryExampleEvents::ToD1).unwrap();

    assert!(sm.is(&OnEntryExampleStates::D1));
    assert_eq!(context.exited_d0.get(), 1);
    assert_eq!(context.entered_d1.get(), 1);

    let _ = sm.process_event(OnEntryExampleEvents::ToD2).unwrap();

    assert!(sm.is(&OnEntryExampleStates::D2));
    assert_eq!(context.exited_d0.get(), 1);
    assert_eq!(context.entered_d1.get(), 1);

    let _ = sm.process_event(OnEntryExampleEvents::ToD1).unwrap();

    assert!(sm.is(&OnEntryExampleStates::D1));
    assert_eq!(context.exited_d0.get(), 1);
    assert_eq!(context.entered_d1.get(), 2);

    let _ = sm.process_event(OnEntryExampleEvents::ToD0).unwrap();

    assert!(sm.is(&OnEntryExampleStates::D0));
    assert_eq!(context.exited_d0.get(), 1);
    assert_eq!(context.entered_d1.get(), 2);

    let _ = sm.process_event(OnEntryExampleEvents::ToD3).unwrap();

    assert!(sm.is(&OnEntryExampleStates::D3));
    assert_eq!(context.exited_d0.get(), 2);
    assert_eq!(context.entered_d1.get(), 2);
}
