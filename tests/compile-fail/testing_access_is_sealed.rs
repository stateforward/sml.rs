extern crate sml;

use sml::{sml, PolicyBundle, Testing};

#[derive(Default)]
struct ProductionTestingMarker;

impl Testing for ProductionTestingMarker {}

// State override capability is intentionally sealed. A production policy must
// not be able to opt itself into this testing-only API.
impl sml::TestingAccess for ProductionTestingMarker {}

sml! {
    StrictStateOverride {
        *Idle + event<Tick> = Ready,
    }
}

struct Context;

impl StrictStateOverrideStateMachineContext for Context {}

struct Tick;

fn main() {
    let policy = PolicyBundle::<(), (), sml::JumpTable, (), ProductionTestingMarker>::default();
    let mut machine = StrictStateOverrideStateMachine::new_with_policy(Context, policy);
    machine.set_current_states(StrictStateOverrideStates::Ready);
}
