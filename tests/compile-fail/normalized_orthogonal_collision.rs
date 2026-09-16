extern crate sml;

use sml::sml;

sml! {
    Orthogonal {
        *"foo-bar"_s + Start = Ready,
        *FooBar + Stop = Done,
    }
}

fn main() {}
