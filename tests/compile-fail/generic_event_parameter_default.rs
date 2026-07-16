use sml::sml;

pub struct Message<T>(T);

sml! {
    Defaulted<T = u32> {
        *Idle + event<Message<T>> = X,
    }
}

fn main() {}
