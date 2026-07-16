use sml::sml;

struct Message<T>(T);
struct Stored<T>(T);

sml! {
    GenericStateData<T> {
        *"idle"_s + event<Message<T>> / store = "ready"_s(Stored<T>),
         "ready"_s(Stored<T>) + event<Message<T>> = X,
    }
}

fn main() {}
