use sml::sml;

struct Operation<T>(T);

sml! {
    MutableCompletion<'event, T: Clone + 'event> {
        *Idle + event<&'event mut Operation<T>> = Finishing,
         Finishing + completion<Operation> = X,
    }
}

fn main() {}
