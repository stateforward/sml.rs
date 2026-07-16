use sml::sml;

pub struct Message<T>(T);
pub struct Enter;

sml! {
    Child<T> {
        *Idle + event<Message<T>> = X,
    }

    Parent {
        *Outside + event<Enter> = state<Child>,
    }
}

fn main() {}
