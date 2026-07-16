use sml::sml;

sml! {
    WhereWithoutParameters
    where
        T: Clone,
    {
        *Idle + Event = Idle,
    }
}

fn main() {}
