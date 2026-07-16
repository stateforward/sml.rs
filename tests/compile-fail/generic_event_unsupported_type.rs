use sml::sml;

sml! {
    Unsupported<T> {
        *Idle + event<(T, T)> = X,
    }
}

fn main() {}
