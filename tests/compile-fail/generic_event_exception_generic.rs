use sml::sml;

struct Work<T>(T);
struct Recover<T>(T);

sml! {
    GenericException<T> {
        *Idle + event<Work<T>> / fail,
         Idle + exception<Recover>(Recover<T>) / recover = X,
    }
}

fn main() {}
