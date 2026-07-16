use sml::sml;

pub struct BorrowedEvent<'a, T>(&'a T);

sml! {
    SharedStateLifetime<'a, T>
    where
        T: 'a,
    {
        *"ready"_s(&'a str) + event<BorrowedEvent<'a, T>> / retain_state
            = "ready"_s(&'a str),
    }
}

struct Context;

impl SharedStateLifetimeStateMachineContext for Context {
    fn retain_state<'a, T: 'a>(
        &mut self,
        state: &'a str,
        event: &BorrowedEvent<'a, T>,
    ) -> Result<&'a str, ()> {
        let _ = event.0;
        Ok(state)
    }
}

fn main() {
    let value = 7_u32;
    let mut machine = SharedStateLifetimeStateMachine::new(Context, "ready");
    machine.process_event(BorrowedEvent(&value)).unwrap();
}
