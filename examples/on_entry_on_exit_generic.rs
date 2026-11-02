//! An example of entry and exit hooks (See issue-17)
//!
//! `on_exit_d0` and `on_entry_d1` run whenever the machine leaves `D0` or
//! enters `D1`. Each hook reports itself as an output; a step hook collects
//! the outputs after each event.

#![deny(missing_docs)]

use std::cell::RefCell;
use std::rc::Rc;

use sml::{sml, sml_policies};

sml! {
    OnEntryExample {
        *D0 + ToD1 = D1,
        D0 + ToD3 = D3,
        D1  + ToD2 = D2,
        D2 + ToD1 = D1,
        D1 + ToD0 = D0,
    },
}

/// What the entry and exit hooks emit.
#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    /// The machine left `D0`.
    ExitedD0,
    /// The machine entered `D1`.
    EnteredD1,
}

/// Context
#[derive(Default)]
pub struct Context {
    outputs: Vec<Output>,
}

impl OnEntryExampleStateMachineContext for Context {
    fn on_exit_d0(&mut self) {
        self.outputs.push(Output::ExitedD0);
    }
    fn on_entry_d1(&mut self) {
        self.outputs.push(Output::EnteredD1);
    }
}

impl sml::Emit<Output> for Context {
    fn drain_outputs(&mut self, sink: impl FnMut(Output)) {
        self.outputs.drain(..).for_each(sink);
    }
}

/// Collects every output the machine emits.
#[derive(Default)]
pub struct Collect(Rc<RefCell<Vec<Output>>>);

impl<L> sml::StepHook<L, OnEntryExampleEvents> for Collect {
    type Output = Output;

    fn output(&mut self, _logger: &mut L, output: Output) {
        self.0.borrow_mut().push(output);
    }
}

sml_policies!(Observed { step_hook: Collect });

fn main() {
    let collect = Collect::default();
    let seen = Rc::clone(&collect.0);
    let mut sm = OnEntryExampleStateMachine::new_with_policy(
        Context::default(),
        Observed::default().with_step_hook(collect),
    );

    // D0 -> D1: exit D0, then enter D1
    assert!(sm.process_event(OnEntryExampleEvents::ToD1).unwrap());
    assert_eq!(seen.take(), [Output::ExitedD0, Output::EnteredD1]);

    // D1 -> D2: neither hook runs
    assert!(sm.process_event(OnEntryExampleEvents::ToD2).unwrap());
    assert!(seen.take().is_empty());

    // D2 -> D1: enter D1 again
    assert!(sm.process_event(OnEntryExampleEvents::ToD1).unwrap());
    assert_eq!(seen.take(), [Output::EnteredD1]);

    // D1 -> D0: neither hook runs
    assert!(sm.process_event(OnEntryExampleEvents::ToD0).unwrap());
    assert!(seen.take().is_empty());

    // D0 -> D3: exit D0 again
    assert!(sm.process_event(OnEntryExampleEvents::ToD3).unwrap());
    assert_eq!(seen.take(), [Output::ExitedD0]);
}
