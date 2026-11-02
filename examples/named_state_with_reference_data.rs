//! State data example
//!
//! An example of using referenced state data with lifetimes together with an action.
//! The action emits the state data it produced; a step hook collects it.

#![deny(missing_docs)]

use std::cell::RefCell;
use std::rc::Rc;

use sml::{sml, sml_policies};

/// State data
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MyStateData<'a>(&'a u32);

sml! {
    StatesWithRefData {
        *State1 + Event1 / action = State2,
        State2(MyStateData<'a>) + Event2 = State1,
        // ...
    }
}

/// Context
#[derive(Default)]
pub struct Context {
    entered: Vec<MyStateData<'static>>,
}

impl StatesWithRefDataStateMachineContext for Context {
    fn action<'a>(&mut self) -> Result<MyStateData<'a>, ()> {
        let data = MyStateData(&42);
        self.entered.push(data);
        Ok(data)
    }
}

impl sml::Emit<MyStateData<'static>> for Context {
    fn drain_outputs(&mut self, sink: impl FnMut(MyStateData<'static>)) {
        self.entered.drain(..).for_each(sink);
    }
}

/// Collects the state data the machine emits.
#[derive(Default)]
pub struct Collect(Rc<RefCell<Vec<MyStateData<'static>>>>);

impl<L> sml::StepHook<L, StatesWithRefDataEvents> for Collect {
    type Output = MyStateData<'static>;

    fn output(&mut self, _logger: &mut L, output: MyStateData<'static>) {
        self.0.borrow_mut().push(output);
    }
}

sml_policies!(Observed { step_hook: Collect });

fn main() {
    let collect = Collect::default();
    let seen = Rc::clone(&collect.0);
    let mut sm = StatesWithRefDataStateMachine::new_with_policy(
        Context::default(),
        Observed::default().with_step_hook(collect),
    );
    let result = sm.process_event(StatesWithRefDataEvents::Event1);

    assert_eq!(result, Ok(true));
    assert_eq!(seen.take(), [MyStateData(&42)]);
}
