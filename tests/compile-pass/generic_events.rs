use core::fmt::Debug;
use sml::sml;

pub struct Event<'a, T, const N: usize> {
    value: &'a T,
    bytes: [u8; N],
}

sml! {
    Generic<'event, T, const N: usize>
    where
        T: Debug + 'event,
    {
        *Idle + event<Event<'event, T, N>> / inspect,
    }
}

struct Context;

impl GenericStateMachineContext for Context {
    fn inspect<'event, T, const N: usize>(
        &mut self,
        event: &Event<'event, T, N>,
    ) -> Result<(), ()>
    where
        T: Debug + 'event,
    {
        let _ = (event.value, event.bytes.len());
        Ok(())
    }
}

fn main() {
    let value = 7_u32;
    GenericStateMachine::new(Context)
        .process_event(Event {
            value: &value,
            bytes: [0; 4],
        })
        .unwrap();
}
