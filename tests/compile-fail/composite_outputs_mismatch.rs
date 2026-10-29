extern crate sml;

use sml::sml;

pub struct Event;

sml! {
    Child[outputs: u16] {
        *Inner + event<Event> = X,
    }

    Parent[outputs: u8] {
        *Outer + event<Event> = state<Child>,
    }
}

fn main() {}
