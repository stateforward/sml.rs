//! State data example
//!
//! An example of using state data together with an action.
//! The action emits the state data it produced; a step hook collects it.

#![deny(missing_docs)]

use std::cell::RefCell;
use std::rc::Rc;

use sml::{sml, sml_policies};

/// State data
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MyStateData(pub u32);

sml! {
    StatesWithData {
        *State1 + Event1 / action = State2,
        State2(MyStateData) + Event2 = State1,
        // ...
    }
}

/// Context
#[derive(Default)]
pub struct Context {
    entered: Vec<MyStateData>,
}

impl StatesWithDataStateMachineContext for Context {
    fn action(&mut self) -> Result<MyStateData, ()> {
        let data = MyStateData(42);
        self.entered.push(data);
        Ok(data)
    }
}

impl sml::Emit<MyStateData> for Context {
    fn drain_outputs(&mut self, sink: impl FnMut(MyStateData)) {
        self.entered.drain(..).for_each(sink);
    }
}

/// Collects the state data the machine emits.
#[derive(Default)]
pub struct Collect(Rc<RefCell<Vec<MyStateData>>>);

impl<L> sml::StepHook<L, StatesWithDataEvents> for Collect {
    type Output = MyStateData;

    fn output(&mut self, _logger: &mut L, output: MyStateData) {
        self.0.borrow_mut().push(output);
    }
}

sml_policies!(Observed { step_hook: Collect });

fn main() {
    let collect = Collect::default();
    let seen = Rc::clone(&collect.0);
    let mut sm = StatesWithDataStateMachine::new_with_policy(
        Context::default(),
        Observed::default().with_step_hook(collect),
    );
    let result = sm.process_event(StatesWithDataEvents::Event1);

    assert_eq!(result, Ok(true));
    assert_eq!(seen.take(), [MyStateData(42)]);
}
