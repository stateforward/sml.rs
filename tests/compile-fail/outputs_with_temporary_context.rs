extern crate sml;

use sml::sml;

pub struct Event;

sml! {
    Mixed[outputs: u8, temporary_context: &mut u8] {
        *Idle + event<Event> / act = Done,
    }
}

fn main() {}
