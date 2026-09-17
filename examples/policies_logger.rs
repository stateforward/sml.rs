//! Log generated machine callbacks through an owner-supplied policy.

use sml::{sml, sml_policies, Logger};

pub struct Start;

sml! {
    LoggerMachine {
        *Idle + event<Start> / finish = X,
    }
}

struct Context;

impl LoggerMachineStateMachineContext for Context {
    fn finish(&mut self, _: &Start) -> Result<(), ()> {
        Ok(())
    }
}

#[derive(Default)]
struct Trace;

impl Logger for Trace {
    fn log_process_event(&mut self, event: &'static str) {
        println!("process {event}");
    }

    fn log_action(&mut self, action: &'static str, event: &'static str) {
        println!("action {action} for {event}");
    }

    fn log_state_change(&mut self, from: &'static str, to: &'static str) {
        println!("state {from} -> {to}");
    }
}

sml_policies!(LoggerPolicies { logger: Trace });

fn main() {
    let policy = LoggerPolicies::new(
        Trace,
        (),
        (),
        (),
        sml::DefaultQueuePolicy,
        sml::DefaultQueuePolicy,
    );
    let mut machine = LoggerMachineStateMachine::new_with_policy(Context, policy);
    machine.process_event(Start).unwrap();
}
