extern crate sml;

use sml::sml;

#[derive(Clone)]
pub struct Payload;

sml! {
    _ {
        *Idle + Go(Payload) = Ready,
        Ready + completion<Go>(Payload) = X,
    }
}

struct Context;

impl StateMachineContext for Context {}

fn main() {
    let mut machine = StateMachine::new(Context);
    machine.process_event(Events::Go(Payload)).unwrap();
}
