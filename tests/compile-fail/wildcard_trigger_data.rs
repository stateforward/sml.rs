use sml::sml;

sml! {
        _ {
        *Idle + Start = Ready,
        Ready + completion<_>(u32) = Done,
    }
}

fn main() {}
