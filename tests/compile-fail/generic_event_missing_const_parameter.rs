use sml::sml;

pub struct Message<T>(T);

sml! {
    MissingConst<T, const N: usize> {
        *Idle + event<Message<T>> = X,
    }
}

fn main() {}
