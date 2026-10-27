//! A transition runs its source's `on_exit` before its action and replaces the
//! state only after the action succeeds. When that action fails and an
//! exception row's recovery leaves the source state, the source must not be
//! exited a second time: each entry gets exactly one exit.

use sml::sml;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault;

pub struct Poke {
    fail: bool,
}

#[derive(Default)]
struct ExitContext {
    log: Vec<&'static str>,
    fail_work: bool,
}

impl ExitContext {
    fn push(&mut self, entry: &'static str) {
        self.log.push(entry);
    }

    fn act(&mut self, fail: bool) -> Result<(), Fault> {
        self.log.push("action");
        if fail {
            Err(Fault)
        } else {
            Ok(())
        }
    }
}

pub struct Go {
    fail: bool,
}

sml! {
    CompletionExitKid {
        *"working"_s + completion<Go> / go = X,
         "working"_s + on_entry<_> / entered,
         "working"_s + on_exit<_> / exited,
    }

    CompletionExit {
        *"idle"_s + event<Go> = state<CompletionExitKid>,
         state<CompletionExitKid> + completion<_> = "idle"_s,
         state<CompletionExitKid> + exception<Fault> / recover = "idle"_s,
    }
}

impl CompletionExitStateMachineContext for ExitContext {
    fn entered(&mut self) -> Result<(), Fault> {
        self.push("entry");
        Ok(())
    }
    fn exited(&mut self) -> Result<(), Fault> {
        self.push("exit");
        Ok(())
    }
    fn go(&mut self, event: &Go) -> Result<(), Fault> {
        self.act(event.fail)
    }
    fn recover(&mut self, _: &Fault) -> Result<(), Fault> {
        self.push("recover");
        Ok(())
    }
}

#[test]
fn composite_completion_fault_recovered_by_the_parent_exits_the_child_once() {
    let mut machine = CompletionExitStateMachine::new(ExitContext::default());
    machine.process_event(Go { fail: true }).unwrap();
    assert!(machine.is(&CompletionExitStates::Idle));
    assert_eq!(
        machine.context().log,
        ["entry", "exit", "action", "recover"]
    );
    // The next cycle pairs one entry with one exit.
    machine.context_mut().log.clear();
    machine.process_event(Go { fail: false }).unwrap();
    assert!(machine.is(&CompletionExitStates::Idle));
    assert_eq!(machine.context().log, ["entry", "exit", "action"]);
}

sml! {
    EventExitKid {
        *"working"_s + event<Poke> / poke = "done"_s,
         "working"_s + on_entry<_> / entered,
         "working"_s + on_exit<_> / exited,
         "done"_s + event<Poke> = X,
    }

    EventExit {
        *"idle"_s + event<Go> = state<EventExitKid>,
         state<EventExitKid> + completion<_> = "idle"_s,
         state<EventExitKid> + exception<Fault> / recover = "idle"_s,
    }
}

impl EventExitStateMachineContext for ExitContext {
    fn entered(&mut self) -> Result<(), Fault> {
        self.push("entry");
        Ok(())
    }
    fn exited(&mut self) -> Result<(), Fault> {
        self.push("exit");
        Ok(())
    }
    fn poke(&mut self, event: &Poke) -> Result<(), Fault> {
        self.act(event.fail)
    }
    fn recover(&mut self, _: &Fault) -> Result<(), Fault> {
        self.push("recover");
        Ok(())
    }
}

#[test]
fn composite_event_fault_recovered_by_the_parent_exits_the_child_once() {
    let mut machine = EventExitStateMachine::new(ExitContext::default());
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Poke { fail: true }).unwrap();
    assert!(machine.is(&EventExitStates::Idle));
    assert_eq!(
        machine.context().log,
        ["entry", "exit", "action", "recover"]
    );
    machine.context_mut().log.clear();
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Poke { fail: false }).unwrap();
    assert!(machine.is_child(&EventExitEventExitKidStates::Done));
    assert_eq!(machine.context().log, ["entry", "exit", "action"]);
}

