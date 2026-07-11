extern crate sml;

use sml::sml;

sml! {
        _ {
        State1 + Event1 = Fault,
        *State1 + Event1 [guard] = State2,
    }
}

fn main() {}
