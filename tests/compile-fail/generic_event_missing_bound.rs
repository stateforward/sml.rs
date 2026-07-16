use sml::sml;

pub struct RequiresClone<T: Clone>(T);

sml! {
    MissingBound<T> {
        *Idle + event<RequiresClone<T>> = X,
    }
}

struct Context;
impl MissingBoundStateMachineContext for Context {}

fn main() {}
