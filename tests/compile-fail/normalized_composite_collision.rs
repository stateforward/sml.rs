extern crate sml;

use sml::sml;

sml! {
    Parent {
        *Idle + "foo-bar"_e = state<Child>,
        state<Child> + Stop = X,
    },
    Child {
        *Init + FooBar = X,
    },
}

fn main() {}
