//! Generic typed events with owned and synchronously borrowed payloads.
//!
//! The owned-event action reports each payload it observed as an output; a
//! step hook collects the outputs after each event. The borrowed operation
//! carries its result back to the caller directly.

use std::cell::RefCell;
use std::fmt::Debug;
use std::rc::Rc;

use sml::{sml, sml_policies};

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

/// What the actions emit.
#[derive(Debug, PartialEq, Eq)]
enum Output {
    /// `observe` saw a message with this payload (`Debug`-formatted).
    Observed(String),
}

#[derive(Default)]
struct Context {
    outputs: Vec<Output>,
}

impl GenericEventsStateMachineContext for Context {
    fn observe<T>(&mut self, event: &Message<T>) -> Result<(), ()>
    where
        T: Clone + Debug,
    {
        self.outputs
            .push(Output::Observed(format!("{:?}", event.0)));
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

impl sml::Emit<Output> for Context {
    fn drain_outputs(&mut self, sink: impl FnMut(Output)) {
        self.outputs.drain(..).for_each(sink);
    }
}

/// Collects every output the machine emits, whatever the event type.
#[derive(Default)]
struct Collect(Rc<RefCell<Vec<Output>>>);

impl<L, E> sml::StepHook<L, E> for Collect {
    type Output = Output;

    fn output(&mut self, _logger: &mut L, output: Output) {
        self.0.borrow_mut().push(output);
    }
}

sml_policies!(Observed { step_hook: Collect });

fn main() {
    let collect = Collect::default();
    let seen = Rc::clone(&collect.0);
    let mut machine = GenericEventsStateMachine::new_with_policy(
        Context::default(),
        Observed::default().with_step_hook(collect),
    );

    assert!(machine.process_event(Message(7_u32)).unwrap());
    assert!(machine
        .process_event(Message(String::from("typed")))
        .unwrap());
    assert_eq!(
        seen.take(),
        [
            Output::Observed(String::from("7")),
            Output::Observed(String::from("\"typed\"")),
        ]
    );

    let mut operation = Operation {
        input: String::from("result"),
        result: None,
    };
    assert!(machine.process_event(&mut operation).unwrap());

    assert_eq!(operation.result.as_deref(), Some("result"));
    assert!(seen.take().is_empty());
}
