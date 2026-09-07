//! Visit the event payload types that have transitions from the active state.

use sml::{sml, Event, EventVisitor};

pub struct Begin;
pub struct ChildWork;
pub struct ParentReset;

sml! {
    EventsChild {
        *Idle + event<ChildWork> = X,
    }

    EventsParent {
        *Outside + event<Begin> = state<EventsChild>,
        state<EventsChild> + event<ParentReset> = Outside,
    }
}

struct Context;

impl EventsParentStateMachineContext for Context {}

struct PrintEvents;

impl EventVisitor for PrintEvents {
    fn visit<E: Event>(&mut self) {
        print!("{} ", E::name());
    }
}

fn print_current_events(machine: &EventsParentStateMachine<Context>) {
    let state_name = machine.visit_current_state(|state| match state {
        EventsParentStates::Outside => "Outside",
        EventsParentStates::EventsChild => "EventsChild",
    });
    print!("{state_name}: ");
    machine.visit_current_events(&mut PrintEvents);
    println!();
}

fn main() {
    let mut machine = EventsParentStateMachine::new(Context);
    print_current_events(&machine);

    machine.process_event(Begin).unwrap();
    print_current_events(&machine);
}
