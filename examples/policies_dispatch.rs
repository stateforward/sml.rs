//! Use an owner-supplied policy to reject an event and select a transition.

use sml::{sml, sml_policies, Dispatch};

pub struct Accept;
pub struct Reject;

sml! {
    DispatchMachine {
        *Idle + event<Accept> [first] = FirstPath,
        Idle + event<Accept> [second] = SecondPath,
        Idle + event<Reject> = X,
    }
}

struct Context;
impl DispatchMachineStateMachineContext for Context {
    fn first(&self, _: &Accept) -> Result<bool, ()> {
        Ok(true)
    }

    fn second(&self, _: &Accept) -> Result<bool, ()> {
        Ok(true)
    }
}

#[derive(Default)]
struct OwnerDispatch;

impl Dispatch for OwnerDispatch {
    fn dispatch(&self, event: &'static str) -> bool {
        event != "Reject"
    }

    fn dispatch_candidate(
        &self,
        _state: &'static str,
        event: &'static str,
        candidate: usize,
    ) -> bool {
        event != "Reject" && candidate == 1
    }
}

sml_policies!(DispatchPolicies {
    dispatch: OwnerDispatch,
});

fn main() {
    let policy = DispatchPolicies::with_dispatch(
        OwnerDispatch,
        (),
        (),
        (),
        (),
        sml::DefaultQueuePolicy,
        sml::DefaultQueuePolicy,
    );
    let mut machine = DispatchMachineStateMachine::new_with_policy(Context, policy);
    assert!(machine.process_event(Reject).is_err());
    machine.process_event(Accept).unwrap();
    assert!(machine.is(&DispatchMachineStates::SecondPath));
}
