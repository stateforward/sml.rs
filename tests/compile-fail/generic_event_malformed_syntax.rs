use sml::sml;

pub struct Message<T>(T);

sml! {
    Malformed<T where T: Clone> {
        *Idle + event<Message<T>> = X,
    }
}

fn main() {}
