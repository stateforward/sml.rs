extern crate sml;

use sml::sml;

sml! {
    _ {
        *Idle + "foo-bar"_e = Ready,
        Ready + FooBar = Done,
    }
}

fn main() {}
