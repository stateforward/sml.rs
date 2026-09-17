//! Opt into state setup helpers for focused owner-side tests.

use sml::{sml, sml_policies};

pub struct Finish;

sml! {
    TestingMachine {
        *Idle + event<Finish> = X,
    }
}

struct Context;
impl TestingMachineStateMachineContext for Context {}

sml_policies!(TestingPolicies {
    testing: sml::TestingPolicy,
});

fn main() {
    let mut machine: TestingMachineStateMachine<Context, TestingPolicies> =
        TestingMachineStateMachine::new_with_policy(Context, TestingPolicies::default());
    machine.set_current_states(TestingMachineStates::Idle);
    machine.process_event(Finish).unwrap();
}
