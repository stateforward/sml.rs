extern crate sml;

use sml::sml;

sml! {
    ProductionMachine {
        *Idle + event<Tick> = Ready,
    }
}

struct Context;

impl ProductionMachineStateMachineContext for Context {}

struct Tick;

fn main() {
    let mut machine = ProductionMachineStateMachine::new(Context);
    machine.set_current_states(ProductionMachineStates::Ready);
}
