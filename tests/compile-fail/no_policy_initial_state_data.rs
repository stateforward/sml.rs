#![deny(warnings)]
#![forbid(unsafe_code)]

use sml::sml;

#[derive(Default)]
pub struct InitialData(u32);
pub struct Advance;

sml! {
    TypedInitial {
        *state<InitialData> + event<Advance> = X,
    }
}

struct Context;

impl TypedInitialStateMachineContext for Context {}

fn main() {
    let _machine = <TypedInitialStateMachine<Context, sml::NoPolicy>>::new_with_state_data(
        Context,
        InitialData(1),
    );
}
