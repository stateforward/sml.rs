use sml::sml;

pub struct Message<T>(T);

sml! {
    Deferred<T> {
        *Idle + event<Message<T>> / defer = X,
    }
}

fn main() {}
