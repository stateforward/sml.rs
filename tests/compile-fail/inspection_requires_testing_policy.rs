extern crate sml;

use sml::sml;

sml! {
    ProductionInspection {
        *Idle + event<Tick> = Ready,
    }
}

struct Context;

impl ProductionInspectionStateMachineContext for Context {}

struct Tick;

struct Names;

impl sml::EventVisitor for Names {
    fn visit<E: sml::Event>(&mut self) {}
}

// State and context inspection is testing-only. A machine built with the
// default policy (no `testing: sml::TestingPolicy`) exposes none of it.
fn main() {
    let mut machine = ProductionInspectionStateMachine::new(Context);
    let _ = machine.process_event(Tick);
    let _ = machine.is(&ProductionInspectionStates::Ready);
    let _ = machine.context();
    machine.visit_current_events(&mut Names);
}
