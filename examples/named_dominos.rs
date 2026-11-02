//! An example of using state data to propagate events (See issue-17)
//!
//! Each action stores the next event as the new state's data and emits it. A
//! step hook collects the emitted events into an outbox the caller drains.

#![deny(missing_docs)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use sml::{sml, sml_policies};

sml! {
    Dominos[events_attr: #[derive(Debug, Clone, Copy, PartialEq, Eq)]] {
        *D0 +  ToD1 / to_d2  = D1,
        D1(Option<DominosEvents>) +  ToD2 / to_d3  = D2,
        D2(Option<DominosEvents>) +  ToD3 / to_d4  = D3,
        D3(Option<DominosEvents>) +  ToD4 / to_d5  = D4,
        D4(Option<DominosEvents>) +  ToD5  = D5,
    }
}

/// Context
#[derive(Default)]
pub struct Context {
    outbox: Vec<DominosEvents>,
}

impl Context {
    /// Stores `next` as the entered state's data and emits it.
    fn fire(&mut self, next: DominosEvents) -> DominosEvents {
        self.outbox.push(next);
        next
    }
}

impl DominosStateMachineContext for Context {
    fn to_d2(&mut self) -> Result<Option<DominosEvents>, ()> {
        Ok(Some(self.fire(DominosEvents::ToD2)))
    }

    fn to_d3(&mut self, _state_data: &Option<DominosEvents>) -> Result<Option<DominosEvents>, ()> {
        Ok(Some(self.fire(DominosEvents::ToD3)))
    }

    fn to_d4(&mut self, _state_data: &Option<DominosEvents>) -> Result<Option<DominosEvents>, ()> {
        Ok(Some(self.fire(DominosEvents::ToD4)))
    }

    fn to_d5(&mut self, _state_data: &Option<DominosEvents>) -> Result<Option<DominosEvents>, ()> {
        Ok(Some(self.fire(DominosEvents::ToD5)))
    }
}

impl sml::Emit<DominosEvents> for Context {
    fn drain_outputs(&mut self, sink: impl FnMut(DominosEvents)) {
        self.outbox.drain(..).for_each(sink);
    }
}

/// Queues every event the machine emits for the caller to process next.
#[derive(Default)]
pub struct Outbox(Rc<RefCell<VecDeque<DominosEvents>>>);

impl<L> sml::StepHook<L, DominosEvents> for Outbox {
    type Output = DominosEvents;

    fn output(&mut self, _logger: &mut L, output: DominosEvents) {
        self.0.borrow_mut().push_back(output);
    }
}

sml_policies!(Observed { step_hook: Outbox });

fn main() {
    let outbox = Outbox::default();
    let pending = Rc::clone(&outbox.0);
    let mut sm = DominosStateMachine::new_with_policy(
        Context::default(),
        Observed::default().with_step_hook(outbox),
    );

    // first event starts the dominos
    let mut event = Some(DominosEvents::ToD1);
    let mut fallen = Vec::new();

    // use a while let loop to let the events propagate and the dominos fall
    while let Some(e) = event {
        assert!(sm.process_event(e).unwrap());
        fallen.push(e);

        // the action that ran emitted the next event, if any
        event = pending.borrow_mut().pop_front();
    }

    // All the dominos fell!
    assert_eq!(
        fallen,
        [
            DominosEvents::ToD1,
            DominosEvents::ToD2,
            DominosEvents::ToD3,
            DominosEvents::ToD4,
            DominosEvents::ToD5,
        ]
    );
}
