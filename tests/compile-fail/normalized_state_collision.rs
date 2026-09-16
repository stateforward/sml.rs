extern crate sml;

use sml::sml;

sml! {
    _ {
        *"foo-bar"_s + Start = Ready,
        FooBar + Stop = Done,
    }
}

fn main() {}
