//! An example of using state data to propagate events (See issue-17)

#![deny(missing_docs)]

use sml::{sml, sml_policies};

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
    exited_d0: i32,
    entered_d1: i32,
}

impl OnEntryExampleStateMachineContext for Context {
    fn on_exit_d0(&mut self) {
        self.exited_d0 += 1;
    }
    fn on_entry_d1(&mut self) {
        self.entered_d1 += 1;
    }
}

// The entry/exit counters live in the context; reading them back is a
// testing-only capability, so opt into `sml::TestingPolicy`.
sml_policies!(InspectContext {
    testing: sml::TestingPolicy,
});

fn main() {
    let mut sm = OnEntryExampleStateMachine::new_with_policy(
        Context {
            exited_d0: 0,
            entered_d1: 0,
        },
        InspectContext::default(),
    );

    // first event starts the dominos
    assert!(sm.process_event(OnEntryExampleEvents::ToD1).unwrap());

    assert!(sm.is(&OnEntryExampleStates::D1));
    assert_eq!(sm.context().exited_d0, 1);
    assert_eq!(sm.context().entered_d1, 1);

    assert!(sm.process_event(OnEntryExampleEvents::ToD2).unwrap());

    assert!(sm.is(&OnEntryExampleStates::D2));
    assert_eq!(sm.context().exited_d0, 1);
    assert_eq!(sm.context().entered_d1, 1);

    assert!(sm.process_event(OnEntryExampleEvents::ToD1).unwrap());

    assert!(sm.is(&OnEntryExampleStates::D1));
    assert_eq!(sm.context().exited_d0, 1);
    assert_eq!(sm.context().entered_d1, 2);

    assert!(sm.process_event(OnEntryExampleEvents::ToD0).unwrap());

    assert!(sm.is(&OnEntryExampleStates::D0));
    assert_eq!(sm.context().exited_d0, 1);
    assert_eq!(sm.context().entered_d1, 2);

    assert!(sm.process_event(OnEntryExampleEvents::ToD3).unwrap());

    assert!(sm.is(&OnEntryExampleStates::D3));
    assert_eq!(sm.context().exited_d0, 2);
    assert_eq!(sm.context().entered_d1, 2);
}
