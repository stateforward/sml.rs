use sml::sml;

struct Pair<T, U>(T, U);

sml! {
    TemporaryContextSubset<T, U>[temporary_context: &mut Vec<T>] {
        *Idle + event<Pair<T, U>> = X,
    }
}

fn main() {}
