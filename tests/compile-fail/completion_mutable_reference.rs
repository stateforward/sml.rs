use sml::sml;

struct Payload;

sml! {
        _ {
        *Idle + Go(&'a mut Payload) = Step,
        Step + completion<Go>(&'a mut Payload) = Done,
    }
}

fn main() {}
