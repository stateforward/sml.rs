//! A transition runs its source's `on_exit` before its action and replaces the
//! state only after the action succeeds. When that action fails and an
//! exception row's recovery leaves the source state, the source must not be
//! exited a second time: each entry gets exactly one exit.

use sml::sml;

/// Returns whether root region `$region` is in a state matching `$pattern`.
macro_rules! region_is {
    ($machine:expr, $region:expr, $pattern:pat) => {{
        let mut index = 0;
        let mut matched = false;
        $machine.visit_current_states(|state| {
            if index == $region {
                matched = matches!(state, $pattern);
            }
            index += 1;
        });
        matched
    }};
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault;

pub struct Poke {
    fail: bool,
}

#[derive(Default)]
struct ExitContext {
    log: core::cell::RefCell<Vec<&'static str>>,
    fail_work: core::cell::Cell<bool>,
}

impl ExitContext {
    fn push(&self, entry: &'static str) {
        self.log.borrow_mut().push(entry);
    }

    fn act(&self, fail: bool) -> Result<(), Fault> {
        self.log.borrow_mut().push("action");
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

impl CompletionExitStateMachineContext for &ExitContext {
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
    let context = ExitContext::default();
    let mut machine = CompletionExitStateMachine::new(&context);
    machine.process_event(Go { fail: true }).unwrap();
    assert!(machine.is(&CompletionExitStates::Idle));
    assert_eq!(
        *context.log.borrow(),
        ["entry", "exit", "action", "recover"]
    );
    // The next cycle pairs one entry with one exit.
    context.log.borrow_mut().clear();
    machine.process_event(Go { fail: false }).unwrap();
    assert!(machine.is(&CompletionExitStates::Idle));
    assert_eq!(*context.log.borrow(), ["entry", "exit", "action"]);
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

impl EventExitStateMachineContext for &ExitContext {
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
    let context = ExitContext::default();
    let mut machine = EventExitStateMachine::new(&context);
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Poke { fail: true }).unwrap();
    assert!(machine.is(&EventExitStates::Idle));
    assert_eq!(
        *context.log.borrow(),
        ["entry", "exit", "action", "recover"]
    );
    context.log.borrow_mut().clear();
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Poke { fail: false }).unwrap();
    assert!(machine.is(&EventExitEventExitKidStates::Done));
    assert_eq!(*context.log.borrow(), ["entry", "exit", "action"]);
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

impl SameLevelExitStateMachineContext for &ExitContext {
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
    let context = ExitContext::default();
    let mut machine = SameLevelExitStateMachine::new(&context);
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Poke { fail: true }).unwrap();
    assert!(machine.is(&SameLevelExitSameLevelExitKidStates::Recovered));
    assert_eq!(
        *context.log.borrow(),
        ["entry", "exit", "action", "recover"]
    );
    // Leave and re-enter the child: the next cycle pairs one entry with one
    // exit.
    machine.process_event(Poke { fail: false }).unwrap();
    assert!(machine.is(&SameLevelExitStates::Idle));
    context.log.borrow_mut().clear();
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Poke { fail: false }).unwrap();
    assert!(machine.is(&SameLevelExitSameLevelExitKidStates::Done));
    assert_eq!(*context.log.borrow(), ["entry", "exit", "action"]);
}

sml! {
    OrthogonalExit {
        *"left working"_s + event<Poke> / poke = "left done"_s,
         "left working"_s + on_entry<_> / entered,
         "left working"_s + on_exit<_> / exited,
         "left working"_s + exception<Fault> / recover = "left recovered"_s,
         "left recovered"_s + event<Go> = "left working"_s,

        *"right idle"_s + event<Halt> = X,
    }
}

pub struct Halt;

impl OrthogonalExitStateMachineContext for &ExitContext {
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
    let context = ExitContext::default();
    let mut machine = OrthogonalExitStateMachine::new(&context);
    machine.initialize().unwrap();
    machine.process_event(Poke { fail: true }).unwrap();
    assert!(region_is!(machine, 0, OrthogonalExitStates::LeftRecovered));
    assert_eq!(
        *context.log.borrow(),
        ["entry", "exit", "action", "recover"]
    );
    // Re-enter the region state: the next cycle pairs one entry with one exit.
    context.log.borrow_mut().clear();
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Poke { fail: false }).unwrap();
    assert!(region_is!(machine, 0, OrthogonalExitStates::LeftDone));
    assert_eq!(*context.log.borrow(), ["entry", "exit", "action"]);
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

impl TreeExitStateMachineContext for &ExitContext {
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
    let context = ExitContext::default();
    let mut machine = TreeExitStateMachine::new(&context);
    machine.process_event(Go { fail: true }).unwrap();
    assert!(machine.is(&TreeExitStates::Idle));
    assert_eq!(
        *context.log.borrow(),
        ["entry", "exit", "action", "recover"]
    );
    context.log.borrow_mut().clear();
    machine.process_event(Go { fail: false }).unwrap();
    assert!(machine.is(&TreeExitStates::Idle));
    assert_eq!(*context.log.borrow(), ["entry", "exit", "action"]);
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

impl RegionExitStateMachineContext for &ExitContext {
    fn entered(&mut self) -> Result<(), Fault> {
        self.push("entry");
        Ok(())
    }
    fn exited(&mut self) -> Result<(), Fault> {
        self.push("exit");
        Ok(())
    }
    fn work(&mut self) -> Result<(), Fault> {
        self.act(self.fail_work.get())
    }
    fn recover(&mut self, _: &Fault) -> Result<(), Fault> {
        self.push("recover");
        Ok(())
    }
}

#[test]
fn orthogonal_node_completion_fault_recovered_by_the_parent_exits_the_region_once() {
    let context = ExitContext {
        fail_work: core::cell::Cell::new(true),
        ..ExitContext::default()
    };
    let mut machine = RegionExitStateMachine::new(&context);
    machine.process_event(Go { fail: true }).unwrap();
    assert!(machine.is(&RegionExitStates::Idle));
    assert_eq!(
        *context.log.borrow(),
        ["entry", "exit", "action", "recover"]
    );
    context.log.borrow_mut().clear();
    context.fail_work.set(false);
    // The next cycle pairs one entry with one exit.
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Poke { fail: false }).unwrap();
    assert!(machine.is(&RegionExitStates::Idle));
    assert_eq!(*context.log.borrow(), ["entry", "exit", "action"]);
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

impl FlatExitStateMachineContext for &ExitContext {
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
    let context = ExitContext::default();
    let mut machine = FlatExitStateMachine::new(&context);
    machine.process_event(Poke { fail: false }).unwrap();
    machine.process_event(Poke { fail: true }).unwrap();
    assert!(machine.is(&FlatExitStates::Idle));
    assert_eq!(
        *context.log.borrow(),
        ["entry", "exit", "action", "recover"]
    );
    context.log.borrow_mut().clear();
    machine.process_event(Poke { fail: false }).unwrap();
    machine.process_event(Poke { fail: false }).unwrap();
    assert!(machine.is(&FlatExitStates::Idle));
    assert_eq!(*context.log.borrow(), ["entry", "exit", "action"]);
}

pub struct Leave;

sml! {
    ReNestLeaf {
        *"working"_s + event<Poke> / poke = "done"_s,
         "working"_s + on_entry<_> / entered,
         "working"_s + on_exit<_> / exited,
         "done"_s + event<Halt> = X,
    }

    ReNestNode {
        *"left idle"_s + event<Go> = state<ReNestLeaf>,
         state<ReNestLeaf> + completion<_> = "left idle"_s,

        *"right idle"_s + event<Halt> = X,
    }

    ReNest {
        *"idle"_s + event<Go> = state<ReNestNode>,
         state<ReNestNode> + exception<Fault> / recover = "idle"_s,
         state<ReNestNode> + event<Leave> = "idle"_s,
    }
}

impl ReNestStateMachineContext for &ExitContext {
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

// A stale token must not outlive the occupancy it belongs to. Leaving an
// orthogonal node exits the composite child active in each of its regions,
// and re-entering that child clears its token, so the next exit after a
// failed nested action that an ancestor recovered still runs exactly once.

#[test]
fn reentering_a_nested_child_after_an_ancestor_recovery_pairs_its_hooks() {
    let context = ExitContext::default();
    let mut machine = ReNestStateMachine::new(&context);
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Poke { fail: true }).unwrap();
    assert!(machine.is(&ReNestStates::Idle));
    assert_eq!(
        *context.log.borrow(),
        ["entry", "exit", "action", "recover"]
    );
    for _ in 0..2 {
        context.log.borrow_mut().clear();
        machine.process_event(Go { fail: false }).unwrap();
        machine.process_event(Go { fail: false }).unwrap();
        machine.process_event(Poke { fail: false }).unwrap();
        assert_eq!(*context.log.borrow(), ["entry", "exit", "action"]);
        machine.process_event(Leave).unwrap();
        assert!(machine.is(&ReNestStates::Idle));
    }
}

#[test]
fn leaving_an_orthogonal_node_exits_its_nested_child() {
    let context = ExitContext::default();
    let mut machine = ReNestStateMachine::new(&context);
    for _ in 0..2 {
        context.log.borrow_mut().clear();
        machine.process_event(Go { fail: false }).unwrap();
        machine.process_event(Go { fail: false }).unwrap();
        machine.process_event(Leave).unwrap();
        assert!(machine.is(&ReNestStates::Idle));
        assert_eq!(*context.log.borrow(), ["entry", "exit"]);
    }
}

sml! {
    InitNestLeaf {
        *"working"_s + event<Poke> / poke = "done"_s,
         "working"_s + on_entry<_> / entered,
         "working"_s + on_exit<_> / exited,
         "done"_s + event<Halt> = X,
    }

    InitNestNode {
        *state<InitNestLeaf> + completion<_> = "left idle"_s,

        *"right idle"_s + event<Halt> = X,
    }

    InitNest {
        *"idle"_s + event<Go> = state<InitNestNode>,
         state<InitNestNode> + event<Leave> = "idle"_s,
         state<InitNestNode> + exception<Fault> = "idle"_s,
    }
}

impl InitNestStateMachineContext for &ExitContext {
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
}

#[test]
fn entering_an_orthogonal_node_enters_a_nested_initial_child() {
    let context = ExitContext::default();
    let mut machine = InitNestStateMachine::new(&context);
    for _ in 0..2 {
        context.log.borrow_mut().clear();
        machine.process_event(Go { fail: false }).unwrap();
        assert_eq!(*context.log.borrow(), ["entry"]);
        machine.process_event(Leave).unwrap();
        assert!(machine.is(&InitNestStates::Idle));
        assert_eq!(*context.log.borrow(), ["entry", "exit"]);
    }
}

sml! {
    RootNestLeaf {
        *"working"_s + event<Poke> / poke = "done"_s,
         "working"_s + on_entry<_> / entered,
         "working"_s + on_exit<_> / exited,
         "done"_s + event<Halt> = X,
    }

    RootNest {
        *state<RootNestLeaf> + event<Leave> = "left idle"_s,
         state<RootNestLeaf> + exception<Fault> = "left idle"_s,

        *"right idle"_s + event<Halt> = X,
    }
}

impl RootNestStateMachineContext for &ExitContext {
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
}

#[test]
fn initializing_an_orthogonal_root_enters_a_nested_initial_child_once() {
    let context = ExitContext::default();
    let mut machine = RootNestStateMachine::new(&context);
    machine.initialize().unwrap();
    assert_eq!(*context.log.borrow(), ["entry"]);
    machine.process_event(Leave).unwrap();
    assert!(region_is!(machine, 0, RootNestStates::LeftIdle));
    assert_eq!(*context.log.borrow(), ["entry", "exit"]);
}

// Leaving an orthogonal node exits each region in turn. When a later
// region's exit fails and an ancestor exception row leaves the node, the
// regions that already exited are not exited again.

#[derive(Default)]
struct TeardownContext {
    log: core::cell::RefCell<Vec<&'static str>>,
    exit_failures: core::cell::Cell<u32>,
}

sml! {
    TeardownNode {
        *"left a"_s + on_entry<_> / a_entered,
         "left a"_s + on_exit<_> / a_exited,
         "left a"_s + event<Halt> = X,

        *"right b"_s + on_entry<_> / b_entered,
         "right b"_s + on_exit<_> / b_exited,
         "right b"_s + event<Halt> = X,
    }

    Teardown {
        *"idle"_s + event<Go> = state<TeardownNode>,
         state<TeardownNode> + event<Leave> = "idle"_s,
         state<TeardownNode> + exception<Fault> / recover = "idle"_s,
    }
}

impl TeardownStateMachineContext for &TeardownContext {
    fn a_entered(&mut self) -> Result<(), Fault> {
        self.log.borrow_mut().push("a entry");
        Ok(())
    }
    fn a_exited(&mut self) -> Result<(), Fault> {
        self.log.borrow_mut().push("a exit");
        Ok(())
    }
    fn b_entered(&mut self) -> Result<(), Fault> {
        self.log.borrow_mut().push("b entry");
        Ok(())
    }
    fn b_exited(&mut self) -> Result<(), Fault> {
        if self.exit_failures.get() > 0 {
            self.exit_failures.set(self.exit_failures.get() - 1);
            self.log.borrow_mut().push("b exit failed");
            return Err(Fault);
        }
        self.log.borrow_mut().push("b exit");
        Ok(())
    }
    fn recover(&mut self, _: &Fault) -> Result<(), Fault> {
        self.log.borrow_mut().push("recover");
        Ok(())
    }
}

#[test]
fn a_failed_region_exit_recovered_by_an_ancestor_does_not_exit_its_peers_again() {
    let context = TeardownContext {
        exit_failures: core::cell::Cell::new(1),
        ..TeardownContext::default()
    };
    let mut machine = TeardownStateMachine::new(&context);
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Leave).unwrap();
    assert!(machine.is(&TeardownStates::Idle));
    assert_eq!(
        *context.log.borrow(),
        [
            "a entry",
            "b entry",
            "a exit",
            "b exit failed",
            "b exit",
            "recover"
        ]
    );
    // The next occupancy pairs each entry with one exit.
    context.log.borrow_mut().clear();
    machine.process_event(Go { fail: false }).unwrap();
    machine.process_event(Leave).unwrap();
    assert_eq!(
        *context.log.borrow(),
        ["a entry", "b entry", "a exit", "b exit"]
    );
}
