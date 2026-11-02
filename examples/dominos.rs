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
    _[
        states_attr: #[derive(Debug)],
        events_attr: #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    ] {
        *D0 +  ToD1 / to_d2  = D1,
        D1(Option<Events>) +  ToD2 / to_d3  = D2,
        D2(Option<Events>) +  ToD3 / to_d4  = D3,
        D3(Option<Events>) +  ToD4 / to_d5  = D4,
        D4(Option<Events>) +  ToD5  = D5,
    }
}

/// Context
#[derive(Default)]
pub struct Context {
    outbox: Vec<Events>,
}

impl Context {
    /// Emits `next`; the caller also stores it as the entered state's data.
    fn fire(&mut self, next: Events) -> Events {
        self.outbox.push(next);
        next
    }
}

impl StateMachineContext for Context {
    fn to_d2(&mut self) -> Result<Option<Events>, ()> {
        Ok(Some(self.fire(Events::ToD2)))
    }

    fn to_d3(&mut self, _state_data: &Option<Events>) -> Result<Option<Events>, ()> {
        Ok(Some(self.fire(Events::ToD3)))
    }

    fn to_d4(&mut self, _state_data: &Option<Events>) -> Result<Option<Events>, ()> {
        Ok(Some(self.fire(Events::ToD4)))
    }

    fn to_d5(&mut self, _state_data: &Option<Events>) -> Result<Option<Events>, ()> {
        Ok(Some(self.fire(Events::ToD5)))
    }

    fn transition_callback(&self, exit: &States, entry: &States) {
        println!("Domino {exit:?} fell. Next up: {entry:?}");
    }
}

impl sml::Emit<Events> for Context {
    fn drain_outputs(&mut self, sink: impl FnMut(Events)) {
        self.outbox.drain(..).for_each(sink);
    }
}

/// Queues every event the machine emits for the caller to process next.
#[derive(Default)]
pub struct Outbox(Rc<RefCell<VecDeque<Events>>>);

impl<L> sml::StepHook<L, Events> for Outbox {
    type Output = Events;

    fn output(&mut self, _logger: &mut L, output: Events) {
        self.0.borrow_mut().push_back(output);
    }
}

sml_policies!(Observed { step_hook: Outbox });

fn main() {
    let outbox = Outbox::default();
    let pending = Rc::clone(&outbox.0);
    let mut sm = StateMachine::new_with_policy(
        Context::default(),
        Observed::default().with_step_hook(outbox),
    );

    // first event starts the dominos
    let mut event = Some(Events::ToD1);
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
            Events::ToD1,
            Events::ToD2,
            Events::ToD3,
            Events::ToD4,
            Events::ToD5,
        ]
    );
}
