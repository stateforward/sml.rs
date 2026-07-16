use core::marker::PhantomData;
use sml::sml;

struct Message<F, T>(F, PhantomData<T>);

sml! {
    HigherRankedShadow<'event, T> {
        *Idle + event<Message<for<'event> fn(&'event T), T>> = X,
    }
}

fn main() {}
