#![deny(warnings)]
#![forbid(unsafe_code)]

use sml::sml;

pub struct Tick;

sml! {
    SafeGeneratedMachine {
        *Idle + event<Tick> = X,
    }
}

struct Context;

impl SafeGeneratedMachineStateMachineContext for Context {}

fn main() {
    let mut machine = SafeGeneratedMachineStateMachine::new(Context);
    machine.process_event(Tick).unwrap();
}
