//! Generic typed events with owned and synchronously borrowed payloads.

use core::fmt::Debug;
use sml::sml;

/// An owned event.
pub struct Message<T>(pub T);

/// A call-scoped operation completed during dispatch.
pub struct Operation<T> {
    /// Operation input.
    pub input: T,
    /// Result populated by the state-machine callback.
    pub result: Option<T>,
}

sml! {
    GenericEvents<'operation, T>
    where
        T: Clone + Debug + 'operation,
    {
        *"idle"_s + event<Message<T>> / observe,
         "idle"_s + event<&'operation mut Operation<T>> / complete,
    }
}

#[derive(Default)]
struct Context {
    observed: usize,
}

impl GenericEventsStateMachineContext for Context {
    fn observe<T>(&mut self, event: &Message<T>) -> Result<(), ()>
    where
        T: Clone + Debug,
    {
        let _ = format_args!("{:?}", event.0);
        self.observed += 1;
        Ok(())
    }

    fn complete<'operation, T>(&mut self, operation: &'operation mut Operation<T>) -> Result<(), ()>
    where
        T: Clone + Debug + 'operation,
    {
        operation.result = Some(operation.input.clone());
        Ok(())
    }
}

fn main() {
    let mut machine = GenericEventsStateMachine::new(Context::default());
    machine.process_event(Message(7_u32)).unwrap();
    machine
        .process_event(Message(String::from("typed")))
        .unwrap();

    let mut operation = Operation {
        input: String::from("result"),
        result: None,
    };
    machine.process_event(&mut operation).unwrap();

    assert_eq!(operation.result.as_deref(), Some("result"));
    assert_eq!(machine.context().observed, 2);
}
