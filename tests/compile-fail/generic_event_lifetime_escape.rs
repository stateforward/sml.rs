use sml::sml;

pub struct Borrowed<'a>(&'a str);

sml! {
    Escape<'event> {
        *Idle + event<Borrowed<'event>> / retain,
    }
}

struct Context {
    retained: Option<&'static str>,
}

impl EscapeStateMachineContext for Context {
    fn retain<'event>(&mut self, event: &Borrowed<'event>) -> Result<(), ()> {
        self.retained = Some(event.0);
        Ok(())
    }
}

fn main() {}