sml! {
    SameLevelExitKid {
        *"working"_s + event<Poke> / poke = "done"_s,
         "working"_s + on_entry<_> / entered,
         "working"_s + on_exit<_> / exited,
         "working"_s + exception<Fault> / recover = "recovered"_s,
         "done"_s + event<Poke> = X,
         "recovered"_s + event<Poke> = X,
    }

    SameLevelExit {
        *"idle"_s + event<Go> = state<SameLevelExitKid>,
         state<SameLevelExitKid> + completion<_> = "idle"_s,
    }
}

impl SameLevelExitStateMachineContext for ExitContext {
    fn entered(&mut self) -> Result<(), Fault> {
        self.push("entry");
        Ok(())
    }
    fn exited(&mut self) -> Result<(), Fault> {
        self.push("exit");
        Ok(())
    }
    fn poke(&mut self, event: &Poke) -> Result<(), Fault> {
        self.act(event.fail)
    }
    fn recover(&mut self, _: &Fault) -> Result<(), Fault> {
        self.push("recover");
        Ok(())
    }
}

#[test]
fn composite_event_fault_recovered_by_a_same_level_row_exits_the_source_once() {
    let mut machine = SameLevelExitStateMachine::new(ExitContext::default());
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Poke { fail: true }).unwrap();
    assert!(machine.is_child(&SameLevelExitSameLevelExitKidStates::Recovered));
    assert_eq!(
        machine.context().log,
        ["entry", "exit", "action", "recover"]
    );
}

sml! {
    OrthogonalExit {
        *"left working"_s + event<Poke> / poke = "left done"_s,
         "left working"_s + on_entry<_> / entered,
         "left working"_s + on_exit<_> / exited,
         "left working"_s + exception<Fault> / recover = "left recovered"_s,

        *"right idle"_s + event<Halt> = X,
    }
}

pub struct Halt;

impl OrthogonalExitStateMachineContext for ExitContext {
    fn entered(&mut self) -> Result<(), Fault> {
        self.push("entry");
        Ok(())
    }
    fn exited(&mut self) -> Result<(), Fault> {
        self.push("exit");
        Ok(())
    }
    fn poke(&mut self, event: &Poke) -> Result<(), Fault> {
        self.act(event.fail)
    }
    fn recover(&mut self, _: &Fault) -> Result<(), Fault> {
        self.push("recover");
        Ok(())
    }
}

#[test]
fn orthogonal_event_fault_recovered_by_a_region_row_exits_the_source_once() {
    let mut machine = OrthogonalExitStateMachine::new(ExitContext::default());
    machine.initialize().unwrap();
    machine.process_event(Poke { fail: true }).unwrap();
    assert!(matches!(
        machine.state(0),
        Some(OrthogonalExitStates::LeftRecovered)
    ));
    assert_eq!(
        machine.context().log,
        ["entry", "exit", "action", "recover"]
    );
}

sml! {
    TreeExitLeaf {
        *"working"_s + completion<Go> / go = X,
         "working"_s + on_entry<_> / entered,
         "working"_s + on_exit<_> / exited,
    }

    TreeExitMiddle {
        *state<TreeExitLeaf> + completion<_> = X,
    }

    TreeExit {
        *"idle"_s + event<Go> = state<TreeExitMiddle>,
         state<TreeExitMiddle> + completion<_> = "idle"_s,
         state<TreeExitMiddle> + exception<Fault> / recover = "idle"_s,
    }
}

impl TreeExitStateMachineContext for ExitContext {
    fn entered(&mut self) -> Result<(), Fault> {
        self.push("entry");
        Ok(())
    }
    fn exited(&mut self) -> Result<(), Fault> {
        self.push("exit");
        Ok(())
    }
    fn go(&mut self, event: &Go) -> Result<(), Fault> {
        self.act(event.fail)
    }
    fn recover(&mut self, _: &Fault) -> Result<(), Fault> {
        self.push("recover");
        Ok(())
    }
}

