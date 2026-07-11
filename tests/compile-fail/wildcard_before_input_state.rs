extern crate sml;

use sml::sml;

sml! {
        _ {
        _ + Event1 = Fault, //~ State and event combination specified multiple times, remove duplicates.
        *State1 + Event1 = State2,
    }
}

fn main() {}
