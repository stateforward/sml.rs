//! Async guards and actions example
//!
//! An example of using async guards and actions mixed with standard ones. The
//! actions record what they did as outputs; a step hook collects them after
//! each event.

#![deny(missing_docs)]

use std::cell::RefCell;
use std::rc::Rc;

use sml::{sml, sml_policies};

sml! {
    _ {
        *State1 + Event1 [guard1] / async action1 = State2,
        State2 + Event2 [async guard2 && guard3] / async action2 = State3,
        State3 + Event3 / action3 = State4(bool),
    }
}

/// What the actions emit.
#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    /// `action1` ran.
    Action1,
    /// `action2` ran.
    Action2,
    /// `action3` entered `State4` with this state data.
    State4(bool),
}

/// Context with member
pub struct Context {
    lock: smol::lock::RwLock<bool>,
    done: bool,
    outputs: Vec<Output>,
}

impl StateMachineContext for Context {
    fn guard3(&self) -> Result<bool, ()> {
        println!("`guard3` called from async context");
        Ok(true)
    }

    fn guard1(&self) -> Result<bool, ()> {
        println!("`guard1` called from sync context");
        Ok(true)
    }

    async fn guard2(&self) -> Result<bool, ()> {
        println!("`guard2` called from async context");
        let mut lock = self.lock.write().await;
        *lock = false;
        drop(lock);
        Ok(true)
    }

    fn action3(&mut self) -> Result<bool, ()> {
        println!("`action3` called from sync context, done = `{}`", self.done);
        self.outputs.push(Output::State4(self.done));
        Ok(self.done)
    }

    async fn action1(&mut self) -> Result<(), ()> {
        println!("`action1` called from async context");
        let mut lock = self.lock.write().await;
        *lock = true;
        drop(lock);
        self.outputs.push(Output::Action1);
        Ok(())
    }

    async fn action2(&mut self) -> Result<(), ()> {
        println!("`action2` called from async context");
        if !*self.lock.read().await {
            self.done = true;
        }
        self.outputs.push(Output::Action2);
        Ok(())
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

impl<L> sml::StepHook<L, Events> for Collect {
    type Output = Output;

    fn output(&mut self, _logger: &mut L, output: Output) {
        self.0.borrow_mut().push(output);
    }
}

sml_policies!(Observed { step_hook: Collect });

fn main() {
    smol::block_on(async {
        let collect = Collect::default();
        let seen = Rc::clone(&collect.0);
        let mut sm = StateMachine::new_with_policy(
            Context {
                lock: smol::lock::RwLock::new(false),
                done: false,
                outputs: Vec::new(),
            },
            Observed::default().with_step_hook(collect),
        );

        let r = sm.process_event(Events::Event1).await;
        assert_eq!(r, Ok(true));
        assert_eq!(seen.take(), [Output::Action1]);

        let r = sm.process_event(Events::Event2).await;
        assert_eq!(r, Ok(true));
        assert_eq!(seen.take(), [Output::Action2]);

        let r = sm.process_event(Events::Event3).await;
        assert_eq!(r, Ok(true));
        assert_eq!(seen.take(), [Output::State4(true)]);

        // Now all events will not give any change of state, and nothing runs
        let r = sm.process_event(Events::Event1).await;
        assert_eq!(r, Ok(false));

        let r = sm.process_event(Events::Event2).await;
        assert_eq!(r, Ok(false));
        assert!(seen.take().is_empty());
    });

    // ...
}
