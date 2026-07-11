extern crate sml;

use sml::sml;

sml! {
        _ {
        *State1 + Event1 = State2,
        _ + Event1 = Fault,
        _ + Event1 = State3, //~ Wildcard has no effect
    }
}

fn main() {}
