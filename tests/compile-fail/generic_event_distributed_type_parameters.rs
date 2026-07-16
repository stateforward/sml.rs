use sml::sml;

pub struct First<T>(T);
pub struct Second<U>(U);

sml! {
    Distributed<T, U> {
        *Idle + event<First<T>> = Ready,
         Ready + event<Second<U>> = X,
    }
}

fn main() {}