#[test]
fn tree_completion_fault_recovered_by_the_root_exits_the_leaf_once() {
    let mut machine = TreeExitStateMachine::new(ExitContext::default());
    machine.process_event(Go { fail: true }).unwrap();
    assert!(machine.is(&TreeExitStates::Idle));
    assert_eq!(
        machine.context().log,
        ["entry", "exit", "action", "recover"]
    );
    machine.context_mut().log.clear();
    machine.process_event(Go { fail: false }).unwrap();
    assert!(machine.is(&TreeExitStates::Idle));
    assert_eq!(machine.context().log, ["entry", "exit", "action"]);
}

sml! {
    RegionExitNode {
        *"left working"_s + completion<_> / work = "left done"_s,
         "left working"_s + on_entry<_> / entered,
         "left working"_s + on_exit<_> / exited,
         "left done"_s + event<Halt> = X,

        *"right idle"_s + event<Halt> = X,
    }

    RegionExit {
        *"idle"_s + event<Go> = state<RegionExitNode>,
         state<RegionExitNode> + exception<Fault> / recover = "idle"_s,
         state<RegionExitNode> + event<Poke> = "idle"_s,
    }
}

impl RegionExitStateMachineContext for ExitContext {
    fn entered(&mut self) -> Result<(), Fault> {
        self.push("entry");
        Ok(())
    }
    fn exited(&mut self) -> Result<(), Fault> {
        self.push("exit");
        Ok(())
    }
    fn work(&mut self) -> Result<(), Fault> {
        self.act(self.fail_work)
    }
    fn recover(&mut self, _: &Fault) -> Result<(), Fault> {
        self.push("recover");
        Ok(())
    }
}

#[test]
fn orthogonal_node_completion_fault_recovered_by_the_parent_exits_the_region_once() {
    let mut machine = RegionExitStateMachine::new(ExitContext {
        fail_work: true,
        ..ExitContext::default()
    });
    machine.process_event(Go { fail: true }).unwrap();
    assert!(machine.is(&RegionExitStates::Idle));
    assert_eq!(
        machine.context().log,
        ["entry", "exit", "action", "recover"]
    );
    machine.context_mut().log.clear();
    machine.context_mut().fail_work = false;
    // The next cycle pairs one entry with one exit.
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Poke { fail: false }).unwrap();
    assert!(machine.is(&RegionExitStates::Idle));
    assert_eq!(machine.context().log, ["entry", "exit", "action"]);
}

sml! {
    FlatExit {
        *"idle"_s + event<Poke> = "working"_s,
         "working"_s + on_entry<_> / entered,
         "working"_s + on_exit<_> / exited,
         "working"_s + event<Poke> / poke = "idle"_s,
         "working"_s + exception<Fault> / recover = "idle"_s,
    }
}

impl FlatExitStateMachineContext for ExitContext {
    fn entered(&mut self) -> Result<(), Fault> {
        self.push("entry");
        Ok(())
    }
    fn exited(&mut self) -> Result<(), Fault> {
        self.push("exit");
        Ok(())
    }
    fn poke(&mut self, event: &Poke) -> Result<(), Fault> {
        self.act(event.fail)
    }
    fn recover(&mut self, _: &Fault) -> Result<(), Fault> {
        self.push("recover");
        Ok(())
    }
}

#[test]
fn flat_event_fault_recovery_exits_the_source_once() {
    let mut machine = FlatExitStateMachine::new(ExitContext::default());
    machine.process_event(Poke { fail: false }).unwrap();
    machine.process_event(Poke { fail: true }).unwrap();
    assert!(machine.is(&FlatExitStates::Idle));
    assert_eq!(
        machine.context().log,
        ["entry", "exit", "action", "recover"]
    );
    machine.context_mut().log.clear();
    machine.process_event(Poke { fail: false }).unwrap();
    machine.process_event(Poke { fail: false }).unwrap();
    assert!(machine.is(&FlatExitStates::Idle));
    assert_eq!(machine.context().log, ["entry", "exit", "action"]);
}
