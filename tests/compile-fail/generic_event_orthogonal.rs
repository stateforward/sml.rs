use sml::sml;

pub struct Message<T>(T);

sml! {
    Orthogonal<T> {
        *A + event<Message<T>> = B,
        *C + event<Message<T>> = D,
    }
}

fn main() {}
