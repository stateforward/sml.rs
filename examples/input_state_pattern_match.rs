//! Pattern Matching State Machine
//!
//! This demonstrates the use of input state pattern matching so that states that share a common
//! transition to the same output state can be described more succinctly
#![deny(missing_docs)]

use sml::{sml, sml_policies};

sml_policies!(TestingPolicies {
    testing: sml::TestingPolicy,
});

// sml! {
//     transitions: {
//         *Idle + Charge = Charging,
//         Idle + Discharge = Discharging,
//         Charging + ChargeComplete = Charged,
//         Discharging + DischargeComplete = Discharged,
//         Charged + Discharge = Discharging,
//         Dischaged + Charge = Charging,
//         Charging + Discharge = Discharging,
//         Discharging + Charge = Charging,
//         Idle + FaultDetected = Fault,
//         Charging + FaultDetected = Fault,
//         Discharging + FaultDetected = Fault,
//         Charged + FaultDetected = Fault,
//         Discharged + FaultDetected = Fault,
//         Fault + FaultCleard = Idle,
//     },
// }

// A simple charge/discharge state machine that has a dedicated "Fault" state
sml! {
    _ {
        *Idle | Discharging | Discharged + Charge = Charging,
        Idle | Charging | Charged + Discharge = Discharging,
        Charging + ChargeComplete = Charged,
        Discharging + DischargeComplete = Discharged,
        _ + FaultDetected = Fault,
        Fault + FaultCleard = Idle,
    }
}

/// Context
pub struct Context;

impl StateMachineContext for Context {}

fn main() {
    let mut sm: StateMachine<Context, TestingPolicies> =
        StateMachine::new_with_policy(Context, TestingPolicies::default());

    assert!(matches!(sm.state(), &States::Idle));

    let r = sm.process_event(Events::Charge);
    assert!(matches!(r, Ok(&States::Charging)));

    let r = sm.process_event(Events::Discharge);
    assert!(matches!(r, Ok(&States::Discharging)));

    let r = sm.process_event(Events::Charge);
    assert!(matches!(r, Ok(&States::Charging)));

    let r = sm.process_event(Events::ChargeComplete);
    assert!(matches!(r, Ok(&States::Charged)));

    let r = sm.process_event(Events::Charge);
    assert!(matches!(r, Err(Error::InvalidEvent)));
    assert!(matches!(sm.state(), &States::Charged));

    let r = sm.process_event(Events::Discharge);
    assert!(matches!(r, Ok(&States::Discharging)));

    let r = sm.process_event(Events::DischargeComplete);
    assert!(matches!(r, Ok(&States::Discharged)));

    let r = sm.process_event(Events::Discharge);
    assert!(matches!(r, Err(Error::InvalidEvent)));
    assert!(matches!(sm.state(), &States::Discharged));

    sm.set_current_states(States::Idle);
    let r = sm.process_event(Events::FaultDetected);
    assert!(matches!(r, Ok(&States::Fault)));

    sm.set_current_states(States::Charging);
    let r = sm.process_event(Events::FaultDetected);
    assert!(matches!(r, Ok(&States::Fault)));

    sm.set_current_states(States::Charged);
    let r = sm.process_event(Events::FaultDetected);
    assert!(matches!(r, Ok(&States::Fault)));

    sm.set_current_states(States::Discharging);
    let r = sm.process_event(Events::FaultDetected);
    assert!(matches!(r, Ok(&States::Fault)));

    sm.set_current_states(States::Discharged);
    let r = sm.process_event(Events::FaultDetected);
    assert!(matches!(r, Ok(&States::Fault)));

    let r = sm.process_event(Events::Charge);
    assert!(matches!(r, Err(Error::InvalidEvent)));
    assert!(matches!(sm.state(), &States::Fault));

    let r = sm.process_event(Events::Discharge);
    assert!(matches!(r, Err(Error::InvalidEvent)));
    assert!(matches!(sm.state(), &States::Fault));

    let r = sm.process_event(Events::ChargeComplete);
    assert!(matches!(r, Err(Error::InvalidEvent)));
    assert!(matches!(sm.state(), &States::Fault));

    let r = sm.process_event(Events::DischargeComplete);
    assert!(matches!(r, Err(Error::InvalidEvent)));
    assert!(matches!(sm.state(), &States::Fault));
}
