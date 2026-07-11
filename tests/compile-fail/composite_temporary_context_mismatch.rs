use sml::sml;

struct Enter;

sml! {
    Child[temporary_context: &mut u64] {
        *"idle"_s + event<Enter> = X,
    }

    Parent[temporary_context: &mut u32] {
        *"outside"_s + event<Enter> = state<Child>,
    }
}

fn main() {}
