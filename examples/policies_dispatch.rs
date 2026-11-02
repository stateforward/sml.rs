//! Use an owner-supplied policy to reject an event and select a transition.
//!
//! Each path's action emits which path ran; a step hook collects it.

use std::cell::RefCell;
use std::rc::Rc;

use sml::{sml, sml_policies, Dispatch};

pub struct Accept;
pub struct Reject;

sml! {
    DispatchMachine {
        *Idle + event<Accept> [first] / took_first = FirstPath,
        Idle + event<Accept> [second] / took_second = SecondPath,
        Idle + event<Reject> = X,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Path {
    First,
    Second,
}

#[derive(Default)]
struct Context {
    taken: Vec<Path>,
}

impl DispatchMachineStateMachineContext for Context {
    fn first(&self, _: &Accept) -> Result<bool, ()> {
        Ok(true)
    }

    fn second(&self, _: &Accept) -> Result<bool, ()> {
        Ok(true)
    }

    fn took_first(&mut self, _: &Accept) -> Result<(), ()> {
        self.taken.push(Path::First);
        Ok(())
    }

    fn took_second(&mut self, _: &Accept) -> Result<(), ()> {
        self.taken.push(Path::Second);
        Ok(())
    }
}

impl sml::Emit<Path> for Context {
    fn drain_outputs(&mut self, sink: impl FnMut(Path)) {
        self.taken.drain(..).for_each(sink);
    }
}

#[derive(Default)]
struct Paths(Rc<RefCell<Vec<Path>>>);

impl<L> sml::StepHook<L, DispatchMachineEvents> for Paths {
    type Output = Path;

    fn output(&mut self, _logger: &mut L, output: Path) {
        self.0.borrow_mut().push(output);
    }
}

#[derive(Default)]
struct OwnerDispatch;

impl Dispatch for OwnerDispatch {
    fn dispatch(&self, event: &'static str) -> bool {
        event != "Reject"
    }

    fn dispatch_candidate(
        &self,
        _state: &'static str,
        event: &'static str,
        candidate: usize,
    ) -> bool {
        event != "Reject" && candidate == 1
    }
}

sml_policies!(DispatchPolicies {
    dispatch: OwnerDispatch,
    step_hook: Paths,
});

fn main() {
    let paths = Paths::default();
    let taken = Rc::clone(&paths.0);
    let policy: DispatchPolicies = sml::PolicyBundle::with_dispatch(
        OwnerDispatch,
        (),
        (),
        (),
        (),
        sml::DefaultQueuePolicy,
        sml::DefaultQueuePolicy,
    )
    .with_step_hook(paths);
    let mut machine = DispatchMachineStateMachine::new_with_policy(Context::default(), policy);
    // The dispatch policy filters `Reject` out, so no transition handles it.
    assert_eq!(machine.process_event(Reject), Ok(false));
    assert!(taken.take().is_empty());
    // The dispatch policy skips the first candidate, so the second path runs.
    assert_eq!(machine.process_event(Accept), Ok(true));
    assert_eq!(taken.take(), [Path::Second]);
}
